use crate::error::DbError;
use crate::models::{
    ActiveWork, AutorunSession, AutorunTaskRun, AutorunWorker, Epic, MemoryEvent, Session, Task,
};
use crate::models::{
    AutorunSessionUpdate, AutorunTaskRunUpdate, AutorunWorkerUpdate, EpicFilter, EpicUpdate,
    MemoryEventFilter, SessionFilter, SessionUpdate, TaskFilter, TaskUpdate,
};

/// Result of a sync operation.
#[derive(Debug, Default)]
pub struct SyncResult {
    pub events_processed: u64,
    pub sessions_upserted: u64,
    pub epics_upserted: u64,
    pub tasks_upserted: u64,
    pub memory_events_inserted: u64,
    pub errors_skipped: u64,
}

/// `DataStore` abstracts all persistent storage operations.
///
/// The sole production implementation is `SurrealStore` (surrealkv:// embedded).
/// Test implementations use `SurrealStore` backed by `Surreal<Mem>` (in-memory).
///
/// All methods take `&self` -- implementations handle interior mutability
/// via `SurrealDB`'s internally reference-counted `Surreal` instance.
pub trait DataStore: Send + Sync {
    // -- Schema lifecycle --

    /// Apply the schema (idempotent DEFINE statements). Safe to call on every startup.
    async fn apply_schema(&self) -> Result<(), DbError>;

    /// Check database integrity.
    async fn check_integrity(&self) -> Result<(), DbError>;

    // -- Session CRUD --

    async fn create_session(&self, session: &Session) -> Result<(), DbError>;
    async fn get_session(&self, id: &str) -> Result<Option<Session>, DbError>;
    async fn update_session(&self, id: &str, update: SessionUpdate) -> Result<(), DbError>;
    async fn list_sessions(&self, filter: SessionFilter) -> Result<Vec<Session>, DbError>;

    // -- Epic CRUD --

    async fn create_epic(&self, epic: &Epic) -> Result<(), DbError>;
    async fn get_epic(&self, id: &str) -> Result<Option<Epic>, DbError>;
    async fn get_epic_by_format_id(&self, format_id: &str) -> Result<Option<Epic>, DbError>;
    async fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), DbError>;
    async fn list_epics(&self, filter: EpicFilter) -> Result<Vec<Epic>, DbError>;

    // -- Task CRUD --

    async fn create_task(&self, task: &Task) -> Result<(), DbError>;
    async fn get_task(&self, id: &str) -> Result<Option<Task>, DbError>;
    async fn get_task_by_format_id(&self, format_id: &str) -> Result<Option<Task>, DbError>;
    async fn update_task(&self, id: &str, update: TaskUpdate) -> Result<(), DbError>;
    async fn list_tasks(&self, filter: TaskFilter) -> Result<Vec<Task>, DbError>;

    // -- Active work --

    async fn get_active_work(&self) -> Result<Option<ActiveWork>, DbError>;
    async fn set_active_work(&self, work: &ActiveWork) -> Result<(), DbError>;
    async fn clear_active_work(&self, id: &str) -> Result<(), DbError>;

    // -- Memory events --

    async fn create_memory_event(&self, event: &MemoryEvent) -> Result<(), DbError>;
    async fn list_memory_events(
        &self,
        filter: MemoryEventFilter,
    ) -> Result<Vec<MemoryEvent>, DbError>;

    // -- Autorun --

    async fn create_autorun_session(&self, session: &AutorunSession) -> Result<(), DbError>;
    async fn get_autorun_session(&self, id: &str) -> Result<Option<AutorunSession>, DbError>;
    async fn update_autorun_session(
        &self,
        id: &str,
        update: AutorunSessionUpdate,
    ) -> Result<(), DbError>;

    async fn create_autorun_worker(&self, worker: &AutorunWorker) -> Result<(), DbError>;
    async fn update_autorun_worker(
        &self,
        id: &str,
        update: AutorunWorkerUpdate,
    ) -> Result<(), DbError>;

    async fn create_autorun_task_run(&self, run: &AutorunTaskRun) -> Result<(), DbError>;
    async fn update_autorun_task_run(
        &self,
        id: &str,
        update: AutorunTaskRunUpdate,
    ) -> Result<(), DbError>;

    // -- Generic query --

    /// Execute a read-only query and return results as JSON.
    async fn query_to_json(&self, query: &str) -> Result<serde_json::Value, DbError>;

    // -- Sync --

    /// Rebuild database state from JSONL ledger events.
    async fn sync_from_events(
        &self,
        events: impl Iterator<Item = crate::ledger::Event> + Send,
    ) -> Result<SyncResult, DbError>;
}
