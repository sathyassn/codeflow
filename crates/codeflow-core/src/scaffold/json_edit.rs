//! Byte-preserving edits to a JSON object file (SPC-013 R-84, R-115).
//!
//! `init` and `update` add or change a few members of an adopter's
//! `.codeflow/policy.json`. Serializing the whole value again would rewrite
//! every byte the adopter chose (order, spacing, compact arrays), so these
//! helpers locate a member by its path with a real JSON scan and splice only
//! the member's own bytes. Callers parse the result and compare it with the
//! value they expect before writing, so a surprising layout is refused rather
//! than guessed.

use serde_json::Value;

fn skip_ws(text: &str, mut at: usize) -> usize {
    let bytes = text.as_bytes();
    while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r') {
        at += 1;
    }
    at
}

/// The byte just past the JSON value that starts at `at`.
fn value_end(text: &str, at: usize) -> Option<usize> {
    let mut stream = serde_json::Deserializer::from_str(&text[at..]).into_iter::<Value>();
    stream.next()?.ok()?;
    Some(at + stream.byte_offset())
}

/// The member `key` of the object whose `{` ends at `open`: the byte span
/// of its value.
fn member(text: &str, open: usize, key: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut at = skip_ws(text, open);
    loop {
        // The object ends (`}`), or the next member starts with its name.
        if *bytes.get(at)? != b'"' {
            return None;
        }
        let mut names = serde_json::Deserializer::from_str(&text[at..]).into_iter::<String>();
        let name = names.next()?.ok()?;
        at = skip_ws(text, at + names.byte_offset());
        if *bytes.get(at)? != b':' {
            return None;
        }
        let start = skip_ws(text, at + 1);
        let end = value_end(text, start)?;
        if name == key {
            return Some((start, end));
        }
        at = skip_ws(text, end);
        match bytes.get(at)? {
            b',' => at = skip_ws(text, at + 1),
            _ => return None,
        }
    }
}

/// The byte just past the `{` of the object at `path` (empty: the top level).
#[must_use]
pub fn object_open(text: &str, path: &[&str]) -> Option<usize> {
    let start = skip_ws(text, 0);
    if *text.as_bytes().get(start)? != b'{' {
        return None;
    }
    let mut open = start + 1;
    for key in path {
        let (value, _) = member(text, open, key)?;
        if *text.as_bytes().get(value)? != b'{' {
            return None;
        }
        open = value + 1;
    }
    Some(open)
}

/// The byte span of the value at `path` (non-empty).
#[must_use]
pub fn value_span(text: &str, path: &[&str]) -> Option<(usize, usize)> {
    let (last, parent) = path.split_last()?;
    member(text, object_open(text, parent)?, last)
}

/// Insert `"key": value` (value already JSON text) as the first member of
/// the object at `path`, in the file's own layout: on its own line with the
/// next member's indentation when members sit on lines, inline otherwise.
#[must_use]
pub fn insert_member(text: &str, path: &[&str], key: &str, value: &str) -> Option<String> {
    let open = object_open(text, path)?;
    let rest = &text[open..];
    let name = serde_json::to_string(key).ok()?;
    let member = if rest
        .trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with('}')
    {
        format!("{name}: {value}")
    } else if let Some(next) = rest.strip_prefix('\n') {
        let indent: String = next
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        format!("\n{indent}{name}: {value},")
    } else {
        format!("{name}: {value}, ")
    };
    Some(format!("{}{member}{rest}", &text[..open]))
}

/// Replace the value at `path` with `value` (JSON text).
#[must_use]
pub fn replace_value(text: &str, path: &[&str], value: &str) -> Option<String> {
    let (start, end) = value_span(text, path)?;
    Some(format!("{}{value}{}", &text[..start], &text[end..]))
}

/// Remove the whole line that carries the member at `path` (with its comma),
/// when it sits on its own line and another member follows it.
#[must_use]
pub fn remove_member_line(text: &str, path: &[&str]) -> Option<String> {
    let (start, end) = value_span(text, path)?;
    let line_start = text[..start].rfind('\n')? + 1;
    let after = &text[end..];
    let comma = after.trim_start_matches([' ', '\t']);
    let comma = comma.strip_prefix(',')?;
    let line_end = comma.find('\n')?;
    let consumed = end + (after.len() - comma.len()) + line_end + 1;
    if !text[consumed..]
        .trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with('"')
    {
        return None;
    }
    Some(format!("{}{}", &text[..line_start], &text[consumed..]))
}

/// `edited` when it parses to exactly `expected`; `None` otherwise.
#[must_use]
pub fn verified(edited: String, expected: &Value) -> Option<String> {
    let parsed: Value = serde_json::from_str(&edited).ok()?;
    (parsed == *expected).then_some(edited)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRETTY: &str = "{\n  \"schema_version\": 1,\n  \"git\": {\n    \"note\": \"a \\\"git\\\": { trap\",\n    \"pr_sections\": \"block\"\n  },\n  \"x\": [1, 2]\n}\n";

    #[test]
    fn members_are_found_by_path_not_by_text() {
        // The string value mentioning `"git": {` is never taken for the key.
        let (s, e) = value_span(PRETTY, &["git", "pr_sections"]).unwrap();
        assert_eq!(&PRETTY[s..e], "\"block\"");
        let (s, e) = value_span(PRETTY, &["x"]).unwrap();
        assert_eq!(&PRETTY[s..e], "[1, 2]");
        assert!(value_span(PRETTY, &["git", "absent"]).is_none());
        assert!(value_span("[]", &["git"]).is_none());
    }

    #[test]
    fn inserts_keep_every_other_byte_and_the_layout() {
        let out = insert_member(PRETTY, &["git"], "automation_profiles", "[]").unwrap();
        assert_eq!(
            out,
            PRETTY.replace(
                "\"git\": {\n",
                "\"git\": {\n    \"automation_profiles\": [],\n"
            )
        );
        let compact = "{\"git\":{\"a\":1}}";
        assert_eq!(
            insert_member(compact, &["git"], "b", "2").unwrap(),
            "{\"git\":{\"b\": 2, \"a\":1}}"
        );
        assert_eq!(
            insert_member("{}\n", &[], "git", "{}").unwrap(),
            "{\"git\": {}}\n"
        );
        let nested = insert_member("{}\n", &[], "git", "{}").unwrap();
        assert_eq!(
            insert_member(&nested, &["git"], "k", "\"v\"").unwrap(),
            "{\"git\": {\"k\": \"v\"}}\n"
        );
    }

    #[test]
    fn replace_and_remove_touch_only_their_member() {
        assert_eq!(
            replace_value(PRETTY, &["schema_version"], "2").unwrap(),
            PRETTY.replace("\"schema_version\": 1", "\"schema_version\": 2")
        );
        let out = remove_member_line(PRETTY, &["git", "note"]).unwrap();
        assert_eq!(
            out,
            PRETTY.replace("    \"note\": \"a \\\"git\\\": { trap\",\n", "")
        );
        // The last member cannot be removed without touching a comma.
        assert!(remove_member_line(PRETTY, &["git", "pr_sections"]).is_none());
    }
}
