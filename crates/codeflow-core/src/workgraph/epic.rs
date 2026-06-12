//! Epic CRUD operations.

use std::collections::HashMap;

use serde_json::Value;

use crate::ledger::{Event, LedgerWriter};
use crate::models::{Epic, EpicFilter, EpicStatus, EpicUpdate};

use super::format_id::generate_epic_format_id;
use super::store::RecordStore;
use super::transitions::validate_epic_transition;
use super::WorkgraphError;

/// Input for creating a new epic.
pub struct CreateEpicInput {
    pub title: String,
    pub summary: Option<String>,
    /// Work intent label from the v2 branch-prefix set (e.g. `feat`).
    pub work_type: String,
    pub priority: String,
}

/// Create a new epic with generated ids.
pub fn create_epic(
    store: &impl RecordStore,
    ledger: &impl LedgerWriter,
    input: CreateEpicInput,
) -> Result<Epic, WorkgraphError> {
    if input.title.is_empty() {
        return Err(WorkgraphError::Validation("title is required".to_string()));
    }

    let ulid = ulid::Ulid::new();
    let id = format!("epic-{}", ulid.to_string().to_lowercase());
    let format_id = generate_epic_format_id(store)?;
    let now = super::now_rfc3339();

    let epic = Epic {
        id: id.clone(),
        format_id: format_id.clone(),
        title: input.title.clone(),
        summary: input.summary,
        status: EpicStatus::Draft,
        work_type: input.work_type.clone(),
        priority: input.priority,
        pr_number: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    store.create_epic(&epic)?;

    let mut data = HashMap::new();
    data.insert("id".to_string(), Value::String(id));
    data.insert("format_id".to_string(), Value::String(format_id));
    data.insert("title".to_string(), Value::String(input.title));
    data.insert("work_type".to_string(), Value::String(input.work_type));
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

    Ok(epic)
}

/// Get an epic by ULID-based id.
pub fn get_epic(store: &impl RecordStore, id: &str) -> Result<Epic, WorkgraphError> {
    store
        .get_epic(id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{id}")))
}

/// Get an epic by format id.
pub fn get_epic_by_format_id(
    store: &impl RecordStore,
    format_id: &str,
) -> Result<Epic, WorkgraphError> {
    store
        .get_epic_by_format_id(format_id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{format_id}")))
}

/// Update an epic. Validates status transitions and emits events.
///
/// The store update is applied before the event is emitted, so the ledger
/// never records a status change that failed to persist.
pub fn update_epic(
    store: &impl RecordStore,
    ledger: &impl LedgerWriter,
    id: &str,
    update: EpicUpdate,
) -> Result<(), WorkgraphError> {
    let epic = store
        .get_epic(id)?
        .ok_or_else(|| WorkgraphError::NotFound(format!("epic:{id}")))?;

    let status_change = update.status.map(|new_status| (epic.status, new_status));

    if let Some((from, to)) = status_change {
        validate_epic_transition(from, to)?;
    }

    store.update_epic(id, update)?;

    if let Some((from, to)) = status_change {
        let mut data = HashMap::new();
        data.insert("epic_id".to_string(), Value::String(epic.id));
        data.insert("format_id".to_string(), Value::String(epic.format_id));
        data.insert("from_status".to_string(), Value::String(from.to_string()));
        data.insert("to_status".to_string(), Value::String(to.to_string()));

        let event = Event {
            event_type: "epic_status_changed".to_string(),
            timestamp: super::now_rfc3339(),
            session_id: None,
            worktree: None,
            data,
        };
        ledger.append_event(event)?;
    }

    Ok(())
}

/// List epics matching a filter.
pub fn list_epics(
    store: &impl RecordStore,
    filter: EpicFilter,
) -> Result<Vec<Epic>, WorkgraphError> {
    Ok(store.list_epics(filter)?)
}

#[cfg(test)]
mod tests {
    use crate::models::{EpicFilter, EpicStatus, EpicUpdate};

    use super::super::test_support::{default_epic_input, new_service};
    use super::super::WorkgraphError;

    #[test]
    fn test_create_epic() {
        let (service, events, _dir) = new_service();
        let output = service.create_epic(default_epic_input()).unwrap();

        assert!(output.id.starts_with("epic-"));
        assert_eq!(output.format_id, "EPC-001");
        assert_eq!(output.title, "Test Epic");
        assert_eq!(output.status, EpicStatus::Draft);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].event_type, "epic_created");
        assert_eq!(evts[0].data["format_id"], "EPC-001");
        assert_eq!(evts[0].data["work_type"], "feat");
    }

    #[test]
    fn test_create_epic_empty_title_rejected() {
        let (service, _, _dir) = new_service();
        let mut input = default_epic_input();
        input.title = String::new();

        let err = service.create_epic(input).unwrap_err();
        assert!(matches!(err, WorkgraphError::Validation(_)));
    }

    #[test]
    fn test_get_epic_by_id() {
        let (service, _, _dir) = new_service();
        let created = service.create_epic(default_epic_input()).unwrap();

        let fetched = service.get_epic(&created.id).unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.title, "Test Epic");
    }

    #[test]
    fn test_get_epic_not_found() {
        let (service, _, _dir) = new_service();
        let err = service.get_epic("epic-nonexistent").unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[test]
    fn test_get_epic_by_format_id() {
        let (service, _, _dir) = new_service();
        let created = service.create_epic(default_epic_input()).unwrap();

        let fetched = service.get_epic_by_format_id("EPC-001").unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[test]
    fn test_update_epic_status() {
        let (service, events, _dir) = new_service();
        let created = service.create_epic(default_epic_input()).unwrap();

        service
            .update_epic(
                &created.id,
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let updated = service.get_epic(&created.id).unwrap();
        assert_eq!(updated.status, EpicStatus::InProgress);

        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 2);
        assert_eq!(evts[1].event_type, "epic_status_changed");
        assert_eq!(evts[1].data["from_status"], "draft");
        assert_eq!(evts[1].data["to_status"], "in_progress");
    }

    #[test]
    fn test_list_epics() {
        let (service, _, _dir) = new_service();
        service.create_epic(default_epic_input()).unwrap();

        let mut input2 = default_epic_input();
        input2.title = "Second Epic".to_string();
        service.create_epic(input2).unwrap();

        let all = service.list_epics(EpicFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_update_nonexistent_epic() {
        let (service, _, _dir) = new_service();
        let err = service
            .update_epic(
                "epic-nonexistent",
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::NotFound(_)));
    }

    #[test]
    fn test_invalid_epic_transition_via_service() {
        let (service, events, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();

        let err = service
            .update_epic(
                &epic.id,
                EpicUpdate {
                    status: Some(EpicStatus::Complete),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(err, WorkgraphError::InvalidTransition { .. }));

        // No status-change event for a rejected transition.
        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 1);
    }

    #[test]
    fn test_create_epic_summary_round_trip() {
        let (service, _, _dir) = new_service();
        let created = service.create_epic(default_epic_input()).unwrap();
        assert_eq!(created.summary.as_deref(), Some("A test epic"));

        let fetched = service.get_epic(&created.id).unwrap();
        assert_eq!(fetched.summary.as_deref(), Some("A test epic"));
    }

    #[test]
    fn test_create_epic_no_summary() {
        let (service, _, _dir) = new_service();
        let mut input = default_epic_input();
        input.summary = None;
        let created = service.create_epic(input).unwrap();
        assert_eq!(created.summary, None);

        let fetched = service.get_epic(&created.id).unwrap();
        assert_eq!(fetched.summary, None);
    }

    #[test]
    fn test_list_epics_with_status_filter() {
        let (service, _, _dir) = new_service();
        let first = service.create_epic(default_epic_input()).unwrap();
        let second = service.create_epic(default_epic_input()).unwrap();

        service
            .update_epic(
                &second.id,
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .unwrap();

        let drafts = service
            .list_epics(EpicFilter {
                status: Some(EpicStatus::Draft),
            })
            .unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].id, first.id);
    }

    #[test]
    fn test_update_epic_title() {
        let (service, _, _dir) = new_service();
        let epic = service.create_epic(default_epic_input()).unwrap();

        service
            .update_epic(
                &epic.id,
                EpicUpdate {
                    title: Some("Updated Title".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        let fetched = service.get_epic(&epic.id).unwrap();
        assert_eq!(fetched.title, "Updated Title");
    }
}
