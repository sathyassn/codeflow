//! `PostToolUse` hook handlers.
//!
//! Three handlers that run after tool invocations:
//! - `SentinelWrite`: Creates stage sentinels on `STAGE-COMPLETE` messages
//! - `CheckpointRegister`: Registers `PF{N}-TSK-{NN}` tasks in checkpoint
//! - `SettingsValidate`: Validates settings.json consistency
//!
//! Note: `PostToolUse` logging was moved to `hooks::logging::ToolUseLogging`
//! for full-featured tool-use logging with redaction, truncation, and config.

use std::path::PathBuf;
use std::sync::OnceLock;

use regex::Regex;

use super::{BlockCategory, HookEvent, HookHandler, HookInput, HookOutput};
use crate::error::HookError;
use crate::pathflow::{checkpoint::Checkpoint, sentinel};
use crate::session;

// ---------------------------------------------------------------------------
// Shared regex patterns (compiled once)
// ---------------------------------------------------------------------------

fn stage_complete_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"STAGE-COMPLETE:\s+WS-(DEV|REV|QA|TEST|PLAN|DOCS)").expect("valid regex")
    })
}

fn pf_task_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"PF\d+-TSK-\d+").expect("valid regex"))
}

/// Primary work stages that must complete before `WS-REV`.
const PRIMARY_STAGES: &[&str] = &["dev", "plan", "docs", "test"];

// ---------------------------------------------------------------------------
// SentinelWrite handler
// ---------------------------------------------------------------------------

/// Creates stage sentinels when `STAGE-COMPLETE: WS-{STAGE}` appears in
/// `SendMessage` content. Validates stage ordering (REV needs a primary
/// stage sentinel, QA needs `ws-dev` or `ws-test`).
pub struct SentinelWrite {
    /// Project root directory for resolving sentinel paths.
    pub project_dir: PathBuf,
}

impl SentinelWrite {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    fn sentinel_dir(&self) -> Result<PathBuf, HookError> {
        let sid = session::current_session_id(&self.project_dir.join(".state"))
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;
        sentinel::resolve_dir(&self.project_dir, sid.as_ref())
            .map_err(|e| HookError::Config(format!("sentinel dir: {e}")))
    }
}

impl HookHandler for SentinelWrite {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Only process SendMessage PostToolUse events.
        match &input.tool_name {
            Some(name) if name == "SendMessage" => {}
            _ => return Ok(HookOutput::Allow),
        }

        // Extract content from tool_input.
        let content = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if content.is_empty() {
            return Ok(HookOutput::Allow);
        }

        // Normalize: uppercase + collapse whitespace.
        let normalized = collapse_whitespace(&content.to_uppercase());

        let Some(captures) = stage_complete_re().captures(&normalized) else {
            return Ok(HookOutput::Allow);
        };

        let stage_lower = captures[1].to_lowercase();

        let sentinel_dir = self.sentinel_dir()?;

        // Stage ordering validation.
        match stage_lower.as_str() {
            "rev" => {
                let has_primary = PRIMARY_STAGES
                    .iter()
                    .any(|ps| sentinel::check_by_name(&sentinel_dir, &format!("ws-{ps}")));
                if !has_primary {
                    return Ok(HookOutput::Block {
                        reason:
                            "BLOCKED: ws-rev requires prior primary stage (ws-dev/ws-plan/ws-docs/ws-test)"
                                .into(),
                        category: Some(BlockCategory::Gate),
                    });
                }
            }
            "qa" => {
                if !sentinel::check_by_name(&sentinel_dir, "ws-dev")
                    && !sentinel::check_by_name(&sentinel_dir, "ws-test")
                {
                    return Ok(HookOutput::Block {
                        reason: "BLOCKED: ws-qa requires prior ws-dev or ws-test sentinel".into(),
                        category: Some(BlockCategory::Gate),
                    });
                }
            }
            _ => {}
        }

        // Create sentinel file.
        let sentinel_name = format!("ws-{stage_lower}");
        sentinel::create_by_name(&sentinel_dir, &sentinel_name)
            .map_err(|e| HookError::Config(format!("sentinel creation failed: {e}")))?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "sentinel-write"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

// ---------------------------------------------------------------------------
// CheckpointRegister handler
// ---------------------------------------------------------------------------

/// Registers `PF{N}-TSK-{NN}` tasks in the checkpoint file on `TaskCreate`
/// events. Blocks cross-phase registration when the previous phase sentinel
/// is missing.
pub struct CheckpointRegister {
    pub project_dir: PathBuf,
}

impl CheckpointRegister {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }

    fn checkpoint_path(&self, session_id: &str) -> PathBuf {
        self.project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-phase-tasks.json")
    }

    fn sentinel_dir(&self, session_id: &str) -> Result<PathBuf, HookError> {
        sentinel::resolve_dir(&self.project_dir, session_id)
            .map_err(|e| HookError::Config(format!("sentinel dir: {e}")))
    }
}

