//! Partial-update structs for epic and task records.
//!
//! `None` means "leave the field untouched". v2 trim: session and autorun
//! updates dropped with their subsystems; pathflow stage fields dropped.

use super::status::{EpicStatus, TaskStatus};

#[derive(Debug, Default, Clone)]
pub struct EpicUpdate {
    pub status: Option<EpicStatus>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub pr_number: Option<i64>,
}

#[derive(Debug, Default, Clone)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epic_update_default_all_none() {
        let u = EpicUpdate::default();
        assert!(u.status.is_none());
        assert!(u.title.is_none());
        assert!(u.summary.is_none());
        assert!(u.pr_number.is_none());
    }

    #[test]
    fn test_epic_update_status_only() {
        let u = EpicUpdate {
            status: Some(EpicStatus::InProgress),
            ..Default::default()
        };
        assert_eq!(u.status, Some(EpicStatus::InProgress));
        assert!(u.title.is_none());
    }

    #[test]
    fn test_epic_update_pr_number() {
        let u = EpicUpdate {
            pr_number: Some(165),
            ..Default::default()
        };
        assert_eq!(u.pr_number, Some(165));
    }

    #[test]
    fn test_task_update_default_all_none() {
        let u = TaskUpdate::default();
        assert!(u.status.is_none());
        assert!(u.branch.is_none());
        assert!(u.pr_number.is_none());
        assert!(u.started_at.is_none());
        assert!(u.completed_at.is_none());
    }

    #[test]
    fn test_task_update_status_and_branch() {
        let u = TaskUpdate {
            status: Some(TaskStatus::InProgress),
            branch: Some("feat/v2-records".to_string()),
            ..Default::default()
        };
        assert_eq!(u.status, Some(TaskStatus::InProgress));
        assert_eq!(u.branch.as_deref(), Some("feat/v2-records"));
    }

    #[test]
    fn test_updates_debug_format() {
        let eu = EpicUpdate::default();
        assert!(format!("{eu:?}").contains("EpicUpdate"));
        let tu = TaskUpdate::default();
        assert!(format!("{tu:?}").contains("TaskUpdate"));
    }
}
