//! Interactive session management: `codeflow interactive` / `codeflow -i`.
//!
//! Creates a worktree and generates a session ID BEFORE starting Claude Code,
//! eliminating the shared `codeflow-env.sh` singleton race condition.
//!
//! Subcommands: `status`, `list`, `cleanup`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;

use crate::helpers;

/// Interactive session subcommands.
#[derive(Debug, Subcommand)]
pub enum InteractiveCommand {
    /// Show active interactive sessions with PID liveness
    Status,
    /// List all interactive sessions (active, complete, stale)
    List,
    /// Remove stale sessions (dead PID detection)
    Cleanup,
}

/// Run the interactive subcommand or launch a new session.
pub async fn run(command: Option<InteractiveCommand>) -> Result<()> {
    match command {
        None => run_launch().await,
        Some(InteractiveCommand::Status) => run_status().await,
        Some(InteractiveCommand::List) => run_list().await,
        Some(InteractiveCommand::Cleanup) => run_cleanup().await,
    }
}

/// Launch a new interactive session: generate SID, optionally create worktree,
/// register InteractiveSession, then exec claude.
async fn run_launch() -> Result<()> {
    let project_dir =
        helpers::detect_project_root().context("detecting project root for interactive session")?;

    // Generate session ID.
    let session_id = codeflow_core::session::generate_session_id();
    let sid_str = session_id.as_str().to_string();

    // Load worktree config.
    let wt_mode = codeflow_core::autorun::config::load_config(&project_dir).map_or(
        codeflow_core::autorun::config::WorktreeMode::Disabled,
        |c| c.worktree.mode,
    );

    let worktree_path = match wt_mode {
        codeflow_core::autorun::config::WorktreeMode::Always => {
            // Create worktree.
            let wt_name = worktree_name(&sid_str);
            let mgr = codeflow_core::worktree::WorktreeManager::new(&project_dir);
            let entry = mgr
                .setup_detached(&wt_name)
                .map_err(|e| anyhow::anyhow!("worktree creation failed: {e}"))?;

            // Update registry with session_id and lead_pid.
            let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
            let _ = codeflow_core::worktree::locked_update_session_id(
                &registry_path,
                &wt_name,
                &sid_str,
            );
            let _ = codeflow_core::worktree::locked_update_lead_pid(
                &registry_path,
                &wt_name,
                std::process::id(),
            );
            let _ = codeflow_core::worktree::locked_update_source(
                &registry_path,
                &wt_name,
                "interactive",
            );

            let wt_path_str = entry.path.clone();

            // Write env file inside the worktree.
            let wt_path = PathBuf::from(&wt_path_str);
            let wt_runtime = wt_path.join(".state").join("runtime");
            let project_root_str = project_dir.to_string_lossy().to_string();
            let _ = codeflow_core::session::write_env_file_with_worktree(
                &wt_runtime,
                &session_id,
                &project_root_str,
                Some(&wt_path_str),
            );

            // Write session pointer in main repo.
            codeflow_core::session::write_session_pointer(
                &project_dir,
                &sid_str,
                std::process::id(),
                &wt_path_str,
                &chrono::Utc::now().to_rfc3339(),
            );

            // Write per-PID env file in main repo.
            let main_runtime = project_dir.join(".state").join("runtime");
            codeflow_core::session::write_pid_env_file(
                &main_runtime,
                std::process::id(),
                &wt_path_str,
            );

            Some(wt_path_str)
        }
        codeflow_core::autorun::config::WorktreeMode::Disabled
        | codeflow_core::autorun::config::WorktreeMode::Autorun => {
            // No worktree: write shared env file.
            let runtime_dir = project_dir.join(".state").join("runtime");
            let project_root_str = project_dir.to_string_lossy().to_string();
            let _ = codeflow_core::session::write_env_file(
                &runtime_dir,
                &session_id,
                &project_root_str,
            );
            None
        }
    };

    // Register InteractiveSession in DB (best-effort).
    register_interactive_session(
        &project_dir,
        &sid_str,
        worktree_path.as_deref(),
        "codeflow",
        true,
    )
    .await;

    // Build env vars for the claude process.
    let project_dir_str = project_dir.to_string_lossy().to_string();
    let env_vars = build_env_vars(&sid_str, &project_dir_str, worktree_path.as_deref());

    eprintln!("codeflow: session {sid_str} starting");
    if let Some(ref wt_path) = worktree_path {
        eprintln!("codeflow: worktree at {wt_path}");
    }

    // Exec claude -- replaces the current process.
    let work_dir = resolve_work_dir(worktree_path.as_deref(), &project_dir_str);

    let err = exec_claude(work_dir, &env_vars);
    anyhow::bail!("failed to exec claude: {err}");
}

/// Register an `InteractiveSession` record in SurrealDB.
///
/// Non-blocking: continues if DB is unavailable.
async fn register_interactive_session(
    project_dir: &Path,
    session_id: &str,
    worktree_path: Option<&str>,
    source_cli: &str,
    managed: bool,
) {
    let store = match open_store(project_dir).await {
        Ok(s) => s,
        Err(_) => return,
    };

    let now = chrono::Utc::now().to_rfc3339();
    let pid = i64::from(std::process::id());
    let wt = worktree_path.map(String::from);
    let _ = store
        .db()
        .query(
            "CREATE interactive_session SET \
             session_id = $session_id, \
             pid = $pid, \
             status = 'active', \
             worktree_path = $worktree_path, \
             branch = NONE, \
             work_type = NONE, \
             team_name = NONE, \
             source_cli = $source_cli, \
             managed = $managed, \
             created_at = $created_at, \
             updated_at = NONE, \
             completed_at = NONE;",
        )
        .bind(("session_id", session_id.to_string()))
        .bind(("pid", pid))
        .bind(("worktree_path", wt))
        .bind(("source_cli", source_cli.to_string()))
        .bind(("managed", managed))
        .bind(("created_at", now))
        .await;
}