impl HookHandler for CheckpointRegister {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Only process TaskCreate PostToolUse events.
        match &input.tool_name {
            Some(name) if name == "TaskCreate" => {}
            _ => return Ok(HookOutput::Allow),
        }

        // Extract subject from tool_input.
        let subject = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("subject"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Extract PF{N}-TSK-{NN} from subject.
        let task_id = match pf_task_id_re().find(subject) {
            Some(m) => m.as_str().to_string(),
            None => return Ok(HookOutput::Allow),
        };

        // Resolve session ID.
        let sid = session::current_session_id(&self.project_dir.join(".state"))
            .map_err(|e| HookError::Config(format!("session ID: {e}")))?;

        let checkpoint_path = self.checkpoint_path(sid.as_ref());
        let sentinel_dir = self.sentinel_dir(sid.as_ref())?;

        let cp = Checkpoint::new();
        match cp.register_task(&checkpoint_path, &sentinel_dir, &task_id) {
            Ok(()) => Ok(HookOutput::Allow),
            Err(crate::error::PathflowError::CrossPhaseBlock(reason)) => Ok(HookOutput::Block {
                reason: format!("BLOCKED: {reason}"),
                category: Some(BlockCategory::Gate),
            }),
            Err(e) => {
                // Non-blocking: log warning but allow through.
                Ok(HookOutput::Warn {
                    message: format!("checkpoint register warning: {e}"),
                })
            }
        }
    }

    fn name(&self) -> &'static str {
        "checkpoint-register"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

// ---------------------------------------------------------------------------
// SettingsValidate handler
// ---------------------------------------------------------------------------

/// Validates `settings.json` consistency after Edit/Write operations that
/// target the settings file.
pub struct SettingsValidate {
    pub project_dir: PathBuf,
}

impl SettingsValidate {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for SettingsValidate {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Only process Edit/Write PostToolUse events that target settings.
        match &input.tool_name {
            Some(name) if name == "Edit" || name == "Write" => {}
            _ => return Ok(HookOutput::Allow),
        }

        // Check if the file path targets settings.json.
        let file_path = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if !file_path.contains("settings.json") {
            return Ok(HookOutput::Allow);
        }

        // Validate that the file is valid JSON.
        match std::fs::read_to_string(file_path) {
            Ok(data) => match serde_json::from_str::<serde_json::Value>(&data) {
                Ok(_) => Ok(HookOutput::Allow),
                Err(e) => Ok(HookOutput::Warn {
                    message: format!("settings.json has invalid JSON: {e}"),
                }),
            },
            Err(e) => Ok(HookOutput::Warn {
                message: format!("could not read settings.json for validation: {e}"),
            }),
        }
    }

    fn name(&self) -> &'static str {
        "settings-validate"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::PostToolUse]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(tool_name: &str, tool_input: serde_json::Value) -> HookInput {
        HookInput {
            tool_name: Some(tool_name.into()),
            tool_input: Some(tool_input),
            event: HookEvent::PostToolUse,
            session_id: Some("ses-test".into()),
            project_dir: Some("/tmp/test-project".into()),
            source: None,
            transcript_path: None,
        }
    }

    // -- SentinelWrite tests --

