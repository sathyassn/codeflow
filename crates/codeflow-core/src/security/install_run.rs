//! Install runs (TSK-1476): a run of the `CodeFlow` binary as `update` or
//! `init`, which writes the project's install files: harness settings,
//! hooks, policy and workflows. Only the operator runs them, so exec-guard
//! refuses such a run from an agent under `git.hook_integrity`, in every
//! folder and whichever program starts it.
//!
//! Each simple command is read as git-guard reads it
//! (`git_guard::simple_commands`), with the lexer's mark of whether the
//! shell passes each word as written. A word that names the binary is
//! judged with the words after it when it can be the program that runs: the
//! first word, a word after a launcher, supplier or package runner, or any
//! word of a program this reader does not know. A program known never to
//! run its operands (`cp`, `make` and the like) gives no such word; `npm`
//! is read that way except after a subcommand that runs a command.
//! `cargo run` and `cargo r` are judged by the words cargo passes to the
//! program it builds. A word the shell expands (a variable, a substitution,
//! a pattern, braces or a leading `=`) in the program position or as the
//! first operand refuses where the verb could follow. Only the exact help
//! and version shapes are allowed.
//!
//! A quoted argument holding a space is read again as a command line,
//! except for programs that only print, search or record the words they are
//! given. An assignment's value is read the same way when a command on the
//! line can read it: a prefix assignment's command, or any later command
//! that is not one of those programs. A line the reader cannot resolve, and
//! interpreter code, are judged on their raw text: the binary's name
//! followed, anywhere later, by a verb.
//!
//! A program an agent builds, renames or copies, and the verb's words
//! written to a file and run later, are not named on the line. They are a
//! named residual.

use super::headless::{basename, shell_command_string};
use super::launchers::{LAUNCHERS, PACKAGE_RUNNERS, SHELLS, SUPPLIERS};
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

/// Programs that print, search, compare or show the words they are given
/// and never run one, so the binary's name among their words is data.
const DATA_PROGRAMS: &[&str] = &[
    "[", "ack", "ag", "cat", "echo", "gh", "git", "grep", "head", "jq", "ls", "printf", "rg",
    "tail", "test", "type", "wc", "which",
];

/// Options of a [`DATA_PROGRAMS`] search program that run a program: with
/// one of them the call is judged as any other command, and a value naming
/// the binary refuses.
const RUNS_THROUGH_OPTION: &[&str] = &["--pager", "--pre"];

/// The binary's own verbs that record or read the words they are given and
/// never run one, so their free text is data.
const RECORD_VERBS: &[&str] = &["adr", "epic", "feedback", "recall", "spec", "task"];

/// Programs known never to run one of their operands as a program, so a
/// folder or file named like the binary among their words is data. A
/// quoted argument holding a space is still read as a command line, since
/// a value such as a `make` variable can be. `npm` runs a command only
/// after a word in [`STARTS_A_COMMAND`]; package managers that run a
/// package's binary by name, such as `yarn` and `pnpm`, are not listed.
const NOT_LAUNCHERS: &[&str] = &[
    "chmod", "chown", "cmp", "cp", "cut", "diff", "du", "file", "ln", "make", "mkdir", "mv", "npm",
    "rm", "rmdir", "sort", "stat", "tar", "tee", "touch", "tr", "uniq",
];

/// Shell builtins that set variables from their operands.
const DECLARES: &[&str] = &["declare", "export", "local", "readonly", "typeset"];

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
    /// `false` when the line could not be parsed and its raw text named the
    /// binary with a verb.
    pub parsed: bool,
    /// `true` when the run was found inside a quoted argument or an
    /// assignment's value, which the guard read as a command line.
    pub in_argument: bool,
}

/// The first install run in `command`, if any. `posix` is false for a
/// shell that reads a backslash as a path separator, such as `PowerShell`,
/// where the command is also read with each backslash as a `/`.
#[must_use]
pub fn install_run(command: &str, posix: bool) -> Option<InstallRun> {
    find(command, 0).or_else(|| {
        (!posix && command.contains('\\'))
            .then(|| find(&command.replace('\\', "/"), 0))
            .flatten()
    })
}

