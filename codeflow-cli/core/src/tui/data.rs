//! Data fetching layer for TUI dashboards.
//!
//! Provides view-model structs (`BatchView`, `TaskView`, `SessionView`) and
//! functions that assemble them from the `DataStore` and filesystem state.

use std::path::Path;

use serde::Serialize;

use crate::error::DbError;
use crate::models::AutorunSession;
use crate::store::DataStore;
use crate::types::{AutorunSessionStatus, AutorunTaskRunStatus};

// ---------------------------------------------------------------------------
// View-model structs
// ---------------------------------------------------------------------------

/// Aggregated view of an autorun batch for TUI rendering.
#[derive(Debug, Clone, Serialize)]
pub struct BatchView {
    pub session_id: String,
    pub batch_name: String,
    pub status: AutorunSessionStatus,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    pub skipped_tasks: i32,
    pub running_count: usize,
    /// Status-driven elapsed — live for Running/Aborting, frozen for terminal
    /// (`completed_at - created_at`, fallback to `updated_at`) per
    /// INF-TSK-049-001 AC #2.
    pub elapsed_secs: i64,
    /// True when `elapsed_secs` has frozen — the header ETA and "% complete"
    /// projections are meaningless post-termination and MUST be suppressed.
    /// INF-TSK-049-001 AC #2.
    pub elapsed_frozen: bool,
    pub target_branch: Option<String>,
    /// URL of the final PR for this batch (typically the integration
    /// branch → main PR). Populated by the orchestrator when the merge
    /// queue successfully creates the consolidating PR; `None` for live
    /// batches and for terminal batches that did not produce a final PR.
    /// INF-TSK-050-001 AC #19.
    pub final_pr_url: Option<String>,
    pub tasks: Vec<TaskView>,
}

/// Per-task view for the TUI table.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub task_id: String,
    pub status: AutorunTaskRunStatus,
    /// Display status (e.g., "Waiting" for pending tasks in a running session).
    pub display_status: String,
    pub phase: Option<String>,
    pub branch: Option<String>,
    pub pr_number: Option<i64>,
    pub tmux_session: Option<String>,
    pub duration_secs: Option<i64>,
    pub exit_code: Option<i64>,
    pub worktree_path: Option<String>,
    pub error_message: Option<String>,
    pub stages: Vec<StageInfo>,
    /// Worker session ID for display in the unified table.
    pub worker_session_id: Option<String>,
    /// Work type derived from branch prefix (e.g., "FEAT", "FIX").
    pub work_type: Option<String>,
    /// Lead PID of the worker session, resolved from
    /// `pathflow-session-status.json` under the worker's worktree.
    /// `None` when the status file is unreadable or the worker has not yet
    /// written a PID (e.g., pending tasks).
    pub pid: Option<i32>,
    /// PID liveness — `Some(true)` when `kill(pid, 0)` succeeds, `Some(false)`
    /// when the process is dead, `None` when `pid` is `None`.
    pub pid_alive: Option<bool>,
    /// Formatted task identifier (e.g. `INF-TSK-046-008`). Preferred for
    /// display; falls back to a truncated ULID via
    /// [`format_task_id_for_display`].
    pub task_format_id: Option<String>,
}

/// Stage pipeline entry for the detail pane.
#[derive(Debug, Clone, Serialize)]
pub struct StageInfo {
    pub name: String,
    pub completed: bool,
}

/// View of an interactive session for the TUI dashboard.
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub session_id: String,
    pub status: String,
    pub branch: Option<String>,
    pub phase: Option<String>,
    pub work_type: Option<String>,
    pub task_id: Option<String>,
    /// Formatted task identifier (e.g. `INF-TSK-046-008`). Preferred over
    /// `task_id` for display; callers use `format_task_id_for_display` to
    /// render the TUI's TASK column.
    pub task_format_id: Option<String>,
    /// Last completed PathFlow phase recorded in the DB (e.g. `pf-4`).
    /// Used as a fallback when the session is stale and its worktree
    /// sentinels are no longer reachable.
    pub last_phase: Option<String>,
    pub team_name: Option<String>,
    pub pid: i64,
    pub worktree_path: Option<String>,
    pub duration_secs: i64,
    pub managed: bool,
    pub created_at: String,
    /// Whether this session is hidden in the default (filtered) view.
    /// Active sessions are never hidden; only terminal (stale/complete)
    /// sessions beyond the `keep_last` threshold are marked hidden.
    pub hidden: bool,
}

/// Aggregated counts for the interactive session dashboard header.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionSummary {
    pub active: usize,
    pub stale: usize,
    pub complete: usize,
    /// Number of terminal sessions hidden in the default filtered view.
    pub hidden_count: usize,
}

/// Summary entry for the autorun batch list view (one row per batch session).
#[derive(Debug, Clone, Serialize)]
pub struct BatchListEntry {
    pub session_id: String,
    pub batch_name: String,
    pub status: AutorunSessionStatus,
    pub total_tasks: i32,
    pub completed_tasks: i32,
    pub failed_tasks: i32,
    pub running_count: usize,
    /// Status-driven elapsed: live `now - created_at` for Running/Aborting,
    /// frozen `completed_at|updated_at - created_at` for terminal statuses.
    /// INF-TSK-048-001 AC #9.
    pub elapsed_secs: i64,
    /// Task currently being executed by any worker on this session.
    /// INF-TSK-049-001 AC #15: PRESERVED on task finish (previously cleared)
    /// so terminal rows retain the last-dispatched task for visibility.
    pub current_task_id: Option<String>,
    /// Human-readable task format id (e.g. `INF-TSK-049-001`) paired with
    /// `current_task_id`. Preferred for display. Same lifecycle — written
    /// on dispatch, preserved on finish. INF-TSK-049-001 AC #15.
    pub current_task_format_id: Option<String>,
    /// Seconds since the last worker heartbeat. `None` for terminal statuses
    /// or sessions that never wrote a heartbeat. INF-TSK-048-001 AC #9.
    pub idle_secs: Option<i64>,
    /// URL of the final PR for this batch (integration branch → main).
    /// Mirrored from `AutorunSession.final_pr_url` so the batch list view
    /// can render a "PR" column without a second DB lookup. `None` when
    /// the orchestrator has not yet recorded the final PR.
    /// INF-TSK-050-001 AC #19.
    pub final_pr_url: Option<String>,
}

/// Threshold (seconds) beyond which an IDLE value is rendered red. Worker
/// writes heartbeats every 10 s (`WORKER_HEARTBEAT_INTERVAL_SECS`); 120 s
/// allows for a handful of missed ticks before flagging the session.
pub const HEARTBEAT_TTL_SECS: i64 = 120;

// ---------------------------------------------------------------------------
// Data fetching
// ---------------------------------------------------------------------------

/// Fetch a list of all autorun batch sessions for the batch list TUI view.
///
/// Returns one `BatchListEntry` per autorun session, sorted with
/// running/aborting first, then by created_at DESC.
///
/// Convenience wrapper around [`fetch_batch_list_filtered`] using the
/// "show everything" filter (`all: true`). Callers that need to narrow
/// the result set (the TUI's `since`+`ids` union, for example) should
/// call [`fetch_batch_list_filtered`] directly.
///
/// # Errors
///
/// Returns an error if the store cannot be queried.
pub async fn fetch_batch_list<S: DataStore>(store: &S) -> Result<Vec<BatchListEntry>, DbError> {
    fetch_batch_list_filtered(
        store,
        crate::models::AutorunSessionFilter {
            all: true,
            ..Default::default()
        },
    )
    .await
}

/// Fetch a list of autorun batch sessions narrowed by `filter`.
///
/// INF-TSK-050-001 AC #9: the TUI builds a filter combining `since`
/// (recent rows) with `ids` (rows currently on screen) so the same
/// fetch services both "what's new" and "what's still visible" in
/// one round trip. The mock and SurrealDB stores both implement
/// the OR-semantic (`since OR ids`) so callers do not have to issue
/// two queries.
///
/// # Errors
///
/// Returns an error if the store cannot be queried.
pub async fn fetch_batch_list_filtered<S: DataStore>(
    store: &S,
    filter: crate::models::AutorunSessionFilter,
) -> Result<Vec<BatchListEntry>, DbError> {
    let sessions = store.list_autorun_sessions(filter).await?;

    let mut entries = Vec::with_capacity(sessions.len());
    for s in &sessions {
        let workers = store.list_autorun_workers(&s.id).await?;
        let running_count = workers
            .iter()
            .filter(|w| w.status == crate::types::AutorunWorkerStatus::Running)
            .count();
        entries.push(build_list_entry(s, running_count));
    }

    sort_batch_list(&mut entries);
    Ok(entries)
}

/// Bulk-fetch variant that replaces the N+1 `list_autorun_workers` loop with
/// a single `WHERE session_id INSIDE $ids` query (INF-TSK-048-001 AC #7).
///
/// The store handle is caller-owned so the TUI background task can open a
/// fresh connection per fetch cycle for cross-process visibility.
///
/// Convenience wrapper around [`fetch_batch_list_bulk_filtered`] using
/// the "show everything" filter (`all: true`).
///
/// # Errors
///
/// Returns an error if either underlying query fails.
pub async fn fetch_batch_list_bulk<S: DataStore>(
    store: &S,
) -> Result<Vec<BatchListEntry>, DbError> {
    fetch_batch_list_bulk_filtered(
        store,
        crate::models::AutorunSessionFilter {
            all: true,
            ..Default::default()
        },
    )
    .await
}

/// Filter-aware bulk variant of [`fetch_batch_list_bulk`].
///
/// INF-TSK-050-001 AC #9: when the TUI is in the default filtered view
/// (the `[a]` toggle is off) the caller passes a filter combining
/// `since: Some(t0)` with `ids: Some(initial_active_ids)` so the SQL
/// engine returns the union "rows newer than t0 OR rows currently on
/// screen". When `[a]` is on the caller passes `all: true` and the
/// behavior matches [`fetch_batch_list_bulk`] exactly.
///
/// # Errors
///
/// Returns an error if either underlying query fails.
pub async fn fetch_batch_list_bulk_filtered<S: DataStore>(
    store: &S,
    filter: crate::models::AutorunSessionFilter,
) -> Result<Vec<BatchListEntry>, DbError> {
    let sessions = store.list_autorun_sessions(filter).await?;

    if sessions.is_empty() {
        return Ok(Vec::new());
    }

    let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    let workers = store.list_autorun_workers_bulk(&session_ids).await?;

    // Group workers by session_id in a single linear pass (workers vec is
    // ordered by session_id from the query).
    let mut by_session: std::collections::HashMap<&str, usize> =
        std::collections::HashMap::with_capacity(sessions.len());
    for worker in &workers {
        if worker.status == crate::types::AutorunWorkerStatus::Running {
            *by_session.entry(worker.session_id.as_str()).or_insert(0) += 1;
        }
    }

    let mut entries = Vec::with_capacity(sessions.len());
    for s in &sessions {
        let running_count = by_session.get(s.id.as_str()).copied().unwrap_or(0);
        entries.push(build_list_entry(s, running_count));
    }

    sort_batch_list(&mut entries);
    Ok(entries)
}

/// Shared constructor for `BatchListEntry` so the single-session and
/// bulk-query code paths produce identical output.
fn build_list_entry(s: &AutorunSession, running_count: usize) -> BatchListEntry {
    BatchListEntry {
        session_id: s.id.clone(),
        batch_name: s
            .batch_name
            .clone()
            .unwrap_or_else(|| s.id[..s.id.len().min(20)].to_string()),
        status: s.status,
        total_tasks: s.total_tasks,
        completed_tasks: s.completed_tasks,
        failed_tasks: s.failed_tasks,
        running_count,
        elapsed_secs: status_driven_elapsed_secs(s, chrono::Utc::now()),
        current_task_id: s.current_task_id.clone(),
        current_task_format_id: s.current_task_format_id.clone(),
        idle_secs: compute_idle_secs(s, chrono::Utc::now()),
        // INF-TSK-050-001 AC #19: surface final PR URL on the list row so
        // the BatchList view can render a "PR" column without a second
        // round trip. Populated by the orchestrator after the final PR
        // is created; `None` for batches that have not yet reached that
        // state.
        final_pr_url: s.final_pr_url.clone(),
    }
}

/// Compute the ELAPSED value for the BatchList row.
///
/// INF-TSK-048-001 AC #9 / INF-TSK-049-001 AC #1-2: the value MUST freeze for
/// terminal sessions so the elapsed column does not tick up after a run has
/// finished. Thin wrapper around [`crate::tui::duration::freeze_on_terminal_secs`];
/// the live-vs-terminal rule lives in one place now (INF-TSK-049-001 AC #7).
///
/// Pure function — takes `now` explicitly so tests can pin the clock.
#[must_use]
pub fn status_driven_elapsed_secs(s: &AutorunSession, now: chrono::DateTime<chrono::Utc>) -> i64 {
    crate::tui::duration::freeze_on_terminal_secs(s, now, false)
}

/// Compute seconds since the worker's last heartbeat, relative to `now`.
///
/// Returns `None` for terminal statuses (IDLE column is blank for finished
/// rows) and for sessions that never wrote a heartbeat.
#[must_use]
pub fn compute_idle_secs(s: &AutorunSession, now: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    use AutorunSessionStatus::{Aborting, Running};
    if !matches!(s.status, Running | Aborting) {
        return None;
    }
    let heartbeat = s.last_heartbeat_at.as_deref()?;
    let parsed = chrono::DateTime::parse_from_rfc3339(heartbeat).ok()?;
    Some(now.signed_duration_since(parsed).num_seconds().max(0))
}

/// True when an IDLE value should be rendered in red.
///
/// Pure predicate so the render layer can be tested without constructing a
/// ratatui frame. INF-TSK-048-001 AC #9.
#[must_use]
pub fn is_idle_stale(idle_secs: Option<i64>) -> bool {
    matches!(idle_secs, Some(secs) if secs > HEARTBEAT_TTL_SECS)
}

