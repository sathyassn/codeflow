//! Session start and end logging handlers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput};

use super::writer::ActivityWriter;
use super::{git_branch, read_config, resolve_session_id};

// ---------------------------------------------------------------------------
// SessionStartLogging
// ---------------------------------------------------------------------------

/// Logs `SessionStart` events to `session-{YYYY-MM-DD}.jsonl` with
/// optional git and task metadata.
pub struct SessionStartLogging {
    pub project_dir: PathBuf,
}

impl SessionStartLogging {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for SessionStartLogging {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let cfg = read_config(&self.project_dir);
        if !cfg.session_start.enabled {
            return Ok(HookOutput::Allow);
        }

        let session_id = resolve_session_id(&self.project_dir);

        let writer = match ActivityWriter::new(&self.project_dir, &cfg.session_start.log_directory)
        {
            Ok(w) => w,
            Err(e) => {
                return Ok(HookOutput::Warn {
                    message: format!("session-start logging init failed: {e}"),
                });
            }
        };

        let mut record = HashMap::new();
        record.insert("ts".into(), serde_json::json!(writer.timestamp()));
        record.insert("session_id".into(), serde_json::json!(session_id));
        record.insert("event".into(), serde_json::json!("session_start"));

        if cfg.session_start.capture_metadata {
            let approval_mode = input
                .tool_input
                .as_ref()
                .and_then(|v| v.get("permission_mode"))
                .and_then(|v| v.as_str())
                .unwrap_or("standard");

            let metadata = gather_session_metadata(&self.project_dir, approval_mode);
            record.insert("metadata".into(), serde_json::json!(metadata));
        }

