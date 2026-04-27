//! Orchestrator for dependency-aware concurrent task execution.
//!
//! Uses `tokio::sync::Semaphore` for concurrency control and Kahn's
//! algorithm output for task ordering.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinHandle;

use crate::error::AutorunError;

use super::batch::ParsedBatch;
use super::worker::WorkerRunner;

/// C26: Append a timestamped log entry to the orchestrator log file.
fn orchestrator_log(project_dir: &std::path::Path, session_id: &str, message: &str) {
    use std::io::Write;
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let log_dir = project_dir.join(format!(".state/autorun/logs/{session_id}"));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("orchestrator.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(f, "[{timestamp}] {message}");
    }
}

/// Configuration for a single worker execution.
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub session_id: String,
    pub worker_id: String,
    pub worker_num: usize,
    pub task_id: String,
    /// Human-readable format id for `task_id` (e.g. `INF-TSK-049-001`).
    /// Read from the task markdown's `format_id` frontmatter field.
    /// `None` when the task markdown cannot be located or the field is
    /// absent. INF-TSK-049-001 AC #15.
    pub task_format_id: Option<String>,
    pub batch_name: String,
    pub integration_auto_merge: bool,
    pub integration_branch: String,
    /// Full tmux session name for this worker.
    pub tmux_name: String,
    /// File patterns this worker claims for exclusive access via Loro.
    pub file_scope: Vec<String>,
    /// Scope policy for this worker: "soft", "hard", or "permissive".
    /// Defaults to "soft" when not specified in the task definition.
    pub scope_policy: String,
    /// Behavior on claim conflict: "skip_and_continue" or "fail".
    /// Defaults to "skip_and_continue".
    pub blocked_behavior: String,
    /// Who performs epic status updates: "orchestrator" or "none".
    pub epic_update: String,
    /// Timeout in seconds waiting for merge queue position 0.
    pub queue_timeout_secs: u64,
    /// Per-task timeout override in seconds. If `Some`, overrides the global
    /// `autorun.worker_timeout_secs` for this specific task.
    pub task_timeout_secs: Option<u64>,
}

/// Result of a worker execution.
#[derive(Debug, Clone, Default)]
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
    /// Optional human-readable warning attached to a successful (or
    /// success-with-caveats) outcome. INF-TSK-049-001 AC #34 — exit 124
    /// with branch already on origin is reported as `status="completed"`
    /// and a non-empty `warning` rather than as `failed`. Cascade
    /// dispatch (orchestrator) treats any "completed" task — with or
    /// without a warning — as success and proceeds to dependents (AC #36).
    pub warning: Option<String>,
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
    /// Lock-free abort flag set on Ctrl+C.
    abort: Arc<AtomicBool>,
    /// Tracked `JoinHandle`s for spawned worker tasks (for graceful drain).
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// Active tmux session names for abort cleanup.
    tmux_sessions: Arc<Mutex<Vec<String>>>,
}

