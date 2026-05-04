//! Active task management (`active-task.json`) for session runtime state.
//!
//! The active task file is a bridge between the session runtime and hook
//! context. It stores the currently active task details at
//! `.state/runtime/active-task.json`.
//!
//! Worktree-aware wrappers (`get_active_task_worktree_aware`,
//! `set_active_task_worktree_aware`, `clear_active_task_worktree_aware`)
//! resolve the runtime directory from `CODEFLOW_WORKTREE_PATH` when set,
//! falling back to the project-level `.state/runtime/` directory.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::SessionError;
use crate::types::{EpicId, FormatId, SessionId, TaskId};
use crate::worktree::WorktreePaths;

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

    /// Work type classification (e.g., "FEAT", "FIX", "PLAN").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<String>,

    /// Scope enforcement policy: "soft", "hard", or "permissive".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope_policy: Option<String>,

    /// List of file paths this task is allowed to edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_scope: Option<Vec<String>>,

    /// Target branch for PRs (e.g., "main" or "autorun/batch-xxx").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_branch: Option<String>,

    /// Whether auto_merge is enabled for this task's PR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_merge: Option<bool>,

    /// Who performs epic status updates: "orchestrator" or "none".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epic_update: Option<String>,
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

// ---------------------------------------------------------------------------
// Worktree-aware wrappers
// ---------------------------------------------------------------------------

/// Resolve the runtime directory, preferring worktree-local when available.
fn resolve_runtime_dir(project_dir: &Path, worktree_path: Option<&str>) -> PathBuf {
    if let Some(wt_path) = worktree_path {
        let wp = WorktreePaths::new(wt_path);
        return wp.runtime_dir();
    }
    project_dir.join(".state").join("runtime")
}

/// Read active task, resolving worktree path from env var.
///
/// Checks `CODEFLOW_WORKTREE_PATH` first. If set, reads from
/// `{worktree}/.state/runtime/active-task.json`. If that file is missing,
/// falls back to the project-level `.state/runtime/`.
///
/// # Errors
///
/// Returns `SessionError::Io` on read failure, `SessionError::Serialization`
/// on JSON parse failure.
pub fn get_active_task_worktree_aware(
    project_dir: &Path,
) -> Result<Option<ActiveTask>, SessionError> {
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
    get_active_task_resolved(project_dir, worktree_path.as_deref())
}

/// Inner: testable with explicit worktree path.
///
/// Public so integration tests in `codeflow-cli/cli/tests/` can drive the
/// resolver with explicit worktree paths.
///
/// # Errors
///
/// Returns `SessionError::Io` on read failure, `SessionError::Serialization`
/// on JSON parse failure.
pub fn get_active_task_resolved(
    project_dir: &Path,
    worktree_path: Option<&str>,
) -> Result<Option<ActiveTask>, SessionError> {
    // Check worktree-local active task first.
    if let Some(wt_path) = worktree_path {
        let wp = WorktreePaths::new(wt_path);
        let result = get_active_task(&wp.runtime_dir());
        if let Ok(Some(_)) = &result {
            return result;
        }
    }
    // Fallback: main project runtime dir.
    let runtime_dir = project_dir.join(".state").join("runtime");
    get_active_task(&runtime_dir)
}

/// Write active task, resolving worktree path from env var.
///
/// When `CODEFLOW_WORKTREE_PATH` is set, writes to the worktree-local
/// runtime directory. Otherwise writes to the project-level directory.
///
/// # Errors
///
/// Returns `SessionError::Io` on write failure, `SessionError::Serialization`
/// on JSON serialization failure.
pub fn set_active_task_worktree_aware(
    project_dir: &Path,
    task: &ActiveTask,
) -> Result<(), SessionError> {
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
    set_active_task_resolved(project_dir, task, worktree_path.as_deref())
}

/// Inner: testable with explicit worktree path.
///
/// Public so integration tests in `codeflow-cli/cli/tests/` can drive the
/// resolver with explicit worktree paths.
///
/// # Errors
///
/// Returns `SessionError::Io` on write failure, `SessionError::Serialization`
/// on JSON serialization failure.
pub fn set_active_task_resolved(
    project_dir: &Path,
    task: &ActiveTask,
    worktree_path: Option<&str>,
) -> Result<(), SessionError> {
    let runtime_dir = resolve_runtime_dir(project_dir, worktree_path);
    set_active_task(&runtime_dir, task)
}

/// Clear active task, resolving worktree path from env var.
///
/// When `CODEFLOW_WORKTREE_PATH` is set, clears from the worktree-local
/// runtime directory. Otherwise clears from the project-level directory.
///
/// # Errors
///
/// Returns `SessionError::Io` on removal failure (other than not-found).
pub fn clear_active_task_worktree_aware(project_dir: &Path) -> Result<(), SessionError> {
    let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
    clear_active_task_resolved(project_dir, worktree_path.as_deref())
}