        if let Err(e) = writer.append("session", &record) {
            return Ok(HookOutput::Warn {
                message: format!("session-start logging write failed: {e}"),
            });
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// SessionEndLogging
// ---------------------------------------------------------------------------

/// Logs `SessionEnd` events to `session-{YYYY-MM-DD}.jsonl` with
/// duration calculation.
pub struct SessionEndLogging {
    pub project_dir: PathBuf,
}

impl SessionEndLogging {
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl HookHandler for SessionEndLogging {
    fn handle(&self, _input: HookInput) -> Result<HookOutput, HookError> {
        let cfg = read_config(&self.project_dir);
        if !cfg.session_end.enabled {
            return Ok(HookOutput::Allow);
        }

        let session_id = resolve_session_id(&self.project_dir);

        let writer = match ActivityWriter::new(&self.project_dir, &cfg.session_end.log_directory) {
            Ok(w) => w,
            Err(e) => {
                return Ok(HookOutput::Warn {
                    message: format!("session-end logging init failed: {e}"),
                });
            }
        };

        let duration = calculate_duration(&writer, &session_id);

        let mut record = HashMap::new();
        record.insert("ts".into(), serde_json::json!(writer.timestamp()));
        record.insert("session_id".into(), serde_json::json!(session_id));
        record.insert("event".into(), serde_json::json!("session_end"));
        record.insert("duration_seconds".into(), serde_json::json!(duration));

        if let Err(e) = writer.append("session", &record) {
            return Ok(HookOutput::Warn {
                message: format!("session-end logging write failed: {e}"),
            });
        }

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-end-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionEnd]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Gather git and task metadata for session-start logs.
fn gather_session_metadata(
    project_dir: &Path,
    approval_mode: &str,
) -> HashMap<String, serde_json::Value> {
    let mut meta = HashMap::new();
    meta.insert(
        "cwd".into(),
        serde_json::json!(project_dir.display().to_string()),
    );

    if let Some(branch) = git_branch(project_dir) {
        meta.insert("git_branch".into(), serde_json::json!(branch));
    }
    if let Some(commit) = git_short_commit(project_dir) {
        meta.insert("git_commit".into(), serde_json::json!(commit));
    }

    meta.insert(
        "approval_mode".into(),
        serde_json::json!(if approval_mode.is_empty() {
            "standard"
        } else {
            approval_mode
        }),
    );

    if let Some(task_id) = read_active_task_id(project_dir) {
        meta.insert("active_task".into(), serde_json::json!(task_id));
    }

    meta
}

/// Get the short HEAD commit hash.
fn git_short_commit(project_dir: &Path) -> Option<String> {
    let dir_str = project_dir.display().to_string();
    let output = std::process::Command::new("git")
        .args(["-C", &dir_str, "rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let commit = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if commit.is_empty() {
        None
    } else {
        Some(commit)
    }
}

/// Read the active task ID from `active-task.json`.
fn read_active_task_id(project_dir: &Path) -> Option<String> {
    let path = project_dir
        .join(".state")
        .join("runtime")
        .join("active-task.json");
    let data = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    v.get("task_id")?
        .as_str()
        .map(std::string::ToString::to_string)
}

/// Calculate session duration from the session meta file.
/// Returns 0 if no metadata is available.
fn calculate_duration(writer: &ActivityWriter, session_id: &str) -> i64 {
    let meta_file = writer.dir().join(format!("session-{session_id}.meta"));
    let Ok(data) = std::fs::read_to_string(meta_file) else {
        return 0;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
        return 0;
    };
    let started_epoch = v
        .get("started_epoch")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    if started_epoch == 0 {
        return 0;
    }
    chrono::Utc::now().timestamp() - started_epoch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_start_logging_metadata() {
        let handler = SessionStartLogging::new(PathBuf::from("/tmp"));
        assert_eq!(handler.name(), "session-start-logging");
        assert_eq!(handler.events(), &[HookEvent::SessionStart]);
    }

    #[test]
    fn test_session_end_logging_metadata() {
        let handler = SessionEndLogging::new(PathBuf::from("/tmp"));
        assert_eq!(handler.name(), "session-end-logging");
        assert_eq!(handler.events(), &[HookEvent::SessionEnd]);
    }

    #[test]
    fn test_session_start_logging_writes_record() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SessionStartLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: Some("ses-test".into()),
            project_dir: Some(dir.path().to_string_lossy().to_string()),
            source: Some("startup".into()),
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
        assert!(content.contains("session_start"));
    }

    #[test]
    fn test_session_start_logging_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/enforcement");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("enforcement-policy.json"),
            r#"{"logging": {"session_start": {"enabled": false}}}"#,
        )
        .unwrap();

        let handler = SessionStartLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
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
    fn test_session_end_logging_writes_record() {
        let dir = tempfile::tempdir().unwrap();
        let handler = SessionEndLogging::new(dir.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
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
    }

    #[test]
    fn test_gather_session_metadata_basic() {
        let dir = tempfile::tempdir().unwrap();
        let meta = gather_session_metadata(&dir.path().to_path_buf(), "default");
        assert!(meta.contains_key("cwd"));
        assert_eq!(meta["approval_mode"], serde_json::json!("default"));
    }

    #[test]
    fn test_gather_session_metadata_empty_approval() {
        let dir = tempfile::tempdir().unwrap();
        let meta = gather_session_metadata(&dir.path().to_path_buf(), "");
        assert_eq!(meta["approval_mode"], serde_json::json!("standard"));
    }

    #[test]
    fn test_read_active_task_id_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_active_task_id(&dir.path().to_path_buf()).is_none());
    }

    #[test]
    fn test_read_active_task_id_valid() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(
            runtime_dir.join("active-task.json"),
            r#"{"task_id": "INF-TSK-022-017", "status": "in_progress"}"#,
        )
        .unwrap();
        let result = read_active_task_id(&dir.path().to_path_buf());
        assert_eq!(result.as_deref(), Some("INF-TSK-022-017"));
    }

    #[test]
    fn test_calculate_duration_no_meta() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::new(dir.path(), ".state/logs/sessions").unwrap();
        assert_eq!(calculate_duration(&writer, "ses-test"), 0);
    }

    #[test]
    fn test_calculate_duration_with_meta() {
        let dir = tempfile::tempdir().unwrap();
        let writer = ActivityWriter::new(dir.path(), ".state/logs/sessions").unwrap();
        let meta_path = writer.dir().join("session-ses-test.meta");
        let started = chrono::Utc::now().timestamp() - 120; // 2 minutes ago
        std::fs::write(meta_path, format!(r#"{{"started_epoch": {started}}}"#)).unwrap();
        let duration = calculate_duration(&writer, "ses-test");
        assert!(
            duration >= 119 && duration <= 121,
            "duration should be ~120s, got {duration}"
        );
    }
}
