//! Headless peer runs (TSK-136): Claude Code, Codex or Grok run one-shot,
//! without their interactive session.
//!
//! Peer seats run interactively, never headless (cf-delegate): a one-shot
//! headless run has no verified native session, no task tools or guards, and
//! no recheckable thread. This classifier finds such a run in a shell command
//! so exec-guard can refuse it (the default since ADR-0075 D4) or warn.
//!
//! The headless forms come from each CLI's own help (Claude Code 2.1.283,
//! Codex CLI 0.157.1, Grok 1.0.41):
//! - `claude -p`/`--print`, and `claude ultrareview`, which prints a review;
//! - `codex exec` (alias `e`) and `codex review`, both non-interactive;
//! - `grok -p`/`--single <PROMPT>` (also `--single=…` and `-p…`),
//!   `--prompt-file`, `--prompt-json` and `grok agent`.
//!
//! Each simple command is read the way git-guard reads it
//! (`git_guard::simple_commands`: control-structure bodies, groups, substitutions,
//! `bash -c` and `eval` strings, quotes and escapes), then the launch
//! wrappers are unwrapped (`nice`, `timeout`, `nohup`, `sudo`, `xargs`,
//! `find -exec`, a nested shell `-c`, `grok wrap`, …), and the peer's own
//! arguments are parsed by role: an option's value and anything after `--`
//! are never read as the headless flag. A help or version flag in an option
//! position (`claude --help -p`, `codex exec --help`) is no run, since the
//! CLI prints and exits (TSK-141); the same word as a value, a prompt or
//! after `--` is data.
//!
//! A command the classifier cannot resolve is judged by what it can run
//! (TSK-223). A variable or substitution as the program is judged by what
//! it can expand to with its own arguments: a launcher of a peer
//! (`$SUDO codex exec`), a value the line assigns (`CMD="codex exec"`), a
//! substitution's text (`$(echo codex exec) x`), or any peer the line names
//! (`CMD=codex; $CMD exec`). An alias the line defines is expanded where it
//! is used. A here-string is judged only when it feeds something other than
//! a known reader, and a pipe only when it feeds a shell or such a program.
//! None of these reads a heredoc body the shell reader treats as data
//! (`cat > brief.md <<EOF`) or a commit message. What is left (a shell,
//! `source` or a function reading a script this line may have written, a
//! launcher the reader does not model) is judged on the line's raw text the
//! way each CLI parses it: a peer name followed by its headless flag, or by
//! its headless subcommand as the first positional word (`codex exec`,
//! `claude -p`, `grok -p`). Such a line, and interpreter code, can still
//! flag text that reads as a run to the CLI (`the Codex review`); the
//! message says the line was not fully parsed, and a project that needs
//! such a run sets `security.headless_peer_runs` to `warn`.

use crate::hooks::git_guard::{
    command_argv, expand_commands, simple_commands, strip_launchers, strip_reserved_words,
};

/// A headless peer run found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessRun {
    /// The peer CLI: `claude`, `codex` or `grok`.
    pub peer: &'static str,
    /// The headless form matched, such as `claude -p` or `codex exec`.
    pub form: &'static str,
    /// `false` when the line could not be parsed and its raw text named the
    /// peer with a headless marker.
    pub parsed: bool,
}

/// The first headless peer run in `command`, if any.
#[must_use]
pub fn headless_peer_run(command: &str) -> Option<HeadlessRun> {
    find(command, 0)
}

/// What one simple command is.
enum Found {
    Run(HeadlessRun),
    /// It runs something this classifier cannot resolve.
    Unresolved,
    /// A variable or substitution names the program; these are its
    /// arguments.
    UnknownProgram(Vec<String>),
    Nothing,
}

/// The peer CLIs.
const PEERS: [&str; 3] = ["claude", "codex", "grok"];

/// Programs that read a here-string as data and never run it. Tools that
/// can run their input (`sed e`, `awk` with `system()`, shells,
/// interpreters) are left out, as is every program not listed.
const HERE_STRING_READERS: &[&str] = &[
    "cat",
    "tee",
    "echo",
    "printf",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "wc",
    "head",
    "tail",
    "sort",
    "uniq",
    "tr",
    "cut",
    "diff",
    "jq",
    "base64",
    "read",
    "mapfile",
    "readarray",
    "git",
    "gh",
];

fn find(command: &str, depth: usize) -> Option<HeadlessRun> {
    if depth > 4 {
        return raw_run(command);
    }
    let mut unresolved = here_string_runs(command);
    let mut aliases: Vec<(String, String)> = Vec::new();
    for argv in simple_commands(command) {
        if argv.first().is_some_and(|program| program == "alias") {
            for (name, value) in argv[1..].iter().filter_map(|arg| arg.split_once('=')) {
                // A definition that itself starts a run is judged as one.
                if let Some(run) = find(value, depth + 1) {
                    return Some(HeadlessRun {
                        parsed: false,
                        ..run
                    });
                }
                aliases.push((name.to_string(), value.to_string()));
            }
            continue;
        }
        // An alias the line defined runs its value with these arguments.
        if let Some((_, value)) = argv
            .first()
            .and_then(|program| aliases.iter().find(|(name, _)| name == program))
        {
            let expanded = format!("{value} {}", quoted(&argv[1..]));
            if let Some(run) = find(&expanded, depth + 1) {
                return Some(HeadlessRun {
                    parsed: false,
                    ..run
                });
            }
            continue;
        }
        match classify(&argv, depth) {
            Found::Run(run) => return Some(run),
            Found::Unresolved => unresolved = true,
            Found::UnknownProgram(args) => {
                if let Some(run) = unknown_program_run(command, &args, depth) {
                    return Some(run);
                }
            }
            Found::Nothing => {}
        }
    }
    if unresolved || pipes_into_unknown(command) {
        raw_run(command)
    } else {
        None
    }
}

