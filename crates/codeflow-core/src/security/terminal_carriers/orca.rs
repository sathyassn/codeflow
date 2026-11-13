//! Orca command, terminal-input and prompt routes.

use super::{options, Carried, Grammar, Options, Place, Run, Words};

const ROUTING: &[&str] = &["--pairing-code", "--environment"];

/// Read only the forms whose carried text and option roles are known.
#[allow(clippy::too_many_lines)] // Keep the supported text routes beside their option grammars.
pub(super) fn read(words: &Words<'_>, place: &Place, input: Option<&str>, out: &mut Carried) {
    let args = words.args;
    if args.len() == 1
        && matches!(
            args.first().map(String::as_str),
            Some("--help" | "--version" | "help")
        )
    {
        return;
    }
    let pair = (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    );
    let route = format!(
        "{} {}",
        pair.0.unwrap_or_default(),
        pair.1.unwrap_or_default()
    );
    let reads_terminal =
        pair.0 == Some("terminal") && matches!(pair.1, Some("read" | "list" | "wait" | "close"));
    if !reads_terminal && !super::inventory::contains("orca", &route) {
        out.unread("orca", "this text route is not in the carrier inventory");
        return;
    }
    match pair {
        (Some("terminal"), Some("create")) => shell(
            words,
            place,
            "orca terminal create",
            "--command",
            &Grammar {
                flags: &["--focus"],
                valued: &["--worktree", "--title", "--command", "--shell"],
            },
            out,
        ),
        (Some("terminal"), Some("split")) => shell(
            words,
            &Place {
                cwd: None,
                home: place.home.clone(),
            },
            "orca terminal split",
            "--command",
            &Grammar {
                flags: &[],
                valued: &["--terminal", "--direction", "--command", "--retry-split"],
            },
            out,
        ),
        (Some("terminal"), Some("send")) => prompt(
            words,
            input,
            "orca terminal send",
            "--text",
            &Grammar {
                flags: &["--enter", "--interrupt"],
                valued: &["--terminal", "--text", "--wait-submit", "--retry-request"],
            },
            out,
        ),
        (Some("terminal"), Some("read" | "list" | "wait" | "close")) => {
            let grammar = Grammar {
                flags: if pair.1 == Some("read") {
                    &["--all", "--full", "--screen"]
                } else {
                    &["--all", "--full"]
                },
                valued: &[
                    "--terminal",
                    "--worktree",
                    "--lines",
                    "--for",
                    "--timeout",
                    "--timeout-ms",
                    "--stable-for",
                    "--since",
                ],
            };
            if let Err(why) = parse(args, &grammar) {
                out.unread("orca terminal", why);
            }
        }
        (Some("computer"), Some("type-text" | "paste-text")) => prompt(
            words,
            input,
            "orca computer text",
            "--text",
            &Grammar {
                flags: &[
                    "--text-stdin",
                    "--restore-window",
                    "--no-screenshot",
                    "--full-tree",
                ],
                valued: &["--app", "--text", "--window-id", "--window-index"],
            },
            out,
        ),
        (Some("emulator"), Some("type")) => prompt(
            words,
            input,
            "orca emulator type",
            "--text",
            &Grammar {
                flags: &[],
                valued: &["--text", "--device", "--udid"],
            },
            out,
        ),
        (Some("agent"), Some("launch")) => prompt(
            words,
            input,
            "orca agent launch",
            "--prompt",
            &Grammar {
                flags: &["--focus"],
                valued: &[
                    "--agent",
                    "--worktree",
                    "--prompt",
                    "--model",
                    "--effort",
                    "--surface",
                    "--retry-launch",
                ],
            },
            out,
        ),
        (Some("worktree"), Some("create")) => prompt(
            words,
            input,
            "orca worktree create",
            "--prompt",
            &Grammar {
                flags: &["--no-parent", "--run-hooks", "--activate"],
                valued: &[
                    "--name",
                    "--repo",
                    "--project",
                    "--host",
                    "--project-host-setup",
                    "--agent",
                    "--prompt",
                    "--model",
                    "--effort",
                    "--surface",
                    "--setup",
                    "--base-branch",
                    "--branch",
                    "--reference",
                    "--reference-site",
                    "--issue",
                    "--pr",
                    "--linear-issue",
                    "--gitlab-issue",
                    "--gitlab-mr",
                    "--comment",
                    "--parent-worktree",
                    "--parent-environment",
                    "--retry-launch",
                ],
            },
            out,
        ),
        (Some("automations"), Some("create")) => {
            let grammar = Grammar {
                flags: &[
                    "--enabled",
                    "--disabled",
                    "--reuse-session",
                    "--fresh-session",
                ],
                valued: &[
                    "--name",
                    "--trigger",
                    "--schedule",
                    "--prompt",
                    "--provider",
                    "--precheck",
                    "--precheck-timeout",
                    "--repo",
                    "--workspace",
                    "--project",
                    "--host",
                    "--project-host-setup",
                    "--setup",
                    "--source-context",
                    "--workspace-mode",
                    "--base-branch",
                    "--time",
                    "--day",
                    "--timezone",
                    "--missed-run-grace-minutes",
                ],
            };
            shell(
                words,
                place,
                "orca automations create",
                "--precheck",
                &grammar,
                out,
            );
            prompt(
                words,
                input,
                "orca automations create",
                "--prompt",
                &grammar,
                out,
            );
        }
        (Some("orchestration"), Some("worker-start")) => prompt(
            words,
            input,
            "orca orchestration worker-start",
            "--spec",
            &Grammar {
                flags: &["--keep-open", "--full"],
                valued: &[
                    "--task",
                    "--spec",
                    "--spec-file",
                    "--on",
                    "--worktree",
                    "--agent",
                    "--terminal",
                    "--task-title",
                    "--deps",
                    "--parent",
                    "--model",
                    "--effort",
                    "--name",
                    "--repo",
                    "--base-branch",
                    "--display-name",
                    "--comment",
                    "--setup",
                    "--retry-of",
                    "--timeout-ms",
                    "--run",
                    "--from",
                    "--retry-request",
                ],
            },
            out,
        ),
        (Some("orchestration"), Some("send" | "ask" | "reply")) => prompt(
            words,
            input,
            "orca orchestration message",
            "--body",
            &Grammar {
                flags: &["--full"],
                valued: &[
                    "--id",
                    "--to",
                    "--run",
                    "--from",
                    "--subject",
                    "--body",
                    "--type",
                    "--priority",
                    "--thread-id",
                    "--payload",
                    "--task-id",
                    "--dispatch-id",
                    "--dispatch-capability",
                    "--outcome",
                    "--files-modified",
                    "--report-path",
                    "--phase",
                    "--retry-request",
                ],
            },
            out,
        ),
        (Some("orchestration"), Some("dispatch")) => out.unread(
            "orca orchestration dispatch",
            "the stored task text cannot be established from this command",
        ),
        (Some("plugin"), Some("dev" | "test" | "check")) => out.unread(
            "orca plugin",
            "plugin code runs from files outside the guarded shell; this text rule cannot judge it",
        ),
        _ => out.unread(
            "orca",
            "this command is not one whose text routes the guard reads",
        ),
    }
}

