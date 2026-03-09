//! Active task management (`active-task.json`) for session runtime state.
//!
//! The active task file is a bridge between the session runtime and hook
//! context. It stores the currently active task details at
//! `.state/runtime/active-task.json`.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::SessionError;
use crate::types::{EpicId, FormatId, SessionId, TaskId};

/// Default filename for the active task file.
const ACTIVE_TASK_FILENAME: &str = "active-task.json";

/// Runtime active task state, persisted as JSON at `.state/runtime/active-task.json`.
///
/// Mirrors the Go `ActiveTask` struct from `internal/workstate/activetask.go`.
/// All fields except `task_id` are optional.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveTask {
    pub task_id: TaskId,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub epic_id: Option<EpicId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_format_id: Option<FormatId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub epic_format_id: Option<FormatId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_stage: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
}

/// Read the active task from `runtime_dir/active-task.json`.
///
/// Returns `None` if the file does not exist.
///
/// # Errors
///
/// Returns `SessionError::Io` on read failure, `SessionError::Serialization`
/// on JSON parse failure.
pub fn get_active_task(runtime_dir: &Path) -> Result<Option<ActiveTask>, SessionError> {
    let path = runtime_dir.join(ACTIVE_TASK_FILENAME);
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    let task: ActiveTask = serde_json::from_str(&content)?;
    Ok(Some(task))
}

/// Write the active task to `runtime_dir/active-task.json` atomically.
///
/// Creates the directory if it does not exist.
///
/// # Errors
///
/// Returns `SessionError::Io` on write failure, `SessionError::Serialization`
/// on JSON serialization failure.
pub fn set_active_task(runtime_dir: &Path, task: &ActiveTask) -> Result<(), SessionError> {
    fs::create_dir_all(runtime_dir)?;

    let target = runtime_dir.join(ACTIVE_TASK_FILENAME);
    let tmp_path = runtime_dir.join(format!(".{ACTIVE_TASK_FILENAME}.tmp"));

    let json = serde_json::to_string_pretty(task)?;

    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }

    fs::rename(&tmp_path, &target)?;
    Ok(())
}

/// Remove the active task file. Idempotent: returns `Ok(())` if file does not exist.
///
/// # Errors
///
/// Returns `SessionError::Io` on removal failure (other than not-found).
pub fn clear_active_task(runtime_dir: &Path) -> Result<(), SessionError> {
    let path = runtime_dir.join(ACTIVE_TASK_FILENAME);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SessionError::Io(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_task() -> ActiveTask {
        ActiveTask {
            task_id: TaskId::new_unchecked("task-01abc"),
            epic_id: Some(EpicId::new_unchecked("epic-01xyz")),
            task_format_id: Some(FormatId::new_unchecked("INF-TSK-022-011")),
            epic_format_id: Some(FormatId::new_unchecked("INF-EPC-022")),
            title: Some("Test task".into()),
            status: Some("in_progress".into()),
            branch: Some("feat/test".into()),
            session_id: Some(SessionId::new_unchecked("ses-01test")),
            created_at: Some("2026-03-09T00:00:00Z".into()),
            updated_at: Some("2026-03-09T00:00:00Z".into()),
            current_stage: Some("WS-DEV".into()),
            team_name: Some("codeflow-team".into()),
        }
    }

    #[test]
    fn test_set_and_get_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let task = make_test_task();

        set_active_task(dir.path(), &task).unwrap();
        let loaded = get_active_task(dir.path()).unwrap().unwrap();

        assert_eq!(loaded, task);
    }

    #[test]
    fn test_get_active_task_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = get_active_task(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_clear_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let task = make_test_task();

        set_active_task(dir.path(), &task).unwrap();
        assert!(dir.path().join(ACTIVE_TASK_FILENAME).exists());

        clear_active_task(dir.path()).unwrap();
        assert!(!dir.path().join(ACTIVE_TASK_FILENAME).exists());
    }

    #[test]
    fn test_clear_active_task_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        // Should not fail when file does not exist.
        clear_active_task(dir.path()).unwrap();
    }

    #[test]
    fn test_set_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("deep").join("runtime");
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-nested"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };

        set_active_task(&nested, &task).unwrap();
        assert!(nested.join(ACTIVE_TASK_FILENAME).exists());
    }

    #[test]
    fn test_set_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let task1 = ActiveTask {
            task_id: TaskId::new_unchecked("task-first"),
            ..make_test_task()
        };
        let task2 = ActiveTask {
            task_id: TaskId::new_unchecked("task-second"),
            ..make_test_task()
        };

        set_active_task(dir.path(), &task1).unwrap();
        set_active_task(dir.path(), &task2).unwrap();

        let loaded = get_active_task(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.task_id.as_str(), "task-second");
    }

    #[test]
    fn test_malformed_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(ACTIVE_TASK_FILENAME);
        fs::write(&path, "{ invalid json }").unwrap();

        let result = get_active_task(dir.path());
        assert!(result.is_err());
        // Should be a serialization error
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("serialization")
                || err.to_string().contains("expected")
                || err.to_string().contains("key must be"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_serde_roundtrip_with_optional_fields_none() {
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-minimal"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: None,
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
        };

        let json = serde_json::to_string(&task).unwrap();
        // None fields should be omitted
        assert!(!json.contains("epic_id"));
        assert!(!json.contains("title"));

        let parsed: ActiveTask = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, task);
    }
}
