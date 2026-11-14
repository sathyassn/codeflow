//! Install runs (TSK-1476): a run of the `CodeFlow` binary as `update` or
//! `init`, which writes the project's install files: harness settings,
//! hooks, policy and workflows. Only the operator runs them, so exec-guard
//! refuses such a run from an agent under `git.hook_integrity`, in every
//! folder and whichever program starts it.
//!
//! A line is judged in three steps.
//!
//! 1. A POSIX line in the closed subset [`admission`] lists is data and
//!    draws no finding: every command a listed data program (`echo`,
//!    `printf` with a format of literal text and `%s`, `cat`, `head`,
//!    `tail`, `wc`, `grep`, `rg` with `--no-config` first, and the binary by
//!    its bare name in its exact help and version shapes), each argument
//!    read by that program's own grammar and proven inert, output
//!    redirected to `/dev/null` only, input read from a named file, and no
//!    construct outside the model. No Git
//!    call is listed: a search can fetch a missing object through a
//!    transport its configuration names.
//! 2. Any other line is read for a run whose text need not hold the
//!    binary's name with a verb: a word the shell expands where a program
//!    runs, with a verb after it, or a run inside a quoted argument or an
//!    assigned value, read again as a command line.
//! 3. Every other line is judged on its raw text ([`raw_run`]): the
//!    binary's name followed, anywhere later, by a verb, or `cargo`
//!    followed by `run` or `r` and then a verb. So any line outside the
//!    subset that names the binary with a verb refuses. The same match is
//!    made on the texts the line produces ([`produced`]): its words as the
//!    shell passes them, what `printf`, `echo` and `print` write from them
//!    where that is plain literal text (any other output refuses, since
//!    the guard decodes no escape and models no conversion but `%s`), the
//!    values the shell composer resolves, and all of it joined in line
//!    order.
//!
//! The reader of step 2 reads each simple command as git-guard reads it
//! (`git_guard::simple_commands`), with the lexer's mark of whether the
//! shell passes each word as written. A word that names the binary is
//! judged with the words after it when it can be the program that runs:
//! the first word, a word after a launcher, supplier, package runner or
//! package manager, or any word of a program this reader does not know. A
//! program known never to run its operands (`cp`, `make` and the like)
//! gives no such word; `npm` is read that way only after one of its own
//! commands that never runs a program, written in full or as a documented
//! alias. `cargo run` and `cargo r` are judged by the words cargo passes to
//! the program it builds. A word the shell expands (a variable, a
//! substitution, a pattern, braces or a leading `=`) in a program position
//! or as the first operand refuses where the verb could follow. A search
//! program's option that runs a program (`ack` and `ag` `--pager`, `rg
//! --pre`), the words `gh repo clone`, `gh repo fork` and `gh gist clone`
//! pass to `git` after `--`, and an editor's own command (`vim -c`, `emacs
//! --eval`, a `+` word for vi or less) are read for the run they start.
//!
//! A program an agent builds, renames or copies, the verb's words written
//! to a file and run later, text built by a program this reader does not
//! model and fed to a process already running that executes what it reads,
//! and state the line does not show (an exported session variable, a shell
//! alias or function from a profile, a program put first on the path) are
//! not on the line. They are named residuals.

mod admission;
mod produced;

pub(crate) use produced::UNSETTLED;

use super::headless::basename;
use super::launchers::{LAUNCHERS, PACKAGE_RUNNERS, SUPPLIERS};
use crate::hooks::git_guard::{
    command_argv_literal, expand_commands, shell_blank, strip_launchers, strip_reserved_words,
};

/// The binary's program name.
const BINARY: &str = "codeflow";

/// The verbs that write the project's install files.
const VERBS: &[&str] = &["update", "init"];

/// The words after the binary's name that only print help or the version,
/// in their exact shapes. Any other word on the call, an option's value
/// included, makes it a run.
const PRINTS_ONLY: &[&[&str]] = &[
    &["update", "--help"],
    &["update", "-h"],
    &["init", "--help"],
    &["init", "-h"],
    &["help", "update"],
    &["help", "init"],
    &["help"],
    &["--help"],
    &["-h"],
    &["--version"],
    &["-V"],
];

/// Programs the reader of step 2 reads only for the program an option of
/// theirs runs ([`RUNS_THROUGH_OPTION`]) or, for `gh`, the words it passes
/// to `git`: their other words are judged on the line's raw text.
const DATA_PROGRAMS: &[&str] = &[
    "[", "ack", "ag", "cat", "echo", "gh", "git", "grep", "head", "jq", "ls", "printf", "rg",
    "tail", "test", "type", "wc", "which",
];

/// How a search program runs the value of an option that runs a program.
#[derive(Clone, Copy)]
enum OptionValue {
    /// A command line, handed to a shell or a command pipe: `ack --pager`
    /// (Perl's pipe `open`) and `ag --pager` (`popen`).
    CommandLine,
    /// One program path, run with each searched path as its argument and
    /// the file's content on its input: `rg --pre`.
    ProgramPath,
}

/// Options of a [`DATA_PROGRAMS`] search program that run a program, with
/// how each runs its value. An option is matched by any prefix of its
/// name, since `ack` (Perl's `Getopt::Long`) and `ag` (`getopt_long`)
/// accept a unique abbreviation; the value is attached after `=` or is the
/// next word.
const RUNS_THROUGH_OPTION: &[(&str, OptionValue)] = &[
    ("--pager", OptionValue::CommandLine),
    ("--pre", OptionValue::ProgramPath),
];

/// The `gh` commands that pass the words after their `--` to the `git
/// clone` they run, as gh's source declares them: `repo clone <repository>
/// [<directory>] [-- <gitflags>...]`, `repo fork [<repository>] [--
/// <gitflags>...]` (used when it clones) and `gist clone <gist>
/// [<directory>] [-- <gitflags>...]`. Each is its two words joined by one
/// space.
const GH_GIT_COMMANDS: &[&str] = &["gist clone", "repo clone", "repo fork"];

/// Editors and a pager whose own command language runs a shell command
/// (`!` in vi and less, `shell-command` in emacs). Their command options'
/// values and their `+` words are judged on their raw text, as
/// interpreter code is.
const EDITORS: &[&str] = &[
    "emacs", "ex", "less", "nvim", "rview", "rvim", "vi", "view", "vim",
];

/// The options of [`EDITORS`] whose value is a command in the editor's
/// own language.
const EDITOR_COMMAND_OPTIONS: &[&str] = &["-c", "--cmd", "--eval", "-eval", "--execute"];

/// Programs known never to run one of their operands as a program, so a
/// folder or file named like the binary among their words is no program
/// position. A quoted argument holding a space is still read as a command
/// line, since a value such as a `make` variable can be. `npm` is read
/// this way only after one of [`NPM_OWN_COMMANDS`]; after any other word
/// it runs a package's binary, as [`PACKAGE_MANAGERS`] do.
const NOT_LAUNCHERS: &[&str] = &[
    "chmod", "chown", "cmp", "cp", "cut", "diff", "du", "file", "ln", "make", "mkdir", "mv", "npm",
    "rm", "rmdir", "sort", "stat", "tar", "tee", "touch", "tr", "uniq",
];

/// Package and tool managers that run a package's or a tool's binary by
/// name with the words after it, with or without a subcommand such as
/// `run`, `exec`, `dlx` or `x` (`yarn <name>`, `pnpm run <name>`,
/// `bun x <name>`, `corepack <manager>`, `volta run <name>`, `uv run
/// <name>`, `uvx <name>`). Every word after one is read as a program
/// position, whatever subcommand comes first, so no alias or abbreviation
/// of a subcommand moves a word out of it. `pn` is pnpm's short name.
const PACKAGE_MANAGERS: &[&str] = &[
    "bun", "corepack", "deno", "pn", "pnpm", "uv", "uvx", "volta", "yarn",
];

/// npm's own commands, in their exact names and documented aliases, that
/// never run a program named on the command line: they install, list,
/// show, publish or configure packages. The names are npm's `commands` and
/// `aliases` (`lib/utils/cmd-list.js` in the npm/cli repository). npm also
/// accepts any unambiguous abbreviation of a name (`deref` there, through
/// `abbrev`), so `exe` runs `exec`; this list holds exact words only, and
/// any other word, an abbreviation included, leaves npm read as a package
/// runner. Left out: the commands that run a program, a package's binary
/// or a script (`exec`, `explore`, `init`, `run`, `start`, `stop`,
/// `restart`, `test`, `install-test`, `install-ci-test`), those that open
/// an editor, browser or viewer (`edit`, `config` and its alias `c`, whose
/// `edit` opens one, `docs`, `bugs`, `repo`, `help`, `fund`), and those
/// whose effect this reader has not established
/// (`approve-scripts`, `deny-scripts`, `install-scripts`, `patch`,
/// `stage`, `trust`, `help-search`).
const NPM_OWN_COMMANDS: &[&str] = &[
    // Commands.
    "access",
    "audit",
    "cache",
    "ci",
    "completion",
    "dedupe",
    "deprecate",
    "diff",
    "dist-tag",
    "doctor",
    "explain",
    "find-dupes",
    "get",
    "install",
    "link",
    "ll",
    "login",
    "logout",
    "ls",
    "org",
    "outdated",
    "owner",
    "pack",
    "ping",
    "pkg",
    "prefix",
    "profile",
    "prune",
    "publish",
    "query",
    "rebuild",
    "root",
    "sbom",
    "search",
    "set",
    "team",
    "token",
    "undeprecate",
    "uninstall",
    "unpublish",
    "update",
    "version",
    "view",
    "whoami",
    // Their aliases.
    "add",
    "author",
    "clean-install",
    "ddp",
    "dist-tags",
    "find",
    "i",
    "ic",
    "in",
    "info",
    "ins",
    "inst",
    "insta",
    "instal",
    "install-clean",
    "isnt",
    "isnta",
    "isntal",
    "isntall",
    "isntall-clean",
    "la",
    "list",
    "ln",
    "ogr",
    "r",
    "rb",
    "remove",
    "rm",
    "s",
    "se",
    "show",
    "u",
    "udpate",
    "un",
    "unlink",
    "up",
    "upgrade",
    "v",
    "verison",
    "why",
];

/// npm options before its command that take the next word as their value.
const NPM_VALUE_OPTIONS: &[&str] = &[
    "-C",
    "-w",
    "--cache",
    "--globalconfig",
    "--loglevel",
    "--prefix",
    "--registry",
    "--userconfig",
    "--workspace",
];

/// npm options before its command that take no value.
const NPM_FLAGS: &[&str] = &[
    "-d",
    "-f",
    "-g",
    "-q",
    "-s",
    "-ws",
    "-y",
    "--dry-run",
    "--force",
    "--global",
    "--json",
    "--no-audit",
    "--no-fund",
    "--offline",
    "--quiet",
    "--silent",
    "--verbose",
    "--workspaces",
    "--yes",
];

/// Words after which a supplier or package manager starts the command
/// that follows.
const STARTS_A_COMMAND: &[&str] = &[
    "-exec", "-execdir", "-ok", "-okdir", "exec", "explore", "x", "dlx",
];

/// Suppliers that add the words they read after the command they run.
const APPENDING: &[&str] = &["parallel", "xargs"];

/// cargo's options, before or after its subcommand, that take the next
/// word as their value.
const CARGO_VALUE_OPTIONS: &[&str] = &[
    "-C",
    "-F",
    "-Z",
    "-j",
    "-p",
    "--bench",
    "--bin",
    "--color",
    "--config",
    "--example",
    "--features",
    "--jobs",
    "--lockfile-path",
    "--manifest-path",
    "--message-format",
    "--package",
    "--profile",
    "--target",
    "--target-dir",
    "--test",
];

/// An install run found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRun {
    /// The form matched, such as `codeflow update` or `cargo run`.
    pub form: &'static str,
    /// `false` when the line was judged on its raw text, which named the
    /// binary with a verb.
    pub parsed: bool,
    /// `true` when the run was found inside a quoted argument or an
    /// assignment's value, which the guard read as a command line.
    pub in_argument: bool,
}

