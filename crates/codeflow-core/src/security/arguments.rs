//! The paths an interpreter is given as arguments (TSK-302, issue 173).
//!
//! exec-guard judges an interpreter's arguments as it judges the words of
//! its inline code: one that names policy authority metadata always
//! refuses, and one that names an enforcement path refuses under
//! `git.hook_integrity`. An argument the guard cannot read, filled in at
//! run time or used from a directory it cannot follow, takes git-guard's
//! rule for writer operands: it refuses when its spelled part could name a
//! protected path or when the line names one. The arguments `xargs` and
//! `find -exec` supply are judged the same way, `find`'s by the protected
//! files its starting points reach. Authority metadata refuses in every
//! one of these forms, whatever the integrity level. A program that builds
//! a path internally is past this reading; the OS sandbox is the backstop.
//!
//! The governing rule for directories (TSK-302 review round 3): within the
//! modeled grammar the directory set is exact; anything outside it makes
//! the set unknown, and an unknown set judges every known word, argument
//! or code string literal, by its trailing components, at the same levels.
//! The guard never fails open on grammar it does not understand. The
//! modeled grammar is `cd`, `chdir`, `pushd` and `popd`; the launchers
//! `command`, `builtin` (with `--`), `exec`, `nohup`, `nice`, `timeout`,
//! `stdbuf`, `time`, `env`, `xargs`, `caffeinate`, `xcrun` and `ionice`
//! with their options, read under their GNU-prefixed names too (`genv`,
//! `gtimeout`); the zsh precommands `noglob`, `nocorrect` and `-`;
//! `git -C`; composed `ruby -C`; and `find -execdir` started from every
//! directory in the set. `source`, `.`, `sudo`, `env -S` and any option
//! outside that list make the set unknown. Any other word followed by a
//! directory-change token the grammar did not consume (`cd`, `chdir`,
//! `pushd`, `popd`, or `-C` or `--chdir` after an env-like word) is read
//! as an unmodeled launcher, and the set becomes unknown (review round 4).
//! An unmodeled program followed, in a command position, by a modeled
//! launcher or an interpreter name may launch it: the rest of the segment
//! is read as a call whose directory is unknown. A bare `--` does not end
//! that walk (`taskpolicy -- python3`). A script string such a program is
//! given, the value of `-c` or `-e` before a `--` or any quoted word, is
//! read as a command line with an unknown directory (`expect -c`,
//! `osascript -e`, `ssh host '...'`); a single-word `-m` value is a
//! message. `echo` and `printf` only print, so their operands stay data.
//! When a shell or interpreter takes its code from anything other than a
//! literal argument or a heredoc body the guard reads (a pipe, a
//! here-string, a descriptor such as `/dev/stdin` or `/dev/fd/N`, a
//! process substitution, or a `-c`/`-e` value holding a substitution),
//! every word on the line is judged by its trailing components, with
//! authority at its fixed level; code the guard can see there is also
//! judged as that program's code (review round 7). On a line where a shell
//! (`source` and `.` included) or an interpreter runs, every quoted string
//! anywhere on the line is judged as possible code the same way; unquoted
//! operands of other programs stay data (review round 8). Quoted strings
//! are read as the shell builds them: `$'...'` decoded, adjacent quoted
//! pieces joined into one word, and a `printf` of `%s` pieces read as the
//! value it joins (review round 9). A text any construct composes from
//! pieces, a `printf` escape or `%b`/`%q`, an `echo -e` or `print` decode,
//! or a join of on-line assignments, is resolved exactly where the guard
//! can and judged by the tail rule, and on a running line a composing
//! construct it cannot resolve refuses as cannot-verify, naming it (review
//! round 10). That resolution is closed-world (`composed`, review round
//! 11): a value is resolved only when every input is a literal read
//! exactly through a construct the guard models, and the value equals what
//! bash and zsh produce, checked against both shells by a differential
//! test; any other construct feeding it is unresolved, named by its
//! family, never read by its spelling. The output of a program other than
//! the modelled builtins is outside text, as an unassigned variable is.
//! A part filled in at run time may be empty, so a word holding one is
//! also judged with it removed, the spelled pieces joined (review round
//! 15). A word is also
//! judged with one surrounding `{}` pair removed, as Tcl runs it. Code in a
//! file the guard does not read stays past the guard; the OS sandbox is the
//! backstop. The guard stops at shell grammar: a path the interpreter's own
//! code builds at run time, such as Python adjacent string literals or a
//! node `+` concatenation, is the same residual (operator decision,
//! 2026-10-15).
//! Only string literals in code are read as file names, never identifiers
//! or numbers.
use super::{actions, composed, outward};
use crate::hooks::{edit_guard, git_guard, PolicyLevel, Violation};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// The sanctioned route of an authority refusal, as the code check gives it.
const AUTHORITY_ROUTE: &str =
    "the operator manages remote-tracking authority; use the configured remote's ordinary fetch";

/// What an argument reaches: authority metadata, which always refuses, or
/// an enforcement path, which refuses at the integrity level.
enum Reach {
    Authority(String),
    Enforcement(String),
}

