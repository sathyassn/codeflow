//! E2E integration test for scope_policy=hard enforcement.
//!
//! Verifies INF-TSK-050-010 AC-09:
//! - Setup: write a hard-policy active-task.json with `file_scope=['src/in.rs']`
//! - Edit `src/out.rs` -> exit 2 (Block) + stderr scope message + ClaimConflict event
//! - Edit `src/in.rs`  -> exit 0 (Allow)                              + ClaimAcquired event
//!
//! Mutates the `CODEFLOW_WORKTREE_PATH` environment variable; serialised via
//! `#[serial_test::serial]`.

use std::path::Path;

use codeflow_core::hooks::pre_tool_use::GateCheck;
use codeflow_core::hooks::{HookEvent, HookHandler, HookInput, HookOutput};
use codeflow_core::pathflow::sentinel;
use codeflow_core::session::active_task::{ActiveTask, set_active_task_resolved};
use codeflow_core::types::{EpicId, FormatId, SessionId, TaskId};

fn make_hard_active_task(file_scope: Vec<String>) -> ActiveTask {
    ActiveTask {
        task_id: TaskId::new_unchecked("task-e2e-hard-001"),
        epic_id: Some(EpicId::new_unchecked("epic-e2e")),
        task_format_id: Some(FormatId::new_unchecked("INF-TSK-050-010")),
        epic_format_id: Some(FormatId::new_unchecked("INF-EPC-050")),
        title: Some("Hard scope E2E".to_string()),
        status: Some("in_progress".to_string()),
        branch: Some("fix/e2e-hard".to_string()),
        session_id: Some(SessionId::new_unchecked("ses-e2e-hard01")),
        created_at: Some("2026-05-04T11:00:00Z".to_string()),
        updated_at: Some("2026-05-04T11:00:00Z".to_string()),
        current_stage: None,
        team_name: None,
        work_type: Some("FIX".to_string()),
        scope_policy: Some("hard".to_string()),
        file_scope: Some(file_scope),
        target_branch: None,
        auto_merge: None,
        epic_update: None,
    }
}

fn coordination_events_path(worktree: &Path) -> std::path::PathBuf {
    worktree
        .join(".state")
        .join("ledger")
        .join("coordination-events")
        .join("coordination-events.jsonl")
}

fn read_event_lines(path: &Path) -> Vec<serde_json::Value> {
    if !path.exists() {
        return Vec::new();
    }
    let content = std::fs::read_to_string(path).expect("read coordination-events.jsonl");
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("parse JSONL line"))
        .collect()
}