/// Inner: testable with explicit worktree path.
///
/// Public so integration tests in `codeflow-cli/cli/tests/` can drive the
/// resolver with explicit worktree paths.
///
/// # Errors
///
/// Returns `SessionError::Io` on removal failure (other than not-found).
pub fn clear_active_task_resolved(
    project_dir: &Path,
    worktree_path: Option<&str>,
) -> Result<(), SessionError> {
    let runtime_dir = resolve_runtime_dir(project_dir, worktree_path);
    clear_active_task(&runtime_dir)
}

/// Read the active task file path, resolving worktree when available.
///
/// Used by logging hooks that read the file directly (not via
/// `get_active_task`). Returns the path to `active-task.json` in the
/// worktree-local runtime directory when the worktree env var is set,
/// falling back to the project-level path if the worktree file is missing.
#[must_use]
pub fn active_task_path_resolved(project_dir: &Path, worktree_path: Option<&str>) -> PathBuf {
    if let Some(wt_path) = worktree_path {
        let wp = WorktreePaths::new(wt_path);
        let wt_file = wp.runtime_dir().join(ACTIVE_TASK_FILENAME);
        if wt_file.exists() {
            return wt_file;
        }
    }
    project_dir
        .join(".state")
        .join("runtime")
        .join(ACTIVE_TASK_FILENAME)
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
            work_type: None,
            scope_policy: None,
            file_scope: None,
            target_branch: None,
            auto_merge: None,
            epic_update: None,
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
            work_type: None,
            scope_policy: None,
            file_scope: None,
            target_branch: None,
            auto_merge: None,
            epic_update: None,
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
            work_type: None,
            scope_policy: None,
            file_scope: None,
            target_branch: None,
            auto_merge: None,
            epic_update: None,
        };

        let json = serde_json::to_string(&task).unwrap();
        // None fields should be omitted
        assert!(!json.contains("epic_id"));
        assert!(!json.contains("title"));
        assert!(!json.contains("work_type"));

        let parsed: ActiveTask = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, task);
    }

    #[test]
    fn test_work_type_field_serialization() {
        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-wt"),
            work_type: Some("FEAT".into()),
            ..make_test_task()
        };

        let json = serde_json::to_string(&task).unwrap();
        assert!(json.contains("\"work_type\":\"FEAT\""));

        let parsed: ActiveTask = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.work_type.as_deref(), Some("FEAT"));
    }

    #[test]
    fn test_work_type_field_deserialization_missing() {
        // JSON without work_type field should deserialize with work_type=None.
        let json = r#"{"task_id":"task-old"}"#;
        let task: ActiveTask = serde_json::from_str(json).unwrap();
        assert!(task.work_type.is_none());
    }

    // --- Worktree-aware wrapper tests ---

    #[test]
    fn test_get_active_task_prefers_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let project_task = ActiveTask {
            task_id: TaskId::new_unchecked("task-project"),
            ..make_test_task()
        };
        let worktree_task = ActiveTask {
            task_id: TaskId::new_unchecked("task-worktree"),
            ..make_test_task()
        };

        // Write to both locations.
        let project_runtime = project_dir.path().join(".state").join("runtime");
        set_active_task(&project_runtime, &project_task).unwrap();

        let wp = WorktreePaths::new(worktree_dir.path());
        set_active_task(&wp.runtime_dir(), &worktree_task).unwrap();

        // With worktree path, should prefer worktree task.
        let result = get_active_task_resolved(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            result.task_id.as_str(),
            "task-worktree",
            "should read from worktree"
        );
    }

    #[test]
    fn test_get_active_task_falls_back_without_worktree() {
        let project_dir = tempfile::tempdir().unwrap();

        let project_task = ActiveTask {
            task_id: TaskId::new_unchecked("task-fallback"),
            ..make_test_task()
        };

        let project_runtime = project_dir.path().join(".state").join("runtime");
        set_active_task(&project_runtime, &project_task).unwrap();

        // No worktree -- falls back to project dir.
        let result = get_active_task_resolved(project_dir.path(), None)
            .unwrap()
            .unwrap();
        assert_eq!(
            result.task_id.as_str(),
            "task-fallback",
            "should fall back to project"
        );
    }

    #[test]
    fn test_get_active_task_worktree_missing_file_falls_back() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let project_task = ActiveTask {
            task_id: TaskId::new_unchecked("task-project-only"),
            ..make_test_task()
        };

        // Write only to project dir (NOT worktree).
        let project_runtime = project_dir.path().join(".state").join("runtime");
        set_active_task(&project_runtime, &project_task).unwrap();

        // Worktree dir exists but has no active-task.json -- should fall back.
        let result = get_active_task_resolved(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            result.task_id.as_str(),
            "task-project-only",
            "should fall back when worktree file missing"
        );
    }

    #[test]
    fn test_set_active_task_writes_to_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-wt-write"),
            ..make_test_task()
        };

        // Set with worktree path -- should write to worktree.
        set_active_task_resolved(
            project_dir.path(),
            &task,
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();

        // Verify written to worktree runtime dir.
        let wp = WorktreePaths::new(worktree_dir.path());
        let loaded = get_active_task(&wp.runtime_dir()).unwrap().unwrap();
        assert_eq!(loaded.task_id.as_str(), "task-wt-write");

        // Verify NOT written to project dir.
        let project_runtime = project_dir.path().join(".state").join("runtime");
        let project_result = get_active_task(&project_runtime).unwrap();
        assert!(
            project_result.is_none(),
            "should not write to project dir when worktree is active"
        );
    }

    #[test]
    fn test_set_active_task_falls_back_without_worktree() {
        let project_dir = tempfile::tempdir().unwrap();

        let task = ActiveTask {
            task_id: TaskId::new_unchecked("task-proj-write"),
            ..make_test_task()
        };

        // Set without worktree -- should write to project dir.
        set_active_task_resolved(project_dir.path(), &task, None).unwrap();

        let project_runtime = project_dir.path().join(".state").join("runtime");
        let loaded = get_active_task(&project_runtime).unwrap().unwrap();
        assert_eq!(loaded.task_id.as_str(), "task-proj-write");
    }

    #[test]
    fn test_clear_active_task_clears_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        let task = make_test_task();

        // Write to worktree.
        let wp = WorktreePaths::new(worktree_dir.path());
        set_active_task(&wp.runtime_dir(), &task).unwrap();
        assert!(wp.active_task().exists());

        // Clear with worktree path.
        clear_active_task_resolved(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();
        assert!(
            !wp.active_task().exists(),
            "should clear worktree active task"
        );
    }

    #[test]
    fn test_clear_active_task_falls_back_without_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let task = make_test_task();

        let project_runtime = project_dir.path().join(".state").join("runtime");
        set_active_task(&project_runtime, &task).unwrap();
        assert!(project_runtime.join(ACTIVE_TASK_FILENAME).exists());

        // Clear without worktree -- should clear project dir.
        clear_active_task_resolved(project_dir.path(), None).unwrap();
        assert!(!project_runtime.join(ACTIVE_TASK_FILENAME).exists());
    }

    #[test]
    fn test_two_worktrees_independent_active_tasks() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_a = tempfile::tempdir().unwrap();
        let worktree_b = tempfile::tempdir().unwrap();

        let task_a = ActiveTask {
            task_id: TaskId::new_unchecked("task-session-a"),
            title: Some("Session A work".into()),
            ..make_test_task()
        };
        let task_b = ActiveTask {
            task_id: TaskId::new_unchecked("task-session-b"),
            title: Some("Session B work".into()),
            ..make_test_task()
        };

        // Write to each worktree independently.
        set_active_task_resolved(
            project_dir.path(),
            &task_a,
            Some(worktree_a.path().to_str().unwrap()),
        )
        .unwrap();
        set_active_task_resolved(
            project_dir.path(),
            &task_b,
            Some(worktree_b.path().to_str().unwrap()),
        )
        .unwrap();

        // Read from each -- should see their own task.
        let read_a = get_active_task_resolved(
            project_dir.path(),
            Some(worktree_a.path().to_str().unwrap()),
        )
        .unwrap()
        .unwrap();
        let read_b = get_active_task_resolved(
            project_dir.path(),
            Some(worktree_b.path().to_str().unwrap()),
        )
        .unwrap()
        .unwrap();

        assert_eq!(read_a.task_id.as_str(), "task-session-a");
        assert_eq!(read_b.task_id.as_str(), "task-session-b");
        assert_ne!(
            read_a.task_id, read_b.task_id,
            "two worktrees should have independent tasks"
        );
    }

    #[test]
    fn test_active_task_path_resolved_prefers_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        // Write to worktree so the file exists.
        let wp = WorktreePaths::new(worktree_dir.path());
        set_active_task(&wp.runtime_dir(), &make_test_task()).unwrap();

        let path = active_task_path_resolved(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        );
        assert!(
            path.starts_with(worktree_dir.path()),
            "should resolve to worktree path: {path:?}"
        );
    }

    #[test]
    fn test_active_task_path_resolved_falls_back() {
        let project_dir = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();

        // No file in worktree -- should fall back to project.
        let path = active_task_path_resolved(
            project_dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
        );
        assert!(
            path.starts_with(project_dir.path()),
            "should fall back to project path: {path:?}"
        );
    }

    #[test]
    fn test_active_task_path_resolved_no_worktree() {
        let project_dir = tempfile::tempdir().unwrap();
        let path = active_task_path_resolved(project_dir.path(), None);
        let expected = project_dir
            .path()
            .join(".state")
            .join("runtime")
            .join(ACTIVE_TASK_FILENAME);
        assert_eq!(path, expected);
    }
}
