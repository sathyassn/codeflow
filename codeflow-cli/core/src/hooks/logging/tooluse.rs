//! `PostToolUse` logging handler with sensitive-pattern redaction.
//!
//! Replaces the `PostToolUseLogging` stub in `post_tool_use.rs` with a
//! full implementation matching Go's `logging/tooluse.go`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use regex::Regex;

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput};

use super::writer::ActivityWriter;
use super::{read_config, resolve_session_id};

/// Logs `PostToolUse` events to `tool-use-{YYYY-MM-DD}.jsonl` with
/// configurable tool filtering, input truncation, and sensitive redaction.
pub struct ToolUseLogging {
    pub project_dir: PathBuf,
}

impl ToolUseLogging {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for ToolUseLogging {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let cfg = read_config(&self.project_dir);
        if !cfg.post_tool_use.enabled {
            return Ok(HookOutput::Allow);
        }

        let tool_name = match &input.tool_name {
            Some(name) if !name.is_empty() => name.clone(),
            _ => return Ok(HookOutput::Allow),
        };

        // Check tool filter.
        if !cfg.post_tool_use.tools_to_log.is_empty()
            && !cfg.post_tool_use.tools_to_log.contains(&tool_name)
        {
            return Ok(HookOutput::Allow);
        }

        let session_id = resolve_session_id(&self.project_dir);

        // Summarize tool input.
        let mut tool_input_str = input
            .tool_input
            .as_ref()
            .map(summarize_json)
            .unwrap_or_default();

        // Summarize tool result (from tool_input.tool_result if present).
        let mut result_str = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("tool_result"))
            .map(summarize_json)
            .unwrap_or_default();

        let truncated = result_str.len() > cfg.post_tool_use.max_result_size;
        if truncated {
            result_str.truncate(cfg.post_tool_use.max_result_size);
            result_str.push_str("...[truncated]");
        }

        if cfg.post_tool_use.redact_sensitive {
            tool_input_str = redact_sensitive(&tool_input_str);
            result_str = redact_sensitive(&result_str);
        }

        // Best-effort logging: create writer, append record.
        let writer = match ActivityWriter::new(&self.project_dir, &cfg.post_tool_use.log_directory)
        {
            Ok(w) => w,
            Err(e) => {
                return Ok(HookOutput::Warn {
                    message: format!("tool-use logging init failed: {e}"),
                });
            }
        };

        let mut record = HashMap::new();
        record.insert("ts".into(), serde_json::json!(writer.timestamp()));
        record.insert("session_id".into(), serde_json::json!(session_id));
        record.insert("event".into(), serde_json::json!("tool_completed"));
        record.insert("tool_name".into(), serde_json::json!(tool_name));
        record.insert("tool_input".into(), serde_json::json!(tool_input_str));
        record.insert("tool_result".into(), serde_json::json!(result_str));
        record.insert("result_truncated".into(), serde_json::json!(truncated));

        if let Err(e) = writer.append("tool-use", &record) {
            return Ok(HookOutput::Warn {
                message: format!("tool-use logging write failed: {e}"),
            });
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "post-tool-use-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

/// Convert a JSON value to a compact string.
fn summarize_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Sensitive pattern redaction
// ---------------------------------------------------------------------------

/// Compiled sensitive-pattern regexes (compiled once via `OnceLock`).
fn sensitive_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                Regex::new(r"(?i)(password\s*[:=]\s*)\S+").expect("valid regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new(r"(?i)(token\s*[:=]\s*)\S+").expect("valid regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new(r"(?i)(key\s*[:=]\s*)\S+").expect("valid regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new(r"(?i)(secret\s*[:=]\s*)\S+").expect("valid regex"),
                "${1}[REDACTED]",
            ),
            (
                Regex::new(r"Bearer\s+[A-Za-z0-9_-]+").expect("valid regex"),
                "Bearer [REDACTED]",
            ),
            (
                Regex::new(r"[A-Za-z0-9]{40,}").expect("valid regex"),
                "[LONG_TOKEN_REDACTED]",
            ),
            (
                Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
                    .expect("valid regex"),
                "[EMAIL_REDACTED]",
            ),
        ]
    })
}

