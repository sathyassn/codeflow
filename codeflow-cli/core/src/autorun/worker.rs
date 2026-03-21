//! Worker DI traits and types for autorun task execution.
//!
//! Provides `TmuxRunner`, `ClaudeInvoker`, `WorktreeProvider`, and `WorkerRunner`
//! traits for isolated, testable worker execution. Each worker creates a
//! per-worker git worktree for filesystem isolation.

use std::path::PathBuf;
use std::time::Duration;

use crate::error::AutorunError;

use super::orchestrator::{WorkerConfig, WorkerResult};

/// Default timeout for a single worker (60 minutes).
pub const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Executes commands via tmux. Enables testing without actual tmux sessions.
pub trait TmuxRunner: Send + Sync {
    /// Create a new tmux session with the given name.
    fn create_session(
        &self,
        name: &str,
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
#[derive(Debug, Clone)]
pub struct InvokeConfig {
    pub work_dir: String,
    pub prompt: String,
    pub session_id: String,
    pub task_id: String,
    pub auto_merge: bool,
    pub target: String,
    pub tmux_session: String,
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
pub struct TmuxWorker<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider> {
    pub tmux: T,
    pub claude: C,
    pub worktree: W,
    pub timeout: Duration,
    pub project_dir: PathBuf,
}

impl<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider> TmuxWorker<T, C, W> {
    /// Create a new `TmuxWorker` with the default timeout.
    #[must_use]
    pub fn new(tmux: T, claude: C, worktree: W, project_dir: PathBuf) -> Self {
        Self {
            tmux,
            claude,
            worktree,
            timeout: DEFAULT_WORKER_TIMEOUT,
            project_dir,
        }
    }

    /// Create a new `TmuxWorker` with a custom timeout.
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
        }
    }
}

impl<T: TmuxRunner, C: ClaudeInvoker, W: WorktreeProvider> WorkerRunner for TmuxWorker<T, C, W> {
    async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
        let tmux_name = format!("{}-{}", cfg.tmux_prefix, cfg.worker_num);

        let timeout = if self.timeout.is_zero() {
            DEFAULT_WORKER_TIMEOUT
        } else {
            self.timeout
        };

        // Generate a unique worktree name from the session ID.
        // Use 16 chars of the ULID (chars 4..20) to ensure uniqueness even
        // when workers start within the same millisecond (timestamp portion
        // is only 10 Crockford chars; remaining 16 are random).
        let worker_sid = crate::session::generate_session_id();
        let sid_str = worker_sid.as_str();
        let suffix_end = std::cmp::min(20, sid_str.len());
        let wt_name = format!("worktree-{}", &sid_str[4..suffix_end]);

