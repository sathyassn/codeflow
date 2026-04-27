//! Session-end hook handlers.
//!
//! Implements `SessionEndCleanup` as a `HookHandler` implementation.
//! This mirrors the Go handler in `internal/hooks/session/end.go`.
//!
//! Note: `SessionEndLogging` (session-end ledger event) is in
//! `hooks::logging::SessionEndLogging` -- the logging module handles all
//! session lifecycle logging.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono;

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput, PathflowTeamInfo};
use crate::pathflow;
use crate::session;

/// Time source for deterministic testing.
pub type NowFn = fn() -> String;

// ---------------------------------------------------------------------------
// SessionEndCleanup
// ---------------------------------------------------------------------------

/// Output from a session-end cleanup operation.
#[derive(Debug, Clone)]
pub struct CleanupResult {
    /// The `CODEFLOW_SESSION_ID` used for this session.
    pub session_id: String,
    /// `true` when the `pathflow-pf-7` sentinel was found.
    pub pf7_valid: bool,
    /// Count of sentinel directories removed.
    pub sentinels_cleaned: u32,
    /// `true` when an active task was kept for the next session.
    pub task_preserved: bool,
    /// Team name read from `pathflow-team.json` (if any).
    pub team_name: String,
    /// Non-fatal warning messages.
    pub warnings: Vec<String>,
    /// Informational messages for stderr.
    pub messages: Vec<String>,
}

/// Session-end cleanup handler with injectable dependencies.
///
/// Performs the cleanup flow: `PathFlow` guard, PF7 validation,
/// sentinel cleanup, active task handling, worktree cleanup, team backstop,
/// runtime cleanup, and ledger event writing.
pub struct SessionEndCleanup {
    pub home_dir: PathBuf,
    /// Claude Code process PID of this agent (via `parent_id()` in hook).
    pub lead_pid: u32,
    pub now: NowFn,
}

