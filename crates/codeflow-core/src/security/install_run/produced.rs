//! The text a line's shell and its printing programs produce (TSK-1476).
//!
//! A line outside the closed subset is judged on its raw text, which reads
//! a backslash both as dropped and as a break. The shell and a printing
//! program can still build a name the raw text does not hold, so the line
//! is also judged on:
//!
//! 1. each simple command's words as the shell passes them, from the
//!    guard's shell lexer, a quoted word that holds a blank read again as a
//!    command line;
//! 2. what `printf`, `echo` and zsh `print` write, but only where that is
//!    settled by plain reading: no backslash the shell consumes on the
//!    command, no escape in a `printf` format but `\n`, no `printf`
//!    conversion but a bare `%s` (whose arguments are written as they
//!    are), no width, precision or flag, no more arguments than
//!    conversions, no backslash in what `echo` or `print` without `-r`
//!    writes, and no value expanded into them that may hold an escape the
//!    composer did not resolve, since zsh decodes escapes in a value too.
//!    A `$'...'` word
//!    that holds an escape is not settled either;
//! 3. the texts the guard's shell composer resolves for bash and zsh: a
//!    word built from a value the line assigns, a substitution over the
//!    builtins it models, arithmetic, `eval` and `sh -c` bodies.
//!
//! A printing program's output that is not settled refuses whatever it
//! would name: three rounds of review found a way past each general model
//! of the escapes and conversions, so this reader keeps none. Text read
//! again past [`DEPTH`] levels refuses the same way.

use super::{raw_run, InstallRun};
use crate::hooks::git_guard::{command_argv_literal, expand_commands, shell_blank};
use crate::security::composed::{composed_texts, Composed};
use crate::security::headless::basename;

/// Programs whose output this reader judges.
const PRINTERS: &[&str] = &["printf", "echo", "print"];

/// How deep a printed text or a quoted word is read again as a command
/// line.
const DEPTH: usize = 3;

/// The form reported when a produced text names the binary with a verb.
const FORM: &str =
    "text the shell or a printing program produces, naming the binary with an install verb";

/// The form reported when a printing program's output is not settled.
pub(crate) const UNSETTLED: &str = "printed text the guard cannot settle";

/// The first install run, or unsettled printed text, that `command`
/// produces.
pub(super) fn produced_run(command: &str) -> Option<InstallRun> {
    let texts = composed_texts(command);
    // A value expanded into `echo` or `print` is settled when the line
    // holds no backslash, or when the composer resolves every value the
    // line builds: a value from outside the line is the named residual.
    let values_unsettled = command.contains('\\')
        && texts
            .iter()
            .any(|text| matches!(text, Composed::Unresolved(_)));
    let form = match read(command, 0, values_unsettled) {
        Some(form) => form,
        None if composed(&texts) => FORM,
        None => return None,
    };
    Some(InstallRun {
        form,
        parsed: false,
        in_argument: false,
    })
}

/// Whether a text the guard's shell composer resolves on the line, as bash
/// and as zsh build it, names the binary with a verb. Outside text is read
/// as written (`$`) and as empty.
fn composed(texts: &[Composed]) -> bool {
    texts.iter().any(|text| match text {
        Composed::Resolved(value) => {
            names(&value.render()) || names(&value.without_open().render())
        }
        Composed::Unresolved(_) => false,
    })
}

/// Whether `text`, read as the raw text is, names the binary with a verb.
fn names(text: &str) -> bool {
    raw_run(text).is_some()
}

/// [`FORM`] when a text `command` produces names the binary with a verb,
/// [`UNSETTLED`] when a printing program's output is not settled.
fn read(command: &str, depth: usize, values_unsettled: bool) -> Option<&'static str> {
    if depth > DEPTH {
        return Some(UNSETTLED);
    }
    for segment in expand_commands(command) {
        if escaped_dollar_quote(&segment) {
            return Some(UNSETTLED);
        }
        let (words, _) = command_argv_literal(&segment);
        if names(&words.join(" ")) {
            return Some(FORM);
        }
        for word in words.iter().skip(1) {
            if word.contains(shell_blank) {
                if let Some(form) = read(word, depth + 1, values_unsettled) {
                    return Some(form);
                }
            }
        }
        for (at, word) in words.iter().enumerate() {
            let program = basename(word);
            if !PRINTERS.contains(&program.as_str()) {
                continue;
            }
            if shell_escaped(&segment, &words) {
                return Some(UNSETTLED);
            }
            let Some(text) = printed(&program, &words[at + 1..], values_unsettled) else {
                return Some(UNSETTLED);
            };
            if names(&text) {
                return Some(FORM);
            }
            if let Some(form) = read(&text, depth + 1, values_unsettled) {
                return Some(form);
            }
        }
    }
    None
}

/// Whether `segment` holds a `$'...'` word with an escape in it.
fn escaped_dollar_quote(segment: &str) -> bool {
    let mut rest = segment;
    while let Some(at) = rest.find("$'") {
        let body = &rest[at + 2..];
        match body.find(['\\', '\'']) {
            Some(end) if body[end..].starts_with('\\') => return true,
            Some(end) => rest = &body[end + 1..],
            None => return false,
        }
    }
    false
}

