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
pub struct Orchestrator<R: WorkerRunner> {
    runner: Arc<R>,
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

impl<R: WorkerRunner + 'static> Orchestrator<R> {
    /// Create a new orchestrator with the given worker runner.
    pub fn new(runner: R) -> Self {
        Self {
            runner: Arc::new(runner),
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
    ) -> Result<Vec<WorkerResult>, AutorunError> {
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

        let state = ExecutionState::new(batch.max_workers);
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
        Ok(final_results)
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
                    tmux_prefix: "codeflow-worker".into(),
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
        let orch = Orchestrator::new(runner);

        let batch =
            make_batch("max_workers: 1\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch.execute("ses-test", &batch).await.unwrap();
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
        let orch = Orchestrator::new(runner);

        let batch = make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n  - id: c\n");

        let results = orch.execute("ses-test", &batch).await.unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.status == "completed"));
    }

    #[tokio::test]
    async fn test_orchestrator_dependency_failure_skips_dependents() {
        let orch = Orchestrator::new(FailingRunner);

        let batch =
            make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch.execute("ses-test", &batch).await.unwrap();
        assert_eq!(results.len(), 2);

        let a_result = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a_result.status, "failed");

        let b_result = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(b_result.status, "skipped");
    }

    #[tokio::test]
    async fn test_orchestrator_single_task() {
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner);

        let batch = make_batch("tasks:\n  - id: only\n");

        let results = orch.execute("ses-test", &batch).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].task_id, "only");
        assert_eq!(results[0].status, "completed");
    }

    #[tokio::test]
    async fn test_orchestrator_diamond_dependency() {
        let runner = OrderTracker::new();
        let order_ref = runner.order.clone();
        let orch = Orchestrator::new(runner);

        let batch = make_batch(
            "max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n  - id: c\n    depends_on: [a]\n  - id: d\n    depends_on: [b, c]\n",
        );

        let results = orch.execute("ses-test", &batch).await.unwrap();
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
        };
        assert_eq!(cfg.task_id, "task-a");
        assert_eq!(cfg.worker_num, 1);
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
}
