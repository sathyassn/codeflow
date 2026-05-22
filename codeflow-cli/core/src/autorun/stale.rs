//! Stale autorun session detection and cleanup.
//!
//! Provides three-layer liveness detection (PID, tmux, heartbeat) and
//! cleanup logic for orphaned sessions and their resources.

use std::path::Path;

use crate::error::AutorunError;
use crate::models::{
    AutorunSession, AutorunSessionUpdate, AutorunTaskRunUpdate, AutorunWorkerUpdate,
};
use crate::store::DataStore;
use crate::types::{AutorunSessionStatus, AutorunTaskRunStatus, AutorunWorkerStatus};

/// Information about a potentially stale or abort-timed-out session.
///
/// INF-TSK-050-003 AC-09: removed `heartbeat_alive` and `heartbeat_age_secs`
/// fields. Heartbeat-based liveness is replaced by the canonical
/// `session::liveness::is_session_alive` chokepoint (PID + name validation).
#[derive(Debug)]
pub struct StaleSessionInfo {
    pub session: AutorunSession,
    pub pid_alive: bool,
    pub tmux_alive: Option<bool>,
    pub orphan_worker_count: usize,
    pub live_worker_count: usize,
    /// INF-TSK-050-001 AC #2: age (seconds) since this session entered the
    /// `Aborting` state. `None` when the session is not `Aborting` or when
    /// no anchor timestamp (`abort_started_at`, falling back to
    /// `updated_at`) is available. The reaper marks a session stuck if
    /// this exceeds `config.autorun.abort_timeout_secs`.
    pub abort_age_secs: Option<u64>,
}

/// Report of cleanup actions taken on a stale session.
#[derive(Debug, Default)]
pub struct CleanupReport {
    pub session_id: String,
    pub workers_killed: usize,
    pub workers_already_dead: usize,
    pub worktrees_removed: usize,
    pub claims_released: usize,
    pub prs_closed: usize,
    pub task_runs_failed: usize,
    pub task_runs_skipped: usize,
}

/// Summary of a sweep across all stale sessions.
#[derive(Debug, Default)]
pub struct SweepSummary {
    pub sessions_cleaned: usize,
    pub sessions_skipped: usize,
    pub errors: Vec<(String, String)>,
}

