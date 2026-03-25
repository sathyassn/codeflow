//! Session-start hook handlers.
//!
//! Implements `SessionStartInit`, `SessionStartInstructions`, and
//! `SessionStartLogging` as `HookHandler` implementations. These mirror
//! the Go handlers in `internal/hooks/session/start.go`.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::HookError;
use crate::hooks::{HookEvent, HookHandler, HookInput, HookOutput};
use crate::ledger::{Event, LedgerWriter};
use crate::pathflow;
use crate::session;
use crate::types::SessionId;
use crate::worktree::{WorktreeHandle, WorktreeManager, WorktreePaths};

// ---------------------------------------------------------------------------
// Session lock
// ---------------------------------------------------------------------------

/// Acquire an exclusive file lock on `session.lock` in `runtime_dir`.
///
/// Serializes concurrent session creation across multiple agents. The returned
/// `File` holds the lock via `flock(LOCK_EX)`. The lock is released when the
/// file is dropped.
/// Remove orphaned `.lock` files from a directory.
///
/// Lock files can be left behind when a process crashes while holding a lock.
fn clean_lock_files_in_dir(dir: &Path) {
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

fn acquire_session_lock(runtime_dir: &Path) -> Result<std::fs::File, std::io::Error> {
    fs::create_dir_all(runtime_dir)?;
    let lock_path = runtime_dir.join("session.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    fs2::FileExt::lock_exclusive(&file)?;
    Ok(file)
}

// ---------------------------------------------------------------------------
// SessionStartInit
// ---------------------------------------------------------------------------

/// Output from a successful session initialization.
#[derive(Debug, Clone)]
pub struct InitResult {
    /// The session ID resolved for this session.
    pub session_id: SessionId,
    /// `true` when an existing `pathflow-active` flag was found.
    pub is_resume: bool,
    /// `true` when joining an active session (teammate mode).
    pub is_teammate: bool,
    /// Environment variables to output for the hook framework.
    pub env_vars: HashMap<String, String>,
    /// Non-fatal warning messages.
    pub warnings: Vec<String>,
    /// Informational messages for stdout.
    pub messages: Vec<String>,
}

/// Time source for deterministic testing.
pub type NowFn = fn() -> String;

/// Session-start init handler with injectable dependencies.
///
/// Performs the 11-section initialization flow (section 12 auto-rebuild is
/// skipped in `codeflow-core`; it belongs in the CLI binary).
pub struct SessionStartInit {
    /// Claude Code process PID (obtained via `parent_id()` in the hook process).
    /// Used as `lead_pid` in status file for teammate detection and stale sweep.
    pub lead_pid: u32,
    pub home_dir: PathBuf,
    pub now: NowFn,
}

/// Check if a process with the given PID is alive.
///
/// Uses `kill -0 <pid>` which sends no signal but exits 0 if the process
/// exists (equivalent to `libc::kill(pid, 0)` without requiring `unsafe`).
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Clean orphaned worktrees before new worktree registration.
///
/// Three cleanup passes:
/// 1. Purge "removed" entries from the registry.
/// 2. Deregister entries whose directories no longer exist on disk.
/// 3. Prune git worktree references that are not in the registry.
///
/// Errors are logged but never fail the session start.
fn clean_orphaned_worktrees(project_dir: &Path, mgr: &WorktreeManager) {
    // Pass 1: Purge "removed" entries (keep 0 — remove all).
    if let Err(e) = crate::worktree::purge_removed_entries(mgr.registry_path(), 0) {
        eprintln!("warn: worktree orphan cleanup: purge_removed failed: {e}");
    }

    // Pass 2: Deregister entries whose directories don't exist.
    if let Ok(registry) = crate::worktree::read_registry(mgr.registry_path()) {
        for entry in &registry.worktrees {
            if entry.status == "active" && !Path::new(&entry.path).exists() {
                eprintln!(
                    "info: worktree orphan cleanup: deregistering missing dir: {}",
                    entry.name
                );
                let _ = crate::worktree::deregister_by_name(mgr.registry_path(), &entry.name);
            }
        }
    }

    // Pass 3: Prune git worktree references not in the registry.
    // Use git2 to list worktrees known to git, then remove any that
    // aren't in the registry.
    if let Ok(repo) = git2::Repository::open(project_dir) {
        if let Ok(wt_names) = repo.worktrees() {
            let registry_names: std::collections::HashSet<String> =
                crate::worktree::read_registry(mgr.registry_path())
                    .map(|r| r.worktrees.iter().map(|e| e.name.clone()).collect())
                    .unwrap_or_default();

            for name in wt_names.iter().flatten() {
                if !registry_names.contains(name) {
                    // Git knows about this worktree but the registry doesn't.
                    // Prune it from git.
                    if let Ok(wt) = repo.find_worktree(name) {
                        let _ = wt.prune(Some(
                            git2::WorktreePruneOptions::new()
                                .valid(false)
                                .working_tree(true),
                        ));
                        eprintln!(
                            "info: worktree orphan cleanup: pruned git ref: {name}"
                        );
                    }
                }
            }
        }
    }
}

/// Verify that a newly created worktree is visible in git and registry.
///
/// Emits warnings to `result` if verification fails, but never blocks.
fn verify_worktree_creation(
    project_dir: &Path,
    wt_name: &str,
    wt_path: &Path,
    result: &mut InitResult,
) {
    // Check directory exists on disk.
    if !wt_path.exists() {
        result
            .warnings
            .push(format!("worktree dir missing after creation: {}", wt_path.display()));
        return;
    }

    // Check git knows about the worktree.
    if let Ok(repo) = git2::Repository::open(project_dir) {
        if let Ok(wt_names) = repo.worktrees() {
            let found = wt_names.iter().flatten().any(|n| n == wt_name);
            if !found {
                result.warnings.push(format!(
                    "worktree '{wt_name}' not found in git worktree list after creation"
                ));
            }
        }
    }
}

#[allow(clippy::unused_self)]
impl SessionStartInit {
    /// Run the full session-start initialization flow.
    ///
    /// `project_dir` is the absolute path to the repository root.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures or missing required state.
    pub fn run(
        &self,
        input: &HookInput,
        project_dir: &Path,
        writer: &mut dyn Write,
    ) -> Result<InitResult, HookError> {
        let source = input.source.as_deref().unwrap_or("unknown");
        let claude_session_id = input.session_id.as_deref().unwrap_or("");

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let runtime_dir = project_dir.join(".state").join("runtime");

        // --- Section 0: Acquire session lock ---
        // Serialize concurrent session creation across multiple agents.
        // Lock is acquired before stale cleanup and released after env file write.
        let lock_file = match acquire_session_lock(&runtime_dir) {
            Ok(f) => Some(f),
            Err(e) => {
                result
                    .warnings
                    .push(format!("session lock: {e} (proceeding without lock)"));
                None
            }
        };

        // --- Section 1: Status-based stale session cleanup + teammate detection ---
        let (existing_sid, team_mode) =
            self.handle_stale_cleanup(project_dir, &runtime_dir, source, &mut result);

        if team_mode {
            if let Some(sid) = &existing_sid {
                result.is_teammate = true;
                result.session_id = sid.clone();
                result.messages.push(format!(
                    "TEAMMATE MODE: You are a teammate joining session {}.",
                    sid.as_str()
                ));
                result
                    .env_vars
                    .insert("CODEFLOW_SESSION_ID".into(), sid.as_str().to_string());
                let project_name = project_dir
                    .file_name()
                    .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
                result
                    .env_vars
                    .insert("CF_PROJECT_ROOT".into(), project_name);

                // Release session lock -- teammate detected, env file already exists.
                drop(lock_file);

                write_env_json(writer, &result.env_vars)?;
                return Ok(result);
            }
        }

        // --- Section 1b: Stale worktree cleanup (startup only) ---
        if source == "startup" {
            self.clean_stale_worktrees(project_dir, &mut result);
        }

        // --- Section 2: Session ID generation (source-gated) ---
        let session_id = self.resolve_or_generate_session_id(
            project_dir,
            &runtime_dir,
            source,
            claude_session_id,
            existing_sid,
            &mut result,
        )?;
        result.session_id = session_id.clone();

        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
        result.env_vars.insert(
            "CODEFLOW_SESSION_ID".into(),
            session_id.as_str().to_string(),
        );
        result
            .env_vars
            .insert("CF_PROJECT_ROOT".into(), project_name.clone());

        // --- Section 2a: EARLY env file write ---
        // Write env file immediately after session ID generation so that
        // downstream handlers can resolve the session even if Init times out
        // during worktree creation or later operations. This write includes
        // an empty CODEFLOW_WORKTREE_PATH placeholder so that any code reading
        // the env var gets a defined (but empty) value rather than a missing key.
        // The worktree path is updated in the UPDATE write after worktree creation.
        if source == "startup" {
            if let Err(e) = session::write_env_file_with_worktree(
                &runtime_dir,
                &session_id,
                &project_name,
                Some(""),
            ) {
                result
                    .warnings
                    .push(format!("early env file write error: {e}"));
            }
        }

        // --- Section 2b: Worktree creation (source-gated + mode-gated) ---
        // On startup: check worktree.mode from parallel-work config, then:
        //   mode=disabled: skip worktree creation entirely
        //   mode=autorun: only use pre-created worktree (orchestrator created it)
        //   mode=always: create worktree for every session
        // On compact/resume/clear: recover existing worktree path from env file.
        // WorktreeHandle provides RAII cleanup on panic; defuse() at end of run().
        let (worktree_paths, mut worktree_handle) = if source == "startup" {
            // Load worktree mode from parallel-work config.
            let wt_mode = crate::autorun::config::load_config(project_dir)
                .map(|c| c.worktree.mode)
                .unwrap_or(crate::autorun::config::WorktreeMode::Autorun);

            match wt_mode {
                crate::autorun::config::WorktreeMode::Disabled => {
                    // Worktrees disabled — skip creation entirely.
                    (None, None)
                }
                crate::autorun::config::WorktreeMode::Autorun => {
                    // Autorun mode: only use pre-created worktree from orchestrator.
                    if let Some(paths) = self.detect_precreated_worktree(
                        project_dir,
                        &runtime_dir,
                        session_id.as_str(),
                        &mut result,
                    ) {
                        (Some(paths), None) // No handle — orchestrator manages lifecycle
                    } else {
                        // No pre-created worktree and mode=autorun — skip for interactive.
                        (None, None)
                    }
                }
                crate::autorun::config::WorktreeMode::Always => {
                    // Always mode: check for pre-created, otherwise create new.
                    if let Some(paths) = self.detect_precreated_worktree(
                        project_dir,
                        &runtime_dir,
                        session_id.as_str(),
                        &mut result,
                    ) {
                        (Some(paths), None)
                    } else {
                        match self.create_session_worktree(
                            project_dir,
                            session_id.as_str(),
                            &mut result,
                        ) {
                            Ok((paths, handle)) => (Some(paths), Some(handle)),
                            Err(e) => {
                                result
                                    .warnings
                                    .push(format!("worktree creation failed: {e}"));
                                (None, None)
                            }
                        }
                    }
                }
            }
        } else {
            // Compact/resume/clear: recover worktree path from main project env file.
            let recovered = self.recover_worktree_path(&runtime_dir, &mut result);
            (recovered, None)
        };

        // Export CODEFLOW_WORKTREE_PATH if we have a worktree.
        if let Some(ref wp) = worktree_paths {
            let wt_path_str = wp.root().to_string_lossy().to_string();
            result
                .env_vars
                .insert("CODEFLOW_WORKTREE_PATH".into(), wt_path_str);
        }

        // Release session lock -- env file written, teammates can now detect this session.
        drop(lock_file);

        // --- Section 3: Directory creation ---
        // When a worktree is available, create directories inside the worktree.
        // Otherwise, fall back to the main project directory.
        if let Some(ref wp) = worktree_paths {
            self.create_directories_in_worktree(wp, session_id.as_str());
        } else {
            self.create_directories(project_dir, session_id.as_str());
        }

        // --- Section 4+5: Sweep all stale sessions (replaces detect_stale_sessions + sweep_orphan_sentinels) ---
        // Stale sweep always operates on main project dir (shared state).
        if source == "startup" || source == "unknown" {
            self.sweep_all_stale_sessions(project_dir, session_id.as_str());
        }

        // --- Section 6: Active task context expiry ---
        // Active task cleanup operates on main project dir.
        self.cleanup_active_task(project_dir);

        // --- Section 7: PathFlow flag creation ---
        // When a worktree is available, PathFlow flag goes in the worktree.
        let flag_base = worktree_paths
            .as_ref()
            .map_or_else(|| project_dir.to_path_buf(), |wp| wp.root().to_path_buf());
        let is_resume = self.create_pathflow_flag(
            &flag_base,
            session_id.as_str(),
            source,
            result.is_teammate,
            &mut result,
        );
        result.is_resume = is_resume;

        // --- Section 7c: Checkpoint pre-initialization ---
        // Checkpoint goes in the same base as pathflow flag.
        self.init_checkpoint(&flag_base, session_id.as_str(), &mut result);

        // --- Section 8: Session metadata ---
        // Eliminated: session-meta.json is superseded by pathflow-session-status.json
        // which now contains lead_pid, source_at_start, and latest_source fields.

        // --- Section 9: Compact recovery detection ---
        self.detect_compact_recovery(project_dir, source, &mut result);

        // --- Section 10: Project temp directory ---
        self.create_project_temp_dir(project_dir, worktree_paths.as_ref(), &mut result);

        // Write env file inside worktree with CODEFLOW_WORKTREE_PATH.
        if let Some(ref wp) = worktree_paths {
            let wt_runtime = wp.runtime_dir();
            let wt_path_str = wp.root().to_string_lossy().to_string();
            if let Err(e) = session::write_env_file_with_worktree(
                &wt_runtime,
                &session_id,
                &project_name,
                Some(&wt_path_str),
            ) {
                result
                    .warnings
                    .push(format!("worktree env file write error: {e}"));
            }

            // Also write main repo env file with CODEFLOW_WORKTREE_PATH.
            // This enables detect_project_dir() Priority 3b fallback: when hooks
            // run from the main repo CWD, they can read the worktree path from
            // the main repo's env file and redirect to the correct worktree.
            let main_runtime = project_dir.join(".state").join("runtime");
            if let Err(e) = session::write_env_file_with_worktree(
                &main_runtime,
                &session_id,
                &project_name,
                Some(&wt_path_str),
            ) {
                result
                    .warnings
                    .push(format!("main repo env redirect write error: {e}"));
            }
        }

        // Surface collected warnings to stderr so they appear in hook output.
        for warning in &result.warnings {
            eprintln!("[codeflow session-start] warning: {warning}");
        }

        // Write env JSON to stdout
        write_env_json(writer, &result.env_vars)?;

        // Defuse the worktree handle -- ownership transfers to SessionEnd hook.
        // If we panic before this point, Drop fires and cleans up the worktree.
        if let Some(ref mut handle) = worktree_handle {
            handle.defuse();
        }

        Ok(result)
    }

    /// Handle stale session cleanup and teammate detection using three
    /// file-based signals only (no env var reads):
    ///
    /// 1. `codeflow-env.sh` -> existing SID (pointer to current session)
    /// 2. `pathflow-session-status.json` for that SID -> session state + team_name
    /// 3. `~/.claude/teams/{team_name}/config.json` -> team alive?
    ///
    /// Returns `(existing_session_id, is_teammate_mode)`.
    fn handle_stale_cleanup(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        source: &str,
        result: &mut InitResult,
    ) -> (Option<SessionId>, bool) {
        // Signal 1: Read env file for existing SID.
        let env_data = match session::read_env_file(runtime_dir) {
            Ok(Some(env)) => env,
            Ok(None) => return (None, false), // No env file -> new lead
            Err(e) => {
                result
                    .warnings
                    .push(format!("stale cleanup: env read error: {e}"));
                return (None, false);
            }
        };

        let existing_sid = env_data.session_id;

        // Compact/resume/clear: reuse existing SID, update source tracking only.
        if source == "compact" || source == "resume" || source == "clear" {
            // Update latest_source in status file for recovery context.
            let session_dir = project_dir
                .join(".state")
                .join("session")
                .join(existing_sid.as_str())
                .join("pathflow");
            crate::hooks::post_tool_use::update_session_status(
                &session_dir,
                &serde_json::json!({
                    "latest_source": source,
                    "latest_source_at": (self.now)(),
                }),
            );
            return (Some(existing_sid), false);
        }

        // Startup/unknown: check if an active session exists (teammate detection).
        if existing_sid.as_str().is_empty() {
            return (None, false);
        }

        // Signal 2: pathflow-session-status.json
        let status_path = project_dir
            .join(".state")
            .join("session")
            .join(existing_sid.as_str())
            .join("pathflow")
            .join("pathflow-session-status.json");

        let status: serde_json::Value =
            match crate::pathflow::file_lock::locked_read_critical(&status_path, 3) {
                Ok(v) => v,
                Err(_) => return (None, false), // Missing/unreadable/unparseable -> new lead
            };
        let session_status = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if session_status.is_empty() || session_status == "pf-complete" {
            return (None, false); // No status or completed -> new lead
        }

        // Status is created/pf-started/pf-in-progress.
        let team_name = status
            .get("team_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if team_name.is_empty() {
            return (None, false); // No team name -> new lead
        }

        // Signal 3: Team config existence.
        let config_path = self
            .home_dir
            .join(".claude")
            .join("teams")
            .join(team_name)
            .join("config.json");
        if !config_path.exists() {
            return (None, false); // Config missing -> session dead, new lead
        }

        // Signal 4: Lead process liveness via PID.
        // The lead_pid is the Claude Code process PID stored at session creation.
        // If the lead is alive, this caller is a teammate joining the session.
        // If the lead is dead, the session crashed and this is a new lead.
        let lead_pid = status
            .get("lead_pid")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0);
        if lead_pid == 0 {
            // No lead_pid recorded (pre-PID session) — cannot verify liveness.
            // Default to new lead (safe: avoids trapping a new lead as teammate).
            return (None, false);
        }

        if !is_process_alive(lead_pid) {
            result.messages.push(format!(
                "STALE SESSION: lead_pid {lead_pid} is dead (team: {team_name}) — new lead"
            ));
            return (None, false); // Lead crashed -> new lead
        }

        // Active session with live lead process -> TEAMMATE.
        result.messages.push(format!(
            "TEAMMATE MODE: Detected via status + config + lead_pid {lead_pid} alive (team: {team_name})"
        ));
        (Some(existing_sid), true)
    }

    /// Clean stale worktrees from dead sessions.
    ///
    /// Reads the worktrees.yaml registry and checks each active worktree's
    /// owning session. If the session is dead (lead_pid not alive), the
    /// worktree is force-cleaned.
    ///
    /// Called at startup (source=startup) only, after stale session cleanup.
    fn clean_stale_worktrees(&self, project_dir: &Path, result: &mut InitResult) {
        let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
        if !registry_path.exists() {
            return;
        }

        let registry = match crate::worktree::read_registry(&registry_path) {
            Ok(reg) => reg,
            Err(e) => {
                result
                    .warnings
                    .push(format!("stale worktree cleanup: registry read error: {e}"));
                return;
            }
        };

        let mgr = crate::worktree::WorktreeManager::new(project_dir);
        let opts = crate::worktree::CleanupOpts {
            force: true,
            ..Default::default()
        };

        for entry in &registry.worktrees {
            if entry.status != "active" {
                continue;
            }

            // Check if the owning session is still alive via its status file.
            let Some(ref sid) = entry.session_id else {
                // No session_id recorded — cannot verify liveness, skip.
                continue;
            };

            let status_path = project_dir
                .join(".state")
                .join("session")
                .join(sid)
                .join("pathflow")
                .join("pathflow-session-status.json");

            let session_alive =
                if let Ok(status) = crate::pathflow::file_lock::locked_read(&status_path) {
                    let lead_pid = status
                        .get("lead_pid")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok())
                        .unwrap_or(0);
                    lead_pid > 0 && is_process_alive(lead_pid)
                } else {
                    false // No status file — session is dead.
                };

            if session_alive {
                continue;
            }

            // Session is dead — clean up this worktree.
            result.messages.push(format!(
                "STALE WORKTREE: cleaning '{}' from dead session {sid}",
                entry.name,
            ));
            if let Err(e) = mgr.cleanup(&entry.name, &opts) {
                result.warnings.push(format!(
                    "stale worktree cleanup failed for '{}': {e}",
                    entry.name,
                ));
            }
        }

        // Detect orphaned worktrees: on disk but not in registry.
        self.clean_orphaned_worktrees(project_dir, &registry, &mgr, result);

        // Prune stale git worktree references.
        let prune_opts = crate::worktree::CleanupOpts {
            prune: true,
            ..Default::default()
        };
        if let Err(e) = mgr.cleanup("", &prune_opts) {
            result
                .warnings
                .push(format!("stale worktree prune failed: {e}"));
        }

        // Purge old "removed" entries from the registry (keep last 5 for forensics).
        if let Err(e) = crate::worktree::purge_removed_entries(&registry_path, 5) {
            result
                .warnings
                .push(format!("purge removed entries failed: {e}"));
        }
    }

    /// Detect and clean orphaned worktrees that exist on disk but are not in
    /// the registry.
    ///
    /// Scans `.git-worktrees/` and compares against registry entries.
    /// An orphaned worktree is cleaned only if:
    /// - Its `pathflow-session-status.json` contains a dead `lead_pid`
    /// - OR the status file is missing
    /// - AND the worktree is older than 5 minutes (grace period for init)
    fn clean_orphaned_worktrees(
        &self,
        _project_dir: &Path,
        registry: &crate::worktree::WorktreeRegistry,
        mgr: &crate::worktree::WorktreeManager,
        result: &mut InitResult,
    ) {
        let base_dir = mgr.base_dir();
        if !base_dir.exists() {
            return;
        }

        // Collect registered worktree directory names.
        let registered_names: std::collections::HashSet<String> = registry
            .worktrees
            .iter()
            .filter(|e| e.status == "active")
            .filter_map(|e| {
                std::path::Path::new(&e.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .collect();

        // Scan the .git-worktrees directory for worktree-* entries.
        let entries = match fs::read_dir(&base_dir) {
            Ok(entries) => entries,
            Err(_) => return,
        };

        let grace_period = std::time::Duration::from_secs(5 * 60); // 5 minutes

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("worktree-") {
                continue;
            }

            if registered_names.contains(&name) {
                continue; // In registry -- not orphaned.
            }

            let wt_path = entry.path();

            // Grace period: skip if recently created (< 5 minutes).
            if let Ok(metadata) = fs::metadata(&wt_path) {
                if let Ok(created) = metadata.created().or_else(|_| metadata.modified()) {
                    if let Ok(age) = created.elapsed() {
                        if age < grace_period {
                            continue; // Too young -- may be initializing.
                        }
                    }
                }
            }

            // Check session status for lead_pid liveness.
            let status_path = wt_path.join(".state").join("session");

            let session_alive = if status_path.exists() {
                if let Ok(session_dirs) = fs::read_dir(&status_path) {
                    session_dirs.flatten().any(|sd| {
                        let pf_status = sd
                            .path()
                            .join("pathflow")
                            .join("pathflow-session-status.json");
                        if let Ok(content) = fs::read_to_string(&pf_status) {
                            if let Ok(status) =
                                serde_json::from_str::<serde_json::Value>(&content)
                            {
                                let lead_pid = status
                                    .get("lead_pid")
                                    .and_then(serde_json::Value::as_u64)
                                    .and_then(|v| u32::try_from(v).ok())
                                    .unwrap_or(0);
                                return lead_pid > 0 && is_process_alive(lead_pid);
                            }
                        }
                        false
                    })
                } else {
                    false
                }
            } else {
                false // No session state -- dead.
            };

            if session_alive {
                continue;
            }

            // Orphaned worktree with dead session -- clean up.
            result.messages.push(format!(
                "ORPHAN WORKTREE: cleaning '{name}' (on disk, not in registry, dead session)",
            ));

            let opts = crate::worktree::CleanupOpts {
                force: true,
                ..Default::default()
            };
            if let Err(e) = mgr.cleanup(&name, &opts) {
                result.warnings.push(format!(
                    "orphan worktree cleanup failed for '{name}': {e}",
                ));
            }
        }
    }

    /// Resolve or generate a session ID based on source type.
    ///
    /// For `startup`/`unknown`: generate a new SID and write env file.
    /// For `compact`/`resume`/`clear`: reuse the existing SID from
    /// `handle_stale_cleanup` (always `Some` for these sources).
    ///
    /// Teammate detection is fully handled by `handle_stale_cleanup` (PID-based).
    /// No duplicate detection here — the SAFEGUARD block was removed because it
    /// had the same false-positive bug (concluded "teammate" for a new lead after
    /// crash when status was active + config existed, without checking PID liveness).
    fn resolve_or_generate_session_id(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        source: &str,
        _claude_session_id: &str,
        existing_sid: Option<SessionId>,
        result: &mut InitResult,
    ) -> Result<SessionId, HookError> {
        // Priority 1: Use existing SID from stale cleanup (teammate detection
        // or compact/resume/clear path). This is always authoritative.
        if let Some(sid) = existing_sid {
            return Ok(sid);
        }

        // For startup/unknown: generate a new SID and write env file.
        if source == "startup" || source == "unknown" {
            let sid = session::generate_session_id();
            let project_name = project_dir
                .file_name()
                .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
            if let Err(e) = session::write_env_file(runtime_dir, &sid, &project_name) {
                result.warnings.push(format!("env file write error: {e}"));
            }
            return Ok(sid);
        }

        // Non-startup source (compact/resume/clear) with no existing SID:
        // handle_stale_cleanup returns (Some, false) for these sources when
        // an env file exists. If we reach here, the env file was missing.
        if let Ok(Some(env)) = session::read_env_file(runtime_dir) {
            return Ok(env.session_id);
        }

        Err(HookError::Config(format!(
            "source={source} requires existing session but none found"
        )))
    }

    /// Create required session directories.
    fn create_directories(&self, project_dir: &Path, session_id: &str) {
        let dirs = [
            project_dir.join(".state").join("runtime"),
            project_dir.join(".state").join("ledger"),
            project_dir
                .join(".state")
                .join("session")
                .join(session_id)
                .join("pathflow"),
            project_dir
                .join(".state")
                .join("sentinels")
                .join("pathflow")
                .join(session_id),
            project_dir.join(".state").join("logs"),
        ];
        for dir in &dirs {
            let _ = fs::create_dir_all(dir);
        }

        // Auto-migrate ledger from flat layout to subdirectory layout.
        // One-time, idempotent. Does not fail session start on error.
        let ledger_dir = project_dir.join(".state").join("ledger");
        match crate::ledger::migrate::migrate_flat_to_subdirs(&ledger_dir) {
            Ok(result) if !result.already_migrated => {
                eprintln!(
                    "info: ledger migration: moved {} files to subdirectory layout",
                    result.migrated_count
                );
            }
            Err(e) => {
                eprintln!("warn: ledger migration failed (non-fatal): {e}");
            }
            _ => {}
        }
    }

    /// Sweep ALL stale sessions using `pathflow-session-status.json`.
    fn sweep_all_stale_sessions(&self, project_dir: &Path, current_sid: &str) {
        let mut swept = 0u32;
        let session_base = project_dir.join(".state").join("session");

        if let Ok(entries) = fs::read_dir(&session_base) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("ses-") || name == current_sid {
                    continue;
                }

                let status_path = session_base
                    .join(&name)
                    .join("pathflow")
                    .join("pathflow-session-status.json");

                let status: serde_json::Value =
                    if let Ok(v) = crate::pathflow::file_lock::locked_read(&status_path) {
                        v
                    } else {
                        self.remove_stale_session_artifacts(project_dir, &name, None);
                        continue;
                    };

                let session_status = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let team_name = status
                    .get("team_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Deterministic signal-based decisions (no time thresholds).
                match session_status {
                    "pf-complete" => {
                        let tn = if team_name.is_empty() {
                            None
                        } else {
                            Some(team_name)
                        };
                        self.remove_stale_session_artifacts(project_dir, &name, tn);
                        swept += 1;
                    }
                    "created" => {
                        // "created" means no team was ever created.
                        // Check if config.json exists (unlikely but defensive).
                        let has_config = !team_name.is_empty()
                            && self
                                .home_dir
                                .join(".claude")
                                .join("teams")
                                .join(team_name)
                                .join("config.json")
                                .exists();
                        if !has_config {
                            self.remove_stale_session_artifacts(project_dir, &name, None);
                            swept += 1;
                        }
                    }
                    "pf-started" | "pf-in-progress" => {
                        if team_name.is_empty() {
                            // No team name -- session is dead.
                            self.remove_stale_session_artifacts(project_dir, &name, None);
                            swept += 1;
                        } else {
                            let cfg = self
                                .home_dir
                                .join(".claude")
                                .join("teams")
                                .join(team_name)
                                .join("config.json");
                            if cfg.exists() {
                                // Config exists — check lead_pid liveness.
                                let lead_pid = status
                                    .get("lead_pid")
                                    .and_then(serde_json::Value::as_u64)
                                    .and_then(|v| u32::try_from(v).ok())
                                    .unwrap_or(0);
                                if lead_pid > 0 && !is_process_alive(lead_pid) {
                                    // Lead process dead — check if any teammate is still alive
                                    // before sweeping (teammate may still be finishing work).
                                    let team_path = session_base
                                        .join(&name)
                                        .join("pathflow")
                                        .join("pathflow-team.json");
                                    let any_teammate_alive =
                                        crate::pathflow::file_lock::locked_read(&team_path)
                                            .ok()
                                            .and_then(|tv| tv.get("teammates")?.as_array().cloned())
                                            .is_some_and(|arr| {
                                                arr.iter().any(|t| {
                                                    let pid = t
                                                        .get("pid")
                                                        .and_then(serde_json::Value::as_u64)
                                                        .and_then(|v| u32::try_from(v).ok())
                                                        .unwrap_or(0);
                                                    pid > 0 && is_process_alive(pid)
                                                })
                                            });
                                    if !any_teammate_alive {
                                        self.remove_stale_session_artifacts(
                                            project_dir,
                                            &name,
                                            Some(team_name),
                                        );
                                        swept += 1;
                                    }
                                    // If any teammate alive, KEEP — let teammate finish gracefully.
                                }
                                // lead_pid alive → KEEP (session genuinely active)
                                // no lead_pid → KEEP (pre-PID session, safe default)
                            } else {
                                // Team config gone -- session is dead.
                                self.remove_stale_session_artifacts(
                                    project_dir,
                                    &name,
                                    Some(team_name),
                                );
                                swept += 1;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Log sweep results.
        if swept > 0 {
            let log_dir = project_dir.join(".state").join("logs").join("sessions");
            let _ = fs::create_dir_all(&log_dir);
            let date = (self.now)().get(..10).unwrap_or("unknown").to_string();
            let log_path = log_dir.join(format!("cleanup-{date}.jsonl"));
            let entry = serde_json::json!({
                "event": "stale_session_sweep",
                "timestamp": (self.now)(),
                "current_session": current_sid,
                "sessions_swept": swept,
            });
            if let Ok(line) = serde_json::to_string(&entry) {
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                    .and_then(|mut f| writeln!(f, "{line}"));
            }
        }

        // Sweep orphan sentinel dirs.
        let sentinel_base = project_dir
            .join(".state")
            .join("sentinels")
            .join("pathflow");
        if let Ok(entries) = fs::read_dir(&sentinel_base) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with("ses-") || name == current_sid {
                    continue;
                }
                if !session_base.join(&name).exists() {
                    let _ = fs::remove_dir_all(entry.path());
                }
            }
        }
    }

    /// Remove all artifacts for a stale session: session dir, sentinel dir, team config/tasks.
    ///
    /// Also cleans up orphaned `.lock` files in the pathflow directory before
    /// removing the session directory.
    fn remove_stale_session_artifacts(
        &self,
        project_dir: &Path,
        sid: &str,
        team_name: Option<&str>,
    ) {
        // Clean lock files from pathflow directory.
        let pathflow_dir = project_dir
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        clean_lock_files_in_dir(&pathflow_dir);

        let _ = fs::remove_dir_all(project_dir.join(".state").join("session").join(sid));
        let _ = fs::remove_dir_all(
            project_dir
                .join(".state")
                .join("sentinels")
                .join("pathflow")
                .join(sid),
        );
        if let Some(name) = team_name {
            let _ = fs::remove_dir_all(self.home_dir.join(".claude").join("teams").join(name));
            let _ = fs::remove_dir_all(self.home_dir.join(".claude").join("tasks").join(name));
        }
    }

    /// Clean up expired active task context (worktree-aware).
    fn cleanup_active_task(&self, project_dir: &Path) {
        let worktree_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
        self.cleanup_active_task_inner(project_dir, worktree_path.as_deref());
    }

    /// Inner implementation with explicit worktree path for testability.
    fn cleanup_active_task_inner(&self, project_dir: &Path, worktree_path: Option<&str>) {
        if let Ok(Some(task)) =
            session::active_task::get_active_task_resolved(project_dir, worktree_path)
        {
            // If the task is complete or cancelled, remove it.
            if let Some(status) = &task.status {
                if status == "complete" || status == "cancelled" {
                    let _ = session::active_task::clear_active_task_resolved(
                        project_dir,
                        worktree_path,
                    );
                }
            }
        }
    }

    /// Create the `pathflow-session-status.json` file. Returns `true` if the
    /// file already existed (resume scenario).
    ///
    /// When `is_teammate` is true, the existing status file is preserved
    /// unconditionally (a teammate joining should not reset the lead's status).
    fn create_pathflow_flag(
        &self,
        project_dir: &Path,
        session_id: &str,
        source: &str,
        is_teammate: bool,
        result: &mut InitResult,
    ) -> bool {
        let flag_dir = project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow");
        let status_path = flag_dir.join("pathflow-session-status.json");

        if status_path.exists() {
            if is_teammate {
                // Teammate joining -- preserve existing status unconditionally.
                result
                    .messages
                    .push("PathFlow: Teammate joining -- preserving existing status.".into());
                return true;
            }
            // Defensive check: if source=startup and status file exists for
            // this SID (SID collision or sweep missed it), reset to "created"
            // rather than preserving stale state.
            if source == "startup" || source == "unknown" {
                result.messages.push(
                    "PathFlow: Status file exists on startup -- resetting to 'created'.".into(),
                );
                // Fall through to overwrite with fresh "created" status.
            } else {
                result
                    .messages
                    .push("PathFlow: Resuming active session (status file already exists).".into());
                return true;
            }
        }

        let _ = fs::create_dir_all(&flag_dir);
        let content = serde_json::json!({
            "session_id": session_id,
            "team_name": "",
            "status": "created",
            "work_type": "",
            "lead_pid": self.lead_pid,
            "last_completed_phase": "",
            "last_completed_stage": "",
            "source_at_start": source,
            "latest_source": source,
            "latest_source_at": (self.now)(),
            "created_at": (self.now)(),
            "updated_at": (self.now)(),
        });
        // Atomic write: tmp file + rename to avoid partial reads.
        let tmp_path = status_path.with_extension("tmp");
        if let Ok(json) = serde_json::to_string_pretty(&content) {
            let _ = fs::write(&tmp_path, json).and_then(|()| fs::rename(&tmp_path, &status_path));
        }

        false
    }

    /// Initialize the `PathFlow` checkpoint from `pathflow-config.json`.
    fn init_checkpoint(&self, project_dir: &Path, session_id: &str, result: &mut InitResult) {
        let config_path = project_dir
            .join(".codeflow")
            .join("config")
            .join("pathflow")
            .join("pathflow-config.json");

        if !config_path.exists() {
            result
                .warnings
                .push("pathflow-config.json not found, skipping checkpoint init".into());
            return;
        }

        let checkpoint_path = project_dir
            .join(".state")
            .join("session")
            .join(session_id)
            .join("pathflow")
            .join("pathflow-phase-tasks.json");

        let cp = pathflow::checkpoint::Checkpoint::new();
        if let Err(e) = cp.init_all_phases(&checkpoint_path, &config_path) {
            result.warnings.push(format!("checkpoint init error: {e}"));
        }
    }

    // session-meta.json ELIMINATED: all fields now in pathflow-session-status.json
    // (lead_pid, source_at_start, latest_source, latest_source_at)

    /// Detect compact recovery (when `source=compact/resume/clear` and active work exists).
    /// Worktree-aware: reads active task from worktree-local path when available.
    fn detect_compact_recovery(&self, project_dir: &Path, source: &str, result: &mut InitResult) {
        if source != "compact" && source != "resume" && source != "clear" {
            return;
        }

        if let Ok(Some(task)) = session::get_active_task_worktree_aware(project_dir) {
            result.messages.push(format!(
                "COMPACT RECOVERY: Active task {} detected. Context was compacted.",
                task.task_id.as_str()
            ));
        }
    }

    /// Create the project-scoped temp directory.
    ///
    /// When a worktree is available, creates the worktree-scoped path via
    /// `WorktreePaths::temp_dir()` so each session gets its own subdirectory.
    /// Without a worktree, falls back to the project-level path.
    fn create_project_temp_dir(
        &self,
        project_dir: &Path,
        worktree_paths: Option<&WorktreePaths>,
        _result: &mut InitResult,
    ) {
        let tmp_dir = Self::resolve_temp_dir(project_dir, worktree_paths);
        let _ = fs::create_dir_all(&tmp_dir);
    }

    /// Pure path resolver for the temp directory (testable without env vars).
    fn resolve_temp_dir(project_dir: &Path, worktree_paths: Option<&WorktreePaths>) -> PathBuf {
        if let Some(wp) = worktree_paths {
            wp.temp_dir()
        } else {
            let project_name = project_dir
                .file_name()
                .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
            PathBuf::from("/tmp/claude")
                .join(&project_name)
                .join("managed")
        }
    }

    /// Create a detached worktree for the current session.
    ///
    /// Called during `source=startup` after session ID generation.
    /// Creates the worktree, sets session_id on the registry entry, and
    /// writes `CODEFLOW_WORKTREE_PATH` to the main project env file.
    fn create_session_worktree(
        &self,
        project_dir: &Path,
        session_id: &str,
        result: &mut InitResult,
    ) -> Result<(WorktreePaths, WorktreeHandle), HookError> {
        // Read max_concurrent from parallel-work config.
        let max_concurrent = crate::autorun::config::load_config(project_dir)
            .map(|c| c.worktree.max_concurrent)
            .unwrap_or(3);

        let wt_name = format!("worktree-{session_id}");
        let mgr = WorktreeManager::new(project_dir);

        // Clean orphaned worktrees before registration to free slots.
        // Orphans: in git but not registry, or in registry with missing dirs.
        clean_orphaned_worktrees(project_dir, &mgr);

        // Check worktree limit before creation using locked_register_with_limit.
        // This atomically checks count + registers under a single file lock.
        let now_ts = (self.now)();
        let reg_entry = crate::worktree::WorktreeEntry {
            name: wt_name.clone(),
            path: String::new(), // placeholder — updated after setup_detached
            branch: String::new(),
            created_at: now_ts,
            status: "active".to_string(),
            session_id: Some(session_id.to_string()),
            task_id: None,
        };
        crate::worktree::locked_register_with_limit(
            mgr.registry_path(),
            &reg_entry,
            max_concurrent,
        )?;

        // Create the detached worktree.
        // NOTE: This happens inside the session lock scope (lock_file still held),
        // so concurrent session creation is serialized.
        let entry = mgr.setup_detached(&wt_name)?;

        // Create RAII handle for cleanup on panic.
        let handle = WorktreeHandle::new(&entry, &mgr);

        let wt_path = PathBuf::from(&entry.path);
        let paths = WorktreePaths::new(&wt_path);

        // Post-creation verification: confirm worktree is in git and registry.
        verify_worktree_creation(project_dir, &wt_name, &wt_path, result);

        // session_id is preserved during dedup in locked_register_with_limit
        // (the guard at registry.rs only overwrites session_id/task_id when the
        // new value is Some, so setup_detached's None does not clobber the
        // pre-registration's session_id). No unlocked registry write needed.

        // Write CODEFLOW_WORKTREE_PATH to the MAIN project env file
        // so compact/resume can recover it.
        let main_runtime = project_dir.join(".state").join("runtime");
        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
        let wt_path_str = wt_path.to_string_lossy().to_string();
        let sid = SessionId::new_unchecked(session_id);
        if let Err(e) = session::write_env_file_with_worktree(
            &main_runtime,
            &sid,
            &project_name,
            Some(&wt_path_str),
        ) {
            result
                .warnings
                .push(format!("main env file update error: {e}"));
        }

        result.messages.push(format!(
            "WORKTREE: Created {} at {}",
            wt_name,
            wt_path.display()
        ));

        Ok((paths, handle))
    }

    /// Recover the worktree path from the main project's env file.
    ///
    /// Called during `source=compact/resume/clear` to find the existing worktree.
    fn recover_worktree_path(
        &self,
        runtime_dir: &Path,
        result: &mut InitResult,
    ) -> Option<WorktreePaths> {
        let env_data = match session::read_env_file(runtime_dir) {
            Ok(Some(env)) => env,
            _ => return None,
        };

        let wt_path_str = env_data.worktree_path?;
        let wt_path = PathBuf::from(&wt_path_str);

        if !wt_path.exists() {
            result.warnings.push(format!(
                "worktree path from env file does not exist: {wt_path_str}"
            ));
            return None;
        }

        result
            .messages
            .push(format!("WORKTREE: Recovered existing at {wt_path_str}"));

        Some(WorktreePaths::new(wt_path))
    }

    /// Detect a pre-created worktree from the autorun orchestrator.
    ///
    /// Checks process environment for `CODEFLOW_WORKTREE_PATH` (primary signal),
    /// then falls back to the env file. Delegates to `_inner` for testability.
    fn detect_precreated_worktree(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        session_id: &str,
        result: &mut InitResult,
    ) -> Option<WorktreePaths> {
        let env_path = std::env::var("CODEFLOW_WORKTREE_PATH").ok();
        self.detect_precreated_worktree_inner(
            project_dir,
            runtime_dir,
            session_id,
            env_path.as_deref(),
            result,
        )
    }

    /// Inner implementation of pre-created worktree detection.
    ///
    /// Takes the env var value as an explicit parameter so tests can call this
    /// directly without modifying process environment.
    fn detect_precreated_worktree_inner(
        &self,
        project_dir: &Path,
        runtime_dir: &Path,
        session_id: &str,
        env_worktree_path: Option<&str>,
        result: &mut InitResult,
    ) -> Option<WorktreePaths> {
        // 1. Resolve worktree path from env var (primary) or env file (fallback).
        let wt_path_str = if let Some(p) = env_worktree_path {
            p.to_string()
        } else {
            // Fallback: read from env file (orchestrator may have written it).
            let env_data = match session::read_env_file(runtime_dir) {
                Ok(Some(env)) => env,
                _ => return None,
            };
            env_data.worktree_path?
        };

        // 2. Dual-condition validation: directory exists AND .state/runtime/ present.
        let wt_path = Path::new(&wt_path_str);
        if !wt_path.exists() {
            return None;
        }
        if !wt_path.join(".state").join("runtime").exists() {
            result.warnings.push(format!(
                "pre-created worktree path exists but .state/runtime/ missing: {wt_path_str}"
            ));
            return None;
        }

        // 3. Update registry session_id (mirrors create_session_worktree lines 960-967).
        if let Some(wt_name) = wt_path.file_name().map(|n| n.to_string_lossy().to_string()) {
            let mgr = WorktreeManager::new(project_dir);
            if let Ok(mut reg) = crate::worktree::read_registry(mgr.registry_path()) {
                for e in &mut reg.worktrees {
                    if e.name == wt_name {
                        e.session_id = Some(session_id.to_string());
                    }
                }
                let _ = crate::worktree::write_registry(mgr.registry_path(), &reg);
            }
        }

        // 4. Update main env file with worker's session ID (mirrors lines 971-986).
        let project_name = project_dir
            .file_name()
            .map_or_else(|| "codeflow".into(), |n| n.to_string_lossy().to_string());
        let sid = SessionId::new_unchecked(session_id);
        if let Err(e) = session::write_env_file_with_worktree(
            runtime_dir,
            &sid,
            &project_name,
            Some(&wt_path_str),
        ) {
            result
                .warnings
                .push(format!("main env file update error: {e}"));
        }

        result
            .messages
            .push(format!("WORKTREE: Pre-created detected at {wt_path_str}"));

        Some(WorktreePaths::new(wt_path.to_path_buf()))
    }

    /// Create required session directories inside the worktree.
    fn create_directories_in_worktree(&self, wp: &WorktreePaths, session_id: &str) {
        let dirs = [
            wp.runtime_dir(),
            wp.session_dir(session_id).join("pathflow"),
            wp.sentinel_dir(session_id),
        ];
        for dir in &dirs {
            let _ = fs::create_dir_all(dir);
        }

        // Migrate worktree ledger from flat to subdirectory layout.
        // Worktrees inherit flat layout from git checkout; the main repo
        // migration in create_directories() does not cover worktree-local
        // ledger dirs. One-time, idempotent.
        let ledger_dir = wp.state_dir().join("ledger");
        if ledger_dir.exists() {
            match crate::ledger::migrate::migrate_flat_to_subdirs(&ledger_dir) {
                Ok(result) if !result.already_migrated => {
                    eprintln!(
                        "info: worktree ledger migration: moved {} files to subdirectory layout",
                        result.migrated_count
                    );
                }
                Err(e) => {
                    eprintln!("warn: worktree ledger migration failed (non-fatal): {e}");
                }
                _ => {}
            }
        }
    }
}

impl HookHandler for SessionStartInit {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input.project_dir.as_deref().ok_or_else(|| {
            HookError::Config("project_dir required for session-start init".into())
        })?;

        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        self.run(&input, Path::new(project_dir), &mut out)?;
        out.flush().map_err(HookError::Io)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-init"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// SessionStartInstructions
// ---------------------------------------------------------------------------

/// Hardcoded fallback instructions when config is unavailable.
/// Matches Go's `fallbackInstructions` constant in `instructions.go`.
const FALLBACK_INSTRUCTIONS: &str = "SESSION START - EXECUTE CLAUDE.md SECTION 2\n\
You MUST execute the Session Start procedure from CLAUDE.md Section 2.\n\
Check for active work (grep Status: active), present options to user, wait for choice.";

/// Instructions config JSON structure matching Go's `instructionsConfig`.
#[derive(serde::Deserialize)]
struct InstructionsConfig {
    hooks: InstructionsHooks,
}

#[derive(serde::Deserialize)]
struct InstructionsHooks {
    #[serde(rename = "SessionStart", default)]
    session_start: HashMap<String, InstructionEntry>,
}

#[derive(serde::Deserialize)]
struct InstructionEntry {
    file: Option<String>,
    enabled: Option<bool>,
}

/// Session-start instructions handler.
///
/// Outputs config-driven instructions, active task context, and `PathFlow`
/// recovery information to help the agent orient after startup or context
/// overflow. Matches Go's `RunInstructions` in `instructions.go`.
pub struct SessionStartInstructions;

impl SessionStartInstructions {
    /// Generate instruction output for the agent.
    ///
    /// Three sections matching Go:
    /// 1. Config-driven instruction loading (with fallback)
    /// 2. Active task context (with "None" fallback)
    /// 3. `PathFlow` context (sentinels, recovery)
    ///
    /// # Errors
    ///
    /// Returns `HookError` on I/O failures when writing to the writer.
    pub fn generate(&self, project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        // Section 1: Config-driven instruction loading.
        Self::output_instructions(project_dir, writer)?;

        // Section 2: Active task context.
        Self::output_active_task(project_dir, writer)?;

        // Section 3: PathFlow context.
        Self::output_pathflow_context(project_dir, writer)?;

        Ok(())
    }

    /// Load and output instructions from config, falling back to hardcoded text.
    fn output_instructions(project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        let instructions_dir = project_dir
            .join(".codeflow")
            .join("config")
            .join("instructions");
        let config_path = instructions_dir.join("instructions-config.json");

        let Ok(data) = fs::read_to_string(&config_path) else {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
            return Ok(());
        };

        let Ok(cfg) = serde_json::from_str::<InstructionsConfig>(&data) else {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
            return Ok(());
        };

        if cfg.hooks.session_start.is_empty() {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
            return Ok(());
        }

        let mut wrote = false;
        for entry in cfg.hooks.session_start.values() {
            let enabled = entry.enabled.unwrap_or(false);
            let file = match &entry.file {
                Some(f) if !f.is_empty() && enabled => f,
                _ => continue,
            };
            let file_path = instructions_dir.join(file);
            if let Ok(content) = fs::read_to_string(&file_path) {
                write!(writer, "{content}").map_err(HookError::Io)?;
                writeln!(writer).map_err(HookError::Io)?; // Blank line between.
                wrote = true;
            }
        }

        if !wrote {
            writeln!(writer, "{FALLBACK_INSTRUCTIONS}").map_err(HookError::Io)?;
        }

        Ok(())
    }

    /// Output active task context or "None" fallback with register-work reminder.
    /// Worktree-aware: reads active task from worktree-local path when available.
    fn output_active_task(project_dir: &Path, writer: &mut dyn Write) -> Result<(), HookError> {
        if let Ok(Some(task)) = session::get_active_task_worktree_aware(project_dir) {
            let task_id = task
                .task_format_id
                .as_ref()
                .map_or_else(|| task.task_id.as_str(), crate::types::FormatId::as_str);

            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "ACTIVE TASKS DETECTED").map_err(HookError::Io)?;
            writeln!(writer, "=====================").map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "Incomplete tasks found:").map_err(HookError::Io)?;
            writeln!(
                writer,
                "  - {} ({})",
                task_id,
                task.status.as_deref().unwrap_or("unknown")
            )
            .map_err(HookError::Io)?;
            if let Some(title) = &task.title {
                if !title.is_empty() {
                    writeln!(writer, "    \"{title}\"").map_err(HookError::Io)?;
                }
            }
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "Options:").map_err(HookError::Io)?;
            writeln!(writer, "  1. Resume task").map_err(HookError::Io)?;
            writeln!(writer, "  2. Start new work").map_err(HookError::Io)?;
            writeln!(writer, "  3. Review tasks").map_err(HookError::Io)?;
        } else {
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(writer, "ACTIVE TASKS DETECTED: None").map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(
                writer,
                "IMPORTANT: Register work before making modifications."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "Delegate to cf-knowledge-layer teammate: \
                 SendMessage(recipient=\"cf-knowledge-layer\", \
                 content=\"ensure-work-registered\")"
            )
            .map_err(HookError::Io)?;
        }

        Ok(())
    }

    /// Output `PathFlow` context: active session, completed phases, recovery checklist.
    fn output_pathflow_context(
        project_dir: &Path,
        writer: &mut dyn Write,
    ) -> Result<(), HookError> {
        let state_dir = project_dir.join(".state");
        let Ok(session_id) = session::current_session_id(project_dir) else {
            return Ok(());
        };

        // Check if PathFlow is active via status.json.
        let status_path = state_dir
            .join("session")
            .join(session_id.as_str())
            .join("pathflow")
            .join("pathflow-session-status.json");
        let pf_active = if let Ok(data) = fs::read_to_string(&status_path) {
            serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(String::from))
                .is_some_and(|s| !s.is_empty() && s != "pf-complete")
        } else {
            false
        };
        if !pf_active {
            return Ok(());
        }

        writeln!(writer).map_err(HookError::Io)?;
        writeln!(writer, "PATHFLOW SESSION ACTIVE").map_err(HookError::Io)?;
        writeln!(writer, "======================").map_err(HookError::Io)?;

        // List completed phase sentinels.
        let sentinel_dir = state_dir
            .join("sentinels")
            .join("pathflow")
            .join(session_id.as_str());
        let names = pathflow::sentinel::list_sentinels(&sentinel_dir).unwrap_or_default();

        let phases: Vec<&str> = names
            .iter()
            .filter(|n| n.starts_with("pf-"))
            .map(String::as_str)
            .collect();

        if phases.is_empty() {
            writeln!(writer, "No completed phases found").map_err(HookError::Io)?;
        } else {
            writeln!(writer, "Completed phases:").map_err(HookError::Io)?;
            for p in &phases {
                writeln!(writer, "  - pathflow-{p}").map_err(HookError::Io)?;
            }
        }

        writeln!(writer, "Mode: pathflow").map_err(HookError::Io)?;
        writeln!(writer, "PCV: bypassed (WS-REV provides quality assurance)")
            .map_err(HookError::Io)?;
        writeln!(writer).map_err(HookError::Io)?;

        // Compact recovery checklist (only if phase sentinels exist).
        if !phases.is_empty() {
            writeln!(
                writer,
                "COMPACT RECOVERY: Task tracker registration check required."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "Phase sentinels exist from prior context. Task tracker may be out of sync."
            )
            .map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(
                writer,
                "MANDATORY: Resume task tracker registration after context overflow."
            )
            .map_err(HookError::Io)?;
            writeln!(writer, "  Step 1: Read checkpoint state at .state/session/{{SID}}/pathflow/pathflow-phase-tasks.json").map_err(HookError::Io)?;
            writeln!(writer, "  Step 2: Identify current phase from sentinel files at .state/sentinels/pathflow/{{SID}}/").map_err(HookError::Io)?;
            writeln!(writer, "  Step 3: Backfill completed phases: TaskCreate then TaskUpdate to completed for each missing task").map_err(HookError::Io)?;
            writeln!(
                writer,
                "  Step 4: Register current phase tasks: TaskCreate for EVERY PF{{N}}-TSK-{{NN}}"
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "  Step 5: Verify sentinel pipeline resumes creating sentinels"
            )
            .map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
            writeln!(
                writer,
                "FORBIDDEN: Skipping task tracker registration after context overflow."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "FORBIDDEN: Clubbing multiple PF{{N}}-TSK-{{NN}} entries into a single TaskCreate."
            )
            .map_err(HookError::Io)?;
            writeln!(
                writer,
                "FORBIDDEN: Proceeding past a phase gate without verifying its sentinel exists."
            )
            .map_err(HookError::Io)?;
            writeln!(writer).map_err(HookError::Io)?;
        }

        Ok(())
    }
}

impl HookHandler for SessionStartInstructions {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        let project_dir = input
            .project_dir
            .as_deref()
            .ok_or_else(|| HookError::Config("project_dir required for instructions".into()))?;

        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        self.generate(Path::new(project_dir), &mut out)?;
        out.flush().map_err(HookError::Io)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-instructions"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// SessionStartLogging
// ---------------------------------------------------------------------------

/// Session-start logging handler.
///
/// Captures `permission_mode` and writes a `session_start` event to the
/// sessions JSONL ledger.
pub struct SessionStartLogging<L: LedgerWriter> {
    pub ledger: L,
    pub now: NowFn,
}

impl<L: LedgerWriter> SessionStartLogging<L> {
    /// Write the `session_start` ledger event.
    ///
    /// # Errors
    ///
    /// Returns `HookError` on ledger write failure.
    pub fn log_start(
        &self,
        session_id: &str,
        source: &str,
        permission_mode: Option<&str>,
    ) -> Result<(), HookError> {
        let mut data = HashMap::new();
        data.insert(
            "source".into(),
            serde_json::Value::String(source.to_string()),
        );
        if let Some(mode) = permission_mode {
            data.insert(
                "permission_mode".into(),
                serde_json::Value::String(mode.to_string()),
            );
        }

        let event = Event {
            event_type: "session_start".into(),
            timestamp: (self.now)(),
            session_id: Some(session_id.to_string()),
            worktree: None,
            data,
        };

        self.ledger
            .append_event(event)
            .map_err(|e| HookError::Config(format!("ledger write error: {e}")))?;

        Ok(())
    }
}

impl<L: LedgerWriter> HookHandler for SessionStartLogging<L> {
    fn handle(&self, input: HookInput) -> Result<HookOutput, HookError> {
        // Prefer CODEFLOW_SESSION_ID from env file over stdin UUID.
        // SessionStartLogging fires early, before env file may be written on
        // first startup, so fall back to input.session_id (Claude per-agent UUID).
        let project_dir = input.project_dir.as_deref().unwrap_or(".");
        let session_id_owned =
            crate::hooks::logging::resolve_session_id(std::path::Path::new(project_dir));
        let session_id = if session_id_owned == "unknown" {
            input.session_id.as_deref().unwrap_or("unknown")
        } else {
            &session_id_owned
        };
        let source = input.source.as_deref().unwrap_or("unknown");

        self.log_start(session_id, source, None)?;

        Ok(HookOutput::Allow)
    }

    fn name(&self) -> &'static str {
        "session-start-logging"
    }

    fn events(&self) -> &[HookEvent] {
        &[HookEvent::SessionStart]
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write the env JSON output to the writer.
///
/// Format: `{"env": {"CODEFLOW_SESSION_ID": "...", "CF_PROJECT_ROOT": "..."}}`
fn write_env_json(
    writer: &mut dyn Write,
    env_vars: &HashMap<String, String>,
) -> Result<(), HookError> {
    let output = serde_json::json!({ "env": env_vars });
    writeln!(
        writer,
        "{}",
        serde_json::to_string(&output).unwrap_or_default()
    )
    .map_err(HookError::Io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::PathflowTeamInfo;
    use crate::types::SessionId;

    fn fixed_now() -> String {
        "2026-03-10T00:00:00Z".to_string()
    }

    fn make_init(home: PathBuf) -> SessionStartInit {
        SessionStartInit {
            lead_pid: 1000,
            home_dir: home,
            now: fixed_now,
        }
    }

    fn make_input(source: &str, project_dir: &str) -> HookInput {
        HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: Some("claude-uuid-123".into()),
            project_dir: Some(project_dir.into()),
            source: Some(source.into()),
            transcript_path: None,
            ..Default::default()
        }
    }

    // --- SessionStartInit tests ---

    #[test]
    fn test_init_startup_generates_new_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.session_id.as_str().starts_with("ses-"));
        assert!(!result.is_resume);
        assert!(!result.is_teammate);
        assert!(result.env_vars.contains_key("CODEFLOW_SESSION_ID"));
        assert!(result.env_vars.contains_key("CF_PROJECT_ROOT"));

        // Verify env JSON was written to stdout
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("CODEFLOW_SESSION_ID"));
    }

    #[test]
    fn test_init_compact_reuses_existing_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        // Write an existing env file.
        let existing_sid = SessionId::new_unchecked("ses-01jq7existing123456789ab");
        session::write_env_file(&runtime_dir, &existing_sid, "codeflow").unwrap();

        let init = make_init(home.path().to_path_buf());
        let input = make_input("compact", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert_eq!(result.session_id, existing_sid);
    }

    #[test]
    fn test_init_teammate_detection_via_file_signals() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let sid = SessionId::new_unchecked("ses-01jq7teammate12345678abc");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Create pathflow-session-status.json with active status and lead_pid.
        // Use current process PID as lead_pid (alive during test).
        let lead_pid = std::process::id();
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": sid.as_str(), "status": "pf-in-progress",
                "team_name": "test-team", "lead_pid": lead_pid,
                "created_at": "2026-03-10T00:00:00Z",
                "updated_at": "2026-03-10T00:00:00Z",
            }))
            .unwrap(),
        )
        .unwrap();

        // Create team config (existence check).
        let config_dir = home.path().join(".claude").join("teams").join("test-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), r#"{"members": []}"#).unwrap();

        // Teammate detection uses 4 signals:
        // 1. env file -> existing SID
        // 2. status.json -> pf-in-progress + team_name
        // 3. config.json -> team alive
        // 4. lead_pid -> alive (current process PID)
        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert!(result.is_teammate);
        assert_eq!(result.session_id, sid);
        assert!(result.messages.iter().any(|m| m.contains("TEAMMATE MODE")));
    }

    #[test]
    fn test_init_teammate_detection_no_config_is_new_lead() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let sid = SessionId::new_unchecked("ses-01jq7noconfig1234567890a");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Create pathflow-session-status.json with active status.
        let pathflow_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": sid.as_str(), "status": "pf-in-progress",
                "team_name": "dead-team", "created_at": "2026-03-10T00:00:00Z",
                "updated_at": "2026-03-10T00:00:00Z",
            }))
            .unwrap(),
        )
        .unwrap();

        // NO team config -- session is dead, should start as new lead.
        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        assert!(!result.is_teammate, "no config -> new lead, not teammate");
        // Should generate a new SID (not reuse the dead session's SID).
        assert_ne!(result.session_id, sid);
    }

    #[test]
    fn test_init_resume_flag_detection() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        // First run: fresh start.
        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();
        let sid = result.session_id.clone();
        assert!(!result.is_resume);

        // Manually set up env file for the second run (to reuse same SID).
        let runtime_dir = dir.path().join(".state").join("runtime");
        session::write_env_file(&runtime_dir, &sid, "codeflow").unwrap();

        // Second run with compact: flag already exists -> resume.
        let input2 = make_input("compact", dir.path().to_str().unwrap());
        let mut buf2 = Vec::new();
        let result2 = init.run(&input2, dir.path(), &mut buf2).unwrap();
        assert!(result2.is_resume);
    }

    #[test]
    fn test_init_creates_directories() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        let sid = result.session_id.as_str();
        assert!(dir.path().join(".state").join("runtime").exists());
        assert!(dir.path().join(".state").join("ledger").exists());
        assert!(dir.path().join(".state").join("logs").exists());
        assert!(
            dir.path()
                .join(".state")
                .join("session")
                .join(sid)
                .join("pathflow")
                .exists()
        );
        assert!(
            dir.path()
                .join(".state")
                .join("sentinels")
                .join("pathflow")
                .join(sid)
                .exists()
        );
    }

    #[test]
    fn test_init_error_on_missing_project_dir() {
        let init = make_init(PathBuf::from("/tmp/nonexistent-home"));
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: None,
            project_dir: None,
            source: Some("startup".into()),
            transcript_path: None,
            ..Default::default()
        };
        let result = init.handle(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project_dir required")
        );
    }

    // --- SessionStartInstructions tests ---

    #[test]
    fn test_instructions_with_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01abc"),
            epic_id: None,
            task_format_id: Some(crate::types::FormatId::new_unchecked("INF-TSK-022-015")),
            epic_format_id: None,
            title: Some("Implement session hooks".into()),
            status: Some("in_progress".into()),
            branch: Some("feat/rust-session-hooks".into()),
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: Some("WS-DEV".into()),
            team_name: None,
            work_type: None,
            scope_policy: None,
            file_scope: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let handler = SessionStartInstructions;
        let mut buf = Vec::new();
        handler.generate(dir.path(), &mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        // Section 1: Fallback instructions (no config file in temp dir).
        assert!(output.contains("SESSION START"));
        // Section 2: Active task detected in full format.
        assert!(output.contains("ACTIVE TASKS DETECTED"));
        assert!(output.contains("INF-TSK-022-015"));
        assert!(output.contains("in_progress"));
        assert!(output.contains("Implement session hooks"));
    }

    #[test]
    fn test_instructions_no_active_task() {
        let dir = tempfile::tempdir().unwrap();

        let handler = SessionStartInstructions;
        let mut buf = Vec::new();
        handler.generate(dir.path(), &mut buf).unwrap();

        let output = String::from_utf8(buf).unwrap();
        // Section 1: Fallback instructions (no config file in temp dir).
        assert!(output.contains("SESSION START"));
        assert!(output.contains("CLAUDE.md"));
        // Section 2: No active task — shows "None" with register-work reminder.
        assert!(output.contains("ACTIVE TASKS DETECTED: None"));
        assert!(output.contains("ensure-work-registered"));
    }

    #[test]
    fn test_instructions_handler_trait() {
        let handler = SessionStartInstructions;
        assert_eq!(handler.name(), "session-start-instructions");
        assert_eq!(handler.events(), &[HookEvent::SessionStart]);
    }

    // --- SessionStartLogging tests ---

    struct MockLedger {
        events: std::sync::Mutex<Vec<Event>>,
    }

    impl MockLedger {
        fn new() -> Self {
            Self {
                events: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn last_event(&self) -> Option<Event> {
            self.events.lock().unwrap().last().cloned()
        }
    }

    impl LedgerWriter for MockLedger {
        fn append_event(&self, event: Event) -> Result<(), crate::error::LedgerError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn append_event_to_file(
            &self,
            _target_file: &str,
            event: Event,
        ) -> Result<(), crate::error::LedgerError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }

        fn route_event(&self, _event_type: &str) -> Result<String, crate::error::LedgerError> {
            Ok("sessions.jsonl".into())
        }

        fn dir(&self) -> &Path {
            Path::new("/tmp")
        }
    }

    #[test]
    fn test_logging_writes_session_start_event() {
        let ledger = MockLedger::new();
        let handler = SessionStartLogging {
            ledger,
            now: fixed_now,
        };

        handler
            .log_start("ses-test123", "startup", Some("default"))
            .unwrap();

        let event = handler.ledger.last_event().unwrap();
        assert_eq!(event.event_type, "session_start");
        assert_eq!(event.session_id.as_deref(), Some("ses-test123"));
        assert_eq!(event.timestamp, "2026-03-10T00:00:00Z");
        assert_eq!(
            event.data.get("source"),
            Some(&serde_json::Value::String("startup".into()))
        );
        assert_eq!(
            event.data.get("permission_mode"),
            Some(&serde_json::Value::String("default".into()))
        );
    }

    #[test]
    fn test_logging_without_permission_mode() {
        let ledger = MockLedger::new();
        let handler = SessionStartLogging {
            ledger,
            now: fixed_now,
        };

        handler.log_start("ses-test", "compact", None).unwrap();

        let event = handler.ledger.last_event().unwrap();
        assert!(!event.data.contains_key("permission_mode"));
    }

    #[test]
    fn test_logging_handler_trait() {
        let ledger = MockLedger::new();
        let handler = SessionStartLogging {
            ledger,
            now: fixed_now,
        };

        assert_eq!(handler.name(), "session-start-logging");
        assert_eq!(handler.events(), &[HookEvent::SessionStart]);

        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: Some("ses-logging-test".into()),
            project_dir: None,
            source: Some("startup".into()),
            transcript_path: None,
            ..Default::default()
        };

        let output = handler.handle(input).unwrap();
        assert_eq!(output.exit_code(), 0);
    }

    // --- write_env_json tests ---

    #[test]
    fn test_write_env_json_format() {
        let mut env = HashMap::new();
        env.insert("CODEFLOW_SESSION_ID".into(), "ses-abc".into());
        env.insert("CF_PROJECT_ROOT".into(), "myproject".into());

        let mut buf = Vec::new();
        write_env_json(&mut buf, &env).unwrap();

        let output = String::from_utf8(buf).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(
            parsed["env"]["CODEFLOW_SESSION_ID"],
            serde_json::Value::String("ses-abc".into())
        );
    }

    #[test]
    fn test_detect_compact_recovery_with_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        // Set up an active task.
        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01compact"),
            epic_id: None,
            task_format_id: Some(crate::types::FormatId::new_unchecked("INF-TSK-022-015")),
            epic_format_id: None,
            title: Some("Test task".into()),
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
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.detect_compact_recovery(dir.path(), "compact", &mut result);

        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("COMPACT RECOVERY")),
            "should detect compact recovery with active task"
        );
    }

    #[test]
    fn test_detect_compact_recovery_non_compact_source() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // source != "compact" should be a no-op.
        init.detect_compact_recovery(dir.path(), "startup", &mut result);

        assert!(
            result.messages.is_empty(),
            "non-compact source should not add messages"
        );
    }

    #[test]
    fn test_detect_compact_recovery_no_active_task() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // compact source but no active task.
        init.detect_compact_recovery(dir.path(), "compact", &mut result);

        assert!(
            result.messages.is_empty(),
            "compact with no active task should not add messages"
        );
    }

    #[test]
    fn test_sweep_stale_sessions_cleans_pf_complete() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000000";

        let session_base = dir.path().join(".state").join("session");
        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");

        fs::create_dir_all(session_base.join(current_sid)).unwrap();

        // Create a completed session with status.json.
        let stale_sid = "ses-01jq7stale00000000000000";
        let stale_pathflow = session_base.join(stale_sid).join("pathflow");
        fs::create_dir_all(&stale_pathflow).unwrap();
        fs::write(
            stale_pathflow.join("pathflow-session-status.json"),
            r#"{"status":"pf-complete","team_name":"stale-team","updated_at":"2026-03-10T00:00:00Z"}"#,
        ).unwrap();
        fs::create_dir_all(sentinel_base.join(stale_sid)).unwrap();
        let stale_team_dir = home.path().join(".claude").join("teams").join("stale-team");
        fs::create_dir_all(&stale_team_dir).unwrap();
        let stale_task_dir = home.path().join(".claude").join("tasks").join("stale-team");
        fs::create_dir_all(&stale_task_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            !session_base.join(stale_sid).exists(),
            "pf-complete session should be removed"
        );
        assert!(
            !sentinel_base.join(stale_sid).exists(),
            "sentinel dir should be removed"
        );
        assert!(!stale_team_dir.exists(), "team dir should be removed");
        assert!(!stale_task_dir.exists(), "task dir should be removed");
        assert!(session_base.join(current_sid).exists());
    }

    #[test]
    fn test_sweep_stale_sessions_skips_active_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000001";

        let session_base = dir.path().join(".state").join("session");

        // Create an active session with recent status.json + team config.
        let alive_sid = "ses-01jq7alive00000000000001";
        let alive_pathflow = session_base.join(alive_sid).join("pathflow");
        fs::create_dir_all(&alive_pathflow).unwrap();
        fs::write(
            alive_pathflow.join("pathflow-session-status.json"),
            serde_json::to_string(&serde_json::json!({
                "status": "pf-in-progress",
                "team_name": "alive-team",
                "updated_at": crate::util::now_rfc3339(),
            }))
            .unwrap(),
        )
        .unwrap();

        // Create team config.
        let config_dir = home.path().join(".claude").join("teams").join("alive-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), "{}").unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            session_base.join(alive_sid).exists(),
            "active session with config should not be removed"
        );
    }

    #[test]
    fn test_sweep_all_stale_sessions_cleans_no_team_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000002";

        let session_base = dir.path().join(".state").join("session");

        // Create a session dir with no team file.
        let orphan_sid = "ses-01jq7orphan0000000000002";
        fs::create_dir_all(session_base.join(orphan_sid)).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        // Session without team file should be removed.
        assert!(
            !session_base.join(orphan_sid).exists(),
            "session without team file should be removed"
        );
    }

    #[test]
    fn test_sweep_all_stale_sessions_sweeps_orphan_sentinels() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000003";

        let session_base = dir.path().join(".state").join("session");
        let sentinel_base = dir.path().join(".state").join("sentinels").join("pathflow");

        // Create orphan sentinel dir (no matching session dir).
        let orphan_sid = "ses-01jq7orphan0000000000003";
        fs::create_dir_all(sentinel_base.join(orphan_sid)).unwrap();

        // Create sentinel dir with matching active session (status.json + config).
        let good_sid = "ses-01jq7goodsid000000000003";
        fs::create_dir_all(sentinel_base.join(good_sid)).unwrap();
        let good_pathflow = session_base.join(good_sid).join("pathflow");
        fs::create_dir_all(&good_pathflow).unwrap();
        fs::write(
            good_pathflow.join("pathflow-session-status.json"),
            serde_json::to_string(&serde_json::json!({
                "status": "pf-in-progress",
                "team_name": "good-team",
                "updated_at": crate::util::now_rfc3339(),
            }))
            .unwrap(),
        )
        .unwrap();

        let config_dir = home.path().join(".claude").join("teams").join("good-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), "{}").unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            !sentinel_base.join(orphan_sid).exists(),
            "orphan sentinel dir should be removed"
        );
        assert!(
            sentinel_base.join(good_sid).exists(),
            "non-orphan sentinel dir should remain"
        );
    }

    #[test]
    fn test_sweep_all_stale_sessions_skips_current_and_non_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000004";

        let session_base = dir.path().join(".state").join("session");

        // Create current session.
        fs::create_dir_all(session_base.join(current_sid)).unwrap();
        // Create a non-session directory.
        fs::create_dir_all(session_base.join("not-a-session")).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        // Neither should be removed.
        assert!(
            session_base.join(current_sid).exists(),
            "current session should not be removed"
        );
        assert!(
            session_base.join("not-a-session").exists(),
            "non-session dir should not be removed"
        );
    }

    #[test]
    fn test_handle_stale_cleanup_stale_session_startup_triggers_full_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7stalesess000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create pathflow-team.json for the stale session.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&team_dir).unwrap();
        let team_info = PathflowTeamInfo {
            team_name: "dead-team".into(),
            ..Default::default()
        };
        fs::write(
            team_dir.join("pathflow-team.json"),
            serde_json::to_string(&team_info).unwrap(),
        )
        .unwrap();

        // No active session status → stale.
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "startup", &mut result);

        assert!(
            existing.is_none(),
            "stale session + startup should return None"
        );
        assert!(!team_mode, "stale session should not be team mode");
    }

    #[test]
    fn test_handle_stale_cleanup_compact_preserves_session() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7deadcompact0000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create pathflow-team.json for the stale session.
        let team_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&team_dir).unwrap();
        let team_info = PathflowTeamInfo {
            team_name: "compact-team".into(),
            ..Default::default()
        };
        let team_file = team_dir.join("pathflow-team.json");
        fs::write(&team_file, serde_json::to_string(&team_info).unwrap()).unwrap();

        // Source is "compact" -> should preserve existing session.
        // The caller is a surviving teammate whose lead's context overflowed.
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "compact", &mut result);

        assert!(existing.is_some(), "compact should return existing SID");
        assert_eq!(existing.unwrap(), old_sid);
        assert!(!team_mode, "compact should not be team mode");

        // Verify team file still exists (compact should not destroy it).
        assert!(team_file.exists(), "compact should preserve team file");
    }

    #[test]
    fn test_handle_stale_cleanup_resume_returns_existing_sid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7deadresume00000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Resume always takes minimal path -- returns existing SID without mutation.
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "resume", &mut result);

        assert!(existing.is_some(), "resume should return existing SID");
        assert_eq!(existing.unwrap(), old_sid);
        assert!(!team_mode, "resume should not be team mode");
    }

    #[test]
    fn test_handle_stale_cleanup_no_team_file_with_active_flag_compact() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7noteam000000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // Create status file but NO team file.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(old_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7noteam000000000000"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // compact source with active flag but no team file -> reuse SID.
        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "compact", &mut result);

        assert!(
            existing.is_some(),
            "compact with active flag should reuse SID"
        );
        assert_eq!(existing.unwrap(), old_sid);
        assert!(!team_mode);
    }

    #[test]
    fn test_handle_stale_cleanup_no_status_file_startup_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let old_sid = SessionId::new_unchecked("ses-01jq7orphanenv000000000");
        session::write_env_file(&runtime_dir, &old_sid, "codeflow").unwrap();

        // No status.json -> no active session -> new lead.
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let (existing, team_mode) =
            init.handle_stale_cleanup(dir.path(), &runtime_dir, "startup", &mut result);

        assert!(
            existing.is_none(),
            "no status file + startup should return None (new lead)"
        );
        assert!(!team_mode);
    }

    #[test]
    fn test_cleanup_active_task_removes_completed() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let home = tempfile::tempdir().unwrap();

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01complete"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: Some("Done task".into()),
            status: Some("complete".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            work_type: None,
            scope_policy: None,
            file_scope: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.cleanup_active_task(dir.path());

        assert!(
            session::get_active_task(&runtime_dir).unwrap().is_none(),
            "completed task should be cleared"
        );
    }

    #[test]
    fn test_cleanup_active_task_preserves_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let home = tempfile::tempdir().unwrap();

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01active"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: Some("Active task".into()),
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
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.cleanup_active_task(dir.path());

        assert!(
            session::get_active_task(&runtime_dir).unwrap().is_some(),
            "in_progress task should be preserved"
        );
    }

    #[test]
    fn test_cleanup_active_task_removes_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        let home = tempfile::tempdir().unwrap();

        let task = session::ActiveTask {
            task_id: crate::types::TaskId::new_unchecked("task-01cancel"),
            epic_id: None,
            task_format_id: None,
            epic_format_id: None,
            title: Some("Cancelled task".into()),
            status: Some("cancelled".into()),
            branch: None,
            session_id: None,
            created_at: None,
            updated_at: None,
            current_stage: None,
            team_name: None,
            work_type: None,
            scope_policy: None,
            file_scope: None,
        };
        session::set_active_task(&runtime_dir, &task).unwrap();

        let init = make_init(home.path().to_path_buf());
        init.cleanup_active_task(dir.path());

        assert!(
            session::get_active_task(&runtime_dir).unwrap().is_none(),
            "cancelled task should be cleared"
        );
    }

    #[test]
    fn test_init_checkpoint_missing_config() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.init_checkpoint(dir.path(), "ses-test", &mut result);

        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("pathflow-config.json not found")),
            "should warn about missing config"
        );
    }

    #[test]
    fn test_create_pathflow_flag_new() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7newflag0000000000000";

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let is_resume = init.create_pathflow_flag(dir.path(), sid, "startup", false, &mut result);
        assert!(!is_resume, "new status file should not be resume");

        // Verify status file was created.
        let status_path = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow")
            .join("pathflow-session-status.json");
        assert!(status_path.exists(), "status file should be created");

        // Verify content.
        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&status_path).unwrap()).unwrap();
        assert_eq!(data["status"], "created");
        assert_eq!(data["session_id"], sid);
    }

    #[test]
    fn test_create_pathflow_flag_existing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7existflag000000000000";

        // Pre-create the status file.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7existflag000000000000"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // Use "compact" source to test the resume path (startup would reset).
        let is_resume = init.create_pathflow_flag(dir.path(), sid, "compact", false, &mut result);
        assert!(
            is_resume,
            "existing status file with compact source should indicate resume"
        );
        assert!(result.messages.iter().any(|m| m.contains("Resuming")));
    }

    // test_write_session_metadata REMOVED: session-meta.json eliminated,
    // fields moved to pathflow-session-status.json.

    #[test]
    fn test_create_project_temp_dir_without_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.create_project_temp_dir(dir.path(), None, &mut result);

        let project_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let expected = PathBuf::from("/tmp/claude")
            .join(&project_name)
            .join("managed");
        assert!(expected.exists(), "project temp dir should be created");

        // Cleanup.
        let _ = fs::remove_dir_all(PathBuf::from("/tmp/claude").join(&project_name));
    }

    #[test]
    fn test_create_project_temp_dir_with_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        // Create a fake worktree structure: {dir}/.git-worktrees/worktree-ses-test
        let wt_parent = dir.path().join(".git-worktrees");
        fs::create_dir_all(&wt_parent).unwrap();
        let wt_root = wt_parent.join("worktree-ses-test");
        fs::create_dir_all(&wt_root).unwrap();

        let wp = WorktreePaths::new(&wt_root);
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.create_project_temp_dir(dir.path(), Some(&wp), &mut result);

        let expected = wp.temp_dir();
        assert!(
            expected.exists(),
            "worktree-scoped temp dir should be created"
        );
        assert!(
            expected.to_string_lossy().contains("worktree-ses-test"),
            "path should contain worktree name"
        );

        // Cleanup.
        let project_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let _ = fs::remove_dir_all(PathBuf::from("/tmp/claude").join(&project_name));
    }

    #[test]
    fn test_resolve_temp_dir_with_worktree() {
        let wt = WorktreePaths::new("/proj/.git-worktrees/worktree-ses-abc");
        let result = SessionStartInit::resolve_temp_dir(Path::new("/proj"), Some(&wt));
        assert_eq!(
            result,
            PathBuf::from("/tmp/claude/proj/worktree-ses-abc/managed")
        );
    }

    #[test]
    fn test_resolve_temp_dir_without_worktree() {
        let result = SessionStartInit::resolve_temp_dir(Path::new("/myproject"), None);
        assert_eq!(result, PathBuf::from("/tmp/claude/myproject/managed"));
    }

    #[test]
    fn test_resolve_temp_dir_creates_all_parents() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let wt_parent = dir.path().join(".git-worktrees");
        fs::create_dir_all(&wt_parent).unwrap();
        let wt_root = wt_parent.join("worktree-ses-parents");
        fs::create_dir_all(&wt_root).unwrap();

        let wp = WorktreePaths::new(&wt_root);
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // The temp dir shouldn't exist yet.
        let expected = wp.temp_dir();
        assert!(!expected.exists());

        init.create_project_temp_dir(dir.path(), Some(&wp), &mut result);

        // Now it should exist (create_dir_all creates parents).
        assert!(expected.exists());

        // Cleanup.
        let project_name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let _ = fs::remove_dir_all(PathBuf::from("/tmp/claude").join(&project_name));
    }

    #[test]
    fn test_instructions_handler_error_on_missing_project_dir() {
        let handler = SessionStartInstructions;
        let input = HookInput {
            tool_name: None,
            tool_input: None,
            event: HookEvent::SessionStart,
            session_id: None,
            project_dir: None,
            source: None,
            transcript_path: None,
            ..Default::default()
        };
        let result = handler.handle(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("project_dir required")
        );
    }

    #[test]
    fn test_resolve_session_id_from_existing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let existing = SessionId::new_unchecked("ses-01jq7exist000000000000a");
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "startup",
                "",
                Some(existing.clone()),
                &mut result,
            )
            .unwrap();

        assert_eq!(sid, existing, "should use existing SID when provided");
    }

    #[test]
    fn test_resolve_session_id_non_startup_no_env_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        // No env file on disk -- non-startup source should error.
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let err = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "resume",
                "",
                None,
                &mut result,
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("requires existing session"),
            "resume with no env file should error"
        );
    }

    #[test]
    fn test_resolve_session_id_non_startup_with_existing_sid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");

        let existing_sid = SessionId::new_unchecked("ses-01jq7envfile0000000000a");
        session::write_env_file(&runtime_dir, &existing_sid, "codeflow").unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // In the new design, handle_stale_cleanup for resume always returns
        // Some(existing_sid), so resolve_or_generate_session_id gets it
        // as Priority 1 (not None).
        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "resume",
                "",
                Some(existing_sid.clone()),
                &mut result,
            )
            .unwrap();

        assert_eq!(
            sid, existing_sid,
            "resume should use existing SID from stale cleanup"
        );
    }

    // --- Session lock tests ---

    #[test]
    fn test_acquire_session_lock_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        // Acquire first lock.
        let lock1 = acquire_session_lock(&runtime_dir).unwrap();

        // Try to acquire second lock in another thread -- should block.
        let runtime_dir2 = runtime_dir.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            // Signal that we're about to try locking.
            tx.send("trying").unwrap();
            let _lock2 = acquire_session_lock(&runtime_dir2).unwrap();
            tx.send("acquired").unwrap();
        });

        // Wait for the thread to start trying.
        assert_eq!(rx.recv().unwrap(), "trying");

        // Give it a moment -- it should NOT have acquired yet.
        assert!(
            rx.recv_timeout(std::time::Duration::from_millis(100))
                .is_err(),
            "second lock should block while first is held"
        );

        // Release first lock.
        drop(lock1);

        // Now the second lock should succeed.
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            "acquired"
        );
        handle.join().unwrap();
    }

    #[test]
    fn test_acquire_session_lock_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("nonexistent").join("runtime");

        let lock = acquire_session_lock(&runtime_dir).unwrap();
        assert!(runtime_dir.join("session.lock").exists());
        drop(lock);
    }

    #[test]
    fn test_concurrent_startup_single_session_id() {
        // Simulates the real race: multiple agents start concurrently.
        // The lead (thread 0) runs first (serialized by lock), generates a
        // session, writes env file, writes pathflow-session-status.json
        // (simulating TeamCreate), and creates team config.
        // Subsequent threads acquire the lock, see the env file + status file
        // + team config, and enter teammate mode -- all converging on same SID.
        // No env var needed -- detection is purely file-based.
        use std::sync::{Arc, Barrier, Mutex};
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let project_dir = Arc::new(dir.path().to_path_buf());
        let home_dir = Arc::new(home.path().to_path_buf());

        // Phase 1: Lead agent creates the session.
        let lead_init = SessionStartInit {
            lead_pid: 1000,
            home_dir: home_dir.as_ref().clone(),
            now: fixed_now,
        };
        let lead_input = make_input("startup", project_dir.to_str().unwrap());
        let mut buf = Vec::new();
        let lead_result = lead_init.run(&lead_input, &project_dir, &mut buf).unwrap();
        let lead_sid = lead_result.session_id.clone();

        // Lead writes pathflow-session-status.json (simulates TeamCreate PostToolUse).
        // Use current process PID as lead_pid so kill(pid, 0) succeeds in test.
        let test_pid = std::process::id();
        let pathflow_dir = project_dir
            .join(".state")
            .join("session")
            .join(lead_sid.as_str())
            .join("pathflow");
        fs::create_dir_all(&pathflow_dir).unwrap();
        fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": lead_sid.as_str(), "status": "pf-in-progress",
                "team_name": "test-team", "lead_pid": test_pid,
                "updated_at": fixed_now(),
            }))
            .unwrap(),
        )
        .unwrap();

        // Create Claude Code team config.
        let config_dir = home.path().join(".claude").join("teams").join("test-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), r#"{"members": []}"#).unwrap();

        // Phase 2: Multiple teammate agents start concurrently.
        // Teammate detection is file-based: env file + status.json + config.json.
        let num_teammates = 4;
        let barrier = Arc::new(Barrier::new(num_teammates));
        let results = Arc::new(Mutex::new(Vec::new()));

        let handles: Vec<_> = (0..num_teammates)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let project_dir = Arc::clone(&project_dir);
                let home_dir = Arc::clone(&home_dir);
                let results = Arc::clone(&results);

                thread::spawn(move || {
                    let init = SessionStartInit {
                        lead_pid: 2000,
                        home_dir: home_dir.as_ref().clone(),
                        now: fixed_now,
                    };
                    let input = make_input("startup", project_dir.to_str().unwrap());

                    // Synchronize all threads to start at once.
                    barrier.wait();

                    let mut buf = Vec::new();
                    let result = init.run(&input, &project_dir, &mut buf).unwrap();
                    results.lock().unwrap().push(result);
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let teammate_results = results.lock().unwrap();
        // All teammates should get the same SID as the lead.
        for (i, r) in teammate_results.iter().enumerate() {
            assert_eq!(
                r.session_id,
                lead_sid,
                "teammate {i} got different SID: {} vs {lead_sid}",
                r.session_id.as_str()
            );
            assert!(r.is_teammate, "teammate {i} should be in teammate mode");
        }
    }

    // --- Bug 3: Deterministic sweep tests ---

    #[test]
    fn test_sweep_created_no_config_cleans() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000010";
        let session_base = dir.path().join(".state").join("session");

        // Create a "created" session with no team config.
        let stale_sid = "ses-01jq7stale00000000000010";
        let stale_pathflow = session_base.join(stale_sid).join("pathflow");
        fs::create_dir_all(&stale_pathflow).unwrap();
        fs::write(
            stale_pathflow.join("pathflow-session-status.json"),
            r#"{"status":"created","team_name":"","updated_at":"2026-03-10T00:00:00Z"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            !session_base.join(stale_sid).exists(),
            "created session with no config should be cleaned (no time threshold)"
        );
    }

    #[test]
    fn test_sweep_active_no_team_name_cleans() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000011";
        let session_base = dir.path().join(".state").join("session");

        // pf-in-progress but empty team_name -- dead session.
        let stale_sid = "ses-01jq7stale00000000000011";
        let stale_pathflow = session_base.join(stale_sid).join("pathflow");
        fs::create_dir_all(&stale_pathflow).unwrap();
        fs::write(
            stale_pathflow.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"","updated_at":"2026-03-10T00:00:00Z"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            !session_base.join(stale_sid).exists(),
            "active session with empty team_name should be cleaned (no time threshold)"
        );
    }

    #[test]
    fn test_sweep_active_config_gone_cleans() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000012";
        let session_base = dir.path().join(".state").join("session");

        // pf-in-progress with team_name but config.json missing -- dead session.
        let stale_sid = "ses-01jq7stale00000000000012";
        let stale_pathflow = session_base.join(stale_sid).join("pathflow");
        fs::create_dir_all(&stale_pathflow).unwrap();
        fs::write(
            stale_pathflow.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"dead-team","updated_at":"2026-03-10T00:00:00Z"}"#,
        )
        .unwrap();
        // No team config created -- config.json does not exist.

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            !session_base.join(stale_sid).exists(),
            "active session with missing config should be cleaned (no time threshold)"
        );
    }

    #[test]
    fn test_sweep_active_config_exists_keeps() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let current_sid = "ses-01jq7current000000000013";
        let session_base = dir.path().join(".state").join("session");

        // pf-in-progress with team config that EXISTS -- alive session.
        let alive_sid = "ses-01jq7alive00000000000013";
        let alive_pathflow = session_base.join(alive_sid).join("pathflow");
        fs::create_dir_all(&alive_pathflow).unwrap();
        fs::write(
            alive_pathflow.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","team_name":"alive-team","updated_at":"2026-03-10T00:00:00Z"}"#,
        )
        .unwrap();

        let config_dir = home.path().join(".claude").join("teams").join("alive-team");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.json"), "{}").unwrap();

        let init = make_init(home.path().to_path_buf());
        init.sweep_all_stale_sessions(dir.path(), current_sid);

        assert!(
            session_base.join(alive_sid).exists(),
            "active session with existing config should be kept"
        );
    }

    // --- Bug 4: Startup always generates new SID ---

    #[test]
    fn test_resolve_startup_generates_new_sid() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "startup",
                "",
                None,
                &mut result,
            )
            .unwrap();

        assert!(
            sid.as_str().starts_with("ses-"),
            "startup should generate new SID with correct prefix"
        );
    }

    #[test]
    fn test_resolve_generates_new_sid_when_no_existing() {
        // After SAFEGUARD removal: when handle_stale_cleanup returns (None, false),
        // resolve_or_generate should always generate a new SID for startup.
        // Teammate detection is fully handled by handle_stale_cleanup (PID-based).
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-unknown"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let sid = init
            .resolve_or_generate_session_id(
                dir.path(),
                &runtime_dir,
                "startup",
                "",
                None,
                &mut result,
            )
            .unwrap();

        assert!(
            sid.as_str().starts_with("ses-"),
            "should generate a new SID: {}",
            sid.as_str()
        );
        assert!(!result.is_teammate, "should not be teammate");
    }

    // --- Bug 5: Startup resets stale status file ---

    #[test]
    fn test_create_pathflow_flag_startup_resets_stale_status() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7staleflag000000000000";

        // Pre-create the status file with stale active status.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7staleflag000000000000"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // source=startup should reset the status file, not resume.
        let is_resume = init.create_pathflow_flag(dir.path(), sid, "startup", false, &mut result);
        assert!(
            !is_resume,
            "startup with stale status file should NOT resume"
        );
        assert!(
            result.messages.iter().any(|m| m.contains("resetting")),
            "should log that status was reset"
        );

        // Verify the status file was overwritten with "created".
        let status_path = flag_dir.join("pathflow-session-status.json");
        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&status_path).unwrap()).unwrap();
        assert_eq!(
            data["status"], "created",
            "status should be reset to 'created'"
        );
    }

    #[test]
    fn test_create_pathflow_flag_compact_preserves_resume() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7compctflag00000000000";

        // Pre-create the status file with active status.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7compctflag00000000000"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // source=compact should preserve the existing status and return resume.
        let is_resume = init.create_pathflow_flag(dir.path(), sid, "compact", false, &mut result);
        assert!(
            is_resume,
            "compact with active status file should indicate resume"
        );
        assert!(result.messages.iter().any(|m| m.contains("Resuming")));
    }

    #[test]
    fn test_create_pathflow_flag_teammate_preserves_status() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7teammateflag0000000";

        // Pre-create the status file with active status.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        let original_content = r#"{"status":"pf-in-progress","session_id":"ses-01jq7teammateflag0000000","team_name":"my-team"}"#;
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            original_content,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // is_teammate=true should preserve existing status regardless of source.
        let is_resume = init.create_pathflow_flag(dir.path(), sid, "startup", true, &mut result);
        assert!(is_resume, "teammate should preserve status (resume=true)");
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Teammate joining")),
            "should log teammate preserving status"
        );

        // Verify the status file was NOT overwritten.
        let status_path = flag_dir.join("pathflow-session-status.json");
        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&status_path).unwrap()).unwrap();
        assert_eq!(
            data["status"], "pf-in-progress",
            "teammate should not reset status to 'created'"
        );
    }

    #[test]
    fn test_create_pathflow_flag_not_teammate_startup_resets() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7nonteammate00000000";

        // Pre-create the status file with active status.
        let flag_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow");
        fs::create_dir_all(&flag_dir).unwrap();
        fs::write(
            flag_dir.join("pathflow-session-status.json"),
            r#"{"status":"pf-in-progress","session_id":"ses-01jq7nonteammate00000000"}"#,
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // is_teammate=false + source=startup should reset the status.
        let is_resume = init.create_pathflow_flag(dir.path(), sid, "startup", false, &mut result);
        assert!(!is_resume, "non-teammate startup should reset status");

        let status_path = flag_dir.join("pathflow-session-status.json");
        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&status_path).unwrap()).unwrap();
        assert_eq!(
            data["status"], "created",
            "non-teammate startup should reset to 'created'"
        );
    }

    // --- Worktree integration tests ---

    /// Helper: initialize a git repo with an initial commit (required for worktree creation).
    fn init_git_repo(dir: &Path) -> git2::Repository {
        let repo = git2::Repository::init(dir).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .unwrap();
        }
        repo
    }

    /// Write a parallel-work config with `worktree.mode=always` into the tempdir.
    /// Required for tests that expect worktree creation on startup.
    fn write_worktree_always_config(dir: &Path) {
        let config_dir = dir.join(".codeflow").join("config").join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "mode": "always" } }"#,
        )
        .unwrap();
    }

    #[test]
    fn test_startup_creates_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        let sid = result.session_id.as_str();

        // Verify CODEFLOW_WORKTREE_PATH is in env_vars.
        assert!(
            result.env_vars.contains_key("CODEFLOW_WORKTREE_PATH"),
            "CODEFLOW_WORKTREE_PATH should be set on startup"
        );
        let wt_path = &result.env_vars["CODEFLOW_WORKTREE_PATH"];
        assert!(
            wt_path.contains(".git-worktrees"),
            "worktree path should be under .git-worktrees: {wt_path}"
        );
        assert!(
            wt_path.contains(sid),
            "worktree path should contain session ID: {wt_path}"
        );

        // Verify the worktree directory exists.
        assert!(
            PathBuf::from(wt_path).exists(),
            "worktree directory should exist: {wt_path}"
        );

        // Verify the worktree has a .git file (valid git worktree).
        assert!(
            PathBuf::from(wt_path).join(".git").exists(),
            "worktree should have .git file"
        );
    }

    #[test]
    fn test_compact_reuses_existing_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        // First run: startup creates worktree.
        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());
        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();
        let sid = result.session_id.clone();
        let wt_path = result.env_vars["CODEFLOW_WORKTREE_PATH"].clone();

        // Simulate compact: env file already has CODEFLOW_WORKTREE_PATH from startup.
        // The startup run already wrote it to main env file.
        let input2 = make_input("compact", dir.path().to_str().unwrap());
        let mut buf2 = Vec::new();
        let result2 = init.run(&input2, dir.path(), &mut buf2).unwrap();

        assert_eq!(result2.session_id, sid);
        assert_eq!(
            result2
                .env_vars
                .get("CODEFLOW_WORKTREE_PATH")
                .map(|s| s.as_str()),
            Some(wt_path.as_str()),
            "compact should reuse the same worktree path"
        );
    }

    #[test]
    fn test_resume_reuses_existing_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        // Startup: create worktree.
        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());
        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();
        let sid = result.session_id.clone();
        let wt_path = result.env_vars["CODEFLOW_WORKTREE_PATH"].clone();

        // Resume: should recover the worktree path.
        let input2 = make_input("resume", dir.path().to_str().unwrap());
        let mut buf2 = Vec::new();
        let result2 = init.run(&input2, dir.path(), &mut buf2).unwrap();

        assert_eq!(result2.session_id, sid);
        assert_eq!(
            result2
                .env_vars
                .get("CODEFLOW_WORKTREE_PATH")
                .map(|s| s.as_str()),
            Some(wt_path.as_str()),
            "resume should reuse the same worktree path"
        );
    }

    #[test]
    fn test_startup_worktree_handle_defused() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        // The worktree should still exist after run() completes (handle was defused).
        let wt_path = &result.env_vars["CODEFLOW_WORKTREE_PATH"];
        assert!(
            PathBuf::from(wt_path).exists(),
            "worktree should persist after defused handle drop"
        );
    }

    #[test]
    fn test_startup_env_file_in_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        let wt_path = PathBuf::from(&result.env_vars["CODEFLOW_WORKTREE_PATH"]);
        let wt_env_file = wt_path
            .join(".state")
            .join("runtime")
            .join("codeflow-env.sh");
        assert!(
            wt_env_file.exists(),
            "codeflow-env.sh should be written in worktree: {}",
            wt_env_file.display()
        );

        // Read and verify the worktree env file content.
        let content = fs::read_to_string(&wt_env_file).unwrap();
        assert!(content.contains("CODEFLOW_SESSION_ID"));
        assert!(content.contains("CODEFLOW_WORKTREE_PATH"));
    }

    #[test]
    fn test_startup_pathflow_flag_in_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        let wt_path = PathBuf::from(&result.env_vars["CODEFLOW_WORKTREE_PATH"]);
        let sid = result.session_id.as_str();

        // PathFlow flag should be in the worktree.
        let flag_path = wt_path
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow")
            .join("pathflow-session-status.json");
        assert!(
            flag_path.exists(),
            "pathflow-session-status.json should be in worktree: {}",
            flag_path.display()
        );
    }

    #[test]
    fn test_startup_worktree_session_id_in_registry() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        let sid = result.session_id.as_str();

        // Read the worktree registry and verify session_id is set.
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = crate::worktree::read_registry(&reg_path).unwrap();
        let wt_name = format!("worktree-{sid}");
        let entry = reg.worktrees.iter().find(|e| e.name == wt_name);
        assert!(entry.is_some(), "registry should contain worktree entry");
        assert_eq!(
            entry.unwrap().session_id.as_deref(),
            Some(sid),
            "registry entry should have session_id set"
        );
    }

    #[test]
    fn test_startup_worktree_env_json_output() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _repo = init_git_repo(dir.path());
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let _result = init.run(&input, dir.path(), &mut buf).unwrap();

        // Verify JSON output includes CODEFLOW_WORKTREE_PATH.
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("CODEFLOW_WORKTREE_PATH"),
            "env JSON output should contain CODEFLOW_WORKTREE_PATH"
        );
    }

    #[test]
    fn test_startup_without_git_repo_degrades_gracefully() {
        // When there's no git repo, worktree creation fails gracefully
        // and session proceeds without a worktree.
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        // No git init -- setup_detached will fail.
        write_worktree_always_config(dir.path());

        let init = make_init(home.path().to_path_buf());
        let input = make_input("startup", dir.path().to_str().unwrap());

        let mut buf = Vec::new();
        let result = init.run(&input, dir.path(), &mut buf).unwrap();

        // Should still succeed, just without worktree.
        assert!(result.session_id.as_str().starts_with("ses-"));
        assert!(
            !result.env_vars.contains_key("CODEFLOW_WORKTREE_PATH"),
            "no worktree should mean no CODEFLOW_WORKTREE_PATH"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("worktree creation failed")),
            "should have warning about worktree creation failure"
        );
    }

    // --- Stale worktree cleanup tests ---

    #[test]
    fn test_clean_stale_worktrees_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // No worktrees.yaml — should return early without errors.
        init.clean_stale_worktrees(dir.path(), &mut result);

        assert!(result.warnings.is_empty(), "no warnings expected");
        assert!(result.messages.is_empty(), "no messages expected");
    }

    #[test]
    fn test_clean_stale_worktrees_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());

        // Initialize a git repo (required for the prune call at the end).
        let _repo = git2::Repository::init(dir.path()).unwrap();

        // Create an empty registry.
        let reg = crate::worktree::WorktreeRegistry::new("2026-03-18T00:00:00Z");
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.clean_stale_worktrees(dir.path(), &mut result);

        assert!(result.warnings.is_empty(), "no warnings expected");
        assert!(result.messages.is_empty(), "no messages expected");
    }

    #[test]
    fn test_clean_stale_worktrees_dead_session_triggers_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());

        // Create a registry with an active worktree owned by a dead session.
        let dead_sid = "ses-01jq7deadbeef000000000ab";
        let mut reg = crate::worktree::WorktreeRegistry::new("2026-03-18T00:00:00Z");
        reg.worktrees.push(crate::worktree::WorktreeEntry {
            name: "stale-wt".to_string(),
            path: dir
                .path()
                .join(".git-worktrees")
                .join("stale-wt")
                .to_string_lossy()
                .to_string(),
            branch: "feat/stale".to_string(),
            created_at: "2026-03-18T00:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: Some(dead_sid.to_string()),
            task_id: None,
        });
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        // Create session status file with a dead PID (PID 1 is init, use a very
        // large PID that certainly doesn't exist).
        let status_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(dead_sid)
            .join("pathflow");
        fs::create_dir_all(&status_dir).unwrap();
        let status = serde_json::json!({
            "session_id": dead_sid,
            "status": "pf-in-progress",
            "lead_pid": 999999999_u64,
            "team_name": "dead-team",
        });
        fs::write(
            status_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.clean_stale_worktrees(dir.path(), &mut result);

        // Should detect and report the stale worktree.
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("STALE WORKTREE") && m.contains("stale-wt")),
            "should report stale worktree cleanup: {:?}",
            result.messages,
        );
    }

    #[test]
    fn test_clean_stale_worktrees_skips_active_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());

        // Use current process PID so is_process_alive returns true.
        let live_pid = std::process::id();
        let live_sid = "ses-01jq7livesess000000000ab";

        let mut reg = crate::worktree::WorktreeRegistry::new("2026-03-18T00:00:00Z");
        reg.worktrees.push(crate::worktree::WorktreeEntry {
            name: "active-wt".to_string(),
            path: dir
                .path()
                .join(".git-worktrees")
                .join("active-wt")
                .to_string_lossy()
                .to_string(),
            branch: "feat/active".to_string(),
            created_at: "2026-03-18T00:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: Some(live_sid.to_string()),
            task_id: None,
        });
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        // Create session status with a LIVE PID.
        let status_dir = dir
            .path()
            .join(".state")
            .join("session")
            .join(live_sid)
            .join("pathflow");
        fs::create_dir_all(&status_dir).unwrap();
        let status = serde_json::json!({
            "session_id": live_sid,
            "status": "pf-in-progress",
            "lead_pid": live_pid,
            "team_name": "live-team",
        });
        fs::write(
            status_dir.join("pathflow-session-status.json"),
            serde_json::to_string_pretty(&status).unwrap(),
        )
        .unwrap();

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.clean_stale_worktrees(dir.path(), &mut result);

        // Active session — should NOT be cleaned.
        assert!(
            !result.messages.iter().any(|m| m.contains("STALE WORKTREE")),
            "should not report stale worktree for active session: {:?}",
            result.messages,
        );
    }

    #[test]
    fn test_clean_stale_worktrees_skips_no_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());

        // Registry entry with no session_id (legacy or manually created).
        let mut reg = crate::worktree::WorktreeRegistry::new("2026-03-18T00:00:00Z");
        reg.worktrees.push(crate::worktree::WorktreeEntry {
            name: "legacy-wt".to_string(),
            path: dir
                .path()
                .join(".git-worktrees")
                .join("legacy-wt")
                .to_string_lossy()
                .to_string(),
            branch: "feat/legacy".to_string(),
            created_at: "2026-03-18T00:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.clean_stale_worktrees(dir.path(), &mut result);

        // No session_id — should be skipped (cannot determine liveness).
        assert!(
            !result.messages.iter().any(|m| m.contains("STALE WORKTREE")),
            "should skip worktree without session_id: {:?}",
            result.messages,
        );
    }

    #[test]
    fn test_clean_stale_worktrees_skips_removed_entries() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let init = make_init(home.path().to_path_buf());

        // Registry entry with status "removed" — already cleaned up.
        let mut reg = crate::worktree::WorktreeRegistry::new("2026-03-18T00:00:00Z");
        reg.worktrees.push(crate::worktree::WorktreeEntry {
            name: "old-wt".to_string(),
            path: dir
                .path()
                .join(".git-worktrees")
                .join("old-wt")
                .to_string_lossy()
                .to_string(),
            branch: "feat/old".to_string(),
            created_at: "2026-03-18T00:00:00Z".to_string(),
            status: "removed".to_string(),
            session_id: Some("ses-01jq7deadbeef000000000ab".to_string()),
            task_id: None,
        });
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-test"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.clean_stale_worktrees(dir.path(), &mut result);

        // Already removed — should be skipped.
        assert!(
            !result.messages.iter().any(|m| m.contains("STALE WORKTREE")),
            "should skip already-removed worktree: {:?}",
            result.messages,
        );
    }

    // --- Pre-created worktree detection tests ---

    /// Helper: create a minimal pre-created worktree directory structure.
    /// Simulates what the autorun orchestrator would create.
    fn setup_precreated_worktree(
        project_dir: &Path,
        wt_name: &str,
    ) -> (PathBuf, crate::worktree::WorktreeRegistry) {
        let wt_dir = project_dir.join(".git-worktrees").join(wt_name);
        let state_runtime = wt_dir.join(".state").join("runtime");
        fs::create_dir_all(&state_runtime).unwrap();

        // Create a registry with the worktree entry.
        let mut reg = crate::worktree::WorktreeRegistry::new("2026-03-18T00:00:00Z");
        reg.worktrees.push(crate::worktree::WorktreeEntry {
            name: wt_name.to_string(),
            path: wt_dir.to_string_lossy().to_string(),
            branch: String::new(),
            created_at: "2026-03-18T00:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: Some("ses-orchestrator00000000000".to_string()),
            task_id: None,
        });

        let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
        fs::create_dir_all(registry_path.parent().unwrap()).unwrap();
        crate::worktree::write_registry(&registry_path, &reg).unwrap();

        (wt_dir, reg)
    }

    #[test]
    fn test_detect_precreated_from_env_var() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let (wt_dir, _reg) = setup_precreated_worktree(dir.path(), "worktree-pre1");

        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-worker00000000000000001"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            "ses-worker00000000000000001",
            Some(wt_dir.to_str().unwrap()),
            &mut result,
        );

        assert!(
            detected.is_some(),
            "should detect pre-created worktree from env var"
        );
        assert_eq!(
            detected.unwrap().root(),
            wt_dir,
            "detected path should match"
        );
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Pre-created detected")),
            "should log detection: {:?}",
            result.messages,
        );
    }

    #[test]
    fn test_detect_precreated_from_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let (wt_dir, _reg) = setup_precreated_worktree(dir.path(), "worktree-pre2");

        let runtime_dir = dir.path().join(".state").join("runtime");
        // Write env file WITH worktree path (simulates orchestrator writing it).
        let sid = SessionId::new_unchecked("ses-orchestrator00000000000");
        session::write_env_file_with_worktree(
            &runtime_dir,
            &sid,
            "codeflow",
            Some(wt_dir.to_str().unwrap()),
        )
        .unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-worker00000000000000002"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // No env var (None) — should fall back to env file.
        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            "ses-worker00000000000000002",
            None,
            &mut result,
        );

        assert!(
            detected.is_some(),
            "should detect pre-created worktree from env file"
        );
        assert_eq!(detected.unwrap().root(), wt_dir);
        assert!(
            result
                .messages
                .iter()
                .any(|m| m.contains("Pre-created detected")),
            "should log detection: {:?}",
            result.messages,
        );
    }

    #[test]
    fn test_detect_precreated_no_signal() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-worker00000000000000003"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // No env var, no env file — should return None.
        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            "ses-worker00000000000000003",
            None,
            &mut result,
        );

        assert!(detected.is_none(), "no signal should return None");
        assert!(result.messages.is_empty(), "no messages expected");
        assert!(result.warnings.is_empty(), "no warnings expected");
    }

    #[test]
    fn test_detect_precreated_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-worker00000000000000004"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        // Env var points to nonexistent directory.
        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            "ses-worker00000000000000004",
            Some("/nonexistent/worktree/path"),
            &mut result,
        );

        assert!(detected.is_none(), "nonexistent dir should return None");
    }

    #[test]
    fn test_detect_precreated_state_missing() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();

        // Create worktree directory but WITHOUT .state/runtime/.
        let wt_dir = dir
            .path()
            .join(".git-worktrees")
            .join("worktree-incomplete");
        fs::create_dir_all(&wt_dir).unwrap();

        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked("ses-worker00000000000000005"),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            "ses-worker00000000000000005",
            Some(wt_dir.to_str().unwrap()),
            &mut result,
        );

        assert!(
            detected.is_none(),
            "missing .state/runtime should return None"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains(".state/runtime/ missing")),
            "should warn about missing .state/runtime: {:?}",
            result.warnings,
        );
    }

    #[test]
    fn test_detect_precreated_updates_registry_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let wt_name = "worktree-pre-reg";
        let (wt_dir, _reg) = setup_precreated_worktree(dir.path(), wt_name);

        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let worker_sid = "ses-worker00000000000000006";

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(worker_sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            worker_sid,
            Some(wt_dir.to_str().unwrap()),
            &mut result,
        );
        assert!(detected.is_some());

        // Verify registry was updated with worker's session_id.
        let mgr = WorktreeManager::new(dir.path());
        let reg = crate::worktree::read_registry(mgr.registry_path()).unwrap();
        let entry = reg.worktrees.iter().find(|e| e.name == wt_name).unwrap();
        assert_eq!(
            entry.session_id.as_deref(),
            Some(worker_sid),
            "registry entry should have worker's session_id, not orchestrator's"
        );
    }

    #[test]
    fn test_detect_precreated_updates_main_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let (wt_dir, _reg) = setup_precreated_worktree(dir.path(), "worktree-pre-env");

        let runtime_dir = dir.path().join(".state").join("runtime");
        fs::create_dir_all(&runtime_dir).unwrap();

        let worker_sid = "ses-worker00000000000000007";

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(worker_sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        let detected = init.detect_precreated_worktree_inner(
            dir.path(),
            &runtime_dir,
            worker_sid,
            Some(wt_dir.to_str().unwrap()),
            &mut result,
        );
        assert!(detected.is_some());

        // Verify main env file was rewritten with worker's session_id.
        let env = session::read_env_file(&runtime_dir).unwrap().unwrap();
        assert_eq!(
            env.session_id.as_str(),
            worker_sid,
            "env file should have worker's session_id"
        );
        assert_eq!(
            env.worktree_path.as_deref(),
            Some(wt_dir.to_str().unwrap()),
            "env file should have worktree path"
        );
    }

    // -- BUG #1: work_type field in initial pathflow-session-status.json --

    #[test]
    fn test_create_pathflow_flag_includes_work_type() {
        let dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let sid = "ses-01jq7worktype000000000000";

        let init = make_init(home.path().to_path_buf());
        let mut result = InitResult {
            session_id: SessionId::new_unchecked(sid),
            is_resume: false,
            is_teammate: false,
            env_vars: HashMap::new(),
            warnings: Vec::new(),
            messages: Vec::new(),
        };

        init.create_pathflow_flag(dir.path(), sid, "startup", false, &mut result);

        let status_path = dir
            .path()
            .join(".state")
            .join("session")
            .join(sid)
            .join("pathflow")
            .join("pathflow-session-status.json");
        assert!(status_path.exists(), "status file should be created");

        let data: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&status_path).unwrap()).unwrap();

        // BUG #1 fix: work_type field must exist with empty string default.
        assert!(
            data.get("work_type").is_some(),
            "pathflow-session-status.json must include work_type field"
        );
        assert_eq!(
            data["work_type"], "",
            "initial work_type should be empty string"
        );
    }

    // -- BUG #5: early env write includes CODEFLOW_WORKTREE_PATH placeholder --

    #[test]
    fn test_early_env_write_includes_worktree_path_placeholder() {
        // write_env_file_with_worktree(dir, sid, root, Some("")) should produce
        // an env file containing CODEFLOW_WORKTREE_PATH with an empty value.
        let dir = tempfile::tempdir().unwrap();
        let sid = SessionId::new_unchecked("ses-01jq7earlyenv00000000000");

        session::write_env_file_with_worktree(dir.path(), &sid, "codeflow", Some("")).unwrap();

        let env = session::read_env_file(dir.path()).unwrap().unwrap();
        assert_eq!(env.session_id, sid);
        assert_eq!(env.project_root, "codeflow");

        // BUG #5 fix: CODEFLOW_WORKTREE_PATH must be present (even if empty).
        assert!(
            env.worktree_path.is_some(),
            "early env write must include CODEFLOW_WORKTREE_PATH"
        );
        assert_eq!(
            env.worktree_path.as_deref(),
            Some(""),
            "early env write should have empty CODEFLOW_WORKTREE_PATH placeholder"
        );
    }
}
