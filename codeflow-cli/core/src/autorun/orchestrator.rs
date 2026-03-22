//! Orchestrator for dependency-aware concurrent task execution.
//!
//! Uses `tokio::sync::Semaphore` for concurrency control and Kahn's
//! algorithm output for task ordering.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{Mutex, Semaphore};

use crate::error::AutorunError;

use super::batch::ParsedBatch;
use super::worker::WorkerRunner;

/// Configuration for a single worker execution.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub session_id: String,
    pub worker_id: String,
    pub worker_num: usize,
    pub task_id: String,
    pub batch_name: String,
    pub auto_merge: bool,
    pub target: String,
    pub tmux_prefix: String,
    /// File patterns this worker claims for exclusive access via Loro.
    pub file_scope: Vec<String>,
    /// Scope policy for this worker: "soft", "hard", or "permissive".
    /// Defaults to "soft" when not specified in the task definition.
    pub scope_policy: String,
}

/// Result of a worker execution.
#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub worker_id: String,
    pub task_id: String,
    pub status: String,
    pub exit_code: i32,
    pub pr_number: i64,
    pub pr_url: String,
    pub error: String,
    pub branch_name: String,
    pub duration_sec: i64,
}

/// Manages task sequencing and worker lifecycle using semaphore-based
/// concurrency control.
pub struct Orchestrator<R: WorkerRunner, S: crate::store::DataStore = crate::store::NoopStore> {
    runner: Arc<R>,
    store: Arc<S>,
}

/// Shared mutable state for tracking task completion during batch execution.
struct ExecutionState {
    completed: Arc<Mutex<HashSet<String>>>,
    failed: Arc<Mutex<HashSet<String>>>,
    running: Arc<Mutex<HashSet<String>>>,
    results: Arc<Mutex<Vec<WorkerResult>>>,
    semaphore: Arc<Semaphore>,
}

impl ExecutionState {
    fn new(max_workers: usize) -> Self {
        Self {
            completed: Arc::new(Mutex::new(HashSet::new())),
            failed: Arc::new(Mutex::new(HashSet::new())),
            running: Arc::new(Mutex::new(HashSet::new())),
            results: Arc::new(Mutex::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(max_workers)),
        }
    }
}

impl<R: WorkerRunner + 'static, S: crate::store::DataStore + 'static> Orchestrator<R, S> {
    /// Create a new orchestrator with the given worker runner and data store.
    pub fn new(runner: R, store: Arc<S>) -> Self {
        Self {
            runner: Arc::new(runner),
            store,
        }
    }

