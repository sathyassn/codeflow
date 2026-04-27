use serde::{Deserialize, Serialize};

use crate::types::{AutorunSessionStatus, AutorunTaskRunStatus, AutorunWorkerStatus};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutorunSession {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub batch_file: String,
    pub batch_name: Option<String>,
    pub status: AutorunSessionStatus,
    pub max_session_workers: i32,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    #[serde(default)]
    pub pid: Option<i64>,
    #[serde(default)]
    pub skipped_tasks: i32,
    #[serde(default)]
    pub tmux_session: Option<String>,
    #[serde(default)]
    pub stale_reason: Option<String>,
    /// Target branch for PRs in this autorun session (e.g., "autorun/batch-xxx").
    #[serde(default)]
    pub target_branch: Option<String>,
    /// URL of the final PR (integration branch -> main) if created.
    #[serde(default)]
    pub final_pr_url: Option<String>,
    /// Task ID currently being executed by a worker in this session.
    ///
    /// INF-TSK-049-001 AC #15: written on task dispatch and PRESERVED on
    /// task finish (retained as the last-dispatched task on terminal rows
    /// so the TUI TASK column does not go blank mid-run). Used by the
    /// status TUI to populate the TASK column.
    #[serde(default)]
    pub current_task_id: Option<String>,
    /// Human-readable task format ID (e.g. `INF-TSK-048-001`) paired with
    /// `current_task_id`. Same lifecycle: written on dispatch, preserved
    /// on finish. Preferred over `current_task_id` for display so the
    /// TUI shows identifiers operators recognize.
    #[serde(default)]
    pub current_task_format_id: Option<String>,
    /// Last time any field on this record was updated (RFC 3339).
    ///
    /// Updated on task state changes and orchestrator heartbeat. Used by the
    /// TUI to compute ELAPSED for terminal rows when `completed_at` is absent.
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Last worker heartbeat timestamp (RFC 3339).
    ///
    /// Written every `worker_heartbeat_interval_secs` by the worker main loop.
    /// Used by the TUI to render the IDLE column and flag stale workers.
    #[serde(default)]
    pub last_heartbeat_at: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
    /// Timestamp (RFC 3339) recorded when the orchestrator first observes an
    /// abort signal for this session. Drives the abort-timeout watchdog
    /// (`AutorunConfig::abort_timeout_secs`). `None` for sessions that have
    /// never entered an Aborting transition.
    #[serde(default)]
    pub abort_started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutorunWorker {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub session_id: String,
    pub worker_num: i32,
    pub task_id: String,
    pub status: AutorunWorkerStatus,
    pub tmux_session: Option<String>,
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub file_scope: Vec<String>,
    #[serde(default = "default_scope_policy")]
    pub scope_policy: String,
    #[serde(default)]
    pub worker_session_id: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

fn default_scope_policy() -> String {
    "soft".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutorunTaskRun {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub worker_id: String,
    pub task_id: String,
    pub session_id: String,
    pub status: AutorunTaskRunStatus,
    pub branch_name: Option<String>,
    pub worktree_path: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub claim_conflicts: Option<Vec<String>>,
    #[serde(default)]
    pub merge_conflicts: Option<Vec<String>>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub verification_result: Option<String>,
    /// Last PathFlow phase reached before cleanup (e.g., "PF4").
    /// Stored so TUI can display phase after worktree is destroyed.
    #[serde(default)]
    pub last_phase: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_scope_policy_returns_soft() {
        assert_eq!(default_scope_policy(), "soft");
    }

    #[test]
    fn test_autorun_session_new_fields() {
        let session = AutorunSession {
            id: "ar-001".into(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("test".into()),
            status: AutorunSessionStatus::Running,
            max_session_workers: 3,
            total_tasks: 10,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: Some(12345),
            skipped_tasks: 2,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: None,
            abort_started_at: None,
        };
        assert_eq!(session.pid, Some(12345));
        assert_eq!(session.skipped_tasks, 2);
    }

    #[test]
    fn test_autorun_session_pid_none() {
        let session = AutorunSession {
            id: "ar-002".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Cancelled,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: None,
            abort_started_at: None,
        };
        assert!(session.pid.is_none());
        assert_eq!(session.skipped_tasks, 0);
    }

    #[test]
    fn test_autorun_session_serializes_new_fields() {
        let session = AutorunSession {
            id: "ar-003".into(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("serde-test".into()),
            status: AutorunSessionStatus::Completed,
            max_session_workers: 2,
            total_tasks: 5,
            completed_tasks: 3,
            failed_tasks: 1,
            pid: Some(999),
            skipped_tasks: 1,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: Some("2026-03-21T01:00:00Z".into()),
            abort_started_at: None,
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"pid\":999"));
        assert!(json.contains("\"skipped_tasks\":1"));
    }

    #[test]
    fn test_autorun_session_tui_fields_serialization() {
        // Verify that current_task_id, updated_at, and last_heartbeat_at
        // serialize correctly. Note: `id` uses `serialize_record_id` so full
        // round-trip needs the SurrealDB deserializer; we only assert on the
        // serialized form here (the DB side handles deserialization).
        let session = AutorunSession {
            id: "ar-tui".into(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("tui-test".into()),
            status: AutorunSessionStatus::Running,
            max_session_workers: 1,
            total_tasks: 3,
            completed_tasks: 1,
            failed_tasks: 0,
            pid: Some(42),
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: Some("INF-TSK-001-001".into()),
            current_task_format_id: Some("INF-TSK-001-001".into()),
            updated_at: Some("2026-04-20T10:00:00Z".into()),
            last_heartbeat_at: Some("2026-04-20T10:00:05Z".into()),
            created_at: "2026-04-20T09:00:00Z".into(),
            completed_at: None,
            abort_started_at: None,
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(
            json.contains("\"current_task_id\":\"INF-TSK-001-001\""),
            "expected current_task_id in JSON, got: {json}"
        );
        assert!(
            json.contains("\"updated_at\":\"2026-04-20T10:00:00Z\""),
            "expected updated_at in JSON, got: {json}"
        );
        assert!(
            json.contains("\"last_heartbeat_at\":\"2026-04-20T10:00:05Z\""),
            "expected last_heartbeat_at in JSON, got: {json}"
        );
    }

    #[test]
    fn test_autorun_session_tui_fields_none_serialization() {
        // When the new fields are None, they serialize as JSON null (not
        // absent) — #[serde(default)] controls deserialization, not the
        // serialized representation. This confirms the DB write path emits
        // explicit nulls so legacy records get populated on update.
        let session = AutorunSession {
            id: "ar-none".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Cancelled,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-04-01T00:00:00Z".into(),
            completed_at: None,
            abort_started_at: None,
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"current_task_id\":null"));
        assert!(json.contains("\"updated_at\":null"));
        assert!(json.contains("\"last_heartbeat_at\":null"));
    }

    #[test]
    fn test_autorun_session_defaults_for_new_fields() {
        // Verify that default values are correct for new fields.
        let session = AutorunSession {
            id: "ar-def".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Cancelled,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: None,
            abort_started_at: None,
        };
        assert!(session.pid.is_none());
        assert_eq!(session.skipped_tasks, 0);
        assert!(session.abort_started_at.is_none());
    }

    #[test]
    fn test_autorun_session_abort_started_at_some_serializes() {
        // INF-TSK-050-001 AC #1: Some(timestamp) serializes as the RFC 3339
        // string. The DB write path needs explicit values for legacy rows.
        let session = AutorunSession {
            id: "ar-abort".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Aborting,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-04-25T00:00:00Z".into(),
            completed_at: None,
            abort_started_at: Some("2026-04-25T00:01:00Z".into()),
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(
            json.contains("\"abort_started_at\":\"2026-04-25T00:01:00Z\""),
            "expected abort_started_at in JSON, got: {json}"
        );
    }

    #[test]
    fn test_autorun_session_abort_started_at_none_serializes_null() {
        let session = AutorunSession {
            id: "ar-noabort".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Running,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-04-25T00:00:00Z".into(),
            completed_at: None,
            abort_started_at: None,
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"abort_started_at\":null"));
    }

    #[test]
    fn test_autorun_worker_new_fields() {
        let worker = AutorunWorker {
            id: "aw-001".into(),
            session_id: "ar-001".into(),
            worker_num: 1,
            task_id: "task-a".into(),
            status: AutorunWorkerStatus::Running,
            tmux_session: Some("tmux-1".into()),
            worktree_path: Some("/tmp/wt".into()),
            file_scope: vec!["src/**/*.rs".into(), "tests/".into()],
            scope_policy: "hard".into(),
            worker_session_id: Some("ses-test-123".into()),
            pr_number: None,
            started_at: Some("2026-03-21T00:00:00Z".into()),
            completed_at: None,
        };
        assert_eq!(worker.file_scope.len(), 2);
        assert_eq!(worker.scope_policy, "hard");
        assert_eq!(worker.worker_session_id.as_deref(), Some("ses-test-123"));
    }

    #[test]
    fn test_autorun_worker_defaults_for_new_fields() {
        // Verify default values for new fields when constructed explicitly.
        let worker = AutorunWorker {
            id: "aw-def".into(),
            session_id: "ar-1".into(),
            worker_num: 0,
            task_id: "t-1".into(),
            status: AutorunWorkerStatus::Queued,
            tmux_session: None,
            worktree_path: None,
            file_scope: vec![],
            scope_policy: default_scope_policy(),
            worker_session_id: None,
            pr_number: None,
            started_at: None,
            completed_at: None,
        };
        assert!(worker.file_scope.is_empty());
        assert_eq!(worker.scope_policy, "soft");
        assert!(worker.worker_session_id.is_none());
    }

    #[test]
    fn test_autorun_task_run_new_fields() {
        let run = AutorunTaskRun {
            id: "atr-001".into(),
            worker_id: "aw-001".into(),
            task_id: "task-a".into(),
            session_id: "ar-001".into(),
            status: AutorunTaskRunStatus::Failed,
            branch_name: Some("feat/x".into()),
            worktree_path: Some("/tmp/wt".into()),
            pr_number: None,
            pr_url: None,
            blocked_reason: Some("claim conflict".into()),
            claim_conflicts: Some(vec!["src/main.rs".into()]),
            merge_conflicts: Some(vec!["src/lib.rs".into()]),
            started_at: Some("2026-03-21T00:00:00Z".into()),
            completed_at: Some("2026-03-21T01:00:00Z".into()),
            duration_seconds: Some(60),
            exit_code: Some(1),
            error_message: Some("failed".into()),
            last_phase: None,
            verification_result: None,
            created_at: "2026-03-21T00:00:00Z".into(),
        };
        assert_eq!(run.blocked_reason.as_deref(), Some("claim conflict"));
        assert_eq!(run.claim_conflicts.as_ref().unwrap().len(), 1);
        assert_eq!(run.merge_conflicts.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_autorun_task_run_defaults_for_new_fields() {
        let run = AutorunTaskRun {
            id: "atr-def".into(),
            worker_id: "w".into(),
            task_id: "t".into(),
            session_id: "s".into(),
            status: AutorunTaskRunStatus::Pending,
            branch_name: None,
            worktree_path: None,
            pr_number: None,
            pr_url: None,
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: None,
            completed_at: None,
            duration_seconds: None,
            exit_code: None,
            error_message: None,
            last_phase: None,
            verification_result: None,
            created_at: "2026-03-21T00:00:00Z".into(),
        };
        assert!(run.blocked_reason.is_none());
        assert!(run.claim_conflicts.is_none());
        assert!(run.merge_conflicts.is_none());
    }

    #[test]
    fn test_autorun_task_run_serializes_conflict_fields() {
        let run = AutorunTaskRun {
            id: "atr-cf".into(),
            worker_id: "w".into(),
            task_id: "t".into(),
            session_id: "s".into(),
            status: AutorunTaskRunStatus::Failed,
            branch_name: None,
            worktree_path: None,
            pr_number: None,
            pr_url: None,
            blocked_reason: Some("blocked".into()),
            claim_conflicts: Some(vec!["a.rs".into(), "b.rs".into()]),
            merge_conflicts: None,
            started_at: None,
            completed_at: None,
            duration_seconds: None,
            exit_code: None,
            error_message: None,
            last_phase: None,
            verification_result: None,
            created_at: "2026-03-21T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("blocked"));
        assert!(json.contains("a.rs"));
        assert!(json.contains("b.rs"));
    }
}
