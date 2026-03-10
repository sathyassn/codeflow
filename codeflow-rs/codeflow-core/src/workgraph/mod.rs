//! Workgraph operations: epic/task CRUD, status transitions, and queries.
//!
//! Provides `WorkgraphService` as the main entry point for all workgraph
//! operations. All persistence flows through the `DataStore` trait and all
//! event emission flows through `LedgerWriter`.

mod epic;
mod format_id;
mod query;
mod task;
mod transitions;

pub use epic::{CreateEpicInput, EpicOutput};
pub use format_id::{generate_epic_format_id, generate_task_format_id};
pub use query::{QueryFilter, QueryResult};
pub use task::{CreateTaskInput, TaskOutput};
pub use transitions::{validate_epic_transition, validate_task_transition};

use crate::error::WorkgraphError;
use crate::ledger::LedgerWriter;
use crate::models::{EpicFilter, EpicUpdate, TaskFilter, TaskUpdate};
use crate::store::DataStore;
use crate::types::{EpicId, EpicStatus, FormatId, TaskId, TaskStatus};

/// Central service for workgraph operations.
///
/// Generic over `DataStore` and `LedgerWriter` for testability.
/// Production uses `SurrealStore` + `JsonlWriter`; tests use `MockStore`
/// + a recording `MockLedger`.
pub struct WorkgraphService<S: DataStore, L: LedgerWriter> {
    store: S,
    ledger: L,
}

impl<S: DataStore, L: LedgerWriter> WorkgraphService<S, L> {
    /// Create a new workgraph service with the given store and ledger.
    pub fn new(store: S, ledger: L) -> Self {
        Self { store, ledger }
    }

    // -- Epic operations --

    /// Create a new epic.
    ///
    /// Generates dual IDs (ULID-based + format ID), persists via `DataStore`,
    /// and emits an `epic_created` event to the ledger.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Validation` if the title is empty, or
    /// `WorkgraphError::Db` / `WorkgraphError::Ledger` on persistence failure.
    pub async fn create_epic(&self, input: CreateEpicInput) -> Result<EpicOutput, WorkgraphError> {
        epic::create_epic(&self.store, &self.ledger, input).await
    }

    /// Get an epic by its ULID-based ID.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::NotFound` if the epic does not exist.
    pub async fn get_epic(&self, id: &EpicId) -> Result<EpicOutput, WorkgraphError> {
        epic::get_epic(&self.store, id).await
    }

    /// Get an epic by its human-readable format ID.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::NotFound` if no epic matches the format ID.
    pub async fn get_epic_by_format_id(
        &self,
        format_id: &FormatId,
    ) -> Result<EpicOutput, WorkgraphError> {
        epic::get_epic_by_format_id(&self.store, format_id).await
    }

    /// Update an epic's fields. Status changes emit `epic_status_changed` events.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::NotFound` if the epic does not exist,
    /// `WorkgraphError::InvalidTransition` for invalid status changes.
    pub async fn update_epic(&self, id: &EpicId, update: EpicUpdate) -> Result<(), WorkgraphError> {
        epic::update_epic(&self.store, &self.ledger, id, update).await
    }

    /// List epics matching the given filter.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Db` on persistence failure.
    pub async fn list_epics(&self, filter: EpicFilter) -> Result<Vec<EpicOutput>, WorkgraphError> {
        epic::list_epics(&self.store, filter).await
    }

    // -- Task operations --

    /// Create a new task.
    ///
    /// Generates dual IDs, validates the parent epic exists, persists via
    /// `DataStore`, and emits a `task_created` event.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Validation` if the title is empty,
    /// `WorkgraphError::NotFound` if the parent epic does not exist.
    pub async fn create_task(&self, input: CreateTaskInput) -> Result<TaskOutput, WorkgraphError> {
        task::create_task(&self.store, &self.ledger, input).await
    }

    /// Get a task by its ULID-based ID.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::NotFound` if the task does not exist.
    pub async fn get_task(&self, id: &TaskId) -> Result<TaskOutput, WorkgraphError> {
        task::get_task(&self.store, id).await
    }