#[test]
#[serial_test::serial]
fn hard_mode_out_of_scope_edit_blocked_and_logs_claim_conflict() {
    let project_dir = tempfile::tempdir().expect("project_dir");
    let worktree_dir = tempfile::tempdir().expect("worktree_dir");

    // Make CODEFLOW_WORKTREE_PATH visible to the resolver.
    // SAFETY: serial test exclusivity over env vars.
    unsafe {
        std::env::set_var("CODEFLOW_WORKTREE_PATH", worktree_dir.path());
    }

    // Write hard-policy active-task with file_scope=["src/in.rs"].
    let task = make_hard_active_task(vec!["src/in.rs".to_string()]);
    set_active_task_resolved(
        project_dir.path(),
        &task,
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .expect("write active-task");

    // pf-3 sentinel must exist in the project_dir's sentinel space (the
    // GateCheck handler is constructed with project_dir as sentinel_dir
    // here, mirroring existing GateCheck unit tests).
    sentinel::create_by_name(project_dir.path(), "pf-3").expect("create pf-3");

    let state_path = project_dir.path().join("state.loro");
    let handler = GateCheck::new(
        project_dir.path().to_path_buf(),
        state_path,
        SessionId::new_unchecked("ses-e2e-hard01"),
        worktree_dir.path().to_path_buf(),
    );

    let input = HookInput {
        tool_name: Some("Edit".to_string()),
        tool_input: Some(serde_json::json!({"file_path": "src/out.rs"})),
        event: HookEvent::PreToolUse,
        session_id: Some("ses-e2e-hard01".to_string()),
        ..Default::default()
    };
    let out = handler.handle(input).expect("hook ran");

    match &out {
        HookOutput::Block { reason, .. } => {
            assert_eq!(out.exit_code(), 2, "hard out-of-scope must exit 2");
            assert!(
                reason.contains("scope") && reason.contains("out.rs"),
                "block reason must mention scope and the file path; got: {reason}"
            );
        }
        other => panic!("expected Block on hard out-of-scope; got {other:?}"),
    }

    // Verify ClaimConflict event landed in the worktree-local
    // coordination-events ledger.
    let events_path = coordination_events_path(worktree_dir.path());
    let events = read_event_lines(&events_path);
    assert!(
        !events.is_empty(),
        "coordination-events.jsonl must contain at least one event at {events_path:?}"
    );
    let conflict_count = events
        .iter()
        .filter(|e| e["event"].as_str() == Some("claim_conflict"))
        .count();
    assert!(
        conflict_count >= 1,
        "expected ClaimConflict event; got events: {events:?}"
    );

    let any_conflict = events
        .iter()
        .find(|e| e["event"].as_str() == Some("claim_conflict"))
        .expect("ClaimConflict present");
    assert_eq!(
        any_conflict["path"].as_str(),
        Some("src/out.rs"),
        "ClaimConflict must record the offending path"
    );
    assert_eq!(
        any_conflict["session_id"].as_str(),
        Some("ses-e2e-hard01"),
        "ClaimConflict must carry the session_id"
    );

    // SAFETY: serial test exclusivity over env vars.
    unsafe {
        std::env::remove_var("CODEFLOW_WORKTREE_PATH");
    }
}

#[test]
#[serial_test::serial]
fn hard_mode_in_scope_edit_allowed_and_logs_claim_acquired() {
    let project_dir = tempfile::tempdir().expect("project_dir");
    let worktree_dir = tempfile::tempdir().expect("worktree_dir");

    // SAFETY: serial test exclusivity over env vars.
    unsafe {
        std::env::set_var("CODEFLOW_WORKTREE_PATH", worktree_dir.path());
    }

    let task = make_hard_active_task(vec!["src/in.rs".to_string()]);
    set_active_task_resolved(
        project_dir.path(),
        &task,
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .expect("write active-task");

    sentinel::create_by_name(project_dir.path(), "pf-3").expect("create pf-3");

    let state_path = project_dir.path().join("state.loro");
    let handler = GateCheck::new(
        project_dir.path().to_path_buf(),
        state_path,
        SessionId::new_unchecked("ses-e2e-hard01"),
        worktree_dir.path().to_path_buf(),
    );

    let input = HookInput {
        tool_name: Some("Edit".to_string()),
        tool_input: Some(serde_json::json!({"file_path": "src/in.rs"})),
        event: HookEvent::PreToolUse,
        session_id: Some("ses-e2e-hard01".to_string()),
        ..Default::default()
    };
    let out = handler.handle(input).expect("hook ran");
    assert!(matches!(out, HookOutput::Allow), "in-scope must be Allow");

    let events_path = coordination_events_path(worktree_dir.path());
    let events = read_event_lines(&events_path);
    let acquired_count = events
        .iter()
        .filter(|e| e["event"].as_str() == Some("claim_acquired"))
        .count();
    assert!(
        acquired_count >= 1,
        "expected ClaimAcquired event; got events: {events:?}"
    );

    let acquired = events
        .iter()
        .find(|e| e["event"].as_str() == Some("claim_acquired"))
        .expect("ClaimAcquired present");
    assert_eq!(acquired["path"].as_str(), Some("src/in.rs"));
    assert_eq!(acquired["session_id"].as_str(), Some("ses-e2e-hard01"));

    // SAFETY: serial test exclusivity over env vars.
    unsafe {
        std::env::remove_var("CODEFLOW_WORKTREE_PATH");
    }
}

#[test]
#[serial_test::serial]
fn hard_mode_with_empty_file_scope_blocks_with_misconfig_message() {
    let project_dir = tempfile::tempdir().expect("project_dir");
    let worktree_dir = tempfile::tempdir().expect("worktree_dir");

    // SAFETY: serial test exclusivity over env vars.
    unsafe {
        std::env::set_var("CODEFLOW_WORKTREE_PATH", worktree_dir.path());
    }

    // Write hard-policy active-task with EMPTY file_scope (simulating a
    // stale active-task.json from an older binary).
    let task = make_hard_active_task(vec![]);
    set_active_task_resolved(
        project_dir.path(),
        &task,
        Some(worktree_dir.path().to_str().unwrap()),
    )
    .expect("write active-task");

    sentinel::create_by_name(project_dir.path(), "pf-3").expect("create pf-3");

    let state_path = project_dir.path().join("state.loro");
    let handler = GateCheck::new(
        project_dir.path().to_path_buf(),
        state_path,
        SessionId::new_unchecked("ses-e2e-stale01"),
        worktree_dir.path().to_path_buf(),
    );

    let input = HookInput {
        tool_name: Some("Edit".to_string()),
        tool_input: Some(serde_json::json!({"file_path": "any/file.rs"})),
        event: HookEvent::PreToolUse,
        session_id: Some("ses-e2e-stale01".to_string()),
        ..Default::default()
    };
    let out = handler.handle(input).expect("hook ran");

    match &out {
        HookOutput::Block { reason, .. } => {
            assert!(
                reason.contains("empty file_scope") || reason.contains("misconfiguration"),
                "block reason must explain misconfig; got: {reason}"
            );
        }
        other => panic!("expected Block on hard+empty; got {other:?}"),
    }

    // Verify ClaimConflict was emitted for the misconfig case.
    let events_path = coordination_events_path(worktree_dir.path());
    let events = read_event_lines(&events_path);
    assert!(
        events
            .iter()
            .any(|e| e["event"].as_str() == Some("claim_conflict")),
        "expected ClaimConflict for hard+empty misconfig; got events: {events:?}"
    );

    // SAFETY: serial test exclusivity over env vars.
    unsafe {
        std::env::remove_var("CODEFLOW_WORKTREE_PATH");
    }
}
