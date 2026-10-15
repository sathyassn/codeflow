//! Parsed action-family checks shared by the execution guard.
use super::actions::{table, PatternToken};
use super::headless::{
    after_runner_options, package_bin, shell_command_string, skip_assignments, skip_env,
    skip_options,
};
use crate::hooks::git_guard::{
    command_argv, expand_commands, read_alias, split_alias, strip_launchers, strip_reserved_words,
    AliasAnswer, AliasQuery, Retarget,
};
use crate::hooks::policy::SecuritySection;
use crate::hooks::Violation;
use std::path::{Path, PathBuf};

#[cfg(test)]
pub(crate) fn evaluate(command: &str, levels: &SecuritySection) -> Vec<Violation> {
    evaluate_at(command, levels, None)
}

pub(crate) fn evaluate_at(
    command: &str,
    levels: &SecuritySection,
    cwd: Option<&Path>,
) -> Vec<Violation> {
    let mut parsed = Parsed::default();
    visit(command, cwd, 0, &mut parsed);
    let mut embedded = Parsed::default();
    for code in &parsed.code {
        let words = literal_words(code);
        for at in 0..words.len() {
            argv(&words[at..], cwd, 0, &mut embedded);
        }
    }
    let mut families = parsed.families;
    families.extend(embedded.families);
    families.sort_unstable();
    families.dedup();
    let release_roots: Vec<_> = families
        .iter()
        .filter(|(id, _)| *id == "release")
        .map(|(_, root)| root.clone())
        .collect();
    families.retain(|(id, root)| *id != "configured_tag_push" || !release_roots.contains(root));
    let mut violations: Vec<_> = parsed
        .unreadable
        .into_iter()
        .chain(embedded.unreadable)
        .map(|reason| {
            Violation::always_blocking(
                "git.policy_authority",
                reason,
                "restore the command repository and its readable policy authority",
            )
        })
        .collect();
    violations.extend(families
        .into_iter()
        .filter_map(|(id, root)| {
            let authority = root.as_deref().map(crate::hooks::landed_policy::load).transpose();
            let authority = match authority {
                Ok(value) => value,
                Err(reason) => return Some(Violation::always_blocking("git.policy_authority", reason, "restore the named remote-tracking policy authority")),
            };
            let levels = authority.as_ref().map_or(levels, |a| &a.policy.security);
            let source = authority.as_ref().map_or(String::new(), |a| format!("; policy source: {}", a.source));
            let level = match id {
                "privilege" => levels.privilege_escalation,
                "keychain" => levels.secret_reads,
                _ => levels.outward_actions,
            };
            level.is_active().then(|| {
                if id == "configured_tag_push" {
                    return Violation::new(
                        "security.outward_actions",
                        level,
                        format!("release action: effective push.followTags config enables tag publication{source}"),
                        crate::remedy::PUSH_WITHOUT_FOLLOW_TAGS.remedy(),
                    );
                }
                Violation::new(
                    match id {
                        "privilege" => "security.privilege_escalation",
                        "keychain" => "security.secret_reads",
                        _ => "security.outward_actions",
                    },
                    level,
                    format!(
                        "{id} action: {}{source}",
                        table()
                            .families
                            .iter()
                            .find(|f| f.id == id)
                            .map_or("operator-owned action", |f| f.why.as_str())
                    ),
                    crate::remedy::OUTWARD_ACTION.remedy(),
                )
            })
        })
        .collect::<Vec<_>>());
    violations
}

#[derive(Default)]
struct Parsed {
    unreadable: Vec<String>,
    families: Vec<(&'static str, Option<PathBuf>)>,
    code: Vec<String>,
    stdin_interpreter: usize,
    commands: Vec<Vec<String>>,
    created_tags: Vec<(PathBuf, String)>,
    calls: Vec<InterpreterCall>,
    /// What gives the command being read more arguments, while the
    /// command an `xargs` or `find -exec` runs is read.
    supplier: Option<Supplier>,
    /// A shell or interpreter on the line takes its code from somewhere the
    /// guard cannot read in full (TSK-302 review round 7): stdin from a
    /// pipe, a here-string, a process substitution, or a `-c`/`-e` value
    /// holding a substitution.
    opaque: bool,
    /// A shell (`source` and `.` included) or an interpreter family runs
    /// somewhere on the line (TSK-302 review round 8).
    runs: bool,
}

/// One interpreter call (TSK-302, issue 173): the words it receives other
/// than its inline code, the directory it runs in, and what else gives it
/// arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InterpreterCall {
    /// The words the interpreter hands its program, read by the family's
    /// own option grammar: the values of its options, a script, and every
    /// word after the code or script boundary. Inline code is left out.
    /// Any of them can name a file the program writes.
    pub(crate) args: Vec<String>,
    /// The directory the call runs in; `None` after a `cd` the guard
    /// cannot follow.
    pub(crate) cwd: Option<PathBuf>,
    /// The directories the interpreter changes to, in order, before it
    /// runs the program (`ruby -C a -C b`), each relative to the last.
    pub(crate) chdir: Vec<String>,
    /// The inline code the call carries, as read for the code checks.
    pub(crate) code: Vec<String>,
    /// The `xargs` or `find -exec` that runs the interpreter, if one does.
    pub(crate) supplier: Option<Supplier>,
}

/// A command that runs an interpreter with arguments the line does not
/// spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Supplier {
    /// `xargs` adds words read from its input or an argument file.
    Xargs,
    /// `find -exec` puts each path it visits in place of `{}`.
    Find {
        /// The starting points, as written.
        starts: Vec<String>,
        /// The expression before the `-exec`, whose `-name` tests decide
        /// which paths reach it.
        before: Vec<String>,
        /// The directory `find` runs in; `None` when the guard cannot
        /// follow it.
        cwd: Option<PathBuf>,
        /// `-execdir` or `-okdir`: the command runs in the directory of
        /// each path `find` visits.
        in_dir: bool,
    },
}

pub(crate) fn unwrapped_commands(command: &str) -> Vec<Vec<String>> {
    let mut parsed = Parsed::default();
    visit(command, None, 0, &mut parsed);
    parsed.commands
}

/// What the line's shells and interpreters read (TSK-302 review rounds 7
/// and 8). The first value: whether one runs code the guard cannot read in
/// full, its program coming from stdin through a pipe or a here-string,
/// from a descriptor (`/dev/stdin`, `/dev/fd/N`), from a process
/// substitution, or from a `-c`/`-e` value holding a substitution; every
/// word on such a line is judged by its trailing components. The second:
/// whether a shell (`source` and `.` included) or an interpreter family
/// runs on the line at all, so every quoted string on it may be code.
pub(crate) fn code_sources(command: &str) -> (bool, bool) {
    let mut parsed = Parsed::default();
    visit(command, None, 0, &mut parsed);
    let runs = parsed.runs || !parsed.calls.is_empty();
    (parsed.opaque, runs)
}

/// Every quoted string on a line, read as the shell quotes it: the text of
/// each single- or double-quoted region, a command substitution or
/// backtick inside a double-quoted one read whole, quotes and all.
pub(crate) fn quoted_texts(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(&ch) = chars.get(at) {
        at += 1;
        match ch {
            '\\' => at += 1,
            '\'' => {
                let mut text = String::new();
                while let Some(&c) = chars.get(at) {
                    at += 1;
                    if c == '\'' {
                        break;
                    }
                    text.push(c);
                }
                found.push(text);
            }
            '"' => {
                let mut text = String::new();
                let mut depth = 0usize;
                let mut inner: Option<char> = None;
                while let Some(&c) = chars.get(at) {
                    at += 1;
                    if c == '\\' {
                        text.push(c);
                        text.extend(chars.get(at));
                        at += 1;
                        continue;
                    }
                    if let Some(q) = inner {
                        if c == q {
                            inner = None;
                        }
                    } else if depth > 0 && matches!(c, '\'' | '"') {
                        inner = Some(c);
                    } else if c == '$' && chars.get(at) == Some(&'(') {
                        depth += 1;
                        text.push(c);
                        text.push('(');
                        at += 1;
                        continue;
                    } else if depth > 0 && c == '(' {
                        depth += 1;
                    } else if depth > 0 && c == ')' {
                        depth -= 1;
                    } else if depth == 0 && c == '"' {
                        break;
                    }
                    text.push(c);
                }
                found.push(text);
            }
            _ => {}
        }
    }
    found
}

