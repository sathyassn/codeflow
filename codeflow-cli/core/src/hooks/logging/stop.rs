//! Stop event logging handler with task context capture.
//!
//! Handles both `Stop` and `SubagentStop` events (both map to `HookEvent::Stop`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput};

use super::writer::ActivityWriter;
use super::{git_branch, read_config, resolve_session_id};

/// Logs `Stop` events to `session-{YYYY-MM-DD}.jsonl` with optional
/// task context and git state.
pub struct StopLogging {
    pub project_dir: PathBuf,
}

impl StopLogging {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for StopLogging {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let cfg = read_config(&self.project_dir);
        if !cfg.stop.enabled {
            return Ok(HookOutput::Allow);
        }

        let session_id = resolve_session_id(&self.project_dir);

        let stop_reason = input
            .tool_input
            .as_ref()
            .and_then(|v| v.get("stop_reason"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let writer = match ActivityWriter::new(&self.project_dir, &cfg.stop.log_directory) {
            Ok(w) => w,
            Err(e) => {
                return Ok(HookOutput::Warn {
                    message: format!("stop logging init failed: {e}"),
                });
            }
        };

        let mut record = HashMap::new();
        record.insert("ts".into(), serde_json::json!(writer.timestamp()));
        record.insert("session_id".into(), serde_json::json!(session_id));
        record.insert("event".into(), serde_json::json!("stop"));
        record.insert("stop_reason".into(), serde_json::json!(stop_reason));

        if cfg.stop.capture_task_context {
            if let Some(task_ctx) = read_task_context(&self.project_dir) {
                record.insert("task_context".into(), serde_json::json!(task_ctx));
            }
        }

        if let Some(branch) = git_branch(&self.project_dir) {
            record.insert("git_branch".into(), serde_json::json!(branch));
        }
        record.insert(
            "has_uncommitted_changes".into(),
            serde_json::json!(has_uncommitted_changes(&self.project_dir)),
        );

        if let Err(e) = writer.append("session", &record) {
            return Ok(HookOutput::Warn {
                message: format!("stop logging write failed: {e}"),
            });
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "stop-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::Stop]
    }
}

/// Read task context from `active-task.json` (worktree-aware).
fn read_task_context(project_dir: &Path) -> Option<HashMap<String, String>> {
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
    let path = crate::session::active_task_path_resolved(project_dir, worktree_path.as_deref());
    let data = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    let task_id = v.get("task_id")?.as_str()?;
    if task_id.is_empty() {
        return None;
    }
    let status = v
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut ctx = HashMap::new();
    ctx.insert("task_id".into(), task_id.to_string());
    ctx.insert("status".into(), status);
    Some(ctx)
}

/// Check if there are uncommitted changes via `git status --porcelain`.
fn has_uncommitted_changes(project_dir: &Path) -> bool {
    let dir_str = project_dir.display().to_string();
    let output = std::process::Command::new("git")
        .args(["-C", &dir_str, "status", "--porcelain"])
        .output();
    match output {
        Ok(o) if o.status.success() => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stop_logging_handler_metadata() {
        let handler = StopLogging::new(PathBuf::from("/tmp"));
        assert_eq!(handler.name(), "stop-logging");
        assert_eq!(handler.events(), &[HookEvent::Stop]);
    }

    #[test]
    fn test_stop_logging_writes_record() {
        let dir = tempfile::tempdir().unwrap();
        let handler = StopLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"stop_reason": "end_turn"})),
            event: HookEvent::Stop,
            session_id: Some("ses-test".into()),
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
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
                name.starts_with("session-") && name.ends_with(".jsonl")
            })
            .collect();
        assert!(!entries.is_empty());
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("stop"));
        assert!(content.contains("end_turn"));
    }

    #[test]
    fn test_stop_logging_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"stop": {"enabled": false}}}"#,
        )
        .unwrap();

        let handler = StopLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"stop_reason": "end_turn"})),
            event: HookEvent::Stop,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));
        assert!(!dir.path().join(".state/logs/sessions").exists());
    }

    #[test]
    fn test_stop_logging_default_reason() {
        let dir = tempfile::tempdir().unwrap();
        let handler = StopLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::Stop,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input).unwrap();
        assert!(matches!(result, HookOutput::Allow));

        let log_dir = dir.path().join(".state/logs/sessions");
        let entries: Vec<_> = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("session-") && name.ends_with(".jsonl")
            })
            .collect();
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("unknown")); // default stop reason
    }

    #[test]
    fn test_read_task_context_valid() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id": "INF-TSK-022-017", "status": "in_progress"}"#,
        )
        .unwrap();
        let ctx = read_task_context(&dir.path().to_path_buf());
        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx["task_id"], "INF-TSK-022-017");
        assert_eq!(ctx["status"], "in_progress");
    }

    #[test]
    fn test_read_task_context_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_task_context(&dir.path().to_path_buf()).is_none());
    }

    #[test]
    fn test_read_task_context_empty_task_id() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id": "", "status": "in_progress"}"#,
        )
        .unwrap();
        assert!(read_task_context(&dir.path().to_path_buf()).is_none());
    }

    #[test]
    fn test_stop_logging_with_task_context() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id": "TEST-001", "status": "in_progress"}"#,
        )
        .unwrap();

        let handler = StopLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: Some(serde_json::json!({"stop_reason": "end_turn"})),
            event: HookEvent::Stop,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        handler.handle(input).unwrap();

        let log_dir = dir.path().join(".state/logs/sessions");
        let entries: Vec<_> = std::fs::read_dir(&log_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("session-") && name.ends_with(".jsonl")
            })
            .collect();
        let content = std::fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("task_context"));
        assert!(content.contains("TEST-001"));
    }
}
