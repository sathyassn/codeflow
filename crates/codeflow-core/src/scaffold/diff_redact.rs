//! What `codeflow update --diff` may show of a file it changed (TSK-1029).
//!
//! A project's files can hold secrets: Claude's `env`, an MCP server's
//! headers, a Codex `env_http_headers` table, a literal credential a
//! project added to a workflow. The export shows only what CodeFlow itself
//! wrote, so no project value reaches it, not even as context beside a line
//! the update changed.
//!
//! - **Fail closed by format.** The format comes from the path, and a path
//!   with no rule here is withheld whole. A file that does not parse in its
//!   format is withheld too.
//! - **JSON, TOML and YAML: owned keys only.** A scalar is shown only where
//!   a shipped version (the old baseline or the new rendering) has a scalar
//!   at the same key path, and an array item only when it equals an item of
//!   the shipped array. Every other value becomes [`REDACTED`], or
//!   [`REDACTED_CHANGED`] where the two sides differ. Key names stay
//!   visible. A key that names a secret (`env`, `headers`, `token`,
//!   `api-key` in any case or separator) is hidden even when owned, as
//!   defence in depth. The rendering is normalised, so comments and layout
//!   do not show.
//! - **Markdown, code and other line-based text: owned lines only.** A line
//!   is shown when a shipped version holds the same line; every other line
//!   becomes [`REDACTED_LINE`].

use std::collections::HashSet;

use serde_json::Value;

/// The placeholder for a value the diff does not show.
pub const REDACTED: &str = "<redacted>";

/// The placeholder for a hidden value that this update changed.
pub const REDACTED_CHANGED: &str = "<redacted: changed>";

/// The placeholder for a line of text that CodeFlow did not ship.
pub const REDACTED_LINE: &str = "<redacted: project line>";

/// Normalised key names whose object or array holds values that may each
/// be a secret.
const SECRET_CONTAINERS: [&str; 4] = ["env", "headers", "httpheaders", "envhttpheaders"];

/// Normalised words in a key name that mark its value as a possible secret.
const SECRET_WORDS: [&str; 8] = [
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "apikey",
    "accesskey",
    "privatekey",
];

/// Extensions of line-based text that CodeFlow ships: instructions, code,
/// styles and pages.
const TEXT_EXTENSIONS: [&str; 13] = [
    "md", "markdown", "txt", "py", "js", "mjs", "cjs", "ts", "sh", "css", "html", "htm", "rules",
];

/// The two sides of a diff, ready to show.
#[derive(Debug, PartialEq, Eq)]
pub enum DiffSides {
    /// Show these texts: both sides with every value or line CodeFlow did
    /// not ship replaced by a placeholder.
    Show(String, String),
    /// Show no content: the format has no redaction rule here, or a side
    /// did not parse.
    Withheld,
}

/// The sides of the diff for `dest` that `update --diff` may export.
/// `shipped` holds the versions CodeFlow wrote: the old baseline when there
/// is one, and the new rendering.
#[must_use]
pub fn sides_for(dest: &str, old: &str, new: &str, shipped: &[&str]) -> DiffSides {
    match Kind::of(dest) {
        Kind::Structured(format) => structured(format, old, new, shipped),
        Kind::Text => {
            let owned: HashSet<&str> = shipped.iter().flat_map(|text| text.lines()).collect();
            DiffSides::Show(owned_lines(old, &owned), owned_lines(new, &owned))
        }
        Kind::Unknown => DiffSides::Withheld,
    }
}

/// How a path's contents are redacted.
enum Kind {
    Structured(Format),
    Text,
    /// No rule: the default, so a new format is withheld until it has one.
    Unknown,
}