/// Words joined for a shell to read back, each in single quotes.
fn quoted(words: &[String]) -> String {
    words
        .iter()
        .map(|word| format!("'{}'", word.replace('\'', r"'\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A program named by a variable or substitution, run with `args`, is a
/// run when what it can expand to starts one: it launches a peer through
/// its arguments (`$SUDO codex exec`); a value the line assigns, followed
/// by the arguments, runs one (`CMD="codex exec"; $CMD x`); the text of a
/// substitution on the line, followed by the arguments, names one
/// (`$(echo codex exec) x`); or a peer the line names would run headless
/// with these arguments (`CMD=codex; $CMD exec`). Other text on the line
/// never supplies the marker, so a brief or commit message that names a
/// peer stays data.
fn unknown_program_run(command: &str, args: &[String], depth: usize) -> Option<HeadlessRun> {
    let unparsed = |run: HeadlessRun| HeadlessRun {
        parsed: false,
        ..run
    };
    if let Found::Run(run) = classify(args, depth + 1) {
        return Some(unparsed(run));
    }
    let args_text = quoted(args);
    let assigned = expand_commands(command)
        .iter()
        .flat_map(|segment| command_argv(segment))
        .filter_map(|word| {
            word.split_once('=')
                .filter(|(name, _)| is_assignment(&format!("{name}=")))
                .map(|(_, value)| value.to_string())
        })
        .collect::<Vec<_>>();
    if let Some(run) = assigned
        .iter()
        .filter(|value| {
            PEERS
                .iter()
                .any(|peer| value.to_ascii_lowercase().contains(peer))
        })
        .find_map(|value| find(&format!("{value} {args_text}"), depth + 1))
    {
        return Some(unparsed(run));
    }
    if let Some(run) = substitution_bodies(command)
        .into_iter()
        .find_map(|body| raw_run(&format!("{body} {args_text}")))
    {
        return Some(run);
    }
    let named = raw_words(command);
    PEERS
        .iter()
        .filter(|peer| named.iter().any(|word| raw_name(word) == **peer))
        .find_map(|peer| match peer_run(peer, args, depth + 1) {
            Found::Run(run) => Some(unparsed(run)),
            _ => None,
        })
}

/// The text inside every `$(…)` and backtick substitution of `command`,
/// outermost first; quotes are not tracked, so a substitution inside
/// quotes is read too.
fn substitution_bodies(command: &str) -> Vec<&str> {
    let bytes = command.as_bytes();
    let mut bodies = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'$' && bytes.get(at + 1) == Some(&b'(') {
            let start = at + 2;
            let mut level = 1;
            let mut end = start;
            while end < bytes.len() && level > 0 {
                match bytes[end] {
                    b'(' => level += 1,
                    b')' => level -= 1,
                    _ => {}
                }
                end += 1;
            }
            let close = if level == 0 { end - 1 } else { end };
            bodies.push(&command[start..close]);
            at = start;
        } else if bytes[at] == b'`' {
            let start = at + 1;
            let close = command[start..]
                .find('`')
                .map_or(command.len(), |off| start + off);
            bodies.push(&command[start..close]);
            at = close + 1;
        } else {
            at += 1;
        }
    }
    bodies
}

/// Whether a here-string (`<<<`) feeds a command that may run its text:
/// anything but a known reader (a shell, an interpreter, a launcher, a
/// program a variable or substitution names). A here-string fed to `grep`
/// or `git` is data.
fn here_string_runs(command: &str) -> bool {
    expand_commands(command).iter().any(|segment| {
        if !segment.contains("<<<") {
            return false;
        }
        let mut words = command_argv(segment);
        strip_reserved_words(&mut words);
        strip_launchers(&words)
            .is_none_or(|(program, _)| !HERE_STRING_READERS.contains(&basename(program).as_str()))
    })
}

/// Whether a pipe feeds a program that a variable or substitution names
/// (`… | $SHELL`): its input may run as a script.
fn pipes_into_unknown(command: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut after_pipe = false;
    let mut chars = command.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if ch == open {
                quote = None;
            }
            continue;
        }
        // The first word after a pipe is the program that reads it.
        if after_pipe && !ch.is_whitespace() {
            after_pipe = false;
            let rest = &command[at..];
            let program = command_argv(rest).into_iter().next().unwrap_or_default();
            if program.contains(['$', '`'])
                || program.chars().any(char::is_control)
                || rest.starts_with("$(")
                || rest.starts_with("\"$")
            {
                return true;
            }
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '|' if chars.peek().is_some_and(|(_, next)| *next == '|') => {
                chars.next();
            }
            '|' => after_pipe = true,
            _ => {}
        }
    }
    false
}

/// Classify one simple command's argument vector, unwrapping launchers.
fn classify(argv: &[String], depth: usize) -> Found {
    let mut argv = argv;
    for _ in 0..8 {
        let Some(program) = argv.first() else {
            return Found::Nothing;
        };
        // A variable or substitution names the program; git-guard's reader
        // marks a substitution with a control character.
        if program.contains(['$', '`']) || program.chars().any(char::is_control) {
            return Found::UnknownProgram(argv[1..].to_vec());
        }
        let name = basename(program);
        let rest = &argv[1..];
        argv = match name.as_str() {
            "claude" | "codex" | "grok" => return peer_run(&name, rest, depth),
            "alias" | "function" | "source" | "." => return Found::Unresolved,
            "bash" | "sh" | "zsh" | "dash" | "ksh" | "ash" => {
                return match shell_command_string(rest) {
                    Some(inner) => find(inner, depth + 1).map_or(Found::Nothing, Found::Run),
                    // A script file, or a script read from stdin.
                    None => Found::Unresolved,
                };
            }
            "env" => match skip_env(rest) {
                Some(rest) => rest,
                None => return Found::Unresolved,
            },
            "command" | "builtin" | "exec" | "nohup" | "setsid" | "caffeinate" | "unbuffer"
            | "time" => skip_options(rest, &["-a"]),
            "nice" => skip_options(rest, &["-n", "--adjustment"]),
            "stdbuf" => skip_options(rest, &["-i", "-o", "-e"]),
            "timeout" => {
                let after = skip_options(rest, &["-s", "--signal", "-k", "--kill-after"]);
                after.get(1..).unwrap_or_default()
            }
            "sudo" | "doas" => skip_assignments(skip_options(
                rest,
                &[
                    "-u",
                    "--user",
                    "-g",
                    "--group",
                    "-C",
                    "--close-from",
                    "-D",
                    "--chdir",
                    "-h",
                    "--host",
                    "-p",
                    "--prompt",
                    "-r",
                    "--role",
                    "-t",
                    "--type",
                    "-T",
                    "--command-timeout",
                    "-U",
                    "--other-user",
                ],
            )),
            "xargs" => skip_options(
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
                    "--max-rest",
                    "--max-procs",
                    "--max-chars",
                    "--replace",
                ],
            ),
            "find" => return find_exec(rest, depth),
            // A package runner starts the package's binary (TSK-141 AC-6).
            // pnpm 11 also answers to `pn` and `pnx`.
            "npx" | "bunx" | "pnpx" | "pnx" => return package_run(rest, depth),
            "pn" => return runner("pnpm", rest, depth),
            "npm" | "pnpm" | "yarn" | "bun" => return runner(&name, rest, depth),
            _ => return Found::Nothing,
        };
    }
    Found::Unresolved
}