pub(crate) fn evaluate(
    line: &str,
    integrity: PolicyLevel,
    payload_cwd: &Path,
    root: &Path,
    home: Option<&Path>,
) -> Vec<Violation> {
    let _scope = edit_guard::RepoFactsScope::enter();
    let calls = outward::interpreter_calls(line, payload_cwd);
    let (opaque, runs) = outward::code_sources(line);
    if calls.is_empty() && !runs && !opaque && outward::interpreter_bodies(line).is_empty() {
        return Vec::new();
    }
    let line_dirs = outward::line_directories(line, payload_cwd, home);
    let mut facts = Facts {
        line,
        payload_cwd,
        root,
        home,
        names: ProtectedNames::load(root, home),
        run_names: HashMap::new(),
        line_enforcement: HashMap::new(),
        line_authority: HashMap::new(),
    };
    let mut reaches = Vec::new();
    for call in &calls {
        reaches.extend(call_reaches(call, &line_dirs, &mut facts));
    }
    // Code text is judged as its arguments are, from every directory the
    // line may run it in, bare names included (TSK-302 review round 2); an
    // unresolved code word is not a shell operand, so it does not take the
    // line's writer rule.
    let code_dirs = dir_options(&line_dirs);
    for code in outward::interpreter_bodies(line) {
        for word in code_literal_words(&code) {
            for dir in &code_dirs {
                reaches.extend(code_reach(&word, dir.as_deref(), &mut facts));
            }
        }
    }
    // A shell or interpreter whose code the guard cannot read in full makes
    // every word on the line a possible part of that code, judged by its
    // trailing components with authority at its fixed level (TSK-302
    // review round 7).
    if opaque {
        for word in outward::literal_words(line) {
            reaches.extend(opaque_line_word(&word, OPAQUE));
        }
    }
    // Where a shell or interpreter runs, every quoted string on the line
    // may reach it as code (a here-string with any descriptor, a pipe
    // producer, a process substitution, an `exec N<<<` body, a fifo
    // writer, a literal `-c` that runs its stdin), so each is judged the
    // same way (TSK-302 review round 8). Unquoted operands stay data.
    if runs {
        // A text some construct composes from pieces seen apart: a resolved
        // value (a `printf`, an `echo -e`/`print` operand, a join of on-line
        // assignments) is judged by the tail rule; one the guard cannot
        // resolve refuses as cannot-verify, naming it (review rounds 9, 10).
        for composed in composed::composed_texts(line) {
            match composed {
                composed::Composed::Resolved(value) => {
                    // As written, and with each unknown piece empty, which
                    // joins the spelled pieces around it (review rounds 15
                    // and 16): the value knows where its pieces are.
                    for reading in [value.clone(), value.without_open()] {
                        for word in outward::literal_words(&reading.render()) {
                            reaches.extend(composed_word(&word));
                        }
                        // A glob is judged whole, by what its pattern can name.
                        for word in reading.glob_words() {
                            reaches.extend(composed_word(&word));
                        }
                    }
                }
                composed::Composed::Unresolved(construct) => {
                    reaches.push(Reach::Enforcement(format!(
                        "a path is composed with {construct} on a line where a shell or interpreter runs"
                    )));
                }
            }
        }
        for text in outward::quoted_texts(line) {
            for word in outward::literal_words(&text) {
                reaches.extend(opaque_line_word(&word, QUOTED));
            }
        }
    }
    let mut found = Vec::new();
    for reach in reaches {
        let violation = match reach {
            Reach::Authority(message) => Some(Violation::always_blocking(
                "git.hook_integrity",
                message,
                AUTHORITY_ROUTE,
            )),
            Reach::Enforcement(message) => integrity.is_active().then(|| {
                Violation::new(
                    "git.hook_integrity",
                    integrity,
                    message,
                    crate::remedy::INTERPRETER_ARGUMENT.remedy(),
                )
            }),
        };
        found.extend(violation);
    }
    found
}

/// What one evaluation reads once and reuses for every call on the line
/// (TSK-302 review F4): the protected names, and what the whole line names
/// from each directory a call runs in.
struct Facts<'a> {
    line: &'a str,
    payload_cwd: &'a Path,
    root: &'a Path,
    home: Option<&'a Path>,
    names: ProtectedNames,
    run_names: HashMap<PathBuf, ProtectedNames>,
    line_enforcement: HashMap<Option<PathBuf>, Option<String>>,
    line_authority: HashMap<Option<PathBuf>, Option<String>>,
}

impl Facts<'_> {
    /// The enforcement path the line names from `dir`, read once per
    /// directory; a word whose path passes through no protected name is
    /// not resolved.
    fn line_enforcement(&mut self, dir: Option<&Path>) -> Option<String> {
        let key = dir.map(Path::to_path_buf);
        if let Some(found) = self.line_enforcement.get(&key) {
            return found.clone();
        }
        let from = dir.unwrap_or(self.payload_cwd);
        let names = &self.names;
        let found = git_guard::line_names_where(
            self.line,
            from,
            self.payload_cwd,
            &|path| names.may_reach(path),
            true,
        );
        self.line_enforcement.insert(key, found.clone());
        found
    }

    /// The word of the line that names policy authority metadata from
    /// `dir`, read once per directory.
    fn line_authority(&mut self, dir: Option<&Path>) -> Option<String> {
        let key = dir.map(Path::to_path_buf);
        if let Some(found) = self.line_authority.get(&key) {
            return found.clone();
        }
        let from = dir.unwrap_or(self.payload_cwd);
        let found = git_guard::line_path_readings(self.line)
            .into_iter()
            .find(|(word, pattern)| {
                // A glob the guard cannot list is left to the enforcement
                // reading of the line, which refuses it near protected paths.
                literal_paths(word, *pattern, from, self.payload_cwd, self.home)
                    .is_ok_and(|paths| paths.iter().any(|path| self.authority(path)))
            })
            .map(|(word, _)| git_guard::shown_word(&word));
        self.line_authority.insert(key, found.clone());
        found
    }

    /// Whether `path` is policy authority metadata.
    fn authority(&self, path: &Path) -> bool {
        self.names.may_reach(path)
            && edit_guard::repository_authority_target(path, self.root, false)
    }
}