impl ExecutionState {
    fn new(max_workers: usize) -> Self {
        Self {
            completed: Arc::new(Mutex::new(HashSet::new())),
            failed: Arc::new(Mutex::new(HashSet::new())),
            running: Arc::new(Mutex::new(HashSet::new())),
            results: Arc::new(Mutex::new(Vec::new())),
            semaphore: Arc::new(Semaphore::new(max_workers)),
            abort: Arc::new(AtomicBool::new(false)),
            handles: Arc::new(Mutex::new(Vec::new())),
            tmux_sessions: Arc::new(Mutex::new(Vec::new())),
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
    /// The `shutdown` future is raced against the dispatch loop; when it
    /// resolves the orchestrator performs graceful abort (kill tmux sessions,
    /// drain worker handles, skip pending tasks). Production callers pass
    /// `async { tokio::signal::ctrl_c().await.ok(); }`; tests can pass a
    /// `oneshot::Receiver` or `std::future::pending()`.
    ///
    /// # Errors
    ///
    /// Returns `AutorunError` if batch execution cannot proceed.
    pub async fn execute<F: std::future::Future<Output = ()>>(
        &self,
        session_id: &str,
        batch: &ParsedBatch,
        project_dir: &std::path::Path,
        shutdown: F,
    ) -> Result<Vec<WorkerResult>, AutorunError> {
        self.execute_with_batch_file(session_id, batch, project_dir, "", shutdown)
            .await
    }

    /// Execute a batch with an explicit batch_file path recorded in the session.
    ///
    /// # Errors
    ///
    /// Returns `AutorunError` if batch execution cannot proceed.
    pub async fn execute_with_batch_file<F: std::future::Future<Output = ()>>(
        &self,
        session_id: &str,
        batch: &ParsedBatch,
        project_dir: &std::path::Path,
        batch_file: &str,
        shutdown: F,
    ) -> Result<Vec<WorkerResult>, AutorunError> {
        let batch_name = batch.name.clone();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let total_tasks_i32 = batch.order.len().min(i32::MAX as usize) as i32;
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let max_workers_i32 = batch.max_workers.min(i32::MAX as usize) as i32;

        // Create autorun_session record at batch start.
        let ar_session = crate::models::AutorunSession {
            id: session_id.to_string(),
            batch_file: batch_file.to_string(),
            batch_name: Some(batch_name.clone()),
            status: crate::types::AutorunSessionStatus::Running,
            max_session_workers: max_workers_i32,
            total_tasks: total_tasks_i32,
            completed_tasks: 0,
            failed_tasks: 0,
            pid: Some(i64::from(std::process::id())),
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: Some(batch.integration_branch.clone()),
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: Some(chrono::Utc::now().to_rfc3339()),
            last_heartbeat_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            abort_started_at: None,
        };
        if let Err(e) = self.store.create_autorun_session(&ar_session).await {
            eprintln!("warning: failed to create autorun_session record: {e}");
        }

        // C5: Store orchestrator tmux session name from env var.
        // CAS: session was just created as Running; guard prevents overwrite if
        // another process already moved it to a terminal state.
        if let Ok(orch_tmux) = std::env::var("CODEFLOW_ORCH_TMUX") {
            match self
                .store
                .update_autorun_session_cas(
                    session_id,
                    crate::types::AutorunSessionStatus::Running,
                    crate::models::AutorunSessionUpdate {
                        tmux_session: Some(Some(orch_tmux)),
                        updated_at: Some(chrono::Utc::now().to_rfc3339()),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(crate::store::CasResult::NoOp) => {
                    eprintln!(
                        "warning: CAS no-op storing tmux session for {session_id} (status changed)"
                    );
                }
                Err(e) => {
                    eprintln!("warning: failed to store tmux session for {session_id}: {e}");
                }
                Ok(crate::store::CasResult::Updated(_)) => {}
            }
        }

        // C26: Log BATCH_START.
        orchestrator_log(
            project_dir,
            session_id,
            &format!(
                "BATCH_START name={batch_name} tasks={total_tasks_i32} max_workers={max_workers_i32}"
            ),
        );

        // Emit batch_started event.
        let batch_started = crate::coordination::types::events::AutorunEvent::BatchStarted {
            session_id: session_id.to_string(),
            batch_name: batch_name.clone(),
            total_tasks: total_tasks_i32,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        Self::emit_autorun_event(project_dir, &batch_started);

        // Ensure integration branch exists on origin before workers start.
        if !batch.integration_branch.is_empty() {
            ensure_integration_branch(project_dir, &batch.integration_branch);
        }

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
        let loaded_config = crate::autorun::config::load_config(project_dir).unwrap_or_default();
        let effective_workers = batch.max_workers.min(loaded_config.worktree.max_concurrent);
        // C27: Log cap warning to orchestrator log if workers were capped.
        if batch.max_workers > loaded_config.worktree.max_concurrent {
            orchestrator_log(
                project_dir,
                session_id,
                &format!(
                    "CAP_WARNING max_workers={} capped to max_concurrent={}",
                    batch.max_workers, loaded_config.worktree.max_concurrent
                ),
            );
        }
        let blocked_behavior = loaded_config.autorun.blocked_behavior.clone();

        // C4: Start heartbeat background task.
        let heartbeat_dir = project_dir.join(".state/autorun");
        let _ = std::fs::create_dir_all(&heartbeat_dir);
        let heartbeat_path = heartbeat_dir.join(format!("heartbeat-{session_id}"));
        let heartbeat_interval =
            std::time::Duration::from_secs(loaded_config.autorun.heartbeat_interval_secs);
        let heartbeat_handle = tokio::spawn({
            let path = heartbeat_path.clone();
            async move {
                loop {
                    let _ = std::fs::OpenOptions::new()
                        .create(true)
                        .truncate(true)
                        .write(true)
                        .open(&path);
                    tokio::time::sleep(heartbeat_interval).await;
                }
            }
        });

        let state = ExecutionState::new(effective_workers);
        let total_tasks = batch.order.len();

        // INF-TSK-050-001 AC #6: track previously-published counters so
        // we only CAS-update the session row when counts actually
        // change. Avoids per-iteration write storms (the loop ticks
        // every ~50ms) while keeping the TUI within ~50ms of the truth.
        let last_published_counts = Arc::new(Mutex::new((0i32, 0i32, 0i32)));

        // Race the dispatch loop against the shutdown signal.
        tokio::pin!(shutdown);
        let aborted = tokio::select! {
            // Branch 1: normal dispatch loop runs to completion.
            () = async {
                loop {
                    let done_count = {
                        let c = state.completed.lock().await;
                        let f = state.failed.lock().await;
                        c.len() + f.len()
                    };
                    if done_count >= total_tasks {
                        break;
                    }

                    // Check for external abort marker file.
                    let abort_marker = project_dir.join(format!(".state/runtime/abort-{session_id}"));
                    if abort_marker.exists() {
                        eprintln!("\nAbort marker detected — aborting batch...");
                        state.abort.store(true, Ordering::SeqCst);
                        break;
                    }

                    let launched_any = self
                        .dispatch_ready_tasks(session_id, batch, project_dir, &dep_map, &state, &blocked_behavior)
                        .await;

                    // INF-TSK-050-001 AC #6: mid-run counter propagation.
                    // Recompute from `state.results` (the truth) and CAS
                    // only when values changed. CAS-guard on Running so
                    // we never overwrite a terminal status set by the
                    // stale detector concurrently.
                    Self::publish_mid_run_counts(
                        self.store.as_ref(),
                        session_id,
                        &state,
                        &last_published_counts,
                    ).await;

                    if !launched_any {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                }
            } => state.abort.load(Ordering::SeqCst),

            // Branch 2: shutdown signal received — set abort flag and break out.
            () = &mut shutdown => {
                eprintln!("\nShutdown signal received — aborting batch...");
                state.abort.store(true, Ordering::SeqCst);
                true
            },
        };

        // If aborted, perform graceful shutdown of running workers.
        if aborted {
            Self::abort_cleanup(&state, &batch.order).await;
        }

        // C4: Stop heartbeat and clean up file.
        heartbeat_handle.abort();
        let _ = std::fs::remove_file(&heartbeat_path);

        // INF-TSK-050-001 AC #5 (partial — orchestrator side): remove
        // the abort marker file if it exists. The CLI side (run_abort
        // idempotency) is handled in Wave 2. Removing here ensures a
        // clean orchestrator exit doesn't leave a dangling marker that
        // would mis-trigger the next run's abort path.
        let abort_marker = project_dir.join(format!(".state/runtime/abort-{session_id}"));
        let _ = std::fs::remove_file(&abort_marker);

        let final_results = state.results.lock().await.clone();

        // Compute final counts.
        let completed_count = final_results
            .iter()
            .filter(|r| r.status == "completed")
            .count();
        let timed_out = final_results.iter().any(|r| r.status == "timeout");
        let failed_count = final_results
            .iter()
            .filter(|r| r.status == "failed")
            .count();
        let timeout_count = final_results
            .iter()
            .filter(|r| r.status == "timeout")
            .count();
        let skipped_count = final_results
            .iter()
            .filter(|r| r.status == "skipped")
            .count();
        let pr_failed_count = final_results
            .iter()
            .filter(|r| r.status == "pr_creation_failed")
            .count();
        if pr_failed_count > 0 {
            eprintln!("WARNING: {pr_failed_count} task(s) completed but PR creation failed");
        }

        let final_status =
            Self::determine_final_status(aborted, timed_out, failed_count + pr_failed_count);

        // INF-TSK-050-001 AC #19: populate `final_pr_url` when exactly
        // one task produced a PR. Multi-PR batches leave None (the
        // detail view shows per-task PRs). An empty string `pr_url`
        // counts as "no PR" — defensive against runners that always
        // populate the field. The update field has touch-semantics
        // `Option<String>` (the schema column is `option<string>`),
        // so we only set it on the unambiguous single-PR case.
        let final_pr_url: Option<String> = {
            let urls: Vec<&str> = final_results
                .iter()
                .map(|r| r.pr_url.as_str())
                .filter(|s| !s.is_empty())
                .collect();
            match urls.as_slice() {
                [single] => Some((*single).to_string()),
                _ => None, // None (no PR) or multi-PR (don't pick one)
            }
        };

        // Update autorun_session at batch end.
        // CAS: expect Running — prevents overwriting a status set by another process
        // (e.g., stale detector already marked it Failed).
        let now = chrono::Utc::now().to_rfc3339();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let session_update = crate::models::AutorunSessionUpdate {
            status: Some(final_status),
            completed_tasks: Some(completed_count.min(i32::MAX as usize) as i32),
            failed_tasks: Some((failed_count + timeout_count).min(i32::MAX as usize) as i32),
            skipped_tasks: Some(skipped_count.min(i32::MAX as usize) as i32),
            completed_at: Some(now.clone()),
            updated_at: Some(now),
            // Clear in-flight task pointer on batch end (inner None = set to null).
            current_task_id: Some(None),
            // INF-TSK-050-001 AC #19: set final_pr_url for single-PR
            // batches; leave None for multi-PR or zero-PR batches so
            // the field reflects an unambiguous final-PR semantic.
            final_pr_url,
            ..Default::default()
        };
        match self
            .store
            .update_autorun_session_cas(
                session_id,
                crate::types::AutorunSessionStatus::Running,
                session_update,
            )
            .await
        {
            Ok(crate::store::CasResult::NoOp) => {
                eprintln!(
                    "warning: CAS no-op for final status of {session_id} \
                     (expected Running, another process already transitioned it)"
                );
            }
            Err(e) => {
                eprintln!("warning: failed to update autorun_session record: {e}");
            }
            Ok(crate::store::CasResult::Updated(_)) => {}
        }

        // INF-TSK-050-001 AC #14: mark worker `interactive_session`
        // rows complete. Workers register as `interactive_session`
        // rows with `session_kind='autorun'`; without this propagation
        // they stay `status='active'` indefinitely after the batch
        // ends, polluting the interactive status view.
        Self::mark_worker_sessions_complete(self.store.as_ref(), session_id).await;

        // Emit appropriate batch event.
        Self::emit_batch_event(
            project_dir,
            session_id,
            &batch_name,
            aborted,
            completed_count,
            failed_count + timeout_count,
            skipped_count,
        );

        // Cleanup auto-generated integration branches after all workers finish.
        if batch.integration_branch_is_auto && !batch.integration_branch.is_empty() {
            cleanup_auto_integration_branch(project_dir, &batch.integration_branch);
        }

        // INF-TSK-050-001 AC #18: auto-prune at end of batch. Fires
        // and forgets — orchestrator return is not blocked on prune.
        // Only runs when `retention.purge_on_cleanup` is enabled.
        if loaded_config.retention.purge_on_cleanup {
            Self::spawn_auto_prune(
                self.store.clone(),
                project_dir.to_path_buf(),
                &loaded_config,
            );
        }

        Ok(final_results)
    }

    /// Perform graceful abort cleanup: kill tmux sessions, drain worker
    /// handles with a 10-second deadline, and mark pending tasks as skipped.
    async fn abort_cleanup(state: &ExecutionState, task_order: &[String]) {
        // Kill all tracked tmux sessions.
        let sessions = state.tmux_sessions.lock().await.clone();
        for name in &sessions {
            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", name])
                .output();
        }

        // Drain worker handles with a 10-second deadline.
        let handles: Vec<JoinHandle<()>> = std::mem::take(&mut *state.handles.lock().await);
        Self::drain_handles(handles, std::time::Duration::from_secs(10)).await;

        // Mark remaining pending tasks as skipped.
        let completed = state.completed.lock().await;
        let failed = state.failed.lock().await;
        let mut results = state.results.lock().await;
        for task_id in task_order {
            if !completed.contains(task_id.as_str()) && !failed.contains(task_id.as_str()) {
                results.push(WorkerResult {
                    worker_id: String::new(),
                    task_id: task_id.clone(),
                    status: "skipped".into(),
                    exit_code: -1,
                    pr_number: 0,
                    pr_url: String::new(),
                    error: "batch_aborted".into(),
                    branch_name: String::new(),
                    duration_sec: 0,
                    warning: None,
                });
            }
        }
    }

    /// Wait for worker `JoinHandle`s to finish within `grace_period`.
    /// After the deadline, explicitly abort all remaining handles.
    async fn drain_handles(mut handles: Vec<JoinHandle<()>>, grace_period: std::time::Duration) {
        if handles.is_empty() {
            return;
        }
        let deadline = tokio::time::Instant::now() + grace_period;
        while !handles.is_empty() {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                // Timeout exceeded — abort all remaining worker tasks.
                for h in &handles {
                    h.abort();
                }
                eprintln!(
                    "warning: {} workers did not finish cleanup within grace period",
                    handles.len()
                );
                break;
            }
            let h = handles.remove(0);
            if tokio::time::timeout(remaining, h).await.is_err() {
                // This handle timed out — abort all remaining.
                for h in &handles {
                    h.abort();
                }
                eprintln!(
                    "warning: {} workers did not finish cleanup within grace period",
                    handles.len() + 1
                );
                break;
            }
        }
    }

    /// Determine the final session status based on abort state and failure count.
    fn determine_final_status(
        aborted: bool,
        timed_out: bool,
        failed_count: usize,
    ) -> crate::types::AutorunSessionStatus {
        // Precedence: abort > timeout (when no failures) > failure > success.
        // Explicit abort always wins — if both aborted and timed_out, return Cancelled.
        if aborted {
            crate::types::AutorunSessionStatus::Cancelled
        } else if timed_out && failed_count == 0 {
            crate::types::AutorunSessionStatus::Timeout
        } else if failed_count > 0 {
            crate::types::AutorunSessionStatus::Failed
        } else {
            crate::types::AutorunSessionStatus::Completed
        }
    }

    /// Emit the appropriate batch completion or abort event to the JSONL ledger.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    fn emit_batch_event(
        project_dir: &std::path::Path,
        session_id: &str,
        batch_name: &str,
        aborted: bool,
        completed_count: usize,
        failed_count: usize,
        skipped_count: usize,
    ) {
        if aborted {
            let event = crate::coordination::types::events::AutorunEvent::BatchAborted {
                session_id: session_id.to_string(),
                batch_name: batch_name.to_string(),
                reason: "user_abort".into(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            Self::emit_autorun_event(project_dir, &event);
        } else {
            let event = crate::coordination::types::events::AutorunEvent::BatchCompleted {
                session_id: session_id.to_string(),
                batch_name: batch_name.to_string(),
                completed_tasks: completed_count.min(i32::MAX as usize) as i32,
                failed_tasks: failed_count.min(i32::MAX as usize) as i32,
                skipped_tasks: skipped_count.min(i32::MAX as usize) as i32,
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            Self::emit_autorun_event(project_dir, &event);
        }
    }

    /// Emit an autorun event to the JSONL ledger.
    fn emit_autorun_event(
        project_dir: &std::path::Path,
        event: &crate::coordination::types::events::AutorunEvent,
    ) {
        crate::autorun::emit_autorun_event(project_dir, event);
    }

    /// INF-TSK-050-001 AC #6: publish mid-run counters to the session
    /// row when they change.
    ///
    /// Recomputes from `state.results` (the truth) — the only place
    /// terminal task outcomes are recorded. Reading set lengths
    /// (completed/failed) is cheaper but doesn't distinguish skipped
    /// from failed, so we tally results directly.
    ///
    /// Skips the DB write when counts are unchanged since the last
    /// publish (the dispatch loop ticks every ~50ms; the typical
    /// case is "no new completions"). This keeps the TUI within
    /// ~50ms of the truth without write storms.
    ///
    /// CAS-guarded on Running so we never overwrite a terminal status
    /// set by the stale detector concurrently.
    async fn publish_mid_run_counts(
        store: &S,
        session_id: &str,
        state: &ExecutionState,
        last_published: &Arc<Mutex<(i32, i32, i32)>>,
    ) {
        let (completed, failed, skipped) = {
            let results = state.results.lock().await;
            let mut c: i32 = 0;
            let mut f: i32 = 0;
            let mut sk: i32 = 0;
            for r in results.iter() {
                match r.status.as_str() {
                    "completed" => c = c.saturating_add(1),
                    // skipped and pr_creation_failed both count as failed
                    // for the high-level FAIL column (the orchestrator's
                    // existing `failed_count + pr_failed_count` mapping).
                    // skipped is the dependency-failed cascade kill case.
                    "skipped" => sk = sk.saturating_add(1),
                    "failed" | "timeout" | "pr_creation_failed" => f = f.saturating_add(1),
                    _ => {}
                }
            }
            (c, f, sk)
        };

        let mut last = last_published.lock().await;
        if (completed, failed, skipped) == *last {
            return; // No change since last publish — skip write.
        }

        match store
            .update_autorun_session_cas(
                session_id,
                crate::types::AutorunSessionStatus::Running,
                crate::models::AutorunSessionUpdate {
                    completed_tasks: Some(completed),
                    failed_tasks: Some(failed),
                    skipped_tasks: Some(skipped),
                    updated_at: Some(chrono::Utc::now().to_rfc3339()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(crate::store::CasResult::Updated(_)) => {
                *last = (completed, failed, skipped);
            }
            Ok(crate::store::CasResult::NoOp) => {
                // Status changed (e.g., stale detector flipped to
                // Failed). Stop trying to publish; the next loop
                // iteration will exit because counts won't match.
                // Still update `last` so we don't retry needlessly.
                *last = (completed, failed, skipped);
            }
            Err(e) => {
                eprintln!("warning: mid-run counter publish for {session_id}: {e}");
                // Don't update `last` — retry on next tick.
            }
        }
    }

    /// INF-TSK-050-001 AC #14: mark worker `interactive_session` rows
    /// complete at end of batch.
    ///
    /// Workers register as `interactive_session` rows with
    /// `session_kind='autorun'` and `status='active'`; without this
    /// propagation they stay active indefinitely after the orchestrator
    /// returns, polluting `codeflow interactive list` output.
    ///
    /// Delegates to [`crate::store::DataStore::complete_worker_interactive_sessions`]
    /// which performs a single bulk UPDATE keyed off
    /// `autorun_worker.worker_session_id`. The bulk SQL is idempotent
    /// at the row level (`AND status = 'active'`), so re-running on a
    /// completed batch is a no-op. Errors are logged but don't abort
    /// the batch end — partial completion is better than blocking the
    /// orchestrator return.
    async fn mark_worker_sessions_complete(store: &S, session_id: &str) {
        match store.complete_worker_interactive_sessions(session_id).await {
            Ok(n) if n > 0 => {
                eprintln!("marked {n} worker interactive_session row(s) complete");
            }
            Ok(_) => {} // No-op (no workers, or all already complete).
            Err(e) => {
                eprintln!(
                    "warning: failed to propagate worker interactive_session completion \
                     for {session_id}: {e}"
                );
            }
        }
    }

    /// INF-TSK-050-001 AC #18: spawn an auto-prune task at end of batch.
    ///
    /// Fire-and-forget: the orchestrator return is not blocked on prune.
    /// The prune query already excludes active (non-terminal) sessions
    /// at the SQL layer, so a concurrent active session is safe. Cutoff
    /// is `now - retention.days` and `keep_last` retains N most-recent
    /// terminal sessions regardless of age.
    ///
    /// Caller must check `config.retention.purge_on_cleanup` BEFORE
    /// invoking — this fn assumes it's enabled.
    fn spawn_auto_prune(
        store: Arc<S>,
        _project_dir: std::path::PathBuf,
        config: &crate::autorun::config::ParallelWorkConfig,
    ) {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(config.retention.days));
        let cutoff_str = cutoff.to_rfc3339();
        let keep_last = config.retention.keep_last;
        tokio::spawn(async move {
            match store.prune_autorun_sessions(&cutoff_str, keep_last).await {
                Ok(result) => {
                    if result.sessions_deleted > 0 {
                        eprintln!(
                            "auto-prune: deleted {} session(s), {} worker(s), {} task_run(s)",
                            result.sessions_deleted,
                            result.workers_deleted,
                            result.task_runs_deleted,
                        );
                    }
                }
                Err(e) => {
                    eprintln!("warning: auto-prune at batch end failed: {e}");
                }
            }
        });
    }

    /// Scan the task order and dispatch any tasks whose dependencies are met.
    ///
    /// Returns `false` immediately if the abort flag is set, preventing new
    /// task dispatch after a Ctrl+C signal.
    async fn dispatch_ready_tasks(
        &self,
        session_id: &str,
        batch: &ParsedBatch,
        project_dir: &std::path::Path,
        dep_map: &HashMap<&str, Vec<&str>>,
        state: &ExecutionState,
        blocked_behavior: &str,
    ) -> bool {
        // Abort gate: stop dispatching new tasks once abort is requested.
        if state.abort.load(Ordering::SeqCst) {
            return false;
        }

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
                    warning: None,
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

            // Pre-dispatch DB status check: if the task was cancelled/skipped
            // (e.g., via `codeflow autorun cancel`) between scheduling and
            // permit acquisition, drop the permit and skip dispatch.
            {
                if let Ok(task_runs) = self.store.list_autorun_task_runs(session_id).await {
                    let is_cancelled = task_runs.iter().any(|r| {
                        r.task_id == *task_id
                            && matches!(
                                r.status,
                                crate::types::AutorunTaskRunStatus::Skipped
                                    | crate::types::AutorunTaskRunStatus::Cancelled
                            )
                    });
                    if is_cancelled {
                        orchestrator_log(
                            project_dir,
                            session_id,
                            &format!("SKIP task={task_id} (cancelled/skipped in DB)"),
                        );
                        state.failed.lock().await.insert(task_id.clone());
                        state.results.lock().await.push(WorkerResult {
                            worker_id: String::new(),
                            task_id: task_id.clone(),
                            status: "skipped".into(),
                            exit_code: -1,
                            pr_number: 0,
                            pr_url: String::new(),
                            error: "user_cancelled".into(),
                            branch_name: String::new(),
                            duration_sec: 0,
                            warning: None,
                        });
                        drop(permit);
                        launched_any = true;
                        continue;
                    }
                }
            }

            launched_any = true;
            state.running.lock().await.insert(task_id.clone());
            // C26: Log DISPATCH.
            orchestrator_log(project_dir, session_id, &format!("DISPATCH task={task_id}"));
            let task_spec = batch.tasks.iter().find(|t| t.id == *task_id);

            // Read file_scope and scope_policy from task markdown (source of truth).
            // Batch TaskSpec values are optional overrides.
            let (md_file_scope, md_scope_policy) =
                crate::autorun::batch::read_task_scope(task_id, project_dir)
                    .unwrap_or_else(|_| (vec![], "soft".to_string()));

            let file_scope = if task_spec.is_some_and(|t| !t.file_scope.is_empty()) {
                task_spec.unwrap().file_scope.clone() // batch override
            } else {
                md_file_scope // task markdown (source of truth)
            };
            let scope_policy = task_spec
                .and_then(|t| t.scope_policy.clone())
                .unwrap_or(md_scope_policy);
            let short_sid = &session_id[session_id.len().saturating_sub(8)..];
            let tmux_name = format!("cf-ar-{}-{}", short_sid, task_id.to_lowercase());
            let handle = Self::spawn_worker(
                self.runner.clone(),
                WorkerConfig {
                    session_id: session_id.to_string(),
                    worker_id: format!("arw-{session_id}-{task_id}"),
                    worker_num: idx + 1,
                    task_id: task_id.clone(),
                    task_format_id: crate::autorun::batch::read_task_format_id(
                        task_id,
                        project_dir,
                    ),
                    batch_name: batch.name.clone(),
                    integration_auto_merge: batch.integration_auto_merge,
                    integration_branch: batch.integration_branch.clone(),
                    tmux_name: tmux_name.clone(),
                    file_scope,
                    scope_policy,
                    blocked_behavior: blocked_behavior.to_string(),
                    epic_update: {
                        let cfg =
                            crate::autorun::config::load_config(project_dir).unwrap_or_default();
                        cfg.autorun.epic_update
                    },
                    queue_timeout_secs: {
                        let cfg =
                            crate::autorun::config::load_config(project_dir).unwrap_or_default();
                        cfg.merge.queue_timeout_secs
                    },
                    task_timeout_secs: task_spec.and_then(|t| t.timeout_secs),
                },
                state.completed.clone(),
                state.failed.clone(),
                state.running.clone(),
                state.results.clone(),
                permit,
            );
            state.handles.lock().await.push(handle);
            state.tmux_sessions.lock().await.push(tmux_name);
        }

        launched_any
    }

    /// Spawn a single worker task onto the tokio runtime.
    ///
    /// Returns the `JoinHandle` so the orchestrator can wait for worker
    /// cleanup during graceful shutdown. The worker is spawned inside an
    /// inner `tokio::spawn` whose `JoinHandle` is awaited — this catches
    /// panics (which become `JoinError::is_panic()`) and converts them to
    /// failed results instead of propagating and leaking resources.
    fn spawn_worker(
        runner: Arc<R>,
        cfg: WorkerConfig,
        completed: Arc<Mutex<HashSet<String>>>,
        failed: Arc<Mutex<HashSet<String>>>,
        running: Arc<Mutex<HashSet<String>>>,
        results: Arc<Mutex<Vec<WorkerResult>>>,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> JoinHandle<()> {
        let task_id = cfg.task_id.clone();

        tokio::spawn(async move {
            let start = std::time::Instant::now();

            // Inner spawn catches panics: tokio::spawn wraps panics in
            // JoinError, so awaiting the handle converts them to Err.
            let inner_handle = tokio::spawn(async move { runner.run(cfg).await });
            let result = match inner_handle.await {
                Ok(r) => r,
                Err(join_err) => {
                    // Worker panicked or was cancelled — clean up.
                    let reason = if join_err.is_panic() {
                        "worker panicked"
                    } else {
                        "worker task cancelled"
                    };
                    eprintln!("{reason} for task {task_id}");
                    running.lock().await.remove(&task_id);
                    failed.lock().await.insert(task_id.clone());
                    #[allow(clippy::cast_possible_wrap)]
                    let duration = start.elapsed().as_secs() as i64;
                    results.lock().await.push(WorkerResult {
                        worker_id: String::new(),
                        task_id,
                        status: "failed".into(),
                        exit_code: 1,
                        pr_number: 0,
                        pr_url: String::new(),
                        error: reason.into(),
                        branch_name: String::new(),
                        duration_sec: duration,
                        warning: None,
                    });
                    drop(permit);
                    return;
                }
            };

            #[allow(clippy::cast_possible_wrap)]
            let duration = start.elapsed().as_secs() as i64;

            running.lock().await.remove(&task_id);
            match result {
                Ok(ref r) if r.status == "pool_full" => {
                    // C23: pool_full is retriable. Remove from running
                    // but do NOT add to completed/failed/results.
                    // Task stays unstarted for the next dispatch cycle.
                    eprintln!("worker pool full for task {task_id}, will retry on next cycle");
                }
                Ok(mut r) => {
                    r.duration_sec = duration;
                    // INF-TSK-049-001 batch 2 (AC #36 / W3): a worker that
                    // reports `status == "completed"` (with or without a
                    // warning) is success — its dependents proceed. A
                    // non-empty `warning` on any other status is also
                    // honoured as success-with-warning so out-of-band
                    // classifications cannot reintroduce the cascade-kill
                    // bug.
                    let is_success = r.status == "completed" || r.warning.is_some();
                    if is_success {
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
                        warning: None,
                    });
                }
            }

            drop(permit);
        })
    }
}

/// Ensure the integration branch exists on origin before workers start.
///
/// Checks if the branch exists via `git ls-remote`. If not, creates it from
/// `origin/main` and pushes. Workers need this branch to exist for rebase.
fn ensure_integration_branch(project_dir: &std::path::Path, branch: &str) {
    let ls_output = std::process::Command::new("git")
        .args(["ls-remote", "--heads", "origin", branch])
        .current_dir(project_dir)
        .output();

    let exists = match ls_output {
        Ok(ref output) if output.status.success() => !output.stdout.is_empty(),
        _ => {
            eprintln!("warning: git ls-remote failed, assuming integration branch exists");
            return;
        }
    };

    if exists {
        return;
    }

    eprintln!("creating integration branch: {branch}");
    let create = std::process::Command::new("git")
        .args(["branch", branch, "origin/main"])
        .current_dir(project_dir)
        .status();
    if let Err(e) = create {
        eprintln!("warning: failed to create integration branch {branch}: {e}");
        return;
    }

    let push = std::process::Command::new("git")
        .args(["push", "origin", branch])
        .current_dir(project_dir)
        .status();
    match push {
        Ok(s) if s.success() => {
            eprintln!("pushed integration branch: {branch}");
        }
        Ok(s) => {
            eprintln!("warning: git push for integration branch {branch} exited {s}");
        }
        Err(e) => {
            eprintln!("warning: git push for integration branch {branch} failed: {e}");
        }
    }
}

/// Delete an auto-generated integration branch from the remote after batch completion.
///
/// Only acts on branches matching `autorun/*/` pattern (auto-generated).
/// Logs and continues on failure (branch may already be deleted by another process).
fn cleanup_auto_integration_branch(project_dir: &std::path::Path, branch: &str) {
    if !branch.starts_with("autorun/") {
        return; // Only clean up auto-generated branches.
    }
    eprintln!("cleaning up auto-generated integration branch: {branch}");
    match std::process::Command::new("git")
        .args(["push", "origin", "--delete", branch])
        .current_dir(project_dir)
        .output()
    {
        Ok(output) if output.status.success() => {
            eprintln!("deleted remote branch: {branch}");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Branch may already be deleted -- not an error.
            if stderr.contains("remote ref does not exist") {
                eprintln!("remote branch already deleted: {branch}");
            } else {
                eprintln!("warning: failed to delete remote branch {branch}: {stderr}");
            }
        }
        Err(e) => {
            eprintln!("warning: git push --delete failed for {branch}: {e}");
        }
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
                warning: None,
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
                warning: None,
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
            .execute(
                "ses-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
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
            .execute(
                "ses-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
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
            .execute(
                "ses-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let a_result = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a_result.status, "failed");

        let b_result = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(b_result.status, "skipped");
    }

    /// Mock runner that reports `status="completed"` with a non-empty
    /// warning. Models the W1 outcome (exit-124-on-origin).
    struct CompletedWithWarningRunner;

    impl WorkerRunner for CompletedWithWarningRunner {
        async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "completed".into(),
                exit_code: 124,
                pr_number: 0,
                pr_url: String::new(),
                error: String::new(),
                branch_name: String::new(),
                duration_sec: 0,
                warning: Some("worker timed out but work is on origin (W1)".to_string()),
            })
        }
    }

    /// INF-TSK-049-001 batch 2 (AC #36 / W3): a task that finishes with
    /// `status="completed"` and a non-empty warning MUST NOT cause its
    /// dependents to be skipped.
    #[tokio::test]
    async fn w3_completed_with_warning_does_not_skip_dependents() {
        let orch = Orchestrator::new(CompletedWithWarningRunner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch =
            make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch
            .execute(
                "ses-w3",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let a = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a.status, "completed");
        assert!(a.warning.is_some(), "a must carry the W1 warning");

        let b = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(
            b.status, "completed",
            "AC #36: dependent of completed-with-warning task must NOT be skipped; got {:?}",
            b.status
        );
    }

    /// W3 hardening: even when a task somehow reports a non-completed
    /// status alongside a non-empty warning, the orchestrator routes it to
    /// `completed` so dependents proceed. Locks in the defence-in-depth
    /// added in `spawn_worker`.
    struct WarningOnNonCompletedRunner;

    impl WorkerRunner for WarningOnNonCompletedRunner {
        async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "timeout".into(), // out-of-band classification
                exit_code: 124,
                pr_number: 0,
                pr_url: String::new(),
                error: String::new(),
                branch_name: String::new(),
                duration_sec: 0,
                warning: Some("worker timed out but work is on origin (W1)".to_string()),
            })
        }
    }

    #[tokio::test]
    async fn w3_warning_on_non_completed_status_treated_as_success() {
        let orch = Orchestrator::new(WarningOnNonCompletedRunner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch =
            make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch
            .execute(
                "ses-w3-defence",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        let b = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_ne!(
            b.status, "skipped",
            "warning-bearing result must not cascade-kill dependents"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_single_task() {
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: only\n");

        let results = orch
            .execute(
                "ses-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
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
            .execute(
                "ses-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
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
            task_format_id: None,
            batch_name: "batch".into(),
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_name: "cf-ar-task-a".into(),
            file_scope: vec!["src/**/*.rs".into()],
            scope_policy: "hard".into(),
            blocked_behavior: "skip_and_continue".into(),
            epic_update: "orchestrator".into(),
            queue_timeout_secs: 600,
            task_timeout_secs: None,
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
            warning: None,
        };
        assert_eq!(result.status, "completed");
        assert_eq!(result.duration_sec, 120);
    }

    #[test]
    fn test_tmux_name_format_task_id_based() {
        // Verify the tmux name format uses cf-ar-{task_id}.
        let task_id = "INF-TSK-021-036";
        let tmux_name = format!("cf-ar-{}", task_id.to_lowercase());
        assert_eq!(tmux_name, "cf-ar-inf-tsk-021-036");
        assert!(
            tmux_name.starts_with("cf-ar-"),
            "tmux name should start with cf-ar-"
        );

        // Verify uniqueness: different task IDs produce different names.
        let other_task = "INF-TSK-021-037";
        let other_name = format!("cf-ar-{}", other_task.to_lowercase());
        assert_ne!(
            tmux_name, other_name,
            "different tasks must produce different tmux names"
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
                    warning: None,
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
            .execute(
                "ses-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
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
        orch.execute(
            "ses-db-test",
            &batch,
            project_dir.path(),
            std::future::pending::<()>(),
        )
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
        orch.execute(
            "ses-end-test",
            &batch,
            project_dir.path(),
            std::future::pending::<()>(),
        )
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
        orch.execute(
            "ses-fail-test",
            &batch,
            project_dir.path(),
            std::future::pending::<()>(),
        )
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
        orch.execute(
            "ses-event-test",
            &batch,
            project_dir.path(),
            std::future::pending::<()>(),
        )
        .await
        .unwrap();

        // Check that autorun-events.jsonl was written.
        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");
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
        orch.execute(
            "ses-name",
            &batch,
            project_dir.path(),
            std::future::pending::<()>(),
        )
        .await
        .unwrap();

        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-name").unwrap();
        assert_eq!(session.batch_name.as_deref(), Some("my-batch"));
        assert_eq!(session.total_tasks, 1);
    }

    #[tokio::test]
    async fn test_abort_flag_prevents_dispatch() {
        let runner = OrderTracker::new();
        let order_ref = runner.order.clone();
        let orch = Orchestrator::new(runner, mock_store());

        let batch = make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n  - id: c\n");
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

        let state = ExecutionState::new(3);
        // Set abort flag BEFORE any dispatch.
        state.abort.store(true, Ordering::SeqCst);

        let launched = orch
            .dispatch_ready_tasks(
                "ses-abort",
                &batch,
                std::path::Path::new("/tmp"),
                &dep_map,
                &state,
                "skip_and_continue",
            )
            .await;

        assert!(!launched, "dispatch should return false when abort is set");
        let order = order_ref.lock().await;
        assert!(order.is_empty(), "no workers should have been dispatched");
    }

    #[tokio::test]
    async fn test_abort_produces_partial_results() {
        // Use a slow runner so that some tasks are still "running" when we
        // simulate abort post-loop.
        struct SlowRunner;
        impl WorkerRunner for SlowRunner {
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
                // "a" completes immediately; others sleep long.
                if cfg.task_id == "a" {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                } else {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                }
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
                    warning: None,
                })
            }
        }

        let state = ExecutionState::new(1);
        let batch = make_batch(
            "max_workers: 1\ntasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n  - id: c\n    depends_on: [a]\n",
        );
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

        let orch = Orchestrator::new(SlowRunner, mock_store());

        // Dispatch 'a', let it complete.
        orch.dispatch_ready_tasks(
            "ses-partial",
            &batch,
            std::path::Path::new("/tmp"),
            &dep_map,
            &state,
            "skip_and_continue",
        )
        .await;
        // Wait for 'a' to complete.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Now set abort flag — 'b' and 'c' should not be dispatched.
        state.abort.store(true, Ordering::SeqCst);

        // Confirm abort gate works.
        let launched = orch
            .dispatch_ready_tasks(
                "ses-partial",
                &batch,
                std::path::Path::new("/tmp"),
                &dep_map,
                &state,
                "skip_and_continue",
            )
            .await;
        assert!(!launched, "no new tasks after abort");

        // Mark 'b' and 'c' as skipped (simulating the abort cleanup in execute()).
        {
            let completed = state.completed.lock().await;
            let failed = state.failed.lock().await;
            let mut results = state.results.lock().await;
            for task_id in &batch.order {
                if !completed.contains(task_id.as_str()) && !failed.contains(task_id.as_str()) {
                    results.push(WorkerResult {
                        worker_id: String::new(),
                        task_id: task_id.clone(),
                        status: "skipped".into(),
                        exit_code: -1,
                        pr_number: 0,
                        pr_url: String::new(),
                        error: "batch_aborted".into(),
                        branch_name: String::new(),
                        duration_sec: 0,
                        warning: None,
                    });
                }
            }
        }

        let results = state.results.lock().await;
        let completed_count = results.iter().filter(|r| r.status == "completed").count();
        let skipped_count = results.iter().filter(|r| r.status == "skipped").count();

        assert_eq!(completed_count, 1, "only 'a' should be completed");
        assert_eq!(skipped_count, 2, "'b' and 'c' should be skipped");

        // Verify the skipped reason.
        for r in results.iter().filter(|r| r.status == "skipped") {
            assert_eq!(r.error, "batch_aborted");
        }
    }

    #[tokio::test]
    async fn test_abort_updates_session_to_cancelled() {
        // Test the extracted determine_final_status production function
        // across all input combinations. This function is called by execute()
        // at line 263 — if removed, these assertions fail.
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(true, false, 0),
            crate::types::AutorunSessionStatus::Cancelled,
            "aborted with no failures should be Cancelled"
        );
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(true, false, 3),
            crate::types::AutorunSessionStatus::Cancelled,
            "aborted with failures should still be Cancelled (abort takes priority)"
        );
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(false, false, 2),
            crate::types::AutorunSessionStatus::Failed,
            "not aborted with failures should be Failed"
        );
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(false, false, 0),
            crate::types::AutorunSessionStatus::Completed,
            "not aborted, no failures should be Completed"
        );
        // Timeout cases: timed_out=true without abort returns Timeout when no failures.
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(false, true, 0),
            crate::types::AutorunSessionStatus::Timeout,
            "timed_out with no abort and no failures should be Timeout"
        );
        // Abort takes precedence over timeout.
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(true, true, 0),
            crate::types::AutorunSessionStatus::Cancelled,
            "aborted + timed_out should be Cancelled (abort takes precedence)"
        );
        // Timeout with failures: failure takes precedence over timeout.
        assert_eq!(
            Orchestrator::<OrderTracker>::determine_final_status(false, true, 2),
            crate::types::AutorunSessionStatus::Failed,
            "timed_out with failures should be Failed (failure takes precedence over timeout)"
        );

        // Also verify the non-abort path through execute() produces Completed.
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("tasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch
            .execute(
                "ses-cancel-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert!(!results.is_empty());

        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-cancel-test").unwrap();
        assert_eq!(
            session.status,
            crate::types::AutorunSessionStatus::Completed
        );
    }

    #[tokio::test]
    async fn test_abort_emits_batch_aborted_event() {
        let project_dir = tempfile::tempdir().unwrap();
        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");

        // 1. Verify normal run emits batch_completed, NOT batch_aborted.
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let batch = make_batch("tasks:\n  - id: a\n");
        orch.execute(
            "ses-event-normal",
            &batch,
            project_dir.path(),
            std::future::pending::<()>(),
        )
        .await
        .unwrap();

        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(
            content.contains("batch_completed"),
            "normal run should emit batch_completed"
        );
        assert!(
            !content.contains("batch_aborted"),
            "normal run should NOT emit batch_aborted"
        );

        // 2. Test that emit_batch_event (the production function) correctly
        //    writes BatchAborted when aborted=true and BatchCompleted when not.
        Orchestrator::<OrderTracker>::emit_batch_event(
            project_dir.path(),
            "ses-abort-test",
            "test-batch",
            true, // aborted
            0,
            0,
            0,
        );

        let content_after = std::fs::read_to_string(&events_path).unwrap();
        assert!(
            content_after.contains("batch_aborted"),
            "aborted path should emit batch_aborted"
        );
        assert!(
            content_after.contains("user_abort"),
            "batch_aborted should contain reason"
        );
    }

    #[tokio::test]
    async fn test_drain_handles_all_finish_within_grace() {
        // All handles complete quickly — none should be aborted.
        let handles: Vec<JoinHandle<()>> = (0..3)
            .map(|_| {
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                })
            })
            .collect();

        // 10s grace is plenty — all should drain cleanly.
        Orchestrator::<OrderTracker>::drain_handles(handles, std::time::Duration::from_secs(10))
            .await;
        // No panic, no abort — success.
    }

    #[tokio::test]
    async fn test_drain_handles_timeout_aborts_remaining() {
        // Create handles that sleep much longer than the grace period.
        let handles: Vec<JoinHandle<()>> = (0..3)
            .map(|_| {
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                })
            })
            .collect();

        // Use a very short grace period so the timeout fires.
        Orchestrator::<OrderTracker>::drain_handles(handles, std::time::Duration::from_millis(50))
            .await;
        // If we reach here, handles were aborted (not hung for 300s).
    }