/// Options package runners take without a value.
const RUNNER_FLAGS: &[&str] = &[
    "-y",
    "--yes",
    "--no",
    "-q",
    "--quiet",
    "-s",
    "--silent",
    "--bun",
    "--ignore-existing",
    "--prefer-offline",
    "--prefer-online",
    "--offline",
    "--no-install",
    "--workspaces",
    "--ws",
    "--include-workspace-root",
    "--legacy-peer-deps",
    "--foreground-scripts",
    "--verbose",
    "--stream",
];

/// Options package runners take with a value in the next word.
const RUNNER_VALUES: &[&str] = &[
    "--cache",
    "--cache-folder",
    "--workspace",
    "--prefix",
    "--userconfig",
    "--globalconfig",
    "--registry",
    "--node-options",
    "--script-shell",
    "--loglevel",
    "--cwd",
    "--dir",
    "-C",
    "--filter",
    "-F",
    "--reporter",
];

/// The runner's own help or version: it prints and exits.
fn runner_exits(arg: &str) -> bool {
    matches!(arg, "-h" | "--help" | "-v" | "--version")
}

/// Where the first word after a runner's options may be. An option the
/// grammar does not know may or may not take the next word, so both are
/// kept, though never an option word, which it does not take (npm reads
/// `--unknown --help` as help); a help or version option ends the run.
pub(crate) fn after_runner_options(args: &[String]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![0];
    let mut steps = 0;
    while let Some(at) = stack.pop() {
        steps += 1;
        if steps > 64 {
            break;
        }
        match args.get(at) {
            None => {}
            Some(arg) if arg == "--" => out.push(at + 1),
            Some(arg) if !arg.starts_with('-') || arg == "-" => out.push(at),
            Some(arg) if runner_exits(arg) => {}
            Some(arg) if arg.contains('=') || RUNNER_FLAGS.contains(&arg.as_str()) => {
                stack.push(at + 1);
            }
            Some(arg) if RUNNER_VALUES.contains(&arg.as_str()) => stack.push(at + 2),
            Some(_) => {
                stack.push(at + 1);
                if takes_next(args, at) {
                    stack.push(at + 2);
                }
            }
        }
    }
    out
}

/// `npm`, `pnpm`, `yarn` or `bun` with a subcommand that runs a package or
/// a local binary: `npm exec`/`npm x`, `pnpm dlx`/`yarn dlx`, `bun x`, and
/// `pnpm exec`/`yarn exec` or `pnpm BIN`/`yarn BIN` for an installed peer.
fn runner(name: &str, args: &[String], depth: usize) -> Found {
    let mut found = Found::Nothing;
    for at in after_runner_options(args) {
        let Some((sub, tail)) = args.get(at..).and_then(<[String]>::split_first) else {
            continue;
        };
        let next = match (name, sub.as_str()) {
            ("npm", "exec" | "x") | ("pnpm" | "yarn", "dlx") | ("bun", "x") => {
                package_run(tail, depth)
            }
            ("pnpm" | "yarn", "exec") => after_runner_options(tail)
                .into_iter()
                .map(|start| classify(tail.get(start..).unwrap_or_default(), depth + 1))
                .fold(Found::Nothing, strongest),
            ("pnpm" | "yarn", "claude" | "codex" | "grok") => classify(&args[at..], depth + 1),
            _ => Found::Nothing,
        };
        found = strongest(found, next);
        if matches!(found, Found::Run(_)) {
            break;
        }
    }
    found
}

/// Whether an option the grammar does not know may take the word after
/// it as its value: not when that word is an option itself.
fn takes_next(args: &[String], at: usize) -> bool {
    args.get(at + 1)
        .is_some_and(|next| !next.starts_with('-') || next == "-")
}

/// A run over an unresolved command over nothing. Inside a launcher, a
/// program a variable names counts as unresolved: the line's raw text
/// judges it.
fn strongest(a: Found, b: Found) -> Found {
    match (a, b) {
        (Found::Run(run), _) | (_, Found::Run(run)) => Found::Run(run),
        (Found::Unresolved | Found::UnknownProgram(_), _)
        | (_, Found::Unresolved | Found::UnknownProgram(_)) => Found::Unresolved,
        _ => Found::Nothing,
    }
}

