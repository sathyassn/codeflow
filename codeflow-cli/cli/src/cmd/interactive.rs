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
        let pid_display = format_pid_with_liveness(s.pid);
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
}
