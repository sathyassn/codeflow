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
//! are never read as the headless flag. A line whose commands cannot be
//! resolved (a here-string, a substitution or variable as the program, an
//! alias, a shell reading its script from stdin) is judged on its raw text:
//! a peer name followed by one of its headless markers is flagged. That can
//! over-flag an unusual line, which the default `block` level then refuses;
//! the message says the line was not fully parsed, and a project that needs
//! such a run sets `security.headless_peer_runs` to `warn`.

use crate::hooks::git_guard::simple_commands;

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
    Nothing,
}

fn find(command: &str, depth: usize) -> Option<HeadlessRun> {
    if depth > 4 {
        return raw_run(command);
    }
    let mut unresolved = command.contains("<<<");
    for argv in simple_commands(command) {
        match classify(&argv, depth) {
            Found::Run(run) => return Some(run),
            Found::Unresolved => unresolved = true,
            Found::Nothing => {}
        }
    }
    if unresolved {
        raw_run(command)
    } else {
        None
    }
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
            return Found::Unresolved;
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
            _ => return Found::Nothing,
        };
    }
    Found::Unresolved
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
            Found::Unresolved => found = Found::Unresolved,
            Found::Nothing => {}
        }
        rest = &body[end..];
    }
    found
}

/// Skip a wrapper's options (those in `with_value` take the next token) to
/// the command it runs.
fn skip_options<'a>(args: &'a [String], with_value: &[&str]) -> &'a [String] {
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
fn skip_assignments(args: &[String]) -> &[String] {
    let at = args
        .iter()
        .position(|arg| !is_assignment(arg))
        .unwrap_or(args.len());
    &args[at..]
}

/// `env`'s options and assignments up to its command; `None` for `env -S`,
/// which splits a string this classifier does not model.
fn skip_env(args: &[String]) -> Option<&[String]> {
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
fn shell_command_string(args: &[String]) -> Option<&str> {
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
    /// Boolean short flags that mean a headless run.
    headless_short: &'static str,
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
    headless_long: &[],
    short_value: "cimpsCa",
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
    headless_long: &[
        ("--single", "grok -p"),
        ("--prompt-file", "grok --prompt-file"),
        ("--prompt-json", "grok --prompt-json"),
    ],
    short_value: "ms",
    short_optional: "rw",
    long_value: &[
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

fn peer_run(name: &str, args: &[String], depth: usize) -> Found {
    let cli = match name {
        "claude" => &CLAUDE,
        "codex" => &CODEX,
        _ => &GROK,
    };
    let run = |form| {
        Found::Run(HeadlessRun {
            peer: cli.peer,
            form,
            parsed: true,
        })
    };
    let short_form = cli.headless_long.first().map_or("", |(_, form)| *form);
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        at += 1;
        if arg == "--" {
            return Found::Nothing; // the rest is positional
        }
        if let Some(long) = arg.strip_prefix("--").map(|_| arg.as_str()) {
            let (flag, inline) = long
                .split_once('=')
                .map_or((long, None), |(flag, value)| (flag, Some(value)));
            if let Some((_, form)) = cli.headless_long.iter().find(|(f, _)| *f == flag) {
                return run(form);
            }
            if inline.is_some() {
                continue;
            }
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
            continue;
        }
        if let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) {
            for (index, flag) in cluster.char_indices() {
                if cli.headless_short.contains(flag) {
                    return run(short_form);
                }
                let rest = &cluster[index + flag.len_utf8()..];
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
                        return run(short_form);
                    }
                    if rest.is_empty() && args.get(at).is_some_and(|n| !n.starts_with('-')) {
                        at += 1;
                    }
                    break;
                }
            }
            continue;
        }
        // A positional: a subcommand, or the prompt.
        let word = arg.as_str();
        if let Some((_, form)) = cli.headless_subcommands.iter().find(|(s, _)| *s == word) {
            return run(form);
        }
        if cli.wrap_subcommands.contains(&word) {
            return classify(&args[at..], depth);
        }
        if cli.subcommands.contains(&word) {
            return Found::Nothing;
        }
    }
    Found::Nothing
}

/// The raw-text judgement for a line that could not be resolved: a peer
/// name followed, anywhere later, by one of its headless markers.
fn raw_run(command: &str) -> Option<HeadlessRun> {
    let plain: String = command
        .chars()
        .filter(|c| !matches!(c, '\\' | '\'' | '"'))
        .collect();
    let words: Vec<&str> = plain
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '+' | '-')))
        .filter(|w| !w.is_empty())
        .collect();
    words.iter().enumerate().find_map(|(at, word)| {
        let name = basename(word);
        let later = &words[at + 1..];
        let (peer, form) = match name.as_str() {
            "claude" => later.iter().find_map(|w| {
                let short = w.strip_prefix('-').filter(|c| !c.starts_with('-'));
                (*w == "--print" || *w == "ultrareview" || short.is_some_and(|c| c.contains('p')))
                    .then_some(("claude", "claude -p"))
            })?,
            "codex" => later.iter().find_map(|w| {
                matches!(*w, "exec" | "e" | "review").then_some(("codex", "codex exec"))
            })?,
            "grok" => later.iter().find_map(|w| {
                (w.starts_with("-p")
                    || matches!(*w, "--single" | "--prompt-file" | "--prompt-json" | "agent"))
                .then_some(("grok", "grok -p"))
            })?,
            _ => return None,
        };
        Some(HeadlessRun {
            peer,
            form,
            parsed: false,
        })
    })
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
            "claude --help -p",
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
}
