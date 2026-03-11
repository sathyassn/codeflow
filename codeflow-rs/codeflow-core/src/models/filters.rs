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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_filter_default_all_none() {
        let f = SessionFilter::default();
        assert!(f.status.is_none());
        assert!(f.limit.is_none());
    }

    #[test]
    fn test_session_filter_with_status() {
        let f = SessionFilter {
            status: Some(SessionStatus::Active),
            limit: Some(10),
        };
        assert_eq!(f.status, Some(SessionStatus::Active));
        assert_eq!(f.limit, Some(10));
    }

    #[test]
    fn test_epic_filter_default_all_none() {
        let f = EpicFilter::default();
        assert!(f.status.is_none());
        assert!(f.area_type.is_none());
        assert!(f.work_type.is_none());
        assert!(f.is_ongoing.is_none());
    }

    #[test]
    fn test_epic_filter_with_all_fields() {
        let f = EpicFilter {
            status: Some(EpicStatus::InProgress),
            area_type: Some(AreaType::Inf),
            work_type: Some(WorkType::Feat),
            is_ongoing: Some(false),
        };
        assert_eq!(f.status, Some(EpicStatus::InProgress));
        assert_eq!(f.area_type, Some(AreaType::Inf));
        assert_eq!(f.work_type, Some(WorkType::Feat));
        assert_eq!(f.is_ongoing, Some(false));
    }

    #[test]
    fn test_task_filter_default_all_none() {
        let f = TaskFilter::default();
        assert!(f.epic_id.is_none());
        assert!(f.status.is_none());
        assert!(f.area_type.is_none());
        assert!(f.work_type.is_none());
        assert!(f.assignee_id.is_none());
        assert!(f.autorun_eligible.is_none());
    }

    #[test]
    fn test_task_filter_with_epic_id() {
        let f = TaskFilter {
            epic_id: Some("epic-01abc".to_string()),
            status: Some(TaskStatus::InProgress),
            ..Default::default()
        };
        assert_eq!(f.epic_id.as_deref(), Some("epic-01abc"));
        assert_eq!(f.status, Some(TaskStatus::InProgress));
    }

    #[test]
    fn test_task_filter_autorun_eligible() {
        let f = TaskFilter {
            autorun_eligible: Some(true),
            ..Default::default()
        };
        assert_eq!(f.autorun_eligible, Some(true));
    }

    #[test]
    fn test_memory_event_filter_default_all_none() {
        let f = MemoryEventFilter::default();
        assert!(f.event_type.is_none());
        assert!(f.domain.is_none());
        assert!(f.work_id.is_none());
        assert!(f.limit.is_none());
    }

    #[test]
    fn test_memory_event_filter_with_all_fields() {
        let f = MemoryEventFilter {
            event_type: Some("milestone".to_string()),
            domain: Some("INF".to_string()),
            work_id: Some("work-01abc".to_string()),
            limit: Some(50),
        };
        assert_eq!(f.event_type.as_deref(), Some("milestone"));
        assert_eq!(f.domain.as_deref(), Some("INF"));
        assert_eq!(f.work_id.as_deref(), Some("work-01abc"));
        assert_eq!(f.limit, Some(50));
    }

    #[test]
    fn test_filters_debug_format() {
        let sf = SessionFilter::default();
        assert!(format!("{sf:?}").contains("SessionFilter"));
        let ef = EpicFilter::default();
        assert!(format!("{ef:?}").contains("EpicFilter"));
        let tf = TaskFilter::default();
        assert!(format!("{tf:?}").contains("TaskFilter"));
        let mf = MemoryEventFilter::default();
        assert!(format!("{mf:?}").contains("MemoryEventFilter"));
    }
}
