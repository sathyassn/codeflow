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
    /// Show interactive sessions (TUI when TTY, text when piped)
    #[clap(alias = "list")]
    Status {
        /// Watch mode: interactive TUI dashboard (default when TTY)
        #[arg(long, short = 'w')]
        watch: bool,
        /// Force text output even in TTY (no TUI)
        #[arg(long)]
        once: bool,
        /// Refresh interval in seconds (default: 2)
        #[arg(long, default_value = "2")]
        interval: u64,
        /// Show all sessions regardless of age (default: last 7 days only)
        #[arg(long)]
        all: bool,
        /// Filter by status: active, stale, complete, or all (default: all statuses)
        #[arg(long)]
        status: Option<String>,
    },
    /// Remove stale sessions (dead PID detection)
    Cleanup {
        /// Skip DB purge of old terminal sessions
        #[arg(long)]
        no_purge: bool,
    },
}

/// Run the interactive subcommand or launch a new session.
pub async fn run(command: Option<InteractiveCommand>) -> Result<()> {
    use std::io::IsTerminal;

    match command {
        None => run_launch().await,
        Some(InteractiveCommand::Status {
            watch: _,
            once,
            interval,
            all,
            status,
        }) => {
            // Default to TUI when stdout is a TTY; --once forces text output.
            if once || !std::io::stdout().is_terminal() {
                run_status(all, status.as_deref()).await
            } else {
                let project_dir = helpers::detect_project_root()?;
                run_status_tui(&project_dir, interval).await
            }
        }
        Some(InteractiveCommand::Cleanup { no_purge }) => run_cleanup(no_purge).await,
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

    // Determine tmux availability and whether to use it.
    let use_tmux = is_tmux_available();
    let tmux_name = if use_tmux {
        Some(format!("codeflow-{sid_str}"))
    } else {
        eprintln!("codeflow: tmux not available, using direct exec (no reattach support)");
        None
    };

    // Register InteractiveSession in DB (best-effort).
    register_interactive_session(
        &project_dir,
        &sid_str,
        worktree_path.as_deref(),
        tmux_name.as_deref(),
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

    let work_dir = resolve_work_dir(worktree_path.as_deref(), &project_dir_str);

    if let Some(ref tmux_session) = tmux_name {
        // Launch claude inside a tmux session, then attach.
        launch_in_tmux(tmux_session, work_dir, &env_vars)?;
    }

    // Fallback: direct exec (replaces the current process).
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
    tmux_session: Option<&str>,
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
    let tmux = tmux_session.map(String::from);
    let _ = store
        .db()
        .query(
            "CREATE interactive_session SET \
             session_id = $session_id, \
             pid = $pid, \
             status = 'active', \
             worktree_path = $worktree_path, \
             tmux_session = $tmux_session, \
             branch = NONE, \
             work_type = NONE, \
             task_id = NONE, \
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
        .bind(("tmux_session", tmux))
        .bind(("source_cli", source_cli.to_string()))
        .bind(("managed", managed))
        .bind(("created_at", now))
        .await;
}

/// Show interactive sessions (text mode).
///
/// By default, shows only active sessions. Pass `show_all=true` (via `--all`)
/// to show all sessions. Optionally filter by `--status <active|stale|complete>`.
///
/// Also scans `.state/interactive/heartbeat-*` as a filesystem fallback so
/// sessions are visible even when the DB is unavailable.
async fn run_status(show_all: bool, status_filter: Option<&str>) -> Result<()> {
    let project_dir = helpers::detect_project_root()?;

    // Determine the DB-level status filter.
    // --all => no filter (show everything).
    // --status <val> => filter to that status.
    // Neither => active-only default.
    let db_status_filter = if show_all {
        None
    } else {
        match status_filter {
            Some("all") => None,
            Some(s) => Some(s),
            None => Some("active"),
        }
    };

    let sessions = query_sessions(&project_dir, db_status_filter).await?;

    let db_sids: std::collections::HashSet<String> =
        sessions.iter().map(|s| s.session_id.clone()).collect();

    // Filesystem fallback: scan heartbeat files for sessions not in DB.
    let fs_only = scan_heartbeat_sessions(&project_dir, &db_sids);

    if sessions.is_empty() && fs_only.is_empty() {
        if show_all {
            println!("No interactive sessions found.");
        } else {
            println!("No active interactive sessions found. Use --all to show all sessions.");
        }
        return Ok(());
    }

    println!(
        "{:<32} {:<14} {:<10} {:<10} {:<12} {:<8} {:<8} {:<30}",
        "SESSION ID", "PID", "STATUS", "SOURCE", "BRANCH", "TYPE", "PHASE", "WORKTREE"
    );
    // Limit text output to 50 most recent.
    for s in sessions.iter().take(50) {
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

/// List all interactive sessions (alias for `status --once`).
#[allow(dead_code)]
async fn run_list() -> Result<()> {
    run_status(true, None).await
}

/// Remove stale sessions (dead PID) and sweep filesystem artifacts.
///
/// When `no_purge` is false (the default), also purges terminal sessions
/// from the DB using retention config (days + keep_last from parallel-work-config).
async fn run_cleanup(no_purge: bool) -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    let sessions = query_sessions(&project_dir, Some("active")).await?;

    let mut cleaned = 0u32;
    let store_result = open_store(&project_dir).await;
    if let Ok(ref store) = store_result {
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

    // DB purge: remove old terminal sessions unless --no-purge.
    if !no_purge {
        if let Ok(ref store) = store_result {
            use codeflow_core::store::DataStore;
            let retention = codeflow_core::autorun::load_config(&project_dir)
                .map(|c| c.retention)
                .unwrap_or_default();
            if retention.purge_on_cleanup {
                let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(retention.days));
                let cutoff_str = cutoff.to_rfc3339();
                match store
                    .prune_interactive_sessions(&cutoff_str, retention.keep_last)
                    .await
                {
                    Ok(result) if result.sessions_deleted > 0 => {
                        println!(
                            "Purged {} old terminal session(s) from DB (>{} days, kept last {}).",
                            result.sessions_deleted, retention.days, retention.keep_last,
                        );
                    }
                    Err(e) => {
                        eprintln!("warning: DB purge failed: {e}");
                    }
                    _ => {}
                }
            }
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

/// Check if tmux is available on the system.
fn is_tmux_available() -> bool {
    std::process::Command::new("which")
        .arg("tmux")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Launch claude inside a tmux session, then attach.
///
/// Creates a new tmux session running claude with the given env vars,
/// then replaces the current process with `tmux attach-session`.
///
/// Environment variables are passed via tmux `-e KEY=VALUE` flags rather
/// than shell interpolation, eliminating shell injection vectors.
fn launch_in_tmux(tmux_name: &str, work_dir: &str, env_vars: &[(String, String)]) -> Result<()> {
    // Build tmux args with -e flags for each env var (no shell interpolation).
    let env_pairs: Vec<String> = env_vars.iter().map(|(k, v)| format!("{k}={v}")).collect();

    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["new-session", "-d", "-s", tmux_name, "-c", work_dir]);
    for pair in &env_pairs {
        cmd.args(["-e", pair]);
    }
    cmd.args(["claude"]);

    let status = cmd.status().context("failed to create tmux session")?;

    if !status.success() {
        anyhow::bail!("tmux new-session failed with {status}");
    }

    eprintln!("codeflow: tmux session '{tmux_name}' created");
    eprintln!("codeflow: detach with Ctrl+B D, reattach with: codeflow interactive status");

    // Attach replaces the current process (exec).
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("tmux")
        .args(["attach-session", "-t", tmux_name])
        .exec();

    anyhow::bail!("tmux attach failed: {err}");
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

// ---------------------------------------------------------------------------
// TUI dashboard (--watch mode)
// ---------------------------------------------------------------------------

/// Interactive TUI dashboard for session monitoring.
///
/// Displays a live-updating table of interactive sessions with status badges,
/// phase indicators, and a detail pane. Supports keyboard navigation and
/// session management actions.
async fn run_status_tui(project_dir: &Path, interval_secs: u64) -> Result<()> {
    use std::time::Duration;

    use codeflow_core::tui::data::fetch_session_views_with_keep_last;
    use codeflow_core::tui::theme;
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use ratatui::layout::{Constraint, Layout};
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Paragraph, TableState};

    // Load retention config for keep_last threshold.
    let keep_last = codeflow_core::autorun::load_config(project_dir)
        .map(|c| c.retention.keep_last)
        .unwrap_or(10);

    // Terminal setup.
    let mut terminal = ratatui::init();

    // Cleanup guard: restore terminal on any exit path.
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            ratatui::restore();
        }
    }
    let _guard = TermGuard;

    let mut table_state = TableState::default();
    let mut status_message: Option<(String, std::time::Instant)> = None;
    let mut show_all = false;

    // Fetch initial data (open store fresh each time for cross-process visibility).
    let mut last_data = match open_store(project_dir).await {
        Ok(store) => {
            match fetch_session_views_with_keep_last(&store, project_dir, keep_last).await {
                Ok(d) => Some(d),
                Err(e) => {
                    eprintln!("warn: initial fetch failed: {e}");
                    None
                }
            }
        }
        Err(e) => {
            eprintln!("warn: initial store open failed: {e}");
            None
        }
    };

    loop {
        // Compute filtered view count for navigation bounds.
        let visible_count = last_data.as_ref().map_or(0, |(views, _)| {
            if show_all {
                views.len()
            } else {
                views.iter().filter(|v| !v.hidden).count()
            }
        });

        // Preserve selection index across refreshes.
        if visible_count > 0 && table_state.selected().is_none() {
            table_state.select(Some(0));
        }
        if let Some(sel) = table_state.selected() {
            if sel >= visible_count {
                table_state.select(Some(visible_count.saturating_sub(1)));
            }
        }

        // Draw UI.
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Length(2), // header
                Constraint::Min(5),    // table
                Constraint::Length(6), // detail
                Constraint::Length(1), // keybinding bar
            ])
            .split(area);

            // --- Header ---
            render_session_header(
                frame,
                chunks[0],
                last_data.as_ref().map(|(_, s)| s),
                show_all,
            );

            // --- Session table ---
            render_session_table(
                frame,
                chunks[1],
                &mut table_state,
                last_data.as_ref(),
                show_all,
            );

            // --- Detail pane ---
            let selected_session = last_data.as_ref().and_then(|(views, _)| {
                table_state
                    .selected()
                    .and_then(|i| views.iter().filter(|v| show_all || !v.hidden).nth(i))
            });
            render_session_detail(frame, chunks[2], selected_session);

            // --- Keybinding bar ---
            let bar_line = if let Some((ref msg, at)) = status_message {
                if at.elapsed() < Duration::from_secs(3) {
                    Line::from(Span::styled(
                        format!(" {msg}"),
                        Style::new().fg(theme::YELLOW_RUNNING),
                    ))
                } else {
                    status_message = None;
                    session_keybinding_line()
                }
            } else {
                session_keybinding_line()
            };
            frame.render_widget(Paragraph::new(bar_line), chunks[3]);
        })?;

        // Event handling.
        let tick = Duration::from_secs(interval_secs.max(1));
        if event::poll(tick)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // Ctrl+C: in raw mode SIGINT is not delivered; handle explicitly.
                if key.code == KeyCode::Char('c')
                    && key
                        .modifiers
                        .contains(ratatui::crossterm::event::KeyModifiers::CONTROL)
                {
                    break;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Up => {
                        if visible_count > 0 {
                            let i = table_state.selected().unwrap_or(0);
                            let prev = if i == 0 {
                                visible_count.saturating_sub(1)
                            } else {
                                i - 1
                            };
                            table_state.select(Some(prev));
                        }
                    }
                    KeyCode::Down => {
                        if visible_count > 0 {
                            let i = table_state.selected().unwrap_or(0);
                            let next = (i + 1) % visible_count;
                            table_state.select(Some(next));
                        }
                    }
                    KeyCode::Enter => {
                        // Attach to session's tmux (if in a worktree session).
                        if let Some(session) =
                            get_selected_session(last_data.as_ref(), &table_state, show_all)
                        {
                            // Validate DB-sourced session_id before using as tmux argument.
                            if codeflow_core::session::is_valid_session_id(&session.session_id) {
                                let tmux_name = format!("codeflow-{}", &session.session_id);
                                if tmux_has_session(&tmux_name) {
                                    ratatui::restore();
                                    match std::process::Command::new("tmux")
                                        .args(["attach-session", "-t", &tmux_name])
                                        .status()
                                    {
                                        Ok(s) if !s.success() => {
                                            eprintln!(
                                                "tmux attach failed (session may have ended)"
                                            );
                                        }
                                        Err(e) => eprintln!("tmux attach error: {e}"),
                                        _ => {}
                                    }
                                    terminal = ratatui::init();
                                } else {
                                    status_message = Some((
                                        format!("No tmux session for {}", session.session_id),
                                        std::time::Instant::now(),
                                    ));
                                }
                            } else {
                                status_message = Some((
                                    "Invalid session ID format".to_string(),
                                    std::time::Instant::now(),
                                ));
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        // Cleanup selected stale session.
                        if let Some(session) =
                            get_selected_session(last_data.as_ref(), &table_state, show_all)
                        {
                            // Validate DB-sourced session_id before filesystem operations.
                            if codeflow_core::session::is_valid_session_id(&session.session_id) {
                                if session.status == "stale" || is_session_stale(session.pid) {
                                    let sid = session.session_id.clone();
                                    // Mark stale in DB (fresh connection for cross-process visibility).
                                    if let Ok(cleanup_store) = open_store(project_dir).await {
                                        let now = chrono::Utc::now().to_rfc3339();
                                        let _ = cleanup_store
                                            .db()
                                            .query(
                                                "UPDATE interactive_session SET status = 'stale', updated_at = $now \
                                                 WHERE session_id = $sid AND status = 'active'",
                                            )
                                            .bind(("now", now))
                                            .bind(("sid", sid.clone()))
                                            .await;
                                    }
                                    remove_session_artifacts(project_dir, &sid);
                                    status_message = Some((
                                        format!("Cleaned up {sid}"),
                                        std::time::Instant::now(),
                                    ));
                                } else {
                                    status_message = Some((
                                        "Session is not stale".to_string(),
                                        std::time::Instant::now(),
                                    ));
                                }
                            } else {
                                status_message = Some((
                                    "Invalid session ID format".to_string(),
                                    std::time::Instant::now(),
                                ));
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        show_all = !show_all;
                        table_state.select(Some(0));
                        let label = if show_all { "all" } else { "filtered" };
                        status_message = Some((
                            format!("Showing {label} sessions"),
                            std::time::Instant::now(),
                        ));
                    }
                    _ => {}
                }
            }
        }

        // Refresh data every tick (re-open store for cross-process visibility).
        let refresh_store = match open_store(project_dir).await {
            Ok(s) => s,
            Err(_) => continue, // keep last_data on error
        };
        match fetch_session_views_with_keep_last(&refresh_store, project_dir, keep_last).await {
            Ok(d) => last_data = Some(d),
            Err(e) => eprintln!("warn: fetch failed: {e}"),
        }
    }

    Ok(())
}

fn session_keybinding_line() -> ratatui::text::Line<'static> {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::Span;

    ratatui::text::Line::from(vec![
        Span::styled(" [Enter]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Attach "),
        Span::styled("[c]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Cleanup "),
        Span::styled("[a]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Toggle All "),
        Span::styled("[Up/Down]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Navigate "),
        Span::styled("[q]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Quit"),
    ])
}

fn render_session_header(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    summary: Option<&codeflow_core::tui::data::SessionSummary>,
    show_all: bool,
) {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    let line = if let Some(s) = summary {
        let mut spans = vec![
            Span::styled(
                format!(" {} ", theme::TRIANGLE),
                Style::new().fg(theme::BLUE_ACCENT),
            ),
            Span::styled("Interactive Sessions", theme::header()),
            Span::raw("  "),
            Span::styled(
                format!("{} Active", s.active),
                Style::new().fg(theme::GREEN_SUCCESS),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} Stale", s.stale),
                if s.stale > 0 {
                    Style::new().fg(theme::RED_FAILURE)
                } else {
                    Style::new().fg(theme::DIM_PENDING)
                },
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} Complete", s.complete),
                Style::new().fg(theme::DIM_PENDING),
            ),
        ];
        // Show hidden count hint when in filtered view.
        if !show_all && s.hidden_count > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                format!("({} older hidden -- 'a' for all)", s.hidden_count),
                Style::new().fg(theme::DIM_PENDING),
            ));
        } else if show_all && s.hidden_count > 0 {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(
                "('a' to filter)".to_string(),
                Style::new().fg(theme::DIM_PENDING),
            ));
        }
        Line::from(spans)
    } else {
        Line::styled("Loading sessions...", Style::new().fg(theme::DIM_PENDING))
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_session_table(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut ratatui::widgets::TableState,
    data: Option<&(
        Vec<codeflow_core::tui::data::SessionView>,
        codeflow_core::tui::data::SessionSummary,
    )>,
    show_all: bool,
) {
    use codeflow_core::tui::data::{abbreviate_session_id, format_task_id_for_display};
    use codeflow_core::tui::theme;
    use codeflow_core::tui::widgets::{DurationCell, PhaseBadge};
    use ratatui::layout::Constraint;
    use ratatui::style::Style;
    use ratatui::widgets::{Block, Borders, Cell, Row, Table};

    let header = Row::new(vec![
        "TASK", "SESSION", "BRANCH", "TYPE", "STATUS", "PHASE", "DURATION",
    ])
    .style(theme::header())
    .bottom_margin(1);

    let rows: Vec<Row> = data
        .map(|(views, _)| {
            views
                .iter()
                .filter(|s| show_all || !s.hidden)
                .map(|s| {
                    let sid_display = abbreviate_session_id(&s.session_id);
                    let task_display = format_task_id_for_display(
                        s.task_format_id.as_deref(),
                        s.task_id.as_deref(),
                    );
                    let status_badge = session_status_badge(&s.status);
                    let phase_badge = PhaseBadge::new(s.phase.as_deref());
                    let duration = DurationCell::new(Some(s.duration_secs));
                    let branch = s.branch.as_deref().unwrap_or("--");
                    let work_type = s.work_type.as_deref().unwrap_or("--");

                    Row::new(vec![
                        Cell::from(task_display),
                        Cell::from(sid_display),
                        Cell::from(branch.to_string()),
                        Cell::from(work_type.to_string()),
                        Cell::from(status_badge),
                        Cell::from(phase_badge.to_span()),
                        Cell::from(duration.to_span()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let table = Table::new(
        rows,
        [
            Constraint::Length(20), // TASK
            Constraint::Min(16),    // SESSION
            Constraint::Min(16),    // BRANCH
            Constraint::Length(6),  // TYPE
            Constraint::Length(12), // STATUS
            Constraint::Length(6),  // PHASE
            Constraint::Length(10), // DURATION
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(theme::BORDER_TYPE)
            .title(" Sessions ")
            .style(Style::new().fg(theme::WHITE_TEXT)),
    )
    .row_highlight_style(theme::selected())
    .highlight_symbol(format!("{} ", theme::TRIANGLE));

    frame.render_stateful_widget(table, area, state);
}

/// Map an interactive session status string to a styled Span.
fn session_status_badge(status: &str) -> ratatui::text::Span<'static> {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::Span;

    let (symbol, label, style) = match status {
        "active" => (
            theme::BULLET,
            "Active",
            Style::new().fg(theme::GREEN_SUCCESS),
        ),
        "stale" => (theme::CROSS, "Stale", Style::new().fg(theme::RED_FAILURE)),
        "complete" => (
            theme::CHECKMARK,
            "Complete",
            Style::new().fg(theme::DIM_PENDING),
        ),
        _ => (
            theme::CIRCLE,
            "Unknown",
            Style::new().fg(theme::DIM_PENDING),
        ),
    };

    Span::styled(format!("{symbol} {label}"), style)
}

fn render_session_detail(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    session: Option<&codeflow_core::tui::data::SessionView>,
) {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph};

    let lines = if let Some(s) = session {
        let pid_liveness = if is_session_stale(s.pid) {
            "DEAD"
        } else {
            "alive"
        };

        vec![
            Line::from(vec![
                Span::styled("Session: ", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(s.session_id.clone()),
                Span::raw("  "),
                Span::styled("PID: ", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(format!("{}", s.pid)),
                Span::styled(
                    format!(" ({pid_liveness})"),
                    if pid_liveness == "alive" {
                        Style::new().fg(theme::GREEN_SUCCESS)
                    } else {
                        Style::new().fg(theme::RED_FAILURE)
                    },
                ),
            ]),
            Line::from(vec![
                Span::styled("Worktree: ", Style::new().fg(theme::BLUE_ACCENT)),
                Span::styled(
                    s.worktree_path.as_deref().unwrap_or("--").to_string(),
                    theme::dim(),
                ),
            ]),
            Line::from(vec![
                Span::styled("Team: ", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(s.team_name.as_deref().unwrap_or("--").to_string()),
                Span::raw("  "),
                Span::styled("Branch: ", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(s.branch.as_deref().unwrap_or("--").to_string()),
                Span::raw("  "),
                Span::styled("Type: ", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(s.work_type.as_deref().unwrap_or("--").to_string()),
            ]),
        ]
    } else {
        vec![Line::styled("No session selected", theme::dim())]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(theme::BORDER_TYPE)
        .title(" Details ")
        .style(Style::new().fg(theme::WHITE_TEXT));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn get_selected_session<'a>(
    data: Option<&'a (
        Vec<codeflow_core::tui::data::SessionView>,
        codeflow_core::tui::data::SessionSummary,
    )>,
    state: &ratatui::widgets::TableState,
    show_all: bool,
) -> Option<&'a codeflow_core::tui::data::SessionView> {
    data.and_then(|(views, _)| {
        state
            .selected()
            .and_then(|i| views.iter().filter(|v| show_all || !v.hidden).nth(i))
    })
}

/// Check if a tmux session exists.
fn tmux_has_session(name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
        register_interactive_session(dir.path(), "ses-test", None, None, "codeflow", true).await;
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
            None,
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

        register_interactive_session(dir.path(), "ses-f1", None, None, "claude", false).await;

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

        register_interactive_session(dir.path(), "ses-unmanaged", None, None, "claude", false)
            .await;

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
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
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
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
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
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
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
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
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
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
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
            task_id: None,
            task_format_id: None,
            last_phase: None,
            tmux_session: None,
            team_name: None,
            source_cli: "codeflow".into(),
            managed: true,
            created_at: "2026-04-08T00:00:00Z".into(),
            updated_at: None,
            completed_at: None,
        };
        assert_eq!(derive_session_phase(&session), "-");
    }

    // ─── TUI badge and keybinding tests ──────────────────────────────────

    #[test]
    fn test_session_status_badge_active() {
        let span = session_status_badge("active");
        let content = span.content.to_string();
        assert!(
            content.contains("Active"),
            "badge should say Active: {content}"
        );
        assert_eq!(
            span.style.fg,
            Some(codeflow_core::tui::theme::GREEN_SUCCESS)
        );
    }

    #[test]
    fn test_session_status_badge_stale() {
        let span = session_status_badge("stale");
        let content = span.content.to_string();
        assert!(
            content.contains("Stale"),
            "badge should say Stale: {content}"
        );
        assert_eq!(span.style.fg, Some(codeflow_core::tui::theme::RED_FAILURE));
    }

    #[test]
    fn test_session_status_badge_complete() {
        let span = session_status_badge("complete");
        let content = span.content.to_string();
        assert!(
            content.contains("Complete"),
            "badge should say Complete: {content}"
        );
        assert_eq!(span.style.fg, Some(codeflow_core::tui::theme::DIM_PENDING));
    }

    #[test]
    fn test_session_status_badge_unknown() {
        let span = session_status_badge("something-else");
        let content = span.content.to_string();
        assert!(
            content.contains("Unknown"),
            "badge should say Unknown: {content}"
        );
    }

    #[test]
    fn test_session_keybinding_line_contents() {
        let line = session_keybinding_line();
        let text = line.to_string();
        assert!(text.contains("[Enter]"), "should contain Enter: {text}");
        assert!(text.contains("Attach"), "should contain Attach: {text}");
        assert!(text.contains("[c]"), "should contain c: {text}");
        assert!(text.contains("Cleanup"), "should contain Cleanup: {text}");
        assert!(text.contains("[Up/Down]"), "should contain Up/Down: {text}");
        assert!(text.contains("[q]"), "should contain q: {text}");
        assert!(text.contains("Quit"), "should contain Quit: {text}");
    }

    #[test]
    fn test_get_selected_session_none_data() {
        let state = ratatui::widgets::TableState::default();
        let result = get_selected_session(None, &state, true);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_selected_session_no_selection() {
        let views = vec![codeflow_core::tui::data::SessionView {
            session_id: "ses-001".into(),
            status: "active".into(),
            branch: None,
            phase: None,
            work_type: None,
            task_id: None,
            task_format_id: None,
            team_name: None,
            pid: 1,
            worktree_path: None,
            duration_secs: 0,
            managed: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            hidden: false,
        }];
        let summary = codeflow_core::tui::data::SessionSummary::default();
        let data = Some((views, summary));
        let state = ratatui::widgets::TableState::default();
        let result = get_selected_session(data.as_ref(), &state, true);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_selected_session_valid_selection() {
        let views = vec![codeflow_core::tui::data::SessionView {
            session_id: "ses-001".into(),
            status: "active".into(),
            branch: Some("feat/test".into()),
            phase: Some("PF4".into()),
            work_type: Some("FEAT".into()),
            task_id: None,
            task_format_id: None,
            team_name: Some("team-1".into()),
            pid: 42,
            worktree_path: None,
            duration_secs: 120,
            managed: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            hidden: false,
        }];
        let summary = codeflow_core::tui::data::SessionSummary::default();
        let data = Some((views, summary));
        let mut state = ratatui::widgets::TableState::default();
        state.select(Some(0));
        let result = get_selected_session(data.as_ref(), &state, true);
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "ses-001");
    }

    #[test]
    fn test_get_selected_session_respects_hidden_filter() {
        let views = vec![
            codeflow_core::tui::data::SessionView {
                session_id: "ses-active".into(),
                status: "active".into(),
                branch: None,
                phase: None,
                work_type: None,
                task_id: None,
                task_format_id: None,
                team_name: None,
                pid: 1,
                worktree_path: None,
                duration_secs: 0,
                managed: true,
                created_at: "2026-01-01T00:00:00Z".into(),
                hidden: false,
            },
            codeflow_core::tui::data::SessionView {
                session_id: "ses-hidden".into(),
                status: "stale".into(),
                branch: None,
                phase: None,
                work_type: None,
                task_id: None,
                task_format_id: None,
                team_name: None,
                pid: 2,
                worktree_path: None,
                duration_secs: 0,
                managed: true,
                created_at: "2026-01-01T00:00:00Z".into(),
                hidden: true,
            },
        ];
        let summary = codeflow_core::tui::data::SessionSummary::default();
        let data = Some((views, summary));
        let mut state = ratatui::widgets::TableState::default();
        state.select(Some(0));
        // With show_all=false, only the non-hidden session is visible at index 0.
        let result = get_selected_session(data.as_ref(), &state, false);
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "ses-active");
        // With show_all=true, the hidden session is at index 1.
        state.select(Some(1));
        let result = get_selected_session(data.as_ref(), &state, true);
        assert!(result.is_some());
        assert_eq!(result.unwrap().session_id, "ses-hidden");
    }

    #[test]
    fn test_keybinding_and_badge_do_not_panic() {
        // Verify rendering helper paths don't panic.
        let _line = session_keybinding_line();
        let _active = session_status_badge("active");
        let _stale = session_status_badge("stale");
        let _complete = session_status_badge("complete");
        let _unknown = session_status_badge("other");
    }

    // ─── Clap parsing: Status --watch ────────────────────────────────────

    #[test]
    fn test_interactive_status_watch_flag_parsing() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: InteractiveCommand,
        }

        let cli = TestCli::try_parse_from(["test", "status", "--watch"]).unwrap();
        if let InteractiveCommand::Status {
            watch, interval, ..
        } = cli.cmd
        {
            assert!(watch);
            assert_eq!(interval, 2); // default
        } else {
            panic!("expected Status variant");
        }
    }

    #[test]
    fn test_interactive_status_watch_with_interval() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: InteractiveCommand,
        }

        let cli =
            TestCli::try_parse_from(["test", "status", "--watch", "--interval", "5"]).unwrap();
        if let InteractiveCommand::Status {
            watch, interval, ..
        } = cli.cmd
        {
            assert!(watch);
            assert_eq!(interval, 5);
        } else {
            panic!("expected Status variant");
        }
    }

    #[test]
    fn test_interactive_status_no_watch_default() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: InteractiveCommand,
        }

        let cli = TestCli::try_parse_from(["test", "status"]).unwrap();
        if let InteractiveCommand::Status { watch, .. } = cli.cmd {
            assert!(!watch);
        } else {
            panic!("expected Status variant");
        }
    }
}
