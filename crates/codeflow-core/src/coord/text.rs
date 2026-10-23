//! Free text in the ledger: size bounds, the secret scan and safe rendering.
//!
//! Every free-text field is checked on input and again on replay, so a
//! reader never assumes a line was validated when it was written. The value
//! scanned is the decoded string (JSON escapes already resolved), and its
//! percent-decoded form as well. A refusal names the field and the kind of
//! finding, never the text.

/// The bound for a short field: a title, a reason, a reference.
pub const SHORT: usize = 200;
/// The bound for a condition or an observation.
pub const MEDIUM: usize = 1000;
/// The bound for a description.
pub const LONG: usize = 4000;

/// Check one free-text field.
///
/// `multiline` allows line breaks and tabs; every other control or format
/// character (bidirectional overrides included) is refused.
///
/// # Errors
///
/// The reason the field is refused, without its content.
pub fn check(field: &str, value: &str, max: usize, multiline: bool) -> Result<(), String> {
    let length = value.chars().count();
    if length > max {
        return Err(format!(
            "field `{field}` is {length} characters; the limit is {max}"
        ));
    }
    if let Some(found) = value.chars().find(|c| refused_char(*c, multiline)) {
        return Err(format!(
            "field `{field}` holds the control or format character U+{:04X}",
            u32::from(found)
        ));
    }
    let decoded = percent_decoded(value);
    for text in std::iter::once(value).chain(decoded.as_deref()) {
        for line in text.lines() {
            if let Some(kind) = crate::hooks::scan::scan_line(line) {
                return Err(format!(
                    "field `{field}` looks like it holds a secret ({kind}); nothing was recorded"
                ));
            }
        }
    }
    Ok(())
}

/// Check a field that must also be non-empty.
///
/// # Errors
///
/// The reason the field is refused.
pub fn check_required(field: &str, value: &str, max: usize, multiline: bool) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("field `{field}` is required and cannot be empty"));
    }
    check(field, value, max, multiline)
}

fn refused_char(c: char, multiline: bool) -> bool {
    if multiline && (c == '\n' || c == '\t') {
        return false;
    }
    c.is_control()
        || matches!(c,
            '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{2028}'
            | '\u{2029}')
}

/// The percent-decoded form of `value`, when it holds an escape.
fn percent_decoded(value: &str) -> Option<String> {
    if !value.contains('%') {
        return None;
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let digit = |byte: u8| char::from(byte).to_digit(16);
            if let (Some(high), Some(low)) = (digit(bytes[index + 1]), digit(bytes[index + 2])) {
                out.push(u8::try_from(high * 16 + low).unwrap_or(b'?'));
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// A rejected or quoted value as a diagnostic may show it: the value itself
/// when it is short, plain and not secret-shaped, otherwise a placeholder.
/// A refusal never echoes a value that looks like a secret.
#[must_use]
pub fn shown(value: &str) -> String {
    if check("value", value, 80, false).is_ok() {
        crate::git::display_message(value)
    } else {
        "<value withheld>".to_string()
    }
}

/// Replace every secret-shaped word of a message (split on spaces, quotes,
/// backticks, brackets and commas) with `[redacted]`, so a diagnostic built
/// from a parser's or the log's own text never carries one.
#[must_use]
pub fn scrub(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            let secret = check("word", word, usize::MAX, true)
                .err()
                .is_some_and(|reason| reason.contains("secret"));
            out.push_str(if secret { "[redacted]" } else { word });
            word.clear();
        }
    };
    for c in message.chars() {
        if c.is_whitespace()
            || matches!(
                c,
                '`' | '"' | '\'' | ',' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Replace a secret-shaped line with a marker, for the redacted snapshot.
#[must_use]
pub fn redact(value: &str) -> String {
    value
        .lines()
        .map(|line| {
            if crate::hooks::scan::scan_line(line).is_some() {
                "[redacted]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line of text for a terminal: line breaks and tabs become spaces and
/// any other control or format character is escaped.
#[must_use]
pub fn terminal(value: &str) -> String {
    let flat: String = value
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .collect();
    crate::git::display_message(&flat)
}

/// One Markdown table cell or inline span: the characters Markdown or HTML
/// would read as structure are escaped, and line breaks become spaces.
#[must_use]
pub fn markdown(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in terminal(value).chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '|' | '#' | '!' | '~' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_planted_token_is_refused_without_its_text() {
        let token = format!("ghp_{}", "a".repeat(36));
        let error = check("description", &format!("use {token} here"), LONG, true).unwrap_err();
        assert!(error.contains("secret"), "{error}");
        assert!(!error.contains(&token), "the refusal echoed the secret");
    }

    #[test]
    fn a_percent_encoded_token_is_refused() {
        let encoded = format!("%67hp_{}", "b".repeat(36));
        let error = check("title", &encoded, SHORT, false).unwrap_err();
        assert!(error.contains("secret"), "{error}");
    }

    #[test]
    fn control_and_bidi_characters_are_refused() {
        assert!(check("title", "a\u{1b}[31mred", SHORT, false).is_err());
        assert!(check("title", "a\u{202e}b", SHORT, false).is_err());
        assert!(check("title", "two\nlines", SHORT, false).is_err());
        assert!(check("description", "two\nlines\tok", LONG, true).is_ok());
    }

    #[test]
    fn an_oversized_field_is_refused() {
        let error = check("title", &"x".repeat(SHORT + 1), SHORT, false).unwrap_err();
        assert!(error.contains("limit is 200"), "{error}");
        assert!(check_required("title", "  ", SHORT, false).is_err());
    }

    #[test]
    fn diagnostics_never_echo_a_secret() {
        let token = format!("ghp_{}", "d".repeat(36));
        assert_eq!(shown(&token), "<value withheld>");
        assert_eq!(shown("TSK-001"), "TSK-001");
        let message = format!("unknown variant `{token}`, expected one of `a`");
        let clean = scrub(&message);
        assert!(!clean.contains(&token), "{clean}");
        assert!(
            clean.contains("[redacted]") && clean.contains("`a`"),
            "{clean}"
        );
        let url = format!("https://x:{token}@github.com/o/r");
        assert!(!scrub(&url).contains(&token));
    }

    #[test]
    fn rendering_is_safe() {
        assert_eq!(
            markdown("a|b <i>`x`</i>"),
            "a\\|b &lt;i&gt;\\`x\\`&lt;/i&gt;"
        );
        assert_eq!(terminal("a\nb"), "a b");
        assert_eq!(
            redact(&format!("ok\nghp_{}", "c".repeat(36))),
            "ok\n[redacted]"
        );
    }
}