/// The paths a word names from `dir` once the shell expands its braces and
/// globs, preserving the reason when the guard cannot list them.
fn literal_paths(
    word: &str,
    pattern: bool,
    dir: &Path,
    payload_cwd: &Path,
    home: Option<&Path>,
) -> Result<Vec<PathBuf>, &'static str> {
    // Quote removal happens in the argv parser. Characters left in the
    // value cannot tell us whether the shell saw a pattern or literal text.
    if !pattern {
        return Ok(vec![literal_target(word, dir, home)]);
    }
    let mut paths = Vec::new();
    for reading in git_guard::word_readings(word).ok_or("cannot inspect every brace alternative")? {
        if reading.is_empty() {
            continue;
        }
        if git_guard::has_glob(&reading) {
            paths.extend(git_guard::glob_paths(&reading, dir, payload_cwd)?);
        } else {
            paths.push(literal_target(&reading, dir, home));
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// The words of one interpreter argument that can name a file: the word
/// itself, or for an option the value written after `=` and a short
/// option's attached value (`-o.claude/settings.json`). Stdin's `-` and
/// `--` name none.
fn argument_values(word: &str) -> Vec<&str> {
    if word.is_empty() || word == "-" || word == "--" {
        return Vec::new();
    }
    if !word.starts_with('-') {
        return vec![word];
    }
    let mut values: Vec<&str> = word
        .split_once('=')
        .map(|(_, value)| value)
        .into_iter()
        .collect();
    if !word.starts_with("--") {
        // A short-option cluster may end in an option whose value is
        // attached: `-oFILE`, `-voFILE` (TSK-302 review round 2).
        for (at, ch) in word.char_indices().skip(2) {
            values.push(&word[at..]);
            if !word[1..at].chars().all(|c| c.is_ascii_alphabetic()) || !ch.is_ascii_alphabetic() {
                break;
            }
        }
    }
    values.retain(|value| !value.is_empty());
    values
}

/// The directories of a set as the judgment reads them, `None` standing
/// for a directory the guard cannot follow.
fn dir_options(set: &outward::DirSet) -> Vec<Option<PathBuf>> {
    let mut dirs: Vec<Option<PathBuf>> = set.dirs.iter().cloned().map(Some).collect();
    if set.unknown {
        dirs.push(None);
    }
    dirs
}

/// The directories a call's relative arguments are read from: every one
/// the line may run it in (and the one its own launcher gives it), or for
/// `find -execdir` each one `find` can run it in; then each `ruby -C`
/// applied in order.
fn call_dirs(
    call: &outward::InterpreterCall,
    line_dirs: &outward::DirSet,
    facts: &Facts<'_>,
) -> outward::DirSet {
    let mut set = if let Some(outward::Supplier::Find {
        starts,
        before,
        cwd,
        in_dir: true,
    }) = &call.supplier
    {
        execdir_dirs(starts, before, cwd.as_deref(), line_dirs, facts.payload_cwd)
    } else {
        let mut set = line_dirs.clone();
        match &call.cwd {
            Some(cwd) if !set.dirs.contains(cwd) => set.dirs.push(cwd.clone()),
            Some(_) => {}
            None => set.unknown = true,
        }
        set
    };
    for chdir in &call.chdir {
        match outward::directory_operand(chdir, facts.home) {
            outward::DirChange::Unknown => set.unknown = true,
            outward::DirChange::To(target) => {
                set.dirs = set
                    .dirs
                    .iter()
                    .map(|dir| outward::lexical_join(dir, &target))
                    .collect();
            }
        }
    }
    set
}

/// A `find` test the `-execdir` walk applies: a name or path pattern, or an
/// entry type.
enum FindTest {
    Name(glob::Pattern, bool),
    Path(glob::Pattern, bool),
    Type(char),
}

/// The tests of the expression before an `-execdir` that every matched
/// entry passes, or `None` when the expression has an operator (`-o`, `!`,
/// `-not`, a group) under which they need not all hold. Any other test is
/// read as one every entry passes.
fn find_tests(before: &[String]) -> Option<Vec<FindTest>> {
    // Primaries whose value is the next word.
    const VALUED: &[&str] = &[
        "-maxdepth",
        "-mindepth",
        "-newer",
        "-anewer",
        "-cnewer",
        "-user",
        "-group",
        "-uid",
        "-gid",
        "-perm",
        "-size",
        "-links",
        "-inum",
        "-mtime",
        "-mmin",
        "-atime",
        "-amin",
        "-ctime",
        "-cmin",
        "-printf",
        "-fprint",
        "-fprint0",
        "-fls",
        "-regex",
        "-iregex",
        "-lname",
        "-ilname",
        "-xtype",
        "-fstype",
        "-samefile",
        "-newermt",
        "-files0-from",
        "-context",
    ];
    let mut tests = Vec::new();
    let mut words = before
        .iter()
        .skip_while(|w| matches!(w.as_str(), "-H" | "-L" | "-P"));
    // The starting points come first.
    let mut words = words
        .by_ref()
        .skip_while(|w| !w.starts_with('-') && !matches!(w.as_str(), "!" | "("));
    while let Some(word) = words.next() {
        let pattern = |value: Option<&String>| value.and_then(|v| glob::Pattern::new(v).ok());
        match word.as_str() {
            "-o" | "-or" | "!" | "-not" | "(" | ")" | "," => return None,
            "-name" | "-iname" => {
                tests.push(FindTest::Name(pattern(words.next())?, word == "-iname"));
            }
            "-path" | "-ipath" | "-wholename" | "-iwholename" => {
                tests.push(FindTest::Path(
                    pattern(words.next())?,
                    word.starts_with("-i"),
                ));
            }
            "-type" => {
                let kind = words.next()?;
                if let [kind] = kind.as_bytes() {
                    tests.push(FindTest::Type(char::from(*kind)));
                }
            }
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                for word in words.by_ref() {
                    if word == ";" || word == "+" {
                        break;
                    }
                }
            }
            other if VALUED.contains(&other) => {
                words.next();
            }
            _ => {}
        }
    }
    Some(tests)
}

/// Whether the entry at `path`, which `find` prints as `shown`, passes
/// every test.
fn passes(tests: &[FindTest], path: &Path, shown: &str, kind: Option<std::fs::FileType>) -> bool {
    let options = |insensitive: bool| glob::MatchOptions {
        case_sensitive: !insensitive,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    tests.iter().all(|test| match test {
        FindTest::Name(pattern, insensitive) => path
            .file_name()
            .and_then(|name| name.to_str())
            .is_none_or(|name| pattern.matches_with(name, options(*insensitive))),
        FindTest::Path(pattern, insensitive) => pattern.matches_with(shown, options(*insensitive)),
        FindTest::Type(want) => kind.is_none_or(|kind| match want {
            'f' => kind.is_file(),
            'd' => kind.is_dir(),
            'l' => kind.is_symlink(),
            _ => true,
        }),
    })
}

/// Every directory `find -execdir` can run its command in (TSK-302 review
/// round 3): the run directories of the line itself, and for each start,
/// read from every directory in the line's set, the parent of each entry
/// the expression's tests let through among the protected paths under it,
/// the start itself included (a file start runs in its parent). `-path`
/// tests see find's own spelling (`./.claude/settings.json`). A start the
/// guard cannot read, an expression it cannot apply, or a walk past its
/// limit reads as unknown.
fn execdir_dirs(
    starts: &[String],
    before: &[String],
    cwd: Option<&Path>,
    line_dirs: &outward::DirSet,
    payload_cwd: &Path,
) -> outward::DirSet {
    let mut set = line_dirs.clone();
    if let Some(cwd) = cwd {
        if !set.dirs.contains(&cwd.to_path_buf()) {
            set.dirs.push(cwd.to_path_buf());
        }
    }
    let froms = set.dirs.clone();
    let tests = find_tests(before);
    for start in starts {
        if git_guard::unresolved_word(start) {
            set.unknown = true;
            continue;
        }
        let spelled = start.trim_end_matches('/');
        let spelled = if spelled.is_empty() { "/" } else { spelled };
        for from in &froms {
            let begin = from.join(start);
            let Ok(base) = edit_guard::normalized(&begin, true) else {
                set.unknown = true;
                continue;
            };
            // `find_candidates` is bounded; it returns the enforcement paths
            // and registered checkouts under the start and the directories
            // leading to them, never a full tree walk (TSK-302 review round
            // 2 R2-4, Grok F3).
            let Ok(mut candidates) = edit_guard::find_candidates(&begin, payload_cwd) else {
                set.unknown = true;
                continue;
            };
            if !candidates.contains(&base) {
                candidates.push(base.clone());
            }
            for candidate in candidates {
                // A candidate too large to walk is reported as `<dir>/*`;
                // the command may run anywhere under it.
                if candidate.file_name().is_some_and(|name| name == "*") {
                    set.unknown = true;
                    continue;
                }
                let shown = match candidate.strip_prefix(&base) {
                    Ok(rest) if rest.as_os_str().is_empty() => spelled.to_string(),
                    Ok(rest) => {
                        let Some(rest) = rest.to_str() else {
                            set.unknown = true;
                            continue;
                        };
                        format!("{}/{rest}", spelled.trim_end_matches('/'))
                    }
                    Err(_) => spelled.to_string(),
                };
                let kind = std::fs::symlink_metadata(&candidate)
                    .ok()
                    .map(|m| m.file_type());
                // An entry `find` matches runs the command in its parent.
                if tests
                    .as_deref()
                    .is_none_or(|tests| passes(tests, &candidate, &shown, kind))
                {
                    if let Some(parent) = candidate.parent() {
                        set.dirs.push(parent.to_path_buf());
                    }
                }
                // A directory runs it when an entry directly in it matches.
                if candidate.is_dir() && dir_runs_execdir(&candidate, &shown, tests.as_deref()) {
                    set.dirs.push(candidate);
                }
            }
        }
    }
    dedup(set)
}

/// Whether `find -execdir` can run its command in directory `dir`, which
/// find spells `shown`: it does when an entry directly in `dir` passes the
/// expression's tests. With no resolvable test every entry passes, so any
/// directory qualifies. A directory it cannot read is taken to qualify
/// (fail closed).
fn dir_runs_execdir(dir: &Path, shown: &str, tests: Option<&[FindTest]>) -> bool {
    let Some(tests) = tests else {
        return true;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // A name that is not UTF-8 cannot be shown to a glob, so the entry
        // is taken to match (the directory stays a run location).
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return true;
        };
        let child = format!("{}/{name}", shown.trim_end_matches('/'));
        if passes(tests, &path, &child, entry.file_type().ok()) {
            return true;
        }
    }
    false
}

fn dedup(mut set: outward::DirSet) -> outward::DirSet {
    set.dirs.sort();
    set.dirs.dedup();
    set
}

/// What every argument of one call, and what its supplier gives it, reach.
/// The reach of one word over every directory it may run in: the first
/// authority reach if any (always blocking, so order never hides it),
/// else the first enforcement reach. Stops reading directories at the
/// first authority hit, so a bare name over many protected directories
/// costs one full check, not one per directory (TSK-302 review round 2).
fn strongest_over_dirs(
    value: &str,
    dirs: &[Option<PathBuf>],
    mut reach: impl FnMut(Option<&Path>) -> Option<Reach>,
) -> Option<Reach> {
    // Authority is only reachable when the word could name git metadata by
    // its tail, or is filled in at run time; otherwise the first reach is
    // the answer, so a bare name over many protected directories costs one
    // check rather than one per directory (TSK-302 review round 2 F4).
    let authority_possible = git_guard::unresolved_word(value)
        || value.contains(".git")
        || tail_names_authority(value).is_some();
    let mut enforcement = None;
    for dir in dirs {
        match reach(dir.as_deref()) {
            Some(Reach::Authority(message)) => return Some(Reach::Authority(message)),
            Some(Reach::Enforcement(message)) => {
                enforcement.get_or_insert(Reach::Enforcement(message));
                if !authority_possible {
                    return enforcement;
                }
            }
            None => {}
        }
    }
    enforcement
}

fn call_reaches(
    call: &outward::InterpreterCall,
    line_dirs: &outward::DirSet,
    facts: &mut Facts<'_>,
) -> Vec<Reach> {
    let dirs = dir_options(&call_dirs(call, line_dirs, facts));
    let mut found = Vec::new();
    for (word, pattern) in call.args.iter().zip(&call.patterns) {
        // `find` and `xargs -I {}` fill in `{}`; what they supply is judged
        // below.
        if call.supplier.is_some() && word.contains("{}") {
            continue;
        }
        for value in argument_values(word) {
            // A descriptor names what the line's shell has open when the
            // interpreter runs, never a file the hook can inspect: reading
            // it would read the hook's own descriptor. The redirection that
            // opens it is the writer rule's to judge.
            if outward::descriptor_program(value) {
                continue;
            }
            // One finding per value is enough; its strongest is kept on
            // merge, and a fixed authority finding is never hidden because
            // every authority reach is judged before any enforcement one.
            found.extend(strongest_over_dirs(value, &dirs, |dir| {
                value_reach(value, *pattern, dir, facts)
            }));
            // A Tcl or brace-quoted word loses one `{}` pair before it runs
            // (`expect -c 'spawn ... {.git/config}'`, review round 7 F2).
            if let Some(inner) = value
                .strip_prefix('{')
                .and_then(|inner| inner.strip_suffix('}'))
                .filter(|inner| !inner.is_empty())
            {
                found.extend(strongest_over_dirs(inner, &dirs, |dir| {
                    value_reach(inner, *pattern, dir, facts)
                }));
            }
        }
    }
    // The call's own code is judged from the same directories.
    for code in &call.code {
        for word in code_literal_words(code) {
            found.extend(strongest_over_dirs(&word, &dirs, |dir| {
                code_reach(&word, dir, facts)
            }));
        }
    }
    found.extend(supplied_reach(call, facts));
    found
}

/// The file a literal argument names from `dir`.
fn literal_target(value: &str, dir: &Path, home: Option<&Path>) -> PathBuf {
    let dir = crate::portable_path::without_verbatim(dir.to_path_buf());
    match (
        value.strip_prefix("~/").or((value == "~").then_some("")),
        home,
    ) {
        (Some(rest), Some(home)) => {
            crate::portable_path::without_verbatim(home.to_path_buf()).join(rest)
        }
        _ => dir.join(crate::portable_path::without_verbatim(PathBuf::from(value))),
    }
}

/// Whether a reached-path description from git-guard names authority
/// metadata rather than an enforcement file.
fn names_authority(reached: &str) -> bool {
    reached == edit_guard::AUTHORITY_PATH
        || reached == edit_guard::GIT_CONFIG_AUTHORITY
        || [
            ".git/config",
            "packed-refs",
            "refs/remotes",
            "config.worktree",
            "gitconfig",
        ]
        .iter()
        .any(|name| reached.contains(name))
}

fn value_reach(
    value: &str,
    pattern: bool,
    dir: Option<&Path>,
    facts: &mut Facts<'_>,
) -> Option<Reach> {
    value_reach_at(value, pattern, dir, false, facts)
}

/// The words of the string literals in interpreter code: the text between
/// single, double or back quotes, escapes read past, split into path-like
/// words. Identifiers, keywords and numbers outside a literal are never
/// read as file names (TSK-302 review round 3, R3-4).
fn code_literal_words(code: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = code.chars();
    while let Some(ch) = chars.next() {
        if !matches!(ch, '\'' | '"' | '`') {
            continue;
        }
        let mut literal = String::new();
        let mut spelled = String::new();
        let mut closed = false;
        while let Some(next) = chars.next() {
            if next == '\\' {
                spelled.push(next);
                if let Some(escaped) = chars.next() {
                    spelled.push(escaped);
                    literal.push(escaped);
                }
            } else if next == ch {
                closed = true;
                break;
            } else {
                spelled.push(next);
                literal.push(next);
            }
        }
        // Prefer a decoded native path, then its spelling if escape removal
        // erased the separators. Keep its identity before splitting words:
        // `C:` and directory names are not separate relative operands.
        let native = [&literal, &spelled]
            .into_iter()
            .find(|reading| cfg!(windows) && Path::new(reading).is_absolute())
            .map(|reading| {
                let path = crate::portable_path::without_verbatim(PathBuf::from(reading));
                crate::portable_path::slashed(&path)
            });
        words.extend(outward::literal_words(
            native.as_deref().unwrap_or(&literal),
        ));
        if !closed {
            break;
        }
    }
    words
}

/// A known word read from a directory the guard cannot follow: by its
/// trailing components, authority first at its fixed level.
fn unknown_dir_literal(value: &str) -> Option<Reach> {
    if let Some(name) = tail_names_authority(value) {
        return Some(Reach::Authority(format!(
            "an interpreter's code literal `{value}` could be policy authority metadata `{name}`: it runs from a directory the guard cannot follow"
        )));
    }
    let reached = git_guard::word_could_name(value)?;
    let message = format!(
        "an interpreter's code literal `{value}` could be `{reached}`: it runs from a directory the guard cannot follow"
    );
    Some(if names_authority(reached) {
        Reach::Authority(message)
    } else {
        Reach::Enforcement(message)
    })
}

/// Why a line word is judged: its interpreter code is unreadable.
const OPAQUE: &str = "an interpreter on this line runs code the guard cannot read";
/// Why a `printf` value is judged: a shell or interpreter runs on the line.
const PRINTF: &str =
    "a value `printf` joins on a line where a shell or interpreter runs may be its code";
/// Why a quoted string is judged: a shell or interpreter runs on the line.
const QUOTED: &str = "a quoted string on a line where a shell or interpreter runs may be its code";

/// A word on a line whose shell or interpreter code the guard cannot read
/// in full, or a word of a quoted string where one runs, judged by its
/// trailing components. A part filled in at run time may be empty, which
/// joins the spelled pieces around it (TSK-302 review round 15): the word
/// is judged with each such part removed too, as the shell removes it,
/// for authority and for enforcement alike.
fn opaque_line_word(value: &str, why: &str) -> Option<Reach> {
    tail_reach(value, value, why)
        .or_else(|| empty_spans(value).and_then(|joined| tail_reach(&joined, value, why)))
}

/// A word of a composed value's reading, judged by its trailing
/// components; its unknown pieces are already read both ways.
fn composed_word(word: &str) -> Option<Reach> {
    tail_reach(word, word, PRINTF)
}

/// [`opaque_line_word`] for one reading of a word, shown as `shown`.
fn tail_reach(word: &str, shown: &str, why: &str) -> Option<Reach> {
    let empty = if word == shown {
        String::new()
    } else {
        format!(" (`{word}` when what is filled in is empty)")
    };
    if let Some(name) = tail_names_authority(word) {
        return Some(Reach::Authority(format!(
            "`{shown}` could be policy authority metadata `{name}`{empty}: {why}"
        )));
    }
    let reached = git_guard::word_could_name(word)?;
    let message = format!("`{shown}` could be `{reached}`{empty}: {why}");
    Some(if names_authority(reached) {
        Reach::Authority(message)
    } else {
        Reach::Enforcement(message)
    })
}

/// A raw shell word with every part the shell fills in at run time
/// removed whole, as it reads when each is empty: a `${...}`, `$(...)` or
/// backquoted span, a `$` with the name it introduces, and a substitution
/// placeholder. The parameter's name is never kept as spelled text
/// (review round 16). `None` when nothing is removed.
fn empty_spans(value: &str) -> Option<String> {
    if !value.contains(['$', '`', '\u{1}', '\u{2}']) {
        return None;
    }
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        at += 1;
        match c {
            '\u{1}' | '\u{2}' => {}
            '`' => {
                while chars.get(at).is_some_and(|c| *c != '`') {
                    at += 1;
                }
                at += 1;
            }
            '$' => match chars.get(at) {
                Some(open @ ('{' | '(')) => {
                    let close = if *open == '{' { '}' } else { ')' };
                    let mut depth = 0usize;
                    while let Some(&d) = chars.get(at) {
                        at += 1;
                        if d == *open {
                            depth += 1;
                        } else if d == close {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                }
                Some(d) if d.is_ascii_alphabetic() || *d == '_' => {
                    while chars
                        .get(at)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                    {
                        at += 1;
                    }
                }
                Some(d) if d.is_ascii_digit() || "@*#?!-$".contains(*d) => at += 1,
                _ => {}
            },
            other => out.push(other),
        }
    }
    (out != value && !out.is_empty()).then_some(out)
}

/// [`value_reach`] for a code literal: a word inside the interpreter's own
/// source, not a shell operand, so an unresolved one is not read by the
/// line's writer rule (TSK-302 review round 2, Grok F1).
fn code_reach(value: &str, dir: Option<&Path>, facts: &mut Facts<'_>) -> Option<Reach> {
    value_reach_at(value, false, dir, true, facts)
}

fn value_reach_at(
    value: &str,
    pattern: bool,
    dir: Option<&Path>,
    code: bool,
    facts: &mut Facts<'_>,
) -> Option<Reach> {
    // `~user` and `~-` are filled in by the shell too.
    let unresolved = git_guard::unresolved_word(value)
        || (value.starts_with('~') && !value.starts_with("~/") && value != "~");
    let absolute = value.starts_with('/') || value.starts_with("~/") || value == "~";
    match (unresolved, dir.or(absolute.then_some(facts.payload_cwd))) {
        (false, Some(dir)) => literal_reach(value, pattern, dir, code, facts),
        // A code literal whose directory is unknown is judged by its
        // trailing components, as an argument is; an unresolved expression
        // inside code stays exempt (TSK-302 review round 3, R3-2).
        (false, None) if code => unknown_dir_literal(value),
        _ if code => None,
        _ => unreadable_reach(value, dir, unresolved, facts),
    }
}

/// A literal argument, its braces and globs expanded: authority metadata
/// first, then an enforcement path. Only candidate paths take the writer's
/// full checks, once each, without reinterpreting the original word.
fn literal_reach(
    value: &str,
    pattern: bool,
    dir: &Path,
    code: bool,
    facts: &mut Facts<'_>,
) -> Option<Reach> {
    // Code strings do not undergo shell tilde expansion. A runtime call
    // such as expanduser("~") is beyond this literal-path reading.
    let home = if code { None } else { facts.home };
    let paths = match literal_paths(value, pattern, dir, facts.payload_cwd, home) {
        Ok(paths) => paths,
        Err(why) => {
            let message = format!("cannot inspect interpreter argument `{value}`: {why}");
            return Some(
                if names_authority(value) || tail_names_authority(value).is_some() {
                    Reach::Authority(message)
                } else {
                    Reach::Enforcement(message)
                },
            );
        }
    };
    // The writer checks both the payload repository and the resolved run
    // directory. Cache the latter's candidate names too, once per directory.
    // A proved-absent directory cannot introduce another repository. Its
    // arguments still take the payload repository's candidate checks.
    let run_names = (dir != facts.root && !crate::absence::cannot_exist(dir).unwrap_or(false))
        .then(|| {
            facts
                .run_names
                .entry(dir.to_path_buf())
                .or_insert_with(|| ProtectedNames::load(dir, facts.home))
        });
    // Authority keeps its fixed level even when an earlier candidate names
    // ordinary enforcement state. Most candidates cannot name authority.
    for path in &paths {
        if (facts.names.may_reach_authority(path)
            && edit_guard::repository_authority_target(path, facts.root, true))
            || run_names.as_ref().is_some_and(|names| {
                names.may_reach_authority(path)
                    && edit_guard::repository_authority_target(path, dir, true)
            })
        {
            return Some(Reach::Authority(format!(
                "an interpreter argument `{value}` may reach policy authority metadata"
            )));
        }
    }
    for path in paths.iter().filter(|path| {
        facts.names.may_reach(path)
            || run_names
                .as_ref()
                .is_some_and(|names| names.may_reach(path))
    }) {
        // A string fragment such as "/." is not an argv operand reaching
        // other checkouts. Exact enforcement paths still take file checks.
        let reached = git_guard::interpreter_path_integrity(path, dir, facts.payload_cwd, code)
            .or_else(|| {
                // The explicit home can differ from the guard process's home.
                // Keep its native-edit protections for candidates beneath it.
                let Some(text) = path.to_str() else {
                    return Some("cannot inspect a non-UTF-8 enforcement path".to_string());
                };
                let Some(home) = facts.home else {
                    return Some("cannot resolve the home for enforcement paths".to_string());
                };
                let in_home = path.starts_with(home)
                    || resolved_existing(path)
                        .zip(resolved_existing(home))
                        .is_none_or(|(path, home)| path.starts_with(home));
                if !in_home {
                    return None;
                }
                match edit_guard::enforcement_path(text, dir, facts.root, Some(home)) {
                    Ok(false) => None,
                    Ok(true) => Some("home enforcement files".to_string()),
                    Err(why) => Some(format!("cannot inspect home enforcement paths: {why}")),
                }
            });
        let Some(reached) = reached else {
            continue;
        };
        let message = format!("an interpreter argument `{value}` may reach `{reached}`");
        if names_authority(&reached) {
            return Some(Reach::Authority(message));
        }
        return Some(Reach::Enforcement(message));
    }
    None
}

/// Whether an unreadable value's spelled part is worth git-guard's full
/// reading: it spells a protected name, a home or configuration variable,
/// a tilde, a glob or braces. Any other value names nothing by its text,
/// and the line rule judges what fills it in.
fn spells_protected(value: &str, names: &ProtectedNames) -> bool {
    let lower = value.to_lowercase();
    names.unknown
        || git_guard::has_glob(value)
        || value.contains(['{', '~'])
        || ["home", "xdg_config", "git_config", "gitconfig"]
            .iter()
            .any(|name| lower.contains(name))
        || names.names.iter().any(|name| lower.contains(name.as_str()))
}

/// An argument the guard cannot read: part of the value, or the directory
/// it is read from, is filled in at run time. Its spelled part is read by
/// its names, then the line is read for authority metadata and an
/// enforcement path, as git-guard reads a writer's operand.
fn unreadable_reach(
    value: &str,
    dir: Option<&Path>,
    unresolved: bool,
    facts: &mut Facts<'_>,
) -> Option<Reach> {
    let shown = git_guard::shown_word(value);
    // Authority is decided apart from the first enforcement description:
    // `"$W/.git/"*` can reach `.git/config` though git-guard first names
    // `.git/hooks` (TSK-302 review round 2).
    if let Some(name) = tail_names_authority(value) {
        return Some(Reach::Authority(format!(
            "an interpreter argument `{shown}` could name policy authority metadata `{name}`"
        )));
    }
    let reached = match dir {
        Some(dir) if spells_protected(value, &facts.names) => {
            git_guard::token_integrity_path(value, dir, facts.payload_cwd)
        }
        Some(_) => None,
        None => git_guard::word_could_name(value),
    };
    if let Some(reached) = reached {
        let why = if unresolved {
            "part of it is filled in at run time"
        } else {
            "it is used from a directory the guard cannot follow"
        };
        let message = format!("an interpreter argument `{shown}` could name `{reached}`: {why}");
        return Some(if names_authority(reached) {
            Reach::Authority(message)
        } else {
            Reach::Enforcement(message)
        });
    }
    if !unresolved {
        return None;
    }
    if let Some(word) = facts.line_authority(dir) {
        return Some(Reach::Authority(format!(
            "an interpreter argument `{shown}` is filled in at run time on a line that names policy authority metadata `{word}`"
        )));
    }
    let named = facts.line_enforcement(dir)?;
    Some(Reach::Enforcement(format!(
        "an interpreter argument `{shown}` is filled in at run time on a line that names the enforcement path `{named}`"
    )))
}

/// Authority metadata as a trailing spelling a word can end in.
const AUTHORITY_TAILS: &[&str] = &[
    ".git/config",
    ".git/config.worktree",
    ".git/packed-refs",
    ".git/refs/remotes/x",
    ".gitconfig",
    ".config/git/config",
    "etc/gitconfig",
];

/// The authority metadata a word's spelled tail can name: the part after
/// what the shell fills in (or the whole word, read from a directory the
/// guard cannot follow), read as a pattern against the trailing components
/// of each authority path.
fn tail_names_authority(value: &str) -> Option<&'static str> {
    let cut = value
        .char_indices()
        .rev()
        .find(|(_, c)| matches!(c, '$' | '`' | '\u{1}' | '\u{2}'))
        .map(|(at, _)| at);
    let tail = match cut {
        Some(cut) => value[cut..].split_once('/').map(|(_, tail)| tail)?,
        None => value,
    };
    let tail = tail.trim_start_matches("./");
    if tail.is_empty() || tail.starts_with('/') {
        return None;
    }
    let pattern = glob::Pattern::new(tail).ok()?;
    let options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    // A remote-tracking ref is any name under `refs/remotes/`.
    if tail
        .split_once("refs/remotes/")
        .is_some_and(|(before, name)| {
            !name.is_empty() && (before.is_empty() || before.ends_with('/'))
        })
    {
        return Some(".git/refs/remotes/x");
    }
    let depth = tail.split('/').count();
    AUTHORITY_TAILS.iter().copied().find(|path| {
        // `x` stands for a remote name: a one-component word never
        // matches it (review round 8 F3), a pattern of more does.
        if depth == 1 && path.ends_with("/remotes/x") {
            return false;
        }
        let parts: Vec<&str> = path.split('/').collect();
        parts.len() >= depth
            && pattern.matches_with(&parts[parts.len() - depth..].join("/"), options)
    })
}

/// The authority metadata files of the repository at `root`: its config,
/// packed refs, worktree configs and remote-tracking refs, as far as a
/// bounded walk lists them.
fn authority_files(root: &Path) -> Vec<PathBuf> {
    const WALK_LIMIT: usize = 4096;
    let Ok(Some(repo)) = crate::hooks::repo::open(root) else {
        return Vec::new();
    };
    let common = repo.commondir().to_path_buf();
    let mut files = vec![
        common.join("config"),
        common.join("packed-refs"),
        common.join("config.worktree"),
    ];
    if let Ok(entries) = std::fs::read_dir(common.join("worktrees")) {
        files.extend(
            entries
                .flatten()
                .map(|entry| entry.path().join("config.worktree")),
        );
    }
    // Each path under refs/remotes is listed; one that reads as a
    // directory is walked in turn.
    let mut stack = vec![common.join("refs/remotes")];
    while let Some(path) = stack.pop() {
        if files.len() >= WALK_LIMIT {
            break;
        }
        if let Ok(entries) = std::fs::read_dir(&path) {
            stack.extend(entries.flatten().map(|entry| entry.path()));
        }
        files.push(path);
    }
    files
}

/// The arguments `xargs` or `find -exec` gives an interpreter. `xargs`
/// reads them at run time, so they refuse on a line that names authority
/// metadata or an enforcement path. `find` gives each path its starting
/// points reach and its `-name` tests let through, so they refuse when one
/// is authority metadata or an enforcement path.
fn supplied_reach(call: &outward::InterpreterCall, facts: &mut Facts<'_>) -> Option<Reach> {
    let supplier = call.supplier.as_ref()?;
    let dir = call.cwd.as_deref();
    if let outward::Supplier::Find { .. } = supplier {
        // `find` hands its paths over as `{}`, or to a shell `-c` string
        // that passes them on as its positional parameters.
        if !call
            .args
            .iter()
            .any(|word| word.contains("{}") || git_guard::unresolved_word(word))
        {
            return None;
        }
    }
    let what = match supplier {
        outward::Supplier::Xargs => "`xargs` gives an interpreter arguments it reads at run time,",
        outward::Supplier::Find { .. } => "`find` gives an interpreter paths",
    };
    if let Some(word) = facts.line_authority(dir) {
        return Some(Reach::Authority(format!(
            "{what} on a line that names policy authority metadata `{word}`"
        )));
    }
    if let outward::Supplier::Find {
        starts,
        before,
        cwd,
        ..
    } = supplier
    {
        if let Some(reach) = find_reach(starts, before, cwd.as_deref(), facts) {
            return Some(reach);
        }
    }
    let named = facts.line_enforcement(dir)?;
    Some(Reach::Enforcement(format!(
        "{what} on a line that names the enforcement path `{named}`"
    )))
}

/// The protected path a `find` hands its command: authority metadata or an
/// enforcement path under a start that its `-name` tests let through.
fn find_reach(
    starts: &[String],
    before: &[String],
    cwd: Option<&Path>,
    facts: &Facts<'_>,
) -> Option<Reach> {
    let filter = git_guard::find_name_filter(before);
    let authority = authority_files(facts.root);
    for start in starts {
        // The line check covers a start the guard cannot read.
        let Some(start) = cwd
            .filter(|_| !git_guard::unresolved_word(start))
            .map(|cwd| cwd.join(start))
        else {
            continue;
        };
        let resolved = std::fs::canonicalize(&start).unwrap_or_else(|_| start.clone());
        if let Some(path) = authority.iter().find(|path| {
            let real = std::fs::canonicalize(path).unwrap_or_else(|_| (*path).clone());
            (real.starts_with(&resolved) || path.starts_with(&start))
                && git_guard::find_name_matches(filter.as_deref(), path)
        }) {
            return Some(Reach::Authority(format!(
                "`find` can give an interpreter policy authority metadata `{}`",
                path.display()
            )));
        }
        let candidates = match edit_guard::find_candidates(&start, facts.payload_cwd) {
            Ok(candidates) => candidates,
            Err(error) => {
                return Some(Reach::Enforcement(format!(
                    "cannot read the paths `find` gives an interpreter: {error}"
                )))
            }
        };
        // The candidates are the enforcement paths and checkouts under the
        // start, and the directories leading to them.
        if let Some(candidate) = candidates.iter().find(|candidate| {
            git_guard::find_name_matches(filter.as_deref(), candidate)
                && edit_guard::repository_enforcement_target(candidate, facts.payload_cwd, false)
                    .unwrap_or(true)
        }) {
            return Some(Reach::Enforcement(format!(
                "`find` can give an interpreter the enforcement path `{}`",
                candidate.display()
            )));
        }
    }
    None
}

/// A name as compared here: Unicode NFC, lower case, so a spelling a
/// case-insensitive or normalizing file system reads as the same name
/// matches. `None` for a name that is not valid UTF-8, which the caller
/// sends to the full checks.
fn folded(name: &std::ffi::OsStr) -> Option<String> {
    use unicode_normalization::UnicodeNormalization as _;
    Some(name.to_str()?.nfc().collect::<String>().to_lowercase())
}

/// `path` with the symbolic links on its existing part resolved: the
/// longest prefix that exists, canonicalized, then the rest as spelled.
/// `None` when that cannot be read.
fn resolved_existing(path: &Path) -> Option<PathBuf> {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        rest.push(existing.file_name()?.to_owned());
        if !existing.pop() {
            return None;
        }
    }
    let mut resolved = crate::portable_path::canonicalize(&existing).ok()?;
    resolved.extend(rest.iter().rev());
    Some(resolved)
}

