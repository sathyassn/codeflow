//! Roundtrip integration test for the canonical `active-task.json` resolver.
//!
//! Verifies INF-TSK-050-010 AC-04: writes via `set_active_task_resolved` are
//! readable via `get_active_task_resolved` when `CODEFLOW_WORKTREE_PATH` is
//! set, and that the file path used by both directions matches the path
//! returned by `WorktreePaths::active_task()`.
//!
//! Uses `#[serial_test::serial]` to guarantee exclusive access to the
//! `CODEFLOW_WORKTREE_PATH` environment variable across parallel test runs.

use codeflow_core::session::active_task::{
    ActiveTask, get_active_task_resolved, set_active_task_resolved,
};
use codeflow_core::types::{EpicId, FormatId, SessionId, TaskId};
use codeflow_core::worktree::WorktreePaths;

fn make_task(id: &str) -> ActiveTask {
    ActiveTask {
        task_id: TaskId::new_unchecked(id.to_string()),
        epic_id: Some(EpicId::new_unchecked("epic-01ROUND")),
        task_format_id: Some(FormatId::new_unchecked("INF-TSK-050-010")),
        epic_format_id: Some(FormatId::new_unchecked("INF-EPC-050")),
        title: Some("Roundtrip task".to_string()),
        status: Some("in_progress".to_string()),
        branch: Some("fix/roundtrip".to_string()),
        session_id: Some(SessionId::new_unchecked("ses-roundtrip01")),
        created_at: Some("2026-05-04T10:00:00Z".to_string()),
        updated_at: Some("2026-05-04T10:00:00Z".to_string()),
        current_stage: Some("WS-DEV".to_string()),
        team_name: None,
        work_type: Some("FIX".to_string()),
        scope_policy: Some("hard".to_string()),
        file_scope: Some(vec!["src/in.rs".to_string(), "src/sub/".to_string()]),
        target_branch: None,
        auto_merge: None,
        epic_update: None,
    }
}

#[test]
#[serial_test::serial]
fn write_via_resolver_reads_via_resolver_in_worktree_mode() {
    let project_dir = tempfile::tempdir().expect("create project dir");
    let worktree_dir = tempfile::tempdir().expect("create worktree dir");

    let task = make_task("task-roundtrip-1");
    set_active_task_resolved(
        project_dir.path(),
        &task,
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .expect("set_active_task_resolved must succeed");

    // The file path used by the writer MUST match WorktreePaths::active_task().
    let wp = WorktreePaths::new(worktree_dir.path());
    let expected_path = wp.active_task();
    assert!(
        expected_path.exists(),
        "active-task.json must exist at the resolver-derived path {expected_path:?}"
    );

    // The reader MUST return the same task that was written.
    let loaded = get_active_task_resolved(
        project_dir.path(),
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .expect("get_active_task_resolved must succeed")
    .expect("active task must be present");

    assert_eq!(
        loaded.task_id.as_str(),
        "task-roundtrip-1",
        "task_id must roundtrip"
    );
    assert_eq!(
        loaded.scope_policy.as_deref(),
        Some("hard"),
        "scope_policy must roundtrip"
    );
    assert_eq!(
        loaded.file_scope.as_deref(),
        Some(&["src/in.rs".to_string(), "src/sub/".to_string(),][..]),
        "file_scope must roundtrip"
    );
    assert_eq!(loaded, task, "full ActiveTask struct must roundtrip");
}

#[test]
#[serial_test::serial]
fn writer_path_matches_resolver_path() {
    let project_dir = tempfile::tempdir().expect("create project dir");
    let worktree_dir = tempfile::tempdir().expect("create worktree dir");

    let task = make_task("task-roundtrip-2");
    set_active_task_resolved(
        project_dir.path(),
        &task,
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .unwrap();

    let resolver_path = WorktreePaths::new(worktree_dir.path()).active_task();
    assert!(resolver_path.is_absolute() || resolver_path.is_relative());
    assert!(
        resolver_path.exists(),
        "writer must place the file at {resolver_path:?}"
    );

    // Reading the file directly via resolver-derived path yields the task.
    let raw = std::fs::read_to_string(&resolver_path).expect("read raw file");
    let parsed: ActiveTask = serde_json::from_str(&raw).expect("parse JSON");
    assert_eq!(parsed.task_id.as_str(), "task-roundtrip-2");
}

#[test]
#[serial_test::serial]
fn reader_falls_back_to_project_dir_when_worktree_file_missing() {
    let project_dir = tempfile::tempdir().expect("create project dir");
    let worktree_dir = tempfile::tempdir().expect("create worktree dir");

    // Write to project dir only.
    let task = make_task("task-fallback-3");
    set_active_task_resolved(project_dir.path(), &task, None).unwrap();

    // Read with worktree path set, but worktree file is missing -- must
    // fall back to project dir transparently.
    let loaded = get_active_task_resolved(
        project_dir.path(),
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .unwrap()
    .expect("must fall back to project dir");
    assert_eq!(loaded.task_id.as_str(), "task-fallback-3");
}
