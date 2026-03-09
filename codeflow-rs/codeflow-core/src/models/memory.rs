use serde::{Deserialize, Serialize};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub event_type: String,
    pub domain: String,
    pub work_id: Option<String>,
    pub data: String,
    pub memory_type: Option<String>,
    pub created_at: String,
}