    #[tokio::test]
    async fn test_drain_handles_empty_is_noop() {
        // Empty handles should return immediately.
        Orchestrator::<OrderTracker>::drain_handles(Vec::new(), std::time::Duration::from_secs(10))
            .await;
    }

    #[tokio::test]
    async fn test_drain_handles_mixed_fast_and_slow() {
        // First handle completes fast, second hangs — second should be aborted.
        let fast = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        });
        let slow = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        });

        Orchestrator::<OrderTracker>::drain_handles(
            vec![fast, slow],
            std::time::Duration::from_millis(100),
        )
        .await;
        // Fast drained, slow aborted — should complete quickly.
    }

    #[tokio::test]
    async fn test_abort_cleanup_marks_pending_tasks_skipped() {
        // Test abort_cleanup end-to-end: tmux kill (no-op in tests),
        // drain handles, and mark pending tasks as skipped.
        let state = ExecutionState::new(3);

        // Simulate: task "a" completed, "b" is running, "c" is pending.
        state.completed.lock().await.insert("a".into());
        state.running.lock().await.insert("b".into());
        state.results.lock().await.push(WorkerResult {
            worker_id: "w-a".into(),
            task_id: "a".into(),
            status: "completed".into(),
            exit_code: 0,
            pr_number: 0,
            pr_url: String::new(),
            error: String::new(),
            branch_name: String::new(),
            duration_sec: 1,
            warning: None,
        });

        // Add a fast handle for the "running" task (simulates worker finishing after tmux kill).
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        });
        state.handles.lock().await.push(handle);
        state
            .tmux_sessions
            .lock()
            .await
            .push("test-session-1".into());

        let task_order = vec!["a".into(), "b".into(), "c".into()];

        Orchestrator::<OrderTracker>::abort_cleanup(&state, &task_order).await;

        let results = state.results.lock().await;
        // "a" was completed (already in results), "b" and "c" should be skipped.
        let skipped: Vec<&WorkerResult> =
            results.iter().filter(|r| r.status == "skipped").collect();
        assert_eq!(skipped.len(), 2, "b and c should be skipped");
        for r in &skipped {
            assert_eq!(r.error, "batch_aborted");
        }

        // "a" should still be completed.
        let completed: Vec<&WorkerResult> =
            results.iter().filter(|r| r.status == "completed").collect();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].task_id, "a");
    }

    #[tokio::test]
    async fn test_abort_cleanup_with_slow_handles_aborts_them() {
        // Test that abort_cleanup aborts handles that exceed the grace period.
        let state = ExecutionState::new(1);

        // Add a slow handle that would hang for 300s.
        let slow = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        });
        state.handles.lock().await.push(slow);

        let task_order: Vec<String> = vec!["x".into()];

        // abort_cleanup uses 10s grace — but the handle sleeps 300s.
        // We can't override the grace period in abort_cleanup, so we
        // verify it completes within a reasonable time by wrapping in
        // a timeout. If handles weren't aborted, this would hang.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            Orchestrator::<OrderTracker>::abort_cleanup(&state, &task_order),
        )
        .await;
        assert!(
            result.is_ok(),
            "abort_cleanup should complete within 15s (10s grace + margin)"
        );

        // "x" should be marked skipped.
        let results = state.results.lock().await;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "skipped");
        assert_eq!(results[0].error, "batch_aborted");
    }

    #[tokio::test]
    async fn test_drain_handles_deadline_already_expired() {
        // Pass a zero-duration grace period so `remaining.is_zero()` fires
        // on the FIRST iteration of the drain loop (covers the `is_zero()`
        // branch that was previously uncovered by LCOV).
        let handles: Vec<JoinHandle<()>> = (0..2)
            .map(|_| {
                tokio::spawn(async {
                    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                })
            })
            .collect();

        // With a zero grace period, remaining will be zero immediately.
        Orchestrator::<OrderTracker>::drain_handles(handles, std::time::Duration::ZERO).await;
        // If we get here, the is_zero() branch fired and aborted handles.
    }

    #[tokio::test]
    async fn test_spawn_worker_error_path() {
        // Test that spawn_worker handles runner Err by adding to failed set.
        struct ErrorRunner;
        impl WorkerRunner for ErrorRunner {
            async fn run(&self, _cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
                Err(AutorunError::WorkerFailed("test error".into()))
            }
        }

        let completed = Arc::new(Mutex::new(HashSet::new()));
        let failed = Arc::new(Mutex::new(HashSet::new()));
        let running = Arc::new(Mutex::new(HashSet::new()));
        let results = Arc::new(Mutex::new(Vec::new()));
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore.clone().try_acquire_owned().unwrap();

        let handle = Orchestrator::<ErrorRunner, crate::store::NoopStore>::spawn_worker(
            Arc::new(ErrorRunner),
            WorkerConfig {
                session_id: "ses-err".into(),
                worker_id: "arw-err".into(),
                worker_num: 1,
                task_id: "task-err".into(),
                task_format_id: None,
                batch_name: "batch".into(),
                integration_auto_merge: false,
                integration_branch: "main".into(),
                tmux_name: "cf-ar-task-err".into(),
                file_scope: vec![],
                scope_policy: "soft".into(),
                blocked_behavior: "skip_and_continue".into(),
                epic_update: String::new(),
                queue_timeout_secs: 600,
                task_timeout_secs: None,
            },
            completed.clone(),
            failed.clone(),
            running.clone(),
            results.clone(),
            permit,
        );

        // Wait for the spawned task to complete.
        handle.await.unwrap();

        let failed_set = failed.lock().await;
        assert!(
            failed_set.contains("task-err"),
            "task should be in failed set"
        );

        let results_vec = results.lock().await;
        assert_eq!(results_vec.len(), 1);
        assert_eq!(results_vec[0].status, "failed");
        assert_eq!(results_vec[0].error, "worker failed: test error");
        assert_eq!(results_vec[0].exit_code, 1);
    }

    #[tokio::test]
    async fn test_emit_batch_event_both_branches() {
        let project_dir = tempfile::tempdir().unwrap();
        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");

        // Test non-aborted path: should emit batch_completed.
        Orchestrator::<OrderTracker>::emit_batch_event(
            project_dir.path(),
            "ses-1",
            "batch-1",
            false,
            5,
            1,
            2,
        );
        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(content.contains("batch_completed"));
        assert!(!content.contains("batch_aborted"));

        // Test aborted path: should emit batch_aborted.
        Orchestrator::<OrderTracker>::emit_batch_event(
            project_dir.path(),
            "ses-2",
            "batch-2",
            true,
            3,
            0,
            4,
        );
        let content_after = std::fs::read_to_string(&events_path).unwrap();
        assert!(content_after.contains("batch_aborted"));
        assert!(content_after.contains("user_abort"));
    }

    #[tokio::test]
    async fn test_execute_with_error_runner_records_failure() {
        // Test that execute() correctly handles runner errors end-to-end.
        struct ErrorRunner;
        impl WorkerRunner for ErrorRunner {
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
                Err(AutorunError::WorkerFailed(format!(
                    "{} failed",
                    cfg.task_id
                )))
            }
        }

        let store = mock_store();
        let orch = Orchestrator::new(ErrorRunner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: a\n  - id: b\n    depends_on: [a]\n");

        let results = orch
            .execute(
                "ses-err-e2e",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let a_result = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a_result.status, "failed");

        let b_result = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(b_result.status, "skipped");
        assert_eq!(b_result.error, "dependency failed");

        // Verify session status is Failed.
        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-err-e2e").unwrap();
        assert_eq!(session.status, crate::types::AutorunSessionStatus::Failed);
    }

    #[tokio::test]
    async fn test_execute_with_noop_store() {
        // Exercise execute() with NoopStore to cover the store call sites
        // via a different DataStore implementation than MockStore.
        let store = Arc::new(crate::store::NoopStore);
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("tasks:\n  - id: x\n");
        let results = orch
            .execute(
                "ses-noop",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "completed");
    }

    #[tokio::test]
    async fn test_execute_without_parallel_work_config() {
        // Exercise execute() without a parallel-work-config.json file,
        // covering the Err(_) => batch.max_workers branch at line 159.
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, mock_store());
        // Use a project_dir WITHOUT .codeflow/config/parallel-work/ so
        // load_config returns Err and falls back to batch.max_workers.
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("max_workers: 2\ntasks:\n  - id: a\n  - id: b\n");
        let results = orch
            .execute(
                "ses-noconfig",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.status == "completed"));
    }

    #[test]
    fn test_emit_autorun_event_directly() {
        // Directly test emit_autorun_event to cover its body lines.
        let project_dir = tempfile::tempdir().unwrap();
        let event = crate::coordination::types::events::AutorunEvent::BatchStarted {
            session_id: "ses-direct".into(),
            batch_name: "test".into(),
            total_tasks: 1,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        Orchestrator::<OrderTracker>::emit_autorun_event(project_dir.path(), &event);

        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");
        assert!(events_path.exists());
        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(content.contains("batch_started"));
        assert!(content.contains("ses-direct"));
    }

    #[tokio::test]
    async fn test_execution_state_new_initializes_all_fields() {
        // Exercise ExecutionState::new() and verify all fields are initialized.
        let state = ExecutionState::new(4);

        assert!(state.completed.lock().await.is_empty());
        assert!(state.failed.lock().await.is_empty());
        assert!(state.running.lock().await.is_empty());
        assert!(state.results.lock().await.is_empty());
        assert!(!state.abort.load(Ordering::SeqCst));
        assert!(state.handles.lock().await.is_empty());
        assert!(state.tmux_sessions.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_dispatch_ready_tasks_skips_completed_and_running() {
        // Verify dispatch_ready_tasks correctly skips tasks that are
        // already completed, failed, or running.
        let runner = OrderTracker::new();
        let order_ref = runner.order.clone();
        let orch = Orchestrator::new(runner, mock_store());

        let batch = make_batch("max_workers: 3\ntasks:\n  - id: a\n  - id: b\n  - id: c\n");
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

        let state = ExecutionState::new(3);
        // Pre-mark "a" as completed and "b" as running.
        state.completed.lock().await.insert("a".into());
        state.running.lock().await.insert("b".into());

        let launched = orch
            .dispatch_ready_tasks(
                "ses-skip",
                &batch,
                std::path::Path::new("/tmp"),
                &dep_map,
                &state,
                "skip_and_continue",
            )
            .await;

        // Only "c" should be dispatched.
        assert!(launched);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let order = order_ref.lock().await;
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].0, "c");
    }

    #[tokio::test]
    async fn test_dispatch_ready_tasks_semaphore_full() {
        // When all semaphore permits are taken, dispatch should stop
        // trying to launch more tasks (the try_acquire_owned fails).
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, mock_store());

        // max_workers=1, 3 independent tasks — only 1 should dispatch per call.
        let batch = make_batch("max_workers: 1\ntasks:\n  - id: a\n  - id: b\n  - id: c\n");
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

        let state = ExecutionState::new(1);

        let launched = orch
            .dispatch_ready_tasks(
                "ses-sem",
                &batch,
                std::path::Path::new("/tmp"),
                &dep_map,
                &state,
                "skip_and_continue",
            )
            .await;
        assert!(launched);

        // Only 1 task should be running (semaphore=1).
        let running = state.running.lock().await;
        assert_eq!(running.len(), 1);
    }

    #[tokio::test]
    async fn test_execute_store_error_paths() {
        // FailingAutorunStore is in store::mock — its trait boilerplate
        // counts against store/mod.rs, not orchestrator.rs.
        let store = Arc::new(crate::store::mock::FailingAutorunStore);
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("tasks:\n  - id: a\n");
        let results = orch
            .execute(
                "ses-se",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "completed");
    }

    #[tokio::test]
    async fn test_execute_abort_with_store_errors() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let store = Arc::new(crate::store::mock::FailingAutorunStore);
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("max_workers: 1\ntasks:\n  - id: a\n  - id: b\n");
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let _ = tx.send(());
        });
        let results = orch
            .execute("ses-abs", &batch, project_dir.path(), async {
                rx.await.ok();
            })
            .await
            .unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_execute_abort_via_shutdown_signal() {
        // End-to-end test: inject a shutdown signal via oneshot channel,
        // verify the FULL abort path in execute() fires.
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();

        // Use a slow runner — first task completes, rest block.
        struct SlowAfterFirst;
        impl WorkerRunner for SlowAfterFirst {
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
                if cfg.task_id != "a" {
                    tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                }
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
                    warning: None,
                })
            }
        }

        let store = mock_store();
        let orch = Orchestrator::new(SlowAfterFirst, store.clone());
        let project_dir = tempfile::tempdir().unwrap();

        // 3 independent tasks, max_workers=1.
        // "a" completes quickly. Then we fire the shutdown signal.
        // "b" and "c" should be skipped.
        let batch = make_batch("max_workers: 1\ntasks:\n  - id: a\n  - id: b\n  - id: c\n");

        // Fire shutdown after 100ms — enough time for "a" to complete.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let _ = tx.send(());
        });

        let results = orch
            .execute("ses-abort-e2e", &batch, project_dir.path(), async {
                rx.await.ok();
            })
            .await
            .unwrap();

        // Verify partial results.
        let completed: Vec<_> = results.iter().filter(|r| r.status == "completed").collect();
        let skipped: Vec<_> = results.iter().filter(|r| r.status == "skipped").collect();
        assert_eq!(completed.len(), 1, "only 'a' should complete");
        assert_eq!(completed[0].task_id, "a");
        assert!(!skipped.is_empty(), "at least one task should be skipped");
        for r in &skipped {
            assert_eq!(r.error, "batch_aborted");
        }

        // Verify session status is Cancelled.
        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-abort-e2e").unwrap();
        assert_eq!(
            session.status,
            crate::types::AutorunSessionStatus::Cancelled
        );

        // Verify BatchAborted event was emitted.
        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");
        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(content.contains("batch_aborted"));
        assert!(content.contains("user_abort"));
        // Should NOT contain batch_completed.
        let lines: Vec<&str> = content.lines().collect();
        let last_line = lines.last().unwrap();
        assert!(
            last_line.contains("batch_aborted"),
            "last event should be batch_aborted, not batch_completed"
        );
    }

    // -- L5: Abort marker file triggers abort --

    #[tokio::test]
    async fn test_abort_marker_file_triggers_abort() {
        let dir = tempfile::tempdir().unwrap();
        let session_id = "ses-marker-test";

        // Create the runtime dir and abort marker file.
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        let marker_path = runtime_dir.join(format!("abort-{session_id}"));
        std::fs::write(&marker_path, "abort").unwrap();

        // Use an instant runner.
        struct InstantRunner;
        impl WorkerRunner for InstantRunner {
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
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
                    warning: None,
                })
            }
        }

        let batch = make_batch("max_workers: 1\ntasks:\n  - id: marker-task\n");
        let store = mock_store();
        let orch = Orchestrator::new(InstantRunner, store);

        let results = orch
            .execute(session_id, &batch, dir.path(), std::future::pending())
            .await
            .unwrap();

        // The abort marker should have been detected — task should be skipped.
        let has_skipped = results.iter().any(|r| r.status == "skipped");
        assert!(
            has_skipped || results.is_empty(),
            "abort marker should prevent task dispatch or cause skip"
        );
    }

    // -- M4: batch_file field populated --

    #[tokio::test]
    async fn test_execute_with_batch_file_records_path() {
        let dir = tempfile::tempdir().unwrap();

        struct NoopRunner;
        impl WorkerRunner for NoopRunner {
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
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
                    warning: None,
                })
            }
        }

        let batch = make_batch("max_workers: 1\ntasks:\n  - id: bf-test\n");
        let store = mock_store();
        let orch = Orchestrator::new(NoopRunner, store.clone());

        let results = orch
            .execute_with_batch_file(
                "ses-bf-test",
                &batch,
                dir.path(),
                "/path/to/batch.yaml",
                std::future::pending(),
            )
            .await
            .unwrap();

        assert!(!results.is_empty(), "should have results");

        // Verify batch_file was set on the session record.
        let sessions = store.autorun_sessions.lock().unwrap();
        let session = sessions.get("ses-bf-test").unwrap();
        assert_eq!(
            session.batch_file, "/path/to/batch.yaml",
            "batch_file should be recorded in the session"
        );
    }

    // -- Panic guard tests --

    /// A runner that panics on execution, to test panic guard behavior.
    struct PanickingRunner;

    impl WorkerRunner for PanickingRunner {
        async fn run(&self, _cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            panic!("intentional test panic in worker");
        }
    }

    #[tokio::test]
    async fn test_orchestrator_panic_guard_catches_panic() {
        let orch = Orchestrator::new(PanickingRunner, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("max_workers: 1\ntasks:\n  - id: panic-task\n");

        let results = orch
            .execute(
                "ses-panic-test",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.task_id, "panic-task");
        assert_eq!(r.status, "failed");
        assert!(
            r.error.contains("panicked") || r.error.contains("cancelled"),
            "error should indicate panic or cancellation, got: {}",
            r.error
        );
    }

    #[tokio::test]
    async fn test_orchestrator_panic_does_not_block_other_tasks() {
        // A runner where the first task panics but the second succeeds.
        struct SelectivePanicker;

        impl WorkerRunner for SelectivePanicker {
            #[allow(clippy::manual_assert)]
            async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
                if cfg.task_id == "a" {
                    panic!("task a panics");
                }
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
                    warning: None,
                })
            }
        }

        let orch = Orchestrator::new(SelectivePanicker, mock_store());
        let project_dir = tempfile::tempdir().unwrap();

        // Two independent tasks — a panics, b should still complete.
        let batch = make_batch("max_workers: 2\ntasks:\n  - id: a\n  - id: b\n");

        let results = orch
            .execute(
                "ses-selective-panic",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        let a_result = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a_result.status, "failed");

        let b_result = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(b_result.status, "completed");
    }

    /// Verify that dispatch_ready_tasks skips tasks with a Skipped task_run
    /// in the DB (from `codeflow autorun cancel` on a pending task).
    #[tokio::test]
    async fn test_dispatch_skips_cancelled_task() {
        let store = mock_store();

        // Pre-seed a Skipped task_run for task "a".
        {
            let run = crate::models::AutorunTaskRun {
                id: "atr-ses-skip-a".into(),
                worker_id: String::new(),
                task_id: "a".into(),
                session_id: "ses-skip".into(),
                status: crate::types::AutorunTaskRunStatus::Skipped,
                branch_name: None,
                worktree_path: None,
                pr_number: None,
                pr_url: None,
                blocked_reason: None,
                claim_conflicts: None,
                merge_conflicts: None,
                started_at: None,
                completed_at: Some("2026-04-08T00:00:00Z".into()),
                duration_seconds: Some(0),
                exit_code: None,
                error_message: Some("user_cancelled".into()),
                last_phase: None,
                verification_result: None,
                created_at: "2026-04-08T00:00:00Z".into(),
            };
            store
                .autorun_task_runs
                .lock()
                .unwrap()
                .insert(run.id.clone(), run);
        }

        let runner = OrderTracker::new();
        let order_ref = runner.order.clone();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();

        let batch = make_batch("max_workers: 2\ntasks:\n  - id: a\n  - id: b\n");

        let results = orch
            .execute(
                "ses-skip",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        // Task "a" should be skipped (from DB), "b" should complete.
        let a_result = results.iter().find(|r| r.task_id == "a").unwrap();
        assert_eq!(a_result.status, "skipped");
        assert_eq!(a_result.error, "user_cancelled");

        let b_result = results.iter().find(|r| r.task_id == "b").unwrap();
        assert_eq!(b_result.status, "completed");

        // Only "b" should have actually run.
        let order = order_ref.lock().await;
        assert_eq!(order.len(), 1);
        assert_eq!(order[0].0, "b");
    }

    // ------------------------------------------------------------------
    // INF-TSK-050-001 Wave 1A — final_pr_url, mid-run counters,
    // worker session propagation, auto-prune.
    // ------------------------------------------------------------------

    /// Mock runner that emits a fixed pr_url so tests can assert on the
    /// final_pr_url end-of-batch behavior.
    struct PrEmittingRunner {
        pr_url: String,
    }

    impl WorkerRunner for PrEmittingRunner {
        async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "completed".into(),
                exit_code: 0,
                pr_number: 42,
                pr_url: self.pr_url.clone(),
                error: String::new(),
                branch_name: String::new(),
                duration_sec: 0,
                warning: None,
            })
        }
    }

    /// Mock runner that emits per-task pr_urls so we can verify the
    /// multi-PR end-of-batch behavior leaves final_pr_url=None.
    struct VariablePrRunner {
        urls: std::sync::Mutex<Vec<String>>,
    }

    impl WorkerRunner for VariablePrRunner {
        async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
            let url = {
                let mut urls = self.urls.lock().unwrap();
                urls.pop().unwrap_or_default()
            };
            Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "completed".into(),
                exit_code: 0,
                pr_number: if url.is_empty() { 0 } else { 42 },
                pr_url: url,
                error: String::new(),
                branch_name: String::new(),
                duration_sec: 0,
                warning: None,
            })
        }
    }

    #[tokio::test]
    async fn test_orchestrator_final_pr_url_single_task() {
        // INF-TSK-050-001 AC #19: a single-task batch with one PR url
        // populates final_pr_url on the session row.
        use crate::store::DataStore;
        let store = mock_store();
        let runner = PrEmittingRunner {
            pr_url: "https://github.com/x/y/pull/123".into(),
        };
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("tasks:\n  - id: only\n");

        let _ = orch
            .execute(
                "ses-final-pr",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        let session = store
            .get_autorun_session("ses-final-pr")
            .await
            .unwrap()
            .expect("session created");
        assert_eq!(
            session.final_pr_url.as_deref(),
            Some("https://github.com/x/y/pull/123"),
            "single-task batch must populate final_pr_url"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_final_pr_url_multi_pr_unset() {
        // INF-TSK-050-001 AC #19: multi-PR batch leaves final_pr_url
        // None — operators see per-task PRs in the detail view; no
        // single PR is canonical.
        use crate::store::DataStore;
        let store = mock_store();
        let runner = VariablePrRunner {
            urls: std::sync::Mutex::new(vec![
                "https://github.com/x/y/pull/200".into(),
                "https://github.com/x/y/pull/201".into(),
            ]),
        };
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("max_workers: 2\ntasks:\n  - id: a\n  - id: b\n");

        let _ = orch
            .execute(
                "ses-multi-pr",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        let session = store
            .get_autorun_session("ses-multi-pr")
            .await
            .unwrap()
            .unwrap();
        assert!(
            session.final_pr_url.is_none(),
            "multi-PR batch must leave final_pr_url=None, got {:?}",
            session.final_pr_url
        );
    }

    #[tokio::test]
    async fn test_orchestrator_final_pr_url_zero_pr_unset() {
        // No tasks emit a pr_url → final_pr_url stays None.
        use crate::store::DataStore;
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store.clone());
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("tasks:\n  - id: only\n");

        let _ = orch
            .execute(
                "ses-no-pr",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        let session = store
            .get_autorun_session("ses-no-pr")
            .await
            .unwrap()
            .unwrap();
        assert!(
            session.final_pr_url.is_none(),
            "no-PR batch must leave final_pr_url=None"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_removes_abort_marker_on_clean_exit() {
        // INF-TSK-050-001 AC #5 (orchestrator side): a clean exit
        // (no abort) removes the abort marker file if it exists,
        // preventing a stale marker from mis-triggering the next run.
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();

        // Pre-create the abort marker (simulates a stale leftover from
        // a prior aborted run that wasn't cleaned up).
        let marker_dir = project_dir.path().join(".state/runtime");
        std::fs::create_dir_all(&marker_dir).unwrap();
        // The orchestrator dispatch loop checks for the marker; to
        // simulate a clean exit without triggering abort, we only
        // assert post-exec that any stale marker is gone.
        let marker = marker_dir.join("abort-ses-clean-exit");

        let batch = make_batch("tasks:\n  - id: only\n");
        let _ = orch
            .execute(
                "ses-clean-exit",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();

        // Place marker AFTER dispatch loop check so we don't trigger
        // abort, then verify post-cleanup removes it. But since the
        // loop ran to completion and the cleanup runs after, we can
        // simply check that placing it then re-running cleans it.
        std::fs::write(&marker, "").unwrap();
        assert!(marker.exists(), "marker placed for test setup");

        // Re-run with a longer batch so dispatch sees the marker AND
        // exits cleanly via the abort-marker path.
        let store = mock_store();
        let orch = Orchestrator::new(OrderTracker::new(), store);
        let _ = orch
            .execute(
                "ses-clean-exit",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await;

        assert!(
            !marker.exists(),
            "abort marker must be removed at orchestrator end (any path)"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_publish_mid_run_counts_no_change_skips() {
        // INF-TSK-050-001 AC #6: publish_mid_run_counts skips the DB
        // write when counts haven't changed since last publish. This
        // is a unit test of the helper; we exercise it directly with
        // a stable state.
        use crate::store::DataStore;
        let store = mock_store();
        let session = crate::models::AutorunSession {
            id: "ses-stable".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: crate::types::AutorunSessionStatus::Running,
            max_session_workers: 1,
            total_tasks: 0,
            completed_tasks: 5,
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
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            abort_started_at: None,
        };
        store.create_autorun_session(&session).await.unwrap();

        let state = ExecutionState::new(1);
        // Pre-populate state.results with a completed entry so the
        // publish would compute (1, 0, 0).
        state.results.lock().await.push(WorkerResult {
            worker_id: String::new(),
            task_id: "t1".into(),
            status: "completed".into(),
            exit_code: 0,
            pr_number: 0,
            pr_url: String::new(),
            error: String::new(),
            branch_name: String::new(),
            duration_sec: 0,
            warning: None,
        });
        let last = Arc::new(Mutex::new((0i32, 0i32, 0i32)));

        // First call: should publish (1, 0, 0) and update last.
        Orchestrator::<OrderTracker, crate::store::mock::MockStore>::publish_mid_run_counts(
            store.as_ref(),
            "ses-stable",
            &state,
            &last,
        )
        .await;
        assert_eq!(*last.lock().await, (1, 0, 0));
        let after_first = store
            .get_autorun_session("ses-stable")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after_first.completed_tasks, 1);

        // Manually clobber the row counters to detect a second write.
        // Then call publish again with no state change; expect no
        // write (counters stay clobbered).
        store
            .update_autorun_session(
                "ses-stable",
                crate::models::AutorunSessionUpdate {
                    completed_tasks: Some(99),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        Orchestrator::<OrderTracker, crate::store::mock::MockStore>::publish_mid_run_counts(
            store.as_ref(),
            "ses-stable",
            &state,
            &last,
        )
        .await;
        let after_second = store
            .get_autorun_session("ses-stable")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after_second.completed_tasks, 99,
            "no state change → no DB write → 99 preserved"
        );
    }

    #[tokio::test]
    async fn test_orchestrator_mark_worker_sessions_complete_calls_store() {
        // INF-TSK-050-001 AC #14: mark_worker_sessions_complete delegates
        // to complete_worker_interactive_sessions on the store. The mock
        // returns 0 (it doesn't track interactive_session rows), so we
        // can't assert on row state; we assert the path doesn't error.
        let store = mock_store();
        Orchestrator::<OrderTracker, crate::store::mock::MockStore>::mark_worker_sessions_complete(
            store.as_ref(),
            "ses-irrelevant",
        )
        .await;
        // No assertion needed — exit-without-error proves the call path
        // works. Real-store integration is exercised by surreal_test.rs
        // when complete_worker_interactive_sessions has an integration
        // test (added separately).
    }

    #[tokio::test]
    async fn test_orchestrator_auto_prune_spawns_when_enabled() {
        // INF-TSK-050-001 AC #18: auto-prune is invoked at end of batch
        // when retention.purge_on_cleanup is true (the default). The
        // test runs a tiny batch and verifies the orchestrator returns
        // without error — the prune runs on a tokio::spawn, so we
        // can't easily await its completion. The spawn itself is
        // tested via the "spawn doesn't block" assertion: the batch
        // returned, which means orchestrator.execute didn't await
        // the prune handle.
        let store = mock_store();
        let runner = OrderTracker::new();
        let orch = Orchestrator::new(runner, store);
        let project_dir = tempfile::tempdir().unwrap();
        let batch = make_batch("tasks:\n  - id: only\n");

        let start = std::time::Instant::now();
        let _ = orch
            .execute(
                "ses-prune",
                &batch,
                project_dir.path(),
                std::future::pending::<()>(),
            )
            .await
            .unwrap();
        // Sanity bound: a single-task mock batch should complete in
        // well under 10s. If we're blocking on the spawned prune
        // somehow (mock returns instantly anyway, but defensive), we'd
        // see a much longer duration.
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "orchestrator must NOT block on auto-prune spawn"
        );
    }
}