#[allow(clippy::unused_self)]
impl SessionEndCleanup {
    /// Run the full session-end cleanup flow.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures.
    pub fn run(
        &self,
        _input: &HookInput,
        project_dir: &Path,
        writer: &mut dyn Write,
    ) -> Result<CleanupResult, HookError> {
        let mut result = CleanupResult {
            session_id: String::new(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // --- Section 2: Resolve session ID ---
        let Ok(sid) = session::current_session_id(project_dir) else {
            result
                .messages
                .push("no session ID found, skipping cleanup".into());
            write_messages(writer, &result.messages)?;
            return Ok(result);
        };
        let session_id = sid.as_str().to_string();
        result.session_id.clone_from(&session_id);

        let session_state_dir = project_dir.join(".state").join("session").join(&session_id);

        // Resolve the main repo root for cleanup logs. In worktree mode,
        // project_dir points to the worktree (which gets deleted during cleanup).
        // Writing logs there would either lose them or re-create orphaned directories.
        let log_root = crate::worktree::WorktreeManager::resolve_effective_root(project_dir);

        // Log cleanup start.
        write_cleanup_log(
            &log_root,
            &serde_json::json!({
                "event": "cleanup_started",
                "session_id": session_id,
                "pid": self.lead_pid,
            }),
        );

        // --- Section 3: PathFlow guard ---
        // The guard runs BEFORE any side effect (including mark_pending_cleanup).
        // Previously mark_pending_cleanup ran first so a crashing session's
        // worktree would still be queued for removal. That created a race:
        // a live session whose status file momentarily read stale (e.g. team
        // config gone but process alive) had its worktree marked PendingCleanup,
        // which then let the NEXT session's clean_stale_worktrees physically
        // reap it mid-run. With is_session_alive integrated into
        // should_skip_cleanup as a five-veto safety net, live sessions skip
        // both the marker and the cleanup, so the marker is only applied when
        // we actually proceed.
        if self.should_skip_cleanup(&session_state_dir, &session_id, project_dir, &mut result) {
            write_cleanup_log(
                &log_root,
                &serde_json::json!({
                    "event": "cleanup_skipped",
                    "session_id": session_id,
                    "reason": "session_active",
                }),
            );
            write_messages(writer, &result.messages)?;
            return Ok(result);
        }

        // Mark worktree for cleanup now that we've decided to proceed.
        // This is the primary PendingCleanup trigger.
        let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
        let _ = crate::worktree::mark_pending_cleanup(&registry_path, &session_id);

        // --- Section 4: PF7 diagnostic ---
        result.pf7_valid = self.validate_pf7(project_dir, &session_id, &mut result);

        // --- Section 5: PathFlow sentinel cleanup ---
        self.clean_pathflow_sentinels(project_dir, &session_id, &mut result);

        // --- Section 6: Active task preservation ---
        self.handle_active_task(project_dir, &mut result);

        // --- Section 7: Worktree cleanup ---
        self.clean_worktree(project_dir, &mut result);

        // --- Section 7b: Claim TTL eviction ---
        Self::compact_expired_claims(project_dir, &mut result);

        // --- Section 8: Read team info ---
        let team_name = self.read_team_name(&session_state_dir);
        result.team_name.clone_from(&team_name);

        // --- Section 9: Team config/task list backstop cleanup ---
        self.clean_team_artifacts(&team_name, &session_state_dir, &mut result);

        // --- Section 10: Session state directory cleanup ---
        self.clean_session_state(&session_state_dir, &session_id, &mut result);

        // --- Section 11: Runtime file cleanup ---
        self.clean_runtime_files(project_dir, &mut result);

        // --- Section 12: Project temp directory cleanup ---
        self.clean_project_temp(project_dir);

        // --- Section 13: InteractiveSession status update ---
        // Mark the InteractiveSession as complete in DB (non-blocking).
        Self::complete_interactive_session(project_dir, &session_id);

        // Note: session_end ledger event is written by SessionEndLogging
        // handler (hooks::logging module), not here.

        // Log cleanup completion.
        write_cleanup_log(
            &log_root,
            &serde_json::json!({
                "event": "cleanup_completed",
                "session_id": session_id,
                "pf7_valid": result.pf7_valid,
                "sentinels_cleaned": result.sentinels_cleaned,
                "task_preserved": result.task_preserved,
                "team_name": result.team_name,
                "warnings_count": result.warnings.len(),
            }),
        );

        write_messages(writer, &result.messages)?;

        Ok(result)
    }

    /// Check the `PathFlow` guard using multi-signal approach. Returns `true`
    /// if cleanup should be skipped (session still active).
    ///
    /// Uses `pathflow-session-status.json` as the primary authority, with
    /// team config existence as a secondary check, and the five-veto
    /// `is_session_alive` predicate as a final safety net so a spurious
    /// SessionEnd (misrouted teammate stop, etc.) cannot reap a live session.
    fn should_skip_cleanup(
        &self,
        session_state_dir: &Path,
        session_id: &str,
        project_dir: &Path,
        result: &mut CleanupResult,
    ) -> bool {
        let pathflow_dir = session_state_dir.join("pathflow");
        let status_path = pathflow_dir.join("pathflow-session-status.json");

        // Read under critical retry -- cleanup is destructive; avoid false-negative on transient read.
        let status_result: Result<serde_json::Value, String> =
            crate::pathflow::file_lock::locked_read_critical(&status_path, 2);

        // Five-veto safety net: if ANY live signal fires for this session
        // (tmux pane alive, fresh heartbeat, lead_pid still running, grace
        // window active, self-match), skip cleanup regardless of what the
        // status file says. This stops spurious SessionEnd invocations from
        // reaping live sessions.
        let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH")
            .ok()
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from);
        let alive_inputs = crate::session::liveness::SessionAliveInputs {
            session_id: session_id.to_string(),
            project_dir: project_dir.to_path_buf(),
            worktree_path,
            registry_created_at: None,
        };
        let aggregate_alive = crate::session::liveness::is_session_alive(&alive_inputs);

        let status = match status_result {
            Ok(v) => v,
            Err(e) => {
                if e.contains("read:") {
                    // No status file. Normally this means "no active
                    // PathFlow session", BUT a session that just spawned
                    // can race here -- the status file is written milliseconds
                    // after SessionStart. Defer to is_session_alive.
                    if aggregate_alive {
                        result.messages.push(
                            "SessionEnd: status file missing but session still alive (five-veto) -- skipping cleanup".into(),
                        );
                        return true;
                    }
                    return false;
                }
                result.messages.push(format!(
                    "SessionEnd: pathflow-session-status.json unreadable ({e}) -- proceeding"
                ));
                return false;
            }
        };

        let session_status = status.get("status").and_then(|v| v.as_str()).unwrap_or("");

        // Status "created" means no team was ever created -- safe to clean up,
        // unless is_session_alive says otherwise (race window).
        if session_status == "created" {
            if aggregate_alive {
                result.messages.push(
                    "SessionEnd: status='created' but five-veto says alive -- skipping cleanup"
                        .into(),
                );
                return true;
            }
            result
                .messages
                .push("SessionEnd: Session status is 'created' (no team) -- proceeding".into());
            return false;
        }

        // Status "pf-complete" means session finished normally -- clean up.
        // Do NOT defer to aggregate_alive here: the status file is the
        // authoritative signal that PF7 completed, and the five-veto tmux
        // veto may still see the lead's tmux pane during teardown.
        if session_status == "pf-complete" {
            result.messages.push(
                "SessionEnd: Session status is 'pf-complete' -- proceeding with cleanup".into(),
            );
            return false;
        }

        // Status is "pf-started" or "pf-in-progress" -- session may be active.
        let team_name = status
            .get("team_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if team_name.is_empty() {
            if aggregate_alive {
                result.messages.push(
                    "SessionEnd: no team_name but five-veto says alive -- skipping cleanup".into(),
                );
                return true;
            }
            result
                .messages
                .push("SessionEnd: Session active but no team name -- proceeding".into());
            return false;
        }

        // Check if team config still exists (team not yet deleted).
        let config_path = self
            .home_dir
            .join(".claude")
            .join("teams")
            .join(team_name)
            .join("config.json");

        if !config_path.exists() {
            // Config is gone and session status is pf-started/pf-in-progress.
            // Under the legacy semantics this was "dead -- proceed", but a
            // legitimately live session whose TeamDelete just ran (and whose
            // pf-complete status write is imminent) matches this exact
            // pattern. Gate on aggregate_alive so tmux / lead_pid / heartbeat
            // still protect live sessions.
            if aggregate_alive {
                result.messages.push(format!(
                    "SessionEnd: team config gone for '{team_name}' but five-veto says alive -- skipping cleanup"
                ));
                return true;
            }
            result.messages.push(format!(
                "SessionEnd: Session active but team config gone for '{team_name}' -- proceeding"
            ));
            return false;
        }

        // Edge case: status stuck but PF7 already completed.
        let last_phase = status
            .get("last_completed_phase")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if last_phase == "pf-7" {
            result.messages.push(
                "SessionEnd: Status stuck but PF7 completed -- proceeding with cleanup".into(),
            );
            return false;
        }

        // All status-file signals indicate active session -- skip cleanup.
        // This is the pre-existing behaviour; we add the five-veto decision
        // to the log so post-hoc debugging can tell whether the veto or the
        // status file caused the skip.
        result.messages.push(format!(
            "SessionEnd: Session active (status={session_status}, team={team_name}, phase={last_phase}, five_veto_alive={aggregate_alive}) -- skipping cleanup"
        ));
        true
    }

    /// Validate PF7 completion (check for `pathflow-pf-7` sentinel).
    fn validate_pf7(
        &self,
        project_dir: &Path,
        session_id: &str,
        result: &mut CleanupResult,
    ) -> bool {
        // Sentinels are LOCAL state (not symlinked). In worktree mode,
        // check the worktree path first.
        let sentinel_base = std::env::var("CODEFLOW_WORKTREE_PATH")
            .ok()
            .filter(|p| !p.is_empty() && Path::new(p).exists())
            .map_or_else(|| project_dir.to_path_buf(), PathBuf::from);

        let sentinel_dir = sentinel_base
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);

        let pf7_exists =
            pathflow::sentinel::check_sentinel(&sentinel_dir, crate::types::Sentinel::PathflowPf7);

        if pf7_exists {
            result
                .messages
                .push("SessionEnd: Clean PF7 shutdown (all phases completed)".into());
        } else {
            result
                .messages
                .push("SessionEnd: Incomplete PF7 shutdown (pf-7 sentinel absent)".into());

            if let Ok(sentinels) = pathflow::sentinel::list_sentinels(&sentinel_dir) {
                if !sentinels.is_empty() {
                    result.messages.push(format!(
                        "SessionEnd: existing sentinels: {}",
                        sentinels.join(", ")
                    ));
                }
            }
        }

        pf7_exists
    }

    /// Remove all `PathFlow` sentinels for this session.
    fn clean_pathflow_sentinels(
        &self,
        project_dir: &Path,
        session_id: &str,
        result: &mut CleanupResult,
    ) {
        // Sentinels are LOCAL state (not symlinked). In worktree mode,
        // check the worktree path first.
        let sentinel_base = std::env::var("CODEFLOW_WORKTREE_PATH")
            .ok()
            .filter(|p| !p.is_empty() && Path::new(p).exists())
            .map_or_else(|| project_dir.to_path_buf(), PathBuf::from);

        let sentinel_dir = sentinel_base
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(session_id);

        if sentinel_dir.exists() && fs::remove_dir_all(&sentinel_dir).is_ok() {
            result.sentinels_cleaned += 1;
        }
    }

    /// Handle active task preservation (keep `in_progress`, remove completed).
    /// Worktree-aware: checks worktree-local path first, then project-level.
    /// On cleanup, clears BOTH locations (dual-cleanup pattern).
    fn handle_active_task(&self, project_dir: &Path, result: &mut CleanupResult) {
        let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
        self.handle_active_task_inner(project_dir, worktree_path.as_deref(), result);
    }