    /// Get a task by its human-readable format ID.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::NotFound` if no task matches the format ID.
    pub async fn get_task_by_format_id(
        &self,
        format_id: &FormatId,
    ) -> Result<TaskOutput, WorkgraphError> {
        task::get_task_by_format_id(&self.store, format_id).await
    }

    /// Update a task's fields. Status changes are validated and emit
    /// `task_status_changed` events.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::NotFound` if the task does not exist,
    /// `WorkgraphError::InvalidTransition` for invalid status changes.
    pub async fn update_task(&self, id: &TaskId, update: TaskUpdate) -> Result<(), WorkgraphError> {
        task::update_task(&self.store, &self.ledger, id, update).await
    }

    /// List tasks matching the given filter.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Db` on persistence failure.
    pub async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<TaskOutput>, WorkgraphError> {
        task::list_tasks(&self.store, filter).await
    }

    /// List tasks belonging to a specific epic.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Db` on persistence failure.
    pub async fn list_tasks_by_epic(
        &self,
        epic_id: &EpicId,
    ) -> Result<Vec<TaskOutput>, WorkgraphError> {
        task::list_tasks_by_epic(&self.store, epic_id).await
    }

    // -- Query operations --

    /// Get all tasks with `InProgress` status.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Db` on persistence failure.
    pub async fn active_tasks(&self) -> Result<Vec<TaskOutput>, WorkgraphError> {
        query::active_tasks(&self.store).await
    }

    /// Get tasks matching a specific status.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Db` on persistence failure.
    pub async fn tasks_by_status(
        &self,
        status: TaskStatus,
    ) -> Result<Vec<TaskOutput>, WorkgraphError> {
        query::tasks_by_status(&self.store, status).await
    }

    /// Get tasks belonging to a specific epic (query shortcut).
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::Db` on persistence failure.
    pub async fn tasks_by_epic(&self, epic_id: &EpicId) -> Result<Vec<TaskOutput>, WorkgraphError> {
        query::tasks_by_epic(&self.store, epic_id).await
    }

    // -- Transition validation (exposed for direct use) --

    /// Validate a task status transition without persisting.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::InvalidTransition` if the transition is not allowed.
    pub fn validate_task_transition(
        from: TaskStatus,
        to: TaskStatus,
    ) -> Result<(), WorkgraphError> {
        transitions::validate_task_transition(from, to)
    }

    /// Validate an epic status transition without persisting.
    ///
    /// # Errors
    ///
    /// Returns `WorkgraphError::InvalidTransition` if the transition is not allowed.
    pub fn validate_epic_transition(
        from: EpicStatus,
        to: EpicStatus,
    ) -> Result<(), WorkgraphError> {
        transitions::validate_epic_transition(from, to)
    }
}