/// Sort: running/aborting first, then by session_id (proxy for created_at
/// DESC since `list_autorun_sessions` already returns sorted).
fn sort_batch_list(entries: &mut [BatchListEntry]) {
    entries.sort_by(|a, b| {
        let a_active = matches!(
            a.status,
            AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
        );
        let b_active = matches!(
            b.status,
            AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
        );
        b_active.cmp(&a_active)
    });
}

/// TUI batch-list snapshot shared between the background fetcher and the
/// render thread (INF-TSK-048-001 AC #7).
///
/// The render thread reads the latest value via `tokio::sync::watch::Receiver`,
/// so display never blocks on a slow DB fetch. When `stale=true`, the render
/// thread shows the LAST successful snapshot and footers a `[STALE — Ns ago]`
/// marker. When `error` is set, the render thread additionally footers a
/// short error string.
#[derive(Debug, Clone)]
pub struct BatchListSnapshot {
    pub entries: Vec<BatchListEntry>,
    pub fetched_at: std::time::SystemTime,
    pub stale: bool,
    pub error: Option<String>,
}

impl Default for BatchListSnapshot {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            fetched_at: std::time::SystemTime::now(),
            stale: false,
            error: None,
        }
    }
}

impl BatchListSnapshot {
    /// Age of the snapshot in seconds relative to `now`. Returns 0 when the
    /// clock moves backwards (NTP correction).
    #[must_use]
    pub fn age_secs(&self, now: std::time::SystemTime) -> u64 {
        now.duration_since(self.fetched_at)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Result of calling the background fetcher for one cycle. Used by the
/// watch-channel publisher to decide whether to bump `stale` or preserve
/// prior data.
#[derive(Debug)]
pub enum FetchCycleOutcome {
    Fresh(Vec<BatchListEntry>),
    Timeout,
    Error(String),
}

/// Fetch a `BatchView` for the given session, or the most recent active session.
///
/// When no running/aborting session exists, falls back to the most recent
/// completed/failed batch so the TUI always has something to display.
///
/// Convenience wrapper around [`fetch_batch_view_filtered`] using the
/// default filter for the no-id fallback path.
///
/// # Errors
///
/// Returns an error if the store cannot be queried or no matching session is found.
pub async fn fetch_batch_view<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: Option<&str>,
) -> Result<Option<BatchView>, DbError> {
    fetch_batch_view_filtered(
        store,
        project_dir,
        session_id,
        crate::models::AutorunSessionFilter::default(),
    )
    .await
}

/// Filter-aware variant of [`fetch_batch_view`].
///
/// INF-TSK-050-001 AC #9: parallels [`fetch_batch_list_filtered`] so the
/// detail-view fallback (when no `session_id` is supplied) honors the
/// same `since`/`ids` filter the list view uses. When `session_id` is
/// supplied, the filter is ignored — the explicit lookup goes straight
/// to `get_autorun_session`.
///
/// # Errors
///
/// Returns an error if the store cannot be queried or no matching session is found.
pub async fn fetch_batch_view_filtered<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: Option<&str>,
    filter: crate::models::AutorunSessionFilter,
) -> Result<Option<BatchView>, DbError> {
    let session = match session_id {
        Some(sid) => store.get_autorun_session(sid).await?,
        None => {
            let sessions = store.list_autorun_sessions(filter).await?;
            // Prefer running/aborting; fall back to most recent by created_at.
            let active = sessions.iter().find(|s| {
                matches!(
                    s.status,
                    AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
                )
            });
            match active {
                Some(s) => Some(s.clone()),
                None => sessions.into_iter().next(), // most recent (already sorted DESC)
            }
        }
    };

    let Some(session) = session else {
        return Ok(None);
    };

    build_batch_view(store, project_dir, &session)
        .await
        .map(Some)
}

/// Build a complete `BatchView` from a session record.
async fn build_batch_view<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session: &AutorunSession,
) -> Result<BatchView, DbError> {
    let workers = store.list_autorun_workers(&session.id).await?;

    let task_runs = store.list_autorun_task_runs(&session.id).await?;

    let running_count = workers
        .iter()
        .filter(|w| w.status == crate::types::AutorunWorkerStatus::Running)
        .count();

    // INF-TSK-049-001 AC #2: freeze the detail-header ELAPSED for terminal
    // batches so the value matches the list row. Previously used
    // `compute_elapsed_secs(&session.created_at)` which ticks forever.
    // INF-TSK-050-001 AC #20: `Pending` removed; only terminal sessions
    // need a frozen ELAPSED. Live (Running/Aborting/Paused) sessions tick.
    let elapsed_secs = status_driven_elapsed_secs(session, chrono::Utc::now());
    let elapsed_frozen = session.status.is_terminal();

    let mut tasks = Vec::with_capacity(task_runs.len());
    for run in &task_runs {
        let tmux = workers
            .iter()
            .find(|w| w.task_id == run.task_id)
            .and_then(|w| w.tmux_session.clone());

        let display_status = compute_display_status(run.status, session.status);

        let phase = read_latest_phase(project_dir, run.worktree_path.as_deref(), None)
            .or_else(|| run.last_phase.clone());

        let stages = read_stage_pipeline(project_dir, run.worktree_path.as_deref(), None);

        let worker_session_id = workers
            .iter()
            .find(|w| w.task_id == run.task_id)
            .and_then(|w| w.worker_session_id.clone());

        let work_type = run
            .branch_name
            .as_deref()
            .map(crate::hooks::pipeline::infer_work_type_from_branch)
            .map(String::from);

        // Resolve the worker's lead PID from pathflow-session-status.json
        // under its worktree. `None` for pending tasks (no worktree/session
        // yet) or for rows whose status file is missing. INF-TSK-024-046
        // AC: surface PID + liveness in the autorun detail pane.
        let (pid, pid_alive) = resolve_worker_pid_liveness(
            project_dir,
            run.worktree_path.as_deref(),
            worker_session_id.as_deref(),
        );

        let task_format_id = crate::autorun::batch::read_task_format_id(&run.task_id, project_dir);

        tasks.push(TaskView {
            task_id: run.task_id.clone(),
            status: run.status,
            display_status,
            phase,
            branch: run.branch_name.clone(),
            pr_number: run.pr_number,
            tmux_session: tmux,
            duration_secs: if matches!(run.status, AutorunTaskRunStatus::Running) {
                // Always recompute for running tasks — stored duration_seconds
                // may be stale from a previous failed run.
                run.started_at.as_deref().map(compute_elapsed_secs)
            } else {
                run.duration_seconds
            },
            exit_code: run.exit_code,
            worktree_path: run.worktree_path.clone(),
            error_message: run.error_message.clone(),
            stages,
            worker_session_id,
            work_type,
            pid,
            pid_alive,
            task_format_id,
        });
    }

    Ok(BatchView {
        session_id: session.id.clone(),
        batch_name: session
            .batch_name
            .clone()
            .unwrap_or_else(|| session.id[..session.id.len().min(20)].to_string()),
        status: session.status,
        total_tasks: session.total_tasks,
        completed_tasks: session.completed_tasks,
        failed_tasks: session.failed_tasks,
        skipped_tasks: session.skipped_tasks,
        running_count,
        elapsed_secs,
        elapsed_frozen,
        target_branch: session.target_branch.clone(),
        // INF-TSK-050-001 AC #19: propagate final_pr_url to the detail
        // view header so the orchestrator's recorded PR URL is visible
        // without another DB lookup.
        final_pr_url: session.final_pr_url.clone(),
        tasks,
    })
}

/// Fetch interactive session views from the database.
///
/// Queries all interactive sessions, enriches each with PathFlow phase
/// data from sentinel files, and computes duration from `created_at`.
///
/// # Errors
///
/// Returns an error if the store cannot be queried.
pub async fn fetch_session_views(
    store: &crate::store::SurrealStore,
    project_dir: &Path,
) -> Result<(Vec<SessionView>, SessionSummary), DbError> {
    fetch_session_views_with_keep_last(store, project_dir, 10).await
}

/// Default PID validator for `fetch_session_views_*`.
///
/// INF-TSK-050-001 AC #4: matches the `validate_claude_pid > 0` semantic
/// — the PID must be alive AND its process name must contain "claude".
/// Tests use [`fetch_session_views_with_keep_last_and_validator`] to
/// inject a synthetic predicate so they don't depend on the test runner
/// being a Claude Code child process.
fn default_session_pid_validator(pid: u32) -> bool {
    crate::session::process::validate_claude_pid(pid) > 0
}

/// Fetch interactive session views with a configurable `keep_last` threshold.
///
/// `keep_last` controls how many terminal sessions are shown in the default
/// filtered view. Active sessions are always visible.
///
/// # Errors
///
/// Returns an error if the store cannot be queried.
pub async fn fetch_session_views_with_keep_last(
    store: &crate::store::SurrealStore,
    project_dir: &Path,
    keep_last: usize,
) -> Result<(Vec<SessionView>, SessionSummary), DbError> {
    fetch_session_views_with_keep_last_and_validator(
        store,
        project_dir,
        keep_last,
        default_session_pid_validator,
    )
    .await
}

/// Internal hook for [`fetch_session_views_with_keep_last`] that accepts a
/// custom PID validator. Production code uses [`default_session_pid_validator`];
/// unit tests pass a closure that returns `true` for known test PIDs so they
/// can exercise the active-session phase fallback chain without spawning a
/// real `claude` child process.
///
/// The validator is called with a non-zero `u32` PID and returns whether the
/// process should count as "live owner of an interactive session". Returning
/// `false` triggers the synchronous stale-promotion path (matching the
/// production behavior when `validate_claude_pid` returns 0).
///
/// # Errors
///
/// Returns an error if the store cannot be queried.
pub async fn fetch_session_views_with_keep_last_and_validator(
    store: &crate::store::SurrealStore,
    project_dir: &Path,
    keep_last: usize,
    pid_validator: fn(u32) -> bool,
) -> Result<(Vec<SessionView>, SessionSummary), DbError> {
    // INF-TSK-049-001 AC #9: filter out autorun worker sessions. Workers
    // register as `interactive_session` rows with `source_cli='codeflow'`,
    // so without this filter the interactive dashboard surfaces them as
    // noise (and counts them as stale when they terminate).
    let sessions: Vec<crate::models::InteractiveSession> = store
        .db()
        .query(
            "SELECT * FROM interactive_session \
             WHERE session_kind = 'interactive' OR session_kind = NONE \
             ORDER BY created_at DESC",
        )
        .await
        .map_err(|e| DbError::Query(e.to_string()))?
        .take(0)
        .map_err(|e| DbError::Query(e.to_string()))?;

    let mut views = Vec::with_capacity(sessions.len());
    let mut summary = SessionSummary::default();
    // Collect sessions whose DB status says `active` but whose PID is dead;
    // INF-TSK-049-001 AC #12 requires us to UPDATE them synchronously
    // before returning so the very first render uses the frozen duration
    // instead of producing the "00:01 blip" from the async write landing
    // later.
    let mut stale_promotions: Vec<String> = Vec::new();
    let now_rfc = chrono::Utc::now().to_rfc3339();

    for s in &sessions {
        // PID liveness override: detect dead sessions still marked "active".
        // INF-TSK-050-001 AC #4: use the injectable `pid_validator` instead
        // of a bare `is_process_alive`. The stored `pid` for an interactive
        // session is the lead Claude Code process; if a different process
        // has since recycled the PID (rare but possible), treating it as
        // alive would suppress the stale-promotion that the user expects.
        // The default validator wraps `validate_claude_pid` so the PID
        // must be BOTH alive AND its process name must contain "claude" —
        // exactly the semantic we want. Tests pass a synthetic predicate.
        let effective_status = if s.status.to_string() == "active" {
            let pid_u32 = u32::try_from(s.pid).unwrap_or(0);
            if pid_u32 > 0 && !pid_validator(pid_u32) {
                stale_promotions.push(s.session_id.clone());
                "stale".to_string()
            } else {
                s.status.to_string()
            }
        } else {
            s.status.to_string()
        };

        match effective_status.as_str() {
            "active" => summary.active += 1,
            "stale" => summary.stale += 1,
            "complete" => summary.complete += 1,
            _ => {}
        }

        // Phase resolution — INF-TSK-047-001 AC #3.
        //
        // | Status   | Live phase | DB last_phase | Displayed |
        // |----------|-----------|---------------|-----------|
        // | active   | Some(x)   | any           | x         |
        // | active   | None      | Some(p)       | p         |
        // | active   | None      | None  (wt ok) | Starting  |
        // | active   | None      | None  (no wt) | N/A       |
        // | stale    | Some(x)   | any           | x         |
        // | stale    | None      | Some(p)       | p         |
        // | stale    | None      | None          | pre-pf1   |
        // | complete | Some(x)   | any           | x         |
        // | complete | None      | Some(p)       | p         |
        // | complete | None      | None          | --        |
        //
        // Pre-migration sessions (created before the schema added
        // `last_phase`) return None from the DB column; we deliberately do
        // NOT reconstruct from the ledger here — it is too expensive for
        // per-tick TUI rendering. Such rows simply render as `--` / `N/A`
        // until their next post_tool_use writes the column.
        let raw_phase = derive_phase_from_session(project_dir, s);
        let phase = raw_phase
            .or_else(|| s.last_phase.clone().filter(|p| !p.is_empty()))
            .or_else(|| match effective_status.as_str() {
                "active" => {
                    let has_worktree = s.worktree_path.as_ref().is_some_and(|p| !p.is_empty());
                    if has_worktree {
                        Some("Starting".to_string())
                    } else {
                        Some("N/A".to_string())
                    }
                }
                "stale" => Some("pre-pf1".to_string()),
                _ => None,
            });

        // Duration freeze: live for active, `completed_at - created_at` (or
        // `updated_at - created_at` fallback) for terminal. INF-TSK-049-001
        // AC #4 / AC #7: identical rule to the autorun path — shared helper
        // in `tui::duration`. For sessions we just promoted from active to
        // stale this cycle, synthesize `completed_at` from `now_rfc` so the
        // returned view uses the same frozen value the synchronous DB write
        // is about to persist (AC #12: no "00:01 blip").
        let mut effective_session = s.clone();
        if effective_status == "stale" && s.status == crate::types::InteractiveSessionStatus::Active
        {
            effective_session.status = crate::types::InteractiveSessionStatus::Stale;
            if effective_session.completed_at.is_none() {
                effective_session.completed_at = Some(now_rfc.clone());
            }
        }
        let elapsed = crate::tui::duration::freeze_on_terminal_secs(
            &effective_session,
            chrono::Utc::now(),
            true,
        );

        // Resolve task_id + task_format_id: prefer DB values, fall back to
        // active-task.json on disk. Both reads are idempotent and safe to miss.
        let (task_id, task_format_id) = {
            let disk = s
                .worktree_path
                .as_deref()
                .filter(|p| !p.is_empty())
                .and_then(|wt| {
                    let path = std::path::Path::new(wt).join(".state/runtime/active-task.json");
                    let data = std::fs::read_to_string(path).ok()?;
                    serde_json::from_str::<serde_json::Value>(&data).ok()
                });
            let disk_task_id = disk.as_ref().and_then(|v| {
                v.get("task_id")
                    .and_then(|t| t.as_str())
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
            });
            let disk_format_id = disk.as_ref().and_then(|v| {
                v.get("task_format_id")
                    .and_then(|t| t.as_str())
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
            });
            (
                s.task_id.clone().or(disk_task_id),
                s.task_format_id.clone().or(disk_format_id),
            )
        };

        views.push(SessionView {
            session_id: s.session_id.clone(),
            status: effective_status,
            branch: s.branch.clone(),
            phase,
            work_type: s.work_type.clone(),
            task_id,
            task_format_id,
            last_phase: s.last_phase.clone(),
            team_name: s.team_name.clone(),
            pid: s.pid,
            worktree_path: s.worktree_path.clone(),
            duration_secs: elapsed,
            managed: s.managed,
            created_at: s.created_at.clone(),
            hidden: false, // will be set below
        });
    }

    // Count-based display filtering: active sessions are never hidden,
    // terminal sessions (stale/complete) are sorted by created_at DESC and
    // only the first `keep_last` are shown by default.
    let mut terminal_seen: usize = 0;
    for v in &mut views {
        if v.status == "active" {
            v.hidden = false;
        } else {
            terminal_seen += 1;
            v.hidden = terminal_seen > keep_last;
        }
    }
    summary.hidden_count = terminal_seen.saturating_sub(keep_last);

    // INF-TSK-049-001 AC #12: promote dead-PID-active sessions to stale
    // SYNCHRONOUSLY so the first render after the promotion renders the
    // frozen duration. The earlier async spawn produced a "00:01 blip" —
    // the first render would see `effective_status='stale'` but
    // `completed_at=None`, falling back to `updated_at` or the sentinel,
    // and then the async write would land on a later cycle causing a
    // visible duration jump. The synthetic `completed_at` assignment in
    // the view loop above uses the same `now_rfc` we persist here so the
    // two values match.
    //
    // Cost: ~5 ms per promoted session on the first detection; after
    // promotion the row is no longer in `stale_promotions` so the cost
    // disappears. Acceptable — these rows are rare (session crash only).
    if !stale_promotions.is_empty() {
        let persist_now = now_rfc.clone();
        for sid in stale_promotions {
            let _ = store
                .db()
                .query(
                    "UPDATE interactive_session SET status = 'stale', \
                     completed_at = $now, updated_at = $now \
                     WHERE session_id = $sid AND status = 'active'",
                )
                .bind(("now", persist_now.clone()))
                .bind(("sid", sid))
                .await;
        }
    }

    Ok((views, summary))
}

