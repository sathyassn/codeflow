pub mod schema;
pub mod surreal;

pub use surreal::{QUERY_TIMEOUT, RetryConfig, SurrealStore, with_retry_async, with_timeout};

use crate::error::DbError;
use crate::models::{
    ActiveWork, AutorunSession, AutorunSessionFilter, AutorunTaskRun, AutorunWorker, Epic,
    MemoryEvent, Session, Task,
};
use crate::models::{
    AutorunSessionUpdate, AutorunTaskRunUpdate, AutorunWorkerUpdate, EpicFilter, EpicUpdate,
    MemoryEventFilter, SessionFilter, SessionUpdate, TaskFilter, TaskUpdate,
};
use crate::types::FormatId;

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
    async fn get_epic_by_format_id(&self, format_id: &FormatId) -> Result<Option<Epic>, DbError>;
    async fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), DbError>;
    async fn list_epics(&self, filter: EpicFilter) -> Result<Vec<Epic>, DbError>;

    // -- Task CRUD --

    async fn create_task(&self, task: &Task) -> Result<(), DbError>;
    async fn get_task(&self, id: &str) -> Result<Option<Task>, DbError>;
    async fn get_task_by_format_id(&self, format_id: &FormatId) -> Result<Option<Task>, DbError>;
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
    // These methods use explicit `impl Future + Send` because they are called
    // from `TmuxWorker::run()` which requires `Send` futures (spawned via tokio::spawn).

    fn create_autorun_session(
        &self,
        session: &AutorunSession,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send;

    fn get_autorun_session(
        &self,
        id: &str,
    ) -> impl std::future::Future<Output = Result<Option<AutorunSession>, DbError>> + Send;

    fn update_autorun_session(
        &self,
        id: &str,
        update: AutorunSessionUpdate,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send;

    fn create_autorun_worker(
        &self,
        worker: &AutorunWorker,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send;

    fn update_autorun_worker(
        &self,
        id: &str,
        update: AutorunWorkerUpdate,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send;

    fn create_autorun_task_run(
        &self,
        run: &AutorunTaskRun,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send;

    fn update_autorun_task_run(
        &self,
        id: &str,
        update: AutorunTaskRunUpdate,
    ) -> impl std::future::Future<Output = Result<(), DbError>> + Send;

    fn list_autorun_sessions(
        &self,
        filter: AutorunSessionFilter,
    ) -> impl std::future::Future<Output = Result<Vec<AutorunSession>, DbError>> + Send;

    fn list_autorun_workers(
        &self,
        session_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<AutorunWorker>, DbError>> + Send;

    fn list_autorun_task_runs(
        &self,
        session_id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<AutorunTaskRun>, DbError>> + Send;

    fn get_autorun_worker_by_task_id(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> impl std::future::Future<Output = Result<Option<AutorunWorker>, DbError>> + Send;

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

// ---------------------------------------------------------------------------
// NoopStore: no-op DataStore fallback when DB is unavailable
// ---------------------------------------------------------------------------

/// A no-op `DataStore` implementation that silently succeeds on all writes
/// and returns empty results on all reads. Used as a fallback when the real
/// database cannot be opened.
pub struct NoopStore;

impl DataStore for NoopStore {
    async fn apply_schema(&self) -> Result<(), DbError> { Ok(()) }
    async fn check_integrity(&self) -> Result<(), DbError> { Ok(()) }
    async fn create_session(&self, _: &Session) -> Result<(), DbError> { Ok(()) }
    async fn get_session(&self, _: &str) -> Result<Option<Session>, DbError> { Ok(None) }
    async fn update_session(&self, _: &str, _: SessionUpdate) -> Result<(), DbError> { Ok(()) }
    async fn list_sessions(&self, _: SessionFilter) -> Result<Vec<Session>, DbError> { Ok(vec![]) }
    async fn create_epic(&self, _: &Epic) -> Result<(), DbError> { Ok(()) }
    async fn get_epic(&self, _: &str) -> Result<Option<Epic>, DbError> { Ok(None) }
    async fn get_epic_by_format_id(&self, _: &FormatId) -> Result<Option<Epic>, DbError> { Ok(None) }
    async fn update_epic(&self, _: &str, _: EpicUpdate) -> Result<(), DbError> { Ok(()) }
    async fn list_epics(&self, _: EpicFilter) -> Result<Vec<Epic>, DbError> { Ok(vec![]) }
    async fn create_task(&self, _: &Task) -> Result<(), DbError> { Ok(()) }
    async fn get_task(&self, _: &str) -> Result<Option<Task>, DbError> { Ok(None) }
    async fn get_task_by_format_id(&self, _: &FormatId) -> Result<Option<Task>, DbError> { Ok(None) }
    async fn update_task(&self, _: &str, _: TaskUpdate) -> Result<(), DbError> { Ok(()) }
    async fn list_tasks(&self, _: TaskFilter) -> Result<Vec<Task>, DbError> { Ok(vec![]) }
    async fn get_active_work(&self) -> Result<Option<ActiveWork>, DbError> { Ok(None) }
    async fn set_active_work(&self, _: &ActiveWork) -> Result<(), DbError> { Ok(()) }
    async fn clear_active_work(&self, _: &str) -> Result<(), DbError> { Ok(()) }
    async fn create_memory_event(&self, _: &MemoryEvent) -> Result<(), DbError> { Ok(()) }
    async fn list_memory_events(&self, _: MemoryEventFilter) -> Result<Vec<MemoryEvent>, DbError> { Ok(vec![]) }
    async fn create_autorun_session(&self, _: &AutorunSession) -> Result<(), DbError> { Ok(()) }
    async fn get_autorun_session(&self, _: &str) -> Result<Option<AutorunSession>, DbError> { Ok(None) }
    async fn update_autorun_session(&self, _: &str, _: AutorunSessionUpdate) -> Result<(), DbError> { Ok(()) }
    async fn create_autorun_worker(&self, _: &AutorunWorker) -> Result<(), DbError> { Ok(()) }
    async fn update_autorun_worker(&self, _: &str, _: AutorunWorkerUpdate) -> Result<(), DbError> { Ok(()) }
    async fn create_autorun_task_run(&self, _: &AutorunTaskRun) -> Result<(), DbError> { Ok(()) }
    async fn update_autorun_task_run(&self, _: &str, _: AutorunTaskRunUpdate) -> Result<(), DbError> { Ok(()) }
    async fn list_autorun_sessions(&self, _: AutorunSessionFilter) -> Result<Vec<AutorunSession>, DbError> { Ok(vec![]) }
    async fn list_autorun_workers(&self, _: &str) -> Result<Vec<AutorunWorker>, DbError> { Ok(vec![]) }
    async fn list_autorun_task_runs(&self, _: &str) -> Result<Vec<AutorunTaskRun>, DbError> { Ok(vec![]) }
    async fn get_autorun_worker_by_task_id(&self, _: &str, _: &str) -> Result<Option<AutorunWorker>, DbError> { Ok(None) }
    async fn query_to_json(&self, _: &str) -> Result<serde_json::Value, DbError> { Ok(serde_json::json!([])) }
    async fn sync_from_events(&self, events: impl Iterator<Item = crate::ledger::Event> + Send) -> Result<SyncResult, DbError> {
        let count = events.count() as u64;
        Ok(SyncResult { events_processed: count, ..Default::default() })
    }
}

// ---------------------------------------------------------------------------
// MockStore: HashMap-based DataStore for trait-based testing
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory `DataStore` implementation for testing trait dispatch.
    ///
    /// Uses `Mutex<HashMap>` for interior mutability (single-threaded test context).
    /// Validates that the `DataStore` trait can be implemented by something other
    /// than `SurrealStore`.
    #[derive(Default)]
    pub struct MockStore {
        sessions: Mutex<HashMap<String, Session>>,
        epics: Mutex<HashMap<String, Epic>>,
        tasks: Mutex<HashMap<String, Task>>,
        active_work: Mutex<Option<ActiveWork>>,
        memory_events: Mutex<Vec<MemoryEvent>>,
        pub autorun_sessions: Mutex<HashMap<String, AutorunSession>>,
        pub autorun_workers: Mutex<HashMap<String, AutorunWorker>>,
        pub autorun_task_runs: Mutex<HashMap<String, AutorunTaskRun>>,
    }

    impl MockStore {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl DataStore for MockStore {
        async fn apply_schema(&self) -> Result<(), DbError> {
            Ok(())
        }

        async fn check_integrity(&self) -> Result<(), DbError> {
            Ok(())
        }

        async fn create_session(&self, session: &Session) -> Result<(), DbError> {
            self.sessions
                .lock()
                .unwrap()
                .insert(session.id.clone(), session.clone());
            Ok(())
        }

        async fn get_session(&self, id: &str) -> Result<Option<Session>, DbError> {
            Ok(self.sessions.lock().unwrap().get(id).cloned())
        }

        async fn update_session(&self, id: &str, update: SessionUpdate) -> Result<(), DbError> {
            if let Some(session) = self.sessions.lock().unwrap().get_mut(id) {
                if let Some(status) = update.status {
                    session.status = status;
                }
                if let Some(ended_at) = update.ended_at {
                    session.ended_at = Some(ended_at);
                }
            }
            Ok(())
        }

        async fn list_sessions(&self, _filter: SessionFilter) -> Result<Vec<Session>, DbError> {
            Ok(self.sessions.lock().unwrap().values().cloned().collect())
        }

        async fn create_epic(&self, epic: &Epic) -> Result<(), DbError> {
            self.epics
                .lock()
                .unwrap()
                .insert(epic.id.clone(), epic.clone());
            Ok(())
        }

        async fn get_epic(&self, id: &str) -> Result<Option<Epic>, DbError> {
            Ok(self.epics.lock().unwrap().get(id).cloned())
        }

        async fn get_epic_by_format_id(
            &self,
            format_id: &FormatId,
        ) -> Result<Option<Epic>, DbError> {
            let fid = format_id.as_str();
            Ok(self
                .epics
                .lock()
                .unwrap()
                .values()
                .find(|e| e.format_id == fid)
                .cloned())
        }

        async fn update_epic(&self, id: &str, update: EpicUpdate) -> Result<(), DbError> {
            if let Some(epic) = self.epics.lock().unwrap().get_mut(id) {
                if let Some(status) = update.status {
                    epic.status = status;
                }
                if let Some(title) = update.title {
                    epic.title = title;
                }
            }
            Ok(())
        }

        async fn list_epics(&self, _filter: EpicFilter) -> Result<Vec<Epic>, DbError> {
            Ok(self.epics.lock().unwrap().values().cloned().collect())
        }

        async fn create_task(&self, task: &Task) -> Result<(), DbError> {
            self.tasks
                .lock()
                .unwrap()
                .insert(task.id.clone(), task.clone());
            Ok(())
        }

        async fn get_task(&self, id: &str) -> Result<Option<Task>, DbError> {
            Ok(self.tasks.lock().unwrap().get(id).cloned())
        }

        async fn get_task_by_format_id(
            &self,
            format_id: &FormatId,
        ) -> Result<Option<Task>, DbError> {
            let fid = format_id.as_str();
            Ok(self
                .tasks
                .lock()
                .unwrap()
                .values()
                .find(|t| t.format_id == fid)
                .cloned())
        }

        async fn update_task(&self, id: &str, update: TaskUpdate) -> Result<(), DbError> {
            if let Some(task) = self.tasks.lock().unwrap().get_mut(id) {
                if let Some(status) = update.status {
                    task.status = status;
                }
            }
            Ok(())
        }

        async fn list_tasks(&self, _filter: TaskFilter) -> Result<Vec<Task>, DbError> {
            Ok(self.tasks.lock().unwrap().values().cloned().collect())
        }

        async fn get_active_work(&self) -> Result<Option<ActiveWork>, DbError> {
            Ok(self.active_work.lock().unwrap().clone())
        }

        async fn set_active_work(&self, work: &ActiveWork) -> Result<(), DbError> {
            *self.active_work.lock().unwrap() = Some(work.clone());
            Ok(())
        }

        async fn clear_active_work(&self, _id: &str) -> Result<(), DbError> {
            *self.active_work.lock().unwrap() = None;
            Ok(())
        }

        async fn create_memory_event(&self, event: &MemoryEvent) -> Result<(), DbError> {
            self.memory_events.lock().unwrap().push(event.clone());
            Ok(())
        }

        async fn list_memory_events(
            &self,
            _filter: MemoryEventFilter,
        ) -> Result<Vec<MemoryEvent>, DbError> {
            Ok(self.memory_events.lock().unwrap().clone())
        }

        async fn create_autorun_session(&self, session: &AutorunSession) -> Result<(), DbError> {
            self.autorun_sessions
                .lock()
                .unwrap()
                .insert(session.id.clone(), session.clone());
            Ok(())
        }

        async fn get_autorun_session(&self, id: &str) -> Result<Option<AutorunSession>, DbError> {
            Ok(self.autorun_sessions.lock().unwrap().get(id).cloned())
        }

        async fn update_autorun_session(
            &self,
            id: &str,
            update: AutorunSessionUpdate,
        ) -> Result<(), DbError> {
            if let Some(s) = self.autorun_sessions.lock().unwrap().get_mut(id) {
                if let Some(status) = update.status {
                    s.status = status;
                }
                if let Some(v) = update.completed_tasks {
                    s.completed_tasks = v;
                }
                if let Some(v) = update.failed_tasks {
                    s.failed_tasks = v;
                }
                if let Some(v) = update.skipped_tasks {
                    s.skipped_tasks = v;
                }
                if let Some(v) = update.completed_at {
                    s.completed_at = Some(v);
                }
            }
            Ok(())
        }

        async fn create_autorun_worker(&self, worker: &AutorunWorker) -> Result<(), DbError> {
            self.autorun_workers
                .lock()
                .unwrap()
                .insert(worker.id.clone(), worker.clone());
            Ok(())
        }

        async fn update_autorun_worker(
            &self,
            id: &str,
            update: AutorunWorkerUpdate,
        ) -> Result<(), DbError> {
            if let Some(w) = self.autorun_workers.lock().unwrap().get_mut(id) {
                if let Some(status) = update.status {
                    w.status = status;
                }
                if let Some(v) = update.completed_at {
                    w.completed_at = Some(v);
                }
                if let Some(v) = update.pr_number {
                    w.pr_number = Some(v);
                }
                if let Some(v) = update.worker_session_id {
                    w.worker_session_id = Some(v);
                }
            }
            Ok(())
        }

        async fn create_autorun_task_run(&self, run: &AutorunTaskRun) -> Result<(), DbError> {
            self.autorun_task_runs
                .lock()
                .unwrap()
                .insert(run.id.clone(), run.clone());
            Ok(())
        }

        async fn update_autorun_task_run(
            &self,
            id: &str,
            update: AutorunTaskRunUpdate,
        ) -> Result<(), DbError> {
            if let Some(r) = self.autorun_task_runs.lock().unwrap().get_mut(id) {
                if let Some(status) = update.status {
                    r.status = status;
                }
                if let Some(v) = update.completed_at {
                    r.completed_at = Some(v);
                }
                if let Some(v) = update.exit_code {
                    r.exit_code = Some(v);
                }
                if let Some(v) = update.error_message {
                    r.error_message = Some(v);
                }
                if let Some(v) = update.merge_conflicts {
                    r.merge_conflicts = Some(v);
                }
                if let Some(v) = update.pr_number {
                    r.pr_number = Some(v);
                }
                if let Some(v) = update.pr_url {
                    r.pr_url = Some(v);
                }
            }
            Ok(())
        }

        async fn list_autorun_sessions(
            &self,
            filter: AutorunSessionFilter,
        ) -> Result<Vec<AutorunSession>, DbError> {
            let lock = self.autorun_sessions.lock().unwrap();
            let mut results: Vec<AutorunSession> = lock
                .values()
                .filter(|s| {
                    if let Some(ref status) = filter.status {
                        if s.status != *status {
                            return false;
                        }
                    }
                    if let Some(ref name) = filter.batch_name {
                        if let Some(ref bn) = s.batch_name {
                            if !bn.contains(name.as_str()) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                    if let Some(ref since) = filter.since {
                        if s.created_at.as_str() < since.as_str() {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            if !filter.all {
                if let Some(limit) = filter.limit {
                    results.truncate(limit as usize);
                }
            }
            Ok(results)
        }

        async fn list_autorun_workers(
            &self,
            session_id: &str,
        ) -> Result<Vec<AutorunWorker>, DbError> {
            let lock = self.autorun_workers.lock().unwrap();
            Ok(lock
                .values()
                .filter(|w| w.session_id == session_id)
                .cloned()
                .collect())
        }

        async fn list_autorun_task_runs(
            &self,
            session_id: &str,
        ) -> Result<Vec<AutorunTaskRun>, DbError> {
            let lock = self.autorun_task_runs.lock().unwrap();
            let mut results: Vec<AutorunTaskRun> = lock
                .values()
                .filter(|r| r.session_id == session_id)
                .cloned()
                .collect();
            results.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            Ok(results)
        }

        async fn get_autorun_worker_by_task_id(
            &self,
            session_id: &str,
            task_id: &str,
        ) -> Result<Option<AutorunWorker>, DbError> {
            let lock = self.autorun_workers.lock().unwrap();
            Ok(lock
                .values()
                .find(|w| w.session_id == session_id && w.task_id == task_id)
                .cloned())
        }

        async fn query_to_json(&self, _query: &str) -> Result<serde_json::Value, DbError> {
            Ok(serde_json::json!([]))
        }

        async fn sync_from_events(
            &self,
            events: impl Iterator<Item = crate::ledger::Event> + Send,
        ) -> Result<SyncResult, DbError> {
            let count = events.count() as u64;
            Ok(SyncResult {
                events_processed: count,
                ..Default::default()
            })
        }
    }

    /// DataStore that returns Err from `create_autorun_session` and
    /// `update_autorun_session`. All other methods delegate to `NoopStore`.
    /// Used to test store error warning paths in orchestrator.
    pub struct FailingAutorunStore;
    #[rustfmt::skip]
    impl DataStore for FailingAutorunStore {
        async fn apply_schema(&self) -> Result<(), DbError> { Ok(()) }
        async fn check_integrity(&self) -> Result<(), DbError> { Ok(()) }
        async fn create_session(&self, _: &Session) -> Result<(), DbError> { Ok(()) }
        async fn get_session(&self, _: &str) -> Result<Option<Session>, DbError> { Ok(None) }
        async fn update_session(&self, _: &str, _: SessionUpdate) -> Result<(), DbError> { Ok(()) }
        async fn list_sessions(&self, _: SessionFilter) -> Result<Vec<Session>, DbError> { Ok(vec![]) }
        async fn create_epic(&self, _: &Epic) -> Result<(), DbError> { Ok(()) }
        async fn get_epic(&self, _: &str) -> Result<Option<Epic>, DbError> { Ok(None) }
        async fn get_epic_by_format_id(&self, _: &FormatId) -> Result<Option<Epic>, DbError> { Ok(None) }
        async fn update_epic(&self, _: &str, _: EpicUpdate) -> Result<(), DbError> { Ok(()) }
        async fn list_epics(&self, _: EpicFilter) -> Result<Vec<Epic>, DbError> { Ok(vec![]) }
        async fn create_task(&self, _: &Task) -> Result<(), DbError> { Ok(()) }
        async fn get_task(&self, _: &str) -> Result<Option<Task>, DbError> { Ok(None) }
        async fn get_task_by_format_id(&self, _: &FormatId) -> Result<Option<Task>, DbError> { Ok(None) }
        async fn update_task(&self, _: &str, _: TaskUpdate) -> Result<(), DbError> { Ok(()) }
        async fn list_tasks(&self, _: TaskFilter) -> Result<Vec<Task>, DbError> { Ok(vec![]) }
        async fn get_active_work(&self) -> Result<Option<ActiveWork>, DbError> { Ok(None) }
        async fn set_active_work(&self, _: &ActiveWork) -> Result<(), DbError> { Ok(()) }
        async fn clear_active_work(&self, _: &str) -> Result<(), DbError> { Ok(()) }
        async fn create_memory_event(&self, _: &MemoryEvent) -> Result<(), DbError> { Ok(()) }
        async fn list_memory_events(&self, _: MemoryEventFilter) -> Result<Vec<MemoryEvent>, DbError> { Ok(vec![]) }
        async fn create_autorun_session(&self, _: &AutorunSession) -> Result<(), DbError> { Err(DbError::Query("test: forced failure".into())) }
        async fn get_autorun_session(&self, _: &str) -> Result<Option<AutorunSession>, DbError> { Ok(None) }
        async fn update_autorun_session(&self, _: &str, _: AutorunSessionUpdate) -> Result<(), DbError> { Err(DbError::Query("test: forced failure".into())) }
        async fn create_autorun_worker(&self, _: &AutorunWorker) -> Result<(), DbError> { Ok(()) }
        async fn update_autorun_worker(&self, _: &str, _: AutorunWorkerUpdate) -> Result<(), DbError> { Ok(()) }
        async fn create_autorun_task_run(&self, _: &AutorunTaskRun) -> Result<(), DbError> { Ok(()) }
        async fn update_autorun_task_run(&self, _: &str, _: AutorunTaskRunUpdate) -> Result<(), DbError> { Ok(()) }
        async fn list_autorun_sessions(&self, _: AutorunSessionFilter) -> Result<Vec<AutorunSession>, DbError> { Ok(vec![]) }
        async fn list_autorun_workers(&self, _: &str) -> Result<Vec<AutorunWorker>, DbError> { Ok(vec![]) }
        async fn list_autorun_task_runs(&self, _: &str) -> Result<Vec<AutorunTaskRun>, DbError> { Ok(vec![]) }
        async fn get_autorun_worker_by_task_id(&self, _: &str, _: &str) -> Result<Option<AutorunWorker>, DbError> { Ok(None) }
        async fn query_to_json(&self, _: &str) -> Result<serde_json::Value, DbError> { Ok(serde_json::json!([])) }
        async fn sync_from_events(&self, e: impl Iterator<Item = crate::ledger::Event> + Send) -> Result<SyncResult, DbError> { Ok(SyncResult { events_processed: e.count() as u64, ..Default::default() }) }
    }

    // -- MockStore tests (criterion 9) --

    /// Helper: exercises DataStore trait methods via generic function.
    async fn session_roundtrip(store: &impl DataStore) {
        use crate::types::SessionStatus;

        let session = Session {
            id: "mock-ses-1".into(),
            project_id: None,
            user_id: "user-1".into(),
            user_host: "localhost".into(),
            machine_fingerprint: None,
            started_at: "2026-03-08T00:00:00Z".into(),
            ended_at: None,
            duration_seconds: None,
            status: SessionStatus::Active,
            work_ids: vec![],
            previous_session_id: None,
            context_summary: None,
            tool_stats: serde_json::Value::Null,
            metadata: serde_json::Value::Null,
        };

        store.create_session(&session).await.unwrap();
        let fetched = store.get_session("mock-ses-1").await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().user_id, "user-1");
    }

    #[tokio::test]
    async fn test_mock_store_session_roundtrip() {
        let store = MockStore::new();
        session_roundtrip(&store).await;
    }

    #[tokio::test]
    async fn test_mock_store_epic_by_format_id() {
        use crate::types::{AreaType, EpicStatus, WorkType};

        let store = MockStore::new();
        let epic = Epic {
            id: "epic-001".into(),
            format_id: "INF-EPC-001".into(),
            title: "Mock Epic".into(),
            summary: None,
            status: EpicStatus::Draft,
            area_type: AreaType::Inf,
            work_type: WorkType::Feat,
            domain: "infrastructure".into(),
            is_ongoing: false,
            file_scope: vec![],
            priority: "medium".into(),
            pr_number: None,
            external_id: None,
            external_url: None,
            created_at: "2026-03-08T00:00:00Z".into(),
            updated_at: "2026-03-08T00:00:00Z".into(),
        };

        store.create_epic(&epic).await.unwrap();

        // Lookup by FormatId newtype
        let fmt = FormatId::new_unchecked("INF-EPC-001");
        let found = store.get_epic_by_format_id(&fmt).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "Mock Epic");

        // Non-existent format_id returns None
        let missing_fmt = FormatId::new_unchecked("INF-EPC-999");
        let not_found = store.get_epic_by_format_id(&missing_fmt).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_mock_store_session_update() {
        use crate::types::SessionStatus;

        let store = MockStore::new();
        let session = Session {
            id: "ses-upd".into(),
            project_id: None,
            user_id: "user-1".into(),
            user_host: "localhost".into(),
            machine_fingerprint: None,
            started_at: "2026-03-08T00:00:00Z".into(),
            ended_at: None,
            duration_seconds: None,
            status: SessionStatus::Active,
            work_ids: vec![],
            previous_session_id: None,
            context_summary: None,
            tool_stats: serde_json::Value::Null,
            metadata: serde_json::Value::Null,
        };
        store.create_session(&session).await.unwrap();

        // Update status and ended_at
        store
            .update_session(
                "ses-upd",
                SessionUpdate {
                    status: Some(SessionStatus::Ended),
                    ended_at: Some("2026-03-08T01:00:00Z".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let updated = store.get_session("ses-upd").await.unwrap().unwrap();
        assert_eq!(updated.status, SessionStatus::Ended);
        assert_eq!(updated.ended_at.as_deref(), Some("2026-03-08T01:00:00Z"));
    }

    #[tokio::test]
    async fn test_mock_store_session_update_nonexistent() {
        let store = MockStore::new();
        // Updating non-existent session should succeed silently
        store
            .update_session(
                "no-such-id",
                SessionUpdate {
                    status: Some(crate::types::SessionStatus::Ended),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_mock_store_session_get_nonexistent() {
        let store = MockStore::new();
        let result = store.get_session("no-such-id").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_mock_store_list_sessions() {
        use crate::types::SessionStatus;

        let store = MockStore::new();
        let s1 = Session {
            id: "ses-1".into(),
            project_id: None,
            user_id: "u1".into(),
            user_host: "h1".into(),
            machine_fingerprint: None,
            started_at: "2026-03-08T00:00:00Z".into(),
            ended_at: None,
            duration_seconds: None,
            status: SessionStatus::Active,
            work_ids: vec![],
            previous_session_id: None,
            context_summary: None,
            tool_stats: serde_json::Value::Null,
            metadata: serde_json::Value::Null,
        };
        let s2 = Session {
            id: "ses-2".into(),
            ..s1.clone()
        };
        store.create_session(&s1).await.unwrap();
        store.create_session(&s2).await.unwrap();

        let all = store.list_sessions(SessionFilter::default()).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_store_epic_crud() {
        use crate::types::{AreaType, EpicStatus, WorkType};

        let store = MockStore::new();
        let epic = Epic {
            id: "epic-crud".into(),
            format_id: "INF-EPC-010".into(),
            title: "CRUD Test Epic".into(),
            summary: None,
            status: EpicStatus::Draft,
            area_type: AreaType::Inf,
            work_type: WorkType::Feat,
            domain: "infrastructure".into(),
            is_ongoing: false,
            file_scope: vec![],
            priority: "medium".into(),
            pr_number: None,
            external_id: None,
            external_url: None,
            created_at: "2026-03-08T00:00:00Z".into(),
            updated_at: "2026-03-08T00:00:00Z".into(),
        };
        store.create_epic(&epic).await.unwrap();

        // Get by id
        let fetched = store.get_epic("epic-crud").await.unwrap().unwrap();
        assert_eq!(fetched.title, "CRUD Test Epic");

        // Update
        store
            .update_epic(
                "epic-crud",
                EpicUpdate {
                    status: Some(EpicStatus::InProgress),
                    title: Some("Updated Epic".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let updated = store.get_epic("epic-crud").await.unwrap().unwrap();
        assert_eq!(updated.status, EpicStatus::InProgress);
        assert_eq!(updated.title, "Updated Epic");

        // List
        let all = store.list_epics(EpicFilter::default()).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_store_epic_update_nonexistent() {
        let store = MockStore::new();
        store
            .update_epic(
                "no-epic",
                EpicUpdate {
                    title: Some("nope".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_mock_store_task_crud() {
        use crate::types::{AreaType, TaskStatus, WorkStage, WorkType};

        let store = MockStore::new();
        let task = Task {
            id: "task-crud".into(),
            format_id: "INF-TSK-010-001".into(),
            epic_id: "epic-1".into(),
            title: "CRUD Test Task".into(),
            description: None,
            status: TaskStatus::Todo,
            area_type: AreaType::Inf,
            work_type: WorkType::Feat,
            domain: "infrastructure".into(),
            origin: "adhoc".into(),
            file_scope: vec![],
            scope_policy: "soft".into(),
            scope_root: None,
            estimate: None,
            priority: "medium".into(),
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
            created_at: "2026-03-08T00:00:00Z".into(),
            updated_at: "2026-03-08T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
            stage: Some(WorkStage::WsDev),
            stage_status: Some("pending".into()),
            stage_history: vec![],
        };
        store.create_task(&task).await.unwrap();

        // Get by id
        let fetched = store.get_task("task-crud").await.unwrap().unwrap();
        assert_eq!(fetched.title, "CRUD Test Task");

        // Get by format_id
        let fmt = FormatId::new_unchecked("INF-TSK-010-001");
        let by_fmt = store.get_task_by_format_id(&fmt).await.unwrap().unwrap();
        assert_eq!(by_fmt.id, "task-crud");

        // Missing format_id
        let missing = FormatId::new_unchecked("ZZZ-TSK-999-999");
        assert!(
            store
                .get_task_by_format_id(&missing)
                .await
                .unwrap()
                .is_none()
        );

        // Update
        store
            .update_task(
                "task-crud",
                TaskUpdate {
                    status: Some(TaskStatus::InProgress),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let updated = store.get_task("task-crud").await.unwrap().unwrap();
        assert_eq!(updated.status, TaskStatus::InProgress);

        // List
        let all = store.list_tasks(TaskFilter::default()).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_store_task_update_nonexistent() {
        let store = MockStore::new();
        store
            .update_task(
                "no-task",
                TaskUpdate {
                    status: Some(crate::types::TaskStatus::Complete),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_mock_store_active_work() {
        use crate::types::ActiveWorkStatus;

        let store = MockStore::new();

        // Initially empty
        assert!(store.get_active_work().await.unwrap().is_none());

        let work = ActiveWork {
            id: "aw-1".into(),
            task_id: Some("task-1".into()),
            topic: "Test topic".into(),
            status: ActiveWorkStatus::InProgress,
            branch: Some("feat/test".into()),
            scope: vec!["src/".into()],
            deliverables: vec!["feature".into()],
            agent: None,
            session_id: Some("ses-1".into()),
            current_stage: None,
            team_name: None,
            created_at: "2026-03-08T00:00:00Z".into(),
            updated_at: "2026-03-08T00:00:00Z".into(),
        };
        store.set_active_work(&work).await.unwrap();

        let fetched = store.get_active_work().await.unwrap().unwrap();
        assert_eq!(fetched.task_id, Some("task-1".into()));

        // Clear
        store.clear_active_work("aw-1").await.unwrap();
        assert!(store.get_active_work().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_store_memory_events() {
        let store = MockStore::new();

        let event = MemoryEvent {
            id: "mem-1".into(),
            event_type: "test".into(),
            domain: "testing".into(),
            work_id: None,
            data: "some data".into(),
            memory_type: None,
            created_at: "2026-03-08T00:00:00Z".into(),
        };
        store.create_memory_event(&event).await.unwrap();

        let events = store
            .list_memory_events(MemoryEventFilter::default())
            .await
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].domain, "testing");
    }

    #[tokio::test]
    async fn test_mock_store_autorun_stubs() {
        let store = MockStore::new();

        // These are stub implementations returning Ok(()) / Ok(None)
        store
            .create_autorun_session(&AutorunSession {
                id: "ar-1".into(),
                batch_file: "batch.json".into(),
                batch_name: None,
                status: crate::types::AutorunSessionStatus::Pending,
                max_session_workers: 2,
                total_tasks: 1,
                completed_tasks: 0,
                failed_tasks: 0,
                pid: None,
                skipped_tasks: 0,
                created_at: "2026-03-08T00:00:00Z".into(),
                completed_at: None,
            })
            .await
            .unwrap();

        let fetched = store.get_autorun_session("ar-1").await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().total_tasks, 1);

        store
            .update_autorun_session("ar-1", AutorunSessionUpdate::default())
            .await
            .unwrap();

        store
            .create_autorun_worker(&AutorunWorker {
                id: "aw-1".into(),
                session_id: "ar-1".into(),
                worker_num: 0,
                task_id: "t-1".into(),
                status: crate::types::AutorunWorkerStatus::Queued,
                tmux_session: None,
                worktree_path: None,
                file_scope: vec![],
                scope_policy: "soft".into(),
                worker_session_id: None,
                pr_number: None,
                started_at: None,
                completed_at: None,
            })
            .await
            .unwrap();

        store
            .update_autorun_worker("aw-1", AutorunWorkerUpdate::default())
            .await
            .unwrap();

        store
            .create_autorun_task_run(&AutorunTaskRun {
                id: "atr-1".into(),
                worker_id: "aw-1".into(),
                task_id: "t-1".into(),
                session_id: "ar-1".into(),
                status: crate::types::AutorunTaskRunStatus::Pending,
                branch_name: None,
                worktree_path: None,
                pr_number: None,
                pr_url: None,
                blocked_reason: None,
                claim_conflicts: None,
                merge_conflicts: None,
                started_at: None,
                completed_at: None,
                duration_seconds: None,
                exit_code: None,
                error_message: None,
                verification_result: None,
                created_at: "2026-03-08T00:00:00Z".into(),
            })
            .await
            .unwrap();

        store
            .update_autorun_task_run("atr-1", AutorunTaskRunUpdate::default())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_mock_store_query_to_json() {
        let store = MockStore::new();
        let result = store.query_to_json("SELECT * FROM sessions").await.unwrap();
        assert_eq!(result, serde_json::json!([]));
    }

    #[tokio::test]
    async fn test_mock_store_sync_from_events() {
        let store = MockStore::new();
        let events = std::iter::empty::<crate::ledger::Event>();
        let result = store.sync_from_events(events).await.unwrap();
        assert_eq!(result.events_processed, 0);
    }

    #[tokio::test]
    async fn test_mock_store_apply_schema_and_integrity() {
        let store = MockStore::new();
        store.apply_schema().await.unwrap();
        store.check_integrity().await.unwrap();
    }

    // -- MockStore autorun list/query tests --

    fn make_autorun_session(id: &str, status: crate::types::AutorunSessionStatus, batch_name: Option<&str>, created_at: &str) -> AutorunSession {
        AutorunSession {
            id: id.into(),
            batch_file: "b.yaml".into(),
            batch_name: batch_name.map(Into::into),
            status,
            max_session_workers: 2,
            total_tasks: 5,
            completed_tasks: 3,
            failed_tasks: 1,
            pid: None,
            skipped_tasks: 0,
            created_at: created_at.into(),
            completed_at: None,
        }
    }

    fn make_autorun_worker(id: &str, session_id: &str, task_id: &str, worker_num: i32) -> AutorunWorker {
        AutorunWorker {
            id: id.into(),
            session_id: session_id.into(),
            worker_num,
            task_id: task_id.into(),
            status: crate::types::AutorunWorkerStatus::Running,
            tmux_session: Some(format!("tmux-{id}")),
            worktree_path: None,
            file_scope: vec![],
            scope_policy: "soft".into(),
            worker_session_id: None,
            pr_number: None,
            started_at: None,
            completed_at: None,
        }
    }

    fn make_autorun_task_run(id: &str, session_id: &str, task_id: &str, created_at: &str) -> AutorunTaskRun {
        AutorunTaskRun {
            id: id.into(),
            worker_id: format!("w-{id}"),
            task_id: task_id.into(),
            session_id: session_id.into(),
            status: crate::types::AutorunTaskRunStatus::Completed,
            branch_name: None,
            worktree_path: None,
            pr_number: None,
            pr_url: None,
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: None,
            completed_at: None,
            duration_seconds: None,
            exit_code: None,
            error_message: None,
            verification_result: None,
            created_at: created_at.into(),
        }
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_no_filter() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store.create_autorun_session(&make_autorun_session("s1", AutorunSessionStatus::Completed, Some("batch-a"), "2026-03-10T00:00:00Z")).await.unwrap();
        store.create_autorun_session(&make_autorun_session("s2", AutorunSessionStatus::Running, Some("batch-b"), "2026-03-12T00:00:00Z")).await.unwrap();

        let results = store.list_autorun_sessions(AutorunSessionFilter::default()).await.unwrap();
        assert_eq!(results.len(), 2);
        // Descending order by created_at.
        assert!(results[0].created_at >= results[1].created_at);
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_status() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store.create_autorun_session(&make_autorun_session("s1", AutorunSessionStatus::Completed, None, "2026-03-10T00:00:00Z")).await.unwrap();
        store.create_autorun_session(&make_autorun_session("s2", AutorunSessionStatus::Running, None, "2026-03-11T00:00:00Z")).await.unwrap();
        store.create_autorun_session(&make_autorun_session("s3", AutorunSessionStatus::Completed, None, "2026-03-12T00:00:00Z")).await.unwrap();

        let results = store.list_autorun_sessions(AutorunSessionFilter {
            status: Some(AutorunSessionStatus::Completed),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|s| s.status == AutorunSessionStatus::Completed));
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_batch_name() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store.create_autorun_session(&make_autorun_session("s1", AutorunSessionStatus::Completed, Some("refactor-hooks"), "2026-03-10T00:00:00Z")).await.unwrap();
        store.create_autorun_session(&make_autorun_session("s2", AutorunSessionStatus::Completed, Some("fix-tests"), "2026-03-11T00:00:00Z")).await.unwrap();

        let results = store.list_autorun_sessions(AutorunSessionFilter {
            batch_name: Some("hooks".into()),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].batch_name.as_deref(), Some("refactor-hooks"));
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_since() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store.create_autorun_session(&make_autorun_session("s1", AutorunSessionStatus::Completed, None, "2026-03-05T00:00:00Z")).await.unwrap();
        store.create_autorun_session(&make_autorun_session("s2", AutorunSessionStatus::Completed, None, "2026-03-15T00:00:00Z")).await.unwrap();

        let results = store.list_autorun_sessions(AutorunSessionFilter {
            since: Some("2026-03-10T00:00:00Z".into()),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s2");
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_limit() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        for i in 0..5 {
            store.create_autorun_session(&make_autorun_session(&format!("s{i}"), AutorunSessionStatus::Completed, None, &format!("2026-03-{:02}T00:00:00Z", 10 + i))).await.unwrap();
        }

        let results = store.list_autorun_sessions(AutorunSessionFilter {
            limit: Some(2),
            ..Default::default()
        }).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_all_ignores_limit() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        for i in 0..5 {
            store.create_autorun_session(&make_autorun_session(&format!("s{i}"), AutorunSessionStatus::Completed, None, &format!("2026-03-{:02}T00:00:00Z", 10 + i))).await.unwrap();
        }

        let results = store.list_autorun_sessions(AutorunSessionFilter {
            limit: Some(2),
            all: true,
            ..Default::default()
        }).await.unwrap();
        assert_eq!(results.len(), 5);
    }

    #[tokio::test]
    async fn test_mock_list_autorun_workers_by_session() {
        let store = MockStore::new();
        store.create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1)).await.unwrap();
        store.create_autorun_worker(&make_autorun_worker("w2", "ses-a", "task-2", 2)).await.unwrap();
        store.create_autorun_worker(&make_autorun_worker("w3", "ses-b", "task-3", 1)).await.unwrap();

        let workers = store.list_autorun_workers("ses-a").await.unwrap();
        assert_eq!(workers.len(), 2);
        assert!(workers.iter().all(|w| w.session_id == "ses-a"));

        let workers_b = store.list_autorun_workers("ses-b").await.unwrap();
        assert_eq!(workers_b.len(), 1);

        let workers_none = store.list_autorun_workers("ses-nope").await.unwrap();
        assert!(workers_none.is_empty());
    }

    #[tokio::test]
    async fn test_mock_list_autorun_task_runs_sorted() {
        let store = MockStore::new();
        store.create_autorun_task_run(&make_autorun_task_run("r2", "ses-a", "task-2", "2026-03-10T02:00:00Z")).await.unwrap();
        store.create_autorun_task_run(&make_autorun_task_run("r1", "ses-a", "task-1", "2026-03-10T01:00:00Z")).await.unwrap();
        store.create_autorun_task_run(&make_autorun_task_run("r3", "ses-b", "task-3", "2026-03-10T03:00:00Z")).await.unwrap();

        let runs = store.list_autorun_task_runs("ses-a").await.unwrap();
        assert_eq!(runs.len(), 2);
        // Sorted ascending by created_at.
        assert!(runs[0].created_at <= runs[1].created_at);

        let runs_b = store.list_autorun_task_runs("ses-b").await.unwrap();
        assert_eq!(runs_b.len(), 1);

        let runs_none = store.list_autorun_task_runs("ses-nope").await.unwrap();
        assert!(runs_none.is_empty());
    }

    #[tokio::test]
    async fn test_mock_get_autorun_worker_by_task_id_found() {
        let store = MockStore::new();
        store.create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1)).await.unwrap();
        store.create_autorun_worker(&make_autorun_worker("w2", "ses-a", "task-2", 2)).await.unwrap();

        let found = store.get_autorun_worker_by_task_id("ses-a", "task-1").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().task_id, "task-1");
    }

    #[tokio::test]
    async fn test_mock_get_autorun_worker_by_task_id_wrong_session() {
        let store = MockStore::new();
        store.create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1)).await.unwrap();

        let not_found = store.get_autorun_worker_by_task_id("ses-b", "task-1").await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_mock_get_autorun_worker_by_task_id_wrong_task() {
        let store = MockStore::new();
        store.create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1)).await.unwrap();

        let not_found = store.get_autorun_worker_by_task_id("ses-a", "task-nope").await.unwrap();
        assert!(not_found.is_none());
    }
}