impl Kind {
    fn of(dest: &str) -> Kind {
        let path = std::path::Path::new(dest);
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("json") => Kind::Structured(Format::Json),
            Some("toml") => Kind::Structured(Format::Toml),
            Some("yaml" | "yml") => Kind::Structured(Format::Yaml),
            Some(ext) if TEXT_EXTENSIONS.contains(&ext) => Kind::Text,
            // The managed files with no extension: the ignore list and the
            // git hook scripts CodeFlow ships.
            None if dest == ".gitignore" || dest.starts_with(".codeflow/git-hooks/") => Kind::Text,
            _ => Kind::Unknown,
        }
    }
}

fn structured(format: Format, old: &str, new: &str, shipped: &[&str]) -> DiffSides {
    let (Some(old_value), Some(new_value)) = (format.parse(old), format.parse(new)) else {
        return DiffSides::Withheld;
    };
    // A shipped version that does not parse owns nothing, so it hides more.
    let shipped: Vec<Value> = shipped
        .iter()
        .filter_map(|text| format.parse(text))
        .collect();
    let owners: Vec<&Value> = shipped.iter().collect();
    let mut shown_new = new_value.clone();
    redact(&mut shown_new, Some(&old_value), &owners, false);
    let mut shown_old = old_value;
    redact(&mut shown_old, None, &owners, false);
    match (format.render(&shown_old), format.render(&shown_new)) {
        (Some(old), Some(new)) => DiffSides::Show(old, new),
        _ => DiffSides::Withheld,
    }
}

fn owned_lines(text: &str, owned: &HashSet<&str>) -> String {
    let mut shown = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let (body, end) = match line.strip_suffix('\n') {
            Some(body) => (body, "\n"),
            None => (line, ""),
        };
        shown.push_str(if owned.contains(body) {
            body
        } else {
            REDACTED_LINE
        });
        shown.push_str(end);
    }
    shown
}

#[derive(Clone, Copy)]
enum Format {
    Json,
    Toml,
    Yaml,
}

impl Format {
    fn parse(self, text: &str) -> Option<Value> {
        match self {
            Format::Json => serde_json::from_str(text).ok(),
            Format::Toml => toml::from_str::<toml::Value>(text)
                .ok()
                .and_then(|value| serde_json::to_value(value).ok()),
            Format::Yaml => serde_yaml::from_str(text).ok(),
        }
    }

    fn render(self, value: &Value) -> Option<String> {
        let mut text = match self {
            Format::Json => serde_json::to_string_pretty(value).ok()?,
            Format::Toml => toml::to_string(&toml::Value::try_from(value).ok()?).ok()?,
            Format::Yaml => serde_yaml::to_string(value).ok()?,
        };
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Some(text)
    }
}

/// A key whose value is hidden even where CodeFlow owns it. Case and
/// separators are ignored, so `api-key`, `api key`, `api.key` and `apiKey`
/// all match.
fn secret_key(key: &str) -> bool {
    let key: String = key
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    SECRET_CONTAINERS.contains(&key.as_str()) || SECRET_WORDS.iter().any(|word| key.contains(word))
}