        // Create the worktree for filesystem isolation.
        let wt_info = self.worktree.setup(&wt_name)?;

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
            match crate::file_lock::locked_binary_rmw(
                &state_path,
                crate::coordination::loro::LoroCoordinator::in_memory,
                |bytes| {
                    crate::coordination::loro::LoroCoordinator::from_bytes(bytes, &state_path)
                        .map_err(|e| format!("load coordinator: {e}"))
                },
                |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
                |coord| {
                    let _ =
                        crate::coordination::claims::acquire_batch(coord, &scope_refs, &worker_sid);
                    Ok(())
                },
            ) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("warning: claim acquisition failed: {e}");
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
            "session_id": cfg.session_id,
            "scope_policy": cfg.scope_policy,
            "file_scope": cfg.file_scope,
            "worktree_path": wt_info.path.to_string_lossy(),
        });
        let _ = std::fs::write(&active_task_path, active_task.to_string());

        // Create the tmux session.
        self.tmux.create_session(&tmux_name).await?;

        // Run with timeout, using worktree path as work_dir.
        let result = tokio::time::timeout(timeout, async {
            self.claude
                .invoke(InvokeConfig {
                    work_dir: wt_info.path.to_string_lossy().into_owned(),
                    prompt: format!("Execute autorun task {}", cfg.task_id),
                    session_id: cfg.session_id.clone(),
                    task_id: cfg.task_id.clone(),
                    auto_merge: cfg.auto_merge,
                    target: cfg.target.clone(),
                    tmux_session: tmux_name.clone(),
                })
                .await
        })
        .await;

        // Check for merge conflicts before PR creation.
        if let Ok(Ok(ref invoke_result)) = result {
            if invoke_result.exit_code == 0 && !cfg.target.is_empty() {
                match crate::git::conflict::check_merge_conflicts(&wt_info.path, &cfg.target) {
                    Ok(conflict_result) if conflict_result.has_conflicts => {
                        eprintln!(
                            "warning: merge conflicts detected with {}: {:?}",
                            cfg.target, conflict_result.conflicting_files
                        );
                    }
                    Err(e) => {
                        eprintln!("warning: merge conflict check failed: {e}");
                    }
                    _ => {}
                }
            }
        }

        // Enqueue in merge queue before PR merge.
        if let Ok(Ok(ref invoke_result)) = result {
            if invoke_result.exit_code == 0 {
                let entry = crate::coordination::merge_queue::MergeQueueEntry {
                    session_id: worker_sid.clone(),
                    task_id: cfg.task_id.clone(),
                    branch: invoke_result.branch_name.clone(),
                    pr_ready_at: chrono::Utc::now().to_rfc3339(),
                };
                if let Err(e) =
                    crate::coordination::merge_queue::locked_enqueue(&state_path, &entry)
                {
                    eprintln!("warning: merge queue enqueue failed: {e}");
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
            Ok(()) => {}
            Err(e) => {
                eprintln!("warning: claim release failed: {e}");
            }
        }

        // Dequeue from merge queue after completion.
        if let Err(e) = crate::coordination::merge_queue::locked_dequeue(&state_path) {
            eprintln!("warning: merge queue dequeue failed: {e}");
        }

        let _ = self.tmux.kill_session(&tmux_name).await;
        if let Err(e) = self.worktree.cleanup(&wt_name) {
            eprintln!("warning: worktree cleanup failed for {wt_name}: {e}");
        }

        match result {
            Ok(Ok(invoke_result)) => {
                let status = if invoke_result.exit_code == 0 {
                    "completed"
                } else {
                    "failed"
                };
                Ok(WorkerResult {
                    worker_id: cfg.worker_id,
                    task_id: cfg.task_id,
                    status: status.into(),
                    exit_code: invoke_result.exit_code,
                    pr_number: invoke_result.pr_number,
                    pr_url: invoke_result.pr_url,
                    branch_name: invoke_result.branch_name,
                    error: String::new(),
                    duration_sec: 0,
                })
            }
            Ok(Err(e)) => Err(AutorunError::WorkerFailed(format!(
                "task {}: {e}",
                cfg.task_id
            ))),
            Err(_) => Ok(WorkerResult {
                worker_id: cfg.worker_id,
                task_id: cfg.task_id,
                status: "timeout".into(),
                exit_code: 124, // standard timeout exit code
                pr_number: 0,
                pr_url: String::new(),
                branch_name: String::new(),
                error: "worker exceeded timeout".into(),
                duration_sec: 0,
            }),
        }
    }
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
        Ok(WorktreeInfo {
            path: PathBuf::from(&entry.path),
        })
    }

    fn cleanup(&self, name: &str) -> Result<(), AutorunError> {
        let mgr = crate::worktree::WorktreeManager::new(&self.project_dir);
        let opts = crate::worktree::CleanupOpts {
            force: true,
            dry_run: false,
            prune: false,
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
        async fn create_session(&self, _name: &str) -> Result<(), AutorunError> {
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
            auto_merge: false,
            target: "main".into(),
            tmux_prefix: "codeflow-worker".into(),
            file_scope: Vec::new(),
            scope_policy: "soft".into(),
        }
    }

    #[tokio::test]
    async fn test_tmux_worker_success() {
        let project_dir = make_project_dir();
        let (tmux, created, killed) = MockTmux::new();
        let (wt, wt_setup, wt_cleanup, _, _) =
            MockWorktree::with_tracking(project_dir.path().to_path_buf());
        let claude = MockClaude::simple(0);
        let worker = TmuxWorker::new(tmux, claude, wt, project_dir.path().to_path_buf());

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.pr_number, 42);
        assert!(created.load(Ordering::SeqCst));
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
        assert_eq!(DEFAULT_WORKER_TIMEOUT, Duration::from_secs(3600));
    }

    #[test]
    fn test_invoke_config_fields() {
        let cfg = InvokeConfig {
            work_dir: ".".into(),
            prompt: "do stuff".into(),
            session_id: "ses-1".into(),
            task_id: "t-1".into(),
            auto_merge: true,
            target: "develop".into(),
            tmux_session: "worker-1".into(),
        };
        assert_eq!(cfg.task_id, "t-1");
        assert!(cfg.auto_merge);
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
        let (tmux, created, killed) = MockTmux::new();
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
        assert!(created.load(Ordering::SeqCst));
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
        tmux.create_session("test-session").await.unwrap();
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
}