/// The names a path passes through when it is an enforcement path or
/// policy authority metadata: the first component of every protected
/// pattern in the action table, the repository's common directory, and
/// the git configuration files. Protected directories and ancestors of
/// registered checkout roots also qualify. Ordinary paths, read as spelled
/// and with symbolic links resolved, skip the full repository checks.
struct ProtectedNames {
    names: Vec<String>,
    authority_names: Vec<String>,
    /// Git configuration files named by full path (`XDG_CONFIG_HOME`,
    /// `GIT_CONFIG_GLOBAL`), each folded, as spelled and resolved.
    configs: Vec<String>,
    /// Folded checkout roots. Their ancestors can remove whole checkouts;
    /// ordinary files below them do not need repository or worktree reads.
    checkouts: Vec<PathBuf>,
    /// A protected pattern this reading does not know: every path takes
    /// the full checks.
    unknown: bool,
}

impl ProtectedNames {
    fn load(root: &Path, home: Option<&Path>) -> Self {
        let mut unknown = false;
        let mut checkouts = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut authority_names = vec![".git".into(), ".gitconfig".into(), "gitconfig".into()];
        for rule in &actions::table().claude_edit_denies {
            let first = rule
                .strip_prefix("Edit(")
                .and_then(|s| s.strip_suffix(')'))
                .and_then(|s| s.strip_prefix("~/").or_else(|| s.strip_prefix('/')))
                .and_then(|s| s.split('/').next())
                .filter(|s| !s.is_empty() && !git_guard::has_glob(s));
            match first.and_then(|first| folded(std::ffi::OsStr::new(first))) {
                Some(first) => names.push(first),
                None => unknown = true,
            }
        }
        for name in [".git", ".gitconfig", "gitconfig"] {
            names.push(name.to_string());
        }
        match crate::hooks::repo::open(root) {
            Ok(Some(repo)) => {
                match edit_guard::checkout_roots(&repo) {
                    Ok(roots) => {
                        for path in roots.iter().flat_map(|path| {
                            [
                                edit_guard::normalized(path, false).ok(),
                                resolved_existing(path),
                            ]
                        }) {
                            match path.as_deref().map(Path::as_os_str).and_then(folded) {
                                Some(path) => checkouts.push(PathBuf::from(path)),
                                None => unknown = true,
                            }
                        }
                    }
                    Err(_) => unknown = true,
                }
                match repo.commondir().components().next_back() {
                    Some(Component::Normal(name)) => match folded(name) {
                        Some(name) => {
                            authority_names.push(name.clone());
                            names.push(name);
                        }
                        None => unknown = true,
                    },
                    _ => unknown = true,
                }
            }
            Ok(None) => {}
            Err(_) => unknown = true,
        }
        names.sort();
        names.dedup();
        let mut configs: Vec<PathBuf> = crate::portable_path::user_homes()
            .into_iter()
            .chain(home.map(Path::to_path_buf))
            .map(|home| home.join(".config/git/config"))
            .collect();
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            configs.push(PathBuf::from(xdg).join("git/config"));
        }
        for name in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"] {
            if let Some(path) = std::env::var_os(name).filter(|s| !s.is_empty()) {
                configs.push(root.join(path));
            }
        }
        let mut folded_configs = Vec::new();
        for path in configs.iter().flat_map(|path| {
            [
                edit_guard::normalized(path, false).ok(),
                resolved_existing(path),
            ]
        }) {
            match path.as_deref().map(Path::as_os_str).map(folded) {
                Some(Some(path)) => folded_configs.push(path),
                // A configuration path that cannot be read sends every
                // path to the full checks.
                _ => unknown = true,
            }
        }
        Self {
            names,
            authority_names,
            configs: folded_configs,
            checkouts,
            unknown,
        }
    }

    fn may_reach_authority(&self, target: &Path) -> bool {
        if self.unknown {
            return true;
        }
        let Some(resolved) = resolved_existing(target) else {
            return true;
        };
        [target, resolved.as_path()].iter().any(|path| {
            path.components().any(|component| match component {
                Component::Normal(name) => {
                    folded(name).is_none_or(|name| self.authority_names.contains(&name))
                }
                _ => false,
            }) || folded(path.as_os_str()).is_none_or(|path| self.configs.contains(&path))
        })
    }

    /// Whether `target` may be an enforcement path or authority metadata,
    /// so the full checks must judge it.
    fn may_reach(&self, target: &Path) -> bool {
        // `..` after a symbolic link leaves where the spelling says; the
        // full checks read it.
        if self.unknown || target.components().any(|c| c == Component::ParentDir) {
            return true;
        }
        let Some((lexical, resolved)) = edit_guard::normalized(target, false)
            .ok()
            .and_then(|lexical| resolved_existing(&lexical).map(|resolved| (lexical, resolved)))
        else {
            return true;
        };
        [lexical, resolved].iter().any(|path| {
            path.components().any(|component| match component {
                Component::Normal(name) => {
                    folded(name).is_none_or(|name| self.names.contains(&name))
                }
                _ => false,
            }) || folded(path.as_os_str()).is_none_or(|path| {
                self.configs.contains(&path)
                    || self
                        .checkouts
                        .iter()
                        .any(|checkout| checkout.starts_with(&path))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TSK-302 review round 3 (Codex R3-3): the directories `-execdir`
    /// can run in, compared with native `find START EXPR -print`. Every
    /// matched entry whose parent leads to a protected path must have that
    /// parent in the guard's set, or the set must be unknown. Skipped on a
    /// host with no `find`.
    #[test]
    fn execdir_directories_match_native_find() {
        use std::process::Command;
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        git2::Repository::init(&root).unwrap();
        for (path, body) in [
            (".claude/settings.json", "{}\n"),
            (".claude/notes.txt", "\n"),
            (".codeflow/policy.json", "{}\n"),
            ("src/app.py", "\n"),
            ("src/notes.txt", "\n"),
        ] {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, body).unwrap();
        }
        let relevant = edit_guard::find_candidates(&root, &root).unwrap();
        let cases: &[(&str, &[&str])] = &[
            (".", &["-name", "settings.json"]),
            (".", &["-path", "*cla*json"]),
            (".", &["-path", "*git*config"]),
            (".", &["-ipath", "*CLA*JSON"]),
            (".", &["-path", "./.claude/*"]),
            (".", &["-type", "f", "-name", "*.json"]),
            (".", &["-name", "*.py"]),
            (".", &["-newer", "src/app.py"]),
            (".claude", &["-name", "notes.txt"]),
            (".claude/settings.json", &[]),
            (".git/config", &[]),
        ];
        let probe = Command::new("/usr/bin/find")
            .arg(".")
            .arg("-prune")
            .current_dir(&root)
            .output();
        if !probe.is_ok_and(|out| out.status.success()) {
            eprintln!("skipped: /usr/bin/find is not available");
            return;
        }
        let mut compared = 0;
        for (start, expr) in cases {
            let output = Command::new("/usr/bin/find")
                .arg(start)
                .args(*expr)
                .arg("-print")
                .current_dir(&root)
                .env("PATH", "/usr/bin:/bin")
                .output()
                .unwrap();
            let mut before: Vec<String> = vec![(*start).to_string()];
            before.extend(expr.iter().map(|w| (*w).to_string()));
            let line = outward::DirSet {
                dirs: vec![root.clone()],
                unknown: false,
            };
            let set = execdir_dirs(&[(*start).to_string()], &before, Some(&root), &line, &root);
            for entry in String::from_utf8(output.stdout).unwrap().lines() {
                let Ok(path) = std::fs::canonicalize(root.join(entry)) else {
                    continue;
                };
                let Some(parent) = path.parent().map(Path::to_path_buf) else {
                    continue;
                };
                if !relevant.contains(&parent) {
                    continue;
                }
                compared += 1;
                assert!(
                    set.unknown || set.dirs.contains(&parent),
                    "find {start} {expr:?}: `{entry}` runs in {parent:?}, not in {:?}",
                    set.dirs
                );
            }
        }
        // The match sets reach `.claude`, `.codeflow` and `.git`, so the
        // comparison is not vacuous.
        assert!(compared >= 6, "{compared}");
    }
}
