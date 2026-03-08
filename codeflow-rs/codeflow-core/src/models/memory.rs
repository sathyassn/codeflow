use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub id: String,
    pub event_type: String,
    pub domain: String,
    pub work_id: Option<String>,
    pub data: String,
    pub memory_type: Option<String>,
    pub created_at: String,
}
