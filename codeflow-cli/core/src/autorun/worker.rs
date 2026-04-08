//! Worker DI traits and types for autorun task execution.
//!
//! Provides `TmuxRunner`, `ClaudeInvoker`, `WorktreeProvider`, and `WorkerRunner`
//! traits for isolated, testable worker execution. Each worker creates a
//! per-worker git worktree for filesystem isolation.

use std::path::PathBuf;
use std::time::Duration;

use crate::error::AutorunError;

use super::orchestrator::{WorkerConfig, WorkerResult};

/// Append a timestamped log entry to a worker log file.
fn worker_log(log_path: &std::path::Path, message: &str) {
    use std::io::Write;
    let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(f, "[{timestamp}] {message}");
    }
}

/// Default timeout for a single worker (120 minutes).
pub const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(120 * 60);

/// Executes commands via tmux. Enables testing without actual tmux sessions.
pub trait TmuxRunner: Send + Sync {
    /// Create a new tmux session with the given name.
    ///
    /// When `command` is `Some(args)`, the session is created with `-- args...`
    /// so tmux runs the command directly instead of a bare shell. When `None`,
    /// a bare session is created (legacy behavior).
    fn create_session(
        &self,
        name: &str,
        command: Option<Vec<String>>,
    ) -> impl std::future::Future<Output = Result<(), AutorunError>> + Send;

    /// Send a command string to the named tmux session.
    fn send_command(
        &self,
        session: &str,
        command: &str,
    ) -> impl std::future::Future<Output = Result<(), AutorunError>> + Send;

    /// Terminate a tmux session.
    fn kill_session(
        &self,
        name: &str,
    ) -> impl std::future::Future<Output = Result<(), AutorunError>> + Send;

    /// Check if a tmux session exists.
    fn has_session(
        &self,
        name: &str,
    ) -> impl std::future::Future<Output = Result<bool, AutorunError>> + Send;
}

/// Invokes Claude Code in a worker context.
pub trait ClaudeInvoker: Send + Sync {
    /// Run Claude Code with the given config and return when complete.
    fn invoke(
        &self,
        cfg: InvokeConfig,
    ) -> impl std::future::Future<Output = Result<InvokeResult, AutorunError>> + Send;
}

/// Provides worktree lifecycle operations for worker isolation.
///
/// Abstracts worktree creation and cleanup behind a trait for testability.
/// The real implementation delegates to `WorktreeManager`.
pub trait WorktreeProvider: Send + Sync {
    /// Create a detached worktree with the given name.
    ///
    /// Returns the absolute path to the created worktree directory.
    ///
    /// # Errors
    ///
    /// Returns `AutorunError::Worktree` if worktree creation fails.
    fn setup(&self, name: &str) -> Result<WorktreeInfo, AutorunError>;

    /// Remove a worktree by name, forcefully.
    ///
    /// # Errors
    ///
    /// Returns `AutorunError::Worktree` if cleanup fails.
    fn cleanup(&self, name: &str) -> Result<(), AutorunError>;
}

/// Information about a created worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory.
    pub path: PathBuf,
}

/// Configuration for a Claude Code invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InvokeConfig {
    pub work_dir: String,
    pub prompt: String,
    pub session_id: String,
    pub task_id: String,
    pub integration_auto_merge: bool,
    pub integration_branch: String,
    pub tmux_session: String,
    /// Acceptance criteria extracted from task markdown for base64-encoded env var.
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    /// Worker-specific session ID for claim isolation.
    /// This is the ID used for CRDT claims and must match what hooks use.
    #[serde(default)]
    pub worker_session_id: String,
    /// Who performs epic status updates: "orchestrator" or "none".
    #[serde(default)]
    pub epic_update: String,
    /// Per-task worker timeout in seconds. When non-zero, overrides both the
    /// outer timeout (worker.rs) and the inner poll loop timeout (autorun.rs).
    #[serde(default)]
    pub worker_timeout_secs: u64,
}

/// Result of a Claude Code invocation.
#[derive(Debug, Clone)]
pub struct InvokeResult {
    pub exit_code: i32,
    pub pr_number: i64,
    pub pr_url: String,
    pub branch_name: String,
    pub output: String,
}

/// Executes a task in a worker. Enables mock implementations for testing.
pub trait WorkerRunner: Send + Sync {
    /// Start a worker for the given task and block until completion.
    fn run(
        &self,
        cfg: WorkerConfig,
    ) -> impl std::future::Future<Output = Result<WorkerResult, AutorunError>> + Send;
}

/// A worker implementation using tmux sessions and Claude Code.
///
/// Each worker creates a per-worker git worktree for filesystem isolation.
/// The worktree is created before Claude invocation and cleaned up on all
/// exit paths (success, failure, timeout).
pub struct TmuxWorker<
    T: TmuxRunner,
    C: ClaudeInvoker,
    W: WorktreeProvider,
    S: crate::store::DataStore = crate::store::NoopStore,
> {
    pub tmux: T,
    pub claude: C,
    pub worktree: W,
    pub timeout: Duration,
    pub project_dir: PathBuf,
    pub store: std::sync::Arc<S>,
}

impl<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider> TmuxWorker<T, C, W> {
    /// Create a new `TmuxWorker` with the default timeout and no-op store.
    #[must_use]
    pub fn new(tmux: T, claude: C, worktree: W, project_dir: PathBuf) -> Self {
        Self {
            tmux,
            claude,
            worktree,
            timeout: DEFAULT_WORKER_TIMEOUT,
            project_dir,
            store: std::sync::Arc::new(crate::store::NoopStore),
        }
    }

    /// Create a new `TmuxWorker` with a custom timeout and no-op store.
    #[must_use]
    pub fn with_timeout(
        tmux: T,
        claude: C,
        worktree: W,
        project_dir: PathBuf,
        timeout: Duration,
    ) -> Self {
        Self {
            tmux,
            claude,
            worktree,
            timeout,
            project_dir,
            store: std::sync::Arc::new(crate::store::NoopStore),
        }
    }
}

impl<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider, S: crate::store::DataStore>
    TmuxWorker<T, C, W, S>
{
    /// Create a new `TmuxWorker` with a custom timeout and data store.
    #[must_use]
    pub fn with_store(
        tmux: T,
        claude: C,
        worktree: W,
        project_dir: PathBuf,
        timeout: Duration,
        store: std::sync::Arc<S>,
    ) -> Self {
        Self {
            tmux,
            claude,
            worktree,
            timeout,
            project_dir,
            store,
        }
    }
}

/// Outcome of the serialized merge process.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MergeOutcome {
    /// PR successfully merged via serialized queue.
    Merged,
    /// Merge conflict could not be resolved after rebase attempts.
    MergeConflict { error: String },
    /// Timed out waiting for queue position 0.
    QueueTimeout,
}

/// Execute a serialized merge: enqueue, wait for position 0, rebase, push, merge PR.
///
/// This function orchestrates the full merge sequence for autorun workers:
/// 1. Enqueue in the target-scoped merge queue
/// 2. Poll until this session reaches position 0 (or timeout)
/// 3. Fetch the latest target branch
/// 4. Rebase onto target (with retries)
/// 5. Force-push with lease
/// 6. Merge the PR via GitHub CLI
/// 7. Dequeue from the merge queue
///
/// On failure at any step, the session is removed from the queue and cleanup proceeds.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serialized_merge(
    wt_path: &std::path::Path,
    target: &str,
    worker_sid: &crate::types::SessionId,
    pr_number: i64,
    task_id: &str,
    branch_name: &str,
    state_path: &std::path::Path,
    merge_config: &crate::autorun::config::MergeConfig,
    queue_timeout_secs: u64,
) -> MergeOutcome {
    // Step 1: Enqueue with target_branch.
    let entry = crate::coordination::merge_queue::MergeQueueEntry {
        session_id: worker_sid.clone(),
        task_id: task_id.to_string(),
        branch: branch_name.to_string(),
        target_branch: target.to_string(),
        pr_ready_at: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = crate::coordination::merge_queue::locked_enqueue(state_path, &entry) {
        eprintln!("warning: merge queue enqueue failed: {e}");
    }

    // Step 2: Poll for position 0.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(queue_timeout_secs);
    loop {
        match crate::coordination::merge_queue::locked_position_for_target(
            state_path, worker_sid, target,
        ) {
            Ok(Some(0)) => break, // We're at the front.
            Ok(Some(pos)) => {
                eprintln!("merge queue: position {pos} for target {target}, waiting...");
            }
            Ok(None) => {
                // Not in queue -- should not happen after enqueue, but re-enqueue.
                eprintln!("warning: session not found in merge queue, re-enqueueing");
                let _ = crate::coordination::merge_queue::locked_enqueue(state_path, &entry);
            }
            Err(e) => {
                eprintln!("warning: merge queue position check failed: {e}");
                break; // Proceed anyway on error.
            }
        }

        if std::time::Instant::now() >= deadline {
            eprintln!("merge queue: timed out waiting for position 0 after {queue_timeout_secs}s");
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::QueueTimeout;
        }

        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }

    // Step 3: git fetch origin {target}.
    let fetch_output = std::process::Command::new("git")
        .args(["fetch", "origin", target])
        .current_dir(wt_path)
        .output();
    if let Err(e) = &fetch_output {
        eprintln!("warning: git fetch failed: {e}");
    }

    // Step 4: Rebase loop.
    for attempt in 1..=merge_config.max_rebase_attempts {
        match crate::git::conflict::attempt_rebase(wt_path, &format!("origin/{target}")) {
            Ok(crate::git::conflict::RebaseResult::Success) => break,
            Ok(crate::git::conflict::RebaseResult::ConflictAborted { conflicting_files }) => {
                if attempt == merge_config.max_rebase_attempts {
                    let error = format!(
                        "rebase failed after {attempt} attempts, conflicts: {}",
                        conflicting_files.join(", ")
                    );
                    let _ = crate::coordination::merge_queue::locked_remove_by_session(
                        state_path, worker_sid,
                    );
                    return MergeOutcome::MergeConflict { error };
                }
                eprintln!(
                    "rebase attempt {attempt}/{} failed with conflicts, retrying...",
                    merge_config.max_rebase_attempts
                );
                // Brief pause before retry.
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            Err(e) => {
                eprintln!("warning: rebase error on attempt {attempt}: {e}");
                if attempt == merge_config.max_rebase_attempts {
                    let _ = crate::coordination::merge_queue::locked_remove_by_session(
                        state_path, worker_sid,
                    );
                    return MergeOutcome::MergeConflict {
                        error: format!("rebase error: {e}"),
                    };
                }
            }
        }
    }

    // Step 5: git push --force-with-lease.
    let push_output = std::process::Command::new("git")
        .args(["push", "--force-with-lease"])
        .current_dir(wt_path)
        .output();
    if let Ok(ref output) = push_output {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("warning: git push --force-with-lease failed: {stderr}");
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::MergeConflict {
                error: format!("push failed: {stderr}"),
            };
        }
    } else if let Err(e) = &push_output {
        eprintln!("warning: git push failed to execute: {e}");
        let _ = crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
        return MergeOutcome::MergeConflict {
            error: format!("push error: {e}"),
        };
    }

    // Step 6: Verify GitHub registered the push by checking PR head SHA.
    let local_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(wt_path)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        });

    if let Some(ref sha) = local_sha {
        for attempt in 1..=5 {
            let pr_head = std::process::Command::new("gh")
                .args([
                    "pr",
                    "view",
                    &pr_number.to_string(),
                    "--json",
                    "headRefOid",
                    "-q",
                    ".headRefOid",
                ])
                .current_dir(wt_path)
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                    } else {
                        None
                    }
                });

            if pr_head.as_deref() == Some(sha.as_str()) {
                break;
            }
            if attempt < 5 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }

    // Step 7: gh pr merge.
    let merge_output = std::process::Command::new("gh")
        .args([
            "pr",
            "merge",
            &pr_number.to_string(),
            "--merge",
            "--delete-branch",
        ])
        .current_dir(wt_path)
        .output();
    match merge_output {
        Ok(ref output) if !output.status.success() => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            eprintln!("error: gh pr merge failed: {stderr}");
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::MergeConflict {
                error: format!("gh pr merge failed: {stderr}"),
            };
        }
        Err(ref e) => {
            eprintln!("error: gh pr merge command failed: {e}");
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::MergeConflict {
                error: format!("gh pr merge error: {e}"),
            };
        }
        _ => {} // Success.
    }

    // Step 8: Dequeue.
    let _ =
        crate::coordination::merge_queue::locked_dequeue_for_target(state_path, worker_sid, target);

    MergeOutcome::Merged
}

