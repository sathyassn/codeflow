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

pub(super) fn read(
    name: &str,
    words: &Words<'_>,
    place: &Place,
    input: Option<&str>,
    stdin_bound: bool,
    out: &mut Carried,
) {
    match name {
        "tmux" => {
            tmux(words, place, out);
        }
        "screen" => screen(words, place, out),
        "osascript" => super::applescript::read(words.args, place, input, stdin_bound, out),
        "expect" => expect(words.args, place, out),
        _ => {}
    }
}

fn screen(words: &Words<'_>, place: &Place, out: &mut Carried) {
    let args = words.args;
    let mut at = 0;
    if args.first().is_some_and(|word| word == "-S") && args.get(1).is_some() {
        at = 2;
    }
    match &args[at..] {
        [option, verb, value] if option == "-X" && verb == "stuff" => {
            if value.contains(['^', '\\']) {
                out.unread(
                    "screen stuff",
                    "the terminal input is outside the modeled literal form",
                );
            } else {
                text(value, "screen stuff", out);
            }
        }
        [option, body @ ..] if option == "-dm" && !body.is_empty() => {
            match super::quote(words, at + 1) {
                Ok(line) => {
                    text(&line, "screen command", out);
                    out.runs.push(Run {
                        form: "screen command",
                        line,
                        directory: place.cwd.clone(),
                    });
                }
                Err(why) => out.unread("screen command", why),
            }
        }
        _ => out.unread("screen", "the call is outside the modeled forms"),
    }
}

fn expect(args: &[String], place: &Place, out: &mut Carried) {
    let [option, script] = args else {
        out.unread("expect", "only a modeled inline send statement is admitted");
        return;
    };
    if option == "-c" && script.starts_with("spawn ") {
        return expect_spawn(script, place, out);
    }
    let value = script
        .strip_prefix("send \"")
        .and_then(|body| body.strip_suffix('"'));
    match value.filter(|body| option == "-c" && !body.contains(['"', '\\', '$', '[', ']'])) {
        Some(value) => text(value, "expect send", out),
        None => out.unread(
            "expect",
            "the call is outside the modeled literal send form",
        ),
    }
}

fn expect_spawn(script: &str, place: &Place, out: &mut Carried) {
    let literal = |word: &str| {
        !word.is_empty()
            && word
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "_./:+-=,@%".contains(ch))
    };
    let words: Option<Vec<_>> = script.strip_prefix("spawn ").and_then(|body| {
        if body.contains(['\n', '\r']) {
            return None;
        }
        body.split_ascii_whitespace()
            .map(|word| {
                let word = if word.starts_with('{') {
                    word.strip_prefix('{')?.strip_suffix('}')?
                } else {
                    word
                };
                literal(word).then_some(word)
            })
            .collect()
    });
    let Some(words) =
        words.filter(|words| words.first().is_some_and(|word| !word.starts_with('-')))
    else {
        out.unread(
            "expect spawn",
            "the spawned command is outside the modeled literal form",
        );
        return;
    };
    let line = words
        .iter()
        .map(|word| super::shell_word(word, false))
        .collect::<Vec<_>>()
        .join(" ");
    out.runs.push(Run {
        form: "expect spawn",
        line,
        directory: place.cwd.clone(),
    });
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
        out.unread("tmux", "the call is outside the modeled forms");
        return;
    };
    if matches!(verb, "list-sessions" | "list-windows" | "list-panes") {
        if args.len() != root.operands + 1 {
            out.unread(
                "tmux",
                "the terminal management call has unmodeled arguments",
            );
        }
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
            tmux_send(&args[root.operands + 1..], out);
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

fn tmux_send(args: &[String], out: &mut Carried) {
    let mut at = 0;
    if args.first().is_some_and(|word| word == "-t") {
        if args
            .get(1)
            .is_none_or(|word| word.is_empty() || word.starts_with('-'))
        {
            out.unread("tmux send-keys", "the target cannot be read");
            return;
        }
        at = 2;
    }
    let literal = args.get(at).is_some_and(|word| word == "-l");
    if literal {
        at += 1;
    }
    let tail = &args[at..];
    let value = match tail {
        [value] => Some(value),
        [value, enter] if matches!(enter.as_str(), "Enter" | "C-m") => Some(value),
        _ => None,
    };
    let Some(value) = value.filter(|value| !value.starts_with('-') && tmux_text(value, literal))
    else {
        out.unread(
            "tmux send-keys",
            "only one text argument and an optional final submit key are modeled",
        );
        return;
    };
    text(value, "tmux send-keys", out);
}

fn tmux_text(value: &str, literal: bool) -> bool {
    if value.is_empty() {
        return false;
    }
    if literal {
        return true;
    }
    let name = value.to_ascii_lowercase();
    if value.trim() != value
        || ["c-", "m-", "s-", "^", "0x", "kp"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || name
            .strip_prefix('f')
            .is_some_and(|tail| !tail.is_empty() && tail.chars().all(|ch| ch.is_ascii_digit()))
        || TMUX_KEYS.contains(&name.as_str())
    {
        return false;
    }
    value.chars().any(char::is_whitespace) || value.chars().all(|ch| ch.is_ascii_alphabetic())
}

const TMUX_KEYS: &[&str] = &[
    "space",
    "tab",
    "bspace",
    "backspace",
    "btab",
    "enter",
    "return",
    "escape",
    "esc",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "ppage",
    "npage",
    "pageup",
    "pagedown",
    "pgup",
    "pgdn",
    "insert",
    "delete",
    "ic",
    "dc",
    "backtab",
    "begin",
    "find",
    "select",
    "cancel",
    "clear",
    "help",
    "break",
    "print",
    "scrolllock",
    "numlock",
    "pause",
    "mouse",
    "paste",
    "user",
    "none",
];