/// Whether the shell consumed a backslash in `segment`: the words the lexer
/// passes on hold fewer backslashes than the text as written, so an
/// unquoted escape, a `\\` or `\$` in double quotes, or a `$'...'` escape
/// was read there.
fn shell_escaped(segment: &str, words: &[String]) -> bool {
    let kept: usize = words.iter().map(|word| word.matches('\\').count()).sum();
    segment.matches('\\').count() > kept
}

/// What `program` writes from `args` when that is settled by plain
/// reading, else `None`. `values_unsettled` is whether a value the line
/// expands into `echo` or `print` may hold an escape the composer did not
/// resolve: the line holds a backslash and the composer leaves a value
/// unresolved (a value from outside the line is the named residual).
fn printed(program: &str, args: &[String], values_unsettled: bool) -> Option<String> {
    if program == "printf" {
        let mut args = args;
        if args.first().is_some_and(|word| word == "-v") {
            args = args.get(2..)?;
        }
        if args.first().is_some_and(|word| word == "--") {
            args = &args[1..];
        }
        let (format, rest) = args.split_first()?;
        if format.starts_with('-') || format.contains(['$', '`']) {
            return None;
        }
        let mut out = String::new();
        let mut used = 0;
        let mut chars = format.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if chars.next() != Some('n') {
                        return None;
                    }
                    out.push('\n');
                }
                '%' => {
                    if chars.next() != Some('s') {
                        return None;
                    }
                    out.push_str(rest.get(used).map_or("", String::as_str));
                    used += 1;
                }
                c => out.push(c),
            }
        }
        (rest.len() <= used).then_some(out)
    } else {
        let mut args = args;
        let mut raw = false;
        while let Some(first) = args.first() {
            let flags = first.strip_prefix('-').filter(|flags| !flags.is_empty());
            match flags {
                Some(flags) if program == "echo" && flags.chars().all(|c| "neE".contains(c)) => {}
                Some(flags) if program == "print" && flags.chars().all(|c| "nr".contains(c)) => {
                    raw |= flags.contains('r');
                }
                Some(_) if program == "print" => return None,
                _ => break,
            }
            args = &args[1..];
        }
        let escapes = args.iter().any(|word| word.contains('\\'));
        let expands = args.iter().any(|word| word.contains(['$', '`']));
        let unsettled = escapes || (expands && values_unsettled);
        (raw || !unsettled).then(|| args.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split(' ').map(ToString::to_string).collect()
    }

    /// Plain literal output is written as the program writes it; anything
    /// else is not settled.
    #[test]
    fn only_plain_literal_output_is_settled() {
        for (program, args, values_unsettled, expected) in [
            ("printf", "%s notes", false, Some("notes")),
            ("printf", r"a%sb%s\n x", true, Some("axb\n")),
            ("printf", "-- %s x", false, Some("x")),
            ("printf", "-v p %s%s no tes", false, Some("notes")),
            ("printf", r"%s a\b", true, Some(r"a\b")),
            ("printf", "%s $x", true, Some("$x")),
            ("echo", "-n notes", false, Some("notes")),
            ("echo", "$HOME", false, Some("$HOME")),
            ("print", "-r notes", false, Some("notes")),
            ("print", r"-r a\x75b", true, Some(r"a\x75b")),
            ("printf", "%s a b", false, None),
            ("printf", "%5s a", false, None),
            ("printf", "%%", false, None),
            ("printf", "%b notes", false, None),
            ("printf", r"a\tb", true, None),
            ("printf", r"%s\\n x", true, None),
            ("printf", "$f no", false, None),
            ("printf", "-v", false, None),
            ("echo", r"a\nb", true, None),
            ("echo", "$x", true, None),
            ("print", "-P %n", false, None),
        ] {
            assert_eq!(
                printed(program, &words(args), values_unsettled),
                expected.map(ToString::to_string),
                "{program} {args}"
            );
        }
        assert!(printed("printf", &[], false).is_none());
    }

    /// A backslash the shell consumes is an escape on the command.
    #[test]
    fn a_backslash_the_shell_consumes_is_an_escape() {
        assert!(shell_escaped(r"printf %s\n x", &words(r"printf %sn x")));
        assert!(shell_escaped(r#"echo "a\\b""#, &words(r"echo a\b")));
        assert!(!shell_escaped(r"printf '%s\n' x", &words(r"printf %s\n x")));
    }

    /// A `$'...'` word is plain only without an escape.
    #[test]
    fn a_dollar_quote_with_an_escape_is_not_settled() {
        assert!(escaped_dollar_quote(r"echo $'a\tb'"));
        assert!(escaped_dollar_quote(r"echo $'a' $'\n'"));
        assert!(!escaped_dollar_quote("echo $'notes'"));
        assert!(!escaped_dollar_quote(r"echo 'a\tb' $x"));
    }
}
