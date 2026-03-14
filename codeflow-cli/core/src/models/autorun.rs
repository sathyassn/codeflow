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
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
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
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub exit_code: Option<i64>,
    pub error_message: Option<String>,
    pub verification_result: Option<String>,
    pub created_at: String,
}