/// A package runner's command (`npx`, `bunx`, `npm exec`, `pnpm dlx`,
/// `yarn dlx`, `bun x`): after its options, the package's binary, or with
/// `--package` the command named after it; `-c` or `--call` runs a shell
/// string. An option the grammar does not know is read both with and
/// without a value, and the runner's help or version runs nothing.
fn package_run(args: &[String], depth: usize) -> Found {
    if depth > 4 {
        return Found::Unresolved;
    }
    let call = |script: &str| find(script, depth + 1).map_or(Found::Nothing, Found::Run);
    let mut found = Found::Nothing;
    let mut stack = vec![(0usize, false)];
    let mut steps = 0;
    while let Some((at, package_given)) = stack.pop() {
        steps += 1;
        if steps > 64 {
            return strongest(found, Found::Unresolved);
        }
        let Some(arg) = args.get(at) else {
            continue;
        };
        let next = if arg == "--" {
            launch(args, at + 1, package_given, depth)
        } else if !arg.starts_with('-') || arg == "-" {
            launch(args, at, package_given, depth)
        } else if let Some((option, value)) = arg.split_once('=') {
            match option {
                "-c" | "--call" => call(value),
                "-p" | "--package" => {
                    stack.push((at + 1, true));
                    continue;
                }
                _ => {
                    stack.push((at + 1, package_given));
                    continue;
                }
            }
        } else {
            match arg.as_str() {
                "-c" | "--call" => match args.get(at + 1) {
                    Some(script) => call(script),
                    None => continue,
                },
                "-p" | "--package" => {
                    stack.push((at + 2, true));
                    continue;
                }
                flag if runner_exits(flag) => continue,
                flag if RUNNER_FLAGS.contains(&flag) => {
                    stack.push((at + 1, package_given));
                    continue;
                }
                option if RUNNER_VALUES.contains(&option) => {
                    stack.push((at + 2, package_given));
                    continue;
                }
                _ => {
                    stack.push((at + 1, package_given));
                    if takes_next(args, at) {
                        stack.push((at + 2, package_given));
                    }
                    continue;
                }
            }
        };
        found = strongest(found, next);
        if matches!(found, Found::Run(_)) {
            return found;
        }
    }
    found
}

/// The package (or, with `--package`, the command) at `args[at]`, run with
/// the words after it.
fn launch(args: &[String], at: usize, package_given: bool, depth: usize) -> Found {
    let Some((spec, rest)) = args.get(at..).and_then(<[String]>::split_first) else {
        return Found::Nothing;
    };
    let program = if package_given {
        spec.clone()
    } else {
        package_bin(spec)
    };
    let mut command = vec![program];
    command.extend(rest.iter().cloned());
    classify(&command, depth + 1)
}

/// The binary a package spec runs: its name without scope or version,
/// with the peers' package names read as their commands.
pub(crate) fn package_bin(spec: &str) -> String {
    let unversioned = match spec.strip_prefix('@') {
        Some(scoped) => format!("@{}", scoped.split('@').next().unwrap_or(scoped)),
        None => spec.split('@').next().unwrap_or(spec).to_string(),
    };
    let name = unversioned.rsplit('/').next().unwrap_or(&unversioned);
    match name {
        "claude-code" => "claude".to_string(),
        "grok-cli" => "grok".to_string(),
        other => other.to_string(),
    }
}

/// The commands `find` runs with `-exec`, `-execdir`, `-ok` or `-okdir`.
fn find_exec(args: &[String], depth: usize) -> Found {
    let mut rest = args;
    let mut found = Found::Nothing;
    while let Some(at) = rest
        .iter()
        .position(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir"))
    {
        let body = &rest[at + 1..];
        let end = body
            .iter()
            .position(|a| a == ";" || a == "+")
            .unwrap_or(body.len());
        match classify(&body[..end], depth) {
            Found::Run(run) => return Found::Run(run),
            Found::Unresolved | Found::UnknownProgram(_) => found = Found::Unresolved,
            Found::Nothing => {}
        }
        rest = &body[end..];
    }
    found
}

/// Skip a wrapper's options (those in `with_value` take the next token) to
/// the command it runs.
pub(crate) fn skip_options<'a>(args: &'a [String], with_value: &[&str]) -> &'a [String] {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "--" {
            return &args[at + 1..];
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }
        at += if with_value.contains(&arg.as_str()) {
            2
        } else {
            1
        };
    }
    args.get(at..).unwrap_or_default()
}

/// Skip `NAME=value` words.
pub(crate) fn skip_assignments(args: &[String]) -> &[String] {
    let at = args
        .iter()
        .position(|arg| !is_assignment(arg))
        .unwrap_or(args.len());
    &args[at..]
}

/// `env`'s options and assignments up to its command; `None` for `env -S`,
/// which splits a string this classifier does not model.
pub(crate) fn skip_env(args: &[String]) -> Option<&[String]> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if arg == "-S" || arg.starts_with("--split-string") || arg.starts_with("-S") {
            return None;
        }
        if arg == "-u" || arg == "--unset" || arg == "-C" || arg == "--chdir" {
            at += 2;
        } else if arg.starts_with('-') || is_assignment(arg) {
            at += 1;
        } else {
            break;
        }
    }
    Some(args.get(at..).unwrap_or_default())
}

/// The string after `-c` (or a cluster holding `c`, such as `-lc`).
pub(crate) fn shell_command_string(args: &[String]) -> Option<&str> {
    let at = args.iter().position(|arg| {
        arg == "-c"
            || arg == "--command"
            || (arg.starts_with('-')
                && !arg.starts_with("--")
                && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
                && arg.contains('c'))
    })?;
    args.get(at + 1).map(String::as_str)
}