/// Check if a process with the given PID is alive.
///
/// INF-TSK-050-001 AC #4: this is the legacy bare-PID liveness probe. It's
/// retained for callers that genuinely don't care about the process name
/// (e.g. signal-only health checks). The autorun reaper paths use
/// [`is_session_pid_alive`] instead, which delegates to
/// `validate_orchestrator_pid` (INF-TSK-024-053 AC-5 — was previously
/// `validate_claude_pid`, but `autorun_session.pid` is the orchestrator,
/// not Claude, so the name guard always tripped).
///
/// Fail-secure: returns `false` for non-positive PIDs.
#[must_use]
pub fn check_pid_alive(pid: i64) -> bool {
    #[allow(clippy::cast_possible_truncation)]
    let pid_i32 = pid as i32;
    if pid_i32 <= 0 {
        return false;
    }
    // SAFETY: kill(pid, 0) is a standard POSIX liveness check that sends no
    // actual signal. It returns 0 if the process exists and we have permission
    // to signal it, or -1 with ESRCH if the process does not exist.
    let ret = unsafe { libc::kill(pid_i32, 0) };
    if ret == 0 {
        return true;
    }
    // EPERM means process exists but is owned by another user -- still alive.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Validate that an autorun session's PID (the orchestrator process) is
/// alive. Returns `true` only when the kernel reports the process alive.
///
/// INF-TSK-024-053 AC-5: this function validates `autorun_session.pid`,
/// which is the **orchestrator** PID (`codeflow autorun run`) — written
/// from `std::process::id()` in `orchestrator.rs:189`. It is NEVER a
/// Claude Code PID. The validator therefore uses
/// `session::liveness::validate_orchestrator_pid` (alive-only) rather
/// than `validate_claude_pid` (alive AND named "claude"), which would
/// always return `false` for the orchestrator and falsely mark every
/// live autorun session stuck.
///
/// Fail-secure invariants (cf-security flagged these as load-bearing):
///
/// - `None` PID → `false` (no recorded owner means we can't prove
///   liveness; treat as dead so the reaper proceeds).
/// - PID `<= 0` → `false` (kernel/invalid PIDs are never live owners).
/// - PID `0` after `i64 → u32` conversion → `false`.
/// - Cannot determine liveness (kill -0 failed, not EPERM) → `false`.
///
/// PID-reuse race note: in the very narrow window between orchestrator
/// crash and PID reuse by an unrelated process, this function will
/// return `true` until the next sweep. Acceptable here because (a) the
/// orchestrator's session row stores `tmux_session` which is also
/// checked, and (b) the reaper marks stuck on `!pid_alive ||
/// tmux_alive == Some(false)` — so a stuck-but-reused PID still triggers
/// the tmux-side signal.
#[must_use]
pub fn is_session_pid_alive(pid: Option<i64>) -> bool {
    is_session_pid_alive_with(pid, default_pid_validator)
}

/// INF-TSK-024-053 AC-5: the autorun reaper validates the
/// `autorun_session.pid` column, which is the **orchestrator process**
/// PID (`codeflow autorun run` — written by `orchestrator.rs:189` from
/// `std::process::id()`). The orchestrator is NEVER named "claude", so
/// the prior delegation to `session::liveness::default_pid_validator`
/// (which calls `validate_claude_pid`, requiring "claude" in the process
/// name) always returned false → every autorun session was marked stuck
/// the moment the sweep ran. Switching to `validate_orchestrator_pid`
/// (alive-only PID check, no name requirement) fixes the false-stuck.
///
/// Historical context (now obsolete, kept for reviewer orientation):
/// INF-TSK-024-051 Phase 7-rework (AC #6) shared the "alive AND named
/// claude" semantic between the canonical chokepoint and the autorun
/// reaper, on the assumption that both validated a Claude lead PID.
/// That assumption was wrong here — the autorun reaper validates an
/// orchestrator PID, not a Claude PID. The chokepoint's `claude` name
/// check is still correct for the lead-PID resolution path.
fn default_pid_validator(pid: u32) -> bool {
    crate::session::liveness::validate_orchestrator_pid(pid)
}

/// Internal hook for [`is_session_pid_alive`] that accepts a custom PID
/// validator. The validator is called with a non-zero `u32` PID and
/// returns whether the process should count as "live owner of an
/// autorun session". Production code uses [`default_pid_validator`];
/// unit tests use synthetic predicates so they don't depend on the
/// presence of a real `claude` ancestor in the test runner.
///
/// This function is the autorun-side adapter for the
/// `autorun_session.pid` column (which is `Option<i64>`-shaped because
/// the schema column is nullable) — it cannot route through the
/// session-id-driven chokepoint (`is_session_alive`) directly because
/// the autorun reaper already has the validated PID on hand and would
/// pay an unnecessary file read for the same answer.
#[must_use]
pub fn is_session_pid_alive_with(pid: Option<i64>, validator: fn(u32) -> bool) -> bool {
    let Some(pid) = pid else {
        return false;
    };
    // i64 → u32: PIDs on every supported platform fit in u32 (Linux
    // PID_MAX_LIMIT is 2^22, macOS 2^15..2^31). Any out-of-range or
    // negative value is treated as invalid (fail-secure → dead).
    let Ok(pid_u32) = u32::try_from(pid) else {
        return false;
    };
    if pid_u32 == 0 {
        return false;
    }
    validator(pid_u32)
}

/// Check if a tmux session with the given name is alive.
#[must_use]
pub fn check_tmux_alive(name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// INF-TSK-050-003 AC-09: deleted `check_heartbeat_alive` and
// `is_session_stale`. The canonical liveness chokepoint
// (`session::liveness::is_session_alive`) is now the single source of
// truth for "is this session alive?". Heartbeat files were a Signal-2
// data source the chokepoint never consulted; removing the writer
// (orchestrator.rs) and these readers eliminates the divergent signal
// path that PR #309/#310/#311 progressively peeled away.
//
// Replacement guidance for callers:
//
// - Was: `is_session_stale(pid_alive, tmux_alive, heartbeat_alive)` returns true
//   on stale.
// - Now: `!session::liveness::is_session_alive(project_dir, &session.id).is_alive()`
//   returns true on dead/unknown. For dead-only (treating Unknown as alive),
//   match on `SessionLiveness::Dead`.
//
// `detect_stuck_sessions_with` (below) folds the residual two-signal
// stuck check (`!pid_alive || tmux_alive == Some(false)`) inline.

/// Compute the age (seconds) since an `Aborting` session entered abort.
///
/// INF-TSK-050-001 AC #2 helper. Prefers `abort_started_at`; falls back to
/// `updated_at` for legacy rows that pre-date the column. Returns `None`
/// when neither anchor exists or parsing fails — caller must treat that
/// as "not yet stuck" so a freshly-aborting row with a missing anchor
/// is never reaped.
#[must_use]
pub fn compute_abort_age_secs(
    session: &AutorunSession,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<u64> {
    let anchor = session
        .abort_started_at
        .as_deref()
        .or(session.updated_at.as_deref())?;
    let parsed = chrono::DateTime::parse_from_rfc3339(anchor).ok()?;
    let secs = now.signed_duration_since(parsed).num_seconds();
    // Saturate at 0 if clock went backwards (now < anchor).
    Some(u64::try_from(secs).unwrap_or(0))
}

/// Detect all stale or abort-timed-out sessions from the database.
///
/// INF-TSK-050-001 AC #2: extends the prior `Running`-only sweep to also
/// cover `Aborting` sessions whose abort has exceeded
/// `config.autorun.abort_timeout_secs`. The function loads config inline
/// so existing callers don't need to thread a new parameter.
///
/// # Errors
///
/// Returns `AutorunError` if the database query fails.
pub async fn detect_stale_sessions<S: DataStore>(
    store: &S,
    project_dir: &Path,
    stale_threshold_secs: u64,
) -> Result<Vec<StaleSessionInfo>, AutorunError> {
    detect_stuck_sessions(store, project_dir, stale_threshold_secs).await
}

/// Detect all stuck sessions: `Running` with dead liveness, OR `Aborting`
/// past the `abort_timeout_secs` threshold.
///
/// INF-TSK-050-001 AC #2: this is the canonical detector. Loads abort
/// timeout from `config.autorun.abort_timeout_secs` (default 300s). Falls
/// back to the default if config cannot be loaded (defensive — never
/// block reaping on a config read).
///
/// # Errors
///
/// Returns `AutorunError` if the database query fails.
pub async fn detect_stuck_sessions<S: DataStore>(
    store: &S,
    project_dir: &Path,
    stale_threshold_secs: u64,
) -> Result<Vec<StaleSessionInfo>, AutorunError> {
    detect_stuck_sessions_with(
        store,
        project_dir,
        stale_threshold_secs,
        default_pid_validator,
    )
    .await
}

/// Same as [`detect_stuck_sessions`] but accepts a custom PID validator.
///
/// Production callers use [`detect_stuck_sessions`] which delegates to
/// [`default_pid_validator`] (i.e. `validate_claude_pid > 0`). Tests
/// inject synthetic validators so they can express liveness without
/// spawning a real Claude Code process — the production code path
/// passes through unchanged.
///
/// # Errors
///
/// Returns `AutorunError` if the database query fails.
pub async fn detect_stuck_sessions_with<S: DataStore>(
    store: &S,
    project_dir: &Path,
    // INF-TSK-050-003 AC-09: heartbeat threshold no longer consulted —
    // the heartbeat reader was deleted. Parameter kept for source compat
    // with `detect_stuck_sessions` and external callers; prefixed `_` to
    // suppress unused-var warning.
    _stale_threshold_secs: u64,
    pid_validator: fn(u32) -> bool,
) -> Result<Vec<StaleSessionInfo>, AutorunError> {
    let abort_timeout_secs = crate::autorun::config::load_config(project_dir)
        .map(|c| c.autorun.abort_timeout_secs)
        .unwrap_or(300);

    let filter = crate::models::AutorunSessionFilter {
        all: true,
        limit: Some(50),
        ..Default::default()
    };
    let sessions = store
        .list_autorun_sessions(filter)
        .await
        .map_err(|e| AutorunError::WorkerFailed(format!("listing sessions: {e}")))?;

    let candidates: Vec<_> = sessions
        .into_iter()
        .filter(|s| {
            matches!(
                s.status,
                AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
            )
        })
        .collect();

    let now = chrono::Utc::now();
    let mut stale = Vec::new();
    for session in candidates {
        // INF-TSK-050-001 AC #4: name-verified PID liveness closes the
        // PID-reuse race. `check_pid_alive` (the bare kernel-only probe)
        // is no longer used in the reaper hot path.
        let pid_alive = is_session_pid_alive_with(session.pid, pid_validator);
        let tmux_alive = session.tmux_session.as_deref().map(check_tmux_alive);

        let abort_age = if matches!(session.status, AutorunSessionStatus::Aborting) {
            compute_abort_age_secs(&session, now)
        } else {
            None
        };

        // INF-TSK-050-003 AC-09: stuck criterion now folds the inline
        // two-signal residual after dropping heartbeat as a Signal-3
        // input. Status-dependent:
        // - Running:  PID dead OR tmux known dead.
        // - Aborting: abort took longer than `abort_timeout_secs`. If
        //   `abort_age` is None (no anchor), do NOT mark stuck — defensive
        //   against a freshly-aborting row whose timestamp hasn't been
        //   written yet.
        let stuck = match session.status {
            AutorunSessionStatus::Running => !pid_alive || tmux_alive == Some(false),
            AutorunSessionStatus::Aborting => abort_age.is_some_and(|age| age > abort_timeout_secs),
            _ => false,
        };

        if !stuck {
            continue;
        }

        // Count orphan/live workers.
        let workers = store
            .list_autorun_workers(&session.id)
            .await
            .unwrap_or_default();
        let mut orphan_count = 0;
        let mut live_count = 0;
        for w in &workers {
            if w.status == AutorunWorkerStatus::Running {
                if let Some(ref tmux_name) = w.tmux_session {
                    if check_tmux_alive(tmux_name) {
                        live_count += 1;
                    } else {
                        orphan_count += 1;
                    }
                } else {
                    orphan_count += 1;
                }
            }
        }

        stale.push(StaleSessionInfo {
            session,
            pid_alive,
            tmux_alive,
            orphan_worker_count: orphan_count,
            live_worker_count: live_count,
            abort_age_secs: abort_age,
        });
    }

    Ok(stale)
}

/// Reconcile a stuck session's database row to a terminal state.
///
/// INF-TSK-050-001 AC #3: this is the DB-only half of the cleanup split.
/// It is **idempotent**: a second call on a row that's already terminal
/// is a no-op. It performs only safe, fast, reversible operations:
///
/// - Status flip (Running/Aborting → Failed/Cancelled, CAS-guarded)
/// - Counter sync from authoritative `autorun_task_run` rows
/// - Heartbeat file removal (cheap, no user-visible impact)
/// - Task run flips: Running→Failed, Pending→Skipped (CAS-guarded)
///
/// Use this entry point on its own when the caller wants to reconcile DB
/// state without performing destructive resource operations (e.g., the
/// TUI reconcile-only path). For full cleanup, use
/// [`cleanup_stale_session`] which composes this with
/// [`cleanup_session_resources`].
///
/// Status mapping:
///
/// - `Running` → `Failed` (the legacy stale-cleanup outcome).
/// - `Aborting` → `Cancelled` (the abort flow's intended terminal state).
/// - Anything terminal → unchanged (idempotent no-op).
///
/// # Errors
///
/// Returns `AutorunError` if the session cannot be read from the database.
pub async fn reconcile_session_status<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: &str,
    stale_reason: &str,
) -> Result<CleanupReport, AutorunError> {
    let mut report = CleanupReport {
        session_id: session_id.to_string(),
        ..Default::default()
    };

    let session = store
        .get_autorun_session(session_id)
        .await
        .map_err(|e| AutorunError::WorkerFailed(format!("reading session: {e}")))?;

    let Some(session) = session else {
        return Ok(report);
    };

    // Idempotency: terminal status → no-op. Re-running this fn on a
    // reconciled row must not double-count or overwrite history.
    if session.status.is_terminal() {
        return Ok(report);
    }

    // INF-TSK-050-001 AC #3: counter sync from authoritative task_run rows.
    // The orchestrator may have crashed mid-batch, leaving session counters
    // stale relative to actual task_run state. Recompute from truth.
    let task_runs = store
        .list_autorun_task_runs(session_id)
        .await
        .unwrap_or_default();

    let mut completed_count: i32 = 0;
    let mut failed_count: i32 = 0;
    let mut skipped_count: i32 = 0;
    for run in &task_runs {
        match run.status {
            AutorunTaskRunStatus::Completed => completed_count = completed_count.saturating_add(1),
            AutorunTaskRunStatus::Failed => failed_count = failed_count.saturating_add(1),
            AutorunTaskRunStatus::Skipped => skipped_count = skipped_count.saturating_add(1),
            _ => {}
        }
    }

    // Flip in-flight task runs to terminal states.
    let now = chrono::Utc::now().to_rfc3339();
    for run in &task_runs {
        match run.status {
            AutorunTaskRunStatus::Running => {
                let _ = store
                    .update_autorun_task_run(
                        &run.id,
                        AutorunTaskRunUpdate {
                            status: Some(AutorunTaskRunStatus::Failed),
                            error_message: Some(format!("stale cleanup: {stale_reason}")),
                            completed_at: Some(now.clone()),
                            ..Default::default()
                        },
                    )
                    .await;
                report.task_runs_failed += 1;
                failed_count = failed_count.saturating_add(1);
            }
            AutorunTaskRunStatus::Pending => {
                let _ = store
                    .update_autorun_task_run(
                        &run.id,
                        AutorunTaskRunUpdate {
                            status: Some(AutorunTaskRunStatus::Skipped),
                            error_message: Some(format!("stale cleanup: {stale_reason}")),
                            completed_at: Some(now.clone()),
                            ..Default::default()
                        },
                    )
                    .await;
                report.task_runs_skipped += 1;
                skipped_count = skipped_count.saturating_add(1);
            }
            _ => {}
        }
    }

    // Status mapping: Aborting → Cancelled (abort intent), else → Failed.
    let target_status = match session.status {
        AutorunSessionStatus::Aborting => AutorunSessionStatus::Cancelled,
        _ => AutorunSessionStatus::Failed,
    };

    // CAS: expect the status we read; abort if another process already
    // moved it. Prevents overwriting a terminal status set concurrently.
    match store
        .update_autorun_session_cas(
            session_id,
            session.status,
            AutorunSessionUpdate {
                status: Some(target_status),
                stale_reason: Some(Some(stale_reason.to_string())),
                completed_tasks: Some(completed_count),
                failed_tasks: Some(failed_count),
                skipped_tasks: Some(skipped_count),
                completed_at: Some(now.clone()),
                updated_at: Some(now),
                ..Default::default()
            },
        )
        .await
    {
        Ok(crate::store::CasResult::NoOp) => {
            eprintln!(
                "warning: reconcile CAS no-op for {session_id} \
                 (status changed since read — another process transitioned it)"
            );
        }
        Err(e) => {
            eprintln!("warning: failed to reconcile session {session_id}: {e}");
        }
        Ok(crate::store::CasResult::Updated(_)) => {}
    }

    // Clean up heartbeat file (cheap, no user impact even on a live row;
    // the heartbeat task will recreate it next tick if anything is still
    // running, but in this code path we've already flipped to terminal).
    let heartbeat_path = project_dir
        .join(".state/autorun")
        .join(format!("heartbeat-{session_id}"));
    let _ = std::fs::remove_file(&heartbeat_path);

    Ok(report)
}

/// Perform destructive resource cleanup for a stuck session's workers.
///
/// INF-TSK-050-001 AC #3: this is the destructive half of the cleanup
/// split. Each worker is gated by the cleanup-context liveness check
/// ([`crate::session::liveness::is_session_alive_for_cleanup`]) before ANY destructive
/// action — if the worker's session is still alive, the entire per-worker
/// block is skipped (no tmux kill, no claim release, no registry
/// deregister, no `git worktree remove`, no gh PR close, no DB worker
/// status flip). Vetoed workers are reported as `workers_already_dead`
/// (the existing accounting bucket; semantically "skipped because not
/// confirmed dead").
///
/// **Pre-condition:** caller has typically already run
/// [`reconcile_session_status`] so the session DB row reflects terminal
/// state. This function does NOT touch the session row itself; it only
/// acts on workers and their owned resources.
///
/// **Idempotency:** safe to call multiple times. A second call after a
/// successful first will see no `Running` workers, no live tmux sessions,
/// and no claims to release — all per-worker counters return zero and
/// the function exits cleanly.
///
/// # Errors
///
/// Returns `AutorunError` only on catastrophic failures. Per-worker
/// errors are logged and the loop continues; the report's per-worker
/// counters reflect what actually succeeded.
pub async fn cleanup_session_resources<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: &str,
) -> Result<CleanupReport, AutorunError> {
    let mut report = CleanupReport {
        session_id: session_id.to_string(),
        ..Default::default()
    };

    // Process each worker. Idempotent: workers already in a terminal
    // status are skipped at the top of the loop (no double-kill).
    let workers = store
        .list_autorun_workers(session_id)
        .await
        .unwrap_or_default();

    for worker in &workers {
        if worker.status != AutorunWorkerStatus::Running {
            continue;
        }

        // F2 (WS-REV iteration 2): five-veto liveness check FIRST, before
        // ANY destructive action. Previously the worker tmux kill,
        // claim release, registry deregister, and `git worktree remove`
        // ran first, then the liveness check gated only the physical
        // removal — leaving a registry entry orphaned without a backing
        // directory if the session was still alive. Now: if any veto
        // fires, we skip every destructive step for this worker and let
        // the next sweep cycle re-evaluate.
        if let Some(ref wt_path) = worker.worktree_path {
            let wt = Path::new(wt_path);
            // Only veto when there is a worktree to protect AND it
            // physically exists. A non-existent worktree means there's
            // nothing left to reap — the cleanup path is the only way
            // to deregister the dangling registry entry.
            if wt.exists() {
                let worker_sid = worker
                    .worker_session_id
                    .clone()
                    .unwrap_or_else(|| worker.session_id.clone());
                let alive_inputs = crate::session::liveness::SessionAliveInputs {
                    session_id: worker_sid,
                    project_dir: project_dir.to_path_buf(),
                    worktree_path: Some(wt.to_path_buf()),
                    registry_created_at: None,
                };
                if crate::session::liveness::is_session_alive_for_cleanup(&alive_inputs) {
                    eprintln!(
                        "warn: skipping cleanup of live session for worker {}: session still alive",
                        worker.id
                    );
                    report.workers_already_dead += 1;
                    continue;
                }
            }
        }

        // Kill tmux session if alive.
        if let Some(ref tmux_name) = worker.tmux_session {
            if check_tmux_alive(tmux_name) {
                // Send Ctrl-C first.
                let _ = std::process::Command::new("tmux")
                    .args(["send-keys", "-t", tmux_name, "C-c", ""])
                    .output();
                // Brief wait, then check if it exited gracefully.
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                if check_tmux_alive(tmux_name) {
                    // Ctrl-C didn't work, force kill.
                    let _ = std::process::Command::new("tmux")
                        .args(["kill-session", "-t", tmux_name])
                        .output();
                }
                report.workers_killed += 1;
            } else {
                report.workers_already_dead += 1;
            }
        } else {
            report.workers_already_dead += 1;
        }

        // Release CRDT claims.
        let state_path = project_dir.join(".state/coordination/state.loro");
        if state_path.exists() && !worker.file_scope.is_empty() {
            if let Some(ref wsid) = worker.worker_session_id {
                let sid = crate::types::SessionId::new_unchecked(wsid);
                let mut released_count = 0usize;
                match crate::file_lock::locked_binary_rmw(
                    &state_path,
                    crate::coordination::loro::LoroCoordinator::in_memory,
                    |bytes| {
                        crate::coordination::loro::LoroCoordinator::from_bytes(bytes, &state_path)
                            .map_err(|e| format!("load coordinator: {e}"))
                    },
                    |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
                    |coord| {
                        match crate::coordination::claims::release_all(coord, &sid) {
                            Ok(count) => {
                                released_count = count;
                                Ok(())
                            }
                            Err(e) => {
                                eprintln!("warn: claim release for worker {}: {e}", worker.id);
                                Ok(()) // Continue cleanup even if claim release fails.
                            }
                        }
                    },
                ) {
                    Ok(()) => {
                        report.claims_released += released_count;
                    }
                    Err(e) => {
                        eprintln!(
                            "warn: failed to release claims for worker {}: {e}",
                            worker.id
                        );
                    }
                }
            }
        }

        // Deregister worktree unconditionally (even if dir was manually removed).
        // The liveness veto above already skipped this branch if the
        // session was alive — by this point the worker is confirmed dead
        // and it's safe to remove both the registry entry and the disk
        // directory together.
        if let Some(ref wt_path) = worker.worktree_path {
            let registry_path = project_dir.join(".state/worktrees");
            let _ = crate::worktree::locked_deregister_worktree(&registry_path, wt_path);
            let wt = Path::new(wt_path);
            if wt.exists() {
                let _ = std::process::Command::new("git")
                    .args(["worktree", "remove", "--force", wt_path])
                    .current_dir(project_dir)
                    .output();
                report.worktrees_removed += 1;
            }
        }

        // Close PR if open.
        if let Some(pr_num) = worker.pr_number {
            let _ = std::process::Command::new("gh")
                .args(["pr", "close", &pr_num.to_string(), "--delete-branch"])
                .current_dir(project_dir)
                .output();
            report.prs_closed += 1;
        }

        // Update worker to Failed.
        let _ = store
            .update_autorun_worker(
                &worker.id,
                AutorunWorkerUpdate {
                    status: Some(AutorunWorkerStatus::Failed),
                    completed_at: Some(chrono::Utc::now().to_rfc3339()),
                    ..Default::default()
                },
            )
            .await;
    }

    Ok(report)
}

/// Clean up a single stuck session and all its resources.
///
/// INF-TSK-050-001 AC #1, #3: composes [`reconcile_session_status`]
/// (DB-only) and [`cleanup_session_resources`] (destructive). Accepts
/// both `Running` and `Aborting` sessions — the AC #1 fix removed the
/// `Running`-only gate that previously short-circuited cleanup of an
/// `Aborting` row.
///
/// The combined report aggregates counters from both halves: counters
/// from `reconcile_session_status` (task_runs_failed, task_runs_skipped)
/// merged with counters from `cleanup_session_resources` (workers_killed,
/// workers_already_dead, worktrees_removed, claims_released, prs_closed).
///
/// # Errors
///
/// Returns `AutorunError` if the session cannot be read from the database.
/// Per-worker errors during destructive cleanup are logged and the loop
/// continues — the report reflects what actually succeeded.
pub async fn cleanup_stale_session<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: &str,
    stale_reason: &str,
) -> Result<CleanupReport, AutorunError> {
    // Pre-check: skip terminal sessions outright. INF-TSK-050-001 AC #1
    // removed the Running-only gate so Aborting can be reconciled too.
    let session = store
        .get_autorun_session(session_id)
        .await
        .map_err(|e| AutorunError::WorkerFailed(format!("reading session: {e}")))?;

    let Some(session) = session else {
        return Ok(CleanupReport {
            session_id: session_id.to_string(),
            ..Default::default()
        });
    };

    // Allow Running and Aborting; skip already-terminal sessions to
    // preserve the legacy "skip non-active" callers' expectation.
    if !matches!(
        session.status,
        AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
    ) {
        return Ok(CleanupReport {
            session_id: session_id.to_string(),
            ..Default::default()
        });
    }

    // Phase A: reconcile DB state (idempotent, safe).
    let reconcile_report =
        reconcile_session_status(store, project_dir, session_id, stale_reason).await?;

    // Phase B: destructive resource cleanup (gated by five-veto liveness).
    let resources_report = cleanup_session_resources(store, project_dir, session_id).await?;

    // Merge reports.
    Ok(CleanupReport {
        session_id: session_id.to_string(),
        workers_killed: resources_report.workers_killed,
        workers_already_dead: resources_report.workers_already_dead,
        worktrees_removed: resources_report.worktrees_removed,
        claims_released: resources_report.claims_released,
        prs_closed: resources_report.prs_closed,
        task_runs_failed: reconcile_report.task_runs_failed,
        task_runs_skipped: reconcile_report.task_runs_skipped,
    })
}

