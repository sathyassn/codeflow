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
            let wt_name = format!("worktree-{sid_str}");
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
    let mut env_vars: Vec<(String, String)> = vec![
        ("CODEFLOW_MANAGED".into(), "true".into()),
        ("CODEFLOW_SESSION_ID".into(), sid_str.clone()),
        (
            "CF_PROJECT_ROOT".into(),
            project_dir.to_string_lossy().to_string(),
        ),
    ];
    if let Some(ref wt_path) = worktree_path {
        env_vars.push(("CODEFLOW_WORKTREE_PATH".into(), wt_path.clone()));
    }

    eprintln!("codeflow: session {sid_str} starting");
    if let Some(ref wt_path) = worktree_path {
        eprintln!("codeflow: worktree at {wt_path}");
    }

    // Exec claude -- replaces the current process.
    let project_dir_str = project_dir.to_string_lossy().to_string();
    let work_dir = worktree_path.as_deref().unwrap_or(&project_dir_str);

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
async fn run_status() -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    let sessions = query_sessions(&project_dir, Some("active")).await?;

    if sessions.is_empty() {
        println!("No active interactive sessions.");
        return Ok(());
    }

    println!(
        "{:<32} {:<8} {:<10} {:<10} {:<40}",
        "SESSION ID", "PID", "STATUS", "SOURCE", "WORKTREE"
    );
    for s in &sessions {
        let pid_u32 = u32::try_from(s.pid).unwrap_or(0);
        let pid_alive = codeflow_core::session::process::is_process_alive(pid_u32);
        let liveness = if pid_alive { "alive" } else { "DEAD" };
        let pid_display = format!("{} ({liveness})", s.pid);
        let wt = s.worktree_path.as_deref().unwrap_or("-");
        println!(
            "{:<32} {:<8} {:<10} {:<10} {wt}",
            s.session_id, pid_display, s.status, s.source_cli
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

/// Remove stale sessions (dead PID).
async fn run_cleanup() -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    let sessions = query_sessions(&project_dir, Some("active")).await?;

    let mut cleaned = 0u32;
    if let Ok(store) = open_store(&project_dir).await {
        for s in &sessions {
            let pid_u32 = u32::try_from(s.pid).unwrap_or(0);
            if !codeflow_core::session::process::is_process_alive(pid_u32) {
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

    if cleaned == 0 {
        println!("No stale sessions found.");
    } else {
        println!("Cleaned {cleaned} stale session(s).");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_claude_function_signature() {
        // Verify the function exists and has the expected signature.
        // Cannot call exec_claude because it replaces the process.
        let fn_ptr: fn(&str, &[(String, String)]) -> std::io::Error = exec_claude;
        assert_eq!(std::mem::size_of_val(&fn_ptr), std::mem::size_of::<usize>());
    }
}