    #[test]
    fn test_sentinel_write_ignores_non_send_message() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("Bash", serde_json::json!({"command": "ls"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_ignores_empty_content() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input("SendMessage", serde_json::json!({"content": ""}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_ignores_non_stage_content() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "DEV-COMPLETE: all done"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_stage_complete_regex_matches() {
        let re = stage_complete_re();
        assert!(re.is_match("STAGE-COMPLETE: WS-DEV"));
        assert!(re.is_match("STAGE-COMPLETE:  WS-REV"));
        assert!(re.is_match("STAGE-COMPLETE: WS-QA"));
        assert!(re.is_match("STAGE-COMPLETE: WS-TEST"));
        assert!(re.is_match("STAGE-COMPLETE: WS-PLAN"));
        assert!(re.is_match("STAGE-COMPLETE: WS-DOCS"));
        assert!(!re.is_match("STAGE-COMPLETE: WS-UNKNOWN"));
    }

    #[test]
    fn test_pf_task_id_regex_matches() {
        let re = pf_task_id_re();
        assert!(re.is_match("PF1-TSK-01"));
        assert!(re.is_match("PF7-TSK-03"));
        assert!(re.is_match("Create PF4-TSK-05: register task"));
        assert!(!re.is_match("TSK-01"));
        assert!(!re.is_match("PF-TSK-01"));
    }

    #[test]
    fn test_collapse_whitespace() {
        assert_eq!(collapse_whitespace("hello  world"), "hello world");
        assert_eq!(
            collapse_whitespace("  STAGE-COMPLETE:   WS-DEV  "),
            "STAGE-COMPLETE: WS-DEV"
        );
    }

    // -- CheckpointRegister tests --

    #[test]
    fn test_checkpoint_register_ignores_non_task_create() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointRegister::new(dir.path().to_path_buf());
        let input = make_input("Bash", serde_json::json!({"command": "ls"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_checkpoint_register_ignores_non_pf_subject() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointRegister::new(dir.path().to_path_buf());
        let input = make_input(
            "TaskCreate",
            serde_json::json!({"subject": "Some regular task"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- SettingsValidate tests --

    #[test]
    fn test_settings_validate_ignores_non_edit_write() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input("Bash", serde_json::json!({"command": "ls"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_ignores_non_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Edit",
            serde_json::json!({"file_path": "/tmp/other-file.rs"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, r#"{"key": "value"}"#).unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Edit",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, "not valid json {{{").unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Edit",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_settings_validate_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Write",
            serde_json::json!({"file_path": "/tmp/nonexistent/settings.json"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    // -- SentinelWrite: stage ordering and sentinel creation --

    /// Helper to set up a tempdir with session env so sentinel_dir() works.
    fn setup_sentinel_env(dir: &std::path::Path) -> String {
        let sid = "ses-1234567890abc";
        let state_dir = dir.join(".state");
        std::fs::create_dir_all(state_dir.join("runtime")).unwrap();
        std::fs::write(
            state_dir.join("codeflow-env.sh"),
            format!("export CODEFLOW_SESSION_ID=\"{sid}\"\nexport CF_PROJECT_ROOT=\"test\"\n"),
        )
        .unwrap();
        sid.to_string()
    }

    #[test]
    fn test_sentinel_write_creates_dev_sentinel() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE: WS-DEV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        // Verify sentinel file was created.
        let sentinel_path = dir
            .path()
            .join(".state/sentinels/pathflow")
            .join(&sid)
            .join("pathflow-ws-dev");
        assert!(sentinel_path.exists(), "ws-dev sentinel should exist");
    }

    #[test]
    fn test_sentinel_write_rev_blocked_without_primary() {
        let dir = tempfile::tempdir().unwrap();
        let _sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE: WS-REV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(
            matches!(result, HookOutput::Block { .. }),
            "ws-rev should be blocked without a primary stage sentinel"
        );
    }

    #[test]
    fn test_sentinel_write_rev_allowed_after_dev() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        // Create ws-dev sentinel first.
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(&sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-ws-dev"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE: WS-REV"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_qa_blocked_without_dev_or_test() {
        let dir = tempfile::tempdir().unwrap();
        let _sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE: WS-QA"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Block { .. }));
    }

    #[test]
    fn test_sentinel_write_qa_allowed_after_dev() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(&sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-ws-dev"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE: WS-QA"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_qa_allowed_after_test() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());

        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(&sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-ws-test"), "").unwrap();

        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE: WS-QA"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_sentinel_write_normalizes_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        let sid = setup_sentinel_env(dir.path());
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = make_input(
            "SendMessage",
            serde_json::json!({"content": "STAGE-COMPLETE:   WS-PLAN"}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let sentinel_path = dir
            .path()
            .join(".state/sentinels/pathflow")
            .join(&sid)
            .join("pathflow-ws-plan");
        assert!(sentinel_path.exists());
    }

    #[test]
    fn test_sentinel_write_no_tool_input() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SentinelWrite::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: Some("SendMessage".into()),
            tool_input: None,
            event: HookEvent::PostToolUse,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- CheckpointRegister: with valid PF task subjects --

    #[test]
    fn test_checkpoint_register_no_subject_field() {
        let dir = tempfile::tempdir().unwrap();
        let handler = CheckpointRegister::new(dir.path().to_path_buf());
        let input = make_input("TaskCreate", serde_json::json!({"description": "stuff"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- SettingsValidate: Write tool variant --

    #[test]
    fn test_settings_validate_write_tool_valid() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, r#"{"hooks": []}"#).unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Write",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    #[test]
    fn test_settings_validate_write_tool_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::write(&settings_path, "{broken").unwrap();

        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input(
            "Write",
            serde_json::json!({"file_path": settings_path.to_str().unwrap()}),
        );
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Warn { .. }));
    }

    #[test]
    fn test_settings_validate_no_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SettingsValidate::new(dir.path().to_path_buf());
        let input = make_input("Edit", serde_json::json!({"content": "hello"}));
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
    }

    // -- Handler metadata tests --

    #[test]
    fn test_handler_names() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            SentinelWrite::new(dir.path().to_path_buf()).name(),
            "sentinel-write"
        );
        assert_eq!(
            CheckpointRegister::new(dir.path().to_path_buf()).name(),
            "checkpoint-register"
        );
        assert_eq!(
            SettingsValidate::new(dir.path().to_path_buf()).name(),
            "settings-validate"
        );
    }

    #[test]
    fn test_handler_events() {
        let dir = tempfile::tempdir().unwrap();
        let sw = SentinelWrite::new(dir.path().to_path_buf());
        assert_eq!(sw.events(), &[HookEvent::PostToolUse]);

        let cr = CheckpointRegister::new(dir.path().to_path_buf());
        assert_eq!(cr.events(), &[HookEvent::PostToolUse]);
    }
}
