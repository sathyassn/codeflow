use crate::types::{
    AutorunSessionStatus, AutorunTaskRunStatus, AutorunWorkerStatus, EpicStatus, SessionStatus,
    TaskStatus, WorkStage,
};

#[derive(Debug, Default)]
pub struct SessionUpdate {
    pub status: Option<SessionStatus>,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub context_summary: Option<String>,
    pub work_ids: Option<Vec<String>>,
}

#[derive(Debug, Default)]
pub struct EpicUpdate {
    pub status: Option<EpicStatus>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub pr_number: Option<i64>,
}

#[derive(Debug, Default)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub stage: Option<WorkStage>,
    pub stage_status: Option<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub assignee_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunSessionUpdate {
    pub status: Option<AutorunSessionStatus>,
    pub completed_tasks: Option<i32>,
    pub failed_tasks: Option<i32>,
    pub skipped_tasks: Option<i32>,
    pub completed_at: Option<String>,
    pub tmux_session: Option<Option<String>>,
    pub stale_reason: Option<Option<String>>,
    pub target_branch: Option<String>,
    pub final_pr_url: Option<String>,
    /// Task currently being executed (set on dispatch, PRESERVED on finish
    /// per INF-TSK-049-001 AC #15 so terminal rows retain the last task for
    /// TUI display).
    /// Outer `Option` = "should this field be touched", inner = new value.
    pub current_task_id: Option<Option<String>>,
    /// Human-readable format id paired with `current_task_id`. Same
    /// touch/no-touch semantics. INF-TSK-049-001 AC #15.
    pub current_task_format_id: Option<Option<String>>,
    /// Timestamp of this update (RFC 3339). Written on every state-changing update.
    pub updated_at: Option<String>,
    /// Worker heartbeat timestamp (RFC 3339). Written every 10 s by the worker.
    pub last_heartbeat_at: Option<String>,
    /// Timestamp (RFC 3339) recorded when an abort signal is first observed.
    /// Outer `Option` = touch-or-not; inner = new value (Some(ts) sets the
    /// anchor; Some(None) clears it for re-armed sessions). INF-TSK-050-001
    /// AC #1: drives the Wave 1A `compute_abort_age_secs` watchdog.
    pub abort_started_at: Option<Option<String>>,
}