fn parse(args: &[String], grammar: &Grammar) -> Result<Options, String> {
    // Remote contexts cannot be mistaken for a local command selector.
    let mut at = 2;
    while at < args.len() {
        let word = &args[at];
        let name = word.split('=').next().unwrap_or(word);
        if ROUTING.contains(&name) {
            return Err("the remote execution context cannot be established".into());
        }
        at += 1 + usize::from(grammar.valued.contains(&name) && !word.contains('='));
    }
    let parsed = options(args, 2, grammar, true)?;
    if parsed.operands != args.len() {
        return Err("unexpected operands leave the text route unknown".into());
    }
    Ok(parsed)
}

fn shell(
    words: &Words<'_>,
    place: &Place,
    form: &'static str,
    option: &str,
    grammar: &Grammar,
    out: &mut Carried,
) {
    let parsed = match parse(words.args, grammar) {
        Ok(parsed) => parsed,
        Err(why) => {
            out.unread(form, why);
            return;
        }
    };
    if parsed.last("--shell").is_some() {
        out.unread(
            form,
            "the selected shell cannot be established by this reader",
        );
    }
    let selector = parsed.last("--worktree");
    let known = selector.is_none_or(|value| matches!(value, "active" | "current"))
        && !["--repo", "--workspace", "--project", "--project-host-setup"]
            .iter()
            .any(|key| parsed.last(key).is_some());
    for text in parsed.all(option) {
        super::terminal_inputs::text(text, form, out);
        if crate::hooks::git_guard::unresolved_word(text) {
            out.unread(form, "the command text cannot be read");
        } else {
            out.runs.push(Run {
                form,
                line: text.to_string(),
                directory: place.cwd.clone().filter(|_| known),
            });
        }
    }
}

fn prompt(
    words: &Words<'_>,
    input: Option<&str>,
    form: &'static str,
    option: &str,
    grammar: &Grammar,
    out: &mut Carried,
) {
    let parsed = match parse(words.args, grammar) {
        Ok(parsed) => parsed,
        Err(why) => {
            out.unread(form, why);
            return;
        }
    };
    if parsed.last("--spec-file").is_some() || parsed.last("--task").is_some() {
        out.unread(
            form,
            "the prompt comes from a file or stored task this text rule cannot judge",
        );
    }
    if form == "orca terminal send" && parsed.last(option).is_none_or(str::is_empty) {
        out.unread(
            form,
            "the terminal input text is absent, so the command it submits is unknown",
        );
    }
    if parsed.last("--text-stdin").is_some() {
        match input {
            Some(text) => super::terminal_inputs::text(text, form, out),
            None => out.unread(form, "the stdin text cannot be read"),
        }
    }
    for text in parsed.all(option) {
        super::terminal_inputs::text(text, form, out);
    }
    // Message subjects and structured payloads can also reach the agent.
    for key in ["--subject", "--payload", "--task-title"] {
        for text in parsed.all(key) {
            super::terminal_inputs::text(text, form, out);
        }
    }
}