/// How a peer CLI reads its arguments, from its `--help`.
struct Cli {
    peer: &'static str,
    /// Short flags that mean a headless run.
    headless_short: &'static str,
    /// Whether the headless short flag takes a value (`grok -p <PROMPT>`).
    headless_short_value: bool,
    /// Short flags that print help or the version and exit, as
    /// `--help` and `--version` do.
    help_short: &'static str,
    /// Long flags (boolean or taking a value) that mean a headless run, with
    /// the form reported.
    headless_long: &'static [(&'static str, &'static str)],
    /// Short options taking a required value.
    short_value: &'static str,
    /// Short options taking an optional value.
    short_optional: &'static str,
    /// Long options taking a required value.
    long_value: &'static [&'static str],
    /// Long options taking an optional value.
    long_optional: &'static [&'static str],
    /// Long options taking every following non-option word.
    long_variadic: &'static [&'static str],
    /// Subcommands that run headless, with the form reported.
    headless_subcommands: &'static [(&'static str, &'static str)],
    /// Subcommands that wrap another command (`grok wrap <command>`).
    wrap_subcommands: &'static [&'static str],
    /// Other subcommands: their arguments are not the peer's.
    subcommands: &'static [&'static str],
}

const CLAUDE: Cli = Cli {
    peer: "claude",
    headless_short: "p",
    headless_short_value: false,
    help_short: "hv",
    headless_long: &[("--print", "claude -p")],
    short_value: "n",
    short_optional: "drw",
    long_value: &[
        "--agent",
        "--agents",
        "--append-system-prompt",
        "--append-system-prompt-file",
        "--autocompact",
        "--client-data-url",
        "--debug-file",
        "--effort",
        "--environment",
        "--fallback-model",
        "--input-format",
        "--json-schema",
        "--max-budget-usd",
        "--model",
        "--name",
        "--output-format",
        "--permission-mode",
        "--permission-prompts",
        "--plugin-dir",
        "--plugin-url",
        "--remote-control-session-name-prefix",
        "--session-id",
        "--setting-sources",
        "--settings",
        "--system-prompt",
        "--system-prompt-file",
        "--system-prompt-snapshot",
    ],
    long_optional: &[
        "--cloud",
        "--debug",
        "--from-pr",
        "--prompt-suggestions",
        "--remote-control",
        "--resume",
        "--teleport",
        "--worktree",
    ],
    long_variadic: &[
        "--add-dir",
        "--allowedTools",
        "--allowed-tools",
        "--betas",
        "--disallowedTools",
        "--disallowed-tools",
        "--file",
        "--mcp-config",
        "--tools",
    ],
    headless_subcommands: &[("ultrareview", "claude ultrareview")],
    wrap_subcommands: &[],
    subcommands: &[
        "agents",
        "attach",
        "auth",
        "auto-mode",
        "doctor",
        "gateway",
        "import",
        "install",
        "logs",
        "mcp",
        "plugin",
        "plugins",
        "project",
        "respawn",
        "rm",
        "setup-token",
        "stop",
        "kill",
        "update",
        "upgrade",
    ],
};

const CODEX: Cli = Cli {
    peer: "codex",
    headless_short: "",
    headless_short_value: false,
    help_short: "hV",
    headless_long: &[],
    short_value: "cimpsCao",
    short_optional: "",
    long_value: &[
        "--config",
        "--enable",
        "--disable",
        "--remote",
        "--remote-auth-token-env",
        "--image",
        "--model",
        "--local-provider",
        "--profile",
        "--sandbox",
        "--cd",
        "--add-dir",
        "--ask-for-approval",
        // `codex exec` and `codex review` options.
        "--output-schema",
        "--color",
        "--output-last-message",
        "--thread-source",
        "--base",
        "--commit",
        "--title",
    ],
    long_optional: &[],
    long_variadic: &[],
    headless_subcommands: &[
        ("exec", "codex exec"),
        ("e", "codex exec"),
        ("review", "codex review"),
    ],
    wrap_subcommands: &[],
    subcommands: &[
        "agents",
        "login",
        "logout",
        "mcp",
        "plugin",
        "app-server",
        "remote-control",
        "app",
        "completion",
        "update",
        "doctor",
        "sandbox",
        "debug",
        "apply",
        "a",
        "resume",
        "queue",
        "archive",
        "delete",
        "migrate-rollouts",
        "unarchive",
        "fork",
        "cloud",
        "exec-server",
        "features",
        "help",
    ],
};

const GROK: Cli = Cli {
    peer: "grok",
    headless_short: "p",
    headless_short_value: true,
    help_short: "hv",
    headless_long: &[
        ("--single", "grok -p"),
        ("--prompt-file", "grok --prompt-file"),
        ("--prompt-json", "grok --prompt-json"),
    ],
    short_value: "ms",
    short_optional: "rw",
    long_value: &[
        "--single",
        "--prompt-file",
        "--prompt-json",
        "--agent",
        "--agents",
        "--allow",
        "--allowedTools",
        "--cwd",
        "--debug-file",
        "--deny",
        "--disallowedTools",
        "--disallowed-tools",
        "--json-schema",
        "--leader-socket",
        "--model",
        "--max-turns",
        "--output-format",
        "--permission-mode",
        "--reasoning-effort",
        "--effort",
        "--rules",
        "--session-id",
        "--sandbox",
        "--system-prompt-override",
        "--system-prompt",
        "--tools",
        "--worktree-ref",
        "--ref",
    ],
    long_optional: &["--resume", "--worktree"],
    long_variadic: &[],
    headless_subcommands: &[("agent", "grok agent")],
    wrap_subcommands: &["wrap"],
    subcommands: &[
        "clone",
        "completions",
        "cursor-worker",
        "dashboard",
        "doctor",
        "du",
        "disk-usage",
        "export",
        "help",
        "inspect",
        "leader",
        "login",
        "logout",
        "mcp",
        "memory",
        "models",
        "plugin",
        "sessions",
        "setup",
        "trace",
        "update",
        "usage",
        "version",
        "v",
        "worktree",
    ],
};

