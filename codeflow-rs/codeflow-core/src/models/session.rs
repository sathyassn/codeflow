use serde::{Deserialize, Serialize};

use crate::types::SessionStatus;

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub project_id: Option<String>,
    pub user_id: String,
    pub user_host: String,
    pub machine_fingerprint: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_seconds: Option<i64>,
    pub status: SessionStatus,
    pub work_ids: Vec<String>,
    pub previous_session_id: Option<String>,
    pub context_summary: Option<String>,
    #[serde(default)]
    pub tool_stats: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
