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

/// Information about a potentially stale session.
#[derive(Debug)]
pub struct StaleSessionInfo {
    pub session: AutorunSession,
    pub pid_alive: bool,
    pub tmux_alive: Option<bool>,
    pub heartbeat_alive: Option<bool>,
    pub heartbeat_age_secs: Option<u64>,
    pub orphan_worker_count: usize,
    pub live_worker_count: usize,
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

/// Check if the heartbeat file for a session is fresh.
///
/// Returns `(alive, age_secs)`. If the heartbeat file does not exist,
/// returns `(true, None)` to avoid marking old sessions (pre-heartbeat)
/// as stale.
#[must_use]
pub fn check_heartbeat_alive(
    project_dir: &Path,
    session_id: &str,
    threshold_secs: u64,
) -> (bool, Option<u64>) {
    let heartbeat_path = project_dir
        .join(".state/autorun")
        .join(format!("heartbeat-{session_id}"));

    if !heartbeat_path.exists() {
        // No heartbeat file -- don't assume stale (pre-heartbeat session).
        return (true, None);
    }

    match heartbeat_path.metadata().and_then(|m| m.modified()) {
        Ok(mtime) => {
            let age = mtime.elapsed().map(|d| d.as_secs()).unwrap_or(0);
            (age <= threshold_secs, Some(age))
        }
        Err(_) => (true, None),
    }
}

/// Determine if a session is stale based on the three liveness signals.
///
/// A session is stale if:
/// - PID is dead, OR
/// - tmux is known dead AND heartbeat is NOT known fresh, OR
/// - heartbeat is known dead (Some(false))
///
/// A fresh heartbeat (Some(true)) overrides a dead tmux signal because
/// the orchestrator process may have restarted its tmux session.
#[must_use]
pub fn is_session_stale(
    pid_alive: bool,
    tmux_alive: Option<bool>,
    heartbeat_alive: Option<bool>,
) -> bool {
    if !pid_alive {
        return true;
    }
    if tmux_alive == Some(false) && heartbeat_alive != Some(true) {
        return true;
    }
    if heartbeat_alive == Some(false) {
        return true;
    }
    false
}

/// Detect all stale sessions from the database.
///
/// # Errors
///
/// Returns `AutorunError` if the database query fails.
pub async fn detect_stale_sessions<S: DataStore>(
    store: &S,
    project_dir: &Path,
    stale_threshold_secs: u64,
) -> Result<Vec<StaleSessionInfo>, AutorunError> {
    let filter = crate::models::AutorunSessionFilter {
        all: true,
        limit: Some(50),
        ..Default::default()
    };
    let sessions = store
        .list_autorun_sessions(filter)
        .await
        .map_err(|e| AutorunError::WorkerFailed(format!("listing sessions: {e}")))?;

    let running: Vec<_> = sessions
        .into_iter()
        .filter(|s| matches!(s.status, AutorunSessionStatus::Running))
        .collect();

    let mut stale = Vec::new();
    for session in running {
        let pid_alive = session.pid.is_some_and(check_pid_alive);
        let tmux_alive = session.tmux_session.as_deref().map(check_tmux_alive);
        let (heartbeat_alive_val, heartbeat_age) =
            check_heartbeat_alive(project_dir, &session.id, stale_threshold_secs);
        // Only report heartbeat status if file exists (age is Some).
        let heartbeat_alive = heartbeat_age.map(|_| heartbeat_alive_val);

        if !is_session_stale(pid_alive, tmux_alive, heartbeat_alive) {
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
            heartbeat_alive,
            heartbeat_age_secs: heartbeat_age,
            orphan_worker_count: orphan_count,
            live_worker_count: live_count,
        });
    }

    Ok(stale)
}

