//! The admitted script statements for terminal and app delivery.

use super::{terminal_inputs, Carried, Place, Run};

enum Word {
    Keyword(String),
    Text(String),
}

pub(super) fn read(
    args: &[String],
    place: &Place,
    input: Option<&str>,
    stdin_bound: bool,
    out: &mut Carried,
) {
    use Word::{Keyword as K, Text as T};
    let Some(script) = script(args).filter(|_| input.is_none() && !stdin_bound) else {
        out.unread("osascript", "only a modeled inline script is admitted");
        return;
    };
    let Some(words) = words(&script) else {
        out.unread("osascript", "the script is outside the modeled forms");
        return;
    };
    match words.as_slice() {
        [K(return_), T(_)]
            if return_ == "return"
                && args.iter().filter(|arg| arg.as_str() == "-e").count() == 1 => {}
        [K(do_), K(shell), K(script), T(body)]
            if do_ == "do" && shell == "shell" && script == "script" =>
        {
            out.runs.push(Run {
                form: "osascript do shell script",
                line: body.clone(),
                directory: place.cwd.clone(),
            });
        }
        [K(tell), K(application), T(name), K(to), K(do_), K(script), T(body)]
            if tell == "tell"
                && application == "application"
                && !name.is_empty()
                && to == "to"
                && do_ == "do"
                && script == "script" =>
        {
            terminal_inputs::text(body, "osascript terminal input", out);
        }
        [K(tell), K(application), T(name), K(to), K(keystroke), T(body)]
            if tell == "tell"
                && application == "application"
                && !name.is_empty()
                && to == "to"
                && keystroke == "keystroke" =>
        {
            terminal_inputs::text(body, "osascript terminal input", out);
        }
        _ => out.unread("osascript", "the script is outside the modeled forms"),
    }
}

fn script(args: &[String]) -> Option<String> {
    let mut rest = args;
    if rest.first().is_some_and(|word| word == "-l") {
        if !rest.get(1)?.eq_ignore_ascii_case("AppleScript") {
            return None;
        }
        rest = &rest[2..];
    }
    let mut statements = Vec::new();
    while let [option, body, tail @ ..] = rest {
        if option != "-e" {
            return None;
        }
        statements.push(body.as_str());
        rest = tail;
    }
    (!statements.is_empty() && rest.is_empty()).then(|| statements.join("\n"))
}

fn words(script: &str) -> Option<Vec<Word>> {
    let mut chars = script.chars().peekable();
    let mut out = Vec::new();
    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let mut value = String::new();
        if ch == '"' {
            let mut closed = false;
            while let Some(ch) = chars.next() {
                match ch {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => match chars.next()? {
                        next @ ('"' | '\\') => value.push(next),
                        _ => return None,
                    },
                    _ => value.push(ch),
                }
            }
            if !closed || chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                return None;
            }
            out.push(Word::Text(value));
        } else {
            if !ch.is_ascii_alphabetic() {
                return None;
            }
            value.push(ch);
            while chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                let ch = chars.next()?;
                if !ch.is_ascii_alphabetic() {
                    return None;
                }
                value.push(ch);
            }
            out.push(Word::Keyword(value.to_ascii_lowercase()));
        }
    }
    Some(out)
}
