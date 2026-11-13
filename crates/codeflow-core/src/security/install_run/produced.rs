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
//!    writes, and no value expanded into them that holds a backslash or may
//!    hold one the composer did not resolve, since zsh decodes escapes in a
//!    value too.
//!    A `$'...'` word
//!    that holds an escape is not settled either;
//! 3. the texts the guard's shell composer resolves for bash and zsh: a
//!    word built from a value the line assigns, a substitution over the
//!    builtins it models, arithmetic, `eval` and `sh -c` bodies;
//! 4. everything the line writes, joined in line order, since a reader on
//!    the other end takes it as one stream: the printed texts of item 2,
//!    and the composer's outputs in the order the line runs them, whatever
//!    the redirections, separators, groups, subshells or loops. A program
//!    this reader does not model adds nothing to the stream.
//!
//! Each of those texts takes one judgment ([`judge`]): it names the binary
//! with a verb as written, or it is read again as a command line, where
//! the same unsettled checks apply. A printing program's output that is
//! not settled refuses whatever it would name: three rounds of review
//! found a way past each general model of the escapes and conversions, so
//! this reader keeps none. So does a value the line binds, expanded into a
//! printing program, that the composer does not resolve exactly; a value
//! from outside the line is the named residual. Text read again past
//! [`DEPTH`] levels refuses the same way.

use super::{raw_run, InstallRun};
use crate::hooks::git_guard::{command_argv_literal, expand_commands, shell_blank};
use crate::security::composed::{bindings, composed_texts, outputs, Composed, Dialect};
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

/// What the composer settles about the values a line builds.
struct Values {
    /// A value expanded into `echo` or `print` may hold a backslash: the
    /// composer resolves a value that holds one, or leaves a value
    /// unresolved on a line that holds one.
    escape: bool,
    /// The names the line binds (an assignment, a `for` variable, `read`,
    /// `printf -v`) to a value the composer does not resolve exactly, as
    /// bash or as zsh reads it: a construct it does not model, or text
    /// from outside the line (a file `read` takes, a program's output, a
    /// glob's names).
    inexact: Vec<String>,
    /// The names every binding of which the composer resolves exactly.
    exact: Vec<String>,
    /// The line as written, where a name the composer reports no binding
    /// for may still be bound (a `for` variable, `read` in a `while`
    /// condition and other constructs).
    line: String,
    /// Whether the composer resolves every output on the line exactly, as
    /// bash and as zsh read it, with at least one output: then a name it
    /// reports no binding for (a `for` variable) is written exactly.
    outputs_exact: bool,
}

impl Values {
    fn of(command: &str, texts: &[Composed]) -> Self {
        let escape = texts.iter().any(|text| match text {
            Composed::Resolved(value) => {
                value.render().contains('\\') || value.without_open().render().contains('\\')
            }
            Composed::Unresolved(_) => command.contains('\\'),
        });
        let mut inexact = Vec::new();
        let mut exact = Vec::new();
        for (name, value) in bindings(command) {
            if matches!(&value, Composed::Resolved(value) if value.exact().is_some()) {
                if !exact.contains(&name) {
                    exact.push(name);
                }
            } else if !inexact.contains(&name) {
                inexact.push(name);
            }
        }
        exact.retain(|name| !inexact.contains(name));
        let outputs_exact = [Dialect::Bash, Dialect::Zsh].into_iter().all(|dialect| {
            let outputs = outputs(command, dialect);
            !outputs.is_empty()
                && outputs.iter().all(|(output, claimed)| {
                    *claimed
                        && matches!(output, Composed::Resolved(value) if value.exact().is_some())
                })
        });
        Self {
            escape,
            inexact,
            exact,
            line: command.to_string(),
            outputs_exact,
        }
    }

    /// Whether `args` expand a value the line binds that the composer does
    /// not resolve exactly. A name the line spells bare, outside an
    /// expansion (`read w`, `for w`, `w=`), counts as bound by the line;
    /// with no binding the composer reports, it is settled only when every
    /// output on the line is exact.
    fn unsettled_in(&self, args: &[String]) -> bool {
        args.iter().any(|arg| {
            expanded_names(arg).iter().any(|name| {
                self.inexact.contains(name)
                    || (!self.exact.contains(name)
                        && !self.outputs_exact
                        && spelled_bare(&self.line, name))
            })
        })
    }
}

