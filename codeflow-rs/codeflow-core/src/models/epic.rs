use serde::{Deserialize, Serialize};

use crate::types::{AreaType, EpicStatus, WorkType};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    pub id: String,
    pub format_id: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: EpicStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub is_ongoing: bool,
    pub file_scope: Vec<String>,
    pub priority: String,
    pub pr_number: Option<i64>,
    pub external_id: Option<String>,
    pub external_url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