/// One simple command: its argument vector from the program on, whether
/// the shell passes each of those words as written, and the values of the
/// assignments before the program.
struct Simple {
    argv: Vec<String>,
    literal: Vec<bool>,
    values: Vec<String>,
}

/// The simple commands of `command`, read as
/// `git_guard::simple_commands` reads them, with each word's literal mark
/// and the assignment values the launcher strip drops.
fn simple(command: &str) -> Vec<Simple> {
    expand_commands(command)
        .iter()
        .map(|segment| {
            let (mut words, mut literal) = command_argv_literal(segment);
            let total = words.len();
            strip_reserved_words(&mut words);
            literal.drain(..total - words.len());
            let start = strip_launchers(&words)
                .map_or(words.len(), |(_, args)| words.len() - args.len() - 1);
            let values = words[..start]
                .iter()
                .filter_map(|word| assigned(word))
                .collect();
            Simple {
                argv: words[start..].to_vec(),
                literal: literal[start..].to_vec(),
                values,
            }
        })
        .collect()
}

/// The value of a `NAME=value` word.
fn assigned(word: &str) -> Option<String> {
    let (name, value) = word.split_once('=')?;
    let mut chars = name.chars();
    let identifier = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    identifier.then(|| value.to_string())
}

/// What one simple command is.
enum Found {
    Run(InstallRun),
    /// It runs something this reader cannot resolve.
    Unresolved,
    Nothing,
}