/// Remove common secret patterns from text.
#[must_use]
pub fn redact_sensitive(text: &str) -> String {
    let mut result = text.to_string();
    for (pattern, replacement) in sensitive_patterns() {
        result = pattern.replace_all(&result, *replacement).into_owned();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_password() {
        let input = "password: my_secret_pass";
        let result = redact_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("my_secret_pass"));
    }

    #[test]
    fn test_redact_token() {
        let input = "token = abc123xyz";
        let result = redact_sensitive(input);
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("abc123xyz"));
    }

    #[test]
    fn test_redact_bearer() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9";
        let result = redact_sensitive(input);
        assert!(result.contains("Bearer [REDACTED]"));
        assert!(!result.contains("eyJhbGciOiJIUzI1NiJ9"));
    }

    #[test]
    fn test_redact_long_token() {
        let input = "ghp_1234567890abcdef1234567890abcdef12345678";
        let result = redact_sensitive(input);
        assert!(result.contains("[LONG_TOKEN_REDACTED]"));
    }

    #[test]
    fn test_redact_email() {
        let input = "contact: user@example.com";
        let result = redact_sensitive(input);
        assert!(result.contains("[EMAIL_REDACTED]"));
        assert!(!result.contains("user@example.com"));
    }

    #[test]
    fn test_redact_preserves_safe_text() {
        let input = "normal log message with no secrets";
        let result = redact_sensitive(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_redact_key() {
        let input = "api_key: sk-proj-abcdef123";
        let result = redact_sensitive(input);
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_redact_secret() {
        let input = "secret = hunter2";
        let result = redact_sensitive(input);
        assert!(result.contains("[REDACTED]"));
    }

    #[test]
    fn test_summarize_json_null() {
        assert_eq!(summarize_json(&serde_json::Value::Null), "");
    }

    #[test]
    fn test_summarize_json_string() {
        let v = serde_json::json!("hello");
        assert_eq!(summarize_json(&v), "hello");
    }

    #[test]
    fn test_summarize_json_object() {
        let v = serde_json::json!({"key": "val"});
        let result = summarize_json(&v);
        assert!(result.contains("key"));
        assert!(result.contains("val"));
    }

    #[test]
    fn test_tool_use_logging_handler_metadata() {
        let handler = ToolUseLogging::new(PathBuf::from("/tmp/test"));
        assert_eq!(handler.name(), "post-tool-use-logging");
        assert_eq!(handler.events(), &[HookEvent::PostToolUse]);
    }

    #[test]
    fn test_tool_use_logging_no_tool_name() {
        let dir = tempfile::tempdir().unwrap();
        let handler = ToolUseLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::PostToolUse,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_tool_use_logging_writes_record() {
        let dir = tempfile::tempdir().unwrap();
        // Ensure the config yields defaults (enabled=true).
        let handler = ToolUseLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PostToolUse,
            session_id: Some("ses-test".into()),
            project_dir: Some(dir.path().to_string_lossy().to_string()),
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Check that a tool-use log file was created.
        let log_dir = dir.path().join(".state/logs/sessions");
        assert!(log_dir.exists(), "log directory should be created");
        let entries: Vec<_> = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("tool-use-"))
            .collect();
        assert!(!entries.is_empty(), "tool-use log file should exist");
    }

    #[test]
    fn test_tool_use_logging_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"post_tool_use": {"enabled": false}}}"#,
        )
        .unwrap();

        let handler = ToolUseLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PostToolUse,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // No log files should be created.
        let log_dir = dir.path().join(".state/logs/sessions");
        assert!(!log_dir.exists());
    }

    #[test]
    fn test_tool_use_logging_tool_filter() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"post_tool_use": {"enabled": true, "tools_to_log": ["Edit", "Write"]}}}"#,
        )
        .unwrap();

        let handler = ToolUseLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "ls"})),
            event: HookEvent::PostToolUse,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Bash is not in the filter list, so no log file should be created.
        let log_dir = dir.path().join(".state/logs/sessions");
        assert!(!log_dir.exists());
    }
}
