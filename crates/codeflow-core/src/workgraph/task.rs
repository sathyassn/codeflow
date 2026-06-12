//! Task CRUD operations.

use std::collections::HashMap;

use serde_json::Value;

use crate::ledger::{Event, LedgerWriter};
use crate::models::{Task, TaskFilter, TaskStatus, TaskUpdate};

use super::format_id::generate_task_format_id;
use super::store::RecordStore;
use super::transitions::validate_task_transition;
use super::WorkgraphError;

/// Input for creating a new task.
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    /// Parent epic's ULID-based id.
    pub epic_id: String,
    /// Work intent label from the v2 branch-prefix set (e.g. `fix`).
    pub work_type: String,
    pub priority: String,
    /// Acceptance criteria — the input-clarity contract (charter §2.1).
    pub acceptance: Vec<String>,
}

/// Create a new task.
pub fn create_task(
    store: &impl RecordStore,
    ledger: &impl LedgerWriter,
    input: CreateTaskInput,
) -> Result<Task, WorkgraphError> {
    if input.title.is_empty() {
        return Err(WorkgraphError::Validation("title is required".to_string()));
    }

    // Verify parent epic exists.
    let epic = store
        .get_epic(&input.epic_id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{}", input.epic_id)))?;

    let ulid = ulid::Ulid::new();
    let id = format!("task-{}", ulid.to_string().to_lowercase());
    let format_id = generate_task_format_id(store, &epic.format_id)?;
    let now = super::now_rfc3339();

    let task = Task {
        id: id.clone(),
        format_id: format_id.clone(),
        epic_id: input.epic_id.clone(),
        title: input.title.clone(),
        description: input.description,
        status: TaskStatus::Todo,
        work_type: input.work_type.clone(),
        priority: input.priority,
        estimate: None,
        acceptance: input.acceptance,
        tests: vec![],
        branch: None,
        pr_number: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        started_at: None,
        completed_at: None,
    };

    store.create_task(&task)?;

    let mut data = HashMap::new();
    data.insert("id".to_string(), Value::String(id));
    data.insert("format_id".to_string(), Value::String(format_id));
    data.insert("epic_id".to_string(), Value::String(input.epic_id));
    data.insert("title".to_string(), Value::String(input.title));
    data.insert("work_type".to_string(), Value::String(input.work_type));
    data.insert(
        "status".to_string(),
        Value::String(TaskStatus::Todo.to_string()),
    );

    let event = Event {
        event_type: "task_created".to_string(),
        timestamp: now,
        session_id: None,
        worktree: None,
        data,
    };
    ledger.append_event(event)?;

    Ok(task)
}

/// Get a task by ULID-based id.
pub fn get_task(store: &impl RecordStore, id: &str) -> Result<Task, WorkgraphError> {
    store
        .get_task(id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("task:{id}")))
}

/// Get a task by format id.
pub fn get_task_by_format_id(
    store: &impl RecordStore,
    format_id: &str,
) -> Result<Task, WorkgraphError> {
    store
        .get_task_by_format_id(format_id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("task:{format_id}")))
}

/// Update a task. Validates status transitions and emits events.
///
/// The store update is applied before the event is emitted, so the ledger
/// never records a status change that failed to persist.
pub fn update_task(
    store: &impl RecordStore,
    ledger: &impl LedgerWriter,
    id: &str,
    update: TaskUpdate,
) -> Result<(), WorkgraphError> {
    let task = store
        .get_task(id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("task:{id}")))?;

    let status_change = update.status.map(|new_status| (task.status, new_status));

    if let Some((from, to)) = status_change {
        validate_task_transition(from, to)?;
    }

    store.update_task(id, update)?;

    if let Some((from, to)) = status_change {
        let mut data = HashMap::new();
        data.insert("task_id".to_string(), Value::String(task.id));
        data.insert("format_id".to_string(), Value::String(task.format_id));
        data.insert("from_status".to_string(), Value::String(from.to_string()));
        data.insert("to_status".to_string(), Value::String(to.to_string()));

        let event = Event {
            event_type: "task_status_changed".to_string(),
            timestamp: super::now_rfc3339(),
            session_id: None,
            worktree: None,
            data,
        };
        ledger.append_event(event)?;
    }

    Ok(())
}

/// List tasks matching a filter.
pub fn list_tasks(
    store: &impl RecordStore,
    filter: TaskFilter,
) -> Result<Vec<Task>, WorkgraphError> {
    Ok(store.list_tasks(filter)?)
}