/// Show active interactive sessions.
///
/// Queries the DB for active sessions AND scans `.state/interactive/heartbeat-*`
/// as a filesystem fallback so sessions are visible even when the DB is unavailable.
async fn run_status() -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    let sessions = query_sessions(&project_dir, Some("active")).await?;

    let db_sids: std::collections::HashSet<String> =
        sessions.iter().map(|s| s.session_id.clone()).collect();

    // Filesystem fallback: scan heartbeat files for sessions not in DB.
    let fs_only = scan_heartbeat_sessions(&project_dir, &db_sids);

    if sessions.is_empty() && fs_only.is_empty() {
        println!("No active interactive sessions.");
        return Ok(());
    }

    println!(
        "{:<32} {:<14} {:<10} {:<10} {:<12} {:<8} {:<8} {:<30}",
        "SESSION ID", "PID", "STATUS", "SOURCE", "BRANCH", "TYPE", "PHASE", "WORKTREE"
    );
    for s in &sessions {
        let pid_display = format_pid_with_liveness(s.pid);
        let wt = s.worktree_path.as_deref().unwrap_or("-");
        let branch = s.branch.as_deref().unwrap_or("-");
        let work_type = s.work_type.as_deref().unwrap_or("-");
        // Derive phase from pathflow-session-status.json if available.
        let phase = derive_session_phase(s);
        println!(
            "{:<32} {:<14} {:<10} {:<10} {:<12} {:<8} {:<8} {wt}",
            s.session_id, pid_display, s.status, s.source_cli, branch, work_type, phase
        );
    }
    for (sid, alive) in &fs_only {
        let liveness = if *alive { "alive" } else { "DEAD" };
        println!(
            "{:<32} {:<14} {:<10} {:<10} {:<12} {:<8} {:<8} -",
            sid,
            format!("({liveness})"),
            "(fs-only)",
            "-",
            "-",
            "-",
            "-"
        );
    }
    Ok(())
}

/// List all interactive sessions.
async fn run_list() -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    let sessions = query_sessions(&project_dir, None).await?;

    if sessions.is_empty() {
        println!("No interactive sessions found.");
        return Ok(());
    }

    println!(
        "{:<32} {:<8} {:<10} {:<10} {:<10} {:<26}",
        "SESSION ID", "PID", "STATUS", "MANAGED", "SOURCE", "CREATED"
    );
    let managed_str = |m: bool| if m { "yes" } else { "no" };
    for s in &sessions {
        let created = &s.created_at;
        println!(
            "{:<32} {:<8} {:<10} {:<10} {:<10} {created}",
            s.session_id,
            s.pid,
            s.status,
            managed_str(s.managed),
            s.source_cli
        );
    }
    Ok(())
}

/// Remove stale sessions (dead PID) and sweep filesystem artifacts.
async fn run_cleanup() -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    let sessions = query_sessions(&project_dir, Some("active")).await?;

    let mut cleaned = 0u32;
    if let Ok(store) = open_store(&project_dir).await {
        for s in &sessions {
            if is_session_stale(s.pid) {
                let now = chrono::Utc::now().to_rfc3339();
                let _ = store
                    .db()
                    .query(
                        "UPDATE interactive_session SET status = 'stale', updated_at = $now \
                         WHERE session_id = $sid AND status = 'active'",
                    )
                    .bind(("now", now))
                    .bind(("sid", s.session_id.clone()))
                    .await;
                eprintln!(
                    "cleaned stale session: {} (PID {} dead)",
                    s.session_id, s.pid
                );
                cleaned += 1;
            }
        }
    }

    // Filesystem artifact sweeps (independent of DB state).
    let hb_cleaned = sweep_stale_heartbeats(&project_dir);
    let env_cleaned = sweep_dead_pid_env_files(&project_dir);
    let sess_cleaned = sweep_stale_session_dirs(&project_dir);
    let map_cleaned = clean_session_worktree_map(&project_dir);
    let wt_cleaned = mark_stale_worktree_entries(&project_dir);

    let total = cleaned + hb_cleaned + env_cleaned + sess_cleaned + map_cleaned + wt_cleaned;
    if total == 0 {
        println!("No stale sessions or artifacts found.");
    } else {
        if cleaned > 0 {
            println!("Cleaned {cleaned} stale DB session(s).");
        }
        if hb_cleaned > 0 {
            println!("Removed {hb_cleaned} stale heartbeat file(s).");
        }
        if env_cleaned > 0 {
            println!("Removed {env_cleaned} dead-PID env file(s).");
        }
        if sess_cleaned > 0 {
            println!("Removed {sess_cleaned} stale session dir(s).");
        }
        if map_cleaned > 0 {
            println!("Removed {map_cleaned} stale worktree map entry(ies).");
        }
        if wt_cleaned > 0 {
            println!("Marked {wt_cleaned} stale worktree registry entry(ies).");
        }
    }
    Ok(())
}

/// Open a `SurrealStore` for subcommand DB queries.
async fn open_store(project_dir: &Path) -> Result<codeflow_core::store::SurrealStore> {
    let db_dir = project_dir.join(".state/db");
    codeflow_core::store::SurrealStore::open(&db_dir)
        .await
        .context("opening data store")
}

/// Query InteractiveSession records from DB.
async fn query_sessions(
    project_dir: &Path,
    status_filter: Option<&str>,
) -> Result<Vec<codeflow_core::models::InteractiveSession>> {
    let store = open_store(project_dir).await?;

    let mut result = match status_filter {
        Some(status) => store
            .db()
            .query(
                "SELECT * FROM interactive_session WHERE status = $status ORDER BY created_at DESC",
            )
            .bind(("status", status.to_string()))
            .await
            .context("querying interactive sessions")?,
        None => store
            .db()
            .query("SELECT * FROM interactive_session ORDER BY created_at DESC")
            .await
            .context("querying interactive sessions")?,
    };

    let sessions: Vec<codeflow_core::models::InteractiveSession> =
        result.take(0).unwrap_or_default();
    Ok(sessions)
}

/// Build the worktree name from a session ID.
fn worktree_name(session_id: &str) -> String {
    format!("worktree-{session_id}")
}

/// Build environment variables for the claude child process.
///
/// Always includes `CODEFLOW_MANAGED`, `CODEFLOW_SESSION_ID`, `CF_PROJECT_ROOT`.
/// Adds `CODEFLOW_WORKTREE_PATH` when a worktree is active.
fn build_env_vars(
    session_id: &str,
    project_root: &str,
    worktree_path: Option<&str>,
) -> Vec<(String, String)> {
    let mut vars = vec![
        ("CODEFLOW_MANAGED".into(), "true".into()),
        ("CODEFLOW_SESSION_ID".into(), session_id.to_string()),
        ("CF_PROJECT_ROOT".into(), project_root.to_string()),
    ];
    if let Some(wt_path) = worktree_path {
        vars.push(("CODEFLOW_WORKTREE_PATH".into(), wt_path.to_string()));
    }
    vars
}

/// Determine the working directory for the claude process.
fn resolve_work_dir<'a>(worktree_path: Option<&'a str>, project_dir: &'a str) -> &'a str {
    worktree_path.unwrap_or(project_dir)
}

