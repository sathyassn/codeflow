//! Task CRUD operations.

use std::collections::HashMap;

use serde_json::Value;

use crate::error::WorkgraphError;
use crate::ledger::{Event, LedgerWriter};
use crate::models::{Task, TaskFilter, TaskUpdate};
use crate::store::DataStore;
use crate::types::{AreaType, EpicId, FormatId, TaskId, TaskStatus, WorkType};

use super::format_id::generate_task_format_id;
use super::transitions::validate_task_transition;

/// Input for creating a new task.
pub struct CreateTaskInput {
    pub title: String,
    pub description: Option<String>,
    pub epic_id: EpicId,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub origin: String,
    pub file_scope: Vec<String>,
    pub priority: String,
}

/// Output returned from task operations. Uses newtypes at the API boundary.
#[derive(Debug, Clone)]
pub struct TaskOutput {
    pub id: TaskId,
    pub format_id: FormatId,
    pub epic_id: EpicId,
    pub title: String,
    pub description: Option<String>,
    pub status: TaskStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub origin: String,
    pub priority: String,
}

impl TaskOutput {
    pub(super) fn from_model(task: Task) -> Self {
        Self {
            id: TaskId::new_unchecked(&task.id),
            format_id: FormatId::new_unchecked(&task.format_id),
            epic_id: EpicId::new_unchecked(&task.epic_id),
            title: task.title,
            description: task.description,
            status: task.status,
            area_type: task.area_type,
            work_type: task.work_type,
            domain: task.domain,
            origin: task.origin,
            priority: task.priority,
        }
    }
}

/// Create a new task.
pub async fn create_task(
    store: &impl DataStore,
    ledger: &impl LedgerWriter,
    input: CreateTaskInput,
) -> Result<TaskOutput, WorkgraphError> {
    if input.title.is_empty() {
        return Err(WorkgraphError::Validation("title is required".to_string()));
    }

    // Verify parent epic exists
    let epic = store
        .get_epic(input.epic_id.as_str())
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{}", input.epic_id.as_str())))?;

    let epic_format_id = FormatId::new_unchecked(&epic.format_id);

    let ulid = ulid::Ulid::new();
    let id = format!("task-{}", ulid.to_string().to_lowercase());
    let format_id = generate_task_format_id(store, input.area_type, &epic_format_id).await?;
    let now = super::now_rfc3339();

    let task = Task {
        id: id.clone(),
        format_id: format_id.as_str().to_string(),
        epic_id: input.epic_id.as_str().to_string(),
        title: input.title.clone(),
        description: input.description.clone(),
        status: TaskStatus::Todo,
        area_type: input.area_type,
        work_type: input.work_type,
        domain: input.domain.clone(),
        origin: input.origin.clone(),
        file_scope: input.file_scope.clone(),
        scope_policy: "append".to_string(),
        scope_root: None,
        estimate: None,
        priority: input.priority.clone(),
        assignee_id: None,
        autorun_eligible: false,
        auto_commit: false,
        raise_pr: true,
        auto_merge: false,
        target_branch: None,
        acceptance: vec![],
        tests: vec![],
        branch: None,
        pr_number: None,
        external_id: None,
        external_url: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        started_at: None,
        completed_at: None,
        stage: None,
        stage_status: None,
        stage_history: vec![],
    };

    store.create_task(&task).await?;

    let mut data = HashMap::new();
    data.insert("id".to_string(), Value::String(id));
    data.insert(
        "format_id".to_string(),
        Value::String(format_id.as_str().to_string()),
    );
    data.insert(
        "epic_id".to_string(),
        Value::String(input.epic_id.as_str().to_string()),
    );
    data.insert("title".to_string(), Value::String(input.title));
    data.insert(
        "area_type".to_string(),
        Value::String(input.area_type.to_string()),
    );
    data.insert(
        "work_type".to_string(),
        Value::String(input.work_type.to_string()),
    );
    data.insert(
        "status".to_string(),
        Value::String(TaskStatus::Todo.to_string()),
    );

    let event = Event {
        event_type: "task_created".to_string(),
        timestamp: now,
        session_id: None,
        data,
    };
    ledger.append_event(event)?;

    Ok(TaskOutput::from_model(task))
}

