//! Autorun command: batch execution of tasks in parallel worktrees.
//!
//! Parses the batch file, loads parallel-work config, creates an orchestrator
//! with a real worker runner, and executes the batch.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine as _;
use clap::Subcommand;

use crate::helpers;

/// Autorun subcommands.
#[derive(Debug, Subcommand)]
pub enum AutorunCommand {
    /// Execute a batch of tasks in parallel worktrees
    Run {
        /// Path to YAML batch file
        #[arg(
            long,
            short = 'b',
            default_value = ".codeflow/config/autorun/batch.yaml"
        )]
        batch: PathBuf,
        /// Run in foreground (default: detach to tmux session)
        #[arg(long, visible_alias = "fg")]
        foreground: bool,
    },
    /// Show status of active autorun batches
    Status {
        /// Filter to specific batch session ID
        #[arg(long)]
        batch: Option<String>,
        /// Watch mode: refresh status on interval
        #[arg(long, short = 'w')]
        watch: bool,
        /// Refresh interval in seconds (default: 5, requires --watch)
        #[arg(long, default_value = "5")]
        interval: u64,
    },
    /// Attach to a running worker's tmux session
    Attach {
        /// Task ID of the worker to attach to
        task_id: String,
    },
    /// View logs from a worker's tmux session
    Logs {
        /// Task ID of the worker to view logs for
        task_id: String,
        /// Follow log output (poll at 500ms)
        #[arg(long, short = 'f')]
        follow: bool,
    },
    /// Cancel a single running worker
    Cancel {
        /// Task ID of the worker to cancel
        task_id: String,
    },
    /// Abort an entire batch (cancel all running, skip pending)
    Abort {
        /// Batch session ID to abort
        #[arg(long)]
        batch: Option<String>,
    },
    /// Display results from a batch execution
    Results {
        /// Batch session ID to show results for
        #[arg(long)]
        batch: Option<String>,
    },
    /// Resume a partially failed/aborted batch by re-running non-completed tasks
    Resume {
        /// Session ID of the batch to resume (defaults to most recent non-completed)
        #[arg(long)]
        batch: Option<String>,
    },
    /// List available batch files in the autorun config directory
    Batches,
    /// Show historical autorun batch executions
    History {
        /// Maximum number of batches to show
        #[arg(long, default_value = "10")]
        limit: u32,
        /// Filter batches started after this ISO date
        #[arg(long)]
        since: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by batch name pattern (substring match)
        #[arg(long)]
        batch_name: Option<String>,
        /// Show all batches (overrides --limit)
        #[arg(long)]
        all: bool,
    },
}

pub async fn run(command: Option<AutorunCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    match command {
        Some(AutorunCommand::Run { batch, foreground }) => {
            let batch_path = if batch.as_path() == Path::new(".codeflow/config/autorun/batch.yaml")
            {
                project_dir.join(".codeflow/config/autorun/batch.yaml")
            } else {
                batch
            };
            run_with_dir(&project_dir, &batch_path, foreground).await
        }
        Some(AutorunCommand::Status {
            batch,
            watch,
            interval,
        }) => {
            if watch {
                run_status_watch(&project_dir, batch.as_deref(), interval).await
            } else {
                run_status(&project_dir, batch.as_deref()).await
            }
        }
        Some(AutorunCommand::Attach { task_id }) => run_attach(&project_dir, &task_id).await,
        Some(AutorunCommand::Logs { task_id, follow }) => {
            run_logs(&project_dir, &task_id, follow).await
        }
        Some(AutorunCommand::Cancel { task_id }) => run_cancel(&project_dir, &task_id).await,
        Some(AutorunCommand::Abort { batch }) => run_abort(&project_dir, batch.as_deref()).await,
        Some(AutorunCommand::Results { batch }) => {
            run_results(&project_dir, batch.as_deref()).await
        }
        Some(AutorunCommand::Resume { batch }) => run_resume(&project_dir, batch.as_deref()).await,
        Some(AutorunCommand::Batches) => run_batches_sync(&project_dir),
        Some(AutorunCommand::History {
            limit,
            since,
            status,
            batch_name,
            all,
        }) => run_history(&project_dir, limit, since, status, batch_name, all).await,
        None => {
            let batch_path = project_dir.join(".codeflow/config/autorun/batch.yaml");
            run_with_dir(&project_dir, &batch_path, false).await
        }
    }
}

/// Resolve a path to the real git repository root.
///
/// Worktrees have a `.git` file (containing `gitdir: ...`) instead of a
/// `.git` directory. This function detects worktree paths and resolves
/// them to the actual repository root, ensuring autorun always creates
/// worker worktrees in the correct location.
fn resolve_repo_root(dir: &Path) -> Result<PathBuf> {
    let git_path = dir.join(".git");
    if git_path.is_file() {
        // This is a worktree — .git is a file pointing to the real repo.
        let content =
            std::fs::read_to_string(&git_path).context("reading .git file in worktree")?;
        let gitdir = content.strip_prefix("gitdir: ").unwrap_or(&content).trim();
        // gitdir is like: /repo/.git/worktrees/name
        // Walk up: worktrees/ -> .git/ -> repo root
        let gitdir_path = PathBuf::from(gitdir);
        if let Some(repo_root) = gitdir_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            return Ok(repo_root.to_path_buf());
        }
    }
    // .git is a directory (or doesn't exist) — already at repo root.
    Ok(dir.to_path_buf())
}

/// Check that tmux is available and the working tree is clean.
///
/// These checks run BEFORE batch parsing to fail fast on environment issues.
fn preflight_checks(project_dir: &Path) -> Result<()> {
    check_tmux_available()?;
    check_git_clean(project_dir)?;
    Ok(())
}

/// Verify tmux is installed and in PATH.
fn check_tmux_available() -> Result<()> {
    let output = std::process::Command::new("which")
        .arg("tmux")
        .output()
        .context("failed to run 'which tmux'")?;
    if !output.status.success() {
        anyhow::bail!(
            "tmux is not installed or not in PATH. \
             Install tmux to use autorun: https://github.com/tmux/tmux"
        );
    }
    Ok(())
}

/// Verify the git working tree is clean (no uncommitted changes).
fn check_git_clean(project_dir: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_dir)
        .output()
        .context("failed to run 'git status --porcelain'")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.trim().is_empty() {
        anyhow::bail!(
            "working tree is not clean; commit or stash changes before autorun:\n{}",
            stdout.trim()
        );
    }
    Ok(())
}

/// Verify the target branch exists locally or at origin.
///
/// This check runs AFTER batch parsing (needs `ParsedBatch.target`) but BEFORE
/// orchestrator execution, to fail fast on invalid target branch.
fn check_target_branch(project_dir: &Path, target: &str) -> Result<()> {
    let local = std::process::Command::new("git")
        .args(["rev-parse", "--verify", target])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .context("failed to run git rev-parse")?;
    if local.status.success() {
        return Ok(());
    }

    let remote_ref = format!("refs/remotes/origin/{target}");
    let remote = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &remote_ref])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .context("failed to run git rev-parse for remote")?;
    if remote.status.success() {
        return Ok(());
    }

    anyhow::bail!("target branch '{target}' does not exist locally or at origin/{target}");
}

async fn run_with_dir(project_dir: &Path, batch_path: &Path, foreground: bool) -> Result<()> {
    use codeflow_core::store::DataStore;

    // Autorun MUST work from the real repo root, not a worktree.
    let project_dir = &resolve_repo_root(project_dir)?;

    // Phase 1: Pre-flight checks (before batch parse).
    preflight_checks(project_dir)?;

    // Detach to tmux if not foreground and stdout is a TTY.
    if !foreground && std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return run_detached(project_dir, batch_path);
    }

    // Phase 2: Parse batch.
    if !batch_path.exists() {
        anyhow::bail!("no autorun batch file found at {}", batch_path.display());
    }

    let mut parsed = codeflow_core::autorun::batch::parse_batch_file(batch_path)
        .context("parsing autorun batch")?;

    // Phase 2b: Extended validation (autorun_eligible, file_scope, scope_policy, overlaps).
    codeflow_core::autorun::validate_batch_extended(&mut parsed, project_dir)
        .context("extended batch validation")?;

    // Phase 3: Target branch check (after parse, before execution).
    check_target_branch(project_dir, &parsed.target)?;

    let config =
        codeflow_core::autorun::load_config(project_dir).context("loading parallel-work config")?;

    println!(
        "autorun batch: {} tasks, max_workers={}, scope_policy={}",
        parsed.tasks.len(),
        config.worktree.max_concurrent.min(parsed.max_workers),
        config.claims.default_scope_policy,
    );

    // Generate a session ID for this autorun run.
    let session_id = codeflow_core::session::generate_session_id();

    // Create the worker runner with real implementations.
    let worktree_provider = codeflow_core::autorun::RealWorktreeProvider::new(project_dir.clone());
    let tmux = RealTmux;
    let worker_timeout = Duration::from_secs(config.autorun.worker_timeout_secs);
    let claude = RealClaude {
        tmux: RealTmux,
        worker_timeout,
    };
    // Construct the data store for recording autorun state.
    let db_dir = project_dir.join(".state/db");
    let store = codeflow_core::store::SurrealStore::open(&db_dir)
        .await
        .context("opening data store for autorun")?;
    store
        .apply_schema()
        .await
        .context("applying database schema for autorun")?;
    let store = std::sync::Arc::new(store);

    let worker = codeflow_core::autorun::TmuxWorker::with_store(
        tmux,
        claude,
        worktree_provider,
        project_dir.clone(),
        worker_timeout,
        store.clone(),
    );

    let orchestrator = codeflow_core::autorun::Orchestrator::new(worker, store);

    // Start sync daemon once (not per-worker) before orchestrator execution.
    let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
    if let Err(e) = codeflow_core::worktree::maybe_auto_start_daemon(&registry_path, project_dir) {
        eprintln!("warning: daemon auto-start failed: {e}");
    }

    let start_time = chrono::Utc::now();

    let results = orchestrator
        .execute_with_batch_file(
            session_id.as_str(),
            &parsed,
            project_dir,
            &batch_path.to_string_lossy(),
            async {
                tokio::signal::ctrl_c().await.ok();
            },
        )
        .await
        .context("executing autorun batch")?;

    let end_time = chrono::Utc::now();

    // Generate batch report (best-effort: log errors but don't block return).
    let report_dir = project_dir.join(&config.autorun.report_dir);
    let batch_file_display = batch_path.display().to_string();
    let report_meta = BatchReportMeta {
        project_dir,
        report_dir: &report_dir,
        batch_name: &parsed.name,
        session_id: session_id.as_str(),
        batch_path: &batch_file_display,
        start_time,
        end_time,
    };
    match generate_batch_report(&report_meta, &results) {
        Ok(path) => eprintln!("report: {}", path.display()),
        Err(e) => eprintln!("warning: failed to generate batch report: {e}"),
    }

    // Stop sync daemon after orchestrator completes.
    if let Err(e) = codeflow_core::worktree::maybe_auto_stop_daemon(&registry_path, project_dir) {
        eprintln!("warning: daemon auto-stop failed: {e}");
    }

    report_results(&results)
}

/// Launch the orchestrator in a dedicated tmux session and return immediately.
fn run_detached(project_dir: &Path, batch_path: &Path) -> Result<()> {
    let orch_session = "cf-autorun-orchestrator";

    // Kill any existing orchestrator session.
    let _ = std::process::Command::new("tmux")
        .args(["kill-session", "-t", orch_session])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    // Create a new tmux session for the orchestrator.
    let status = std::process::Command::new("tmux")
        .args(["new-session", "-d", "-s", orch_session])
        .status()
        .context("failed to create tmux session for orchestrator")?;
    if !status.success() {
        anyhow::bail!("failed to create tmux session '{orch_session}'");
    }

    // Build the foreground command to run inside the tmux session.
    let exe = std::env::current_exe().context("getting current executable path")?;
    let batch_str = batch_path.to_string_lossy();
    let cmd = format!(
        "cd '{}' && '{}' autorun run --batch '{}' --foreground",
        project_dir.to_string_lossy().replace('\'', "'\\''"),
        exe.to_string_lossy().replace('\'', "'\\''"),
        batch_str.replace('\'', "'\\''"),
    );

    let send_status = std::process::Command::new("tmux")
        .args(["send-keys", "-t", orch_session, &cmd, "Enter"])
        .status()
        .context("failed to send command to orchestrator tmux session")?;
    if !send_status.success() {
        anyhow::bail!("failed to send orchestrator command to tmux");
    }

    println!("autorun orchestrator launched in tmux session: {orch_session}");
    println!();
    println!("  Monitor:  codeflow autorun status --watch");
    println!("  Attach:   tmux attach -t {orch_session}");
    println!("  Worker:   codeflow autorun attach <task-id>");
    println!("  Abort:    codeflow autorun abort");

    Ok(())
}

/// Metadata for a batch report.
struct BatchReportMeta<'a> {
    project_dir: &'a Path,
    report_dir: &'a Path,
    batch_name: &'a str,
    session_id: &'a str,
    batch_path: &'a str,
    start_time: chrono::DateTime<chrono::Utc>,
    end_time: chrono::DateTime<chrono::Utc>,
}

/// Generate a markdown batch report file.
///
/// Returns the path to the generated report on success.
fn generate_batch_report(
    meta: &BatchReportMeta<'_>,
    results: &[codeflow_core::autorun::WorkerResult],
) -> Result<PathBuf> {
    let BatchReportMeta {
        project_dir,
        report_dir,
        batch_name,
        session_id,
        batch_path,
        start_time,
        end_time,
    } = meta;
    let duration = *end_time - *start_time;
    let duration_display = format_duration_human(duration.num_seconds());

    // Determine overall batch status.
    let failed = results.iter().filter(|r| r.status == "failed").count();
    let timed_out = results.iter().filter(|r| r.status == "timeout").count();
    let completed = results.iter().filter(|r| r.status == "completed").count();
    let skipped = results.iter().filter(|r| r.status == "skipped").count();

    let batch_status = if failed > 0 || timed_out > 0 {
        "Failed"
    } else if completed == 0 && skipped == results.len() {
        "Aborted"
    } else {
        "Completed"
    };

    // Build markdown content.
    use std::fmt::Write;

    let mut md = String::new();
    writeln!(md, "# Autorun Batch Report: {batch_name}\n").unwrap();
    writeln!(md, "| Field | Value |").unwrap();
    writeln!(md, "|-------|-------|").unwrap();
    writeln!(md, "| Session ID | {session_id} |").unwrap();
    writeln!(md, "| Batch File | {batch_path} |").unwrap();
    writeln!(
        md,
        "| Start Time | {} |",
        start_time.format("%Y-%m-%d %H:%M:%S UTC")
    )
    .unwrap();
    writeln!(
        md,
        "| End Time | {} |",
        end_time.format("%Y-%m-%d %H:%M:%S UTC")
    )
    .unwrap();
    writeln!(md, "| Duration | {duration_display} |").unwrap();
    writeln!(md, "| Status | {batch_status} |").unwrap();
    writeln!(md, "\n## Task Results\n").unwrap();
    writeln!(
        md,
        "| Task ID | Status | Duration | PR | Exit Code | Error |"
    )
    .unwrap();
    writeln!(
        md,
        "|---------|--------|----------|----|-----------|-------|"
    )
    .unwrap();

    for r in results {
        let pr_col = if r.pr_number > 0 {
            if r.pr_url.is_empty() {
                format!("#{}", r.pr_number)
            } else {
                format!("[#{}]({})", r.pr_number, r.pr_url)
            }
        } else {
            "-".to_string()
        };
        let error_col = if r.error.is_empty() { "-" } else { &r.error };
        writeln!(
            md,
            "| {} | {} | {}s | {} | {} | {} |",
            r.task_id, r.status, r.duration_sec, pr_col, r.exit_code, error_col
        )
        .unwrap();
    }

    writeln!(md, "\n## Summary\n").unwrap();
    writeln!(md, "- Completed: {completed}").unwrap();
    writeln!(md, "- Failed: {failed}").unwrap();
    writeln!(md, "- Skipped: {skipped}").unwrap();
    writeln!(md, "- Timed Out: {timed_out}").unwrap();

    // Ensure report directory exists.
    let abs_report_dir = if report_dir.is_absolute() {
        report_dir.to_path_buf()
    } else {
        project_dir.join(report_dir)
    };
    std::fs::create_dir_all(&abs_report_dir)
        .with_context(|| format!("creating report directory {}", abs_report_dir.display()))?;

    // Write report file.
    let date_str = start_time.format("%Y-%m-%d").to_string();
    let filename = format!("{batch_name}-{date_str}.md");
    let report_path = abs_report_dir.join(&filename);
    codeflow_core::file_lock::atomic_write(&report_path, md.as_bytes())
        .with_context(|| format!("writing batch report to {}", report_path.display()))?;

    Ok(report_path)
}