/// Check if a session's PID is stale (dead).
fn is_session_stale(pid: i64) -> bool {
    let pid_u32 = u32::try_from(pid).unwrap_or(0);
    !codeflow_core::session::process::is_process_alive(pid_u32)
}

/// Format a PID with liveness indicator for status display.
fn format_pid_with_liveness(pid: i64) -> String {
    let liveness = if is_session_stale(pid) {
        "DEAD"
    } else {
        "alive"
    };
    format!("{pid} ({liveness})")
}

/// Derive the current PathFlow phase from the session's pathflow-session-status.json.
///
/// Reads `last_completed_phase` from the session's state directory, falling back
/// to the session `status` field if the pathflow dir is unavailable.
fn derive_session_phase(session: &codeflow_core::models::InteractiveSession) -> String {
    // Try reading pathflow-session-status.json from the worktree or project dir.
    let wt = session.worktree_path.as_deref().unwrap_or("");
    if wt.is_empty() {
        return "-".to_string();
    }
    // Path traversal guards: both session_id and worktree_path are
    // user-adjacent data from the DB; reject directory traversal.
    if session.session_id.contains("..") || wt.contains("..") {
        return "-".to_string();
    }
    let status_path = std::path::Path::new(wt)
        .join(".state/session")
        .join(&session.session_id)
        .join("pathflow/pathflow-session-status.json");
    if let Ok(content) = std::fs::read_to_string(&status_path) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(phase) = val.get("last_completed_phase").and_then(|v| v.as_str()) {
                if !phase.is_empty() {
                    return phase.to_string();
                }
            }
        }
    }
    "-".to_string()
}

/// Exec `claude` replacing the current process.
fn exec_claude(work_dir: &str, env_vars: &[(String, String)]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new("claude");
    cmd.current_dir(work_dir);
    for (key, value) in env_vars {
        cmd.env(key, value);
    }
    cmd.exec()
}

// ─── Filesystem sweep helpers ──────────────────────────────────────────

/// Scan `.state/interactive/heartbeat-*` and return sessions found on disk
/// but NOT in `db_sids`. Each entry is `(session_id, is_alive)`.
fn scan_heartbeat_sessions(
    project_dir: &Path,
    db_sids: &std::collections::HashSet<String>,
) -> Vec<(String, bool)> {
    let hb_dir = project_dir.join(".state/interactive");
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir(&hb_dir) else {
        return result;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(sid) = name.strip_prefix("heartbeat-") else {
            continue;
        };
        if sid.contains("..") {
            continue;
        }
        if db_sids.contains(sid) {
            continue;
        }
        let alive = is_heartbeat_session_alive(project_dir, sid);
        result.push((sid.to_string(), alive));
    }
    result
}

/// Check if a session is still alive by reading its session pointer.
fn is_heartbeat_session_alive(project_dir: &Path, sid: &str) -> bool {
    if sid.contains("..") {
        return false;
    }
    let pointer_path = project_dir
        .join(".state/session")
        .join(sid)
        .join("session-pointer.json");
    let Ok(content) = std::fs::read_to_string(&pointer_path) else {
        return false;
    };
    let Ok(pointer) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let pid = pointer
        .get("lead_pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    codeflow_core::session::process::is_process_alive(pid)
}

/// Sweep stale heartbeat files whose sessions are dead.
///
/// Returns the number of files removed.
fn sweep_stale_heartbeats(project_dir: &Path) -> u32 {
    sweep_stale_heartbeats_inner(project_dir, &project_dir.join(".state/interactive"))
}

/// Inner implementation for testability.
fn sweep_stale_heartbeats_inner(project_dir: &Path, hb_dir: &Path) -> u32 {
    let mut count = 0u32;
    let Ok(entries) = std::fs::read_dir(hb_dir) else {
        return count;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(sid) = name.strip_prefix("heartbeat-") else {
            continue;
        };
        if sid.contains("..") {
            continue;
        }
        if !is_heartbeat_session_alive(project_dir, sid) {
            let _ = std::fs::remove_file(entry.path());
            eprintln!("removed stale heartbeat: {name}");
            count += 1;
        }
    }
    count
}

/// Sweep `.state/runtime/shared/codeflow-env-*.sh` files whose PIDs are dead.
///
/// Returns the number of files removed.
fn sweep_dead_pid_env_files(project_dir: &Path) -> u32 {
    sweep_dead_pid_env_files_inner(&project_dir.join(".state/runtime/shared"))
}

/// Inner implementation for testability.
fn sweep_dead_pid_env_files_inner(shared_dir: &Path) -> u32 {
    let mut count = 0u32;
    let Ok(entries) = std::fs::read_dir(shared_dir) else {
        return count;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("codeflow-env-")
            || !Path::new(&name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sh"))
        {
            continue;
        }
        // Extract PID from "codeflow-env-{PID}.sh".
        let pid_str = name
            .strip_prefix("codeflow-env-")
            .and_then(|s| s.strip_suffix(".sh"))
            .unwrap_or("");
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        if !codeflow_core::session::process::is_process_alive(pid) {
            let _ = std::fs::remove_file(entry.path());
            eprintln!("removed dead-PID env file: {name}");
            count += 1;
        }
    }
    count
}

/// Sweep stale session directories under `.state/session/ses-*`.
///
/// Applies the same logic as `sweep_all_stale_sessions` in session_start.rs:
/// - Status `pf-complete` or `created` with no team config -> remove
/// - Status `pf-in-progress`/`pf-started` with dead lead -> remove
///
/// Returns the number of directories removed.
fn sweep_stale_session_dirs(project_dir: &Path) -> u32 {
    let session_base = project_dir.join(".state/session");
    let mut count = 0u32;
    let Ok(entries) = std::fs::read_dir(&session_base) else {
        return count;
    };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("ses-") {
            continue;
        }
        if name.contains("..") {
            continue;
        }
        let status_path = session_base
            .join(&name)
            .join("pathflow/pathflow-session-status.json");
        let status: serde_json::Value = match std::fs::read_to_string(&status_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(v) => v,
            None => {
                // No status file — check session pointer liveness.
                if !is_heartbeat_session_alive(project_dir, &name) {
                    remove_session_artifacts(project_dir, &name);
                    count += 1;
                }
                continue;
            }
        };

        let session_status = status.get("status").and_then(|v| v.as_str()).unwrap_or("");
        match session_status {
            "pf-complete" => {
                remove_session_artifacts(project_dir, &name);
                count += 1;
            }
            "created" => {
                // "created" with no team config is stale.
                let team_name = status
                    .get("team_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if team_name.is_empty() {
                    remove_session_artifacts(project_dir, &name);
                    count += 1;
                }
            }
            "pf-started" | "pf-in-progress" => {
                let lead_pid = status
                    .get("lead_pid")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0);
                if lead_pid > 0 && !codeflow_core::session::process::is_process_alive(lead_pid) {
                    remove_session_artifacts(project_dir, &name);
                    count += 1;
                }
            }
            _ => {}
        }
    }
    count
}

