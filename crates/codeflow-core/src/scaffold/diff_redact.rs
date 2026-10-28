//! What `codeflow update --diff` may show of a JSON or TOML file
//! (TSK-1029).
//!
//! A settings file can hold secrets: Claude's `env`, an MCP server's `env`
//! or `headers`, a Codex `env_http_headers` table, or a key named like a
//! token. An update that changes such a file must not export those values,
//! not even as diff context beside a line it changed. Both sides of the
//! diff are parsed and every such value is replaced by a placeholder before
//! the patch is made; a file that does not parse is withheld, since nothing
//! can show which of its lines are safe.

use serde_json::Value;

/// The placeholder for a value the diff does not show.
pub const REDACTED: &str = "<redacted>";

/// The placeholder for a hidden value that this update changed.
pub const REDACTED_CHANGED: &str = "<redacted: changed>";

/// Keys whose object or array holds values that may each be a secret.
const SECRET_CONTAINERS: [&str; 4] = ["env", "headers", "http_headers", "env_http_headers"];

/// Words in a key name that mark its value as a possible secret.
const SECRET_WORDS: [&str; 7] = [
    "token",
    "secret",
    "password",
    "passwd",
    "credential",
    "apikey",
    "api_key",
];

/// The two sides of a diff, ready to show.
#[derive(Debug, PartialEq, Eq)]
pub enum DiffSides {
    /// Show these texts: the originals for a file that is neither JSON nor
    /// TOML, or the redacted renderings of both sides.
    Show(String, String),
    /// Show no content: a JSON or TOML side did not parse.
    Withheld,
}

/// The sides of the diff for `dest` that `update --diff` may export.
#[must_use]
pub fn sides_for(dest: &str, old: &str, new: &str) -> DiffSides {
    let format = match std::path::Path::new(dest)
        .extension()
        .and_then(|ext| ext.to_str())
    {
        Some("json") => Format::Json,
        Some("toml") => Format::Toml,
        _ => return DiffSides::Show(old.to_string(), new.to_string()),
    };
    let (Some(old_value), Some(new_value)) = (format.parse(old), format.parse(new)) else {
        return DiffSides::Withheld;
    };
    let mut shown_new = new_value.clone();
    redact(&mut shown_new, Some(&old_value), false);
    let mut shown_old = old_value;
    redact(&mut shown_old, None, false);
    match (format.render(&shown_old), format.render(&shown_new)) {
        (Some(old), Some(new)) => DiffSides::Show(old, new),
        _ => DiffSides::Withheld,
    }
}

#[derive(Clone, Copy)]
enum Format {
    Json,
    Toml,
}

impl Format {
    fn parse(self, text: &str) -> Option<Value> {
        match self {
            Format::Json => serde_json::from_str(text).ok(),
            Format::Toml => toml::from_str::<toml::Value>(text)
                .ok()
                .and_then(|value| serde_json::to_value(value).ok()),
        }
    }

    fn render(self, value: &Value) -> Option<String> {
        let mut text = match self {
            Format::Json => serde_json::to_string_pretty(value).ok()?,
            Format::Toml => toml::to_string(&toml::Value::try_from(value).ok()?).ok()?,
        };
        if !text.ends_with('\n') {
            text.push('\n');
        }
        Some(text)
    }
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    SECRET_CONTAINERS.contains(&key.as_str()) || SECRET_WORDS.iter().any(|word| key.contains(word))
}

/// Replace every value under a secret key with a placeholder. `before` is
/// the same place on the other side, unredacted, so a changed value reads
/// as changed; `None` marks every hidden value as plain [`REDACTED`].
fn redact(value: &mut Value, before: Option<&Value>, secret: bool) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let prior = before.and_then(|before| before.get(key));
                redact(child, prior, secret || secret_key(key));
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter_mut().enumerate() {
                let prior = before.and_then(|before| before.get(index));
                redact(child, prior, secret);
            }
        }
        scalar if secret => {
            let changed = before.is_some_and(|before| *before != *scalar);
            *scalar = Value::String(if changed { REDACTED_CHANGED } else { REDACTED }.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic value no output may show.
    const SECRET: &str = "opaque-neighbour-value-31337";

    fn shown(dest: &str, old: &str, new: &str) -> (String, String) {
        match sides_for(dest, old, new) {
            DiffSides::Show(old, new) => (old, new),
            DiffSides::Withheld => panic!("withheld {dest}"),
        }
    }

    #[test]
    fn a_settings_env_shows_keys_and_marks_a_changed_value() {
        let old = format!(r#"{{"env": {{"TOKEN": "{SECRET}", "PCT": "50"}}, "model": "x"}}"#);
        let new = format!(r#"{{"env": {{"TOKEN": "{SECRET}", "PCT": "35"}}, "model": "y"}}"#);
        let (old, new) = shown(".claude/settings.json", &old, &new);
        for text in [&old, &new] {
            assert!(!text.contains(SECRET) && !text.contains("50"), "{text}");
            assert!(text.contains("\"TOKEN\": \"<redacted>\""), "{text}");
        }
        assert!(new.contains("\"PCT\": \"<redacted: changed>\""), "{new}");
        assert!(old.contains("\"PCT\": \"<redacted>\""), "{old}");
        assert!(new.contains("\"model\": \"y\""), "plain values stay: {new}");
    }

    #[test]
    fn nested_and_named_secrets_are_hidden_in_json_and_toml() {
        let json = format!(
            r#"{{"mcpServers": {{"a": {{"env": {{"K": "{SECRET}"}},
                "headers": {{"Authorization": "{SECRET}"}}}}}},
               "apiKeyHelper": "/bin/helper", "github_token": "{SECRET}"}}"#
        );
        let (old, _) = shown(".mcp.json", &json, &json);
        assert!(!old.contains(SECRET), "{old}");
        assert!(old.contains("\"Authorization\": \"<redacted>\""), "{old}");

        let toml_text = format!(
            "model = \"m\"\n[mcp_servers.a]\nbearer_token = \"{SECRET}\"\n\
             [mcp_servers.a.env]\nK = \"{SECRET}\"\n\
             [mcp_servers.a.env_http_headers]\nX = \"{SECRET}\"\n"
        );
        let (old, _) = shown(".codex/config.toml", &toml_text, &toml_text);
        assert!(!old.contains(SECRET), "{old}");
        assert!(old.contains("model = \"m\""), "{old}");
    }

    #[test]
    fn an_unparsable_side_is_withheld_and_other_files_are_shown() {
        let bad = format!("{{\"env\": {{\"K\": \"{SECRET}\"");
        assert_eq!(
            sides_for(".claude/settings.json", &bad, "{}"),
            DiffSides::Withheld
        );
        assert_eq!(
            sides_for(".codex/config.toml", "x = [", "x = 1\n"),
            DiffSides::Withheld
        );
        assert_eq!(
            sides_for("AGENTS.md", "a\n", "b\n"),
            DiffSides::Show("a\n".to_string(), "b\n".to_string())
        );
    }
}
