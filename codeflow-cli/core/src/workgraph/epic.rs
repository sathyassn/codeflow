//! Epic CRUD operations.

use std::collections::HashMap;

use serde_json::Value;

use crate::error::WorkgraphError;
use crate::ledger::{Event, LedgerWriter};
use crate::models::{Epic, EpicFilter, EpicUpdate};
use crate::store::DataStore;
use crate::types::{AreaType, EpicId, EpicStatus, FormatId, WorkType};

use super::format_id::generate_epic_format_id;
use super::transitions::validate_epic_transition;

/// Input for creating a new epic.
pub struct CreateEpicInput {
    pub title: String,
    pub summary: Option<String>,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub is_ongoing: bool,
    pub file_scope: Vec<String>,
    pub priority: String,
}

/// Output returned from epic operations. Uses newtypes at the API boundary.
#[derive(Debug, Clone)]
pub struct EpicOutput {
    pub id: EpicId,
    pub format_id: FormatId,
    pub title: String,
    pub summary: Option<String>,
    pub status: EpicStatus,
    pub area_type: AreaType,
    pub work_type: WorkType,
    pub domain: String,
    pub is_ongoing: bool,
    pub file_scope: Vec<String>,
    pub priority: String,
    pub pr_number: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

impl EpicOutput {
    fn from_model(epic: Epic) -> Self {
        Self {
            id: EpicId::new_unchecked(&epic.id),
            format_id: FormatId::new_unchecked(&epic.format_id),
            title: epic.title,
            summary: epic.summary,
            status: epic.status,
            area_type: epic.area_type,
            work_type: epic.work_type,
            domain: epic.domain,
            is_ongoing: epic.is_ongoing,
            file_scope: epic.file_scope,
            priority: epic.priority,
            pr_number: epic.pr_number,
            created_at: epic.created_at,
            updated_at: epic.updated_at,
        }
    }
}

/// Create a new epic with generated IDs.
pub async fn create_epic(
    store: &impl DataStore,
    ledger: &impl LedgerWriter,
    input: CreateEpicInput,
) -> Result<EpicOutput, WorkgraphError> {
    if input.title.is_empty() {
        return Err(WorkgraphError::Validation("title is required".to_string()));
    }

    let ulid = ulid::Ulid::new();
    let id = format!("epic-{}", ulid.to_string().to_lowercase());
    let format_id = generate_epic_format_id(store, input.area_type).await?;
    let now = now_rfc3339();

    let epic = Epic {
        id: id.clone(),
        format_id: format_id.as_str().to_string(),
        title: input.title.clone(),
        summary: input.summary.clone(),
        status: EpicStatus::Draft,
        area_type: input.area_type,
        work_type: input.work_type,
        domain: input.domain.clone(),
        is_ongoing: input.is_ongoing,
        file_scope: input.file_scope.clone(),
        priority: input.priority.clone(),
        pr_number: None,
        external_id: None,
        external_url: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    store.create_epic(&epic).await?;

    let mut data = HashMap::new();
    data.insert("id".to_string(), Value::String(id));
    data.insert(
        "format_id".to_string(),
        Value::String(format_id.as_str().to_string()),
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
        Value::String(EpicStatus::Draft.to_string()),
    );

    let event = Event {
        event_type: "epic_created".to_string(),
        timestamp: now,
        session_id: None,
        worktree: None,
        data,
    };
    ledger.append_event(event)?;

    Ok(EpicOutput::from_model(epic))
}

/// Get an epic by ULID-based ID.
pub async fn get_epic(store: &impl DataStore, id: &EpicId) -> Result<EpicOutput, WorkgraphError> {
    let epic = store
        .get_epic(id.as_str())
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{id}")))?;
    Ok(EpicOutput::from_model(epic))
}

/// Get an epic by format ID.
pub async fn get_epic_by_format_id(
    store: &impl DataStore,
    format_id: &FormatId,
) -> Result<EpicOutput, WorkgraphError> {
    let epic = store
        .get_epic_by_format_id(format_id)
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{format_id}")))?;
    Ok(EpicOutput::from_model(epic))
}

/// Update an epic. Validates status transitions and emits events.
pub async fn update_epic(
    store: &impl DataStore,
    ledger: &impl LedgerWriter,
    id: &EpicId,
    update: EpicUpdate,
) -> Result<(), WorkgraphError> {
    let epic = store
        .get_epic(id.as_str())
        .await?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{id}")))?;

    if let Some(new_status) = update.status {
        validate_epic_transition(epic.status, new_status)?;

        let mut data = HashMap::new();
        data.insert("epic_id".to_string(), Value::String(epic.id.clone()));
        data.insert(
            "format_id".to_string(),
            Value::String(epic.format_id.clone()),
        );
        data.insert(
            "from_status".to_string(),
            Value::String(epic.status.to_string()),
        );
        data.insert(
            "to_status".to_string(),
            Value::String(new_status.to_string()),
        );

        let event = Event {
            event_type: "epic_status_changed".to_string(),
            timestamp: now_rfc3339(),
            session_id: None,
            worktree: None,
            data,
        };
        ledger.append_event(event)?;
    }

    store.update_epic(id.as_str(), update).await?;
    Ok(())
}

/// List epics matching a filter.
pub async fn list_epics(
    store: &impl DataStore,
    filter: EpicFilter,
) -> Result<Vec<EpicOutput>, WorkgraphError> {
    let epics = store.list_epics(filter).await?;
    Ok(epics.into_iter().map(EpicOutput::from_model).collect())
}

fn now_rfc3339() -> String {
    super::now_rfc3339()
}

#[cfg(test)]
mod tests {
    use crate::error::WorkgraphError;
    use crate::models::{EpicFilter, EpicUpdate};
    use crate::types::{EpicId, EpicStatus, FormatId};

    use super::super::test_support::{default_epic_input, new_service};

    #[tokio::test]
    async fn test_create_epic() {
        let (service, events) = new_service();
        let output = service.create_epic(default_epic_input()).await.unwrap();

        assert!(output.id.as_str().starts_with("epic-"));
        assert_eq!(output.format_id.as_str(), "INF-EPC-001");
        assert_eq!(output.title, "Test Epic");
        assert_eq!(output.status, EpicStatus::Draft);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].event_type, "epic_created");
    }

