//! Worker DI traits and types for autorun task execution.
//!
//! Provides `TmuxRunner`, `ClaudeInvoker`, and `WorkerRunner` traits
//! for isolated, testable worker execution.

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
pub struct TmuxWorker<T: TmuxRunner, C: ClaudeInvoker> {
    pub tmux: T,
    pub claude: C,
    pub timeout: Duration,
}

impl<T: TmuxRunner, C: ClaudeInvoker> TmuxWorker<T, C> {
    /// Create a new `TmuxWorker` with the default timeout.
    #[must_use]
    pub fn new(tmux: T, claude: C) -> Self {
        Self {
            tmux,
            claude,
            timeout: DEFAULT_WORKER_TIMEOUT,
        }
    }

    /// Create a new `TmuxWorker` with a custom timeout.
    #[must_use]
    pub fn with_timeout(tmux: T, claude: C, timeout: Duration) -> Self {
        Self {
            tmux,
            claude,
            timeout,
        }
    }
}

impl<T: TmuxRunner, C: ClaudeInvoker> WorkerRunner for TmuxWorker<T, C> {
    async fn run(&self, cfg: WorkerConfig) -> Result<WorkerResult, AutorunError> {
        let tmux_name = format!("{}-{}", cfg.tmux_prefix, cfg.worker_num);

        let timeout = if self.timeout.is_zero() {
            DEFAULT_WORKER_TIMEOUT
        } else {
            self.timeout
        };

        // Create the tmux session.
        self.tmux.create_session(&tmux_name).await?;

        // Run with timeout.
        let result = tokio::time::timeout(timeout, async {
            self.claude
                .invoke(InvokeConfig {
                    work_dir: ".".into(),
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

        // Always cleanup the tmux session.
        let _ = self.tmux.kill_session(&tmux_name).await;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

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
    }

    impl ClaudeInvoker for MockClaude {
        async fn invoke(&self, _cfg: InvokeConfig) -> Result<InvokeResult, AutorunError> {
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
        }
    }

    #[tokio::test]
    async fn test_tmux_worker_success() {
        let (tmux, created, killed) = MockTmux::new();
        let claude = MockClaude { exit_code: 0 };
        let worker = TmuxWorker::new(tmux, claude);

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.pr_number, 42);
        assert!(created.load(Ordering::SeqCst));
        assert!(killed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_tmux_worker_failure() {
        let (tmux, _, _) = MockTmux::new();
        let claude = MockClaude { exit_code: 1 };
        let worker = TmuxWorker::new(tmux, claude);

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_tmux_worker_timeout() {
        let (tmux, _, killed) = MockTmux::new();
        let worker = TmuxWorker::with_timeout(tmux, SlowClaude, Duration::from_millis(100));

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "timeout");
        assert_eq!(result.exit_code, 124);
        assert!(result.error.contains("timeout"));
        assert!(
            killed.load(Ordering::SeqCst),
            "session should be cleaned up on timeout"
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
        let (tmux, created, killed) = MockTmux::new();
        let claude = MockClaude { exit_code: 0 };
        let worker = TmuxWorker::with_timeout(tmux, claude, Duration::ZERO);

        let result = worker.run(make_worker_config()).await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.exit_code, 0);
        assert!(created.load(Ordering::SeqCst));
        assert!(killed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_tmux_worker_invoke_error() {
        let (tmux, _, killed) = MockTmux::new();
        let worker = TmuxWorker::new(tmux, FailingClaude);

        let err = worker.run(make_worker_config()).await.unwrap_err();
        assert!(
            matches!(err, AutorunError::WorkerFailed(ref msg) if msg.contains("task-a")),
            "expected WorkerFailed for task-a, got: {err:?}"
        );
        assert!(
            killed.load(Ordering::SeqCst),
            "session should be cleaned up after invoke error"
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
}