/// Whether a program operand is a descriptor the guard does not read:
/// `/dev/stdin`, `/dev/fd/N` or `/proc/self/fd/N`.
fn descriptor_program(operand: &str) -> bool {
    operand == "/dev/stdin"
        || ["/dev/fd/", "/proc/self/fd/"].iter().any(|prefix| {
            operand
                .strip_prefix(prefix)
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
}

pub(crate) fn interpreter_bodies(command: &str) -> Vec<String> {
    let mut parsed = Parsed::default();
    visit(command, None, 0, &mut parsed);
    parsed.code
}

/// Every interpreter call on the line, read through the launchers,
/// wrappers and `cd` changes this parser follows, starting in `cwd`.
pub(crate) fn interpreter_calls(command: &str, cwd: &Path) -> Vec<InterpreterCall> {
    let mut parsed = Parsed::default();
    visit(command, Some(cwd), 0, &mut parsed);
    let mut calls = Vec::new();
    for call in parsed.calls {
        if !calls.contains(&call) {
            calls.push(call);
        }
    }
    calls
}

pub(crate) fn literal_words(code: &str) -> Vec<String> {
    code.replace("${HOME}", "$HOME")
        .split(|c: char| {
            !(c.is_alphanumeric()
                || matches!(c, '_' | '.' | '/' | '~' | '+' | '-' | '@' | '=' | ':' | '$'))
        })
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

fn visit(command: &str, cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    if depth > 16 {
        return;
    }
    shell_inputs(command, out);
    let mut directories = vec![cwd.map(Path::to_path_buf)];
    for segment in expand_commands(command) {
        let mut words = command_argv(&segment);
        strip_reserved_words(&mut words);
        let mut assigned = skip_assignments(&words).to_vec();
        if let Some(setting) = words
            .iter()
            .take_while(|s| s.contains('='))
            .find(|s| s.starts_with("GIT_DIR="))
        {
            assigned.splice(0..0, ["env".to_string(), setting.clone()]);
        }
        let args = assigned.as_slice();
        let before = out.stdin_interpreter;
        for dir in &directories {
            argv(args, dir.as_deref(), depth + 1, out);
        }
        if out.stdin_interpreter > before && command.contains("<<") {
            if let Some((_, body)) = command.split_once('\n') {
                out.code.push(body.to_string());
            }
        }
        if args.first().is_some_and(|s| s == "cd") {
            if let Some(path) = cd_operand(&args[1..]).filter(|p| !p.contains(['$', '`'])) {
                let next: Vec<_> = directories
                    .iter()
                    .filter_map(|dir| dir.as_ref().map(|dir| Some(dir.join(path))))
                    .collect();
                directories.extend(next);
                directories.sort();
                directories.dedup();
            }
        }
    }
}

/// Every directory a line may run an interpreter in (TSK-302 review round
/// 2): the session directory and each one a directory change anywhere on
/// the line may reach, applied in order to every directory listed before
/// it, whether or not it succeeds. `unknown` is set when a change cannot be
/// resolved, so a command may run anywhere.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct DirSet {
    pub(crate) dirs: Vec<PathBuf>,
    pub(crate) unknown: bool,
}

/// The most directories a [`DirSet`] lists before it reads as unknown.
const DIR_SET_LIMIT: usize = 64;

impl DirSet {
    /// Apply one change: a literal directory joins every listed one, a
    /// change the guard cannot resolve makes the set unknown.
    pub(crate) fn change(&mut self, change: &DirChange) {
        match change {
            DirChange::To(target) => {
                let next: Vec<PathBuf> = self
                    .dirs
                    .iter()
                    .map(|dir| lexical_join(dir, target))
                    .collect();
                for dir in next {
                    if !self.dirs.contains(&dir) {
                        self.dirs.push(dir);
                    }
                }
                if self.dirs.len() > DIR_SET_LIMIT {
                    self.unknown = true;
                }
            }
            DirChange::Unknown => self.unknown = true,
        }
    }
}

/// `dir` joined with `target`, `.` and `..` removed by spelling, as the
/// shell's logical `cd` reads them. An absolute target replaces `dir`.
pub(crate) fn lexical_join(dir: &Path, target: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in dir.join(target).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A directory change one simple command makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirChange {
    To(PathBuf),
    Unknown,
}

/// The directory changes the words of one simple command make (TSK-302
/// review round 3). Within the modeled grammar the answer is exact;
/// anything outside it is [`DirChange::Unknown`], never "no change":
///
/// - `cd`, `chdir` and `pushd` with any options; `cd` alone goes to
///   `home`; `cd -`, a stack index, `~user` and a value filled in at run
///   time are unknown. `popd` and `dirs` return to directories already
///   listed.
/// - `source` and `.` can move the shell to anywhere: unknown.
/// - The launchers `command`, `builtin`, `exec`, `nohup`, `nice`,
///   `timeout`, `stdbuf`, `time`, `env` and `xargs` are unwrapped through
///   their option grammar, values included, attached or separate, and
///   the command they run is read in turn. `env -C`, `-CDIR`, `--chdir`
///   and `--chdir=` change directory. An option the grammar does not list,
///   `env -S`, and `sudo` are unknown.
/// - `git -C DIR` counts as a change.
/// - Any other program cannot move the shell, so it makes no change.
pub(crate) fn dir_changes(words: &[String], home: Option<&Path>) -> Vec<DirChange> {
    let mut found = Vec::new();
    launcher_changes(words, home, 0, &mut found);
    found
}

/// A launcher's option grammar: the options that take a value, and the
/// flags. `None` for a word with no modeled options.
fn launcher_options(launcher: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    Some(match launcher {
        "command" | "time" => (&[], &["-p"]),
        "caffeinate" => (&["-t", "-w"], &["-d", "-i", "-m", "-s", "-u"]),
        "xcrun" => (
            &["--sdk", "--toolchain"],
            &[
                "-v",
                "--verbose",
                "-l",
                "--log",
                "-n",
                "--no-cache",
                "-r",
                "--run",
                "-k",
                "--kill-cache",
            ],
        ),
        "ionice" => (
            &[
                "-c",
                "--class",
                "-n",
                "--classdata",
                "-p",
                "--pid",
                "-P",
                "--pgid",
                "-u",
                "--uid",
            ],
            &["-t", "--ignore"],
        ),
        "exec" => (&["-a"], &["-c", "-l", "-cl", "-lc"]),
        "nice" => (&["-n", "--adjustment"], &[]),
        "timeout" => (
            &["-s", "--signal", "-k", "--kill-after"],
            &["--foreground", "--preserve-status", "-v", "--verbose"],
        ),
        "stdbuf" => (&["-i", "-o", "-e", "--input", "--output", "--error"], &[]),
        "xargs" => (
            &[
                "-I",
                "-L",
                "-n",
                "-P",
                "-s",
                "-d",
                "-E",
                "-a",
                "--arg-file",
                "--delimiter",
                "--eof",
                "--max-lines",
                "--max-args",
                "--max-procs",
                "--max-chars",
                "--replace",
                "--process-slot-var",
            ],
            &[
                "-0",
                "-r",
                "-t",
                "-p",
                "-x",
                "--null",
                "--no-run-if-empty",
                "--verbose",
                "--interactive",
                "--exit",
                "--open-tty",
            ],
        ),
        _ => return None,
    })
}

/// How an option of a launcher reads: `Some(true)` takes a value,
/// `Some(false)` is a flag, `None` is outside the modeled grammar.
fn launcher_option(launcher: &str, option: &str) -> Option<bool> {
    let (valued, flags) = launcher_options(launcher)?;
    if valued.contains(&option) {
        Some(true)
    } else {
        flags.contains(&option).then_some(false)
    }
}

/// Read one launcher's options from `rest`, returning where its command
/// starts, or `None` for an option the grammar does not know. A long
/// option's value may follow `=`; a short one's may be attached
/// (`-n5`, `-oL`), or be the next word.
fn skip_launcher_options(launcher: &str, rest: &[String]) -> Option<usize> {
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        if word == "--" {
            return Some(at + 1);
        }
        if !word.starts_with('-') || word == "-" {
            return Some(at);
        }
        // `nice -5`: a legacy numeric adjustment.
        if launcher == "nice" && word[1..].chars().all(|c| c.is_ascii_digit()) {
            at += 1;
            continue;
        }
        if let Some((name, _)) = word.split_once('=').filter(|_| word.starts_with("--")) {
            launcher_option(launcher, name)?;
            at += 1;
            continue;
        }
        if let Some(valued) = launcher_option(launcher, word) {
            at += if valued { 2 } else { 1 };
            continue;
        }
        // A short option with its value attached: `-n5`, `-oL`, `-I{}`.
        let short = word.get(..2).filter(|_| !word.starts_with("--"))?;
        if launcher_option(launcher, short)? {
            at += 1;
            continue;
        }
        return None;
    }
    Some(at)
}

/// Read env's options: flags, `-u NAME`, `-C DIR` in every spelling and
/// clusters such as `-iC.claude`; `NAME=value` settings follow them.
/// Returns where the command starts, or `None` for an option it does not
/// know (and for `-S`, whose string can hold more options).
fn env_options(
    rest: &[String],
    target: &dyn Fn(&str) -> DirChange,
    found: &mut Vec<DirChange>,
) -> Option<usize> {
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        at += 1;
        if word == "--" {
            break;
        }
        if word == "-" {
            continue;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            match name {
                "ignore-environment"
                | "null"
                | "debug"
                | "block-signal"
                | "default-signal"
                | "ignore-signal"
                | "list-signal-handling" => {}
                "unset" | "chdir" => {
                    let value = if let Some(value) = value {
                        value.to_string()
                    } else {
                        at += 1;
                        rest.get(at - 1)?.clone()
                    };
                    if name == "chdir" {
                        found.push(target(&value));
                    }
                }
                _ => return None,
            }
            continue;
        }
        if let Some(letters) = word.strip_prefix('-') {
            for (i, letter) in letters.char_indices() {
                let tail = &letters[i + letter.len_utf8()..];
                match letter {
                    'i' | '0' | 'v' => {}
                    'u' | 'C' => {
                        let value = if tail.is_empty() {
                            at += 1;
                            rest.get(at - 1)?.clone()
                        } else {
                            tail.to_string()
                        };
                        if letter == 'C' {
                            found.push(target(&value));
                        }
                        break;
                    }
                    _ => return None,
                }
            }
            continue;
        }
        if word.contains('=') {
            continue;
        }
        return Some(at - 1);
    }
    Some(at)
}

fn launcher_changes(
    words: &[String],
    home: Option<&Path>,
    depth: usize,
    found: &mut Vec<DirChange>,
) {
    if depth > 8 {
        found.push(DirChange::Unknown);
        return;
    }
    let mut at = 0;
    while let Some(word) = words.get(at) {
        let assignment = word.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if word == "!" || assignment {
            at += 1;
        } else {
            break;
        }
    }
    let Some(program) = words.get(at) else {
        return;
    };
    let rest = &words[at + 1..];
    let target = |operand: &str| -> DirChange {
        if crate::hooks::git_guard::unresolved_word(operand) || operand == "-" {
            return DirChange::Unknown;
        }
        if let Some(tail) = operand
            .strip_prefix("~/")
            .or((operand == "~").then_some(""))
        {
            return home.map_or(DirChange::Unknown, |home| DirChange::To(home.join(tail)));
        }
        if operand.starts_with(['~', '+']) {
            return DirChange::Unknown;
        }
        DirChange::To(PathBuf::from(operand))
    };
    // A GNU-prefixed coreutils name (`genv`, `gtimeout`) is the launcher it
    // names, as git-guard reads it.
    let name = crate::hooks::git_guard::launcher_name(program);
    let inner = |from: Option<usize>, found: &mut Vec<DirChange>| match from {
        Some(from) => launcher_changes(&rest[from.min(rest.len())..], home, depth + 1, found),
        None => found.push(DirChange::Unknown),
    };
    match name {
        "cd" | "chdir" | "pushd" => {
            found.extend(cd_destination(name, rest, home, &target));
        }
        "popd" | "dirs" => {}
        "source" | "." | "sudo" | "doas" => found.push(DirChange::Unknown),
        // zsh precommands run the command after them in this shell.
        "noglob" | "nocorrect" | "-" => inner(Some(0), found),
        "builtin" => inner(
            Some(usize::from(rest.first().is_some_and(|w| w == "--"))),
            found,
        ),
        // `xcrun -f` and `--find` only print a tool's path.
        "xcrun"
            if rest
                .first()
                .is_some_and(|w| matches!(w.as_str(), "-f" | "--find")) => {}
        // `command -v` and `-V` only print what a name is.
        "command"
            if rest
                .first()
                .is_some_and(|w| matches!(w.as_str(), "-v" | "-V")) => {}
        "command" | "exec" | "nohup" | "nice" | "stdbuf" | "time" | "xargs" | "caffeinate"
        | "xcrun" | "ionice" => {
            let from = skip_launcher_options(name, rest);
            inner(from, found);
        }
        "timeout" => {
            // The duration follows the options.
            let from = skip_launcher_options(name, rest).map(|from| from + 1);
            inner(from, found);
        }
        "env" => {
            let from = env_options(rest, &target, found);
            inner(from, found);
        }
        "git" => {
            let mut words = rest.iter();
            while let Some(word) = words.next() {
                if word == "-C" {
                    found.extend(words.next().map(|dir| target(dir)));
                } else if !word.starts_with('-') {
                    break;
                }
            }
        }
        // Any other word: a program that cannot move this shell, unless a
        // directory-change token follows it that the grammar above did not
        // consume. Then the word is an unmodeled launcher, and the
        // directory is unknown rather than unchanged (TSK-302 review round
        // 4): the guard never fails open on a word it does not know.
        _ => {
            if hides_a_directory_change(rest) {
                found.push(DirChange::Unknown);
            }
        }
    }
}

/// Where a `cd`, `chdir` or `pushd` moves: its operand, home when it has
/// none, or `None` for a directory already in the set (a `pushd` with no
/// operand or a stack index).
fn cd_destination(
    name: &str,
    rest: &[String],
    home: Option<&Path>,
    target: &dyn Fn(&str) -> DirChange,
) -> Option<DirChange> {
    let mut operands = rest.iter();
    let mut operand = None;
    while let Some(word) = operands.next() {
        if word == "--" {
            operand = operands.next();
            break;
        }
        // Every dash word is an option (`-L`, `-P`, `-q`, `-e`, `-@`); `-`
        // alone is the previous directory, and a `pushd` stack index (`-1`)
        // rotates to one already listed.
        if word.starts_with('-') && word.len() > 1 {
            if name == "pushd" && word[1..].chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            continue;
        }
        operand = Some(word);
        break;
    }
    match operand {
        Some(operand) if name == "pushd" && operand.starts_with('+') => None,
        Some(operand) => Some(target(operand)),
        None if name == "pushd" => None,
        None => Some(home.map_or(DirChange::Unknown, |home| DirChange::To(home.to_path_buf()))),
    }
}

/// Whether the words after an unmodeled program hold a directory-change
/// token: `cd`, `chdir`, `pushd` or `popd`, or `-C` or `--chdir` in any
/// spelling after an env-like word.
fn hides_a_directory_change(rest: &[String]) -> bool {
    let mut env_like = false;
    for word in rest {
        if matches!(word.as_str(), "cd" | "chdir" | "pushd" | "popd") {
            return true;
        }
        if crate::hooks::git_guard::launcher_name(word) == "env" {
            env_like = true;
        } else if env_like && (word.starts_with("-C") || word.starts_with("--chdir")) {
            return true;
        }
    }
    false
}

/// The directory set of `command` from `cwd`: every simple command the
/// line runs, nested bodies included, read in order.
pub(crate) fn line_directories(command: &str, cwd: &Path, home: Option<&Path>) -> DirSet {
    let mut set = DirSet {
        dirs: vec![cwd.to_path_buf()],
        unknown: false,
    };
    for segment in expand_commands(command) {
        let mut words = command_argv(&segment);
        strip_reserved_words(&mut words);
        for change in dir_changes(&words, home) {
            set.change(&change);
        }
    }
    set
}

#[allow(clippy::too_many_lines)] // Keep each launcher dispatch beside the shared recursion bound.
fn argv(args: &[String], cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    if depth > 32 {
        return;
    }
    let Some(program) = args.first() else {
        return;
    };
    let name = crate::hooks::git_guard::launcher_name(program);
    out.commands.push(args.to_vec());
    let rest = &args[1..];
    if name == "env" {
        if let (Some(dir), Some(inner)) = (
            rest.iter().find_map(|s| s.strip_prefix("GIT_DIR=")),
            skip_env(rest),
        ) {
            if inner.first().is_some_and(|s| s == "git") {
                let mut target = vec!["git".into(), format!("--git-dir={dir}")];
                target.extend_from_slice(&inner[1..]);
                argv(&target, cwd, depth + 1, out);
                return;
            }
        }
    }
    record_interpreter(name, rest, cwd, out);
    if let Err(reason) = direct_family(name, rest, cwd, out) {
        out.unreadable.push(reason);
    }
    if name == "git" {
        git_alias(rest, cwd, depth, out);
    }
    let inner = match name {
        "env" => {
            if let Some(at) = rest
                .iter()
                .position(|a| a == "-C" || a == "--chdir" || a.starts_with("--chdir="))
            {
                let path = rest[at]
                    .strip_prefix("--chdir=")
                    .or_else(|| rest.get(at + 1).map(String::as_str));
                if let (Some(cwd), Some(path), Some(inner)) = (cwd, path, skip_env(rest)) {
                    argv(inner, Some(&cwd.join(path)), depth + 1, out);
                }
            }
            if let Some(at) = rest.iter().position(|a| a == "-S" || a == "--split-string") {
                if let Some(script) = rest.get(at + 1) {
                    visit(script, cwd, depth + 1, out);
                }
            }
            skip_env(rest)
        }
        "command" | "builtin" | "exec" | "nohup" | "setsid" | "caffeinate" | "unbuffer"
        | "time" => Some(skip_options(rest, &["-a", "-t", "-w"])),
        // zsh precommands run the command after them in this shell.
        "noglob" | "nocorrect" | "-" => Some(rest),
        "xcrun" => Some(skip_options(rest, &["--sdk", "--toolchain"])),
        "ionice" => Some(skip_options(rest, &["-c", "-n", "-p", "-P", "-u"])),
        "nice" => Some(skip_options(rest, &["-n", "--adjustment"])),
        "stdbuf" => Some(skip_options(rest, &["-i", "-o", "-e"])),
        "timeout" => Some(
            skip_options(rest, &["-s", "--signal", "-k", "--kill-after"])
                .get(1..)
                .unwrap_or_default(),
        ),
        "xargs" => Some(skip_options(
            rest,
            &[
                "-I",
                "-L",
                "-n",
                "-P",
                "-s",
                "-d",
                "-E",
                "-a",
                "--arg-file",
                "--delimiter",
                "--eof",
                "--max-lines",
                "--max-args",
                "--max-procs",
                "--max-chars",
                "--replace",
            ],
        )),
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash" => {
            if let Some(script) = shell_command_string(rest) {
                visit(script, cwd, depth + 1, out);
            }
            None
        }
        "find" => {
            for (at, word) in rest.iter().enumerate() {
                if matches!(word.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
                    let tail = &rest[at + 1..];
                    let end = tail
                        .iter()
                        .position(|s| s == ";" || s == "+")
                        .unwrap_or(tail.len());
                    let supplier = Supplier::Find {
                        starts: find_starts(rest),
                        before: rest[..at].to_vec(),
                        cwd: cwd.map(Path::to_path_buf),
                        in_dir: matches!(word.as_str(), "-execdir" | "-okdir"),
                    };
                    let outer = out.supplier.replace(supplier);
                    argv(&tail[..end], cwd, depth + 1, out);
                    out.supplier = outer;
                }
            }
            None
        }
        "npx" | "bunx" | "pnpx" | "pnx" => {
            for at in after_runner_options(rest) {
                let mut inner = rest.get(at..).unwrap_or_default().to_vec();
                if let Some(program) = inner.first_mut() {
                    *program = package_bin(program);
                }
                argv(&inner, cwd, depth + 1, out);
            }
            None
        }
        _ => {
            unmodeled_launch(name, rest, depth, out);
            None
        }
    };
    if let Some(inner) = inner {
        let outer = if name == "xargs" {
            out.supplier.replace(Supplier::Xargs)
        } else {
            out.supplier.clone()
        };
        argv(inner, cwd, depth + 1, out);
        out.supplier = outer;
    }
}

