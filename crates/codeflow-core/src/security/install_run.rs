//! Install runs (TSK-1476): a run of the `CodeFlow` binary as `update` or
//! `init`, which writes the project's install files: harness settings,
//! hooks, policy and workflows. Only the operator runs them, so exec-guard
//! refuses such a run from an agent under `git.hook_integrity`, in every
//! folder and whichever program starts it.
//!
//! Each simple command is read as git-guard reads it
//! (`git_guard::simple_commands`).
//! A word that names the binary is judged with the words after it,
//! whichever program comes before it, so a launcher the other readers do
//! not unwrap still has its run judged. `cargo run` and `cargo r` are
//! judged by every later word, since cargo passes them to the binary it
//! builds. A program word or first operand the guard cannot resolve refuses
//! where the verb could follow. Only the exact help and version shapes are
//! allowed. A line the reader cannot resolve, and interpreter code, are
//! judged on their raw text: the binary's name followed, anywhere later, by
//! a verb.
//!
//! A program an agent builds, renames or copies, and the verb's words
//! written to a file and run later, are not named on the line. The write
//! side bounds those: the preset denies on each worktree's copies and the
//! OS sandbox.

use super::headless::{basename, shell_command_string};
use super::launchers::{LAUNCHERS, PACKAGE_RUNNERS, SHELLS, SUPPLIERS};
use crate::hooks::git_guard::simple_commands;

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

/// Programs that print, search or show the words they are given and never
/// run one, so the binary's name among their words is data.
const DATA_PROGRAMS: &[&str] = &[
    "cat", "echo", "gh", "git", "grep", "head", "ls", "printf", "rg", "tail", "type", "wc", "which",
];

/// Words after which a supplier or package runner starts the command that
/// follows.
const STARTS_A_COMMAND: &[&str] = &["-exec", "-execdir", "-ok", "-okdir", "exec", "x", "dlx"];

/// Suppliers that add the words they read after the command they run.
const APPENDING: &[&str] = &["parallel", "xargs"];

/// An install run found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRun {
    /// The form matched, such as `codeflow update` or `cargo run`.
    pub form: &'static str,
    /// `false` when the line could not be parsed and its raw text named the
    /// binary with a verb.
    pub parsed: bool,
}

/// The first install run in `command`, if any.
#[must_use]
pub fn install_run(command: &str) -> Option<InstallRun> {
    find(command, 0)
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

/// Judge one simple command's argument vector.
fn classify(argv: &[String], depth: usize) -> Found {
    let Some(program) = argv.first() else {
        return Found::Nothing;
    };
    let name = basename(program);
    if DATA_PROGRAMS.contains(&name.as_str()) {
        return Found::Nothing;
    }
    let mut found = Found::Nothing;
    if unresolved(program)
        || matches!(name.as_str(), "alias" | "function" | "source" | ".")
        || (SHELLS.contains(&name.as_str()) && shell_command_string(&argv[1..]).is_none())
    {
        found = Found::Unresolved;
    }
    for (at, word) in argv.iter().enumerate() {
        let after = &argv[at + 1..];
        let before = &argv[..at];
        let run = match basename(word).as_str() {
            BINARY if runs_a_verb(after, supplied(before)) => Some(binary_form(after)),
            "cargo" if cargo_runs_a_verb(after) => Some("cargo run"),
            _ if (word == "{}" || unresolved(word))
                && starts_a_command(before)
                && first_operand(after).is_some_and(|operand| is_verb(operand)) =>
            {
                Some("an unresolved program with an install verb")
            }
            _ => None,
        };
        if let Some(form) = run {
            return Found::Run(InstallRun { form, parsed: true });
        }
        if at > 0 && word.contains(char::is_whitespace) {
            if let Some(run) = find(word, depth + 1) {
                return Found::Run(run);
            }
        }
    }
    found
}

/// Whether the words after the binary's name run a verb. `supplied` is
/// true when a supplier before it adds words of its own after them.
fn runs_a_verb(after: &[String], supplied: bool) -> bool {
    let rest = without_separators(after);
    if !supplied
        && PRINTS_ONLY
            .iter()
            .any(|shape| shape.len() == rest.len() && shape.iter().zip(rest).all(|(a, b)| a == b))
    {
        return false;
    }
    let open = |word: &String| is_verb(word) || unresolved(word) || supplier_slot(word);
    match rest.first() {
        None => supplied,
        Some(first) if open(first) => true,
        Some(first) if first.starts_with('-') => rest[1..].iter().any(open),
        Some(_) => supplied && rest.iter().any(open),
    }
}

/// `cargo run` or `cargo r` with a verb as any later word.
fn cargo_runs_a_verb(after: &[String]) -> bool {
    after
        .iter()
        .position(|word| word == "run" || word == "r")
        .is_some_and(|run| after[run + 1..].iter().any(|word| is_verb(word)))
}

/// The raw-text form of [`cargo_runs_a_verb`] and the binary's run: the
/// verb anywhere among `words`.
fn names_a_verb(words: &[&str]) -> bool {
    words.iter().any(|word| is_verb(word))
}

/// The form reported for a run of the binary.
fn binary_form(after: &[String]) -> &'static str {
    match without_separators(after).first().map(String::as_str) {
        Some("update") => "codeflow update",
        Some("init") => "codeflow init",
        _ => "codeflow with an unresolved or supplied verb",
    }
}

/// The words after any leading `--`.
fn without_separators(words: &[String]) -> &[String] {
    let skip = words.iter().take_while(|word| *word == "--").count();
    &words[skip..]
}

/// The first word after any leading `--`.
fn first_operand(words: &[String]) -> Option<&String> {
    without_separators(words).first()
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
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(command: &str) -> bool {
        install_run(command).is_some()
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