/// The first install run in `command`, if any. `posix` is false for a
/// shell that reads a backslash as a path separator, such as `pwsh`, where
/// the command is also read with each backslash as a `/`; no line of such
/// a shell is admitted as data.
#[must_use]
pub fn install_run(command: &str, posix: bool) -> Option<InstallRun> {
    if posix && admission::admitted(command) {
        return None;
    }
    let slashed = (!posix && command.contains('\\')).then(|| command.replace('\\', "/"));
    find(command, 0)
        .or_else(|| slashed.as_deref().and_then(|text| find(text, 0)))
        .or_else(|| raw_run(command))
        .or_else(|| slashed.as_deref().and_then(raw_run))
        .or_else(|| produced::produced_run(command))
}

/// One simple command: the argument vector from the program on, after
/// every launcher, with whether the shell passes each word as written, and
/// every word as written.
struct Simple {
    argv: Vec<String>,
    literal: Vec<bool>,
    words: Vec<String>,
}

/// The simple commands of `command`, read as
/// `git_guard::simple_commands` reads them, with each word's literal mark.
fn simple(command: &str) -> Vec<Simple> {
    expand_commands(command)
        .iter()
        .map(|segment| {
            let (mut words, mut literal) = command_argv_literal(segment);
            let all = words.clone();
            let total = words.len();
            strip_reserved_words(&mut words);
            literal.drain(..total - words.len());
            let start = strip_launchers(&words)
                .map_or(words.len(), |(_, args)| words.len() - args.len() - 1);
            Simple {
                argv: words[start..].to_vec(),
                literal: literal[start..].to_vec(),
                words: all,
            }
        })
        .collect()
}

/// The name and value of a `NAME=value` word.
fn assigned(word: &str) -> Option<(String, String)> {
    let (name, value) = word.split_once('=')?;
    identifier(name).then(|| (name.to_string(), value.to_string()))
}

fn identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The first run in `command` the reader of step 2 sees: in each simple
/// command, the value of each of its `NAME=value` words, read as a command
/// line, then its argument vector. Past a depth of four nested values the
/// text is judged raw.
fn find(command: &str, depth: usize) -> Option<InstallRun> {
    if depth > 4 {
        return raw_run(command);
    }
    simple(command).iter().find_map(|command| {
        command
            .words
            .iter()
            .filter_map(|word| assigned(word))
            .find_map(|(_, value)| in_value(&value, depth))
            .or_else(|| classify(&command.argv, &command.literal, depth))
    })
}

/// A run inside `value`, a quoted argument or an assigned value read as a
/// command line, or after its own `=` when the text before that holds no
/// blank (`--checkpoint-action=exec=...`).
fn in_value(value: &str, depth: usize) -> Option<InstallRun> {
    find(value, depth + 1)
        .or_else(|| {
            let (head, tail) = value.split_once('=')?;
            if head.contains(shell_blank) {
                None
            } else {
                in_value(tail, depth + 1)
            }
        })
        .map(|run| InstallRun {
            in_argument: true,
            ..run
        })
}

/// The run one simple command's argument vector starts, with whether the
/// shell passes each word as written.
fn classify(argv: &[String], literal: &[bool], depth: usize) -> Option<InstallRun> {
    let program = argv.first()?;
    let written = |at: usize| literal.get(at).copied().unwrap_or(false) && !unresolved(&argv[at]);
    let name = basename(program);
    if DATA_PROGRAMS.contains(&name.as_str()) {
        if name == "gh" {
            if let Some(run) = gh_git_arguments(argv, depth) {
                return Some(run);
            }
        }
        return runs_through_option(argv, &written, depth);
    }
    if EDITORS.contains(&name.as_str()) {
        if let Some(run) = editor_command(argv) {
            return Some(run);
        }
    }
    let known = NOT_LAUNCHERS.contains(&name.as_str());
    let runner_from = package_manager_runs_from(argv, &written);
    for (at, word) in argv.iter().enumerate() {
        let after = &argv[at + 1..];
        let before = &argv[..at];
        let in_program_position =
            at == 0 || starts_a_command(before) || runner_from.is_some_and(|from| at >= from);
        let may_run = in_program_position || !known;
        let open = |at: usize| is_verb(&argv[at]) || !written(at) || supplier_slot(&argv[at]);
        let form = match program_word(word, written(at)).as_str() {
            BINARY if may_run && runs_a_verb(at + 1, argv, &open, supplied(before)) => {
                Some(binary_form(after))
            }
            "cargo" if may_run && cargo_runs_a_verb(at + 1, argv, &open) => Some("cargo run"),
            _ if (word == "{}" || !written(at))
                && in_program_position
                && operand_at(at + 1, argv).is_some_and(|operand| is_verb(&argv[operand])) =>
            {
                Some("an unresolved program with an install verb")
            }
            // A word the shell expands where a program this reader does
            // not know may run it: judged with the words after it on their
            // raw text, as a literal name there would be.
            _ if may_run
                && !in_program_position
                && !written(at)
                && raw_run(&argv[at..].join(" ")).is_some() =>
            {
                Some("an expanded word naming the binary with an install verb")
            }
            _ => None,
        };
        if let Some(form) = form {
            return Some(InstallRun {
                form,
                parsed: true,
                in_argument: false,
            });
        }
        if at > 0 && word.contains(shell_blank) {
            if let Some(run) = in_value(word, depth) {
                return Some(run);
            }
        }
    }
    None
}

/// A search program run with an option that runs a program
/// ([`RUNS_THROUGH_OPTION`]): the option's value is read for the run it
/// starts, and a value the shell expands is a run.
fn runs_through_option(
    argv: &[String],
    written: &dyn Fn(usize) -> bool,
    depth: usize,
) -> Option<InstallRun> {
    for (at, word) in argv.iter().enumerate().skip(1) {
        let Some(name) = word.strip_prefix("--") else {
            continue;
        };
        let (name, attached) = match name.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (name, None),
        };
        if name.is_empty() {
            continue;
        }
        for &(option, runs) in RUNS_THROUGH_OPTION {
            if !option[2..].starts_with(name) {
                continue;
            }
            let (value, value_at) = match (attached, argv.get(at + 1)) {
                (Some(value), _) => (value, at),
                (None, Some(value)) => (value.as_str(), at + 1),
                (None, None) => continue,
            };
            if !written(value_at) {
                return Some(InstallRun {
                    form: "codeflow with a supplied verb",
                    parsed: true,
                    in_argument: false,
                });
            }
            let run = match runs {
                OptionValue::CommandLine => in_value(value, depth),
                OptionValue::ProgramPath => {
                    program_path_value(value, argv, value_at + 1, written, depth)
                }
            };
            if run.is_some() {
                return run;
            }
        }
    }
    None
}

/// The words after `--` that a [`GH_GIT_COMMANDS`] command passes to the
/// `git clone` it runs: each one holding a blank, such as a `-c` setting or
/// an `--upload-pack` value, is read as a quoted argument is. The command
/// is the first two words of `argv` after the program that are not
/// options.
fn gh_git_arguments(argv: &[String], depth: usize) -> Option<InstallRun> {
    let start = argv.iter().position(|word| word == "--")?;
    let mut words = argv[1..start].iter().filter(|word| !word.starts_with('-'));
    let command = format!("{} {}", words.next()?, words.next()?);
    if !GH_GIT_COMMANDS.contains(&command.as_str()) {
        return None;
    }
    argv[start + 1..]
        .iter()
        .filter(|word| word.contains(shell_blank))
        .find_map(|word| in_value(word, depth))
}

/// A program path an option's value names, run with each word of `argv`
/// from `from` on that is not an option as its argument, so a supplier
/// there adds words of its own. A value naming the binary is a run
/// whatever it is given.
fn program_path_value(
    value: &str,
    argv: &[String],
    from: usize,
    written: &dyn Fn(usize) -> bool,
    depth: usize,
) -> Option<InstallRun> {
    let first = value
        .split(shell_blank)
        .find(|part| !part.is_empty())
        .unwrap_or(value);
    if basename(first) == BINARY {
        return Some(InstallRun {
            form: "codeflow with a supplied verb",
            parsed: true,
            in_argument: false,
        });
    }
    argv.iter()
        .enumerate()
        .skip(from)
        .filter(|(_, operand)| !operand.starts_with('-') && !operand.contains(shell_blank))
        .find_map(|(at, operand)| {
            let words = [value.to_string(), operand.clone()];
            let marks = [true, written(at)];
            classify(&words, &marks, depth + 1)
        })
}

/// A command in an editor's own language ([`EDITORS`]) that names the
/// binary with a verb: an [`EDITOR_COMMAND_OPTIONS`] value, attached after
/// `=` or as the next word, or a `+` word, judged on its raw text.
fn editor_command(argv: &[String]) -> Option<InstallRun> {
    argv.iter().enumerate().skip(1).find_map(|(at, word)| {
        let command =
            if word.starts_with('+') || EDITOR_COMMAND_OPTIONS.contains(&argv[at - 1].as_str()) {
                Some(word.as_str())
            } else {
                word.split_once('=')
                    .filter(|(option, _)| EDITOR_COMMAND_OPTIONS.contains(option))
                    .map(|(_, value)| value)
            };
        command.and_then(raw_run).map(|run| InstallRun {
            in_argument: true,
            ..run
        })
    })
}

/// The name a program word runs as: its base name, with zsh's leading `=`
/// removed from a word the shell expands.
fn program_word(word: &str, written: bool) -> String {
    if written {
        basename(word)
    } else {
        basename(word.strip_prefix('=').unwrap_or(word))
    }
}

/// Whether the words of `argv` from `from` on, given to the binary, run a
/// verb. `supplied` is true when a supplier before it adds words of its own
/// after them. `open` marks a word that is a verb or may become one.
fn runs_a_verb(from: usize, argv: &[String], open: &dyn Fn(usize) -> bool, supplied: bool) -> bool {
    let start = operand_at(from, argv).unwrap_or(argv.len());
    let rest = &argv[start..];
    if !supplied
        && PRINTS_ONLY
            .iter()
            .any(|shape| shape.len() == rest.len() && shape.iter().zip(rest).all(|(a, b)| a == b))
    {
        return false;
    }
    match rest.first() {
        None => supplied,
        Some(_) if open(start) => true,
        Some(first) if first.starts_with('-') => (start + 1..argv.len()).any(open),
        Some(_) => supplied && (start..argv.len()).any(open),
    }
}

/// `cargo run` or `cargo r`, read from `from` in `argv`: true when a word
/// cargo passes to the program it builds is a verb or may become one. When
/// the program is the binary, by `--bin codeflow` or its package, the
/// passed words are judged as the binary's own. A subcommand the reader
/// cannot resolve is judged as `run`.
fn cargo_runs_a_verb(from: usize, argv: &[String], open: &dyn Fn(usize) -> bool) -> bool {
    let mut at = from;
    // The subcommand: the first word that is not an option, an option's
    // value or a toolchain.
    let subcommand = loop {
        let Some(word) = argv.get(at) else {
            return false;
        };
        if CARGO_VALUE_OPTIONS.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') || word.starts_with('+') {
            at += 1;
        } else {
            break at;
        }
    };
    let resolved = !open(subcommand) || is_verb(&argv[subcommand]);
    if resolved && !matches!(argv[subcommand].as_str(), "run" | "r") {
        return false;
    }
    let mut passed = Vec::new();
    let mut binary = false;
    let mut at = subcommand + 1;
    while let Some(word) = argv.get(at) {
        if word == "--" {
            passed.extend(at + 1..argv.len());
            break;
        }
        if CARGO_VALUE_OPTIONS.contains(&word.as_str()) {
            if let Some(value) = argv.get(at + 1) {
                binary |= matches!(word.as_str(), "--bin" | "-p" | "--package")
                    && matches!(value.as_str(), "codeflow" | "codeflow-cli");
            }
            at += 2;
            continue;
        }
        if word.starts_with('-') {
            binary |= matches!(
                word.as_str(),
                "--bin=codeflow" | "--package=codeflow-cli" | "-pcodeflow-cli"
            );
        } else {
            passed.push(at);
        }
        at += 1;
    }
    if !resolved {
        return passed.iter().any(|&at| open(at)) || (subcommand + 1..argv.len()).any(open);
    }
    if binary {
        let words: Vec<String> = passed.iter().map(|&at| argv[at].clone()).collect();
        let marks: Vec<bool> = passed.iter().map(|&at| open(at)).collect();
        let open_passed = |at: usize| marks[at];
        return runs_a_verb(0, &words, &open_passed, false);
    }
    passed.iter().any(|&at| open(at))
}