/// The launchers `argv` unwraps, read by their bare name.
const MODELED_LAUNCHERS: &[&str] = &[
    "env",
    "command",
    "builtin",
    "exec",
    "nohup",
    "setsid",
    "caffeinate",
    "unbuffer",
    "time",
    "noglob",
    "nocorrect",
    "xcrun",
    "ionice",
    "nice",
    "stdbuf",
    "timeout",
    "xargs",
];

/// A program the grammar does not model may itself launch what follows it
/// (`frobnicate env -C.claude python3 tool.py settings.json`), or run a
/// script string it is given (`expect -c '...'`, `osascript -e '...'`,
/// `ssh host '...'`). Each such reading is a possible call whose directory
/// is unknown, so its known words are judged by their trailing components
/// and authority keeps its fixed level (TSK-302, review rounds 5 and 6):
///
/// - a later word in a command position that is a modeled launcher or an
///   interpreter starts a call made of the rest of the segment; a bare
///   `--` does not end the walk (`taskpolicy -- python3`);
/// - before a `--`, the value of `-c` or `-e` is read as a command line;
/// - any word that holds a blank or a shell operator was quoted, and is read
///   as a command line too;
/// - before a `--`, a single-word `-m` value is a message, never a command.
///
/// `echo` and `printf` only print their operands, so their quoted words and
/// option values are data, never script strings: documentation text written
/// with them stays allowed (TSK-302 AC-5).
fn unmodeled_launch(name: &str, rest: &[String], depth: usize, out: &mut Parsed) {
    if name == "git" || interpreter_family(name).is_some() {
        return;
    }
    // Only the calls are kept: the rest of what these readings find stays
    // with the checks that read the modeled grammar.
    let mut inner = Parsed {
        supplier: out.supplier.clone(),
        ..Parsed::default()
    };
    let prints = matches!(name, "echo" | "printf");
    let mut operands = false;
    let mut words = rest.iter().enumerate();
    while let Some((at, word)) = words.next() {
        match word.as_str() {
            "--" if !operands => {
                operands = true;
                continue;
            }
            "-c" | "-e" if !operands && !prints => {
                if let Some((_, script)) = words.next() {
                    visit(script, None, depth + 1, &mut inner);
                }
                continue;
            }
            "-m" if !operands => {
                if let Some((_, message)) = words.next() {
                    if !prints && holds_a_command_line(message) {
                        visit(message, None, depth + 1, &mut inner);
                    }
                }
                continue;
            }
            _ => {}
        }
        if !prints && holds_a_command_line(word) {
            visit(word, None, depth + 1, &mut inner);
            continue;
        }
        let bare = crate::hooks::git_guard::launcher_name(word);
        if MODELED_LAUNCHERS.contains(&bare) || interpreter_family(bare).is_some() {
            argv(&rest[at..], None, depth + 1, &mut inner);
            break;
        }
    }
    out.calls.extend(inner.calls);
    out.opaque |= inner.opaque;
    out.runs |= inner.runs;
}