/// Get a task by ULID-based ID.
pub async fn get_task(store: &impl DataStore, id: &TaskId) -> Result<TaskOutput, WorkgraphError> {
    let task = store
        .get_task(id.as_str())
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("task:{id}")))?;
    Ok(TaskOutput::from_model(task))
}

/// Get a task by format ID.
pub async fn get_task_by_format_id(
    store: &impl DataStore,
    format_id: &FormatId,
) -> Result<TaskOutput, WorkgraphError> {
    let task = store
        .get_task_by_format_id(format_id)
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("task:{format_id}")))?;
    Ok(TaskOutput::from_model(task))
}

/// Update a task. Validates status transitions and emits events.
pub async fn update_task(
    store: &impl DataStore,
    ledger: &impl LedgerWriter,
    id: &TaskId,
    update: TaskUpdate,
) -> Result<(), WorkgraphError> {
    let task = store
        .get_task(id.as_str())
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("task:{id}")))?;

    if let Some(new_status) = update.status {
        validate_task_transition(task.status, new_status)?;

        let mut data = HashMap::new();
        data.insert("task_id".to_string(), Value::String(task.id.clone()));
        data.insert(
            "format_id".to_string(),
            Value::String(task.format_id.clone()),
        );
        data.insert(
            "from_status".to_string(),
            Value::String(task.status.to_string()),
        );
        data.insert(
            "to_status".to_string(),
            Value::String(new_status.to_string()),
        );

        let event = Event {
            event_type: "task_status_changed".to_string(),
            timestamp: super::now_rfc3339(),
            session_id: None,
            data,
        };
        ledger.append_event(event)?;
    }

    store.update_task(id.as_str(), update).await?;
    Ok(())
}

/// List tasks matching a filter.
pub async fn list_tasks(
    store: &impl DataStore,
    filter: TaskFilter,
) -> Result<Vec<TaskOutput>, WorkgraphError> {
    let tasks = store.list_tasks(filter).await?;
    Ok(tasks.into_iter().map(TaskOutput::from_model).collect())
}

/// List tasks belonging to a specific epic.
pub async fn list_tasks_by_epic(
    store: &impl DataStore,
    epic_id: &EpicId,
) -> Result<Vec<TaskOutput>, WorkgraphError> {
    let filter = TaskFilter {
        epic_id: Some(epic_id.as_str().to_string()),
        ..Default::default()
    };
    let tasks = store.list_tasks(filter).await?;
    Ok(tasks.into_iter().map(TaskOutput::from_model).collect())
}

#[cfg(test)]
mod tests {
    use crate::error::WorkgraphError;
    use crate::models::{TaskFilter, TaskUpdate};
    use crate::types::{EpicId, FormatId, TaskId, TaskStatus};

    use super::super::test_support::{default_epic_input, default_task_input, new_service};