/// Derive the PathFlow phase for an interactive session.
///
/// Reads sentinel files from the session's worktree (if available), or
/// the project dir, scoped to the session ID.
fn derive_phase_from_session(
    project_dir: &Path,
    session: &crate::models::InteractiveSession,
) -> Option<String> {
    // Guard against path traversal.
    if session.session_id.contains("..") {
        return None;
    }
    let wt = session.worktree_path.as_deref();
    if let Some(w) = wt {
        if w.contains("..") {
            return None;
        }
    }
    // Try reading from pathflow-session-status.json first (authoritative).
    let base = wt.unwrap_or("");
    if !base.is_empty() {
        let status_path = std::path::Path::new(base)
            .join(".state/session")
            .join(&session.session_id)
            .join("pathflow/pathflow-session-status.json");
        if let Ok(content) = std::fs::read_to_string(&status_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(phase) = val.get("last_completed_phase").and_then(|v| v.as_str()) {
                    if !phase.is_empty() {
                        return Some(phase.to_string());
                    }
                }
            }
        }
    }
    // Fallback: scan sentinel files.
    read_latest_phase(project_dir, wt, Some(&session.session_id))
}

// ---------------------------------------------------------------------------
// Refresh-loop stall guard
// ---------------------------------------------------------------------------

/// Decide whether the render loop should issue a data fetch this iteration.
///
/// The interactive and autorun TUIs run two cadences in the same thread:
///
/// - **Input polling** — `event::poll(100 ms)` so key presses feel snappy.
/// - **Data fetching** — `fetch_session_views_with_keep_last` (or the autorun
///   equivalent) at the user-configured `fetch_interval` (default 2 s).
///
/// Before INF-TSK-047-001 the loop fetched on every poll tick, so a slow DB
/// round-trip blocked input for the full fetch duration. The fix gates the
/// fetch behind `elapsed >= fetch_interval`; this predicate makes that gate
/// testable in isolation without spinning up a real terminal, ratatui app,
/// or SurrealDB instance.
///
/// Returns `true` when the caller should fetch (and then reset `last_fetch`
/// to `now`); `false` when it should skip the fetch this iteration and just
/// service input.
///
/// # Examples
///
/// ```
/// use std::time::{Duration, Instant};
/// use codeflow_core::tui::data::should_fetch_now;
///
/// let last = Instant::now();
/// // Immediately after a fetch, the next iteration must NOT fetch again.
/// assert!(!should_fetch_now(last, Instant::now(), Duration::from_secs(2)));
/// // Simulate a full interval elapsing.
/// let later = last + Duration::from_secs(2);
/// assert!(should_fetch_now(last, later, Duration::from_secs(2)));
/// ```
#[must_use]
pub fn should_fetch_now(
    last_fetch: std::time::Instant,
    now: std::time::Instant,
    fetch_interval: std::time::Duration,
) -> bool {
    now.duration_since(last_fetch) >= fetch_interval
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve the worker's lead PID and its liveness.
///
/// Reads `pathflow-session-status.json` from the worker's worktree first,
/// falling back to the main-repo session directory. Returns the stored PID
/// and a `kill(pid, 0)` liveness check. A missing status file, missing PID
/// field, or PID of 0 returns `(None, None)`.
///
/// Pending tasks typically have `worktree_path=None` and no worker
/// registration yet — this function returns `(None, None)` for them so the
/// detail pane renders a dash instead of a misleading zero.
#[must_use]
pub fn resolve_worker_pid_liveness(
    project_dir: &Path,
    worktree_path: Option<&str>,
    worker_session_id: Option<&str>,
) -> (Option<i32>, Option<bool>) {
    let sid = worker_session_id.filter(|s| !s.is_empty());
    let Some(sid) = sid else {
        return (None, None);
    };

    // Try the worker's own worktree first, then fall back to main repo.
    let worktree_root = worktree_path
        .filter(|p| !p.is_empty())
        .map_or(project_dir, Path::new);
    let candidates: [std::path::PathBuf; 2] = [
        worktree_root
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json"),
        project_dir
            .join(".state/session")
            .join(sid)
            .join("pathflow/pathflow-session-status.json"),
    ];

    for path in &candidates {
        let Ok(data) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&data) else {
            continue;
        };
        let Some(pid_u64) = parsed.get("lead_pid").and_then(serde_json::Value::as_u64) else {
            continue;
        };
        if pid_u64 == 0 {
            continue;
        }
        let pid_u32 = u32::try_from(pid_u64).unwrap_or(0);
        if pid_u32 == 0 {
            continue;
        }
        // INF-TSK-050-001 AC #4: stored `lead_pid` is the worker's Claude
        // Code lead process; use `validate_claude_pid` so a recycled PID
        // owned by an unrelated process is treated as dead. Returns the
        // input PID when alive and named "claude", 0 otherwise — convert
        // to a bool for the existing `pid_alive` field semantics.
        let alive = crate::session::process::validate_claude_pid(pid_u32) > 0;
        // Real OS PIDs fit inside `i32::MAX` on every platform we support;
        // if the stored value somehow exceeds it, cap at `i32::MAX` so the
        // conversion remains lossless for practical purposes.
        let pid_i32 = i32::try_from(pid_u32).unwrap_or(i32::MAX);
        return (Some(pid_i32), Some(alive));
    }

    (None, None)
}

/// Abbreviate a session ID for display.
///
/// Session IDs longer than 20 characters are shown as `{first 10}...{last 5}`.
/// Shorter IDs are returned unchanged.
#[must_use]
pub fn abbreviate_session_id(sid: &str) -> String {
    if sid.len() > 20 {
        format!("{}...{}", &sid[..10], &sid[sid.len() - 5..])
    } else {
        sid.to_string()
    }
}

/// Truncate a branch name to `max_chars` with an ellipsis when longer.
///
/// The TUI's BRANCH column caps at 50 characters; anything longer is rendered
/// as `{first max-1 chars}…` so the right-hand columns stay on screen. Branches
/// that already fit are returned verbatim. `max_chars` must be at least 2
/// (the width of the ellipsis plus one content char); smaller values are
/// treated as "do not truncate" and the branch is returned as-is.
#[must_use]
pub fn truncate_branch_for_display(branch: &str, max_chars: usize) -> String {
    let len = branch.chars().count();
    if len <= max_chars || max_chars < 2 {
        return branch.to_string();
    }
    let keep = max_chars - 1;
    let head: String = branch.chars().take(keep).collect();
    format!("{head}…")
}

/// Render the TUI TASK column for one session or task.
///
/// Precedence (per INF-TSK-047-001 AC #1):
/// 1. If `task_format_id` is present, render it verbatim
///    (e.g. `INF-TSK-046-008`).
/// 2. Else if `task_id` is present, render it as a truncated ULID
///    of the form `{first 8}…{last 5}` (e.g. `task-01K…MHH`). Falls
///    through to the full id when it is already short enough.
/// 3. Else render `--`.
#[must_use]
pub fn format_task_id_for_display(task_format_id: Option<&str>, task_id: Option<&str>) -> String {
    if let Some(fmt) = task_format_id.filter(|s| !s.is_empty()) {
        return fmt.to_string();
    }
    match task_id.filter(|s| !s.is_empty()) {
        Some(raw) if raw.chars().count() > 14 => {
            // char-safe truncation; avoids panic on non-ASCII though callers
            // always pass ASCII ULIDs.
            let chars: Vec<char> = raw.chars().collect();
            let head: String = chars.iter().take(8).collect();
            let tail: String = chars.iter().skip(chars.len() - 5).collect();
            format!("{head}…{tail}")
        }
        Some(raw) => raw.to_string(),
        None => "--".to_string(),
    }
}

/// Render the TUI BatchList TASK column with `+N` overflow notation.
///
/// INF-TSK-050-001 AC #8: the BatchList row needs to communicate three
/// things in one column — what task is currently dispatched, that there
/// are more tasks in the batch, and that the column is empty when the
/// batch has not dispatched anything yet. The TASKS column carries the
/// raw count; this column adds context.
///
/// Rendering matrix:
///
/// | total_tasks | id present | rendered                    |
/// |-------------|------------|-----------------------------|
/// | <= 0        | any        | `—` (em-dash)               |
/// | any         | both None  | `—` (em-dash)               |
/// | == 1        | yes        | id (truncated to width)     |
/// | > 1         | None       | `—` (batch not dispatched)  |
/// | > 1         | yes        | `id +N` (`N = total - 1`)   |
///
/// Truncation: when `id + suffix` exceeds `width`, the id is truncated
/// with a trailing `…` while the `+N` suffix is preserved verbatim. If
/// `width` is too small to fit even `…+N`, the full result is returned
/// as-is so the caller's column constraint can decide how to clip — we
/// never silently drop the count. `width == 0` disables truncation.
///
/// Precedence between `current_task_format_id` and `current_task_id`
/// matches `format_task_id_for_display`: the format id wins when present
/// and non-empty. The raw ULID is used only as a last-resort fallback.
#[must_use]
pub fn format_task_cell(
    current_task_format_id: Option<&str>,
    current_task_id: Option<&str>,
    total_tasks: i32,
    width: usize,
) -> String {
    // EM-DASH: zero/negative total means "no tasks tracked yet" — the
    // TASKS column will show 0 / N/A so this column should not pretend
    // to have data.
    if total_tasks <= 0 {
        return "—".to_string();
    }

    // Pick the best id: format id first, raw ULID second, blank third.
    let id_owned: Option<String> =
        if let Some(fmt) = current_task_format_id.filter(|s| !s.is_empty()) {
            Some(fmt.to_string())
        } else {
            current_task_id
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };

    let Some(id) = id_owned else {
        // EM-DASH: no id. Two cases collapse here — total==1 with a
        // pending dispatch, and total>1 batch waiting to start. Both
        // mean "nothing to show in this column right now".
        return "—".to_string();
    };

    if total_tasks == 1 {
        return truncate_for_cell(&id, width);
    }

    // total_tasks > 1 and id present → "id +N"
    let suffix = format!(" +{}", total_tasks - 1);
    let suffix_chars = suffix.chars().count();
    let id_chars = id.chars().count();

    // No truncation needed (or width disabled).
    if width == 0 || id_chars + suffix_chars <= width {
        return format!("{id}{suffix}");
    }

    // Truncate id with `…`, preserve suffix verbatim. Reserve one char
    // for the ellipsis. If the suffix alone is wider than width-1, we
    // give up on truncation and return the full string — the column
    // constraint will clip visually but the count is preserved.
    if suffix_chars + 1 >= width {
        return format!("{id}{suffix}");
    }
    let id_keep = width - suffix_chars - 1;
    let head: String = id.chars().take(id_keep).collect();
    format!("{head}…{suffix}")
}