/// Remove session directory + sentinel directory + interactive heartbeat.
fn remove_session_artifacts(project_dir: &Path, sid: &str) {
    if sid.contains("..") {
        return;
    }
    let _ = std::fs::remove_file(
        project_dir
            .join(".state/interactive")
            .join(format!("heartbeat-{sid}")),
    );
    let _ = std::fs::remove_dir_all(project_dir.join(".state/session").join(sid));
    let _ = std::fs::remove_dir_all(project_dir.join(".state/sentinels/pathflow").join(sid));
    eprintln!("removed stale session artifacts: {sid}");
}

/// Clean stale entries from `session-worktree-map.json`.
///
/// Checks both the flat layout path (`.state/runtime/session-worktree-map.json`)
/// and the migrated shared path (`.state/runtime/shared/session-worktree-map.json`).
/// Returns the total number of entries removed across both copies.
fn clean_session_worktree_map(project_dir: &Path) -> u32 {
    let flat_path = project_dir.join(".state/runtime/session-worktree-map.json");
    let shared_path = project_dir.join(".state/runtime/shared/session-worktree-map.json");
    clean_session_worktree_map_inner(&flat_path) + clean_session_worktree_map_inner(&shared_path)
}

/// Inner implementation for testability.
fn clean_session_worktree_map_inner(map_path: &Path) -> u32 {
    let Ok(data) = std::fs::read_to_string(map_path) else {
        return 0;
    };
    let Ok(mut map) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&data)
    else {
        return 0;
    };
    let before = map.len();
    map.retain(|_sid, wt_path| wt_path.as_str().is_some_and(|p| Path::new(p).exists()));
    let removed = before - map.len();
    if removed > 0 {
        let _ = std::fs::write(
            map_path,
            serde_json::to_string_pretty(&map).unwrap_or_default(),
        );
    }
    u32::try_from(removed).unwrap_or(0)
}

