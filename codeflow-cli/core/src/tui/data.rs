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
    pub elapsed_secs: i64,
    pub target_branch: Option<String>,
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
    pub elapsed_secs: i64,
}

// ---------------------------------------------------------------------------
// Data fetching
// ---------------------------------------------------------------------------

/// Fetch a list of all autorun batch sessions for the batch list TUI view.
///
/// Returns one `BatchListEntry` per autorun session, sorted with
/// running/aborting first, then by created_at DESC.
///
/// # Errors
///
/// Returns an error if the store cannot be queried.
pub async fn fetch_batch_list<S: DataStore>(store: &S) -> Result<Vec<BatchListEntry>, DbError> {
    let sessions = store
        .list_autorun_sessions(crate::models::AutorunSessionFilter {
            all: true,
            ..Default::default()
        })
        .await?;

    let mut entries = Vec::with_capacity(sessions.len());
    for s in &sessions {
        let workers = store.list_autorun_workers(&s.id).await?;
        let running_count = workers
            .iter()
            .filter(|w| w.status == crate::types::AutorunWorkerStatus::Running)
            .count();
        entries.push(BatchListEntry {
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
            elapsed_secs: compute_elapsed_secs(&s.created_at),
        });
    }

    // Sort: running/aborting first, then by session_id (proxy for created_at DESC
    // since list_autorun_sessions already returns sorted).
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

    Ok(entries)
}

