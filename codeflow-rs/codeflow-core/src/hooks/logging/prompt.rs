//! `UserPromptSubmit` logging handler with intent detection.

use std::collections::HashMap;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput};

use super::writer::ActivityWriter;
use super::{read_config, resolve_session_id};

/// Logs `UserPromptSubmit` events to `prompts-{YYYY-MM-DD}.jsonl` with
/// SHA-256 hashing, length tracking, and intent classification.
pub struct PromptLogging {
    pub project_dir: PathBuf,
}

impl PromptLogging {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for PromptLogging {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let cfg = read_config(&self.project_dir);
        if !cfg.user_prompt.enabled {
            return Ok(HookOutput::Allow);
        }

        let session_id = resolve_session_id(&self.project_dir);

        // Extract user prompt text from tool_input.
        let user_prompt = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("user_prompt"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let prompt_len = user_prompt.len();
        let prompt_hash = if user_prompt.is_empty() {
            String::new()
        } else {
            let hash = Sha256::digest(user_prompt.as_bytes());
            format!("{hash:x}")
        };

        let prompt_type = if cfg.user_prompt.detect_intent && !user_prompt.is_empty() {
            detect_prompt_type(user_prompt)
        } else {
            "general"
        };

        let writer = match ActivityWriter::new(&self.project_dir, &cfg.user_prompt.log_directory) {
            Ok(w) => w,
            Err(e) => {
                return Ok(HookOutput::Warn {
                    message: format!("prompt logging init failed: {e}"),
                });
            }
        };

        let mut record = HashMap::new();
        record.insert("ts".into(), serde_json::json!(writer.timestamp()));
        record.insert("session_id".into(), serde_json::json!(session_id));
        record.insert("event".into(), serde_json::json!("prompt_submitted"));
        record.insert("prompt_length".into(), serde_json::json!(prompt_len));
        record.insert("prompt_hash".into(), serde_json::json!(prompt_hash));
        record.insert("prompt_type".into(), serde_json::json!(prompt_type));

        // In privacy mode, never include full text.
        if !cfg.user_prompt.privacy_mode
            && cfg.user_prompt.capture_full_text
            && !user_prompt.is_empty()
        {
            record.insert("prompt_text".into(), serde_json::json!(user_prompt));
        }

        if let Err(e) = writer.append("prompts", &record) {
            return Ok(HookOutput::Warn {
                message: format!("prompt logging write failed: {e}"),
            });
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "user-prompt-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::UserPromptSubmit]
    }
}

/// Classify a user prompt into a type category.
#[must_use]
fn detect_prompt_type(prompt: &str) -> &'static str {
    if prompt.starts_with('/') {
        return "command";
    }
    let lower = prompt.to_lowercase();
    if prompt.contains('?') {
        return "question";
    }
    if contains_any(&lower, &["fix", "bug", "error", "issue"]) {
        return "debugging";
    }
    if contains_any(&lower, &["create", "add", "implement", "build"]) {
        return "creation";
    }
    if contains_any(&lower, &["update", "change", "modify", "edit"]) {
        return "modification";
    }
    if contains_any(&lower, &["review", "check", "verify", "test"]) {
        return "review";
    }
    if contains_any(&lower, &["find", "search", "locate", "where"]) {
        return "navigation";
    }
    "general"
}

/// Check if text contains any of the given words.
fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_command() {
        assert_eq!(detect_prompt_type("/cf-develop"), "command");
        assert_eq!(detect_prompt_type("/help"), "command");
    }

    #[test]
    fn test_detect_question() {
        assert_eq!(detect_prompt_type("What is this?"), "question");
        assert_eq!(detect_prompt_type("How does it work?"), "question");
    }

    #[test]
    fn test_detect_debugging() {
        assert_eq!(detect_prompt_type("fix the broken test"), "debugging");
        assert_eq!(
            detect_prompt_type("there is a bug in session.go"),
            "debugging"
        );
        assert_eq!(
            detect_prompt_type("I see an error in the output"),
            "debugging"
        );
    }

    #[test]
    fn test_detect_creation() {
        assert_eq!(detect_prompt_type("create a new module"), "creation");
        assert_eq!(detect_prompt_type("add a validation function"), "creation");
        assert_eq!(
            detect_prompt_type("implement the logging handler"),
            "creation"
        );
        assert_eq!(detect_prompt_type("build the autorun feature"), "creation");
    }

    #[test]
    fn test_detect_modification() {
        assert_eq!(detect_prompt_type("update the config"), "modification");
        assert_eq!(
            detect_prompt_type("change the timeout value"),
            "modification"
        );
        assert_eq!(
            detect_prompt_type("modify the struct fields"),
            "modification"
        );
    }

    #[test]
    fn test_detect_review() {
        assert_eq!(detect_prompt_type("review the code"), "review");
        assert_eq!(detect_prompt_type("check the coverage"), "review");
        assert_eq!(detect_prompt_type("verify the output"), "review");
    }

    #[test]
    fn test_detect_navigation() {
        assert_eq!(detect_prompt_type("find the session module"), "navigation");
        assert_eq!(detect_prompt_type("search for the function"), "navigation");
        assert_eq!(detect_prompt_type("where is the config file"), "navigation");
    }

    #[test]
    fn test_detect_general() {
        assert_eq!(detect_prompt_type("hello world"), "general");
        assert_eq!(detect_prompt_type("thanks"), "general");
    }

    #[test]
    fn test_prompt_hash_deterministic() {
        let text = "test prompt";
        let h1 = Sha256::digest(text.as_bytes());
        let h2 = Sha256::digest(text.as_bytes());
        assert_eq!(format!("{h1:x}"), format!("{h2:x}"));
    }

    #[test]
    fn test_prompt_logging_handler_metadata() {
        let handler = PromptLogging::new(PathBuf::from("/tmp"));
        assert_eq!(handler.name(), "user-prompt-logging");
        assert_eq!(handler.events(), &[HookEvent::UserPromptSubmit]);
    }

    #[test]
    fn test_prompt_logging_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"user_prompt": {"enabled": false}}}"#,
        )
        .unwrap();

        let handler = PromptLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"user_prompt": "hello"})),
            event: HookEvent::UserPromptSubmit,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
        assert!(!dir.path().join(".state/logs/sessions").exists());
    }

    #[test]
    fn test_prompt_logging_writes_record() {
        let dir = tempfile::tempdir().unwrap();
        let handler = PromptLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"user_prompt": "/cf-develop"})),
            event: HookEvent::UserPromptSubmit,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let log_dir = dir.path().join(".state/logs/sessions");
        assert!(log_dir.exists());
        let entries: Vec<_> = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("prompts-") && name.ends_with(".jsonl")
            })
            .collect();
        assert!(!entries.is_empty());

        // Verify content.
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("prompt_submitted"));
        assert!(content.contains("command")); // /cf-develop is a command
    }

    #[test]
    fn test_prompt_logging_privacy_mode() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"user_prompt": {"enabled": true, "privacy_mode": true}}}"#,
        )
        .unwrap();

        let handler = PromptLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"user_prompt": "secret stuff"})),
            event: HookEvent::UserPromptSubmit,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        handler.handle(input).unwrap();

        let log_dir = dir.path().join(".state/logs/sessions");
        let entries: Vec<_> = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("prompts-") && name.ends_with(".jsonl")
            })
            .collect();
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(
            !content.contains("secret stuff"),
            "privacy mode should omit prompt text"
        );
        assert!(content.contains("prompt_hash")); // hash still present
    }
}