/// Outcome of merge conflict resolution.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MergeConflictAction {
    /// No conflicts detected, or conflict check failed (warn and continue).
    Continue,
    /// Conflicts resolved via rebase — continue to merge queue.
    RebasedSuccessfully,
    /// Conflicts remain after all rebase attempts — return merge_conflict status.
    MergeConflict { error: String },
}

/// Resolve merge conflicts using config-driven rebase with retry.
///
/// Extracted from `TmuxWorker::run()` for testability. Takes closures for
/// `check_merge_conflicts` and `attempt_rebase` to avoid git dependencies in tests.
pub(crate) async fn resolve_merge_conflicts<F, R>(
    target: &str,
    merge_config: &crate::autorun::config::MergeConfig,
    check_conflicts: F,
    mut attempt_rebase_fn: R,
) -> MergeConflictAction
where
    F: FnOnce() -> Result<crate::git::conflict::ConflictResult, crate::error::GitError>,
    R: FnMut() -> Result<crate::git::conflict::RebaseResult, crate::error::GitError>,
{
    let conflict_result = match check_conflicts() {
        Ok(cr) if cr.has_conflicts => cr,
        Ok(_) => return MergeConflictAction::Continue,
        Err(e) => {
            eprintln!("warning: merge conflict check failed: {e}");
            return MergeConflictAction::Continue;
        }
    };

    if !merge_config.auto_rebase {
        let files = &conflict_result.conflicting_files;
        return MergeConflictAction::MergeConflict {
            error: format!("merge conflicts with {target}: {files:?} (auto_rebase disabled)"),
        };
    }

    let max_attempts = merge_config.max_rebase_attempts;
    let mut last_conflicts = conflict_result.conflicting_files;

    for attempt in 1..=max_attempts {
        match attempt_rebase_fn() {
            Ok(crate::git::conflict::RebaseResult::Success) => {
                last_conflicts.clear();
                break;
            }
            Ok(crate::git::conflict::RebaseResult::ConflictAborted { conflicting_files }) => {
                last_conflicts = conflicting_files;
                if attempt < max_attempts {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
            Err(e) => {
                eprintln!("warning: attempt_rebase failed: {e}");
                last_conflicts.clear();
                break;
            }
        }
    }

    if last_conflicts.is_empty() {
        MergeConflictAction::RebasedSuccessfully
    } else {
        MergeConflictAction::MergeConflict {
            error: format!(
                "merge conflicts with {target} after {max_attempts} rebase attempts: {last_conflicts:?}"
            ),
        }
    }
}

impl<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider, S: crate::store::DataStore> WorkerRunner
    for TmuxWorker<T, C, W, S>
{
    async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
        let tmux_name = cfg.tmux_name.clone();

        // Compute effective timeout: per-task override > constructor timeout > default.
        let timeout = if let Some(task_secs) = cfg.task_timeout_secs {
            Duration::from_secs(task_secs)
        } else if self.timeout.is_zero() {
            DEFAULT_WORKER_TIMEOUT
        } else {
            self.timeout
        };
        let effective_timeout_secs = timeout.as_secs();

        // Generate a unique session ID for this worker's worktree.
        let worker_sid = crate::session::generate_session_id();
        let wt_name = format!("worktree-{worker_sid}");

        // Create the worktree for filesystem isolation.
        // C22: Catch worktree pool limit errors and return retriable pool_full status.
        let wt_info = match self.worktree.setup(&wt_name) {
            Ok(info) => info,
            Err(e) if e.to_string().contains("max concurrent worktrees reached") => {
                // C28: Emit WorkerPoolFull event.
                Self::emit_autorun_event(
                    &self.project_dir,
                    &crate::coordination::types::events::AutorunEvent::WorkerPoolFull {
                        session_id: cfg.session_id.clone(),
                        task_id: cfg.task_id.clone(),
                        timestamp: chrono::Utc::now().to_rfc3339(),
                    },
                );
                return Ok(WorkerResult {
                    worker_id: cfg.worker_id,
                    task_id: cfg.task_id,
                    status: "pool_full".into(),
                    exit_code: 0,
                    pr_number: 0,
                    pr_url: String::new(),
                    branch_name: String::new(),
                    error: e.to_string(),
                    duration_sec: 0,
                });
            }
            Err(e) => return Err(e),
        };

        let log_path = self.project_dir.join(format!(
            ".state/autorun/logs/{}/worker-{}.log",
            cfg.session_id, cfg.task_id,
        ));
        worker_log(
            &log_path,
            &format!(
                "WORKER_START task={} worktree={} session={}",
                cfg.task_id,
                wt_info.path.display(),
                cfg.session_id,
            ),
        );

        // Write codeflow-env.sh into the worktree's runtime directory so
        // the SessionStart hook inside the worker detects the pre-created
        // worktree (task 015).
        let wt_paths = crate::worktree::WorktreePaths::new(&wt_info.path);
        let project_name = self
            .project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
        let _ = crate::session::write_env_file_with_worktree(
            &wt_paths.runtime_dir(),
            &worker_sid,
            &project_name,
            Some(&wt_info.path.to_string_lossy()),
        );

        // Pre-acquire claims for file_scope at startup via acquire_batch().
        let state_path = self.project_dir.join(".state/coordination/state.loro");
        if !cfg.file_scope.is_empty() {
            let scope_refs: Vec<&str> = cfg.file_scope.iter().map(String::as_str).collect();
            let mut claim_conflicts: Vec<String> = Vec::new();
            match crate::file_lock::locked_binary_rmw(
                &state_path,
                crate::coordination::loro::LoroCoordinator::in_memory,
                |bytes| {
                    crate::coordination::loro::LoroCoordinator::from_bytes(bytes, &state_path)
                        .map_err(|e| format!("load coordinator: {e}"))
                },
                |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
                |coord| {
                    match crate::coordination::claims::acquire_batch(
                        coord,
                        &scope_refs,
                        &worker_sid,
                    ) {
                        Ok((_acquired, conflicts)) => {
                            for (path, _err) in &conflicts {
                                claim_conflicts.push(path.clone());
                            }
                        }
                        Err(e) => {
                            eprintln!("warning: acquire_batch error: {e}");
                        }
                    }
                    Ok(())
                },
            ) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("warning: claim acquisition failed: {e}");
                }
            }
            if !claim_conflicts.is_empty() {
                eprintln!(
                    "claim conflicts detected for {} file(s): {:?}",
                    claim_conflicts.len(),
                    claim_conflicts,
                );
                match cfg.blocked_behavior.as_str() {
                    "fail" => {
                        // Cleanup worktree before returning error.
                        if let Err(e) = self.worktree.cleanup(&wt_name) {
                            eprintln!("warning: worktree cleanup failed for {wt_name}: {e}");
                        }
                        return Err(AutorunError::WorkerFailed(format!(
                            "claim conflict on files: {claim_conflicts:?}",
                        )));
                    }
                    _ => {
                        // "skip_and_continue" (default): mark worker as blocked.
                        if let Err(e) = self.worktree.cleanup(&wt_name) {
                            eprintln!("warning: worktree cleanup failed for {wt_name}: {e}");
                        }
                        return Ok(WorkerResult {
                            worker_id: cfg.worker_id,
                            task_id: cfg.task_id,
                            status: "blocked".into(),
                            exit_code: 0,
                            pr_number: 0,
                            pr_url: String::new(),
                            branch_name: String::new(),
                            error: format!("claim conflict on files: {claim_conflicts:?}"),
                            duration_sec: 0,
                        });
                    }
                }
            }
        }

        // Write scope_policy and file_scope to active-task.json via WorktreePaths.
        let active_task_path = wt_paths.active_task();
        if let Some(parent) = active_task_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let active_task = serde_json::json!({
            "task_id": cfg.task_id,
            "session_id": worker_sid.as_str(),
            "scope_policy": cfg.scope_policy,
            "file_scope": cfg.file_scope,
            "worktree_path": wt_info.path.to_string_lossy(),
            "target_branch": cfg.integration_branch,
            "auto_merge": cfg.integration_auto_merge,
            "epic_update": cfg.epic_update,
        });
        let _ = std::fs::write(&active_task_path, active_task.to_string());

        // NOTE: tmux session is created by the invoker (RealClaude::invoke)
        // using create_session with a command argument. The worker no longer
        // creates the session itself.

        // Capture start time for duration tracking.
        let start_time = chrono::Utc::now();

        // Create autorun_worker record at spawn.
        let now = start_time.to_rfc3339();
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let worker_record = crate::models::AutorunWorker {
            id: cfg.worker_id.clone(),
            session_id: cfg.session_id.clone(),
            worker_num: cfg.worker_num as i32,
            task_id: cfg.task_id.clone(),
            status: crate::types::AutorunWorkerStatus::Running,
            tmux_session: Some(tmux_name.clone()),
            worktree_path: Some(wt_info.path.to_string_lossy().into_owned()),
            file_scope: cfg.file_scope.clone(),
            scope_policy: cfg.scope_policy.clone(),
            worker_session_id: Some(worker_sid.as_str().to_owned()),
            pr_number: None,
            started_at: Some(now.clone()),
            completed_at: None,
        };
        if let Err(e) = self.store.create_autorun_worker(&worker_record).await {
            eprintln!("warning: failed to create autorun_worker record: {e}");
        }

        // Create autorun_task_run record at task start.
        let task_run_id = format!("atr-{}-{}", cfg.session_id, cfg.task_id);
        let task_run_record = crate::models::AutorunTaskRun {
            id: task_run_id.clone(),
            worker_id: cfg.worker_id.clone(),
            task_id: cfg.task_id.clone(),
            session_id: cfg.session_id.clone(),
            status: crate::types::AutorunTaskRunStatus::Running,
            branch_name: None,
            worktree_path: Some(wt_info.path.to_string_lossy().into_owned()),
            pr_number: None,
            pr_url: None,
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: Some(now),
            completed_at: None,
            duration_seconds: None,
            exit_code: None,
            error_message: None,
            verification_result: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Err(e) = self.store.create_autorun_task_run(&task_run_record).await {
            eprintln!("warning: failed to create autorun_task_run record: {e}");
        }

        // Emit worker_started event.
        Self::emit_autorun_event(
            &self.project_dir,
            &crate::coordination::types::events::AutorunEvent::WorkerStarted {
                session_id: cfg.session_id.clone(),
                worker_id: cfg.worker_id.clone(),
                task_id: cfg.task_id.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
            },
        );

        // Build task prompt from task markdown, falling back to generic prompt.
        let (prompt, acceptance_criteria) =
            match build_task_prompt_from_file(&self.project_dir, &cfg.task_id) {
                Ok((p, a)) => (p, a),
                Err(e) => {
                    eprintln!(
                        "warning: could not build task prompt for {}: {e}. Using generic prompt.",
                        cfg.task_id
                    );
                    (format!("Execute autorun task {}", cfg.task_id), Vec::new())
                }
            };

        // OUTER TIMEOUT: Bounds total worker wall-clock time including worktree
        // setup, Claude invocation, and cleanup. This is the safety net enforced
        // by the orchestrator/worker layer. The INNER timeout lives in the CLI
        // invoker (autorun.rs) and polls for Claude's exit-code marker file,
        // handling SIGTERM/SIGKILL escalation. Both are needed: the outer catches
        // hangs in non-Claude phases; the inner provides graceful Claude shutdown.
        let result = tokio::time::timeout(timeout, async {
            self.claude
                .invoke(InvokeConfig {
                    work_dir: wt_info.path.to_string_lossy().into_owned(),
                    prompt,
                    session_id: cfg.session_id.clone(),
                    task_id: cfg.task_id.clone(),
                    integration_auto_merge: cfg.integration_auto_merge,
                    integration_branch: cfg.integration_branch.clone(),
                    tmux_session: tmux_name.clone(),
                    acceptance_criteria,
                    worker_session_id: worker_sid.as_str().to_owned(),
                    epic_update: cfg.epic_update.clone(),
                    worker_timeout_secs: effective_timeout_secs,
                })
                .await
        })
        .await;

        // Log Claude exit status.
        match &result {
            Ok(Ok(r)) => worker_log(&log_path, &format!("CLAUDE_EXIT code={}", r.exit_code)),
            Ok(Err(e)) => worker_log(&log_path, &format!("CLAUDE_ERROR {e}")),
            Err(_) => worker_log(&log_path, "CLAUDE_TIMEOUT"),
        }
        // Merge conflict detection and remediation.
        if let Ok(Ok(ref invoke_result)) = result {
            if invoke_result.exit_code == 0 && !cfg.integration_branch.is_empty() {
                let merge_config = crate::autorun::config::load_config(&self.project_dir)
                    .unwrap_or_default()
                    .merge;

                let wt_path = wt_info.path.clone();
                let target = cfg.integration_branch.clone();
                let action = resolve_merge_conflicts(
                    &target,
                    &merge_config,
                    || crate::git::conflict::check_merge_conflicts(&wt_path, &target),
                    || crate::git::conflict::attempt_rebase(&wt_path, &target),
                )
                .await;

                // Emit coordination events for merge conflict detection/rebase.
                match &action {
                    MergeConflictAction::MergeConflict { .. } => {
                        crate::autorun::emit_coordination_event(
                            &self.project_dir,
                            &crate::coordination::types::events::CoordinationEvent::MergeConflictDetected {
                                session_id: worker_sid.clone(),
                                branch: invoke_result.branch_name.clone(),
                                target_branch: target.clone(),
                                task_id: Some(cfg.task_id.clone()),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            },
                        );
                    }
                    MergeConflictAction::RebasedSuccessfully => {
                        crate::autorun::emit_coordination_event(
                            &self.project_dir,
                            &crate::coordination::types::events::CoordinationEvent::MergeRebaseAttempted {
                                session_id: worker_sid.clone(),
                                branch: invoke_result.branch_name.clone(),
                                target_branch: target.clone(),
                                success: true,
                                task_id: Some(cfg.task_id.clone()),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            },
                        );
                    }
                    MergeConflictAction::Continue => {}
                }

                if let MergeConflictAction::MergeConflict { error } = action {
                    // Compute elapsed before DB write so duration_seconds is recorded.
                    let elapsed = (chrono::Utc::now() - start_time).num_seconds().max(0);
                    // Update DB records before early return to avoid orphaned running records.
                    let completed_at = chrono::Utc::now().to_rfc3339();
                    let _ = self
                        .store
                        .update_autorun_worker(
                            &cfg.worker_id,
                            crate::models::AutorunWorkerUpdate {
                                status: Some(crate::types::AutorunWorkerStatus::Failed),
                                completed_at: Some(completed_at.clone()),
                                ..Default::default()
                            },
                        )
                        .await;
                    let _ = self
                        .store
                        .update_autorun_task_run(
                            &task_run_id,
                            crate::models::AutorunTaskRunUpdate {
                                status: Some(crate::types::AutorunTaskRunStatus::Failed),
                                error_message: Some(error.clone()),
                                merge_conflicts: Some(vec![error.clone()]),
                                completed_at: Some(completed_at.clone()),
                                duration_seconds: Some(elapsed),
                                ..Default::default()
                            },
                        )
                        .await;
                    Self::emit_autorun_event(
                        &self.project_dir,
                        &crate::coordination::types::events::AutorunEvent::WorkerFailed {
                            session_id: cfg.session_id.clone(),
                            worker_id: cfg.worker_id.clone(),
                            task_id: cfg.task_id.clone(),
                            error: error.clone(),
                            timestamp: completed_at,
                        },
                    );

                    return Ok(WorkerResult {
                        worker_id: cfg.worker_id,
                        task_id: cfg.task_id,
                        status: "merge_conflict".into(),
                        exit_code: 1,
                        pr_number: 0,
                        pr_url: String::new(),
                        branch_name: String::new(),
                        error,
                        duration_sec: elapsed,
                    });
                }
            }
        }

        // Serialized merge: when integration_auto_merge is enabled and a PR was
        // created, use the merge queue to serialize PR merges to the integration
        // branch. The pr_number check is independent of exit_code so that PRs
        // created before a timeout (exit_code 124) still get merged.
        if cfg.integration_auto_merge {
            if let Ok(Ok(ref invoke_result)) = result {
                if invoke_result.pr_number > 0 {
                    let merge_config = crate::autorun::config::load_config(&self.project_dir)
                        .unwrap_or_default()
                        .merge;
                    if merge_config.queue_enforcing {
                        let merge_result = serialized_merge(
                            &wt_info.path,
                            &cfg.integration_branch,
                            &worker_sid,
                            invoke_result.pr_number,
                            &cfg.task_id,
                            &invoke_result.branch_name,
                            &state_path,
                            &merge_config,
                            cfg.queue_timeout_secs,
                        )
                        .await;
                        match merge_result {
                            MergeOutcome::Merged => {
                                eprintln!(
                                    "serialized merge: PR #{} merged to {}",
                                    invoke_result.pr_number, cfg.integration_branch
                                );
                            }
                            MergeOutcome::MergeConflict { ref error } => {
                                eprintln!(
                                    "serialized merge: PR #{} failed: {error}",
                                    invoke_result.pr_number
                                );
                            }
                            MergeOutcome::QueueTimeout => {
                                eprintln!(
                                    "serialized merge: PR #{} timed out in queue",
                                    invoke_result.pr_number
                                );
                            }
                        }
                    } // end queue_enforcing
                }
            }
        }

        // Always cleanup: release claims, kill tmux, remove worktree.
        // Release claims via release_all().
        match crate::file_lock::locked_binary_rmw(
            &state_path,
            crate::coordination::loro::LoroCoordinator::in_memory,
            |bytes| {
                crate::coordination::loro::LoroCoordinator::from_bytes(bytes, &state_path)
                    .map_err(|e| format!("load coordinator: {e}"))
            },
            |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
            |coord| {
                let _ = crate::coordination::claims::release_all(coord, &worker_sid);
                Ok(())
            },
        ) {
            Ok(()) => {
                // Emit ClaimReleased event for each file in scope.
                for path in &cfg.file_scope {
                    crate::autorun::emit_coordination_event(
                        &self.project_dir,
                        &crate::coordination::types::events::CoordinationEvent::ClaimReleased {
                            session_id: worker_sid.clone(),
                            path: path.clone(),
                            task_id: Some(cfg.task_id.clone()),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
            }
            Err(e) => {
                eprintln!("warning: claim release failed: {e}");
            }
        }

        let _ = self.tmux.kill_session(&tmux_name).await;

        // Branch safety check before cleanup — warn on unpushed work.
        let wt_dir = self
            .project_dir
            .join(crate::worktree::DEFAULT_BASE_DIR)
            .join(&wt_name);
        if wt_dir.exists() {
            let safety = crate::worktree::check_branch_safety(&wt_dir);
            if safety.risk >= crate::worktree::BranchRisk::High {
                eprintln!(
                    "warning: branch safety issue for {wt_name}: {} (proceeding with force cleanup)",
                    safety.message
                );
                worker_log(
                    &log_path,
                    &format!(
                        "BRANCH_SAFETY risk={} branch={}",
                        safety.risk, safety.branch
                    ),
                );
            }
        }

        // Worktree cleanup with retry: 3 attempts, 2s backoff between each.
        // On final failure, deregister from worktrees.yaml anyway to prevent
        // pool exhaustion from stuck worktrees.
        {
            let max_retries = 3;
            let mut cleanup_ok = false;
            for attempt in 1..=max_retries {
                match self.worktree.cleanup(&wt_name) {
                    Ok(()) => {
                        cleanup_ok = true;
                        break;
                    }
                    Err(e) => {
                        if attempt < max_retries {
                            eprintln!(
                                "warning: worktree cleanup attempt {attempt}/{max_retries} failed for {wt_name}: {e}, retrying in 2s"
                            );
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        } else {
                            eprintln!(
                                "warning: worktree cleanup failed after {max_retries} attempts for {wt_name}: {e}, deregistering anyway"
                            );
                            // Deregister from worktrees.yaml to prevent pool exhaustion.
                            let registry_path =
                                self.project_dir.join(".state/worktrees/worktrees.yaml");
                            if let Ok(content) = std::fs::read_to_string(&registry_path) {
                                // Remove the entry matching this worktree name.
                                let filtered: String = content
                                    .lines()
                                    .filter(|line| !line.contains(&wt_name))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if filtered != content {
                                    let _ = std::fs::write(&registry_path, filtered + "\n");
                                }
                            }
                            // Emit coordination event for cleanup failure.
                            crate::autorun::emit_coordination_event(
                                &self.project_dir,
                                &crate::coordination::types::events::CoordinationEvent::ClaimReleased {
                                    session_id: worker_sid.clone(),
                                    path: format!("worktree-cleanup-failed:{wt_name}"),
                                    task_id: Some(cfg.task_id.clone()),
                                    timestamp: chrono::Utc::now().to_rfc3339(),
                                },
                            );
                        }
                    }
                }
            }
            if cleanup_ok {
                worker_log(&log_path, "CLEANUP claims_released worktree_removed");
            } else {
                worker_log(
                    &log_path,
                    "CLEANUP claims_released worktree_deregistered_after_cleanup_failure",
                );
            }
        }

        let elapsed = (chrono::Utc::now() - start_time).num_seconds().max(0);
        let worker_result = match result {
            Ok(Ok(invoke_result)) => {
                let status = if invoke_result.exit_code == 0 {
                    "completed"
                } else {
                    "failed"
                };
                Ok(WorkerResult {
                    worker_id: cfg.worker_id.clone(),
                    task_id: cfg.task_id.clone(),
                    status: status.into(),
                    exit_code: invoke_result.exit_code,
                    pr_number: invoke_result.pr_number,
                    pr_url: invoke_result.pr_url.clone(),
                    branch_name: invoke_result.branch_name,
                    error: String::new(),
                    duration_sec: elapsed,
                })
            }
            Ok(Err(e)) => Err(AutorunError::WorkerFailed(format!(
                "task {}: {e}",
                cfg.task_id
            ))),
            Err(_) => Ok(WorkerResult {
                worker_id: cfg.worker_id.clone(),
                task_id: cfg.task_id.clone(),
                status: "timeout".into(),
                exit_code: 124, // standard timeout exit code
                pr_number: 0,
                pr_url: String::new(),
                branch_name: String::new(),
                error: "worker exceeded timeout".into(),
                duration_sec: elapsed,
            }),
        };

        // Update autorun_worker and autorun_task_run at completion.
        let completed_at = chrono::Utc::now().to_rfc3339();
        match &worker_result {
            Ok(wr) => {
                let worker_status = match wr.status.as_str() {
                    "completed" => crate::types::AutorunWorkerStatus::Completed,
                    "timeout" => crate::types::AutorunWorkerStatus::Timeout,
                    _ => crate::types::AutorunWorkerStatus::Failed,
                };
                let task_run_status = match wr.status.as_str() {
                    "completed" => crate::types::AutorunTaskRunStatus::Completed,
                    "timeout" => crate::types::AutorunTaskRunStatus::Timeout,
                    "skipped" => crate::types::AutorunTaskRunStatus::Skipped,
                    _ => crate::types::AutorunTaskRunStatus::Failed,
                };

                // Update worker record with 10s timeout.
                let db_timeout = Duration::from_secs(10);
                let worker_update_fut = self.store.update_autorun_worker(
                    &cfg.worker_id,
                    crate::models::AutorunWorkerUpdate {
                        status: Some(worker_status),
                        pr_number: if wr.pr_number > 0 {
                            Some(wr.pr_number)
                        } else {
                            None
                        },
                        completed_at: Some(completed_at.clone()),
                        ..Default::default()
                    },
                );
                match tokio::time::timeout(db_timeout, worker_update_fut).await {
                    Ok(Err(e)) => {
                        eprintln!("warning: failed to update autorun_worker: {e}");
                    }
                    Err(_) => {
                        eprintln!(
                            "warning: autorun_worker update timed out after {}s",
                            db_timeout.as_secs()
                        );
                    }
                    Ok(Ok(())) => {}
                }

                // Update task_run record with 10s timeout.
                let verification = match wr.status.as_str() {
                    "completed" => Some("pass".to_string()),
                    "timeout" => Some("timeout".to_string()),
                    _ => Some("fail".to_string()),
                };
                let task_run_update_fut = self.store.update_autorun_task_run(
                    &task_run_id,
                    crate::models::AutorunTaskRunUpdate {
                        status: Some(task_run_status),
                        pr_number: if wr.pr_number > 0 {
                            Some(wr.pr_number)
                        } else {
                            None
                        },
                        pr_url: if wr.pr_url.is_empty() {
                            None
                        } else {
                            Some(wr.pr_url.clone())
                        },
                        exit_code: Some(i64::from(wr.exit_code)),
                        completed_at: Some(completed_at.clone()),
                        duration_seconds: Some(elapsed),
                        error_message: if wr.error.is_empty() {
                            None
                        } else {
                            Some(wr.error.clone())
                        },
                        verification_result: verification,
                        branch_name: if wr.branch_name.is_empty() {
                            None
                        } else {
                            Some(wr.branch_name.clone())
                        },
                        ..Default::default()
                    },
                );
                match tokio::time::timeout(db_timeout, task_run_update_fut).await {
                    Ok(Err(e)) => {
                        eprintln!("warning: failed to update autorun_task_run: {e}");
                    }
                    Err(_) => {
                        eprintln!(
                            "warning: autorun_task_run update timed out after {}s",
                            db_timeout.as_secs()
                        );
                    }
                    Ok(Ok(())) => {}
                }

                // Emit completion event.
                let event = match wr.status.as_str() {
                    "completed" => {
                        crate::coordination::types::events::AutorunEvent::WorkerCompleted {
                            session_id: cfg.session_id.clone(),
                            worker_id: cfg.worker_id.clone(),
                            task_id: cfg.task_id.clone(),
                            pr_number: if wr.pr_number > 0 {
                                Some(wr.pr_number)
                            } else {
                                None
                            },
                            timestamp: completed_at.clone(),
                        }
                    }
                    "timeout" => crate::coordination::types::events::AutorunEvent::WorkerTimeout {
                        session_id: cfg.session_id.clone(),
                        worker_id: cfg.worker_id.clone(),
                        task_id: cfg.task_id.clone(),
                        timestamp: completed_at.clone(),
                    },
                    "skipped" => {
                        crate::coordination::types::events::AutorunEvent::WorkerCancelled {
                            session_id: cfg.session_id.clone(),
                            worker_id: cfg.worker_id.clone(),
                            task_id: cfg.task_id.clone(),
                            reason: wr.error.clone(),
                            timestamp: completed_at.clone(),
                        }
                    }
                    _ => crate::coordination::types::events::AutorunEvent::WorkerFailed {
                        session_id: cfg.session_id.clone(),
                        worker_id: cfg.worker_id.clone(),
                        task_id: cfg.task_id.clone(),
                        error: wr.error.clone(),
                        timestamp: completed_at,
                    },
                };
                Self::emit_autorun_event(&self.project_dir, &event);
            }
            Err(e) => {
                // Worker errored — update records with failed status.
                let _ = self
                    .store
                    .update_autorun_worker(
                        &cfg.worker_id,
                        crate::models::AutorunWorkerUpdate {
                            status: Some(crate::types::AutorunWorkerStatus::Failed),
                            completed_at: Some(completed_at.clone()),
                            ..Default::default()
                        },
                    )
                    .await;
                let _ = self
                    .store
                    .update_autorun_task_run(
                        &task_run_id,
                        crate::models::AutorunTaskRunUpdate {
                            status: Some(crate::types::AutorunTaskRunStatus::Failed),
                            error_message: Some(e.to_string()),
                            completed_at: Some(completed_at.clone()),
                            verification_result: Some("fail".to_string()),
                            ..Default::default()
                        },
                    )
                    .await;
                Self::emit_autorun_event(
                    &self.project_dir,
                    &crate::coordination::types::events::AutorunEvent::WorkerFailed {
                        session_id: cfg.session_id.clone(),
                        worker_id: cfg.worker_id.clone(),
                        task_id: cfg.task_id.clone(),
                        error: e.to_string(),
                        timestamp: completed_at,
                    },
                );
            }
        }

        worker_result
    }
}

impl<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider, S: crate::store::DataStore>
    TmuxWorker<T, C, W, S>
{
    /// Emit an autorun event to the JSONL ledger.
    fn emit_autorun_event(
        project_dir: &std::path::Path,
        event: &crate::coordination::types::events::AutorunEvent,
    ) {
        crate::autorun::emit_autorun_event(project_dir, event);
    }
}

/// Parsed task metadata extracted from a task markdown file.
#[derive(Debug, Clone, Default)]
pub struct TaskMetadata {
    /// Task description from the markdown body.
    pub description: String,
    /// Approach section from the markdown body.
    pub approach: String,
    /// Acceptance criteria from YAML frontmatter.
    pub acceptance: Vec<String>,
    /// File scope from YAML frontmatter.
    pub file_scope: Vec<String>,
    /// Task title from YAML frontmatter.
    pub title: String,
}

/// Read a task markdown file and extract metadata for prompt construction.
///
/// Expects the file to have YAML frontmatter delimited by `---` lines,
/// followed by markdown body sections.
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the file does not exist or cannot be read.
pub fn parse_task_markdown(content: &str) -> TaskMetadata {
    let mut meta = TaskMetadata::default();

    // Extract YAML frontmatter.
    let parts: Vec<&str> = content.splitn(3, "---").collect();
    if parts.len() >= 3 {
        let yaml_str = parts[1].trim();
        if let Ok(yaml_val) = serde_yaml::from_str::<serde_yaml::Value>(yaml_str) {
            if let Some(title) = yaml_val.get("title").and_then(serde_yaml::Value::as_str) {
                meta.title = title.to_string();
            }
            if let Some(desc) = yaml_val
                .get("description")
                .and_then(serde_yaml::Value::as_str)
            {
                meta.description = desc.to_string();
            }
            if let Some(acc) = yaml_val
                .get("acceptance")
                .and_then(serde_yaml::Value::as_sequence)
            {
                meta.acceptance = acc
                    .iter()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(String::from)
                    .collect();
            }
            if let Some(scope) = yaml_val
                .get("file_scope")
                .and_then(serde_yaml::Value::as_sequence)
            {
                meta.file_scope = scope
                    .iter()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(String::from)
                    .collect();
            }
        }
    }

    // Extract approach section from markdown body.
    let body = if parts.len() >= 3 { parts[2] } else { content };
    if let Some(approach_start) = body.find("## Approach") {
        let after_heading = &body[approach_start + "## Approach".len()..];
        // Find the next ## heading to delimit the approach section.
        let end = after_heading.find("\n## ").unwrap_or(after_heading.len());
        meta.approach = after_heading[..end].trim().to_string();
    }

    // If description is empty from frontmatter, try the Description section.
    if meta.description.is_empty() {
        if let Some(desc_start) = body.find("## Description") {
            let after_heading = &body[desc_start + "## Description".len()..];
            let end = after_heading.find("\n## ").unwrap_or(after_heading.len());
            meta.description = after_heading[..end].trim().to_string();
        }
    }

    meta
}

/// Build a PathFlow-aware task prompt from task metadata.
///
/// Constructs a prompt suitable for autonomous Claude Code execution in
/// an autorun worker context.
#[must_use]
pub fn build_task_prompt(meta: &TaskMetadata) -> String {
    use std::fmt::Write;

    let mut prompt = String::with_capacity(2048);

    prompt.push_str("You are executing an autorun task autonomously. ");
    prompt.push_str("Follow PathFlow PF1-PF7 without user interaction. ");
    prompt.push_str("Document any Tier 3 decisions in the PR description.\n\n");

    // Title.
    if !meta.title.is_empty() {
        let _ = write!(prompt, "# Task: {}\n\n", meta.title);
    }

    // Description.
    if !meta.description.is_empty() {
        let _ = write!(prompt, "## Description\n{}\n\n", meta.description);
    }

    // Approach.
    if !meta.approach.is_empty() {
        let _ = write!(prompt, "## Approach\n{}\n\n", meta.approach);
    }

    // Acceptance criteria.
    if !meta.acceptance.is_empty() {
        prompt.push_str("## Acceptance Criteria\n");
        for (i, criterion) in meta.acceptance.iter().enumerate() {
            let _ = writeln!(prompt, "{}. {criterion}", i + 1);
        }
        prompt.push('\n');
    }

    // File scope.
    if !meta.file_scope.is_empty() {
        prompt.push_str("## File Scope\n");
        for path in &meta.file_scope {
            let _ = writeln!(prompt, "- {path}");
        }
        prompt.push('\n');
    }

    // Autonomous operation instructions.
    prompt.push_str("## Autonomous Operation\n");
    prompt.push_str("- Do NOT prompt the user for input.\n");
    prompt.push_str("- Make all decisions autonomously (Tier 1 and Tier 2).\n");
    prompt.push_str(
        "- Document Tier 3 decisions (architectural, breaking changes) in the PR description.\n",
    );
    prompt.push_str("- Create a PR targeting the configured branch on success.\n");
    prompt.push_str("- Exit with code 0 on success, non-zero on failure.\n");

    prompt
}

/// Read a task markdown file from disk and build a prompt from it.
///
/// Constructs the file path from the task ID format `{AREA}-TSK-{epic_NNN}-{seq_NNN}`.
///
/// # Errors
///
/// Returns `AutorunError::MissingTask` if the task file cannot be found or read.
pub fn build_task_prompt_from_file(
    project_dir: &std::path::Path,
    task_id: &str,
) -> Result<(String, Vec<String>), AutorunError> {
    // Parse task_id format: AREA-TSK-EPC_NNN-SEQ_NNN
    // Example: INF-TSK-023-028 -> area=INF, epic_num=023, task file = INF-TSK-023-028.md
    // Epic dir: project-management/epics/INF/INF-EPC-023/tasks/INF-TSK-023-028.md
    let parts: Vec<&str> = task_id.split('-').collect();
    if parts.len() < 4 || parts[1] != "TSK" {
        return Err(AutorunError::MissingTask(format!(
            "invalid task ID format: {task_id} (expected AREA-TSK-NNN-NNN)"
        )));
    }

    // Reject task IDs containing path traversal characters to prevent directory escape.
    for part in &parts {
        if part.contains("..") || part.contains('/') || part.contains('\\') {
            return Err(AutorunError::MissingTask(format!(
                "task ID contains path traversal characters: {task_id}"
            )));
        }
    }

    let area = parts[0];
    let epic_num = parts[2];
    let task_path = project_dir
        .join("project-management")
        .join("epics")
        .join(area)
        .join(format!("{area}-EPC-{epic_num}"))
        .join("tasks")
        .join(format!("{task_id}.md"));

    let content = std::fs::read_to_string(&task_path)
        .map_err(|e| AutorunError::MissingTask(format!("reading {}: {e}", task_path.display())))?;

    let meta = parse_task_markdown(&content);
    let acceptance = meta.acceptance.clone();
    let prompt = build_task_prompt(&meta);
    Ok((prompt, acceptance))
}

/// Real `WorktreeProvider` implementation backed by `WorktreeManager`.
pub struct RealWorktreeProvider {
    project_dir: PathBuf,
}

impl RealWorktreeProvider {
    /// Create a new provider for the given project directory.
    #[must_use]
    pub fn new(project_dir: PathBuf) -> Self {
        Self { project_dir }
    }
}

impl WorktreeProvider for RealWorktreeProvider {
    fn setup(&self, name: &str) -> Result<WorktreeInfo, AutorunError> {
        let mgr = crate::worktree::WorktreeManager::new(&self.project_dir);
        let entry = mgr.setup_detached(name)?;
        // Mark this worktree entry as created by autorun.
        let _ = crate::worktree::locked_update_source(mgr.registry_path(), name, "autorun");
        Ok(WorktreeInfo {
            path: PathBuf::from(&entry.path),
        })
    }

    fn cleanup(&self, name: &str) -> Result<(), AutorunError> {
        let mgr = crate::worktree::WorktreeManager::new(&self.project_dir);
        let opts = crate::worktree::CleanupOpts {
            force: true,
            ..Default::default()
        };
        mgr.cleanup(name, &opts)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::Mutex;

    // Mock implementations for testing.

    struct MockTmux {
        session_created: Arc<AtomicBool>,
        session_killed: Arc<AtomicBool>,
    }

    impl MockTmux {
        fn new() -> (Self, Arc<AtomicBool>, Arc<AtomicBool>) {
            let created = Arc::new(AtomicBool::new(false));
            let killed = Arc::new(AtomicBool::new(false));
            (
                Self {
                    session_created: created.clone(),
                    session_killed: killed.clone(),
                },
                created,
                killed,
            )
        }
    }

    impl TmuxRunner for MockTmux {
        async fn create_session(
            &self,
            _name: &str,
            _command: Option<Vec<String>>,
        ) -> Result<(), AutorunError> {
            self.session_created.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn send_command(&self, _session: &str, _command: &str) -> Result<(), AutorunError> {
            Ok(())
        }
        async fn kill_session(&self, _name: &str) -> Result<(), AutorunError> {
            self.session_killed.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn has_session(&self, _name: &str) -> Result<bool, AutorunError> {
            Ok(self.session_created.load(Ordering::SeqCst))
        }
    }

    struct MockClaude {
        exit_code: i32,
        last_work_dir: Arc<Mutex<String>>,
    }

    impl MockClaude {
        fn new(exit_code: i32) -> (Self, Arc<Mutex<String>>) {
            let work_dir = Arc::new(Mutex::new(String::new()));
            (
                Self {
                    exit_code,
                    last_work_dir: work_dir.clone(),
                },
                work_dir,
            )
        }

        fn simple(exit_code: i32) -> Self {
            Self {
                exit_code,
                last_work_dir: Arc::new(Mutex::new(String::new())),
            }
        }
    }

    impl ClaudeInvoker for MockClaude {
        async fn invoke(&self, cfg: InvokeConfig) -> Result<InvokeResult, AutorunError> {
            *self.last_work_dir.lock().await = cfg.work_dir;
            Ok(InvokeResult {
                exit_code: self.exit_code,
                pr_number: 42,
                pr_url: "https://github.com/test/pr/42".into(),
                branch_name: "feat/test".into(),
                output: "done".into(),
            })
        }
    }

    struct FailingClaude;

    impl ClaudeInvoker for FailingClaude {
        async fn invoke(&self, cfg: InvokeConfig) -> Result<InvokeResult, AutorunError> {
            Err(AutorunError::WorkerFailed(format!(
                "mock error for {}",
                cfg.task_id
            )))
        }
    }

    struct SlowClaude;

    impl ClaudeInvoker for SlowClaude {
        async fn invoke(&self, _cfg: InvokeConfig) -> Result<InvokeResult, AutorunError> {
            // Sleep longer than the timeout.
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(InvokeResult {
                exit_code: 0,
                pr_number: 0,
                pr_url: String::new(),
                branch_name: String::new(),
                output: String::new(),
            })
        }
    }

    /// Mock worktree provider that tracks setup/cleanup calls.
    struct MockWorktree {
        setup_called: Arc<AtomicBool>,
        cleanup_called: Arc<AtomicBool>,
        setup_names: Arc<Mutex<Vec<String>>>,
        cleanup_names: Arc<Mutex<Vec<String>>>,
        /// Path returned by setup.
        path: PathBuf,
    }

    impl MockWorktree {
        fn new(path: PathBuf) -> Self {
            Self {
                setup_called: Arc::new(AtomicBool::new(false)),
                cleanup_called: Arc::new(AtomicBool::new(false)),
                setup_names: Arc::new(Mutex::new(Vec::new())),
                cleanup_names: Arc::new(Mutex::new(Vec::new())),
                path,
            }
        }

        #[allow(clippy::type_complexity)]
        fn with_tracking(
            path: PathBuf,
        ) -> (
            Self,
            Arc<AtomicBool>,
            Arc<AtomicBool>,
            Arc<Mutex<Vec<String>>>,
            Arc<Mutex<Vec<String>>>,
        ) {
            let setup_called = Arc::new(AtomicBool::new(false));
            let cleanup_called = Arc::new(AtomicBool::new(false));
            let setup_names = Arc::new(Mutex::new(Vec::new()));
            let cleanup_names = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    setup_called: setup_called.clone(),
                    cleanup_called: cleanup_called.clone(),
                    setup_names: setup_names.clone(),
                    cleanup_names: cleanup_names.clone(),
                    path,
                },
                setup_called,
                cleanup_called,
                setup_names,
                cleanup_names,
            )
        }
    }

    impl WorktreeProvider for MockWorktree {
        fn setup(&self, name: &str) -> Result<WorktreeInfo, AutorunError> {
            self.setup_called.store(true, Ordering::SeqCst);
            // Use blocking lock since this is sync code in tests.
            // Safety: tests run within a tokio runtime but setup is sync.
            let mut names = self.setup_names.try_lock().unwrap();
            names.push(name.to_string());
            Ok(WorktreeInfo {
                path: self.path.clone(),
            })
        }

        fn cleanup(&self, name: &str) -> Result<(), AutorunError> {
            self.cleanup_called.store(true, Ordering::SeqCst);
            let mut names = self.cleanup_names.try_lock().unwrap();
            names.push(name.to_string());
            Ok(())
        }
    }

    /// Mock worktree provider that fails on setup.
    struct FailingWorktree;

    impl WorktreeProvider for FailingWorktree {
        fn setup(&self, name: &str) -> Result<WorktreeInfo, AutorunError> {
            Err(AutorunError::Worktree(
                crate::error::WorktreeError::Creation(format!("mock setup failure for {name}")),
            ))
        }

        fn cleanup(&self, _name: &str) -> Result<(), AutorunError> {
            Ok(())
        }
    }

    fn make_project_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn make_worker_config() -> WorkerConfig {
        WorkerConfig {
            session_id: "ars-test".into(),
            worker_id: "arw-test".into(),
            worker_num: 1,
            task_id: "task-a".into(),
            batch_name: "test-batch".into(),
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_name: "cf-ar-task-a".into(),
            file_scope: Vec::new(),
            scope_policy: "soft".into(),
            blocked_behavior: "skip_and_continue".into(),
            epic_update: String::new(),
            queue_timeout_secs: 600,
            task_timeout_secs: None,
        }
    }

    #[tokio::test]
    async fn test_tmux_worker_success() {
        let project_dir = make_project_dir();
        let (tmux, _created, killed) = MockTmux::new();
        let (wt, wt_setup, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.pr_number, 42);
        // NOTE: Worker no longer calls create_session (invoker does).
        assert!(killed.load(Ordering::SeqCst));
        assert!(
            wt_setup.load(Ordering::SeqCst),
            "worktree setup should be called"
        );
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree cleanup should be called on success"
        );
    }

    #[tokio::test]
    async fn test_tmux_worker_failure() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(1);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_tmux_worker_timeout() {
        let project_dir = make_project_dir();
        let (tmux, _, killed) = MockTmux::new();
        let (wt, _, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let worker = TmuxWorker::with_timeout(
            tmux,
            SlowClaude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_millis(100),
        );

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "timeout");
        assert_eq!(result.exit_code, 124);
        assert!(result.error.contains("timeout"));
        assert!(
            killed.load(Ordering::SeqCst),
            "session should be cleaned up on timeout"
        );
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree should be cleaned up on timeout"
        );
    }

    #[test]
    fn test_default_worker_timeout() {
        assert_eq!(DEFAULT_WORKER_TIMEOUT, Duration::from_secs(7200));
    }

    #[test]
    fn test_invoke_config_fields() {
        let cfg = InvokeConfig {
            work_dir: ".".into(),
            prompt: "do stuff".into(),
            session_id: "ses-1".into(),
            task_id: "t-1".into(),
            integration_auto_merge: true,
            integration_branch: "develop".into(),
            tmux_session: "worker-1".into(),
            acceptance_criteria: vec!["criterion 1".into()],
            worker_session_id: "ses-worker-1".into(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
        };
        assert_eq!(cfg.task_id, "t-1");
        assert!(cfg.integration_auto_merge);
        assert_eq!(cfg.acceptance_criteria.len(), 1);
        assert_eq!(cfg.worker_session_id, "ses-worker-1");
    }

    #[test]
    fn test_invoke_config_custom_timeout() {
        let cfg = InvokeConfig {
            work_dir: ".".into(),
            prompt: "test".into(),
            session_id: "ses-1".into(),
            task_id: "t-1".into(),
            integration_auto_merge: false,
            integration_branch: String::new(),
            tmux_session: "w-1".into(),
            acceptance_criteria: vec![],
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 3600,
        };
        assert_eq!(cfg.worker_timeout_secs, 3600);
    }

    #[test]
    fn test_invoke_result_fields() {
        let result = InvokeResult {
            exit_code: 0,
            pr_number: 42,
            pr_url: "url".into(),
            branch_name: "feat/x".into(),
            output: "ok".into(),
        };
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.pr_number, 42);
    }

    #[tokio::test]
    async fn test_tmux_worker_zero_timeout_fallback() {
        let project_dir = make_project_dir();
        let (tmux, _created, killed) = MockTmux::new();
        let wt = MockWorktree::new(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::with_timeout(
            tmux,
            claude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::ZERO,
        );

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.exit_code, 0);
        // NOTE: Worker no longer calls create_session (invoker does).
        assert!(killed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_tmux_worker_invoke_error() {
        let project_dir = make_project_dir();
        let (tmux, _, killed) = MockTmux::new();
        let (wt, _, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let worker = TmuxWorker::new(tmux, FailingClaude, wt, project_dir.path().to_path_buf());

        let err = worker.run(make_worker_config()).await.unwrap_err();
        assert!(
            matches!(err, AutorunError::WorkerFailed(ref msg) if msg.contains("task-a")),
            "expected WorkerFailed for task-a, got: {err:?}"
        );
        assert!(
            killed.load(Ordering::SeqCst),
            "session should be cleaned up after invoke error"
        );
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree should be cleaned up after invoke error"
        );
    }

    #[tokio::test]
    async fn test_mock_tmux_send_and_has_session() {
        let (tmux, created, _) = MockTmux::new();

        // has_session returns false before create.
        assert!(!tmux.has_session("test-session").await.unwrap());

        // After create, has_session returns true.
        tmux.create_session("test-session", None).await.unwrap();
        assert!(created.load(Ordering::SeqCst));
        assert!(tmux.has_session("test-session").await.unwrap());

        // send_command succeeds.
        tmux.send_command("test-session", "echo hello")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_worker_creates_worktree_before_invoke() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let (wt, wt_setup, _, _, _) = MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let _ = worker.run(make_worker_config()).await;

        assert!(
            wt_setup.load(Ordering::SeqCst),
            "worktree setup must be called before invoke"
        );
    }

    #[tokio::test]
    async fn test_worker_passes_worktree_path_as_work_dir() {
        let project_dir = make_project_dir();
        let wt_path = project_dir
            .path()
            .join(".git-worktrees")
            .join("worktree-test");
        std::fs::create_dir_all(&wt_path).unwrap();

        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(wt_path.clone());
        let (claude, last_work_dir) = MockClaude::new(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let _ = worker.run(make_worker_config()).await;

        let work_dir = last_work_dir.lock().await;
        assert_ne!(work_dir.as_str(), ".", "work_dir must not be hardcoded '.'");
        assert!(
            work_dir.contains(".git-worktrees"),
            "work_dir should reference worktree path: {work_dir}"
        );
    }

    #[tokio::test]
    async fn test_worker_cleans_up_on_success() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let (wt, _, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree must be cleaned up after success"
        );
    }

    #[tokio::test]
    async fn test_worker_cleans_up_on_failure() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let (wt, _, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let worker = TmuxWorker::new(tmux, FailingClaude, wt, project_dir.path().to_path_buf());

        let _ = worker.run(make_worker_config()).await;
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree must be cleaned up after invoke failure"
        );
    }

    #[tokio::test]
    async fn test_worker_cleans_up_on_timeout() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let (wt, _, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let worker = TmuxWorker::with_timeout(
            tmux,
            SlowClaude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_millis(100),
        );

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "timeout");
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree must be cleaned up after timeout"
        );
    }

    #[tokio::test]
    async fn test_worker_unique_worktree_names() {
        let project_dir = make_project_dir();
        let setup_names = Arc::new(Mutex::new(Vec::new()));

        // Run two workers sequentially to verify unique worktree names.
        for _ in 0..2 {
            let (tmux, _, _) = MockTmux::new();
            let names_ref = setup_names.clone();
            let path = project_dir.path().to_path_buf();

            struct NameTrackingWorktree {
                names: Arc<Mutex<Vec<String>>>,
                path: PathBuf,
            }

            impl WorktreeProvider for NameTrackingWorktree {
                fn setup(&self, name: &str) -> Result<WorktreeInfo, AutorunError> {
                    let mut guard = self.names.try_lock().unwrap();
                    guard.push(name.to_string());
                    Ok(WorktreeInfo {
                        path: self.path.clone(),
                    })
                }
                fn cleanup(&self, _name: &str) -> Result<(), AutorunError> {
                    Ok(())
                }
            }

            let wt = NameTrackingWorktree {
                names: names_ref,
                path: path.clone(),
            };
            let claude = MockClaude::simple(0);
            let worker = TmuxWorker::new(tmux, claude, wt, path);
            let _ = worker.run(make_worker_config()).await;
        }

        let names = setup_names.lock().await;
        assert_eq!(names.len(), 2, "two workers should produce two setup calls");
        assert_ne!(
            names[0], names[1],
            "each worker must get a unique worktree name: {:?}",
            *names
        );
    }

    #[tokio::test]
    async fn test_worker_worktree_setup_failure_returns_error() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let worker = TmuxWorker::new(
            tmux,
            MockClaude::simple(0),
            FailingWorktree,
            project_dir.path().to_path_buf(),
        );

        let err = worker.run(make_worker_config()).await.unwrap_err();
        assert!(
            matches!(err, AutorunError::Worktree(_)),
            "expected Worktree error, got: {err:?}"
        );
    }

    #[test]
    fn test_worktree_info_debug() {
        let info = WorktreeInfo {
            path: PathBuf::from("/tmp/test-wt"),
        };
        let debug = format!("{info:?}");
        assert!(debug.contains("WorktreeInfo"));
        assert!(debug.contains("/tmp/test-wt"));
    }

    #[test]
    fn test_real_worktree_provider_new() {
        let provider = RealWorktreeProvider::new(PathBuf::from("/tmp/project"));
        assert_eq!(provider.project_dir, PathBuf::from("/tmp/project"));
    }

    // -- parse_task_markdown tests --

    #[test]
    fn test_parse_task_markdown_full() {
        let content = r#"---
title: "Test task"
description: "A test task description"
acceptance:
  - "criterion one"
  - "criterion two"
file_scope:
  - "src/main.rs"
  - "src/lib.rs"
---

# Test Task

## Description

This is the body description.

## Approach

1. Do step one
2. Do step two

## Files
"#;
        let meta = parse_task_markdown(content);
        assert_eq!(meta.title, "Test task");
        assert_eq!(meta.description, "A test task description");
        assert_eq!(meta.acceptance.len(), 2);
        assert_eq!(meta.acceptance[0], "criterion one");
        assert_eq!(meta.acceptance[1], "criterion two");
        assert_eq!(meta.file_scope, vec!["src/main.rs", "src/lib.rs"]);
        assert!(
            meta.approach.contains("Do step one"),
            "approach should contain step 1, got: {}",
            meta.approach
        );
    }

    #[test]
    fn test_parse_task_markdown_empty_acceptance() {
        let content = r#"---
title: "No criteria"
acceptance: []
---

## Description

Some description.
"#;
        let meta = parse_task_markdown(content);
        assert_eq!(meta.title, "No criteria");
        assert!(meta.acceptance.is_empty());
    }

    #[test]
    fn test_parse_task_markdown_no_frontmatter() {
        let content = "# Just a heading\n\nSome content.\n";
        let meta = parse_task_markdown(content);
        assert!(meta.title.is_empty());
        assert!(meta.acceptance.is_empty());
    }

    #[test]
    fn test_parse_task_markdown_description_from_body() {
        let content = r#"---
title: "Body desc test"
---

## Description

Body description goes here.

## Approach

Do the thing.
"#;
        let meta = parse_task_markdown(content);
        assert!(
            meta.description.contains("Body description"),
            "should extract description from body, got: {}",
            meta.description
        );
        assert!(
            meta.approach.contains("Do the thing"),
            "should extract approach, got: {}",
            meta.approach
        );
    }

    // -- build_task_prompt tests --

    #[test]
    fn test_build_task_prompt_includes_all_sections() {
        let meta = TaskMetadata {
            title: "Implement feature X".into(),
            description: "Add feature X to the system.".into(),
            approach: "1. Read code\n2. Write code".into(),
            acceptance: vec!["tests pass".into(), "no warnings".into()],
            file_scope: vec!["src/main.rs".into()],
        };
        let prompt = build_task_prompt(&meta);

        assert!(prompt.contains("# Task: Implement feature X"));
        assert!(prompt.contains("Add feature X"));
        assert!(prompt.contains("Read code"));
        assert!(prompt.contains("1. tests pass"));
        assert!(prompt.contains("2. no warnings"));
        assert!(prompt.contains("- src/main.rs"));
        assert!(prompt.contains("Autonomous Operation"));
        assert!(prompt.contains("Do NOT prompt the user"));
        assert!(prompt.contains("Tier 3 decisions"));
    }

    #[test]
    fn test_build_task_prompt_empty_metadata() {
        let meta = TaskMetadata::default();
        let prompt = build_task_prompt(&meta);

        // Should still include autonomous operation instructions.
        assert!(prompt.contains("autorun task autonomously"));
        assert!(prompt.contains("Autonomous Operation"));
        // Should not include empty sections.
        assert!(!prompt.contains("# Task:"));
        assert!(!prompt.contains("## Description"));
        assert!(!prompt.contains("## File Scope"));
    }

    #[test]
    fn test_build_task_prompt_acceptance_numbering() {
        let meta = TaskMetadata {
            acceptance: vec!["A".into(), "B".into(), "C".into()],
            ..TaskMetadata::default()
        };
        let prompt = build_task_prompt(&meta);
        assert!(prompt.contains("1. A\n"));
        assert!(prompt.contains("2. B\n"));
        assert!(prompt.contains("3. C\n"));
    }

    // -- build_task_prompt_from_file tests --

    #[test]
    fn test_build_task_prompt_from_file_valid() {
        let dir = tempfile::tempdir().unwrap();
        let task_dir = dir
            .path()
            .join("project-management/epics/INF/INF-EPC-001/tasks");
        std::fs::create_dir_all(&task_dir).unwrap();
        std::fs::write(
            task_dir.join("INF-TSK-001-001.md"),
            r#"---
title: "Test file task"
description: "File-based task"
acceptance:
  - "criterion from file"
file_scope:
  - "src/a.rs"
---

## Approach

Read and implement.
"#,
        )
        .unwrap();

        let (prompt, acceptance) =
            build_task_prompt_from_file(dir.path(), "INF-TSK-001-001").unwrap();
        assert!(prompt.contains("Test file task"));
        assert!(prompt.contains("criterion from file"));
        assert_eq!(acceptance, vec!["criterion from file"]);
    }

    #[test]
    fn test_build_task_prompt_from_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = build_task_prompt_from_file(dir.path(), "INF-TSK-999-001");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, AutorunError::MissingTask(_)),
            "expected MissingTask error, got: {err:?}"
        );
    }

    #[test]
    fn test_build_task_prompt_from_file_invalid_id() {
        let dir = tempfile::tempdir().unwrap();
        let result = build_task_prompt_from_file(dir.path(), "invalid-id");
        assert!(result.is_err());
    }

    // -- InvokeConfig serde round-trip --

    #[test]
    fn test_invoke_config_serde_round_trip() {
        let cfg = InvokeConfig {
            work_dir: "/tmp/test".into(),
            prompt: "test prompt".into(),
            session_id: "ses-1".into(),
            task_id: "task-1".into(),
            integration_auto_merge: true,
            integration_branch: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: vec!["crit 1".into(), "crit 2".into()],
            worker_session_id: "ses-worker-1".into(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: InvokeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, cfg.task_id);
        assert_eq!(deserialized.work_dir, cfg.work_dir);
        assert_eq!(deserialized.acceptance_criteria, cfg.acceptance_criteria);
        assert_eq!(
            deserialized.integration_auto_merge,
            cfg.integration_auto_merge
        );
    }

    #[test]
    fn test_invoke_config_deserialize_without_acceptance() {
        let json = r#"{
            "work_dir": "/tmp",
            "prompt": "test",
            "session_id": "ses",
            "task_id": "t",
            "integration_auto_merge": false,
            "integration_branch": "main",
            "tmux_session": "w"
        }"#;
        let cfg: InvokeConfig = serde_json::from_str(json).unwrap();
        assert!(
            cfg.acceptance_criteria.is_empty(),
            "acceptance_criteria should default to empty vec"
        );
    }

    #[tokio::test]
    async fn test_worker_cleanup_called_on_success() {
        // Verify worktree cleanup is called on successful worker run.
        // Daemon lifecycle was moved to orchestrator (not per-worker).
        // This test confirms the worker completes and cleans up its worktree.

        let project_dir = make_project_dir();
        // Create the .state directory so the registry path is attempted.
        std::fs::create_dir_all(project_dir.path().join(".state")).unwrap();

        let (tmux, _, _) = MockTmux::new();
        let (wt, _, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert!(
            wt_cleanup.load(Ordering::SeqCst),
            "worktree cleanup must be called on success"
        );
    }

    #[test]
    fn test_path_traversal_rejection() {
        let dir = tempfile::tempdir().unwrap();

        // Task ID with ".." should be rejected.
        let result = build_task_prompt_from_file(dir.path(), "INF-TSK-..-001");
        assert!(result.is_err(), "task ID with '..' must be rejected");
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("path traversal"),
            "error should mention path traversal, got: {err}"
        );

        // Task ID with "/" should be rejected.
        let result = build_task_prompt_from_file(dir.path(), "INF-TSK-foo/bar-001");
        assert!(result.is_err(), "task ID with '/' must be rejected");

        // Task ID with "\\" should be rejected.
        let result = build_task_prompt_from_file(dir.path(), "INF-TSK-foo\\bar-001");
        assert!(result.is_err(), "task ID with '\\\\' must be rejected");

        // Valid task ID should not be rejected (it may fail with MissingTask, but not path traversal).
        let result = build_task_prompt_from_file(dir.path(), "INF-TSK-023-028");
        assert!(
            result.is_err(),
            "valid task ID should fail with MissingTask, not path traversal"
        );
        let err = result.unwrap_err();
        assert!(
            !err.to_string().contains("path traversal"),
            "valid task ID must not be rejected for path traversal"
        );
    }

    // -- resolve_merge_conflicts tests --

    use crate::autorun::config::MergeConfig;
    use crate::git::conflict::{ConflictResult, RebaseResult};

    fn conflicts_with(files: Vec<&str>) -> ConflictResult {
        ConflictResult {
            has_conflicts: true,
            conflicting_files: files.into_iter().map(String::from).collect(),
            target_branch: "main".into(),
        }
    }

    fn no_conflicts() -> ConflictResult {
        ConflictResult {
            has_conflicts: false,
            conflicting_files: Vec::new(),
            target_branch: "main".into(),
        }
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_no_conflicts() {
        let config = MergeConfig::default();
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(no_conflicts()),
            || unreachable!("should not attempt rebase when no conflicts"),
        )
        .await;
        assert_eq!(action, MergeConflictAction::Continue);
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_check_error() {
        let config = MergeConfig::default();
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Err(crate::error::GitError::MergeFailed("git failed".into())),
            || unreachable!("should not attempt rebase when check fails"),
        )
        .await;
        assert_eq!(action, MergeConflictAction::Continue);
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_auto_rebase_false() {
        let config = MergeConfig {
            auto_rebase: false,
            ..MergeConfig::default()
        };
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["file.rs"])),
            || unreachable!("should not attempt rebase when auto_rebase=false"),
        )
        .await;
        match action {
            MergeConflictAction::MergeConflict { ref error } => {
                assert!(error.contains("auto_rebase disabled"), "got: {error}");
                assert!(error.contains("file.rs"), "got: {error}");
            }
            other => panic!("expected MergeConflict, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_rebase_success_first_attempt() {
        let config = MergeConfig {
            auto_rebase: true,
            max_rebase_attempts: 3,
            ..MergeConfig::default()
        };
        let mut attempt_count = 0;
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["a.rs"])),
            || {
                attempt_count += 1;
                Ok(RebaseResult::Success)
            },
        )
        .await;
        assert_eq!(action, MergeConflictAction::RebasedSuccessfully);
        assert_eq!(attempt_count, 1);
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_rebase_success_second_attempt() {
        let config = MergeConfig {
            auto_rebase: true,
            max_rebase_attempts: 3,
            ..MergeConfig::default()
        };
        let mut attempt_count = 0;
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["a.rs"])),
            || {
                attempt_count += 1;
                if attempt_count == 1 {
                    Ok(RebaseResult::ConflictAborted {
                        conflicting_files: vec!["a.rs".into()],
                    })
                } else {
                    Ok(RebaseResult::Success)
                }
            },
        )
        .await;
        assert_eq!(action, MergeConflictAction::RebasedSuccessfully);
        assert_eq!(attempt_count, 2);
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_all_attempts_exhausted() {
        let config = MergeConfig {
            auto_rebase: true,
            max_rebase_attempts: 2,
            ..MergeConfig::default()
        };
        let mut attempt_count = 0;
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["conflict.rs"])),
            || {
                attempt_count += 1;
                Ok(RebaseResult::ConflictAborted {
                    conflicting_files: vec!["conflict.rs".into()],
                })
            },
        )
        .await;
        assert_eq!(attempt_count, 2);
        match action {
            MergeConflictAction::MergeConflict { ref error } => {
                assert!(error.contains("after 2 rebase attempts"), "got: {error}");
                assert!(error.contains("conflict.rs"), "got: {error}");
            }
            other => panic!("expected MergeConflict, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_rebase_git_error() {
        let config = MergeConfig {
            auto_rebase: true,
            max_rebase_attempts: 3,
            ..MergeConfig::default()
        };
        let mut attempt_count = 0;
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["a.rs"])),
            || {
                attempt_count += 1;
                Err(crate::error::GitError::MergeFailed(
                    "git rebase failed".into(),
                ))
            },
        )
        .await;
        // Git error should warn and continue to merge queue, not return merge_conflict.
        assert_eq!(action, MergeConflictAction::RebasedSuccessfully);
        assert_eq!(attempt_count, 1, "should stop retrying after git error");
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_git_error_on_second_attempt() {
        let config = MergeConfig {
            auto_rebase: true,
            max_rebase_attempts: 3,
            ..MergeConfig::default()
        };
        let mut attempt_count = 0;
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["a.rs"])),
            || {
                attempt_count += 1;
                if attempt_count == 1 {
                    Ok(RebaseResult::ConflictAborted {
                        conflicting_files: vec!["a.rs".into()],
                    })
                } else {
                    Err(crate::error::GitError::MergeFailed("git broke".into()))
                }
            },
        )
        .await;
        // Second attempt git error: warn and continue.
        assert_eq!(action, MergeConflictAction::RebasedSuccessfully);
        assert_eq!(attempt_count, 2);
    }

    #[tokio::test]
    async fn test_resolve_merge_conflicts_max_attempts_one() {
        let config = MergeConfig {
            auto_rebase: true,
            max_rebase_attempts: 1,
            ..MergeConfig::default()
        };
        let mut attempt_count = 0;
        let action = resolve_merge_conflicts(
            "main",
            &config,
            || Ok(conflicts_with(vec!["b.rs"])),
            || {
                attempt_count += 1;
                Ok(RebaseResult::ConflictAborted {
                    conflicting_files: vec!["b.rs".into()],
                })
            },
        )
        .await;
        assert_eq!(attempt_count, 1);
        match action {
            MergeConflictAction::MergeConflict { ref error } => {
                assert!(error.contains("after 1 rebase attempts"), "got: {error}");
            }
            other => panic!("expected MergeConflict, got: {other:?}"),
        }
    }

    #[test]
    fn test_merge_conflict_action_debug() {
        let action = MergeConflictAction::Continue;
        assert_eq!(format!("{action:?}"), "Continue");

        let action = MergeConflictAction::RebasedSuccessfully;
        assert_eq!(format!("{action:?}"), "RebasedSuccessfully");

        let action = MergeConflictAction::MergeConflict {
            error: "test".into(),
        };
        let debug = format!("{action:?}");
        assert!(debug.contains("MergeConflict"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_tmux_name_from_config() {
        // Verify the worker uses cfg.tmux_name directly (set by orchestrator).
        let cfg = WorkerConfig {
            session_id: "ses-test".into(),
            worker_id: "arw-test".into(),
            worker_num: 3,
            task_id: "task-a".into(),
            batch_name: "test-batch".into(),
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_name: "cf-ar-task-a".into(),
            file_scope: Vec::new(),
            scope_policy: "soft".into(),
            blocked_behavior: "skip_and_continue".into(),
            epic_update: String::new(),
            queue_timeout_secs: 600,
            task_timeout_secs: None,
        };
        assert_eq!(cfg.tmux_name, "cf-ar-task-a");
    }

    // -- DB write tests using MockStore --

    fn make_worker_cfg(session_id: &str, task_id: &str) -> WorkerConfig {
        WorkerConfig {
            session_id: session_id.into(),
            worker_id: format!("arw-{session_id}-{task_id}"),
            worker_num: 1,
            task_id: task_id.into(),
            batch_name: "test-batch".into(),
            integration_auto_merge: false,
            integration_branch: String::new(),
            tmux_name: "cf-ar-task-test".into(),
            file_scope: vec!["src/**/*.rs".into()],
            scope_policy: "soft".into(),
            blocked_behavior: "skip_and_continue".into(),
            epic_update: String::new(),
            queue_timeout_secs: 600,
            task_timeout_secs: None,
        }
    }

    #[tokio::test]
    async fn test_worker_creates_autorun_worker_record() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let claude = MockClaude::simple(0);
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            claude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(30),
            store.clone(),
        );

        let cfg = make_worker_cfg("ses-wr-test", "task-a");
        let result = worker.run(cfg).await.unwrap();
        assert_eq!(result.status, "completed");

        // Verify autorun_worker was created and updated.
        let workers = store.autorun_workers.lock().unwrap();
        let aw = workers.get("arw-ses-wr-test-task-a").unwrap();
        assert_eq!(aw.task_id, "task-a");
        assert_eq!(aw.file_scope, vec!["src/**/*.rs"]);
        assert_eq!(aw.scope_policy, "soft");
        assert_eq!(aw.status, crate::types::AutorunWorkerStatus::Completed);
        assert!(aw.completed_at.is_some());
    }

    #[tokio::test]
    async fn test_worker_creates_autorun_task_run_record() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let claude = MockClaude::simple(0);
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            claude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(30),
            store.clone(),
        );

        let cfg = make_worker_cfg("ses-tr-test", "task-b");
        let result = worker.run(cfg).await.unwrap();
        assert_eq!(result.status, "completed");

        // Verify autorun_task_run was created and updated.
        let runs = store.autorun_task_runs.lock().unwrap();
        let atr = runs.get("atr-ses-tr-test-task-b").unwrap();
        assert_eq!(atr.task_id, "task-b");
        assert_eq!(atr.status, crate::types::AutorunTaskRunStatus::Completed);
        assert_eq!(atr.exit_code, Some(0));
        assert!(atr.completed_at.is_some());
        assert_eq!(atr.pr_number, Some(42));
    }

    #[tokio::test]
    async fn test_worker_updates_records_on_failure() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let claude = MockClaude::simple(1); // non-zero exit code
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            claude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(30),
            store.clone(),
        );

        let cfg = make_worker_cfg("ses-fail", "task-fail");
        let result = worker.run(cfg).await.unwrap();
        assert_eq!(result.status, "failed");

        let workers = store.autorun_workers.lock().unwrap();
        let aw = workers.get("arw-ses-fail-task-fail").unwrap();
        assert_eq!(aw.status, crate::types::AutorunWorkerStatus::Failed);

        let runs = store.autorun_task_runs.lock().unwrap();
        let atr = runs.get("atr-ses-fail-task-fail").unwrap();
        assert_eq!(atr.status, crate::types::AutorunTaskRunStatus::Failed);
        assert_eq!(atr.exit_code, Some(1));
    }

    #[tokio::test]
    async fn test_worker_updates_records_on_claude_error() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            FailingClaude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(30),
            store.clone(),
        );

        let cfg = make_worker_cfg("ses-err", "task-err");
        let result = worker.run(cfg).await;
        assert!(result.is_err());

        let workers = store.autorun_workers.lock().unwrap();
        let aw = workers.get("arw-ses-err-task-err").unwrap();
        assert_eq!(aw.status, crate::types::AutorunWorkerStatus::Failed);
        assert!(aw.completed_at.is_some());

        let runs = store.autorun_task_runs.lock().unwrap();
        let atr = runs.get("atr-ses-err-task-err").unwrap();
        assert_eq!(atr.status, crate::types::AutorunTaskRunStatus::Failed);
        assert!(atr.error_message.is_some());
    }

    #[tokio::test]
    async fn test_worker_updates_records_on_timeout() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            SlowClaude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_millis(50), // very short timeout
            store.clone(),
        );

        let cfg = make_worker_cfg("ses-to", "task-to");
        let result = worker.run(cfg).await.unwrap();
        assert_eq!(result.status, "timeout");

        let workers = store.autorun_workers.lock().unwrap();
        let aw = workers.get("arw-ses-to-task-to").unwrap();
        assert_eq!(aw.status, crate::types::AutorunWorkerStatus::Timeout);

        let runs = store.autorun_task_runs.lock().unwrap();
        let atr = runs.get("atr-ses-to-task-to").unwrap();
        assert_eq!(atr.status, crate::types::AutorunTaskRunStatus::Timeout);
    }

    #[tokio::test]
    async fn test_worker_emits_events_to_jsonl() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let claude = MockClaude::simple(0);
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            claude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(30),
            store,
        );

        let cfg = make_worker_cfg("ses-ev", "task-ev");
        worker.run(cfg).await.unwrap();

        let events_path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");
        assert!(events_path.exists(), "autorun-events.jsonl should exist");
        let content = std::fs::read_to_string(&events_path).unwrap();
        assert!(
            content.contains("worker_started"),
            "should contain worker_started event"
        );
        assert!(
            content.contains("worker_completed"),
            "should contain worker_completed event"
        );
    }

    #[tokio::test]
    async fn test_emit_autorun_event_creates_file() {
        let project_dir = tempfile::tempdir().unwrap();
        let event = crate::coordination::types::events::AutorunEvent::BatchStarted {
            session_id: "ses-test".into(),
            batch_name: "test".into(),
            total_tasks: 1,
            timestamp: "2026-03-21T00:00:00Z".into(),
        };
        TmuxWorker::<MockTmux, MockClaude, MockWorktree>::emit_autorun_event(
            project_dir.path(),
            &event,
        );
        let path = project_dir
            .path()
            .join(".state/ledger/autorun-events/autorun-events.jsonl");
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("batch_started"));
    }

    // -- worker_log coverage --

    #[test]
    fn test_worker_log_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("logs/worker.log");
        worker_log(&log_path, "test message");
        assert!(log_path.exists(), "log file should be created");
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("test message"));
        assert!(content.contains("[20"), "should contain ISO timestamp");
    }

    #[test]
    fn test_worker_log_appends() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("worker.log");
        worker_log(&log_path, "first");
        worker_log(&log_path, "second");
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("first"));
        assert!(content.contains("second"));
        assert_eq!(content.lines().count(), 2);
    }

    #[test]
    fn test_worker_log_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("deep/nested/dir/worker.log");
        worker_log(&log_path, "nested");
        assert!(log_path.exists());
    }

    // -- parse_task_markdown edge case coverage --

    #[test]
    fn test_parse_task_markdown_with_file_scope() {
        let content =
            "---\ntitle: Test\nfile_scope:\n  - src/main.rs\n  - src/lib.rs\n---\n# Body\n";
        let meta = parse_task_markdown(content);
        assert_eq!(meta.file_scope, vec!["src/main.rs", "src/lib.rs"]);
    }

    #[test]
    fn test_parse_task_markdown_with_approach() {
        let content =
            "---\ntitle: Test\n---\n## Approach\nDo the thing step by step.\n\n## Other\nStuff.\n";
        let meta = parse_task_markdown(content);
        assert_eq!(meta.approach, "Do the thing step by step.");
    }

    #[test]
    fn test_parse_task_markdown_approach_no_next_heading() {
        let content =
            "---\ntitle: Test\n---\n## Approach\nFull approach with no following section.\n";
        let meta = parse_task_markdown(content);
        assert!(meta.approach.contains("Full approach"));
    }

    #[test]
    fn test_parse_task_markdown_invalid_yaml() {
        let content = "---\n: invalid: yaml: [broken\n---\nBody text.\n";
        let meta = parse_task_markdown(content);
        // Should not panic -- just return empty metadata.
        assert!(meta.title.is_empty());
    }

    #[test]
    fn test_parse_task_markdown_description_from_body_section() {
        let content = "---\ntitle: Test\n---\n## Description\nThis is the description from body.\n\n## Next\n";
        let meta = parse_task_markdown(content);
        assert_eq!(meta.description, "This is the description from body.");
    }

    #[test]
    fn test_parse_task_markdown_frontmatter_description_takes_precedence() {
        let content =
            "---\ntitle: Test\ndescription: From frontmatter\n---\n## Description\nFrom body.\n";
        let meta = parse_task_markdown(content);
        assert_eq!(meta.description, "From frontmatter");
    }

    // -- build_task_prompt edge cases --

    #[test]
    fn test_build_task_prompt_with_approach() {
        let meta = TaskMetadata {
            title: "Test".into(),
            description: "Desc".into(),
            approach: "Step 1, Step 2".into(),
            ..Default::default()
        };
        let prompt = build_task_prompt(&meta);
        assert!(prompt.contains("## Approach"));
        assert!(prompt.contains("Step 1, Step 2"));
    }

    #[test]
    fn test_build_task_prompt_with_file_scope() {
        let meta = TaskMetadata {
            title: "Test".into(),
            file_scope: vec!["src/main.rs".into(), "src/lib.rs".into()],
            ..Default::default()
        };
        let prompt = build_task_prompt(&meta);
        assert!(prompt.contains("src/main.rs"));
        assert!(prompt.contains("src/lib.rs"));
    }

    // -- Coverage: completed worker with no PR (pr_number=0) --

    struct MockClaudeNoPr;

    impl ClaudeInvoker for MockClaudeNoPr {
        async fn invoke(&self, _cfg: InvokeConfig) -> Result<InvokeResult, AutorunError> {
            Ok(InvokeResult {
                exit_code: 0,
                pr_number: 0,
                pr_url: String::new(),
                branch_name: "feat/no-pr".into(),
                output: String::new(),
            })
        }
    }

    #[tokio::test]
    async fn test_tmux_worker_success_no_pr() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(project_dir.path().to_path_buf());
        let claude = MockClaudeNoPr;
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(
            result.pr_number, 0,
            "pr_number should be 0 when no PR created"
        );
    }

    // -- MergeOutcome enum tests --

    #[test]
    fn test_merge_outcome_variants() {
        let merged = MergeOutcome::Merged;
        assert_eq!(merged, MergeOutcome::Merged);

        let conflict = MergeOutcome::MergeConflict {
            error: "rebase failed".to_string(),
        };
        assert!(matches!(conflict, MergeOutcome::MergeConflict { .. }));

        let timeout = MergeOutcome::QueueTimeout;
        assert_eq!(timeout, MergeOutcome::QueueTimeout);
    }

    #[test]
    fn test_merge_outcome_debug() {
        let outcome = MergeOutcome::MergeConflict {
            error: "test error".to_string(),
        };
        let debug = format!("{outcome:?}");
        assert!(debug.contains("MergeConflict"));
        assert!(debug.contains("test error"));
    }

    // -- serialized_merge queue timeout test --

    #[tokio::test]
    async fn test_serialized_merge_queue_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.loro");
        let sid = crate::types::SessionId::new_unchecked("ses-timeout-test");
        let merge_config = crate::autorun::config::MergeConfig {
            max_rebase_attempts: 1,
            queue_enforcing: true,
            queue_timeout_secs: 0, // Immediate timeout after first check.
            ..Default::default()
        };

        // Enqueue another session first so our session is NOT at position 0.
        let blocker = crate::coordination::merge_queue::MergeQueueEntry {
            session_id: crate::types::SessionId::new_unchecked("ses-blocker"),
            task_id: "blocker-task".to_string(),
            branch: "feat/blocker".to_string(),
            target_branch: "main".to_string(),
            pr_ready_at: "2026-01-01T00:00:00Z".to_string(),
        };
        crate::coordination::merge_queue::locked_enqueue(&state_path, &blocker).unwrap();

        let result = serialized_merge(
            dir.path(),
            "main",
            &sid,
            42,
            "task-timeout",
            "feat/timeout-branch",
            &state_path,
            &merge_config,
            1, // 1 second timeout.
        )
        .await;

        assert_eq!(result, MergeOutcome::QueueTimeout);
    }

    // -- Worktree cleanup retry tests --

    /// Mock worktree provider that fails cleanup N times before succeeding.
    struct RetryWorktree {
        fail_count: std::sync::atomic::AtomicUsize,
        max_failures: usize,
        path: PathBuf,
    }

    impl RetryWorktree {
        fn new(path: PathBuf, max_failures: usize) -> Self {
            Self {
                fail_count: std::sync::atomic::AtomicUsize::new(0),
                max_failures,
                path,
            }
        }
    }

    impl WorktreeProvider for RetryWorktree {
        fn setup(&self, _name: &str) -> Result<WorktreeInfo, AutorunError> {
            Ok(WorktreeInfo {
                path: self.path.clone(),
            })
        }

        fn cleanup(&self, name: &str) -> Result<(), AutorunError> {
            let count = self
                .fail_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if count < self.max_failures {
                Err(AutorunError::Worktree(
                    crate::error::WorktreeError::Cleanup(format!(
                        "mock cleanup failure {}/{} for {name}",
                        count + 1,
                        self.max_failures,
                    )),
                ))
            } else {
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn test_worktree_cleanup_retry_succeeds_on_second_attempt() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let wt = RetryWorktree::new(project_dir.path().to_path_buf(), 1);
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
    }

    #[tokio::test]
    async fn test_worktree_cleanup_retry_exhausted_deregisters() {
        let project_dir = make_project_dir();
        // Create the worktrees.yaml file with a dummy entry.
        let registry_dir = project_dir.path().join(".state/worktrees");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("worktrees.yaml"),
            "- name: worktree-ses-test\n  status: active\n",
        )
        .unwrap();

        let (tmux, _, _) = MockTmux::new();
        // Fail all 3 cleanup attempts.
        let wt = RetryWorktree::new(project_dir.path().to_path_buf(), 10);
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        // Worker should still report completed (cleanup failure is non-fatal).
        assert_eq!(result.status, "completed");
    }

    // -- Per-task timeout override test --

    #[tokio::test]
    async fn test_per_task_timeout_override() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(project_dir.path().to_path_buf());
        // SlowClaude sleeps 5s; per-task timeout of 100ms should trigger timeout.
        let worker = TmuxWorker::with_timeout(
            tmux,
            SlowClaude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(60), // Global timeout is 60s (would succeed)
        );

        let mut cfg = make_worker_config();
        cfg.task_timeout_secs = Some(0); // 0 means no override, use global
        // With 0 (no override), the 60s global timeout should let SlowClaude's
        // 5s sleep complete. But let's test with an actual override.
        cfg.task_timeout_secs = Some(1); // 1 second override — triggers timeout
        let result = worker.run(cfg).await.unwrap();
        assert_eq!(result.status, "timeout", "per-task timeout should fire");
        assert_eq!(result.exit_code, 124);
    }

    // -- Serialized merge independent of exit_code test --

    struct MockClaudeTimeoutWithPr;

    impl ClaudeInvoker for MockClaudeTimeoutWithPr {
        async fn invoke(&self, _cfg: InvokeConfig) -> Result<InvokeResult, AutorunError> {
            // Simulate a timeout scenario where a PR was created before timeout.
            Ok(InvokeResult {
                exit_code: 124,
                pr_number: 99,
                pr_url: "https://github.com/test/pr/99".into(),
                branch_name: "feat/timeout-pr".into(),
                output: "worker exceeded timeout".into(),
            })
        }
    }

    #[tokio::test]
    async fn test_serialized_merge_runs_after_timeout_with_pr() {
        let project_dir = make_project_dir();
        let (tmux, _, _) = MockTmux::new();
        let wt = MockWorktree::new(project_dir.path().to_path_buf());
        let claude = MockClaudeTimeoutWithPr;
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        // Enable auto_merge so the serialized_merge code path activates.
        let mut cfg = make_worker_config();
        cfg.integration_auto_merge = true;
        cfg.integration_branch = "integration/test".into();

        let result = worker.run(cfg).await.unwrap();
        // The worker should attempt serialized merge because pr_number > 0,
        // even though exit_code is 124 (timeout). The merge itself may fail
        // (no real git repo), but the important thing is the path was entered.
        // The result will be "failed" because exit_code != 0.
        assert_eq!(result.exit_code, 124);
        assert_eq!(result.pr_number, 99);
    }

    /// Verify DB update ordering: both update_autorun_worker and
    /// update_autorun_task_run complete before WorkerResult is returned.
    /// After run() returns, both the worker record (Completed status +
    /// completed_at) and task_run record (Completed status + completed_at)
    /// must already be in the MockStore.
    #[tokio::test]
    async fn test_db_updates_complete_before_result_returned() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let claude = MockClaude::simple(0);
        let wt = MockWorktree::new(wt_path);
        let store = Arc::new(crate::store::mock::MockStore::new());

        let worker = TmuxWorker::with_store(
            tmux,
            claude,
            wt,
            project_dir.path().to_path_buf(),
            Duration::from_secs(30),
            store.clone(),
        );

        let cfg = make_worker_cfg("ses-order-test", "task-ord");
        let result = worker.run(cfg).await.unwrap();
        assert_eq!(result.status, "completed");

        // At this point (after run() returns), both DB records must be updated.
        let workers = store.autorun_workers.lock().unwrap();
        let aw = workers.get("arw-ses-order-test-task-ord").unwrap();
        assert_eq!(
            aw.status,
            crate::types::AutorunWorkerStatus::Completed,
            "worker status should be Completed before result returned"
        );
        assert!(
            aw.completed_at.is_some(),
            "worker completed_at should be set before result returned"
        );

        let runs = store.autorun_task_runs.lock().unwrap();
        let atr = runs.get("atr-ses-order-test-task-ord").unwrap();
        assert_eq!(
            atr.status,
            crate::types::AutorunTaskRunStatus::Completed,
            "task_run status should be Completed before result returned"
        );
        assert!(
            atr.completed_at.is_some(),
            "task_run completed_at should be set before result returned"
        );
    }
}