/// Whether one word holds more than a bare name: a shell blank (space,
/// tab, newline) or a shell operator, which the shell only leaves in a word
/// that was quoted.
fn holds_a_command_line(word: &str) -> bool {
    word.contains([' ', '\t', '\n', ';', '&', '|'])
}

/// The starting points of a `find` call: the words before its expression,
/// after the `-H`, `-L` and `-P` options, or `.` when there are none.
fn find_starts(args: &[String]) -> Vec<String> {
    let rest = &args[args
        .iter()
        .take_while(|a| matches!(a.as_str(), "-H" | "-L" | "-P"))
        .count()..];
    let starts: Vec<String> = rest
        .iter()
        .take_while(|a| !a.starts_with('-') && !matches!(a.as_str(), "!" | "("))
        .cloned()
        .collect();
    if starts.is_empty() {
        vec![".".to_string()]
    } else {
        starts
    }
}

/// The directory operand of a `cd`, after its options (`-L`, `-P`, `-e`,
/// `-@`) and an ending `--`; `None` for `cd` alone, which goes home.
fn cd_operand(args: &[String]) -> Option<&str> {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            return words.next().map(String::as_str);
        }
        let options = word.strip_prefix('-').filter(|letters| {
            !letters.is_empty() && letters.chars().all(|c| matches!(c, 'L' | 'P' | 'e' | '@'))
        });
        if options.is_none() {
            return Some(word);
        }
    }
    None
}

/// The interpreter a program name runs, by its family name: a version
/// suffix (`python3.13`, `ruby3.3`) and a Windows `.exe` are read as the
/// interpreter they name.
fn interpreter_family(name: &str) -> Option<&'static str> {
    let name = name.strip_suffix(".exe").unwrap_or(name);
    // The name without its version suffix: the digits and dots it ends with.
    let end = name
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_ascii_digit() || *c == '.'))
        .map_or(0, |(at, c)| at + c.len_utf8());
    match &name[..end] {
        "python" => Some("python"),
        "node" => Some("node"),
        "nodejs" => Some("nodejs"),
        "perl" => Some("perl"),
        "ruby" => Some("ruby"),
        _ => None,
    }
}

fn record_interpreter(name: &str, rest: &[String], cwd: Option<&Path>, out: &mut Parsed) {
    if let Some(name) = interpreter_family(name) {
        let flag = if name.starts_with("python") {
            "-c"
        } else {
            "-e"
        };
        let mut found = false;
        let code_from = out.code.len();
        for (at, arg) in rest.iter().enumerate() {
            let cluster = arg
                .strip_prefix('-')
                .filter(|s| !s.starts_with('-'))
                .is_some_and(|s| {
                    s.chars().all(|c| c.is_ascii_alphabetic())
                        && (!arg.starts_with(flag)
                            || arg == flag
                            || (name.starts_with("node") && s == "ep"))
                        && match name {
                            "perl" | "ruby" => s.contains(['e', 'E']),
                            "node" | "nodejs" => s.contains(['e', 'p']),
                            _ => s.ends_with('c'),
                        }
                });
            if let Some(code) = (name == "perl").then(|| perl_attached_code(arg)).flatten() {
                out.code.push(code.to_string());
                found = true;
            } else if cluster
                || arg == flag
                || (name.starts_with("node") && matches!(arg.as_str(), "--eval" | "-p" | "--print"))
            {
                if let Some(code) = rest.get(at + 1) {
                    out.code.push(code.clone());
                    found = true;
                }
            } else if let Some(code) = arg.strip_prefix(flag).filter(|code| !code.is_empty()) {
                out.code.push(code.to_string());
                found = true;
            } else if let Some(code) = arg.strip_prefix("--eval=") {
                out.code.push(code.to_string());
                found = true;
            }
        }
        if !found {
            out.stdin_interpreter += 1;
        }
        // The code collected above is read conservatively, from any word
        // that may be code; the arguments come from the family's own option
        // grammar, so a word after the code or script boundary is never
        // dropped as code (TSK-302 review F1).
        let (args, chdir) = interpreter_arguments(name, rest);
        out.calls.push(InterpreterCall {
            args,
            cwd: cwd.map(Path::to_path_buf),
            chdir,
            code: out.code[code_from..].to_vec(),
            supplier: out.supplier.clone(),
        });
    }
}

/// How a short option of an interpreter family reads.
enum Short {
    /// Takes no value.
    Flag,
    /// Takes inline code, attached or as the next word.
    Code,
    /// Takes a value, attached or as the next word.
    Value,
    /// Takes the next word as a module and ends the options (`python -m`).
    Module,
    /// Changes directory to its value, attached or the next word (`ruby -C`).
    Chdir,
    /// Takes an optional value attached to it, which is the rest of the word.
    Attached,
}

fn short_option(family: &str, letter: char) -> Short {
    match (family, letter) {
        ("python", 'c')
        | ("perl" | "ruby", 'e')
        | ("perl", 'E')
        | ("node" | "nodejs", 'e' | 'p') => Short::Code,
        ("python", 'm') => Short::Module,
        ("python", 'W' | 'X')
        | ("perl", 'I' | 'M' | 'm')
        | ("ruby", 'I' | 'r' | 'E')
        | ("node" | "nodejs", 'r' | 'C') => Short::Value,
        ("ruby", 'C') => Short::Chdir,
        ("perl", 'i' | 'x' | '0' | 'l' | 'F' | 'd' | 'D' | 'C' | 'V')
        | ("ruby", 'i' | 'x' | '0' | 'F' | 'K' | 'T' | 'W') => Short::Attached,
        _ => Short::Flag,
    }
}

/// Long options that take the next word as their value when written
/// without `=`.
fn long_takes_value(family: &str, name: &str) -> bool {
    match family {
        "python" => name == "check-hash-based-pycs",
        "ruby" => matches!(
            name,
            "encoding" | "external-encoding" | "internal-encoding" | "enable" | "disable"
        ),
        "node" | "nodejs" => matches!(
            name,
            "require"
                | "import"
                | "conditions"
                | "input-type"
                | "loader"
                | "experimental-loader"
                | "env-file"
                | "title"
                | "redirect-warnings"
                | "diagnostic-dir"
                | "report-directory"
                | "cpu-prof-dir"
                | "heap-prof-dir"
        ),
        _ => false,
    }
}

/// The words an interpreter hands its program, read by the family's
/// option grammar, and a `ruby -C` directory. Its own options' values are
/// kept, inline code is left out, and the script and every word after the
/// boundary are kept whole: `python -c CODE`, `-m MODULE`, `-`, `--` or a
/// script ends the options, and so does node's eval code; perl and ruby
/// read options until the first other word. An option the table does not
/// know is a flag, so a word after it stays an argument either way.
fn interpreter_arguments(family: &str, rest: &[String]) -> (Vec<String>, Vec<String>) {
    let mut args = Vec::new();
    let mut chdir = Vec::new();
    let mut at = 0;
    let after = |at: usize| rest.get(at..).unwrap_or_default().to_vec();
    while let Some(word) = rest.get(at) {
        at += 1;
        if word == "--" {
            args.extend(after(at));
            return (args, chdir);
        }
        if word == "-" || !word.starts_with('-') {
            args.extend(after(at - 1));
            return (args, chdir);
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            if matches!(family, "node" | "nodejs") && matches!(name, "eval" | "print") {
                at += usize::from(value.is_none());
                args.extend(after(at));
                return (args, chdir);
            }
            if value.is_none() && long_takes_value(family, name) {
                args.extend(rest.get(at).cloned());
                at += 1;
            } else {
                args.push(word.clone());
            }
            continue;
        }
        let letters = &word[1..];
        for (i, letter) in letters.char_indices() {
            let tail = &letters[i + letter.len_utf8()..];
            // Node never attaches a value to a short option (`-pe CODE`).
            let attached = !tail.is_empty() && !matches!(family, "node" | "nodejs");
            let value = |at: &mut usize| {
                if attached {
                    Some(tail.to_string())
                } else {
                    *at += 1;
                    rest.get(*at - 1).cloned()
                }
            };
            match short_option(family, letter) {
                Short::Flag => continue,
                Short::Attached => {}
                Short::Code => {
                    if matches!(family, "node" | "nodejs") && !tail.is_empty() {
                        // `-pe`: the code is the next word.
                        at += 1;
                    } else {
                        value(&mut at);
                    }
                    if !matches!(family, "perl" | "ruby") {
                        args.extend(after(at));
                        return (args, chdir);
                    }
                }
                Short::Value => args.extend(value(&mut at)),
                Short::Chdir => chdir.extend(value(&mut at)),
                Short::Module => {
                    // `-m MODULE`: a module name, not a path; the words
                    // after it are the program's argv (TSK-302 R2-4).
                    value(&mut at);
                    args.extend(after(at));
                    return (args, chdir);
                }
            }
            break;
        }
    }
    (args, chdir)
}

// Only cross flags that do not consume a value. In particular, the `e` in
// `-Mfeature` or a backup suffix is not an evaluation switch.
fn perl_attached_code(arg: &str) -> Option<&str> {
    let flags = arg.strip_prefix('-')?;
    for (at, flag) in flags.char_indices() {
        match flag {
            'e' | 'E' => return flags.get(at + 1..).filter(|code| !code.is_empty()),
            'a' | 'c' | 'f' | 'l' | 'n' | 'p' | 's' | 't' | 'T' | 'u' | 'U' | 'w' | 'W' | 'X' => {}
            _ => return None,
        }
    }
    None
}