/// Generate an RFC 3339 UTC timestamp string.
fn now_rfc3339() -> String {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;
    let (year, month, day) = days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert days since Unix epoch to (year, month, day).
/// Algorithm from Howard Hinnant's `civil_from_days`.
fn days_to_ymd(days_since_epoch: u64) -> (u64, u64, u64) {
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use crate::error::LedgerError;
    use crate::ledger::{Event, LedgerWriter};
    use crate::store::mock::MockStore;

    use super::WorkgraphService;
    use super::epic::CreateEpicInput;
    use super::task::CreateTaskInput;

    pub struct MockLedger {
        pub events: Arc<Mutex<Vec<Event>>>,
        dir: PathBuf,
    }

    impl MockLedger {
        pub fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
                dir: PathBuf::from("/tmp/test-ledger"),
            }
        }
    }

    impl LedgerWriter for MockLedger {
        fn append_event(&self, event: Event) -> Result<(), LedgerError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn append_event_to_file(
            &self,
            _target_file: &str,
            event: Event,
        ) -> Result<(), LedgerError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn route_event(&self, event_type: &str) -> Result<String, LedgerError> {
            crate::ledger::route_event_type(event_type).map(|s| s.to_string())
        }

        fn dir(&self) -> &Path {
            &self.dir
        }
    }

    pub fn new_service() -> (
        WorkgraphService<MockStore, MockLedger>,
        Arc<Mutex<Vec<Event>>>,
    ) {
        let ledger = MockLedger::new();
        let events = Arc::clone(&ledger.events);
        let service = WorkgraphService::new(MockStore::new(), ledger);
        (service, events)
    }

    pub fn default_epic_input() -> CreateEpicInput {
        CreateEpicInput {
            title: "Test Epic".to_string(),
            summary: Some("A test epic".to_string()),
            area_type: crate::types::AreaType::Inf,
            work_type: crate::types::WorkType::Feat,
            domain: "infrastructure".to_string(),
            is_ongoing: false,
            file_scope: vec!["src/".to_string()],
            priority: "high".to_string(),
        }
    }

    pub fn default_task_input(epic_id: crate::types::EpicId) -> CreateTaskInput {
        CreateTaskInput {
            title: "Test Task".to_string(),
            description: Some("A test task".to_string()),
            epic_id,
            area_type: crate::types::AreaType::Inf,
            work_type: crate::types::WorkType::Feat,
            domain: "infrastructure".to_string(),
            origin: "adhoc".to_string(),
            file_scope: vec!["src/foo.rs".to_string()],
            priority: "high".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::WorkgraphError;
    use crate::models::{EpicUpdate, TaskUpdate};
    use crate::types::{EpicStatus, TaskStatus};

    use super::test_support::{default_epic_input, default_task_input, new_service};

    #[test]
    fn test_workgraph_error_display() {
        let err = WorkgraphError::NotFound("epic:foo".to_string());
        assert_eq!(err.to_string(), "not found: epic:foo");

        let err = WorkgraphError::InvalidTransition {
            entity: "task".to_string(),
            from: "todo".to_string(),
            to: "complete".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "invalid transition: task from todo to complete"
        );

        let err = WorkgraphError::Validation("title is required".to_string());
        assert_eq!(err.to_string(), "validation error: title is required");

        let err = WorkgraphError::FormatIdGeneration("bad sequence".to_string());
        assert_eq!(err.to_string(), "format ID generation error: bad sequence");
    }

    #[test]
    fn test_now_rfc3339_format() {
        let ts = super::now_rfc3339();
        // Format: YYYY-MM-DDTHH:MM:SSZ
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn test_days_to_ymd_epoch() {
        let (y, m, d) = super::days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
    }

    #[test]
    fn test_days_to_ymd_known_date() {
        // 2024-01-01 = 19723 days since epoch
        let (y, m, d) = super::days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }

    #[tokio::test]
    async fn test_epic_task_lifecycle() {
        let (service, events) = new_service();

        // Create epic
        let epic = service.create_epic(default_epic_input()).await.unwrap();
        assert_eq!(epic.status, EpicStatus::Draft);

        // Create task under epic
        let task = service
            .create_task(default_task_input(epic.id.clone()))
            .await
            .unwrap();
        assert_eq!(task.status, TaskStatus::Todo);
        assert_eq!(task.epic_id, epic.id);

        // Progress task: Todo -> InProgress -> Complete
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
        service
            .update_task(
                &task.id,
                TaskUpdate {
                    status: Some(TaskStatus::Complete),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let completed = service.get_task(&task.id).await.unwrap();
        assert_eq!(completed.status, TaskStatus::Complete);

        // Progress epic: Draft -> InProgress -> Complete
        service
            .update_epic(
                &epic.id,
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        service
            .update_epic(
                &epic.id,
                EpicUpdate {
                    status: Some(EpicStatus::Complete),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let completed_epic = service.get_epic(&epic.id).await.unwrap();
        assert_eq!(completed_epic.status, EpicStatus::Complete);

        // Verify ledger events: epic_created, task_created,
        // task_status_changed x2, epic_status_changed x2
        let evts = events.lock().unwrap();
        assert_eq!(evts.len(), 6);
        let types: Vec<&str> = evts.iter().map(|e| e.event_type.as_str()).collect();
        assert_eq!(
            types,
            vec![
                "epic_created",
                "task_created",
                "task_status_changed",
                "task_status_changed",
                "epic_status_changed",
                "epic_status_changed",
            ]
        );
    }
}
