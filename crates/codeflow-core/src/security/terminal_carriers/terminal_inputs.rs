//! Text sent to a terminal, app or agent is checked before delivery.

use super::{options, Carried, Grammar, Place, Run, Words};
use crate::hooks::git_guard;

pub(super) fn text(value: &str, form: &str, out: &mut Carried) {
    if git_guard::unresolved_word(value)
        || value
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\t'))
        || git_guard::command_segments(value).iter().any(|segment| {
            segment == git_guard::NESTING_UNREAD || git_guard::unread_carried_program(segment)
        })
    {
        out.unread(
            form,
            "the delivered text cannot be read as a complete command line",
        );
    } else if guarded(value, 0, false) {
        out.unread(form, "the delivered text names a guarded command");
    }
}

/// Read the words with the existing shell and headless-call readers.
pub(super) fn carrier_reference(value: &str) -> bool {
    guarded(value, 0, true)
}

fn guarded(value: &str, depth: usize, carriers_only: bool) -> bool {
    if depth > super::DEPTH_LIMIT
        || (!carriers_only && super::super::headless::raw_run(value).is_some())
    {
        return true;
    }
    git_guard::simple_commands(value).iter().any(|args| {
        args.iter().any(|word| {
            super::super::arguments::argument_values(word)
                .into_iter()
                .any(|part| {
                    let name = super::carrier_name(part);
                    super::is_carrier_program(part)
                        || (!carriers_only
                            && (matches!(name.as_str(), "git" | "gh" | "eval" | "source" | ".")
                                || git_guard::is_shell(&name)
                                || super::super::outward::MODELED_LAUNCHERS
                                    .contains(&name.as_str())
                                || git_guard::COMMAND_CARRIERS.contains(&name.as_str())))
                        || (part.contains([' ', '\t', '\n', ';', '|', '&'])
                            && guarded(part, depth + 1, carriers_only))
                })
        })
    })
}

pub(super) fn read(name: &str, words: &Words<'_>, place: &Place, out: &mut Carried) {
    let args = words.args;
    match name {
        "tmux" => {
            tmux(words, place, out);
        }
        "screen" => {
            let stuff = args
                .windows(2)
                .any(|pair| pair[0] == "-X" && pair[1] == "stuff");
            let rest = super::super::headless::skip_options(args, &["-S", "-t", "-c", "-p", "-s"]);
            let command = !args.iter().any(|word| word == "-X") && !rest.is_empty();
            if args.iter().any(|word| word == "-X") && !stuff {
                out.unread("screen", "this text route is not in the carrier inventory");
            }
            if stuff || command {
                let route = if stuff { "stuff" } else { "command" };
                if super::inventory::contains(name, route) {
                    let body = if stuff {
                        let at = args
                            .iter()
                            .position(|word| word == "-X")
                            .unwrap_or_default();
                        args.get(at + 2..).unwrap_or_default()
                    } else {
                        rest
                    };
                    for value in body {
                        text(value, "screen terminal input", out);
                    }
                    // Read the complete text too, including adjacent operands.
                    text(&body.concat(), "screen terminal input", out);
                    text(&body.join(" "), "screen terminal input", out);
                }
            }
        }
        "osascript" => {
            for script in args {
                let lower = script.to_ascii_lowercase();
                if lower.contains("keystroke") || lower.contains("do script") {
                    text(script, "osascript terminal input", out);
                }
            }
        }
        "expect" => {
            for script in args {
                text(script, "expect terminal input", out);
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_lines)] // Keep each command's options beside its carried text.
fn tmux(words: &Words<'_>, place: &Place, out: &mut Carried) {
    let args = words.args;
    let root = match options(
        args,
        0,
        &Grammar {
            flags: &["-u", "-v", "-V"],
            valued: &["-L", "-S"],
        },
        false,
    ) {
        Ok(parsed) => parsed,
        Err(why) => {
            out.unread("tmux", why);
            return;
        }
    };
    let Some(verb) = args.get(root.operands).map(String::as_str) else {
        return;
    };
    if matches!(verb, "list-sessions" | "list-windows" | "list-panes") {
        return;
    }
    if !super::inventory::contains("tmux", verb) {
        out.unread("tmux", "this text route is not in the carrier inventory");
        return;
    }
    if args
        .iter()
        .any(|word| word == ";" || word.contains("#{") || word.contains("#("))
    {
        out.unread("tmux", "the carrier may change text before delivery");
        return;
    }
    match verb {
        "send-keys" | "send" => {
            let parsed = match options(
                args,
                root.operands + 1,
                &Grammar {
                    flags: &["-l"],
                    valued: &["-t"],
                },
                false,
            ) {
                Ok(parsed) => parsed,
                Err(why) => {
                    out.unread("tmux send-keys", why);
                    return;
                }
            };
            let mut keys = &args[parsed.operands..];
            if keys.last().is_some_and(|key| key == "Enter") {
                keys = &keys[..keys.len() - 1];
            }
            if keys.is_empty()
                || keys
                    .iter()
                    .any(|key| key.starts_with("C-") || key.starts_with("M-") || key == "Escape")
            {
                out.unread("tmux send-keys", "the terminal input cannot be established");
            } else {
                text(&keys.concat(), "tmux send-keys", out);
            }
        }
        "paste-buffer" => out.unread("tmux paste-buffer", "the buffered text cannot be read"),
        "run-shell" | "new-session" | "new-window" | "split-window" => {
            let parsed = match options(
                args,
                root.operands + 1,
                &Grammar {
                    flags: &["-d", "-b", "-a", "-k", "-P", "-E", "-D"],
                    valued: &["-s", "-n", "-t", "-c", "-x", "-y", "-F"],
                },
                false,
            ) {
                Ok(parsed) => parsed,
                Err(why) => {
                    out.unread("tmux command", why);
                    return;
                }
            };
            let body = &args[parsed.operands..];
            if body.is_empty() {
                return;
            }
            let directory = match (parsed.last("-c"), place.cwd.as_deref()) {
                (Some(dir), Some(base)) if !git_guard::unresolved_word(dir) => {
                    match super::cwd_path(dir, base, place.home.as_deref(), cfg!(windows)) {
                        Ok(directory) => directory,
                        Err(why) => {
                            out.unread("tmux command", why);
                            None
                        }
                    }
                }
                _ => None,
            };
            let line = if body.len() == 1 {
                body[0].clone()
            } else {
                match super::quote(words, parsed.operands) {
                    Ok(line) => line,
                    Err(why) => {
                        out.unread("tmux command", why);
                        return;
                    }
                }
            };
            text(&line, "tmux command", out);
            if git_guard::unresolved_word(&line) {
                out.unread("tmux command", "the command text cannot be read");
            } else {
                out.runs.push(Run {
                    form: "tmux command",
                    line,
                    directory,
                });
            }
        }
        _ => out.unread("tmux", "this text route has no reader"),
    }
}