/// Long flags that print help or the version and exit, in every peer CLI.
const HELP_LONG: &[&str] = &["--help", "--version"];

/// Judge a peer's arguments by role (TSK-136, TSK-141 AC-3). A headless
/// flag or subcommand makes a run, unless the line also asks for help or
/// the version in an option position: each CLI's parser (commander for
/// Claude Code, clap for Codex and Grok) then prints and exits before any
/// session starts. An option's value, anything after `--`, and a prompt are
/// data, never help.
fn peer_run(name: &str, args: &[String], depth: usize) -> Found {
    let cli = match name {
        "claude" => &CLAUDE,
        "codex" => &CODEX,
        _ => &GROK,
    };
    let short_form = cli.headless_long.first().map_or("", |(_, form)| *form);
    let mut headless: Option<&'static str> = None;
    let mut help = false;
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        if arg == "--" {
            break; // the rest is positional
        }
        if arg.starts_with("--") {
            let (flag, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
            if HELP_LONG.contains(&flag) && inline.is_none() {
                help = true;
                continue;
            }
            if let Some((_, form)) = cli.headless_long.iter().find(|(f, _)| *f == flag) {
                headless = headless.or(Some(form));
            }
            if inline.is_none() {
                at = after_long_value(cli, flag, args, at);
            }
            continue;
        }
        if let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (index, flag) in cluster.char_indices() {
                let rest = &cluster[index + flag.len_utf8()..];
                if cli.help_short.contains(flag) {
                    help = true;
                    continue;
                }
                if cli.headless_short.contains(flag) {
                    headless = headless.or(Some(short_form));
                    if cli.headless_short_value {
                        if rest.is_empty() {
                            at += 1;
                        }
                        break;
                    }
                    continue;
                }
                if cli.short_value.contains(flag) {
                    if rest.is_empty() {
                        at += 1;
                    }
                    break;
                }
                if cli.short_optional.contains(flag) {
                    // An attached value could as well be more flags to
                    // another parser: judge it conservatively.
                    if rest.chars().any(|c| cli.headless_short.contains(c)) {
                        headless = headless.or(Some(short_form));
                    }
                    if rest.is_empty() && args.get(at).is_some_and(|n| !n.starts_with('-')) {
                        at += 1;
                    }
                    break;
                }
            }
            continue;
        }
        // A positional: a subcommand, or the prompt. Options after it are
        // still parsed, so a later `--help` still prints help.
        let word = arg.as_str();
        if headless.is_some() {
            continue;
        }
        if let Some((_, form)) = cli.headless_subcommands.iter().find(|(s, _)| *s == word) {
            headless = Some(form);
            continue;
        }
        if cli.wrap_subcommands.contains(&word) {
            return if help {
                Found::Nothing
            } else {
                classify(&args[at..], depth)
            };
        }
        if cli.subcommands.contains(&word) {
            return Found::Nothing;
        }
    }
    match headless {
        Some(form) if !help => Found::Run(HeadlessRun {
            peer: cli.peer,
            form,
            parsed: true,
        }),
        _ => Found::Nothing,
    }
}

/// Where the arguments resume after long option `flag`, whose value (if it
/// takes one) starts at `at`.
fn after_long_value(cli: &Cli, flag: &str, args: &[String], mut at: usize) -> usize {
    if cli.long_value.contains(&flag) {
        at += 1;
    } else if cli.long_optional.contains(&flag) {
        if args.get(at).is_some_and(|next| !next.starts_with('-')) {
            at += 1;
        }
    } else if cli.long_variadic.contains(&flag) {
        while args.get(at).is_some_and(|next| !next.starts_with('-')) {
            at += 1;
        }
    }
    at
}

/// The words of raw text: quotes and escapes dropped, split at anything
/// that is not part of a word, a path or an `name=value` pair.
fn raw_words(text: &str) -> Vec<String> {
    let plain: String = text
        .chars()
        .filter(|c| !matches!(c, '\\' | '\'' | '"'))
        .collect();
    plain
        .split(|c: char| {
            !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '+' | '-' | '='))
        })
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The program a raw word may name: the value of a `name=value` word
/// (`CMD=codex`, `peer=claude`), as a basename.
fn raw_name(word: &str) -> String {
    basename(word.rsplit('=').next().unwrap_or(word))
}

/// The raw-text judgement for a line that could not be resolved. Within
/// each command of the text (split at `;`, `|`, `&` and newlines), a peer
/// name runs headless when the words after it read as its CLI parses
/// them (see [`raw_marker`]). A later positional word is never the
/// subcommand (TSK-223), so prose such as "Codex adversarial seat: please
/// review" is no run.
pub(crate) fn raw_run(command: &str) -> Option<HeadlessRun> {
    command.split(['\n', ';', '|', '&']).find_map(|chunk| {
        let words = raw_words(chunk);
        words.iter().enumerate().find_map(|(at, word)| {
            let cli = match raw_name(word).as_str() {
                "claude" => &CLAUDE,
                "codex" => &CODEX,
                "grok" => &GROK,
                _ => return None,
            };
            raw_marker(cli, &words[at + 1..]).map(|form| HeadlessRun {
                peer: cli.peer,
                form,
                parsed: false,
            })
        })
    })
}