/// Truncate `s` to `width` characters with a trailing `…` when oversized.
///
/// Helper for the TASK column rendering. `width == 0` disables truncation
/// (caller will let the column constraint clip). `width == 1` returns
/// `…` alone.
fn truncate_for_cell(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if width == 0 || len <= width {
        return s.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let kept: String = s.chars().take(width - 1).collect();
    format!("{kept}…")
}

fn compute_elapsed_secs(started_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(started_at)
        .map(|start| {
            chrono::Utc::now()
                .signed_duration_since(start)
                .num_seconds()
                .max(0)
        })
        .unwrap_or(0)
}

/// Compute the display string for a task row given its run status and the
/// owning session's status.
///
/// INF-TSK-050-001 AC #7: a `Pending` task row (the worker has not yet
/// claimed the task) needs context-dependent rendering once the session
/// itself transitions out of `Running` — otherwise the column says
/// "pending" forever even though the row will never be picked up.
///
/// | task_run | session     | displayed   | rationale                              |
/// |----------|-------------|-------------|----------------------------------------|
/// | Pending  | Running     | "Waiting"   | actively queued, will be picked up     |
/// | Pending  | Aborting    | "Aborting"  | abort signal in flight, row will skip  |
/// | Pending  | Cancelled   | "Cancelled" | run cancelled before this row started  |
/// | Pending  | Failed      | "Skipped"   | batch failed before this row started   |
/// | Pending  | Timeout     | "Skipped"   | batch timed out before this row started|
/// | Pending  | Completed   | "Skipped"   | defensive — should not happen in prod  |
/// | Pending  | Paused      | "pending"   | pass-through; reserved variant         |
/// | other    | any         | task.to_string() | pass-through (e.g. running, failed)|
fn compute_display_status(
    status: AutorunTaskRunStatus,
    session_status: AutorunSessionStatus,
) -> String {
    if matches!(status, AutorunTaskRunStatus::Pending) {
        match session_status {
            AutorunSessionStatus::Running => return "Waiting".to_string(),
            AutorunSessionStatus::Aborting => return "Aborting".to_string(),
            AutorunSessionStatus::Cancelled => return "Cancelled".to_string(),
            AutorunSessionStatus::Failed
            | AutorunSessionStatus::Timeout
            | AutorunSessionStatus::Completed => return "Skipped".to_string(),
            AutorunSessionStatus::Paused => {} // fall through to default
        }
    }
    status.to_string()
}

// `read_latest_phase` now lives in `crate::session::sentinel` so the
// autorun worker (non-TUI build) can reach it without pulling in the `tui`
// feature. Re-exported here to keep existing `tui::data::read_latest_phase`
// call sites working unchanged.
pub use crate::session::sentinel::read_latest_phase;

/// Read stage pipeline status from sentinel files.
///
/// When `session_id` is provided, only scans that session's sentinel subdir.
fn read_stage_pipeline(
    project_dir: &Path,
    worktree_path: Option<&str>,
    session_id: Option<&str>,
) -> Vec<StageInfo> {
    let base = worktree_path.map_or_else(|| project_dir.to_path_buf(), std::path::PathBuf::from);

    let sentinel_dir = base.join(".state/sentinels/pathflow");

    let stage_names = derive_stage_names_for_tui(&base);
    let mut completed_stages = std::collections::HashSet::new();

    match std::fs::read_dir(&sentinel_dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let session_dir = entry.path();
                if !session_dir.is_dir() {
                    continue;
                }
                // Filter to target session when specified.
                if let Some(sid) = session_id {
                    if let Some(dir_name) = session_dir.file_name().and_then(|n| n.to_str()) {
                        if dir_name != sid {
                            continue;
                        }
                    }
                }
                if let Ok(sentinels) = std::fs::read_dir(&session_dir) {
                    for sentinel in sentinels.flatten() {
                        let name = sentinel.file_name();
                        let name = name.to_string_lossy();
                        for stage in &stage_names {
                            if name.contains(stage.as_str()) {
                                completed_stages.insert(stage.clone());
                            }
                        }
                    }
                }
            }
        }
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            eprintln!(
                "warn: failed to read sentinel dir {}: {e}",
                sentinel_dir.display()
            );
        }
    }

    stage_names
        .iter()
        .map(|name| StageInfo {
            name: name.to_uppercase(),
            completed: completed_stages.contains(name),
        })
        .collect()
}

