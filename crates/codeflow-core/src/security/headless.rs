//! Headless peer runs (TSK-136): `claude -p`, `codex exec` and `grok -p`.
//!
//! Peer seats run interactively, never headless (cf-delegate): a one-shot
//! headless run has no verified native session, no task tools or guards, and
//! no recheckable thread. This classifier finds such a run in a shell command
//! so exec-guard can warn (the default) or refuse. It looks only at the
//! program position of each simple command, after the usual launch wrappers
//! (`env`, `command`, `nohup`, `sudo`, a shell `-c`), so a string that merely
//! mentions `claude -p`, such as a commit message or a `grep` pattern, is not a
//! run. It is a small classifier, not a shell parser.

use super::dangerous::{command_tokens, effective_invocation, program_name};

/// A headless peer run found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessRun {
    /// The peer CLI: `claude`, `codex` or `grok`.
    pub peer: &'static str,
    /// The headless form matched, such as `claude -p` or `codex exec`.
    pub form: &'static str,
}

/// The first headless peer run in `command`, if any.
#[must_use]
pub fn headless_peer_run(command: &str) -> Option<HeadlessRun> {
    find(command, 0)
}

fn find(command: &str, depth: usize) -> Option<HeadlessRun> {
    if depth > 3 {
        return None;
    }
    segments(command).into_iter().find_map(|segment| {
        let tokens = command_tokens(segment);
        let start = tokens
            .iter()
            .position(|token| !is_assignment(token))
            .unwrap_or(tokens.len());
        let tokens = &tokens[start..];
        // A shell `-c` string can hold several commands: classify it whole.
        if let Some(inner) = shell_command_string(tokens) {
            if let Some(run) = find(&inner, depth + 1) {
                return Some(run);
            }
        }
        classify(&effective_invocation(tokens))
    })
}

fn classify(invocation: &[String]) -> Option<HeadlessRun> {
    let program = program_name(invocation.first()?);
    let program = program.strip_suffix(".exe").unwrap_or(&program);
    let args = &invocation[1..];
    let has = |flags: &[&str]| args.iter().any(|arg| flags.contains(&arg.as_str()));
    match program {
        "claude" if has(&["-p", "--print"]) => Some(HeadlessRun {
            peer: "claude",
            form: "claude -p",
        }),
        "grok" if has(&["-p", "--print"]) => Some(HeadlessRun {
            peer: "grok",
            form: "grok -p",
        }),
        "codex" if codex_subcommand(args).is_some_and(|sub| sub == "exec" || sub == "e") => {
            Some(HeadlessRun {
                peer: "codex",
                form: "codex exec",
            })
        }
        _ => None,
    }
}

/// Codex's subcommand: the first positional argument, skipping the values
/// of its global options that take one.
fn codex_subcommand(args: &[String]) -> Option<&str> {
    const WITH_VALUE: &[&str] = &[
        "-m",
        "--model",
        "-c",
        "--config",
        "-p",
        "--profile",
        "-C",
        "--cd",
        "-s",
        "--sandbox",
        "-a",
        "--ask-for-approval",
        "-i",
        "--image",
        "--add-dir",
        "--local-provider",
        "--enable",
        "--disable",
    ];
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if WITH_VALUE.contains(&arg.as_str()) {
            args.next();
        } else if !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

/// The string after `-c` for `sh`, `bash` or `zsh`.
fn shell_command_string(tokens: &[String]) -> Option<String> {
    let program = program_name(tokens.first()?);
    if !matches!(program.as_str(), "sh" | "bash" | "zsh") {
        return None;
    }
    let index = tokens.iter().skip(1).position(|arg| {
        arg == "-c"
            || (arg.starts_with('-')
                && !arg.starts_with("--")
                && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
                && arg.contains('c'))
    })?;
    tokens.get(index + 2).cloned()
}

/// `NAME=value` before the program.
fn is_assignment(token: &str) -> bool {
    token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit())
    })
}

/// Split on `;`, `&`, `|` and newlines outside quotes, so a separator inside
/// a quoted argument does not start a new command.
fn segments(command: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (index, ch) in command.char_indices() {
        match (quote, ch) {
            (Some(open), c) if c == open => quote = None,
            (None, '"' | '\'') => quote = Some(ch),
            (None, ';' | '&' | '|' | '\n') => {
                out.push(&command[start..index]);
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&command[start..]);
    out.into_iter().filter(|s| !s.trim().is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(command: &str) -> Option<&'static str> {
        headless_peer_run(command).map(|run| run.peer)
    }

    #[test]
    fn headless_runs_are_found() {
        for (command, expected) in [
            ("claude -p 'review this'", "claude"),
            ("claude --print --model opus 'x'", "claude"),
            ("/usr/local/bin/claude -p hi", "claude"),
            ("codex exec 'fix the bug'", "codex"),
            ("codex --model gpt-6-sol exec 'x'", "codex"),
            ("codex e 'x'", "codex"),
            ("grok -p 'x'", "grok"),
            ("FOO=1 claude -p x", "claude"),
            ("env HOME=/tmp codex exec x", "codex"),
            ("cd /w && codex exec x", "codex"),
            ("echo x | claude -p", "claude"),
            ("bash -lc 'cd /w; claude -p x'", "claude"),
            ("nohup grok -p x &", "grok"),
        ] {
            assert_eq!(peer(command), Some(expected), "{command}");
        }
    }

    #[test]
    fn other_commands_are_not_runs() {
        for command in [
            "codex --version",
            "codex",
            "codex resume --last",
            "claude",
            "claude --help",
            "claude --model opus --effort high",
            "grok --version",
            "git commit -m 'run claude -p; then codex exec'",
            "grep -rn \"codex exec\" docs",
            "echo 'claude -p'",
            "cargo test -p codeflow-core",
            "rg -p pattern",
        ] {
            assert_eq!(peer(command), None, "{command}");
        }
    }
}