/// Clean up a single stale session and all its resources.
///
/// # Errors
///
/// Returns `AutorunError` if the session cannot be read from the database.
pub async fn cleanup_stale_session<S: DataStore>(
    store: &S,
    project_dir: &Path,
    session_id: &str,
    stale_reason: &str,
) -> Result<CleanupReport, AutorunError> {
    let mut report = CleanupReport {
        session_id: session_id.to_string(),
        ..Default::default()
    };

    // Re-read session to verify it's still Running.
    let session = store
        .get_autorun_session(session_id)
        .await
        .map_err(|e| AutorunError::WorkerFailed(format!("reading session: {e}")))?;

    let Some(session) = session else {
        return Ok(report);
    };

    if !matches!(session.status, AutorunSessionStatus::Running) {
        return Ok(report);
    }

    // Process each worker.
    let workers = store
        .list_autorun_workers(session_id)
        .await
        .unwrap_or_default();

    for worker in &workers {
        if worker.status != AutorunWorkerStatus::Running {
            continue;
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
        if let Some(ref wt_path) = worker.worktree_path {
            let registry_path = project_dir.join(".state/worktrees");
            let _ = crate::worktree::locked_deregister_worktree(&registry_path, wt_path);
            // Remove the worktree directory if it still exists.
            let wt = Path::new(wt_path);
            if wt.exists() {
                let _ = std::process::Command::new("git")
                    .args(["worktree", "remove", "--force", wt_path])
                    .current_dir(project_dir)
                    .output();
            }
            report.worktrees_removed += 1;
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

    // Update task runs.
    let task_runs = store
        .list_autorun_task_runs(session_id)
        .await
        .unwrap_or_default();

    for run in &task_runs {
        match run.status {
            AutorunTaskRunStatus::Running => {
                let _ = store
                    .update_autorun_task_run(
                        &run.id,
                        AutorunTaskRunUpdate {
                            status: Some(AutorunTaskRunStatus::Failed),
                            error_message: Some(format!("stale cleanup: {stale_reason}")),
                            completed_at: Some(chrono::Utc::now().to_rfc3339()),
                            ..Default::default()
                        },
                    )
                    .await;
                report.task_runs_failed += 1;
            }
            AutorunTaskRunStatus::Pending => {
                let _ = store
                    .update_autorun_task_run(
                        &run.id,
                        AutorunTaskRunUpdate {
                            status: Some(AutorunTaskRunStatus::Skipped),
                            error_message: Some(format!("stale cleanup: {stale_reason}")),
                            completed_at: Some(chrono::Utc::now().to_rfc3339()),
                            ..Default::default()
                        },
                    )
                    .await;
                report.task_runs_skipped += 1;
            }
            _ => {}
        }
    }

    // Update session to Failed with stale_reason.
    let _ = store
        .update_autorun_session(
            session_id,
            AutorunSessionUpdate {
                status: Some(AutorunSessionStatus::Failed),
                stale_reason: Some(Some(stale_reason.to_string())),
                completed_at: Some(chrono::Utc::now().to_rfc3339()),
                ..Default::default()
            },
        )
        .await;

    // Clean up heartbeat file.
    let heartbeat_path = project_dir
        .join(".state/autorun")
        .join(format!("heartbeat-{session_id}"));
    let _ = std::fs::remove_file(&heartbeat_path);

    Ok(report)
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
    let stale = detect_stale_sessions(store, project_dir, stale_threshold_secs).await?;
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
fn build_stale_reason(info: &StaleSessionInfo) -> String {
    let mut reasons = Vec::new();
    if !info.pid_alive {
        reasons.push(format!("pid {} dead", info.session.pid.unwrap_or(0)));
    }
    if info.tmux_alive == Some(false) {
        reasons.push(format!(
            "tmux '{}' dead",
            info.session.tmux_session.as_deref().unwrap_or("?")
        ));
    }
    if info.heartbeat_alive == Some(false) {
        reasons.push(format!(
            "heartbeat stale ({}s old)",
            info.heartbeat_age_secs.unwrap_or(0)
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

    #[test]
    fn test_is_session_stale_pid_dead() {
        assert!(is_session_stale(false, None, None));
    }

    #[test]
    fn test_is_session_stale_tmux_dead() {
        assert!(is_session_stale(true, Some(false), None));
    }

    #[test]
    fn test_is_session_stale_tmux_dead_heartbeat_fresh() {
        // Fresh heartbeat overrides dead tmux -- not stale.
        assert!(!is_session_stale(true, Some(false), Some(true)));
    }

    #[test]
    fn test_is_session_stale_heartbeat_dead() {
        assert!(is_session_stale(true, None, Some(false)));
    }

    #[test]
    fn test_is_session_stale_all_alive() {
        assert!(!is_session_stale(true, Some(true), Some(true)));
    }

    #[test]
    fn test_is_session_stale_no_tmux_info() {
        assert!(!is_session_stale(true, None, Some(true)));
    }

    #[test]
    fn test_is_session_stale_no_heartbeat_info() {
        assert!(!is_session_stale(true, Some(true), None));
    }

    #[test]
    fn test_is_session_stale_no_info() {
        assert!(!is_session_stale(true, None, None));
    }

    #[test]
    fn test_check_heartbeat_alive_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let (alive, age) = check_heartbeat_alive(dir.path(), "nonexistent-session", 90);
        assert!(alive, "no heartbeat file should not be stale");
        assert!(age.is_none(), "no file means no age");
    }

    #[test]
    fn test_check_heartbeat_alive_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let autorun_dir = dir.path().join(".state/autorun");
        std::fs::create_dir_all(&autorun_dir).unwrap();
        let hb_path = autorun_dir.join("heartbeat-test-session");
        std::fs::write(&hb_path, "").unwrap();
        let (alive, age) = check_heartbeat_alive(dir.path(), "test-session", 90);
        assert!(alive, "just-created heartbeat should be fresh");
        assert!(age.is_some(), "existing file should have age");
        assert!(age.unwrap() < 5, "age should be near zero");
    }

    #[test]
    fn test_check_heartbeat_alive_expired() {
        let dir = tempfile::tempdir().unwrap();
        let autorun_dir = dir.path().join(".state/autorun");
        std::fs::create_dir_all(&autorun_dir).unwrap();
        let hb_path = autorun_dir.join("heartbeat-expired-session");
        std::fs::write(&hb_path, "").unwrap();
        // Set mtime to 200 seconds ago.
        let old_time = filetime::FileTime::from_system_time(
            std::time::SystemTime::now() - std::time::Duration::from_secs(200),
        );
        filetime::set_file_mtime(&hb_path, old_time).unwrap();
        let (alive, age) = check_heartbeat_alive(dir.path(), "expired-session", 90);
        assert!(!alive, "old heartbeat should be stale");
        assert!(age.unwrap() >= 190, "age should reflect actual staleness");
    }

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
                created_at: String::new(),
                completed_at: None,
            },
            pid_alive: false,
            tmux_alive: None,
            heartbeat_alive: None,
            heartbeat_age_secs: None,
            orphan_worker_count: 0,
            live_worker_count: 0,
        };
        let reason = build_stale_reason(&info);
        assert!(reason.contains("pid 12345 dead"));
    }

    #[test]
    fn test_build_stale_reason_multiple() {
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
                created_at: String::new(),
                completed_at: None,
            },
            pid_alive: false,
            tmux_alive: Some(false),
            heartbeat_alive: Some(false),
            heartbeat_age_secs: Some(300),
            orphan_worker_count: 2,
            live_worker_count: 0,
        };
        let reason = build_stale_reason(&info);
        assert!(reason.contains("pid 99 dead"));
        assert!(reason.contains("tmux 'cf-orch-x' dead"));
        assert!(reason.contains("heartbeat stale (300s old)"));
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
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
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
            started_at: None,
            completed_at: None,
        }
    }

    // -- Async tests for detect_stale_sessions --

    #[tokio::test]
    async fn test_detect_stale_sessions_none_stale() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        // Create a Running session with the current process PID (alive).
        let session = make_session(
            "ses-alive",
            AutorunSessionStatus::Running,
            Some(i64::from(std::process::id())),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let result = detect_stale_sessions(&store, dir.path(), 90).await.unwrap();
        assert!(
            result.is_empty(),
            "live session should not be detected as stale"
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

    #[tokio::test]
    async fn test_detect_stale_sessions_expired_heartbeat() {
        use crate::store::DataStore;
        let store = crate::store::mock::MockStore::new();
        // Alive PID but expired heartbeat.
        let session = make_session(
            "ses-hb-stale",
            AutorunSessionStatus::Running,
            Some(i64::from(std::process::id())),
        );
        store.create_autorun_session(&session).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let autorun_dir = dir.path().join(".state/autorun");
        std::fs::create_dir_all(&autorun_dir).unwrap();
        let hb_path = autorun_dir.join("heartbeat-ses-hb-stale");
        std::fs::write(&hb_path, "").unwrap();
        let old_time = filetime::FileTime::from_system_time(
            std::time::SystemTime::now() - std::time::Duration::from_secs(200),
        );
        filetime::set_file_mtime(&hb_path, old_time).unwrap();

        let result = detect_stale_sessions(&store, dir.path(), 90).await.unwrap();
        assert_eq!(
            result.len(),
            1,
            "expired heartbeat should trigger stale detection"
        );
        assert_eq!(result[0].heartbeat_alive, Some(false));
    }

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
        // Two stale sessions (dead PIDs), one alive.
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
            Some(i64::from(std::process::id())),
        );
        store.create_autorun_session(&s1).await.unwrap();
        store.create_autorun_session(&s2).await.unwrap();
        store.create_autorun_session(&s3).await.unwrap();

        let dir = tempfile::tempdir().unwrap();
        let summary = sweep_stale_sessions(&store, dir.path(), 90).await.unwrap();
        assert_eq!(
            summary.sessions_cleaned, 2,
            "both stale sessions should be cleaned"
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
        let summary = sweep_stale_sessions(&store, dir.path(), 90).await.unwrap();
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
}