/// Mark stale worktree registry entries as pending_cleanup.
///
/// For active entries whose lead_pid is dead, calls `mark_pending_cleanup`.
/// Returns the number of entries marked.
fn mark_stale_worktree_entries(project_dir: &Path) -> u32 {
    let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
    let Ok(registry) = codeflow_core::worktree::read_registry(&registry_path) else {
        return 0;
    };
    let mut count = 0u32;
    for entry in &registry.worktrees {
        if entry.status != codeflow_core::worktree::WorktreeStatus::Active {
            continue;
        }
        let lead_pid = entry.lead_pid.unwrap_or(0);
        if lead_pid > 0 && !codeflow_core::session::process::is_process_alive(lead_pid) {
            if let Some(ref sid) = entry.session_id {
                let _ = codeflow_core::worktree::mark_pending_cleanup(&registry_path, sid);
                eprintln!(
                    "marked stale worktree: {} (PID {lead_pid} dead)",
                    entry.name
                );
                count += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Session ID generation ─────────────────────────────────────────

    #[test]
    fn test_session_id_has_ses_prefix() {
        let sid = codeflow_core::session::generate_session_id();
        assert!(
            sid.as_str().starts_with("ses-"),
            "session ID must start with ses-"
        );
    }

    #[test]
    fn test_session_id_is_unique() {
        let a = codeflow_core::session::generate_session_id();
        let b = codeflow_core::session::generate_session_id();
        assert_ne!(a.as_str(), b.as_str(), "two session IDs must differ");
    }

    #[test]
    fn test_session_id_ulid_length() {
        let sid = codeflow_core::session::generate_session_id();
        // ses- (4 chars) + 26-char ULID = 30 chars
        assert_eq!(sid.as_str().len(), 30, "ses-{{26-char-ULID}} = 30 chars");
    }

    // ─── Worktree name ─────────────────────────────────────────────────

    #[test]
    fn test_worktree_name_format() {
        assert_eq!(worktree_name("ses-abc123"), "worktree-ses-abc123");
    }

    #[test]
    fn test_worktree_name_includes_full_sid() {
        let sid = codeflow_core::session::generate_session_id();
        let name = worktree_name(sid.as_str());
        assert!(name.starts_with("worktree-ses-"));
        assert!(name.contains(sid.as_str()));
    }

    // ─── Env var construction ──────────────────────────────────────────

    #[test]
    fn test_build_env_vars_without_worktree() {
        let vars = build_env_vars("ses-test123", "/project", None);
        assert_eq!(vars.len(), 3);
        assert_eq!(vars[0], ("CODEFLOW_MANAGED".into(), "true".into()));
        assert_eq!(
            vars[1],
            ("CODEFLOW_SESSION_ID".into(), "ses-test123".into())
        );
        assert_eq!(vars[2], ("CF_PROJECT_ROOT".into(), "/project".into()));
    }

    #[test]
    fn test_build_env_vars_with_worktree() {
        let vars = build_env_vars("ses-test456", "/project", Some("/wt/path"));
        assert_eq!(vars.len(), 4);
        assert_eq!(
            vars[3],
            ("CODEFLOW_WORKTREE_PATH".into(), "/wt/path".into())
        );
    }

    #[test]
    fn test_build_env_vars_managed_always_true() {
        let vars = build_env_vars("ses-x", "/p", None);
        let managed = vars.iter().find(|(k, _)| k == "CODEFLOW_MANAGED");
        assert_eq!(managed.unwrap().1, "true");
    }

    #[test]
    fn test_build_env_vars_session_id_matches() {
        let sid = "ses-01abc2def3ghi4jkl5mno6pq";
        let vars = build_env_vars(sid, "/root", None);
        let found = vars.iter().find(|(k, _)| k == "CODEFLOW_SESSION_ID");
        assert_eq!(found.unwrap().1, sid);
    }

    // ─── Work dir resolution ───────────────────────────────────────────

    #[test]
    fn test_resolve_work_dir_with_worktree() {
        assert_eq!(resolve_work_dir(Some("/wt/path"), "/project"), "/wt/path");
    }

    #[test]
    fn test_resolve_work_dir_without_worktree() {
        assert_eq!(resolve_work_dir(None, "/project"), "/project");
    }

    // ─── Stale PID detection ───────────────────────────────────────────

    #[test]
    fn test_is_session_stale_dead_pid() {
        // PID 0 is never a valid user process.
        assert!(is_session_stale(0));
    }

    #[test]
    fn test_is_session_stale_current_pid() {
        let pid = i64::from(std::process::id());
        assert!(!is_session_stale(pid), "current process should be alive");
    }

    #[test]
    fn test_is_session_stale_negative_pid() {
        // Negative PID should be treated as dead (u32 conversion yields 0).
        assert!(is_session_stale(-1));
    }

    #[test]
    fn test_is_session_stale_very_large_pid() {
        // Very large PID unlikely to be alive.
        assert!(is_session_stale(999_999_999));
    }

    // ─── PID formatting ────────────────────────────────────────────────

    #[test]
    fn test_format_pid_with_liveness_dead() {
        let result = format_pid_with_liveness(0);
        assert!(result.contains("DEAD"), "PID 0 should show DEAD");
        assert!(result.contains('0'), "should include the PID number");
    }

    #[test]
    fn test_format_pid_with_liveness_alive() {
        let pid = i64::from(std::process::id());
        let result = format_pid_with_liveness(pid);
        assert!(result.contains("alive"), "current PID should show alive");
    }

    // ─── Exec function ─────────────────────────────────────────────────

    #[test]
    fn test_exec_claude_function_signature() {
        // Cannot call exec_claude because it replaces the process.
        // Verify the function has the expected signature.
        let fn_ptr: fn(&str, &[(String, String)]) -> std::io::Error = exec_claude;
        assert_eq!(std::mem::size_of_val(&fn_ptr), std::mem::size_of::<usize>());
    }

    // ─── WorktreeMode config ───────────────────────────────────────────

    #[test]
    fn test_worktree_mode_fallback_on_error() {
        // Verify the map_or pattern used in run_launch: when load_config
        // returns Err, it falls back to Disabled.
        let simulated_err: Result<codeflow_core::autorun::config::ParallelWorkConfig, _> =
            Err(codeflow_core::ConfigError::NotFound("test".into()));
        let mode = simulated_err.map_or(
            codeflow_core::autorun::config::WorktreeMode::Disabled,
            |c| c.worktree.mode,
        );
        assert_eq!(mode, codeflow_core::autorun::config::WorktreeMode::Disabled);
    }

    // ─── DB registration (integration) ─────────────────────────────────

    #[tokio::test]
    async fn test_register_interactive_session_no_db() {
        // When DB directory doesn't exist, register should silently return.
        let dir = tempfile::tempdir().unwrap();
        // No .state/db directory — should not panic.
        register_interactive_session(dir.path(), "ses-test", None, "codeflow", true).await;
    }

    #[tokio::test]
    async fn test_register_and_query_interactive_session() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();

        // Register a session.
        register_interactive_session(
            dir.path(),
            "ses-inttest1",
            Some("/tmp/wt"),
            "codeflow",
            true,
        )
        .await;

        // Query it back.
        let sessions = query_sessions(dir.path(), Some("active")).await;
        assert!(sessions.is_ok(), "query should succeed");
        let list = sessions.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].session_id, "ses-inttest1");
        assert_eq!(list[0].source_cli, "codeflow");
        assert!(list[0].managed);
        assert_eq!(list[0].worktree_path, Some("/tmp/wt".to_string()));
    }

    #[tokio::test]
    async fn test_query_sessions_empty_db() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();

        let sessions = query_sessions(dir.path(), None).await;
        assert!(sessions.is_ok());
        assert!(sessions.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_query_sessions_status_filter() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();

        register_interactive_session(dir.path(), "ses-f1", None, "claude", false).await;

        // Filter by 'active' should find it.
        let active = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert_eq!(active.len(), 1);

        // Filter by 'complete' should find nothing.
        let complete = query_sessions(dir.path(), Some("complete")).await.unwrap();
        assert!(complete.is_empty());
    }

    #[tokio::test]
    async fn test_query_sessions_no_db_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        // SurrealDB creates the directory on open, so this succeeds with empty results.
        let result = query_sessions(dir.path(), None).await;
        if let Ok(sessions) = result {
            assert!(sessions.is_empty(), "empty DB should return no sessions");
        }
        // Err is also acceptable if DB open fails on some platforms.
    }

    #[tokio::test]
    async fn test_register_unmanaged_session() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();

        register_interactive_session(dir.path(), "ses-unmanaged", None, "claude", false).await;

        let sessions = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].source_cli, "claude");
        assert!(!sessions[0].managed);
    }

    // ─── Heartbeat sweep ──────────────────────────────────────────────

    #[test]
    fn test_sweep_stale_heartbeats_removes_dead_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        // Create heartbeat for a session with no pointer (dead).
        std::fs::write(hb_dir.join("heartbeat-ses-dead1"), "2026-01-01T00:00:00Z").unwrap();

        let removed = sweep_stale_heartbeats_inner(dir.path(), &hb_dir);
        assert_eq!(removed, 1);
        assert!(!hb_dir.join("heartbeat-ses-dead1").exists());
    }

    #[test]
    fn test_sweep_stale_heartbeats_preserves_alive_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        // Create a session pointer with current process PID (alive).
        let sid = "ses-alive1";
        let pointer_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&pointer_dir).unwrap();
        let pointer = serde_json::json!({
            "lead_pid": std::process::id(),
            "worktree_path": "/tmp/test",
            "session_id": sid,
            "created_at": "2026-01-01T00:00:00Z"
        });
        std::fs::write(
            pointer_dir.join("session-pointer.json"),
            serde_json::to_string(&pointer).unwrap(),
        )
        .unwrap();

        std::fs::write(
            hb_dir.join(format!("heartbeat-{sid}")),
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        let removed = sweep_stale_heartbeats_inner(dir.path(), &hb_dir);
        assert_eq!(removed, 0);
        assert!(hb_dir.join(format!("heartbeat-{sid}")).exists());
    }

    #[test]
    fn test_sweep_stale_heartbeats_ignores_non_heartbeat_files() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        std::fs::write(hb_dir.join("other-file.txt"), "data").unwrap();

        let removed = sweep_stale_heartbeats_inner(dir.path(), &hb_dir);
        assert_eq!(removed, 0);
        assert!(hb_dir.join("other-file.txt").exists());
    }

    #[test]
    fn test_sweep_stale_heartbeats_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        let removed = sweep_stale_heartbeats_inner(dir.path(), &hb_dir);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_sweep_stale_heartbeats_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        // Directory does not exist.
        let removed = sweep_stale_heartbeats_inner(dir.path(), &hb_dir);
        assert_eq!(removed, 0);
    }

    // ─── Dead-PID env file sweep ──────────────────────────────────────

    #[test]
    fn test_sweep_dead_pid_env_files_removes_dead() {
        let dir = tempfile::tempdir().unwrap();
        let shared_dir = dir.path().join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();

        // PID 4000000 is certainly dead.
        std::fs::write(
            shared_dir.join("codeflow-env-4000000.sh"),
            "export CODEFLOW_SESSION_ID=test",
        )
        .unwrap();

        let removed = sweep_dead_pid_env_files_inner(&shared_dir);
        assert_eq!(removed, 1);
        assert!(!shared_dir.join("codeflow-env-4000000.sh").exists());
    }

    #[test]
    fn test_sweep_dead_pid_env_files_preserves_alive() {
        let dir = tempfile::tempdir().unwrap();
        let shared_dir = dir.path().join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();

        // Current process PID is alive.
        let filename = format!("codeflow-env-{}.sh", std::process::id());
        std::fs::write(shared_dir.join(&filename), "export X=1").unwrap();

        let removed = sweep_dead_pid_env_files_inner(&shared_dir);
        assert_eq!(removed, 0);
        assert!(shared_dir.join(&filename).exists());
    }

    #[test]
    fn test_sweep_dead_pid_env_files_ignores_non_matching() {
        let dir = tempfile::tempdir().unwrap();
        let shared_dir = dir.path().join("shared");
        std::fs::create_dir_all(&shared_dir).unwrap();

        // Files that don't match the pattern.
        std::fs::write(shared_dir.join("other-file.sh"), "data").unwrap();
        std::fs::write(shared_dir.join("codeflow-env-notapid.sh"), "data").unwrap();

        let removed = sweep_dead_pid_env_files_inner(&shared_dir);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_sweep_dead_pid_env_files_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let shared_dir = dir.path().join("nonexistent");
        let removed = sweep_dead_pid_env_files_inner(&shared_dir);
        assert_eq!(removed, 0);
    }

    // ─── Session-worktree map cleanup ─────────────────────────────────

    #[test]
    fn test_clean_session_worktree_map_removes_stale() {
        let dir = tempfile::tempdir().unwrap();
        let map_path = dir.path().join("map.json");

        // Create a map with one valid and one stale entry.
        let existing_dir = dir.path().join("existing-wt");
        std::fs::create_dir_all(&existing_dir).unwrap();

        let map = serde_json::json!({
            "ses-valid": existing_dir.to_string_lossy().to_string(),
            "ses-stale": "/tmp/nonexistent-worktree-path-12345"
        });
        std::fs::write(&map_path, serde_json::to_string_pretty(&map).unwrap()).unwrap();

        let removed = clean_session_worktree_map_inner(&map_path);
        assert_eq!(removed, 1);

        // Verify the map was rewritten with only the valid entry.
        let content = std::fs::read_to_string(&map_path).unwrap();
        let updated: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&content).unwrap();
        assert_eq!(updated.len(), 1);
        assert!(updated.contains_key("ses-valid"));
    }

    #[test]
    fn test_clean_session_worktree_map_no_changes() {
        let dir = tempfile::tempdir().unwrap();
        let map_path = dir.path().join("map.json");

        let existing_dir = dir.path().join("wt");
        std::fs::create_dir_all(&existing_dir).unwrap();

        let map = serde_json::json!({
            "ses-ok": existing_dir.to_string_lossy().to_string()
        });
        std::fs::write(&map_path, serde_json::to_string_pretty(&map).unwrap()).unwrap();

        let removed = clean_session_worktree_map_inner(&map_path);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_clean_session_worktree_map_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let map_path = dir.path().join("nonexistent.json");
        let removed = clean_session_worktree_map_inner(&map_path);
        assert_eq!(removed, 0);
    }

    #[test]
    fn test_clean_session_worktree_map_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let map_path = dir.path().join("bad.json");
        std::fs::write(&map_path, "not valid json").unwrap();
        let removed = clean_session_worktree_map_inner(&map_path);
        assert_eq!(removed, 0);
    }

    // ─── Scan heartbeat sessions ──────────────────────────────────────

    #[test]
    fn test_scan_heartbeat_sessions_excludes_db_known() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        std::fs::write(hb_dir.join("heartbeat-ses-known"), "ts").unwrap();
        std::fs::write(hb_dir.join("heartbeat-ses-unknown"), "ts").unwrap();

        let mut db_sids = std::collections::HashSet::new();
        db_sids.insert("ses-known".to_string());

        let fs_only = scan_heartbeat_sessions(dir.path(), &db_sids);
        assert_eq!(fs_only.len(), 1);
        assert_eq!(fs_only[0].0, "ses-unknown");
    }

    #[test]
    fn test_scan_heartbeat_sessions_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db_sids = std::collections::HashSet::new();
        let fs_only = scan_heartbeat_sessions(dir.path(), &db_sids);
        assert!(fs_only.is_empty());
    }

    // ─── is_heartbeat_session_alive ───────────────────────────────────

    #[test]
    fn test_is_heartbeat_session_alive_no_pointer() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_heartbeat_session_alive(dir.path(), "ses-nope"));
    }

    #[test]
    fn test_is_heartbeat_session_alive_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-deadpid";
        let pointer_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&pointer_dir).unwrap();
        let pointer = serde_json::json!({
            "lead_pid": 4_000_000,
            "worktree_path": "/tmp/test",
            "session_id": sid,
            "created_at": "2026-01-01T00:00:00Z"
        });
        std::fs::write(
            pointer_dir.join("session-pointer.json"),
            serde_json::to_string(&pointer).unwrap(),
        )
        .unwrap();
        assert!(!is_heartbeat_session_alive(dir.path(), sid));
    }

    #[test]
    fn test_is_heartbeat_session_alive_current_pid() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-alivepid";
        let pointer_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&pointer_dir).unwrap();
        let pointer = serde_json::json!({
            "lead_pid": std::process::id(),
            "worktree_path": "/tmp/test",
            "session_id": sid,
            "created_at": "2026-01-01T00:00:00Z"
        });
        std::fs::write(
            pointer_dir.join("session-pointer.json"),
            serde_json::to_string(&pointer).unwrap(),
        )
        .unwrap();
        assert!(is_heartbeat_session_alive(dir.path(), sid));
    }

    // ─── remove_session_artifacts ─────────────────────────────────────

    #[test]
    fn test_remove_session_artifacts_cleans_all() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-cleanup1";

        // Create artifacts.
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();
        std::fs::write(hb_dir.join(format!("heartbeat-{sid}")), "ts").unwrap();

        let sess_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&sess_dir).unwrap();

        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();

        remove_session_artifacts(dir.path(), sid);

        assert!(!hb_dir.join(format!("heartbeat-{sid}")).exists());
        assert!(!sess_dir.exists());
        assert!(!sentinel_dir.exists());
    }

    #[test]
    fn test_remove_session_artifacts_missing_dirs_ok() {
        let dir = tempfile::tempdir().unwrap();
        // Should not panic when nothing exists.
        remove_session_artifacts(dir.path(), "ses-nonexistent");
    }

    // ─── Worktree registry source field ──────────────────────────────

    #[test]
    fn test_interactive_worktree_registry_sets_source() {
        // Verify that locked_update_source("interactive") correctly sets the
        // source field on a worktree registry entry (mirrors the call added
        // to run_launch after locked_update_lead_pid).
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");

        // Create a registry with one entry (no source field).
        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-01-01T00:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "worktree-ses-srctest".to_string(),
            path: "/tmp/wt-srctest".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some("ses-srctest".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        // Verify source is initially None.
        let before = codeflow_core::worktree::read_registry(&registry_path).unwrap();
        assert!(
            before.worktrees[0].source.is_none(),
            "source should be None initially"
        );

        // Call locked_update_source (same call as in run_launch).
        let result = codeflow_core::worktree::locked_update_source(
            &registry_path,
            "worktree-ses-srctest",
            "interactive",
        );
        assert!(result.is_ok(), "locked_update_source should succeed");

        // Verify source is now "interactive".
        let after = codeflow_core::worktree::read_registry(&registry_path).unwrap();
        assert_eq!(
            after.worktrees[0].source.as_deref(),
            Some("interactive"),
            "source field should be set to 'interactive'"
        );
    }

    #[test]
    fn test_interactive_worktree_registry_source_not_overwritten_by_pid() {
        // Verify that locked_update_lead_pid does NOT affect the source field.
        // This validates that both calls are needed independently.
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");

        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-01-01T00:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "worktree-ses-indep".to_string(),
            path: "/tmp/wt-indep".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some("ses-indep".to_string()),
            task_id: None,
            source: None,
            lead_pid: None,
        });
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        // Update lead_pid only.
        let _ = codeflow_core::worktree::locked_update_lead_pid(
            &registry_path,
            "worktree-ses-indep",
            1234,
        );

        // Source should still be None after pid update.
        let after_pid = codeflow_core::worktree::read_registry(&registry_path).unwrap();
        assert!(
            after_pid.worktrees[0].source.is_none(),
            "source should remain None after locked_update_lead_pid"
        );

        // Now set source.
        let _ = codeflow_core::worktree::locked_update_source(
            &registry_path,
            "worktree-ses-indep",
            "interactive",
        );

        let after_both = codeflow_core::worktree::read_registry(&registry_path).unwrap();
        assert_eq!(
            after_both.worktrees[0].source.as_deref(),
            Some("interactive"),
            "source should be set after locked_update_source"
        );
        assert_eq!(
            after_both.worktrees[0].lead_pid,
            Some(1234),
            "lead_pid should still be set"
        );
    }

    // ─── sweep_stale_session_dirs ─────────────────────────────────────

    /// Helper: create a session dir with a pathflow-session-status.json.
    fn create_session_status(project_dir: &Path, sid: &str, status: &serde_json::Value) {
        let pathflow_dir = project_dir
            .join(".state/session")
            .join(sid)
            .join("pathflow");
        std::fs::create_dir_all(&pathflow_dir).unwrap();
        std::fs::write(
            pathflow_dir.join("pathflow-session-status.json"),
            serde_json::to_string(status).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn test_sweep_stale_session_dirs_pf_complete() {
        let dir = tempfile::tempdir().unwrap();
        create_session_status(
            dir.path(),
            "ses-complete1",
            &serde_json::json!({
                "status": "pf-complete",
                "session_id": "ses-complete1",
            }),
        );

        let removed = sweep_stale_session_dirs(dir.path());
        assert_eq!(removed, 1, "pf-complete session should be removed");
        assert!(
            !dir.path().join(".state/session/ses-complete1").exists(),
            "session dir should be deleted"
        );
    }

    #[test]
    fn test_sweep_stale_session_dirs_created_no_team() {
        let dir = tempfile::tempdir().unwrap();
        create_session_status(
            dir.path(),
            "ses-created1",
            &serde_json::json!({
                "status": "created",
                "session_id": "ses-created1",
                "team_name": "",
            }),
        );

        let removed = sweep_stale_session_dirs(dir.path());
        assert_eq!(
            removed, 1,
            "'created' with empty team_name should be removed"
        );
    }

    #[test]
    fn test_sweep_stale_session_dirs_in_progress_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        create_session_status(
            dir.path(),
            "ses-deadpid1",
            &serde_json::json!({
                "status": "pf-in-progress",
                "session_id": "ses-deadpid1",
                "lead_pid": 4_000_000,
            }),
        );

        let removed = sweep_stale_session_dirs(dir.path());
        assert_eq!(removed, 1, "pf-in-progress with dead PID should be removed");
    }

    #[test]
    fn test_sweep_stale_session_dirs_in_progress_alive_pid() {
        let dir = tempfile::tempdir().unwrap();
        let current_pid = std::process::id();
        create_session_status(
            dir.path(),
            "ses-alive1",
            &serde_json::json!({
                "status": "pf-in-progress",
                "session_id": "ses-alive1",
                "lead_pid": current_pid,
            }),
        );

        let removed = sweep_stale_session_dirs(dir.path());
        assert_eq!(
            removed, 0,
            "pf-in-progress with alive PID should be preserved"
        );
        assert!(
            dir.path().join(".state/session/ses-alive1").exists(),
            "session dir should still exist"
        );
    }

    #[test]
    fn test_sweep_stale_session_dirs_no_status_dead_pointer() {
        let dir = tempfile::tempdir().unwrap();
        // Create session dir with NO status file, and NO alive pointer.
        let sess_dir = dir.path().join(".state/session/ses-nostatus1");
        std::fs::create_dir_all(&sess_dir).unwrap();

        let removed = sweep_stale_session_dirs(dir.path());
        assert_eq!(
            removed, 1,
            "session with no status file and no alive pointer should be removed"
        );
        assert!(!sess_dir.exists(), "session dir should be deleted");
    }

    // ─── mark_stale_worktree_entries ──────────────────────────────────

    #[test]
    fn test_mark_stale_worktree_entries_dead_pid() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");

        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-01-01T00:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "worktree-ses-stale1".to_string(),
            path: "/tmp/wt-stale1".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some("ses-stale1".to_string()),
            task_id: None,
            source: Some("interactive".to_string()),
            lead_pid: Some(4_000_000), // Dead PID.
        });
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let count = mark_stale_worktree_entries(dir.path());
        assert_eq!(count, 1, "dead PID entry should be marked");

        // Verify it's now pending_cleanup.
        let updated = codeflow_core::worktree::read_registry(&registry_path).unwrap();
        assert_eq!(
            updated.worktrees[0].status,
            codeflow_core::worktree::WorktreeStatus::PendingCleanup,
            "entry should be PendingCleanup after mark_stale"
        );
    }

    #[test]
    fn test_mark_stale_worktree_entries_alive_pid() {
        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");

        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-01-01T00:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "worktree-ses-alive1".to_string(),
            path: "/tmp/wt-alive1".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some("ses-alive1".to_string()),
            task_id: None,
            source: Some("interactive".to_string()),
            lead_pid: Some(std::process::id()), // Alive PID.
        });
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let count = mark_stale_worktree_entries(dir.path());
        assert_eq!(count, 0, "alive PID entry should be preserved");

        // Verify it's still Active.
        let updated = codeflow_core::worktree::read_registry(&registry_path).unwrap();
        assert_eq!(
            updated.worktrees[0].status,
            codeflow_core::worktree::WorktreeStatus::Active,
            "entry should remain Active"
        );
    }

    // ─── Path traversal guards ───────────────────────────────────────

    #[test]
    fn test_is_heartbeat_session_alive_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            !is_heartbeat_session_alive(dir.path(), "../etc"),
            "path traversal SID should return false"
        );
    }

    #[test]
    fn test_remove_session_artifacts_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        // Create a file that would be hit by traversal.
        let target = dir.path().join("canary.txt");
        std::fs::write(&target, "should survive").unwrap();

        remove_session_artifacts(dir.path(), "../evil");

        assert!(
            target.exists(),
            "path traversal should be a no-op, canary file must survive"
        );
    }

    #[test]
    fn test_scan_heartbeat_sessions_skips_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        // Create a heartbeat whose SID contains ".." (triggers the guard).
        // Use "..evil" not "../evil" since "/" is a path separator the OS won't allow.
        std::fs::write(hb_dir.join("heartbeat-..evil"), "ts").unwrap();

        let db_sids = std::collections::HashSet::new();
        let result = scan_heartbeat_sessions(dir.path(), &db_sids);
        assert!(
            result.is_empty(),
            "path traversal heartbeat should be skipped"
        );
    }

    #[test]
    fn test_sweep_stale_heartbeats_skips_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let hb_dir = dir.path().join(".state/interactive");
        std::fs::create_dir_all(&hb_dir).unwrap();

        // Create a heartbeat whose SID contains ".." (triggers the guard).
        std::fs::write(hb_dir.join("heartbeat-ses..evil"), "ts").unwrap();
        // Also create a normal stale heartbeat for comparison.
        std::fs::write(hb_dir.join("heartbeat-ses-normalstale"), "ts").unwrap();

        let removed = sweep_stale_heartbeats_inner(dir.path(), &hb_dir);
        // Only the normal stale one should be removed (no alive pointer).
        assert_eq!(
            removed, 1,
            "only non-traversal stale heartbeat should be removed"
        );
        // The traversal one should still exist (skipped, not removed).
        assert!(
            hb_dir.join("heartbeat-ses..evil").exists(),
            "traversal heartbeat should be skipped, not removed"
        );
    }

    // -----------------------------------------------------------------------
    // derive_session_phase tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_derive_session_phase_no_worktree() {
        let session = codeflow_core::models::InteractiveSession {
            id: "test:1".into(),
            session_id: "ses-test".into(),
            pid: 1234,
            status: codeflow_core::types::InteractiveSessionStatus::Active,
            worktree_path: None,
            branch: None,
            work_type: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "-");
    }

    #[test]
    fn test_derive_session_phase_with_status_file() {
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().to_string_lossy().to_string();
        let status_dir = dir.path().join(".state/session/ses-phase-test/pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            r#"{"last_completed_phase": "pf-3", "status": "pf-in-progress"}"#,
        )
        .unwrap();

        let session = codeflow_core::models::InteractiveSession {
            id: "test:2".into(),
            session_id: "ses-phase-test".into(),
            pid: 1234,
            status: codeflow_core::types::InteractiveSessionStatus::Active,
            worktree_path: Some(wt_path),
            branch: Some("fix/test".into()),
            work_type: Some("FIX".into()),
            team_name: Some("team-1".into()),
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "pf-3");
    }

    #[test]
    fn test_derive_session_phase_rejects_traversal() {
        let session = codeflow_core::models::InteractiveSession {
            id: "test:3".into(),
            session_id: "ses-../../../etc/passwd".into(),
            pid: 1234,
            status: codeflow_core::types::InteractiveSessionStatus::Active,
            worktree_path: Some("/tmp/wt".into()),
            branch: None,
            work_type: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "-");
    }

    #[test]
    fn test_derive_session_phase_rejects_worktree_traversal() {
        let session = codeflow_core::models::InteractiveSession {
            id: "test:4".into(),
            session_id: "ses-clean-id".into(),
            pid: 1234,
            status: codeflow_core::types::InteractiveSessionStatus::Active,
            worktree_path: Some("/tmp/../../../etc".into()),
            branch: None,
            work_type: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "-");
    }

    #[test]
    fn test_derive_session_phase_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().to_string_lossy().to_string();
        let status_dir = dir.path().join(".state/session/ses-bad-json/pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            "not valid json {{{",
        )
        .unwrap();

        let session = codeflow_core::models::InteractiveSession {
            id: "test:5".into(),
            session_id: "ses-bad-json".into(),
            pid: 1234,
            status: codeflow_core::types::InteractiveSessionStatus::Active,
            worktree_path: Some(wt_path),
            branch: None,
            work_type: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "-");
    }

    #[test]
    fn test_derive_session_phase_missing_field() {
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().to_string_lossy().to_string();
        let status_dir = dir.path().join(".state/session/ses-no-phase/pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            r#"{"status": "pf-in-progress", "team_name": "my-team"}"#,
        )
        .unwrap();

        let session = codeflow_core::models::InteractiveSession {
            id: "test:6".into(),
            session_id: "ses-no-phase".into(),
            pid: 1234,
            status: codeflow_core::types::InteractiveSessionStatus::Active,
            worktree_path: Some(wt_path),
            branch: None,
            work_type: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "-");
    }
}
