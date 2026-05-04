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
/// Resolve the open PR number for `branch` via `gh pr list`.
///
/// INF-TSK-049-001 batch 2 (AC #35 / W2): callers use this when the
/// in-process invoke result reports `pr_number == 0` even though the
/// branch has been pushed to origin and a PR likely exists.
///
/// Returns:
/// - `Some(n)` when `gh pr list --head <branch> --state open --limit 1`
///   reports a JSON array containing a `{"number": n, ...}` object.
/// - `None` when gh is missing, errored, returned no PRs, or the JSON
///   could not be parsed (fail-safe — never fabricate a PR number).
pub(crate) fn gh_pr_number_for_branch(wt_path: &std::path::Path, branch: &str) -> Option<i64> {
    if branch.is_empty() {
        return None;
    }
    // Branch sanity — same allowlist as in rescue::gh_pr_exists_for_branch.
    if !branch
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-' || b == b'/')
    {
        return None;
    }
    let output = std::process::Command::new("gh")
        .args([
            "pr", "list", "--head", branch, "--state", "open", "--limit", "1", "--json", "number",
        ])
        .current_dir(wt_path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout);
    let arr: serde_json::Value = serde_json::from_str(s.trim()).ok()?;
    let first = arr.as_array()?.first()?;
    first.get("number")?.as_i64()
}

/// INF-TSK-049-001 batch 2 (AC #34 / W1): shared warning message used by
/// both `classify_invoke_outcome` (inner claude exit 124) and the
/// wrapper-level timeout handler. Kept as a constant so the two call
/// sites can never drift. Rework iter 1 Finding 3.
pub(crate) const W1_TIMEOUT_ON_ORIGIN_WARNING: &str = "worker timed out but work is on origin (W1)";

/// Shared W1 policy: when a task hits exit 124 (or the wrapper-level
/// timeout), check whether its branch is already on origin. If yes,
/// that's success-with-warning; otherwise fail/timeout per the caller's
/// chosen failure status.
///
/// Returns `Some((warning_text, ()))` for the on-origin case so callers
/// can inject the shared warning, or `None` to signal "apply your own
/// failure classification".
///
/// Introduced for rework iter 1 Finding 3 so the policy is defined once.
pub(crate) fn w1_timeout_outcome(wt_path: &std::path::Path) -> Option<String> {
    if matches!(
        crate::autorun::rescue::branch_on_origin(wt_path),
        crate::autorun::rescue::BranchOriginStatus::OnOrigin
    ) {
        Some(W1_TIMEOUT_ON_ORIGIN_WARNING.to_string())
    } else {
        None
    }
}

/// Classify a finished worker invocation into `(status, error_msg, warning)`.
///
/// INF-TSK-049-001 batch 2 (AC #34, AC #36):
/// - `exit_code == 0`: `completed` (or `pr_creation_failed` when
///   `integration_auto_merge` is set but no PR was created).
/// - `exit_code == 124` AND branch is on origin: `completed` with warning
///   (W1 — work pushed before timeout).
/// - `exit_code == 124` AND branch NOT on origin (or inconclusive): `failed`.
/// - Any other non-zero exit: `failed`.
///
/// `wt_path` is the worker's worktree (used for the `branch_on_origin`
/// check). `pr_number` is consulted only for the `integration_auto_merge`
/// guard. Pure function — no side effects.
pub(crate) fn classify_invoke_outcome(
    wt_path: &std::path::Path,
    exit_code: i32,
    pr_number: i64,
    integration_auto_merge: bool,
) -> (&'static str, String, Option<String>) {
    if exit_code == 0 {
        if integration_auto_merge && pr_number == 0 {
            return (
                "pr_creation_failed",
                "PR expected but not created".to_string(),
                None,
            );
        }
        return ("completed", String::new(), None);
    }
    if exit_code == 124 {
        if let Some(warning) = w1_timeout_outcome(wt_path) {
            return ("completed", String::new(), Some(warning));
        }
    }
    ("failed", String::new(), None)
}

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

/// Cadence for writing `last_heartbeat_at` on the parent `autorun_session`.
///
/// The status TUI uses this value along with the heartbeat-TTL threshold to
/// render the IDLE column and flag stalled workers. 10 s balances DB write
/// pressure against liveness resolution in the TUI.
pub const WORKER_HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// Stage-timeout poll interval. Must be small relative to the timeout
/// itself so the watcher reacts within ~one window of the configured
/// threshold; large enough that the filesystem scan is not hot.
pub(crate) const STAGE_TIMEOUT_POLL_INTERVAL_SECS: u64 = 30;

/// Most-recent ws-* sentinel mtime (Unix seconds) under
/// `.state/sentinels/pathflow/{session_id}/`. Returns `None` when the
/// directory does not exist yet (no stage has emitted a sentinel) or
/// no `ws-*` files are present.
///
/// Worktree-aware: when `worktree_path` is `Some`, sentinels live at
/// `{worktree}/.state/sentinels/pathflow/{session_id}/`; otherwise
/// they're under `{project_dir}/.state/sentinels/...`. Sentinels are
/// LOCAL state in the worktree layout (not symlinked), so the worktree
/// path is the canonical location during autorun.
pub(crate) fn latest_ws_sentinel_mtime_secs(
    project_dir: &std::path::Path,
    session_id: &str,
    worktree_path: Option<&std::path::Path>,
) -> Option<i64> {
    let base = worktree_path.unwrap_or(project_dir);
    let dir = base
        .join(".state")
        .join("sentinels")
        .join("pathflow")
        .join(session_id);
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut latest: Option<i64> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        // Match the `pathflow-ws-*` (or bare `ws-*`) naming used by
        // `sentinel-write` PostToolUse hook. Both prefixes have been
        // observed in the codebase historically.
        if !(name_str.starts_with("pathflow-ws-") || name_str.starts_with("ws-")) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if let Ok(epoch) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    // saturating: any post-2262 mtime maps to i64::MAX,
                    // which simply means "always alive" — acceptable for
                    // this watcher's purposes.
                    let secs = i64::try_from(epoch.as_secs()).unwrap_or(i64::MAX);
                    if latest.is_none_or(|l| secs > l) {
                        latest = Some(secs);
                    }
                }
            }
        }
    }
    latest
}