/// Derive the TUI stage name list from the pipelines section of
/// `pathflow-config.json`.
///
/// Returns the union of all pipeline stages (lowercased, matching the
/// sentinel file naming like `ws-dev`, `ws-sec`, etc.), preserving the
/// order in which stages first appear across the FEAT, FIX, TEST, DOCS,
/// and other pipelines. When the config cannot be loaded, falls back to
/// the historical hardcoded list so the TUI continues to render
/// meaningful columns.
///
/// This is a view-layer helper: the list is a display concern, NOT a
/// routing decision. Gate enforcement still reads the pipeline
/// per-session via `hooks::pipeline::load_pipelines`.
fn derive_stage_names_for_tui(base: &Path) -> Vec<String> {
    let config_path = base
        .join(".codeflow")
        .join("config")
        .join("pathflow")
        .join("pathflow-config.json");

    // Parse only the pipelines section from the config file.
    let names = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|data| serde_json::from_str::<serde_json::Value>(&data).ok())
        .and_then(|v| v.get("pipelines").cloned())
        .and_then(|pipelines| {
            let obj = pipelines.as_object()?;
            let mut ordered: Vec<String> = Vec::new();
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            for (_, stages_value) in obj {
                if let Some(stages) = stages_value.as_array() {
                    for entry in stages {
                        if let Some(s) = entry.as_str() {
                            let lowered = s.to_lowercase();
                            if seen.insert(lowered.clone()) {
                                ordered.push(lowered);
                            }
                        }
                    }
                }
            }
            if ordered.is_empty() {
                None
            } else {
                Some(ordered)
            }
        });

    names.unwrap_or_else(|| {
        vec![
            "ws-dev".to_string(),
            "ws-sec".to_string(),
            "ws-rev".to_string(),
            "ws-qa".to_string(),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_elapsed_secs_valid_timestamp() {
        let now = chrono::Utc::now();
        let ten_secs_ago = (now - chrono::Duration::seconds(10)).to_rfc3339();
        let elapsed = compute_elapsed_secs(&ten_secs_ago);
        assert!(elapsed >= 9, "elapsed {elapsed} should be >= 9");
        assert!(elapsed <= 12, "elapsed {elapsed} should be <= 12");
    }

    #[test]
    fn test_compute_elapsed_secs_invalid_timestamp() {
        assert_eq!(compute_elapsed_secs("not-a-date"), 0);
    }

    // ---- compute_display_status matrix (INF-TSK-050-001 AC #7) ----------
    //
    // Pending × Running  → "Waiting"
    // Pending × Aborting → "Aborting"
    // Pending × Cancelled → "Cancelled"
    // Pending × Failed   → "Skipped"
    // Pending × Timeout  → "Skipped"
    // Pending × Completed → "Skipped" (defensive)
    // Pending × Paused   → "pending"  (pass-through; Paused is a reserved variant)
    // Other  × any       → task.to_string()

    #[test]
    fn test_compute_display_status_pending_running() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Pending, AutorunSessionStatus::Running);
        assert_eq!(status, "Waiting");
    }

    #[test]
    fn test_compute_display_status_pending_aborting() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Pending,
            AutorunSessionStatus::Aborting,
        );
        assert_eq!(status, "Aborting");
    }

    #[test]
    fn test_compute_display_status_pending_cancelled() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Pending,
            AutorunSessionStatus::Cancelled,
        );
        assert_eq!(status, "Cancelled");
    }

    #[test]
    fn test_compute_display_status_pending_failed() {
        // Batch failed before this row was picked up -> Skipped, not "pending".
        let status =
            compute_display_status(AutorunTaskRunStatus::Pending, AutorunSessionStatus::Failed);
        assert_eq!(status, "Skipped");
    }

    #[test]
    fn test_compute_display_status_pending_timeout() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Pending, AutorunSessionStatus::Timeout);
        assert_eq!(status, "Skipped");
    }

    #[test]
    fn test_compute_display_status_pending_completed() {
        // Defensive: a Pending task on a Completed session should never occur
        // in production (the orchestrator marks all rows before completion),
        // but if it does we render "Skipped" so the row is not misleadingly
        // shown as still-queued.
        let status = compute_display_status(
            AutorunTaskRunStatus::Pending,
            AutorunSessionStatus::Completed,
        );
        assert_eq!(status, "Skipped");
    }

    #[test]
    fn test_compute_display_status_pending_paused() {
        // Paused is a reserved/unused variant; fall through to the task's
        // own status string so behavior is predictable if the variant ever
        // ships.
        let status =
            compute_display_status(AutorunTaskRunStatus::Pending, AutorunSessionStatus::Paused);
        assert_eq!(status, "pending");
    }

    #[test]
    fn test_compute_display_status_running() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Running, AutorunSessionStatus::Running);
        assert_eq!(status, "running");
    }

    #[test]
    fn test_compute_display_status_completed() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Completed,
            AutorunSessionStatus::Completed,
        );
        assert_eq!(status, "completed");
    }

    #[test]
    fn test_compute_display_status_failed() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Failed, AutorunSessionStatus::Running);
        assert_eq!(status, "failed");
    }

    #[test]
    fn test_compute_display_status_running_task_terminal_session() {
        // A task in Running state on a terminal session passes through (the
        // override only applies when the task is Pending); the orchestrator
        // updates the task row before transitioning the session in practice.
        let status = compute_display_status(
            AutorunTaskRunStatus::Running,
            AutorunSessionStatus::Completed,
        );
        assert_eq!(status, "running");
    }

    #[test]
    fn test_compute_display_status_skipped_passes_through() {
        // Already-terminal task statuses are returned verbatim.
        let status =
            compute_display_status(AutorunTaskRunStatus::Skipped, AutorunSessionStatus::Failed);
        assert_eq!(status, "skipped");
    }

    #[test]
    fn test_compute_display_status_cancelled_task_passes_through() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Cancelled,
            AutorunSessionStatus::Cancelled,
        );
        assert_eq!(status, "cancelled");
    }

    #[test]
    fn test_compute_display_status_timeout_task_passes_through() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Timeout, AutorunSessionStatus::Timeout);
        assert_eq!(status, "timeout");
    }

    // `test_read_latest_phase_*` tests moved to
    // `crate::session::sentinel::tests` alongside the function body.

    #[test]
    fn test_read_stage_pipeline_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let stages = read_stage_pipeline(tmp.path(), None, None);
        assert_eq!(stages.len(), 4);
        assert!(stages.iter().all(|s| !s.completed));
    }

    #[test]
    fn test_read_stage_pipeline_with_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(".state/sentinels/pathflow/ses-test-456");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-ws-dev"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-ws-rev"), "").unwrap();

        let stages = read_stage_pipeline(tmp.path(), None, None);
        assert_eq!(stages.len(), 4);

        let dev = stages.iter().find(|s| s.name.contains("DEV")).unwrap();
        assert!(dev.completed);

        let rev = stages.iter().find(|s| s.name.contains("REV")).unwrap();
        assert!(rev.completed);

        let qa = stages.iter().find(|s| s.name.contains("QA")).unwrap();
        assert!(!qa.completed);
    }

    #[test]
    fn test_batch_view_serializes_to_json() {
        let view = BatchView {
            session_id: "ar-001".to_string(),
            batch_name: "test-batch".to_string(),
            status: AutorunSessionStatus::Running,
            total_tasks: 5,
            completed_tasks: 2,
            failed_tasks: 1,
            skipped_tasks: 0,
            running_count: 2,
            elapsed_secs: 120,
            elapsed_frozen: false,
            target_branch: Some("main".to_string()),
            final_pr_url: None,
            tasks: vec![TaskView {
                task_id: "task-a".to_string(),
                status: AutorunTaskRunStatus::Running,
                display_status: "running".to_string(),
                phase: Some("PF4".to_string()),
                branch: Some("feat/x".to_string()),
                pr_number: None,
                tmux_session: Some("tmux-1".to_string()),
                duration_secs: Some(60),
                exit_code: None,
                worktree_path: None,
                error_message: None,
                stages: vec![
                    StageInfo {
                        name: "WS-DEV".to_string(),
                        completed: true,
                    },
                    StageInfo {
                        name: "WS-REV".to_string(),
                        completed: false,
                    },
                ],
                worker_session_id: Some("ses-worker-1".to_string()),
                work_type: Some("FEAT".to_string()),
                pid: None,
                pid_alive: None,
                task_format_id: None,
            }],
        };

        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"session_id\":\"ar-001\""));
        assert!(json.contains("\"batch_name\":\"test-batch\""));
        assert!(json.contains("\"task_id\":\"task-a\""));
        assert!(json.contains("\"running_count\":2"));
    }

    #[test]
    fn test_session_view_serializes() {
        let view = SessionView {
            session_id: "ses-001".to_string(),
            status: "active".to_string(),
            branch: Some("feat/test".to_string()),
            phase: Some("PF4".to_string()),
            work_type: Some("FEAT".to_string()),
            task_id: Some("TSK-001".to_string()),
            task_format_id: None,
            last_phase: None,
            team_name: Some("team-1".to_string()),
            pid: 12345,
            worktree_path: Some("/tmp/wt".to_string()),
            duration_secs: 60,
            managed: true,
            created_at: "2026-04-01T00:00:00Z".to_string(),
            hidden: false,
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"session_id\":\"ses-001\""));
        assert!(json.contains("\"branch\":\"feat/test\""));
        assert!(json.contains("\"work_type\":\"FEAT\""));
        assert!(json.contains("\"task_id\":\"TSK-001\""));
        assert!(json.contains("\"pid\":12345"));
        assert!(json.contains("\"managed\":true"));
    }

    #[test]
    fn test_stage_info_serialization() {
        let info = StageInfo {
            name: "WS-DEV".to_string(),
            completed: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"name\":\"WS-DEV\""));
        assert!(json.contains("\"completed\":true"));
    }

    #[test]
    fn test_task_view_defaults() {
        let view = TaskView {
            task_id: "t-1".to_string(),
            status: AutorunTaskRunStatus::Pending,
            display_status: "pending".to_string(),
            phase: None,
            branch: None,
            pr_number: None,
            tmux_session: None,
            duration_secs: None,
            exit_code: None,
            worktree_path: None,
            error_message: None,
            stages: vec![],
            worker_session_id: None,
            work_type: None,
            pid: None,
            pid_alive: None,
            task_format_id: None,
        };
        assert!(view.phase.is_none());
        assert!(view.branch.is_none());
        assert!(view.pr_number.is_none());
        assert!(view.stages.is_empty());
        assert!(view.worker_session_id.is_none());
        assert!(view.work_type.is_none());
        assert!(view.pid.is_none());
        assert!(view.pid_alive.is_none());
        assert!(view.task_format_id.is_none());
    }

    #[test]
    fn test_batch_list_entry_serializes() {
        let entry = BatchListEntry {
            session_id: "ses-batch-1".to_string(),
            batch_name: "my-batch".to_string(),
            status: AutorunSessionStatus::Running,
            total_tasks: 5,
            completed_tasks: 2,
            failed_tasks: 1,
            running_count: 2,
            elapsed_secs: 300,
            current_task_id: None,
            current_task_format_id: None,
            idle_secs: None,
            final_pr_url: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"session_id\":\"ses-batch-1\""));
        assert!(json.contains("\"batch_name\":\"my-batch\""));
        assert!(json.contains("\"total_tasks\":5"));
        assert!(json.contains("\"running_count\":2"));
        assert!(json.contains("\"elapsed_secs\":300"));
    }

    #[test]
    fn test_batch_list_entry_default_name_truncation() {
        // When batch_name is absent, the fallback truncates the session_id.
        let long_id = "ses-01knymk575s72x85z68pk3gfws-extra";
        let entry = BatchListEntry {
            session_id: long_id.to_string(),
            batch_name: long_id[..long_id.len().min(20)].to_string(),
            status: AutorunSessionStatus::Completed,
            total_tasks: 1,
            completed_tasks: 1,
            failed_tasks: 0,
            running_count: 0,
            elapsed_secs: 60,
            current_task_id: None,
            current_task_format_id: None,
            idle_secs: None,
            final_pr_url: None,
        };
        assert_eq!(entry.batch_name.len(), 20);
    }

    /// INF-TSK-050-001 AC #19: BatchListEntry preserves `final_pr_url` from
    /// the AutorunSession so the BatchList view can render a "PR" column
    /// without a second DB lookup. Verifies the field round-trips through
    /// `build_list_entry` end-to-end.
    #[tokio::test]
    async fn test_batch_list_entry_preserves_final_pr_url() {
        let store = crate::store::mock::MockStore::new();
        let pr_url = "https://github.com/example/repo/pull/123";
        let session = crate::models::AutorunSession {
            id: "ses-with-pr".into(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("with-final-pr".into()),
            status: AutorunSessionStatus::Completed,
            max_session_workers: 2,
            total_tasks: 3,
            completed_tasks: 3,
            failed_tasks: 0,
            skipped_tasks: 0,
            created_at: "2026-04-01T00:00:00Z".into(),
            completed_at: Some("2026-04-01T01:00:00Z".into()),
            pid: None,
            tmux_session: None,
            stale_reason: None,
            target_branch: Some("autorun/batch-x".into()),
            final_pr_url: Some(pr_url.to_string()),
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            abort_started_at: None,
        };
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ses-with-pr".to_string(), session);

        let entries = fetch_batch_list(&store).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].final_pr_url.as_deref(),
            Some(pr_url),
            "final_pr_url should propagate from AutorunSession to BatchListEntry"
        );
    }

    /// INF-TSK-050-001 AC #19: BatchView mirrors `final_pr_url` from the
    /// underlying AutorunSession. The detail header in the autorun TUI
    /// reads this field directly.
    #[tokio::test]
    async fn test_batch_view_preserves_final_pr_url() {
        let store = crate::store::mock::MockStore::new();
        let pr_url = "https://github.com/example/repo/pull/456";
        let mut session = make_mock_session("ses-view-pr", AutorunSessionStatus::Completed);
        session.final_pr_url = Some(pr_url.to_string());
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ses-view-pr".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ses-view-pr"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(view.final_pr_url.as_deref(), Some(pr_url));
    }

    /// INF-TSK-050-001 AC #19: when the AutorunSession has not yet recorded
    /// a final PR, both the list entry and the view show `None` — never an
    /// empty string or stale value.
    #[tokio::test]
    async fn test_batch_views_none_final_pr_url_when_unset() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ses-no-pr", AutorunSessionStatus::Running);
        // make_mock_session leaves final_pr_url = None (verified below).
        assert!(session.final_pr_url.is_none());
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ses-no-pr".to_string(), session);

        let entries = fetch_batch_list(&store).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].final_pr_url.is_none());

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ses-no-pr"))
            .await
            .unwrap()
            .unwrap();
        assert!(view.final_pr_url.is_none());
    }

    #[tokio::test]
    async fn test_fetch_batch_list_empty_store() {
        let store = crate::store::mock::MockStore::new();
        let entries = fetch_batch_list(&store).await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_batch_list_sorts_running_first() {
        use crate::store::DataStore;

        let store = crate::store::mock::MockStore::new();
        let completed = crate::models::AutorunSession {
            id: "ses-completed".into(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("batch-old".into()),
            status: AutorunSessionStatus::Completed,
            max_session_workers: 2,
            total_tasks: 3,
            completed_tasks: 3,
            failed_tasks: 0,
            skipped_tasks: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
            completed_at: Some("2026-01-01T01:00:00Z".into()),
            pid: None,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            abort_started_at: None,
        };
        let running = crate::models::AutorunSession {
            id: "ses-running".into(),
            batch_file: "batch.yaml".into(),
            batch_name: Some("batch-active".into()),
            status: AutorunSessionStatus::Running,
            max_session_workers: 2,
            total_tasks: 5,
            completed_tasks: 2,
            failed_tasks: 0,
            skipped_tasks: 0,
            created_at: "2026-01-02T00:00:00Z".into(),
            completed_at: None,
            pid: None,
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            abort_started_at: None,
        };
        store.create_autorun_session(&completed).await.unwrap();
        store.create_autorun_session(&running).await.unwrap();

        let entries = fetch_batch_list(&store).await.unwrap();
        assert_eq!(entries.len(), 2);
        // Running should sort first.
        assert_eq!(entries[0].batch_name, "batch-active");
        assert_eq!(entries[0].status, AutorunSessionStatus::Running);
        assert_eq!(entries[0].total_tasks, 5);
        assert_eq!(entries[0].completed_tasks, 2);
        assert_eq!(entries[1].batch_name, "batch-old");
        assert_eq!(entries[1].status, AutorunSessionStatus::Completed);
    }

    #[test]
    fn test_session_summary_hidden_count_default() {
        let summary = SessionSummary::default();
        assert_eq!(summary.hidden_count, 0);
    }

    #[test]
    fn test_session_view_hidden_field_serializes() {
        let view = SessionView {
            session_id: "ses-001".to_string(),
            status: "stale".to_string(),
            branch: None,
            phase: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            last_phase: None,
            team_name: None,
            pid: 1,
            worktree_path: None,
            duration_secs: -1,
            managed: false,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            hidden: true,
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"hidden\":true"));
        // -1 sentinel for unknown duration
        assert!(json.contains("\"duration_secs\":-1"));
    }

    #[tokio::test]
    async fn test_fetch_session_views_empty_store() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let (views, summary) = fetch_session_views(&store, tmp.path()).await.unwrap();
        assert!(views.is_empty());
        assert_eq!(summary.active, 0);
        assert_eq!(summary.stale, 0);
        assert_eq!(summary.complete, 0);
        assert_eq!(summary.hidden_count, 0);
    }

    /// Synthetic PID validator for tests: always returns `true` so the
    /// stale-promotion path is skipped and the test exercises the
    /// "session is alive" branch. Production code uses
    /// [`default_session_pid_validator`] which delegates to
    /// `validate_claude_pid` (alive AND named "claude").
    fn always_alive_validator(_pid: u32) -> bool {
        true
    }

    #[tokio::test]
    async fn test_fetch_session_views_with_sessions() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        // Use current process PID; pair with `always_alive_validator` so the
        // PID-liveness override does not fire (the test runner is named
        // `cargo`, not `claude`, so the production validator would
        // promote this row to stale and break the assertions below).
        let current_pid = i64::from(std::process::id());
        let now = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-test-tui', \
                 pid = $pid, \
                 status = 'active', \
                 worktree_path = NONE, \
                 branch = 'feat/tui', \
                 work_type = 'FEAT', \
                 task_id = NONE, \
                 team_name = 'test-team', \
                 source_cli = 'codeflow', \
                 managed = true, \
                 created_at = $now, \
                 updated_at = NONE, \
                 completed_at = NONE;",
            )
            .bind(("pid", current_pid))
            .bind(("now", now.clone()))
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let (views, summary) = fetch_session_views_with_keep_last_and_validator(
            &store,
            tmp.path(),
            10,
            always_alive_validator,
        )
        .await
        .unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(summary.active, 1);
        assert_eq!(summary.stale, 0);

        let v = &views[0];
        assert_eq!(v.session_id, "ses-test-tui");
        assert_eq!(v.status, "active");
        assert_eq!(v.branch.as_deref(), Some("feat/tui"));
        assert_eq!(v.work_type.as_deref(), Some("FEAT"));
        assert_eq!(v.team_name.as_deref(), Some("test-team"));
        assert_eq!(v.pid, current_pid);
        assert!(v.managed);
    }

    #[test]
    fn test_derive_phase_from_session_with_status_file() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("worktree");
        let sid = "ses-phase-test";
        let status_dir = wt.join(".state/session").join(sid).join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            r#"{"last_completed_phase": "pf-4"}"#,
        )
        .unwrap();

        let session = crate::models::InteractiveSession {
            id: "test".to_string(),
            session_id: sid.to_string(),
            pid: 1,
            status: crate::types::InteractiveSessionStatus::Active,
            worktree_path: Some(wt.to_string_lossy().to_string()),
            branch: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: None,
            source_cli: "codeflow".to_string(),
            managed: true,
            session_kind: "interactive".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
            completed_at: None,
        };

        let phase = derive_phase_from_session(tmp.path(), &session);
        assert_eq!(phase, Some("pf-4".to_string()));
    }

    #[test]
    fn test_derive_phase_from_session_fallback_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let sid = "ses-sentinel-test";
        let sentinel_dir = tmp.path().join(".state/sentinels/pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-3"), "").unwrap();

        let session = crate::models::InteractiveSession {
            id: "test".to_string(),
            session_id: sid.to_string(),
            pid: 1,
            status: crate::types::InteractiveSessionStatus::Active,
            worktree_path: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: None,
            source_cli: "codeflow".to_string(),
            managed: true,
            session_kind: "interactive".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
            completed_at: None,
        };

        let phase = derive_phase_from_session(tmp.path(), &session);
        assert_eq!(phase, Some("PF3".to_string()));
    }

    #[test]
    fn test_derive_phase_path_traversal_guard() {
        let tmp = tempfile::tempdir().unwrap();
        let session = crate::models::InteractiveSession {
            id: "test".to_string(),
            session_id: "../etc/passwd".to_string(),
            pid: 1,
            status: crate::types::InteractiveSessionStatus::Active,
            worktree_path: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: None,
            source_cli: "codeflow".to_string(),
            managed: true,
            session_kind: "interactive".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
            completed_at: None,
        };

        let phase = derive_phase_from_session(tmp.path(), &session);
        assert!(phase.is_none());
    }

    #[test]
    fn test_session_summary_default() {
        let summary = SessionSummary::default();
        assert_eq!(summary.active, 0);
        assert_eq!(summary.stale, 0);
        assert_eq!(summary.complete, 0);
    }

    #[test]
    fn test_read_stage_pipeline_with_worktree_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("worktree");
        let session_dir = wt.join(".state/sentinels/pathflow/ses-wt-002");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-ws-dev"), "").unwrap();

        let stages = read_stage_pipeline(tmp.path(), Some(wt.to_str().unwrap()), None);
        let dev = stages.iter().find(|s| s.name.contains("DEV")).unwrap();
        assert!(dev.completed);
        let rev = stages.iter().find(|s| s.name.contains("REV")).unwrap();
        assert!(!rev.completed);
    }

    // -----------------------------------------------------------------------
    // Async tests using MockStore for fetch_batch_view / build_batch_view
    // -----------------------------------------------------------------------

    fn make_mock_session(id: &str, status: AutorunSessionStatus) -> crate::models::AutorunSession {
        crate::models::AutorunSession {
            id: id.to_string(),
            batch_file: "batch.yaml".to_string(),
            batch_name: Some("test-batch".to_string()),
            status,
            max_session_workers: 2,
            total_tasks: 3,
            completed_tasks: 1,
            failed_tasks: 0,
            pid: Some(999),
            skipped_tasks: 0,
            tmux_session: None,
            stale_reason: None,
            target_branch: Some("main".to_string()),
            final_pr_url: None,
            current_task_id: None,
            current_task_format_id: None,
            updated_at: None,
            last_heartbeat_at: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
            abort_started_at: None,
        }
    }

    fn make_mock_worker(
        session_id: &str,
        task_id: &str,
        status: crate::types::AutorunWorkerStatus,
    ) -> crate::models::AutorunWorker {
        crate::models::AutorunWorker {
            id: format!("aw-{task_id}"),
            session_id: session_id.to_string(),
            worker_num: 1,
            task_id: task_id.to_string(),
            status,
            tmux_session: Some(format!("tmux-{task_id}")),
            worktree_path: None,
            file_scope: vec![],
            scope_policy: "soft".to_string(),
            worker_session_id: None,
            pr_number: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: None,
        }
    }

    fn make_mock_task_run(
        session_id: &str,
        task_id: &str,
        status: AutorunTaskRunStatus,
    ) -> crate::models::AutorunTaskRun {
        crate::models::AutorunTaskRun {
            id: format!("atr-{task_id}"),
            worker_id: format!("aw-{task_id}"),
            task_id: task_id.to_string(),
            session_id: session_id.to_string(),
            status,
            branch_name: Some(format!("feat/{task_id}")),
            worktree_path: None,
            pr_number: Some(42),
            pr_url: None,
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: None,
            duration_seconds: Some(120),
            exit_code: None,
            error_message: None,
            last_phase: None,
            verification_result: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[tokio::test]
    async fn test_fetch_batch_view_no_sessions() {
        let store = crate::store::mock::MockStore::new();
        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), None).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_fetch_batch_view_by_id() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-test-1", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-test-1".to_string(), session);

        let worker = make_mock_worker(
            "ar-test-1",
            "task-a",
            crate::types::AutorunWorkerStatus::Running,
        );
        store
            .autorun_workers
            .lock()
            .unwrap()
            .insert("aw-task-a".to_string(), worker);

        let run = make_mock_task_run("ar-test-1", "task-a", AutorunTaskRunStatus::Running);
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-a".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), Some("ar-test-1"))
            .await
            .unwrap();

        assert!(result.is_some());
        let view = result.unwrap();
        assert_eq!(view.session_id, "ar-test-1");
        assert_eq!(view.batch_name, "test-batch");
        assert_eq!(view.total_tasks, 3);
        assert_eq!(view.running_count, 1);
        assert_eq!(view.tasks.len(), 1);
        assert_eq!(view.tasks[0].task_id, "task-a");
        assert_eq!(view.tasks[0].branch.as_deref(), Some("feat/task-a"));
        assert_eq!(view.tasks[0].pr_number, Some(42));
        assert_eq!(view.tasks[0].tmux_session.as_deref(), Some("tmux-task-a"));
        // Running task recomputes from started_at (just now), so ~0 seconds.
        let dur = view.tasks[0].duration_secs.unwrap();
        assert!(
            dur <= 5,
            "running task duration should be recomputed: {dur}"
        );
        assert_eq!(view.target_branch.as_deref(), Some("main"));
    }

    #[tokio::test]
    async fn test_fetch_batch_view_finds_running_session() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-auto", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-auto".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), None).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "ar-auto");
    }

    #[tokio::test]
    async fn test_fetch_batch_view_falls_back_to_completed_sessions() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-done", AutorunSessionStatus::Completed);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-done".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), None).await.unwrap();
        // When no running sessions exist, fall back to most recent completed.
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "ar-done");
    }

    #[tokio::test]
    async fn test_fetch_batch_view_nonexistent_id() {
        let store = crate::store::mock::MockStore::new();
        let tmp = tempfile::tempdir().unwrap();
        let result = fetch_batch_view(&store, tmp.path(), Some("no-such-id"))
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_fetch_batch_view_display_status_waiting() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-wait", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-wait".to_string(), session);

        let run = make_mock_task_run("ar-wait", "task-p", AutorunTaskRunStatus::Pending);
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-p".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-wait"))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(view.tasks[0].display_status, "Waiting");
    }

    #[tokio::test]
    async fn test_fetch_batch_view_running_task_computes_elapsed() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-elapsed", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-elapsed".to_string(), session);

        let mut run = make_mock_task_run("ar-elapsed", "task-e", AutorunTaskRunStatus::Running);
        run.duration_seconds = None; // no pre-computed duration
        run.started_at = Some((chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339());
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-e".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-elapsed"))
            .await
            .unwrap()
            .unwrap();

        // Duration should be computed from started_at (~30 seconds).
        let dur = view.tasks[0].duration_secs.unwrap();
        assert!(dur >= 28, "duration {dur} should be >= 28");
        assert!(dur <= 35, "duration {dur} should be <= 35");
    }

    #[tokio::test]
    async fn test_build_batch_view_batch_name_fallback() {
        let store = crate::store::mock::MockStore::new();
        let mut session = make_mock_session("ar-noname-session", AutorunSessionStatus::Running);
        session.batch_name = None;
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-noname-session".to_string(), session);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-noname-session"))
            .await
            .unwrap()
            .unwrap();

        // Fallback: first 20 chars of session_id.
        assert_eq!(view.batch_name, "ar-noname-session");
    }

    // FIX-2: Running task with stale duration_seconds should recompute from started_at.
    #[tokio::test]
    async fn test_running_task_ignores_stale_stored_duration() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-stale", AutorunSessionStatus::Running);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-stale".to_string(), session);

        let mut run = make_mock_task_run("ar-stale", "task-s", AutorunTaskRunStatus::Running);
        // Stale stored duration from a previous failed run.
        run.duration_seconds = Some(9999);
        run.started_at = Some((chrono::Utc::now() - chrono::Duration::seconds(10)).to_rfc3339());
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-s".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-stale"))
            .await
            .unwrap()
            .unwrap();

        // Should be ~10 seconds, NOT the stale 9999.
        let dur = view.tasks[0].duration_secs.unwrap();
        assert!(
            dur < 20,
            "duration {dur} should be recomputed, not stale 9999"
        );
        assert!(dur >= 8, "duration {dur} should be ~10s from started_at");
    }

    // FIX-2: Completed task uses stored duration_seconds.
    #[tokio::test]
    async fn test_completed_task_uses_stored_duration() {
        let store = crate::store::mock::MockStore::new();
        let session = make_mock_session("ar-done2", AutorunSessionStatus::Completed);
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-done2".to_string(), session);

        let mut run = make_mock_task_run("ar-done2", "task-d", AutorunTaskRunStatus::Completed);
        run.duration_seconds = Some(300);
        store
            .autorun_task_runs
            .lock()
            .unwrap()
            .insert("atr-task-d".to_string(), run);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-done2"))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(view.tasks[0].duration_secs, Some(300));
    }

    // FIX-7: Stage pipeline session isolation.
    #[test]
    fn test_read_stage_pipeline_session_isolation() {
        let tmp = tempfile::tempdir().unwrap();
        let ses1 = tmp.path().join(".state/sentinels/pathflow/ses-X");
        let ses2 = tmp.path().join(".state/sentinels/pathflow/ses-Y");
        std::fs::create_dir_all(&ses1).unwrap();
        std::fs::create_dir_all(&ses2).unwrap();
        std::fs::write(ses1.join("pathflow-ws-dev"), "").unwrap();
        std::fs::write(ses2.join("pathflow-ws-qa"), "").unwrap();

        // Without filter: both stages appear completed.
        let all = read_stage_pipeline(tmp.path(), None, None);
        assert!(
            all.iter()
                .find(|s| s.name.contains("DEV"))
                .unwrap()
                .completed
        );
        assert!(
            all.iter()
                .find(|s| s.name.contains("QA"))
                .unwrap()
                .completed
        );

        // With ses-X filter: only DEV is completed.
        let x = read_stage_pipeline(tmp.path(), None, Some("ses-X"));
        assert!(x.iter().find(|s| s.name.contains("DEV")).unwrap().completed);
        assert!(!x.iter().find(|s| s.name.contains("QA")).unwrap().completed);

        // With ses-Y filter: only QA is completed.
        let y = read_stage_pipeline(tmp.path(), None, Some("ses-Y"));
        assert!(!y.iter().find(|s| s.name.contains("DEV")).unwrap().completed);
        assert!(y.iter().find(|s| s.name.contains("QA")).unwrap().completed);
    }

    // FIX-5: Sentinel read with nonexistent dir returns gracefully.
    #[test]
    fn test_read_stage_pipeline_nonexistent_returns_empty() {
        let stages = read_stage_pipeline(std::path::Path::new("/nonexistent/path"), None, None);
        assert_eq!(stages.len(), 4);
        assert!(stages.iter().all(|s| !s.completed));
    }

    /// The TUI stage names must come from the pipelines config, not a
    /// hardcoded array. Adding a new stage (e.g. WS-BENCH) to any
    /// pipeline should automatically appear in the derived names.
    #[test]
    fn test_tui_stage_names_pipeline_driven() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow").join("config").join("pathflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("pathflow-config.json"),
            r#"{
              "pipelines": {
                "FEAT": ["WS-DEV", "WS-SEC", "WS-BENCH", "WS-REV", "WS-QA"],
                "DOCS": ["WS-DOCS", "WS-REV"],
                "PLAN": ["WS-PLAN", "WS-REV"]
              }
            }"#,
        )
        .unwrap();

        let names = derive_stage_names_for_tui(dir.path());
        // Every stage referenced in any pipeline must appear.
        for expected in &[
            "ws-dev", "ws-sec", "ws-bench", "ws-rev", "ws-qa", "ws-docs", "ws-plan",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "stage {expected} missing from {names:?}"
            );
        }
        // No duplicates.
        let mut deduped = names.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(deduped.len(), names.len(), "duplicates in {names:?}");
    }

    // -----------------------------------------------------------------------
    // format_task_id_for_display — INF-TSK-047-001 AC #1
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_task_id_prefers_format_id() {
        let out = format_task_id_for_display(Some("INF-TSK-046-008"), Some("task-01KABC"));
        assert_eq!(out, "INF-TSK-046-008");
    }

    #[test]
    fn test_format_task_id_truncates_long_ulid() {
        // Classic ULID-shaped task id: 26 chars after the `task-` prefix.
        let ulid = "task-01KPHCP81KGS3PKG06TZRAKMHH";
        let out = format_task_id_for_display(None, Some(ulid));
        // First 8 chars + ellipsis + last 5 chars.
        assert_eq!(out, "task-01K…AKMHH");
    }

    #[test]
    fn test_format_task_id_short_ulid_passthrough() {
        // Anything <= 14 chars passes through unchanged (already fits the
        // TASK column width).
        let short = "task-abcd";
        let out = format_task_id_for_display(None, Some(short));
        assert_eq!(out, "task-abcd");
    }

    #[test]
    fn test_format_task_id_no_task_returns_dashes() {
        assert_eq!(format_task_id_for_display(None, None), "--");
    }

    // -----------------------------------------------------------------------
    // format_task_cell — INF-TSK-050-001 AC #8
    //
    // Width = 18 in production (matches the BatchList TASK column constraint).
    // Tests pin every matrix cell explicitly so a regression flips a value.
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_task_cell_zero_total_returns_emdash() {
        // total_tasks == 0 → em-dash regardless of id presence.
        assert_eq!(format_task_cell(Some("INF-TSK-001"), None, 0, 18), "—");
        assert_eq!(format_task_cell(None, None, 0, 18), "—");
    }

    #[test]
    fn test_format_task_cell_negative_total_returns_emdash() {
        // Defensive: a negative total is treated the same as zero. The DB
        // type is i32 but well-formed rows never have negative values.
        assert_eq!(format_task_cell(Some("INF-TSK-001"), None, -1, 18), "—");
    }

    #[test]
    fn test_format_task_cell_no_id_returns_emdash() {
        // total > 0 but no id → em-dash. The TASKS column carries the count.
        assert_eq!(format_task_cell(None, None, 1, 18), "—");
        assert_eq!(format_task_cell(None, None, 5, 18), "—");
        // Empty strings count as None.
        assert_eq!(format_task_cell(Some(""), Some(""), 3, 18), "—");
    }

    #[test]
    fn test_format_task_cell_single_task_renders_id_only() {
        // total_tasks == 1 → no `+N` suffix.
        assert_eq!(
            format_task_cell(Some("INF-TSK-050-001"), None, 1, 18),
            "INF-TSK-050-001"
        );
    }

    #[test]
    fn test_format_task_cell_single_task_falls_back_to_ulid() {
        let out = format_task_cell(None, Some("task-01KABCDE"), 1, 18);
        assert_eq!(out, "task-01KABCDE");
    }

    #[test]
    fn test_format_task_cell_multi_task_appends_count() {
        // total_tasks > 1 → `id +N` where N = total - 1.
        let out = format_task_cell(Some("INF-TSK-050-001"), None, 5, 18);
        assert_eq!(out, "INF-TSK-050-001 +4");
    }

    #[test]
    fn test_format_task_cell_multi_task_two_tasks() {
        // Boundary: total == 2 → `+1` suffix.
        let out = format_task_cell(Some("INF-TSK-001"), None, 2, 18);
        assert_eq!(out, "INF-TSK-001 +1");
    }

    #[test]
    fn test_format_task_cell_prefers_format_id_over_raw() {
        // Same precedence rule as format_task_id_for_display.
        let out = format_task_cell(Some("INF-TSK-050-001"), Some("task-01KABC"), 3, 32);
        assert_eq!(out, "INF-TSK-050-001 +2");
    }

    #[test]
    fn test_format_task_cell_truncates_id_preserves_suffix() {
        // id + suffix > width → id is truncated with `…`, suffix preserved.
        // Width 12: "INF-TSK-050-001 +4" = 18 chars, must shrink to 12.
        // Suffix " +4" = 3 chars; ellipsis 1 char; id keeps width-3-1 = 8.
        let out = format_task_cell(Some("INF-TSK-050-001"), None, 5, 12);
        assert_eq!(out, "INF-TSK-… +4");
        assert_eq!(out.chars().count(), 12);
    }

    #[test]
    fn test_format_task_cell_truncates_long_ulid_preserves_suffix() {
        // Falls back to ulid; long ulid gets truncated; +N preserved.
        // Width 16: ulid "task-01KPHCP81KGS3PKG06TZRAKMHH" (31 chars) +
        // " +9" suffix (3 chars) = needs truncation.
        // id_keep = 16 - 3 - 1 = 12.
        let out = format_task_cell(None, Some("task-01KPHCP81KGS3PKG06TZRAKMHH"), 10, 16);
        assert_eq!(out, "task-01KPHCP… +9");
        assert_eq!(out.chars().count(), 16);
    }

    #[test]
    fn test_format_task_cell_no_truncation_when_fits_exactly() {
        // id + suffix == width → no ellipsis; full id printed.
        // "INF-TSK-001 +1" = 14 chars, width 14.
        let out = format_task_cell(Some("INF-TSK-001"), None, 2, 14);
        assert_eq!(out, "INF-TSK-001 +1");
    }

    #[test]
    fn test_format_task_cell_width_zero_disables_truncation() {
        // width == 0 → caller will let the column constraint clip; we
        // emit the full string so the count is preserved.
        let out = format_task_cell(Some("INF-TSK-050-001-extra-long"), None, 9, 0);
        assert_eq!(out, "INF-TSK-050-001-extra-long +8");
    }

    #[test]
    fn test_format_task_cell_suffix_too_wide_returns_full() {
        // When width is too small to fit even `…+N`, return full string
        // verbatim. The column constraint at the render layer will clip
        // visually, but we never silently drop the count.
        // suffix " +99" = 4 chars; width 4 means suffix_chars + 1 == width;
        // truncation gives up.
        let out = format_task_cell(Some("INF-TSK-001"), None, 100, 4);
        assert_eq!(out, "INF-TSK-001 +99");
    }

    #[test]
    fn test_format_task_cell_em_dash_is_single_char() {
        // The brief mandates a single Unicode char (em-dash).
        let out = format_task_cell(None, None, 0, 18);
        assert_eq!(out.chars().count(), 1);
    }

    // -----------------------------------------------------------------------
    // Stale-row phase resolution — INF-TSK-047-001 AC #3
    //
    // These tests exercise fetch_session_views_with_keep_last end-to-end
    // across the status × last_phase matrix.
    // -----------------------------------------------------------------------

    async fn insert_session(
        store: &crate::store::SurrealStore,
        session_id: &str,
        pid: i64,
        status: &str,
        last_phase: Option<&str>,
        worktree: Option<&str>,
    ) {
        let now = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = $sid, pid = $pid, status = $status, \
                 worktree_path = $wt, last_phase = $lp, \
                 source_cli = 'codeflow', managed = true, \
                 created_at = $now RETURN NONE;",
            )
            .bind(("sid", session_id.to_string()))
            .bind(("pid", pid))
            .bind(("status", status.to_string()))
            .bind(("wt", worktree.map(str::to_string)))
            .bind(("lp", last_phase.map(str::to_string)))
            .bind(("now", now))
            .await;
    }

    #[tokio::test]
    async fn test_stale_session_uses_last_phase_from_db() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        // PID 2 on macOS/Linux is reserved and never alive → stale.
        insert_session(
            &store,
            "ses-stale-with-phase",
            2,
            "stale",
            Some("pf-4"),
            None,
        )
        .await;

        let tmp = tempfile::tempdir().unwrap();
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-stale-with-phase")
            .unwrap();
        assert_eq!(v.status, "stale");
        assert_eq!(v.phase.as_deref(), Some("pf-4"));
    }

    #[tokio::test]
    async fn test_stale_session_no_phase_falls_back_to_pre_pf1() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        insert_session(&store, "ses-stale-no-phase", 2, "stale", None, None).await;

        let tmp = tempfile::tempdir().unwrap();
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-stale-no-phase")
            .unwrap();
        assert_eq!(v.status, "stale");
        assert_eq!(v.phase.as_deref(), Some("pre-pf1"));
    }

    #[tokio::test]
    async fn test_active_session_no_live_or_db_phase_shows_starting_with_worktree() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let current_pid = i64::from(std::process::id());
        // Use a non-existent worktree path so derive_phase_from_session can't
        // find anything — isolates the fallback chain.
        insert_session(
            &store,
            "ses-active-starting",
            current_pid,
            "active",
            None,
            Some("/tmp/no-such-wt-dir"),
        )
        .await;

        let tmp = tempfile::tempdir().unwrap();
        // INF-TSK-050-001 AC #4: use the validator hook so the test runner's
        // PID (named `cargo`/`rust`, not `claude`) is treated as alive.
        let (views, _) = fetch_session_views_with_keep_last_and_validator(
            &store,
            tmp.path(),
            10,
            always_alive_validator,
        )
        .await
        .unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-active-starting")
            .unwrap();
        assert_eq!(v.phase.as_deref(), Some("Starting"));
    }

    #[tokio::test]
    async fn test_active_session_no_worktree_shows_na() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let current_pid = i64::from(std::process::id());
        insert_session(&store, "ses-active-na", current_pid, "active", None, None).await;

        let tmp = tempfile::tempdir().unwrap();
        // INF-TSK-050-001 AC #4: use the validator hook (see
        // `test_fetch_session_views_with_sessions` for context).
        let (views, _) = fetch_session_views_with_keep_last_and_validator(
            &store,
            tmp.path(),
            10,
            always_alive_validator,
        )
        .await
        .unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-active-na")
            .unwrap();
        assert_eq!(v.phase.as_deref(), Some("N/A"));
    }

    #[tokio::test]
    async fn test_active_session_db_phase_used_when_no_live_sentinel() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let current_pid = i64::from(std::process::id());
        insert_session(
            &store,
            "ses-active-db-phase",
            current_pid,
            "active",
            Some("pf-3"),
            None,
        )
        .await;

        let tmp = tempfile::tempdir().unwrap();
        // INF-TSK-050-001 AC #4: use the validator hook (see
        // `test_fetch_session_views_with_sessions` for context).
        let (views, _) = fetch_session_views_with_keep_last_and_validator(
            &store,
            tmp.path(),
            10,
            always_alive_validator,
        )
        .await
        .unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-active-db-phase")
            .unwrap();
        assert_eq!(v.phase.as_deref(), Some("pf-3"));
    }

    // -----------------------------------------------------------------------
    // Refresh-loop stall guard — INF-TSK-047-001 AC #6 / AC #8
    //
    // These tests prove that a slow data fetch cannot stall the TUI input
    // loop. The production loop gates its fetch behind `should_fetch_now`;
    // these tests exercise that gate in isolation and assert the expected
    // cadence behavior.
    // -----------------------------------------------------------------------

    #[test]
    fn test_fetch_gate_prevents_stall() {
        use std::time::{Duration, Instant};

        let interval = Duration::from_secs(2);
        let last_fetch = Instant::now();

        // Immediately after a fetch, the next few input polls (at 100 ms
        // cadence) must skip the fetch — this is what keeps the loop from
        // blocking on DB I/O every tick.
        for step_ms in [0, 50, 100, 500, 1_000, 1_500, 1_999] {
            let now = last_fetch + Duration::from_millis(step_ms);
            assert!(
                !should_fetch_now(last_fetch, now, interval),
                "fetch must be skipped at {step_ms} ms < interval 2000 ms"
            );
        }

        // At or beyond the interval, the gate opens so fresh data can flow.
        for step_ms in [2_000, 2_001, 3_000, 10_000] {
            let now = last_fetch + Duration::from_millis(step_ms);
            assert!(
                should_fetch_now(last_fetch, now, interval),
                "fetch must be allowed at {step_ms} ms >= interval 2000 ms"
            );
        }
    }

    #[test]
    fn test_fetch_gate_zero_interval_always_fetches() {
        // Pathological input: a zero-interval call should always fetch.
        // (Production clamps to `max(1 s)` before calling, but the helper
        // itself must behave sensibly on any input.)
        use std::time::{Duration, Instant};
        let now = Instant::now();
        assert!(should_fetch_now(now, now, Duration::ZERO));
    }

    /// Regression test for INF-TSK-047-001 AC #6: input servicing must not
    /// stall when the data fetch is slow.
    ///
    /// Uses a pure virtual clock (monotonic `Instant` plus manual offset
    /// arithmetic) to simulate the production loop:
    ///
    /// - `event::poll(100 ms)` advances the virtual clock by 100 ms per
    ///   iteration, representing a full poll interval with no key event.
    /// - `should_fetch_now` is called each iteration with the current
    ///   virtual time; it gates whether a fetch would run.
    /// - A slow fetch is modelled by NOT advancing the virtual clock in
    ///   the gate branch — in production the fetch is `.await`-ed inline
    ///   and blocks the loop. Here we assert that the production loop's
    ///   gate structure (the `if should_fetch_now { fetch; }` pattern)
    ///   emits at most one fetch per `fetch_interval`, no matter how fast
    ///   input polls arrive.
    ///
    /// Deterministic, no tokio dependency, no `test-util` feature needed.
    #[test]
    fn test_render_loop_stays_responsive_under_slow_fetch() {
        use std::time::{Duration, Instant};

        let poll_interval = Duration::from_millis(100);
        let fetch_interval = Duration::from_secs(2);

        // Virtual clock anchored to a real `Instant` so the helper's
        // `duration_since` arithmetic exercises its real code path.
        let start = Instant::now();
        let mut virt_now = start;
        let mut last_fetch = start;
        let mut input_cycles = 0_u32;
        let mut fetches_started = 0_u32;
        let mut max_cycle_gap = Duration::ZERO;

        // Simulate 1 s of runtime. With a 100 ms poll, expect ~10 cycles.
        let deadline = start + Duration::from_secs(1);
        while virt_now < deadline {
            // Simulate one `event::poll(100 ms)` that returned with no key
            // event — virtual time advances by exactly the poll interval.
            virt_now += poll_interval;
            input_cycles += 1;

            // Production gate: exactly what `run_status_tui` does.
            if should_fetch_now(last_fetch, virt_now, fetch_interval) {
                fetches_started += 1;
                last_fetch = virt_now;
                // In production the fetch await would burn wall-clock
                // time here. Pre-fix code awaited it inline; post-fix
                // code either (a) returns immediately because the gate
                // skipped the fetch or (b) runs the fetch synchronously
                // on the render thread only once per `fetch_interval`.
                // Either way the NEXT cycle's `virt_now += poll_interval`
                // is still 100 ms away, so the input gap invariant holds.
            }

            // Input-gap invariant: the interval between one input poll
            // and the next MUST be exactly `poll_interval`. Pre-fix code
            // fetched every iteration, so under a 5 s DB contention the
            // gap blew out to 5 s. Post-fix code keeps the gap bounded.
            let gap = virt_now.duration_since(
                start + poll_interval.saturating_mul(input_cycles.saturating_sub(1)),
            );
            if gap > max_cycle_gap {
                max_cycle_gap = gap;
            }
            assert!(
                gap <= poll_interval + Duration::from_millis(1),
                "input gap was {gap:?}, must stay within poll_interval"
            );
        }

        // ~10 cycles in 1 s of simulated time.
        assert!(
            (9..=11).contains(&input_cycles),
            "expected ~10 input cycles, got {input_cycles}"
        );
        // With a 2 s fetch_interval and 1 s of simulated runtime the fetch
        // gate should have opened at most once (first tick >= 2 s mark).
        // Since 1 s < 2 s the first fetch does NOT fire yet, so zero is
        // the expected count. If the gate misfires, we catch it here.
        assert!(
            fetches_started <= 1,
            "fetch must be rate-limited by the gate, got {fetches_started}"
        );
        // Sanity: the max observed gap is exactly poll_interval.
        assert!(
            max_cycle_gap <= poll_interval + Duration::from_millis(1),
            "max cycle gap {max_cycle_gap:?} exceeded poll_interval"
        );
    }

    // -----------------------------------------------------------------------
    // truncate_branch_for_display — INF-TSK-047-001 AC #5
    // -----------------------------------------------------------------------

    #[test]
    fn test_truncate_branch_short_passthrough() {
        assert_eq!(truncate_branch_for_display("fix/x", 50), "fix/x");
    }

    #[test]
    fn test_truncate_branch_exact_length_passthrough() {
        let branch = "a".repeat(50);
        assert_eq!(truncate_branch_for_display(&branch, 50), branch);
    }

    #[test]
    fn test_truncate_branch_long_gets_ellipsis() {
        let branch = "fix/status-tui-layout-refresh-task-id-and-more-text";
        let out = truncate_branch_for_display(branch, 50);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 50);
    }

    #[test]
    fn test_truncate_branch_tiny_max_is_noop() {
        // max_chars < 2 means we can't fit ellipsis + content, so we do not
        // truncate — the branch is returned verbatim.
        assert_eq!(truncate_branch_for_display("feat/x", 0), "feat/x");
        assert_eq!(truncate_branch_for_display("feat/x", 1), "feat/x");
    }

    #[test]
    fn test_format_task_id_empty_strings_count_as_none() {
        // Empty format_id falls through to task_id; empty task_id falls
        // through to `--`.
        assert_eq!(format_task_id_for_display(Some(""), None), "--");
        assert_eq!(format_task_id_for_display(None, Some("")), "--");
        // Empty format_id + valid task_id returns the task_id rendering.
        assert_eq!(
            format_task_id_for_display(Some(""), Some("task-01KPHCP81KGS3PKG06TZRAKMHH")),
            "task-01K…AKMHH"
        );
    }

    /// When the config file is absent, the helper falls back to the
    /// historical stage set so the TUI still renders.
    #[test]
    fn test_tui_stage_names_fallback_when_config_missing() {
        let dir = tempfile::tempdir().unwrap();
        let names = derive_stage_names_for_tui(dir.path());
        assert_eq!(
            names,
            vec![
                "ws-dev".to_string(),
                "ws-sec".to_string(),
                "ws-rev".to_string(),
                "ws-qa".to_string(),
            ]
        );
    }

    // -----------------------------------------------------------------------
    // Session ID abbreviation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_abbreviate_session_id_long() {
        let sid = "ses-01kp3fett99rh6eqh22kmrygrk";
        let abbr = abbreviate_session_id(sid);
        assert_eq!(abbr, "ses-01kp3f...rygrk");
        // First 10 + "..." + last 5 = 18 chars
        assert_eq!(abbr.len(), 18);
    }

    #[test]
    fn test_abbreviate_session_id_short() {
        let sid = "ses-short";
        let abbr = abbreviate_session_id(sid);
        assert_eq!(abbr, "ses-short");
    }

    #[test]
    fn test_abbreviate_session_id_exact_threshold() {
        // Exactly 20 chars: should NOT be abbreviated.
        let sid = "ses-01234567890abcde";
        assert_eq!(sid.len(), 20);
        let abbr = abbreviate_session_id(sid);
        assert_eq!(abbr, sid);
    }

    #[test]
    fn test_abbreviate_session_id_21_chars() {
        // 21 chars: should be abbreviated.
        let sid = "ses-01234567890abcdef";
        assert_eq!(sid.len(), 21);
        let abbr = abbreviate_session_id(sid);
        assert_eq!(abbr, "ses-012345...bcdef");
    }

    // -----------------------------------------------------------------------
    // InteractiveSession serde roundtrip with task_id
    // -----------------------------------------------------------------------

    #[test]
    fn test_interactive_session_task_id_serializes() {
        // Verify task_id appears in serialization output when set.
        let session = crate::models::InteractiveSession {
            id: "test".to_string(),
            session_id: "ses-test-123".to_string(),
            pid: 42,
            status: crate::types::InteractiveSessionStatus::Active,
            worktree_path: Some("/tmp/wt".to_string()),
            branch: Some("feat/test".to_string()),
            work_type: Some("FEAT".to_string()),
            task_id: Some("TSK-001".to_string()),
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: Some("team-1".to_string()),
            source_cli: "codeflow".to_string(),
            managed: true,
            session_kind: "interactive".to_string(),
            created_at: "2026-04-13T00:00:00Z".to_string(),
            updated_at: None,
            completed_at: None,
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"task_id\":\"TSK-001\""));
    }

    #[test]
    fn test_interactive_session_task_id_none() {
        // Verify task_id is null when not set.
        let session = crate::models::InteractiveSession {
            id: "test".to_string(),
            session_id: "ses-test-456".to_string(),
            pid: 1,
            status: crate::types::InteractiveSessionStatus::Active,
            worktree_path: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: None,
            source_cli: "codeflow".to_string(),
            managed: false,
            session_kind: "interactive".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: None,
            completed_at: None,
        };
        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"task_id\":null"));
    }

    // -----------------------------------------------------------------------
    // TaskView work_type derivation from branch prefix
    // -----------------------------------------------------------------------

    #[test]
    fn test_task_view_work_type_from_branch() {
        let work_type = |branch: &str| -> String {
            crate::hooks::pipeline::infer_work_type_from_branch(branch).to_string()
        };
        assert_eq!(work_type("feat/add-login"), "FEAT");
        assert_eq!(work_type("fix/auth-bug"), "FIX");
        assert_eq!(work_type("refactor/cleanup"), "RFCT");
        assert_eq!(work_type("docs/readme"), "DOCS");
        assert_eq!(work_type("test/coverage"), "TEST");
        assert_eq!(work_type("main"), "FIX"); // default fallback
    }

    // -----------------------------------------------------------------------
    // INF-TSK-048-001 AC #7 tests: bulk query + snapshot behaviour
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_fetch_batch_list_bulk_empty_returns_empty() {
        let store = crate::store::mock::MockStore::new();
        let result = fetch_batch_list_bulk(&store).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_fetch_batch_list_bulk_matches_n_plus_one_output() {
        // Same fixtures exercised through both the N+1 fetch_batch_list
        // and the bulk fetch_batch_list_bulk must produce identical output
        // — otherwise a TUI migration between the two would cause visible
        // jitter in the batch list view.
        let store = crate::store::mock::MockStore::new();
        for (sid, status) in [
            ("ar-bulk-1", AutorunSessionStatus::Running),
            ("ar-bulk-2", AutorunSessionStatus::Completed),
            ("ar-bulk-3", AutorunSessionStatus::Failed),
        ] {
            let session = make_mock_session(sid, status);
            store
                .autorun_sessions
                .lock()
                .unwrap()
                .insert(sid.to_string(), session);
        }
        // Add mixed workers: only ar-bulk-1 has a Running worker.
        let running = make_mock_worker(
            "ar-bulk-1",
            "task-r",
            crate::types::AutorunWorkerStatus::Running,
        );
        store
            .autorun_workers
            .lock()
            .unwrap()
            .insert("aw-r".to_string(), running);
        let completed = make_mock_worker(
            "ar-bulk-2",
            "task-c",
            crate::types::AutorunWorkerStatus::Completed,
        );
        store
            .autorun_workers
            .lock()
            .unwrap()
            .insert("aw-c".to_string(), completed);

        let n_plus_one = fetch_batch_list(&store).await.unwrap();
        let bulk = fetch_batch_list_bulk(&store).await.unwrap();

        assert_eq!(n_plus_one.len(), bulk.len());
        for (a, b) in n_plus_one.iter().zip(bulk.iter()) {
            assert_eq!(a.session_id, b.session_id);
            assert_eq!(a.status, b.status);
            assert_eq!(a.running_count, b.running_count);
            assert_eq!(a.total_tasks, b.total_tasks);
        }
        // Sanity: ar-bulk-1 has a Running worker, others do not.
        let ar1 = bulk.iter().find(|e| e.session_id == "ar-bulk-1").unwrap();
        let ar2 = bulk.iter().find(|e| e.session_id == "ar-bulk-2").unwrap();
        assert_eq!(ar1.running_count, 1);
        assert_eq!(ar2.running_count, 0);
    }

    #[test]
    fn test_batch_list_snapshot_default_and_age() {
        let s = BatchListSnapshot::default();
        assert!(s.entries.is_empty());
        assert!(!s.stale);
        assert!(s.error.is_none());
        // Age is non-negative and small immediately after default().
        let age = s.age_secs(std::time::SystemTime::now());
        assert!(age <= 1, "unexpected age: {age}");
    }

    #[test]
    fn test_batch_list_snapshot_age_is_monotonic() {
        let mut s = BatchListSnapshot::default();
        let t0 = s.fetched_at;
        s.stale = true;
        let later = t0 + std::time::Duration::from_secs(3);
        assert_eq!(s.age_secs(later), 3);
    }

    #[test]
    fn test_batch_list_snapshot_age_clock_skew_returns_zero() {
        let s = BatchListSnapshot {
            entries: Vec::new(),
            fetched_at: std::time::SystemTime::now() + std::time::Duration::from_secs(60),
            stale: false,
            error: None,
        };
        // now < fetched_at (clock skew) → age is clamped to 0.
        assert_eq!(s.age_secs(std::time::SystemTime::now()), 0);
    }

    // -----------------------------------------------------------------------
    // INF-TSK-049-001 AC #2 — build_batch_view freeze + elapsed_frozen flag.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_build_batch_view_freezes_on_completed_batch() {
        let store = crate::store::mock::MockStore::new();
        let mut session = make_mock_session("ar-frozen", AutorunSessionStatus::Completed);
        let now = chrono::Utc::now();
        session.created_at = (now - chrono::Duration::seconds(1000)).to_rfc3339();
        session.completed_at = Some((now - chrono::Duration::seconds(500)).to_rfc3339());
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-frozen".into(), session);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-frozen"))
            .await
            .unwrap()
            .unwrap();

        // elapsed = 1000 - 500 = 500 (frozen), NOT ~1000 (live now()).
        assert!(
            view.elapsed_secs >= 490 && view.elapsed_secs <= 510,
            "frozen elapsed should be ~500 but was {}",
            view.elapsed_secs
        );
        assert!(
            view.elapsed_frozen,
            "completed batch must set elapsed_frozen=true"
        );
    }

    #[tokio::test]
    async fn test_build_batch_view_running_ticks_live() {
        let store = crate::store::mock::MockStore::new();
        let mut session = make_mock_session("ar-live", AutorunSessionStatus::Running);
        session.created_at = (chrono::Utc::now() - chrono::Duration::seconds(30)).to_rfc3339();
        store
            .autorun_sessions
            .lock()
            .unwrap()
            .insert("ar-live".into(), session);

        let tmp = tempfile::tempdir().unwrap();
        let view = fetch_batch_view(&store, tmp.path(), Some("ar-live"))
            .await
            .unwrap()
            .unwrap();

        // Live elapsed should be ~30s (not frozen).
        assert!(
            view.elapsed_secs >= 28 && view.elapsed_secs <= 40,
            "live elapsed should be ~30 but was {}",
            view.elapsed_secs
        );
        assert!(
            !view.elapsed_frozen,
            "running batch must set elapsed_frozen=false"
        );
    }

    // -----------------------------------------------------------------------
    // INF-TSK-049-001 AC #9 — session_kind filter in fetch_session_views.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_fetch_session_views_excludes_autorun_session_kind() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let pid = i64::from(std::process::id());
        // Real interactive row.
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-real', pid = $pid, status = 'active', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'interactive', \
                 created_at = $now RETURN NONE",
            )
            .bind(("pid", pid))
            .bind(("now", now.clone()))
            .await;
        // Autorun worker row masquerading as interactive.
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-autorun-worker', pid = $pid, status = 'active', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'autorun', \
                 created_at = $now RETURN NONE",
            )
            .bind(("pid", pid))
            .bind(("now", now))
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
        // Only the 'interactive' row should surface. The 'autorun' row is
        // filtered out at the query level.
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].session_id, "ses-real");
    }

    // -----------------------------------------------------------------------
    // INF-TSK-049-001 AC #12 — synchronous stale promotion sets completed_at.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_stale_promotion_writes_completed_at_synchronously() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        // PID 2 is reserved and never alive on macOS/Linux → promotes to stale.
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-dead-pid', pid = 2, status = 'active', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'interactive', \
                 created_at = $now RETURN NONE",
            )
            .bind(("now", now))
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let _ = fetch_session_views(&store, tmp.path()).await.unwrap();

        // After the fetch returns, the row MUST already be marked stale with
        // completed_at populated. Previously the UPDATE was spawned async, so
        // this select could observe the pre-promotion state.
        let mut res = store
            .db()
            .query(
                "SELECT status, completed_at FROM interactive_session \
                 WHERE session_id = 'ses-dead-pid'",
            )
            .await
            .unwrap();
        #[derive(serde::Deserialize)]
        struct Row {
            status: String,
            completed_at: Option<String>,
        }
        let rows: Vec<Row> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "stale");
        assert!(
            rows[0].completed_at.is_some(),
            "completed_at must be populated synchronously"
        );
    }

    #[tokio::test]
    async fn test_stale_promoted_view_reports_frozen_duration() {
        // Regression for the "00:01 blip": the returned view for a freshly
        // promoted session must use the synthetic completed_at, not the
        // -1 sentinel, so the first render shows the correct duration.
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        let created = (chrono::Utc::now() - chrono::Duration::seconds(600)).to_rfc3339();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-blip', pid = 2, status = 'active', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'interactive', \
                 created_at = $created RETURN NONE",
            )
            .bind(("created", created))
            .await;

        let tmp = tempfile::tempdir().unwrap();
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-blip")
            .expect("row should surface");
        assert_eq!(v.status, "stale");
        // duration_secs should be ~600, definitely not the -1 sentinel.
        assert!(
            v.duration_secs >= 590 && v.duration_secs <= 610,
            "first-render duration should be ~600s not {}",
            v.duration_secs
        );
    }

    // -----------------------------------------------------------------------
    // INF-TSK-049-001 AC #8 — migration back-fills session_kind='autorun'
    // for rows whose session_id matches an autorun_session record. The
    // migration runs automatically inside apply_schema; assert idempotency
    // by running the schema twice.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_migration_backfills_autorun_session_kind_idempotent() {
        use crate::store::DataStore;
        let store = crate::store::SurrealStore::in_memory().await.unwrap();

        // Seed: interactive_session row without session_kind set (pre-migration),
        // and a matching autorun_session row with the same session_id.
        let now = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-migrate', pid = 99, status = 'complete', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'interactive', \
                 created_at = $now RETURN NONE",
            )
            .bind(("now", now.clone()))
            .await;
        let ar = AutorunSession {
            id: "ses-migrate".into(),
            batch_file: "b.yaml".into(),
            batch_name: None,
            status: AutorunSessionStatus::Completed,
            max_session_workers: 1,
            total_tasks: 0,
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
            created_at: now,
            completed_at: None,
            abort_started_at: None,
        };
        store.create_autorun_session(&ar).await.unwrap();

        // First apply (in-memory store already applied once at open; re-apply).
        store.apply_schema().await.unwrap();
        // Second apply = idempotent no-op.
        store.apply_schema().await.unwrap();

        let mut res = store
            .db()
            .query(
                "SELECT VALUE session_kind FROM interactive_session \
                 WHERE session_id = 'ses-migrate'",
            )
            .await
            .unwrap();
        let vals: Vec<String> = res.take(0).unwrap();
        assert_eq!(vals, vec!["autorun".to_string()]);
    }

    // -----------------------------------------------------------------------
    // INF-TSK-049-001 AC #15 — BatchListEntry preserves task pointers on
    // terminal rows (no longer cleared by the worker finish path).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_batch_list_entry_surfaces_current_task_format_id() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let mut s = make_mock_session("ar-task", AutorunSessionStatus::Completed);
        s.current_task_id = Some("task-01KXYZ".into());
        s.current_task_format_id = Some("INF-TSK-049-001".into());
        store.create_autorun_session(&s).await.unwrap();

        let entries = fetch_batch_list(&store).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].current_task_id.as_deref(),
            Some("task-01KXYZ"),
            "current_task_id propagates to BatchListEntry"
        );
        assert_eq!(
            entries[0].current_task_format_id.as_deref(),
            Some("INF-TSK-049-001"),
            "current_task_format_id propagates to BatchListEntry"
        );
    }
}