fn direct_family(
    name: &str,
    rest: &[String],
    cwd: Option<&Path>,
    out: &mut Parsed,
) -> Result<(), String> {
    let source = || {
        if name == "git" && has_explicit_git_directory(rest) {
            git_policy_root(cwd, rest)
        } else {
            Ok(None)
        }
    };
    let mut canonical = vec![name.to_string()];
    canonical.extend(global_args(name, rest).iter().cloned());
    for family in &table().families {
        if family
            .codex
            .iter()
            .any(|rule| prefix(&canonical, &rule.pattern))
        {
            out.families.push((family_id(&family.id), source()?));
        }
    }
    let sub = canonical.get(1).map_or("", String::as_str);
    let body = canonical.get(2..).unwrap_or_default();
    if name == "git" && sub == "tag" {
        let tag_args = skip_options(
            body,
            &["-m", "--message", "-F", "--file", "-u", "--local-user"],
        );
        if !body.iter().any(|a| {
            matches!(
                a.as_str(),
                "-d" | "--delete" | "-l" | "--list" | "-v" | "--verify"
            )
        }) {
            if let (Some(dir), Some(tag)) = (git_directory(cwd, rest)?, tag_args.first()) {
                out.created_tags.push((dir, tag.clone()));
            }
        }
    }
    if name == "git" && sub == "update-ref" {
        if let (Some(dir), Some(tag)) = (
            git_directory(cwd, rest)?,
            body.iter().find_map(|a| a.strip_prefix("refs/tags/")),
        ) {
            out.created_tags.push((dir, tag.to_owned()));
        }
    }
    if name == "git" && sub == "config" && enables_follow_tags(body) {
        out.families.push(("release", source()?));
    }
    if name == "git" && sub == "push" {
        if body.iter().any(|a| a == "--mirror") {
            out.families.push(("account", source()?));
        }
        if body.iter().any(|a| {
            matches!(a.as_str(), "--tags" | "--follow-tags" | "tag") || a.contains("refs/tags/")
        }) || tag_push(body, cwd, rest, &out.created_tags)?
        {
            out.families.push(("release", source()?));
        } else if follow_tags(cwd, rest)? {
            out.families.push(("configured_tag_push", source()?));
        }
    }
    if name == "gh" {
        if sub == "repo"
            && body.first().is_some_and(|s| s == "edit")
            && body
                .iter()
                .any(|s| s == "--visibility" || s.starts_with("--visibility="))
        {
            out.families.push(("account", source()?));
        }
        if sub == "api"
            && body.iter().enumerate().any(|(i, a)| {
                a.eq_ignore_ascii_case("-XDELETE")
                    || a.eq_ignore_ascii_case("--method=DELETE")
                    || ((a == "-X" || a == "--method")
                        && body
                            .get(i + 1)
                            .is_some_and(|v| v.eq_ignore_ascii_case("DELETE")))
            })
        {
            out.families.push(("account", source()?));
        }
    }
    if name.starts_with("git-credential-") {
        out.families.push(("account", source()?));
    }
    if matches!(name, "runuser" | "gsudo.exe" | "runas.exe")
        || (name == "osascript" && rest.join(" ").contains("with administrator privileges"))
    {
        out.families.push(("privilege", source()?));
    }
    if name.eq_ignore_ascii_case("Set-ExecutionPolicy")
        || (name == "crontab"
            && !rest
                .iter()
                .any(|a| matches!(a.as_str(), "-l" | "--help" | "-h")))
    {
        out.families.push(("persistence", source()?));
    }
    Ok(())
}

fn has_explicit_git_directory(args: &[String]) -> bool {
    args.iter()
        .any(|s| s == "-C" || s == "--git-dir" || s.starts_with("--git-dir="))
}

fn family_id(id: &str) -> &'static str {
    match id {
        "privilege" => "privilege",
        "publish" => "publish",
        "release" => "release",
        "account" => "account",
        "keychain" => "keychain",
        "persistence" => "persistence",
        _ => unreachable!("action table family lacks guard routing"),
    }
}

// Only options before the subcommand are skipped. Data after an ordinary
// subcommand (for example cargo test --features publish) is never an action.
fn global_args<'a>(name: &str, rest: &'a [String]) -> &'a [String] {
    let values: &[&str] = match name {
        "git" => super::git::GLOBAL_VALUE_OPTIONS,
        "gh" => &["-R", "--repo", "--hostname"],
        "cargo" => &[
            "--manifest-path",
            "--config",
            "--color",
            "--target-dir",
            "-Z",
        ],
        "npm" | "pnpm" | "yarn" | "uv" => &[
            "--prefix",
            "--registry",
            "--cwd",
            "--dir",
            "-C",
            "--project",
            "--directory",
            "--cache-dir",
        ],
        _ => &[],
    };
    if !matches!(
        name,
        "git"
            | "gh"
            | "cargo"
            | "npm"
            | "pnpm"
            | "yarn"
            | "uv"
            | "systemctl"
            | "security"
            | "defaults"
            | "launchctl"
            | "twine"
            | "gem"
    ) {
        return rest;
    }
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        if name == "cargo" && word.starts_with('+') {
            at += 1;
        } else if word == "--" {
            at += 1;
            break;
        } else if values.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') {
            at += 1;
        } else {
            break;
        }
    }
    rest.get(at..).unwrap_or_default()
}

fn prefix(args: &[String], pattern: &[PatternToken]) -> bool {
    args.len() >= pattern.len()
        && args.iter().zip(pattern).all(|(arg, token)| match token {
            PatternToken::Word(word) => arg == word,
            PatternToken::AnyOf(words) => words.contains(arg),
        })
}

fn git_policy_root(cwd: Option<&Path>, globals: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(mut dir) = cwd.map(Path::to_path_buf) else {
        return Ok(None);
    };
    let mut git_dir = None;
    let args = global_args("git", globals);
    let end = globals.len() - args.len();
    let mut at = 0;
    while at < end {
        match globals[at].as_str() {
            "-C" => {
                dir = dir.join(globals.get(at + 1).ok_or("git -C has no directory")?);
                at += 1;
            }
            "--git-dir" => {
                git_dir = Some(
                    globals
                        .get(at + 1)
                        .ok_or("git --git-dir has no directory")?
                        .clone(),
                );
                at += 1;
            }
            value => {
                if let Some(path) = value.strip_prefix("--git-dir=") {
                    git_dir = Some(path.to_owned());
                }
            }
        }
        at += 1;
    }
    let repo = if let Some(path) = git_dir {
        let path = dir.join(path);
        // An explicit git directory must name that repository, never an ancestor.
        match git2::Repository::open(&path) {
            Ok(repo) => Some(repo),
            Err(e)
                if e.code() == git2::ErrorCode::NotFound
                    && crate::absence::proven_absent(&path).map_err(|e| e.to_string())? =>
            {
                None
            }
            Err(e) => {
                return Err(format!(
                    "cannot open command repository {}: {e}",
                    path.display()
                ))
            }
        }
    } else {
        crate::hooks::repo::open(&dir)?
    };
    Ok(repo.map(|repo| repo.workdir().unwrap_or(repo.path()).to_path_buf()))
}

fn git_directory(cwd: Option<&Path>, globals: &[String]) -> Result<Option<PathBuf>, String> {
    let Some(root) = git_policy_root(cwd, globals)? else {
        return Ok(None);
    };
    let repo = crate::hooks::repo::open(&root)?.ok_or("command repository disappeared")?;
    repo.commondir().canonicalize().map(Some).map_err(|e| {
        format!(
            "cannot resolve command repository {}: {e}",
            repo.commondir().display()
        )
    })
}

fn tag_push(
    args: &[String],
    cwd: Option<&Path>,
    globals: &[String],
    created: &[(PathBuf, String)],
) -> Result<bool, String> {
    let Some(dir) = git_directory(cwd, globals)? else {
        return Ok(false);
    };
    let repo = crate::hooks::repo::open(&dir)?.ok_or("command repository disappeared")?;
    for arg in args.iter().filter(|arg| !arg.starts_with('-')) {
        for part in arg.trim_start_matches('+').split(':') {
            let shorthand = part
                .strip_prefix("tags/")
                .or_else(|| part.strip_prefix("refs/tags/"));
            for tag_name in [Some(part), shorthand]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
            {
                if created
                    .iter()
                    .any(|(at, tag)| at == &dir && tag == tag_name)
                {
                    return Ok(true);
                }
                let reference = format!("refs/tags/{tag_name}");
                // A URL, wildcard or other invalid ref spelling cannot name a literal tag.
                if !git2::Reference::is_valid_name(&reference) {
                    continue;
                }
                match repo.find_reference(&reference) {
                    Ok(_) => return Ok(true),
                    Err(e) if e.code() == git2::ErrorCode::NotFound => {}
                    Err(e) => return Err(format!("cannot read tag {tag_name}: {e}")),
                }
            }
        }
    }
    Ok(false)
}

fn truthy(value: &str) -> bool {
    !matches!(
        value.to_ascii_lowercase().as_str(),
        "false" | "no" | "off" | "0"
    )
}

fn enables_follow_tags(args: &[String]) -> bool {
    if args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--get"
                | "--get-all"
                | "--get-regexp"
                | "--list"
                | "-l"
                | "--unset"
                | "--unset-all"
                | "get"
                | "list"
                | "unset"
        )
    }) {
        return false;
    }
    args.windows(2)
        .any(|p| p[0].eq_ignore_ascii_case("push.followTags") && truthy(&p[1]))
}

fn follow_tags(cwd: Option<&Path>, globals: &[String]) -> Result<bool, String> {
    let sub = global_args("git", globals);
    if let Some(option) = sub
        .iter()
        .rev()
        .find(|s| matches!(s.as_str(), "--follow-tags" | "--no-follow-tags"))
    {
        return Ok(option == "--follow-tags");
    }
    let end = globals.len() - sub.len();
    let override_value = globals[..end]
        .windows(2)
        .filter(|p| p[0] == "-c")
        .map(|p| p[1].split_once('=').unwrap_or((&p[1], "true")))
        .filter(|(key, _)| key.eq_ignore_ascii_case("push.followTags"))
        .map(|(_, value)| truthy(value))
        .next_back();
    if let Some(value) = override_value {
        return Ok(value);
    }
    let Some(dir) = git_directory(cwd, globals)? else {
        return Ok(false);
    };
    let repo = crate::hooks::repo::open(&dir)?.ok_or("command repository disappeared")?;
    let config = repo
        .config()
        .map_err(|e| format!("cannot read command repository config: {e}"))?;
    match config.get_bool("push.followTags") {
        Ok(value) => Ok(value),
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(false),
        Err(e) => Err(format!("cannot read push.followTags: {e}")),
    }
}