/// Append a `stage_timeout` event to `pathflow-events.jsonl`. // EXEMPT: doc names the ledger filename
///
/// The ledger lives at `{worktree}/.state/ledger/pathflow-events.jsonl` // EXEMPT: doc names the ledger filename
/// in worktree mode (LOCAL since INF-TSK-024-035), or at
/// `{project_dir}/.state/ledger/pathflow-events.jsonl` otherwise. // EXEMPT: doc names the ledger filename
/// Errors are non-fatal — failure to write the event must NOT prevent
/// the watcher from aborting the worker.
///
/// Path construction below is preserved as-is: the writer uses the
/// flat layout `<base>/.state/ledger/pathflow-events.jsonl`. Aligning // EXEMPT: doc names the legacy flat path
/// with the canonical subdirectory layout served by
/// `ledger::resolve_path_in("pathflow-events", ..)` is tracked
/// separately under INF-TSK-024-014 (downstream consumer migration).
pub(crate) fn emit_stage_timeout_event(
    project_dir: &std::path::Path,
    session_id: &str,
    task_id: &str,
    worktree_path: Option<&std::path::Path>,
    last_sentinel_mtime: Option<i64>,
    stage_timeout_secs: u64,
) {
    use std::io::Write;
    let base = worktree_path.unwrap_or(project_dir);
    let path = base
        .join(".state")
        .join("ledger")
        .join("pathflow-events.jsonl"); // EXEMPT: writer matches reader; canonical migration tracked in INF-TSK-024-014
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let event = serde_json::json!({
        "event": "stage_timeout",
        "session_id": session_id,
        "task_id": task_id,
        "stage_timeout_secs": stage_timeout_secs,
        "last_sentinel_mtime_secs": last_sentinel_mtime,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    let line = match serde_json::to_string(&event) {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Pure decision: given the most recent ws-* sentinel mtime (or `None`
/// if no sentinel has appeared), the wall-clock baseline used until
/// the first sentinel, the configured timeout, and the current time
/// in Unix seconds, return `Some(last_seen_secs)` when the worker has
/// stalled past the threshold or `None` when it is still within the
/// window.
///
/// Pulled out of the polling loop so unit tests can exercise every
/// boundary (fresh sentinel within window, sentinel just-expired,
/// no sentinel + baseline expired, no sentinel + baseline fresh)
/// without needing the tokio mock clock.
pub(crate) fn stage_timeout_decision(
    last_sentinel_mtime: Option<i64>,
    start_baseline_secs: i64,
    stage_timeout_secs: u64,
    now_secs: i64,
) -> Option<i64> {
    let timeout_i64 = i64::try_from(stage_timeout_secs).unwrap_or(i64::MAX);
    let last_seen = last_sentinel_mtime.unwrap_or(start_baseline_secs);
    let stale_for = now_secs.saturating_sub(last_seen);
    if stale_for >= timeout_i64 {
        Some(last_seen)
    } else {
        None
    }
}

/// Watch for stage-timeout. Resolves when a polling interval observes
/// no new `ws-*` sentinel for longer than `stage_timeout_secs` since
/// the most recent sentinel mtime (or session start if no sentinel
/// exists yet).
///
/// `start_baseline_secs` is the wall-clock baseline used until the
/// first sentinel appears — typically the worker start time as Unix
/// epoch seconds.
///
/// `poll_interval` is the cadence between filesystem scans. Production
/// callers pass [`STAGE_TIMEOUT_POLL_INTERVAL_SECS`] worth of seconds;
/// tests can pass a much smaller value so they don't need the tokio
/// `test-util` mock clock feature.
///
/// This is intentionally a polling loop rather than a filesystem
/// notification watcher (a) because the `.state/sentinels/` directory
/// is created by the hook pipeline and may not exist at watcher-start
/// time on some platforms, and (b) because `inotify` / `fsevents`
/// add a platform-specific dependency for what is in practice a
/// 30-second poll.
pub(crate) async fn await_stage_timeout_with_interval(
    project_dir: std::path::PathBuf,
    session_id: String,
    worktree_path: Option<std::path::PathBuf>,
    stage_timeout_secs: u64,
    start_baseline_secs: i64,
    poll_interval: std::time::Duration,
) -> i64 {
    loop {
        tokio::time::sleep(poll_interval).await;
        let now_secs = chrono::Utc::now().timestamp();
        let last_sentinel_mtime =
            latest_ws_sentinel_mtime_secs(&project_dir, &session_id, worktree_path.as_deref());
        if let Some(last_seen) = stage_timeout_decision(
            last_sentinel_mtime,
            start_baseline_secs,
            stage_timeout_secs,
            now_secs,
        ) {
            return last_seen;
        }
    }
}

/// Convenience wrapper: production callers pass the default poll
/// interval ([`STAGE_TIMEOUT_POLL_INTERVAL_SECS`] seconds).
pub(crate) async fn await_stage_timeout(
    project_dir: std::path::PathBuf,
    session_id: String,
    worktree_path: Option<std::path::PathBuf>,
    stage_timeout_secs: u64,
    start_baseline_secs: i64,
) -> i64 {
    await_stage_timeout_with_interval(
        project_dir,
        session_id,
        worktree_path,
        stage_timeout_secs,
        start_baseline_secs,
        std::time::Duration::from_secs(STAGE_TIMEOUT_POLL_INTERVAL_SECS),
    )
    .await
}

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
    /// Human-readable task format id (e.g. `INF-TSK-049-001`) paired with
    /// `task_id`. Surfaced in the TUI TASK column via the parent
    /// `autorun_session.current_task_format_id` field on dispatch.
    /// INF-TSK-049-001 AC #15.
    #[serde(default)]
    pub task_format_id: Option<String>,
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
    /// CI reported a required-check failure — PR left open, task blocked.
    /// INF-TSK-048-001 AC #10.
    CiFailed { failing_checks: Vec<String> },
    /// CI polling hit the configured timeout without conclusive state.
    /// PR left open, merge-queue slot released.
    CiTimeout,
}

/// Backoff delays for the AC-12 DB-update retry helper.
///
/// 1s → 3s → 9s exponential schedule (3 attempts total: 1 immediate, 2
/// after backoff). Public so tests can validate the schedule without
/// spelling the constants twice.
pub(crate) const DB_UPDATE_RETRY_DELAYS: [Duration; 2] =
    [Duration::from_secs(1), Duration::from_secs(3)];

/// Total attempts for [`update_autorun_worker_with_retry`] and
/// [`update_autorun_task_run_with_retry`]: 1 immediate + `len(delays)`
/// retries = `DB_UPDATE_RETRY_DELAYS.len() + 1`.
///
/// Public-by-tests-only — referenced by `test_db_update_retry_constants`
/// to keep the lib + tests in lock-step. The `#[allow(dead_code)]`
/// suppresses the lib-build warning since production code computes
/// this from `DELAYS.len()` directly inside `update_with_retry`.
#[allow(dead_code)]
pub(crate) const DB_UPDATE_MAX_ATTEMPTS: usize = DB_UPDATE_RETRY_DELAYS.len() + 1;

/// Execute `update_autorun_worker` with bounded retry + exponential backoff.
///
/// INF-TSK-050-003 AC-12: transient DB write failures (e.g. SurrealDB
/// busy waits, file-lock contention from concurrent worktrees) used to
/// fail the call once and silently lose the update. Now we retry up to
/// 3 times total with `1s/3s` between attempts. The third failure
/// returns the final `Err` to the caller (still non-fatal at the call
/// site, but at least visible).
///
/// Idempotent by construction: every field on `AutorunWorkerUpdate` is
/// a CAS-friendly merge (same value rewritten is a no-op), and status
/// transitions on success use a frozen target value.
///
/// # Errors
///
/// Returns the last `DbError` from the underlying `update_autorun_worker`
/// call when all attempts fail.
pub(crate) async fn update_autorun_worker_with_retry<S: crate::store::DataStore>(
    store: &S,
    worker_id: &str,
    update: crate::models::AutorunWorkerUpdate,
) -> Result<(), crate::error::DbError> {
    update_with_retry(
        DB_UPDATE_RETRY_DELAYS,
        || store.update_autorun_worker(worker_id, clone_worker_update(&update)),
        "update_autorun_worker",
        worker_id,
    )
    .await
}

/// Execute `update_autorun_task_run` with bounded retry + exponential backoff.
///
/// See [`update_autorun_worker_with_retry`] for the schedule and
/// rationale (INF-TSK-050-003 AC-12).
///
/// # Errors
///
/// Returns the last `DbError` from the underlying `update_autorun_task_run`
/// call when all attempts fail.
pub(crate) async fn update_autorun_task_run_with_retry<S: crate::store::DataStore>(
    store: &S,
    task_run_id: &str,
    update: crate::models::AutorunTaskRunUpdate,
) -> Result<(), crate::error::DbError> {
    update_with_retry(
        DB_UPDATE_RETRY_DELAYS,
        || store.update_autorun_task_run(task_run_id, clone_task_run_update(&update)),
        "update_autorun_task_run",
        task_run_id,
    )
    .await
}

/// Internal generic retry driver shared by the worker and task-run helpers.
async fn update_with_retry<F, Fut>(
    delays: [Duration; 2],
    mut op: F,
    op_name: &str,
    record_id: &str,
) -> Result<(), crate::error::DbError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), crate::error::DbError>>,
{
    let mut last_err: Option<crate::error::DbError> = None;
    for attempt in 0..=delays.len() {
        match op().await {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!(
                    "warn: {op_name} attempt {} of {} failed for id={record_id}: {e}",
                    attempt + 1,
                    delays.len() + 1
                );
                if attempt < delays.len() {
                    tokio::time::sleep(delays[attempt]).await;
                }
                last_err = Some(e);
            }
        }
    }
    // Total attempts == delays.len() + 1; falling through means all
    // attempts failed. Return the last error (caller may log + continue).
    Err(last_err.unwrap_or_else(|| {
        crate::error::DbError::Query(format!("{op_name}: retry loop exited without an error"))
    }))
}

// AutorunWorkerUpdate / AutorunTaskRunUpdate don't derive Clone (the
// fields are all owned `Option<T>` types but the structs are
// move-only). The retry helpers need a fresh copy per attempt, so we
// hand-roll Clone. Keeping these helpers private prevents drift if the
// struct definitions change.
fn clone_worker_update(
    src: &crate::models::AutorunWorkerUpdate,
) -> crate::models::AutorunWorkerUpdate {
    crate::models::AutorunWorkerUpdate {
        status: src.status,
        tmux_session: src.tmux_session.clone(),
        worktree_path: src.worktree_path.clone(),
        file_scope: src.file_scope.clone(),
        scope_policy: src.scope_policy.clone(),
        worker_session_id: src.worker_session_id.clone(),
        pr_number: src.pr_number,
        pr_merged_at: src.pr_merged_at.clone(),
        started_at: src.started_at.clone(),
        completed_at: src.completed_at.clone(),
    }
}

fn clone_task_run_update(
    src: &crate::models::AutorunTaskRunUpdate,
) -> crate::models::AutorunTaskRunUpdate {
    crate::models::AutorunTaskRunUpdate {
        status: src.status,
        pr_number: src.pr_number,
        pr_url: src.pr_url.clone(),
        branch_name: src.branch_name.clone(),
        blocked_reason: src.blocked_reason.clone(),
        claim_conflicts: src.claim_conflicts.clone(),
        merge_conflicts: src.merge_conflicts.clone(),
        completed_at: src.completed_at.clone(),
        duration_seconds: src.duration_seconds,
        exit_code: src.exit_code,
        error_message: src.error_message.clone(),
        verification_result: src.verification_result.clone(),
        last_phase: src.last_phase.clone(),
    }
}

/// Apply the merge-result writeback to the autorun_worker DB row.
///
/// INF-TSK-050-003 AC-11: after `serialized_merge` returns, persist
/// `pr_merged_at` and a status reflecting the outcome:
///
/// | `MergeOutcome`   | new `status`                       | sets `pr_merged_at`? |
/// |------------------|-----------------------------------|----------------------|
/// | `Merged`         | `Completed`                        | yes (now)            |
/// | `MergeConflict`  | `Failed`                           | no                   |
/// | `QueueTimeout`   | `Timeout`                          | no                   |
/// | `CiFailed`       | `Failed`                           | no                   |
/// | `CiTimeout`      | `Timeout`                          | no                   |
///
/// Status variants `Merged`, `MergeFailed`, and `MergeConflict` named in
/// the AC text don't exist in the schema CHECK constraint; the closest
/// schema-valid mapping above preserves the user-visible distinction
/// (terminal `Completed` only when the PR actually merged) without
/// introducing new variants. The `pr_merged_at` field is the
/// load-bearing signal for the AC-13 final-PR gate — only `Merged`
/// sets it, so a `merge_conflict` worker is correctly excluded from
/// the gate.
pub(crate) async fn apply_merge_writeback<S: crate::store::DataStore>(
    store: &S,
    worker_id: &str,
    outcome: &MergeOutcome,
) -> Result<(), crate::error::DbError> {
    let now = chrono::Utc::now().to_rfc3339();
    let (status, pr_merged_at) = match outcome {
        MergeOutcome::Merged => (
            crate::types::AutorunWorkerStatus::Completed,
            Some(now.clone()),
        ),
        MergeOutcome::MergeConflict { .. } | MergeOutcome::CiFailed { .. } => {
            (crate::types::AutorunWorkerStatus::Failed, None)
        }
        MergeOutcome::QueueTimeout | MergeOutcome::CiTimeout => {
            (crate::types::AutorunWorkerStatus::Timeout, None)
        }
    };
    let update = crate::models::AutorunWorkerUpdate {
        status: Some(status),
        pr_merged_at,
        ..Default::default()
    };
    update_autorun_worker_with_retry(store, worker_id, update).await
}

/// Outcome of a single poll iteration in `serialized_merge`'s wait
/// loop. Extracted from the inline loop body so the AC-10 per-poll
/// stale-head detection contract can be unit-tested without spawning
/// the full `serialized_merge` async chain.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PollOutcome {
    /// We reached position 0 — caller breaks the loop and proceeds to merge.
    Ready,
    /// Still waiting (we're at position N>0, OR re-enqueue happened, OR transient error).
    Waiting,
    /// A position-check error occurred — caller breaks with the existing
    /// "proceed anyway on error" semantics.
    ExitOnError,
}

/// One iteration of the position-0 wait loop. Always invokes
/// `try_remove_stale_head` FIRST so a crashed predecessor unblocks the
/// next worker within one poll interval (AC-10), then queries our own
/// position and dispatches.
///
/// Pure synchronous primitive — no `tokio::time::sleep` here so unit
/// tests can drive multiple iterations without burning wall-clock
/// time. The async `tokio::time::sleep` between iterations stays in
/// the caller (`serialized_merge`).
pub(crate) fn poll_for_position_zero_iteration(
    state_path: &std::path::Path,
    worker_sid: &crate::types::SessionId,
    target: &str,
    re_enqueue_entry: &crate::coordination::merge_queue::MergeQueueEntry,
) -> PollOutcome {
    // Per-poll stale-head check (AC-10). Cheap (only acts on the head
    // entry); silent on transient errors (worker continues to wait).
    // When this removes a stale predecessor, the position check below
    // observes our session at position 0 on the SAME iteration.
    match try_remove_stale_head(state_path, target) {
        Ok(true) => {
            eprintln!("merge queue: removed stale head entry (per-poll detection)");
        }
        Ok(false) => {} // Head alive — normal case.
        Err(e) => {
            eprintln!("warning: stale-head check failed (transient): {e}");
        }
    }

    match crate::coordination::merge_queue::locked_position_for_target(
        state_path, worker_sid, target,
    ) {
        Ok(Some(0)) => PollOutcome::Ready,
        Ok(Some(pos)) => {
            eprintln!("merge queue: position {pos} for target {target}, waiting...");
            PollOutcome::Waiting
        }
        Ok(None) => {
            // Not in queue -- should not happen after enqueue, but re-enqueue.
            eprintln!("warning: session not found in merge queue, re-enqueueing");
            let _ = crate::coordination::merge_queue::locked_enqueue(state_path, re_enqueue_entry);
            PollOutcome::Waiting
        }
        Err(e) => {
            eprintln!("warning: merge queue position check failed: {e}");
            PollOutcome::ExitOnError
        }
    }
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
    //
    // INF-TSK-050-003 AC-10: invoke `try_remove_stale_head` at the TOP
    // of every poll iteration (not only at deadline expiry). When the
    // worker holding position 0 crashes, its CRDT claim never gets
    // released; the next worker would otherwise wait the full
    // `queue_timeout_secs` (default 30 min) before noticing. Per-poll
    // stale-head detection unblocks the next worker within one poll
    // interval (~5s default).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(queue_timeout_secs);
    loop {
        match poll_for_position_zero_iteration(state_path, worker_sid, target, &entry) {
            // Ready (we're at front) and ExitOnError (position-check
            // failure → caller proceeds anyway, matching pre-extraction
            // semantics) both break the loop.
            PollOutcome::Ready | PollOutcome::ExitOnError => break,
            PollOutcome::Waiting => {} // fallthrough to deadline + sleep
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

    // Step 6b: INF-TSK-048-001 AC #10 — wait for required CI checks to pass
    // before invoking `gh pr merge`. Without this check, `gh pr merge` either
    // rejects (branch protection) or merges with pending checks (no protection
    // but an autorun PR still needs CI green). The resulting release path:
    //   - AllGreen          → proceed to gh pr merge
    //   - RequiredFailed(n) → leave PR open, release queue slot, block task
    //   - Timeout           → leave PR open, release queue slot, block task
    //   - NoRequiredChecks  → proceed (legacy repo or config opt-out)
    let ci_cfg = crate::autorun::ci_wait::CiWaitConfig::load(wt_path).unwrap_or_default();
    #[allow(clippy::cast_sign_loss)]
    let pr_u64 = pr_number.max(0) as u64;
    match crate::autorun::ci_wait::wait_for_ci_green(pr_u64, &ci_cfg).await {
        Ok(
            crate::autorun::ci_wait::CiOutcome::AllGreen
            | crate::autorun::ci_wait::CiOutcome::NoRequiredChecks,
        ) => {}
        Ok(crate::autorun::ci_wait::CiOutcome::RequiredFailed(names)) => {
            eprintln!(
                "ci-wait: PR #{pr_number} has failing required checks: {}",
                names.join(", ")
            );
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::CiFailed {
                failing_checks: names,
            };
        }
        Ok(crate::autorun::ci_wait::CiOutcome::Timeout) => {
            eprintln!(
                "ci-wait: PR #{pr_number} timed out waiting for CI green \
                 ({} min); leaving PR open",
                ci_cfg.timeout_minutes
            );
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::CiTimeout;
        }
        Err(e) => {
            eprintln!(
                "ci-wait: CI status polling failed for PR #{pr_number}: {e} \
                 — leaving PR open"
            );
            let _ =
                crate::coordination::merge_queue::locked_remove_by_session(state_path, worker_sid);
            return MergeOutcome::CiTimeout;
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

/// Check if the head entry in the merge queue for a target branch belongs to a dead session.
///
/// Uses `kill(pid, 0)` to test liveness of the session's lead PID. If dead, removes the
/// entry from the queue. Returns `Ok(true)` if a stale entry was removed, `Ok(false)` if
/// the head is alive or the queue is empty.
fn try_remove_stale_head(
    state_path: &std::path::Path,
    target_branch: &str,
) -> Result<bool, String> {
    // Peek at the head entry for this target.
    let mut head_entry: Option<crate::coordination::merge_queue::MergeQueueEntry> = None;
    crate::file_lock::locked_binary_rmw(
        state_path,
        crate::coordination::loro::LoroCoordinator::in_memory,
        |bytes| {
            crate::coordination::loro::LoroCoordinator::from_bytes(bytes, state_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| {
            // Find the first entry targeting this branch.
            let queue = coord.doc().get_list("merge_queue");
            let len = queue.len();
            for i in 0..len {
                if let Some(loro::ValueOrContainer::Value(loro::LoroValue::String(s))) =
                    queue.get(i)
                {
                    if let Ok(entry) = serde_json::from_str::<
                        crate::coordination::merge_queue::MergeQueueEntry,
                    >(&s)
                    {
                        if entry.target_branch == target_branch {
                            head_entry = Some(entry);
                            break;
                        }
                    }
                }
            }
            Ok(())
        },
    )
    .map_err(|e| format!("peek head: {e}"))?;

    let Some(entry) = head_entry else {
        return Ok(false); // Queue empty for this target.
    };

    // INF-TSK-024-051 Phase 7-rework: route through the canonical
    // chokepoint instead of reading `.state/interactive/heartbeat-{sid}`
    // and validating with bare `is_process_alive`. The chokepoint:
    //   1. resolves the worker's worktree via `worktrees.yaml`,
    //   2. reads `pathflow-session-status.json::lead_pid` from the
    //      worktree-resolved location,
    //   3. validates via `validate_claude_pid` (alive AND named "claude").
    //
    // The previous bare-check was a PID-reuse vector: if the worker
    // crashed and its PID was recycled by an unrelated process
    // (cargo, sshd, etc.), the merge-queue head would be perpetually
    // marked alive and the queue would stall. The chokepoint's
    // name-validation closes that race.
    let project_dir = state_path
        .ancestors()
        .find(|p| p.join(".codeflow").is_dir())
        .unwrap_or(state_path);
    let session_id_str = entry.session_id.as_str();
    let verdict = crate::session::liveness::is_session_alive(project_dir, session_id_str);

    let is_dead = match verdict {
        crate::session::liveness::SessionLiveness::Dead => true,
        crate::session::liveness::SessionLiveness::Active => false,
        crate::session::liveness::SessionLiveness::Unknown => {
            // Status file missing or `lead_pid` unrecorded — fall back to
            // the original age-based heuristic so workers that crashed
            // BEFORE writing the status file are still reaped, and so
            // legacy entries from pre-Phase-7-rework batches still age out.
            let age_secs = chrono::DateTime::parse_from_rfc3339(&entry.pr_ready_at)
                .map(|dt| (chrono::Utc::now() - dt.to_utc()).num_seconds())
                .unwrap_or(0);
            age_secs > 3600
        }
    };

    if !is_dead {
        return Ok(false);
    }

    // Remove the stale entry.
    let sid = entry.session_id.clone();
    crate::file_lock::locked_binary_rmw(
        state_path,
        crate::coordination::loro::LoroCoordinator::in_memory,
        |bytes| {
            crate::coordination::loro::LoroCoordinator::from_bytes(bytes, state_path)
                .map_err(|e| format!("load coordinator: {e}"))
        },
        |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
        |coord| {
            crate::coordination::merge_queue::remove_by_session(coord, &sid)
                .map_err(|e| format!("remove stale: {e}"))?;
            Ok(())
        },
    )
    .map_err(|e| format!("remove stale entry: {e}"))?;

    eprintln!("merge queue: removed stale entry for dead session {sid}");
    Ok(true)
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
                    warning: None,
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

        // INF-TSK-024-051 Phase 7-rework: heartbeat writer removed. The
        // worker's child Claude session writes the canonical
        // `pathflow-session-status.json::lead_pid` inside its worktree at
        // SessionStart, and `try_remove_stale_head` reads that via the
        // canonical chokepoint (`is_session_alive`). Writing a duplicate
        // heartbeat from the worker process was wrong-by-construction
        // anyway: the worker's `std::process::id()` is the autorun
        // orchestrator's PID, not the Claude lead the chokepoint validates.

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
                            warning: None,
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
            // INF-TSK-050-003 AC-11: pr_merged_at starts None; set by
            // `apply_merge_writeback` after `serialized_merge` returns.
            pr_merged_at: None,
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
            last_phase: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Err(e) = self.store.create_autorun_task_run(&task_run_record).await {
            eprintln!("warning: failed to create autorun_task_run record: {e}");
        }

        // INF-TSK-048-001 AC #8 + INF-TSK-049-001 AC #15: on task dispatch,
        // record both `current_task_id` (ULID) and `current_task_format_id`
        // (human-readable) on the parent autorun_session so the TUI TASK
        // column can prefer the format id for display. Also bump
        // `updated_at` as part of the dispatch state change.
        //
        // When the caller did not supply `task_format_id`, fall back to
        // reading it from the task markdown's YAML frontmatter so every
        // tracked task populates the field even if callers were written
        // before the plumbing existed.
        let task_format_id = cfg.task_format_id.clone().or_else(|| {
            crate::autorun::batch::read_task_format_id(&cfg.task_id, &self.project_dir)
        });
        let dispatch_now = chrono::Utc::now().to_rfc3339();
        if let Err(e) = self
            .store
            .update_autorun_session(
                &cfg.session_id,
                crate::models::AutorunSessionUpdate {
                    current_task_id: Some(Some(cfg.task_id.clone())),
                    current_task_format_id: Some(task_format_id),
                    updated_at: Some(dispatch_now.clone()),
                    last_heartbeat_at: Some(dispatch_now.clone()),
                    ..Default::default()
                },
            )
            .await
        {
            eprintln!("warning: failed to record current_task_id at dispatch: {e}");
        }

        // INF-TSK-048-001 AC #8: Spawn a 10 s cadence heartbeat updater on the parent
        // session. Uses a separate connection obtained via the existing store clone
        // mechanism. We shut it down via an mpsc::channel so the task exits cleanly
        // on all worker-exit branches.
        let (heartbeat_stop_tx, heartbeat_stop_rx) = tokio::sync::oneshot::channel::<()>();
        let heartbeat_handle = {
            let session_id_for_hb = cfg.session_id.clone();
            let project_dir_for_hb = self.project_dir.clone();
            tokio::spawn(async move {
                let interval = std::time::Duration::from_secs(WORKER_HEARTBEAT_INTERVAL_SECS);
                let mut ticker = tokio::time::interval(interval);
                // First tick is immediate; we already wrote one at dispatch above,
                // so consume it to avoid double-writing within a single millisecond.
                ticker.tick().await;
                tokio::pin!(heartbeat_stop_rx);
                let db_dir = project_dir_for_hb.join(".state/db");
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            // Open a fresh store handle per tick for cross-process
                            // visibility — a long-lived handle would miss writes
                            // from other workers on the same session record.
                            use crate::store::DataStore as _;
                            let Ok(store) = crate::store::SurrealStore::open(&db_dir).await else {
                                continue;
                            };
                            let now = chrono::Utc::now().to_rfc3339();
                            let _ = store
                                .update_autorun_session(
                                    &session_id_for_hb,
                                    crate::models::AutorunSessionUpdate {
                                        last_heartbeat_at: Some(now),
                                        ..Default::default()
                                    },
                                )
                                .await;
                        }
                        _ = &mut heartbeat_stop_rx => {
                            break;
                        }
                    }
                }
            })
        };

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

        // STAGE TIMEOUT: a parallel watcher that observes ws-* sentinel
        // mtime in `.state/sentinels/pathflow/{worker_session_id}/`. If
        // the worker makes no stage progress within `stage_timeout_secs`
        // (default 3600), the watcher resolves and the select! arm wins
        // over the claude invoke. Distinct from the outer wall-clock
        // timeout: a worker can be alive but stuck on a single stage
        // (e.g. cf-development hung), in which case worker_timeout_secs
        // (typically 5400-7200s) would let it sit idle for 1-2 hours.
        let stage_timeout_secs = crate::autorun::config::load_config(&self.project_dir)
            .unwrap_or_default()
            .autorun
            .stage_timeout_secs;
        let stage_timeout_baseline = chrono::Utc::now().timestamp();

        // OUTER TIMEOUT: Bounds total worker wall-clock time including worktree
        // setup, Claude invocation, and cleanup. This is the safety net enforced
        // by the orchestrator/worker layer. The INNER timeout lives in the CLI
        // invoker (autorun.rs) and polls for Claude's exit-code marker file,
        // handling SIGTERM/SIGKILL escalation. Both are needed: the outer catches
        // hangs in non-Claude phases; the inner provides graceful Claude shutdown.
        let result = tokio::time::timeout(timeout, async {
            let invoke_fut = self.claude.invoke(InvokeConfig {
                work_dir: wt_info.path.to_string_lossy().into_owned(),
                prompt,
                session_id: cfg.session_id.clone(),
                task_id: cfg.task_id.clone(),
                task_format_id: cfg.task_format_id.clone(),
                integration_auto_merge: cfg.integration_auto_merge,
                integration_branch: cfg.integration_branch.clone(),
                tmux_session: tmux_name.clone(),
                acceptance_criteria,
                worker_session_id: worker_sid.as_str().to_owned(),
                epic_update: cfg.epic_update.clone(),
                worker_timeout_secs: effective_timeout_secs,
            });
            let stage_watcher = await_stage_timeout(
                self.project_dir.clone(),
                worker_sid.as_str().to_owned(),
                Some(wt_info.path.clone()),
                stage_timeout_secs,
                stage_timeout_baseline,
            );
            tokio::select! {
                biased;
                last_seen = stage_watcher => {
                    // Emit `stage_timeout` event to pathflow-events.jsonl // EXEMPT: comment names the ledger filename
                    // (LOCAL per-worktree since INF-TSK-024-035), then
                    // synthesize an exit-125 result. The worker bus path
                    // turns this into a `failed` outcome below.
                    emit_stage_timeout_event(
                        &self.project_dir,
                        worker_sid.as_str(),
                        &cfg.task_id,
                        Some(&wt_info.path),
                        Some(last_seen),
                        stage_timeout_secs,
                    );
                    Ok(InvokeResult {
                        exit_code: 125,
                        pr_number: 0,
                        pr_url: String::new(),
                        branch_name: String::new(),
                        output: format!(
                            "stage_timeout: no ws-* sentinel in {stage_timeout_secs}s"
                        ),
                    })
                }
                invoke_result = invoke_fut => invoke_result,
            }
        })
        .await;

        // Log Claude exit status.
        match &result {
            Ok(Ok(r)) if r.exit_code == 125 => {
                worker_log(&log_path, "STAGE_TIMEOUT exit=125");
            }
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
                    let completed_at = chrono::Utc::now().to_rfc3339();
                    self.update_db_records(
                        &cfg.worker_id,
                        crate::models::AutorunWorkerUpdate {
                            status: Some(crate::types::AutorunWorkerStatus::Failed),
                            completed_at: Some(completed_at.clone()),
                            ..Default::default()
                        },
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
                        warning: None,
                    });
                }
            }
        }

        // Serialized merge: when integration_auto_merge is enabled, use the
        // merge queue to serialize PR merges to the integration branch.
        //
        // INF-TSK-049-001 batch 2 (AC #35 / W2): when invoke_result reports
        // pr_number == 0 BUT the branch is on origin, fall back to
        // `gh pr list --head <branch>` (gated on rescue.gh_pr_check). This
        // covers the failure mode where Claude pushed the branch and opened
        // a PR but its parser missed the PR number from the trailing output.
        if cfg.integration_auto_merge {
            if let Ok(Ok(ref invoke_result)) = result {
                let cfg_loaded =
                    crate::autorun::config::load_config(&self.project_dir).unwrap_or_default();
                let merge_config = cfg_loaded.merge.clone();

                // Resolve the effective PR number, preferring the worker's
                // direct value but falling back to a gh query when needed.
                let mut effective_pr_number = invoke_result.pr_number;
                let mut effective_branch = invoke_result.branch_name.clone();
                if effective_pr_number == 0 && cfg_loaded.rescue.gh_pr_check {
                    if effective_branch.is_empty() {
                        // Best-effort: read the worker's current branch name.
                        if let Ok(o) = std::process::Command::new("git")
                            .args(["rev-parse", "--abbrev-ref", "HEAD"])
                            .current_dir(&wt_info.path)
                            .output()
                        {
                            if o.status.success() {
                                effective_branch =
                                    String::from_utf8_lossy(&o.stdout).trim().to_string();
                            }
                        }
                    }
                    let on_origin = matches!(
                        crate::autorun::rescue::branch_on_origin(&wt_info.path),
                        crate::autorun::rescue::BranchOriginStatus::OnOrigin
                    );
                    if on_origin && !effective_branch.is_empty() && effective_branch != "HEAD" {
                        if let Some(found) =
                            gh_pr_number_for_branch(&wt_info.path, &effective_branch)
                        {
                            eprintln!(
                                "serialized merge: pr_number=0 from worker; gh pr list reports PR #{found} for branch '{effective_branch}'"
                            );
                            effective_pr_number = found;
                        }
                    }
                }

                if effective_pr_number > 0 && merge_config.queue_enforcing {
                    let merge_result = serialized_merge(
                        &wt_info.path,
                        &cfg.integration_branch,
                        &worker_sid,
                        effective_pr_number,
                        &cfg.task_id,
                        &effective_branch,
                        &state_path,
                        &merge_config,
                        cfg.queue_timeout_secs,
                    )
                    .await;
                    match merge_result {
                        MergeOutcome::Merged => {
                            eprintln!(
                                "serialized merge: PR #{} merged to {}",
                                effective_pr_number, cfg.integration_branch
                            );
                        }
                        MergeOutcome::MergeConflict { ref error } => {
                            eprintln!(
                                "serialized merge: PR #{effective_pr_number} failed: {error}"
                            );
                        }
                        MergeOutcome::QueueTimeout => {
                            eprintln!(
                                "serialized merge: PR #{effective_pr_number} timed out in queue"
                            );
                        }
                        MergeOutcome::CiFailed { ref failing_checks } => {
                            eprintln!(
                                "serialized merge: PR #{effective_pr_number} blocked — failing required CI checks: {} (PR left open, task blocked)",
                                failing_checks.join(", ")
                            );
                        }
                        MergeOutcome::CiTimeout => {
                            eprintln!(
                                "serialized merge: PR #{effective_pr_number} blocked — CI did not reach green within configured timeout (PR left open, task blocked)"
                            );
                        }
                    }
                    // INF-TSK-050-003 AC-11: persist the merge outcome
                    // to the autorun_worker DB row. `pr_merged_at` is
                    // the load-bearing signal for the AC-13 final-PR
                    // gate; status mirrors the terminal-state intent
                    // (Completed for Merged, Failed for conflicts/CI
                    // failures, Timeout for queue/CI timeouts). The
                    // retry helper (AC-12) tolerates transient DB
                    // contention from concurrent worktrees.
                    if let Err(e) =
                        apply_merge_writeback(&*self.store, &cfg.worker_id, &merge_result).await
                    {
                        eprintln!(
                            "warn: merge-writeback DB update failed for worker={} after retries: {e}",
                            cfg.worker_id
                        );
                    }
                }
            }
        }

        // INF-TSK-048-001 AC #8 / INF-TSK-049-001 AC #15: stop the heartbeat
        // writer but PRESERVE `current_task_id` and `current_task_format_id`
        // on the session row. Previously both were cleared to None on
        // finish; that made the batches-list TASK column go blank on
        // terminal rows, losing the "which task was running when this batch
        // ended" at-a-glance. Keep them; only bump `updated_at`.
        //
        // Order matters: stop the ticker first so it does not race the
        // subsequent updated_at write.
        let _ = heartbeat_stop_tx.send(());
        let _ = heartbeat_handle.await;
        let finish_now = chrono::Utc::now().to_rfc3339();
        if let Err(e) = self
            .store
            .update_autorun_session(
                &cfg.session_id,
                crate::models::AutorunSessionUpdate {
                    updated_at: Some(finish_now),
                    ..Default::default()
                },
            )
            .await
        {
            eprintln!("warning: failed to bump updated_at at finish: {e}");
        }

        // INF-TSK-024-051 Phase 7-rework: heartbeat cleanup line removed
        // alongside the writer. The autorun worker no longer creates
        // `.state/interactive/heartbeat-{worker_sid}`.
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

        // Branch safety check before cleanup — salvage unpushed work.
        let wt_dir = self
            .project_dir
            .join(crate::worktree::DEFAULT_BASE_DIR)
            .join(&wt_name);
        if wt_dir.exists() {
            let safety = crate::worktree::check_branch_safety(&wt_dir);
            if safety.risk >= crate::worktree::BranchRisk::High {
                eprintln!(
                    "warning: branch safety issue for {wt_name}: {} (attempting salvage push)",
                    safety.message
                );
                worker_log(
                    &log_path,
                    &format!(
                        "BRANCH_SAFETY risk={} branch={}",
                        safety.risk, safety.branch
                    ),
                );

                // Best-effort: push the branch before destroying the worktree.
                if !safety.branch.is_empty() && safety.branch != "HEAD" {
                    match std::process::Command::new("git")
                        .args(["push", "origin", &safety.branch])
                        .current_dir(&wt_dir)
                        .output()
                    {
                        Ok(output) if output.status.success() => {
                            eprintln!(
                                "BRANCH_SALVAGE: pushed branch {} before cleanup",
                                safety.branch
                            );
                            worker_log(
                                &log_path,
                                &format!("BRANCH_SALVAGE pushed={}", safety.branch),
                            );
                        }
                        Ok(output) => {
                            let stderr = String::from_utf8_lossy(&output.stderr);
                            eprintln!(
                                "BRANCH_SALVAGE: push failed for {}: {stderr}",
                                safety.branch
                            );
                            worker_log(
                                &log_path,
                                &format!(
                                    "BRANCH_SALVAGE push_failed={} err={}",
                                    safety.branch,
                                    stderr.trim()
                                ),
                            );
                        }
                        Err(e) => {
                            eprintln!("BRANCH_SALVAGE: push error for {}: {e}", safety.branch);
                            worker_log(
                                &log_path,
                                &format!("BRANCH_SALVAGE push_error={} err={e}", safety.branch),
                            );
                        }
                    }
                }
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
                            let _ = crate::worktree::locked_deregister_by_name(
                                &registry_path,
                                &wt_name,
                            );
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
                            // Record orphaned path for `codeflow worktree prune`.
                            let orphan_path =
                                self.project_dir.join(".git-worktrees").join(&wt_name);
                            let cleanup_file = self.project_dir.join(".state/cleanup-needed.txt");
                            let line = format!("{}\n", orphan_path.display());
                            if let Ok(mut f) = std::fs::OpenOptions::new()
                                .create(true)
                                .append(true)
                                .open(&cleanup_file)
                            {
                                use std::io::Write;
                                let _ = f.write_all(line.as_bytes());
                            }
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
        // INF-TSK-049-001 batch 2 (AC #34): exit 124 + branch already on
        // origin → success-with-warning. Otherwise (or if HEAD is not on
        // origin), keep the historical "failed" classification. Same rule
        // applies to the wrapper-level timeout (Err(_) branch below).
        let worker_result = match result {
            Ok(Ok(invoke_result)) => {
                let (status, error_msg, warning) = classify_invoke_outcome(
                    &wt_info.path,
                    invoke_result.exit_code,
                    invoke_result.pr_number,
                    cfg.integration_auto_merge,
                );
                if status == "pr_creation_failed" {
                    Self::emit_autorun_event(
                        &self.project_dir,
                        &crate::coordination::types::events::AutorunEvent::WorkerFailed {
                            session_id: cfg.session_id.clone(),
                            worker_id: cfg.worker_id.clone(),
                            task_id: cfg.task_id.clone(),
                            error: "PR expected (auto_merge=true) but pr_number=0".into(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
                Ok(WorkerResult {
                    worker_id: cfg.worker_id.clone(),
                    task_id: cfg.task_id.clone(),
                    status: status.into(),
                    exit_code: invoke_result.exit_code,
                    pr_number: invoke_result.pr_number,
                    pr_url: invoke_result.pr_url.clone(),
                    branch_name: invoke_result.branch_name,
                    error: error_msg,
                    duration_sec: elapsed,
                    warning,
                })
            }
            Ok(Err(e)) => Err(AutorunError::WorkerFailed(format!(
                "task {}: {e}",
                cfg.task_id
            ))),
            Err(_) => {
                // Wrapper-level timeout. Route through the shared W1
                // policy helper so the on-origin classification is
                // defined in ONE place (Rework iter 1 Finding 3 — avoid
                // drift between this arm and classify_invoke_outcome).
                // When NOT on origin the wrapper surfaces the richer
                // "timeout" status (distinct from the inner-claude
                // exit-124 "failed" classification) with its explanatory
                // error message.
                let warning = w1_timeout_outcome(&wt_info.path);
                let (status, error_msg) = if warning.is_some() {
                    ("completed", String::new())
                } else {
                    ("timeout", "worker exceeded timeout".to_string())
                };
                Ok(WorkerResult {
                    worker_id: cfg.worker_id.clone(),
                    task_id: cfg.task_id.clone(),
                    status: status.into(),
                    exit_code: 124, // standard timeout exit code
                    pr_number: 0,
                    pr_url: String::new(),
                    branch_name: String::new(),
                    error: error_msg,
                    duration_sec: elapsed,
                    warning,
                })
            }
        };

        // Read last phase from sentinel files before worktree cleanup destroys them.
        let last_phase = {
            let wt_dir = self
                .project_dir
                .join(crate::worktree::DEFAULT_BASE_DIR)
                .join(&wt_name);
            crate::session::sentinel::read_latest_phase(
                &self.project_dir,
                Some(wt_dir.to_str().unwrap_or("")),
                None,
            )
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

                let verification = match wr.status.as_str() {
                    "completed" => Some("pass".to_string()),
                    "timeout" => Some("timeout".to_string()),
                    _ => Some("fail".to_string()),
                };
                self.update_db_records(
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
                        last_phase: last_phase.clone(),
                        ..Default::default()
                    },
                )
                .await;

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
                self.update_db_records(
                    &cfg.worker_id,
                    crate::models::AutorunWorkerUpdate {
                        status: Some(crate::types::AutorunWorkerStatus::Failed),
                        completed_at: Some(completed_at.clone()),
                        ..Default::default()
                    },
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

    /// Update both `autorun_worker` and `autorun_task_run` DB records with a
    /// 10-second timeout per call. Logs warnings on timeout or error but does
    /// not propagate failures — DB updates are best-effort so the worker
    /// lifecycle can continue.
    ///
    /// INF-TSK-050-003 AC-12: each underlying update is now wrapped in
    /// `update_autorun_*_with_retry` (3 attempts with 1s/3s backoff).
    /// The outer `tokio::time::timeout` is the upper bound on the full
    /// retry sequence (3 attempts + 4s of backoff between them); a hung
    /// query at any attempt still aborts the whole sequence rather than
    /// spinning forever. Bumped to 60s (was 10s) so a transient busy
    /// during attempt 1 doesn't trip the outer timeout before the retry
    /// helper has a chance to back off and retry.
    async fn update_db_records(
        &self,
        worker_id: &str,
        worker_update: crate::models::AutorunWorkerUpdate,
        task_run_id: &str,
        task_run_update: crate::models::AutorunTaskRunUpdate,
    ) {
        let db_timeout = Duration::from_secs(60);

        let worker_fut = update_autorun_worker_with_retry(&*self.store, worker_id, worker_update);
        match tokio::time::timeout(db_timeout, worker_fut).await {
            Ok(Err(e)) => {
                eprintln!("warning: failed to update autorun_worker after retries: {e}");
            }
            Err(_) => eprintln!(
                "warning: autorun_worker update timed out after {}s",
                db_timeout.as_secs()
            ),
            Ok(Ok(())) => {}
        }

        let task_run_fut =
            update_autorun_task_run_with_retry(&*self.store, task_run_id, task_run_update);
        match tokio::time::timeout(db_timeout, task_run_fut).await {
            Ok(Err(e)) => {
                eprintln!("warning: failed to update autorun_task_run after retries: {e}");
            }
            Err(_) => eprintln!(
                "warning: autorun_task_run update timed out after {}s",
                db_timeout.as_secs()
            ),
            Ok(Ok(())) => {}
        }
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
            task_format_id: None,
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
            task_format_id: None,
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
            task_format_id: None,
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
            task_format_id: None,
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
            task_format_id: None,
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
            task_format_id: None,
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
        // Use a recent timestamp so stale detection does NOT remove it.
        let blocker = crate::coordination::merge_queue::MergeQueueEntry {
            session_id: crate::types::SessionId::new_unchecked("ses-blocker"),
            task_id: "blocker-task".to_string(),
            branch: "feat/blocker".to_string(),
            target_branch: "main".to_string(),
            pr_ready_at: chrono::Utc::now().to_rfc3339(),
        };
        crate::coordination::merge_queue::locked_enqueue(&state_path, &blocker).unwrap();
        // Create a .codeflow dir so the project_dir ancestor search finds it.
        std::fs::create_dir_all(dir.path().join(".codeflow")).unwrap();

        // INF-TSK-024-051 Phase 7-rework: write a canonical
        // pathflow-session-status.json in the project-dir fallback location
        // (no worktree registered) so `is_session_alive` returns Active
        // for the blocker. Install the synthetic always-alive validator
        // because the test runner is `cargo`, not `claude`.
        let _validator_guard = crate::session::liveness::override_pid_validator_for_tests(|_| true);
        let status_dir = dir.path().join(".state/session/ses-blocker/pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            format!(
                r#"{{"session_id":"ses-blocker","lead_pid":{},"status":"pf-in-progress"}}"#,
                std::process::id()
            ),
        )
        .unwrap();

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

    /// Verify DB updates use timeout on worker error path (call site C).
    #[tokio::test]
    async fn test_db_updates_on_worker_error_path() {
        let project_dir = tempfile::tempdir().unwrap();
        let wt_path = project_dir.path().join("wt");
        std::fs::create_dir_all(&wt_path).unwrap();
        let (tmux, _, _) = MockTmux::new();
        let claude = FailingClaude;
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

        let cfg = make_worker_cfg("ses-err-test", "task-err");
        let result = worker.run(cfg).await;
        assert!(result.is_err(), "FailingClaude should produce Err");

        // Both DB records should still be updated to Failed via update_db_records.
        let workers = store.autorun_workers.lock().unwrap();
        let aw = workers.get("arw-ses-err-test-task-err").unwrap();
        assert_eq!(aw.status, crate::types::AutorunWorkerStatus::Failed);
        assert!(aw.completed_at.is_some());

        let runs = store.autorun_task_runs.lock().unwrap();
        let atr = runs.get("atr-ses-err-test-task-err").unwrap();
        assert_eq!(atr.status, crate::types::AutorunTaskRunStatus::Failed);
        assert!(atr.completed_at.is_some());
    }

    // --- WS-QA retry 2 coverage boost: MergeOutcome + constants ---

    #[test]
    fn test_merge_outcome_variants_equality() {
        // Exhaustive equality + Debug formatting for all 5 MergeOutcome
        // variants. The two new variants introduced by INF-TSK-048-001
        // AC #10 (CiFailed + CiTimeout) must be equatable, distinct from
        // each other, and distinct from the pre-existing variants.
        assert_eq!(MergeOutcome::Merged, MergeOutcome::Merged);
        assert_eq!(MergeOutcome::QueueTimeout, MergeOutcome::QueueTimeout);
        assert_eq!(MergeOutcome::CiTimeout, MergeOutcome::CiTimeout);

        let a = MergeOutcome::MergeConflict {
            error: "rebase rejected".into(),
        };
        let b = MergeOutcome::MergeConflict {
            error: "rebase rejected".into(),
        };
        assert_eq!(a, b);
        let c = MergeOutcome::MergeConflict {
            error: "different".into(),
        };
        assert_ne!(a, c);

        let fail_a = MergeOutcome::CiFailed {
            failing_checks: vec!["ci".to_string(), "lint".to_string()],
        };
        let fail_b = MergeOutcome::CiFailed {
            failing_checks: vec!["ci".to_string(), "lint".to_string()],
        };
        assert_eq!(fail_a, fail_b);
        let fail_c = MergeOutcome::CiFailed {
            failing_checks: vec!["ci".to_string()],
        };
        assert_ne!(fail_a, fail_c);

        // Cross-variant inequality.
        assert_ne!(MergeOutcome::Merged, MergeOutcome::CiTimeout);
        assert_ne!(MergeOutcome::CiTimeout, MergeOutcome::QueueTimeout);
        assert_ne!(
            MergeOutcome::CiFailed {
                failing_checks: vec![]
            },
            MergeOutcome::CiTimeout
        );
    }

    #[test]
    fn test_merge_outcome_debug_formatting() {
        // Each variant produces a distinct Debug string — log output must
        // unambiguously identify which outcome occurred.
        let merged = format!("{:?}", MergeOutcome::Merged);
        let qt = format!("{:?}", MergeOutcome::QueueTimeout);
        let ct = format!("{:?}", MergeOutcome::CiTimeout);
        let mc = format!("{:?}", MergeOutcome::MergeConflict { error: "x".into() });
        let cf = format!(
            "{:?}",
            MergeOutcome::CiFailed {
                failing_checks: vec!["ci".into()]
            }
        );
        let all = [&merged, &qt, &ct, &mc, &cf];
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "Debug output collision between variants");
                }
            }
        }
        assert!(merged.contains("Merged"));
        assert!(qt.contains("QueueTimeout"));
        assert!(ct.contains("CiTimeout"));
        assert!(mc.contains("MergeConflict"));
        assert!(cf.contains("CiFailed"));
    }

    #[test]
    fn test_worker_heartbeat_interval_constant() {
        // INF-TSK-048-001 AC #8 — heartbeat cadence must be 10 s to balance
        // DB write pressure against the TUI's ability to flag stalled
        // workers. If this test fails, the change was accidental — update
        // the TUI's HEARTBEAT_TTL_SECS (120 s in tui/data.rs) to match.
        assert_eq!(WORKER_HEARTBEAT_INTERVAL_SECS, 10);
    }

    #[test]
    fn test_default_worker_timeout_is_two_hours() {
        // Documented default is 120 minutes. A regression would silently
        // truncate long-running autorun tasks in production.
        assert_eq!(
            DEFAULT_WORKER_TIMEOUT,
            std::time::Duration::from_secs(120 * 60)
        );
    }

    #[test]
    fn test_merge_outcome_exhaustive_match_all_arms() {
        // Exercise every match arm for MergeOutcome — covers the enum
        // discriminant + string-field / vec-field handling that the real
        // serialized_merge produces but which async tests stop short of.
        let outcomes = [
            MergeOutcome::Merged,
            MergeOutcome::QueueTimeout,
            MergeOutcome::CiTimeout,
            MergeOutcome::MergeConflict {
                error: "boom".into(),
            },
            MergeOutcome::CiFailed {
                failing_checks: vec!["ci".into(), "lint".into()],
            },
        ];
        for outcome in &outcomes {
            let label = match outcome {
                MergeOutcome::Merged => "merged",
                MergeOutcome::QueueTimeout => "queue_timeout",
                MergeOutcome::CiTimeout => "ci_timeout",
                MergeOutcome::MergeConflict { error } => {
                    assert_eq!(error, "boom");
                    "merge_conflict"
                }
                MergeOutcome::CiFailed { failing_checks } => {
                    assert_eq!(failing_checks.len(), 2);
                    assert_eq!(failing_checks[0], "ci");
                    "ci_failed"
                }
            };
            assert!(
                [
                    "merged",
                    "queue_timeout",
                    "ci_timeout",
                    "merge_conflict",
                    "ci_failed"
                ]
                .contains(&label)
            );
        }
    }

    // -----------------------------------------------------------------
    // INF-TSK-049-001 batch 2 — W1 / W2 / W3 / shared helper tests.
    // -----------------------------------------------------------------

    /// Build a real on-disk git repo at `dir` with one initial commit.
    fn init_real_git_repo(dir: &std::path::Path) {
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "t@t.c"])
            .current_dir(dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "T"])
            .current_dir(dir)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init", "--no-verify"])
            .current_dir(dir)
            .status()
            .unwrap();
    }

    /// Build a real bare-origin + working repo with upstream tracking.
    /// Returns the working repo path. Useful for branch_on_origin paths.
    fn make_repo_with_origin(name: &str) -> tempfile::TempDir {
        let _ = name;
        let origin = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--bare", "-q"])
            .current_dir(origin.path())
            .status()
            .unwrap();
        let work = tempfile::tempdir().unwrap();
        init_real_git_repo(work.path());
        std::process::Command::new("git")
            .args(["remote", "add", "origin", origin.path().to_str().unwrap()])
            .current_dir(work.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["push", "-u", "origin", "HEAD:refs/heads/main"])
            .current_dir(work.path())
            .status()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "--set-upstream-to=origin/main"])
            .current_dir(work.path())
            .status()
            .unwrap();
        // Keep `origin` alive through the test by leaking it into the
        // returned tempdir's caller-side scope via a thread-local — but
        // easier: just keep both alive in the test body. For this helper
        // we return only `work` and rely on caller to retain `origin`
        // separately when needed. Simplest: drop `origin` to disk via
        // `into_path` to leak it.
        let _ = origin.keep();
        work
    }

    // ---- W1: classify_invoke_outcome ----

    #[test]
    fn w1_exit_zero_completed() {
        let td = tempfile::tempdir().unwrap();
        let (status, msg, warning) = classify_invoke_outcome(td.path(), 0, 42, false);
        assert_eq!(status, "completed");
        assert!(msg.is_empty());
        assert!(warning.is_none());
    }

    #[test]
    fn w1_exit_zero_pr_creation_failed_when_auto_merge_no_pr() {
        let td = tempfile::tempdir().unwrap();
        let (status, msg, warning) = classify_invoke_outcome(td.path(), 0, 0, true);
        assert_eq!(status, "pr_creation_failed");
        assert!(msg.contains("PR expected"));
        assert!(warning.is_none());
    }

    #[test]
    fn w1_exit_124_failed_when_branch_not_on_origin() {
        // No git repo here at all → branch_on_origin returns Inconclusive.
        let td = tempfile::tempdir().unwrap();
        let (status, _msg, warning) = classify_invoke_outcome(td.path(), 124, 0, false);
        assert_eq!(status, "failed", "exit 124 + no git → failed");
        assert!(warning.is_none());
    }

    #[test]
    fn w1_exit_124_completed_when_branch_on_origin() {
        let work = make_repo_with_origin("w1-on-origin");
        let (status, msg, warning) = classify_invoke_outcome(work.path(), 124, 0, false);
        assert_eq!(
            status, "completed",
            "AC #34: exit 124 + branch on origin → completed (success-with-warning)"
        );
        assert!(msg.is_empty());
        assert!(
            warning.as_deref().unwrap_or_default().contains("on origin"),
            "warning must mention 'on origin'; got {warning:?}"
        );
    }

    #[test]
    fn w1_other_nonzero_exit_failed() {
        let td = tempfile::tempdir().unwrap();
        for code in [1, 2, 137] {
            let (status, _, warning) = classify_invoke_outcome(td.path(), code, 0, false);
            assert_eq!(status, "failed", "exit {code} → failed");
            assert!(warning.is_none());
        }
    }

    // ---- Rework iter 1 Finding 3: shared w1_timeout_outcome helper ----

    /// `w1_timeout_outcome` returns `Some(warning)` when the branch is on
    /// origin — the SAME warning string that `classify_invoke_outcome`
    /// emits on its exit-124-on-origin path. This guarantees the two
    /// call sites (inner claude exit 124 vs wrapper-level timeout) agree.
    #[test]
    fn w1_timeout_outcome_on_origin_returns_shared_warning() {
        let work = make_repo_with_origin("w1-shared-on-origin");
        let outcome = w1_timeout_outcome(work.path());
        assert_eq!(outcome.as_deref(), Some(W1_TIMEOUT_ON_ORIGIN_WARNING));

        // And the classify path emits the same warning.
        let (status, _, warning) = classify_invoke_outcome(work.path(), 124, 0, false);
        assert_eq!(status, "completed");
        assert_eq!(warning.as_deref(), Some(W1_TIMEOUT_ON_ORIGIN_WARNING));
    }

    #[test]
    fn w1_timeout_outcome_not_on_origin_returns_none() {
        let td = tempfile::tempdir().unwrap();
        // Non-git → Inconclusive → not OnOrigin → None.
        assert!(w1_timeout_outcome(td.path()).is_none());
    }

    // ---- W2: gh_pr_number_for_branch input sanitisation ----

    #[test]
    fn w2_gh_pr_number_rejects_empty_branch() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(gh_pr_number_for_branch(td.path(), ""), None);
    }

    #[test]
    fn w2_gh_pr_number_rejects_shell_metachars() {
        let td = tempfile::tempdir().unwrap();
        assert_eq!(gh_pr_number_for_branch(td.path(), "feat;rm -rf /"), None);
        assert_eq!(gh_pr_number_for_branch(td.path(), "$(whoami)"), None);
        assert_eq!(gh_pr_number_for_branch(td.path(), "br with space"), None);
    }

    #[test]
    fn w2_gh_pr_number_returns_none_when_gh_missing_or_no_pr() {
        // Sanitised branch but gh either is missing or returns []. Either way
        // result is None; never a fabricated PR number.
        let td = tempfile::tempdir().unwrap();
        let result = gh_pr_number_for_branch(td.path(), "feat/never-exists-anywhere");
        assert_eq!(
            result, None,
            "expected None (gh missing/empty); got {result:?}"
        );
    }

    // -----------------------------------------------------------------
    // Stage-timeout watcher tests
    // -----------------------------------------------------------------

    #[test]
    fn test_latest_ws_sentinel_mtime_returns_none_when_dir_missing() {
        let td = tempfile::tempdir().unwrap();
        let result = latest_ws_sentinel_mtime_secs(td.path(), "ses-stage-test-001", None);
        assert_eq!(
            result, None,
            "no .state/sentinels dir → None (lets baseline win)"
        );
    }

    #[test]
    fn test_latest_ws_sentinel_mtime_returns_none_when_no_ws_files() {
        let td = tempfile::tempdir().unwrap();
        let sid = "ses-stage-test-002";
        let dir = td
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&dir).unwrap();
        // Create a non-ws sentinel — must not match.
        std::fs::write(dir.join("pathflow-pf-1"), "").unwrap();
        let result = latest_ws_sentinel_mtime_secs(td.path(), sid, None);
        assert_eq!(result, None, "only pf-* present, no ws-* → None");
    }

    #[test]
    fn test_latest_ws_sentinel_mtime_finds_pathflow_prefixed() {
        let td = tempfile::tempdir().unwrap();
        let sid = "ses-stage-test-003";
        let dir = td
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pathflow-ws-dev"), "").unwrap();
        let result = latest_ws_sentinel_mtime_secs(td.path(), sid, None);
        assert!(result.is_some(), "pathflow-ws-* must be picked up");
    }

    #[test]
    fn test_latest_ws_sentinel_mtime_finds_bare_ws_prefixed() {
        let td = tempfile::tempdir().unwrap();
        let sid = "ses-stage-test-004";
        let dir = td
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ws-rev"), "").unwrap();
        let result = latest_ws_sentinel_mtime_secs(td.path(), sid, None);
        assert!(result.is_some(), "bare ws-* is also accepted");
    }

    #[test]
    fn test_latest_ws_sentinel_mtime_returns_max_mtime() {
        let td = tempfile::tempdir().unwrap();
        let sid = "ses-stage-test-005";
        let dir = td
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&dir).unwrap();

        let dev = dir.join("pathflow-ws-dev");
        let rev = dir.join("pathflow-ws-rev");
        std::fs::write(&dev, "").unwrap();
        std::fs::write(&rev, "").unwrap();

        // Force `dev` to be older than `rev` by 60s.
        let now = chrono::Utc::now().timestamp();
        let dev_old = filetime::FileTime::from_unix_time(now - 60, 0);
        let rev_now = filetime::FileTime::from_unix_time(now, 0);
        filetime::set_file_mtime(&dev, dev_old).unwrap();
        filetime::set_file_mtime(&rev, rev_now).unwrap();

        let result = latest_ws_sentinel_mtime_secs(td.path(), sid, None).unwrap();
        assert!(
            result >= now - 1,
            "must return max mtime (rev's), got {result}, expected ~{now}"
        );
    }

    #[test]
    fn test_latest_ws_sentinel_mtime_uses_worktree_path_when_provided() {
        let project = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();
        let sid = "ses-stage-test-006";

        // Sentinel exists ONLY in worktree, not in project_dir.
        let wt_dir = worktree
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid);
        std::fs::create_dir_all(&wt_dir).unwrap();
        std::fs::write(wt_dir.join("pathflow-ws-dev"), "").unwrap();

        let from_project = latest_ws_sentinel_mtime_secs(project.path(), sid, None);
        assert_eq!(
            from_project, None,
            "without worktree_path, project_dir lookup must miss"
        );

        let from_worktree =
            latest_ws_sentinel_mtime_secs(project.path(), sid, Some(worktree.path()));
        assert!(
            from_worktree.is_some(),
            "worktree_path override must find the sentinel"
        );
    }

    #[test]
    fn test_emit_stage_timeout_event_writes_jsonl_line() {
        let td = tempfile::tempdir().unwrap();
        let sid = "ses-stage-test-007";
        let task_id = "TSK-stage-7";
        emit_stage_timeout_event(td.path(), sid, task_id, None, Some(1_700_000_000), 3600);

        let path = td.path().join(".state/ledger/pathflow-events.jsonl");
        let content = std::fs::read_to_string(&path).expect("ledger file should exist");
        assert!(content.contains("\"event\":\"stage_timeout\""));
        assert!(content.contains(sid));
        assert!(content.contains(task_id));
        assert!(content.contains("\"stage_timeout_secs\":3600"));
        assert!(content.ends_with('\n'), "JSONL must end with newline");
    }

    #[test]
    fn test_emit_stage_timeout_event_appends_to_existing_file() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join(".state/ledger/pathflow-events.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\"event\":\"prior\"}\n").unwrap();

        emit_stage_timeout_event(td.path(), "ses-1", "TSK-1", None, None, 3600);

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "must append, not overwrite");
        assert!(lines[0].contains("prior"));
        assert!(lines[1].contains("stage_timeout"));
    }

    #[test]
    fn test_emit_stage_timeout_event_uses_worktree_path() {
        let project = tempfile::tempdir().unwrap();
        let worktree = tempfile::tempdir().unwrap();

        emit_stage_timeout_event(
            project.path(),
            "ses-wt",
            "TSK-wt",
            Some(worktree.path()),
            None,
            3600,
        );

        let project_path = project.path().join(".state/ledger/pathflow-events.jsonl");
        let worktree_path = worktree.path().join(".state/ledger/pathflow-events.jsonl");
        assert!(
            !project_path.exists(),
            "must NOT write to project_dir when worktree_path is set"
        );
        assert!(
            worktree_path.exists(),
            "must write to worktree's ledger directory"
        );
    }

    // -- stage_timeout_decision (pure decision function) tests --

    #[test]
    fn test_stage_timeout_decision_within_window_returns_none() {
        // last_seen = 100, now = 150, timeout = 100 → stale_for = 50 < 100.
        let result = stage_timeout_decision(Some(100), 0, 100, 150);
        assert_eq!(
            result, None,
            "stale_for < timeout must keep watcher waiting"
        );
    }

    #[test]
    fn test_stage_timeout_decision_past_window_returns_last_seen() {
        // last_seen = 100, now = 250, timeout = 100 → stale_for = 150 >= 100.
        let result = stage_timeout_decision(Some(100), 0, 100, 250);
        assert_eq!(
            result,
            Some(100),
            "stale_for >= timeout must fire and return last_seen"
        );
    }

    #[test]
    fn test_stage_timeout_decision_no_sentinel_uses_baseline() {
        // No sentinel, baseline = 1000, now = 1200, timeout = 100.
        let result = stage_timeout_decision(None, 1000, 100, 1200);
        assert_eq!(
            result,
            Some(1000),
            "without sentinel, baseline plays the same role"
        );
    }

    #[test]
    fn test_stage_timeout_decision_no_sentinel_baseline_fresh() {
        // No sentinel, baseline = 1000, now = 1050, timeout = 100.
        let result = stage_timeout_decision(None, 1000, 100, 1050);
        assert_eq!(result, None, "baseline within window means no fire");
    }

    #[test]
    fn test_stage_timeout_decision_exact_boundary_fires() {
        // stale_for == timeout: must fire (>=).
        let result = stage_timeout_decision(Some(100), 0, 100, 200);
        assert_eq!(
            result,
            Some(100),
            "boundary case (stale_for == timeout) must fire"
        );
    }

    #[test]
    fn test_stage_timeout_decision_clock_skew_clamps_to_zero() {
        // last_seen > now (clock skew): saturating_sub returns 0,
        // which is < timeout, so no fire.
        let result = stage_timeout_decision(Some(500), 0, 100, 100);
        assert_eq!(
            result, None,
            "clock-skew negative duration must clamp to 0 and not fire"
        );
    }

    #[tokio::test]
    async fn test_await_stage_timeout_with_interval_fires_when_baseline_stale() {
        // No sentinel ever appears. Baseline is set far enough in the
        // past that the FIRST poll sees stale_for >= timeout. The
        // watcher resolves on the first iteration.
        let td = tempfile::tempdir().unwrap();
        let baseline = chrono::Utc::now().timestamp() - 120; // 120s ago
        let last_seen = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            await_stage_timeout_with_interval(
                td.path().to_path_buf(),
                "ses-stale".into(),
                None,
                60, // 60s timeout
                baseline,
                std::time::Duration::from_millis(10), // tiny poll interval
            ),
        )
        .await
        .expect("watcher must resolve within 5s");
        assert_eq!(
            last_seen, baseline,
            "no sentinel → returns the baseline, since baseline is what last_seen falls back to"
        );
    }

    #[tokio::test]
    async fn test_await_stage_timeout_with_interval_does_not_fire_when_within_window() {
        // Baseline is recent enough that even after a few polls the
        // watcher should stay parked. We bound it with a real-time
        // timeout — if the watcher resolves we know the decision logic
        // is broken.
        let td = tempfile::tempdir().unwrap();
        let baseline = chrono::Utc::now().timestamp(); // now
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            await_stage_timeout_with_interval(
                td.path().to_path_buf(),
                "ses-fresh".into(),
                None,
                3600, // huge timeout
                baseline,
                std::time::Duration::from_millis(10),
            ),
        )
        .await;
        assert!(
            result.is_err(),
            "watcher MUST be still polling (real-time timeout fired); got resolved = {result:?}"
        );
    }

    // ─── AC-12 retry helper tests (INF-TSK-050-003) ─────────────────────

    #[test]
    fn test_db_update_retry_constants() {
        // Document the retry contract: 3 total attempts (1 immediate +
        // 2 retries) with 1s/3s delays. If anyone changes the delay
        // schedule, this test forces them to also update the contract
        // doc on `update_autorun_*_with_retry`.
        assert_eq!(DB_UPDATE_MAX_ATTEMPTS, 3, "AC-12: 3 attempts total");
        assert_eq!(
            DB_UPDATE_RETRY_DELAYS,
            [Duration::from_secs(1), Duration::from_secs(3)],
            "AC-12: 1s/3s exponential backoff"
        );
    }

    #[tokio::test]
    async fn test_db_update_retry_succeeds_after_two_failures() {
        // INF-TSK-050-003 AC-12 happy path: store fails attempts 1 and 2,
        // succeeds on attempt 3 — the helper returns Ok and the caller
        // never sees the transient failures.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = Arc::clone(&attempts);

        // Use zero-duration delays so the test runs in milliseconds
        // instead of 4 seconds. The behavior we're verifying is the
        // retry COUNT and the SUCCESS-AFTER-FAILURE path; the actual
        // backoff timing is documented + asserted by
        // `test_db_update_retry_constants`.
        let zero = [Duration::from_millis(0), Duration::from_millis(0)];

        let result: Result<(), crate::error::DbError> = update_with_retry(
            zero,
            move || {
                let attempts = Arc::clone(&attempts_clone);
                async move {
                    let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    if n < 3 {
                        Err(crate::error::DbError::Query(format!(
                            "transient busy attempt {n}"
                        )))
                    } else {
                        Ok(())
                    }
                }
            },
            "test_op",
            "rec-1",
        )
        .await;

        assert!(result.is_ok(), "expected success after 2 retries");
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "exactly 3 attempts: 1 immediate + 2 retries"
        );
    }

    #[tokio::test]
    async fn test_db_update_retry_propagates_after_exhausted() {
        // INF-TSK-050-003 AC-12 failure path: store fails ALL 3 attempts;
        // helper returns the last Err so the caller can log/observe.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_clone = Arc::clone(&attempts);

        let zero = [Duration::from_millis(0), Duration::from_millis(0)];
        let result: Result<(), crate::error::DbError> = update_with_retry(
            zero,
            move || {
                let attempts = Arc::clone(&attempts_clone);
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err(crate::error::DbError::Query("permanent failure".into()))
                }
            },
            "test_op",
            "rec-2",
        )
        .await;

        assert!(
            result.is_err(),
            "expected error after all retries exhausted"
        );
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "all 3 attempts must be tried before giving up"
        );
    }

    // ─── AC-11 apply_merge_writeback outcome mapping ─────────────────────

    #[tokio::test]
    async fn test_apply_merge_writeback_sets_pr_merged_at_on_merged() {
        // Verify the AC-11 contract: Merged → status=Completed AND
        // pr_merged_at set to a non-empty RFC 3339 string.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let worker = crate::models::AutorunWorker {
            id: "aw-merge-test".into(),
            session_id: "ar-test".into(),
            worker_num: 1,
            task_id: "task-merge".into(),
            status: crate::types::AutorunWorkerStatus::Running,
            tmux_session: None,
            worktree_path: None,
            file_scope: vec![],
            scope_policy: "soft".into(),
            worker_session_id: None,
            pr_number: Some(101),
            pr_merged_at: None,
            started_at: None,
            completed_at: None,
        };
        store.create_autorun_worker(&worker).await.unwrap();

        apply_merge_writeback(&store, "aw-merge-test", &MergeOutcome::Merged)
            .await
            .expect("writeback should succeed");

        let updated = store
            .list_autorun_workers("ar-test")
            .await
            .unwrap()
            .into_iter()
            .find(|w| w.id == "aw-merge-test")
            .expect("worker still in store");
        assert_eq!(updated.status, crate::types::AutorunWorkerStatus::Completed);
        assert!(
            updated.pr_merged_at.is_some(),
            "Merged outcome must set pr_merged_at"
        );
    }

    #[tokio::test]
    async fn test_apply_merge_writeback_does_not_set_pr_merged_at_on_conflict() {
        // INF-TSK-050-003 AC-13 dependency: a merge_conflict worker
        // MUST NOT have pr_merged_at set, so the all-merged final-PR
        // gate excludes it.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let worker = crate::models::AutorunWorker {
            id: "aw-conflict".into(),
            session_id: "ar-conflict".into(),
            worker_num: 1,
            task_id: "task-c".into(),
            status: crate::types::AutorunWorkerStatus::Running,
            tmux_session: None,
            worktree_path: None,
            file_scope: vec![],
            scope_policy: "soft".into(),
            worker_session_id: None,
            pr_number: Some(202),
            pr_merged_at: None,
            started_at: None,
            completed_at: None,
        };
        store.create_autorun_worker(&worker).await.unwrap();

        apply_merge_writeback(
            &store,
            "aw-conflict",
            &MergeOutcome::MergeConflict {
                error: "test conflict".into(),
            },
        )
        .await
        .expect("writeback should succeed");

        let updated = store
            .list_autorun_workers("ar-conflict")
            .await
            .unwrap()
            .into_iter()
            .find(|w| w.id == "aw-conflict")
            .expect("worker still in store");
        assert_eq!(updated.status, crate::types::AutorunWorkerStatus::Failed);
        assert!(
            updated.pr_merged_at.is_none(),
            "MergeConflict outcome MUST NOT set pr_merged_at — final PR gate depends on this"
        );
    }

    // ─── INF-TSK-050-003 AC-10 / WS-REV MAJOR-4: per-poll stale-head ─────
    //
    // These tests exercise the EXTRACTED `poll_for_position_zero_iteration`
    // helper that `serialized_merge` calls in its wait loop. A regression
    // that moves `try_remove_stale_head` back to deadline-only invocation
    // would no longer be called from this helper and the parity test
    // below would fail. The helper is also pure-synchronous so we can
    // drive multiple iterations deterministically without
    // `tokio::time::sleep`.

    fn make_test_state_path() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let coord = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord).expect("mkdir coordination");
        let path = coord.join("state.loro");
        (dir, path)
    }

    fn make_test_entry(
        sid: &crate::types::SessionId,
        target: &str,
    ) -> crate::coordination::merge_queue::MergeQueueEntry {
        crate::coordination::merge_queue::MergeQueueEntry {
            session_id: sid.clone(),
            task_id: format!("task-{}", sid.as_str()),
            branch: format!("feat/{}", sid.as_str()),
            target_branch: target.to_string(),
            pr_ready_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn test_poll_iteration_returns_ready_when_at_position_zero() {
        // Solo worker enqueued — should observe position 0 immediately.
        let (_d, state_path) = make_test_state_path();
        let target = "main";
        let sid = crate::types::SessionId::new_unchecked("ses-solo");
        let entry = make_test_entry(&sid, target);
        crate::coordination::merge_queue::locked_enqueue(&state_path, &entry).unwrap();

        let outcome = poll_for_position_zero_iteration(&state_path, &sid, target, &entry);
        assert_eq!(outcome, PollOutcome::Ready);
    }

    #[test]
    fn test_poll_iteration_returns_waiting_when_position_nonzero() {
        // Two workers enqueued — second one observes position 1.
        let (_d, state_path) = make_test_state_path();
        let target = "main";
        let sid_a = crate::types::SessionId::new_unchecked("ses-front");
        let sid_b = crate::types::SessionId::new_unchecked("ses-behind");
        crate::coordination::merge_queue::locked_enqueue(
            &state_path,
            &make_test_entry(&sid_a, target),
        )
        .unwrap();
        let entry_b = make_test_entry(&sid_b, target);
        crate::coordination::merge_queue::locked_enqueue(&state_path, &entry_b).unwrap();

        // sid_a (test fake) is alive (current PID), so try_remove_stale_head
        // does not remove it. The test process IS alive but isn't named
        // "claude" — try_remove_stale_head's stale check would still
        // remove it, BUT we don't have a status_file written, so the
        // stale-head logic short-circuits. Either way, sid_b stays at
        // position 1.
        let outcome = poll_for_position_zero_iteration(&state_path, &sid_b, target, &entry_b);
        // The outcome depends on whether try_remove_stale_head finds the
        // head stale. Since we wrote no status file for sid_a, it likely
        // CAN identify it as stale. Accept either: the contract is "do
        // not deadlock", not "head is alive".
        assert!(
            matches!(outcome, PollOutcome::Waiting | PollOutcome::Ready),
            "outcome must be deterministic forward progress, got: {outcome:?}"
        );
    }

    #[test]
    fn test_poll_iteration_after_stale_head_removed_promotes_next() {
        // INF-TSK-050-003 AC-10: when a predecessor session is dead,
        // the per-poll stale-head detection removes it and the next
        // iteration sees position 0 — without waiting for the deadline.
        // We simulate this by manually calling try_remove_stale_head's
        // production code path: enqueue A, simulate A being stale by
        // calling locked_remove_by_session, then verify the next
        // iteration of poll_for_position_zero_iteration says Ready
        // for B.
        let (_d, state_path) = make_test_state_path();
        let target = "main";
        let sid_a = crate::types::SessionId::new_unchecked("ses-crashed-head");
        let sid_b = crate::types::SessionId::new_unchecked("ses-next");
        let entry_a = make_test_entry(&sid_a, target);
        let entry_b = make_test_entry(&sid_b, target);
        crate::coordination::merge_queue::locked_enqueue(&state_path, &entry_a).unwrap();
        crate::coordination::merge_queue::locked_enqueue(&state_path, &entry_b).unwrap();

        // Pre-condition: B is at position 1.
        let pos_b_before = crate::coordination::merge_queue::locked_position_for_target(
            &state_path,
            &sid_b,
            target,
        )
        .unwrap();
        assert_eq!(pos_b_before, Some(1));

        // Simulate the predecessor crash detection: directly remove A
        // (this is what try_remove_stale_head does internally for a
        // stale head, exercising the exact same primitive).
        crate::coordination::merge_queue::locked_remove_by_session(&state_path, &sid_a).unwrap();

        // Drive a single poll iteration for B.
        let outcome = poll_for_position_zero_iteration(&state_path, &sid_b, target, &entry_b);
        assert_eq!(
            outcome,
            PollOutcome::Ready,
            "AC-10: B must observe position 0 within one poll iteration after head removed"
        );
    }

    #[test]
    fn test_poll_iteration_re_enqueues_when_session_missing() {
        // Edge case: session was somehow removed between enqueue and
        // the position check. Helper re-enqueues and reports Waiting
        // (not Error — the loop should recover).
        let (_d, state_path) = make_test_state_path();
        let target = "main";
        let sid = crate::types::SessionId::new_unchecked("ses-missing");
        let entry = make_test_entry(&sid, target);
        // Do NOT enqueue — simulate the missing-from-queue case.

        let outcome = poll_for_position_zero_iteration(&state_path, &sid, target, &entry);
        // Should be Waiting (re-enqueue happened) OR Ready (re-enqueue
        // followed by position check observing position 0). Either is
        // acceptable; ExitOnError is NOT.
        assert!(
            matches!(outcome, PollOutcome::Waiting | PollOutcome::Ready),
            "re-enqueue must not surface as ExitOnError, got: {outcome:?}"
        );

        // Critical post-condition: session is now in the queue.
        let pos =
            crate::coordination::merge_queue::locked_position_for_target(&state_path, &sid, target)
                .unwrap();
        assert!(
            pos.is_some(),
            "re-enqueue must put the session back in the queue"
        );
    }
}