/// The headless form the words after a peer name start, read the way its
/// CLI parses them. The headless subcommand counts only as the first
/// positional word; a headless flag counts before or after it, as the
/// CLIs read flags anywhere. An option the grammar does not know may take
/// the next word as its value, unless that word is the headless
/// subcommand. Help is not honoured here: raw text is judged
/// conservatively.
fn raw_marker(cli: &Cli, words: &[String]) -> Option<&'static str> {
    let short_form = cli.headless_long.first().map_or("", |(_, form)| *form);
    let subcommand = |word: &str| {
        cli.headless_subcommands
            .iter()
            .find(|(sub, _)| *sub == word)
            .map(|(_, form)| *form)
    };
    let mut positional = false;
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at += 1;
        if word == "--" {
            return None;
        }
        // Whether this option takes the next word as its value.
        let mut value = false;
        let mut known = false;
        if word.starts_with("--") {
            let (flag, inline) = word
                .split_once('=')
                .map_or((word.as_str(), false), |(flag, _)| (flag, true));
            if let Some((_, form)) = cli.headless_long.iter().find(|(long, _)| *long == flag) {
                return Some(form);
            }
            // An inline value (`--model=demo`) takes no further word.
            known = inline || cli.long_value.contains(&flag);
            value = !inline && known;
        } else if let Some(cluster) = word.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (index, flag) in cluster.char_indices() {
                if cli.headless_short.contains(flag) {
                    return Some(short_form);
                }
                if cli.short_value.contains(flag) {
                    known = true;
                    value = cluster[index + flag.len_utf8()..].is_empty();
                    break;
                }
            }
            known = known
                || cluster
                    .chars()
                    .all(|flag| cli.short_optional.contains(flag));
        } else {
            if !positional {
                if let Some(form) = subcommand(word) {
                    return Some(form);
                }
            }
            positional = true;
            continue;
        }
        let next = words.get(at).filter(|next| !next.starts_with('-'));
        if value {
            at += 1;
        } else if let Some(next) = next.filter(|_| !known && !positional) {
            // An unknown option: its value, unless it is the subcommand.
            if let Some(form) = subcommand(next) {
                return Some(form);
            }
            at += 1;
        }
    }
    None
}

/// The program name: its last path component, without `.exe`, lower case.
fn basename(program: &str) -> String {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = name.to_ascii_lowercase();
    name.strip_suffix(".exe")
        .map_or(name.clone(), str::to_string)
}

