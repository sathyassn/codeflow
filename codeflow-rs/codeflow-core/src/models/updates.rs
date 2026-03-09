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
    pub completed_at: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunWorkerUpdate {
    pub status: Option<AutorunWorkerStatus>,
    pub tmux_session: Option<String>,
    pub worktree_path: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Default)]
pub struct AutorunTaskRunUpdate {
    pub status: Option<AutorunTaskRunStatus>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub verification_result: Option<String>,
}
