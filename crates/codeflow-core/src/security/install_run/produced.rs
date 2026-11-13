//! The text a line's shell and its printing programs produce (TSK-1476).
//!
//! A line outside the closed subset is judged on its raw text, which reads
//! a backslash both as dropped and as a break. That guess cannot follow a
//! shell escape and a printf escape in one word, so the line is also judged
//! on the texts they produce:
//!
//! 1. each simple command's words as the shell passes them, from the
//!    guard's shell lexer: quotes and backslashes removed and `$'...'`
//!    decoded;
//! 2. what a printing program writes from those words: `printf` with its
//!    format, its `%s`, `%b`, `%c` and numeric conversions, precision and
//!    escapes, and `echo` and `print` with the escapes bash, zsh and sh
//!    decode, with `-e`, `-E` or each shell's default alike. Octal is read
//!    both as `\NNN` and as `\0NNN`, and a NUL both as a break and as
//!    dropped, as a shell reading the text drops it. Where a format holds a
//!    conversion this reader does not model, the format and each argument
//!    are also judged alone and joined.
//!
//! A printed text is judged as written, decoded once more (a later printer
//! may decode it again) and read again as a command line, as a shell
//! reading a pipe or a FIFO would read it. A word that holds a blank is
//! read again as a command line (`sh -c '...'`). Each text is matched as
//! the raw text is ([`raw_run`]): the binary as a whole word, then a verb.
//!
//! Text built by a program this reader does not model (a decoder, a
//! character translator, an interpreter) is not on the line. That is a
//! named residual.

use std::iter::Peekable;
use std::str::Chars;

use super::{raw_run, InstallRun};
use crate::hooks::git_guard::{command_argv_literal, expand_commands, shell_blank};
use crate::security::headless::basename;

/// Programs whose output this reader decodes.
const PRINTERS: &[&str] = &["printf", "echo", "print"];

/// How deep a printed text or a quoted word is read again as a command
/// line.
const DEPTH: usize = 3;

/// The most times `printf` reuses its format for the arguments left.
const PASSES: usize = 16;

const FORM: &str =
    "text the shell or a printing program produces, naming the binary with an install verb";

/// One reading of the escape language: whether a leading `0` starts a
/// three-digit octal escape (`\0NNN`, as `echo` and `%b` read it) or counts
/// as one of the three digits (`\NNN`, as a `printf` format reads it), and
/// whether a NUL is dropped.
#[derive(Clone, Copy)]
struct Reading {
    zero_led: bool,
    drop_nul: bool,
}

const READINGS: [Reading; 4] = [
    Reading {
        zero_led: false,
        drop_nul: false,
    },
    Reading {
        zero_led: true,
        drop_nul: false,
    },
    Reading {
        zero_led: false,
        drop_nul: true,
    },
    Reading {
        zero_led: true,
        drop_nul: true,
    },
];

/// A run the texts `command` produces name: the binary with an install
/// verb in its words as the shell passes them or in what a printing
/// program writes from them.
pub(super) fn produced_run(command: &str) -> Option<InstallRun> {
    read(command, 0).then_some(InstallRun {
        form: FORM,
        parsed: false,
        in_argument: false,
    })
}

/// Whether a text `command` produces names the binary with a verb.
fn read(command: &str, depth: usize) -> bool {
    if depth > DEPTH {
        return false;
    }
    expand_commands(command).iter().any(|segment| {
        let (words, _) = command_argv_literal(segment);
        raw_run(&words.join(" ")).is_some()
            || words
                .iter()
                .skip(1)
                .any(|word| word.contains(shell_blank) && read(word, depth + 1))
            || words.iter().enumerate().any(|(at, word)| {
                PRINTERS.contains(&basename(word).as_str())
                    && printed(&basename(word), &words[at + 1..])
                        .iter()
                        .any(|text| judged(text, depth))
            })
    })
}

/// A printed text, judged as written, decoded once more, and read again
/// as a command line.
fn judged(text: &str, depth: usize) -> bool {
    raw_run(text).is_some()
        || READINGS.iter().any(|reading| {
            let (decoded, _) = decode(text, *reading);
            decoded != text && raw_run(&decoded).is_some()
        })
        || read(text, depth + 1)
}