/// `NAME=value` before a program.
fn is_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::super::guard_forms::{HELP_PAIRS, PACKAGE_RUNNER_PAIRS};
    use super::*;

    fn found(command: &str) -> Option<HeadlessRun> {
        headless_peer_run(command)
    }

    /// Every probe of the TSK-136 review round 1, with its expected verdict,
    /// and the forms each CLI's help documents.
    #[test]
    fn headless_runs_are_found_however_they_are_spelled() {
        for command in [
            "claude -p x",
            "codex exec x",
            "grok -p x",
            "'claude' -p x",
            "cl\"au\"de -p x",
            r"clau\de -p x",
            "$(printf claude) -p x",
            "/usr/local/bin/claude -p x",
            "alias peer='claude -p'\npeer x",
            "shopt -s expand_aliases\nalias peer='claude -p'\npeer x",
            "peer() { claude -p x; }; peer",
            "printf x | xargs claude -p",
            r"find . -maxdepth 0 -exec claude -p x \;",
            "timeout 5 claude -p x",
            "nice -n 1 claude -p x",
            "time claude -p x",
            "command claude -p x",
            "exec claude -p x",
            "bash -lc 'claude -p x'",
            "env bash -lc 'echo harmless; claude -p x'",
            "env bash -c 'echo harmless; claude -p x'",
            "bash <<< 'claude -p x'",
            "for x in one; do claude -p x; done",
            "while false; do claude -p x; done",
            "while true; do claude -p x; break; done",
            "if true; then claude -p x; fi",
            "(claude -p x)",
            "{ claude -p x; }",
            "claude -p <<< x",
            "claude -dp x",
            "grok -px",
            "grok --single x",
            "grok --single=x",
            "grok --prompt-file prompt.txt",
            "grok --prompt-json \"[]\"",
            "codex --model demo exec x",
            "codex --model=demo exec x",
            "codex -c model=\"demo\" exec x",
            // Beyond the probes: further wrappers and documented forms.
            "! claude -p x",
            "sudo -u me claude --print x",
            "nohup nice timeout -s KILL 5 codex e x &",
            "stdbuf -o0 grok agent",
            "grok wrap claude -p x",
            "codex review",
            "claude ultrareview",
            "claude --model opus 'hi' --print",
            "claude --tools Bash Edit -p x",
            "echo 'claude -p x' | bash",
            "CLAUDE=1 env -u X FOO=2 claude -cp x",
            "eval 'codex exec x'",
            "sh -c \"$(echo x); grok --single x\"",
        ] {
            assert!(found(command).is_some(), "{command}");
        }
    }

    #[test]
    fn other_commands_are_not_runs() {
        for command in [
            "claude --help",
            "codex login",
            "grok --version",
            "claude -- -p",
            "grok -- -p",
            "codex -- exec",
            "claude --system-prompt \"-p\"",
            "rg -p pattern",
            "cat -- -p",
            "git commit -m 'claude -p x'",
            "printf '%s' 'claude -p x'",
            // Beyond the probes.
            "codex --version",
            "codex",
            "codex resume --last",
            "codex -p review",
            "codex -i exec.png 'hi'",
            "claude",
            "claude --model opus --effort high",
            "claude -n p",
            "claude --resume abc 'x'",
            "claude mcp list -p",
            "grok -m p",
            "grok --rules '-p' 'fix it'",
            "grok models",
            "grep -rn \"codex exec\" docs",
            "echo 'claude -p'",
            "cargo test -p codeflow-core",
            "find . -name claude -print",
            "nice -n 5 cargo build",
        ] {
            assert_eq!(found(command), None, "{command}");
        }
    }

    #[test]
    fn a_help_or_version_invocation_is_not_a_run_and_its_data_twin_is() {
        for (help, twin) in HELP_PAIRS {
            assert_eq!(found(help), None, "{help}");
            assert!(found(twin).is_some(), "{twin}");
        }
    }

    #[test]
    fn unresolved_lines_say_they_were_not_parsed() {
        let run = found("alias peer='claude -p'\npeer x").unwrap();
        assert!(!run.parsed);
        assert_eq!(run.peer, "claude");
        let run = found("grok --single=x").unwrap();
        assert!(run.parsed);
        assert_eq!(run.form, "grok -p");
        assert_eq!(found("codex review").unwrap().form, "codex review");
        // An unresolved line without a peer marker is not a run.
        assert_eq!(found("$(printf ls) -la"), None);
        assert_eq!(found("bash <<< 'echo claude'"), None);
    }

    /// TSK-223 AC-3 (sathyassn/codeflow#52): text that only names a peer,
    /// in a heredoc body, a commit message or other quoted data, is no run,
    /// even on a line whose program is a variable or substitution or that
    /// feeds a here-string to a reader. Each was refused before the change.
    #[test]
    fn text_that_only_names_a_peer_is_not_a_run() {
        for command in [
            // The report's brief, beside a variable program.
            "D=$PWD; cat > brief.md <<EOF\nCodex adversarial seat: please review $D/page.html\nEOF\n$EDITOR brief.md",
            "cat > brief.md <<EOF\nThe Codex review found two issues.\nEOF\n\"$EDITOR\" brief.md",
            // A commit message naming a model, beside a variable program.
            "$EDITOR notes.md; git commit -m 'docs: record the Codex review'",
            "M=$(date); $PAGER notes.md; git commit -m \"docs: record the Codex review $M\"",
            "$(git rev-parse --show-toplevel)/check.sh && git commit -m 'fix: apply the Claude -p finding'",
            // A here-string fed to a reader.
            "grep -c review <<< 'Codex review: approve'",
            // Prose in raw text names the peer, then the marker later on.
            "bash run.sh; echo 'Codex adversarial seat: please review the page'",
            "source .venv/bin/activate && echo 'Grok, the agent seat, and Claude will review it'",
        ] {
            assert_eq!(found(command), None, "{command}");
        }
    }

    /// TSK-223 AC-4: the headless runs an unresolved line can still start
    /// stay refused.
    #[test]
    fn unresolved_lines_that_run_a_peer_are_still_runs() {
        for command in [
            "CMD=codex; $CMD exec x",
            "export P=$(command -v codex); $P exec x",
            "$SUDO codex exec x",
            "CMD=codex; timeout 5 $CMD --model demo exec x",
            "$(printf claude) -p x",
            "alias peer=codex\npeer exec x",
            "alias peer='claude -p'",
            "bash <<< \"codex exec x\"",
            "python3 <<< 'import os; os.system(\"codex exec x\")'",
            "$RUN <<< 'codex exec x'",
            "echo 'codex exec x' | bash",
            "echo 'codex exec x' | $SHELL",
            "printf 'grok -p x' | \"$SH\" -s",
            "cat > run.sh <<EOF\ncodex exec x\nEOF\nbash run.sh",
            "cat > run.sh <<EOF\nclaude --print hi\nEOF\nsource run.sh",
            "function peer { claude -p x; }; peer",
            "CMD=\"codex exec\"; $CMD x",
            "export RUN='claude -p'; $RUN hi",
            "$(echo codex exec) x",
            "`printf 'grok agent'`",
            "bash <<< 'claude \"hi\" --print'",
            "echo 'grok \"hi\" -p' | sh",
            "echo 'codex --new-flag value exec x' | bash",
            "echo 'codex --full-auto exec x' | bash",
        ] {
            assert!(found(command).is_some(), "{command}");
        }
    }

    /// TSK-223 AC-3: the raw text reads a peer the way its CLI parses:
    /// the headless flag among the options before the first positional
    /// word, or the headless subcommand as that word, never a later word.
    #[test]
    fn the_raw_text_reads_a_peer_the_way_its_cli_parses() {
        for (text, form) in [
            ("codex exec x", Some("codex exec")),
            ("codex --model demo exec x", Some("codex exec")),
            ("codex -m demo review", Some("codex review")),
            ("codex e", Some("codex exec")),
            ("claude -p hi", Some("claude -p")),
            ("claude --model opus --print hi", Some("claude -p")),
            ("claude -dp hi", Some("claude -p")),
            ("claude ultrareview", Some("claude ultrareview")),
            ("grok -px", Some("grok -p")),
            ("grok --single=x", Some("grok -p")),
            ("grok --prompt-file p.txt", Some("grok --prompt-file")),
            ("grok agent", Some("grok agent")),
            ("Codex adversarial seat: please review", None),
            ("codex then exec", None),
            ("claude said -p", Some("claude -p")),
            ("Claude, the review seat, approves", None),
            ("codex --full-auto exec x", Some("codex exec")),
            ("codex --new-flag value exec x", Some("codex exec")),
            ("codex -c model=demo exec x", Some("codex exec")),
            ("codex --model=demo exec x", Some("codex exec")),
            ("CMD=codex exec", Some("codex exec")),
            ("grok models agent", None),
            ("codex -- exec", None),
            ("codex; exec ls", None),
            ("the Codex review", Some("codex review")),
        ] {
            assert_eq!(raw_run(text).map(|run| run.form), form, "{text}");
        }
    }

    /// TSK-141 AC-6: a peer started through `npx`, `bunx`, `pnpm dlx` or
    /// `yarn dlx` is judged as its direct invocation, headless run and help
    /// alike.
    #[test]
    fn a_package_runner_is_judged_as_its_direct_invocation() {
        for (direct, runner) in PACKAGE_RUNNER_PAIRS {
            assert_eq!(
                headless_peer_run(runner),
                headless_peer_run(direct),
                "{runner} as {direct}"
            );
        }
        assert_eq!(package_bin("@anthropic-ai/claude-code@2.1.283"), "claude");
        assert_eq!(package_bin("@openai/codex"), "codex");
        assert_eq!(package_bin("grok@latest"), "grok");
        // A runner of anything else is no peer run.
        assert_eq!(headless_peer_run("npx prettier -p x"), None);
        assert_eq!(headless_peer_run("pnpm install -p"), None);
    }
}
