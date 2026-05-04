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
use crate::types::{AutorunSessionStatus, FormatId};

/// Result of a compare-and-swap (CAS) status transition.
///
/// Used by `update_autorun_session_cas` to communicate whether the conditional
/// update succeeded or was a no-op because the current status did not match
/// the expected status.
#[derive(Debug, Clone)]
pub enum CasResult {
    /// The update was applied successfully. Contains the updated record.
    Updated(Box<AutorunSession>),
    /// The WHERE guard prevented the update (status was not the expected value).
    NoOp,
}

/// Result of a prune operation.
#[derive(Debug, Default, Clone)]
pub struct PruneResult {
    pub sessions_deleted: u64,
    pub workers_deleted: u64,
    pub task_runs_deleted: u64,
    pub failures: Vec<String>,
}

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

    /// Compare-and-swap update: only applies the MERGE if the current status
    /// matches `expected`. Returns `CasResult::Updated` with the new record on
    /// success, or `CasResult::NoOp` if the WHERE guard prevented the write.
    fn update_autorun_session_cas(
        &self,
        id: &str,
        expected: AutorunSessionStatus,
        update: AutorunSessionUpdate,
    ) -> impl std::future::Future<Output = Result<CasResult, DbError>> + Send;

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

    /// Bulk-fetch workers for MANY sessions in a single round trip.
    ///
    /// INF-TSK-048-001 AC #7: The status TUI previously issued N+1 queries
    /// (one `list_autorun_workers` per batch session). For TUIs that display
    /// dozens of batches, this blocks the render thread for seconds on a
    /// cold DB. This method replaces the loop with one `WHERE session_id IN
    /// $ids` query and lets callers group results client-side.
    ///
    /// Returns an empty vec when `session_ids` is empty. Order within a
    /// session is stable (by `worker_num`) but sessions are not partitioned
    /// in the result — callers must group by `session_id`.
    fn list_autorun_workers_bulk(
        &self,
        session_ids: &[String],
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

    /// Prune old interactive sessions (stale/complete) from the database.
    ///
    /// Deletes records whose `completed_at` is before `before`. If `keep_last > 0`,
    /// retains the N most recent terminal sessions regardless of age. Simpler than
    /// autorun prune (no FK cascade -- interactive sessions have no child tables).
    fn prune_interactive_sessions(
        &self,
        before: &str,
        keep_last: usize,
    ) -> impl std::future::Future<Output = Result<PruneResult, DbError>> + Send;

    /// INF-TSK-050-001 AC #14: bulk-mark `interactive_session` rows for
    /// every worker of `autorun_session_id` as complete.
    ///
    /// Workers register themselves as `interactive_session` rows with
    /// `session_kind='autorun'` and `status='active'`. Without this
    /// propagation they remain `active` indefinitely after the
    /// orchestrator returns, polluting the interactive status view.
    ///
    /// Idempotent at the row level: rows already in a non-active
    /// status are skipped via the `WHERE status = 'active'` predicate.
    /// Returns the number of rows updated. Errors should be logged
    /// but not block batch return — a partially-propagated state is
    /// recoverable on the next sweep.
    fn complete_worker_interactive_sessions(
        &self,
        autorun_session_id: &str,
    ) -> impl std::future::Future<Output = Result<usize, DbError>> + Send;

    /// Prune old autorun sessions, workers, and task runs in FK-safe cascade order.
    ///
    /// Deletes records for terminal-status sessions (Completed, Failed, Cancelled, Timeout)
    /// whose `completed_at` is before `before`. If `keep_last > 0`, retains the N most
    /// recent terminal sessions regardless of age.
    fn prune_autorun_sessions(
        &self,
        before: &str,
        keep_last: usize,
    ) -> impl std::future::Future<Output = Result<PruneResult, DbError>> + Send;

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
    async fn apply_schema(&self) -> Result<(), DbError> {
        Ok(())
    }
    async fn check_integrity(&self) -> Result<(), DbError> {
        Ok(())
    }
    async fn create_session(&self, _: &Session) -> Result<(), DbError> {
        Ok(())
    }
    async fn get_session(&self, _: &str) -> Result<Option<Session>, DbError> {
        Ok(None)
    }
    async fn update_session(&self, _: &str, _: SessionUpdate) -> Result<(), DbError> {
        Ok(())
    }
    async fn list_sessions(&self, _: SessionFilter) -> Result<Vec<Session>, DbError> {
        Ok(vec![])
    }
    async fn create_epic(&self, _: &Epic) -> Result<(), DbError> {
        Ok(())
    }
    async fn get_epic(&self, _: &str) -> Result<Option<Epic>, DbError> {
        Ok(None)
    }
    async fn get_epic_by_format_id(&self, _: &FormatId) -> Result<Option<Epic>, DbError> {
        Ok(None)
    }
    async fn update_epic(&self, _: &str, _: EpicUpdate) -> Result<(), DbError> {
        Ok(())
    }
    async fn list_epics(&self, _: EpicFilter) -> Result<Vec<Epic>, DbError> {
        Ok(vec![])
    }
    async fn create_task(&self, _: &Task) -> Result<(), DbError> {
        Ok(())
    }
    async fn get_task(&self, _: &str) -> Result<Option<Task>, DbError> {
        Ok(None)
    }
    async fn get_task_by_format_id(&self, _: &FormatId) -> Result<Option<Task>, DbError> {
        Ok(None)
    }
    async fn update_task(&self, _: &str, _: TaskUpdate) -> Result<(), DbError> {
        Ok(())
    }
    async fn list_tasks(&self, _: TaskFilter) -> Result<Vec<Task>, DbError> {
        Ok(vec![])
    }
    async fn get_active_work(&self) -> Result<Option<ActiveWork>, DbError> {
        Ok(None)
    }
    async fn set_active_work(&self, _: &ActiveWork) -> Result<(), DbError> {
        Ok(())
    }
    async fn clear_active_work(&self, _: &str) -> Result<(), DbError> {
        Ok(())
    }
    async fn create_memory_event(&self, _: &MemoryEvent) -> Result<(), DbError> {
        Ok(())
    }
    async fn list_memory_events(&self, _: MemoryEventFilter) -> Result<Vec<MemoryEvent>, DbError> {
        Ok(vec![])
    }
    async fn create_autorun_session(&self, _: &AutorunSession) -> Result<(), DbError> {
        Ok(())
    }
    async fn get_autorun_session(&self, _: &str) -> Result<Option<AutorunSession>, DbError> {
        Ok(None)
    }
    async fn update_autorun_session(
        &self,
        _: &str,
        _: AutorunSessionUpdate,
    ) -> Result<(), DbError> {
        Ok(())
    }
    async fn update_autorun_session_cas(
        &self,
        _: &str,
        _: AutorunSessionStatus,
        _: AutorunSessionUpdate,
    ) -> Result<CasResult, DbError> {
        Ok(CasResult::NoOp)
    }
    async fn create_autorun_worker(&self, _: &AutorunWorker) -> Result<(), DbError> {
        Ok(())
    }
    async fn update_autorun_worker(&self, _: &str, _: AutorunWorkerUpdate) -> Result<(), DbError> {
        Ok(())
    }
    async fn create_autorun_task_run(&self, _: &AutorunTaskRun) -> Result<(), DbError> {
        Ok(())
    }
    async fn update_autorun_task_run(
        &self,
        _: &str,
        _: AutorunTaskRunUpdate,
    ) -> Result<(), DbError> {
        Ok(())
    }
    async fn list_autorun_sessions(
        &self,
        _: AutorunSessionFilter,
    ) -> Result<Vec<AutorunSession>, DbError> {
        Ok(vec![])
    }
    async fn list_autorun_workers(&self, _: &str) -> Result<Vec<AutorunWorker>, DbError> {
        Ok(vec![])
    }
    async fn list_autorun_workers_bulk(&self, _: &[String]) -> Result<Vec<AutorunWorker>, DbError> {
        Ok(vec![])
    }
    async fn list_autorun_task_runs(&self, _: &str) -> Result<Vec<AutorunTaskRun>, DbError> {
        Ok(vec![])
    }
    async fn get_autorun_worker_by_task_id(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<AutorunWorker>, DbError> {
        Ok(None)
    }
    async fn prune_interactive_sessions(&self, _: &str, _: usize) -> Result<PruneResult, DbError> {
        Ok(PruneResult::default())
    }
    async fn prune_autorun_sessions(&self, _: &str, _: usize) -> Result<PruneResult, DbError> {
        Ok(PruneResult::default())
    }
    async fn complete_worker_interactive_sessions(&self, _: &str) -> Result<usize, DbError> {
        Ok(0)
    }
    async fn query_to_json(&self, _: &str) -> Result<serde_json::Value, DbError> {
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

// ---------------------------------------------------------------------------
// MockStore: HashMap-based DataStore for trait-based testing
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Apply an `AutorunSessionUpdate` to an existing `AutorunSession` row.
    /// Centralizes the touch-semantics for every field so all
    /// MockStore mutators stay aligned with `AutorunSessionUpdate`.
    /// Adding a new field on the update struct only needs one site
    /// here (otherwise the mock silently drops writes — exposed by
    /// INF-TSK-050-001 final_pr_url tests).
    fn apply_session_update(s: &mut AutorunSession, update: AutorunSessionUpdate) {
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
        if let Some(v) = update.tmux_session {
            s.tmux_session = v;
        }
        if let Some(v) = update.stale_reason {
            s.stale_reason = v;
        }
        if let Some(v) = update.target_branch {
            s.target_branch = Some(v);
        }
        if let Some(v) = update.final_pr_url {
            s.final_pr_url = Some(v);
        }
        if let Some(v) = update.current_task_id {
            s.current_task_id = v;
        }
        if let Some(v) = update.current_task_format_id {
            s.current_task_format_id = v;
        }
        if let Some(v) = update.updated_at {
            s.updated_at = Some(v);
        }
        if let Some(v) = update.last_heartbeat_at {
            s.last_heartbeat_at = Some(v);
        }
        if let Some(v) = update.abort_started_at {
            s.abort_started_at = v;
        }
    }

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
                apply_session_update(s, update);
            }
            Ok(())
        }

        async fn update_autorun_session_cas(
            &self,
            id: &str,
            expected: AutorunSessionStatus,
            update: AutorunSessionUpdate,
        ) -> Result<CasResult, DbError> {
            let mut sessions = self.autorun_sessions.lock().unwrap();
            if let Some(s) = sessions.get_mut(id) {
                if s.status != expected {
                    return Ok(CasResult::NoOp);
                }
                apply_session_update(s, update);
                Ok(CasResult::Updated(Box::new(s.clone())))
            } else {
                Ok(CasResult::NoOp)
            }
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
                if let Some(v) = update.pr_merged_at {
                    w.pr_merged_at = Some(v);
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
                if let Some(v) = update.last_phase {
                    r.last_phase = Some(v);
                }
            }
            Ok(())
        }

        async fn list_autorun_sessions(
            &self,
            filter: AutorunSessionFilter,
        ) -> Result<Vec<AutorunSession>, DbError> {
            // INF-TSK-050-001 AC #9: mirror the surreal.rs `since` OR `ids`
            // semantics so tests written against the trait observe identical
            // behavior. An empty `Some(ids)` is treated as "filter inactive"
            // (matches the surreal layer's `is_some_and(non_empty)` guard).
            let ids_active = filter.ids.as_ref().is_some_and(|ids| !ids.is_empty());
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
                    // OR-combine `since` and `ids`: a row matches if either
                    // gate accepts it. When neither is set, this branch is
                    // a no-op.
                    let since_match = filter
                        .since
                        .as_ref()
                        .map(|since| s.created_at.as_str() > since.as_str());
                    let ids_match = if ids_active {
                        Some(filter.ids.as_ref().unwrap().iter().any(|id| id == &s.id))
                    } else {
                        None
                    };
                    match (since_match, ids_match) {
                        (Some(since), Some(ids)) => {
                            if !(since || ids) {
                                return false;
                            }
                        }
                        (Some(since), None) => {
                            if !since {
                                return false;
                            }
                        }
                        (None, Some(ids)) => {
                            if !ids {
                                return false;
                            }
                        }
                        (None, None) => {}
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

        async fn list_autorun_workers_bulk(
            &self,
            session_ids: &[String],
        ) -> Result<Vec<AutorunWorker>, DbError> {
            if session_ids.is_empty() {
                return Ok(Vec::new());
            }
            let wanted: std::collections::HashSet<&str> =
                session_ids.iter().map(String::as_str).collect();
            let lock = self.autorun_workers.lock().unwrap();
            let mut results: Vec<AutorunWorker> = lock
                .values()
                .filter(|w| wanted.contains(w.session_id.as_str()))
                .cloned()
                .collect();
            // Stable order: session_id asc, worker_num asc — matches Surreal
            // ordering so tests can assert on exact vectors.
            results.sort_by(|a, b| {
                a.session_id
                    .cmp(&b.session_id)
                    .then(a.worker_num.cmp(&b.worker_num))
            });
            Ok(results)
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

        async fn prune_interactive_sessions(
            &self,
            _before: &str,
            _keep_last: usize,
        ) -> Result<PruneResult, DbError> {
            // MockStore does not track interactive sessions; return zero counts.
            Ok(PruneResult::default())
        }

        async fn complete_worker_interactive_sessions(
            &self,
            _autorun_session_id: &str,
        ) -> Result<usize, DbError> {
            // MockStore does not track interactive sessions; return zero
            // (no rows to update). Tests that need to assert this method
            // was called should track invocations separately.
            Ok(0)
        }

        async fn prune_autorun_sessions(
            &self,
            before: &str,
            keep_last: usize,
        ) -> Result<PruneResult, DbError> {
            use crate::types::AutorunSessionStatus;

            let terminal = [
                AutorunSessionStatus::Completed,
                AutorunSessionStatus::Failed,
                AutorunSessionStatus::Cancelled,
                AutorunSessionStatus::Timeout,
            ];

            let sessions_lock = self.autorun_sessions.lock().unwrap();
            let mut candidates: Vec<&AutorunSession> = sessions_lock
                .values()
                .filter(|s| {
                    terminal.contains(&s.status)
                        && s.completed_at.as_deref().is_some_and(|ca| ca < before)
                })
                .collect();

            // Sort by completed_at descending to preserve keep_last most recent
            candidates.sort_by(|a, b| {
                b.completed_at
                    .as_deref()
                    .unwrap_or("")
                    .cmp(a.completed_at.as_deref().unwrap_or(""))
            });

            let to_delete: Vec<String> = candidates
                .into_iter()
                .skip(keep_last)
                .map(|s| s.id.clone())
                .collect();
            drop(sessions_lock);

            let mut result = PruneResult::default();

            // FK-safe order: task_runs, workers, sessions
            {
                let mut runs = self.autorun_task_runs.lock().unwrap();
                let before_len = runs.len();
                runs.retain(|_, r| !to_delete.contains(&r.session_id));
                result.task_runs_deleted = (before_len - runs.len()) as u64;
            }
            {
                let mut workers = self.autorun_workers.lock().unwrap();
                let before_len = workers.len();
                workers.retain(|_, w| !to_delete.contains(&w.session_id));
                result.workers_deleted = (before_len - workers.len()) as u64;
            }
            {
                let mut sessions = self.autorun_sessions.lock().unwrap();
                let before_len = sessions.len();
                sessions.retain(|id, _| !to_delete.contains(id));
                result.sessions_deleted = (before_len - sessions.len()) as u64;
            }

            Ok(result)
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
        async fn update_autorun_session_cas(&self, _: &str, _: AutorunSessionStatus, _: AutorunSessionUpdate) -> Result<CasResult, DbError> { Err(DbError::Query("test: forced failure".into())) }
        async fn create_autorun_worker(&self, _: &AutorunWorker) -> Result<(), DbError> { Ok(()) }
        async fn update_autorun_worker(&self, _: &str, _: AutorunWorkerUpdate) -> Result<(), DbError> { Ok(()) }
        async fn create_autorun_task_run(&self, _: &AutorunTaskRun) -> Result<(), DbError> { Ok(()) }
        async fn update_autorun_task_run(&self, _: &str, _: AutorunTaskRunUpdate) -> Result<(), DbError> { Ok(()) }
        async fn list_autorun_sessions(&self, _: AutorunSessionFilter) -> Result<Vec<AutorunSession>, DbError> { Ok(vec![]) }
        async fn list_autorun_workers(&self, _: &str) -> Result<Vec<AutorunWorker>, DbError> { Ok(vec![]) }
        async fn list_autorun_workers_bulk(&self, _: &[String]) -> Result<Vec<AutorunWorker>, DbError> { Ok(vec![]) }
        async fn list_autorun_task_runs(&self, _: &str) -> Result<Vec<AutorunTaskRun>, DbError> { Ok(vec![]) }
        async fn get_autorun_worker_by_task_id(&self, _: &str, _: &str) -> Result<Option<AutorunWorker>, DbError> { Ok(None) }
        async fn prune_interactive_sessions(&self, _: &str, _: usize) -> Result<PruneResult, DbError> { Ok(PruneResult::default()) }
        async fn prune_autorun_sessions(&self, _: &str, _: usize) -> Result<PruneResult, DbError> { Err(DbError::Query("test: forced failure".into())) }
        async fn complete_worker_interactive_sessions(&self, _: &str) -> Result<usize, DbError> { Err(DbError::Query("test: forced failure".into())) }
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
                status: crate::types::AutorunSessionStatus::Running,
                max_session_workers: 2,
                total_tasks: 1,
                completed_tasks: 0,
                failed_tasks: 0,
                pid: None,
                skipped_tasks: 0,
                tmux_session: None,
                stale_reason: None,
                target_branch: None,
                final_pr_url: None,
                current_task_id: None,
                current_task_format_id: None,
                updated_at: None,
                last_heartbeat_at: None,
                created_at: "2026-03-08T00:00:00Z".into(),
                completed_at: None,
                abort_started_at: None,
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
                pr_merged_at: None,
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
                last_phase: None,
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

    fn make_autorun_session(
        id: &str,
        status: crate::types::AutorunSessionStatus,
        batch_name: Option<&str>,
        created_at: &str,
    ) -> AutorunSession {
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
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: created_at.into(),
            completed_at: None,
            abort_started_at: None,
        }
    }

    fn make_autorun_worker(
        id: &str,
        session_id: &str,
        task_id: &str,
        worker_num: i32,
    ) -> AutorunWorker {
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
            pr_merged_at: None,
            started_at: None,
            completed_at: None,
        }
    }

    fn make_autorun_task_run(
        id: &str,
        session_id: &str,
        task_id: &str,
        created_at: &str,
    ) -> AutorunTaskRun {
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
            last_phase: None,
            verification_result: None,
            created_at: created_at.into(),
        }
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_no_filter() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session(
                "s1",
                AutorunSessionStatus::Completed,
                Some("batch-a"),
                "2026-03-10T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "s2",
                AutorunSessionStatus::Running,
                Some("batch-b"),
                "2026-03-12T00:00:00Z",
            ))
            .await
            .unwrap();

        let results = store
            .list_autorun_sessions(AutorunSessionFilter::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        // Descending order by created_at.
        assert!(results[0].created_at >= results[1].created_at);
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_status() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session(
                "s1",
                AutorunSessionStatus::Completed,
                None,
                "2026-03-10T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "s2",
                AutorunSessionStatus::Running,
                None,
                "2026-03-11T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "s3",
                AutorunSessionStatus::Completed,
                None,
                "2026-03-12T00:00:00Z",
            ))
            .await
            .unwrap();

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                status: Some(AutorunSessionStatus::Completed),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .all(|s| s.status == AutorunSessionStatus::Completed)
        );
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_batch_name() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session(
                "s1",
                AutorunSessionStatus::Completed,
                Some("refactor-hooks"),
                "2026-03-10T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "s2",
                AutorunSessionStatus::Completed,
                Some("fix-tests"),
                "2026-03-11T00:00:00Z",
            ))
            .await
            .unwrap();

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                batch_name: Some("hooks".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].batch_name.as_deref(), Some("refactor-hooks"));
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_since() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session(
                "s1",
                AutorunSessionStatus::Completed,
                None,
                "2026-03-05T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "s2",
                AutorunSessionStatus::Completed,
                None,
                "2026-03-15T00:00:00Z",
            ))
            .await
            .unwrap();

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                since: Some("2026-03-10T00:00:00Z".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s2");
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_limit() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        for i in 0..5 {
            store
                .create_autorun_session(&make_autorun_session(
                    &format!("s{i}"),
                    AutorunSessionStatus::Completed,
                    None,
                    &format!("2026-03-{:02}T00:00:00Z", 10 + i),
                ))
                .await
                .unwrap();
        }

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                limit: Some(2),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_all_ignores_limit() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        for i in 0..5 {
            store
                .create_autorun_session(&make_autorun_session(
                    &format!("s{i}"),
                    AutorunSessionStatus::Completed,
                    None,
                    &format!("2026-03-{:02}T00:00:00Z", 10 + i),
                ))
                .await
                .unwrap();
        }

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                limit: Some(2),
                all: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 5);
    }

    // -- INF-TSK-050-001 AC #9: ids filter --

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_ids_subset() {
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        for i in 0..5 {
            store
                .create_autorun_session(&make_autorun_session(
                    &format!("s{i}"),
                    AutorunSessionStatus::Completed,
                    None,
                    &format!("2026-03-{:02}T00:00:00Z", 10 + i),
                ))
                .await
                .unwrap();
        }

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                ids: Some(vec!["s1".to_string(), "s3".to_string()]),
                all: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        let ids: Vec<_> = results.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"s1"));
        assert!(ids.contains(&"s3"));
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_ids_empty_disables_filter() {
        // Empty `Some(vec![])` is treated as "filter inactive" — without
        // this guard the WHERE clause would match nothing.
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        for i in 0..3 {
            store
                .create_autorun_session(&make_autorun_session(
                    &format!("s{i}"),
                    AutorunSessionStatus::Completed,
                    None,
                    &format!("2026-03-{:02}T00:00:00Z", 10 + i),
                ))
                .await
                .unwrap();
        }
        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                ids: Some(Vec::new()),
                all: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 3, "empty ids should return all rows");
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_since_or_ids_union() {
        // INF-TSK-050-001 AC #9: `since` and `ids` are OR-combined so
        // callers can union "recent rows" with "still-on-screen rows".
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session(
                "old-1",
                AutorunSessionStatus::Completed,
                None,
                "2026-01-01T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "old-2",
                AutorunSessionStatus::Completed,
                None,
                "2026-01-02T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "recent",
                AutorunSessionStatus::Completed,
                None,
                "2026-04-01T00:00:00Z",
            ))
            .await
            .unwrap();

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                since: Some("2026-03-15T00:00:00Z".to_string()),
                ids: Some(vec!["old-1".to_string()]),
                all: true,
                ..Default::default()
            })
            .await
            .unwrap();
        // `recent` matches `since`; `old-1` matches `ids`. `old-2`
        // matches neither.
        assert_eq!(results.len(), 2);
        let ids: Vec<_> = results.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"recent"));
        assert!(ids.contains(&"old-1"));
        assert!(!ids.contains(&"old-2"));
    }

    #[tokio::test]
    async fn test_mock_list_autorun_sessions_filter_ids_only_no_since() {
        // ids without `since` returns exactly the listed rows.
        use crate::types::AutorunSessionStatus;
        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session(
                "a",
                AutorunSessionStatus::Completed,
                None,
                "2026-01-01T00:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session(
                "b",
                AutorunSessionStatus::Running,
                None,
                "2026-04-01T00:00:00Z",
            ))
            .await
            .unwrap();

        let results = store
            .list_autorun_sessions(AutorunSessionFilter {
                ids: Some(vec!["a".to_string()]),
                all: true,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "a");
    }

    #[tokio::test]
    async fn test_mock_list_autorun_workers_by_session() {
        let store = MockStore::new();
        store
            .create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1))
            .await
            .unwrap();
        store
            .create_autorun_worker(&make_autorun_worker("w2", "ses-a", "task-2", 2))
            .await
            .unwrap();
        store
            .create_autorun_worker(&make_autorun_worker("w3", "ses-b", "task-3", 1))
            .await
            .unwrap();

        let workers = store.list_autorun_workers("ses-a").await.unwrap();
        assert_eq!(workers.len(), 2);
        assert!(workers.iter().all(|w| w.session_id == "ses-a"));

        let workers_b = store.list_autorun_workers("ses-b").await.unwrap();
        assert_eq!(workers_b.len(), 1);

        let workers_none = store.list_autorun_workers("ses-nope").await.unwrap();
        assert!(workers_none.is_empty());
    }

    #[tokio::test]
    async fn test_mock_list_autorun_workers_bulk_three_sessions_four_workers() {
        // INF-TSK-048-001 AC #7 acceptance test: seed 3 sessions × 4 workers
        // each, bulk-fetch all 12 workers in one call, assert count + correct
        // session_id grouping. Mirrors the real SurrealDB bulk query path
        // (`WHERE session_id INSIDE $ids`) via MockStore's equivalent filter.
        let store = MockStore::new();
        let sessions = ["ses-bulk-a", "ses-bulk-b", "ses-bulk-c"];
        for sid in &sessions {
            for n in 1..=4 {
                let worker_id = format!("w-{sid}-{n}");
                let task_id = format!("task-{sid}-{n}");
                store
                    .create_autorun_worker(&make_autorun_worker(&worker_id, sid, &task_id, n))
                    .await
                    .unwrap();
            }
        }

        let ids: Vec<String> = sessions.iter().map(|s| (*s).to_string()).collect();
        let bulk = store.list_autorun_workers_bulk(&ids).await.unwrap();

        // All 12 workers in one call.
        assert_eq!(bulk.len(), 12, "expected 12 workers, got {}", bulk.len());

        // Every returned row carries one of the requested session_ids — no
        // cross-session bleed.
        for w in &bulk {
            assert!(
                sessions.contains(&w.session_id.as_str()),
                "unexpected session_id {} in bulk result",
                w.session_id
            );
        }

        // Group-by parity: 4 workers per requested session_id.
        for sid in &sessions {
            let count = bulk.iter().filter(|w| w.session_id == *sid).count();
            assert_eq!(
                count, 4,
                "expected 4 workers for session {sid}, got {count}"
            );
        }

        // Empty session_ids → empty result (no round trip).
        let empty = store.list_autorun_workers_bulk(&[]).await.unwrap();
        assert!(empty.is_empty());

        // Subset fetch: pass only 2 of 3 → 8 rows, none from the excluded session.
        let subset: Vec<String> = vec!["ses-bulk-a".into(), "ses-bulk-c".into()];
        let partial = store.list_autorun_workers_bulk(&subset).await.unwrap();
        assert_eq!(partial.len(), 8);
        assert!(partial.iter().all(|w| w.session_id != "ses-bulk-b"));
    }

    #[tokio::test]
    async fn test_mock_list_autorun_task_runs_sorted() {
        let store = MockStore::new();
        store
            .create_autorun_task_run(&make_autorun_task_run(
                "r2",
                "ses-a",
                "task-2",
                "2026-03-10T02:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_task_run(&make_autorun_task_run(
                "r1",
                "ses-a",
                "task-1",
                "2026-03-10T01:00:00Z",
            ))
            .await
            .unwrap();
        store
            .create_autorun_task_run(&make_autorun_task_run(
                "r3",
                "ses-b",
                "task-3",
                "2026-03-10T03:00:00Z",
            ))
            .await
            .unwrap();

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
        store
            .create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1))
            .await
            .unwrap();
        store
            .create_autorun_worker(&make_autorun_worker("w2", "ses-a", "task-2", 2))
            .await
            .unwrap();

        let found = store
            .get_autorun_worker_by_task_id("ses-a", "task-1")
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().task_id, "task-1");
    }

    #[tokio::test]
    async fn test_mock_get_autorun_worker_by_task_id_wrong_session() {
        let store = MockStore::new();
        store
            .create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1))
            .await
            .unwrap();

        let not_found = store
            .get_autorun_worker_by_task_id("ses-b", "task-1")
            .await
            .unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_mock_get_autorun_worker_by_task_id_wrong_task() {
        let store = MockStore::new();
        store
            .create_autorun_worker(&make_autorun_worker("w1", "ses-a", "task-1", 1))
            .await
            .unwrap();

        let not_found = store
            .get_autorun_worker_by_task_id("ses-a", "task-nope")
            .await
            .unwrap();
        assert!(not_found.is_none());
    }

    // -- MockStore prune tests --

    fn make_autorun_session_for_prune(
        id: &str,
        status: crate::types::AutorunSessionStatus,
        completed_at: Option<&str>,
    ) -> AutorunSession {
        AutorunSession {
            id: id.to_string(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("test".into()),
            status,
            max_session_workers: 2,
            total_tasks: 3,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: None,
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: completed_at.map(String::from),
            abort_started_at: None,
        }
    }

    #[tokio::test]
    async fn test_mock_prune_deletes_terminal_sessions() {
        use crate::types::AutorunSessionStatus;

        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session_for_prune(
                "ms-1",
                AutorunSessionStatus::Completed,
                Some("2026-01-05T00:00:00Z"),
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session_for_prune(
                "ms-2",
                AutorunSessionStatus::Running,
                None,
            ))
            .await
            .unwrap();
        store
            .create_autorun_worker(&make_autorun_worker("mw-1", "ms-1", "task-mw1", 1))
            .await
            .unwrap();
        store
            .create_autorun_task_run(&make_autorun_task_run(
                "mtr-1",
                "ms-1",
                "task-mtr1",
                "2026-01-01T00:00:00Z",
            ))
            .await
            .unwrap();

        let result = store
            .prune_autorun_sessions("2026-02-01T00:00:00Z", 0)
            .await
            .unwrap();

        assert_eq!(result.sessions_deleted, 1);
        assert_eq!(result.workers_deleted, 1);
        assert_eq!(result.task_runs_deleted, 1);

        // Running session still exists
        assert!(store.get_autorun_session("ms-2").await.unwrap().is_some());
        // Completed session is gone
        assert!(store.get_autorun_session("ms-1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_mock_prune_respects_keep_last() {
        use crate::types::AutorunSessionStatus;

        let store = MockStore::new();
        store
            .create_autorun_session(&make_autorun_session_for_prune(
                "mkl-1",
                AutorunSessionStatus::Completed,
                Some("2026-01-05T00:00:00Z"),
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session_for_prune(
                "mkl-2",
                AutorunSessionStatus::Completed,
                Some("2026-01-10T00:00:00Z"),
            ))
            .await
            .unwrap();
        store
            .create_autorun_session(&make_autorun_session_for_prune(
                "mkl-3",
                AutorunSessionStatus::Completed,
                Some("2026-01-15T00:00:00Z"),
            ))
            .await
            .unwrap();

        // keep_last=2: only oldest (mkl-1) should be deleted
        let result = store
            .prune_autorun_sessions("2026-02-01T00:00:00Z", 2)
            .await
            .unwrap();

        assert_eq!(result.sessions_deleted, 1);
        assert!(store.get_autorun_session("mkl-1").await.unwrap().is_none());
        assert!(store.get_autorun_session("mkl-2").await.unwrap().is_some());
        assert!(store.get_autorun_session("mkl-3").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_mock_prune_noop_when_empty() {
        let store = MockStore::new();
        let result = store
            .prune_autorun_sessions("2026-12-01T00:00:00Z", 0)
            .await
            .unwrap();

        assert_eq!(result.sessions_deleted, 0);
        assert_eq!(result.workers_deleted, 0);
        assert_eq!(result.task_runs_deleted, 0);
        assert!(result.failures.is_empty());
    }
}