/// The texts `program` can write from `args`, one for each reading.
fn printed(program: &str, args: &[String]) -> Vec<String> {
    let mut texts = Vec::new();
    if program == "printf" {
        let mut args = args;
        if args.first().is_some_and(|word| word == "-v") {
            args = args.get(2..).unwrap_or_default();
        }
        if args.first().is_some_and(|word| word == "--") {
            args = &args[1..];
        }
        let Some((format, rest)) = args.split_first() else {
            return texts;
        };
        for reading in READINGS {
            let (text, unmodelled) = printf_text(format, rest, reading);
            texts.push(text);
            if unmodelled {
                texts.push(decode(format, reading).0);
                for arg in rest {
                    texts.push(arg.clone());
                    texts.push(decode(arg, reading).0);
                }
                texts.push(decode(&rest.join(" "), reading).0);
            }
        }
    } else {
        let joined = args.join(" ");
        for reading in READINGS {
            texts.push(decode(&joined, reading).0);
        }
        texts.push(joined);
    }
    texts.sort();
    texts.dedup();
    texts
}

/// What `printf format args...` writes under `reading`, and whether the
/// format holds a conversion this reader does not model.
fn printf_text(format: &str, args: &[String], reading: Reading) -> (String, bool) {
    let arg = |at: usize| args.get(at).map_or("", String::as_str);
    let mut out = String::new();
    let mut unmodelled = false;
    let mut next = 0;
    for _ in 0..PASSES {
        let start = next;
        let mut chars = format.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if escape(&mut chars, reading, &mut out) {
                    return (out, unmodelled);
                }
                continue;
            }
            if c != '%' {
                out.push(c);
                continue;
            }
            while chars.next_if(|c| "-+ #0'".contains(*c)).is_some() {}
            if chars.next_if_eq(&'*').is_some() {
                next += 1;
            } else {
                while chars.next_if(char::is_ascii_digit).is_some() {}
            }
            let mut precision = None;
            if chars.next_if_eq(&'.').is_some() {
                if chars.next_if_eq(&'*').is_some() {
                    precision = arg(next).parse().ok();
                    next += 1;
                } else {
                    let mut digits = String::new();
                    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                        digits.push(digit);
                    }
                    precision = Some(digits.parse().unwrap_or(0));
                }
            }
            while chars.next_if(|c| "hlLqjzt".contains(*c)).is_some() {}
            let value = arg(next);
            let conversion = chars.next();
            if conversion == Some('%') {
                out.push('%');
                continue;
            }
            next += 1;
            match conversion {
                Some('s') => out.push_str(&cut(value, precision)),
                Some('b') => {
                    let (text, stop) = decode(value, reading);
                    out.push_str(&cut(&text, precision));
                    if stop {
                        return (out, unmodelled);
                    }
                }
                Some('c') => out.extend(value.chars().next()),
                Some(numeric) if "diouxXeEfFgGaA".contains(numeric) => out.push_str(value),
                _ => {
                    unmodelled = true;
                    out.push_str(value);
                }
            }
        }
        if next == start || next >= args.len() {
            break;
        }
    }
    (out, unmodelled)
}

/// `text` cut to `precision` characters.
fn cut(text: &str, precision: Option<usize>) -> String {
    precision.map_or_else(|| text.to_string(), |n| text.chars().take(n).collect())
}

/// `text` with its escapes decoded under `reading`, and whether a `\c`
/// ended the output there.
fn decode(text: &str, reading: Reading) -> (String, bool) {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
        } else if escape(&mut chars, reading, &mut out) {
            return (out, true);
        }
    }
    (out, false)
}