    /// Inner implementation with explicit worktree path for testability.
    fn handle_active_task_inner(
        &self,
        project_dir: &Path,
        worktree_path: Option<&str>,
        result: &mut CleanupResult,
    ) {
        match session::active_task::get_active_task_resolved(project_dir, worktree_path) {
            Ok(Some(task)) => {
                if let Some(status) = &task.status {
                    if status == "in_progress" {
                        result.task_preserved = true;
                        result.messages.push(format!(
                            "SessionEnd: Task {} preserved for next session",
                            task.task_id.as_str()
                        ));
                        return;
                    }
                }
                // Dual-cleanup: clear from both worktree and project dir.
                if let Some(wt_path) = worktree_path {
                    let wp = crate::worktree::WorktreePaths::new(wt_path);
                    let _ = session::clear_active_task(&wp.runtime_dir());
                }
                let project_runtime = project_dir.join(".state").join("runtime");
                let _ = session::clear_active_task(&project_runtime);
            }
            Ok(None) => {}
            Err(_) => {
                // Unreadable -- clean both locations.
                if let Some(wt_path) = worktree_path {
                    let wp = crate::worktree::WorktreePaths::new(wt_path);
                    let _ = session::clear_active_task(&wp.runtime_dir());
                }
                let project_runtime = project_dir.join(".state").join("runtime");
                let _ = session::clear_active_task(&project_runtime);
            }
        }
    }

    /// Remove the session's worktree (if present).
    ///
    /// Reads `CODEFLOW_WORKTREE_PATH` from env. If set, extracts the
    /// worktree name from the path basename and calls
    /// `WorktreeManager::cleanup(name, force=true)`. Also prunes stale
    /// git worktree references afterward.
    ///
    /// Errors are non-fatal: logged as warnings, never abort cleanup.
    fn clean_worktree(&self, project_dir: &Path, result: &mut CleanupResult) {
        let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
        self.clean_worktree_inner(project_dir, worktree_path.as_deref(), result);
    }

    /// Inner implementation with explicit worktree path for testability.
    fn clean_worktree_inner(
        &self,
        project_dir: &Path,
        worktree_path: Option<&str>,
        result: &mut CleanupResult,
    ) {
        let Some(wt_path) = worktree_path else {
            return; // Non-worktree session — nothing to clean.
        };

        let wt_name = Path::new(wt_path)
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_string());
        if wt_name.is_empty() {
            result
                .warnings
                .push("SessionEnd: worktree path has no basename, skipping cleanup".into());
            return;
        }

        // Branch safety check before cleanup — warn but proceed (SessionEnd always uses force).
        let wt_dir = Path::new(wt_path);
        if wt_dir.exists() {
            let safety = crate::worktree::check_branch_safety(wt_dir);
            if safety.risk >= crate::worktree::BranchRisk::High {
                result.warnings.push(format!(
                    "SessionEnd: branch safety warning for '{wt_name}': {} (proceeding with force cleanup)",
                    safety.message
                ));
            }
        }

        let mgr = crate::worktree::WorktreeManager::new(project_dir);
        let opts = crate::worktree::CleanupOpts {
            force: true,
            ..Default::default()
        };

        match mgr.cleanup(&wt_name, &opts) {
            Ok(()) => {
                result
                    .messages
                    .push(format!("SessionEnd: Cleaned worktree '{wt_name}'"));
            }
            Err(e) => {
                result.warnings.push(format!(
                    "SessionEnd: worktree cleanup failed for '{wt_name}': {e}"
                ));
            }
        }

        // Prune stale git worktree references (idempotent).
        let prune_opts = crate::worktree::CleanupOpts {
            prune: true,
            ..Default::default()
        };
        if let Err(e) = mgr.cleanup("", &prune_opts) {
            result
                .warnings
                .push(format!("SessionEnd: worktree prune failed: {e}"));
        }

