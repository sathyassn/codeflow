use serde::{Deserialize, Serialize};

use crate::types::{AreaType, TaskStatus, WorkStage, WorkType};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

// Bool fields mirror the existing Go/SurrealDB schema exactly.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub format_id: String,
    pub epic_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub origin: String,
    pub file_scope: Vec<String>,
    pub scope_policy: String,
    pub scope_root: Option<String>,
    pub estimate: Option<String>,
    pub priority: String,
    pub assignee_id: Option<String>,
    pub autorun_eligible: bool,
    pub auto_commit: bool,
    pub raise_pr: bool,
    pub auto_merge: bool,
    pub target_branch: Option<String>,
    pub acceptance: Vec<String>,
    pub tests: Vec<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub stage: Option<WorkStage>,
    pub stage_status: Option<String>,
    pub stage_history: Vec<serde_json::Value>,
}
