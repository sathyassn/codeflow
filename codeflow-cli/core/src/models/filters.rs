use crate::types::{
    AreaType, AutorunSessionStatus, EpicStatus, SessionStatus, TaskStatus, WorkType,
};

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
pub struct AutorunSessionFilter {
    pub status: Option<AutorunSessionStatus>,
    pub batch_name: Option<String>,
    pub since: Option<String>,
    /// INF-TSK-050-001 AC #9: explicit list of session ids to fetch. When
    /// `Some(ids)` and non-empty, the listing query restricts to rows whose
    /// `id` is in the list. Combined with `since` via OR (a row matches if
    /// either filter accepts it) so callers can union "recent rows" with
    /// "specific rows still on screen". `None` or empty list disables the
    /// filter.
    pub ids: Option<Vec<String>>,
    pub limit: Option<u32>,
    pub all: bool,
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

    #[test]
    fn test_autorun_session_filter_default_all_none() {
        let f = AutorunSessionFilter::default();
        assert!(f.status.is_none());
        assert!(f.batch_name.is_none());
        assert!(f.since.is_none());
        assert!(f.ids.is_none());
        assert!(f.limit.is_none());
        assert!(!f.all);
    }

    #[test]
    fn test_autorun_session_filter_with_all_fields() {
        let f = AutorunSessionFilter {
            status: Some(AutorunSessionStatus::Running),
            batch_name: Some("refactor".to_string()),
            since: Some("2026-03-01T00:00:00Z".to_string()),
            ids: None,
            limit: Some(5),
            all: false,
        };
        assert_eq!(f.status, Some(AutorunSessionStatus::Running));
        assert_eq!(f.batch_name.as_deref(), Some("refactor"));
        assert_eq!(f.since.as_deref(), Some("2026-03-01T00:00:00Z"));
        assert_eq!(f.limit, Some(5));
    }

    #[test]
    fn test_autorun_session_filter_with_ids() {
        // INF-TSK-050-001 AC #9: ids filter accepts an explicit list.
        let f = AutorunSessionFilter {
            ids: Some(vec!["ar-001".to_string(), "ar-002".to_string()]),
            ..Default::default()
        };
        let ids = f.ids.expect("ids set above");
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], "ar-001");
        assert_eq!(ids[1], "ar-002");
    }

    #[test]
    fn test_autorun_session_filter_empty_ids_distinct_from_none() {
        // An empty `Some(vec![])` is a valid edge case that the SQL builder
        // must skip — otherwise the WHERE clause would match nothing. The
        // distinction between `None` and `Some(empty)` is preserved at the
        // model layer; the store decides how to handle the empty case.
        let f = AutorunSessionFilter {
            ids: Some(Vec::new()),
            ..Default::default()
        };
        assert!(f.ids.is_some());
        assert_eq!(f.ids.as_ref().unwrap().len(), 0);
    }

    #[test]
    fn test_autorun_session_filter_debug() {
        let f = AutorunSessionFilter::default();
        assert!(format!("{f:?}").contains("AutorunSessionFilter"));
    }
}