        // Remove this session from the session-worktree mapping.
        let map_path = project_dir
            .join(".state")
            .join("runtime")
            .join("session-worktree-map.json");
        if map_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&map_path) {
                if let Ok(mut map) =
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&data)
                {
                    if let Some(sid) = wt_name.strip_prefix("worktree-") {
                        map.remove(sid);
                    }
                    let _ = std::fs::write(
                        &map_path,
                        serde_json::to_string_pretty(&map).unwrap_or_default(),
                    );
                }
            }
        }
    }

    /// Proactively evict expired claim entries from the Loro CRDT document.
    ///
    /// Uses `locked_binary_rmw` to hold the sidecar lock for the entire
    /// read-modify-write cycle, matching the concurrency pattern used by
    /// all other claim operations. Non-fatal: errors are logged as warnings.
    fn compact_expired_claims(project_dir: &Path, result: &mut CleanupResult) {
        let state_path = project_dir
            .join(".state")
            .join("coordination")
            .join("state.loro");

        if !state_path.exists() {
            return; // No coordination state — nothing to compact.
        }

        use crate::coordination::claims::compact_expired_claims as compact;
        use crate::coordination::loro::LoroCoordinator;

        let rmw_result = crate::file_lock::locked_binary_rmw(
            &state_path,
            LoroCoordinator::in_memory,
            |bytes| {
                LoroCoordinator::from_bytes(bytes, &state_path)
                    .map_err(|e| format!("loro load: {e}"))
            },
            |coord| coord.export_bytes().map_err(|e| format!("loro save: {e}")),
            |coord| {
                match compact(coord) {
                    Ok(count) => {
                        if count > 0 {
                            result
                                .messages
                                .push(format!("SessionEnd: compacted {count} expired claim(s)"));
                        }
                    }
                    Err(e) => {
                        result
                            .warnings
                            .push(format!("SessionEnd: claim compaction failed: {e}"));
                    }
                }
                Ok(())
            },
        );

        if let Err(e) = rmw_result {
            result.warnings.push(format!(
                "SessionEnd: claim compaction failed (lock/io): {e}"
            ));
        }
    }

    /// Read the team name from `pathflow-team.json`.
    fn read_team_name(&self, session_state_dir: &Path) -> String {
        let team_file_path = session_state_dir
            .join("pathflow")
            .join("pathflow-team.json");

        fs::read_to_string(&team_file_path)
            .ok()
            .and_then(|data| serde_json::from_str::<PathflowTeamInfo>(&data).ok())
            .map(|info| info.team_name)
            .unwrap_or_default()
    }

    /// Remove stale team config and task list directories.
    ///
    /// Re-checks session status before deleting to guard against race conditions
    /// where the session became active again between the skip check and this call.
    fn clean_team_artifacts(
        &self,
        team_name: &str,
        session_state_dir: &Path,
        result: &mut CleanupResult,
    ) {
        if team_name.is_empty() {
            return;
        }

        // Re-check session status under shared lock -- guard against race where session reactivated.
        let status_path = session_state_dir
            .join("pathflow")
            .join("pathflow-session-status.json");
        if let Ok(status) = crate::pathflow::file_lock::locked_read(&status_path) {
            let s = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if s == "pf-started" || s == "pf-in-progress" {
                result.messages.push(format!(
                    "SessionEnd: Skipping team artifact cleanup -- session still active (status={s})"
                ));
                return;
            }
        }

        let teams_dir = self.home_dir.join(".claude").join("teams").join(team_name);
        if teams_dir.exists() && fs::remove_dir_all(&teams_dir).is_ok() {
            result
                .messages
                .push(format!("SessionEnd: Cleaned team config: {team_name}"));
        }

        let tasks_dir = self.home_dir.join(".claude").join("tasks").join(team_name);
        if tasks_dir.exists() {
            let _ = fs::remove_dir_all(&tasks_dir);
        }
    }

    /// Remove the session state directory.
    ///
    /// Also cleans up orphaned `.lock` files in the pathflow subdirectory
    /// (both main repo and worktree paths). Lock files may outlive sessions
    /// if the process crashed while holding a lock.
    fn clean_session_state(
        &self,
        session_state_dir: &Path,
        _session_id: &str,
        _result: &mut CleanupResult,
    ) {
        // Clean lock files from pathflow directory before removing.
        let pathflow_dir = session_state_dir.join("pathflow");
        clean_lock_files(&pathflow_dir);

        // Also clean worktree-local lock files if in worktree mode.
        if let Ok(wt_path) = std::env::var("CODEFLOW_WORKTREE_PATH") {
            if let Some(sid_name) = session_state_dir.file_name() {
                let wt_pathflow = Path::new(&wt_path)
                    .join(".state")
                    .join("session")
                    .join(sid_name)
                    .join("pathflow");
                clean_lock_files(&wt_pathflow);
            }
        }

        if session_state_dir.exists() {
            let _ = fs::remove_dir_all(session_state_dir);
        }
    }

    /// Remove runtime files (env file, session lock).
    ///
    /// Worktree-aware: checks `CODEFLOW_WORKTREE_PATH` env var. If set, cleans
    /// the worktree-local env file first, then the main project env file.
    ///
    /// Race safety: reads `codeflow-env.sh` and only removes it if the session ID
    /// inside matches the session being cleaned up. If a different session owns the
    /// file (concurrent session started between cleanup phases), it is preserved.
    fn clean_runtime_files(&self, project_dir: &Path, result: &mut CleanupResult) {
        let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
        self.clean_runtime_files_inner(project_dir, worktree_path.as_deref(), result);
    }

    /// Inner implementation with explicit worktree path for testability.
    fn clean_runtime_files_inner(
        &self,
        project_dir: &Path,
        worktree_path: Option<&str>,
        result: &mut CleanupResult,
    ) {
        // Clean worktree-local env file first (if worktree is active).
        if let Some(wt_path) = worktree_path {
            let wp = crate::worktree::WorktreePaths::new(wt_path);
            let wt_runtime_dir = wp.runtime_dir();
            self.race_safe_env_cleanup(&wt_runtime_dir, result);
        }

        // Clean main project env file.
        let runtime_dir = project_dir.join(".state").join("runtime");
        self.race_safe_env_cleanup(&runtime_dir, result);

        // Remove per-PID env file for this Claude Code process.
        session::remove_pid_env_file(&runtime_dir, self.lead_pid);

        // Remove session pointer from main repo.
        if !result.session_id.is_empty() {
            session::remove_session_pointer(project_dir, &result.session_id);
        }

        // Remove heartbeat file (signals clean exit to stale sweep).
        crate::session::heartbeat::remove(project_dir);

        // Remove interactive heartbeat file for this session.
        if !result.session_id.is_empty() {
            let interactive_hb = project_dir
                .join(".state/interactive")
                .join(format!("heartbeat-{}", result.session_id));
            let _ = fs::remove_file(&interactive_hb);
        }

        // Remove session lock file (always in main project).
        let lock_path = runtime_dir.join("session.lock");
        if lock_path.exists() {
            let _ = fs::remove_file(&lock_path);
        }

        // Legacy: remove current-session-id if it exists (no longer written,
        // kept for one-time cleanup of old installations).
        let legacy_sid_path = runtime_dir.join("current-session-id");
        if legacy_sid_path.exists() {
            let _ = fs::remove_file(&legacy_sid_path);
        }
    }

    /// Race-safe env file removal: reads the file and only removes it if the
    /// session ID matches the session being cleaned up.
    fn race_safe_env_cleanup(&self, runtime_dir: &Path, result: &mut CleanupResult) {
        match session::read_env_file(runtime_dir) {
            Ok(Some(env)) => {
                let file_sid = env.session_id.as_str().to_string();
                if file_sid == result.session_id {
                    let _ = session::remove_env_file(runtime_dir);
                } else {
                    result.warnings.push(format!(
                        "SessionEnd: env file owned by different session ({file_sid}), preserving"
                    ));
                }
            }
            Ok(None) => {} // No env file, nothing to remove.
            Err(_) => {
                // Unreadable env file -- may belong to another session mid-write.
                // Preserve it; next SessionStart will overwrite if needed.
                result
                    .warnings
                    .push("SessionEnd: env file unreadable, preserving".to_string());
            }
        }
    }

    /// Remove the project temp directory.
    ///
    /// Reads `CODEFLOW_WORKTREE_PATH` to scope cleanup. Delegates to the
    /// inner function for testability (avoids env-var dependency in tests).
    fn clean_project_temp(&self, project_dir: &Path) {
        let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
        Self::clean_project_temp_inner(project_dir, worktree_path.as_deref());
    }

    /// Testable inner function for temp directory cleanup.
    ///
    /// With a worktree path: removes only `/tmp/claude/{project}/{wt-name}/`.
    /// Without: removes `/tmp/claude/{project}/managed/` (legacy fallback).
    /// NEVER removes `/tmp/claude/{project}/` — only the subdirectory.
    fn clean_project_temp_inner(project_dir: &Path, worktree_path: Option<&str>) {
        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());

        if let Some(wt_path) = worktree_path {
            // Scoped cleanup: remove only this worktree's subdirectory.
            let wt_name = Path::new(wt_path)
                .file_name()
                .map_or_else(|| "default".into(), |n| n.to_string_lossy().to_string());
            let scoped_dir = PathBuf::from("/tmp/claude")
                .join(&project_name)
                .join(&wt_name);
            if scoped_dir.exists() {
                let _ = fs::remove_dir_all(&scoped_dir);
            }
        } else {
            // Legacy fallback: remove managed/ only (no worktree).
            let tmp_dir = PathBuf::from("/tmp/claude")
                .join(&project_name)
                .join("managed");
            if tmp_dir.exists() {
                let _ = fs::remove_dir_all(&tmp_dir);
            }
        }
    }

    /// Mark InteractiveSession as complete in DB (non-blocking).
    ///
    /// Works for both managed (`codeflow -i`) and unmanaged (`claude`) sessions.
    /// Uses a new tokio runtime because hook handlers run in a sync context.
    fn complete_interactive_session(project_dir: &Path, session_id: &str) {
        let db_dir = project_dir.join(".state/db");
        if !db_dir.exists() {
            return;
        }
        let sid = session_id.to_string();
        // INF-TSK-050-001 AC #16: surface DB write failures to stderr
        // instead of silently swallowing them via `.ok()?`. Errors stay
        // non-fatal (we don't break session-end on a DB hiccup) but
        // become visible to operators so a degraded DB state can be
        // diagnosed instead of treated as success.
        let update = async move {
            let store = crate::store::SurrealStore::open(&db_dir)
                .await
                .map_err(|e| format!("opening store: {e}"))?;
            let now = chrono::Utc::now().to_rfc3339();
            let _: Option<serde_json::Value> = store
                .db()
                .query(
                    "UPDATE interactive_session SET status = 'complete', \
                     completed_at = $now, updated_at = $now \
                     WHERE session_id = $sid AND status = 'active'",
                )
                .bind(("now", now))
                .bind(("sid", sid))
                .await
                .map_err(|e| format!("UPDATE interactive_session: {e}"))?
                .take(0)
                .map_err(|e| format!("take UPDATE result: {e}"))?;
            Ok::<(), String>(())
        };
        // Hooks run as #[tokio::main] processes; use block_in_place to avoid
        // creating a nested runtime.
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            if let Err(e) = tokio::task::block_in_place(|| handle.block_on(update)) {
                eprintln!("warn: session_end interactive_session complete failed: {e}");
            }
        } else if let Ok(rt) = tokio::runtime::Runtime::new() {
            if let Err(e) = rt.block_on(update) {
                eprintln!("warn: session_end interactive_session complete failed: {e}");
            }
        }
    }
}

