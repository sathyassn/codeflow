//! Headless peer runs (TSK-136): Claude Code, Codex or Grok run one-shot,
//! without their interactive session.
//!
//! Peer seats run interactively, never headless (cf-delegate): a one-shot
//! headless run has no verified native session, no task tools or guards, and
//! no recheckable thread. This classifier finds such a run in a shell command
//! so exec-guard can warn (the default) or refuse.
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
//! after `--` is data. A line whose commands cannot be
//! resolved (a here-string, a substitution or variable as the program, an
//! alias, a shell reading its script from stdin) is judged on its raw text:
//! a peer name followed by one of its headless markers is flagged. That can
//! over-flag an unusual line, which the default `warn` level tolerates; the
//! message says the line was not fully parsed.

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
            // A package runner starts the package's binary (TSK-141 AC-6).
            "npx" | "bunx" | "pnpx" => return package_run(rest, depth),
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
/// kept; a help or version option ends the run.
fn after_runner_options(args: &[String]) -> Vec<usize> {
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
                stack.push(at + 2);
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

/// A run over an unresolved command over nothing.
fn strongest(a: Found, b: Found) -> Found {
    match (a, b) {
        (Found::Run(run), _) | (_, Found::Run(run)) => Found::Run(run),
        (Found::Unresolved, _) | (_, Found::Unresolved) => Found::Unresolved,
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
                    stack.push((at + 2, package_given));
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
fn package_bin(spec: &str) -> String {
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