fn find(command: &str, depth: usize) -> Option<InstallRun> {
    if depth > 4 {
        return raw_run(command);
    }
    let mut unresolved = command.contains("<<<");
    // Values assigned on the line, read once a later command can read them.
    let mut pending: Vec<String> = Vec::new();
    for command in simple(command) {
        let Some(program) = command.argv.first() else {
            pending.extend(command.values);
            continue;
        };
        for value in &command.values {
            if let Some(run) = in_value(value, depth) {
                return Some(run);
            }
        }
        let name = basename(program);
        if DECLARES.contains(&name.as_str()) {
            pending.extend(command.argv[1..].iter().filter_map(|word| assigned(word)));
            continue;
        }
        if !DATA_PROGRAMS.contains(&name.as_str()) {
            for value in pending.drain(..) {
                if let Some(run) = in_value(&value, depth) {
                    return Some(run);
                }
            }
        }
        match classify(&command.argv, &command.literal, depth) {
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

/// A run inside a quoted argument or an assignment's value, read as a
/// command line. A value whose first word holds an `=`, such as an
/// option's `exec=<command>`, is also read from after that `=`.
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

/// Judge one simple command's argument vector, with whether the shell
/// passes each word as written.
fn classify(argv: &[String], literal: &[bool], depth: usize) -> Found {
    let Some(program) = argv.first() else {
        return Found::Nothing;
    };
    let written = |at: usize| literal.get(at).copied().unwrap_or(false) && !unresolved(&argv[at]);
    let name = basename(program);
    if DATA_PROGRAMS.contains(&name.as_str()) {
        return runs_through_option(argv, &written);
    }
    if name == BINARY
        && written(0)
        && first_operand(&argv[1..]).is_some_and(|operand| RECORD_VERBS.contains(&operand.as_str()))
        && argv[1..]
            .iter()
            .position(|word| word != "--")
            .is_some_and(|at| written(at + 1))
    {
        return Found::Nothing;
    }
    let known = NOT_LAUNCHERS.contains(&name.as_str());
    let mut found = Found::Nothing;
    for (at, word) in argv.iter().enumerate() {
        let after = &argv[at + 1..];
        let before = &argv[..at];
        let in_program_position = at == 0 || starts_a_command(before);
        if in_program_position
            && (!written(at)
                || matches!(
                    basename(word).as_str(),
                    "alias" | "function" | "source" | "."
                )
                || (SHELLS.contains(&basename(word).as_str())
                    && shell_command_string(after).is_none()))
        {
            found = Found::Unresolved;
        }
        let may_run = in_program_position || !known;
        let open = |at: usize| is_verb(&argv[at]) || !written(at) || supplier_slot(&argv[at]);
        let run = match program_word(word, written(at)).as_str() {
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
            _ => None,
        };
        if let Some(form) = run {
            return Found::Run(InstallRun {
                form,
                parsed: true,
                in_argument: false,
            });
        }
        if at > 0 && word.contains(shell_blank) {
            if let Some(run) = in_value(word, depth) {
                return Found::Run(run);
            }
        }
    }
    found
}

/// A search program run with an option that runs a program: refused when
/// the option's value names the binary or cannot be read, and otherwise
/// judged no further.
fn runs_through_option(argv: &[String], written: &dyn Fn(usize) -> bool) -> Found {
    for (at, word) in argv.iter().enumerate().skip(1) {
        let Some(option) = RUNS_THROUGH_OPTION
            .iter()
            .find(|option| word == *option || word.starts_with(&format!("{option}=")))
        else {
            continue;
        };
        let (value, value_at) = match word.strip_prefix(&format!("{option}=")) {
            Some(value) => (value, at),
            None => match argv.get(at + 1) {
                Some(value) => (value.as_str(), at + 1),
                None => continue,
            },
        };
        let first = value
            .split(shell_blank)
            .find(|part| !part.is_empty())
            .unwrap_or(value);
        if !written(value_at) || basename(first) == BINARY {
            return Found::Run(InstallRun {
                form: "codeflow with a supplied verb",
                parsed: true,
                in_argument: false,
            });
        }
    }
    Found::Nothing
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

/// The raw-text judgement for a line that could not be resolved, and for
/// interpreter code: the binary's name followed, anywhere later, by a
/// verb, or `cargo` followed by `run` or `r` and then a verb.
#[must_use]
pub(crate) fn raw_run(text: &str) -> Option<InstallRun> {
    let plain: String = text
        .chars()
        .filter(|c| !matches!(c, '\\' | '\'' | '"'))
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

    /// A shell that reads its input behind a launcher the reader does not
    /// strip has the line's text judged; an assignment's value is read when
    /// a command can read it; a search program's option that runs a
    /// program is judged.
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
        // A program this reader does not know: a word the shell expands
        // is judged on its raw text with the words after it.
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
            "npm --prefix codeflow update",
            "npm -C codeflow update",
            "npm --prefix=codeflow update",
            "npm --workspace codeflow update",
            "npm -g update codeflow",
            "npm update codeflow",
            "npm up codeflow",
            "npm i codeflow",
            "npm view codeflow init",
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
            "codeflow",
            "codeflow doctor",
            "codeflow test --mode quick",
            "command -v codeflow",
            "which codeflow",
            "cargo run -- doctor",
            "cargo build",
            "cargo test update",
            "cargo update",
            "cargo init --vcs none",
            "npm init -y",
            "brew update",
            "rustup update",
            "echo the operator runs codeflow update",
            "printf 'codeflow update\\n' > notes.txt",
            "grep -rn 'codeflow update' docs",
            "git commit -m 'docs: the operator runs codeflow update'",
            "xargs codeflow doctor",
            "find . -name codeflow",
            "find . -name codeflow -print",
            "ls ./target/*/codeflow",
        ] {
            assert!(!runs(command), "{command}");
        }
    }

    /// Words that only name the verbs as data, which no agent's install
    /// run needs refused.
    #[test]
    fn data_that_names_the_verbs_is_not_a_run() {
        for command in [
            "codeflow task new --title \"the operator runs codeflow update\"",
            "codeflow feedback new \"codeflow init wrote a file twice\"",
            "codeflow epic new --title \"codeflow update reads the pin\"",
            "codeflow task new codeflow update",
            "[ \"$x\" = \"codeflow update\" ]",
            "test \"$x\" = \"codeflow init\"",
            "cargo nextest run update",
            "cargo +stable nextest run init",
            "cargo run --example init",
            "cargo run --example init -- --help",
            "cargo run -p codeflow-cli -- task update TSK-1",
            "cargo run --bin codeflow -- task status",
            "export NOTE=\"run codeflow update\"",
            "NOTE=\"run codeflow update\"; echo done",
            "npm --prefix codeflow update",
            "make -C codeflow init",
            "cp -r codeflow update",
            "ag 'codeflow init' docs",
            "ack 'codeflow update' docs",
            "jq '.steps[] | select(. == \"codeflow update\")' plan.json",
            "rg --pre ./tools/pdftext pattern docs",
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