/// The raw-text form of [`cargo_runs_a_verb`] and the binary's run: the
/// verb anywhere among `words`.
fn names_a_verb(words: &[&str]) -> bool {
    words.iter().any(|word| is_verb(word))
}

/// The form reported for a run of the binary.
fn binary_form(after: &[String]) -> &'static str {
    match first_operand(after).map(String::as_str) {
        Some("update") => "codeflow update",
        Some("init") => "codeflow init",
        _ => "codeflow with an unresolved or supplied verb",
    }
}

/// The index of the first word of `argv` from `from` on after any leading
/// `--`.
fn operand_at(from: usize, argv: &[String]) -> Option<usize> {
    (from..argv.len()).find(|&at| argv[at] != "--")
}

/// The first word after any leading `--`.
fn first_operand(words: &[String]) -> Option<&String> {
    words.iter().find(|word| *word != "--")
}

fn is_verb(word: &str) -> bool {
    VERBS.contains(&word)
}

/// A word whose value the guard cannot read: a variable, a substitution,
/// which git-guard's reader marks with a control character, or a backtick.
fn unresolved(word: &str) -> bool {
    word.contains(['$', '`']) || word.chars().any(char::is_control)
}

/// A word a supplier fills in (`{}`) or a separator before its own words
/// (`:::`).
fn supplier_slot(word: &str) -> bool {
    word.contains("{}") || word.starts_with(":::")
}

/// Whether a supplier among `before` adds words after the command it runs.
fn supplied(before: &[String]) -> bool {
    before
        .iter()
        .any(|word| APPENDING.contains(&basename(word).as_str()))
}

/// The index from which every word of `argv` is in a program position
/// because a package manager in a program position before it runs a
/// binary by name: one of [`PACKAGE_MANAGERS`], or `npm` with any command
/// other than [`NPM_OWN_COMMANDS`].
fn package_manager_runs_from(argv: &[String], written: &dyn Fn(usize) -> bool) -> Option<usize> {
    (0..argv.len()).find_map(|at| {
        if at > 0 && !starts_a_command(&argv[..at]) {
            return None;
        }
        let name = basename(&argv[at]);
        let runs = PACKAGE_MANAGERS.contains(&name.as_str())
            || (name == "npm"
                && !npm_runs_its_own_command(&argv[at + 1..], &|word| written(at + 1 + word)));
        runs.then_some(at + 1)
    })
}

/// Whether npm, given `args`, runs one of [`NPM_OWN_COMMANDS`]: its command
/// is the first word after its options, as npm's option parser leaves it
/// (the first word of `remain`). Fail closed: an option this reader does
/// not know, a value that starts with `-`, or a word the shell expands
/// before the command means the command is not established.
fn npm_runs_its_own_command(args: &[String], written: &dyn Fn(usize) -> bool) -> bool {
    let mut at = 0;
    while let Some(word) = args.get(at) {
        if !written(at) {
            return false;
        }
        if word == "--" {
            return written(at + 1)
                && args
                    .get(at + 1)
                    .is_some_and(|command| NPM_OWN_COMMANDS.contains(&command.as_str()));
        }
        if NPM_VALUE_OPTIONS.contains(&word.as_str()) {
            match args.get(at + 1) {
                Some(value) if written(at + 1) && !value.starts_with('-') => at += 2,
                _ => return false,
            }
        } else if NPM_FLAGS.contains(&word.as_str())
            || (word.starts_with("--") && word.contains('='))
        {
            at += 1;
        } else {
            return !word.starts_with('-') && NPM_OWN_COMMANDS.contains(&word.as_str());
        }
    }
    false
}

/// Whether the word after `before` is in a program position: the first
/// word, or after a launcher, supplier or package runner.
fn starts_a_command(before: &[String]) -> bool {
    before.is_empty()
        || before.iter().any(|word| {
            let name = basename(word);
            LAUNCHERS.contains(&name.as_str())
                || SUPPLIERS.contains(&name.as_str())
                || PACKAGE_RUNNERS.contains(&name.as_str())
                || STARTS_A_COMMAND.contains(&word.as_str())
        })
}

/// The raw-text judgment for a line outside the admitted subset and for
/// interpreter code: the binary's name followed, anywhere later, by a
/// verb, or `cargo` followed by `run` or `r` and then a verb.
#[must_use]
pub(crate) fn raw_run(text: &str) -> Option<InstallRun> {
    // A backslash is read both ways: dropped, as the shell drops one that
    // quotes a letter (`cod\eflow`), and as a break, as a printf or echo
    // escape such as `\n` ends the word before it (`update\n`).
    raw_run_reading(text, false).or_else(|| raw_run_reading(text, true))
}