/// Replace every value CodeFlow does not own, and every value under a
/// secret key, with a placeholder. `owners` are the shipped values at the
/// same place; `before` is the same place on the other side, unredacted,
/// so a changed value reads as changed, and `None` marks every hidden value
/// as plain [`REDACTED`].
fn redact(value: &mut Value, before: Option<&Value>, owners: &[&Value], secret: bool) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let prior = before.and_then(|before| before.get(key));
                let owned: Vec<&Value> = owners.iter().filter_map(|owner| owner.get(key)).collect();
                redact(child, prior, &owned, secret || secret_key(key));
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter_mut().enumerate() {
                let prior = before.and_then(|before| before.get(index));
                let owned: Vec<&Value> = owners
                    .iter()
                    .filter_map(|owner| owner.as_array())
                    .flatten()
                    .filter(|item| **item == *child)
                    .collect();
                redact(child, prior, &owned, secret);
            }
        }
        scalar => {
            let owned = owners
                .iter()
                .any(|owner| !owner.is_object() && !owner.is_array());
            if secret || !owned {
                let changed = before.is_some_and(|before| *before != *scalar);
                *scalar =
                    Value::String(if changed { REDACTED_CHANGED } else { REDACTED }.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic value no output may show.
    const SECRET: &str = "opaque-neighbour-value-31337";

    fn shown(dest: &str, old: &str, new: &str, shipped: &[&str]) -> (String, String) {
        match sides_for(dest, old, new, shipped) {
            DiffSides::Show(old, new) => (old, new),
            DiffSides::Withheld => panic!("withheld {dest}"),
        }
    }

    #[test]
    fn a_settings_env_shows_keys_and_marks_a_changed_value() {
        let old = format!(r#"{{"env": {{"TOKEN": "{SECRET}", "PCT": "50"}}, "model": "x"}}"#);
        let new = format!(r#"{{"env": {{"TOKEN": "{SECRET}", "PCT": "35"}}, "model": "y"}}"#);
        let shipped = [r#"{"env": {"PCT": "35"}, "model": "y"}"#];
        let (old, new) = shown(".claude/settings.json", &old, &new, &shipped);
        for text in [&old, &new] {
            assert!(!text.contains(SECRET) && !text.contains("50"), "{text}");
            assert!(text.contains("\"TOKEN\": \"<redacted>\""), "{text}");
        }
        assert!(new.contains("\"PCT\": \"<redacted: changed>\""), "{new}");
        assert!(old.contains("\"PCT\": \"<redacted>\""), "{old}");
        assert!(new.contains("\"model\": \"y\""), "owned values stay: {new}");
    }

    #[test]
    fn only_owned_values_and_items_are_shown() {
        let shipped =
            [r#"{"model": "m", "permissions": {"allow": ["Read"]}, "sandbox": {"on": true}}"#];
        let project = format!(
            r#"{{"model": "m", "note": "{SECRET}", "sandbox": "{SECRET}",
                "permissions": {{"allow": ["Read", "Bash(curl {SECRET})"]}}}}"#
        );
        let (old, _) = shown(".claude/settings.json", &project, &project, &shipped);
        assert!(!old.contains(SECRET), "{old}");
        assert!(old.contains("\"model\": \"m\""), "{old}");
        assert!(old.contains("\"Read\""), "{old}");
        assert!(old.contains("\"note\": \"<redacted>\""), "{old}");
    }

    #[test]
    fn nested_and_named_secrets_are_hidden_in_json_and_toml() {
        let json = format!(
            r#"{{"mcpServers": {{"a": {{"env": {{"K": "{SECRET}"}},
                "headers": {{"Authorization": "{SECRET}"}}}}}},
               "apiKeyHelper": "/bin/helper", "github_token": "{SECRET}"}}"#
        );
        // Owned paths with secret names stay hidden.
        let (old, _) = shown(".mcp.json", &json, &json, &[&json]);
        assert!(!old.contains(SECRET), "{old}");
        assert!(old.contains("\"Authorization\": \"<redacted>\""), "{old}");

        let toml_text = format!(
            "model = \"m\"\n[mcp_servers.a]\nbearer_token = \"{SECRET}\"\n\
             [mcp_servers.a.env]\nK = \"{SECRET}\"\n\
             [mcp_servers.a.env_http_headers]\nX = \"{SECRET}\"\n"
        );
        let (old, _) = shown(".codex/config.toml", &toml_text, &toml_text, &[&toml_text]);
        assert!(!old.contains(SECRET), "{old}");
        assert!(old.contains("model = \"m\""), "{old}");
    }

    #[test]
    fn separator_variants_are_hidden_in_json_and_toml() {
        let json = format!(
            r#"{{"api-key": "{SECRET}", "api key": 271828, "api.key": [true],
                "tools": {{"httpHeaders": {{"Authorization": "{SECRET}"}},
                           "http-headers": {{"X": 271828}},
                           "inner": {{"api-key": {{"deep": "{SECRET}"}}}}}}}}"#
        );
        let toml_text = format!(
            "\"api-key\" = \"{SECRET}\"\n\"api key\" = 271828\n\"api.key\" = true\n\
             [tools.httpHeaders]\nAuthorization = \"{SECRET}\"\n\
             [tools.\"http-headers\"]\nX = 271828\n\
             [tools.inner.\"api-key\"]\ndeep = \"{SECRET}\"\n"
        );
        // Unowned, and then owned at every path: the name rule still hides
        // each value.
        for (dest, text) in [
            (".claude/settings.json", &json),
            (".codex/config.toml", &toml_text),
        ] {
            for shipped in [vec![], vec![text.as_str()]] {
                let (old, _) = shown(dest, text, text, &shipped);
                assert!(!old.contains(SECRET) && !old.contains("271828"), "{old}");
                assert!(!old.contains("true"), "{old}");
            }
        }
    }

    #[test]
    fn a_managed_workflow_shows_owned_keys_and_hides_project_values() {
        let shipped = "name: ci\non:\n  push: {}\njobs:\n  test:\n    runs-on: ubuntu-latest\n";
        let project = format!("{shipped}env:\n  RELEASE_NOTE: {SECRET}\n");
        let (old, new) = shown(
            ".github/workflows/codeflow-ci.yml",
            &project,
            shipped,
            &[shipped],
        );
        assert!(!old.contains(SECRET), "{old}");
        assert!(old.contains("RELEASE_NOTE: <redacted>"), "{old}");
        assert!(old.contains("runs-on: ubuntu-latest"), "{old}");
        assert!(new.contains("name: ci"), "{new}");
    }

    #[test]
    fn malformed_yaml_is_withheld() {
        let bad = format!("name: ci\nenv: [\n  NOTE: {SECRET}\n");
        assert_eq!(
            sides_for(
                ".github/workflows/codeflow-ci.yml",
                &bad,
                "name: ci\n",
                &["name: ci\n"]
            ),
            DiffSides::Withheld
        );
    }

    #[test]
    fn text_shows_shipped_lines_and_hides_project_lines() {
        let shipped = "# Title\n\nshipped line\n";
        let project = format!("# Title\n\nshipped line\nexport KEY={SECRET}\n");
        for dest in ["AGENTS.md", ".codeflow/git-hooks/pre-push", ".gitignore"] {
            let (old, new) = shown(dest, &project, shipped, &[shipped]);
            assert_eq!(old, format!("# Title\n\nshipped line\n{REDACTED_LINE}\n"));
            assert_eq!(new, shipped);
        }
    }

    #[test]
    fn an_unknown_format_is_withheld() {
        for dest in ["notes.ini", ".env", "bin/run", "config.properties"] {
            let old = format!("K={SECRET}\n");
            assert_eq!(
                sides_for(dest, &old, "K=1\n", &["K=1\n"]),
                DiffSides::Withheld,
                "{dest}"
            );
        }
    }

    #[test]
    fn an_unparsable_side_is_withheld() {
        let bad = format!("{{\"env\": {{\"K\": \"{SECRET}\"");
        assert_eq!(
            sides_for(".claude/settings.json", &bad, "{}", &["{}"]),
            DiffSides::Withheld
        );
        assert_eq!(
            sides_for(".codex/config.toml", "x = [", "x = 1\n", &["x = 1\n"]),
            DiffSides::Withheld
        );
    }

    #[test]
    fn a_shipped_version_that_does_not_parse_owns_nothing() {
        let (old, _) = shown(
            ".claude/settings.json",
            r#"{"model": "m"}"#,
            r#"{"model": "m"}"#,
            &["{"],
        );
        assert!(old.contains("\"model\": \"<redacted>\""), "{old}");
    }
}
