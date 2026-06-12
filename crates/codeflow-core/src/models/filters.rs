//! List filters for epic and task queries.
//!
//! v2 trim: session, autorun, and memory-event filters dropped with their
//! subsystems; area/work-type filtering dropped with the v1 `types/` module.

use super::status::{EpicStatus, TaskStatus};

#[derive(Debug, Default, Clone)]
pub struct EpicFilter {
    pub status: Option<EpicStatus>,
}

#[derive(Debug, Default, Clone)]
pub struct TaskFilter {
    /// Parent epic's ULID-based id.
    pub epic_id: Option<String>,
    pub status: Option<TaskStatus>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epic_filter_default_all_none() {
        let f = EpicFilter::default();
        assert!(f.status.is_none());
    }

    #[test]
    fn test_epic_filter_with_status() {
        let f = EpicFilter {
            status: Some(EpicStatus::InProgress),
        };
        assert_eq!(f.status, Some(EpicStatus::InProgress));
    }

    #[test]
    fn test_task_filter_default_all_none() {
        let f = TaskFilter::default();
        assert!(f.epic_id.is_none());
        assert!(f.status.is_none());
    }

    #[test]
    fn test_task_filter_with_epic_id() {
        let f = TaskFilter {
            epic_id: Some("epic-01abc".to_string()),
            status: Some(TaskStatus::InProgress),
        };
        assert_eq!(f.epic_id.as_deref(), Some("epic-01abc"));
        assert_eq!(f.status, Some(TaskStatus::InProgress));
    }

    #[test]
    fn test_filters_debug_format() {
        let ef = EpicFilter::default();
        assert!(format!("{ef:?}").contains("EpicFilter"));
        let tf = TaskFilter::default();
        assert!(format!("{tf:?}").contains("TaskFilter"));
    }
}
