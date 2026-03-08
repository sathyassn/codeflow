use serde::{Deserialize, Serialize};

use crate::types::WorkStage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWork {
    pub id: String,
    pub task_id: Option<String>,
    pub topic: String,
    pub status: String,
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