#[derive(Debug, Default)]
pub struct AutorunWorkerUpdate {
    pub status: Option<AutorunWorkerStatus>,
    pub tmux_session: Option<String>,
    pub worktree_path: Option<String>,
    pub file_scope: Option<Vec<String>>,
    pub scope_policy: Option<String>,
    pub worker_session_id: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunTaskRunUpdate {
    pub status: Option<AutorunTaskRunStatus>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub branch_name: Option<String>,
    pub blocked_reason: Option<String>,
    pub claim_conflicts: Option<Vec<String>>,
    pub merge_conflicts: Option<Vec<String>>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub verification_result: Option<String>,
    /// Last PathFlow phase reached before cleanup.
    pub last_phase: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_update_default_all_none() {
        let u = SessionUpdate::default();
        assert!(u.status.is_none());
        assert!(u.ended_at.is_none());
        assert!(u.duration_seconds.is_none());
        assert!(u.context_summary.is_none());
        assert!(u.work_ids.is_none());
    }

    #[test]
    fn test_session_update_with_status() {
        let u = SessionUpdate {
            status: Some(SessionStatus::Ended),
            ended_at: Some("2026-03-07T01:00:00Z".to_string()),
            duration_seconds: Some(3600),
            ..Default::default()
        };
        assert_eq!(u.status, Some(SessionStatus::Ended));
        assert_eq!(u.ended_at.as_deref(), Some("2026-03-07T01:00:00Z"));
        assert_eq!(u.duration_seconds, Some(3600));
    }

    #[test]
    fn test_epic_update_default_all_none() {
        let u = EpicUpdate::default();
        assert!(u.status.is_none());
        assert!(u.title.is_none());
        assert!(u.summary.is_none());
        assert!(u.pr_number.is_none());
    }

    #[test]
    fn test_epic_update_status_only() {
        let u = EpicUpdate {
            status: Some(EpicStatus::InProgress),
            ..Default::default()
        };
        assert_eq!(u.status, Some(EpicStatus::InProgress));
        assert!(u.title.is_none());
    }

    #[test]
    fn test_epic_update_pr_number() {
        let u = EpicUpdate {
            pr_number: Some(165),
            ..Default::default()
        };
        assert_eq!(u.pr_number, Some(165));
    }

    #[test]
    fn test_task_update_default_all_none() {
        let u = TaskUpdate::default();
        assert!(u.status.is_none());
        assert!(u.stage.is_none());
        assert!(u.stage_status.is_none());
        assert!(u.branch.is_none());
        assert!(u.pr_number.is_none());
        assert!(u.started_at.is_none());
        assert!(u.completed_at.is_none());
        assert!(u.assignee_id.is_none());
    }

    #[test]
    fn test_task_update_status_and_stage() {
        let u = TaskUpdate {
            status: Some(TaskStatus::InProgress),
            stage: Some(WorkStage::WsDev),
            stage_status: Some("running".to_string()),
            branch: Some("test/rust-hardening".to_string()),
            ..Default::default()
        };
        assert_eq!(u.status, Some(TaskStatus::InProgress));
        assert_eq!(u.stage, Some(WorkStage::WsDev));
        assert_eq!(u.stage_status.as_deref(), Some("running"));
        assert_eq!(u.branch.as_deref(), Some("test/rust-hardening"));
    }

    #[test]
    fn test_autorun_session_update_default_all_none() {
        let u = AutorunSessionUpdate::default();
        assert!(u.status.is_none());
        assert!(u.completed_tasks.is_none());
        assert!(u.failed_tasks.is_none());
        assert!(u.skipped_tasks.is_none());
        assert!(u.completed_at.is_none());
    }

    #[test]
    fn test_autorun_session_update_with_counts() {
        let u = AutorunSessionUpdate {
            status: Some(AutorunSessionStatus::Completed),
            completed_tasks: Some(5),
            failed_tasks: Some(0),
            skipped_tasks: Some(1),
            completed_at: Some("2026-03-07T02:00:00Z".to_string()),
            ..Default::default()
        };
        assert_eq!(u.completed_tasks, Some(5));
        assert_eq!(u.failed_tasks, Some(0));
        assert_eq!(u.skipped_tasks, Some(1));
    }

    #[test]
    fn test_autorun_worker_update_default_all_none() {
        let u = AutorunWorkerUpdate::default();
        assert!(u.status.is_none());
        assert!(u.tmux_session.is_none());
        assert!(u.worktree_path.is_none());
        assert!(u.pr_number.is_none());
    }

    #[test]
    fn test_autorun_task_run_update_default_all_none() {
        let u = AutorunTaskRunUpdate::default();
        assert!(u.status.is_none());
        assert!(u.pr_number.is_none());
        assert!(u.pr_url.is_none());
        assert!(u.error_message.is_none());
        assert!(u.verification_result.is_none());
    }

    #[test]
    fn test_autorun_task_run_update_with_failure() {
        let u = AutorunTaskRunUpdate {
            status: Some(AutorunTaskRunStatus::Failed), // Failed is a valid variant
            exit_code: Some(1),
            error_message: Some("cargo nextest failed".to_string()),
            duration_seconds: Some(42),
            ..Default::default()
        };
        assert_eq!(u.exit_code, Some(1));
        assert_eq!(u.error_message.as_deref(), Some("cargo nextest failed"));
        assert_eq!(u.duration_seconds, Some(42));
    }

    #[test]
    fn test_updates_debug_format() {
        let su = SessionUpdate::default();
        assert!(format!("{su:?}").contains("SessionUpdate"));
        let eu = EpicUpdate::default();
        assert!(format!("{eu:?}").contains("EpicUpdate"));
        let tu = TaskUpdate::default();
        assert!(format!("{tu:?}").contains("TaskUpdate"));
    }
}
