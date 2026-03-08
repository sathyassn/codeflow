use crate::types::{AreaType, EpicStatus, SessionStatus, TaskStatus, WorkType};

#[derive(Debug, Default)]
pub struct SessionFilter {
    pub status: Option<SessionStatus>,
    pub limit: Option<u32>,
}

#[derive(Debug, Default)]
pub struct EpicFilter {
    pub status: Option<EpicStatus>,
    pub area_type: Option<AreaType>,
    pub work_type: Option<WorkType>,
    pub is_ongoing: Option<bool>,
}

#[derive(Debug, Default)]
pub struct TaskFilter {
    pub epic_id: Option<String>,
    pub status: Option<TaskStatus>,
    pub area_type: Option<AreaType>,
    pub work_type: Option<WorkType>,
    pub assignee_id: Option<String>,
    pub autorun_eligible: Option<bool>,
}

#[derive(Debug, Default)]
pub struct MemoryEventFilter {
    pub event_type: Option<String>,
    pub domain: Option<String>,
    pub work_id: Option<String>,
    pub limit: Option<u32>,
}