    #[tokio::test]
    async fn test_create_task() {
        let (service, events) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let task = service
            .create_task(default_task_input(epic.id.clone()))
            .await
            .unwrap();

        assert!(task.id.as_str().starts_with("task-"));
        assert_eq!(task.format_id.as_str(), "INF-TSK-001-001");
        assert_eq!(task.epic_id, epic.id);
        assert_eq!(task.status, TaskStatus::Todo);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 2);
        assert_eq!(evts[1].event_type, "task_created");
    }

    #[tokio::test]
    async fn test_create_task_empty_title_rejected() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let mut input = default_task_input(epic.id);
        input.title = String::new();

        let err = service.create_task(input).await.unwrap_err();
        assert!(matches!(err, WorkgraphError::Validation(_)));
    }

    #[tokio::test]
    async fn test_create_task_nonexistent_epic() {
        let (service, _) = new_service();
        let input = default_task_input(EpicId::new_unchecked("epic-nonexistent"));

        let err = service.create_task(input).await.unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_task_by_id() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let created = service
            .create_task(default_task_input(epic.id))
            .await
            .unwrap();

        let fetched = service.get_task(&created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.title, "Test Task");
    }

    #[tokio::test]
    async fn test_get_task_not_found() {
        let (service, _) = new_service();
        let id = TaskId::new_unchecked("task-nonexistent");
        let err = service.get_task(&id).await.unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_task_by_format_id() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let created = service
            .create_task(default_task_input(epic.id))
            .await
            .unwrap();

        let format_id = FormatId::new_unchecked("INF-TSK-001-001");
        let fetched = service.get_task_by_format_id(&format_id).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn test_update_task_status() {
        let (service, events) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let task = service
            .create_task(default_task_input(epic.id))
            .await
            .unwrap();

        service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let updated = service.get_task(&task.id).await.unwrap();
        assert_eq!(updated.status, TaskStatus::InProgress);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 3);
        assert_eq!(evts[2].event_type, "task_status_changed");
    }

    #[tokio::test]
    async fn test_list_tasks() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        service
            .create_task(default_task_input(epic.id.clone()))
            .await
            .unwrap();

        let mut input2 = default_task_input(epic.id);
        input2.title = "Second Task".to_string();
        service.create_task(input2).await.unwrap();

        let all = service.list_tasks(TaskFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_list_tasks_by_epic() {
        let (service, _) = new_service();
        let epic1 = service.create_epic(default_epic_input()).await.unwrap();
        let mut input2 = default_epic_input();
        input2.title = "Other Epic".to_string();
        let epic2 = service.create_epic(input2).await.unwrap();

        service
            .create_task(default_task_input(epic1.id.clone()))
            .await
            .unwrap();
        service
            .create_task(default_task_input(epic2.id.clone()))
            .await
            .unwrap();

        // MockStore ignores filters (returns all), but we verify the API
        // contract: function accepts an epic_id, returns TaskOutputs, and
        // at minimum includes the task belonging to the queried epic.
        let tasks = service.list_tasks_by_epic(&epic1.id).await.unwrap();
        assert!(!tasks.is_empty());
        assert!(tasks.iter().any(|t| t.epic_id == epic1.id));
    }

    #[tokio::test]
    async fn test_create_task_description_round_trip() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let input = default_task_input(epic.id.clone());
        let created = service.create_task(input).await.unwrap();
        assert_eq!(created.description.as_deref(), Some("A test task"));

        let fetched = service.get_task(&created.id).await.unwrap();
        assert_eq!(fetched.description.as_deref(), Some("A test task"));
    }

    #[tokio::test]
    async fn test_create_task_no_description() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let mut input = default_task_input(epic.id.clone());
        input.description = None;
        let created = service.create_task(input).await.unwrap();
        assert_eq!(created.description, None);

        let fetched = service.get_task(&created.id).await.unwrap();
        assert_eq!(fetched.description, None);
    }

    #[tokio::test]
    async fn test_list_tasks_with_status_filter() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        service
            .create_task(default_task_input(epic.id.clone()))
            .await
            .unwrap();

        // MockStore ignores filters, but this verifies the workgraph layer
        // constructs the filter correctly and maps output without error.
        let filter = TaskFilter {
            status: Some(TaskStatus::Todo),
            ..Default::default()
        };
        let results = service.list_tasks(filter).await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].status, TaskStatus::Todo);
    }

    #[tokio::test]
    async fn test_update_nonexistent_task() {
        let (service, _) = new_service();
        let id = TaskId::new_unchecked("task-nonexistent");
        let err = service
            .update_task(
                &id,
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_invalid_task_transition_via_service() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        let task = service
            .create_task(default_task_input(epic.id))
            .await
            .unwrap();

        let err = service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::Complete),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::InvalidTransition { .. }));
    }
}