/// Sweep all stale sessions, cleaning up each one.
///
/// # Errors
///
/// Returns `AutorunError` if stale session detection fails.
pub async fn sweep_stale_sessions<S: DataStore>(
    store: &S,
    project_dir: &Path,
    stale_threshold_secs: u64,
) -> Result<SweepSummary, AutorunError> {
    sweep_stale_sessions_with(
        store,
        project_dir,
        stale_threshold_secs,
        default_pid_validator,
    )
    .await
}

/// Same as [`sweep_stale_sessions`] but accepts a custom PID validator.
/// Production callers use [`sweep_stale_sessions`]; tests inject
/// synthetic validators.
///
/// # Errors
///
/// Returns `AutorunError` if stale session detection fails.
pub async fn sweep_stale_sessions_with<S: DataStore>(
    store: &S,
    project_dir: &Path,
    stale_threshold_secs: u64,
    pid_validator: fn(u32) -> bool,
) -> Result<SweepSummary, AutorunError> {
    let stale =
        detect_stuck_sessions_with(store, project_dir, stale_threshold_secs, pid_validator).await?;
    let mut summary = SweepSummary::default();

    for info in &stale {
        let reason = build_stale_reason(info);
        match cleanup_stale_session(store, project_dir, &info.session.id, &reason).await {
            Ok(report) => {
                eprintln!(
                    "cleaned stale session '{}': {} workers killed, {} worktrees removed, {} claims released",
                    report.session_id,
                    report.workers_killed,
                    report.worktrees_removed,
                    report.claims_released,
                );
                summary.sessions_cleaned += 1;
            }
            Err(e) => {
                summary
                    .errors
                    .push((info.session.id.clone(), e.to_string()));
            }
        }
    }

    summary.sessions_skipped = 0; // All detected stale sessions are attempted.
    Ok(summary)
}