fn git_alias(rest: &[String], cwd: Option<&Path>, depth: usize, out: &mut Parsed) {
    let sub = global_args("git", rest);
    let Some(name) = sub.first() else {
        return;
    };
    if crate::hooks::git_guard::GIT_BUILTINS.contains(&name.as_str()) {
        return;
    }
    let Some(mut dir) = cwd.map(Path::to_path_buf) else {
        return;
    };
    let source = has_explicit_git_directory(rest)
        .then(|| git_policy_root(cwd, rest))
        .transpose();
    let source = match source {
        Ok(source) => source.flatten(),
        Err(reason) => {
            out.unreadable.push(reason);
            return;
        }
    };
    let end = rest.len() - sub.len();
    // Preserve the command's directory and configuration order, including
    // relative includes and worktree-specific conditional includes.
    let mut config = Vec::new();
    let mut git_dir = None;
    let mut config_unknown = false;
    let mut at = 0;
    while at < end {
        match rest[at].as_str() {
            "-c" => {
                config.extend(rest.get(at + 1).cloned());
                at += 1;
            }
            "-C" => {
                if let Some(path) = rest.get(at + 1) {
                    dir = dir.join(path);
                }
                at += 1;
            }
            "--git-dir" => {
                git_dir = rest.get(at + 1).map(String::as_str);
                at += 1;
            }
            "--work-tree" | "--namespace" => at += 1,
            "--config-env" => {
                config_unknown = true;
                at += 1;
            }
            value => {
                if let Some(path) = value.strip_prefix("--git-dir=") {
                    git_dir = Some(path);
                } else if value.starts_with("--config-env=") {
                    config_unknown = true;
                } else if let Some(setting) = value.strip_prefix("-c") {
                    config.push(setting.to_string());
                }
            }
        }
        at += 1;
    }
    let answer = if config_unknown || depth >= 32 {
        AliasAnswer::Unreadable("alias configuration or expansion is unresolved".into())
    } else {
        read_alias(
            &dir,
            &AliasQuery {
                target: git_dir.map(|path| Retarget {
                    path,
                    git_dir: true,
                }),
                config: &config,
                name,
            },
        )
    };
    let value = match answer {
        AliasAnswer::NotAlias => return,
        AliasAnswer::Expansion(value) if !value.starts_with('!') => split_alias(&value),
        // Neither an unreadable byte string nor shell code can prove its ref
        // operands safe. Do not inspect replacement characters or guess from
        // top-level words: a shell alias can hide another git invocation.
        AliasAnswer::Expansion(_) | AliasAnswer::Unreadable(_) => None,
    };
    if let Some(words) = value {
        let mut args = vec!["git".to_string()];
        args.extend_from_slice(&rest[..end]);
        args.extend(words);
        args.extend_from_slice(&sub[1..]);
        argv(&args, cwd, depth + 1, out);
    } else {
        out.families.push(("release", source));
    }
}

/// Where an interpreter family takes its program from.
#[derive(PartialEq, Eq)]
enum Program {
    /// `-c`, `-e` or `--eval` code on the command line.
    Inline,
    /// `python -m MODULE`.
    Module,
    /// A script operand other than `-`.
    Script,
    /// Standard input: no script, or the script `-`.
    Stdin,
}

/// Where an interpreter family of `family` takes its program from, read by
/// the same option grammar as [`interpreter_arguments`].
fn family_program(family: &str, rest: &[String]) -> Program {
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        at += 1;
        if word == "--" {
            return match rest.get(at) {
                Some(script) if script != "-" && !descriptor_program(script) => Program::Script,
                _ => Program::Stdin,
            };
        }
        if word == "-" || descriptor_program(word) {
            return Program::Stdin;
        }
        if !word.starts_with('-') {
            return Program::Script;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, value) = long
                .split_once('=')
                .map_or((long, None), |(n, v)| (n, Some(v)));
            if matches!(family, "node" | "nodejs") && matches!(name, "eval" | "print") {
                return Program::Inline;
            }
            if value.is_none() && long_takes_value(family, name) {
                at += 1;
            }
            continue;
        }
        let letters = &word[1..];
        for (i, letter) in letters.char_indices() {
            let attached = !letters[i + letter.len_utf8()..].is_empty()
                && !matches!(family, "node" | "nodejs");
            match short_option(family, letter) {
                Short::Flag => continue,
                Short::Code => return Program::Inline,
                Short::Module => return Program::Module,
                Short::Value | Short::Chdir => at += usize::from(!attached),
                Short::Attached => {}
            }
            break;
        }
    }
    Program::Stdin
}

/// The bodies of the process substitutions (`<(...)`, `>(...)`) in one
/// segment, read outside quotes.
fn process_substitutions(segment: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = segment.char_indices().collect();
    let mut found = Vec::new();
    let mut quote = None;
    let mut at = 0;
    while let Some(&(_, ch)) = chars.get(at) {
        at += 1;
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        let Some(&(open, '(')) = chars.get(at).filter(|_| matches!(ch, '<' | '>')) else {
            continue;
        };
        // The body runs to the matching parenthesis, quotes respected.
        let mut depth = 0;
        let mut inner = None;
        let mut end = segment.len();
        at += 1;
        while let Some(&(offset, c)) = chars.get(at) {
            at += 1;
            if let Some(q) = inner {
                if c == q {
                    inner = None;
                }
                continue;
            }
            match c {
                '\'' | '"' => inner = Some(c),
                '(' => depth += 1,
                ')' if depth == 0 => {
                    end = offset;
                    break;
                }
                ')' => depth -= 1,
                _ => {}
            }
        }
        found.push(segment[open + 1..end].to_string());
    }
    found
}

/// The text after a here-string operator (`<<<`) in one segment, read
/// outside quotes, as written.
fn here_string(segment: &str) -> Option<String> {
    let mut quote = None;
    for (at, ch) in segment.char_indices() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if segment[at..].starts_with("<<<") {
            // Kept as written: the readers split on quotes and blanks.
            return Some(segment[at + 3..].to_string());
        }
    }
    None
}

/// Read where a shell or interpreter in one pipeline segment takes its
/// code from (TSK-302 review round 7). Code the guard can read, a quoted
/// producer, a here-string or a process substitution, is kept as that
/// program's code so its paths are judged precisely; any program fed from
/// a pipe, a here-string, a process substitution or a `-c`/`-e` value
/// holding a substitution marks the line opaque.
fn read_program_sources(
    name: &str,
    rest: &[String],
    segment: &str,
    producers: &str,
    out: &mut Parsed,
) {
    let shell = matches!(
        name,
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" | "source" | "."
    );
    let family = interpreter_family(name);
    if !shell && family.is_none() {
        return;
    }
    out.runs = true;
    let substituted = |code: &str| code.contains("$(") || code.contains('`');
    let reads_stdin = if let Some(family) = family {
        let mut inline = Parsed::default();
        record_interpreter(name, rest, None, &mut inline);
        if inline.code.iter().any(|code| substituted(code)) {
            out.opaque = true;
        }
        family_program(family, rest) == Program::Stdin
    } else if let Some(script) = shell_command_string(rest) {
        if substituted(script) {
            out.code.push(script.to_string());
            out.opaque = true;
        }
        false
    } else {
        let operand = rest.iter().find(|a| !a.starts_with('-'));
        operand.is_none_or(|operand| descriptor_program(operand))
            || rest.iter().take_while(|a| a.as_str() != "--").any(|a| {
                a.strip_prefix('-')
                    .is_some_and(|flags| !flags.starts_with('-') && flags.contains('s'))
            })
    };
    let substitutions = process_substitutions(segment);
    if !substitutions.is_empty() {
        out.opaque = true;
        out.code.extend(substitutions);
    }
    if reads_stdin {
        if let Some(body) = here_string(segment) {
            out.opaque = true;
            out.code.push(body);
        }
        if !producers.is_empty() {
            out.opaque = true;
            out.code.push(producers.to_string());
        }
    }
}

