//! Query operations for the workgraph.
//!
//! Convenience query functions: active tasks, tasks by status, and tasks
//! by epic. (v1's reserved `QueryFilter`/`QueryResult` placeholders were
//! dead weight and did not cross — charter D22.)

use crate::models::{Task, TaskFilter, TaskStatus};

use super::store::RecordStore;
use super::WorkgraphError;

/// Get all tasks with `InProgress` status.
pub fn active_tasks(store: &impl RecordStore) -> Result<Vec<Task>, WorkgraphError> {
    tasks_by_status(store, TaskStatus::InProgress)
}

/// Get tasks matching a specific status.
pub fn tasks_by_status(
    store: &impl RecordStore,
    status: TaskStatus,
) -> Result<Vec<Task>, WorkgraphError> {
    let filter = TaskFilter {
        status: Some(status),
        ..Default::default()
    };
    Ok(store.list_tasks(filter)?)
}

/// Get tasks belonging to a specific epic.
pub fn tasks_by_epic(
    store: &impl RecordStore,
    epic_id: &str,
) -> Result<Vec<Task>, WorkgraphError> {
    let filter = TaskFilter {
        epic_id: Some(epic_id.to_string()),
        ..Default::default()
    };
    Ok(store.list_tasks(filter)?)
}

#[cfg(test)]
mod tests {
    use crate::models::{TaskStatus, TaskUpdate};

    use super::super::test_support::{default_epic_input, default_task_input, new_service};

    #[test]
    fn test_active_tasks() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();

        // Transition to InProgress
        service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let active = service.active_tasks().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, task.id);
    }

    #[test]
    fn test_active_tasks_empty_when_none_in_progress() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        // Task is created in Todo status, not InProgress.
        service.create_task(default_task_input(&epic.id)).unwrap();

        let active = service.active_tasks().unwrap();
        assert!(active.is_empty());
    }

    #[test]
    fn test_tasks_by_status() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();

        let todo_tasks = service.tasks_by_status(TaskStatus::Todo).unwrap();
        assert_eq!(todo_tasks.len(), 1);
        assert_eq!(todo_tasks[0].id, task.id);
        assert_eq!(todo_tasks[0].status, TaskStatus::Todo);
    }

    #[test]
    fn test_tasks_by_epic() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let other = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();
        service.create_task(default_task_input(&other.id)).unwrap();

        let tasks = service.tasks_by_epic(&epic.id).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task.id);
        assert_eq!(tasks[0].epic_id, epic.id);
    }
}
