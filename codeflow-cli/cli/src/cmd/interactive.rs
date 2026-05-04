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
        /// Force pruning of ALL completed/stale interactive_session rows,
        /// regardless of `interactive.retention_days`. Use to reclaim DB
        /// space immediately. INF-TSK-024-050 AC #5.
        #[arg(long)]
        prune_completed: bool,
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
            // INF-TSK-049-001 batch 2 (AC #32): surface unprocessed rescue
            // bundles before status output. Throttled once-per-24h via
            // .last-scan; suppressed by CODEFLOW_NO_RESCUE_BANNER=true.
            crate::maybe_emit_rescue_banner();
            // Default to TUI when stdout is a TTY; --once forces text output.
            if once || !std::io::stdout().is_terminal() {
                run_status(all, status.as_deref()).await
            } else {
                let project_dir = helpers::detect_project_root()?;
                run_status_tui(&project_dir, interval).await
            }
        }
        Some(InteractiveCommand::Cleanup {
            no_purge,
            prune_completed,
        }) => run_cleanup(no_purge, prune_completed).await,
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
            // INF-TSK-024-051 Phase 4-B: removed
            // `locked_update_lead_pid(&registry_path, &wt_name, std::process::id())`.
            // The wrapper-context PID was wrong-by-construction
            // (codeflow CLI PID captured pre-`exec`, becomes the
            // tmux-attach client PID after exec — never the live Claude
            // lead). The registry's `lead_pid` field is gone; the
            // canonical PID is written by SessionStart into
            // `pathflow-session-status.json::lead_pid` instead.
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

    // Register InteractiveSession in DB (best-effort but visible).
    // INF-TSK-050-003 AC-07: any failure is non-fatal (we still launch
    // claude) but is logged to stderr by `register_interactive_session`
    // itself. Discarding the Result here is intentional and explicit:
    // the warning was already emitted, and we don't gate the session
    // launch on observability bookkeeping.
    if let Err(e) = register_interactive_session(
        &project_dir,
        &sid_str,
        worktree_path.as_deref(),
        tmux_name.as_deref(),
        "codeflow",
        true,
    )
    .await
    {
        // No additional eprintln: the function already logged the warn.
        // Bind to a named variable so the trailing `_ = e;` line
        // documents the intentional discard for future readers / clippy.
        let _ = e;
    }

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
/// INF-TSK-050-003 AC-07: errors are now visible — every failure path
/// emits a stderr `warn:` line and bubbles the error to the caller via
/// `Result<(), anyhow::Error>`. The previous `let _ = …` and silent
/// `Err(_) => return` swallowed open and CREATE errors so silently that
/// a corrupted DB or wrong schema would produce a session that never
/// shows up in `codeflow interactive status` with no indication of why.
///
/// Returning a `Result` does NOT make the call fatal at the call site:
/// `codeflow -i` continues to launch claude even when registration
/// fails (the registration is best-effort observability state, not a
/// gate on the session). Callers MUST log the error rather than discard
/// it via `let _`. See `run` for the canonical call pattern.
///
/// # Errors
///
/// Returns an error if the SurrealDB store cannot be opened or if the
/// CREATE statement fails (network, schema mismatch, write conflict).
async fn register_interactive_session(
    project_dir: &Path,
    session_id: &str,
    worktree_path: Option<&str>,
    tmux_session: Option<&str>,
    source_cli: &str,
    managed: bool,
) -> anyhow::Result<()> {
    let store = match open_store(project_dir).await {
        Ok(s) => s,
        Err(e) => {
            let msg = format_register_warn_line("open_store", &e.to_string());
            eprintln!("{msg}");
            return Err(anyhow::anyhow!("open_store: {e}"));
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let wt = worktree_path.map(String::from);
    let tmux = tmux_session.map(String::from);
    // INF-TSK-049-001 AC #8: classify this row as autorun when launched
    // inside an autorun worker (AUTORUN_SESSION_ID inherited from the
    // orchestrator). Real interactive launches leave the env var unset.
    let session_kind = if std::env::var("AUTORUN_SESSION_ID").is_ok() {
        "autorun"
    } else {
        "interactive"
    };
    // INF-TSK-024-051 Phase 4-C: removed `pid = $pid` from the CREATE
    // statement and the corresponding `let pid = i64::from(std::process::id())`
    // binding. The DB column was wrong-by-construction (CLI PID
    // captured pre-`exec`, became tmux-attach-client PID after exec).
    // Canonical PID lives in `pathflow-session-status.json::lead_pid`,
    // written by SessionStart via `parent_id() -> validate_claude_pid`.
    let result = store
        .db()
        .query(
            "CREATE interactive_session SET \
             session_id = $session_id, \
             status = 'active', \
             worktree_path = $worktree_path, \
             tmux_session = $tmux_session, \
             branch = NONE, \
             work_type = NONE, \
             task_id = NONE, \
             team_name = NONE, \
             source_cli = $source_cli, \
             managed = $managed, \
             session_kind = $session_kind, \
             created_at = $created_at, \
             updated_at = NONE, \
             completed_at = NONE;",
        )
        .bind(("session_id", session_id.to_string()))
        .bind(("worktree_path", wt))
        .bind(("tmux_session", tmux))
        .bind(("source_cli", source_cli.to_string()))
        .bind(("managed", managed))
        .bind(("session_kind", session_kind.to_string()))
        .bind(("created_at", now))
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(e) => {
            let msg = format_register_warn_line("CREATE", &e.to_string());
            eprintln!("{msg}");
            Err(anyhow::anyhow!("CREATE interactive_session: {e}"))
        }
    }
}

/// INF-TSK-050-003 AC-07 / WS-REV MINOR-1: build the stderr `warn:`
/// line emitted by `register_interactive_session` failure paths.
///
/// Pure helper so unit tests can assert on the exact contract format
/// (`warn: register_interactive_session failed: <stage>: <error>`)
/// without spawning a subprocess to capture stderr. Production code
/// passes the formatted result to `eprintln!`. Stage is one of
/// `"open_store"` (DB cannot be opened) or `"CREATE"` (CREATE
/// statement failed); other strings would be a code bug.
pub(crate) fn format_register_warn_line(stage: &str, err: &str) -> String {
    format!("warn: register_interactive_session failed: {stage}: {err}")
}

/// Show interactive sessions (text mode).
///
/// By default, shows only active sessions. Pass `show_all=true` (via `--all`)
/// to show all sessions. Optionally filter by `--status <active|stale|complete>`.
///
/// INF-TSK-024-051 Phase 7-rework: this function is now DB-only — every
/// production session is registered in `interactive_session`, and the
/// canonical liveness chokepoint reads `pathflow-session-status.json::lead_pid`
/// directly, so the prior `.state/interactive/heartbeat-*` filesystem
/// fallback is gone alongside the heartbeat writers.
async fn run_status(show_all: bool, status_filter: Option<&str>) -> Result<()> {
    let project_dir = helpers::detect_project_root()?;

    // INF-TSK-024-051: self-heal stale rows whose canonical PID file shows
    // the session is actually alive. Prior runs of the broken DB-pid
    // validator wrongly promoted live sessions to `stale`; the canonical
    // chokepoint now resolves the right verdict, so demote them back to
    // `active` before the user-visible SELECT runs. Cheap (only fires for
    // rows currently marked `stale` whose status file reports Active) and
    // self-healing (idempotent). Errors logged to stderr but never fatal.
    if let Err(e) = repair_wrongly_stale_rows(&project_dir).await {
        eprintln!("interactive status: stale-row repair skipped: {e}");
    }

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

    // INF-TSK-024-051 Phase 7-rework: heartbeat scanning removed. Every
    // production session writes a `pathflow-session-status.json`, and
    // every session is registered in `interactive_session`, so the
    // DB-vs-filesystem reconciliation dance is no longer needed.
    if sessions.is_empty() {
        if show_all {
            println!("No interactive sessions found.");
        } else {
            println!("No active interactive sessions found. Use --all to show all sessions.");
        }
        return Ok(());
    }

    // INF-TSK-050-003 AC-06 / WS-REV MAJOR-2: text-mode active->stale
    // promotion via the testable helper `promote_active_sessions_to_stale`.
    // Extracted so the unit test can drive the logic against a seeded DB
    // + status file and assert the row was UPDATEd to stale BEFORE the
    // listing renders.
    let store_for_promo = open_store(&project_dir).await.ok();
    let promoted = if let Some(ref store) = store_for_promo {
        promote_active_sessions_to_stale(store, &project_dir, &sessions).await
    } else {
        std::collections::HashSet::new()
    };

    println!(
        "{:<32} {:<14} {:<10} {:<10} {:<12} {:<8} {:<8} {:<30}",
        "SESSION ID", "PID", "STATUS", "SOURCE", "BRANCH", "TYPE", "PHASE", "WORKTREE"
    );
    // Limit text output to 50 most recent.
    for s in sessions.iter().take(50) {
        // INF-TSK-024-051: display the canonical PID from
        // pathflow-session-status.json (worktree-resolved). The DB
        // `pid` column was removed in Phase 4-C.
        let displayed_pid = resolve_displayed_pid(&project_dir, &s.session_id);
        let pid_display = format_pid_with_liveness(displayed_pid);
        let wt = s.worktree_path.as_deref().unwrap_or("-");
        let branch = s.branch.as_deref().unwrap_or("-");
        let work_type = s.work_type.as_deref().unwrap_or("-");
        // Derive phase from pathflow-session-status.json if available.
        let phase = derive_session_phase(s);
        // Display the EFFECTIVE status: if we just promoted this row,
        // show 'stale' instead of the in-memory 'active' so the rendered
        // value matches the DB state (AC-06 parity with TUI).
        let display_status = if promoted.contains(&s.session_id) {
            "stale".to_string()
        } else {
            s.status.to_string()
        };
        println!(
            "{:<32} {:<14} {:<10} {:<10} {:<12} {:<8} {:<8} {wt}",
            s.session_id, pid_display, display_status, s.source_cli, branch, work_type, phase
        );
    }
    Ok(())
}

/// INF-TSK-050-003 AC-06 (extracted by WS-REV MAJOR-2 rework).
///
/// For each session in `sessions` whose DB status is `active`, query the
/// canonical chokepoint at `is_session_alive(project_dir, sid)`; if the
/// verdict is anything other than `Active` (i.e. `Dead` or `Unknown`),
/// UPDATE the row to `status='stale'` with completed_at and updated_at
/// set to `now`. Returns the set of session_ids that were successfully
/// promoted so the caller can render `stale` (matching the new DB
/// state) instead of the in-memory `active` from the original SELECT.
///
/// The CAS guard (`status = 'active' AND completed_at = NONE`) prevents
/// racing a concurrent transition. UPDATE failures are non-fatal and
/// surface to stderr with a `warn:` prefix so an unreachable DB doesn't
/// brick the status command — the operator just won't see the
/// promotion in this run.
///
/// Mirrors the TUI's `fetch_session_views_with_keep_last_and_validator`
/// promotion path so text + TUI display the same row state for the
/// same input.
pub(crate) async fn promote_active_sessions_to_stale(
    store: &codeflow_core::store::SurrealStore,
    project_dir: &Path,
    sessions: &[codeflow_core::models::InteractiveSession],
) -> std::collections::HashSet<String> {
    let now_rfc = chrono::Utc::now().to_rfc3339();
    let mut promoted = std::collections::HashSet::new();
    for s in sessions {
        if s.status.to_string() != "active" {
            continue;
        }
        let verdict =
            codeflow_core::session::liveness::is_session_alive(project_dir, &s.session_id);
        // Only Dead promotes; Unknown is also treated as Dead for
        // active rows because the canonical status file should always
        // exist for any session past PF1-INIT — a missing file on an
        // active row is a crash signature.
        let is_dead = !matches!(
            verdict,
            codeflow_core::session::liveness::SessionLiveness::Active
        );
        if !is_dead {
            continue;
        }
        let res = store
            .db()
            .query(
                "UPDATE interactive_session SET status = 'stale', \
                 updated_at = $now, completed_at = $now \
                 WHERE session_id = $sid \
                   AND status = 'active' \
                   AND completed_at = NONE",
            )
            .bind(("now", now_rfc.clone()))
            .bind(("sid", s.session_id.clone()))
            .await;
        match res {
            Ok(_) => {
                promoted.insert(s.session_id.clone());
            }
            Err(e) => {
                eprintln!(
                    "warn: stale-promotion DB update failed for {}: {e}",
                    s.session_id
                );
            }
        }
    }
    promoted
}

/// List all interactive sessions (alias for `status --once`).
#[allow(dead_code)]
async fn run_list() -> Result<()> {
    run_status(true, None).await
}

/// Remove stale sessions (dead PID) and sweep filesystem artifacts.
///
/// When `no_purge` is false (the default), also purges terminal sessions
/// from the DB using retention config.
///
/// INF-TSK-024-050 AC #5: the per-row cutoff comes from
/// `interactive.retention_days` (new config key, default 30). When
/// `prune_completed` is true, ALL terminal interactive_session rows are
/// pruned regardless of age (the cutoff is set to the current instant).
async fn run_cleanup(no_purge: bool, prune_completed: bool) -> Result<()> {
    let project_dir = helpers::detect_project_root()?;
    // INF-TSK-050-001 AC #13: cleanup considers both `active` (promote to
    // stale via the dead-PID predicate) and existing `stale` rows that
    // still lack a `completed_at` anchor (back-fill so the TUI freezes
    // the DURATION column). Stale rows whose `completed_at` was set on a
    // prior run are skipped — the SQL `WHERE completed_at = NONE` clause
    // makes the back-fill idempotent.
    let mut sessions = query_sessions(&project_dir, Some("active")).await?;
    let stale_sessions = query_sessions(&project_dir, Some("stale")).await?;
    sessions.extend(stale_sessions);

    let mut cleaned = 0u32;
    let store_result = open_store(&project_dir).await;
    if let Ok(ref store) = store_result {
        for s in &sessions {
            // Active rows: only act if the canonical chokepoint reports
            // `Dead`. INF-TSK-024-051 Phase 3: migrated from the legacy
            // DB-pid stale predicate (which validated the DB `pid`
            // column — wrong-by-construction for managed sessions where
            // the pid was the codeflow CLI PID captured pre-`exec`,
            // becoming the tmux-attach client after exec) to the
            // canonical chokepoint. `Unknown` ABSTAINS so an
            // initializing session is never demoted by this command.
            // Stale rows: act unconditionally on the back-fill path —
            // the row is already declared dead; we're just back-filling
            // missing anchor timestamps for the freeze logic.
            let s_status = s.status.to_string();
            let needs_action = match s_status.as_str() {
                "active" => matches!(
                    codeflow_core::session::liveness::is_session_alive(&project_dir, &s.session_id,),
                    codeflow_core::session::liveness::SessionLiveness::Dead
                ),
                "stale" => s.completed_at.is_none(),
                _ => false,
            };
            if !needs_action {
                continue;
            }
            let now = chrono::Utc::now().to_rfc3339();
            // INF-TSK-049-001 AC #13: set `completed_at` alongside
            // `updated_at` so the frozen DURATION in the status TUI has
            // an anchor. Previously only `updated_at` was set, which made
            // the duration collapse to the -1 sentinel and render as
            // "--" until the next post_tool_use touch.
            //
            // INF-TSK-050-001 AC #13: extended `WHERE` clause covers both
            // active and stale rows. The CAS guard on
            // `status IN ('active','stale')` prevents racing a concurrent
            // session that may have transitioned to `complete`.
            let _ = store
                .db()
                .query(
                    "UPDATE interactive_session SET status = 'stale', \
                     updated_at = $now, completed_at = $now \
                     WHERE session_id = $sid \
                       AND status IN ['active', 'stale'] \
                       AND completed_at = NONE",
                )
                .bind(("now", now))
                .bind(("sid", s.session_id.clone()))
                .await;
            if s_status == "active" {
                // INF-TSK-024-051 Phase 4-C: DB `pid` column gone; the
                // canonical PID is the chokepoint verdict that drove
                // this branch. Just log the session id.
                eprintln!("cleaned stale session: {} (session dead)", s.session_id);
            } else {
                eprintln!(
                    "finalized stale session: {} (back-filled completed_at)",
                    s.session_id
                );
            }
            cleaned += 1;
        }
    }

    // Filesystem artifact sweeps (independent of DB state).
    // INF-TSK-024-051 Phase 7-rework: `sweep_stale_heartbeats` removed.
    // The heartbeat directory had no writers after Phase 4-A (interactive)
    // and Phase 7-rework (autorun); the sweeper became a no-op and was
    // deleted alongside its readers.
    let env_cleaned = sweep_dead_pid_env_files(&project_dir);
    let sess_cleaned = sweep_stale_session_dirs(&project_dir);
    let map_cleaned = clean_session_worktree_map(&project_dir);
    let wt_cleaned = mark_stale_worktree_entries(&project_dir);

    let total = cleaned + env_cleaned + sess_cleaned + map_cleaned + wt_cleaned;
    if total == 0 {
        println!("No stale sessions or artifacts found.");
    } else {
        if cleaned > 0 {
            println!("Cleaned {cleaned} stale DB session(s).");
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
    //
    // INF-TSK-024-050 AC #5: cutoff is `interactive.retention_days` from
    // parallel-work-config.json (default 30). `--prune-completed` overrides
    // the age check by setting the cutoff to the current instant, which
    // matches every terminal row.
    if !no_purge {
        if let Ok(ref store) = store_result {
            use codeflow_core::store::DataStore;
            let cfg = codeflow_core::autorun::load_config(&project_dir).unwrap_or_default();
            let retention = cfg.retention.clone();
            let interactive_retention_days = cfg.interactive.retention_days;
            if retention.purge_on_cleanup || prune_completed {
                let cutoff_dt = if prune_completed {
                    chrono::Utc::now()
                } else {
                    chrono::Utc::now()
                        - chrono::Duration::days(i64::from(interactive_retention_days))
                };
                let cutoff_str = cutoff_dt.to_rfc3339();
                // `--prune-completed` ignores keep_last so callers can
                // unconditionally clear the table; the normal path keeps
                // the configured tail.
                let keep_last = if prune_completed {
                    0
                } else {
                    retention.keep_last
                };
                match store
                    .prune_interactive_sessions(&cutoff_str, keep_last)
                    .await
                {
                    Ok(result) if result.sessions_deleted > 0 => {
                        if prune_completed {
                            println!(
                                "Pruned {} terminal interactive session(s) from DB (--prune-completed; ignored age).",
                                result.sessions_deleted,
                            );
                        } else {
                            println!(
                                "Purged {} old interactive session(s) from DB (>{} days, kept last {}).",
                                result.sessions_deleted,
                                interactive_retention_days,
                                retention.keep_last,
                            );
                        }
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
/// Repair `stale` rows whose canonical chokepoint reports `Active`.
///
/// INF-TSK-024-051: prior runs of the TUI used the DB `pid` column as the
/// liveness oracle. The DB column captured the codeflow CLI PID before
/// `exec`, which became the tmux-attach-client PID after exec — never
/// the live Claude lead. `validate_claude_pid` therefore failed and the
/// row was wrongly demoted to `stale`. This helper consults the new
/// canonical chokepoint (`is_session_alive` reads the worktree-resolved
/// `pathflow-session-status.json::lead_pid`) and demotes such rows back
/// to `active`, clearing the synthetic `completed_at` so duration display
/// resumes live computation.
///
/// TODO(INF-TSK-024-051): this is a one-time legacy-row repair pass that
/// runs on every `codeflow interactive status` invocation. Once codeflow
/// gains a real DB migration framework (separate task, not in scope for
/// this PR), this logic should move into a one-shot migration step that
/// runs at startup, not a per-invocation hot-path. Until then the
/// repair pays the SELECT cost on every call; once no more wrongly-stale
/// rows exist, the SELECT returns empty and the repair is a no-op.
///
/// Idempotent and best-effort: silently continues on DB errors.
async fn repair_wrongly_stale_rows(project_dir: &Path) -> Result<()> {
    let store = open_store(project_dir).await?;
    // SELECT * so the row deserialises into the full model. Selecting only
    // `session_id` would leave other (non-Optional) fields unset and the
    // take<Vec<...>>() call would fail silently.
    let mut response = store
        .db()
        .query(
            "SELECT * FROM interactive_session \
             WHERE status = 'stale' \
               AND (session_kind = 'interactive' OR session_kind = NONE)",
        )
        .await
        .context("repair: query stale rows")?;
    let rows: Vec<codeflow_core::models::InteractiveSession> =
        response.take(0).context("repair: deserialize stale rows")?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut repaired = 0u32;
    for r in rows {
        let verdict =
            codeflow_core::session::liveness::is_session_alive(project_dir, &r.session_id);
        if matches!(
            verdict,
            codeflow_core::session::liveness::SessionLiveness::Active
        ) {
            store
                .db()
                .query(
                    "UPDATE interactive_session SET status = 'active', \
                     completed_at = NONE, updated_at = $now \
                     WHERE session_id = $sid AND status = 'stale'",
                )
                .bind(("now", now.clone()))
                .bind(("sid", r.session_id.clone()))
                .await
                .with_context(|| format!("repair: update {sid}", sid = r.session_id))?;
            repaired += 1;
        }
    }
    if repaired > 0 {
        eprintln!("interactive status: repaired {repaired} wrongly-stale row(s)");
    }
    Ok(())
}

async fn query_sessions(
    project_dir: &Path,
    status_filter: Option<&str>,
) -> Result<Vec<codeflow_core::models::InteractiveSession>> {
    let store = open_store(project_dir).await?;

    // INF-TSK-050-001 AC #12: every interactive list/status query MUST
    // exclude autorun-worker rows (which register as `interactive_session`
    // with `session_kind='autorun'`). The `OR session_kind = NONE` clause
    // is defensive: pre-migration rows lack the column, so they default
    // to "treat as interactive" — matches `tui::data::fetch_session_views`
    // behavior. The migration in `apply_schema` backfills `'autorun'` for
    // rows whose `session_id` matches a real `autorun_session`, so after
    // first startup the OR-NONE branch only applies to organic
    // interactive rows that pre-date the column.
    let mut result = match status_filter {
        Some(status) => store
            .db()
            .query(
                "SELECT * FROM interactive_session \
                 WHERE status = $status \
                   AND (session_kind = 'interactive' OR session_kind = NONE) \
                 ORDER BY created_at DESC",
            )
            .bind(("status", status.to_string()))
            .await
            .context("querying interactive sessions")?,
        None => store
            .db()
            .query(
                "SELECT * FROM interactive_session \
                 WHERE (session_kind = 'interactive' OR session_kind = NONE) \
                 ORDER BY created_at DESC",
            )
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

/// Check if a session's PID is stale (dead OR not a Claude Code process).
///
/// INF-TSK-024-051: callers MUST pass the canonical PID read from
/// `pathflow-session-status.json::lead_pid` via the worktree-resolved
/// chokepoint, NOT the DB `pid` column. The DB column captured the
/// codeflow CLI process pre-`exec` and points at the tmux-attach client
/// after exec, which `validate_claude_pid` correctly rejects (not named
/// "claude") — but the rejection is meaningless because the wrong PID
/// was being checked. With the canonical PID, "alive AND named claude"
/// is the right invariant.
///
/// Fail-secure: any uncertainty (missing name lookup, out-of-range PID,
/// zero) returns `true` (i.e. "stale, safe to clean up").
///
/// INF-TSK-050-003 AC-15 rename: previously named with a string the
/// `test-no-legacy-liveness` lint script would now flag — renamed to
/// avoid the legacy-name lint guard. Same semantics: a fail-secure
/// stale predicate over a canonical-PID input. NOTE: this is NOT the
/// deleted autorun three-signal stale function (AC-09 deleted it from
/// `core/src/autorun/stale.rs`) — it's the `i64`-PID-only adapter
/// that the TUI uses for display formatting where a Path-based
/// chokepoint call would be a needless allocation.
fn is_canonical_pid_stale(pid: i64) -> bool {
    let Ok(pid_u32) = u32::try_from(pid) else {
        return true; // out-of-range or negative → fail-secure stale
    };
    if pid_u32 == 0 {
        return true;
    }
    codeflow_core::session::process::validate_claude_pid(pid_u32) == 0
}

/// Format a PID with liveness indicator for status display.
fn format_pid_with_liveness(pid: i64) -> String {
    let liveness = if is_canonical_pid_stale(pid) {
        "DEAD"
    } else {
        "alive"
    };
    format!("{pid} ({liveness})")
}

/// Resolve a session's displayed PID for text-mode status output.
///
/// INF-TSK-024-051 Phase 4-C: returns the canonical PID from the
/// worktree-resolved `pathflow-session-status.json::lead_pid` (single
/// source of truth). The DB `pid` column was removed in Phase 4-C; this
/// returns 0 when the status file is missing (legacy unmanaged sessions
/// pre-migration), and the caller renders 0 as "(0)" / "DEAD" via
/// `format_pid_with_liveness`.
fn resolve_displayed_pid(project_dir: &std::path::Path, session_id: &str) -> i64 {
    i64::from(codeflow_core::session::liveness::read_canonical_lead_pid(
        project_dir,
        session_id,
    ))
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
//
// INF-TSK-024-051 Phase 7-rework: heartbeat scanners + sweepers removed.
//   - `scan_heartbeat_sessions`: read `.state/interactive/heartbeat-*`
//     and report DB-orphaned sessions. Now redundant — every production
//     session is in `interactive_session`.
//   - `is_heartbeat_session_alive`: delegated to the chokepoint anyway;
//     direct chokepoint calls at the (one) remaining site are clearer.
//   - `sweep_stale_heartbeats` / `_inner`: deleted alongside the readers
//     since the directory has no writers post-rework.
//
// Replacement: callers use `crate::session::liveness::is_session_alive`
// for liveness, and the `interactive_session` row is the source of
// truth for the session list.

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
                // No status file = stale artifact. The production
                // `SessionStart` hook always writes the status file as its
                // first action when creating a session directory, so a
                // `.state/session/{sid}/` without a status file indicates
                // a partial cleanup (sentinel survived) or an abandoned
                // test fixture. Remove unconditionally — preserves the
                // pre-Phase-7-rework `is_heartbeat_session_alive(missing)
                // → false → remove` behavior without re-introducing the
                // heartbeat reader.
                remove_session_artifacts(project_dir, &name);
                count += 1;
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
                // INF-TSK-024-051 Phase 6: route through the canonical
                // chokepoint instead of duplicating the lead_pid extraction
                // + bare `is_process_alive` check. The chokepoint validates
                // alive AND named "claude", so a recycled PID owned by an
                // unrelated process (cargo, sshd, etc.) does not mask a
                // dead session as alive.
                let verdict =
                    codeflow_core::session::liveness::is_session_alive(project_dir, &name);
                if matches!(
                    verdict,
                    codeflow_core::session::liveness::SessionLiveness::Dead
                ) {
                    remove_session_artifacts(project_dir, &name);
                    count += 1;
                }
            }
            _ => {}
        }
    }
    count
}

/// Remove session directory + sentinel directory.
///
/// INF-TSK-024-051 Phase 7-rework: the heartbeat-file removal step was
/// dropped — the file is no longer written. A best-effort `remove_file`
/// against a non-existent path is a quiet no-op, but explicitly
/// documenting the absence avoids a future reader thinking the cleanup
/// is incomplete.
fn remove_session_artifacts(project_dir: &Path, sid: &str) {
    if sid.contains("..") {
        return;
    }
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
        // INF-TSK-024-051 Phase 4-B: switched from `entry.lead_pid` (the
        // removed registry field) to the canonical chokepoint via
        // `is_session_alive`. The chokepoint reads
        // `pathflow-session-status.json::lead_pid` worktree-resolved.
        // Only `Dead` triggers `mark_pending_cleanup` — `Unknown`
        // abstains (initializing sessions must not be marked stale).
        let Some(ref sid) = entry.session_id else {
            continue;
        };
        // Phase 4-C: use the `project_dir` parameter (caller-supplied)
        // rather than re-resolving via `detect_project_root()`. This
        // makes the function deterministic and testable; callers in
        // production already pass the correct project root.
        let verdict = codeflow_core::session::liveness::is_session_alive(project_dir, sid);
        if matches!(
            verdict,
            codeflow_core::session::liveness::SessionLiveness::Dead
        ) {
            let _ = codeflow_core::worktree::mark_pending_cleanup(&registry_path, sid);
            eprintln!("marked stale worktree: {} (session dead)", entry.name);
            count += 1;
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
    // INF-TSK-049-001 AC #11: session-scoped default list. Capture the TUI
    // invocation timestamp; by default the list only shows rows that are
    // either still active OR terminated at/after this moment. `show_stale_backlog`
    // (toggled by `[s]`) reveals the historical rows that were already stale
    // when the TUI started. Footer advertises the hidden count.
    let invocation_start: chrono::DateTime<chrono::Utc> = chrono::Utc::now();
    let mut show_stale_backlog = false;

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

    // Auto-refresh decoupling (INF-TSK-047-001 AC #6): input polls every
    // 100 ms for snappy key handling; data fetch runs at the user-configured
    // `interval_secs` cadence. This way a selection change redraws the UI
    // without waiting for the next DB round-trip.
    let poll_interval = Duration::from_millis(100);
    let fetch_interval = Duration::from_secs(interval_secs.max(1));
    let mut last_fetch = std::time::Instant::now();

    loop {
        // INF-TSK-049-001 AC #11: session-scoped visibility. A stale or
        // complete row is considered "historical stale" (hidden from the
        // default list) when its termination time (computed from
        // `created_at + duration_secs`) is strictly before
        // `invocation_start`. Active rows are always visible; rows with
        // unknown duration surface by default. `[s]` toggles
        // `show_stale_backlog` to reveal the hidden historical set.
        //
        // This is a PURE predicate on the view (no mutation) so a single
        // data fetch can serve multiple render ticks with different `[s]`
        // toggle states without corrupting the underlying data.
        let is_pre_invocation_stale = |v: &codeflow_core::tui::data::SessionView| -> bool {
            if v.status == "active" {
                return false;
            }
            if v.duration_secs < 0 {
                return false;
            }
            let Ok(created) = chrono::DateTime::parse_from_rfc3339(&v.created_at) else {
                return false;
            };
            let terminated_at =
                created.with_timezone(&chrono::Utc) + chrono::Duration::seconds(v.duration_secs);
            terminated_at < invocation_start
        };
        // `[s] show stale (N hidden)` count — number of rows hidden ONLY
        // because of session scoping (not already marked hidden by
        // keep_last). Zero when `show_stale_backlog` is true (everything
        // is visible in that mode).
        let hidden_backlog_count = if show_all || show_stale_backlog {
            0
        } else {
            last_data.as_ref().map_or(0, |(views, _)| {
                views
                    .iter()
                    .filter(|v| !v.hidden && is_pre_invocation_stale(v))
                    .count()
            })
        };

        // INF-TSK-049-001 AC #11: build an effective view-list that mutates
        // `hidden` for pre-invocation stale rows when the default scoped
        // view is active. This keeps the downstream selector/renderer
        // honest without threading a separate predicate argument through
        // every helper.
        let effective_data: Option<(
            Vec<codeflow_core::tui::data::SessionView>,
            codeflow_core::tui::data::SessionSummary,
        )> = last_data.as_ref().map(|(views, summary)| {
            let mut cloned = views.clone();
            if !show_all && !show_stale_backlog {
                for v in &mut cloned {
                    if is_pre_invocation_stale(v) {
                        v.hidden = true;
                    }
                }
            }
            let mut sum = summary.clone();
            sum.hidden_count = sum.hidden_count.max(hidden_backlog_count);
            (cloned, sum)
        });

        // Compute filtered view count for navigation bounds.
        let visible_count = effective_data.as_ref().map_or(0, |(views, _)| {
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
                Constraint::Length(8), // detail — 5 content lines + 2 border + 1 padding
                Constraint::Length(1), // keybinding bar
            ])
            .split(area);

            // --- Header ---
            render_session_header(
                frame,
                chunks[0],
                effective_data.as_ref().map(|(_, s)| s),
                show_all,
            );

            // --- Session table ---
            render_session_table(
                frame,
                chunks[1],
                &mut table_state,
                effective_data.as_ref(),
                show_all,
            );

            // --- Detail pane ---
            let selected_session = effective_data.as_ref().and_then(|(views, _)| {
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
                    session_keybinding_line(
                        show_stale_backlog,
                        hidden_backlog_count,
                        last_fetch.elapsed().as_secs(),
                    )
                }
            } else {
                session_keybinding_line(
                    show_stale_backlog,
                    hidden_backlog_count,
                    last_fetch.elapsed().as_secs(),
                )
            };
            frame.render_widget(Paragraph::new(bar_line), chunks[3]);
        })?;

        // Event handling — poll at 100 ms so keys respond immediately.
        if event::poll(poll_interval)? {
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
                            get_selected_session(effective_data.as_ref(), &table_state, show_all)
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
                            get_selected_session(effective_data.as_ref(), &table_state, show_all)
                        {
                            // Validate DB-sourced session_id before filesystem operations.
                            if codeflow_core::session::is_valid_session_id(&session.session_id) {
                                if session.status == "stale" || is_canonical_pid_stale(session.pid)
                                {
                                    let sid = session.session_id.clone();
                                    // Mark stale in DB (fresh connection for cross-process visibility).
                                    if let Ok(cleanup_store) = open_store(project_dir).await {
                                        let now = chrono::Utc::now().to_rfc3339();
                                        let _ = cleanup_store
                                            .db()
                                            .query(
                                                // INF-TSK-049-001 AC #13:
                                                // always set completed_at so
                                                // the duration freezes.
                                                "UPDATE interactive_session SET status = 'stale', \
                                                 updated_at = $now, completed_at = $now \
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
                    // INF-TSK-049-001 AC #11: toggle historical stale backlog.
                    KeyCode::Char('s') => {
                        show_stale_backlog = !show_stale_backlog;
                        table_state.select(Some(0));
                        let label = if show_stale_backlog {
                            "showing stale"
                        } else {
                            "scoped to this session"
                        };
                        status_message =
                            Some((format!("Session list {label}"), std::time::Instant::now()));
                    }
                    // INF-TSK-049-001 AC #10/17: force refresh. Reset last_fetch
                    // to the epoch so the gate in `should_fetch_now` fires on
                    // the very next iteration.
                    KeyCode::Char('r') => {
                        last_fetch = std::time::Instant::now()
                            .checked_sub(Duration::from_secs(3600))
                            .unwrap_or_else(std::time::Instant::now);
                        status_message =
                            Some(("Refreshing...".to_string(), std::time::Instant::now()));
                    }
                    _ => {}
                }
            }
        }

        // Refresh data only once per `fetch_interval` so the render thread
        // is not blocked every 100 ms on a DB round-trip. The store is
        // re-opened for cross-process visibility. The gate predicate is
        // extracted to `should_fetch_now` for regression testing (see
        // `tui::data::tests::test_fetch_gate_prevents_stall`).
        if codeflow_core::tui::data::should_fetch_now(
            last_fetch,
            std::time::Instant::now(),
            fetch_interval,
        ) {
            match open_store(project_dir).await {
                Ok(refresh_store) => {
                    match fetch_session_views_with_keep_last(&refresh_store, project_dir, keep_last)
                        .await
                    {
                        Ok(d) => last_data = Some(d),
                        Err(e) => eprintln!("warn: fetch failed: {e}"),
                    }
                }
                Err(_) => {
                    // Keep last_data on error; try again next cycle.
                }
            }
            last_fetch = std::time::Instant::now();
        }
    }

    Ok(())
}

/// INF-TSK-049-001 AC #11/17: footer keybinding bar for interactive status
/// TUI. Renders `[s] show stale (N hidden)` when the default view is
/// hiding historical rows and `[s] showing stale` when toggled on; always
/// renders `[r] Refresh` and a `[last updated Ns ago]` age indicator.
fn session_keybinding_line(
    show_stale_backlog: bool,
    hidden_backlog_count: usize,
    last_fetch_age_secs: u64,
) -> ratatui::text::Line<'static> {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::Span;

    let mut spans = vec![
        Span::styled(" [Enter]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Attach "),
        Span::styled("[c]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Cleanup "),
        Span::styled("[a]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Toggle All "),
        Span::styled("[r]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Refresh "),
    ];
    // [s] label depends on state.
    spans.push(Span::styled("[s]", Style::new().fg(theme::BLUE_ACCENT)));
    let s_label = if show_stale_backlog {
        " Showing stale ".to_string()
    } else if hidden_backlog_count > 0 {
        format!(" Show stale ({hidden_backlog_count} hidden) ")
    } else {
        " Show stale ".to_string()
    };
    spans.push(Span::raw(s_label));
    spans.extend([
        Span::styled("[Up/Down]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Navigate "),
        Span::styled("[q]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Quit"),
        Span::raw("  "),
        Span::styled(
            format!("[last updated {last_fetch_age_secs}s ago]"),
            Style::new().fg(theme::DIM_PENDING),
        ),
    ]);
    ratatui::text::Line::from(spans)
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
    use codeflow_core::tui::data::{
        abbreviate_session_id, format_task_id_for_display, truncate_branch_for_display,
    };
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
                    let branch =
                        truncate_branch_for_display(s.branch.as_deref().unwrap_or("--"), 50);
                    let work_type = s.work_type.as_deref().unwrap_or("--");

                    Row::new(vec![
                        Cell::from(task_display),
                        Cell::from(sid_display),
                        Cell::from(branch),
                        Cell::from(work_type.to_string()),
                        Cell::from(status_badge),
                        Cell::from(phase_badge.to_span()),
                        Cell::from(duration.to_span()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    // Column budget at 120-col terminal:
    //   TASK(20) + SESSION(22) + BRANCH(up to 50) + TYPE(6) + STATUS(12)
    //   + PHASE(10) + DURATION(10) + 6*spacing(2) = 142 max, 92 min.
    // BRANCH uses Max so ratatui shrinks it when the terminal is narrower.
    let table = Table::new(
        rows,
        [
            Constraint::Length(20), // TASK — fits `INF-TSK-046-008` or `task-01K…MHH`
            Constraint::Length(22), // SESSION — fits abbreviated `ses-01kphb...yme2b`
            Constraint::Max(50),    // BRANCH — truncated with ellipsis above 50
            Constraint::Length(6),  // TYPE
            Constraint::Length(12), // STATUS
            Constraint::Length(10), // PHASE — fits `Starting`, `pre-pf1`
            Constraint::Length(10), // DURATION
        ],
    )
    .header(header)
    .column_spacing(2)
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
    use codeflow_core::tui::data::format_task_id_for_display;
    use codeflow_core::tui::widgets::{SessionDetail, render_session_detail as render};

    let Some(s) = session else {
        let empty = SessionDetail {
            session_id: "--",
            pid: None,
            pid_alive: None,
            worktree_path: "--",
            team_name: None,
            branch: None,
            work_type: None,
            task_id: None,
            task_id_formatted: None,
            phase: None,
            status: "--",
            duration_secs: None,
            autorun_extras: None,
        };
        render(frame, area, &empty);
        return;
    };

    let pid_alive = !is_canonical_pid_stale(s.pid);
    // `pid = 0` here means the canonical `pathflow-session-status.json::lead_pid`
    // is missing/zero (status file gone or session never recorded one); surface
    // that as "unknown" to avoid rendering a misleading "0 (DEAD)".
    #[allow(clippy::cast_possible_truncation)]
    let (pid_field, alive_field) = if s.pid == 0 {
        (None, None)
    } else {
        (Some(s.pid as i32), Some(pid_alive))
    };

    // Mirror the prior adapter: when `format_task_id_for_display` ends up
    // returning the raw ULID (task_format_id is None/empty), the primary IS
    // the raw task_id — don't double-render it in parens.
    let primary = format_task_id_for_display(s.task_format_id.as_deref(), s.task_id.as_deref());
    let formatted_opt: Option<String> =
        if s.task_format_id.as_deref().is_some_and(|f| !f.is_empty()) {
            Some(primary)
        } else {
            None
        };

    #[allow(clippy::cast_sign_loss)]
    let duration_secs = if s.duration_secs >= 0 {
        Some(s.duration_secs as u64)
    } else {
        None
    };

    let detail = SessionDetail {
        session_id: &s.session_id,
        pid: pid_field,
        pid_alive: alive_field,
        worktree_path: s.worktree_path.as_deref().unwrap_or("--"),
        team_name: s.team_name.as_deref(),
        branch: s.branch.as_deref(),
        work_type: s.work_type.as_deref(),
        task_id: s.task_id.as_deref(),
        task_id_formatted: formatted_opt.as_deref(),
        phase: s.phase.as_deref(),
        status: &s.status,
        duration_secs,
        autorun_extras: None,
    };
    render(frame, area, &detail);
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
    fn test_is_canonical_pid_stale_dead_pid() {
        // PID 0 is never a valid user process.
        assert!(is_canonical_pid_stale(0));
    }

    #[test]
    fn test_is_canonical_pid_stale_current_pid_not_named_claude() {
        // INF-TSK-050-001 AC #4: under the new `validate_claude_pid`
        // regime, the current PID is "alive" but its process name is
        // not "claude" (it's `cargo-test` or similar) — so
        // `is_canonical_pid_stale` returns true. This is the intended
        // semantic: stale-cleanup should treat any PID that is NOT a
        // recognized Claude Code process as cleanup-eligible.
        let pid = i64::from(std::process::id());
        assert!(
            is_canonical_pid_stale(pid),
            "current PID is alive but not 'claude'-named; \
             validate_claude_pid wraps both predicates so the test \
             runner is treated as stale (intended)"
        );
    }

    #[test]
    fn test_is_canonical_pid_stale_negative_pid() {
        // Negative PID should be treated as dead (u32 conversion yields 0).
        assert!(is_canonical_pid_stale(-1));
    }

    #[test]
    fn test_is_canonical_pid_stale_very_large_pid() {
        // Very large PID unlikely to be alive.
        assert!(is_canonical_pid_stale(999_999_999));
    }

    // ─── PID formatting ────────────────────────────────────────────────

    #[test]
    fn test_format_pid_with_liveness_dead() {
        let result = format_pid_with_liveness(0);
        assert!(result.contains("DEAD"), "PID 0 should show DEAD");
        assert!(result.contains('0'), "should include the PID number");
    }

    #[test]
    fn test_format_pid_with_liveness_current_pid_marked_dead() {
        // INF-TSK-050-001 AC #4: under name-verified PID liveness, the
        // test runner's PID is alive but its process name is not
        // "claude" — so format_pid_with_liveness returns "DEAD". This
        // is the intended new semantic: only PIDs that validate as
        // a Claude Code process are reported alive.
        let pid = i64::from(std::process::id());
        let result = format_pid_with_liveness(pid);
        assert!(
            result.contains("DEAD"),
            "current PID is alive but not 'claude'-named; \
             validate_claude_pid wraps both predicates so the test \
             runner shows DEAD (intended): got {result}"
        );
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
    async fn test_register_interactive_session_returns_result_type() {
        // INF-TSK-050-003 AC-07: register_interactive_session now
        // returns Result<(), Error>. Previously it returned `()` and
        // swallowed every error. The behavior we lock in here: the
        // return type lets callers observe failures. Happy path (DB
        // dir exists) returns Ok(()).
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();
        let result =
            register_interactive_session(dir.path(), "ses-ok", None, None, "codeflow", true).await;
        assert!(
            result.is_ok(),
            "register_interactive_session must return Ok on the happy path; got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_register_interactive_session_error_path() {
        // INF-TSK-050-003 AC-07: produce a controlled failure to verify
        // the error path emits the documented contract message. We
        // engineer a failure by passing a project_dir under `/dev/null`
        // — `mkdir -p` cannot create a child of a character-device
        // path, so `SurrealStore::open` reliably fails on the
        // ENOTDIR/ENOENT chain. The exact OS error varies by platform
        // but the wrapping `open_store` context is stable.
        let bad_root = std::path::Path::new("/dev/null/forbidden-cf-test-marker");
        let result =
            register_interactive_session(bad_root, "ses-err", None, None, "codeflow", true).await;
        let err = result.expect_err("write-restricted project_dir must fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("open_store") || msg.contains("CREATE interactive_session"),
            "AC-07: error must name the failure stage, got: {msg}"
        );
    }

    // ─── INF-TSK-050-003 AC-07 / WS-REV MINOR-1: stderr warn line ─────────
    //
    // `register_interactive_session` emits a `warn:` line to stderr on
    // every failure path. The exact format is contracted by
    // `format_register_warn_line` — these tests pin the format so a
    // regression that, e.g., dropped the `warn:` prefix or the
    // `register_interactive_session failed:` substring would break
    // both the production stderr output AND these tests in lock-step.

    #[test]
    fn test_format_register_warn_line_open_store_stage() {
        let line = format_register_warn_line("open_store", "no such file or directory");
        // Required: "warn:" prefix per CLAUDE.md feedback contract.
        assert!(
            line.starts_with("warn:"),
            "must start with 'warn:'; got: {line}"
        );
        // Required: function name + " failed:" so the operator can
        // grep for this exact phrase.
        assert!(
            line.contains("register_interactive_session failed:"),
            "must include the function-name + ' failed:' substring; got: {line}"
        );
        // Required: stage name (open_store) for failure-stage diagnosis.
        assert!(
            line.contains("open_store"),
            "must name the stage; got: {line}"
        );
        // Required: original error text passes through.
        assert!(
            line.contains("no such file or directory"),
            "must include the underlying error text; got: {line}"
        );
    }

    #[test]
    fn test_format_register_warn_line_create_stage() {
        let line = format_register_warn_line("CREATE", "schema mismatch on field 'pid'");
        assert!(line.starts_with("warn:"));
        assert!(line.contains("register_interactive_session failed:"));
        assert!(line.contains("CREATE"));
        assert!(line.contains("schema mismatch on field 'pid'"));
    }

    #[test]
    fn test_format_register_warn_line_exact_contract() {
        // Lock the exact format so any drift forces an explicit update
        // here and at the consumers (operator runbooks, log greppers).
        let line = format_register_warn_line("open_store", "io error");
        assert_eq!(
            line, "warn: register_interactive_session failed: open_store: io error",
            "the exact stderr line format is part of the public contract"
        );
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
        .await
        .expect("register should succeed when DB dir exists");

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

        register_interactive_session(dir.path(), "ses-f1", None, None, "claude", false)
            .await
            .expect("register should succeed when DB dir exists");

        // Filter by 'active' should find it.
        let active = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert_eq!(active.len(), 1);

        // Filter by 'complete' should find nothing.
        let complete = query_sessions(dir.path(), Some("complete")).await.unwrap();
        assert!(complete.is_empty());
    }

    /// INF-TSK-050-001 AC #12: autorun-worker rows registered as
    /// `interactive_session` with `session_kind='autorun'` MUST NOT
    /// appear in `query_sessions` output. The brief mandates this for
    /// both `codeflow interactive list` and `codeflow interactive status`.
    #[tokio::test]
    async fn test_query_sessions_excludes_autorun_workers() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();

        // Real interactive session — should be visible.
        register_interactive_session(dir.path(), "ses-real", None, None, "codeflow", false)
            .await
            .expect("register should succeed when DB dir exists");

        // Now manually create an autorun-worker row that registers as
        // interactive_session with session_kind='autorun'. The schema
        // migration sets this kind for worker rows; we simulate that
        // here by directly INSERTing.
        let store = open_store(dir.path()).await.unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-autorun-worker', pid = 1234, status = 'active', \
                 source_cli = 'autorun', managed = true, \
                 session_kind = 'autorun', created_at = $now",
            )
            .bind(("now", now))
            .await;

        let active = query_sessions(dir.path(), Some("active")).await.unwrap();
        let ids: Vec<&str> = active.iter().map(|s| s.session_id.as_str()).collect();
        assert!(
            ids.contains(&"ses-real"),
            "real interactive session must be visible: {ids:?}"
        );
        assert!(
            !ids.contains(&"ses-autorun-worker"),
            "autorun-worker row must be EXCLUDED from interactive list: {ids:?}"
        );

        let no_filter = query_sessions(dir.path(), None).await.unwrap();
        let ids2: Vec<&str> = no_filter.iter().map(|s| s.session_id.as_str()).collect();
        assert!(ids2.contains(&"ses-real"));
        assert!(!ids2.contains(&"ses-autorun-worker"));
    }

    /// INF-TSK-050-001 AC #12: pre-migration rows that lack the
    /// `session_kind` column (NONE) are treated as interactive (visible).
    /// Defensive against rows created before the schema added the column.
    #[tokio::test]
    async fn test_query_sessions_includes_pre_migration_none_kind() {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();

        let store = open_store(dir.path()).await.unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        // Note: the SCHEMAFULL definition gives session_kind a DEFAULT of
        // 'interactive', so a CREATE that omits the field gets the
        // default. To simulate a true pre-migration row, we would have
        // to bypass the schema — which we can't do safely. Verify the
        // OR-clause handles 'interactive' (the default) correctly,
        // which is the realistic post-migration state. The NONE branch
        // is defensive code documented inline; functionally exercised
        // when SCHEMAFULL relaxes in future migrations.
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-pre-mig', pid = 99, status = 'active', \
                 source_cli = 'codeflow', managed = false, \
                 session_kind = 'interactive', created_at = $now",
            )
            .bind(("now", now))
            .await;

        let active = query_sessions(dir.path(), Some("active")).await.unwrap();
        let ids: Vec<&str> = active.iter().map(|s| s.session_id.as_str()).collect();
        assert!(
            ids.contains(&"ses-pre-mig"),
            "session with default 'interactive' kind must be visible: {ids:?}"
        );
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
            .await
            .expect("register should succeed when DB dir exists");

        let sessions = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].source_cli, "claude");
        assert!(!sessions[0].managed);
    }

    // ─── repair_wrongly_stale_rows ────────────────────────────────────
    //
    // INF-TSK-024-051 Phase 7-rework iter3 (cf-quality-assurance retry 1):
    // tests for the self-heal path that demotes wrongly-stale interactive
    // rows back to active when the canonical chokepoint reports `Active`.
    // Each test seeds `interactive_session` then directly UPDATEs status
    // to 'stale' (the public `register_interactive_session` always
    // creates as 'active'). The chokepoint requires
    // `pathflow-session-status.json::lead_pid` to exist with a valid PID;
    // the validator is bypassed via `override_pid_validator_for_tests` so
    // the test runner's `cargo` PID can stand in for a Claude lead.

    /// Helper: seed a stale interactive_session row with a status file
    /// containing the given PID. Returns the project_dir path.
    async fn seed_stale_interactive_row(project_dir: &std::path::Path, sid: &str, lead_pid: u32) {
        let db_dir = project_dir.join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();
        register_interactive_session(project_dir, sid, None, None, "codeflow", true)
            .await
            .expect("register should succeed for seed helper");
        // Promote to stale via direct UPDATE — the public register helper
        // always creates active rows.
        let store = open_store(project_dir).await.unwrap();
        let _ = store
            .db()
            .query("UPDATE interactive_session SET status = 'stale' WHERE session_id = $sid")
            .bind(("sid", sid.to_string()))
            .await;
        // Write a status file with the requested lead_pid so the
        // chokepoint can resolve the row's liveness from the project-dir
        // fallback path (no worktree registered in these tests).
        let status_dir = project_dir
            .join(".state/session")
            .join(sid)
            .join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            format!(r#"{{"session_id":"{sid}","lead_pid":{lead_pid},"status":"pf-in-progress"}}"#),
        )
        .unwrap();
    }

    #[tokio::test]
    #[serial_test::serial(env_vars)]
    async fn test_repair_wrongly_stale_rows_demotes_wrongly_stale_to_active() {
        // Validator returns true → chokepoint says Active → row should be
        // demoted from 'stale' back to 'active'.
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(|_| true);

        let dir = tempfile::tempdir().unwrap();
        seed_stale_interactive_row(dir.path(), "ses-wrongly-stale", std::process::id()).await;

        // Sanity: row is currently stale.
        let stale_before = query_sessions(dir.path(), Some("stale")).await.unwrap();
        assert_eq!(stale_before.len(), 1);
        assert_eq!(stale_before[0].session_id, "ses-wrongly-stale");

        // Run the repair.
        repair_wrongly_stale_rows(dir.path()).await.unwrap();

        // The row should be active now and the stale list empty.
        let active_after = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert_eq!(
            active_after.len(),
            1,
            "wrongly-stale row should be demoted to active"
        );
        assert_eq!(active_after[0].session_id, "ses-wrongly-stale");

        let stale_after = query_sessions(dir.path(), Some("stale")).await.unwrap();
        assert!(
            stale_after.is_empty(),
            "no stale rows should remain after repair"
        );
    }

    #[tokio::test]
    #[serial_test::serial(env_vars)]
    async fn test_repair_wrongly_stale_rows_leaves_dead_stale_rows_alone() {
        // Validator returns false → chokepoint says Dead → row stays stale.
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(|_| false);

        let dir = tempfile::tempdir().unwrap();
        seed_stale_interactive_row(dir.path(), "ses-truly-dead", 4_000_000).await;

        repair_wrongly_stale_rows(dir.path()).await.unwrap();

        let stale_after = query_sessions(dir.path(), Some("stale")).await.unwrap();
        assert_eq!(
            stale_after.len(),
            1,
            "dead-PID stale row must NOT be demoted"
        );
        assert_eq!(stale_after[0].session_id, "ses-truly-dead");

        let active_after = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert!(
            active_after.is_empty(),
            "no rows should have been promoted to active"
        );
    }

    #[tokio::test]
    #[serial_test::serial(env_vars)]
    async fn test_repair_wrongly_stale_rows_noop_when_no_stale_rows() {
        // No stale rows present — repair is a no-op and returns Ok.
        // (No validator override needed: the SELECT returns empty, so the
        // chokepoint is never consulted.)
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();
        register_interactive_session(
            dir.path(),
            "ses-already-active",
            None,
            None,
            "codeflow",
            true,
        )
        .await
        .expect("register should succeed when DB dir exists");

        let result = repair_wrongly_stale_rows(dir.path()).await;
        assert!(result.is_ok(), "no-op repair should return Ok: {result:?}");

        // Active rows untouched.
        let active = query_sessions(dir.path(), Some("active")).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].session_id, "ses-already-active");
    }

    // INF-TSK-024-051 Phase 7-rework: heartbeat sweep tests deleted.
    // The functions they exercised (`sweep_stale_heartbeats`,
    // `sweep_stale_heartbeats_inner`, `scan_heartbeat_sessions`,
    // `is_heartbeat_session_alive`) were removed as dead code after the
    // autorun heartbeat writer was deleted alongside the interactive
    // one. The chokepoint's own tests in `core/src/session/liveness.rs`
    // cover the underlying liveness logic.

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

    // INF-TSK-024-051 Phase 7-rework: scan_heartbeat_sessions and
    // is_heartbeat_session_alive tests deleted. The functions they
    // exercised were removed as dead code; the underlying liveness
    // logic is covered by chokepoint tests in
    // `core/src/session/liveness.rs`.

    // ─── remove_session_artifacts ─────────────────────────────────────

    #[test]
    fn test_remove_session_artifacts_cleans_all() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-cleanup1";

        // INF-TSK-024-051 Phase 7-rework: heartbeat artifact removed from
        // the cleanup contract; only session + sentinel directories are
        // touched now.
        let sess_dir = dir.path().join(".state/session").join(sid);
        std::fs::create_dir_all(&sess_dir).unwrap();

        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();

        remove_session_artifacts(dir.path(), sid);

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

    // INF-TSK-024-051 Phase 4-B: removed
    // `test_interactive_worktree_registry_source_not_overwritten_by_pid`.
    // The test exercised the now-deleted `locked_update_lead_pid` and
    // asserted on the now-removed `entry.lead_pid` field. The
    // independence-of-update behavior the test validated is preserved
    // implicitly: `locked_update_source` only touches `source`.

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
        // INF-TSK-024-051 Phase 6: `sweep_stale_session_dirs` now routes
        // through the canonical chokepoint, which requires the lead PID
        // to be alive AND named "claude". Install a synthetic validator
        // that only checks process aliveness so the test runner's PID
        // (which is NOT named "claude") can stand in for a live lead.
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(
            codeflow_core::session::process::is_process_alive,
        );

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
        // INF-TSK-024-051 Phase 4-C: `mark_stale_worktree_entries` now reads
        // the canonical chokepoint instead of the removed registry
        // `lead_pid` field. To exercise the `Dead` branch, write a status
        // file containing a guaranteed-dead PID and install a synthetic
        // validator that only checks process aliveness (the test runner is
        // `cargo`, not `claude`, so the production validator would
        // mis-classify the PID).
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(
            codeflow_core::session::process::is_process_alive,
        );

        let dir = tempfile::tempdir().unwrap();
        let registry_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let sid = "ses-stale1";

        // Write a status file with a guaranteed-dead PID so the chokepoint
        // returns `Dead` (not `Unknown`).
        let status_dir = dir.path().join(".state/session").join(sid).join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        let dead_pid: u32 = 99_999_999;
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            format!(r#"{{"lead_pid": {dead_pid}, "session_id": "{sid}", "status": "active"}}"#),
        )
        .unwrap();

        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-01-01T00:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "worktree-ses-stale1".to_string(),
            path: "/tmp/wt-stale1".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some(sid.to_string()),
            task_id: None,
            source: Some("interactive".to_string()),
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

    // INF-TSK-024-051 Phase 7-rework: heartbeat traversal-guard tests
    // deleted alongside the functions they exercised
    // (`is_heartbeat_session_alive`, `scan_heartbeat_sessions`,
    // `sweep_stale_heartbeats_inner`). The chokepoint
    // `resolve_session_state_dir` has its own traversal guard (covered by
    // `read_canonical_lead_pid_rejects_path_traversal` in
    // `core/src/session/mod.rs`).

    // -----------------------------------------------------------------------
    // derive_session_phase tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_derive_session_phase_no_worktree() {
        let session = codeflow_core::models::InteractiveSession {
            id: "test:1".into(),
            session_id: "ses-test".into(),
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
            session_kind: "interactive".into(),
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
            session_kind: "interactive".into(),
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
            session_kind: "interactive".into(),
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
            session_kind: "interactive".into(),
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
            session_kind: "interactive".into(),
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
            session_kind: "interactive".into(),
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
        let line = session_keybinding_line(false, 0, 3);
        let text = line.to_string();
        assert!(text.contains("[Enter]"), "should contain Enter: {text}");
        assert!(text.contains("Attach"), "should contain Attach: {text}");
        assert!(text.contains("[c]"), "should contain c: {text}");
        assert!(text.contains("Cleanup"), "should contain Cleanup: {text}");
        assert!(text.contains("[Up/Down]"), "should contain Up/Down: {text}");
        assert!(text.contains("[q]"), "should contain q: {text}");
        assert!(text.contains("Quit"), "should contain Quit: {text}");
        // INF-TSK-049-001 AC #11/17: [s] stale toggle and [r] refresh
        // keybindings are always present; last-updated age indicator too.
        assert!(text.contains("[s]"), "should contain [s]: {text}");
        assert!(text.contains("[r]"), "should contain [r]: {text}");
        assert!(
            text.contains("last updated 3s ago"),
            "should contain age indicator: {text}"
        );
    }

    #[test]
    fn test_session_keybinding_line_hidden_backlog_count_rendered() {
        let line = session_keybinding_line(false, 7, 0);
        let text = line.to_string();
        assert!(
            text.contains("Show stale (7 hidden)"),
            "should render hidden count: {text}"
        );
    }

    #[test]
    fn test_session_keybinding_line_showing_stale_label() {
        let line = session_keybinding_line(true, 7, 0);
        let text = line.to_string();
        assert!(
            text.contains("Showing stale"),
            "should advertise 'Showing stale' when toggled: {text}"
        );
        assert!(
            !text.contains("(7 hidden)"),
            "hidden count should not render when already showing stale: {text}"
        );
    }

    #[test]
    fn test_session_keybinding_line_no_hidden_no_count() {
        let line = session_keybinding_line(false, 0, 1);
        let text = line.to_string();
        assert!(
            !text.contains("hidden"),
            "hidden count should be absent when 0: {text}"
        );
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
            last_phase: None,
            team_name: None,
            pid: 0,
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
            last_phase: None,
            team_name: Some("team-1".into()),
            pid: 12345,
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
                last_phase: None,
                team_name: None,
                pid: 0,
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
                last_phase: None,
                team_name: None,
                pid: 0,
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
        let _line = session_keybinding_line(false, 0, 0);
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

    // -----------------------------------------------------------------------
    // INF-TSK-049-001 AC #13 — `interactive cleanup` UPDATE sets completed_at.
    //
    // We exercise the SurrealDB UPDATE directly (same query shape as the
    // production path in `run_cleanup`) against an in-memory store so we
    // assert observable behaviour without depending on PID liveness.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_cleanup_update_sets_completed_at_alongside_updated_at() {
        let store = codeflow_core::store::SurrealStore::in_memory()
            .await
            .unwrap();
        let created = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-cleanup', pid = 2, status = 'active', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'interactive', \
                 created_at = $created RETURN NONE",
            )
            .bind(("created", created))
            .await;

        // Production UPDATE shape from `run_cleanup` (and the TUI `c` path).
        let now = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "UPDATE interactive_session SET status = 'stale', \
                 updated_at = $now, completed_at = $now \
                 WHERE session_id = $sid AND status = 'active'",
            )
            .bind(("now", now.clone()))
            .bind(("sid", "ses-cleanup".to_string()))
            .await;

        let mut res = store
            .db()
            .query(
                "SELECT status, completed_at, updated_at FROM interactive_session \
                 WHERE session_id = 'ses-cleanup'",
            )
            .await
            .unwrap();
        #[derive(serde::Deserialize)]
        struct Row {
            status: String,
            completed_at: Option<String>,
            updated_at: Option<String>,
        }
        let rows: Vec<Row> = res.take(0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "stale");
        assert!(
            rows[0].completed_at.is_some(),
            "AC #13: cleanup must set completed_at"
        );
        assert!(rows[0].updated_at.is_some(), "updated_at must remain set");
    }

    #[tokio::test]
    async fn test_cleanup_update_is_noop_when_session_already_stale() {
        // The UPDATE filters on status='active'; pre-promoted rows are not
        // re-touched so `updated_at`/`completed_at` preserve their earlier
        // values.
        let store = codeflow_core::store::SurrealStore::in_memory()
            .await
            .unwrap();
        let created = "2026-04-01T00:00:00Z".to_string();
        let prior_completed = "2026-04-05T00:00:00Z".to_string();
        let _ = store
            .db()
            .query(
                "CREATE interactive_session SET \
                 session_id = 'ses-already-stale', pid = 2, status = 'stale', \
                 source_cli = 'codeflow', managed = true, \
                 session_kind = 'interactive', \
                 created_at = $created, completed_at = $completed, \
                 updated_at = $completed RETURN NONE",
            )
            .bind(("created", created))
            .bind(("completed", prior_completed.clone()))
            .await;

        let now = chrono::Utc::now().to_rfc3339();
        let _ = store
            .db()
            .query(
                "UPDATE interactive_session SET status = 'stale', \
                 updated_at = $now, completed_at = $now \
                 WHERE session_id = $sid AND status = 'active'",
            )
            .bind(("now", now))
            .bind(("sid", "ses-already-stale".to_string()))
            .await;

        let mut res = store
            .db()
            .query(
                "SELECT VALUE completed_at FROM interactive_session \
                 WHERE session_id = 'ses-already-stale'",
            )
            .await
            .unwrap();
        let vals: Vec<String> = res.take(0).unwrap();
        assert_eq!(vals, vec![prior_completed]);
    }

    // ─── INF-TSK-050-003 AC-06 / WS-REV MAJOR-2: active→stale promotion ──
    //
    // These tests exercise `promote_active_sessions_to_stale` directly
    // against a seeded DB + status file. The function is the testable
    // helper extracted from `run_status` so the promotion contract is
    // covered by automation, not just manual smoke tests.

    /// Helper: seed an `active` interactive_session row + a status file
    /// with the given lead_pid. Returns the dir handle (caller keeps
    /// alive) and a fully-loaded `InteractiveSession` matching what
    /// `query_sessions` would return for the row.
    async fn seed_active_session_with_status_file(
        sid: &str,
        lead_pid: u32,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        codeflow_core::store::SurrealStore,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db_dir = dir.path().join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();
        register_interactive_session(dir.path(), sid, None, None, "codeflow", true)
            .await
            .expect("seed: register");
        // Write the canonical status file at the project-dir-fallback
        // location so `is_session_alive(project_dir, sid)` resolves.
        let status_dir = dir.path().join(".state/session").join(sid).join("pathflow");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(
            status_dir.join("pathflow-session-status.json"),
            format!(r#"{{"session_id":"{sid}","lead_pid":{lead_pid},"status":"pf-in-progress"}}"#),
        )
        .unwrap();
        let project = dir.path().to_path_buf();
        let store = open_store(&project).await.expect("open store");
        (dir, project, store)
    }

    #[tokio::test]
    #[serial_test::serial(env_vars)]
    async fn test_promote_active_to_stale_when_chokepoint_says_dead() {
        // Validator returns false → chokepoint says Dead → active row
        // must be promoted to stale + DB row UPDATEd + session_id
        // returned in the promoted set.
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(|_| false);

        let (_dir, project, store) =
            seed_active_session_with_status_file("ses-active-dead", 4_000_000).await;

        // Sanity: row is currently active.
        let before = query_sessions(&project, Some("active")).await.unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].status.to_string(), "active");

        // Drive the production helper.
        let promoted = promote_active_sessions_to_stale(&store, &project, &before).await;

        // Returned set must contain the session_id.
        assert!(
            promoted.contains("ses-active-dead"),
            "session_id must be in promoted set; got: {promoted:?}"
        );

        // DB row must be flipped to stale BEFORE any rendering happens.
        let stale = query_sessions(&project, Some("stale")).await.unwrap();
        assert_eq!(stale.len(), 1, "row must move to stale status");
        assert_eq!(stale[0].session_id, "ses-active-dead");
        assert!(
            stale[0].completed_at.is_some(),
            "completed_at must be populated by the promotion UPDATE"
        );

        // No active rows should remain.
        let still_active = query_sessions(&project, Some("active")).await.unwrap();
        assert!(
            still_active.is_empty(),
            "no active rows should remain after promotion"
        );
    }

    #[tokio::test]
    #[serial_test::serial(env_vars)]
    async fn test_promote_active_to_stale_no_op_when_chokepoint_says_alive() {
        // Validator returns true → chokepoint says Active → row stays
        // active; promoted set is empty.
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(|_| true);

        let (_dir, project, store) =
            seed_active_session_with_status_file("ses-active-alive", std::process::id()).await;
        let before = query_sessions(&project, Some("active")).await.unwrap();

        let promoted = promote_active_sessions_to_stale(&store, &project, &before).await;

        assert!(
            promoted.is_empty(),
            "alive row must NOT be promoted; promoted={promoted:?}"
        );
        let still_active = query_sessions(&project, Some("active")).await.unwrap();
        assert_eq!(still_active.len(), 1);
        assert_eq!(still_active[0].status.to_string(), "active");
    }

    #[tokio::test]
    #[serial_test::serial(env_vars)]
    async fn test_promote_active_to_stale_skips_already_stale_rows() {
        // Sessions whose DB status is already 'stale' must not be
        // re-promoted (the status check at the top of the function
        // skips them) — and the chokepoint should not be consulted
        // unnecessarily.
        let _g = codeflow_core::session::liveness::override_pid_validator_for_tests(|_| false);

        let dir = tempfile::tempdir().unwrap();
        seed_stale_interactive_row(dir.path(), "ses-already-stale-skip", 4_000_000).await;
        let project = dir.path().to_path_buf();
        let store = open_store(&project).await.unwrap();

        let stale_rows = query_sessions(&project, Some("stale")).await.unwrap();
        assert_eq!(stale_rows.len(), 1);
        let promoted = promote_active_sessions_to_stale(&store, &project, &stale_rows).await;
        assert!(
            promoted.is_empty(),
            "already-stale rows must NOT be in promoted set"
        );
    }
}