// Keep pipeline boundaries and quoted producer text. Only a shell that reads
// stdin makes that text code; ordinary pipes and separate commands stay data.
fn shell_inputs(command: &str, out: &mut Parsed) {
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    let mut producers = String::new();
    for (at, ch) in command
        .char_indices()
        .chain(std::iter::once((command.len(), ';')))
    {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            continue;
        }
        if !matches!(ch, '|' | ';' | '&' | '\n') {
            continue;
        }
        let segment = &command[start..at];
        let words = command_argv(segment);
        if let Some((program, rest)) = strip_launchers(&words) {
            let name = program.rsplit('/').next().unwrap_or(program);
            read_program_sources(name, rest, segment, &producers, out);
        }
        if ch == '|' && !command[at..].starts_with("||") {
            producers.push_str(segment);
            producers.push(' ');
        } else {
            producers.clear();
        }
        start = at + ch.len_utf8();
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn unreadable_and_shell_aliases_fail_closed_through_all_config_sources() {
        use std::io::Write as _;
        let temp = tempfile::tempdir().unwrap();
        git2::Repository::init(temp.path()).unwrap();
        let included = temp.path().join("aliases");
        std::fs::write(&included, b"[alias]\n included = push origin caf\xff\n").unwrap();
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(temp.path().join(".git/config"))
            .unwrap();
        config.write_all(b"[alias]\n bad = push origin caf\xff\n nested = bad\n shell = !sh -c 'git push origin caf\xff'\n text = !sh -c 'git bad'\n safe = log --oneline\n").unwrap();
        let cli_included = temp.path().join("cli-aliases");
        std::fs::write(&cli_included, b"[alias]\n cli = push origin caf\xff\n").unwrap();
        let conditional = temp.path().join("conditional-aliases");
        std::fs::write(
            &conditional,
            b"[alias]\n conditional = push origin caf\xff\n",
        )
        .unwrap();
        // Paths are written with `/`: git reads `\` in a config value as an
        // escape, so a Windows path would make the whole file unreadable.
        // The condition names the temporary folder alone, which git matches
        // at any depth, so a Windows short name (`RUNNER~1`) or a linked
        // temporary root on macOS reads the same.
        let slashed = crate::portable_path::slashed;
        writeln!(config, "[include]\n path = {}", slashed(&included)).unwrap();
        writeln!(
            config,
            "[includeIf \"gitdir:{}/.git\"]\n path = {}",
            temp.path().file_name().unwrap().to_str().unwrap(),
            slashed(&conditional)
        )
        .unwrap();
        drop(config);
        let loaded = crate::git::command()
            .current_dir(temp.path())
            .args(["config", "--get", "alias.conditional"])
            .output()
            .unwrap();
        assert!(loaded.status.success(), "the conditional source applies");
        for command in [
            "git bad".to_string(),
            "git nested".into(),
            "git shell".into(),
            "git text".into(),
            "git included".into(),
            "git -c alias.outer=bad outer".into(),
            "git -c 'alias.outer=!sh -c git\\ bad' outer".into(),
            format!("git -c include.path={} cli", slashed(&cli_included)),
            "git -c include.path=cli-aliases cli".into(),
            "git conditional".into(),
        ] {
            assert!(
                !evaluate_at(&command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate_at("git safe", &SecuritySection::default(), Some(temp.path())).is_empty());
        assert!(evaluate_at(
            "git -c 'alias.bad=log --oneline' bad",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }
    use super::*;
    use crate::security::actions::{table, PatternToken};

    #[test]
    fn n1_tag_shorthand_matches_existing_and_new_tags() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        let id = repo.blob(b"tag target").unwrap();
        repo.tag_lightweight("v1.2.3", &repo.find_object(id, None).unwrap(), false)
            .unwrap();
        for command in [
            "git push origin tags/v1.2.3",
            "git push origin +tags/v1.2.3",
            "git push origin refs/tags/v1.2.3",
            "git tag v9 && git push origin tags/v9",
            "git tag v9 && git push origin HEAD:tags/v9",
        ] {
            assert!(
                !evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        for command in [
            "git push origin tags-x",
            "git push origin tags/task/x",
            "git tag v9 && git push origin tags-x",
        ] {
            assert!(
                evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn n2_valueless_follow_tags_is_true() {
        for command in [
            "git -c push.followTags push origin task/x",
            "git -c PUSH.FOLLOWTAGS push origin task/x",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "git -c push.followTags=false push origin task/x",
            "git -c push.followTags -c push.followTags=false push origin task/x",
            "git -c push.followTags push --no-follow-tags origin task/x",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn git_global_options_with_a_value_word_are_skipped() {
        // Issue 120: the value word of a git global option is never read as
        // the subcommand, so a publication behind one is still seen.
        for command in [
            "git --attr-source HEAD push --tags",
            "git --shallow-file x push --tags",
            "git --config-env color.ui=C push --tags",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn n5_perl_code_attached_to_clusters() {
        for command in [
            r#"perl -pe'system("cargo publish")'"#,
            r#"perl -E'system("cargo publish")'"#,
            r#"perl -lwe'system("cargo publish")'"#,
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "perl -pe's/a/b/' README.md",
            "perl -lwe'print 1'",
            "perl -Mfeature -pe'print' README.md",
            "perl -i.backup -pe's/a/b/' README.md",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn n6_effective_follow_tags_names_cause_and_route() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.config()
            .unwrap()
            .set_bool("push.followTags", true)
            .unwrap();
        let findings = evaluate_at(
            "git push origin task/x",
            &SecuritySection::default(),
            Some(temp.path()),
        );
        assert_eq!(findings.len(), 1);
        assert!(
            findings[0].message.contains("effective")
                && findings[0].message.contains("push.followTags"),
            "{findings:?}"
        );
        assert!(
            findings[0].remedy.contains("--no-follow-tags"),
            "{findings:?}"
        );
        assert!(evaluate_at(
            "git push --no-follow-tags origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        let direct = evaluate_at(
            "git push origin refs/tags/v1",
            &SecuritySection::default(),
            Some(temp.path()),
        );
        assert!(!direct[0].remedy.contains("--no-follow-tags"), "{direct:?}");
    }

    /// Round fifteen on issue 79: an alias with bytes that are not UTF-8 is
    /// still read for the tag it creates, not taken for no alias.
    #[test]
    fn an_alias_that_is_not_utf8_still_creates_its_tag() {
        use std::io::Write as _;
        let temp = tempfile::tempdir().unwrap();
        git2::Repository::init(temp.path()).unwrap();
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(temp.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(b"[alias]\n\tmark = tag --message=caf\xe9\n")
            .unwrap();
        assert!(!evaluate_at(
            "git mark v9 && git push origin v9",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    /// Round sixteen on issue 79: an alias that is not UTF-8 and pushes names
    /// a ref this text cannot give exactly, so it is judged as a release
    /// action; even a seemingly read-only value needs exact command text.
    #[test]
    fn an_alias_that_is_not_utf8_and_pushes_is_judged_as_a_release() {
        use std::io::Write as _;
        let temp = tempfile::tempdir().unwrap();
        git2::Repository::init(temp.path()).unwrap();
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(temp.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(b"[alias]\n\tpublish = push origin caf\xff\n\tlgx = log --format=caf\xe9\n")
            .unwrap();
        assert!(!evaluate_at(
            "git publish",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        assert!(!evaluate_at("git lgx", &SecuritySection::default(), Some(temp.path())).is_empty());
    }

    #[test]
    fn f1_tags_created_earlier_are_release_refspecs() {
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.config().unwrap().set_str("alias.mark", "tag").unwrap();
        for command in [
            "git tag v9 && git push origin v9",
            "git tag -a v9 -m x && git push origin +v9",
            "git mark v9 && git push origin v9",
            "git -c alias.mark=tag mark v9 && git push origin v9",
            "git tag v9 && bash -c 'git push origin v9'",
        ] {
            assert!(
                !evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate_at(
            "git tag v9 && git push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        let other = temp.path().join("other");
        git2::Repository::init(&other).unwrap();
        for command in [
            "git -C other tag v9 && git -C other push origin v9",
            "cd other && git tag v9 && git push origin v9",
        ] {
            assert!(
                !evaluate_at(command, &SecuritySection::default(), Some(temp.path())).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate_at(
            "git -C other tag v9 && git push origin v9",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    #[test]
    fn f2_shell_code_from_stdin_and_substitution() {
        for command in [
            "echo 'cargo publish' | sh",
            "printf 'npm publish' | bash",
            "echo 'cargo publish' | env sh -s arg",
            r#"bash -c "$(printf 'cargo publish')""#,
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "echo hello | sh -c 'cat'",
            "echo 'cargo publish' | cat",
            "echo 'cargo publish'; sh",
            "echo 'cargo publish' | sh script.sh",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn f3_follow_tags_configuration() {
        for command in [
            "git -c push.followTags=true push origin task/x",
            "git -c PUSH.FOLLOWTAGS=on push origin task/x",
            "git config push.followTags true",
            "git config --global push.followTags true",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in [
            "git -c push.followTags=false push origin task/x",
            "git config --get push.followTags",
            "git config push.followTags",
            "git config push.followTags false",
        ] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        let temp = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(temp.path()).unwrap();
        repo.config()
            .unwrap()
            .set_bool("push.followTags", true)
            .unwrap();
        assert!(!evaluate_at(
            "git push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
        assert!(evaluate_at(
            "git -c push.followTags=false push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    #[test]
    fn f5_interpreter_clusters() {
        for flag in [
            "perl -le",
            "perl -E",
            "perl -ne",
            "perl -pe",
            "ruby -ne",
            "node -pe",
            "python3 -Ic",
        ] {
            let command = format!("{flag} 'run(\"cargo publish\")'");
            assert!(
                !evaluate(&command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        assert!(evaluate("perl -ne 'print' file", &SecuritySection::default()).is_empty());
    }

    #[test]
    fn f6_crontab_stdin_and_read_controls() {
        for command in ["crontab", "echo '* * * * * x' | crontab"] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
        for command in ["crontab -l", "crontab -h", "crontab --help"] {
            assert!(
                evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    const WRAPPERS: &[&str] = &[
        "{}",
        "env -i {}",
        "env -C /tmp {}",
        "env -S '{}'",
        "command {}",
        "nohup {}",
        "timeout 2 {}",
        "xargs -n 1 {}",
        "find . -exec {} \";\"",
        "npx --yes {}",
        "bash -lc '{}'",
        "python3 -c 'run(\"{}\")'",
        "node -e 'run(\"{}\")'",
        "perl -e 'run(\"{}\")'",
        "ruby -e 'run(\"{}\")'",
    ];

    #[test]
    fn table_families_are_refused_through_each_wrapper() {
        for family in &table().families {
            for rule in &family.codex {
                let mut forms = vec![Vec::<String>::new()];
                for token in &rule.pattern {
                    let alternatives = match token {
                        PatternToken::Word(word) => vec![word.clone()],
                        PatternToken::AnyOf(words) => words.clone(),
                    };
                    forms = forms
                        .into_iter()
                        .flat_map(|prefix| {
                            alternatives.iter().map(move |word| {
                                let mut next = prefix.clone();
                                next.push(word.clone());
                                next
                            })
                        })
                        .collect();
                }
                for form in forms {
                    let direct = form.join(" ");
                    for wrapper in WRAPPERS {
                        let command = wrapper.replace("{}", &direct);
                        let found = evaluate(&command, &SecuritySection::default());
                        assert!(
                            found.iter().any(|v| v.message.contains(&family.id)),
                            "{}: {command}: {found:?}",
                            family.id
                        );
                        assert!(
                            found.iter().all(|v| v.remedy.contains("operator")),
                            "{command}"
                        );
                        for level in [
                            crate::hooks::PolicyLevel::Warn,
                            crate::hooks::PolicyLevel::Off,
                        ] {
                            let levels = SecuritySection {
                                privilege_escalation: level,
                                outward_actions: level,
                                secret_reads: level,
                                ..SecuritySection::default()
                            };
                            let findings = evaluate(&command, &levels);
                            if level.is_active() {
                                assert!(!findings.is_empty(), "{command}");
                                assert!(findings.iter().all(|v| v.level == level), "{command}");
                            } else {
                                assert!(findings.is_empty(), "{command}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bare_tags_are_resolved_in_the_command_repository() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("other");
        let repo = git2::Repository::init(&repo_dir).unwrap();
        let id = repo.blob(b"tag target").unwrap();
        let object = repo.find_object(id, None).unwrap();
        repo.tag_lightweight("release-candidate", &object, false)
            .unwrap();
        for command in [
            "git -C other push origin release-candidate",
            "cd other && git push origin release-candidate",
            "env -C other git push origin release-candidate",
        ] {
            let findings = evaluate_at(command, &SecuritySection::default(), Some(temp.path()));
            assert!(
                findings.iter().any(|v| v.message.contains("release")),
                "{command}: {findings:?}"
            );
        }
        assert!(evaluate_at(
            "git -C other push origin task/x",
            &SecuritySection::default(),
            Some(temp.path())
        )
        .is_empty());
    }

    #[test]
    fn leading_flags_and_non_prefix_actions_are_refused() {
        for command in [
            "cargo +stable publish",
            "cargo --manifest-path Cargo.toml publish",
            "npm --ignore-scripts publish",
            "gh -R o/r release create v1",
            "git push origin refs/tags/v1",
            "git push origin --follow-tags",
            "git push origin --mirror",
            "gh repo edit --visibility public",
            "gh api repos/o/r -X DELETE",
            "crontab jobs.txt",
            "osascript -e 'do shell script x with administrator privileges'",
        ] {
            assert!(
                !evaluate(command, &SecuritySection::default()).is_empty(),
                "{command}"
            );
        }
    }

    #[test]
    fn ordinary_actions_pass_direct_and_wrapped() {
        for command in [
            "cargo test",
            "git push origin task/x",
            "gh release view v1",
            "cargo package",
            "grep -rn sudo docs/",
            "cargo +nightly test --features publish",
            "gh auth status",
            "defaults read domain",
            "crontab -l",
        ] {
            for wrapper in &WRAPPERS[..11] {
                let command = wrapper.replace("{}", command);
                assert!(
                    evaluate(&command, &SecuritySection::default()).is_empty(),
                    "{command}"
                );
            }
        }
    }
}

#[cfg(test)]
mod r22_tests {
    use super::*;
    #[test]
    fn r22_outward_readonly_builtin_does_not_require_target_policy() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("lib");
        git2::Repository::init(&target).unwrap();
        std::fs::remove_file(target.join(".git/HEAD")).unwrap();
        let levels = SecuritySection::default();
        for command in ["git -C lib status", "git --git-dir=lib/.git status"] {
            let violations = evaluate_at(command, &levels, Some(dir.path()));
            assert!(
                violations.is_empty(),
                "read-only builtin {command}: {violations:?}"
            );
        }
    }
    #[test]
    fn r22_outward_refuses_unreadable_retarget_with_security_off() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("lib");
        let repo = git2::Repository::init(&target).unwrap();
        repo.config()
            .unwrap()
            .set_str("alias.unknown", "!git push origin v2.0")
            .unwrap();
        let levels = SecuritySection {
            outward_actions: crate::hooks::PolicyLevel::Off,
            ..SecuritySection::default()
        };
        assert!(
            evaluate_at("git -C lib push origin feature", &levels, Some(dir.path())).is_empty()
        );
        std::fs::remove_file(target.join(".git/HEAD")).unwrap();
        for command in [
            "git -C lib push origin v2.0",
            "git -C lib tag v2.0",
            "git -C lib unknown",
            "git -C lib missing-alias",
        ] {
            let violations = evaluate_at(command, &levels, Some(dir.path()));
            assert!(
                violations.iter().any(|v| v.rule == "git.policy_authority"
                    && v.level == crate::hooks::PolicyLevel::Block),
                "{command}: {violations:?}"
            );
        }
    }
    #[test]
    fn r22_outward_refuses_unreadable_tag_and_follow_tags() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        for command in [
            "git push origin v2.0",
            "git push https://example.invalid/repo.git HEAD~1:refs/heads/main",
        ] {
            assert!(
                evaluate_at(command, &SecuritySection::default(), Some(dir.path())).is_empty(),
                "{command}"
            );
        }
        std::fs::write(repo.path().join("refs/tags/v2.0"), "invalid\n").unwrap();
        assert!(!evaluate_at(
            "git push origin v2.0",
            &SecuritySection::default(),
            Some(dir.path())
        )
        .is_empty());
        std::fs::remove_file(repo.path().join("refs/tags/v2.0")).unwrap();
        repo.config()
            .unwrap()
            .set_str("push.followTags", "invalid-bool")
            .unwrap();
        assert!(!evaluate_at(
            "git push origin main",
            &SecuritySection::default(),
            Some(dir.path())
        )
        .is_empty());
    }
    #[test]
    fn r22_outward_absent_repository_is_not_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        assert!(evaluate_at(
            "git push origin main",
            &SecuritySection::default(),
            Some(dir.path())
        )
        .is_empty());
    }

    /// Round 2 item 7: a differential test over directory-changing
    /// prefixes. For each prefix, `<prefix>; pwd` is run natively in a
    /// disposable fixture, and the directory the shell reports must be in
    /// `line_directories`'s set, or the set must be unknown. Nothing
    /// touches an enforcement file. It runs under bash, and under zsh where
    /// that shell is installed; it is skipped, with a printed note, on a
    /// host that has neither.
    #[test]
    #[allow(clippy::too_many_lines)] // One corpus, read under each shell.
    fn directory_prefixes_match_the_native_shell() {
        use std::process::Command;
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        for dir in ["sub", "a/b", ".claude", ".codeflow", "with space"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        // Every reviewer payload's prefix, plus ordinary and unknown ones.
        let prefixes = [
            "cd sub",
            "cd -- sub",
            "cd -P -L sub",
            "cd -q sub",
            "command cd sub",
            "command -p cd sub",
            "builtin cd sub",
            "! cd sub",
            "pushd sub >/dev/null",
            "chdir sub",
            "env -C sub true",
            "env -Csub true",
            "env --chdir=sub true",
            "cd a && cd b",
            "cd a/b",
            "cd .claude && cd ..",
            "cd 'with space'",
            "cd sub && cd ..",
            "cd",
            "cd \"$UNSET\"",
            "cd -",
            // Round 4: zsh precommands and `builtin --`.
            "builtin -- cd sub",
            "noglob cd sub",
            "nocorrect cd sub",
            "true; - cd sub",
            "noglob cd .claude && cd ..",
        ];
        // Words that only zsh reads as precommands.
        let zsh_only = ["noglob", "nocorrect", "true; -"];
        let shells: Vec<&str> = ["bash", "zsh"]
            .into_iter()
            .filter(|shell| {
                Command::new(shell)
                    .arg("-c")
                    .arg("exit 0")
                    .status()
                    .is_ok_and(|status| status.success())
            })
            .collect();
        if shells.is_empty() {
            eprintln!("skipped: neither bash nor zsh is installed");
            return;
        }
        // Launcher lines whose child reports its own directory (Codex R3-1):
        // the line itself prints, so the directory compared is the child's.
        let children = [
            "env -C sub /bin/pwd -P",
            "env -Csub /bin/pwd -P",
            "env --chdir=sub /bin/pwd -P",
            "env -u FOO -C sub /bin/pwd -P",
            "env -u FOO -Csub /bin/pwd -P",
            "env -iCsub /bin/pwd -P",
            "nice env -C sub /bin/pwd -P",
            "nice -n 5 env -Csub /bin/pwd -P",
            "nohup env -C sub /bin/pwd -P",
            "command env -C sub /bin/pwd -P",
            "time -p env -C sub /bin/pwd -P",
            "stdbuf -oL env -C sub /bin/pwd -P",
            "timeout 5 env -C sub /bin/pwd -P",
            "command cd sub && /bin/pwd -P",
            "cd .claude && command cd .. && /bin/pwd -P",
            // Round 4: launchers read by name, installed or not.
            "caffeinate env -Csub /bin/pwd -P",
            "caffeinate env -C sub /bin/pwd -P",
            "caffeinate -i env -Csub /bin/pwd -P",
            "xcrun env -Csub /bin/pwd -P",
            "genv -Csub /bin/pwd -P",
            "gtimeout 5 env -Csub /bin/pwd -P",
            "ionice -c 3 env -Csub /bin/pwd -P",
            "noglob env -Csub /bin/pwd -P",
            "builtin -- cd sub && /bin/pwd -P",
        ];
        let mut skipped = Vec::new();
        for shell in shells {
            let runs = prefixes
                .iter()
                .map(|prefix| (*prefix, format!("{prefix} 2>/dev/null; pwd -P")))
                .chain(
                    children
                        .iter()
                        .map(|line| (*line, format!("{line} 2>/dev/null"))),
                );
            for (line, script) in runs {
                // `chdir` is a zsh builtin, not a bash one.
                if line.starts_with("chdir") && shell == "bash" {
                    continue;
                }
                if shell == "bash" && zsh_only.iter().any(|word| line.starts_with(word)) {
                    skipped.push(format!("{shell}: {line} (zsh only)"));
                    continue;
                }
                let set = line_directories(line, &root, Some(&home));
                let output = Command::new(shell)
                    .arg("-c")
                    .arg(&script)
                    .current_dir(&root)
                    .env("HOME", &home)
                    .env_remove("CDPATH")
                    .env_remove("UNSET")
                    .output()
                    .unwrap();
                let printed = String::from_utf8(output.stdout).unwrap();
                let Some(reported) = printed.lines().next_back() else {
                    // The launcher is not installed here (`timeout`,
                    // `stdbuf` on macOS): nothing ran, so nothing to compare.
                    skipped.push(format!("{shell}: {line}"));
                    continue;
                };
                let reported = std::fs::canonicalize(reported).unwrap_or_else(|_| reported.into());
                let covered = set.unknown
                    || set.dirs.iter().any(|dir| {
                        std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone()) == reported
                    });
                assert!(
                    covered,
                    "{shell}: `{line}` ran in {reported:?}, not in {:?} (unknown={})",
                    set.dirs, set.unknown
                );
            }
        }
        if !skipped.is_empty() {
            eprintln!("skipped, launcher not installed or not this shell's: {skipped:?}");
        }
    }
}