/// One escape after a backslash, written to `out`; `true` for `\c`, which
/// ends the output. An unknown escape keeps its backslash.
fn escape(chars: &mut Peekable<Chars<'_>>, reading: Reading, out: &mut String) -> bool {
    let Some(e) = chars.next() else {
        out.push('\\');
        return false;
    };
    let value = match e {
        'a' => Some(0x7),
        'b' => Some(0x8),
        'e' | 'E' => Some(0x1b),
        'f' => Some(0xc),
        'n' => Some(0xa),
        'r' => Some(0xd),
        't' => Some(0x9),
        'v' => Some(0xb),
        '\\' | '\'' | '"' | '?' => Some(u32::from(e)),
        'c' => return true,
        'x' => digits(chars, 16, 2),
        'u' => digits(chars, 16, 4),
        'U' => digits(chars, 16, 8),
        '0' if reading.zero_led => Some(digits(chars, 8, 3).unwrap_or(0) & 0xff),
        '0'..='7' => {
            let mut value = e.to_digit(8).unwrap_or(0);
            for _ in 0..2 {
                let Some(digit) = chars.peek().and_then(|c| c.to_digit(8)) else {
                    break;
                };
                chars.next();
                value = value * 8 + digit;
            }
            Some(value & 0xff)
        }
        _ => None,
    };
    match value {
        Some(0) if reading.drop_nul => {}
        Some(value) => out.push(char::from_u32(value).unwrap_or('\u{fffd}')),
        None => {
            out.push('\\');
            out.push(e);
        }
    }
    false
}

/// Up to `max` digits of `radix` from `chars`, or `None` when the next
/// character is not one.
fn digits(chars: &mut Peekable<Chars<'_>>, radix: u32, max: usize) -> Option<u32> {
    let mut value = None;
    for _ in 0..max {
        let Some(digit) = chars.peek().and_then(|c| c.to_digit(radix)) else {
            break;
        };
        chars.next();
        value = Some(
            value
                .unwrap_or(0u32)
                .saturating_mul(radix)
                .saturating_add(digit),
        );
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: Reading = READINGS[0];
    const ZERO_LED: Reading = READINGS[1];
    const NUL_DROPPED: Reading = READINGS[2];

    /// Each escape decodes to the character `printf`, `echo` or `print`
    /// writes; an unknown escape keeps its backslash; `\c` ends the text.
    #[test]
    fn escapes_decode_as_the_printing_programs_write_them() {
        let four = format!("{}u0063odeflow", '\\');
        let eight = format!("{}U00000063odeflow", '\\');
        for (text, reading, expected) in [
            (r"\143odeflow", PLAIN, "codeflow"),
            (r"\0143odeflow", ZERO_LED, "codeflow"),
            (r"\x63odeflow", PLAIN, "codeflow"),
            (four.as_str(), PLAIN, "codeflow"),
            (eight.as_str(), PLAIN, "codeflow"),
            (r"a\tb\nc", PLAIN, "a\tb\nc"),
            (r"cod\0eflow", NUL_DROPPED, "codeflow"),
            (r"a\qb", PLAIN, r"a\qb"),
            (r"a\\b", PLAIN, r"a\b"),
        ] {
            assert_eq!(decode(text, reading).0, expected, "{text}");
        }
        assert_eq!(
            decode(r"codeflow\c update", PLAIN),
            ("codeflow".to_string(), true)
        );
    }

    /// `printf` writes its format with each argument, reusing the format
    /// for the arguments left, cutting `%s` and `%b` to their precision,
    /// and marks a conversion it does not model.
    #[test]
    fn printf_writes_its_format_with_each_argument() {
        for (format, words, expected, unmodelled) in [
            (r"%s %s\n", &["a", "b", "c"][..], "a b\nc \n", false),
            (
                r"%.8s update",
                &["codeflowXYZ"][..],
                "codeflow update",
                false,
            ),
            (r"%b", &[r"\x63odeflow"][..], "codeflow", false),
            (r"%c%c", &["cat", "odd"][..], "co", false),
            (r"100%% %d", &["5"][..], "100% 5", false),
            (r"%*s", &["3", "x"][..], "x", false),
            (r"%q", &["x"][..], "x", true),
        ] {
            let args: Vec<String> = words.iter().map(ToString::to_string).collect();
            assert_eq!(
                printf_text(format, &args, PLAIN),
                (expected.to_string(), unmodelled),
                "{format}"
            );
        }
    }
}