impl HookHandler for SessionEndCleanup {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input.project_dir.as_deref().ok_or_else(|| {
            HookError::Config("project_dir required for session-end cleanup".into())
        })?;

        let mut buf = Vec::new();
        self.run(&input, Path::new(project_dir), &mut buf)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-end-cleanup"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionEnd]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Remove orphaned `.lock` files from a directory.
///
/// Lock files (e.g., `pathflow-session-status.json.lock`) can be left behind
/// when a process crashes while holding a file lock.
fn clean_lock_files(dir: &Path) {
    if !dir.exists() {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("lock") {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Write a structured JSONL cleanup event to `.state/logs/sessions/cleanup-{date}.jsonl`.
fn write_cleanup_log(project_dir: &Path, event: &serde_json::Value) {
    let now = chrono::Utc::now();
    let date = now.format("%Y-%m-%d").to_string();
    let log_dir = project_dir.join(".state").join("logs").join("sessions");
    let _ = fs::create_dir_all(&log_dir);
    let log_path = log_dir.join(format!("cleanup-{date}.jsonl"));

    let mut entry = event.clone();
    if let Some(obj) = entry.as_object_mut() {
        obj.insert(
            "timestamp".into(),
            serde_json::Value::String(now.to_rfc3339()),
        );
    }

    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            });
    }
}

fn write_messages(writer: &mut dyn Write, messages: &[String]) -> Result<(), HookError> {
    for msg in messages {
        writeln!(writer, "{msg}").map_err(HookError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    fn fixed_now() -> String {
        "2026-03-10T01:00:00Z".to_string()
    }

    fn make_cleaner(home: PathBuf) -> SessionEndCleanup {
        SessionEndCleanup {
            home_dir: home,
            lead_pid: 1000,
            now: fixed_now,
        }
    }

    fn make_input(project_dir: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
            session_id: None,
            project_dir: Some(project_dir.into()),
            source: None,
            transcript_path: None,
            ..Default::default()
        }
    }

    /// Set up a project directory with a valid session for cleanup testing.
    fn setup_session(dir: &Path) -> String {
        let runtime_dir = dir.join(".state").join("runtime");
        let sid = SessionId::new_unchecked("ses-01jq7cleanup1234567890ab");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Create session state directories.
        let session_dir = dir
            .join(".state")
            .join("session")
            .join(sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&session_dir).unwrap();

        // Write pathflow-team.json with a dead lead_pid so V4 of
        // is_session_alive votes DEAD (not abstain). Tests that need to
        // exercise the cleanup-proceeds path depend on aggregate_alive =
        // false; without this file V4 abstains alive and every cleanup
        // would skip. Tests that want to simulate a live session override
        // this file with a live lead_pid.
        fs::write(
            session_dir.join("pathflow-team.json"),
            serde_json::json!({"team_name": "", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        // Create sentinel directory with some sentinels.
        let sentinel_dir = dir
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(sid.as_str());
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), b"").unwrap();

        // Create ledger directory.
        fs::create_dir_all(dir.join(".state").join("ledger")).unwrap();

        sid.as_str().to_string()
    }

    // --- SessionEndCleanup tests ---

    #[test]
    fn test_cleanup_full_flow() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert_eq!(result.session_id, session_id);
        assert!(!result.pf7_valid); // No pf-7 sentinel.
        assert_eq!(result.sentinels_cleaned, 1);

        // Verify sentinel directory was removed.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        assert!(!sentinel_dir.exists());

        // Verify env file was removed.
        let runtime_dir = dir.path().join(".state").join("runtime");
        assert!(session::read_env_file(&runtime_dir).unwrap().is_none());
    }

    #[test]
    fn test_cleanup_skip_when_session_active() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create pathflow-session-status.json with active status.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "test-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-4",
            "last_completed_stage": "WS-DEV",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:30:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        // Create Claude Code team config so the team existence check passes.
        let config_dir = home.path().join(".claude").join("teams").join("test-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev"}]}"#,
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Cleanup should be skipped (session active with team config present).
        assert_eq!(result.sentinels_cleaned, 0);
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("skipping cleanup"))
        );

        // Verify sentinel directory still exists.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        assert!(sentinel_dir.exists());
    }

    #[test]
    fn test_cleanup_proceeds_when_session_complete() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create pathflow-session-status.json with pf-complete status.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "test-team",
            "status": "pf-complete",
            "last_completed_phase": "pf-7",
            "last_completed_stage": "WS-QA",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T01:00:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Cleanup should proceed (session completed normally).
        assert!(result.messages.iter().any(|m| m.contains("pf-complete")));
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_pf7_valid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create the pf-7 sentinel.
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        pathflow::sentinel::create_sentinel(&sentinel_dir, crate::types::Sentinel::PathflowPf7)
            .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.pf7_valid);
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Clean PF7 shutdown"))
        );
    }

    #[test]
    fn test_cleanup_task_preservation() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        setup_session(dir.path());

        // Set an in_progress active task.
        let runtime_dir = dir.path().join(".state").join("runtime");
        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-preserve"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: None,
            status: Some("in_progress".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            work_type: None,
            scope_policy: None,
            file_scope: None,
            target_branch: None,
            auto_merge: None,
            epic_update: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.task_preserved);
        assert!(result.messages.iter().any(|m| m.contains("preserved")));

        // Active task file should still exist.
        assert!(session::get_active_task(&runtime_dir).unwrap().is_some());
    }

    #[test]
    fn test_cleanup_no_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        // No env file exists.
        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.session_id.is_empty());
        assert!(result.messages.iter().any(|m| m.contains("no session ID")));
    }

    #[test]
    fn test_cleanup_handler_trait_error_on_missing_project_dir() {
        let home = tempfile::tempdir().unwrap();
        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionEnd,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };

        let result = cleaner.handle(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project_dir required")
        );
    }

    #[test]
    fn test_cleanup_proceeds_when_team_config_gone() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file claiming active, but NO team config directory.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "missing-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-3",
            "last_completed_stage": "",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:15:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();
        // V4 needs a team.json with dead lead_pid to vote DEAD (not abstain).
        fs::write(
            pathflow_dir.join("pathflow-team.json"),
            serde_json::json!({"team_name": "missing-team", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (team config gone).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("team config gone"))
        );
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_proceeds_when_empty_team_name() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file with active status but empty team_name.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "",
            "status": "pf-started",
            "last_completed_phase": "",
            "last_completed_stage": "",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:05:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();
        // V4 needs a team.json with dead lead_pid to vote DEAD (not abstain).
        fs::write(
            pathflow_dir.join("pathflow-team.json"),
            serde_json::json!({"team_name": "", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (no team name).
        assert!(result.messages.iter().any(|m| m.contains("no team name")));
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_proceeds_when_status_created() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file with "created" status (no team ever created).
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "",
            "status": "created",
            "last_completed_phase": "",
            "last_completed_stage": "",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:00:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();
        // V4 needs a team.json with dead lead_pid to vote DEAD (not abstain).
        fs::write(
            pathflow_dir.join("pathflow-team.json"),
            serde_json::json!({"team_name": "", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (status is "created").
        assert!(result.messages.iter().any(|m| m.contains("created")));
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_proceeds_when_pf7_stuck_status() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file with active status but last_completed_phase = PF7
        // (status stuck at pf-in-progress but PF7 already done).
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "stuck-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-7",
            "last_completed_stage": "WS-QA",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T01:00:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        // Team config exists.
        let config_dir = home.path().join(".claude").join("teams").join("stuck-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(
            config_dir.join("config.json"),
            r#"{"members": [{"name": "cf-dev"}]}"#,
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (PF7 completed, status just stuck).
        assert!(result.messages.iter().any(|m| m.contains("PF7 completed")));
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_no_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        setup_session(dir.path());

        // No pathflow-session-status.json and no legacy flag -- should proceed.
        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (no status file present).
        assert!(result.sentinels_cleaned > 0);
    }

    #[test]
    fn test_cleanup_no_status_file_allows_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // No status file -- cleanup should proceed (no active PathFlow session).
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // No status file means no active session -- cleanup proceeds.
        assert_eq!(result.session_id, session_id);
    }

    #[test]
    fn test_cleanup_removes_session_lock() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create a session.lock file in runtime dir.
        let runtime_dir = dir.path().join(".state").join("runtime");
        let lock_path = runtime_dir.join("session.lock");
        fs::write(&lock_path, b"").unwrap();
        assert!(lock_path.exists(), "lock file should exist before cleanup");

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Verify lock file was removed.
        assert!(
            !lock_path.exists(),
            "session.lock should be removed during cleanup"
        );

        // Verify env file was also removed (existing behavior).
        assert!(session::read_env_file(&runtime_dir).unwrap().is_none());

        // Verify session_id was resolved correctly (cleanup proceeded).
        assert_eq!(result.session_id, session_id);
    }

    #[test]
    fn test_cleanup_proceeds_when_config_gone_but_sentinels_exist() {
        // Bug 2 regression test: Previously, when team config was gone but
        // sentinels existed, should_skip_cleanup returned true (skip), creating
        // a deadlock for abandoned sessions. Now it returns false (proceed).
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Create status file claiming active session.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(&session_id)
            .join("pathflow");
        let status = serde_json::json!({
            "session_id": session_id,
            "team_name": "dead-team",
            "status": "pf-in-progress",
            "last_completed_phase": "pf-4",
            "last_completed_stage": "WS-DEV",
            "created_at": "2026-03-10T00:00:00Z",
            "updated_at": "2026-03-10T00:30:00Z",
        });
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();
        // Write pathflow-team.json with dead lead_pid so V4 votes DEAD
        // (without this file, V4 abstains ALIVE and cleanup would skip).
        fs::write(
            pathflow_dir.join("pathflow-team.json"),
            serde_json::json!({"team_name": "dead-team", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        // Create sentinel files (would have caused deadlock before fix).
        let sentinel_dir = dir
            .path()
            .join(".state")
            .join("sentinels")
            .join("pathflow")
            .join(&session_id);
        fs::create_dir_all(&sentinel_dir).unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-1"), b"").unwrap();
        fs::write(sentinel_dir.join("pathflow-pf-3"), b"").unwrap();
        fs::write(sentinel_dir.join("pathflow-ws-dev"), b"").unwrap();

        // NO team config directory -- team was deleted or never existed.

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Should proceed with cleanup (team config gone, sentinels irrelevant).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("team config gone")),
            "should proceed with cleanup when config is gone: {:?}",
            result.messages
        );
        assert!(
            result.sentinels_cleaned > 0,
            "sentinels should be cleaned up"
        );
    }

    // --- Worktree-aware cleanup tests ---

    #[test]
    fn test_cleanup_removes_worktree_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());
        let sid = SessionId::new_unchecked(&session_id);

        // Write env file to worktree location too.
        let wt_runtime = worktree_dir.path().join(".state").join("runtime");
        session::write_env_file_with_worktree(
            &wt_runtime,
            &sid,
            "codeflow",
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: session_id.clone(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // Call inner with explicit worktree path.
        cleaner.clean_runtime_files_inner(
            dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
            &mut result,
        );

        // Both env files should be removed.
        let project_runtime = dir.path().join(".state").join("runtime");
        assert!(
            session::read_env_file(&project_runtime).unwrap().is_none(),
            "project env file should be removed"
        );
        assert!(
            session::read_env_file(&wt_runtime).unwrap().is_none(),
            "worktree env file should be removed"
        );
    }

    #[test]
    fn test_cleanup_preserves_worktree_env_owned_by_other_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let worktree_dir = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        // Write env file to worktree with a DIFFERENT session ID.
        let other_sid = SessionId::new_unchecked("ses-01jq7othersid12345678901");
        let wt_runtime = worktree_dir.path().join(".state").join("runtime");
        session::write_env_file_with_worktree(
            &wt_runtime,
            &other_sid,
            "codeflow",
            Some(worktree_dir.path().to_str().unwrap()),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: session_id.clone(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        cleaner.clean_runtime_files_inner(
            dir.path(),
            Some(worktree_dir.path().to_str().unwrap()),
            &mut result,
        );

        // Worktree env file should be preserved (different session).
        assert!(
            session::read_env_file(&wt_runtime).unwrap().is_some(),
            "worktree env file owned by other session should be preserved"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("different session")),
            "should warn about different session ownership"
        );
    }

    #[test]
    fn test_cleanup_without_worktree_only_cleans_project() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: session_id.clone(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // No worktree path -- only project cleanup.
        cleaner.clean_runtime_files_inner(dir.path(), None, &mut result);

        let project_runtime = dir.path().join(".state").join("runtime");
        assert!(
            session::read_env_file(&project_runtime).unwrap().is_none(),
            "project env file should be removed"
        );
        assert!(
            result.warnings.is_empty(),
            "no warnings expected: {:?}",
            result.warnings
        );
    }

    // --- clean_project_temp_inner tests ---

    #[test]
    fn test_clean_project_temp_scoped_to_worktree() {
        let base = tempfile::tempdir().unwrap();
        let project_name = base
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Create two worktree temp dirs under /tmp/claude/{project}/.
        let wt_a_dir = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("worktree-ses-aaa")
            .join("managed");
        let wt_b_dir = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("worktree-ses-bbb")
            .join("managed");
        fs::create_dir_all(&wt_a_dir).unwrap();
        fs::create_dir_all(&wt_b_dir).unwrap();

        // Clean session A only.
        SessionEndCleanup::clean_project_temp_inner(
            base.path(),
            Some("/proj/.git-worktrees/worktree-ses-aaa"),
        );

        // Session A's dir is gone (parent worktree-ses-aaa removed).
        assert!(
            !PathBuf::from("/tmp/claude")
                .join(&project_name)
                .join("worktree-ses-aaa")
                .exists(),
            "session A's worktree dir should be removed"
        );
        // Session B's dir is untouched.
        assert!(
            wt_b_dir.exists(),
            "session B's temp dir should be preserved"
        );

        // Cleanup.
        let _ = fs::remove_dir_all(PathBuf::from("/tmp/claude").join(&project_name));
    }

    #[test]
    fn test_clean_project_temp_preserves_other_worktrees() {
        let base = tempfile::tempdir().unwrap();
        let project_name = base
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Create three worktree temp dirs.
        for name in &["worktree-ses-111", "worktree-ses-222", "worktree-ses-333"] {
            let d = PathBuf::from("/tmp/claude")
                .join(&project_name)
                .join(name)
                .join("managed")
                .join("protected-edits");
            fs::create_dir_all(&d).unwrap();
        }

        // Clean only session 222.
        SessionEndCleanup::clean_project_temp_inner(
            base.path(),
            Some("/proj/.git-worktrees/worktree-ses-222"),
        );

        let base_tmp = PathBuf::from("/tmp/claude").join(&project_name);
        assert!(
            !base_tmp.join("worktree-ses-222").exists(),
            "cleaned worktree should be gone"
        );
        assert!(
            base_tmp.join("worktree-ses-111").exists(),
            "other worktree 111 should survive"
        );
        assert!(
            base_tmp.join("worktree-ses-333").exists(),
            "other worktree 333 should survive"
        );

        // Cleanup.
        let _ = fs::remove_dir_all(base_tmp);
    }

    #[test]
    fn test_clean_project_temp_fallback_no_worktree() {
        let base = tempfile::tempdir().unwrap();
        let project_name = base
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        // Create legacy managed/ dir.
        let managed = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("managed");
        fs::create_dir_all(&managed).unwrap();

        // Clean without worktree path (legacy behavior).
        SessionEndCleanup::clean_project_temp_inner(base.path(), None);

        assert!(!managed.exists(), "legacy managed/ dir should be removed");

        // Project dir itself should still exist (we never remove it).
        // (It may or may not exist depending on whether managed/ was the only child.)

        // Cleanup.
        let _ = fs::remove_dir_all(PathBuf::from("/tmp/claude").join(&project_name));
    }

    // --- Worktree cleanup (Section 7) tests ---

    #[test]
    fn test_clean_worktree_inner_no_worktree_path() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: "ses-test".into(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // No worktree path -- should return early with no messages.
        cleaner.clean_worktree_inner(dir.path(), None, &mut result);

        assert!(
            result.messages.is_empty(),
            "no messages when no worktree path"
        );
        assert!(
            result.warnings.is_empty(),
            "no warnings when no worktree path"
        );
    }

    #[test]
    fn test_clean_worktree_inner_empty_basename() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: "ses-test".into(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // Path with no valid basename (trailing slash edge case handled by Path).
        cleaner.clean_worktree_inner(dir.path(), Some("/"), &mut result);

        assert!(
            result.warnings.iter().any(|w| w.contains("no basename")),
            "should warn about empty basename: {:?}",
            result.warnings,
        );
    }

    #[test]
    fn test_clean_worktree_inner_nonexistent_worktree() {
        // WorktreeManager.cleanup with force=true on a nonexistent worktree
        // should succeed (idempotent) — not an error.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: "ses-test".into(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        cleaner.clean_worktree_inner(
            dir.path(),
            Some("/proj/.git-worktrees/nonexistent-wt"),
            &mut result,
        );

        // Should produce a success message (cleanup of nonexistent is OK with force).
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Cleaned worktree"))
                || result.warnings.iter().any(|w| w.contains("cleanup failed")),
            "should produce a message or warning: messages={:?}, warnings={:?}",
            result.messages,
            result.warnings,
        );
    }

    #[test]
    fn test_clean_worktree_inner_with_git_repo_and_worktree() {
        // Full lifecycle: create a real git worktree, then clean it up.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        // Initialize a git repo with one commit (required for worktree creation).
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // Create worktree via the manager.
        let mgr = crate::worktree::WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees/worktrees.yaml"));
        let branch = crate::types::BranchName::new_unchecked("feat/test-cleanup");
        mgr.setup("test-wt", &branch).unwrap();

        let wt_path = mgr.base_dir().join("test-wt");
        assert!(wt_path.exists(), "worktree should exist before cleanup");

        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: "ses-test".into(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // Clean the worktree.
        cleaner.clean_worktree_inner(dir.path(), Some(wt_path.to_str().unwrap()), &mut result);

        // Worktree directory should be removed.
        assert!(
            !wt_path.exists(),
            "worktree directory should be removed after cleanup"
        );

        // Success message should be present.
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Cleaned worktree 'test-wt'")),
            "should have success message: {:?}",
            result.messages,
        );

        // Registry entry should be deleted (not tombstoned as "removed").
        let entries = mgr.list(None).unwrap();
        assert!(
            !entries.iter().any(|e| e.name == "test-wt"),
            "entry should be deleted from registry after cleanup"
        );
    }

    #[test]
    fn test_clean_worktree_inner_abnormal_termination() {
        // Simulate abnormal termination: worktree exists but no PF7.
        // cleanup(force=true) should still remove it.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let mgr = crate::worktree::WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees/worktrees.yaml"));
        let branch = crate::types::BranchName::new_unchecked("feat/crash-test");
        mgr.setup("crash-wt", &branch).unwrap();

        let wt_path = mgr.base_dir().join("crash-wt");
        assert!(wt_path.exists());

        let cleaner = make_cleaner(home.path().to_path_buf());

        let mut result = CleanupResult {
            session_id: "ses-crash".into(),
            pf7_valid: false, // Abnormal — no PF7.
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        cleaner.clean_worktree_inner(dir.path(), Some(wt_path.to_str().unwrap()), &mut result);

        assert!(
            !wt_path.exists(),
            "worktree should be removed even without PF7 (abnormal termination)"
        );
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Cleaned worktree")),
            "should report cleanup success: {:?}",
            result.messages,
        );
    }

    // -- mark_pending_cleanup at session end tests --

    /// Helper to create a worktree registry with one Active entry for the given session.
    fn create_active_registry(dir: &Path, session_id: &str) -> PathBuf {
        use crate::worktree::{WorktreeEntry, WorktreeRegistry, WorktreeStatus, write_registry};

        let registry_path = dir.join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-01-01T00:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: format!("worktree-{session_id}"),
            path: "/tmp/wt".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: WorktreeStatus::Active,
            session_id: Some(session_id.to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&registry_path, &reg).unwrap();
        registry_path
    }

    #[test]
    fn test_session_end_marks_pending_cleanup_after_pf7() {
        // Normal PF7 path: status is pf-complete, worktree should be marked PendingCleanup.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        let registry_path = create_active_registry(dir.path(), &session_id);

        // Set status to pf-complete (simulating post-PF7).
        let pathflow_dir = dir
            .path()
            .join(".state/session")
            .join(&session_id)
            .join("pathflow");
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::json!({"status": "pf-complete", "session_id": session_id}).to_string(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());
        let mut buf = Vec::new();
        let _result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Verify the entry is now PendingCleanup.
        let data = fs::read_to_string(&registry_path).unwrap();
        assert!(
            data.contains("pending_cleanup"),
            "worktree should be marked PendingCleanup after session end: {data}"
        );
    }

    #[test]
    fn test_session_end_marks_pending_cleanup_after_confirmed_crash() {
        // Confirmed-crash path: status is pf-in-progress, pathflow-team.json
        // exists with a dead lead_pid, no interactive heartbeat. Every
        // five-veto signal votes dead, so cleanup proceeds and the entry is
        // marked PendingCleanup.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        let registry_path = create_active_registry(dir.path(), &session_id);

        let pathflow_dir = dir
            .path()
            .join(".state/session")
            .join(&session_id)
            .join("pathflow");
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::json!({"status": "pf-in-progress", "session_id": session_id}).to_string(),
        )
        .unwrap();
        // Write pathflow-team.json with a dead lead_pid so V4 votes DEAD,
        // not abstain. Without this file V4 abstains alive (protecting
        // initializing sessions) which would (correctly) skip cleanup.
        fs::write(
            pathflow_dir.join("pathflow-team.json"),
            serde_json::json!({"team_name": "", "lead_pid": 4_000_000_u32}).to_string(),
        )
        .unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());
        let mut buf = Vec::new();
        let _result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Verify PendingCleanup was marked — now that the guard runs BEFORE
        // mark_pending_cleanup and the five-veto check is DEAD, cleanup proceeds.
        let data = fs::read_to_string(&registry_path).unwrap();
        assert!(
            data.contains("pending_cleanup"),
            "worktree should be marked PendingCleanup after confirmed crash: {data}"
        );
    }

    #[test]
    fn test_session_end_skips_pending_cleanup_when_session_active() {
        // When should_skip_cleanup returns true (live session), the worktree
        // MUST NOT be marked PendingCleanup — mark_pending_cleanup now runs
        // AFTER the guard. Previously it ran first, which let a later session
        // reap this live session's worktree via clean_stale_worktrees.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let session_id = setup_session(dir.path());

        let registry_path = create_active_registry(dir.path(), &session_id);

        // Set status to pf-in-progress WITH a real team config (should_skip_cleanup = true).
        let pathflow_dir = dir
            .path()
            .join(".state/session")
            .join(&session_id)
            .join("pathflow");
        let team_name = "active-team";
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::json!({"status": "pf-in-progress", "session_id": session_id, "team_name": team_name}).to_string(),
        ).unwrap();

        // Create team config so should_skip_cleanup returns true.
        let config_dir = home.path().join(".claude/teams").join(team_name);
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), "{}").unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let input = make_input(dir.path().to_str().unwrap());
        let mut buf = Vec::new();
        let _result = cleaner.run(&input, dir.path(), &mut buf).unwrap();

        // Verify PendingCleanup was NOT marked — live session must be protected.
        let data = fs::read_to_string(&registry_path).unwrap();
        assert!(
            !data.contains("pending_cleanup"),
            "worktree must NOT be marked PendingCleanup when session is alive: {data}"
        );
    }

    #[test]
    fn test_clean_runtime_files_removes_interactive_heartbeat() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7hbcleantest12345678";

        // Create runtime env file so clean_runtime_files_inner can resolve session ID.
        let runtime_dir = dir.path().join(".state/runtime");
        let sid_typed = SessionId::new_unchecked(sid);
        session::write_env_file(&runtime_dir, &sid_typed, "codeflow").unwrap();

        // Create interactive heartbeat file.
        let hb_dir = dir.path().join(".state/interactive");
        fs::create_dir_all(&hb_dir).unwrap();
        let hb_path = hb_dir.join(format!("heartbeat-{sid}"));
        fs::write(&hb_path, "2026-04-07T00:00:00Z").unwrap();
        assert!(hb_path.exists(), "heartbeat should exist before cleanup");

        let cleaner = make_cleaner(home.path().to_path_buf());
        let mut result = CleanupResult {
            session_id: sid.to_string(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };
        cleaner.clean_runtime_files_inner(dir.path(), None, &mut result);

        assert!(
            !hb_path.exists(),
            "interactive heartbeat should be removed by clean_runtime_files_inner"
        );
    }

    #[test]
    fn test_clean_runtime_files_no_panic_when_heartbeat_missing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7nohbtest123456789ab";

        let runtime_dir = dir.path().join(".state/runtime");
        let sid_typed = SessionId::new_unchecked(sid);
        session::write_env_file(&runtime_dir, &sid_typed, "codeflow").unwrap();

        let cleaner = make_cleaner(home.path().to_path_buf());
        let mut result = CleanupResult {
            session_id: sid.to_string(),
            pf7_valid: false,
            sentinels_cleaned: 0,
            task_preserved: false,
            team_name: String::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };
        // No interactive heartbeat exists — should not panic.
        cleaner.clean_runtime_files_inner(dir.path(), None, &mut result);
    }

    // -- write_cleanup_log path resolution tests --

    #[test]
    fn test_write_cleanup_log_uses_main_repo_root_not_worktree() {
        // Simulate a worktree inside .git-worktrees/ -- resolve_effective_root
        // should return the parent (main repo), so cleanup logs go there.
        let main_repo = tempfile::tempdir().unwrap();
        let wt_dir = main_repo
            .path()
            .join(".git-worktrees")
            .join("worktree-ses-test123");
        fs::create_dir_all(&wt_dir).unwrap();

        // resolve_effective_root on the worktree path should yield main_repo.
        let resolved = crate::worktree::WorktreeManager::resolve_effective_root(&wt_dir);
        assert_eq!(
            resolved,
            main_repo.path().to_path_buf(),
            "resolve_effective_root should return main repo root"
        );

        // Write a cleanup log using the resolved root.
        write_cleanup_log(
            &resolved,
            &serde_json::json!({
                "event": "test_cleanup",
                "session_id": "ses-test123",
            }),
        );

        // Log should be in main repo's .state/logs/sessions/, not worktree's.
        let main_log_dir = main_repo
            .path()
            .join(".state")
            .join("logs")
            .join("sessions");
        assert!(
            main_log_dir.exists(),
            "cleanup log dir should exist in main repo"
        );

        let wt_log_dir = wt_dir.join(".state").join("logs").join("sessions");
        assert!(
            !wt_log_dir.exists(),
            "cleanup log dir should NOT exist in worktree"
        );

        // Verify log content.
        let entries: Vec<_> = fs::read_dir(&main_log_dir).unwrap().flatten().collect();
        assert_eq!(entries.len(), 1, "should have one log file");
        let content = fs::read_to_string(entries[0].path()).unwrap();
        assert!(content.contains("test_cleanup"));
    }

    #[test]
    fn test_write_cleanup_log_non_worktree_uses_project_dir_unchanged() {
        // When project_dir is NOT a worktree, resolve_effective_root returns
        // the same path, so cleanup logs go to project_dir as before.
        let dir = tempfile::tempdir().unwrap();

        let resolved = crate::worktree::WorktreeManager::resolve_effective_root(dir.path());
        assert_eq!(
            resolved,
            dir.path().to_path_buf(),
            "non-worktree dir should resolve to itself"
        );

        write_cleanup_log(
            &resolved,
            &serde_json::json!({
                "event": "test_normal",
                "session_id": "ses-normal",
            }),
        );

        let log_dir = dir.path().join(".state").join("logs").join("sessions");
        assert!(log_dir.exists(), "log dir should exist in project dir");
    }
}