/// Whether `line` spells `name` as a whole name outside a `$name` or
/// `${name...}` expansion.
fn spelled_bare(line: &str, name: &str) -> bool {
    let name_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(found) = line[from..].find(name) {
        let at = from + found;
        let end = at + name.len();
        let before = &line[..at];
        let whole = !before.ends_with(name_char) && !line[end..].starts_with(name_char);
        let expanded = before.ends_with('$')
            || before.ends_with("${")
            || before.ends_with("${#")
            || before.ends_with("${!");
        if whole && !expanded {
            return true;
        }
        from = end;
    }
    false
}

/// The first install run, or unsettled printed text, that `command`
/// produces. Every text the line produces takes one judgment
/// ([`judge`]): each printing program's output, the line's printed output
/// joined in line order, each text the composer resolves, and the
/// composer's outputs joined in the order the line runs them.
pub(super) fn produced_run(command: &str) -> Option<InstallRun> {
    let form = produced(command)?;
    Some(InstallRun {
        form,
        parsed: false,
        in_argument: false,
    })
}

fn produced(command: &str) -> Option<&'static str> {
    let texts = composed_texts(command);
    let values = Values::of(command, &texts);
    if let Some(form) = judge_line(command, 0, &values) {
        return Some(form);
    }
    for text in &texts {
        if let Composed::Resolved(value) = text {
            for rendered in [value.render(), value.without_open().render()] {
                if let Some(form) = judge(&rendered, 0, &values) {
                    return Some(form);
                }
            }
        }
    }
    for dialect in [Dialect::Bash, Dialect::Zsh] {
        let mut written = String::new();
        let mut empty = String::new();
        for (output, _) in outputs(command, dialect) {
            match output {
                Composed::Resolved(value) => {
                    written.push_str(&value.render());
                    empty.push_str(&value.without_open().render());
                }
                // An output the composer cannot resolve is unknown text
                // there, as outside text is: a value the line sets that
                // feeds it is refused by `Values::unsettled_in`.
                Composed::Unresolved(_) => written.push('$'),
            }
        }
        for joined in [written, empty] {
            if let Some(form) = judge(&joined, 0, &values) {
                return Some(form);
            }
        }
    }
    None
}

/// The one judgment of a produced text: [`FORM`] when it names the binary
/// with a verb as written, else what reading it again as a command line
/// finds ([`judge_line`]).
fn judge(text: &str, depth: usize, values: &Values) -> Option<&'static str> {
    if names(text) {
        return Some(FORM);
    }
    judge_line(text, depth + 1, values)
}

/// What reading `text` as a command line finds ([`read`]), then the
/// judgment of everything its printing programs write, joined in line
/// order: the stream a reader on the other end receives whatever the
/// redirections, separators, groups or loops. A program this reader does
/// not model adds nothing there.
fn judge_line(text: &str, depth: usize, values: &Values) -> Option<&'static str> {
    let mut stream = Vec::new();
    if let Some(form) = read(text, depth, values, &mut stream) {
        return Some(form);
    }
    if stream.len() < 2 {
        return None;
    }
    judge(&stream.concat(), depth, values)
}

/// Whether `text`, read as the raw text is, names the binary with a verb.
fn names(text: &str) -> bool {
    raw_run(text).is_some()
}

/// [`FORM`] when a text `command` produces names the binary with a verb,
/// [`UNSETTLED`] when a printing program's output is not settled. Each
/// printing program's output is added to `stream` in line order.
fn read(
    command: &str,
    depth: usize,
    values: &Values,
    stream: &mut Vec<String>,
) -> Option<&'static str> {
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
                if let Some(form) = read(word, depth + 1, values, stream) {
                    return Some(form);
                }
            }
        }
        for (at, word) in words.iter().enumerate() {
            let program = basename(word);
            if !PRINTERS.contains(&program.as_str()) {
                continue;
            }
            let args = &words[at + 1..];
            if shell_escaped(&segment, &words) || values.unsettled_in(args) {
                return Some(UNSETTLED);
            }
            let Some(text) = printed(&program, args, values.escape) else {
                return Some(UNSETTLED);
            };
            if let Some(form) = judge(&text, depth, values) {
                return Some(form);
            }
            stream.push(text);
        }
    }
    None
}

/// The names `arg` expands as `$name` or `${name...}`.
fn expanded_names(arg: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = arg;
    while let Some(at) = rest.find('$') {
        let after = &rest[at + 1..];
        let body = match after.strip_prefix('{') {
            Some(braced) => braced.strip_prefix(['#', '!']).unwrap_or(braced),
            None => after,
        };
        let name: String = body
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            names.push(name);
        }
        rest = after;
    }
    names
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