/// List tasks belonging to a specific epic.
pub fn list_tasks_by_epic(
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
    use crate::models::{TaskFilter, TaskStatus, TaskUpdate};

    use super::super::test_support::{default_epic_input, default_task_input, new_service};
    use super::super::WorkgraphError;

    #[test]
    fn test_create_task() {
        let (service, events, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();

        assert!(task.id.starts_with("task-"));
        assert_eq!(task.format_id, "TSK-001-001");
        assert_eq!(task.epic_id, epic.id);
        assert_eq!(task.status, TaskStatus::Todo);
        assert_eq!(task.acceptance, vec!["it works".to_string()]);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 2);
        assert_eq!(evts[1].event_type, "task_created");
        assert_eq!(evts[1].data["format_id"], "TSK-001-001");
    }

    #[test]
    fn test_create_task_empty_title_rejected() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let mut input = default_task_input(&epic.id);
        input.title = String::new();

        let err = service.create_task(input).unwrap_err();
        assert!(matches!(err, WorkgraphError::Validation(_)));
    }

    #[test]
    fn test_create_task_nonexistent_epic() {
        let (service, _, _dir) = new_service();
        let input = default_task_input("epic-nonexistent");

        let err = service.create_task(input).unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[test]
    fn test_get_task_by_id() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let created = service.create_task(default_task_input(&epic.id)).unwrap();

        let fetched = service.get_task(&created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.title, "Test Task");
    }

    #[test]
    fn test_get_task_not_found() {
        let (service, _, _dir) = new_service();
        let err = service.get_task("task-nonexistent").unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[test]
    fn test_get_task_by_format_id() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let created = service.create_task(default_task_input(&epic.id)).unwrap();

        let fetched = service.get_task_by_format_id("TSK-001-001").unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[test]
    fn test_update_task_status() {
        let (service, events, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();

        service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let updated = service.get_task(&task.id).unwrap();
        assert_eq!(updated.status, TaskStatus::InProgress);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 3);
        assert_eq!(evts[2].event_type, "task_status_changed");
        assert_eq!(evts[2].data["from_status"], "todo");
        assert_eq!(evts[2].data["to_status"], "in_progress");
    }

    #[test]
    fn test_list_tasks() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        service.create_task(default_task_input(&epic.id)).unwrap();

        let mut input2 = default_task_input(&epic.id);
        input2.title = "Second Task".to_string();
        service.create_task(input2).unwrap();

        let all = service.list_tasks(TaskFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_list_tasks_by_epic() {
        let (service, _, _dir) = new_service();
        let epic1 = service.create_epic(default_epic_input()).unwrap();
        let mut input2 = default_epic_input();
        input2.title = "Other Epic".to_string();
        let epic2 = service.create_epic(input2).unwrap();

        let first_task = service.create_task(default_task_input(&epic1.id)).unwrap();
        service.create_task(default_task_input(&epic2.id)).unwrap();

        let tasks = service.list_tasks_by_epic(&epic1.id).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, first_task.id);
        assert_eq!(tasks[0].epic_id, epic1.id);
    }

    #[test]
    fn test_create_task_description_round_trip() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let created = service.create_task(default_task_input(&epic.id)).unwrap();
        assert_eq!(created.description.as_deref(), Some("A test task"));

        let fetched = service.get_task(&created.id).unwrap();
        assert_eq!(fetched.description.as_deref(), Some("A test task"));
    }

    #[test]
    fn test_create_task_no_description() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let mut input = default_task_input(&epic.id);
        input.description = None;
        let created = service.create_task(input).unwrap();
        assert_eq!(created.description, None);

        let fetched = service.get_task(&created.id).unwrap();
        assert_eq!(fetched.description, None);
    }

    #[test]
    fn test_list_tasks_with_status_filter() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();
        service.create_task(default_task_input(&epic.id)).unwrap();

        service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let todo = service
            .list_tasks(TaskFilter {
                status: Some(TaskStatus::Todo),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(todo.len(), 1);
        assert_ne!(todo[0].id, task.id);
    }

    #[test]
    fn test_update_nonexistent_task() {
        let (service, _, _dir) = new_service();
        let err = service
            .update_task(
                "task-nonexistent",
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[test]
    fn test_invalid_task_transition_via_service() {
        let (service, events, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();
        let task = service.create_task(default_task_input(&epic.id)).unwrap();

        let err = service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::Complete),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::InvalidTransition { .. }));

        // No status-change event for a rejected transition.
        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 2);
    }
}