    /// Execute a parsed batch, returning results for all tasks.
    ///
    /// Tasks are launched respecting dependency order and concurrency limits.
    /// Failed tasks cause their dependents to be skipped.
    ///
    /// # Errors
    ///
    /// Returns `AutorunError` if batch execution cannot proceed.
    pub async fn execute(
        &self,
        session_id: &str,
        batch: &ParsedBatch,
        project_dir: &std::path::Path,
    ) -> Result<Vec<WorkerResult>, AutorunError> {
        let batch_name = batch.name.clone();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let total_tasks_i32 = batch.order.len().min(i32::MAX as usize) as i32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let max_workers_i32 = batch.max_workers.min(i32::MAX as usize) as i32;

        // Create autorun_session record at batch start.
        let ar_session = crate::models::AutorunSession {
            id: session_id.to_string(),
            batch_file: String::new(),
            batch_name: Some(batch_name.clone()),
            status: crate::types::AutorunSessionStatus::Running,
            max_session_workers: max_workers_i32,
            total_tasks: total_tasks_i32,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: Some(i64::from(std::process::id())),
            skipped_tasks: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        };
        if let Err(e) = self.store.create_autorun_session(&ar_session).await {
            eprintln!("warning: failed to create autorun_session record: {e}");
        }

        // Emit batch_started event.
        let batch_started = crate::coordination::types::events::AutorunEvent::BatchStarted {
            session_id: session_id.to_string(),
            batch_name: batch_name.clone(),
            total_tasks: total_tasks_i32,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        Self::emit_autorun_event(project_dir, &batch_started);

        let dep_map: HashMap<&str, Vec<&str>> = batch
            .tasks
            .iter()
            .map(|t| {
                (
                    t.id.as_str(),
                    t.depends_on.iter().map(String::as_str).collect(),
                )
            })
            .collect();

        // Bound the semaphore by the global worktree limit to prevent
        // exceeding worktree.max_concurrent even if batch.max_workers is larger.
        let effective_workers = match crate::autorun::config::load_config(project_dir) {
            Ok(config) => batch.max_workers.min(config.worktree.max_concurrent),
            Err(_) => batch.max_workers,
        };
        let state = ExecutionState::new(effective_workers);
        let total_tasks = batch.order.len();

        loop {
            let done_count = {
                let c = state.completed.lock().await;
                let f = state.failed.lock().await;
                c.len() + f.len()
            };
            if done_count >= total_tasks {
                break;
            }

            let launched_any = self
                .dispatch_ready_tasks(session_id, batch, &dep_map, &state)
                .await;

            if !launched_any {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }

        let final_results = state.results.lock().await.clone();

        // Compute final counts.
        let completed_count = final_results
            .iter()
            .filter(|r| r.status == "completed")
            .count();
        let failed_count = final_results
            .iter()
            .filter(|r| r.status == "failed" || r.status == "timeout")
            .count();
        let skipped_count = final_results
            .iter()
            .filter(|r| r.status == "skipped")
            .count();

        let final_status = if failed_count > 0 {
            crate::types::AutorunSessionStatus::Failed
        } else {
            crate::types::AutorunSessionStatus::Completed
        };

        // Update autorun_session at batch end.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let session_update = crate::models::AutorunSessionUpdate {
            status: Some(final_status),
            completed_tasks: Some(completed_count.min(i32::MAX as usize) as i32),
            failed_tasks: Some(failed_count.min(i32::MAX as usize) as i32),
            skipped_tasks: Some(skipped_count.min(i32::MAX as usize) as i32),
            completed_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        if let Err(e) = self
            .store
            .update_autorun_session(session_id, session_update)
            .await
        {
            eprintln!("warning: failed to update autorun_session record: {e}");
        }

        // Emit batch_completed event.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let batch_end = crate::coordination::types::events::AutorunEvent::BatchCompleted {
            session_id: session_id.to_string(),
            batch_name: batch_name.clone(),
            completed_tasks: completed_count.min(i32::MAX as usize) as i32,
            failed_tasks: failed_count.min(i32::MAX as usize) as i32,
            skipped_tasks: skipped_count.min(i32::MAX as usize) as i32,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        Self::emit_autorun_event(project_dir, &batch_end);

        Ok(final_results)
    }

    /// Emit an autorun event to the JSONL ledger.
    fn emit_autorun_event(
        project_dir: &std::path::Path,
        event: &crate::coordination::types::events::AutorunEvent,
    ) {
        let ledger_dir = project_dir.join(".state/ledger");
        let file_path = ledger_dir.join(crate::ledger::files::AUTORUN_EVENTS);
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(event) {
            use std::io::Write;
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path);
            if let Ok(mut f) = file {
                let _ = writeln!(f, "{json}");
            }
        }
    }

    /// Scan the task order and dispatch any tasks whose dependencies are met.
    async fn dispatch_ready_tasks(
        &self,
        session_id: &str,
        batch: &ParsedBatch,
        dep_map: &HashMap<&str, Vec<&str>>,
        state: &ExecutionState,
    ) -> bool {
        let mut launched_any = false;

        for (idx, task_id) in batch.order.iter().enumerate() {
            let is_done = {
                let c = state.completed.lock().await;
                let f = state.failed.lock().await;
                let r = state.running.lock().await;
                c.contains(task_id.as_str())
                    || f.contains(task_id.as_str())
                    || r.contains(task_id.as_str())
            };

            if is_done {
                continue;
            }

            let deps = dep_map.get(task_id.as_str()).cloned().unwrap_or_default();
            let (all_deps_done, any_dep_failed) = {
                let c = state.completed.lock().await;
                let f = state.failed.lock().await;
                (
                    deps.iter().all(|d| c.contains(*d)),
                    deps.iter().any(|d| f.contains(*d)),
                )
            };

            if any_dep_failed {
                state.failed.lock().await.insert(task_id.clone());
                state.results.lock().await.push(WorkerResult {
                    worker_id: String::new(),
                    task_id: task_id.clone(),
                    status: "skipped".into(),
                    exit_code: -1,
                    pr_number: 0,
                    pr_url: String::new(),
                    error: "dependency failed".into(),
                    branch_name: String::new(),
                    duration_sec: 0,
                });
                launched_any = true;
                continue;
            }

            if !all_deps_done {
                continue;
            }

            let Ok(permit) = state.semaphore.clone().try_acquire_owned() else {
                break;
            };

            launched_any = true;
            state.running.lock().await.insert(task_id.clone());
            let task_spec = batch.tasks.iter().find(|t| t.id == *task_id);
            let file_scope = task_spec.map(|t| t.file_scope.clone()).unwrap_or_default();
            let scope_policy = task_spec
                .and_then(|t| t.scope_policy.clone())
                .unwrap_or_else(|| "soft".to_string());
            Self::spawn_worker(
                self.runner.clone(),
                WorkerConfig {
                    session_id: session_id.to_string(),
                    worker_id: format!("arw-{task_id}"),
                    worker_num: idx + 1,
                    task_id: task_id.clone(),
                    batch_name: batch.name.clone(),
                    auto_merge: batch.auto_merge,
                    target: batch.target.clone(),
                    tmux_prefix: format!("codeflow-{}-w", &session_id[..session_id.len().min(8)]),
                    file_scope,
                    scope_policy,
                },
                state.completed.clone(),
                state.failed.clone(),
                state.running.clone(),
                state.results.clone(),
                permit,
            );
        }

        launched_any
    }

    /// Spawn a single worker task onto the tokio runtime.
    fn spawn_worker(
        runner: Arc<R>,
        cfg: WorkerConfig,
        completed: Arc<Mutex<HashSet<String>>>,
        failed: Arc<Mutex<HashSet<String>>>,
        running: Arc<Mutex<HashSet<String>>>,
        results: Arc<Mutex<Vec<WorkerResult>>>,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) {
        let task_id = cfg.task_id.clone();

        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let result = runner.run(cfg).await;
            #[allow(clippy::cast_possible_wrap)]
            let duration = start.elapsed().as_secs() as i64;

            running.lock().await.remove(&task_id);
            match result {
                Ok(mut r) => {
                    r.duration_sec = duration;
                    if r.status == "completed" {
                        completed.lock().await.insert(task_id);
                    } else {
                        failed.lock().await.insert(task_id);
                    }
                    results.lock().await.push(r);
                }
                Err(e) => {
                    failed.lock().await.insert(task_id.clone());
                    results.lock().await.push(WorkerResult {
                        worker_id: String::new(),
                        task_id,
                        status: "failed".into(),
                        exit_code: 1,
                        pr_number: 0,
                        pr_url: String::new(),
                        error: e.to_string(),
                        branch_name: String::new(),
                        duration_sec: duration,
                    });
                }
            }

            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autorun::batch::parse_batch_data;
    use crate::error::AutorunError;
    use std::sync::atomic::{AtomicI32, Ordering};

    fn mock_store() -> Arc<crate::store::mock::MockStore> {
        Arc::new(crate::store::mock::MockStore::new())
    }

    // Mock runner that tracks execution order.
    struct OrderTracker {
        counter: Arc<AtomicI32>,
        order: Arc<Mutex<Vec<(String, i32)>>>,
    }

    impl OrderTracker {
        fn new() -> Self {
            Self {
                counter: Arc::new(AtomicI32::new(0)),
                order: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl WorkerRunner for OrderTracker {
        async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            let n = self.counter.fetch_add(1, Ordering::SeqCst);
            self.order.lock().await.push((cfg.task_id.clone(), n));
            // Simulate some work.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "completed".into(),
                exit_code: 0,
                pr_number: 0,
                pr_url: String::new(),
                error: String::new(),
                branch_name: String::new(),
                duration_sec: 0,
            })
        }
    }

    struct FailingRunner;

    impl WorkerRunner for FailingRunner {
        async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "failed".into(),
                exit_code: 1,
                pr_number: 0,
                pr_url: String::new(),
                error: "test failure".into(),
                branch_name: String::new(),
                duration_sec: 0,
            })
        }
    }

    fn make_batch(yaml: &str) -> ParsedBatch {
        parse_batch_data(yaml, "test.yaml").unwrap()
    }

    #[tokio::test]
    async fn test_orchestrator_linear_execution() {
        let runner = OrderTracker::new();
        let order_ref = runner.order.clone();
        let orch = Orchestrator::new(runner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch =
            make_batch("max_workers: 1\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch
            .execute("ses-test", &batch, project_dir.path())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.status == "completed"));

        let order = order_ref.lock().await;
        // With max_workers=1, a must execute before b.
        let a_order = order.iter().find(|(id, _)| id == "a").unwrap().1;
        let b_order = order.iter().find(|(id, _)| id == "b").unwrap().1;
        assert!(a_order < b_order);
    }

    #[tokio::test]
    async fn test_orchestrator_parallel_execution() {
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n  - id: c\n");

        let results = orch
            .execute("ses-test", &batch, project_dir.path())
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.status == "completed"));
    }

    #[tokio::test]
    async fn test_orchestrator_dependency_failure_skips_dependents() {
        let orch = Orchestrator::new(FailingRunner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch =
            make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch
            .execute("ses-test", &batch, project_dir.path())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let a_result = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a_result.status, "failed");

        let b_result = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(b_result.status, "skipped");
    }

    #[tokio::test]
    async fn test_orchestrator_single_task() {
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: only\n");

        let results = orch
            .execute("ses-test", &batch, project_dir.path())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "only");
        assert_eq!(results[0].status, "completed");
    }

    #[tokio::test]
    async fn test_orchestrator_diamond_dependency() {
        let runner = OrderTracker::new();
        let order_ref = runner.order.clone();
        let orch = Orchestrator::new(runner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch(
            "max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n  - id: c\n    depends_on: [a]\n  - id: d\n    depends_on: [b, c]\n",
        );

        let results = orch
            .execute("ses-test", &batch, project_dir.path())
            .await
            .unwrap();
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|r| r.status == "completed"));

        let order = order_ref.lock().await;
        let a_order = order.iter().find(|(id, _)| id == "a").unwrap().1;
        let b_order = order.iter().find(|(id, _)| id == "b").unwrap().1;
        let c_order = order.iter().find(|(id, _)| id == "c").unwrap().1;
        let d_order = order.iter().find(|(id, _)| id == "d").unwrap().1;
        assert!(a_order < b_order);
        assert!(a_order < c_order);
        assert!(b_order < d_order);
        assert!(c_order < d_order);
    }

    #[test]
    fn test_worker_config_fields() {
        let cfg = WorkerConfig {
            session_id: "ses-1".into(),
            worker_id: "arw-1".into(),
            worker_num: 1,
            task_id: "task-a".into(),
            batch_name: "batch".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_prefix: "worker".into(),
            file_scope: vec!["src/**/*.rs".into()],
            scope_policy: "hard".into(),
        };
        assert_eq!(cfg.task_id, "task-a");
        assert_eq!(cfg.worker_num, 1);
        assert_eq!(cfg.file_scope, vec!["src/**/*.rs"]);
        assert_eq!(cfg.scope_policy, "hard");
    }

    #[test]
    fn test_worker_result_fields() {
        let result = WorkerResult {
            worker_id: "arw-1".into(),
            task_id: "task-a".into(),
            status: "completed".into(),
            exit_code: 0,
            pr_number: 42,
            pr_url: "url".into(),
            error: String::new(),
            branch_name: "feat/x".into(),
            duration_sec: 120,
        };
        assert_eq!(result.status, "completed");
        assert_eq!(result.duration_sec, 120);
    }

    #[test]
    fn test_tmux_name_format_batch_scoped() {
        // Verify the tmux_prefix format uses session_id first 8 chars.
        let session_id = "ses-01km9911pmagn2xa8n9b449wdd";
        let prefix = format!("codeflow-{}-w", &session_id[..session_id.len().min(8)]);
        // Worker num 1 should produce: codeflow-ses-01km-w1
        let tmux_name = format!("{prefix}{}", 1);
        assert!(
            tmux_name.starts_with("codeflow-"),
            "tmux name should start with codeflow-"
        );
        assert!(
            tmux_name.ends_with("1"),
            "tmux name should end with worker num"
        );
        // Verify format: codeflow-{8chars with hyphens}-w{N}
        let re_pattern = r"^codeflow-[a-z0-9-]{1,8}-w[0-9]+$";
        let re = regex::Regex::new(re_pattern).unwrap();
        assert!(
            re.is_match(&tmux_name),
            "tmux name '{tmux_name}' must match pattern {re_pattern}"
        );

        // Verify uniqueness: different session IDs produce different prefixes.
        let other_id = "ses-99xx1234abcd";
        let other_prefix = format!("codeflow-{}-w", &other_id[..other_id.len().min(8)]);
        assert_ne!(
            prefix, other_prefix,
            "different sessions must produce different prefixes"
        );
    }

    #[tokio::test]
    async fn test_semaphore_bounded_by_min_max_workers_max_concurrent() {
        // Create a config with max_concurrent=2 while batch has max_workers=5.
        // The semaphore should use min(5, 2) = 2.
        let project_dir = tempfile::tempdir().unwrap();
        let config_dir = project_dir.path().join(".codeflow/config/parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "max_concurrent": 2 } }"#,
        )
        .unwrap();

        // Track concurrency: how many workers run simultaneously.
        let max_concurrent_observed = Arc::new(AtomicI32::new(0));
        let current_concurrent = Arc::new(AtomicI32::new(0));

        struct ConcurrencyTracker {
            max_observed: Arc<AtomicI32>,
            current: Arc<AtomicI32>,
        }

        impl WorkerRunner for ConcurrencyTracker {
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
                let prev = self.current.fetch_add(1, Ordering::SeqCst);
                let concurrent = prev + 1;
                // Update max observed.
                self.max_observed.fetch_max(concurrent, Ordering::SeqCst);
                // Simulate work.
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                self.current.fetch_sub(1, Ordering::SeqCst);
                Ok(WorkerResult {
                    worker_id: cfg.worker_id,
                    task_id: cfg.task_id,
                    status: "completed".into(),
                    exit_code: 0,
                    pr_number: 0,
                    pr_url: String::new(),
                    error: String::new(),
                    branch_name: String::new(),
                    duration_sec: 0,
                })
            }
        }

        let tracker = ConcurrencyTracker {
            max_observed: max_concurrent_observed.clone(),
            current: current_concurrent,
        };
        let orch = Orchestrator::new(tracker, mock_store());

        // 4 independent tasks, max_workers=5, but max_concurrent=2 from config.
        let batch =
            make_batch("max_workers: 5\ntasks:\n  - id: a\n  - id: b\n  - id: c\n  - id: d\n");

        let results = orch
            .execute("ses-test", &batch, project_dir.path())
            .await
            .unwrap();
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|r| r.status == "completed"));

        let max_seen = max_concurrent_observed.load(Ordering::SeqCst);
        assert!(
            max_seen <= 2,
            "max concurrent workers should be <= 2 (min(5, 2)), but saw {max_seen}"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_creates_session_record_at_batch_start() {
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: a\n");
        orch.execute("ses-db-test", &batch, project_dir.path())
            .await
            .unwrap();

        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-db-test").unwrap();
        assert_eq!(session.total_tasks, 1);
        assert!(session.pid.is_some());
        assert!(session.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_orchestrator_updates_session_at_batch_end_completed() {
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: a\n  - id: b\n");
        orch.execute("ses-end-test", &batch, project_dir.path())
            .await
            .unwrap();

        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-end-test").unwrap();
        assert_eq!(
            session.status,
            crate::types::AutorunSessionStatus::Completed
        );
        assert_eq!(session.completed_tasks, 2);
        assert_eq!(session.failed_tasks, 0);
        assert_eq!(session.skipped_tasks, 0);
    }

    #[tokio::test]
    async fn test_orchestrator_updates_session_at_batch_end_with_failures() {
        let store = mock_store();
        let orch = Orchestrator::new(FailingRunner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();

        let batch =
            make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");
        orch.execute("ses-fail-test", &batch, project_dir.path())
            .await
            .unwrap();

        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-fail-test").unwrap();
        assert_eq!(session.status, crate::types::AutorunSessionStatus::Failed);
        // a fails, b is skipped
        assert_eq!(session.completed_tasks, 0);
        assert_eq!(session.failed_tasks, 1);
        assert_eq!(session.skipped_tasks, 1);
    }

    #[tokio::test]
    async fn test_orchestrator_emits_batch_events_to_jsonl() {
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: a\n");
        orch.execute("ses-event-test", &batch, project_dir.path())
            .await
            .unwrap();

        // Check that autorun-events.jsonl was written.
        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events.jsonl");
        assert!(events_path.exists(), "autorun-events.jsonl should exist");
        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(
            content.contains("batch_started"),
            "should contain batch_started event"
        );
        assert!(
            content.contains("batch_completed"),
            "should contain batch_completed event"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_session_batch_name_recorded() {
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("name: my-batch\ntasks:\n  - id: a\n");
        orch.execute("ses-name", &batch, project_dir.path())
            .await
            .unwrap();

        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-name").unwrap();
        assert_eq!(session.batch_name.as_deref(), Some("my-batch"));
        assert_eq!(session.total_tasks, 1);
    }
}