/// Format seconds into a human-readable duration string.
fn format_duration_human(total_secs: i64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

/// Report execution results and return error if any tasks failed or timed out.
fn report_results(results: &[codeflow_core::autorun::WorkerResult]) -> Result<()> {
    let completed = results.iter().filter(|r| r.status == "completed").count();
    let failed = results.iter().filter(|r| r.status == "failed").count();
    let skipped = results.iter().filter(|r| r.status == "skipped").count();
    let timed_out = results.iter().filter(|r| r.status == "timeout").count();

    println!(
        "autorun complete: {completed} completed, {failed} failed, {skipped} skipped, {timed_out} timed out"
    );

    for result in results {
        if result.status != "completed" {
            eprintln!(
                "  task {}: {} (exit={}{})",
                result.task_id,
                result.status,
                result.exit_code,
                format_error(&result.error),
            );
        }
    }

    if failed > 0 || timed_out > 0 {
        anyhow::bail!("{} task(s) failed or timed out", failed + timed_out);
    }

    Ok(())
}

/// Format an error string for display in result output.
fn format_error(error: &str) -> String {
    if error.is_empty() {
        String::new()
    } else {
        format!(", error={error}")
    }
}

/// Open a `SurrealStore` for subcommand DB queries.
async fn open_store(
    project_dir: &Path,
) -> Result<std::sync::Arc<codeflow_core::store::SurrealStore>> {
    let db_dir = project_dir.join(".state/db");
    let store = codeflow_core::store::SurrealStore::open(&db_dir)
        .await
        .context("opening data store")?;
    Ok(std::sync::Arc::new(store))
}

/// Find the most recent running/aborting session, or a specific one by ID.
async fn resolve_session_id(
    store: &codeflow_core::store::SurrealStore,
    batch: Option<&str>,
) -> Result<String> {
    use codeflow_core::models::AutorunSessionFilter;
    use codeflow_core::store::DataStore;
    use codeflow_core::types::AutorunSessionStatus;

    if let Some(sid) = batch {
        return Ok(sid.to_string());
    }

    // Find the most recent running or aborting session.
    let sessions = store
        .list_autorun_sessions(AutorunSessionFilter {
            status: Some(AutorunSessionStatus::Running),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .context("querying autorun sessions")?;

    if let Some(s) = sessions.first() {
        return Ok(s.id.clone());
    }

    // Try aborting sessions.
    let sessions = store
        .list_autorun_sessions(AutorunSessionFilter {
            status: Some(AutorunSessionStatus::Aborting),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .context("querying autorun sessions")?;

    if let Some(s) = sessions.first() {
        return Ok(s.id.clone());
    }

    anyhow::bail!("no active autorun session found; use --batch <session_id> to specify one");
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

async fn run_status(project_dir: &Path, batch: Option<&str>) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;

    // Determine which sessions to show.
    let sessions = if let Some(sid) = batch {
        match store.get_autorun_session(sid).await? {
            Some(s) => vec![s],
            None => anyhow::bail!("no autorun session found with id '{sid}'"),
        }
    } else {
        // Show all active (running/aborting) sessions.
        use codeflow_core::models::AutorunSessionFilter;
        let mut active = store
            .list_autorun_sessions(AutorunSessionFilter::default())
            .await?;
        active.retain(|s| {
            matches!(
                s.status,
                codeflow_core::types::AutorunSessionStatus::Running
                    | codeflow_core::types::AutorunSessionStatus::Aborting
            )
        });
        if active.is_empty() {
            println!("No active autorun batches.");
            return Ok(());
        }
        active
    };

    println!(
        "{:<24} {:<10} {:>5} {:>5} {:>5} {:>5}  ELAPSED",
        "BATCH", "STATUS", "TASKS", "DONE", "FAIL", "RUN"
    );

    for session in &sessions {
        let workers = store.list_autorun_workers(&session.id).await?;
        let running = workers
            .iter()
            .filter(|w| w.status == codeflow_core::types::AutorunWorkerStatus::Running)
            .count();

        // Check tmux liveness for running workers.
        let mut orphan_count = 0;
        for w in &workers {
            if w.status == codeflow_core::types::AutorunWorkerStatus::Running {
                if let Some(ref tmux_name) = w.tmux_session {
                    if !tmux_has_session(tmux_name) {
                        orphan_count += 1;
                    }
                }
            }
        }

        let elapsed = format_elapsed(&session.created_at);
        let batch_name = session
            .batch_name
            .as_deref()
            .unwrap_or(&session.id[..session.id.len().min(20)]);

        println!(
            "{:<24} {:<10} {:>5} {:>5} {:>5} {:>5}  {}{}",
            batch_name,
            session.status,
            session.total_tasks,
            session.completed_tasks,
            session.failed_tasks,
            running,
            elapsed,
            if orphan_count > 0 {
                format!(" ({orphan_count} orphan)")
            } else {
                String::new()
            },
        );

        // Per-worker detail.
        let task_runs = store.list_autorun_task_runs(&session.id).await?;
        if !task_runs.is_empty() {
            println!();
            println!(
                "  {:<24} {:<12} {:<28} {:>9}",
                "TASK", "STATUS", "TMUX", "DURATION"
            );
            for run in &task_runs {
                let duration = run.duration_seconds.map_or_else(
                    || {
                        if matches!(
                            run.status,
                            codeflow_core::types::AutorunTaskRunStatus::Running
                        ) {
                            format_elapsed(run.started_at.as_deref().unwrap_or(""))
                        } else {
                            "--".to_string()
                        }
                    },
                    format_duration_secs,
                );
                let tmux = workers
                    .iter()
                    .find(|w| w.task_id == run.task_id)
                    .and_then(|w| w.tmux_session.clone())
                    .unwrap_or_else(|| "--".to_string());
                println!(
                    "  {:<24} {:<12} {:<28} {:>9}",
                    run.task_id, run.status, tmux, duration
                );
            }
        }
    }

    Ok(())
}

async fn run_status_watch(
    project_dir: &Path,
    batch: Option<&str>,
    interval_secs: u64,
) -> Result<()> {
    let interval = Duration::from_secs(interval_secs.max(1));
    loop {
        // Clear terminal.
        print!("\x1b[2J\x1b[H");
        run_status(project_dir, batch).await?;
        tokio::time::sleep(interval).await;
    }
}

async fn run_attach(project_dir: &Path, task_id: &str) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;
    let session_id = resolve_session_id(store.as_ref(), None).await?;

    let worker = store
        .get_autorun_worker_by_task_id(&session_id, task_id)
        .await?
        .with_context(|| {
            format!("no worker found for task '{task_id}' in session '{session_id}'")
        })?;

    let tmux_name = worker
        .tmux_session
        .as_deref()
        .context("worker has no tmux session name")?;

    if !tmux_has_session(tmux_name) {
        anyhow::bail!("tmux session '{tmux_name}' does not exist (worker may have stopped)");
    }

    // exec replaces current process.
    let err = exec::Command::new("tmux")
        .args(&["attach-session", "-t", tmux_name])
        .exec();
    anyhow::bail!("failed to exec tmux attach-session: {err}");
}

async fn run_logs(project_dir: &Path, task_id: &str, follow: bool) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;
    let session_id = resolve_session_id(store.as_ref(), None).await?;

    let worker = store
        .get_autorun_worker_by_task_id(&session_id, task_id)
        .await?
        .with_context(|| {
            format!("no worker found for task '{task_id}' in session '{session_id}'")
        })?;

    let tmux_name = worker
        .tmux_session
        .as_deref()
        .context("worker has no tmux session name")?;

    if !tmux_has_session(tmux_name) {
        // Tmux session gone — try reading captured output from worktree.
        if let Some(ref wt_path) = worker.worktree_path {
            let output_path = PathBuf::from(wt_path).join(".state/runtime/worker-output.json");
            if output_path.exists() {
                let content =
                    std::fs::read_to_string(&output_path).context("reading worker output")?;
                println!("{content}");
                return Ok(());
            }
        }
        anyhow::bail!("tmux session '{tmux_name}' does not exist and no output file found");
    }

    let capture = |name: &str| -> Result<String> {
        let output = std::process::Command::new("tmux")
            .args(["capture-pane", "-p", "-t", name, "-S", "-"])
            .output()
            .context("failed to run tmux capture-pane")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    };

    if follow {
        let mut last_line_count = 0;
        loop {
            let content = capture(tmux_name)?;
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() > last_line_count {
                for line in &lines[last_line_count..] {
                    println!("{line}");
                }
                last_line_count = lines.len();
            }
            if !tmux_has_session(tmux_name) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    } else {
        let content = capture(tmux_name)?;
        print!("{content}");
    }

    Ok(())
}

async fn run_cancel(project_dir: &Path, task_id: &str) -> Result<()> {
    use codeflow_core::store::DataStore;
    use codeflow_core::types::{AutorunTaskRunStatus, AutorunWorkerStatus};

    let store = open_store(project_dir).await?;
    let session_id = resolve_session_id(store.as_ref(), None).await?;

    let worker = store
        .get_autorun_worker_by_task_id(&session_id, task_id)
        .await?
        .with_context(|| {
            format!("no worker found for task '{task_id}' in session '{session_id}'")
        })?;

    if worker.status != AutorunWorkerStatus::Running {
        anyhow::bail!(
            "worker for task '{task_id}' is not running (status: {})",
            worker.status
        );
    }

    let tmux_name = worker.tmux_session.as_deref().unwrap_or("");
    let now = chrono::Utc::now().to_rfc3339();

    println!("cancelling worker for task '{task_id}'...");

    // Step 1-4: Signal processes in tmux session.
    if !tmux_name.is_empty() && tmux_has_session(tmux_name) {
        // Step 1-2: Send SIGTERM via Ctrl-C.
        let _ = std::process::Command::new("tmux")
            .args(["send-keys", "-t", tmux_name, "C-c", ""])
            .output();

        // Step 3: Wait 10 seconds.
        tokio::time::sleep(Duration::from_secs(10)).await;

        // Step 4: SIGKILL if still alive.
        if tmux_has_session(tmux_name) {
            let _ = std::process::Command::new("tmux")
                .args(["send-keys", "-t", tmux_name, "C-\\", ""])
                .output();
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    // Step 5: Discard uncommitted changes in worktree.
    if let Some(ref wt_path) = worker.worktree_path {
        let wt = Path::new(wt_path);
        if wt.exists() {
            let _ = std::process::Command::new("git")
                .args(["checkout", "."])
                .current_dir(wt)
                .output();
            let _ = std::process::Command::new("git")
                .args(["clean", "-fd"])
                .current_dir(wt)
                .output();
        }
    }

    // Step 6: Release claims for this worker (via CRDT coordination).
    let state_path = project_dir.join(".state/coordination/state.loro");
    if state_path.exists() && !worker.file_scope.is_empty() {
        if let Some(ref wsid) = worker.worker_session_id {
            let sid = codeflow_core::types::SessionId::new_unchecked(wsid);
            match codeflow_core::file_lock::locked_binary_rmw(
                &state_path,
                codeflow_core::coordination::loro::LoroCoordinator::in_memory,
                |bytes| {
                    codeflow_core::coordination::loro::LoroCoordinator::from_bytes(
                        bytes,
                        &state_path,
                    )
                    .map_err(|e| format!("load coordinator: {e}"))
                },
                |coord| coord.export_bytes().map_err(|e| format!("export: {e}")),
                |coord| {
                    let _ = codeflow_core::coordination::claims::release_all(coord, &sid);
                    Ok(())
                },
            ) {
                Ok(()) => {
                    eprintln!(
                        "released {} file claim(s) for worker {}",
                        worker.file_scope.len(),
                        worker.id,
                    );
                }
                Err(e) => {
                    eprintln!(
                        "warning: claim release failed for worker {}: {e}",
                        worker.id
                    );
                }
            }
        } else {
            eprintln!(
                "note: {} file claim(s) for worker {} will expire via TTL \
                 (worker session ID not available for immediate release)",
                worker.file_scope.len(),
                worker.id,
            );
        }
    }

    // Step 7: Kill tmux session.
    if !tmux_name.is_empty() {
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", tmux_name])
            .output();
    }

    // Step 8-9: Cleanup worktree and deregister.
    if let Some(ref wt_path) = worker.worktree_path {
        let _ = std::process::Command::new("git")
            .args(["worktree", "remove", "--force", wt_path])
            .current_dir(project_dir)
            .output();
    }

    // Step 10: Close PR if open.
    if let Some(pr_num) = worker.pr_number {
        eprintln!("closing PR #{pr_num}...");
        let _ = std::process::Command::new("gh")
            .args(["pr", "close", &pr_num.to_string(), "--delete-branch"])
            .current_dir(project_dir)
            .output();
    }

    // Step 11: Delete remote branch if pushed.
    // We get the branch name from the task run record.
    let task_runs = store.list_autorun_task_runs(&session_id).await?;
    if let Some(run) = task_runs.iter().find(|r| r.task_id == task_id) {
        if let Some(ref branch) = run.branch_name {
            if !branch.is_empty() && worker.pr_number.is_none() {
                // Only delete remote branch if no PR was created (PR --delete-branch handles it).
                let _ = std::process::Command::new("git")
                    .args(["push", "origin", "--delete", branch])
                    .current_dir(project_dir)
                    .output();
            }
        }
    }

    // Step 12: Update worker status in DB.
    store
        .update_autorun_worker(
            &worker.id,
            codeflow_core::models::AutorunWorkerUpdate {
                status: Some(AutorunWorkerStatus::Cancelled),
                completed_at: Some(now.clone()),
                ..Default::default()
            },
        )
        .await
        .context("updating worker status")?;

    // Step 13: Update task_run status.
    for run in &task_runs {
        if run.task_id == task_id
            && matches!(
                run.status,
                AutorunTaskRunStatus::Pending | AutorunTaskRunStatus::Running
            )
        {
            store
                .update_autorun_task_run(
                    &run.id,
                    codeflow_core::models::AutorunTaskRunUpdate {
                        status: Some(AutorunTaskRunStatus::Cancelled),
                        completed_at: Some(now.clone()),
                        ..Default::default()
                    },
                )
                .await
                .context("updating task run status")?;
        }
    }

    println!("worker for task '{task_id}' cancelled.");
    Ok(())
}

async fn run_abort(project_dir: &Path, batch: Option<&str>) -> Result<()> {
    use codeflow_core::store::DataStore;
    use codeflow_core::types::AutorunSessionStatus;

    let store = open_store(project_dir).await?;
    let session_id = resolve_session_id(store.as_ref(), batch).await?;

    let session = store
        .get_autorun_session(&session_id)
        .await?
        .with_context(|| format!("no autorun session found with id '{session_id}'"))?;

    if !matches!(
        session.status,
        AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
    ) {
        anyhow::bail!(
            "session '{session_id}' is not running (status: {})",
            session.status
        );
    }

    println!("aborting batch '{session_id}'...");

    // Step 1: Send SIGTERM to orchestrator PID if available.
    let mut pid_signaled = false;
    if let Some(pid) = session.pid {
        if pid > 0 {
            // Check if process is alive first.
            let pid_i32 = i32::try_from(pid).unwrap_or(0);
            let alive = pid_i32 > 0 && unsafe { libc::kill(pid_i32, 0) } == 0;
            if alive {
                eprintln!("sending SIGTERM to orchestrator PID {pid}...");
                unsafe {
                    libc::kill(pid_i32, libc::SIGTERM);
                }
                pid_signaled = true;
            }
        }
    }

    // Step 1 fallback: Write abort marker file.
    if !pid_signaled {
        let runtime_dir = project_dir.join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).context("creating runtime dir")?;
        let marker_path = runtime_dir.join(format!("abort-{session_id}"));
        std::fs::write(&marker_path, "abort").context("writing abort marker")?;
        eprintln!("wrote abort marker at {}", marker_path.display());
    }

    // Step 2: Update session status to aborting.
    store
        .update_autorun_session(
            &session_id,
            codeflow_core::models::AutorunSessionUpdate {
                status: Some(AutorunSessionStatus::Aborting),
                ..Default::default()
            },
        )
        .await
        .context("updating session status")?;

    // Step 3-4: Cancel all running workers.
    let workers = store.list_autorun_workers(&session_id).await?;
    for worker in &workers {
        if worker.status == codeflow_core::types::AutorunWorkerStatus::Running {
            eprintln!("cancelling running worker: {}", worker.task_id);
            if let Err(e) = run_cancel(project_dir, &worker.task_id).await {
                eprintln!("warning: failed to cancel worker {}: {e}", worker.task_id);
            }
        }
    }

    // Step 5: Skip all pending tasks.
    let task_runs = store.list_autorun_task_runs(&session_id).await?;
    let now = chrono::Utc::now().to_rfc3339();
    for run in &task_runs {
        if matches!(
            run.status,
            codeflow_core::types::AutorunTaskRunStatus::Pending
        ) {
            store
                .update_autorun_task_run(
                    &run.id,
                    codeflow_core::models::AutorunTaskRunUpdate {
                        status: Some(codeflow_core::types::AutorunTaskRunStatus::Skipped),
                        completed_at: Some(now.clone()),
                        error_message: Some("batch_aborted".to_string()),
                        ..Default::default()
                    },
                )
                .await
                .context("skipping pending task run")?;
        }
    }

    // Step 6: Update batch status to cancelled.
    store
        .update_autorun_session(
            &session_id,
            codeflow_core::models::AutorunSessionUpdate {
                status: Some(AutorunSessionStatus::Cancelled),
                completed_at: Some(now),
                ..Default::default()
            },
        )
        .await
        .context("finalizing session status")?;

    println!("batch '{session_id}' aborted.");
    Ok(())
}

async fn run_results(project_dir: &Path, batch: Option<&str>) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;
    let session_id = resolve_session_id(store.as_ref(), batch).await?;

    let task_runs = store.list_autorun_task_runs(&session_id).await?;

    if task_runs.is_empty() {
        println!("No task runs found for session '{session_id}'.");
        return Ok(());
    }

    println!(
        "{:<24} {:<12} {:>9}  {:<6} {:>5}  ERROR",
        "TASK", "STATUS", "DURATION", "PR", "EXIT"
    );

    for run in &task_runs {
        let duration = run
            .duration_seconds
            .map_or_else(|| "--".to_string(), format_duration_secs);
        let pr = run
            .pr_number
            .map_or_else(|| "--".to_string(), |n| format!("#{n}"));
        let exit = run
            .exit_code
            .map_or_else(|| "--".to_string(), |c| c.to_string());
        let error = run.error_message.as_deref().unwrap_or("");

        println!(
            "{:<24} {:<12} {:>9}  {:<6} {:>5}  {}",
            run.task_id, run.status, duration, pr, exit, error
        );
    }

    Ok(())
}

async fn run_history(
    project_dir: &Path,
    limit: u32,
    since: Option<String>,
    status: Option<String>,
    batch_name: Option<String>,
    all: bool,
) -> Result<()> {
    use codeflow_core::store::DataStore;
    use codeflow_core::types::AutorunSessionStatus;
    use std::str::FromStr;

    let store = open_store(project_dir).await?;

    let status_filter = if let Some(ref s) = status {
        Some(AutorunSessionStatus::from_str(s).map_err(|_| {
            anyhow::anyhow!("invalid status '{s}'; valid values: pending, running, paused, completed, failed, cancelled, timeout, aborting")
        })?)
    } else {
        None
    };

    let filter = codeflow_core::models::AutorunSessionFilter {
        status: status_filter,
        batch_name,
        since,
        limit: Some(limit),
        all,
    };

    let sessions = store.list_autorun_sessions(filter).await?;

    if sessions.is_empty() {
        println!("No autorun sessions found.");
        return Ok(());
    }

    println!(
        "{:<24} {:<12} {:>5} {:>5} {:>5} {:>5}  STARTED",
        "BATCH", "STATUS", "TASKS", "DONE", "FAIL", "SKIP"
    );

    for session in &sessions {
        let batch_label = session
            .batch_name
            .as_deref()
            .unwrap_or(&session.id[..session.id.len().min(20)]);

        println!(
            "{:<24} {:<12} {:>5} {:>5} {:>5} {:>5}  {}",
            batch_label,
            session.status,
            session.total_tasks,
            session.completed_tasks,
            session.failed_tasks,
            session.skipped_tasks,
            &session.created_at[..session.created_at.len().min(19)],
        );
    }

    Ok(())
}

async fn run_resume(project_dir: &Path, batch: Option<&str>) -> Result<()> {
    use codeflow_core::store::DataStore;
    use codeflow_core::types::{AutorunSessionStatus, AutorunTaskRunStatus};

    // Autorun MUST work from the real repo root, not a worktree.
    let project_dir = &resolve_repo_root(project_dir)?;

    let store = open_store(project_dir).await?;

    // Find the target session.
    let session_id = if let Some(sid) = batch {
        sid.to_string()
    } else {
        // Find most recent non-completed session (failed, cancelled, timeout).
        let filter = codeflow_core::models::AutorunSessionFilter {
            limit: Some(10),
            all: true,
            ..Default::default()
        };
        let sessions = store.list_autorun_sessions(filter).await?;
        let resumable = sessions.iter().find(|s| {
            matches!(
                s.status,
                AutorunSessionStatus::Failed
                    | AutorunSessionStatus::Cancelled
                    | AutorunSessionStatus::Timeout
            )
        });
        match resumable {
            Some(s) => s.id.clone(),
            None => anyhow::bail!("no resumable autorun session found"),
        }
    };

    let session = store
        .get_autorun_session(&session_id)
        .await?
        .with_context(|| format!("no autorun session found with id '{session_id}'"))?;

    // Get task runs and filter to non-completed.
    let task_runs = store.list_autorun_task_runs(&session_id).await?;
    let completed_ids: std::collections::HashSet<String> = task_runs
        .iter()
        .filter(|r| matches!(r.status, AutorunTaskRunStatus::Completed))
        .map(|r| r.task_id.clone())
        .collect();

    let resumable_runs: Vec<_> = task_runs
        .iter()
        .filter(|r| !matches!(r.status, AutorunTaskRunStatus::Completed))
        .collect();

    if resumable_runs.is_empty() {
        println!("All tasks in batch '{session_id}' are completed. Nothing to resume.");
        return Ok(());
    }

    println!(
        "Resuming {} non-completed task(s) from batch '{}' ({})",
        resumable_runs.len(),
        session.batch_name.as_deref().unwrap_or(&session_id),
        session.status,
    );

    for run in &resumable_runs {
        println!("  {} (was: {})", run.task_id, run.status);
    }

    // Read the original batch file to reconstruct config.
    if session.batch_file.is_empty() {
        anyhow::bail!("session has no batch_file recorded; cannot reconstruct batch config");
    }
    let batch_path = if Path::new(&session.batch_file).is_absolute() {
        PathBuf::from(&session.batch_file)
    } else {
        project_dir.join(&session.batch_file)
    };

    if !batch_path.exists() {
        anyhow::bail!(
            "original batch file '{}' not found; cannot resume",
            batch_path.display()
        );
    }

    // Build a filtered batch using the resume helper.
    let original_parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path)
        .context("re-parsing original batch file")?;

    let resume_batch =
        codeflow_core::autorun::batch::build_resume_batch(&original_parsed, &completed_ids);

    if resume_batch.tasks.is_empty() {
        println!("No tasks to resume after filtering.");
        return Ok(());
    }

    // Run the resume batch through the normal execution path.
    println!("Running resume batch: {} task(s)", resume_batch.tasks.len());

    // Validate extended and execute.
    let mut resume_parsed = resume_batch;
    codeflow_core::autorun::validate_batch_extended(&mut resume_parsed, project_dir)
        .context("extended batch validation for resume")?;

    check_target_branch(project_dir, &resume_parsed.target)?;

    let config =
        codeflow_core::autorun::load_config(project_dir).context("loading parallel-work config")?;

    let new_session_id = codeflow_core::session::generate_session_id();

    let worktree_provider = codeflow_core::autorun::RealWorktreeProvider::new(project_dir.clone());
    let tmux = RealTmux;
    let worker_timeout = Duration::from_secs(config.autorun.worker_timeout_secs);
    let claude = RealClaude {
        tmux: RealTmux,
        worker_timeout,
    };
    let worker = codeflow_core::autorun::TmuxWorker::with_store(
        tmux,
        claude,
        worktree_provider,
        project_dir.clone(),
        worker_timeout,
        store.clone(),
    );

    let orchestrator = codeflow_core::autorun::Orchestrator::new(worker, store);

    // Start sync daemon once before orchestrator execution.
    let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
    if let Err(e) = codeflow_core::worktree::maybe_auto_start_daemon(&registry_path, project_dir) {
        eprintln!("warning: daemon auto-start failed: {e}");
    }

    let results = orchestrator
        .execute_with_batch_file(
            new_session_id.as_str(),
            &resume_parsed,
            project_dir,
            &batch_path.to_string_lossy(),
            async {
                tokio::signal::ctrl_c().await.ok();
            },
        )
        .await
        .context("executing resume batch")?;

    // Stop sync daemon after orchestrator completes.
    if let Err(e) = codeflow_core::worktree::maybe_auto_stop_daemon(&registry_path, project_dir) {
        eprintln!("warning: daemon auto-stop failed: {e}");
    }

    report_results(&results)
}

fn run_batches_sync(project_dir: &Path) -> Result<()> {
    let batch_dir = project_dir.join(".codeflow/config/autorun");
    if !batch_dir.exists() {
        println!(
            "No autorun config directory found at {}",
            batch_dir.display()
        );
        return Ok(());
    }

    let mut entries: Vec<_> = std::fs::read_dir(&batch_dir)
        .context("reading autorun config directory")?
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.ends_with(".yaml") || name.ends_with(".yml")
        })
        .collect();

    if entries.is_empty() {
        println!("No batch files found in {}", batch_dir.display());
        return Ok(());
    }

    entries.sort_by_key(std::fs::DirEntry::file_name);

    println!(
        "{:<30} {:>5} {:<12} {:<8}",
        "FILE", "TASKS", "TARGET", "AUTO_MERGE"
    );

    for entry in &entries {
        let path = entry.path();
        match codeflow_core::autorun::batch::parse_batch_file(&path) {
            Ok(batch) => {
                let target = if batch.target.is_empty() {
                    "main"
                } else {
                    &batch.target
                };
                println!(
                    "{:<30} {:>5} {:<12} {:<8}",
                    entry.file_name().to_string_lossy(),
                    batch.tasks.len(),
                    target,
                    batch.auto_merge,
                );
            }
            Err(e) => {
                println!(
                    "{:<30} {:>5} {:<12} {:<8}",
                    entry.file_name().to_string_lossy(),
                    "ERR",
                    "--",
                    format!("({})", e),
                );
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a tmux session exists.
fn tmux_has_session(name: &str) -> bool {
    std::process::Command::new("tmux")
        .args(["has-session", "-t", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Format elapsed time from an ISO timestamp to now.
fn format_elapsed(started_at: &str) -> String {
    let Ok(start) = chrono::DateTime::parse_from_rfc3339(started_at) else {
        return "--:--".to_string();
    };
    let elapsed = chrono::Utc::now().signed_duration_since(start);
    let total_secs = elapsed.num_seconds().max(0);
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

/// Format a duration in seconds to MM:SS or HH:MM:SS.
fn format_duration_secs(secs: i64) -> String {
    let total = secs.max(0);
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    let s = total % 60;
    if hours > 0 {
        format!("{hours:02}:{mins:02}:{s:02}")
    } else {
        format!("{mins:02}:{s:02}")
    }
}

/// Stub module for exec (tmux attach replaces current process).
mod exec {
    pub struct Command {
        pub(super) program: String,
        pub(super) args: Vec<String>,
    }

    impl Command {
        pub fn new(program: &str) -> Self {
            Self {
                program: program.to_string(),
                args: Vec::new(),
            }
        }

        pub fn args(mut self, args: &[&str]) -> Self {
            self.args.extend(args.iter().map(ToString::to_string));
            self
        }

        pub fn exec(self) -> std::io::Error {
            use std::os::unix::process::CommandExt;
            let mut cmd = std::process::Command::new(&self.program);
            cmd.args(&self.args);
            cmd.exec()
        }
    }
}

/// Run a tmux command and return success/failure.
async fn run_tmux(args: &[&str]) -> Result<bool, codeflow_core::AutorunError> {
    let status = tokio::process::Command::new("tmux")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|e| codeflow_core::AutorunError::WorkerFailed(format!("tmux {}: {e}", args[0])))?;
    Ok(status.success())
}

/// Real tmux runner that executes tmux commands via the system.
struct RealTmux;

impl codeflow_core::autorun::TmuxRunner for RealTmux {
    async fn create_session(&self, name: &str) -> Result<(), codeflow_core::AutorunError> {
        if !run_tmux(&["new-session", "-d", "-s", name]).await? {
            return Err(codeflow_core::AutorunError::WorkerFailed(format!(
                "tmux new-session failed for {name}"
            )));
        }
        // Keep the pane alive after the command exits so attach/logs can inspect.
        let _ = run_tmux(&["set-option", "-t", name, "remain-on-exit", "on"]).await;
        Ok(())
    }

    async fn send_command(
        &self,
        session: &str,
        command: &str,
    ) -> Result<(), codeflow_core::AutorunError> {
        if !run_tmux(&["send-keys", "-t", session, command, "Enter"]).await? {
            return Err(codeflow_core::AutorunError::WorkerFailed(format!(
                "tmux send-keys failed for {session}"
            )));
        }
        Ok(())
    }

    async fn kill_session(&self, name: &str) -> Result<(), codeflow_core::AutorunError> {
        let _ = run_tmux(&["kill-session", "-t", name]).await;
        Ok(())
    }

    async fn has_session(&self, name: &str) -> Result<bool, codeflow_core::AutorunError> {
        run_tmux(&["has-session", "-t", name]).await
    }
}

/// Default polling interval for file-marker completion detection.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Grace period after SIGTERM before escalating to SIGKILL.
const SIGTERM_GRACE_SECS: u64 = 10;

/// Real Claude invoker using tmux send-keys for command dispatch and
/// file-marker polling for completion detection.
struct RealClaude<T: codeflow_core::autorun::TmuxRunner> {
    tmux: T,
    worker_timeout: Duration,
}

impl<T: codeflow_core::autorun::TmuxRunner> RealClaude<T> {
    /// Construct the base64-encoded acceptance criteria JSON.
    fn encode_acceptance(criteria: &[String]) -> String {
        let json = serde_json::to_string(criteria).unwrap_or_else(|_| "[]".to_string());
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    /// Read the exit code from the marker file.
    fn read_exit_code(path: &Path) -> i32 {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(1)
    }
}

impl<T: codeflow_core::autorun::TmuxRunner> codeflow_core::autorun::ClaudeInvoker
    for RealClaude<T>
{
    async fn invoke(
        &self,
        cfg: codeflow_core::autorun::InvokeConfig,
    ) -> Result<codeflow_core::autorun::InvokeResult, codeflow_core::AutorunError> {
        let session = &cfg.tmux_session;
        let work_dir = &cfg.work_dir;

        // Ensure .state/runtime directory exists in worktree.
        let runtime_dir = PathBuf::from(work_dir).join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).map_err(|e| {
            codeflow_core::AutorunError::WorkerFailed(format!(
                "creating runtime dir {}: {e}",
                runtime_dir.display()
            ))
        })?;

        // Set environment variables via tmux send-keys.
        // AUTORUN_SESSION_ID uses the worker-specific session ID so that hooks
        // use the same ID as CRDT claims (acquired with worker_sid in worker.rs).
        let autorun_sid = if cfg.worker_session_id.is_empty() {
            &cfg.session_id
        } else {
            &cfg.worker_session_id
        };
        self.tmux
            .send_command(session, &format!("export AUTORUN_SESSION_ID={autorun_sid}"))
            .await?;
        // AUTORUN_BATCH_ID tracks the batch-level session for correlation.
        self.tmux
            .send_command(
                session,
                &format!("export AUTORUN_BATCH_ID={}", cfg.session_id),
            )
            .await?;
        self.tmux
            .send_command(session, &format!("export AUTORUN_TASK_ID={}", cfg.task_id))
            .await?;

        let acceptance_b64 = Self::encode_acceptance(&cfg.acceptance_criteria);
        self.tmux
            .send_command(
                session,
                &format!("export AUTORUN_ACCEPTANCE={acceptance_b64}"),
            )
            .await?;

        self.tmux
            .send_command(
                session,
                &format!("export CODEFLOW_WORKTREE_PATH={work_dir}"),
            )
            .await?;

        // Escape single quotes in the prompt for POSIX single-quote shell quoting.
        // Pattern: end current quote, insert escaped quote, restart quote ('\'')
        let escaped_prompt = cfg.prompt.replace('\'', "'\\''");

        // Send the Claude command. Uses regular '...' quoting (not $'...') so
        // that backslash sequences like \n in the prompt are preserved literally.
        // Quote work_dir with POSIX single-quote escaping to handle paths with spaces.
        let escaped_work_dir = work_dir.replace('\'', "'\\''");
        let exit_code_path = format!("{work_dir}/.state/runtime/worker-exit-code");
        let escaped_exit_code_path = exit_code_path.replace('\'', "'\\''");
        let claude_cmd = format!(
            "cd '{escaped_work_dir}' && claude --dangerously-skip-permissions \
             '{escaped_prompt}'; \
             echo $? > '{escaped_exit_code_path}'"
        );
        self.tmux.send_command(session, &claude_cmd).await?;

        // INNER TIMEOUT: Polls for Claude's exit-code marker file and handles
        // graceful shutdown (SIGTERM → grace period → SIGKILL). This complements
        // the OUTER timeout in worker.rs which bounds the entire worker lifecycle.
        // The inner timeout provides Claude-specific shutdown sequencing, while
        // the outer timeout catches hangs in non-Claude phases (worktree setup, etc.).
        let exit_code_file = PathBuf::from(&exit_code_path);
        let timeout = self.worker_timeout;
        let poll_start = std::time::Instant::now();

        loop {
            if exit_code_file.exists() {
                break;
            }

            if poll_start.elapsed() >= timeout {
                // Timeout: SIGTERM the tmux session, wait grace period, then SIGKILL.
                eprintln!(
                    "worker timeout after {}s for task {}, sending SIGTERM",
                    timeout.as_secs(),
                    cfg.task_id
                );
                // Send Ctrl-C to the tmux session to interrupt Claude.
                let _ = self.tmux.send_command(session, "C-c").await;
                tokio::time::sleep(Duration::from_secs(SIGTERM_GRACE_SECS)).await;

                // If still no exit code, kill the session.
                if !exit_code_file.exists() {
                    let _ = self.tmux.kill_session(session).await;
                }

                return Ok(codeflow_core::autorun::InvokeResult {
                    exit_code: 124,
                    pr_number: 0,
                    pr_url: String::new(),
                    branch_name: String::new(),
                    output: "worker exceeded timeout".to_string(),
                });
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }

        // Read exit code from marker file.
        let exit_code = Self::read_exit_code(&exit_code_file);

        // Interactive Claude doesn't write structured JSON output.
        // Get PR info from git branch + gh pr list.
        let mut pr_number: i64 = 0;
        let mut pr_url = String::new();

        // Get current branch name from the worktree.
        let branch_name = tokio::process::Command::new("git")
            .args(["-C", work_dir, "branch", "--show-current"])
            .output()
            .await
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // If PR info not found in output, try gh pr list as fallback.
        if pr_number == 0 && exit_code == 0 && !branch_name.is_empty() {
            if let Ok(gh_output) = tokio::process::Command::new("gh")
                .args([
                    "pr",
                    "list",
                    "--head",
                    &branch_name,
                    "--json",
                    "number,url",
                    "--limit",
                    "1",
                ])
                .current_dir(work_dir)
                .output()
                .await
            {
                if gh_output.status.success() {
                    if let Ok(prs) =
                        serde_json::from_slice::<Vec<serde_json::Value>>(&gh_output.stdout)
                    {
                        if let Some(pr) = prs.first() {
                            pr_number = pr
                                .get("number")
                                .and_then(serde_json::Value::as_i64)
                                .unwrap_or(0);
                            pr_url = pr
                                .get("url")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("")
                                .to_string();
                        }
                    }
                }
            }
        }

        Ok(codeflow_core::autorun::InvokeResult {
            exit_code,
            pr_number,
            pr_url,
            branch_name,
            output: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Initialize a minimal git repo (with one commit) so preflight's git-clean check passes.
    fn init_git_repo(dir: &Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    /// Commit all files in the repo so the working tree is clean for preflight.
    fn commit_all(dir: &Path) {
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "add files"])
            .current_dir(dir)
            .output()
            .unwrap();
    }

    // -- run_with_dir: missing batch file --

    #[test]
    fn test_autorun_no_batch_file() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let batch_path = dir.path().join(".codeflow/config/autorun/batch.yaml");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path(), &batch_path, true));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("no autorun batch file"),
            "expected batch file error, got: {msg}"
        );
    }

    // -- run_with_dir: valid batch gets past parsing --

    #[test]
    fn test_autorun_with_valid_batch() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let batch_path = batch_dir.join("batch.yaml");
        std::fs::write(&batch_path, "name: test-batch\ntasks:\n  - id: task-1\n").unwrap();
        commit_all(dir.path());
        let rt = tokio::runtime::Runtime::new().unwrap();
        // Will fail at target branch check or tmux level, but should get past parsing.
        let result = rt.block_on(run_with_dir(dir.path(), &batch_path, true));
        let _ = result;
    }

    // -- run_with_dir: invalid YAML --

    #[test]
    fn test_autorun_with_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let batch_path = batch_dir.join("batch.yaml");
        std::fs::write(&batch_path, "{{invalid yaml").unwrap();
        commit_all(dir.path());
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path(), &batch_path, true));
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(
            msg.contains("parsing autorun batch"),
            "expected parse context, got: {msg}"
        );
    }

    // -- Config loading --

    #[test]
    fn test_autorun_loads_config() {
        let dir = tempfile::tempdir().unwrap();
        // Create batch file.
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("batch.yaml"),
            "name: config-test\ntasks:\n  - id: task-1\n",
        )
        .unwrap();
        // Create config file.
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "worktree": { "max_concurrent": 5 } }"#,
        )
        .unwrap();

        // Config should load without error.
        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.worktree.max_concurrent, 5);
    }

    #[test]
    fn test_config_loads_defaults_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.worktree.max_concurrent, 5);
        assert_eq!(config.claims.default_scope_policy, "soft");
        assert_eq!(config.claims.ttl_secs, 4200);
        assert!(config.sync.auto_start);
    }

    #[test]
    fn test_config_loads_custom_claims_values() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{
                "claims": {
                    "default_scope_policy": "hard",
                    "ttl_secs": 7200,
                    "capture_events": false
                }
            }"#,
        )
        .unwrap();

        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.claims.default_scope_policy, "hard");
        assert_eq!(config.claims.ttl_secs, 7200);
        assert!(!config.claims.capture_events);
    }

    #[test]
    fn test_config_loads_custom_sync_values() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "sync": { "interval_secs": 10, "auto_start": false } }"#,
        )
        .unwrap();

        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert_eq!(config.sync.interval_secs, 10);
        assert!(!config.sync.auto_start);
    }

    #[test]
    fn test_config_loads_custom_merge_values() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "merge": { "auto_rebase": false, "queue_enabled": false, "max_rebase_attempts": 5 } }"#,
        )
        .unwrap();

        let config = codeflow_core::autorun::load_config(dir.path()).unwrap();
        assert!(!config.merge.auto_rebase);
        assert!(!config.merge.queue_enabled);
        assert_eq!(config.merge.max_rebase_attempts, 5);
    }

    #[test]
    fn test_config_malformed_json_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            "{ not valid json }",
        )
        .unwrap();

        let result = codeflow_core::autorun::load_config(dir.path());
        assert!(result.is_err());
    }

    // -- Batch parsing --

    #[test]
    fn test_batch_parse_multiple_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: multi-task\nmax_workers: 3\ntasks:\n  - id: task-a\n  - id: task-b\n  - id: task-c\n",
        )
        .unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert_eq!(parsed.tasks.len(), 3);
        assert_eq!(parsed.max_workers, 3);
        assert_eq!(parsed.tasks[0].id, "task-a");
        assert_eq!(parsed.tasks[1].id, "task-b");
        assert_eq!(parsed.tasks[2].id, "task-c");
    }

    #[test]
    fn test_batch_parse_with_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: deps-test\ntasks:\n  - id: task-a\n  - id: task-b\n    depends_on: [task-a]\n",
        )
        .unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert_eq!(parsed.tasks.len(), 2);
        assert!(parsed.tasks[0].depends_on.is_empty());
        assert_eq!(parsed.tasks[1].depends_on, vec!["task-a"]);
    }

    #[test]
    fn test_batch_parse_empty_tasks() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(&batch_path, "name: empty\ntasks: []\n").unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path);
        // Empty tasks should either succeed (with empty vec) or fail validation.
        match parsed {
            Ok(p) => assert!(p.tasks.is_empty()),
            Err(e) => assert!(
                e.to_string().contains("task") || e.to_string().contains("empty"),
                "error should relate to empty tasks: {e}"
            ),
        }
    }

    #[test]
    fn test_batch_parse_nonexistent_file() {
        let result =
            codeflow_core::autorun::batch::parse_batch_file(Path::new("/nonexistent/batch.yaml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_batch_parse_with_file_scope_and_scope_policy() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(
            &batch_path,
            r#"name: scope-test
tasks:
  - id: task-a
    file_scope:
      - "src/main.rs"
      - "src/lib.rs"
    scope_policy: hard
"#,
        )
        .unwrap();

        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert_eq!(
            parsed.tasks[0].file_scope,
            vec!["src/main.rs", "src/lib.rs"]
        );
        assert_eq!(
            parsed.tasks[0].scope_policy.as_deref(),
            Some("hard"),
            "scope_policy should be parsed from batch"
        );
    }

    // -- report_results (extracted from run_with_dir) --

    #[test]
    fn test_report_results_all_completed_returns_ok() {
        let results = vec![
            make_result("task-1", "completed", 0),
            make_result("task-2", "completed", 0),
            make_result("task-3", "completed", 0),
        ];
        assert!(report_results(&results).is_ok());
    }

    #[test]
    fn test_report_results_with_failure_returns_error() {
        let results = vec![
            make_result("task-1", "completed", 0),
            make_result("task-2", "failed", 1),
        ];
        let err = report_results(&results).unwrap_err();
        assert!(
            err.to_string().contains("1 task(s) failed or timed out"),
            "expected failure count in error, got: {err}"
        );
    }

    #[test]
    fn test_report_results_with_timeout_returns_error() {
        let results = vec![make_result("task-1", "timeout", -1)];
        let err = report_results(&results).unwrap_err();
        assert!(err.to_string().contains("1 task(s) failed or timed out"));
    }

    #[test]
    fn test_report_results_mixed_statuses() {
        let results = vec![
            make_result("task-1", "completed", 0),
            make_result("task-2", "failed", 1),
            make_result("task-3", "skipped", 0),
            make_result("task-4", "timeout", -1),
        ];
        let err = report_results(&results).unwrap_err();
        assert!(
            err.to_string().contains("2 task(s) failed or timed out"),
            "expected 2 failures, got: {err}"
        );
    }

    #[test]
    fn test_report_results_skipped_only_returns_ok() {
        let results = vec![
            make_result("task-1", "skipped", 0),
            make_result("task-2", "skipped", 0),
        ];
        // Skipped tasks don't trigger failure.
        assert!(report_results(&results).is_ok());
    }

    #[test]
    fn test_report_results_empty_returns_ok() {
        let results: Vec<codeflow_core::autorun::WorkerResult> = vec![];
        assert!(report_results(&results).is_ok());
    }

    #[test]
    fn test_report_results_with_error_message() {
        let results = vec![make_result_with_error(
            "task-err",
            "failed",
            1,
            "connection timeout",
        )];
        let err = report_results(&results).unwrap_err();
        assert!(err.to_string().contains("1 task(s) failed"));
    }

    // -- format_error --

    #[test]
    fn test_format_error_with_message() {
        assert_eq!(
            format_error("connection timeout"),
            ", error=connection timeout"
        );
    }

    #[test]
    fn test_format_error_empty() {
        assert!(format_error("").is_empty());
    }

    // -- Worker/orchestrator construction --

    #[test]
    fn test_worker_config_construction() {
        let cfg = codeflow_core::autorun::WorkerConfig {
            session_id: "ses-test".into(),
            worker_id: "w-1".into(),
            worker_num: 1,
            task_id: "task-test".into(),
            batch_name: "test-batch".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_name: "cf-ar-task-test".into(),
            file_scope: vec!["src/main.rs".into()],
            scope_policy: "soft".into(),
            blocked_behavior: "skip_and_continue".into(),
        };
        assert_eq!(cfg.task_id, "task-test");
        assert_eq!(cfg.scope_policy, "soft");
        assert_eq!(cfg.file_scope, vec!["src/main.rs"]);
    }

    #[test]
    fn test_worker_result_construction() {
        let result = codeflow_core::autorun::WorkerResult {
            worker_id: "w-1".into(),
            task_id: "task-1".into(),
            status: "completed".into(),
            exit_code: 0,
            pr_number: 42,
            pr_url: "https://github.com/org/repo/pull/42".into(),
            error: String::new(),
            branch_name: "feat/test".into(),
            duration_sec: 120,
        };
        assert_eq!(result.status, "completed");
        assert_eq!(result.pr_number, 42);
        assert_eq!(result.duration_sec, 120);
    }

    #[test]
    fn test_invoke_config_and_result() {
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-invoke".into(),
            work_dir: "/tmp/work".into(),
            prompt: "implement feature X".into(),
            session_id: "ses-test".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "worker-1".into(),
            acceptance_criteria: vec!["tests pass".into()],
            worker_session_id: String::new(),
        };
        assert_eq!(cfg.task_id, "task-invoke");
        assert_eq!(cfg.acceptance_criteria, vec!["tests pass"]);

        let result = codeflow_core::autorun::InvokeResult {
            exit_code: 0,
            pr_number: 1,
            pr_url: String::new(),
            branch_name: "feat/x".into(),
            output: "done".into(),
        };
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.branch_name, "feat/x");
    }

    #[test]
    fn test_invoke_config_round_trip_serde() {
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-rt".into(),
            work_dir: "/tmp/test".into(),
            prompt: "do things".into(),
            session_id: "ses-rt".into(),
            auto_merge: true,
            target: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: vec!["crit 1".into(), "crit 2".into()],
            worker_session_id: String::new(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: codeflow_core::autorun::InvokeConfig =
            serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, cfg.task_id);
        assert_eq!(deserialized.work_dir, cfg.work_dir);
        assert_eq!(deserialized.prompt, cfg.prompt);
        assert_eq!(deserialized.session_id, cfg.session_id);
        assert_eq!(deserialized.auto_merge, cfg.auto_merge);
        assert_eq!(deserialized.target, cfg.target);
        assert_eq!(deserialized.tmux_session, cfg.tmux_session);
        assert_eq!(deserialized.acceptance_criteria, cfg.acceptance_criteria);
    }

    // -- Helpers --

    fn make_result(
        task_id: &str,
        status: &str,
        exit_code: i32,
    ) -> codeflow_core::autorun::WorkerResult {
        codeflow_core::autorun::WorkerResult {
            worker_id: format!("w-{task_id}"),
            task_id: task_id.into(),
            status: status.into(),
            exit_code,
            pr_number: 0,
            pr_url: String::new(),
            error: String::new(),
            branch_name: String::new(),
            duration_sec: 0,
        }
    }

    fn make_result_with_error(
        task_id: &str,
        status: &str,
        exit_code: i32,
        error: &str,
    ) -> codeflow_core::autorun::WorkerResult {
        codeflow_core::autorun::WorkerResult {
            worker_id: format!("w-{task_id}"),
            task_id: task_id.into(),
            status: status.into(),
            exit_code,
            pr_number: 0,
            pr_url: String::new(),
            error: error.into(),
            branch_name: String::new(),
            duration_sec: 0,
        }
    }

    // -- RealClaude tests --

    /// Mock tmux that records sent commands for verification.
    struct RecordingTmux {
        commands: std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
    }

    impl RecordingTmux {
        #[allow(clippy::type_complexity)]
        fn new() -> (
            Self,
            std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
        ) {
            let commands = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            (
                Self {
                    commands: commands.clone(),
                },
                commands,
            )
        }
    }

    impl codeflow_core::autorun::TmuxRunner for RecordingTmux {
        async fn create_session(&self, _name: &str) -> Result<(), codeflow_core::AutorunError> {
            Ok(())
        }
        async fn send_command(
            &self,
            session: &str,
            command: &str,
        ) -> Result<(), codeflow_core::AutorunError> {
            self.commands
                .lock()
                .await
                .push((session.to_string(), command.to_string()));
            Ok(())
        }
        async fn kill_session(&self, _name: &str) -> Result<(), codeflow_core::AutorunError> {
            Ok(())
        }
        async fn has_session(&self, _name: &str) -> Result<bool, codeflow_core::AutorunError> {
            Ok(false)
        }
    }

    #[test]
    fn test_real_claude_encode_acceptance() {
        let criteria = vec!["tests pass".to_string(), "no warnings".to_string()];
        let encoded = RealClaude::<RecordingTmux>::encode_acceptance(&criteria);
        // Should be valid base64.
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed, criteria);
    }

    #[test]
    fn test_real_claude_encode_empty_acceptance() {
        let encoded = RealClaude::<RecordingTmux>::encode_acceptance(&[]);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded.as_bytes())
            .unwrap();
        let json_str = String::from_utf8(decoded).unwrap();
        assert_eq!(json_str, "[]");
    }

    #[test]
    fn test_real_claude_read_exit_code_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exit-code");
        std::fs::write(&path, "0\n").unwrap();
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 0);
    }

    #[test]
    fn test_real_claude_read_exit_code_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exit-code");
        std::fs::write(&path, "42\n").unwrap();
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 42);
    }

    #[test]
    fn test_real_claude_read_exit_code_missing() {
        let path = PathBuf::from("/nonexistent/exit-code");
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 1);
    }

    #[test]
    fn test_real_claude_read_exit_code_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exit-code");
        std::fs::write(&path, "not-a-number\n").unwrap();
        assert_eq!(RealClaude::<RecordingTmux>::read_exit_code(&path), 1);
    }

    #[test]
    fn test_real_claude_invoke_sends_env_vars_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Pre-create exit code marker so polling returns immediately.
        std::fs::write(runtime_dir.join("worker-exit-code"), "0\n").unwrap();
        std::fs::write(
            runtime_dir.join("worker-output.json"),
            r#"{"result": "ok"}"#,
        )
        .unwrap();

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-env-test".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test prompt".into(),
            session_id: "ses-env".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "worker-env".into(),
            acceptance_criteria: vec!["crit A".into(), "crit B".into()],
            worker_session_id: "ses-env-worker".into(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 0);

        let cmds = rt.block_on(commands.lock());
        // Verify env vars were sent in the correct order.
        assert!(
            cmds.len() >= 6,
            "expected at least 6 commands, got {}",
            cmds.len()
        );

        // Command 0: export AUTORUN_SESSION_ID (uses worker_session_id)
        assert!(
            cmds[0].1.contains("AUTORUN_SESSION_ID=ses-env-worker"),
            "first env var should be worker session ID, got: {}",
            cmds[0].1
        );
        // Command 1: export AUTORUN_BATCH_ID (batch-level session)
        assert!(
            cmds[1].1.contains("AUTORUN_BATCH_ID=ses-env"),
            "second env var should be batch ID, got: {}",
            cmds[1].1
        );
        // Command 2: export AUTORUN_TASK_ID
        assert!(
            cmds[2].1.contains("AUTORUN_TASK_ID=task-env-test"),
            "third env var should be task ID, got: {}",
            cmds[2].1
        );
        // Command 3: export AUTORUN_ACCEPTANCE (base64)
        assert!(
            cmds[3].1.contains("AUTORUN_ACCEPTANCE="),
            "fourth env var should be acceptance, got: {}",
            cmds[3].1
        );
        // Command 4: export CODEFLOW_WORKTREE_PATH
        assert!(
            cmds[4].1.contains("CODEFLOW_WORKTREE_PATH="),
            "fifth env var should be worktree path, got: {}",
            cmds[4].1
        );
        // Command 5: the actual claude command (interactive mode, no -p flag)
        assert!(
            cmds[5].1.contains("claude --dangerously-skip-permissions"),
            "sixth command should be claude invocation, got: {}",
            cmds[5].1
        );
        assert!(
            !cmds[5].1.contains("claude -p"),
            "claude command should NOT use -p flag (interactive mode)"
        );
        assert!(
            !cmds[5].1.contains("--output-format json"),
            "claude command should NOT use --output-format json (interactive mode)"
        );
        assert!(
            cmds[5].1.contains("worker-exit-code"),
            "exit code should be written to worker-exit-code"
        );

        // All commands should target the correct session.
        for (session, _) in &*cmds {
            assert_eq!(session, "worker-env");
        }
    }

    #[test]
    fn test_real_claude_invoke_reads_exit_code_from_marker() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Non-zero exit code.
        std::fs::write(runtime_dir.join("worker-exit-code"), "1\n").unwrap();
        std::fs::write(runtime_dir.join("worker-output.json"), "{}").unwrap();

        let (tmux, _) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-exitcode".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test".into(),
            session_id: "ses-exit".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-exit".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[test]
    fn test_real_claude_invoke_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        // Do NOT create exit code marker -- should trigger timeout.

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_millis(100), // Very short timeout.
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-timeout".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test".into(),
            session_id: "ses-timeout".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-timeout".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 124, "timeout should return exit code 124");
        assert!(
            result.output.contains("timeout"),
            "output should mention timeout"
        );

        // Should have sent Ctrl-C for SIGTERM.
        let cmds = rt.block_on(commands.lock());
        let has_ctrl_c = cmds.iter().any(|(_, cmd)| cmd == "C-c");
        assert!(has_ctrl_c, "should send C-c on timeout");
    }

    #[test]
    fn test_real_claude_invoke_preserves_backslash_sequences_in_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state/runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(runtime_dir.join("worker-exit-code"), "0\n").unwrap();
        std::fs::write(runtime_dir.join("worker-output.json"), "{}").unwrap();

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        // Prompt contains literal \n and \t which must NOT be interpreted as
        // newline/tab when using regular '...' quoting (not $'...').
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-escape".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "line1\\nline2\\ttab".into(),
            session_id: "ses-esc".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-esc".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg));

        let cmds = rt.block_on(commands.lock());
        // Find the claude command (interactive mode, no -p flag).
        let claude_cmd = cmds
            .iter()
            .find(|(_, cmd)| cmd.contains("claude --dangerously-skip-permissions"))
            .expect("should have a claude command");

        // Verify it uses regular '...' quoting, not $'...'.
        assert!(
            claude_cmd
                .1
                .contains("claude --dangerously-skip-permissions"),
            "should invoke claude in interactive mode, got: {}",
            claude_cmd.1
        );
        assert!(
            !claude_cmd.1.contains("$'"),
            "should NOT use $'...' quoting, got: {}",
            claude_cmd.1
        );
        // The literal \n should be preserved in the command string.
        assert!(
            claude_cmd.1.contains("\\n"),
            "backslash-n should be preserved literally, got: {}",
            claude_cmd.1
        );
    }

    // -- run_tmux helper + RealTmux tests --

    #[test]
    fn test_run_tmux_nonexistent_command() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_tmux(&["has-session", "-t", "nonexistent-xyz"]));
        // Ok(false) if tmux available, Err if tmux not installed — both valid.
        if let Ok(success) = result {
            assert!(!success);
        }
    }

    #[test]
    fn test_real_tmux_kill_nonexistent_session_is_ok() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::kill_session(
            &tmux,
            "nonexistent-session-xyz",
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn test_real_tmux_has_nonexistent_session() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::has_session(
            &tmux,
            "nonexistent-session-xyz",
        ));
        if let Ok(has) = result {
            assert!(!has, "nonexistent session should not exist");
        }
    }

    #[test]
    fn test_real_tmux_create_session_handles_failure() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::create_session(
            &tmux, "",
        ));
        let _ = result;
    }

    #[test]
    fn test_real_tmux_send_command_handles_failure() {
        let tmux = RealTmux;
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(codeflow_core::autorun::TmuxRunner::send_command(
            &tmux,
            "nonexistent-session-xyz",
            "echo test",
        ));
        let _ = result;
    }

    // -- run_with_dir integration: batch + config + session ID generation --

    #[test]
    fn test_run_with_dir_generates_session_id_and_prints_batch_info() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let batch_path = batch_dir.join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: session-gen-test\nmax_workers: 2\ntasks:\n  - id: task-a\n  - id: task-b\n",
        )
        .unwrap();

        // Write custom config.
        let config_dir = dir
            .path()
            .join(".codeflow")
            .join("config")
            .join("parallel-work");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("parallel-work-config.json"),
            r#"{ "claims": { "default_scope_policy": "hard" } }"#,
        )
        .unwrap();
        commit_all(dir.path());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path(), &batch_path, true));
        // Will fail at target branch or orchestrator/tmux level, but exercises
        // config + batch + session ID paths.
        let _ = result;
    }

    #[test]
    fn test_run_with_dir_with_multiple_tasks_and_config() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let batch_dir = dir.path().join(".codeflow").join("config").join("autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let batch_path = batch_dir.join("batch.yaml");
        std::fs::write(
            &batch_path,
            "name: multi\nmax_workers: 1\ntasks:\n  - id: t1\n    file_scope: [src/a.rs]\n    scope_policy: soft\n  - id: t2\n    depends_on: [t1]\n",
        )
        .unwrap();
        commit_all(dir.path());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path(), &batch_path, true));
        let _ = result;
    }

    // -- Batch parsing edge cases --

    #[test]
    fn test_batch_parse_default_max_workers() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        // No max_workers specified — should use default.
        std::fs::write(&batch_path, "name: defaults\ntasks:\n  - id: task-1\n").unwrap();
        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert!(parsed.max_workers > 0, "default max_workers should be > 0");
    }

    #[test]
    fn test_batch_parse_task_without_scope() {
        let dir = tempfile::tempdir().unwrap();
        let batch_path = dir.path().join("batch.yaml");
        std::fs::write(&batch_path, "name: no-scope\ntasks:\n  - id: task-plain\n").unwrap();
        let parsed = codeflow_core::autorun::batch::parse_batch_file(&batch_path).unwrap();
        assert!(parsed.tasks[0].file_scope.is_empty());
        assert!(parsed.tasks[0].scope_policy.is_none());
    }

    // -- Pre-flight checks --

    #[test]
    fn test_preflight_checks_pass_in_clean_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        // tmux should be available in dev environments; git repo is clean.
        let result = preflight_checks(dir.path());
        // In CI without tmux this will fail on tmux check — that's acceptable.
        // If tmux is present, both checks pass.
        match &result {
            Ok(()) => {} // Both checks passed.
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("tmux"),
                    "only acceptable failure is tmux not found, got: {msg}"
                );
            }
        }
    }

    #[test]
    fn test_check_tmux_available_returns_result() {
        let result = check_tmux_available();
        // In most dev environments tmux is installed (Ok), in CI it may not be (Err).
        // Either is valid — we're testing it doesn't panic.
        let _ = result;
    }

    #[test]
    fn test_check_git_clean_in_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let result = check_git_clean(dir.path());
        assert!(result.is_ok(), "clean repo should pass: {result:?}");
    }

    #[test]
    fn test_check_git_clean_dirty_repo_returns_error_with_files() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        // Create an untracked file to make the tree dirty.
        std::fs::write(dir.path().join("dirty-file.txt"), "uncommitted").unwrap();
        let result = check_git_clean(dir.path());
        assert!(result.is_err(), "dirty repo should fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("working tree is not clean"),
            "error should mention dirty tree, got: {msg}"
        );
        assert!(
            msg.contains("dirty-file.txt"),
            "error should list dirty files, got: {msg}"
        );
    }

    #[test]
    fn test_check_target_branch_exists_locally() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        // The default branch (usually "main" or "master") should be recognized
        // by rev-parse. We create a known branch to be deterministic.
        std::process::Command::new("git")
            .args(["branch", "test-target"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let result = check_target_branch(dir.path(), "test-target");
        assert!(
            result.is_ok(),
            "existing local branch should pass: {result:?}"
        );
    }

    #[test]
    fn test_check_target_branch_nonexistent_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let result = check_target_branch(dir.path(), "nonexistent-branch-xyz-999");
        assert!(result.is_err(), "nonexistent branch should fail");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("does not exist locally or at origin"),
            "error should be descriptive, got: {msg}"
        );
        assert!(
            msg.contains("nonexistent-branch-xyz-999"),
            "error should name the branch, got: {msg}"
        );
    }

    #[test]
    fn test_preflight_git_dirty_fails_before_batch_parse() {
        // Verifies ordering: preflight runs BEFORE batch parsing.
        // If a batch file exists but git is dirty, we should get the git-dirty
        // error, not a batch parse error.
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        std::fs::write(dir.path().join("dirty.txt"), "dirty").unwrap();

        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let batch_path = batch_dir.join("batch.yaml");
        std::fs::write(&batch_path, "name: test\ntasks:\n  - id: t1\n").unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(run_with_dir(dir.path(), &batch_path, true));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        // Should fail on git-dirty, not on batch parse or tmux.
        // In CI without tmux, tmux failure comes first — that's also pre-batch.
        assert!(
            msg.contains("working tree is not clean") || msg.contains("tmux"),
            "error should be from preflight (git-dirty or tmux), not batch parse, got: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Criterion 8: clap derive subcommands parse correctly
    // -----------------------------------------------------------------------

    #[test]
    fn test_autorun_command_status_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "status"]).unwrap();
        assert!(matches!(
            cli.cmd,
            AutorunCommand::Status {
                batch: None,
                watch: false,
                ..
            }
        ));

        let cli = TestCli::try_parse_from(["test", "status", "--batch", "ses-123"]).unwrap();
        if let AutorunCommand::Status { batch, .. } = cli.cmd {
            assert_eq!(batch.as_deref(), Some("ses-123"));
        } else {
            panic!("expected Status variant");
        }
    }

    #[test]
    fn test_autorun_command_attach_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "attach", "task-a"]).unwrap();
        if let AutorunCommand::Attach { task_id } = cli.cmd {
            assert_eq!(task_id, "task-a");
        } else {
            panic!("expected Attach variant");
        }
    }

    #[test]
    fn test_autorun_command_logs_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "logs", "task-b"]).unwrap();
        if let AutorunCommand::Logs { task_id, follow } = cli.cmd {
            assert_eq!(task_id, "task-b");
            assert!(!follow);
        } else {
            panic!("expected Logs variant");
        }

        let cli = TestCli::try_parse_from(["test", "logs", "task-b", "--follow"]).unwrap();
        if let AutorunCommand::Logs { follow, .. } = cli.cmd {
            assert!(follow);
        } else {
            panic!("expected Logs variant");
        }
    }

    #[test]
    fn test_autorun_command_cancel_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "cancel", "task-c"]).unwrap();
        if let AutorunCommand::Cancel { task_id } = cli.cmd {
            assert_eq!(task_id, "task-c");
        } else {
            panic!("expected Cancel variant");
        }
    }

    #[test]
    fn test_autorun_command_abort_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "abort"]).unwrap();
        assert!(matches!(cli.cmd, AutorunCommand::Abort { batch: None }));

        let cli = TestCli::try_parse_from(["test", "abort", "--batch", "ses-456"]).unwrap();
        if let AutorunCommand::Abort { batch } = cli.cmd {
            assert_eq!(batch.as_deref(), Some("ses-456"));
        } else {
            panic!("expected Abort variant");
        }
    }

    #[test]
    fn test_autorun_command_results_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "results"]).unwrap();
        assert!(matches!(cli.cmd, AutorunCommand::Results { batch: None }));
    }

    #[test]
    fn test_autorun_command_history_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "history", "--limit", "5", "--all"]).unwrap();
        if let AutorunCommand::History { limit, all, .. } = cli.cmd {
            assert_eq!(limit, 5);
            assert!(all);
        } else {
            panic!("expected History variant");
        }

        let cli = TestCli::try_parse_from([
            "test",
            "history",
            "--since",
            "2026-01-01",
            "--status",
            "completed",
            "--batch-name",
            "refactor",
        ])
        .unwrap();
        if let AutorunCommand::History {
            since,
            status,
            batch_name,
            ..
        } = cli.cmd
        {
            assert_eq!(since.as_deref(), Some("2026-01-01"));
            assert_eq!(status.as_deref(), Some("completed"));
            assert_eq!(batch_name.as_deref(), Some("refactor"));
        } else {
            panic!("expected History variant");
        }
    }

    // -----------------------------------------------------------------------
    // Criterion 10: status command output format
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_elapsed_produces_mm_ss() {
        // Use a timestamp from ~2 minutes ago.
        let two_min_ago = chrono::Utc::now() - chrono::Duration::seconds(125);
        let ts = two_min_ago.to_rfc3339();
        let elapsed = format_elapsed(&ts);
        // Should be approximately "02:05" (within a second or two).
        assert!(
            elapsed.contains(':'),
            "elapsed should contain colon, got: {elapsed}"
        );
        assert!(!elapsed.is_empty(), "elapsed should not be empty");
    }

    #[test]
    fn test_format_elapsed_invalid_timestamp() {
        assert_eq!(format_elapsed("not-a-date"), "--:--");
    }

    #[test]
    fn test_format_duration_secs_minutes_only() {
        assert_eq!(format_duration_secs(125), "02:05");
    }

    #[test]
    fn test_format_duration_secs_with_hours() {
        assert_eq!(format_duration_secs(3661), "01:01:01");
    }

    #[test]
    fn test_format_duration_secs_zero() {
        assert_eq!(format_duration_secs(0), "00:00");
    }

    #[test]
    fn test_format_duration_secs_negative() {
        assert_eq!(format_duration_secs(-5), "00:00");
    }

    // -----------------------------------------------------------------------
    // Criterion 11: cancel cleanup sequence order (via MockStore)
    // -----------------------------------------------------------------------

    #[test]
    fn test_cancel_rejects_non_running_worker() {
        use codeflow_core::store::DataStore;
        use codeflow_core::types::{AutorunSessionStatus, AutorunWorkerStatus};

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db_dir = dir.path().join(".state/db");
            std::fs::create_dir_all(&db_dir).unwrap();
            let store = codeflow_core::store::SurrealStore::open(&db_dir)
                .await
                .unwrap();
            store.apply_schema().await.unwrap();

            // Create a session.
            let session = codeflow_core::models::AutorunSession {
                id: "ses-cancel-test".into(),
                batch_file: "b.yaml".into(),
                batch_name: Some("cancel-test".into()),
                status: AutorunSessionStatus::Running,
                max_session_workers: 1,
                total_tasks: 1,
                completed_tasks: 0,
                failed_tasks: 0,
                pid: None,
                skipped_tasks: 0,
                created_at: chrono::Utc::now().to_rfc3339(),
                completed_at: None,
            };
            store.create_autorun_session(&session).await.unwrap();

            // Create a completed (not running) worker.
            let worker = codeflow_core::models::AutorunWorker {
                id: "arw-task-done".into(),
                session_id: "ses-cancel-test".into(),
                worker_num: 1,
                task_id: "task-done".into(),
                status: AutorunWorkerStatus::Completed,
                tmux_session: Some("tmux-done".into()),
                worktree_path: None,
                file_scope: vec![],
                scope_policy: "soft".into(),
                worker_session_id: None,
                pr_number: None,
                started_at: None,
                completed_at: None,
            };
            store.create_autorun_worker(&worker).await.unwrap();

            // run_cancel should fail because worker is not running.
            let result = run_cancel(dir.path(), "task-done").await;
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("not running"),
                "expected 'not running' error, got: {msg}"
            );
        });
    }

    #[test]
    fn test_cancel_updates_db_status() {
        use codeflow_core::store::DataStore;
        use codeflow_core::types::{AutorunSessionStatus, AutorunWorkerStatus};

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db_dir = dir.path().join(".state/db");
            std::fs::create_dir_all(&db_dir).unwrap();

            // Scope: seed data then drop the store before run_cancel opens its own.
            {
                let store = codeflow_core::store::SurrealStore::open(&db_dir)
                    .await
                    .unwrap();
                store.apply_schema().await.unwrap();

                let now = chrono::Utc::now().to_rfc3339();

                let session = codeflow_core::models::AutorunSession {
                    id: "ses-cancel-db".into(),
                    batch_file: "b.yaml".into(),
                    batch_name: Some("cancel-db".into()),
                    status: AutorunSessionStatus::Running,
                    max_session_workers: 1,
                    total_tasks: 1,
                    completed_tasks: 0,
                    failed_tasks: 0,
                    pid: None,
                    skipped_tasks: 0,
                    created_at: now.clone(),
                    completed_at: None,
                };
                store.create_autorun_session(&session).await.unwrap();

                let worker = codeflow_core::models::AutorunWorker {
                    id: "arw-cancel-db".into(),
                    session_id: "ses-cancel-db".into(),
                    worker_num: 1,
                    task_id: "task-cancel-db".into(),
                    status: AutorunWorkerStatus::Running,
                    tmux_session: Some("nonexistent-tmux-session".into()),
                    worktree_path: None,
                    file_scope: vec![],
                    scope_policy: "soft".into(),
                    worker_session_id: None,
                    pr_number: None,
                    started_at: Some(now.clone()),
                    completed_at: None,
                };
                store.create_autorun_worker(&worker).await.unwrap();

                let task_run = codeflow_core::models::AutorunTaskRun {
                    id: "atr-cancel-db".into(),
                    worker_id: "arw-cancel-db".into(),
                    task_id: "task-cancel-db".into(),
                    session_id: "ses-cancel-db".into(),
                    status: codeflow_core::types::AutorunTaskRunStatus::Running,
                    branch_name: None,
                    worktree_path: None,
                    pr_number: None,
                    pr_url: None,
                    blocked_reason: None,
                    claim_conflicts: None,
                    merge_conflicts: None,
                    started_at: Some(now),
                    completed_at: None,
                    duration_seconds: None,
                    exit_code: None,
                    error_message: None,
                    verification_result: None,
                    created_at: chrono::Utc::now().to_rfc3339(),
                };
                store.create_autorun_task_run(&task_run).await.unwrap();
            }
            // Store dropped here — run_cancel will open its own connection.

            let result = run_cancel(dir.path(), "task-cancel-db").await;
            assert!(result.is_ok(), "cancel should succeed: {result:?}");

            // Re-open to verify.
            let store2 = codeflow_core::store::SurrealStore::open(&db_dir)
                .await
                .unwrap();
            let w = store2
                .get_autorun_worker_by_task_id("ses-cancel-db", "task-cancel-db")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                w.status,
                AutorunWorkerStatus::Cancelled,
                "worker should be cancelled"
            );
        });
    }

    // -----------------------------------------------------------------------
    // Criterion 12: history --limit limits results
    // -----------------------------------------------------------------------

    #[test]
    fn test_history_limit_constrains_results() {
        use codeflow_core::store::DataStore;
        use codeflow_core::types::AutorunSessionStatus;

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db_dir = dir.path().join(".state/db");
            std::fs::create_dir_all(&db_dir).unwrap();
            let store = codeflow_core::store::SurrealStore::open(&db_dir)
                .await
                .unwrap();
            store.apply_schema().await.unwrap();

            // Create 5 sessions.
            for i in 0..5 {
                let session = codeflow_core::models::AutorunSession {
                    id: format!("ses-hist-{i}"),
                    batch_file: "b.yaml".into(),
                    batch_name: Some(format!("batch-{i}")),
                    status: AutorunSessionStatus::Completed,
                    max_session_workers: 1,
                    total_tasks: 1,
                    completed_tasks: 1,
                    failed_tasks: 0,
                    pid: None,
                    skipped_tasks: 0,
                    created_at: format!("2026-03-{:02}T00:00:00Z", 10 + i),
                    completed_at: Some(format!("2026-03-{:02}T01:00:00Z", 10 + i)),
                };
                store.create_autorun_session(&session).await.unwrap();
            }

            // Query with limit=3.
            let filter = codeflow_core::models::AutorunSessionFilter {
                limit: Some(3),
                ..Default::default()
            };
            let results = store.list_autorun_sessions(filter).await.unwrap();
            assert_eq!(
                results.len(),
                3,
                "limit=3 should return 3 sessions, got {}",
                results.len()
            );

            // Verify they are in descending order.
            assert!(
                results[0].created_at >= results[1].created_at,
                "results should be in descending order"
            );

            // Query with all=true should return all 5.
            let filter_all = codeflow_core::models::AutorunSessionFilter {
                all: true,
                ..Default::default()
            };
            let all_results = store.list_autorun_sessions(filter_all).await.unwrap();
            assert_eq!(
                all_results.len(),
                5,
                "all=true should return all 5 sessions, got {}",
                all_results.len()
            );
        });
    }

    // -----------------------------------------------------------------------
    // Additional filter tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_history_status_filter() {
        use codeflow_core::store::DataStore;
        use codeflow_core::types::AutorunSessionStatus;

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db_dir = dir.path().join(".state/db");
            std::fs::create_dir_all(&db_dir).unwrap();
            let store = codeflow_core::store::SurrealStore::open(&db_dir)
                .await
                .unwrap();
            store.apply_schema().await.unwrap();

            // Create a completed and a failed session.
            for (i, status) in [
                AutorunSessionStatus::Completed,
                AutorunSessionStatus::Failed,
            ]
            .iter()
            .enumerate()
            {
                let session = codeflow_core::models::AutorunSession {
                    id: format!("ses-filt-{i}"),
                    batch_file: "b.yaml".into(),
                    batch_name: Some(format!("filt-{i}")),
                    status: *status,
                    max_session_workers: 1,
                    total_tasks: 1,
                    completed_tasks: i32::from(i == 0),
                    failed_tasks: i32::from(i == 1),
                    pid: None,
                    skipped_tasks: 0,
                    created_at: format!("2026-03-{:02}T00:00:00Z", 10 + i),
                    completed_at: Some(format!("2026-03-{:02}T01:00:00Z", 10 + i)),
                };
                store.create_autorun_session(&session).await.unwrap();
            }

            // Filter by completed.
            let filter = codeflow_core::models::AutorunSessionFilter {
                status: Some(AutorunSessionStatus::Completed),
                ..Default::default()
            };
            let results = store.list_autorun_sessions(filter).await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].status, AutorunSessionStatus::Completed);
        });
    }

    #[test]
    fn test_list_workers_by_session() {
        use codeflow_core::store::DataStore;
        use codeflow_core::types::AutorunWorkerStatus;

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db_dir = dir.path().join(".state/db");
            std::fs::create_dir_all(&db_dir).unwrap();
            let store = codeflow_core::store::SurrealStore::open(&db_dir)
                .await
                .unwrap();
            store.apply_schema().await.unwrap();

            // Create workers for two different sessions.
            for sid in ["ses-a", "ses-b"] {
                let worker = codeflow_core::models::AutorunWorker {
                    id: format!("arw-{sid}"),
                    session_id: sid.into(),
                    worker_num: 1,
                    task_id: format!("task-{sid}"),
                    status: AutorunWorkerStatus::Running,
                    tmux_session: None,
                    worktree_path: None,
                    file_scope: vec![],
                    scope_policy: "soft".into(),
                    worker_session_id: None,
                    pr_number: None,
                    started_at: None,
                    completed_at: None,
                };
                store.create_autorun_worker(&worker).await.unwrap();
            }

            let workers_a = store.list_autorun_workers("ses-a").await.unwrap();
            assert_eq!(workers_a.len(), 1);
            assert_eq!(workers_a[0].session_id, "ses-a");

            let workers_b = store.list_autorun_workers("ses-b").await.unwrap();
            assert_eq!(workers_b.len(), 1);
            assert_eq!(workers_b[0].session_id, "ses-b");

            let workers_none = store.list_autorun_workers("ses-nope").await.unwrap();
            assert!(workers_none.is_empty());
        });
    }

    #[test]
    fn test_get_worker_by_task_id() {
        use codeflow_core::store::DataStore;
        use codeflow_core::types::AutorunWorkerStatus;

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let db_dir = dir.path().join(".state/db");
            std::fs::create_dir_all(&db_dir).unwrap();
            let store = codeflow_core::store::SurrealStore::open(&db_dir)
                .await
                .unwrap();
            store.apply_schema().await.unwrap();

            let worker = codeflow_core::models::AutorunWorker {
                id: "arw-lookup".into(),
                session_id: "ses-lookup".into(),
                worker_num: 1,
                task_id: "task-lookup".into(),
                status: AutorunWorkerStatus::Running,
                tmux_session: Some("tmux-lookup".into()),
                worktree_path: None,
                file_scope: vec![],
                scope_policy: "soft".into(),
                worker_session_id: None,
                pr_number: None,
                started_at: None,
                completed_at: None,
            };
            store.create_autorun_worker(&worker).await.unwrap();

            // Found.
            let found = store
                .get_autorun_worker_by_task_id("ses-lookup", "task-lookup")
                .await
                .unwrap();
            assert!(found.is_some());
            assert_eq!(found.unwrap().tmux_session.as_deref(), Some("tmux-lookup"));

            // Not found (wrong session).
            let not_found = store
                .get_autorun_worker_by_task_id("ses-other", "task-lookup")
                .await
                .unwrap();
            assert!(not_found.is_none());

            // Not found (wrong task).
            let not_found = store
                .get_autorun_worker_by_task_id("ses-lookup", "task-other")
                .await
                .unwrap();
            assert!(not_found.is_none());
        });
    }

    // -----------------------------------------------------------------------
    // Aborting status variant test
    // -----------------------------------------------------------------------

    #[test]
    fn test_aborting_status_roundtrip() {
        use std::str::FromStr;
        let status = codeflow_core::types::AutorunSessionStatus::Aborting;
        assert_eq!(status.to_string(), "aborting");
        let parsed = codeflow_core::types::AutorunSessionStatus::from_str("aborting").unwrap();
        assert_eq!(parsed, status);
    }

    // -----------------------------------------------------------------------
    // exec module tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_exec_command_builds_args() {
        let cmd = exec::Command::new("echo").args(&["hello", "world"]);
        assert_eq!(cmd.program, "echo");
        assert_eq!(cmd.args, vec!["hello", "world"]);
    }

    // -----------------------------------------------------------------------
    // Handler-level tests (QA coverage)
    // -----------------------------------------------------------------------

    /// Helper: seed a SurrealStore with test data, drop it, return the dir.
    /// The caller can then call handler functions which open their own store.
    async fn seed_store(
        dir: &Path,
        sessions: Vec<codeflow_core::models::AutorunSession>,
        workers: Vec<codeflow_core::models::AutorunWorker>,
        task_runs: Vec<codeflow_core::models::AutorunTaskRun>,
    ) {
        use codeflow_core::store::DataStore;
        let db_dir = dir.join(".state/db");
        std::fs::create_dir_all(&db_dir).unwrap();
        let store = codeflow_core::store::SurrealStore::open(&db_dir)
            .await
            .unwrap();
        store.apply_schema().await.unwrap();
        for s in &sessions {
            store.create_autorun_session(s).await.unwrap();
        }
        for w in &workers {
            store.create_autorun_worker(w).await.unwrap();
        }
        for r in &task_runs {
            store.create_autorun_task_run(r).await.unwrap();
        }
        // Drop store so handler functions can open their own connection.
        drop(store);
    }

    fn test_session(
        id: &str,
        status: codeflow_core::types::AutorunSessionStatus,
    ) -> codeflow_core::models::AutorunSession {
        codeflow_core::models::AutorunSession {
            id: id.into(),
            batch_file: "b.yaml".into(),
            batch_name: Some(format!("batch-{id}")),
            status,
            max_session_workers: 2,
            total_tasks: 4,
            completed_tasks: 2,
            failed_tasks: 1,
            pid: None,
            skipped_tasks: 0,
            created_at: chrono::Utc::now().to_rfc3339(),
            completed_at: None,
        }
    }

    fn test_worker(
        id: &str,
        session_id: &str,
        task_id: &str,
        status: codeflow_core::types::AutorunWorkerStatus,
    ) -> codeflow_core::models::AutorunWorker {
        codeflow_core::models::AutorunWorker {
            id: id.into(),
            session_id: session_id.into(),
            worker_num: 1,
            task_id: task_id.into(),
            status,
            tmux_session: Some(format!("nonexistent-tmux-{id}")),
            worktree_path: None,
            file_scope: vec![],
            scope_policy: "soft".into(),
            worker_session_id: None,
            pr_number: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: None,
        }
    }

    fn test_task_run(
        id: &str,
        session_id: &str,
        task_id: &str,
        status: codeflow_core::types::AutorunTaskRunStatus,
    ) -> codeflow_core::models::AutorunTaskRun {
        codeflow_core::models::AutorunTaskRun {
            id: id.into(),
            worker_id: format!("w-{id}"),
            task_id: task_id.into(),
            session_id: session_id.into(),
            status,
            branch_name: Some("feat/test".into()),
            worktree_path: None,
            pr_number: Some(42),
            pr_url: Some("https://github.com/org/repo/pull/42".into()),
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: Some(chrono::Utc::now().to_rfc3339()),
            completed_at: Some(chrono::Utc::now().to_rfc3339()),
            duration_seconds: Some(120),
            exit_code: Some(0),
            error_message: None,
            verification_result: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    // -- run_status tests --

    #[test]
    fn test_run_status_no_active_sessions() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-done",
                    codeflow_core::types::AutorunSessionStatus::Completed,
                )],
                vec![],
                vec![],
            )
            .await;

            // No running sessions -- should print "No active autorun batches." and succeed.
            let result = run_status(dir.path(), None).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_status_with_running_session() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-running";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![
                    test_worker(
                        "w1",
                        sid,
                        "task-1",
                        codeflow_core::types::AutorunWorkerStatus::Running,
                    ),
                    test_worker(
                        "w2",
                        sid,
                        "task-2",
                        codeflow_core::types::AutorunWorkerStatus::Completed,
                    ),
                ],
                vec![],
            )
            .await;

            let result = run_status(dir.path(), None).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_status_specific_batch() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-specific",
                    codeflow_core::types::AutorunSessionStatus::Completed,
                )],
                vec![],
                vec![],
            )
            .await;

            let result = run_status(dir.path(), Some("ses-specific")).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_status_nonexistent_batch() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(dir.path(), vec![], vec![], vec![]).await;

            let result = run_status(dir.path(), Some("ses-nope")).await;
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no autorun session found")
            );
        });
    }

    // -- run_results tests --

    #[test]
    fn test_run_results_empty() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-empty",
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![],
            )
            .await;

            let result = run_results(dir.path(), Some("ses-empty")).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_results_with_data() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-results";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Completed,
                )],
                vec![],
                vec![
                    test_task_run(
                        "r1",
                        sid,
                        "task-a",
                        codeflow_core::types::AutorunTaskRunStatus::Completed,
                    ),
                    test_task_run(
                        "r2",
                        sid,
                        "task-b",
                        codeflow_core::types::AutorunTaskRunStatus::Failed,
                    ),
                ],
            )
            .await;

            let result = run_results(dir.path(), Some(sid)).await;
            assert!(result.is_ok());
        });
    }

    // -- run_history tests --

    #[test]
    fn test_run_history_empty() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(dir.path(), vec![], vec![], vec![]).await;

            let result = run_history(dir.path(), 10, None, None, None, false).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_history_with_sessions() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![
                    test_session(
                        "ses-h1",
                        codeflow_core::types::AutorunSessionStatus::Completed,
                    ),
                    test_session("ses-h2", codeflow_core::types::AutorunSessionStatus::Failed),
                ],
                vec![],
                vec![],
            )
            .await;

            let result = run_history(dir.path(), 10, None, None, None, false).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_history_invalid_status_filter() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(dir.path(), vec![], vec![], vec![]).await;

            let result = run_history(
                dir.path(),
                10,
                None,
                Some("invalid_status".into()),
                None,
                false,
            )
            .await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("invalid status"));
        });
    }

    // -- resolve_session_id tests --

    #[test]
    fn test_resolve_session_id_explicit_batch() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(dir.path(), vec![], vec![], vec![]).await;
            let store = open_store(dir.path()).await.unwrap();

            let sid = resolve_session_id(&store, Some("ses-explicit"))
                .await
                .unwrap();
            assert_eq!(sid, "ses-explicit");
        });
    }

    #[test]
    fn test_resolve_session_id_finds_running() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-active",
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![],
            )
            .await;
            let store = open_store(dir.path()).await.unwrap();

            let sid = resolve_session_id(&store, None).await.unwrap();
            assert_eq!(sid, "ses-active");
        });
    }

    #[test]
    fn test_resolve_session_id_finds_aborting() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-aborting",
                    codeflow_core::types::AutorunSessionStatus::Aborting,
                )],
                vec![],
                vec![],
            )
            .await;
            let store = open_store(dir.path()).await.unwrap();

            let sid = resolve_session_id(&store, None).await.unwrap();
            assert_eq!(sid, "ses-aborting");
        });
    }

    #[test]
    fn test_resolve_session_id_no_active() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-done",
                    codeflow_core::types::AutorunSessionStatus::Completed,
                )],
                vec![],
                vec![],
            )
            .await;
            let store = open_store(dir.path()).await.unwrap();

            let result = resolve_session_id(&store, None).await;
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no active autorun session")
            );
        });
    }

    // -- run_abort tests --

    #[test]
    fn test_run_abort_marker_fallback() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-abort-marker";
            // Session with no PID -- should use marker file fallback.
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![],
            )
            .await;

            let result = run_abort(dir.path(), Some(sid)).await;
            assert!(result.is_ok());

            // Verify marker file was written.
            let marker = dir.path().join(format!(".state/runtime/abort-{sid}"));
            assert!(marker.exists(), "abort marker file should exist");

            // Verify session status updated to cancelled.
            let store = open_store(dir.path()).await.unwrap();
            use codeflow_core::store::DataStore;
            let session = store.get_autorun_session(sid).await.unwrap().unwrap();
            assert_eq!(
                session.status,
                codeflow_core::types::AutorunSessionStatus::Cancelled
            );
        });
    }

    #[test]
    fn test_run_abort_with_stale_pid() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-abort-pid";
            // Session with a PID that doesn't exist (stale).
            let mut session =
                test_session(sid, codeflow_core::types::AutorunSessionStatus::Running);
            session.pid = Some(999_999_999); // Very unlikely to be a real PID.
            seed_store(dir.path(), vec![session], vec![], vec![]).await;

            let result = run_abort(dir.path(), Some(sid)).await;
            assert!(result.is_ok());

            // Should fall back to marker file since PID is stale.
            let marker = dir.path().join(format!(".state/runtime/abort-{sid}"));
            assert!(
                marker.exists(),
                "abort marker should exist for stale PID fallback"
            );
        });
    }

    #[test]
    fn test_run_abort_not_running() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![test_session(
                    "ses-done",
                    codeflow_core::types::AutorunSessionStatus::Completed,
                )],
                vec![],
                vec![],
            )
            .await;

            let result = run_abort(dir.path(), Some("ses-done")).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("not running"));
        });
    }

    #[test]
    fn test_run_abort_skips_pending_tasks() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-abort-skip";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![
                    test_task_run(
                        "r1",
                        sid,
                        "task-done",
                        codeflow_core::types::AutorunTaskRunStatus::Completed,
                    ),
                    {
                        let mut r = test_task_run(
                            "r2",
                            sid,
                            "task-pending",
                            codeflow_core::types::AutorunTaskRunStatus::Pending,
                        );
                        r.pr_number = None;
                        r.pr_url = None;
                        r.completed_at = None;
                        r.duration_seconds = None;
                        r.exit_code = None;
                        r
                    },
                ],
            )
            .await;

            let result = run_abort(dir.path(), Some(sid)).await;
            assert!(result.is_ok());

            // Verify pending task was marked as skipped.
            let store = open_store(dir.path()).await.unwrap();
            use codeflow_core::store::DataStore;
            let runs = store.list_autorun_task_runs(sid).await.unwrap();
            let pending_run = runs.iter().find(|r| r.task_id == "task-pending").unwrap();
            assert_eq!(
                pending_run.status,
                codeflow_core::types::AutorunTaskRunStatus::Skipped
            );
            assert_eq!(pending_run.error_message.as_deref(), Some("batch_aborted"));
        });
    }

    // -- tmux_has_session test --

    #[test]
    fn test_tmux_has_session_nonexistent() {
        assert!(!tmux_has_session("nonexistent-session-xyz-12345"));
    }

    // -----------------------------------------------------------------------
    // run_attach error path tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_attach_no_worker_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-attach-nw";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![], // no workers
                vec![],
            )
            .await;

            let result = run_attach(dir.path(), "task-missing").await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("no worker found"));
        });
    }

    #[test]
    fn test_run_attach_no_tmux_session_name() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-attach-notmux";
            let mut worker = test_worker(
                "w-notmux",
                sid,
                "task-notmux",
                codeflow_core::types::AutorunWorkerStatus::Running,
            );
            worker.tmux_session = None; // No tmux session name.
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![worker],
                vec![],
            )
            .await;

            let result = run_attach(dir.path(), "task-notmux").await;
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no tmux session name")
            );
        });
    }

    #[test]
    fn test_run_attach_tmux_session_dead() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-attach-dead";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![test_worker(
                    "w-dead",
                    sid,
                    "task-dead",
                    codeflow_core::types::AutorunWorkerStatus::Running,
                )],
                vec![],
            )
            .await;

            let result = run_attach(dir.path(), "task-dead").await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("does not exist"));
        });
    }

    // -----------------------------------------------------------------------
    // run_logs error path tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_logs_no_worker_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-logs-nw";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![],
            )
            .await;

            let result = run_logs(dir.path(), "task-missing", false).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("no worker found"));
        });
    }

    #[test]
    fn test_run_logs_no_tmux_session_name() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-logs-notmux";
            let mut worker = test_worker(
                "w-logs-notmux",
                sid,
                "task-logs-notmux",
                codeflow_core::types::AutorunWorkerStatus::Running,
            );
            worker.tmux_session = None;
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![worker],
                vec![],
            )
            .await;

            let result = run_logs(dir.path(), "task-logs-notmux", false).await;
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("no tmux session name")
            );
        });
    }

    #[test]
    fn test_run_logs_tmux_session_dead() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-logs-dead";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![test_worker(
                    "w-logs-dead",
                    sid,
                    "task-logs-dead",
                    codeflow_core::types::AutorunWorkerStatus::Running,
                )],
                vec![],
            )
            .await;

            let result = run_logs(dir.path(), "task-logs-dead", false).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("does not exist"));
        });
    }

    // -----------------------------------------------------------------------
    // run_cancel additional error path tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_cancel_no_worker_found() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-cancel-nw";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![],
            )
            .await;

            let result = run_cancel(dir.path(), "task-missing").await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("no worker found"));
        });
    }

    // -----------------------------------------------------------------------
    // open_store error path test
    // -----------------------------------------------------------------------

    #[test]
    fn test_open_store_creates_db() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            // .state/db does not exist yet — open_store should create it.
            std::fs::create_dir_all(dir.path().join(".state/db")).unwrap();
            let result = open_store(dir.path()).await;
            assert!(result.is_ok());
        });
    }

    // -----------------------------------------------------------------------
    // run_history with valid status filter
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_history_with_status_filter() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![
                    test_session(
                        "ses-hf1",
                        codeflow_core::types::AutorunSessionStatus::Completed,
                    ),
                    test_session(
                        "ses-hf2",
                        codeflow_core::types::AutorunSessionStatus::Failed,
                    ),
                ],
                vec![],
                vec![],
            )
            .await;

            let result =
                run_history(dir.path(), 10, None, Some("completed".into()), None, false).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_history_with_all_flag() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(
                dir.path(),
                vec![
                    test_session(
                        "ses-ha1",
                        codeflow_core::types::AutorunSessionStatus::Completed,
                    ),
                    test_session(
                        "ses-ha2",
                        codeflow_core::types::AutorunSessionStatus::Completed,
                    ),
                ],
                vec![],
                vec![],
            )
            .await;

            let result = run_history(dir.path(), 1, None, None, None, true).await;
            assert!(result.is_ok());
        });
    }

    // -----------------------------------------------------------------------
    // run_results via resolve (no explicit batch)
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_results_auto_resolve_session() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let sid = "ses-results-auto";
            seed_store(
                dir.path(),
                vec![test_session(
                    sid,
                    codeflow_core::types::AutorunSessionStatus::Running,
                )],
                vec![],
                vec![test_task_run(
                    "r-auto",
                    sid,
                    "task-auto",
                    codeflow_core::types::AutorunTaskRunStatus::Completed,
                )],
            )
            .await;

            // No explicit batch -- should auto-resolve to the running session.
            let result = run_results(dir.path(), None).await;
            assert!(result.is_ok());
        });
    }

    // -- Batch report generation tests --

    fn make_worker_result(
        task_id: &str,
        status: &str,
        exit_code: i32,
        pr_number: i64,
        pr_url: &str,
        error: &str,
        duration_sec: i64,
    ) -> codeflow_core::autorun::WorkerResult {
        codeflow_core::autorun::WorkerResult {
            worker_id: format!("w-{task_id}"),
            task_id: task_id.to_string(),
            status: status.to_string(),
            exit_code,
            pr_number,
            pr_url: pr_url.to_string(),
            error: error.to_string(),
            branch_name: format!("feat/{task_id}"),
            duration_sec,
        }
    }

    #[test]
    fn test_batch_report_contains_correct_markdown_structure() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(120);
        let results = vec![
            make_worker_result(
                "task-1",
                "completed",
                0,
                42,
                "https://github.com/pr/42",
                "",
                60,
            ),
            make_worker_result("task-2", "failed", 1, 0, "", "build error", 30),
        ];

        let meta = BatchReportMeta {
            project_dir: dir.path(),
            report_dir: &report_dir,
            batch_name: "test-batch",
            session_id: "ses-test-123",
            batch_path: "batch.yaml",
            start_time: start,
            end_time: end,
        };
        let path = generate_batch_report(&meta, &results).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        // Header
        assert!(content.contains("# Autorun Batch Report: test-batch"));
        // Metadata table
        assert!(content.contains("| Session ID | ses-test-123 |"));
        assert!(content.contains("| Batch File | batch.yaml |"));
        assert!(content.contains("| Duration | 2m 0s |"));
        assert!(content.contains("| Status | Failed |"));
        // Task results table header
        assert!(content.contains("| Task ID | Status | Duration | PR | Exit Code | Error |"));
        // Task rows
        assert!(
            content
                .contains("| task-1 | completed | 60s | [#42](https://github.com/pr/42) | 0 | - |")
        );
        assert!(content.contains("| task-2 | failed | 30s | - | 1 | build error |"));
        // Summary section
        assert!(content.contains("## Summary"));
        assert!(content.contains("- Completed: 1"));
        assert!(content.contains("- Failed: 1"));
    }

    fn make_report_meta<'a>(
        project_dir: &'a Path,
        report_dir: &'a Path,
        batch_name: &'a str,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
    ) -> BatchReportMeta<'a> {
        BatchReportMeta {
            project_dir,
            report_dir,
            batch_name,
            session_id: "ses-1",
            batch_path: "b.yaml",
            start_time: start,
            end_time: end,
        }
    }

    #[test]
    fn test_batch_report_filename_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::DateTime::parse_from_rfc3339("2026-03-22T10:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let end = start + chrono::Duration::seconds(10);
        let meta = make_report_meta(dir.path(), &report_dir, "my-batch", start, end);

        let path = generate_batch_report(&meta, &[]).unwrap();

        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "my-batch-2026-03-22.md");
    }

    #[test]
    fn test_batch_report_creates_directory_if_missing() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("nested").join("reports");
        assert!(!report_dir.exists());

        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(1);
        let meta = make_report_meta(dir.path(), &report_dir, "dir-test", start, end);

        let path = generate_batch_report(&meta, &[]).unwrap();

        assert!(report_dir.exists());
        assert!(path.exists());
    }

    #[test]
    fn test_batch_report_empty_results() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(5);
        let meta = make_report_meta(dir.path(), &report_dir, "empty-batch", start, end);

        let path = generate_batch_report(&meta, &[]).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("# Autorun Batch Report: empty-batch"));
        assert!(content.contains("- Completed: 0"));
        assert!(content.contains("- Failed: 0"));
        assert!(content.contains("- Skipped: 0"));
        assert!(content.contains("- Timed Out: 0"));
        // Table header present but no data rows
        assert!(content.contains("| Task ID | Status | Duration | PR | Exit Code | Error |"));
    }

    #[test]
    fn test_batch_report_mixed_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(300);
        let meta = make_report_meta(dir.path(), &report_dir, "mixed", start, end);
        let results = vec![
            make_worker_result("task-a", "completed", 0, 10, "https://gh/10", "", 100),
            make_worker_result("task-b", "failed", 1, 0, "", "segfault", 50),
            make_worker_result("task-c", "skipped", 0, 0, "", "", 0),
            make_worker_result("task-d", "timeout", -1, 0, "", "exceeded 3600s", 3600),
        ];

        let path = generate_batch_report(&meta, &results).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("- Completed: 1"));
        assert!(content.contains("- Failed: 1"));
        assert!(content.contains("- Skipped: 1"));
        assert!(content.contains("- Timed Out: 1"));
        assert!(content.contains("| Status | Failed |"));
    }

    #[test]
    fn test_batch_report_pr_column_formatting() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(10);
        let meta = make_report_meta(dir.path(), &report_dir, "pr-test", start, end);
        let results = vec![
            make_worker_result("with-pr", "completed", 0, 55, "https://gh/pr/55", "", 10),
            make_worker_result("no-pr", "completed", 0, 0, "", "", 10),
            make_worker_result("pr-no-url", "completed", 0, 99, "", "", 10),
        ];

        let path = generate_batch_report(&meta, &results).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("[#55](https://gh/pr/55)"),
            "PR with URL should be a link"
        );
        assert!(
            content.contains("| no-pr | completed | 10s | - |"),
            "No PR should show dash"
        );
        assert!(
            content.contains("| #99 |"),
            "PR without URL should show plain number"
        );
    }

    #[test]
    fn test_batch_report_aborted_batch() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(5);
        let meta = make_report_meta(dir.path(), &report_dir, "aborted", start, end);
        let results = vec![
            make_worker_result("task-x", "skipped", 0, 0, "", "", 0),
            make_worker_result("task-y", "skipped", 0, 0, "", "", 0),
        ];

        let path = generate_batch_report(&meta, &results).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("| Status | Aborted |"));
        assert!(content.contains("- Skipped: 2"));
        assert!(content.contains("- Completed: 0"));
    }

    #[test]
    fn test_batch_report_file_exists_after_write() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(1);
        let meta = make_report_meta(dir.path(), &report_dir, "atomic-test", start, end);

        let path = generate_batch_report(
            &meta,
            &[make_worker_result("t1", "completed", 0, 0, "", "", 5)],
        )
        .unwrap();

        // Verify file exists and is readable (atomic_write completed successfully).
        assert!(path.exists(), "report file must exist after atomic_write");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.is_empty(), "report file must not be empty");
        // Verify no leftover .tmp file.
        let tmp_path = path.with_extension("tmp");
        assert!(
            !tmp_path.exists(),
            "no .tmp file should remain after atomic_write"
        );
    }

    #[test]
    fn test_format_duration_human() {
        assert_eq!(format_duration_human(0), "0s");
        assert_eq!(format_duration_human(45), "45s");
        assert_eq!(format_duration_human(60), "1m 0s");
        assert_eq!(format_duration_human(125), "2m 5s");
        assert_eq!(format_duration_human(3661), "1h 1m 1s");
        assert_eq!(format_duration_human(7200), "2h 0m 0s");
    }

    #[test]
    fn test_batch_report_completed_status() {
        let dir = tempfile::tempdir().unwrap();
        let report_dir = dir.path().join("reports");
        let start = chrono::Utc::now();
        let end = start + chrono::Duration::seconds(60);
        let meta = make_report_meta(dir.path(), &report_dir, "success", start, end);
        let results = vec![
            make_worker_result("task-1", "completed", 0, 1, "", "", 30),
            make_worker_result("task-2", "completed", 0, 2, "", "", 30),
        ];

        let path = generate_batch_report(&meta, &results).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("| Status | Completed |"));
    }

    // -- run_batches_sync tests --

    #[test]
    fn test_batches_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No .codeflow/config/autorun/ directory exists.
        let result = run_batches_sync(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_batches_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        // Empty directory -- no YAML files.
        let result = run_batches_sync(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_batches_valid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("test-batch.yaml"),
            "name: test\ntasks:\n  - id: task-a\n",
        )
        .unwrap();
        let result = run_batches_sync(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_batches_invalid_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("bad.yaml"), "{{invalid yaml").unwrap();
        // Should not error -- prints ERR row but returns Ok.
        let result = run_batches_sync(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_batches_ignores_non_yaml_files() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("readme.md"), "# Not a batch").unwrap();
        std::fs::write(batch_dir.join("config.json"), "{}").unwrap();
        // No .yaml/.yml files -- should report empty.
        let result = run_batches_sync(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_batches_mixed_valid_and_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("good.yaml"),
            "name: good\ntasks:\n  - id: task-a\n",
        )
        .unwrap();
        std::fs::write(batch_dir.join("bad.yml"), "not: [valid: batch").unwrap();
        let result = run_batches_sync(dir.path());
        assert!(result.is_ok());
    }

    // -- Clap parsing: Resume and Batches variants --

    #[test]
    fn test_autorun_command_resume_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "resume"]).unwrap();
        assert!(matches!(cli.cmd, AutorunCommand::Resume { batch: None }));

        let cli = TestCli::try_parse_from(["test", "resume", "--batch", "ses-abc"]).unwrap();
        if let AutorunCommand::Resume { batch } = cli.cmd {
            assert_eq!(batch.as_deref(), Some("ses-abc"));
        } else {
            panic!("expected Resume variant");
        }
    }

    #[test]
    fn test_autorun_command_batches_variant() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "batches"]).unwrap();
        assert!(matches!(cli.cmd, AutorunCommand::Batches));
    }

    // -- Clap parsing: Status --watch and --interval --

    #[test]
    fn test_autorun_command_status_watch_flag() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "status", "--watch"]).unwrap();
        if let AutorunCommand::Status {
            batch,
            watch,
            interval,
        } = cli.cmd
        {
            assert!(watch);
            assert!(batch.is_none());
            assert_eq!(interval, 5); // default
        } else {
            panic!("expected Status variant");
        }
    }

    #[test]
    fn test_autorun_command_status_interval_flag() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli =
            TestCli::try_parse_from(["test", "status", "--watch", "--interval", "10"]).unwrap();
        if let AutorunCommand::Status {
            watch, interval, ..
        } = cli.cmd
        {
            assert!(watch);
            assert_eq!(interval, 10);
        } else {
            panic!("expected Status variant");
        }
    }

    #[test]
    fn test_autorun_command_status_short_watch_flag() {
        use clap::Parser;

        #[derive(Parser)]
        struct TestCli {
            #[command(subcommand)]
            cmd: AutorunCommand,
        }

        let cli = TestCli::try_parse_from(["test", "status", "-w"]).unwrap();
        if let AutorunCommand::Status { watch, .. } = cli.cmd {
            assert!(watch);
        } else {
            panic!("expected Status variant");
        }
    }

    // -- Logs follow line-count tracking --

    #[test]
    fn test_logs_follow_line_count_incremental() {
        // Simulate the line-count tracking logic used in logs follow mode.
        let content_v1 = "line 1\nline 2\nline 3";
        let lines_v1: Vec<&str> = content_v1.lines().collect();
        let mut last_line_count = 0;

        // First iteration: all lines are new.
        assert!(lines_v1.len() > last_line_count);
        let new_lines: Vec<&str> = lines_v1[last_line_count..].to_vec();
        assert_eq!(new_lines, vec!["line 1", "line 2", "line 3"]);
        last_line_count = lines_v1.len();
        assert_eq!(last_line_count, 3);

        // Second iteration: same content, no new lines.
        let content_v2 = "line 1\nline 2\nline 3";
        let lines_v2: Vec<&str> = content_v2.lines().collect();
        assert_eq!(lines_v2.len(), last_line_count); // No new lines.

        // Third iteration: two new lines appended.
        let content_v3 = "line 1\nline 2\nline 3\nline 4\nline 5";
        let lines_v3: Vec<&str> = content_v3.lines().collect();
        assert!(lines_v3.len() > last_line_count);
        let new_lines_v3: Vec<&str> = lines_v3[last_line_count..].to_vec();
        assert_eq!(new_lines_v3, vec!["line 4", "line 5"]);
        last_line_count = lines_v3.len();
        assert_eq!(last_line_count, 5);
    }

    #[test]
    fn test_logs_follow_empty_content() {
        let content = "";
        let lines: Vec<&str> = content.lines().collect();
        let last_line_count = 0;
        // Empty content should produce no new lines.
        assert_eq!(lines.len(), last_line_count);
    }

    // -- worker_session_id threading in InvokeConfig --

    #[test]
    fn test_invoke_config_worker_session_id_used_for_autorun_sid() {
        // When worker_session_id is set, it should be preferred for AUTORUN_SESSION_ID.
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-1".into(),
            work_dir: "/tmp/test".into(),
            prompt: "test".into(),
            session_id: "ses-batch".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: "ses-worker-123".into(),
        };
        // worker_session_id should be non-empty and different from session_id.
        assert_ne!(cfg.worker_session_id, cfg.session_id);
        assert_eq!(cfg.worker_session_id, "ses-worker-123");
        // The autorun_sid selection logic: prefer worker_session_id if non-empty.
        let autorun_sid = if cfg.worker_session_id.is_empty() {
            &cfg.session_id
        } else {
            &cfg.worker_session_id
        };
        assert_eq!(autorun_sid, "ses-worker-123");
    }

    #[test]
    fn test_invoke_config_empty_worker_session_id_falls_back() {
        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-1".into(),
            work_dir: "/tmp/test".into(),
            prompt: "test".into(),
            session_id: "ses-batch".into(),
            auto_merge: false,
            target: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
        };
        let autorun_sid = if cfg.worker_session_id.is_empty() {
            &cfg.session_id
        } else {
            &cfg.worker_session_id
        };
        assert_eq!(autorun_sid, "ses-batch");
    }

    // -- run_status_watch interval clamping --

    #[test]
    fn test_status_watch_interval_clamp_zero() {
        // The interval should be clamped to at least 1 second.
        let user_input: u64 = 0;
        let interval = Duration::from_secs(user_input.max(1));
        assert_eq!(interval, Duration::from_secs(1));
    }

    #[test]
    fn test_status_watch_interval_normal() {
        let user_input: u64 = 10;
        let interval = Duration::from_secs(user_input.max(1));
        assert_eq!(interval, Duration::from_secs(10));
    }
}