/// Fetch a `BatchView` for the given session, or the most recent active session.
///
/// When no running/aborting session exists, falls back to the most recent
/// completed/failed batch so the TUI always has something to display.
///
/// # Errors
///
/// Returns an error if the store cannot be queried or no matching session is found.
pub async fn fetch_batch_view<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: Option<&str>,
) -> Result<Option<BatchView>, DbError> {
    let session = match session_id {
        Some(sid) => store.get_autorun_session(sid).await?,
        None => {
            let sessions = store
                .list_autorun_sessions(crate::models::AutorunSessionFilter::default())
                .await?;
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

    let elapsed_secs = compute_elapsed_secs(&session.created_at);

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
        target_branch: session.target_branch.clone(),
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
    let sessions: Vec<crate::models::InteractiveSession> = store
        .db()
        .query("SELECT * FROM interactive_session ORDER BY created_at DESC")
        .await
        .map_err(|e| DbError::Query(e.to_string()))?
        .take(0)
        .map_err(|e| DbError::Query(e.to_string()))?;

    let mut views = Vec::with_capacity(sessions.len());
    let mut summary = SessionSummary::default();

    for s in &sessions {
        // PID liveness override: detect dead sessions still marked "active".
        let effective_status = if s.status.to_string() == "active" {
            let pid_u32 = u32::try_from(s.pid).unwrap_or(0);
            if pid_u32 > 0 && !crate::session::process::is_process_alive(pid_u32) {
                // Transition to stale in DB (best-effort, don't block TUI on failure).
                let _ = store
                    .db()
                    .query(
                        "UPDATE interactive_session SET status = 'stale', \
                         completed_at = time::now(), updated_at = time::now() \
                         WHERE session_id = $sid AND status = 'active'",
                    )
                    .bind(("sid", s.session_id.clone()))
                    .await;
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

        // Duration freeze: use completed_at or updated_at for non-active sessions.
        // Use -1 as sentinel for "unknown duration" when a stale session has
        // no completed_at (renders as "--" in both TUI and text mode).
        let elapsed = if effective_status == "active" {
            compute_elapsed_secs(&s.created_at)
        } else if let Some(ref completed) = s.completed_at {
            compute_elapsed_between(&s.created_at, completed)
        } else if let Some(ref updated) = s.updated_at {
            compute_elapsed_between(&s.created_at, updated)
        } else {
            // Stale/complete session with no timestamp endpoint -- unknown duration.
            -1
        };

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
// Helpers
// ---------------------------------------------------------------------------

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

/// Compute duration between two RFC 3339 timestamps.
///
/// Returns 0 if either timestamp fails to parse.
fn compute_elapsed_between(start: &str, end: &str) -> i64 {
    let start_dt = chrono::DateTime::parse_from_rfc3339(start);
    let end_dt = chrono::DateTime::parse_from_rfc3339(end);
    match (start_dt, end_dt) {
        (Ok(s), Ok(e)) => e.signed_duration_since(s).num_seconds().max(0),
        _ => 0,
    }
}

fn compute_display_status(
    status: AutorunTaskRunStatus,
    session_status: AutorunSessionStatus,
) -> String {
    if matches!(status, AutorunTaskRunStatus::Pending)
        && matches!(session_status, AutorunSessionStatus::Running)
    {
        "Waiting".to_string()
    } else {
        status.to_string()
    }
}

/// Read the latest PathFlow phase from sentinel files.
///
/// When `session_id` is provided, only scans that session's sentinel subdir.
/// Otherwise scans all session subdirs (returns the highest phase across all).
pub fn read_latest_phase(
    project_dir: &Path,
    worktree_path: Option<&str>,
    session_id: Option<&str>,
) -> Option<String> {
    let base = worktree_path.map_or_else(|| project_dir.to_path_buf(), std::path::PathBuf::from);

    let sentinel_dir = base.join(".state/sentinels/pathflow");
    let entries = match std::fs::read_dir(&sentinel_dir) {
        Ok(e) => e,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            eprintln!(
                "warn: failed to read sentinel dir {}: {e}",
                sentinel_dir.display()
            );
            return None;
        }
    };

    // Look for session directories, then find the latest pf-N sentinel.
    let mut latest_phase: Option<u8> = None;
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
                if let Some(rest) = name.strip_prefix("pathflow-pf-") {
                    if let Ok(n) = rest.parse::<u8>() {
                        latest_phase = Some(latest_phase.map_or(n, |cur| cur.max(n)));
                    }
                }
            }
        }
    }

    latest_phase.map(|n| format!("PF{n}"))
}

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

    #[test]
    fn test_compute_display_status_pending_running() {
        let status =
            compute_display_status(AutorunTaskRunStatus::Pending, AutorunSessionStatus::Running);
        assert_eq!(status, "Waiting");
    }

    #[test]
    fn test_compute_display_status_pending_completed() {
        let status = compute_display_status(
            AutorunTaskRunStatus::Pending,
            AutorunSessionStatus::Completed,
        );
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
    fn test_read_latest_phase_no_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = read_latest_phase(tmp.path(), None, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_latest_phase_with_sentinels() {
        let tmp = tempfile::tempdir().unwrap();
        let session_dir = tmp.path().join(".state/sentinels/pathflow/ses-test-123");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-pf-3"), "").unwrap();

        let result = read_latest_phase(tmp.path(), None, None);
        assert_eq!(result, Some("PF3".to_string()));
    }

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
            target_branch: Some("main".to_string()),
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
        };
        assert!(view.phase.is_none());
        assert!(view.branch.is_none());
        assert!(view.pr_number.is_none());
        assert!(view.stages.is_empty());
        assert!(view.worker_session_id.is_none());
        assert!(view.work_type.is_none());
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
        };
        assert_eq!(entry.batch_name.len(), 20);
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

    #[tokio::test]
    async fn test_fetch_session_views_with_sessions() {
        let store = crate::store::SurrealStore::in_memory().await.unwrap();
        // Use current process PID so PID liveness check considers it alive.
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
        let (views, summary) = fetch_session_views(&store, tmp.path()).await.unwrap();
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
    fn test_read_latest_phase_with_worktree_path() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("worktree");
        let session_dir = wt.join(".state/sentinels/pathflow/ses-wt-001");
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(session_dir.join("pathflow-pf-4"), "").unwrap();

        let result = read_latest_phase(tmp.path(), Some(wt.to_str().unwrap()), None);
        assert_eq!(result, Some("PF4".to_string()));
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
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
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

    // FIX-7: Multi-session sentinel isolation — with session_id filter.
    #[test]
    fn test_read_latest_phase_session_isolation() {
        let tmp = tempfile::tempdir().unwrap();
        // Create two session sentinel dirs.
        let ses1 = tmp.path().join(".state/sentinels/pathflow/ses-AAA");
        let ses2 = tmp.path().join(".state/sentinels/pathflow/ses-BBB");
        std::fs::create_dir_all(&ses1).unwrap();
        std::fs::create_dir_all(&ses2).unwrap();
        std::fs::write(ses1.join("pathflow-pf-3"), "").unwrap();
        std::fs::write(ses2.join("pathflow-pf-6"), "").unwrap();

        // Without filter: returns highest across all (PF6).
        let all = read_latest_phase(tmp.path(), None, None);
        assert_eq!(all, Some("PF6".to_string()));

        // With filter: returns only target session's data.
        let ses1_only = read_latest_phase(tmp.path(), None, Some("ses-AAA"));
        assert_eq!(ses1_only, Some("PF3".to_string()));

        let ses2_only = read_latest_phase(tmp.path(), None, Some("ses-BBB"));
        assert_eq!(ses2_only, Some("PF6".to_string()));

        // Nonexistent session returns None.
        let none = read_latest_phase(tmp.path(), None, Some("ses-ZZZ"));
        assert!(none.is_none());
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
    fn test_read_latest_phase_nonexistent_returns_none() {
        let result = read_latest_phase(std::path::Path::new("/nonexistent/path"), None, None);
        assert!(result.is_none());
    }

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
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
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
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
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
        let (views, _) = fetch_session_views(&store, tmp.path()).await.unwrap();
        let v = views
            .iter()
            .find(|v| v.session_id == "ses-active-db-phase")
            .unwrap();
        assert_eq!(v.phase.as_deref(), Some("pf-3"));
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
}
