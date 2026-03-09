use serde::{Deserialize, Serialize};

use crate::types::{ActiveWorkStatus, WorkStage};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWork {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub task_id: Option<String>,
    pub topic: String,
    pub status: ActiveWorkStatus,
    pub branch: Option<String>,
    pub scope: Vec<String>,
    pub deliverables: Vec<String>,
    pub agent: Option<String>,
    pub session_id: Option<String>,
    pub current_stage: Option<WorkStage>,
    pub team_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
