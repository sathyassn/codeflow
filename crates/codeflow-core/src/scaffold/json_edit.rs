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

/// Apply a semantic JSON change while keeping the bytes of unchanged members
/// and array entries. The caller supplies the merge's result, never a second
/// merge policy. Parsing the result again verifies every splice.
#[must_use]
pub fn preserving_edit(text: &str, expected: &Value) -> Option<String> {
    let before: Value = serde_json::from_str(text).ok()?;
    verified(edit_value(text, &before, expected)?, expected)
}

fn edit_value(text: &str, before: &Value, after: &Value) -> Option<String> {
    if before == after {
        return Some(text.to_string());
    }
    match (before, after) {
        (Value::Object(old), Value::Object(new)) => {
            let mut out = text.to_string();
            for key in old.keys().filter(|key| !new.contains_key(*key)) {
                out = remove_member(&out, key)?;
            }
            for (key, value) in new {
                if let Some(previous) = old.get(key) {
                    let (start, end) = value_span(&out, &[key.as_str()])?;
                    let next = edit_value(&out[start..end], previous, value)?;
                    out.replace_range(start..end, &next);
                } else {
                    out = insert_member(&out, &[], key, &serde_json::to_string(value).ok()?)?;
                }
            }
            Some(out)
        }
        (Value::Array(old), Value::Array(new)) => {
            // Keep each retained entry's spelling and surrounding whitespace,
            // even when the permission merge changes its position. The merge
            // owns ordering and membership; this layer only renders its result.
            let segments = array_segments(text)?;
            let mut entries = Vec::with_capacity(new.len());
            let mut used = vec![false; old.len()];
            for value in new {
                if let Some(index) = old
                    .iter()
                    .enumerate()
                    .position(|(index, prior)| !used[index] && prior == value)
                {
                    used[index] = true;
                    entries.push(segments.get(index)?.to_string());
                } else {
                    entries.push(serde_json::to_string(value).ok()?);
                }
            }
            let open = skip_ws(text, 0);
            let close = text.rfind(']')?;
            Some(format!(
                "{}{}{}",
                &text[..=open],
                entries.join(","),
                &text[close..]
            ))
        }
        _ => {
            let start = skip_ws(text, 0);
            let end = value_end(text, start)?;
            Some(format!(
                "{}{}{}",
                &text[..start],
                serde_json::to_string(after).ok()?,
                &text[end..]
            ))
        }
    }
}

/// Array entries without commas, including the whitespace each person wrote
/// around the value. JSON strings may contain commas, brackets and escapes.
fn array_segments(text: &str) -> Option<Vec<&str>> {
    let open = skip_ws(text, 0);
    if *text.as_bytes().get(open)? != b'[' {
        return None;
    }
    let mut start = open + 1;
    let mut segments = Vec::new();
    loop {
        let value = skip_ws(text, start);
        if *text.as_bytes().get(value)? == b']' {
            return Some(segments);
        }
        let end = skip_ws(text, value_end(text, value)?);
        segments.push(&text[start..end]);
        match text.as_bytes().get(end)? {
            b',' => start = end + 1,
            b']' => return Some(segments),
            _ => return None,
        }
    }
}

/// Remove a member in any object layout, including the final or only member.
/// Parse key tokens rather than searching for their possibly escaped bytes.
fn remove_member(text: &str, key: &str) -> Option<String> {
    let mut at = object_open(text, &[])?;
    let mut preceding_comma = None;
    loop {
        let start = skip_ws(text, at);
        let mut names = serde_json::Deserializer::from_str(&text[start..]).into_iter::<String>();
        let name = names.next()?.ok()?;
        at = skip_ws(text, start + names.byte_offset());
        if *text.as_bytes().get(at)? != b':' {
            return None;
        }
        let end = value_end(text, skip_ws(text, at + 1))?;
        let next = skip_ws(text, end);
        if name == key {
            let (start, end) = if text.as_bytes().get(next) == Some(&b',') {
                (start, next + 1)
            } else {
                (preceding_comma.unwrap_or(start), end)
            };
            return Some(format!("{}{}", &text[..start], &text[end..]));
        }
        if text.as_bytes().get(next) != Some(&b',') {
            return None;
        }
        preceding_comma = Some(next);
        at = next + 1;
    }
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

    #[test]
    fn semantic_edits_keep_project_members_and_array_spelling() {
        let current = "{\n  \"ours\" : [ \"\\u0078\",  {\"a\":1} ],\n  \"rules\": [\n\t\"Read(project/\\u002a)\" ,\n  \"retired\"\n],\n  \"managed\": 1\n}\n";
        let expected = serde_json::json!({
            "ours": ["x", {"a": 1}],
            "rules": ["new", "Read(project/*)"],
            "managed": 2
        });
        let out = preserving_edit(current, &expected).unwrap();
        assert!(out.contains("\"ours\" : [ \"\\u0078\",  {\"a\":1} ]"));
        assert!(out.contains("\n\t\"Read(project/\\u002a)\" "));
        assert_eq!(preserving_edit(&out, &expected).unwrap(), out);
    }

    #[test]
    fn semantic_edits_handle_member_removal_and_nested_values() {
        for text in [
            r#"{"first":1,"keep":{"x":["a,]",2]},"last":3}"#,
            "{\r\n \"first\": 1,\r\n \"keep\": {\"x\":[\"a,]\",2]},\r\n \"last\": 3\r\n}\r\n",
        ] {
            let before: Value = serde_json::from_str(text).unwrap();
            assert_eq!(preserving_edit(text, &before).unwrap(), text);
            // Finite mutation matrix: delete the first, middle, final or every
            // member, then add a nested value. The parsed result is the oracle.
            for removed in [
                vec!["first"],
                vec!["keep"],
                vec!["last"],
                vec!["first", "keep", "last"],
            ] {
                let mut expected = before.clone();
                for key in removed {
                    expected.as_object_mut().unwrap().remove(key);
                }
                expected["added"] = serde_json::json!({"nested": [true, null]});
                let out = preserving_edit(text, &expected).unwrap();
                assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), expected);
            }
        }
    }

    #[test]
    fn duplicate_array_values_keep_their_distinct_spelling() {
        let before = r#"{"rules":["x", "\u0078"]}"#;
        let expected = serde_json::json!({"rules": ["x", "x", "new"]});
        let out = preserving_edit(before, &expected).unwrap();
        assert!(out.contains(r#""x", "\u0078""#), "{out}");
    }
}