/// Build a human-readable stale reason from session info.
///
/// INF-TSK-050-003 AC-09: heartbeat reason removed. Heartbeat as a
/// signal source is deleted; the only remaining signals are PID liveness
/// (canonical), tmux liveness (orchestrator session-only), and
/// abort-timeout for `Aborting` sessions.
fn build_stale_reason(info: &StaleSessionInfo) -> String {
    let mut reasons = Vec::new();
    // INF-TSK-050-001 AC #2: surface abort-timeout as a distinct reason
    // so operators can tell apart a Running-stuck row from an
    // Aborting-stuck row in `codeflow autorun cleanup` output.
    if matches!(info.session.status, AutorunSessionStatus::Aborting) {
        if let Some(age) = info.abort_age_secs {
            reasons.push(format!("abort timeout ({age}s in aborting state)"));
        }
    }
    if !info.pid_alive && matches!(info.session.status, AutorunSessionStatus::Running) {
        reasons.push(format!("pid {} dead", info.session.pid.unwrap_or(0)));
    }
    if info.tmux_alive == Some(false) {
        reasons.push(format!(
            "tmux '{}' dead",
            info.session.tmux_session.as_deref().unwrap_or("?")
        ));
    }
    if reasons.is_empty() {
        "unknown".to_string()
    } else {
        reasons.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_pid_alive_current_process() {
        let pid = i64::from(std::process::id());
        assert!(check_pid_alive(pid), "current process should be alive");
    }

    #[test]
    fn test_check_pid_alive_dead_pid() {
        // PID 0 is always the kernel scheduler, not accessible to normal users.
        // Use a very high PID unlikely to exist.
        assert!(
            !check_pid_alive(999_999_999),
            "nonexistent PID should be dead"
        );
    }

    #[test]
    fn test_check_pid_alive_zero() {
        assert!(!check_pid_alive(0), "PID 0 should return false");
    }

    #[test]
    fn test_check_pid_alive_negative() {
        assert!(!check_pid_alive(-1), "negative PID should return false");
    }

    // INF-TSK-050-003 AC-09: deleted tests for `is_session_stale`,
    // `check_heartbeat_alive`. Replacement coverage lives in
    // `session::liveness::tests` (canonical chokepoint) and in the new
    // stuck-session tests below that exercise the residual two-signal
    // criterion (`!pid_alive || tmux_alive == Some(false)`) directly via
    // `detect_stuck_sessions_with`.

    #[test]
    fn test_check_tmux_alive_nonexistent() {
        // A random name that certainly doesn't exist.
        assert!(!check_tmux_alive("cf-nonexistent-test-session-xyz-999"));
    }

    #[test]
    fn test_build_stale_reason_pid_dead() {
        let info = StaleSessionInfo {
            session: AutorunSession {
                id: "ses-test".into(),
                batch_file: String::new(),
                batch_name: None,
                status: AutorunSessionStatus::Running,
                max_session_workers: 1,
                total_tasks: 1,
                completed_tasks: 0,
                failed_tasks: 0,
                pid: Some(12345),
                skipped_tasks: 0,
                tmux_session: None,
                stale_reason: None,
                target_branch: None,
                final_pr_url: None,
                current_task_id: None,
                current_task_format_id: None,
                updated_at: None,
                last_heartbeat_at: None,
                created_at: String::new(),
                completed_at: None,
                abort_started_at: None,
            },
            pid_alive: false,
            tmux_alive: None,
            orphan_worker_count: 0,
            live_worker_count: 0,
            abort_age_secs: None,
        };
        let reason = build_stale_reason(&info);
        assert!(reason.contains("pid 12345 dead"));
    }

    #[test]
    fn test_build_stale_reason_multiple() {
        // INF-TSK-050-003 AC-09: heartbeat reason removed. Only PID and
        // tmux signals remain. The previous "heartbeat stale (300s old)"
        // assertion is gone alongside the field.
        let info = StaleSessionInfo {
            session: AutorunSession {
                id: "ses-test".into(),
                batch_file: String::new(),
                batch_name: None,
                status: AutorunSessionStatus::Running,
                max_session_workers: 1,
                total_tasks: 1,
                completed_tasks: 0,
                failed_tasks: 0,
                pid: Some(99),
                skipped_tasks: 0,
                tmux_session: Some("cf-orch-x".into()),
                stale_reason: None,
                target_branch: None,
                final_pr_url: None,
                current_task_id: None,
                current_task_format_id: None,
                updated_at: None,
                last_heartbeat_at: None,
                created_at: String::new(),
                completed_at: None,
                abort_started_at: None,
            },
            pid_alive: false,
            tmux_alive: Some(false),
            orphan_worker_count: 2,
            live_worker_count: 0,
            abort_age_secs: None,
        };
        let reason = build_stale_reason(&info);
        assert!(reason.contains("pid 99 dead"));
        assert!(reason.contains("tmux 'cf-orch-x' dead"));
        assert!(
            !reason.contains("heartbeat"),
            "heartbeat must not appear in stale reasons after AC-09"
        );
    }

    #[test]
    fn test_stale_config_defaults() {
        let cfg = crate::autorun::config::AutorunConfig::default();
        assert_eq!(cfg.heartbeat_interval_secs, 30);
        assert_eq!(cfg.stale_threshold_secs, 90);
    }

    #[test]
    fn test_stale_config_validation_threshold_too_low() {
        let mut cfg = crate::autorun::config::ParallelWorkConfig::default();
        cfg.autorun.heartbeat_interval_secs = 30;
        cfg.autorun.stale_threshold_secs = 50; // < 2*30 = 60
        let result = crate::autorun::config::load_config(tempfile::tempdir().unwrap().path());
        // Default config passes, but manual construction with bad values should fail.
        // Let's verify via the validate_config indirectly through config construction.
        let json =
            r#"{ "autorun": { "heartbeat_interval_secs": 30, "stale_threshold_secs": 50 } }"#;
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("parallel-work-config.json"), json).unwrap();
        let err = crate::autorun::config::load_config(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("stale_threshold_secs"),
            "should reject threshold < 2*interval"
        );
        drop(result); // suppress unused warning
    }

    #[test]
    fn test_stale_config_validation_interval_bounds() {
        // Too low.
        let json = r#"{ "autorun": { "heartbeat_interval_secs": 5, "stale_threshold_secs": 10 } }"#;
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join(".codeflow/config/parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("parallel-work-config.json"), json).unwrap();
        let err = crate::autorun::config::load_config(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("heartbeat_interval_secs"),
            "should reject interval < 10"
        );

        // Too high.
        let json =
            r#"{ "autorun": { "heartbeat_interval_secs": 500, "stale_threshold_secs": 1000 } }"#;
        std::fs::write(config_dir.join("parallel-work-config.json"), json).unwrap();
        let err = crate::autorun::config::load_config(dir.path()).unwrap_err();
        assert!(
            err.to_string().contains("heartbeat_interval_secs"),
            "should reject interval > 300"
        );
    }

    // -- Helper to create a MockStore session for async tests --

    fn make_session(id: &str, status: AutorunSessionStatus, pid: Option<i64>) -> AutorunSession {
        AutorunSession {
            id: id.into(),
            batch_file: "b.yaml".into(),
            batch_name: Some("test".into()),
            status,
            max_session_workers: 2,
            total_tasks: 2,
            completed_tasks: 0,
            failed_tasks: 0,
            pid,
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
        }
    }

    fn make_task_run(
        id: &str,
        session_id: &str,
        task_id: &str,
        status: crate::types::AutorunTaskRunStatus,
    ) -> crate::models::AutorunTaskRun {
        crate::models::AutorunTaskRun {
            id: id.into(),
            worker_id: "w1".into(),
            task_id: task_id.into(),
            session_id: session_id.into(),
            status,
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
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn make_worker(
        id: &str,
        session_id: &str,
        task_id: &str,
        status: AutorunWorkerStatus,
    ) -> crate::models::AutorunWorker {
        crate::models::AutorunWorker {
            id: id.into(),
            session_id: session_id.into(),
            worker_num: 1,
            task_id: task_id.into(),
            status,
            tmux_session: None,
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

    // -- Async tests for detect_stale_sessions --

    /// INF-TSK-050-001 AC #4: under the new `validate_claude_pid` regime,
    /// a session whose recorded PID validates as a Claude Code process
    /// is NOT detected as stale. Tests inject `always_alive_validator`
    /// (which treats any PID as live) to express "this PID belongs to a
    /// Claude Code process" without spawning a real one.
    fn always_alive_validator(_pid: u32) -> bool {
        true
    }

    #[tokio::test]
    async fn test_detect_stale_sessions_none_stale() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        // Create a Running session whose PID the validator will accept.
        let session = make_session(
            "ses-alive",
            AutorunSessionStatus::Running,
            Some(i64::from(std::process::id())),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stuck_sessions_with(&store, dir.path(), 90, always_alive_validator)
            .await
            .unwrap();
        assert!(
            result.is_empty(),
            "live session (validator accepted) should not be detected as stale"
        );
    }

    #[tokio::test]
    async fn test_detect_stale_sessions_dead_pid() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        // Create a Running session with a dead PID.
        let session = make_session("ses-dead", AutorunSessionStatus::Running, Some(999_999_999));
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stale_sessions(&store, dir.path(), 90).await.unwrap();
        assert_eq!(result.len(), 1, "dead PID session should be detected");
        assert_eq!(result[0].session.id, "ses-dead");
        assert!(!result[0].pid_alive);
    }

    #[tokio::test]
    async fn test_detect_stale_sessions_completed_ignored() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        // Completed sessions should not be detected even with dead PIDs.
        let session = make_session(
            "ses-done",
            AutorunSessionStatus::Completed,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stale_sessions(&store, dir.path(), 90).await.unwrap();
        assert!(result.is_empty(), "completed session should be ignored");
    }

    // INF-TSK-050-003 AC-09: deleted `test_detect_stale_sessions_expired_heartbeat`.
    // The heartbeat reader/writer are gone; expired-heartbeat is no longer a
    // stale signal. The two-signal residual (PID + tmux) is covered by other
    // tests below. Replacement coverage for "alive PID but tmux dead → stale"
    // lives in the integration test for `detect_stuck_sessions_with`.

    // -- Async tests for cleanup_stale_session --

    #[tokio::test]
    async fn test_cleanup_marks_session_failed() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session(
            "ses-cleanup",
            AutorunSessionStatus::Running,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let report = cleanup_stale_session(&store, dir.path(), "ses-cleanup", "pid dead")
            .await
            .unwrap();
        assert_eq!(report.session_id, "ses-cleanup");

        // Verify session status updated to Failed.
        let updated = store
            .get_autorun_session("ses-cleanup")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, AutorunSessionStatus::Failed);
        assert_eq!(updated.stale_reason.as_deref(), Some("pid dead"));
    }

    #[tokio::test]
    async fn test_cleanup_skips_non_running() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session(
            "ses-done2",
            AutorunSessionStatus::Completed,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let report = cleanup_stale_session(&store, dir.path(), "ses-done2", "pid dead")
            .await
            .unwrap();
        // Should skip because not Running.
        assert_eq!(report.workers_killed, 0);
        assert_eq!(report.task_runs_failed, 0);

        // Status should remain Completed.
        let updated = store
            .get_autorun_session("ses-done2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, AutorunSessionStatus::Completed);
    }

    #[tokio::test]
    async fn test_cleanup_deletes_heartbeat_file() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session(
            "ses-hb-del",
            AutorunSessionStatus::Running,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let autorun_dir = dir.path().join(".state/autorun");
        std::fs::create_dir_all(&autorun_dir).unwrap();
        let hb_path = autorun_dir.join("heartbeat-ses-hb-del");
        std::fs::write(&hb_path, "").unwrap();
        assert!(hb_path.exists());

        let _report = cleanup_stale_session(&store, dir.path(), "ses-hb-del", "test")
            .await
            .unwrap();

        assert!(
            !hb_path.exists(),
            "heartbeat file should be deleted after cleanup"
        );
    }

    #[tokio::test]
    async fn test_cleanup_workers_marked_failed() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session("ses-wk", AutorunSessionStatus::Running, Some(999_999_999));
        store.create_autorun_session(&session).await.unwrap();

        let worker = make_worker("w1", "ses-wk", "task-a", AutorunWorkerStatus::Running);
        store.create_autorun_worker(&worker).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let report = cleanup_stale_session(&store, dir.path(), "ses-wk", "pid dead")
            .await
            .unwrap();
        assert_eq!(
            report.workers_already_dead, 1,
            "worker without tmux should count as already dead"
        );
    }

    #[tokio::test]
    async fn test_cleanup_task_runs_failed_and_skipped() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session("ses-tr", AutorunSessionStatus::Running, Some(999_999_999));
        store.create_autorun_session(&session).await.unwrap();

        let run1 = make_task_run(
            "r1",
            "ses-tr",
            "task-a",
            crate::types::AutorunTaskRunStatus::Running,
        );
        let run2 = make_task_run(
            "r2",
            "ses-tr",
            "task-b",
            crate::types::AutorunTaskRunStatus::Pending,
        );
        let run3 = make_task_run(
            "r3",
            "ses-tr",
            "task-c",
            crate::types::AutorunTaskRunStatus::Completed,
        );
        store.create_autorun_task_run(&run1).await.unwrap();
        store.create_autorun_task_run(&run2).await.unwrap();
        store.create_autorun_task_run(&run3).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let report = cleanup_stale_session(&store, dir.path(), "ses-tr", "pid dead")
            .await
            .unwrap();
        assert_eq!(
            report.task_runs_failed, 1,
            "Running task run should be marked Failed"
        );
        assert_eq!(
            report.task_runs_skipped, 1,
            "Pending task run should be marked Skipped"
        );
    }

    // -- Async tests for sweep_stale_sessions --

    #[tokio::test]
    async fn test_sweep_cleans_multiple_stale() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        // Two stale sessions (PID validator returns false), one alive
        // (PID matches the test runner). The validator below treats
        // only the test runner's PID as a live Claude Code process.
        let alive_pid = std::process::id();
        let s1 = make_session(
            "ses-stale-1",
            AutorunSessionStatus::Running,
            Some(999_999_998),
        );
        let s2 = make_session(
            "ses-stale-2",
            AutorunSessionStatus::Running,
            Some(999_999_997),
        );
        let s3 = make_session(
            "ses-alive",
            AutorunSessionStatus::Running,
            Some(i64::from(alive_pid)),
        );
        store.create_autorun_session(&s1).await.unwrap();
        store.create_autorun_session(&s2).await.unwrap();
        store.create_autorun_session(&s3).await.unwrap();

        let dir = tempfile::tempdir().unwrap();

        // Static fn ptr can't capture `alive_pid`; use std::process::id()
        // inside the validator (always evaluates to the same PID for
        // the lifetime of this test process).
        fn current_pid_validator(pid: u32) -> bool {
            pid == std::process::id()
        }
        let summary = sweep_stale_sessions_with(&store, dir.path(), 90, current_pid_validator)
            .await
            .unwrap();
        assert_eq!(
            summary.sessions_cleaned, 2,
            "both stale sessions should be cleaned (got {summary:?})"
        );
        assert!(summary.errors.is_empty(), "no errors expected");

        // Verify alive session is untouched.
        let alive = store
            .get_autorun_session("ses-alive")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(alive.status, AutorunSessionStatus::Running);
    }

    #[tokio::test]
    async fn test_sweep_empty_when_none_stale() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let s = make_session(
            "ses-ok",
            AutorunSessionStatus::Running,
            Some(i64::from(std::process::id())),
        );
        store.create_autorun_session(&s).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let summary = sweep_stale_sessions_with(&store, dir.path(), 90, always_alive_validator)
            .await
            .unwrap();
        assert_eq!(summary.sessions_cleaned, 0);
    }

    #[tokio::test]
    async fn test_cleanup_nonexistent_session() {
        let store = crate::store::mock::MockStore::new();
        let dir = tempfile::tempdir().unwrap();
        let report = cleanup_stale_session(&store, dir.path(), "nonexistent", "test")
            .await
            .unwrap();
        assert_eq!(report.workers_killed, 0);
        assert_eq!(report.task_runs_failed, 0);
    }

    /// F2 (WS-REV iteration 2 rework): when the five-veto liveness
    /// predicate reports the worker's session is still alive,
    /// `cleanup_stale_session` MUST skip the entire per-worker
    /// cleanup block — including registry deregistration, tmux kill,
    /// CRDT claim release, `git worktree remove`, gh PR close, and
    /// the DB worker-status update. Previously the deregister ran
    /// unconditionally and only the physical removal was vetoed,
    /// orphaning the registry/disk pair.
    #[tokio::test]
    async fn test_cleanup_skips_destructive_actions_when_session_alive() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();

        let session_sid = "ses-stale-alive";
        let worker_sid = "ses-01jq2alivetest1234567890"; // valid 30-char ULID-ish
        let session = make_session(
            session_sid,
            AutorunSessionStatus::Running,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();

        // Build a project dir + worktree dir + registry + pathflow-team.json
        // with a LIVE lead_pid so V4 of `is_session_alive` votes ALIVE.
        let project = tempfile::tempdir().unwrap();
        let project_dir = project.path();
        let wt_dir = project_dir.join(".git-worktrees").join("worktree-alive");
        std::fs::create_dir_all(&wt_dir).unwrap();

        // Registry with one Active entry pointing at wt_dir.
        let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
        let mut reg = crate::worktree::WorktreeRegistry::new(&chrono::Utc::now().to_rfc3339());
        reg.worktrees.push(crate::worktree::WorktreeEntry {
            name: "worktree-alive".into(),
            path: wt_dir.to_string_lossy().into(),
            branch: Some("feat/alive".into()),
            created_at: (chrono::Utc::now() - chrono::Duration::seconds(3600)).to_rfc3339(),
            status: crate::worktree::WorktreeStatus::Active,
            session_id: Some(worker_sid.into()),
            task_id: None,
            source: None,
        });
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        // pathflow-team.json with a LIVE PID drives V4 ALIVE.
        let team_dir = project_dir
            .join(".state/session")
            .join(worker_sid)
            .join("pathflow");
        std::fs::create_dir_all(&team_dir).unwrap();
        std::fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::json!({
                "team_name": "live-team",
                "lead_pid": std::process::id(),
            })
            .to_string(),
        )
        .unwrap();

        // Build a worker pointing at this worktree so cleanup_stale_session
        // exercises the per-worker veto path.
        let mut worker = make_worker(
            "w-alive",
            session_sid,
            "task-alive",
            AutorunWorkerStatus::Running,
        );
        worker.worktree_path = Some(wt_dir.to_string_lossy().into());
        worker.worker_session_id = Some(worker_sid.into());
        store.create_autorun_worker(&worker).await.unwrap();

        let report = cleanup_stale_session(&store, project_dir, session_sid, "pid dead")
            .await
            .unwrap();

        // 1. Registry entry MUST still be present.
        let reg_after = crate::worktree::read_registry(&registry_path).unwrap();
        assert_eq!(
            reg_after.worktrees.len(),
            1,
            "live session: registry entry must NOT be deregistered (was: {:?})",
            reg_after
                .worktrees
                .iter()
                .map(|e| (&e.name, &e.status))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            reg_after.worktrees[0].name, "worktree-alive",
            "the live worktree entry must be the one preserved"
        );

        // 2. Worktree directory MUST still exist.
        assert!(
            wt_dir.exists(),
            "live session: worktree directory must NOT be removed"
        );

        // 3. Report MUST classify the worker as already-dead-or-alive
        //    (skipped path), NOT as workers_killed. With no tmux session
        //    set up, the kill block was skipped entirely, so killed=0
        //    is expected on both old and new code; the load-bearing
        //    invariant is `workers_removed == 0`.
        assert_eq!(
            report.worktrees_removed, 0,
            "live session: report.worktrees_removed must be 0 (got {})",
            report.worktrees_removed
        );

        // 4. The worker's DB status SHOULD remain Running (no DB write
        //    happens before the destructive block). Confirm we didn't
        //    accidentally mark the live worker Failed.
        let updated_worker = store.list_autorun_workers(session_sid).await.unwrap();
        assert_eq!(updated_worker.len(), 1);
        assert_eq!(
            updated_worker[0].status,
            AutorunWorkerStatus::Running,
            "live session: worker DB status must NOT be flipped to Failed"
        );
    }

    // ------------------------------------------------------------------
    // INF-TSK-050-001 AC #2/#3/#4 — abort TTL, reconcile/cleanup split,
    // name-verified PID liveness.
    // ------------------------------------------------------------------

    /// Validator that always returns false — every PID is "dead".
    fn always_dead_validator(_pid: u32) -> bool {
        false
    }

    #[test]
    fn test_is_session_pid_alive_none_returns_false() {
        // INF-TSK-050-001 AC #4 fail-secure: missing PID → dead.
        assert!(!is_session_pid_alive(None));
    }

    #[test]
    fn test_is_session_pid_alive_zero_returns_false() {
        // PID 0 is the kernel scheduler — never a live session owner.
        assert!(!is_session_pid_alive(Some(0)));
    }

    #[test]
    fn test_is_session_pid_alive_negative_returns_false() {
        // u32::try_from rejects negatives → fail-secure dead.
        assert!(!is_session_pid_alive(Some(-1)));
        assert!(!is_session_pid_alive(Some(-99_999)));
    }

    #[test]
    fn test_is_session_pid_alive_overflow_returns_false() {
        // Above u32::MAX → fail-secure dead. Defensive against rows
        // that somehow stored an out-of-range PID.
        assert!(!is_session_pid_alive(Some(i64::from(u32::MAX) + 1)));
    }

    #[test]
    fn test_is_session_pid_alive_with_validator_accepts() {
        // The injection point exists for tests; verify it routes to
        // the validator correctly.
        assert!(is_session_pid_alive_with(Some(42), always_alive_validator));
    }

    #[test]
    fn test_is_session_pid_alive_with_validator_rejects() {
        // Even with a non-zero, in-range PID, the validator decides.
        assert!(!is_session_pid_alive_with(Some(42), always_dead_validator));
    }

    /// INF-TSK-024-053 AC-5: regression guard for the false-stuck bug.
    ///
    /// Before AC-5, `default_pid_validator` delegated to
    /// `validate_claude_pid`, which requires the process name to contain
    /// "claude". The orchestrator process (`codeflow autorun run`,
    /// written to `autorun_session.pid` by `orchestrator.rs:189`) is
    /// NEVER named "claude", so the validator always returned `false`,
    /// causing the reaper to mark every live autorun session stuck.
    ///
    /// Post-AC-5, the validator delegates to `validate_orchestrator_pid`
    /// (alive-only, no name check). This test asserts that a live,
    /// non-claude PID is accepted — which uses the **current process**
    /// PID since `codeflow` (or the test binary) is itself not named
    /// "claude".
    #[test]
    fn test_default_pid_validator_accepts_non_claude_live_pid_ac5() {
        let current_pid = i64::from(std::process::id());
        assert!(
            current_pid > 0,
            "std::process::id() must return a positive PID; got {current_pid}"
        );
        assert!(
            is_session_pid_alive(Some(current_pid)),
            "AC-5: default_pid_validator MUST accept a live non-claude PID \
             (the orchestrator is never named 'claude'). Failing this test \
             means the autorun reaper would mark every live autorun session \
             stuck. PID under test: {current_pid}"
        );
    }

    /// INF-TSK-024-053 AC-5 partner test: confirm the `pid==0` and dead-PID
    /// branches still fail-secure. We cannot easily synthesize a "dead"
    /// PID on every host, but a value above `u32::MAX` is guaranteed to be
    /// rejected by the i64→u32 conversion before the kernel probe runs,
    /// preserving the prior fail-secure contract.
    #[test]
    fn test_default_pid_validator_fail_secure_for_invalid_pid_ac5() {
        // PID 0 is reserved (swapper on Linux, kernel_task on macOS) and the
        // validator rejects it via the explicit `pid == 0` guard in
        // `validate_orchestrator_pid`.
        assert!(
            !is_session_pid_alive(Some(0)),
            "AC-5: PID 0 must remain rejected even after the validator swap"
        );
        // Out-of-range i64 still trips the u32::try_from guard inside
        // `is_session_pid_alive_with`.
        assert!(
            !is_session_pid_alive(Some(i64::from(u32::MAX) + 1)),
            "AC-5: out-of-range PID must remain rejected"
        );
    }

    #[test]
    fn test_compute_abort_age_secs_uses_abort_started_at() {
        // INF-TSK-050-001 AC #2: prefer abort_started_at over updated_at.
        let now = chrono::Utc::now();
        let abort_t = (now - chrono::Duration::seconds(120)).to_rfc3339();
        let updated_t = (now - chrono::Duration::seconds(30)).to_rfc3339();
        let mut session = make_session("ses-abort", AutorunSessionStatus::Aborting, None);
        session.abort_started_at = Some(abort_t);
        session.updated_at = Some(updated_t);
        let age = compute_abort_age_secs(&session, now);
        // Should pick abort_started_at (120s) not updated_at (30s).
        assert!(
            matches!(age, Some(secs) if (118..=122).contains(&secs)),
            "expected age ~120s from abort_started_at, got {age:?}"
        );
    }

    #[test]
    fn test_compute_abort_age_secs_fallback_to_updated_at() {
        // Legacy row: abort_started_at is None, but updated_at exists.
        // The fn falls back to updated_at as the freshness anchor.
        let now = chrono::Utc::now();
        let updated_t = (now - chrono::Duration::seconds(45)).to_rfc3339();
        let mut session = make_session("ses-legacy", AutorunSessionStatus::Aborting, None);
        session.abort_started_at = None;
        session.updated_at = Some(updated_t);
        let age = compute_abort_age_secs(&session, now);
        assert!(
            matches!(age, Some(secs) if (43..=47).contains(&secs)),
            "expected age ~45s from updated_at fallback, got {age:?}"
        );
    }

    #[test]
    fn test_compute_abort_age_secs_no_anchors_returns_none() {
        // No abort_started_at AND no updated_at → None. Caller MUST
        // treat this as "not yet stuck" so a freshly-aborting row
        // whose timestamp hasn't been written is never reaped.
        let now = chrono::Utc::now();
        let mut session = make_session("ses-fresh", AutorunSessionStatus::Aborting, None);
        session.abort_started_at = None;
        session.updated_at = None;
        assert_eq!(compute_abort_age_secs(&session, now), None);
    }

    #[test]
    fn test_compute_abort_age_secs_clock_skew_returns_zero() {
        // If now < abort_started_at (clock went backwards), saturate
        // at 0 instead of producing a negative age.
        let now = chrono::Utc::now();
        let future = (now + chrono::Duration::seconds(60)).to_rfc3339();
        let mut session = make_session("ses-skew", AutorunSessionStatus::Aborting, None);
        session.abort_started_at = Some(future);
        let age = compute_abort_age_secs(&session, now);
        assert_eq!(age, Some(0), "future anchor must clamp to 0, got {age:?}");
    }

    #[tokio::test]
    async fn test_detect_stuck_sessions_aborting_past_timeout() {
        // INF-TSK-050-001 AC #2: an Aborting session with abort_started_at
        // older than abort_timeout_secs is detected as stuck.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let now = chrono::Utc::now();
        // 600s ago — well past the 300s default abort_timeout.
        let abort_t = (now - chrono::Duration::seconds(600)).to_rfc3339();
        let mut session = make_session(
            "ses-abort-stuck",
            AutorunSessionStatus::Aborting,
            // PID alive — only the abort timeout matters here.
            Some(i64::from(std::process::id())),
        );
        session.abort_started_at = Some(abort_t);
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stuck_sessions_with(&store, dir.path(), 90, always_alive_validator)
            .await
            .unwrap();
        assert_eq!(
            result.len(),
            1,
            "Aborting session past abort_timeout must be detected as stuck"
        );
        assert_eq!(result[0].session.id, "ses-abort-stuck");
        assert!(result[0].abort_age_secs.is_some_and(|s| s >= 600));
    }

    #[tokio::test]
    async fn test_detect_stuck_sessions_aborting_within_timeout() {
        // INF-TSK-050-001 AC #2: an Aborting session within abort_timeout
        // is NOT detected as stuck.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let now = chrono::Utc::now();
        // 30s ago — well within 300s default abort_timeout.
        let abort_t = (now - chrono::Duration::seconds(30)).to_rfc3339();
        let mut session = make_session(
            "ses-abort-fresh",
            AutorunSessionStatus::Aborting,
            Some(i64::from(std::process::id())),
        );
        session.abort_started_at = Some(abort_t);
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stuck_sessions_with(&store, dir.path(), 90, always_alive_validator)
            .await
            .unwrap();
        assert!(
            result.is_empty(),
            "Aborting session within abort_timeout must NOT be detected as stuck"
        );
    }

    #[tokio::test]
    async fn test_detect_stuck_sessions_aborting_no_anchor_not_stuck() {
        // INF-TSK-050-001 AC #2 defensive case: Aborting row with no
        // abort_started_at AND no updated_at must NOT be reaped — it
        // may have just transitioned and the timestamp hasn't been
        // written yet.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let mut session = make_session(
            "ses-abort-noancr",
            AutorunSessionStatus::Aborting,
            Some(i64::from(std::process::id())),
        );
        session.abort_started_at = None;
        session.updated_at = None;
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stuck_sessions_with(&store, dir.path(), 90, always_alive_validator)
            .await
            .unwrap();
        assert!(
            result.is_empty(),
            "Aborting row with no anchor must NOT be reaped (defensive)"
        );
    }

    #[tokio::test]
    async fn test_cleanup_stale_session_aborting_status_to_cancelled() {
        // INF-TSK-050-001 AC #1, #3: cleanup of an Aborting row maps to
        // Cancelled (not Failed), because the user/orchestrator
        // signaled abort intent.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let mut session = make_session(
            "ses-abort-cleanup",
            AutorunSessionStatus::Aborting,
            Some(999_999_999),
        );
        session.abort_started_at =
            Some((chrono::Utc::now() - chrono::Duration::seconds(600)).to_rfc3339());
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let _report =
            cleanup_stale_session(&store, dir.path(), "ses-abort-cleanup", "abort timeout")
                .await
                .unwrap();
        let updated = store
            .get_autorun_session("ses-abort-cleanup")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            updated.status,
            AutorunSessionStatus::Cancelled,
            "Aborting row must reconcile to Cancelled, not Failed"
        );
    }

    #[tokio::test]
    async fn test_reconcile_session_status_idempotent() {
        // INF-TSK-050-001 AC #3: calling reconcile twice on the same
        // session is a no-op on the second call. The second call must
        // not double-count task runs or overwrite the (already terminal)
        // status with another flip.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session(
            "ses-recon-idem",
            AutorunSessionStatus::Running,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();

        // Add one Running task_run so reconcile flips it.
        let run = make_task_run(
            "tr-recon-1",
            "ses-recon-idem",
            "task-a",
            crate::types::AutorunTaskRunStatus::Running,
        );
        store.create_autorun_task_run(&run).await.unwrap();

        let dir = tempfile::tempdir().unwrap();

        let r1 = reconcile_session_status(&store, dir.path(), "ses-recon-idem", "test")
            .await
            .unwrap();
        assert_eq!(r1.task_runs_failed, 1, "first call must flip Running run");

        // Second call: the row is already Failed → no-op, no double-flip.
        let r2 = reconcile_session_status(&store, dir.path(), "ses-recon-idem", "test")
            .await
            .unwrap();
        assert_eq!(
            r2.task_runs_failed, 0,
            "second call must be a no-op (row already terminal)"
        );

        // Status must still be Failed (stable).
        let after = store
            .get_autorun_session("ses-recon-idem")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.status, AutorunSessionStatus::Failed);
    }

    #[tokio::test]
    async fn test_reconcile_session_status_syncs_counters() {
        // INF-TSK-050-001 AC #3: counter sync from authoritative
        // task_run rows. The session row arrives with stale counters;
        // reconcile recomputes from truth.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let mut session = make_session(
            "ses-counters",
            AutorunSessionStatus::Running,
            Some(999_999_999),
        );
        // Pre-existing stale counters (orchestrator crashed mid-batch).
        session.completed_tasks = 0;
        session.failed_tasks = 0;
        session.skipped_tasks = 0;
        store.create_autorun_session(&session).await.unwrap();

        // Authoritative task_runs: 2 completed, 1 failed (terminal,
        // not flipped by reconcile), 1 running (will be flipped to
        // failed → bumps failed counter), 1 pending (→ skipped).
        for (i, st) in [
            crate::types::AutorunTaskRunStatus::Completed,
            crate::types::AutorunTaskRunStatus::Completed,
            crate::types::AutorunTaskRunStatus::Failed,
            crate::types::AutorunTaskRunStatus::Running,
            crate::types::AutorunTaskRunStatus::Pending,
        ]
        .iter()
        .enumerate()
        {
            let r = make_task_run(&format!("tr-{i}"), "ses-counters", &format!("t-{i}"), *st);
            store.create_autorun_task_run(&r).await.unwrap();
        }

        let dir = tempfile::tempdir().unwrap();
        let _ = reconcile_session_status(&store, dir.path(), "ses-counters", "test")
            .await
            .unwrap();

        let after = store
            .get_autorun_session("ses-counters")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after.completed_tasks, 2, "2 already-Completed rows");
        // 1 already Failed + 1 Running flipped to Failed = 2.
        assert_eq!(
            after.failed_tasks, 2,
            "1 pre-existing Failed + 1 Running→Failed = 2"
        );
        // 1 Pending flipped to Skipped = 1.
        assert_eq!(after.skipped_tasks, 1, "1 Pending→Skipped");
    }

    #[tokio::test]
    async fn test_cleanup_session_resources_idempotent() {
        // INF-TSK-050-001 AC #3: the destructive half is safe to call
        // multiple times. Workers in non-Running status are skipped at
        // the top of the loop.
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        let session = make_session(
            "ses-res-idem",
            AutorunSessionStatus::Running,
            Some(999_999_999),
        );
        store.create_autorun_session(&session).await.unwrap();
        // Worker without a tmux session counts as already-dead via the
        // existing branch, and gets flipped to Failed by the inner
        // update.
        let worker = make_worker(
            "w-idem",
            "ses-res-idem",
            "task-x",
            AutorunWorkerStatus::Running,
        );
        store.create_autorun_worker(&worker).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let r1 = cleanup_session_resources(&store, dir.path(), "ses-res-idem")
            .await
            .unwrap();
        assert_eq!(r1.workers_already_dead, 1);

        // Second call: worker is now Failed → loop skips it → all
        // counters zero.
        let r2 = cleanup_session_resources(&store, dir.path(), "ses-res-idem")
            .await
            .unwrap();
        assert_eq!(r2.workers_already_dead, 0);
        assert_eq!(r2.workers_killed, 0);
        assert_eq!(r2.worktrees_removed, 0);
    }

    #[tokio::test]
    async fn test_build_stale_reason_aborting_includes_age() {
        let info = StaleSessionInfo {
            session: AutorunSession {
                id: "ses-test".into(),
                batch_file: String::new(),
                batch_name: None,
                status: AutorunSessionStatus::Aborting,
                max_session_workers: 1,
                total_tasks: 1,
                completed_tasks: 0,
                failed_tasks: 0,
                pid: Some(99),
                skipped_tasks: 0,
                tmux_session: None,
                stale_reason: None,
                target_branch: None,
                final_pr_url: None,
                current_task_id: None,
                current_task_format_id: None,
                updated_at: None,
                last_heartbeat_at: None,
                created_at: String::new(),
                completed_at: None,
                abort_started_at: None,
            },
            // For an Aborting row, the PID-dead reason is suppressed by
            // build_stale_reason — the timeout is the load-bearing
            // signal.
            pid_alive: false,
            tmux_alive: None,
            orphan_worker_count: 0,
            live_worker_count: 0,
            abort_age_secs: Some(700),
        };
        let reason = build_stale_reason(&info);
        assert!(
            reason.contains("abort timeout (700s in aborting state)"),
            "expected abort timeout text, got: {reason}"
        );
        // PID-dead is NOT included for Aborting rows (status filter).
        assert!(
            !reason.contains("pid 99 dead"),
            "PID-dead text should not appear for Aborting rows, got: {reason}"
        );
    }
}
