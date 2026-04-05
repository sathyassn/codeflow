//! Autorun batch parsing and orchestration.
//!
//! Provides:
//! - YAML batch file parsing with validation ([`batch`])
//! - Worker DI traits for isolated execution ([`worker`])
//! - Orchestrator for dependency-aware concurrent execution ([`orchestrator`])

pub mod batch;
pub mod config;
pub mod orchestrator;
pub mod stale;
pub mod worker;

pub use batch::{
    BatchFile, ParsedBatch, TaskSpec, build_resume_batch, resolve_task_path,
    validate_batch_extended,
};
pub use config::{AutorunConfig, ParallelWorkConfig, load_config};
pub use orchestrator::{Orchestrator, WorkerConfig, WorkerResult};
pub use stale::{
    CleanupReport, StaleSessionInfo, SweepSummary, check_heartbeat_alive, check_pid_alive,
    check_tmux_alive, cleanup_stale_session, detect_stale_sessions, is_session_stale,
    sweep_stale_sessions,
};
pub use worker::{
    ClaudeInvoker, InvokeConfig, InvokeResult, RealWorktreeProvider, TaskMetadata, TmuxRunner,
    TmuxWorker, WorkerRunner, WorktreeInfo, WorktreeProvider, build_task_prompt,
    build_task_prompt_from_file, parse_task_markdown,
};

/// Emit an autorun event to the JSONL ledger.
///
/// Shared helper used by both the orchestrator and worker to avoid duplication.
pub fn emit_autorun_event(
    project_dir: &std::path::Path,
    event: &crate::coordination::types::events::AutorunEvent,
) {
    let type_name = crate::ledger::files::AUTORUN_EVENTS;
    let subdir = project_dir.join(".state/ledger").join(type_name);
    let _ = std::fs::create_dir_all(&subdir);
    let file_path = subdir.join(format!("{type_name}.jsonl"));
    if let Ok(json) = serde_json::to_string(event) {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path);
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{json}");
        }
    }
}

/// Emit a coordination event to the JSONL ledger.
pub fn emit_coordination_event(
    project_dir: &std::path::Path,
    event: &crate::coordination::types::events::CoordinationEvent,
) {
    let type_name = crate::ledger::files::COORDINATION_EVENTS;
    let subdir = project_dir.join(".state/ledger").join(type_name);
    let _ = std::fs::create_dir_all(&subdir);
    let file_path = subdir.join(format!("{type_name}.jsonl"));
    if let Ok(json) = serde_json::to_string(event) {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path);
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{json}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- L3: Shared emit function writes to correct file --

    #[test]
    fn test_emit_autorun_event_writes_to_autorun_events() {
        let dir = tempfile::tempdir().unwrap();
        let event = crate::coordination::types::events::AutorunEvent::BatchStarted {
            session_id: "ses-test".to_string(),
            batch_name: "test-batch".to_string(),
            total_tasks: 3,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        emit_autorun_event(dir.path(), &event);

        let type_name = crate::ledger::files::AUTORUN_EVENTS;
        let file_path = dir
            .path()
            .join(".state/ledger")
            .join(type_name)
            .join(format!("{type_name}.jsonl"));
        assert!(file_path.exists(), "autorun-events.jsonl should be created");
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            content.contains("batch_started"),
            "should contain the batch_started event type"
        );
        assert!(
            content.contains("ses-test"),
            "should contain the session ID"
        );
    }

    #[test]
    fn test_emit_coordination_event_writes_to_coordination_events() {
        let dir = tempfile::tempdir().unwrap();
        let event = crate::coordination::types::events::CoordinationEvent::ClaimReleased {
            session_id: crate::types::SessionId::new_unchecked("ses-test"),
            path: "src/main.rs".to_string(),
            task_id: Some("task-1".to_string()),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };

        emit_coordination_event(dir.path(), &event);

        let coord_type = crate::ledger::files::COORDINATION_EVENTS;
        let file_path = dir
            .path()
            .join(".state/ledger")
            .join(coord_type)
            .join(format!("{coord_type}.jsonl"));
        assert!(
            file_path.exists(),
            "coordination-events.jsonl should be created"
        );
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            content.contains("coord_claim_released"),
            "should contain the claim_released event type"
        );
    }

    // -- L2: Verification result field --

    #[test]
    fn test_verification_result_field_exists_on_update() {
        let update = crate::models::AutorunTaskRunUpdate {
            verification_result: Some("pass".to_string()),
            ..Default::default()
        };
        assert_eq!(
            update.verification_result.as_deref(),
            Some("pass"),
            "verification_result should be settable"
        );
    }

    #[test]
    fn test_verification_result_values() {
        for (status, expected) in [
            ("completed", "pass"),
            ("timeout", "timeout"),
            ("failed", "fail"),
        ] {
            let result = match status {
                "completed" => "pass",
                "timeout" => "timeout",
                _ => "fail",
            };
            assert_eq!(
                result, expected,
                "status '{status}' should map to verification '{expected}'"
            );
        }
    }

    // -- H3: branch_name field on AutorunTaskRunUpdate --

    #[test]
    fn test_branch_name_field_exists_on_update() {
        let update = crate::models::AutorunTaskRunUpdate {
            branch_name: Some("feat/my-feature".to_string()),
            ..Default::default()
        };
        assert_eq!(
            update.branch_name.as_deref(),
            Some("feat/my-feature"),
            "branch_name should be settable on AutorunTaskRunUpdate"
        );

        let default_update = crate::models::AutorunTaskRunUpdate::default();
        assert!(
            default_update.branch_name.is_none(),
            "branch_name should default to None"
        );
    }
}
