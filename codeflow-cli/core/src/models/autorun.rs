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
    pub created_at: String,
    pub completed_at: Option<String>,
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
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: None,
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
            status: AutorunSessionStatus::Pending,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: None,
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
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: Some("2026-03-21T01:00:00Z".into()),
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"pid\":999"));
        assert!(json.contains("\"skipped_tasks\":1"));
    }

    #[test]
    fn test_autorun_session_defaults_for_new_fields() {
        // Verify that default values are correct for new fields.
        let session = AutorunSession {
            id: "ar-def".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Pending,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            created_at: "2026-03-21T00:00:00Z".into(),
            completed_at: None,
        };
        assert!(session.pid.is_none());
        assert_eq!(session.skipped_tasks, 0);
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
            verification_result: None,
            created_at: "2026-03-21T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("blocked"));
        assert!(json.contains("a.rs"));
        assert!(json.contains("b.rs"));
    }
}