    #[tokio::test]
    async fn test_create_epic_empty_title_rejected() {
        let (service, _) = new_service();
        let mut input = default_epic_input();
        input.title = String::new();

        let err = service.create_epic(input).await.unwrap_err();
        assert!(matches!(err, WorkgraphError::Validation(_)));
    }

    #[tokio::test]
    async fn test_get_epic_by_id() {
        let (service, _) = new_service();
        let created = service.create_epic(default_epic_input()).await.unwrap();

        let fetched = service.get_epic(&created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.title, "Test Epic");
    }

    #[tokio::test]
    async fn test_get_epic_not_found() {
        let (service, _) = new_service();
        let id = EpicId::new_unchecked("epic-nonexistent");
        let err = service.get_epic(&id).await.unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_epic_by_format_id() {
        let (service, _) = new_service();
        let created = service.create_epic(default_epic_input()).await.unwrap();

        let format_id = FormatId::new_unchecked("INF-EPC-001");
        let fetched = service.get_epic_by_format_id(&format_id).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[tokio::test]
    async fn test_update_epic_status() {
        let (service, events) = new_service();
        let created = service.create_epic(default_epic_input()).await.unwrap();

        service
            .update_epic(
                &created.id,
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let updated = service.get_epic(&created.id).await.unwrap();
        assert_eq!(updated.status, EpicStatus::InProgress);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 2);
        assert_eq!(evts[1].event_type, "epic_status_changed");
    }

    #[tokio::test]
    async fn test_list_epics() {
        let (service, _) = new_service();
        service.create_epic(default_epic_input()).await.unwrap();

        let mut input2 = default_epic_input();
        input2.title = "Second Epic".to_string();
        service.create_epic(input2).await.unwrap();

        let all = service.list_epics(EpicFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_update_nonexistent_epic() {
        let (service, _) = new_service();
        let id = EpicId::new_unchecked("epic-nonexistent");
        let err = service
            .update_epic(
                &id,
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_invalid_epic_transition_via_service() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();

        let err = service
            .update_epic(
                &epic.id,
                EpicUpdate {
                    status: Some(EpicStatus::Complete),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::InvalidTransition { .. }));
    }

    #[tokio::test]
    async fn test_create_epic_summary_round_trip() {
        let (service, _) = new_service();
        let input = default_epic_input();
        let created = service.create_epic(input).await.unwrap();
        assert_eq!(created.summary.as_deref(), Some("A test epic"));

        let fetched = service.get_epic(&created.id).await.unwrap();
        assert_eq!(fetched.summary.as_deref(), Some("A test epic"));
    }

    #[tokio::test]
    async fn test_create_epic_no_summary() {
        let (service, _) = new_service();
        let mut input = default_epic_input();
        input.summary = None;
        let created = service.create_epic(input).await.unwrap();
        assert_eq!(created.summary, None);

        let fetched = service.get_epic(&created.id).await.unwrap();
        assert_eq!(fetched.summary, None);
    }

    #[tokio::test]
    async fn test_list_epics_with_status_filter() {
        let (service, _) = new_service();
        service.create_epic(default_epic_input()).await.unwrap();

        // Filter by status — MockStore ignores filters, but this verifies
        // the workgraph layer constructs and passes the filter without error
        // and correctly maps results to EpicOutput.
        let filter = EpicFilter {
            status: Some(EpicStatus::Draft),
            ..Default::default()
        };
        let results = service.list_epics(filter).await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].status, EpicStatus::Draft);
    }

    #[tokio::test]
    async fn test_list_epics_with_area_type_filter() {
        let (service, _) = new_service();
        service.create_epic(default_epic_input()).await.unwrap();

        let filter = EpicFilter {
            area_type: Some(crate::types::AreaType::Inf),
            ..Default::default()
        };
        let results = service.list_epics(filter).await.unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].area_type, crate::types::AreaType::Inf);
    }

    #[tokio::test]
    async fn test_list_epics_default_filter_returns_all() {
        let (service, _) = new_service();
        service.create_epic(default_epic_input()).await.unwrap();

        let mut input2 = default_epic_input();
        input2.title = "Second".to_string();
        service.create_epic(input2).await.unwrap();

        let results = service.list_epics(EpicFilter::default()).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_update_epic_title() {
        let (service, _) = new_service();
        let epic = service.create_epic(default_epic_input()).await.unwrap();

        service
            .update_epic(
                &epic.id,
                EpicUpdate {
                    title: Some("Updated Title".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let fetched = service.get_epic(&epic.id).await.unwrap();
        assert_eq!(fetched.title, "Updated Title");
    }
}