/// [`raw_run`] with each backslash dropped, or read as a word break when
/// `backslash_breaks` is true.
fn raw_run_reading(text: &str, backslash_breaks: bool) -> Option<InstallRun> {
    let plain: String = text
        .chars()
        .filter_map(|c| match c {
            '\\' if backslash_breaks => Some(' '),
            '\\' | '\'' | '"' => None,
            c => Some(c),
        })
        .collect();
    let words: Vec<&str> = plain
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '+' | '-')))
        .filter(|w| !w.is_empty())
        .collect();
    let verb_after = |at: usize| names_a_verb(&words[at..]);
    words.iter().enumerate().find_map(|(at, word)| {
        let form = match basename(word).as_str() {
            BINARY if verb_after(at + 1) => "codeflow update or init",
            "cargo" => {
                let run = words[at + 1..]
                    .iter()
                    .position(|w| *w == "run" || *w == "r")?;
                verb_after(at + run + 2).then_some("cargo run")?
            }
            _ => return None,
        };
        Some(InstallRun {
            form,
            parsed: false,
            in_argument: false,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(command: &str) -> bool {
        install_run(command, true).is_some()
    }

    /// The rows of `refuse` that draw no finding and the rows of `allow`
    /// that draw one, each marked.
    fn misjudged(refuse: &[&str], allow: &[&str]) -> Vec<String> {
        let missed = refuse
            .iter()
            .filter(|command| !runs(command))
            .map(|command| format!("should refuse: {command}"));
        let over = allow
            .iter()
            .filter(|command| runs(command))
            .map(|command| format!("should allow: {command}"));
        missed.chain(over).collect()
    }

    #[test]
    fn a_run_of_either_verb_is_found_in_each_form() {
        for command in [
            "codeflow update",
            "codeflow init --yes",
            "./target/debug/codeflow update --force",
            "/usr/local/bin/codeflow update --pin 4.0.0",
            "CODEFLOW update",
            "codeflow.exe init",
            "nice -n 5 codeflow update",
            "sudo -u root codeflow update",
            "doas codeflow update",
            "chrt -o 0 codeflow update",
            "taskset -c 0 codeflow update",
            "watch -n 60 codeflow update",
            "watch 'codeflow update'",
            "parallel codeflow ::: update",
            "echo update | xargs codeflow",
            "echo update | xargs -I{} codeflow {}",
            "find . -name codeflow -exec {} update ;",
            "busybox env codeflow update",
            "gsudo codeflow update",
            "pkexec codeflow init",
            "npx codeflow update",
            "npm exec -- codeflow update",
            "pnpm exec codeflow update",
            "yarn exec codeflow update",
            "bun x codeflow update",
            "pn exec codeflow update",
            "sh -c 'codeflow update'",
            "eval 'codeflow init --yes'",
            "true; codeflow update",
            "cd /tmp && codeflow init --yes",
            "cargo run -- update",
            "cargo run update",
            "cargo r init --yes",
            "cargo +stable run --bin codeflow -- init",
            "cargo run -p codeflow-cli -- update --force",
            "B=./codeflow; $B update",
            "$(printf codeflow) update",
            "codeflow $V",
            "codeflow \"$@\"",
            "codeflow update --diff=--help",
            "codeflow update --help --force",
            "codeflow init -h --yes",
            "codeflow -- update",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// Words the shell expands without a `$` or a backtick: braces,
    /// patterns and a leading `=`, in the program position, as the first
    /// operand and among the words cargo passes on.
    #[test]
    fn words_the_shell_expands_are_read_as_unresolved() {
        for command in [
            "codeflow {update,--force}",
            "codeflow upd?te",
            "codeflow [u]pdate",
            "codeflow upd*",
            "{codeflow,} update",
            "{./target/debug/codeflow,update}",
            "./target/debug/codeflo? update",
            "=codeflow update",
            "sudo codeflow {update,--force}",
            "cargo run -- $V",
            "cargo run -- \"$@\"",
            "cargo run -- {update,--force}",
            "cargo run -- upd?te",
            "cargo r {init,--yes}",
            "cargo $SUB update",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// A shell that reads its input, an assignment's value read as a
    /// command line, and a search program's option that runs a program.
    #[test]
    fn hidden_runs_behind_launchers_values_and_options_are_found() {
        for command in [
            "printf 'codeflow update' | chrt -o 0 sh",
            "echo codeflow init | taskset -c 0 bash",
            "PAGER='codeflow update' man ls",
            "PAGER='codeflow init --yes'; man ls",
            "export PAGER='codeflow update'; man ls",
            "env PAGER='./target/debug/codeflow update' man ls",
            "NOTE='codeflow update'; $NOTE",
            "rg --pre ./target/debug/codeflow x update",
            "rg --pre=codeflow pattern init",
            "ack --pager='codeflow update' x",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// A search program's option whose value is a command line it runs is
    /// read for that command: behind each launcher, a path spelling, a
    /// shell or a supplier, with the value attached or as the next word,
    /// and under an abbreviated option name. Pagers and preprocessors on a
    /// line that does not name the binary with a verb stay clean.
    #[test]
    fn command_valued_options_are_judged_as_the_command_they_run() {
        for launcher in [
            "env",
            "nice",
            "command",
            "/usr/bin/env",
            "timeout 5",
            "sudo -u root",
            "busybox env",
            "env -i PATH=/usr/bin",
        ] {
            for option in [
                "ack --pager=",
                "ack --pager ",
                "ack --pag=",
                "ag --pager=",
                "ag --pager ",
                "ag --pag=",
            ] {
                for run in ["codeflow update", "./target/debug/codeflow init --yes"] {
                    let command = format!("{option}'{launcher} {run}' pattern README.md");
                    assert!(runs(&command), "{command}");
                }
            }
        }
        for command in [
            "ack --pager='sh -c \"codeflow update\"' pattern README.md",
            "ack --pager='env codefl?w update' pattern README.md",
            "ack --pager='xargs codeflow' pattern README.md",
            "rg --pre xargs pattern codeflow",
            "rg --pre=parallel pattern codeflow",
            "rg --pre 'env codeflow update' pattern docs",
            "rg --pre='nice codeflow init' pattern docs",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "ack --pager=less pattern README.md",
            "ack --pager='less -R' pattern README.md",
            "ack --pager 'less -FRX' pattern README.md",
            "ack --pager='env LESS=-R less' pattern README.md",
            "ag --pager=less pattern README.md",
            "rg --pre ./tools/pdftext pattern docs",
            "rg --pre env pattern codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// The forms the variable model of earlier rounds read (a pager or
    /// editor variable naming a shell, a value no `unset` clears, a name a
    /// builtin writes unseen, a wrapper, a viewer's option, a tool reading
    /// its code from its input) are outside the admitted subset and name
    /// the binary with a verb, so each is judged on its raw text.
    #[test]
    fn forms_the_variable_model_read_refuse_on_their_raw_text() {
        for command in [
            "ACK_PAGER=sh ack --nocolor -h 'codeflow update' notes.txt",
            "export GH_PAGER='bash -s'; gh search issues 'codeflow update'",
            "export ACK_PAGER=sh; true; ack 'codeflow update' docs",
            "export GH_PAGER=sh; unset GH_PAGER; gh search issues 'codeflow update'",
            "export GH_PAGER=sh; (GH_PAGER=less); gh search issues 'codeflow update'",
            "export ACK_PAGER=less; printf -v ACK_PAGER sh; ack -h 'codeflow update' notes.txt",
            "export GH_PAGER=less; read GH_PAGER < pager.txt; gh search issues 'codeflow update'",
            "export GH_PAGER=sh; builtin unset GH_PAGER; gh search issues 'codeflow update'",
            "export GH_PAGER=less; typeset -m 'GH_*=sh'; gh search issues 'codeflow update'",
            "for i in 1 2; do gh search issues 'codeflow update'; GH_PAGER=sh; done",
            "ACK_PAGER='emacs --script /dev/stdin' ack -h 'codeflow update' notes.txt",
            "git -c core.pager=perl log -1 --format='codeflow update'",
            "git -c core.pager='less -k keys' log -1 --format='codeflow update'",
            "git grep -Operl 'codeflow update'",
            "git -c filter.x.clean=perl log -1 --grep='codeflow update'",
            "git -c filter.x.clean='perl x.pl' log -1 --grep='codeflow update'",
            "GIT_EXTERNAL_DIFF=node git log -p -1 --grep='codeflow update'",
            "export RIPGREP_CONFIG_PATH=rg.conf; rg 'codeflow update' docs",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// An interpreter or shell as a Git runner or a fed program is never
    /// admitted, whatever inline code it holds: each such line is outside
    /// the subset and judged on its raw text.
    #[test]
    fn interpreters_as_runners_or_fed_programs_refuse() {
        for command in [
            "git -c diff.external='python3 -c \"import os,sys;os.system(sys.argv[1])\"' \
             diff -- 'codeflow update'",
            "git -c diff.external='python3 -c \"import os,sys;os.system(sys.argv[1])\"' \
             log -p -1 -- 'codeflow update'",
            "echo 'os.execute(\"codeflow update\")' | lua -e \"\" -",
            "lua -e \"\" - <<< 'os.execute(\"codeflow update\")'",
            "lua ./run.lua 'codeflow update'",
            "git -c core.pager='lua -e \"\" -' log -1 --format='os.execute(\"codeflow update\")'",
            "GIT_PAGER='lua -' git log -1 --format='os.execute(\"codeflow update\")'",
            "git -c filter.x.clean='lua -e \"\" -' log -1 --grep='codeflow update'",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// A listed program's own grammar can still run code or reach the
    /// network: a printf format that reads an argument as a number (zsh
    /// evaluates it, substitutions in subscripts included), a ripgrep
    /// `--no-config` that an option takes as its value, and a Git search
    /// that can fetch a missing object through a configured transport.
    /// Each such line is outside the subset and judged on its raw text.
    #[test]
    fn programs_whose_own_grammar_can_run_code_refuse() {
        for command in [
            "printf '%d' 'argv[$(codeflow update)]'",
            "printf '%n' 'codeflow update'",
            "printf '\\045d' 'a[$(codeflow update)]'",
            "printf '%b' 'codeflow update'",
            "printf '%s\\n' 'codeflow update' > input.txt; rg -e --no-config input.txt",
            "rg --regexp=--no-config 'codeflow update'",
            "rg -i --no-config 'codeflow update' docs",
            "git --no-pager grep -e 'codeflow update' HEAD -- ordinary.txt",
            "git --no-pager log --grep='codeflow update'",
            "./target/debug/codeflow update --help",
            "echo 'codeflow update' > /dev/tcp/127.0.0.1/9",
            "printf '%s\\n' 'codeflow update' > commands.fifo",
            "printf '%s\\n' 'codeflow update' > /proc/self/fd/3",
            "printf 'codeflow update\\n' > notes.txt",
            "grep -n 'codeflow update' notes.md 2>&1 | wc -l",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// A line outside the subset is judged on three texts: its raw text,
    /// its words as the shell passes them (quotes and backslashes removed,
    /// `$'...'` decoded), and what a printing program writes from them
    /// where that is settled, besides the values the shell composer
    /// resolves (its model of `printf`, `echo` and `print` included). The
    /// text a printing program writes is read again as a shell would read
    /// it.
    #[test]
    fn text_the_shell_and_printing_programs_produce_is_judged() {
        let refuse = [
            r"printf cod\eflow' update\n' > commands.fifo",
            r"printf '\ncodeflow update' > commands.fifo",
            r"printf '\tcodeflow update' > commands.fifo",
            r"printf 'codeflow\nupdate' > commands.fifo",
            r"printf 'codeflow\x20update' > commands.fifo",
            r"printf 'codeflow update\n\n' > commands.fifo",
            r"printf '\143odeflow update' > commands.fifo",
            r"printf 'codeflow upd\141te' > commands.fifo",
            r"printf '\x63odeflow update' > commands.fifo",
            r"printf 'code\x66low \x75pdate' > commands.fifo",
            r"printf '%b' '\143odeflow update' > commands.fifo",
            r"printf '%b %b\n' '\x63odeflow' 'upd\0141te' > commands.fifo",
            r"echo -e '\x63odeflow update' > commands.fifo",
            r"echo -e 'codeflow\tupdate' > commands.fifo",
            r"echo '\0143odeflow update' > commands.fifo",
            r"print '\143odeflow update' > commands.fifo",
            r"printf '%s\n' $'\x63odeflow update' > commands.fifo",
            r"printf '%s\n' $'codeflow\nupdate' > commands.fifo",
            r"printf c$'\x6f'deflow' up\x64ate' > commands.fifo",
            r#"printf "cod\\\\eflow upd\141te\n" > commands.fifo"#,
            r#"printf '%s\n' "\$'\\x63'odeflow update" > commands.fifo"#,
            r#"sh -c "printf '\\143odeflow update' > commands.fifo""#,
            r#"eval "printf '\\143odeflow update'" > commands.fifo"#,
            r#"a=code; printf '%sflow update' "$a" > commands.fifo"#,
            r#"a=cod; echo "${a}eflow update" > commands.fifo"#,
            r#"x='\143odeflow update'; print "$x" > commands.fifo"#,
            r#"x='\143'; printf '%s%s%s%s\n' '$' "'" "$x" "'odeflow update" > commands.fifo"#,
            r#"x='co\deflow'; printf '%s update\n' "$x" > commands.fifo"#,
            r#"a="'cod'"; b="'eflow'"; printf '%s%s update\n' "$a" "$b" > commands.fifo"#,
            r#"x='$'; y="'\\143'"; printf '%s%s%s\n' "$x" "$y" 'odeflow update' > commands.fifo"#,
            // r22: more than one command and a spelled write, which refuses
            // whatever it writes.
            r#"a=notes; printf '%s\n' "$a" > notes.txt"#,
            r#"x='notes'; echo "$x" > out.txt"#,
            r#"x='a\tb'; printf '%s\n' "$x" > out.txt"#,
            r"{ printf '%s' note; printf '%s\n' 's here'; } > out.txt",
            r"printf '%s\n' a > out.txt; printf '%s\n' b > out.txt",
            r#"for w in no tes; do printf '%s' "$w"; done > out.txt"#,
        ];
        let allow = [
            r"printf '%s\n' 'notes' > notes.txt",
            r"printf 'codeflow\n' > notes.txt",
            r"printf 'update\n' > notes.txt",
            "printf '%s' notes > out.txt",
            "echo notes > out.txt",
            "echo -n notes > out.txt",
            "print -r notes > out.txt",
            r"printf '%s\n' notes > out.txt",
            r#"printf "%s\n" notes > out.txt"#,
            r"printf 'a %s b %s\n' x y > out.txt",
            r"printf '%s %s\n' x > out.txt",
            r#"printf '%s\n' "$a" > out.txt"#,
            r#"echo "$HOME" > out.txt"#,
            r#"find . -type f -exec sh -c 'echo "$1"' _ {} \;"#,
            r"print -r 'a\x75b' > out.txt",
            r"printf -v p '%s%s' no tes; cat $p > out.txt",
            r"printf '%s\n' 'notes' > /dev/null",
            "ls notes > out.txt",
        ];
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Printed text that is not settled (an escape, a conversion other than
    /// a bare `%s`, a width, more arguments than conversions, a value
    /// `echo` may decode) draws no finding on its own: it refuses where the
    /// line hands it to a shell or interpreter on the line.
    #[test]
    fn printed_text_that_is_not_settled_refuses_only_where_it_runs() {
        let refuse = [
            r"printf '%5s\n' notes | sh",
            r"printf 'notes\n' a | sh",
            r"printf 'a\tb' | sh",
            r#"printf "$fmt" | sh"#,
            r"echo -e 'a\tb' | bash",
            r"print -P '%n' | zsh",
            r"echo $'notes' $'a\tb' | sh",
            r"printf '%x\n' 255 | python3",
            // r22: more than one command and a spelled write.
            r#"x='a\cb'; echo "$x" > out.txt"#,
            r#"x='a\tb'; echo "$x" > out.txt"#,
        ];
        let allow = [
            r"printf '%5s\n' notes > out.txt",
            r"printf '%.3s\n' notes > out.txt",
            r"printf '%-s\n' notes > out.txt",
            r"printf '%s\n' a b > out.txt",
            r"printf 'notes\n' a > out.txt",
            r"printf '%d\n' 5 > out.txt",
            r"printf '%b' notes > out.txt",
            r"printf '100%%\n' > out.txt",
            r"printf 'a\tb' > out.txt",
            r"printf '%s\\n' notes > out.txt",
            r"printf %s\n notes > out.txt",
            r#"printf "$fmt" > out.txt"#,
            r"printf > out.txt",
            r"echo -e 'a\tb' > out.txt",
            r"echo 'a\nb' > out.txt",
            r"print 'notes\n' > out.txt",
            r"print -P '%n' > out.txt",
            r"echo $'notes' $'a\tb' > out.txt",
            r"cat $'notes\x2etxt' > out.txt",
            r"echo -e 'codeflow\tversion' > notes.txt",
            r"printf '\x63odeflow\n' > notes.txt",
            r"printf '%x\n' 255 > notes.txt",
            r"printf 'codeflow %x\n' 10 > notes.txt",
            r"printf 'size %x %x\n' $a $b > notes.txt",
        ];
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Text a line hands to something on the line that executes it (a
    /// shell or interpreter fed by a pipe, a here-string, an expanding
    /// here-document or a process substitution; `eval`; a `-c` or `-e`
    /// value that expands; a command word the guard cannot fill in; a
    /// supplier that runs what it reads) is settled only as one printer of
    /// plain literal text, a literal here-string or a literal
    /// here-document. Any other feed refuses, whatever it names. A plain
    /// file the line does not name elsewhere counts as that file run
    /// directly.
    #[test]
    fn text_handed_to_an_executor_on_the_line_must_be_settled() {
        let refuse = [
            r"printf '\143odeflow update' | sh",
            r"{ printf code; printf 'flow update\n'; } | bash",
            r#"x=code; printf '%s' "$x flow update" | sh"#,
            r#"bash <<< "$(printf '\143odeflow update')""#,
            r#"eval "$(printf '\143odeflow update')""#,
            r"$(printf '\143odeflow') update",
            r"{ { sleep 1; printf '%s\n' 'flow update'; } & printf '%s' code; wait; } | sh",
            r#"for w in code 'flow update'; do printf '%s' "$w"; done | sh"#,
            r"printf '\143odeflow update' | python3",
            r"source <(printf '\143odeflow update\n')",
            r"printf '\143odeflow update\n' | xargs -0 sh -c",
            r"printf '\143odeflow update\n' | parallel",
            r"printf '\143odeflow update\n' | xargs env",
            r"x=$(printf '\143odeflow update'); $x",
            r#"x=$(printf '\143odeflow update'); sh -c "$x""#,
            r#"x=$(printf '\143odeflow update'); eval "$x""#,
            "python3 - <<EOF\nimport os; os.system(\"$(printf '\\143odeflow update')\")\nEOF",
            r"printf '%s\n' 'echo hi' 'codeflow update' | sh",
            r"printf 'co%xflow update' 222 > s.sh; cat s.sh | sh",
            r"bash <(printf 'co%xflow update' 222)",
            r#"python3 -c "$(printf 'co%xflow update' 222)""#,
            r"python3 <<< $'import os; os.system(\x27\143odeflow update\x27)'",
            r"printf 'echo hi\n' | tee log.txt | sh",
            r#"printf '%s\n' 'printf "\143odeflow update" | sh' | sh"#,
            r#"echo 'eval "$1"' | sh -s "$x""#,
            r"printf 'echo hi\n' |& sh",
            r"printf 'echo hi\n' | sh; printf '\143odeflow update' | sh",
        ];
        // Settled text is judged: as a command line for a shell or a
        // supplier, as code for an interpreter.
        let named = [
            r"printf '%s%s update\n' code flow | sh",
            r"echo 'codeflow update' | bash",
            r"printf '%s\n' update | xargs codeflow",
            r#"printf 'import os; os.system("codeflow update")\n' | python3"#,
        ];
        let allow = [
            r"printf 'echo hi\n' | sh",
            r"printf '%s\n' notes > out.txt",
            r"echo -e 'a\tb' > out.txt",
            "echo 'echo hi' | bash",
            r"printf 'ls\n' | zsh",
            "echo ls | sh -s",
            r"printf '%s\n' 'echo hi' | sh",
            r"printf 'print(1)\n' | python3",
            "bash <<< 'echo hi'",
            "python3 <<< \"print(1)\"",
            "sh -c 'echo hi'",
            "sh < script.sh",
            "python3 - <<'EOF'\nprint(1)\nEOF",
            "cd src && python3 - <<'EOF'\nprint(1)\nEOF",
            "python3 - <<EOF\nprint(1)\nEOF",
            r"printf 'notes\n' | cat",
            "ls | wc -l",
            "git ls-files | xargs wc -l",
            "x=ls; $x",
            r"cat notes.txt | sh",
            r"cat script.sh | bash",
            r"bash <(cat script.sh)",
            r"bash <(echo 'ls src')",
            r#"python3 -c "$(printf '%s\n' 'print(1)')""#,
            r"python3 -c $'print(1)'",
            r"python3 /dev/fd/3 3<<< 'print(1)'",
            "cat <<'E' | sh\nprintf hello\nE",
            "git -c core.pager=sh log -1",
            "cd src && bash <<< 'echo hi'",
            "sh",
        ];
        let wrong = misjudged(&[&refuse[..], &named[..]].concat(), &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Text a line writes to a process started earlier that executes what
    /// it reads (a named pipe, a descriptor, a background shell) is the
    /// named residual: on a line with no executor of its own, only a
    /// settled printer's output and the composer's values are judged, so
    /// these draw no install finding (AC-6). The sandbox, where it runs,
    /// denies the writes the verbs make. A line of more than one command
    /// that writes spelled text refuses (r22), so what remains is one
    /// command, or a line that writes no file.
    #[test]
    fn text_written_to_an_earlier_process_is_the_named_residual() {
        let comments = "'#' ".repeat(16);
        let spelled = "c o d e f l o w ' ' u p d a t e";
        let reused = format!("printf '%c' {comments}$'\\n' {spelled} $'\\n' > commands.fifo");
        let refuse = [
            r#"while read -r; do printf '%s' "$REPLY"; done < words.txt > commands.fifo"#,
            r"{ printf '%s' code; printf '%s\n' 'flow update'; } > commands.fifo",
            r#"for w in code 'flow update'; do printf '%s' "$w"; done > commands.fifo"#,
            r"printf '%s' code > commands.fifo; printf '%s\n' 'flow update' > commands.fifo",
            r"printf '%s' code >> commands.fifo && printf '%s\n' 'flow update' >> commands.fifo",
            r"{ printf '%s' code || printf '%s\n' 'flow update'; } > commands.fifo",
            r"( printf '%s' code; printf '%s\n' 'flow update' ) > commands.fifo",
            r#"while read -r w; do printf '%s' "$w"; done < words.txt > commands.fifo"#,
            r#"for f in *.txt; do echo "$f"; done > commands.fifo"#,
            r#"x=code; { printf '%s' "$x"; printf '%s\n' 'flow update'; } > commands.fifo"#,
            r#"x='\143odeflow update'; echo "$x" > commands.fifo"#,
        ];
        let residual = [
            r#": ${piece:=code}; printf '%s' "$piece"; printf '%s\n' 'flow update'"#,
            r"printf '\u0063odeflow update' > commands.fifo",
            r"printf '\U00000063odeflow update' > commands.fifo",
            r"printf 'cod\0eflow update' > commands.fifo",
            r"printf '%d %s\n' 1 '\143odeflow update' > commands.fifo",
            r"printf '%.8s update' 'codeflowXYZ' > commands.fifo",
            r"echo -n '\143odeflow update' > commands.fifo",
            r"echo -e co\de$'flow'' up\u0064ate' > commands.fifo",
            r"printf 'codeflow upd%xte' 10 > commands.fifo",
            r"printf '%xodeflow update' 12 > commands.fifo",
            r"printf 'co%xflow update' 222 > commands.fifo",
            r"printf 'co%Xflow update' 0xde > commands.fifo",
            r"printf 'co%xflow update' 0336 > commands.fifo",
            r"printf 'co%xflow update' $((222)) > commands.fifo",
            r"printf 'codeflow upd%.0xate' 0 > commands.fifo",
            r#"printf '%xo%xflow update' "$a" "$b" > commands.fifo"#,
            r"printf '\143odeflow%1supdate\n' '' > commands.fifo",
            r"printf '#\c\n\143odeflow update\n' > commands.fifo",
            reused.as_str(),
        ];
        let wrong = misjudged(&refuse, &residual);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// A line that writes a file with text it spells and also runs a file
    /// (a script operand, a file fed to a shell or interpreter, a path as
    /// the program) refuses as unsettled, whatever the names: the guard
    /// does not match the written file to the run one. A file the line does
    /// not write keeps its handling as that file run directly, and a file a
    /// program writes in its own way (a build) is state the line does not
    /// show.
    #[test]
    fn a_file_the_line_writes_and_runs_is_unsettled() {
        let split = "printf '%s' code > run.sh; printf '%s\\n' 'flow update' >> run.sh";
        let refuse = [
            format!("{split}; sh run.sh"),
            format!("{split}; bash run.sh"),
            format!("{split}; . run.sh"),
            format!("{split}; source run.sh"),
            format!("{split}; python3 run.sh"),
            format!("{split}; chmod +x run.sh; ./run.sh"),
            format!("{split}; sh < run.sh"),
            format!("{split}; cat run.sh | ssh host sh"),
            "printf 'co%xflow update' 222 > Run.sh; cat run.sh | sh".to_string(),
            "printf '%s\\n' 'codeflow update' > Run.sh; cat run.sh | sh".to_string(),
            r#"for w in code 'flow update'; do printf '%s' "$w"; done > run.sh; sh run.sh"#
                .to_string(),
            "{ printf '%s' code; printf '%s\\n' 'flow update'; } > run.sh && bash run.sh"
                .to_string(),
            "printf '%s' code | tee run.sh; printf '%s\\n' 'flow update' >> run.sh; sh run.sh"
                .to_string(),
            "dd of=run.sh <<< 'x'; sh run.sh".to_string(),
            "printf 'co%xflow update' 222 > tool; chmod +x tool; dir/tool".to_string(),
            "sh -c 'printf co > r.sh; printf \"flow update\" >> r.sh; sh r.sh'".to_string(),
            "case a in a) printf 'co%xflow update' 222 > r.sh; sh r.sh;; esac".to_string(),
        ];
        let allow = [
            "sh scripts/test.sh",
            "cat notes.txt | sh",
            "cargo build && ./target/debug/tool",
            r"printf '%s\n' notes > out.txt",
            "git log --oneline > log.txt",
            "sh scripts/test.sh > /dev/null 2>&1",
            "./scripts/check.sh 2>&1 | tail -n 3",
            "python3 tools/report.py",
            // One simple command writes only its own output.
            "./scripts/check.sh > check.log 2>&1",
            "python3 tools/report.py > report.txt",
            "gzip -c < data/in.txt > out.gz",
            "/opt/homebrew/bin/gsed -i s/a/b/ notes.txt",
            "gtee out.log < /dev/null",
            "python3 gen.py > out.json",
            "python3 use.py out.json",
            // A file written with a program's output, not text the line
            // spells, is the named residual (r21).
            "python3 gen.py > out.json; python3 use.py out.json",
            "/bin/ln src/app.py H && dd if=/dev/null of=H",
            "./scripts/check.sh 2>&1 | tee check.log",
        ];
        let refuse: Vec<&str> = refuse.iter().map(String::as_str).collect();
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Text the line spells that a program the guard does not know receives
    /// (by pipe, here-string, here-document or process substitution) takes
    /// the closed form: settled text is judged as a command line, anything
    /// else refuses. A known data reader is not judged, and the output of a
    /// program the line runs, fed to an unknown program, is the named
    /// residual. Lines the reader does not take apart get the same result.
    #[test]
    fn spelled_text_fed_to_an_unknown_program_takes_the_closed_form() {
        let refuse = [
            r"{ printf '%s' code; printf '%s\n' 'flow update'; } | ssh host sh",
            r"printf 'co%xflow update' 222 | ssh host sh",
            r"printf 'co%xflow update' 222 | ssh host",
            r"printf '%s\n' 'codeflow update' | ssh host",
            r"printf '\143odeflow update\n' | mytool",
            r#"x=code; printf '%s' "$x flow update" | ssh host"#,
            r"ssh host <<< $'\143odeflow update'",
            r#"ssh host <<< "$(printf 'co%xflow update' 222)""#,
            r"ssh host < <(printf 'co%xflow update' 222)",
            r"mytool <(printf 'co%xflow update' 222)",
            "ssh host <<EOF\n$(printf 'co%xflow update' 222)\nEOF",
            r#"for w in code 'flow update'; do printf '%s' "$w"; done | ssh host"#,
            r"{ printf 'co%xflow update' 222; } | ssh host",
            r"( printf 'co%xflow update' 222 ) | ssh host",
            r"printf 'co%xflow update' 222 | read -r x",
            // AC-6: a reused format is not settled, and `read` is not a
            // data reader the guard knows.
            r"printf '%s\n' a b | while read -r l; do echo $l; done",
            r"echo hi && { printf 'co%xflow update' 222 | ssh host; }",
            r#"for f in a; do echo "$(printf 'co%xflow update' 222 | ssh host)"; done"#,
            r#"{ sh -c "printf 'co%xflow update' 222 | ssh host"; }"#,
            r"case a in a|b) printf 'co%xflow update' 222 | ssh host;; esac",
            r"case $x in (a) true;; *) printf 'co%xflow update' 222 | mytool; esac",
            // AC-6 (r21): `sort` can hand what it reads to a program an
            // option names, so it is not a data reader.
            "{ echo a; echo b; } | sort",
            r"printf '%x\n' 255 | sort",
        ];
        let allow = [
            r"printf 'ls\n' | ssh host",
            "git log --oneline | head -5",
            r"printf 'echo hi\n' | sh",
            "git log | ssh host 'cat > log.txt'",
            "curl -s https://example.com/data.json | jq .name",
            "ssh host <<< 'uptime'",
            "ssh host <<'EOF'\nuptime\nEOF",
            r"printf 'a\nb\n' | while read -r l; do echo $l; done",
            "git ls-files | while read -r f; do wc -l \"$f\"; done",
            r#"for f in a b; do echo "== $f"; cargo test -p "$f" 2>&1 | tail -3; done"#,
            r"printf '%x\n' 255 | xargs echo",
            "diff <(printf 'a\\n') <(printf 'b\\n')",
            r"{ git log; git status; } | ssh host 'cat > log.txt'",
            r"printf 'fix: x\n' | git commit -F -",
            "case a in a) ls ./build | xargs wc -l;; esac",
            "case $x in a|b) git log | head -3;; *) echo none;; esac",
        ];
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Text the line spells takes the closed form on every channel a
    /// program receives it by, not only standard input: an output process
    /// substitution, which reads what its command writes; a command
    /// substitution or a value the line binds, given as an argument; a
    /// descriptor other than standard input, set on the command or by
    /// `exec`; a descriptor path or a file the line writes, given as a file
    /// operand; and an assignment, for a program the guard does not know or
    /// a variable a program runs as a command. Program output on any of
    /// them stays the named residual.
    #[test]
    fn every_channel_a_program_receives_spelled_text_by_takes_the_closed_form() {
        let join = "printf '%s' code; printf '%s\\n' 'flow update'";
        let refuse = [
            format!("cat <({join}) > >(sh)"),
            format!("{{ {join}; }} > >(ssh host sh)"),
            format!("( {join} ) > >(sh)"),
            format!("{{ {join}; }} 3>&1 > >(mytool)"),
            format!("ssh host \"$({join})\""),
            format!("ssh host \"`{join}`\""),
            format!("mytool --cmd=\"$({join})\""),
            format!("ssh host sh <&3 3< <({join})"),
            format!("exec 3< <({join}); ssh host sh <&3"),
            format!("exec > >(sh); {join}"),
            "exec 3< <(printf 'co%xflow update' 222); mytool <&3".to_string(),
            "mytool /dev/fd/3 3< <(printf 'co%xflow update' 222)".to_string(),
            "cat /dev/fd/3 3< <(printf 'co%xflow update' 222) | sh".to_string(),
            "mytool 3<<< \"$(printf 'co%xflow update' 222)\"".to_string(),
            "printf 'co%xflow update' 222 | tee >(sh)".to_string(),
            "printf 'co%xflow update' 222 > >(ssh host)".to_string(),
            "printf 'co%xflow update' 222 2>&1 > >(mytool)".to_string(),
            "x=\"$(printf 'co%xflow update' 222)\"; ssh host \"$x\"".to_string(),
            "printf -v x 'co%xflow update' 222; ssh host \"$x\"".to_string(),
            "ssh host \"co$(printf '%x' 222)flow update\"".to_string(),
            "ssh host \"${x:-$(printf 'co%xflow update' 222)}\"".to_string(),
            r"ssh host $'\143odeflow update'".to_string(),
            "GIT_SSH_COMMAND=\"$(printf 'co%xflow update' 222)\" git fetch".to_string(),
            "MYCMD=\"$(printf 'co%xflow update' 222)\" mytool".to_string(),
            "env MYCMD=\"$(printf 'co%xflow update' 222)\" mytool".to_string(),
            "{ ssh host \"$(printf 'co%xflow update' 222)\"; }".to_string(),
            "for h in a; do ssh \"$h\" \"$(printf 'co%xflow update' 222)\"; done".to_string(),
            "case a in a) ssh host \"$(printf 'co%xflow update' 222)\";; esac".to_string(),
            "printf 'co%xflow update' 222 |& ssh host".to_string(),
            // A file the line writes, handed by name as a file operand.
            "printf 'co%xflow update' 222 > f.txt; mytool f.txt".to_string(),
            "printf 'co%xflow update' 222 > f.txt; mytool ./f.txt".to_string(),
            "printf 'co%xflow update' 222 > f.txt; mytool *.txt".to_string(),
            "printf 'co%xflow update' 222 > f.txt; cp f.txt g.cfg; ssh -F g.cfg host".to_string(),
            "printf 'co%xflow update' 222 > f.txt; ssh -Ff.txt host".to_string(),
            "printf 'co%xflow update' 222 | tee f.txt; mytool --config=f.txt".to_string(),
            "printf 'co%xflow update' 222 > cfg; GIT_CONFIG_GLOBAL=cfg git fetch".to_string(),
            "printf 'co%xflow update' 222 > cfg; git -c include.path=cfg fetch".to_string(),
            // A plain value joined with other text in a compound command.
            "if true; then x=code; ssh host \"${x}flow update\"; fi".to_string(),
            // An assignment alone that a later program reads.
            "GIT_SSH_COMMAND=\"$(printf 'co%xflow update' 222)\"; git fetch".to_string(),
            "MYCMD=\"$(printf 'co%xflow update' 222)\"; export MYCMD; mytool".to_string(),
            "set -a; MYCMD=\"$(printf 'co%xflow update' 222)\"; mytool".to_string(),
        ];
        let allow = [
            "if true; then D=./build; rm -rf $D; fi",
            "if true; then x=ls; ssh host \"$x\"; fi",
            "curl -s 'https://example.com/x?a=1' > out.json && jq . out.json",
            "cargo build > build.log 2>&1; mytool --rev abc",
            "cargo test 2>&1 | tee test.log; grep FAILED test.log",
            "v=$(printf '%05d' 3); echo \"$v\"",
            "export PATH=\"$(pwd)/bin:$PATH\"; mytool",
            "HOME=\"$(mktemp -d)\" cargo test",
            "if true; then D=/; D=build; rm -rf \"$D\"; fi",
            "diff <(sort a.txt) <(sort b.txt)",
            "ssh host \"$(printf 'ls')\"",
            "git commit -m \"$(printf 'fix: a')\"",
            "echo \"$(date)\" > out.txt",
            "tool --rev \"$(git rev-parse HEAD)\"",
            "tool --rev=\"$(git rev-parse HEAD)\"",
            "make 2>&1 > >(tee build.log)",
            "cargo test 2>&1 | tee >(grep -c ok)",
            "cargo build 2> >(grep -v warning)",
            "exec 3< notes.txt",
            "exec 2>&1",
            "mytool 3< notes.txt",
            "x=$(git rev-parse HEAD); git show \"$x\"",
            "x=$(git rev-parse HEAD); mytool \"$x\"",
            "GIT_PAGER=less git log",
            "ls \"$(pwd)\"",
            "mkdir -p \"$(dirname out/a.txt)\"",
            "ssh host \"`git rev-parse HEAD`\"",
            "mytool \"$(git ls-files)\"",
            "for f in a b; do wc -l \"$f\"; done",
        ];
        let refuse: Vec<&str> = refuse.iter().map(String::as_str).collect();
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// A line of more than one command that writes text it spells to a
    /// file refuses, whatever else is on it: a program can run a file the
    /// line never names (a default file, a loader option, a configuration
    /// file, TSK-1476 r21), the shell finds a program's name on the path,
    /// where the line may write a file of that name, and the shell itself
    /// runs text it evaluates as arithmetic, even beside builtins only
    /// (r22). One simple command whose writes come last is the exception.
    #[test]
    #[allow(clippy::too_many_lines)] // the r22 sets, one row each
    fn a_line_of_several_commands_that_writes_spelled_text_to_a_file_refuses() {
        let refuse = [
            // r22: a file the line writes, run from text the shell
            // evaluates as arithmetic beside builtins only.
            "printf '%s' code > f; printf '%s\\n' 'flow update' >> f; [[ 1 -eq 'a[$(sh f)]' ]]",
            "printf '%s' code > f; printf '%s\\n' 'flow update' >> f; test -v 'a[$(sh f)]'",
            "printf '%s' code > f; printf '%s\\n' 'flow update' >> f; printf -v 'a[$(sh f)]' x",
            "printf '%s' code > f; printf '%s\\n' 'flow update' >> f; read 'a[$(sh f)]' < /dev/null",
            "printf '%s' code > f; printf '%s\\n' 'flow update' >> f; x='a[$(sh f)]'; [[ $x -eq 1 ]]",
            // r22: the rows a builtin-only rule allowed.
            "printf x > f; echo done",
            "printf x > f && cd d",
            "cd d && printf x > f",
            "for f in a b; do printf '%s\\n' \"$f\" > \"$f.txt\"; done",
            "printf x > f; [ -f f ] && echo ok",
            "printf x > f; [[ -f f ]] && echo ok",
            "printf x > f; test -s f || echo empty",
            "x=1; printf '%s' \"$x\" > f; echo \"$x\"",
            "printf x > f & wait",
            "pushd d && printf x > f; popd",
            "printf x > f; pwd; true; false; :",
            "printf -v v '%s' x; printf '%s' \"$v\" > f",
            // r22: a program file written into a folder on the path, then
            // the program run by its plain name, or by an option that runs
            // it.
            "printf '%s\\n' '#!/bin/sh' > /usr/local/bin/grep; printf '%s' 'code' >> /usr/local/bin/grep; printf '%s\\n' 'flow update' >> /usr/local/bin/grep; chmod +x /usr/local/bin/grep; grep x",
            "printf '%s\\n' '#!/bin/sh' > /usr/local/bin/pr; printf '%s' 'code' >> /usr/local/bin/pr; printf '%s\\n' 'flow update' >> /usr/local/bin/pr; chmod +x /usr/local/bin/pr; printf 'a\\n' > a.txt; diff -l a.txt a.txt",
            "printf '%s\\n' '#!/bin/sh' > /opt/homebrew/bin/grep; printf '%s' 'code' >> /opt/homebrew/bin/grep; printf '%s\\n' 'flow update' >> /opt/homebrew/bin/grep; chmod +x /opt/homebrew/bin/grep; grep x",
            "printf '%s\\n' '#!/bin/sh' > /opt/homebrew/bin/git; printf '%s' 'code' >> /opt/homebrew/bin/git; printf '%s\\n' 'flow update' >> /opt/homebrew/bin/git; chmod +x /opt/homebrew/bin/git; printf x > a.txt && git add a.txt",
            // r22: every program, the r21 controls that ran one included.
            "printf x > a.txt && git add a.txt",
            "mkdir -p d && printf x > d/f",
            "echo x > f; cat f",
            "echo x > f && cp f g && chmod +x g",
            "printf '%s\\n' a b > list.txt && wc -l list.txt && cat list.txt",
            "printf '{}' > a.json && jq . a.json",
            "printf x | base64 > enc.txt && tail -n 1 enc.txt",
            "cd out && printf x > f && head f",
            "printf x > a; ln -s a b && rm a",
            "echo x > f.txt; diff f.txt g.txt",
            // r22: each builtin left off the list, and zsh's `print`.
            "printf x > f; source f",
            "printf x > f; . f",
            "printf x > f; eval \"$(cat f)\"",
            "printf x > f; eval echo",
            "printf x > f; exec f",
            "printf x > f; exec",
            "printf x > f; exec 3< f",
            "printf x > f; command grep x f",
            "printf x > f; builtin echo x",
            "printf x > f; trap 'sh f' EXIT",
            "printf x > f; fc -s",
            "printf x > f; enable -f ./f x",
            "printf x > f; alias echo=sh; echo",
            "printf x > f; hash -p ./f echo; echo",
            "printf x > f; type echo",
            "printf x > f; coproc echo x",
            "printf x > f; autoload -U f",
            "printf x > f; emulate sh -c 'sh f'",
            "printf x > f; print done",
            // r22: a listed builtin the line redefines.
            "echo() { sh f; }; printf x > f; echo",
            "printf() { sh f; }; printf x > f; printf y",
            "cd() { sh f; }; printf x > f; cd d",
            "pushd() { sh f; }; printf x > f; pushd d",
            "popd() { sh f; }; printf x > f; popd",
            "pwd() { sh f; }; printf x > f; pwd",
            "true() { sh f; }; printf x > f; true",
            "false() { sh f; }; printf x > f; false",
            "test() { sh f; }; printf x > f; test -f f",
            "read() { sh f; }; printf x > f; read a < f",
            "wait() { sh f; }; printf x > f; wait",
            "function echo { sh f; }; printf x > f; echo",
            // r22: a program before the write, in the background, in a
            // pipeline, in a loop header or in a substitution.
            "grep x f & printf x > f",
            "printf x > f & grep x f",
            "grep x f; printf x > f",
            "printf x > f; echo \"$(grep x f)\"",
            "printf x > f; for w in $(grep x f); do echo \"$w\"; done",
            "printf x > f; echo x | grep x",
            "printf x > f; a=$(grep x f); echo \"$a\"",
            "printf x > f; if grep -q x f; then echo y; fi",
            "printf x > f; while read l; do grep \"$l\" g; done < f",
            "printf x > f; echo x > >(grep x)",
            // r22: rows earlier tests allowed, each a spelled write beside a
            // program: a data program, `git add`, `tee` into a reader, zsh's
            // `print` after an assignment, and a writer group holding a
            // program (once the earlier-process residual).
            "printf 'a\\n' > a.txt; diff a.txt b.txt",
            "printf note > notes.txt && git add notes.txt",
            "ln -s src/app.py app && printf x > app",
            "echo x | tee \"$TMPDIR/run.log\" | grep -E \"SURVIVED|MISSING\"",
            r#"x='a\tb'; print -r "$x" > out.txt"#,
            r"{ { sleep 1; printf '%s\n' 'flow update'; } & printf '%s' code; wait; } > commands.fifo",
            r"{ echo -n code; cat notes.txt; echo 'flow update'; } > commands.fifo",
            r"{ printf '%s' code; head -n 1 notes.txt; printf '%s\n' 'flow update'; } > commands.fifo",
            // A default file, and a loader option, the line never names
            // where the program runs.
            "printf '%s\\n' 'all:' > Makefile; printf '%s' '\tcode' >> Makefile; printf '%s\\n' 'flow update' >> Makefile; make",
            r#"printf '%s' 'require("child_process").execSync("code' > hook.js; printf '%s\n' 'flow update")' >> hook.js; node --require ./hook.js -e 0"#,
            r#"printf '{"scripts":{"test":"x"}}' > package.json && npm test"#,
            "printf 'import os' > mod.py && python -m mod",
            "printf 'default:\\n\\tx\\n' > justfile && just",
            "printf 'task :default' > Rakefile && rake",
            "printf 'fn main(){}' > build.rs && cargo test",
            "printf x > h.js; node h.js",
            "printf 'all:' > Makefile; env make",
            "printf 'all:' > Makefile; exec make",
            // The writer inside a group, a subshell, a loop or a
            // substitution, or a compound command's output to the file.
            "{ printf 'all:\\n\\tx\\n' > Makefile; }; make",
            "( printf 'all:\\n' > Makefile ); make",
            "for f in Makefile; do printf 'all:\\n' > \"$f\"; done; make",
            "for w in all: x; do printf '%s\\n' \"$w\"; done > Makefile; make",
            "{ printf 'all:\\n'; printf 'x\\n'; } > Makefile && make",
            "x=$(printf 'all:' > Makefile); make",
            "printf 'all:\\n' | { cat; } > Makefile; make",
            // Each writer: a pipe into tee or dd, a here-string, sed, an
            // output process substitution, eval and a shell's code.
            "printf 'all:\\n' | tee Makefile; make",
            "printf 'all:\\n' | dd of=Makefile; make",
            "cat <<< 'all:' > Makefile; make",
            "sed -i '1i all:' Makefile; make",
            "printf 'all:\\n' > >(tee Makefile); make",
            "eval \"printf 'all:' > Makefile\"; make",
            "bash -c \"printf 'all:' > Makefile\"; make",
            "x=$(printf 'all:'); echo \"$x\" > Makefile; make",
            // An assignment or option that changes what a listed program
            // runs, and programs with such an option.
            "printf x > cat; chmod +x cat; PATH=. cat f",
            "printf x > cat; chmod +x cat; PATH=.; cat f",
            "printf x > cat; chmod +x cat; printf -v PATH .; cat f",
            "printf x > run; chmod +x run; env PATH=. ls",
            "printf x > z; chmod +x z; sort --compress-program=./z a.txt",
            "printf x > g; rsync -e ./g a host:b",
            "printf x > s; chmod +x s; install -s a b",
            "printf x > run; ./run",
            // AC-6: a supplier beside the write, whatever it runs.
            r"printf '%s\n' '\143odeflow update' | xargs printf > commands.fifo",
        ];
        let allow = [
            // A program's own output written to a file: the named residual.
            "cargo build > build.log 2>&1 && grep error build.log",
            "make > build.log",
            "cargo test > test.log 2>&1; echo done",
            "cargo test 2>&1 | tee test.log; grep FAILED test.log",
            "mytool > out.txt; echo done",
            "curl -s https://example.com > out.json && jq . out.json",
            "cargo build --target-dir \"$(pwd)/t\" > b.log 2>&1; grep error b.log",
            "mytool > out.txt; mytool2 out.txt",
            // A lone write, and one command whose substitutions write
            // nothing and that runs after them.
            "printf x > f",
            "printf '%s\\n' 'echo $(date)' > run.sh",
            "echo \"$(date)\" > out.txt",
            "printf '%s\\n' \"$(git rev-parse HEAD)\" > head.txt",
            "cat <<'EOF' > notes.txt\nline\nEOF",
        ];
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Text the shell evaluates as arithmetic is judged as command text:
    /// each command substitution in it runs, and its output is evaluated,
    /// so it is judged as a command whose output a shell runs, with what
    /// the line binds; a variable such text reads that the line binds to
    /// text from outside the line, to unresolved text or to text with a
    /// substitution refuses, as does a word that joins written substitution
    /// text with an expansion (TSK-1476 r22). So does a variable it reads
    /// that the line assigns in a way no word shows or that the shell sets
    /// from text on the line, and text the shell expands as a prompt has
    /// each substitution judged as a command line. One row per context the
    /// bash and zsh manuals list; the command is built by `printf` so that
    /// only this rule finds it.
    #[test]
    #[allow(clippy::too_many_lines)] // one row per context the manuals list
    fn text_the_shell_evaluates_as_arithmetic_is_judged_as_command_text() {
        let built = r#"printf "co%xflow update" 222 | sh"#;
        let refuse: Vec<String> = [
            "(( a['$(B)'] ))",
            "for (( i = a['$(B)']; i < 1; i++ )); do :; done",
            "echo $(( a['$(B)'] ))",
            "echo $[ a['$(B)'] ]",
            "let 'x = a[$(B)]'",
            "[[ 1 -eq 'a[$(B)]' ]]",
            "[[ 'a[$(B)]' -ne 1 ]]",
            "[[ 1 -lt 'a[$(B)]' ]]",
            "[[ 1 -le 'a[$(B)]' ]]",
            "[[ 1 -gt 'a[$(B)]' ]]",
            "if [[ 1 -ge 'a[$(B)]' ]]; then :; fi",
            "[[ -v 'a[$(B)]' ]]",
            "test -v 'a[$(B)]'",
            "[ -v 'a[$(B)]' ]",
            "printf -v 'a[$(B)]' x",
            "read 'a[$(B)]' < /dev/null",
            "read -a 'a[$(B)]' < /dev/null",
            "mapfile 'a[$(B)]' < /dev/null",
            "getopts ab 'a[$(B)]'",
            "unset 'a[$(B)]'",
            "wait -p 'a[$(B)]'",
            "a['$(B)']=1",
            "a=(['$(B)']=1)",
            "declare 'a[$(B)]=1'",
            "declare -i n='a[$(B)]'",
            "typeset -i n='a[$(B)]'",
            "local -i n='a[$(B)]'",
            "integer n='a[$(B)]'",
            "declare -n r='a[$(B)]'",
            "declare -i n; n='a[$(B)]'",
            "echo \"${s:'a[$(B)]'}\"",
            "echo \"${s:0:'a[$(B)]'}\"",
            "echo \"${a['$(B)']}\"",
            "echo \"${#a['$(B)']}\"",
            "echo $a['$(B)']",
            "repeat 'a[$(B)]' do :; done",
            "printf '%d' 'a[$(B)]'",
            "print -f '%d' 'a[$(B)]'",
            "print -v 'a[$(B)]' x",
            "shift 'a[$(B)]'",
            "return 'a[$(B)]'",
            "exit 'a[$(B)]'",
            "break 'a[$(B)]'",
            "continue 'a[$(B)]'",
            "builtin let 'x = a[$(B)]'",
            "command printf -v 'a[$(B)]' x",
            "setopt promptsubst; print -P '$(B)'",
            // A variable such text reads, bound on the line.
            "x='a[$(B)]'; [[ $x -eq 1 ]]",
            "x='a[$(B)]'; (( x ))",
            "x='a[$(B)]'; y=x; (( y ))",
            "x='a[$(B)]'; echo \"${!x}\"",
            "x='a[$(B)]'; echo \"${(P)x}\"",
            "x='$(B)'; echo \"${(e)x}\"",
            "x='$(B)'; echo \"${x@P}\"",
            "x=$(git log -1 --format=%s); (( x ))",
            "for x in *; do (( x )); done",
            // Output a program writes, evaluated.
            "(( $(B) ))",
            "(( $(wc -l < count.txt) > 3 ))",
            "[[ $(git rev-list --count HEAD) -gt 0 ]]",
            "n=$(wc -l < f); (( n > 3 ))",
            "x=$(printf 'co%xflow update' 222); [[ 1 -eq 'a[$($x)]' ]]",
            "[[ 1 -eq 'a[$(eval \"$(printf \"co%xflow update\" 222)\")]' ]]",
            // A word that joins written substitution text with an
            // expansion, and positional parameters a function reads.
            "d='$'; [[ 1 -eq \"a[${d}(sh f)]\" ]]",
            "v=x; [[ 1 -eq 'a[$('\"$v\"')]' ]]",
            "f() { (( $1 )); }; f 'a[$(B)]'",
            "set -- 'a[$(B)]'; (( $1 ))",
            // A variable the line assigns in a way no word shows, or that
            // the shell sets from text on the line.
            "x=1; eval 'x=a[\\$(B)]'; (( x ))",
            "x=1; trap 'x=a[\\$(B)]' DEBUG; (( x ))",
            "x=; : ${x:='a[$(B)]'}; (( x ))",
            "i=0; read i <<< 'a[$(B)]'; (( i ))",
            "read <<< 'a[$(B)]'; (( REPLY ))",
            ": 'a[$(B)]'; (( _ ))",
            "getopts a: o -a 'a[$(B)]'; (( OPTARG ))",
            "for i j in 1 'a[$(B)]'; do (( j )); done",
            "typeset -T FOO foo; FOO='a[$(B)]'; (( foo ))",
            "mapfile -C 'x=a[\\$(B)]' -c 1 a < /dev/null; (( x ))",
            // Text the shell expands as a prompt.
            "PS4='$(B)'; set -x; true",
            "env PS4='$(B)' bash -x -c true",
            "PROMPT4='$(B)'; setopt promptsubst xtrace; true",
            "PROMPT_COMMAND='B' bash -i < /dev/null",
        ]
        .iter()
        .map(|row| row.replace('B', built))
        .chain([format!("[[ 'a[$({built})]' =~ .* ]]; (( BASH_REMATCH ))")])
        .collect();
        let refuse: Vec<&str> = refuse.iter().map(String::as_str).collect();
        let allow = [
            // Arithmetic on plain literals, with no substitution.
            "(( 1 + 2 ))",
            "[[ 3 -eq 3 ]]",
            "echo $(( 2 * 3 ))",
            "n=0; for f in a b; do (( n++ )); done; echo \"$n\"",
            "i=2; echo \"${s:i:1}\"",
            "[[ $count -gt 0 ]] && echo some",
            // `test` and `[` compare integers and evaluate nothing.
            "[ \"$(wc -l < f)\" -gt 3 ] && echo long",
            "test \"$(git rev-list --count HEAD)\" -gt 0",
            // A file the line runs whose content is state the line does not
            // show, and substitution text no shell evaluates.
            "echo 'a[$(sh f)]'",
            "grep -F '$(B)' notes.md",
            "git commit -m 'docs: explain $(x | sh)'",
            "cd \"$(git rev-parse --show-toplevel)\" && cargo test",
            "read -r line < f; echo \"$line\"",
            "x=$(git rev-parse HEAD); echo \"$x\"",
            "printf '%s\\n' '$(B)'",
            "export PATH=\"$(pwd)/bin:$PATH\"",
            "local x=\"$(git rev-parse HEAD)\"",
            // Variables assigned only by words whose values are literal
            // text or arithmetic, where the composer cannot follow the
            // loop or the function.
            "f() { local x=id_rsa; }; f",
            "i=0; while (( i < 3 )); do i=$((i + 1)); done",
            "for i in 1 2 3; do (( i )); done",
            "a=(1 2 3); (( a[1] > 1 ))",
            "alias ll='ls -l'; ll src",
            // A prompt whose substitutions are ordinary commands.
            "PS4='+ ${LINENO}: '; set -x; true",
            "PS4='+ $(date) '; set -x; true",
        ];
        let allow: Vec<String> = allow.iter().map(|row| row.replace('B', built)).collect();
        let allow: Vec<&str> = allow.iter().map(String::as_str).collect();
        let wrong = misjudged(&refuse, &allow);
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// The brief's controls: the admitted lines that name the binary with
    /// a verb draw no finding, and lines that do not name it stay clean
    /// whatever form they take.
    #[test]
    fn admitted_lines_and_lines_that_do_not_name_the_binary_are_clean() {
        for command in [
            "grep -n 'codeflow update' notes.md",
            "rg --no-config 'codeflow init' docs",
            "printf '%s\\n' 'run codeflow update'",
            "git --no-pager log --grep=update --grep=codeflow --all-match",
            "echo 'run codeflow update'",
            "ACK_PAGER=less ack foo",
            "GIT_PAGER=less git log",
            "git -c filter.lfs.process='git-lfs filter-process' -c filter.lfs.clean='git-lfs clean -- %f' log -1",
            "export X=1; make",
            "git lfs pull",
        ] {
            assert!(!runs(command), "{command}");
        }
        // No line of a shell that reads a backslash as a separator is
        // admitted.
        assert!(install_run("echo 'run codeflow update'", false).is_some());
    }

    /// Data that names the verbs outside the admitted subset is judged on
    /// its raw text: the over-refusal the closed list accepts, each with a
    /// listed form that says the same thing (`echo`, `printf '%s'`, `grep`,
    /// `rg --no-config`).
    #[test]
    fn data_off_the_closed_list_that_names_the_verbs_refuses() {
        for command in [
            "codeflow task new --title \"the operator runs codeflow update\"",
            "codeflow feedback new \"codeflow init wrote a file twice\"",
            "[ \"$x\" = \"codeflow update\" ]",
            "cargo nextest run update",
            "cargo run --example init",
            "export NOTE=\"run codeflow update\"",
            "npm --prefix codeflow update",
            "make -C codeflow init",
            "cp -r codeflow update",
            "ack 'codeflow update' docs",
            "rg 'codeflow init' docs",
            "gh search issues -- 'codeflow update'",
            "jq '.steps[] | select(. == \"codeflow update\")' plan.json",
            "git commit -m 'docs: the operator runs codeflow update'",
            "git log --grep='codeflow update'",
            "git --no-pager grep -n 'codeflow update' -- docs",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// The words `gh` passes to `git` after `--` are read as a quoted
    /// argument is: a setting or option value holding a command line.
    #[test]
    fn words_gh_passes_to_git_are_read() {
        for command in [
            "gh repo clone owner/repo -- -c core.sshCommand='env codeflow update'",
            "gh repo clone owner/repo -- --upload-pack='codeflow update'",
            "gh repo fork owner/repo --clone -- -c core.fsmonitor='nice codeflow init'",
            "gh gist clone abc123 -- -c core.sshCommand='env codeflow update'",
            "gh repo clone -u upstream owner/repo dir -- -c core.sshCommand='codeflow init'",
        ] {
            assert!(runs(command), "{command}");
        }
        assert!(!runs("gh repo clone owner/repo -- --depth 1"));
    }

    /// An editor's or pager's own command, given as an option value or a
    /// `+` word, runs a shell command through its command language, so it
    /// is judged on its text as interpreter code is.
    #[test]
    fn editor_commands_are_judged_on_their_text() {
        for command in [
            "less '+!codeflow update' notes.txt",
            "vim -c '!codeflow update' notes.txt",
            "vim '+!codeflow update' notes.txt",
            "nvim --cmd 'silent !codeflow update' notes.txt",
            "vi -c 'r !./target/debug/codeflow init' notes.txt",
            "emacs --eval '(shell-command \"codeflow update\")'",
            "vim --cmd='!codeflow update' notes.txt",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "less -R notes.txt",
            "less +10 notes.txt",
            "vim -c 'set nu' notes.txt",
            "vim +10 notes.txt",
            "emacs --eval '(setq x 1)' notes.txt",
            "vim docs/codeflow-update.md",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// Package managers that run a package's binary by name, a subcommand
    /// that runs a command, and a quoted option value that names a command
    /// after its own `=`.
    #[test]
    fn package_managers_and_option_values_that_run_the_binary_are_found() {
        for command in [
            "yarn codeflow update",
            "pnpm codeflow update",
            "yarn run codeflow update",
            "pnpm run codeflow init --yes",
            "npm explore pkg -- codeflow update",
            "tar -cf a.tar --checkpoint=1 '--checkpoint-action=exec=codeflow update' notes.txt",
            "tar -cf a.tar --checkpoint-action 'exec=codeflow init --yes' notes.txt",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// The word a package runner or manager runs is a program position, so
    /// a word the shell expands there is judged as one: with and without a
    /// subcommand, for each runner and manager this reader knows, and for
    /// each kind of expansion. A word the shell expands among the words of
    /// a program this reader does not know is judged on its raw text.
    #[test]
    fn expanded_words_a_package_runner_runs_are_found() {
        for runner in [
            "yarn",
            "yarn run",
            "yarn exec",
            "yarn dlx",
            "pnpm",
            "pnpm run",
            "pnpm exec",
            "pnpm dlx",
            "pn",
            "npm exec --",
            "npm x",
            "npm explore pkg --",
            "npx",
            "bunx",
            "bun",
            "bun x",
            "bun run",
            "pnpx",
            "pnx",
            "corepack yarn",
            "volta run",
            "deno run",
            "uv run",
            "uvx",
        ] {
            for word in [
                "{codeflow,update}",
                "{codeflow,} update",
                "codefl?w update",
                "codefl[o]w init --yes",
                "=codeflow update",
                "$B update",
            ] {
                let command = format!("{runner} {word}");
                assert!(runs(&command), "{command}");
            }
        }
        for command in [
            "mise exec -- {codeflow,update}",
            "mise exec -- {codeflow,} update",
            "mise exec -- =codeflow update",
            "tool --run {./target/debug/codeflow,init}",
        ] {
            assert!(runs(command), "{command}");
        }
    }

    /// npm reads its operands as data only after one of its own commands
    /// that never runs a program, in its exact name or a documented alias:
    /// an abbreviation, a command that runs a script or a binary, an option
    /// this reader does not know before the command, or an expanded word
    /// there leaves npm read as a package runner.
    #[test]
    fn npm_runs_the_binary_after_any_word_but_its_own_commands() {
        for command in [
            "npm exe -- codeflow update",
            "npm exe codeflow update",
            "npm expl pkg -- codeflow update",
            "npm explo pkg -- codeflow update",
            "npm explor pkg -- codeflow update",
            "npm run codeflow update",
            "npm run-script codeflow update",
            "npm rum codeflow update",
            "npm urn codeflow init",
            "npm t -- codeflow update",
            "npm Exec codeflow update",
            "npm --prefix codeflow exe -- codeflow update",
            "npm -g exe codeflow update",
            "npm --registry update exe -- codeflow update",
            "npm --reg update exe -- codeflow update",
            "npm --prefix -x update codeflow update",
            "npm -- exe codeflow update",
            "npm $SUB codeflow update",
            "npm --prefix $D update codeflow update",
            "npm exe -- {codeflow,update}",
        ] {
            assert!(runs(command), "{command}");
        }
        for command in [
            "npm -g update codeflow",
            "npm update codeflow",
            "npm up codeflow",
            "npm i codeflow",
            "npm uninstall codeflow",
            "npm ls codeflow",
            "npm -- update codeflow",
            "npm init -y",
            "npm run build",
            "yarn add codeflow",
            "pnpm add -D codeflow",
            "pnpm install --frozen-lockfile",
            "cargo test {a,b}",
            "cp {a,b} codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    #[test]
    fn a_backslash_path_is_read_on_a_shell_that_takes_it_as_a_separator() {
        for command in [
            ".\\target\\debug\\codeflow.exe update",
            "C:\\Users\\me\\.cargo\\bin\\codeflow.exe init --yes",
        ] {
            assert!(install_run(command, false).is_some(), "{command}");
        }
        assert!(install_run(".\\target\\debug\\codeflow.exe doctor", false).is_none());
    }

    #[test]
    fn a_run_inside_a_quoted_argument_says_so() {
        let run = install_run("python3 -m tool \"codeflow update\"", true).unwrap();
        assert!(run.in_argument, "{run:?}");
        let run = install_run("codeflow update", true).unwrap();
        assert!(!run.in_argument, "{run:?}");
    }

    #[test]
    fn forms_that_write_nothing_and_other_programs_are_not_runs() {
        for command in [
            "codeflow update --help",
            "codeflow update -h",
            "codeflow init --help",
            "codeflow init -h",
            "codeflow help update",
            "codeflow help init",
            "codeflow --help",
            "codeflow --version",
            "codeflow -V",
            "./target/debug/codeflow --version",
            "codeflow",
            "codeflow doctor",
            "codeflow test --mode quick",
            "command -v codeflow",
            "which codeflow",
            "cargo run -- doctor",
            "cargo run --bin codeflow -- task status",
            "cargo build",
            "cargo test update",
            "cargo update",
            "cargo init --vcs none",
            "npm init -y",
            "brew update",
            "rustup update",
            "echo the operator runs codeflow update",
            "printf 'codeflow update\\n' > /dev/null",
            "grep -rn 'codeflow update' docs",
            "xargs codeflow doctor",
            "find . -name codeflow",
            "find . -name codeflow -print",
            "ls ./target/*/codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    #[test]
    fn interpreter_code_and_unparsed_text_name_the_binary_and_a_verb() {
        for code in [
            "import subprocess; subprocess.run(['./codeflow', 'update'])",
            "require('child_process').execFileSync('codeflow', ['init'])",
            "system(\"codeflow\", \"update\")",
            "cargo run -- init",
            "printf 'codeflow update\\n' > notes.txt",
            "echo -e 'codeflow init\\tx'",
            "cod\\eflow update",
        ] {
            assert!(raw_run(code).is_some(), "{code}");
        }
        for code in [
            "print('codeflow')",
            "d.update(x)",
            "cargo build && run tests",
        ] {
            assert!(raw_run(code).is_none(), "{code}");
        }
    }
}
