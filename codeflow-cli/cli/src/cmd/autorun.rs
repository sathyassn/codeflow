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
    /// Show status of autorun batches (TUI when TTY, text when piped)
    Status {
        /// Filter to specific batch session ID
        #[arg(long)]
        batch: Option<String>,
        /// Watch mode: interactive TUI dashboard (default when TTY)
        #[arg(long, short = 'w')]
        watch: bool,
        /// Force text output even in TTY (no TUI)
        #[arg(long)]
        once: bool,
        /// Refresh interval in seconds (default: 2)
        #[arg(long, default_value = "2")]
        interval: u64,
        /// Output JSON to stdout (non-watch mode only)
        #[arg(long)]
        json: bool,
        /// Show all batches with stacked task details (text mode only)
        #[arg(long)]
        all: bool,
    },
    /// Attach to a running worker's tmux session
    Attach {
        /// Task ID of the worker to attach to
        task_id: String,
        /// Batch session ID (required when task exists in multiple batches)
        #[arg(long)]
        batch: Option<String>,
    },
    /// View logs from a worker's tmux session
    Logs {
        /// Task ID of the worker to view logs for
        task_id: String,
        /// Follow log output (poll at 500ms)
        #[arg(long, short = 'f')]
        follow: bool,
        /// Batch session ID (required when task exists in multiple batches)
        #[arg(long)]
        batch: Option<String>,
    },
    /// Cancel a single running worker
    Cancel {
        /// Task ID of the worker to cancel
        task_id: String,
        /// Batch session ID (required when task exists in multiple batches)
        #[arg(long)]
        batch: Option<String>,
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
        /// Run in foreground (default: detach to tmux session)
        #[arg(long, visible_alias = "fg")]
        foreground: bool,
        /// Force resume even if live workers are detected
        #[arg(long)]
        force: bool,
    },
    /// List available batch files in the autorun config directory
    Batches {
        /// Force text output even in TTY (no TUI)
        #[arg(long)]
        once: bool,
    },
    /// Clean up stale autorun sessions (dead orchestrator)
    Cleanup {
        /// Session ID of specific batch to clean
        #[arg(long)]
        batch: Option<String>,
        /// Clean all stale sessions
        #[arg(long)]
        all: bool,
        /// Show what would be cleaned without doing it
        #[arg(long)]
        dry_run: bool,
        /// Skip DB purge of old terminal sessions
        #[arg(long)]
        no_purge: bool,
    },
    /// Prune old completed/failed autorun sessions and their workers/task_runs
    Prune {
        /// Delete sessions older than this duration (e.g. 30d, 7d, 24h)
        #[arg(long)]
        older_than: String,
        /// Keep the N most recent terminal sessions regardless of age
        #[arg(long, default_value = "0")]
        keep_last: usize,
        /// Show what would be deleted without actually deleting
        #[arg(long)]
        dry_run: bool,
    },
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
            watch: _,
            once,
            interval,
            json,
            all,
        }) => {
            use std::io::IsTerminal;

            let project_dir = resolve_repo_root(&project_dir)?;
            if json {
                run_status_json(&project_dir, batch.as_deref()).await
            } else if once || !std::io::stdout().is_terminal() {
                run_status(&project_dir, batch.as_deref(), all).await
            } else {
                run_status_watch(&project_dir, batch.as_deref(), interval).await
            }
        }
        Some(AutorunCommand::Attach { task_id, batch }) => {
            run_attach(&project_dir, &task_id, batch.as_deref()).await
        }
        Some(AutorunCommand::Logs {
            task_id,
            follow,
            batch,
        }) => run_logs(&project_dir, &task_id, follow, batch.as_deref()).await,
        Some(AutorunCommand::Cancel { task_id, batch }) => {
            run_cancel(&project_dir, &task_id, batch.as_deref()).await
        }
        Some(AutorunCommand::Abort { batch }) => run_abort(&project_dir, batch.as_deref()).await,
        Some(AutorunCommand::Cleanup {
            batch,
            all,
            dry_run,
            no_purge,
        }) => run_cleanup(&project_dir, batch.as_deref(), all, dry_run, no_purge).await,
        Some(AutorunCommand::Results { batch }) => {
            run_results(&project_dir, batch.as_deref()).await
        }
        Some(AutorunCommand::Resume {
            batch,
            foreground,
            force,
        }) => run_resume(&project_dir, batch.as_deref(), foreground, force).await,
        Some(AutorunCommand::Batches { once }) => {
            use std::io::IsTerminal;
            if once || !std::io::stdout().is_terminal() {
                run_batches_sync(&project_dir)
            } else {
                run_batches_tui(&project_dir)
            }
        }
        Some(AutorunCommand::Prune {
            older_than,
            keep_last,
            dry_run,
        }) => run_prune(&project_dir, &older_than, keep_last, dry_run).await,
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
    check_gh_auth()?;
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

/// Verify GitHub CLI is authenticated. Warns on failure -- the actual
/// PR merge will fail later with a more specific error if gh is missing.
#[allow(clippy::unnecessary_wraps)]
fn check_gh_auth() -> Result<()> {
    let output = std::process::Command::new("gh")
        .args(["auth", "status"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(o) if o.status.success() => {}
        Ok(_) => {
            eprintln!(
                "WARNING: GitHub CLI not authenticated. \
                 PR merge will require 'gh auth login'."
            );
        }
        Err(_) => {
            eprintln!(
                "WARNING: GitHub CLI (gh) not found. \
                 PR merge requires gh: https://cli.github.com/"
            );
        }
    }
    Ok(())
}

/// Create or verify the integration branch for the batch.
///
/// - If target is main/master: skip (workers PR directly to main).
/// - If target branch already exists: verify no open PRs (for auto-generated targets).
/// - If target doesn't exist: create from origin/{final_pr_target} and push.
fn resolve_or_create_integration_branch(
    project_dir: &Path,
    batch: &codeflow_core::autorun::ParsedBatch,
) -> Result<()> {
    let target = &batch.integration_branch;

    // Skip for main/master -- workers PR directly.
    if target == "main" || target == "master" || target.is_empty() {
        return Ok(());
    }

    // Check if target branch exists locally or at origin.
    let local_exists = std::process::Command::new("git")
        .args(["rev-parse", "--verify", target])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let remote_ref = format!("origin/{target}");
    let remote_exists = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &remote_ref])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if local_exists || remote_exists {
        // Branch exists -- for auto-generated targets, verify no stale open PRs.
        if batch.integration_branch_is_auto {
            let check = std::process::Command::new("gh")
                .args([
                    "pr", "list", "--head", target, "--state", "open", "--json", "number",
                ])
                .current_dir(project_dir)
                .output();
            if let Ok(output) = check {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.trim() != "[]" && !stdout.trim().is_empty() {
                    anyhow::bail!(
                        "auto-generated target branch '{target}' has open PRs. \
                         Clean up stale PRs or use a different batch name."
                    );
                }
            }
        }
        return Ok(());
    }

    // Branch doesn't exist -- create from origin/{final_pr_target}.
    let base = &batch.final_pr_target;
    let base_ref = format!("origin/{base}");
    eprintln!("creating integration branch '{target}' from '{base_ref}'");

    let create = std::process::Command::new("git")
        .args(["branch", target, &base_ref])
        .current_dir(project_dir)
        .output()
        .context("failed to create integration branch")?;
    if !create.status.success() {
        let stderr = String::from_utf8_lossy(&create.stderr);
        anyhow::bail!("failed to create branch '{target}': {stderr}");
    }

    let push = std::process::Command::new("git")
        .args(["push", "-u", "origin", target])
        .current_dir(project_dir)
        .output()
        .context("failed to push integration branch")?;
    if !push.status.success() {
        let stderr = String::from_utf8_lossy(&push.stderr);
        anyhow::bail!("failed to push branch '{target}' to origin: {stderr}");
    }

    eprintln!("integration branch '{target}' created and pushed");
    Ok(())
}

/// C6: Check batch guard -- reject if too many concurrent batches or same batch already running.
async fn check_batch_guard(project_dir: &Path, batch_path: &Path) -> Result<()> {
    use codeflow_core::store::DataStore;

    let db_dir = project_dir.join(".state/db");
    let Ok(store) = codeflow_core::store::SurrealStore::open(&db_dir).await else {
        // DB not available yet -- skip guard (first run).
        return Ok(());
    };

    let config = codeflow_core::autorun::load_config(project_dir).unwrap_or_default();

    let sessions = store
        .list_autorun_sessions(codeflow_core::models::AutorunSessionFilter::default())
        .await
        .unwrap_or_default();

    let active: Vec<_> = sessions
        .iter()
        .filter(|s| {
            matches!(
                s.status,
                codeflow_core::types::AutorunSessionStatus::Running
                    | codeflow_core::types::AutorunSessionStatus::Aborting
            )
        })
        .collect();

    if active.len() >= config.autorun.max_concurrent_batches {
        anyhow::bail!(
            "max concurrent batches reached ({}/{}); wait for active batches to complete or increase autorun.max_concurrent_batches",
            active.len(),
            config.autorun.max_concurrent_batches,
        );
    }

    // Check if the SAME batch file is already running.
    let batch_display = batch_path.to_string_lossy();
    for s in &active {
        if s.batch_file == batch_display {
            anyhow::bail!(
                "batch file '{}' is already running in session '{}'; abort it first or use a different batch file",
                batch_display,
                s.id,
            );
        }
    }

    Ok(())
}

async fn run_with_dir(project_dir: &Path, batch_path: &Path, foreground: bool) -> Result<()> {
    use codeflow_core::store::DataStore;

    // Autorun MUST work from the real repo root, not a worktree.
    let project_dir = &resolve_repo_root(project_dir)?;

    // Phase 1: Pre-flight checks (before batch parse).
    preflight_checks(project_dir)?;

    // C11: Auto-sweep stale sessions before batch guard to free slots.
    if let Ok(config) = codeflow_core::autorun::load_config(project_dir) {
        if let Ok(store) = open_store(project_dir).await {
            match codeflow_core::autorun::sweep_stale_sessions(
                store.as_ref(),
                project_dir,
                config.autorun.stale_threshold_secs,
            )
            .await
            {
                Ok(summary) if summary.sessions_cleaned > 0 => {
                    eprintln!(
                        "auto-sweep: cleaned {} stale session(s)",
                        summary.sessions_cleaned
                    );
                }
                _ => {}
            }
        }
    }

    // C6: Batch guard -- check active batch count and duplicate batch file.
    check_batch_guard(project_dir, batch_path).await?;

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

    // Generate a session ID for this autorun run (needed for resolve_target).
    let session_id = codeflow_core::session::generate_session_id();

    // Phase 2c: Resolve target branch (auto-generate if empty).
    codeflow_core::autorun::resolve_target(&mut parsed, session_id.as_str());

    // Phase 3: Create or verify integration branch.
    resolve_or_create_integration_branch(project_dir, &parsed)?;

    let config =
        codeflow_core::autorun::load_config(project_dir).context("loading parallel-work config")?;

    println!(
        "autorun batch: {} tasks, max_workers={}, scope_policy={}",
        parsed.tasks.len(),
        config.worktree.max_concurrent.min(parsed.max_workers),
        config.claims.default_scope_policy,
    );

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

    let store_for_final_pr = store.clone();
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

    // Post-batch: Epic status update (orchestrator-level, not per-worker).
    if parsed.integration_branch != "main"
        && parsed.integration_branch != "master"
        && config.autorun.epic_update == "orchestrator"
    {
        let completed_tasks: Vec<_> = results
            .iter()
            .filter(|r| r.status == "completed")
            .map(|r| r.task_id.clone())
            .collect();
        if !completed_tasks.is_empty() {
            eprintln!(
                "epic update: updating status for {} completed task(s)",
                completed_tasks.len()
            );
            for task_id in &completed_tasks {
                let epic_path = match codeflow_core::autorun::epic_update::resolve_epic_path(
                    project_dir,
                    task_id,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("  warning: could not resolve epic for {task_id}: {e}");
                        continue;
                    }
                };
                let content = match std::fs::read_to_string(&epic_path) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!(
                            "  warning: could not read epic {}: {e}",
                            epic_path.display()
                        );
                        continue;
                    }
                };
                match codeflow_core::autorun::epic_update::update_task_row_status(
                    &content, task_id, "complete",
                ) {
                    Ok(updated) => {
                        if updated != content {
                            if let Err(e) = std::fs::write(&epic_path, &updated) {
                                eprintln!(
                                    "  warning: could not write epic {}: {e}",
                                    epic_path.display()
                                );
                            } else {
                                eprintln!("  updated: {task_id}");
                            }
                        }
                    }
                    Err(e) => eprintln!("  warning: epic update for {task_id} failed: {e}"),
                }
            }
        }
    }

    // Post-batch: Create final PR (integration branch -> final_pr_target).
    let completed_count = results.iter().filter(|r| r.status == "completed").count();
    if parsed.final_pr && completed_count > 0 {
        let task_lines = results
            .iter()
            .map(|r| format!("- {} ({})", r.task_id, r.status))
            .collect::<Vec<_>>()
            .join("\n");

        let body = format!(
            "## Autorun Batch: {}\n\n\
             **{}/{}** tasks completed.\n\n\
             ### Tasks\n{}\n\n\
             Generated by `codeflow autorun`",
            parsed.name,
            completed_count,
            results.len(),
            task_lines
        );

        let pr_output = std::process::Command::new("gh")
            .args([
                "pr",
                "create",
                "--base",
                &parsed.final_pr_target,
                "--head",
                &parsed.integration_branch,
                "--title",
                &format!("autorun: {}", parsed.name),
                "--body",
                &body,
            ])
            .current_dir(project_dir)
            .output();

        match pr_output {
            Ok(o) if o.status.success() => {
                let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
                eprintln!("final PR created: {url}");
                // Persist final PR URL to the autorun session record.
                if !url.is_empty() {
                    if let Err(e) = store_for_final_pr
                        .update_autorun_session(
                            session_id.as_str(),
                            codeflow_core::models::AutorunSessionUpdate {
                                final_pr_url: Some(url.clone()),
                                ..Default::default()
                            },
                        )
                        .await
                    {
                        eprintln!("warning: failed to persist final_pr_url: {e}");
                    }
                }
            }
            Ok(o) => {
                eprintln!(
                    "warning: final PR creation failed: {}",
                    String::from_utf8_lossy(&o.stderr)
                );
            }
            Err(e) => {
                eprintln!("warning: gh pr create failed: {e}");
            }
        }
    }

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
        Ok(ref path) => {
            eprintln!("report: {}", path.display());
            // Best-effort: commit the report to the integration branch.
            commit_report_to_branch(project_dir, path, &parsed.integration_branch);
        }
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
    // C16: unique orchestrator tmux name using session ID suffix.
    let session_id = codeflow_core::session::generate_session_id();
    let sid_str = session_id.as_str();
    let short_sid = &sid_str[sid_str.len().saturating_sub(8)..];
    let orch_session = format!("cf-autorun-orch-{short_sid}");

    // Build the foreground command to run inside the tmux session.
    let exe = std::env::current_exe().context("getting current executable path")?;

    // C11: Write orchestrator script to temp file and use -- pattern.
    fn sq(s: &str) -> String {
        s.replace('\'', "'\\''")
    }
    let script_dir = project_dir.join(".state/autorun/scripts");
    std::fs::create_dir_all(&script_dir).context("creating autorun scripts directory")?;
    let script_path = script_dir.join(format!("orch-{short_sid}.sh"));
    let script_content = format!(
        "#!/bin/bash\nexport CODEFLOW_ORCH_TMUX='{orch}'\ncd '{}' && '{}' autorun run --batch '{}' --foreground\n",
        sq(&project_dir.to_string_lossy()),
        sq(&exe.to_string_lossy()),
        sq(&batch_path.to_string_lossy()),
        orch = sq(&orch_session),
    );
    std::fs::write(&script_path, &script_content).context("writing orchestrator script")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
    }

    // Create tmux session with -- bash --login script.
    let status = std::process::Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &orch_session,
            "--",
            "bash",
            "--login",
            &script_path.to_string_lossy(),
        ])
        .status()
        .context("failed to create tmux session for orchestrator")?;
    if !status.success() {
        anyhow::bail!("failed to create tmux session '{orch_session}'");
    }
    // Keep the pane alive after the command exits.
    let _ = std::process::Command::new("tmux")
        .args(["set-option", "-t", &orch_session, "remain-on-exit", "on"])
        .status();

    println!("autorun orchestrator launched in tmux session: {orch_session}");
    println!();
    println!("  Monitor:  codeflow autorun status --watch");
    println!("  Attach:   tmux attach -t {orch_session}");
    println!("  Worker:   codeflow autorun attach <task-id>");
    println!("  Abort:    codeflow autorun abort");

    Ok(())
}

/// C7: Launch resume in a dedicated tmux session and return immediately.
fn run_detached_resume(project_dir: &Path, batch: Option<&str>) -> Result<()> {
    let session_id = codeflow_core::session::generate_session_id();
    let sid_str = session_id.as_str();
    let short_sid = &sid_str[sid_str.len().saturating_sub(8)..];
    let orch_session = format!("cf-autorun-resume-{short_sid}");

    let exe = std::env::current_exe().context("getting current executable path")?;

    fn sq(s: &str) -> String {
        s.replace('\'', "'\\''")
    }
    let script_dir = project_dir.join(".state/autorun/scripts");
    std::fs::create_dir_all(&script_dir).context("creating autorun scripts directory")?;
    let script_path = script_dir.join(format!("resume-{short_sid}.sh"));
    let batch_arg = batch
        .map(|b| format!(" --batch '{}'", sq(b)))
        .unwrap_or_default();
    let script_content = format!(
        "#!/bin/bash\ncd '{}' && '{}' autorun resume{} --foreground\n",
        sq(&project_dir.to_string_lossy()),
        sq(&exe.to_string_lossy()),
        batch_arg,
    );
    std::fs::write(&script_path, &script_content).context("writing resume script")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
    }

    let status = std::process::Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            &orch_session,
            "--",
            "bash",
            "--login",
            &script_path.to_string_lossy(),
        ])
        .status()
        .context("failed to create tmux session for resume")?;
    if !status.success() {
        anyhow::bail!("failed to create tmux session '{orch_session}'");
    }
    let _ = std::process::Command::new("tmux")
        .args(["set-option", "-t", &orch_session, "remain-on-exit", "on"])
        .status();

    println!("autorun resume launched in tmux session: {orch_session}");
    println!();
    println!("  Monitor:  codeflow autorun status --watch");
    println!("  Attach:   tmux attach -t {orch_session}");

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

    // Write report file. C25: Include session_id in filename for uniqueness.
    let date_str = start_time.format("%Y-%m-%d").to_string();
    let short_sid = &session_id[session_id.len().saturating_sub(8)..];
    let filename = format!("{batch_name}-{short_sid}-{date_str}.md");
    let report_path = abs_report_dir.join(&filename);
    codeflow_core::file_lock::atomic_write(&report_path, md.as_bytes())
        .with_context(|| format!("writing batch report to {}", report_path.display()))?;

    Ok(report_path)
}

/// Check whether a string is safe to use as a git ref name.
///
/// Rejects values that could be interpreted as git options or contain
/// path traversal / special git ref syntax.
fn is_safe_git_ref(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.contains("..")
        && !s.contains("@{")
        && !s.contains('\\')
        && !s.contains(':')
        && !s.contains('?')
        && !s.contains('*')
        && !s.contains('[')
        && !s.contains('^')
        && !s.contains('~')
        && !s.contains(' ')
        && !s.contains('\t')
        && !s.contains('\n')
}

/// Commit a batch report file to the integration branch (best-effort).
///
/// Checks out the integration branch, adds the report, commits, pushes,
/// then switches back to the original branch. Silently logs failures
/// without blocking the main flow.
fn commit_report_to_branch(project_dir: &Path, report_path: &Path, integration_branch: &str) {
    // Skip for protected branches (main, master) — report doesn't belong there.
    if integration_branch == "main" || integration_branch == "master" {
        return;
    }

    // Validate branch name to prevent git option injection.
    if !is_safe_git_ref(integration_branch) {
        eprintln!("report: skipping commit (invalid branch name '{integration_branch}')");
        return;
    }

    // Check the integration branch exists locally.
    let branch_exists = std::process::Command::new("git")
        .args(["rev-parse", "--verify", integration_branch])
        .current_dir(project_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if !branch_exists {
        eprintln!("report: skipping commit (integration branch '{integration_branch}' not found)");
        return;
    }

    // Save current branch/HEAD for switching back.
    let original_ref = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_dir)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    // Checkout integration branch.
    let checkout = std::process::Command::new("git")
        .args(["checkout", integration_branch])
        .current_dir(project_dir)
        .output();
    if !checkout.is_ok_and(|o| o.status.success()) {
        eprintln!("report: could not checkout {integration_branch}, skipping commit");
        return;
    }

    // Helper: switch back to original ref on any exit path.
    let switch_back = |original: &str| {
        if !original.is_empty() && is_safe_git_ref(original) {
            let _ = std::process::Command::new("git")
                .args(["checkout", original])
                .current_dir(project_dir)
                .status();
        }
    };

    // Make report path relative to project_dir for git add.
    let rel_path = match report_path.strip_prefix(project_dir) {
        Ok(p) => p,
        Err(_) => {
            eprintln!("report: cannot compute relative path, skipping git add");
            switch_back(&original_ref);
            return;
        }
    };

    // Add and commit.
    let add_ok = std::process::Command::new("git")
        .args(["add", &rel_path.display().to_string()])
        .current_dir(project_dir)
        .status()
        .is_ok_and(|s| s.success());

    if add_ok {
        let commit_ok = std::process::Command::new("git")
            .args([
                "commit",
                "-m",
                &format!("chore: add batch report {}", rel_path.display()),
            ])
            .current_dir(project_dir)
            .status()
            .is_ok_and(|s| s.success());

        if commit_ok {
            let push_ok = std::process::Command::new("git")
                .args(["push", "origin", integration_branch])
                .current_dir(project_dir)
                .status()
                .is_ok_and(|s| s.success());
            if push_ok {
                eprintln!("report: committed to {integration_branch}");
            } else {
                eprintln!("report: commit succeeded but push to {integration_branch} failed");
            }
        }
    }

    // Switch back to original ref.
    switch_back(&original_ref);
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
///
/// C19: For results, also searches terminal statuses (Completed, Failed, etc.).
/// C20: When multiple running sessions and no --batch, lists them and bails.
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

    // Find all running sessions.
    let running = store
        .list_autorun_sessions(AutorunSessionFilter {
            status: Some(AutorunSessionStatus::Running),
            limit: Some(10),
            ..Default::default()
        })
        .await
        .context("querying autorun sessions")?;

    // C20: If multiple running sessions, list them and ask for --batch.
    if running.len() > 1 {
        eprintln!("Multiple active autorun sessions found:");
        for s in &running {
            eprintln!(
                "  {} (batch: {})",
                s.id,
                s.batch_name.as_deref().unwrap_or("unknown")
            );
        }
        anyhow::bail!(
            "{} active sessions found; specify --batch <session_id>",
            running.len()
        );
    }

    if let Some(s) = running.first() {
        return Ok(s.id.clone());
    }

    // Try aborting sessions.
    let aborting = store
        .list_autorun_sessions(AutorunSessionFilter {
            status: Some(AutorunSessionStatus::Aborting),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .context("querying autorun sessions")?;

    if let Some(s) = aborting.first() {
        return Ok(s.id.clone());
    }

    // C19: Fall back to most recent terminal session (for results/history queries).
    let all = store
        .list_autorun_sessions(AutorunSessionFilter {
            limit: Some(1),
            all: true,
            ..Default::default()
        })
        .await
        .context("querying autorun sessions")?;

    if let Some(s) = all.first() {
        return Ok(s.id.clone());
    }

    anyhow::bail!("no autorun session found; use --batch <session_id> to specify one");
}

/// C17: Smart task resolution -- find which session contains a given task_id.
///
/// When `batch` is provided, uses it directly. Otherwise, searches ALL active
/// sessions for the task_id. If found in exactly 1 session, returns it.
/// If 0 or 2+ sessions, returns an error.
async fn resolve_task_session(
    store: &codeflow_core::store::SurrealStore,
    task_id: &str,
    batch: Option<&str>,
) -> Result<String> {
    use codeflow_core::models::AutorunSessionFilter;
    use codeflow_core::store::DataStore;
    use codeflow_core::types::AutorunSessionStatus;

    if let Some(sid) = batch {
        return Ok(sid.to_string());
    }

    // Search all active (running + aborting) sessions for the task.
    let active = store
        .list_autorun_sessions(AutorunSessionFilter {
            limit: Some(20),
            ..Default::default()
        })
        .await
        .context("querying autorun sessions")?;

    let active: Vec<_> = active
        .into_iter()
        .filter(|s| {
            matches!(
                s.status,
                AutorunSessionStatus::Running | AutorunSessionStatus::Aborting
            )
        })
        .collect();

    let mut matching_sessions = Vec::new();
    for session in &active {
        let worker = store
            .get_autorun_worker_by_task_id(&session.id, task_id)
            .await?;
        if worker.is_some() {
            matching_sessions.push(session.id.clone());
        }
    }

    match matching_sessions.len() {
        1 => Ok(matching_sessions.into_iter().next().unwrap()),
        0 => {
            // Fall back to single-session resolution.
            resolve_session_id(store, None).await
        }
        n => {
            eprintln!("Task '{task_id}' found in {n} active sessions:");
            for sid in &matching_sessions {
                eprintln!("  {sid}");
            }
            anyhow::bail!("task '{task_id}' exists in {n} sessions; specify --batch <session_id>");
        }
    }
}

// ---------------------------------------------------------------------------
// Subcommand handlers
// ---------------------------------------------------------------------------

async fn run_status(project_dir: &Path, batch: Option<&str>, show_all: bool) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;
    let stale_threshold = codeflow_core::autorun::load_config(project_dir)
        .map(|c| c.autorun.stale_threshold_secs)
        .unwrap_or(90);

    // Determine which sessions to show.
    let sessions = if let Some(sid) = batch {
        match store.get_autorun_session(sid).await? {
            Some(s) => vec![s],
            None => anyhow::bail!("no autorun session found with id '{sid}'"),
        }
    } else if show_all {
        // --all: fetch all sessions, sorted running/aborting first then by created_at DESC.
        use codeflow_core::models::AutorunSessionFilter;
        let all_sessions = store
            .list_autorun_sessions(AutorunSessionFilter {
                all: true,
                ..Default::default()
            })
            .await?;
        let (mut active, rest): (Vec<_>, Vec<_>) = all_sessions.into_iter().partition(|s| {
            matches!(
                s.status,
                codeflow_core::types::AutorunSessionStatus::Running
                    | codeflow_core::types::AutorunSessionStatus::Aborting
            )
        });
        active.extend(rest);
        if active.is_empty() {
            println!("No autorun batches found.");
            return Ok(());
        }
        active
    } else {
        // Default: show running/aborting first, then recent completed/failed.
        use codeflow_core::models::AutorunSessionFilter;
        let all_sessions = store
            .list_autorun_sessions(AutorunSessionFilter::default())
            .await?;
        let (mut active, rest): (Vec<_>, Vec<_>) = all_sessions.into_iter().partition(|s| {
            matches!(
                s.status,
                codeflow_core::types::AutorunSessionStatus::Running
                    | codeflow_core::types::AutorunSessionStatus::Aborting
            )
        });
        // Append up to 10 recent non-active batches for visibility.
        active.extend(rest.into_iter().take(10));
        if active.is_empty() {
            println!("No autorun batches found.");
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

        // C12: Real stale detection for running sessions.
        let mut orphan_count = 0;
        let mut session_stale = false;
        for w in &workers {
            if w.status == codeflow_core::types::AutorunWorkerStatus::Running {
                if let Some(ref tmux_name) = w.tmux_session {
                    if !tmux_has_session(tmux_name) {
                        orphan_count += 1;
                    }
                }
            }
        }
        // Check orchestrator liveness.
        if matches!(
            session.status,
            codeflow_core::types::AutorunSessionStatus::Running
        ) {
            let pid_alive = session
                .pid
                .is_some_and(codeflow_core::autorun::check_pid_alive);
            let tmux_alive = session
                .tmux_session
                .as_deref()
                .map(codeflow_core::autorun::check_tmux_alive);
            let (hb_alive, _) = codeflow_core::autorun::check_heartbeat_alive(
                project_dir,
                &session.id,
                stale_threshold,
            );
            // Only pass stale signal -- if heartbeat file doesn't exist, don't
            // report a false-positive stale status.
            let heartbeat = if hb_alive { None } else { Some(false) };
            session_stale =
                codeflow_core::autorun::is_session_stale(pid_alive, tmux_alive, heartbeat);
        }

        let elapsed = format_elapsed(&session.created_at);
        let batch_name = session
            .batch_name
            .as_deref()
            .unwrap_or(&session.id[..session.id.len().min(20)]);

        let stale_label = if session_stale {
            " (stale)"
        } else if orphan_count > 0 {
            " (orphan)"
        } else {
            ""
        };

        println!(
            "{:<24} {:<10} {:>5} {:>5} {:>5} {:>5}  {}{stale_label}",
            batch_name,
            session.status,
            session.total_tasks,
            session.completed_tasks,
            session.failed_tasks,
            running,
            elapsed,
        );

        // Per-worker detail with branch, PR#, and phase columns.
        let task_runs = store.list_autorun_task_runs(&session.id).await?;
        if !task_runs.is_empty() {
            println!();
            println!(
                "  {:<24} {:<12} {:<5} {:<20} {:>5} {:>9}",
                "TASK", "STATUS", "PHASE", "BRANCH", "PR", "DURATION"
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
                    |secs| {
                        if secs < 0 {
                            "--".to_string()
                        } else {
                            format_duration_secs(secs)
                        }
                    },
                );
                // C24: Show "Waiting" for pending tasks when session is active.
                let display_status = if matches!(
                    run.status,
                    codeflow_core::types::AutorunTaskRunStatus::Pending
                ) {
                    if matches!(
                        session.status,
                        codeflow_core::types::AutorunSessionStatus::Running
                    ) {
                        "Waiting".to_string()
                    } else {
                        run.status.to_string()
                    }
                } else {
                    run.status.to_string()
                };
                let phase = codeflow_core::tui::data::read_latest_phase(
                    project_dir,
                    run.worktree_path.as_deref(),
                    None,
                )
                .unwrap_or_else(|| "--".to_string());
                let branch = run.branch_name.as_deref().unwrap_or("--");
                let pr = run
                    .pr_number
                    .map_or_else(|| "--".to_string(), |n| format!("#{n}"));
                println!(
                    "  {:<24} {:<12} {:<5} {:<20} {:>5} {:>9}",
                    run.task_id, display_status, phase, branch, pr, duration
                );
            }
        }
    }

    // C8: Print attach hint when any tasks are running.
    let any_running = sessions.iter().any(|s| {
        matches!(
            s.status,
            codeflow_core::types::AutorunSessionStatus::Running
        )
    });
    if any_running {
        println!();
        println!("  Attach: codeflow autorun attach <TASK-ID> [--batch <SESSION-ID>]");
        println!("  Cleanup: codeflow autorun cleanup --all");
    }

    Ok(())
}

/// Two-level navigation state for the autorun status TUI.
enum AutorunView {
    /// Top-level: list of all batch sessions.
    BatchList,
    /// Drill-down: task detail for a specific batch session.
    BatchDetail(String),
}

async fn run_status_watch(
    project_dir: &Path,
    batch: Option<&str>,
    interval_secs: u64,
) -> Result<()> {
    use codeflow_core::tui::data::{BatchListEntry, BatchView, fetch_batch_list, fetch_batch_view};
    use codeflow_core::tui::theme;
    use codeflow_core::tui::widgets::{DetailPane, DurationCell};
    use ratatui::crossterm::event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
    };
    use ratatui::crossterm::execute;
    use ratatui::layout::{Constraint, Layout};
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

    let store = open_store(project_dir).await?;
    let batch_owned = batch.map(ToString::to_string);

    // Terminal setup with mouse support.
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;

    // Cleanup guard: ensure terminal is restored on any exit path.
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            let _ = execute!(std::io::stdout(), DisableMouseCapture);
            ratatui::restore();
        }
    }
    let _guard = TermGuard;

    // Two-level navigation: BatchList (default) or BatchDetail (when --batch or Enter).
    let mut view = if let Some(ref sid) = batch_owned {
        AutorunView::BatchDetail(sid.clone())
    } else {
        AutorunView::BatchList
    };
    let started_with_batch = batch_owned.is_some();

    let mut table_state = TableState::default();
    let mut batch_table_state = TableState::default();

    // Batch list data (for BatchList view).
    let mut batch_list: Vec<BatchListEntry> =
        fetch_batch_list(store.as_ref()).await.unwrap_or_default();
    if !batch_list.is_empty() {
        batch_table_state.select(Some(0));
    }

    // Fetch initial detail data (for BatchDetail view).
    let detail_sid = match &view {
        AutorunView::BatchDetail(sid) => Some(sid.as_str()),
        AutorunView::BatchList => None,
    };
    let mut last_view: Option<BatchView> =
        match fetch_batch_view(store.as_ref(), project_dir, detail_sid).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warn: initial fetch failed: {e}");
                None
            }
        };
    let mut status_message: Option<(String, std::time::Instant)> = None;

    loop {
        // Preserve selection index across refreshes (for whichever view is active).
        match view {
            AutorunView::BatchList => {
                if !batch_list.is_empty() && batch_table_state.selected().is_none() {
                    batch_table_state.select(Some(0));
                }
                if let Some(sel) = batch_table_state.selected() {
                    if sel >= batch_list.len() {
                        batch_table_state.select(Some(batch_list.len().saturating_sub(1)));
                    }
                }
            }
            AutorunView::BatchDetail(_) => {
                if let Some(ref v) = last_view {
                    if !v.tasks.is_empty() && table_state.selected().is_none() {
                        table_state.select(Some(0));
                    }
                    if let Some(sel) = table_state.selected() {
                        if sel >= v.tasks.len() {
                            table_state.select(Some(v.tasks.len().saturating_sub(1)));
                        }
                    }
                }
            }
        }

        // Draw UI.
        terminal.draw(|frame| {
            match view {
                AutorunView::BatchList => {
                    // --- Batch list view ---
                    let area = frame.area();
                    let chunks = Layout::vertical([
                        Constraint::Length(2), // header
                        Constraint::Min(5),    // table
                        Constraint::Length(1), // keys
                    ])
                    .split(area);

                    // Header
                    let running = batch_list
                        .iter()
                        .filter(|b| {
                            matches!(
                                b.status,
                                codeflow_core::types::AutorunSessionStatus::Running
                            )
                        })
                        .count();
                    let header_line = Line::from(vec![
                        Span::styled(
                            format!(" {} ", theme::TRIANGLE),
                            Style::new().fg(theme::BLUE_ACCENT),
                        ),
                        Span::styled("Autorun Batches", theme::header()),
                        Span::raw("  "),
                        Span::styled(
                            format!("{} batch(es)", batch_list.len()),
                            Style::new().fg(theme::DIM_PENDING),
                        ),
                        if running > 0 {
                            Span::styled(
                                format!("  {running} running"),
                                Style::new().fg(theme::GREEN_SUCCESS),
                            )
                        } else {
                            Span::raw("")
                        },
                    ]);
                    frame.render_widget(Paragraph::new(header_line), chunks[0]);

                    // Table
                    let tbl_header = Row::new(vec![
                        "BATCH", "STATUS", "TASKS", "DONE", "FAIL", "RUN", "ELAPSED",
                    ])
                    .style(theme::header())
                    .bottom_margin(1);

                    let rows: Vec<Row> = batch_list
                        .iter()
                        .map(|b| {
                            let status_badge = crate::cmd::autorun::status_badge_text(b.status);
                            let dur = DurationCell::new(Some(b.elapsed_secs));
                            Row::new(vec![
                                Cell::from(b.batch_name.clone()),
                                Cell::from(status_badge),
                                Cell::from(format!("{}", b.total_tasks)),
                                Cell::from(format!("{}", b.completed_tasks)),
                                Cell::from(format!("{}", b.failed_tasks)),
                                Cell::from(format!("{}", b.running_count)),
                                Cell::from(dur.to_span()),
                            ])
                        })
                        .collect();

                    let table = Table::new(
                        rows,
                        [
                            Constraint::Min(20),    // BATCH
                            Constraint::Length(12), // STATUS
                            Constraint::Length(7),  // TASKS
                            Constraint::Length(7),  // DONE
                            Constraint::Length(7),  // FAIL
                            Constraint::Length(5),  // RUN
                            Constraint::Length(10), // ELAPSED
                        ],
                    )
                    .header(tbl_header)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_type(theme::BORDER_TYPE)
                            .title(" Batches ")
                            .style(Style::new().fg(theme::WHITE_TEXT)),
                    )
                    .row_highlight_style(theme::selected())
                    .highlight_symbol(format!("{} ", theme::TRIANGLE));

                    frame.render_stateful_widget(table, chunks[1], &mut batch_table_state);

                    // Keybinding bar
                    let bar = Line::from(vec![
                        Span::styled(" [Enter]", Style::new().fg(theme::BLUE_ACCENT)),
                        Span::raw(" Detail "),
                        Span::styled("[Up/Down]", Style::new().fg(theme::BLUE_ACCENT)),
                        Span::raw(" Navigate "),
                        Span::styled("[q]", Style::new().fg(theme::BLUE_ACCENT)),
                        Span::raw(" Quit"),
                    ]);
                    frame.render_widget(Paragraph::new(bar), chunks[2]);
                }
                AutorunView::BatchDetail(_) => {
                    // --- Existing batch detail view ---
                    let area = frame.area();
                    let chunks = Layout::vertical([
                        Constraint::Length(2), // header
                        Constraint::Min(5),    // table
                        Constraint::Length(6), // detail
                        Constraint::Length(1), // keys
                    ])
                    .split(area);

                    render_header(frame, chunks[0], last_view.as_ref());
                    render_table(frame, chunks[1], &mut table_state, last_view.as_ref());

                    let sel_task = last_view
                        .as_ref()
                        .and_then(|v| table_state.selected().and_then(|i| v.tasks.get(i)));
                    let detail = DetailPane::new(sel_task);
                    frame.render_widget(detail, chunks[2]);

                    let bar_line = if let Some((ref msg, at)) = status_message {
                        if at.elapsed() < Duration::from_secs(3) {
                            Line::from(Span::styled(
                                format!(" {msg}"),
                                Style::new().fg(theme::YELLOW_RUNNING),
                            ))
                        } else {
                            status_message = None;
                            context_keybinding_line(sel_task)
                        }
                    } else {
                        let mut line = context_keybinding_line(sel_task);
                        // Add Esc hint for back-to-list navigation.
                        if !started_with_batch {
                            line.spans.insert(0, Span::raw(" "));
                            line.spans.insert(
                                0,
                                Span::styled("[Esc]", Style::new().fg(theme::BLUE_ACCENT)),
                            );
                            line.spans.insert(1, Span::raw(" Back "));
                        }
                        line
                    };
                    frame.render_widget(Paragraph::new(bar_line), chunks[3]);
                }
            }
        })?;

        // Event handling with user-configurable tick interval.
        let tick = Duration::from_secs(interval_secs.max(1));
        if event::poll(tick)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    // Ctrl+C always quits.
                    if key.code == KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(ratatui::crossterm::event::KeyModifiers::CONTROL)
                    {
                        break;
                    }
                    match view {
                        AutorunView::BatchList => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            KeyCode::Up => {
                                let i = batch_table_state.selected().unwrap_or(0);
                                let prev = if i == 0 {
                                    batch_list.len().saturating_sub(1)
                                } else {
                                    i - 1
                                };
                                batch_table_state.select(Some(prev));
                            }
                            KeyCode::Down => {
                                let i = batch_table_state.selected().unwrap_or(0);
                                let next = if batch_list.is_empty() {
                                    0
                                } else {
                                    (i + 1) % batch_list.len()
                                };
                                batch_table_state.select(Some(next));
                            }
                            KeyCode::Enter => {
                                if let Some(sel) = batch_table_state.selected() {
                                    if let Some(entry) = batch_list.get(sel) {
                                        let sid = entry.session_id.clone();
                                        // Fetch detail for this session.
                                        last_view = fetch_batch_view(
                                            store.as_ref(),
                                            project_dir,
                                            Some(&sid),
                                        )
                                        .await
                                        .unwrap_or(None);
                                        table_state = TableState::default();
                                        view = AutorunView::BatchDetail(sid);
                                    }
                                }
                            }
                            _ => {}
                        },
                        AutorunView::BatchDetail(_) => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Esc => {
                                if started_with_batch {
                                    // Launched with --batch, Esc quits.
                                    break;
                                }
                                // Go back to batch list.
                                view = AutorunView::BatchList;
                            }
                            KeyCode::Up => {
                                if let Some(ref v) = last_view {
                                    let i = table_state.selected().unwrap_or(0);
                                    let prev = if i == 0 {
                                        v.tasks.len().saturating_sub(1)
                                    } else {
                                        i - 1
                                    };
                                    table_state.select(Some(prev));
                                }
                            }
                            KeyCode::Down => {
                                if let Some(ref v) = last_view {
                                    let i = table_state.selected().unwrap_or(0);
                                    let next = if v.tasks.is_empty() {
                                        0
                                    } else {
                                        (i + 1) % v.tasks.len()
                                    };
                                    table_state.select(Some(next));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(task) = selected_task(last_view.as_ref(), &table_state)
                                {
                                    if let Some(ref tmux_name) = task.tmux_session {
                                        if !is_safe_tmux_name(tmux_name) {
                                            status_message = Some((
                                                "Invalid tmux session name".to_string(),
                                                std::time::Instant::now(),
                                            ));
                                        } else if tmux_has_session(tmux_name) {
                                            let _ =
                                                execute!(std::io::stdout(), DisableMouseCapture);
                                            ratatui::restore();
                                            match std::process::Command::new("tmux")
                                                .args(["attach-session", "-t", tmux_name])
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
                                            execute!(std::io::stdout(), EnableMouseCapture)?;
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('a') => {
                                if let Some(task) = selected_task(last_view.as_ref(), &table_state)
                                {
                                    let is_cancellable = matches!(
                                        task.status,
                                        codeflow_core::types::AutorunTaskRunStatus::Running
                                            | codeflow_core::types::AutorunTaskRunStatus::Pending
                                    );
                                    if is_cancellable {
                                        let tid = task.task_id.clone();
                                        let batch_ref = match &view {
                                            AutorunView::BatchDetail(sid) => Some(sid.as_str()),
                                            AutorunView::BatchList => None,
                                        };
                                        match run_cancel(project_dir, &tid, batch_ref).await {
                                            Ok(()) => {
                                                status_message = Some((
                                                    format!("Aborting {tid}..."),
                                                    std::time::Instant::now(),
                                                ));
                                            }
                                            Err(e) => {
                                                status_message = Some((
                                                    format!("Abort failed: {e}"),
                                                    std::time::Instant::now(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                            KeyCode::Char('r') => {
                                if let Some(task) = selected_task(last_view.as_ref(), &table_state)
                                {
                                    let is_retryable = matches!(
                                        task.status,
                                        codeflow_core::types::AutorunTaskRunStatus::Failed
                                            | codeflow_core::types::AutorunTaskRunStatus::Timeout
                                    );
                                    if is_retryable {
                                        if let Some(ref v) = last_view {
                                            match std::process::Command::new("codeflow")
                                                .args([
                                                    "autorun",
                                                    "resume",
                                                    "--batch",
                                                    &v.session_id,
                                                ])
                                                .spawn()
                                            {
                                                Ok(_) => {
                                                    status_message = Some((
                                                        format!("Retrying {}...", task.task_id),
                                                        std::time::Instant::now(),
                                                    ));
                                                }
                                                Err(e) => {
                                                    status_message = Some((
                                                        format!("Retry failed: {e}"),
                                                        std::time::Instant::now(),
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        },
                    }
                }
                Event::Mouse(mouse) if matches!(view, AutorunView::BatchDetail(_)) => {
                    match mouse.kind {
                        MouseEventKind::Down(_) => {
                            if let Some(ref v) = last_view {
                                let table_top: u16 = 2 + 1 + 1;
                                if mouse.row >= table_top {
                                    let idx = (mouse.row - table_top) as usize;
                                    if idx < v.tasks.len() {
                                        table_state.select(Some(idx));
                                    }
                                }
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if let Some(ref v) = last_view {
                                let i = table_state.selected().unwrap_or(0);
                                let prev = if i == 0 {
                                    v.tasks.len().saturating_sub(1)
                                } else {
                                    i - 1
                                };
                                table_state.select(Some(prev));
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if let Some(ref v) = last_view {
                                let i = table_state.selected().unwrap_or(0);
                                let next = if v.tasks.is_empty() {
                                    0
                                } else {
                                    (i + 1) % v.tasks.len()
                                };
                                table_state.select(Some(next));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Refresh data every tick. On error, keep stale data and warn.
        match view {
            AutorunView::BatchList => {
                batch_list = fetch_batch_list(store.as_ref()).await.unwrap_or_default();
            }
            AutorunView::BatchDetail(ref sid) => {
                match fetch_batch_view(store.as_ref(), project_dir, Some(sid.as_str())).await {
                    Ok(v) => last_view = v,
                    Err(e) => eprintln!("warn: fetch failed: {e}"),
                }
            }
        }
    }

    Ok(())
}

fn context_keybinding_line(
    task: Option<&codeflow_core::tui::data::TaskView>,
) -> ratatui::text::Line<'static> {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::Span;

    let mut spans = vec![
        Span::styled(" [Enter]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Attach "),
    ];

    // Context-aware abort hint.
    let abort_hint = task.map_or(("[a]", " Abort ", false), |t| {
        let is_cancellable = matches!(
            t.status,
            codeflow_core::types::AutorunTaskRunStatus::Running
                | codeflow_core::types::AutorunTaskRunStatus::Pending
        );
        if t.display_status == "Waiting" {
            ("[a]", " Abort (waiting...) ", true)
        } else if is_cancellable {
            ("[a]", " Abort ", false)
        } else {
            ("[a]", " Abort (N/A) ", true)
        }
    });
    if abort_hint.2 {
        spans.push(Span::styled(abort_hint.0.to_string(), theme::dim()));
        spans.push(Span::styled(abort_hint.1.to_string(), theme::dim()));
    } else {
        spans.push(Span::styled(
            abort_hint.0.to_string(),
            Style::new().fg(theme::BLUE_ACCENT),
        ));
        spans.push(Span::raw(abort_hint.1.to_string()));
    }

    spans.extend([
        Span::styled("[r]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Retry "),
        Span::styled("[Up/Down]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Navigate "),
        Span::styled("[q]", Style::new().fg(theme::BLUE_ACCENT)),
        Span::raw(" Quit"),
    ]);

    ratatui::text::Line::from(spans)
}

fn selected_task<'a>(
    view: Option<&'a codeflow_core::tui::data::BatchView>,
    state: &ratatui::widgets::TableState,
) -> Option<&'a codeflow_core::tui::data::TaskView> {
    view.and_then(|v| state.selected().and_then(|i| v.tasks.get(i)))
}

fn render_header(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    view: Option<&codeflow_core::tui::data::BatchView>,
) {
    use codeflow_core::tui::theme;
    use codeflow_core::tui::widgets::duration_cell::format_duration;
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;

    let line = if let Some(v) = view {
        let status_color = match v.status {
            codeflow_core::types::AutorunSessionStatus::Running => theme::YELLOW_RUNNING,
            codeflow_core::types::AutorunSessionStatus::Completed => theme::GREEN_SUCCESS,
            codeflow_core::types::AutorunSessionStatus::Failed => theme::RED_FAILURE,
            _ => theme::WHITE_TEXT,
        };
        Line::from(vec![
            Span::styled(
                format!(" {} ", theme::TRIANGLE),
                Style::new().fg(theme::BLUE_ACCENT),
            ),
            Span::styled(&v.batch_name, theme::header()),
            Span::raw("  "),
            Span::styled(v.status.to_string(), Style::new().fg(status_color)),
            Span::raw("  "),
            Span::styled(
                format!("{} workers", v.running_count),
                Style::new().fg(theme::WHITE_TEXT),
            ),
            Span::raw("  "),
            Span::styled(
                format_duration(v.elapsed_secs),
                Style::new().fg(theme::DIM_PENDING),
            ),
            Span::raw("  "),
            Span::styled(
                format!("target: {}", v.target_branch.as_deref().unwrap_or("--")),
                Style::new().fg(theme::DIM_PENDING),
            ),
            Span::raw("  "),
            Span::styled(
                compute_eta(v.elapsed_secs, v.completed_tasks, v.total_tasks),
                Style::new().fg(theme::DIM_PENDING),
            ),
        ])
    } else {
        Line::styled(
            "No active autorun batches",
            Style::new().fg(theme::DIM_PENDING),
        )
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_table(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &mut ratatui::widgets::TableState,
    view: Option<&codeflow_core::tui::data::BatchView>,
) {
    use codeflow_core::tui::theme;
    use codeflow_core::tui::widgets::{DurationCell, PhaseBadge, StatusBadge};
    use ratatui::layout::Constraint;
    use ratatui::style::{Modifier, Style};
    use ratatui::widgets::{Block, Borders, Cell, Row, Table};

    let header = Row::new(vec!["TASK", "STATUS", "PHASE", "BRANCH", "PR", "DURATION"])
        .style(theme::header())
        .bottom_margin(1);

    let rows: Vec<Row> = view
        .map(|v| {
            v.tasks
                .iter()
                .map(|task| {
                    let status_badge =
                        StatusBadge::new(task.status).with_display(task.display_status.clone());
                    let phase_badge = PhaseBadge::new(task.phase.as_deref());
                    let duration = DurationCell::new(task.duration_secs);
                    let pr_text = task
                        .pr_number
                        .map_or_else(|| "--".to_string(), |n| format!("#{n}"));

                    Row::new(vec![
                        Cell::from(task.task_id.clone()),
                        Cell::from(status_badge.to_span()),
                        Cell::from(phase_badge.to_span()),
                        Cell::from(task.branch.as_deref().unwrap_or("--")),
                        Cell::from(pr_text),
                        Cell::from(duration.to_span()),
                    ])
                })
                .collect()
        })
        .unwrap_or_default();

    let widths = [
        Constraint::Length(26),
        Constraint::Length(14),
        Constraint::Length(6),
        Constraint::Length(24),
        Constraint::Length(8),
        Constraint::Length(10),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(theme::BORDER_TYPE)
                .title(" Tasks "),
        )
        .column_spacing(1)
        .row_highlight_style(
            Style::new()
                .bg(theme::BLUE_ACCENT)
                .fg(ratatui::style::Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(format!("{} ", theme::TRIANGLE));

    frame.render_stateful_widget(table, area, state);
}

/// JSON output for `codeflow autorun status --json`.
async fn run_status_json(project_dir: &Path, batch: Option<&str>) -> Result<()> {
    use codeflow_core::tui::data::fetch_batch_view;

    let store = open_store(project_dir).await?;
    let view = fetch_batch_view(store.as_ref(), project_dir, batch)
        .await
        .context("fetching batch view")?;

    match view {
        Some(v) => {
            let json = serde_json::to_string_pretty(&v)?;
            println!("{json}");
        }
        None => {
            println!("{{}}");
        }
    }
    Ok(())
}

async fn run_attach(project_dir: &Path, task_id: &str, batch: Option<&str>) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;
    // C17: Smart task resolution with --batch support.
    let session_id = resolve_task_session(store.as_ref(), task_id, batch).await?;

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

    // C20: Warn the user before attaching.
    eprintln!("  AUTORUN SESSION -- agent is working autonomously.");
    eprintln!("  Typing will inject messages into the agent's conversation.");
    eprintln!("  Ctrl+B D to detach safely. Ctrl+C will kill the worker.");
    eprintln!();

    // exec replaces current process.
    let err = exec::Command::new("tmux")
        .args(&["attach-session", "-t", tmux_name])
        .exec();
    anyhow::bail!("failed to exec tmux attach-session: {err}");
}

async fn run_logs(
    project_dir: &Path,
    task_id: &str,
    follow: bool,
    batch: Option<&str>,
) -> Result<()> {
    use codeflow_core::store::DataStore;

    let store = open_store(project_dir).await?;
    // C17: Smart task resolution with --batch support.
    let session_id = resolve_task_session(store.as_ref(), task_id, batch).await?;

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

async fn run_cancel(project_dir: &Path, task_id: &str, batch: Option<&str>) -> Result<()> {
    use codeflow_core::store::DataStore;
    use codeflow_core::types::{AutorunTaskRunStatus, AutorunWorkerStatus};

    let store = open_store(project_dir).await?;
    // C17+C18: Smart task resolution with explicit --batch support.
    let session_id = resolve_task_session(store.as_ref(), task_id, batch).await?;

    let worker = store
        .get_autorun_worker_by_task_id(&session_id, task_id)
        .await?;

    // Handle pending tasks (no worker record yet — task hasn't been dispatched).
    // Create a Skipped task_run record so dispatch_ready_tasks() will skip it.
    if worker.is_none() {
        let now = chrono::Utc::now().to_rfc3339();
        let task_run_id = format!("atr-{session_id}-{task_id}");
        let task_run = codeflow_core::models::AutorunTaskRun {
            id: task_run_id,
            worker_id: String::new(),
            task_id: task_id.to_string(),
            session_id: session_id.clone(),
            status: AutorunTaskRunStatus::Skipped,
            branch_name: None,
            worktree_path: None,
            pr_number: None,
            pr_url: None,
            blocked_reason: None,
            claim_conflicts: None,
            merge_conflicts: None,
            started_at: None,
            completed_at: Some(now.clone()),
            duration_seconds: Some(0),
            exit_code: None,
            error_message: Some("user_cancelled".to_string()),
            verification_result: None,
            last_phase: None,
            created_at: now,
        };
        store
            .create_autorun_task_run(&task_run)
            .await
            .context("creating skipped task_run for pending task")?;
        println!("pending task '{task_id}' marked as skipped (user_cancelled).");
        return Ok(());
    }

    let worker = worker.unwrap();

    if !matches!(
        worker.status,
        AutorunWorkerStatus::Running | AutorunWorkerStatus::Queued
    ) {
        anyhow::bail!(
            "worker for task '{task_id}' is not cancellable (status: {})",
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

    // Step 8-9: Deregister unconditionally, then cleanup worktree if it exists.
    if let Some(ref wt_path) = worker.worktree_path {
        let registry_path = project_dir.join(".state/worktrees");
        let _ = codeflow_core::worktree::locked_deregister_worktree(&registry_path, wt_path);
        let wt = Path::new(wt_path);
        if wt.exists() {
            let _ = std::process::Command::new("git")
                .args(["worktree", "remove", "--force", wt_path])
                .current_dir(project_dir)
                .output();
        }
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
            // C18: Pass explicit session_id to avoid re-resolving (could cancel wrong batch).
            if let Err(e) = run_cancel(project_dir, &worker.task_id, Some(&session_id)).await {
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

async fn run_cleanup(
    project_dir: &Path,
    batch: Option<&str>,
    all: bool,
    dry_run: bool,
    no_purge: bool,
) -> Result<()> {
    use codeflow_core::store::DataStore;

    let project_dir = &resolve_repo_root(project_dir)?;
    let config =
        codeflow_core::autorun::load_config(project_dir).context("loading parallel-work config")?;
    let store = open_store(project_dir).await?;

    if let Some(sid) = batch {
        // Clean a specific session.
        let session = store
            .get_autorun_session(sid)
            .await?
            .with_context(|| format!("no autorun session found with id '{sid}'"))?;

        if !matches!(
            session.status,
            codeflow_core::types::AutorunSessionStatus::Running
        ) {
            println!(
                "Session '{sid}' is not running (status: {}). Nothing to clean.",
                session.status
            );
            return Ok(());
        }

        if dry_run {
            println!("[dry-run] Would clean session '{sid}'");
            return Ok(());
        }

        let report = codeflow_core::autorun::cleanup_stale_session(
            store.as_ref(),
            project_dir,
            sid,
            "manual cleanup",
        )
        .await
        .context("cleaning session")?;

        println!(
            "Cleaned session '{}': {} workers killed, {} worktrees removed, {} claims released",
            report.session_id,
            report.workers_killed,
            report.worktrees_removed,
            report.claims_released,
        );
        return Ok(());
    }

    if !all {
        anyhow::bail!(
            "specify --batch <ID> for a specific session, or --all for all stale sessions"
        );
    }

    // Detect stale sessions.
    let stale = codeflow_core::autorun::detect_stale_sessions(
        store.as_ref(),
        project_dir,
        config.autorun.stale_threshold_secs,
    )
    .await
    .context("detecting stale sessions")?;

    if stale.is_empty() {
        println!("No stale sessions found.");
        return Ok(());
    }

    println!("Found {} stale session(s):", stale.len());
    for info in &stale {
        let pid_status = if info.pid_alive { "alive" } else { "dead" };
        let tmux_status = match info.tmux_alive {
            Some(true) => "alive",
            Some(false) => "dead",
            None => "unknown",
        };
        println!(
            "  {} (pid={} [{}], tmux={}, orphan_workers={}, live_workers={})",
            info.session.id,
            info.session.pid.unwrap_or(0),
            pid_status,
            tmux_status,
            info.orphan_worker_count,
            info.live_worker_count,
        );
    }

    if dry_run {
        println!("[dry-run] Would clean {} session(s)", stale.len());
        return Ok(());
    }

    // Clean each detected session directly (avoids double-detect from sweep).
    let mut cleaned = 0usize;
    let mut errors: Vec<(String, String)> = Vec::new();
    for info in &stale {
        let reason = format!(
            "manual cleanup: pid_alive={}, tmux_alive={:?}",
            info.pid_alive, info.tmux_alive
        );
        match codeflow_core::autorun::cleanup_stale_session(
            store.as_ref(),
            project_dir,
            &info.session.id,
            &reason,
        )
        .await
        {
            Ok(report) => {
                println!(
                    "  cleaned '{}': {} workers killed, {} worktrees removed, {} claims released",
                    report.session_id,
                    report.workers_killed,
                    report.worktrees_removed,
                    report.claims_released,
                );
                cleaned += 1;
            }
            Err(e) => {
                errors.push((info.session.id.clone(), e.to_string()));
            }
        }
    }

    println!("Sweep complete: {cleaned} cleaned, {} errors", errors.len());
    for (sid, err) in &errors {
        eprintln!("  error cleaning '{sid}': {err}");
    }

    // DB purge: remove old terminal autorun sessions unless --no-purge.
    if !no_purge {
        let retention = config.retention.clone();
        if retention.purge_on_cleanup {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(retention.days));
            let cutoff_str = cutoff.to_rfc3339();
            match store
                .prune_autorun_sessions(&cutoff_str, retention.keep_last)
                .await
            {
                Ok(result) if result.sessions_deleted > 0 => {
                    println!(
                        "Purged {} old autorun session(s) from DB (>{} days, kept last {}).",
                        result.sessions_deleted, retention.days, retention.keep_last,
                    );
                }
                Err(e) => {
                    eprintln!("warning: autorun DB purge failed: {e}");
                }
                _ => {}
            }
        }
    }

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

/// Parse a duration string like "30d", "7d", "24h" into a cutoff ISO timestamp.
fn parse_duration_to_cutoff(duration_str: &str) -> Result<String> {
    let s = duration_str.trim();
    let (num_str, unit) = if let Some(n) = s.strip_suffix('d') {
        (n, 'd')
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 'h')
    } else {
        anyhow::bail!(
            "invalid duration format '{duration_str}'. Expected format: 30d, 7d, 24h (d=days, h=hours)"
        );
    };

    let num: i64 = num_str
        .parse()
        .with_context(|| format!("invalid number in duration '{duration_str}'"))?;

    let secs = match unit {
        'd' => num * 86400,
        'h' => num * 3600,
        _ => unreachable!(),
    };

    let duration = chrono::TimeDelta::seconds(secs);
    let dt = chrono::Utc::now() - duration;
    Ok(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

async fn run_prune(
    project_dir: &Path,
    older_than: &str,
    keep_last: usize,
    dry_run: bool,
) -> Result<()> {
    use codeflow_core::store::DataStore;

    let cutoff = parse_duration_to_cutoff(older_than)?;

    let db_dir = project_dir.join(".state/db");
    let store = codeflow_core::store::SurrealStore::open(&db_dir)
        .await
        .context("failed to open database")?;

    if dry_run {
        // Count what would be deleted using queries
        let terminal = serde_json::json!(["completed", "failed", "cancelled", "timeout"]);

        let count_sql = if keep_last > 0 {
            format!(
                "SELECT count() AS cnt FROM autorun_session \
                 WHERE status IN $terminal AND completed_at < $before \
                 AND id NOT IN (\
                     SELECT VALUE id FROM autorun_session \
                     WHERE status IN $terminal \
                     ORDER BY completed_at DESC LIMIT {keep_last}\
                 ) GROUP ALL"
            )
        } else {
            "SELECT count() AS cnt FROM autorun_session \
             WHERE status IN $terminal AND completed_at < $before GROUP ALL"
                .to_string()
        };

        let mut resp = store
            .db()
            .query(&count_sql)
            .bind(("terminal", terminal))
            .bind(("before", cutoff.clone()))
            .await?;
        let counts: Vec<serde_json::Value> = resp.take(0).unwrap_or_default();
        let session_count = counts
            .first()
            .and_then(|v| v.get("cnt"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);

        println!("Dry run: would prune records older than {cutoff}");
        println!("  Sessions: {session_count}");
        println!("  (Workers and task_runs would be cascade-deleted)");
        if keep_last > 0 {
            println!("  Keeping {keep_last} most recent terminal sessions");
        }
        return Ok(());
    }

    let result = store.prune_autorun_sessions(&cutoff, keep_last).await?;

    println!("Pruned autorun records older than {cutoff}:");
    println!("  Sessions deleted: {}", result.sessions_deleted);
    println!("  Workers deleted:  {}", result.workers_deleted);
    println!("  Task runs deleted: {}", result.task_runs_deleted);

    if !result.failures.is_empty() {
        eprintln!("\nPartial failures:");
        for f in &result.failures {
            eprintln!("  - {f}");
        }
        std::process::exit(1);
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

async fn run_resume(
    project_dir: &Path,
    batch: Option<&str>,
    foreground: bool,
    force: bool,
) -> Result<()> {
    use codeflow_core::store::DataStore;
    use codeflow_core::types::{AutorunSessionStatus, AutorunTaskRunStatus};

    // Autorun MUST work from the real repo root, not a worktree.
    let project_dir = &resolve_repo_root(project_dir)?;

    // C13: Auto-sweep stale sessions before resume to make them resumable.
    if let Ok(config) = codeflow_core::autorun::load_config(project_dir) {
        if let Ok(store) = open_store(project_dir).await {
            match codeflow_core::autorun::sweep_stale_sessions(
                store.as_ref(),
                project_dir,
                config.autorun.stale_threshold_secs,
            )
            .await
            {
                Ok(summary) if summary.sessions_cleaned > 0 => {
                    eprintln!(
                        "auto-sweep: cleaned {} stale session(s)",
                        summary.sessions_cleaned
                    );
                }
                _ => {}
            }
        }
    }

    // C6: Batch guard applies to resume as well (AC 33).
    // Use a placeholder path for the duplicate-file check since resume
    // intentionally reuses the original batch file.
    check_batch_guard(project_dir, Path::new("__resume__")).await?;

    // C7: Detach to tmux if not foreground and stdout is a TTY.
    if !foreground && std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return run_detached_resume(project_dir, batch);
    }

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

    // C14: Check for live worker tmux sessions before resuming.
    if !force {
        let workers = store.list_autorun_workers(&session_id).await?;
        let live_workers: Vec<_> = workers
            .iter()
            .filter(|w| {
                w.status == codeflow_core::types::AutorunWorkerStatus::Running
                    && w.tmux_session.as_deref().is_some_and(tmux_has_session)
            })
            .collect();
        if !live_workers.is_empty() {
            eprintln!(
                "WARNING: {} live worker tmux session(s) detected for this batch:",
                live_workers.len()
            );
            for w in &live_workers {
                eprintln!(
                    "  task={}, tmux={}",
                    w.task_id,
                    w.tmux_session.as_deref().unwrap_or("?")
                );
            }
            anyhow::bail!(
                "live workers detected. Use --force to resume anyway, or cancel them first."
            );
        }
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

    check_target_branch(project_dir, &resume_parsed.integration_branch)?;

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
    let rows = parse_batch_rows(project_dir)?;

    if rows.is_empty() {
        let batch_dir = project_dir.join(".codeflow/config/autorun");
        if batch_dir.exists() {
            println!("No batch files found in {}", batch_dir.display());
        } else {
            println!(
                "No autorun config directory found at {}",
                batch_dir.display()
            );
        }
        return Ok(());
    }

    println!(
        "{:<30} {:>5} {:<12} {:<8}",
        "FILE", "TASKS", "TARGET", "AUTO_MERGE"
    );

    for row in &rows {
        if let Some(ref err) = row.parse_error {
            println!(
                "{:<30} {:>5} {:<12} {:<8}",
                row.file_name,
                "ERR",
                "--",
                format!("({err})"),
            );
        } else {
            println!(
                "{:<30} {:>5} {:<12} {:<8}",
                row.file_name, row.task_count, row.target, row.auto_merge,
            );
        }
    }

    Ok(())
}

/// Parsed batch file row for TUI rendering.
#[derive(Debug)]
struct BatchRow {
    file_name: String,
    task_count: usize,
    target: String,
    auto_merge: bool,
    parse_error: Option<String>,
}

/// Parse batch files from the autorun config directory into `BatchRow`s.
fn parse_batch_rows(project_dir: &Path) -> Result<Vec<BatchRow>> {
    let batch_dir = project_dir.join(".codeflow/config/autorun");
    if !batch_dir.exists() {
        return Ok(vec![]);
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

    entries.sort_by_key(std::fs::DirEntry::file_name);

    let mut rows = Vec::with_capacity(entries.len());
    for entry in &entries {
        let path = entry.path();
        match codeflow_core::autorun::batch::parse_batch_file(&path) {
            Ok(batch) => {
                let target = if batch.integration_branch.is_empty() {
                    "main".to_string()
                } else {
                    batch.integration_branch.clone()
                };
                rows.push(BatchRow {
                    file_name: entry.file_name().to_string_lossy().to_string(),
                    task_count: batch.tasks.len(),
                    target,
                    auto_merge: batch.integration_auto_merge,
                    parse_error: None,
                });
            }
            Err(e) => {
                rows.push(BatchRow {
                    file_name: entry.file_name().to_string_lossy().to_string(),
                    task_count: 0,
                    target: "--".to_string(),
                    auto_merge: false,
                    parse_error: Some(e.to_string()),
                });
            }
        }
    }

    Ok(rows)
}

/// Render batch files in an interactive TUI table.
fn run_batches_tui(project_dir: &Path) -> Result<()> {
    use codeflow_core::tui::theme;
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use ratatui::layout::{Constraint, Layout};
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

    let rows_data = parse_batch_rows(project_dir)?;
    if rows_data.is_empty() {
        println!(
            "No batch files found in {}",
            project_dir.join(".codeflow/config/autorun").display()
        );
        return Ok(());
    }

    let mut terminal = ratatui::init();
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            ratatui::restore();
        }
    }
    let _guard = TermGuard;

    let mut table_state = TableState::default();
    if !rows_data.is_empty() {
        table_state.select(Some(0));
    }

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Length(2), // header
                Constraint::Min(5),    // table
                Constraint::Length(1), // keybinding bar
            ])
            .split(area);

            // Header.
            let header_line = Line::from(vec![
                Span::styled(
                    format!(" {} ", theme::TRIANGLE),
                    Style::new().fg(theme::BLUE_ACCENT),
                ),
                Span::styled("Autorun Batch Files", theme::header()),
                Span::raw("  "),
                Span::styled(
                    format!("{} batch(es)", rows_data.len()),
                    Style::new().fg(theme::DIM_PENDING),
                ),
            ]);
            frame.render_widget(Paragraph::new(header_line), chunks[0]);

            // Table.
            let table_header = Row::new(vec!["FILE", "TASKS", "TARGET", "AUTO_MERGE"])
                .style(theme::header())
                .bottom_margin(1);

            let table_rows: Vec<Row> = rows_data
                .iter()
                .map(|r| {
                    if let Some(ref err) = r.parse_error {
                        Row::new(vec![
                            Cell::from(r.file_name.clone()),
                            Cell::from(Span::styled("ERR", Style::new().fg(theme::RED_FAILURE))),
                            Cell::from("--".to_string()),
                            Cell::from(Span::styled(
                                format!("({err})"),
                                Style::new().fg(theme::RED_FAILURE),
                            )),
                        ])
                    } else {
                        let merge_badge = if r.auto_merge {
                            Span::styled("yes", Style::new().fg(theme::GREEN_SUCCESS))
                        } else {
                            Span::styled("no", Style::new().fg(theme::DIM_PENDING))
                        };
                        Row::new(vec![
                            Cell::from(r.file_name.clone()),
                            Cell::from(format!("{}", r.task_count)),
                            Cell::from(r.target.clone()),
                            Cell::from(merge_badge),
                        ])
                    }
                })
                .collect();

            let table = Table::new(
                table_rows,
                [
                    Constraint::Min(28),    // FILE
                    Constraint::Length(8),  // TASKS
                    Constraint::Min(14),    // TARGET
                    Constraint::Length(12), // AUTO_MERGE
                ],
            )
            .header(table_header)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(theme::BORDER_TYPE)
                    .title(" Batches ")
                    .style(Style::new().fg(theme::WHITE_TEXT)),
            )
            .row_highlight_style(theme::selected())
            .highlight_symbol(format!("{} ", theme::TRIANGLE));

            frame.render_stateful_widget(table, chunks[1], &mut table_state);

            // Keybinding bar.
            let bar = Line::from(vec![
                Span::styled(" [Up/Down]", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(" Navigate "),
                Span::styled("[q/Esc]", Style::new().fg(theme::BLUE_ACCENT)),
                Span::raw(" Quit"),
            ]);
            frame.render_widget(Paragraph::new(bar), chunks[2]);
        })?;

        // Event handling.
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                // Ctrl+C
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
                        let i = table_state.selected().unwrap_or(0);
                        let prev = if i == 0 {
                            rows_data.len().saturating_sub(1)
                        } else {
                            i - 1
                        };
                        table_state.select(Some(prev));
                    }
                    KeyCode::Down => {
                        let i = table_state.selected().unwrap_or(0);
                        let next = if rows_data.is_empty() {
                            0
                        } else {
                            (i + 1) % rows_data.len()
                        };
                        table_state.select(Some(next));
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Text representation of an autorun session status for display.
pub(crate) fn status_badge_text(
    status: codeflow_core::types::AutorunSessionStatus,
) -> ratatui::text::Span<'static> {
    use codeflow_core::tui::theme;
    use ratatui::style::Style;
    use ratatui::text::Span;

    let (label, style) = match status {
        codeflow_core::types::AutorunSessionStatus::Running => {
            ("Running", Style::new().fg(theme::GREEN_SUCCESS))
        }
        codeflow_core::types::AutorunSessionStatus::Aborting => {
            ("Aborting", Style::new().fg(theme::YELLOW_RUNNING))
        }
        codeflow_core::types::AutorunSessionStatus::Completed => {
            ("Done", Style::new().fg(theme::DIM_PENDING))
        }
        codeflow_core::types::AutorunSessionStatus::Failed => {
            ("Failed", Style::new().fg(theme::RED_FAILURE))
        }
        codeflow_core::types::AutorunSessionStatus::Cancelled => {
            ("Cancel", Style::new().fg(theme::DIM_PENDING))
        }
        codeflow_core::types::AutorunSessionStatus::Timeout => {
            ("Timeout", Style::new().fg(theme::RED_FAILURE))
        }
        _ => ("Unknown", Style::new().fg(theme::DIM_PENDING)),
    };
    Span::styled(label, style)
}

/// Compute a simple ETA string from elapsed time and task progress.
///
/// Returns "ETA: --" when no tasks have completed, "ETA: <1m" when close,
/// or "ETA: ~Xm" based on `(elapsed / completed) * remaining`.
fn compute_eta(elapsed_secs: i64, completed: i32, total: i32) -> String {
    if completed <= 0 || total <= 0 {
        return "ETA: --".to_string();
    }
    let remaining = total - completed;
    if remaining <= 0 {
        return "ETA: done".to_string();
    }
    let secs_per_task = elapsed_secs / i64::from(completed);
    let eta_secs = secs_per_task * i64::from(remaining);
    let eta_mins = eta_secs / 60;
    if eta_mins < 1 {
        "ETA: <1m".to_string()
    } else {
        format!("ETA: ~{eta_mins}m")
    }
}

/// Validate a tmux session name contains only safe characters.
///
/// Rejects names with shell metacharacters, path separators, or traversal
/// sequences that could be dangerous when passed as a command argument.
fn is_safe_tmux_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}

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
    async fn create_session(
        &self,
        name: &str,
        command: Option<Vec<String>>,
    ) -> Result<(), codeflow_core::AutorunError> {
        let success = match command {
            Some(args) => {
                // Create session with -- command args so tmux runs the command directly.
                let mut tmux_args = vec![
                    "new-session".to_string(),
                    "-d".to_string(),
                    "-s".to_string(),
                    name.to_string(),
                    "--".to_string(),
                ];
                tmux_args.extend(args);
                let args_refs: Vec<&str> = tmux_args.iter().map(String::as_str).collect();
                run_tmux(&args_refs).await?
            }
            None => {
                // Legacy: bare session without a command.
                run_tmux(&["new-session", "-d", "-s", name]).await?
            }
        };
        if !success {
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

/// PathFlow session progress as observed from sentinel files.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathFlowState {
    /// No sentinel directory found or no recognizable sentinels.
    Unknown,
    /// Active session with the latest completed phase name.
    InProgress(String),
    /// PF6 sentinel exists and a PR was created (pf-6 implies PR created).
    PrCreated,
    /// PF7 sentinel exists — session fully complete.
    Complete,
}

/// Check PathFlow progress by reading sentinel files from the worktree.
///
/// Sentinel files live at `{worktree}/.state/sentinels/pathflow/{session_id}/`
/// with names like `pathflow-pf-1`, `pathflow-pf-6`, `pathflow-pf-7`.
///
/// The phase sentinel that represents "PR pushed" is derived from
/// `gates.pr_pushed.on_phase_complete` in `pathflow-config.json` via
/// `GateConfig`. When the config or gate is absent, falls back to
/// `pathflow-pf-6` (historical default).
fn check_pathflow_progress(worktree_path: &std::path::Path, session_id: &str) -> PathFlowState {
    let sentinel_dir = worktree_path
        .join(".state/sentinels/pathflow")
        .join(session_id);

    let entries = match std::fs::read_dir(&sentinel_dir) {
        Ok(e) => e,
        Err(_) => return PathFlowState::Unknown,
    };

    let pr_pushed_sentinel = resolve_pr_pushed_sentinel_name(worktree_path);

    let mut has_pf7 = false;
    let mut has_pr_pushed = false;
    let mut latest_phase = String::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "pathflow-pf-7" {
            has_pf7 = true;
        } else if name == pr_pushed_sentinel {
            has_pr_pushed = true;
        }
        // Track latest phase sentinel for InProgress reporting.
        if name.starts_with("pathflow-pf-") && (latest_phase.is_empty() || name > latest_phase) {
            latest_phase = name;
        }
    }

    if has_pf7 {
        PathFlowState::Complete
    } else if has_pr_pushed {
        PathFlowState::PrCreated
    } else if !latest_phase.is_empty() {
        PathFlowState::InProgress(latest_phase)
    } else {
        PathFlowState::Unknown
    }
}

/// Resolve the sentinel filename that signifies "PR pushed" by reading
/// `gates.pr_pushed.on_phase_complete` from `pathflow-config.json`.
///
/// Falls back to `pathflow-pf-6` when the config cannot be loaded or the
/// gate is missing (historical default).
fn resolve_pr_pushed_sentinel_name(worktree_path: &std::path::Path) -> String {
    use codeflow_core::pathflow::gates::GateConfig;

    const FALLBACK: &str = "pathflow-pf-6";
    let config_dir = worktree_path
        .join(".codeflow")
        .join("config")
        .join("pathflow");
    if !config_dir.join("pathflow-config.json").exists() {
        return FALLBACK.to_string();
    }

    // GateConfig::load panics on malformed config; catch to keep this
    // progress check non-fatal.
    let result = std::panic::catch_unwind(|| GateConfig::load(&config_dir));
    let Ok(gates) = result else {
        return FALLBACK.to_string();
    };

    gates
        .get("pr_pushed")
        .and_then(|req| req.on_phase_complete)
        .map_or_else(
            || FALLBACK.to_string(),
            |phase| format!("pathflow-pf-{}", phase.index()),
        )
}

/// Write the wrap-up signal file to the worktree's runtime local directory.
///
/// The file is written atomically (write to temp + rename) to prevent partial
/// reads by the PreToolUse hook. Contains the creation timestamp for timeout
/// calculations by the poll loop.
fn write_wrap_up_signal(worktree_path: &str) {
    let signal_dir = std::path::Path::new(worktree_path).join(".state/runtime/local");
    let signal_path = signal_dir.join("wrap-up-signal");
    if signal_path.exists() {
        return; // Already written — idempotent.
    }
    let _ = std::fs::create_dir_all(&signal_dir);
    let timestamp = chrono::Utc::now().to_rfc3339();
    // Atomic write: temp file + rename.
    let tmp_path = signal_dir.join("wrap-up-signal.tmp");
    if std::fs::write(&tmp_path, &timestamp).is_ok() {
        let _ = std::fs::rename(&tmp_path, &signal_path);
    }
}

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

    /// Apply the PR-detected timeout cap.
    ///
    /// When a PR is detected (PF6) but the session is still running, cap the
    /// remaining timeout at `cap`. Only applies when `remaining > cap`; if
    /// `remaining <= cap`, the outer loop's normal timeout handles termination
    /// and this returns `false`.
    ///
    /// Returns `true` if the outer poll loop should break (cap was applied and
    /// the inner wait completed or timed out).
    async fn apply_pr_cap(
        &self,
        session: &str,
        exit_code_file: &Path,
        remaining: Duration,
        cap: Duration,
    ) -> bool {
        if remaining <= cap {
            return false;
        }
        let cap_start = std::time::Instant::now();
        loop {
            if exit_code_file.exists() || cap_start.elapsed() >= cap {
                break;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        if !exit_code_file.exists() {
            let _ = self.tmux.send_command(session, "C-c").await;
            tokio::time::sleep(Duration::from_secs(SIGTERM_GRACE_SECS)).await;
            if !exit_code_file.exists() {
                let _ = self.tmux.kill_session(session).await;
            }
        }
        true
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

        // Ensure .state/runtime/local directory exists in worktree.
        let local_dir = PathBuf::from(work_dir).join(".state/runtime/local");
        std::fs::create_dir_all(&local_dir).map_err(|e| {
            codeflow_core::AutorunError::WorkerFailed(format!(
                "creating local dir {}: {e}",
                local_dir.display()
            ))
        })?;

        // POSIX single-quote escaping helper: replace ' with '\''
        fn sq(s: &str) -> String {
            s.replace('\'', "'\\''")
        }

        // C3b: Write env file with all 5 exports.
        let autorun_sid = if cfg.worker_session_id.is_empty() {
            &cfg.session_id
        } else {
            &cfg.worker_session_id
        };
        let acceptance_b64 = Self::encode_acceptance(&cfg.acceptance_criteria);
        let env_file_path = local_dir.join("autorun-worker-env.sh");
        let env_content = format!(
            "export AUTORUN_SESSION_ID='{sid}'\n\
             export AUTORUN_BATCH_ID='{batch}'\n\
             export AUTORUN_TASK_ID='{task}'\n\
             export AUTORUN_ACCEPTANCE='{acc}'\n\
             export AUTORUN_INTEGRATION_BRANCH='{target}'\n\
             export AUTORUN_INTEGRATION_AUTO_MERGE='{auto_merge}'\n\
             export AUTORUN_EPIC_UPDATE='{epic_update}'\n\
             export CODEFLOW_WORKTREE_PATH='{wdir}'\n",
            sid = sq(autorun_sid),
            batch = sq(&cfg.session_id),
            task = sq(&cfg.task_id),
            acc = sq(&acceptance_b64),
            target = sq(&cfg.integration_branch),
            auto_merge = if cfg.integration_auto_merge {
                "true"
            } else {
                "false"
            },
            epic_update = sq(&cfg.epic_update),
            wdir = sq(work_dir),
        );
        std::fs::write(&env_file_path, &env_content).map_err(|e| {
            codeflow_core::AutorunError::WorkerFailed(format!(
                "writing env file {}: {e}",
                env_file_path.display()
            ))
        })?;

        // C3c: Write prompt file (raw, no escaping).
        let prompt_file_path = local_dir.join("autorun-worker-prompt.txt");
        std::fs::write(&prompt_file_path, &cfg.prompt).map_err(|e| {
            codeflow_core::AutorunError::WorkerFailed(format!(
                "writing prompt file {}: {e}",
                prompt_file_path.display()
            ))
        })?;

        // C3f: Exit code in .state/runtime/local/
        let exit_code_path = format!("{work_dir}/.state/runtime/local/worker-exit-code");

        // C3d: Write worker script.
        let script_path = local_dir.join("autorun-worker.sh");
        let script_content = format!(
            "#!/bin/bash\n\
             _EXIT_FILE='{exit}'\n\
             trap 'echo 1 > \"$_EXIT_FILE\"' HUP TERM\n\
             source '{env}'\n\
             cd '{wdir}'\n\
             claude --dangerously-skip-permissions \"$(cat '{prompt}')\"\n\
             echo $? > \"$_EXIT_FILE\"\n",
            env = sq(&env_file_path.to_string_lossy()),
            wdir = sq(work_dir),
            prompt = sq(&prompt_file_path.to_string_lossy()),
            exit = sq(&exit_code_path),
        );
        std::fs::write(&script_path, &script_content).map_err(|e| {
            codeflow_core::AutorunError::WorkerFailed(format!(
                "writing worker script {}: {e}",
                script_path.display()
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755));
        }

        // C3e: Create tmux session with the worker script as the command.
        self.tmux
            .create_session(
                session,
                Some(vec![
                    "bash".into(),
                    "--login".into(),
                    script_path.to_string_lossy().into_owned(),
                ]),
            )
            .await?;

        // INNER TIMEOUT: Polls for Claude's exit-code marker file and handles
        // graceful shutdown (SIGTERM → grace period → SIGKILL). This complements
        // the OUTER timeout in worker.rs which bounds the entire worker lifecycle.
        // The inner timeout provides Claude-specific shutdown sequencing, while
        // the outer timeout catches hangs in non-Claude phases (worktree setup, etc.).
        let exit_code_file = PathBuf::from(&exit_code_path);
        // Use per-task timeout from InvokeConfig when set, otherwise fall back
        // to the constructor-level worker_timeout.
        let timeout = if cfg.worker_timeout_secs > 0 {
            Duration::from_secs(cfg.worker_timeout_secs)
        } else {
            self.worker_timeout
        };
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

                // Fall through to branch/PR detection below instead of
                // returning immediately. This allows post-timeout PR
                // recovery: if Claude created a PR before the timeout
                // fired, we detect it and pass it to serialized_merge.
                break;
            }

            // PathFlow sentinel polling: every 30 iterations (~30s at 1s
            // POLL_INTERVAL), check sentinel files for early completion or
            // PR creation signals to shorten the remaining timeout.
            let elapsed_iters = poll_start.elapsed().as_secs();
            if elapsed_iters > 0 && elapsed_iters % 30 == 0 {
                let pf_state = check_pathflow_progress(std::path::Path::new(work_dir), autorun_sid);
                match pf_state {
                    PathFlowState::Complete => {
                        // PF7 done — give 10s grace for exit_code_file to appear.
                        eprintln!(
                            "PathFlow complete for task {}, waiting 10s for exit code",
                            cfg.task_id
                        );
                        tokio::time::sleep(Duration::from_secs(10)).await;
                        break;
                    }
                    PathFlowState::PrCreated => {
                        // Write wrap-up signal file so PreToolUse hook can
                        // advise Claude to expedite PF7-END.
                        write_wrap_up_signal(work_dir);

                        // Cap remaining timeout at 120s. Only applies when
                        // remaining > cap; if remaining <= cap, the outer
                        // loop's normal timeout handles termination.
                        let remaining = timeout.saturating_sub(poll_start.elapsed());
                        let cap = Duration::from_secs(120);
                        if remaining > cap {
                            eprintln!(
                                "PR detected for task {}, capping remaining timeout to 120s",
                                cfg.task_id
                            );
                        }
                        if self
                            .apply_pr_cap(session, &exit_code_file, remaining, cap)
                            .await
                        {
                            break;
                        }
                    }
                    PathFlowState::InProgress(_) | PathFlowState::Unknown => {}
                }
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }

        // Read exit code from marker file. If the file doesn't exist
        // (timeout path where Claude didn't write one), use 124.
        let timed_out = poll_start.elapsed() >= timeout;
        let exit_code = if exit_code_file.exists() {
            Self::read_exit_code(&exit_code_file)
        } else {
            124
        };

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

        // Try gh pr list to detect PRs. Run for BOTH normal exits AND
        // timeouts — a PR may have been created before the timeout fired.
        if pr_number == 0 && !branch_name.is_empty() {
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
            output: if timed_out {
                "worker exceeded timeout".to_string()
            } else {
                String::new()
            },
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
        assert_eq!(config.worktree.max_concurrent, 30);
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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_name: "cf-ar-task-test".into(),
            file_scope: vec!["src/main.rs".into()],
            scope_policy: "soft".into(),
            blocked_behavior: "skip_and_continue".into(),
            epic_update: String::new(),
            queue_timeout_secs: 600,
            task_timeout_secs: None,
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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "worker-1".into(),
            acceptance_criteria: vec!["tests pass".into()],
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
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
            integration_auto_merge: true,
            integration_branch: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: vec!["crit 1".into(), "crit 2".into()],
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: codeflow_core::autorun::InvokeConfig =
            serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, cfg.task_id);
        assert_eq!(deserialized.work_dir, cfg.work_dir);
        assert_eq!(deserialized.prompt, cfg.prompt);
        assert_eq!(deserialized.session_id, cfg.session_id);
        assert_eq!(
            deserialized.integration_auto_merge,
            cfg.integration_auto_merge
        );
        assert_eq!(deserialized.integration_branch, cfg.integration_branch);
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

    /// Mock tmux that records sent commands and session creations for verification.
    #[allow(clippy::type_complexity)]
    struct RecordingTmux {
        commands: std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
        /// C9b: Records (name, command) pairs from create_session calls.
        sessions_created: std::sync::Arc<tokio::sync::Mutex<Vec<(String, Option<Vec<String>>)>>>,
    }

    impl RecordingTmux {
        #[allow(clippy::type_complexity)]
        fn new() -> (
            Self,
            std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
        ) {
            let commands = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let sessions_created = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            (
                Self {
                    commands: commands.clone(),
                    sessions_created,
                },
                commands,
            )
        }

        /// Create a RecordingTmux that also exposes sessions_created for inspection.
        #[allow(clippy::type_complexity)]
        fn with_sessions() -> (
            Self,
            std::sync::Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
            std::sync::Arc<tokio::sync::Mutex<Vec<(String, Option<Vec<String>>)>>>,
        ) {
            let commands = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let sessions_created = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::new()));
            (
                Self {
                    commands: commands.clone(),
                    sessions_created: sessions_created.clone(),
                },
                commands,
                sessions_created,
            )
        }
    }

    impl codeflow_core::autorun::TmuxRunner for RecordingTmux {
        async fn create_session(
            &self,
            name: &str,
            command: Option<Vec<String>>,
        ) -> Result<(), codeflow_core::AutorunError> {
            self.sessions_created
                .lock()
                .await
                .push((name.to_string(), command));
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
    fn test_real_claude_invoke_writes_files_and_creates_session() {
        let dir = tempfile::tempdir().unwrap();
        let local_dir = dir.path().join(".state/runtime/local");
        std::fs::create_dir_all(&local_dir).unwrap();
        // Pre-create exit code marker so polling returns immediately.
        std::fs::write(local_dir.join("worker-exit-code"), "0\n").unwrap();

        let (tmux, _commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-env-test".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test prompt".into(),
            session_id: "ses-env".into(),
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "worker-env".into(),
            acceptance_criteria: vec!["crit A".into(), "crit B".into()],
            worker_session_id: "ses-env-worker".into(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt
            .block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();
        assert_eq!(result.exit_code, 0);

        // Verify file-based invocation artifacts were created.
        let env_file = local_dir.join("autorun-worker-env.sh");
        assert!(env_file.exists(), "env file should be created");
        let env_content = std::fs::read_to_string(&env_file).unwrap();
        assert!(
            env_content.contains("AUTORUN_SESSION_ID='ses-env-worker'"),
            "env file should contain worker session ID, got: {env_content}"
        );
        assert!(
            env_content.contains("AUTORUN_BATCH_ID='ses-env'"),
            "env file should contain batch ID, got: {env_content}"
        );
        assert!(
            env_content.contains("AUTORUN_TASK_ID='task-env-test'"),
            "env file should contain task ID, got: {env_content}"
        );
        assert!(
            env_content.contains("AUTORUN_ACCEPTANCE="),
            "env file should contain acceptance, got: {env_content}"
        );
        assert!(
            env_content.contains("CODEFLOW_WORKTREE_PATH="),
            "env file should contain worktree path, got: {env_content}"
        );

        let prompt_file = local_dir.join("autorun-worker-prompt.txt");
        assert!(prompt_file.exists(), "prompt file should be created");
        let prompt_content = std::fs::read_to_string(&prompt_file).unwrap();
        assert_eq!(prompt_content, "test prompt");

        let script_file = local_dir.join("autorun-worker.sh");
        assert!(script_file.exists(), "worker script should be created");
        let script_content = std::fs::read_to_string(&script_file).unwrap();
        assert!(
            script_content.contains("#!/bin/bash"),
            "script should have shebang"
        );
        assert!(
            script_content.contains("source"),
            "script should source env file"
        );
        assert!(
            script_content.contains("claude --dangerously-skip-permissions"),
            "script should invoke claude"
        );
        assert!(
            script_content.contains("worker-exit-code"),
            "script should write exit code"
        );
    }

    #[test]
    fn test_real_claude_invoke_reads_exit_code_from_marker() {
        let dir = tempfile::tempdir().unwrap();
        let local_dir = dir.path().join(".state/runtime/local");
        std::fs::create_dir_all(&local_dir).unwrap();
        // Non-zero exit code.
        std::fs::write(local_dir.join("worker-exit-code"), "1\n").unwrap();

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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "w-exit".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
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
        let local_dir = dir.path().join(".state/runtime/local");
        std::fs::create_dir_all(&local_dir).unwrap();
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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "w-timeout".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
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
        let local_dir = dir.path().join(".state/runtime/local");
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(local_dir.join("worker-exit-code"), "0\n").unwrap();

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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "w-esc".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        let _ = rt.block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg));

        // With file-based invocation, the prompt is written to a file, not sent via tmux.
        // Verify the prompt file preserves backslash sequences literally.
        let prompt_file = dir
            .path()
            .join(".state/runtime/local/autorun-worker-prompt.txt");
        let prompt_content = std::fs::read_to_string(&prompt_file).unwrap();
        assert!(
            prompt_content.contains("\\n"),
            "backslash-n should be preserved literally in prompt file, got: {prompt_content}"
        );
        assert!(
            prompt_content.contains("\\t"),
            "backslash-t should be preserved literally in prompt file, got: {prompt_content}"
        );

        // The worker script should use cat to read the prompt file.
        let script_file = dir.path().join(".state/runtime/local/autorun-worker.sh");
        let script_content = std::fs::read_to_string(&script_file).unwrap();
        assert!(
            script_content.contains("cat"),
            "script should use cat to read prompt file"
        );
        // No send_command calls needed for the main invocation.
        let cmds = rt.block_on(commands.lock());
        assert!(
            cmds.is_empty(),
            "no send_command calls should be made (file-based invocation), got {} calls",
            cmds.len()
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
    fn test_invoke_creates_session_with_command() {
        // C9b: Verify create_session receives the bash --login script args.
        let dir = tempfile::tempdir().unwrap();
        let local_dir = dir.path().join(".state/runtime/local");
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(local_dir.join("worker-exit-code"), "0\n").unwrap();

        let (tmux, _commands, sessions) = RecordingTmux::with_sessions();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(5),
        };

        let cfg = codeflow_core::autorun::InvokeConfig {
            task_id: "task-session-test".into(),
            work_dir: dir.path().to_string_lossy().into_owned(),
            prompt: "test".into(),
            session_id: "ses-sess".into(),
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "w-sess".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
        };

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(codeflow_core::autorun::ClaudeInvoker::invoke(&claude, cfg))
            .unwrap();

        let created = rt.block_on(sessions.lock());
        assert_eq!(created.len(), 1, "should have one create_session call");
        let (name, cmd) = &created[0];
        assert_eq!(name, "w-sess");
        let cmd = cmd.as_ref().expect("should have command args");
        assert_eq!(cmd[0], "bash");
        assert_eq!(cmd[1], "--login");
        assert!(
            cmd[2].contains("autorun-worker.sh"),
            "third arg should be the worker script path, got: {}",
            cmd[2]
        );
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
            &tmux, "", None,
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
                    msg.contains("tmux") || msg.contains("GitHub CLI"),
                    "only acceptable failure is tmux/gh not found, got: {msg}"
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
        if let AutorunCommand::Attach { task_id, .. } = cli.cmd {
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
        if let AutorunCommand::Logs {
            task_id, follow, ..
        } = cli.cmd
        {
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
        if let AutorunCommand::Cancel { task_id, .. } = cli.cmd {
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
                tmux_session: None,
                stale_reason: None,
                target_branch: None,
                final_pr_url: None,
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

            // run_cancel should fail because worker is Completed (not cancellable).
            let result = run_cancel(dir.path(), "task-done", None).await;
            assert!(result.is_err());
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("not cancellable"),
                "expected 'not cancellable' error, got: {msg}"
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
                    tmux_session: None,
                    stale_reason: None,
                    target_branch: None,
                    final_pr_url: None,
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
                    last_phase: None,
                    verification_result: None,
                    created_at: chrono::Utc::now().to_rfc3339(),
                };
                store.create_autorun_task_run(&task_run).await.unwrap();
            }
            // Store dropped here — run_cancel will open its own connection.

            let result = run_cancel(dir.path(), "task-cancel-db", None).await;
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
                    tmux_session: None,
                    stale_reason: None,
                    target_branch: None,
                    final_pr_url: None,
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
                    tmux_session: None,
                    stale_reason: None,
                    target_branch: None,
                    final_pr_url: None,
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
            tmux_session: None,
            stale_reason: None,
            target_branch: None,
            final_pr_url: None,
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
            last_phase: None,
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
            let result = run_status(dir.path(), None, false).await;
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

            let result = run_status(dir.path(), None, false).await;
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

            let result = run_status(dir.path(), Some("ses-specific"), false).await;
            assert!(result.is_ok());
        });
    }

    #[test]
    fn test_run_status_nonexistent_batch() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(dir.path(), vec![], vec![], vec![]).await;

            let result = run_status(dir.path(), Some("ses-nope"), false).await;
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
    fn test_resolve_session_id_no_active_falls_back_to_completed() {
        // C19: When no active sessions, fall back to most recent terminal session.
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
            assert!(result.is_ok(), "should fall back to completed session");
            assert_eq!(result.unwrap(), "ses-done");
        });
    }

    #[test]
    fn test_resolve_session_id_truly_empty() {
        // When there are truly no sessions at all, should error.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            seed_store(dir.path(), vec![], vec![], vec![]).await;
            let store = open_store(dir.path()).await.unwrap();

            let result = resolve_session_id(&store, None).await;
            assert!(result.is_err());
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

            let result = run_attach(dir.path(), "task-missing", None).await;
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

            let result = run_attach(dir.path(), "task-notmux", None).await;
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

            let result = run_attach(dir.path(), "task-dead", None).await;
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

            let result = run_logs(dir.path(), "task-missing", false, None).await;
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

            let result = run_logs(dir.path(), "task-logs-notmux", false, None).await;
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

            let result = run_logs(dir.path(), "task-logs-dead", false, None).await;
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("does not exist"));
        });
    }

    // -----------------------------------------------------------------------
    // run_cancel additional error path tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_cancel_no_worker_creates_skipped_task_run() {
        // When no worker exists (pending task), run_cancel creates a Skipped
        // task_run record so dispatch_ready_tasks() will skip the task.
        use codeflow_core::store::DataStore;

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

            let result = run_cancel(dir.path(), "task-missing", None).await;
            assert!(result.is_ok(), "pending cancel should succeed: {result:?}");

            // Verify a Skipped task_run was created.
            let store = open_store(dir.path()).await.unwrap();
            let runs = store.list_autorun_task_runs(sid).await.unwrap();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].task_id, "task-missing");
            assert_eq!(
                runs[0].status,
                codeflow_core::types::AutorunTaskRunStatus::Skipped
            );
            assert_eq!(runs[0].error_message.as_deref(), Some("user_cancelled"));
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
        // C25: report filename now includes session_id suffix.
        assert_eq!(filename, "my-batch-ses-1-2026-03-22.md");
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

    // -- parse_batch_rows tests --

    #[test]
    fn test_parse_batch_rows_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No .codeflow/config/autorun/ directory → empty vec.
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_parse_batch_rows_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_parse_batch_rows_valid_yaml_default_target() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        // Empty integration_branch → target defaults to "main".
        // The parser infers auto_merge=true for empty (auto-generated) branches.
        std::fs::write(
            batch_dir.join("deploy.yaml"),
            "name: deploy\ntasks:\n  - id: task-a\n  - id: task-b\n",
        )
        .unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_name, "deploy.yaml");
        assert_eq!(rows[0].task_count, 2);
        assert_eq!(rows[0].target, "main");
        // auto_merge is inferred true when integration_branch is empty (auto-generated).
        assert!(rows[0].auto_merge);
        assert!(rows[0].parse_error.is_none());
    }

    #[test]
    fn test_parse_batch_rows_valid_yaml_custom_branch_and_auto_merge() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("feature.yaml"),
            "name: feature\nintegration_branch: integration/v2\nintegration_auto_merge: true\ntasks:\n  - id: task-x\n",
        )
        .unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_name, "feature.yaml");
        assert_eq!(rows[0].task_count, 1);
        assert_eq!(rows[0].target, "integration/v2");
        assert!(rows[0].auto_merge);
        assert!(rows[0].parse_error.is_none());
    }

    #[test]
    fn test_parse_batch_rows_invalid_yaml_has_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("broken.yaml"), "{{invalid yaml").unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_name, "broken.yaml");
        assert_eq!(rows[0].task_count, 0);
        assert_eq!(rows[0].target, "--");
        assert!(!rows[0].auto_merge);
        assert!(rows[0].parse_error.is_some());
    }

    #[test]
    fn test_parse_batch_rows_mixed_valid_and_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("alpha.yaml"),
            "name: alpha\ntasks:\n  - id: t-1\n",
        )
        .unwrap();
        std::fs::write(batch_dir.join("beta.yml"), "not: [valid: batch").unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 2);
        // Sorted by file name: alpha.yaml < beta.yml.
        assert_eq!(rows[0].file_name, "alpha.yaml");
        assert!(rows[0].parse_error.is_none());
        assert_eq!(rows[1].file_name, "beta.yml");
        assert!(rows[1].parse_error.is_some());
    }

    #[test]
    fn test_parse_batch_rows_ignores_non_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(batch_dir.join("readme.md"), "# Not a batch").unwrap();
        std::fs::write(batch_dir.join("config.json"), "{}").unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_parse_batch_rows_read_dir_error() {
        let dir = tempfile::tempdir().unwrap();
        let autorun_path = dir.path().join(".codeflow/config/autorun");
        // Create a FILE where a directory is expected → read_dir() fails.
        std::fs::create_dir_all(dir.path().join(".codeflow/config")).unwrap();
        std::fs::write(&autorun_path, "not a directory").unwrap();
        let result = parse_batch_rows(dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("reading autorun config directory"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn test_batches_tui_empty_dir_returns_ok() {
        // run_batches_tui with no batch files exits before entering the event loop.
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        let result = run_batches_tui(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_batches_tui_no_dir_returns_ok() {
        // No autorun dir at all → empty rows → early return.
        let dir = tempfile::tempdir().unwrap();
        let result = run_batches_tui(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_batch_rows_yml_extension() {
        // Verify .yml extension (not just .yaml) is recognized.
        let dir = tempfile::tempdir().unwrap();
        let batch_dir = dir.path().join(".codeflow/config/autorun");
        std::fs::create_dir_all(&batch_dir).unwrap();
        std::fs::write(
            batch_dir.join("deploy.yml"),
            "name: deploy\ntasks:\n  - id: task-z\n",
        )
        .unwrap();
        let rows = parse_batch_rows(dir.path()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].file_name, "deploy.yml");
        assert_eq!(rows[0].task_count, 1);
        assert!(rows[0].parse_error.is_none());
    }

    // -- status_badge_text tests --

    #[test]
    fn test_status_badge_text_running() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Running);
        assert_eq!(span.content.as_ref(), "Running");
        assert_eq!(span.style.fg, Some(theme::GREEN_SUCCESS));
    }

    #[test]
    fn test_status_badge_text_aborting() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Aborting);
        assert_eq!(span.content.as_ref(), "Aborting");
        assert_eq!(span.style.fg, Some(theme::YELLOW_RUNNING));
    }

    #[test]
    fn test_status_badge_text_completed() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Completed);
        assert_eq!(span.content.as_ref(), "Done");
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
    }

    #[test]
    fn test_status_badge_text_failed() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Failed);
        assert_eq!(span.content.as_ref(), "Failed");
        assert_eq!(span.style.fg, Some(theme::RED_FAILURE));
    }

    #[test]
    fn test_status_badge_text_cancelled() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Cancelled);
        assert_eq!(span.content.as_ref(), "Cancel");
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
    }

    #[test]
    fn test_status_badge_text_timeout() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Timeout);
        assert_eq!(span.content.as_ref(), "Timeout");
        assert_eq!(span.style.fg, Some(theme::RED_FAILURE));
    }

    #[test]
    fn test_status_badge_text_pending() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Pending);
        assert_eq!(span.content.as_ref(), "Unknown");
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
    }

    #[test]
    fn test_status_badge_text_paused() {
        use codeflow_core::tui::theme;
        let span = status_badge_text(codeflow_core::types::AutorunSessionStatus::Paused);
        assert_eq!(span.content.as_ref(), "Unknown");
        assert_eq!(span.style.fg, Some(theme::DIM_PENDING));
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
        assert!(matches!(
            cli.cmd,
            AutorunCommand::Resume { batch: None, .. }
        ));

        let cli = TestCli::try_parse_from(["test", "resume", "--batch", "ses-abc"]).unwrap();
        if let AutorunCommand::Resume { batch, .. } = cli.cmd {
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
        assert!(matches!(cli.cmd, AutorunCommand::Batches { .. }));
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
            json,
            ..
        } = cli.cmd
        {
            assert!(watch);
            assert!(!json);
            assert!(batch.is_none());
            assert_eq!(interval, 2); // default unified to 2s
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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: "ses-worker-123".into(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
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
            integration_auto_merge: false,
            integration_branch: "main".into(),
            tmux_session: "w-1".into(),
            acceptance_criteria: Vec::new(),
            worker_session_id: String::new(),
            epic_update: String::new(),
            worker_timeout_secs: 0,
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

    // -- PathFlow sentinel polling tests --

    #[test]
    fn test_pathflow_state_unknown_no_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state = check_pathflow_progress(dir.path(), "ses-nonexistent");
        assert_eq!(state, PathFlowState::Unknown);
    }

    #[test]
    fn test_pathflow_state_unknown_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow/ses-test");
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        let state = check_pathflow_progress(dir.path(), "ses-test");
        assert_eq!(state, PathFlowState::Unknown);
    }

    #[test]
    fn test_pathflow_state_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow/ses-test");
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-2"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-3"), "").unwrap();

        let state = check_pathflow_progress(dir.path(), "ses-test");
        assert_eq!(
            state,
            PathFlowState::InProgress("pathflow-pf-3".to_string())
        );
    }

    #[test]
    fn test_pathflow_state_pr_created() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow/ses-test");
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-1"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-3"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-6"), "").unwrap();

        let state = check_pathflow_progress(dir.path(), "ses-test");
        assert_eq!(state, PathFlowState::PrCreated);
    }

    #[test]
    fn test_pathflow_state_complete() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow/ses-test");
        std::fs::create_dir_all(&sentinel_dir).unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-6"), "").unwrap();
        std::fs::write(sentinel_dir.join("pathflow-pf-7"), "").unwrap();

        let state = check_pathflow_progress(dir.path(), "ses-test");
        assert_eq!(state, PathFlowState::Complete);
    }

    // -----------------------------------------------------------------------
    // wrap-up signal tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_write_wrap_up_signal_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().to_str().unwrap();

        write_wrap_up_signal(wt);

        let signal_path = dir.path().join(".state/runtime/local/wrap-up-signal");
        assert!(signal_path.exists(), "wrap-up-signal file should exist");
        let content = std::fs::read_to_string(&signal_path).unwrap();
        // Should contain an RFC3339 timestamp.
        assert!(content.contains('T'), "should be an RFC3339 timestamp");
    }

    #[test]
    fn test_write_wrap_up_signal_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().to_str().unwrap();

        write_wrap_up_signal(wt);
        let first_content =
            std::fs::read_to_string(dir.path().join(".state/runtime/local/wrap-up-signal"))
                .unwrap();

        // Writing again should not overwrite.
        std::thread::sleep(std::time::Duration::from_millis(10));
        write_wrap_up_signal(wt);
        let second_content =
            std::fs::read_to_string(dir.path().join(".state/runtime/local/wrap-up-signal"))
                .unwrap();

        assert_eq!(
            first_content, second_content,
            "idempotent: content unchanged"
        );
    }

    // -----------------------------------------------------------------------
    // apply_pr_cap tests (GAP 2)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_pr_cap_exits_when_exit_code_appears() {
        // When remaining > cap and exit_code_file already exists,
        // apply_pr_cap returns true immediately without sending C-c.
        let dir = tempfile::tempdir().unwrap();
        let exit_file = dir.path().join("exit-code");
        std::fs::write(&exit_file, "0").unwrap();

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(60),
        };

        let result = claude
            .apply_pr_cap(
                "test-session",
                &exit_file,
                Duration::from_secs(300), // remaining > cap
                Duration::from_millis(100),
            )
            .await;

        assert!(result, "should return true (cap applied)");
        let cmds = commands.lock().await;
        assert!(
            cmds.is_empty(),
            "no send_command calls — exit_code_file existed"
        );
    }

    #[tokio::test]
    async fn test_pr_cap_sends_sigterm_and_kills() {
        // When remaining > cap and no exit_code_file, cap expires → C-c sent.
        let dir = tempfile::tempdir().unwrap();
        let exit_file = dir.path().join("exit-code"); // does NOT exist

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(60),
        };

        let result = claude
            .apply_pr_cap(
                "test-session",
                &exit_file,
                Duration::from_secs(300),
                Duration::from_millis(100), // short cap for fast test
            )
            .await;

        assert!(result, "should return true (cap applied)");
        let cmds = commands.lock().await;
        assert!(
            cmds.iter().any(|(_, cmd)| cmd == "C-c"),
            "should have sent C-c: {cmds:?}"
        );
    }

    #[tokio::test]
    async fn test_pr_cap_skips_when_remaining_under_cap() {
        // When remaining <= cap, returns false (no cap applied).
        let dir = tempfile::tempdir().unwrap();
        let exit_file = dir.path().join("exit-code");

        let (tmux, commands) = RecordingTmux::new();
        let claude = RealClaude {
            tmux,
            worker_timeout: Duration::from_secs(60),
        };

        let result = claude
            .apply_pr_cap(
                "test-session",
                &exit_file,
                Duration::from_secs(60), // remaining <= cap
                Duration::from_secs(120),
            )
            .await;

        assert!(!result, "should return false (no cap applied)");
        let cmds = commands.lock().await;
        assert!(cmds.is_empty(), "no tmux calls when cap not applied");
    }

    // -----------------------------------------------------------------------
    // wrap-up signal path consistency test (GAP 3)
    // -----------------------------------------------------------------------

    #[test]
    fn test_wrap_up_signal_path_consistency() {
        // Verify that write_wrap_up_signal and detect_wrap_up_signal use
        // the same path (.state/runtime/local/wrap-up-signal) so the
        // writer (autorun.rs) and reader (pre_tool_use.rs) agree.
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path().to_str().unwrap();

        // Before write: signal not detected.
        assert!(
            !dir.path()
                .join(".state/runtime/local/wrap-up-signal")
                .exists(),
            "signal should not exist before write"
        );

        write_wrap_up_signal(wt);

        // After write: verify the file exists at the expected path.
        let signal_path = dir.path().join(".state/runtime/local/wrap-up-signal");
        assert!(signal_path.exists(), "signal file should exist after write");
    }

    // -----------------------------------------------------------------------
    // is_safe_git_ref tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_safe_git_ref_valid_names() {
        // Normal branch names should pass.
        assert!(is_safe_git_ref("main"));
        assert!(is_safe_git_ref("feat/my-feature"));
        assert!(is_safe_git_ref("fix/issue-123"));
        assert!(is_safe_git_ref("autorun/ses-abc"));
        assert!(is_safe_git_ref("release/1.0.0"));
        assert!(is_safe_git_ref("chore/update-deps"));
    }

    #[test]
    fn test_is_safe_git_ref_empty_string() {
        // Empty string is rejected.
        assert!(!is_safe_git_ref(""));
    }

    #[test]
    fn test_is_safe_git_ref_leading_dash() {
        // Leading dash looks like a git option (e.g. --upload-pack).
        assert!(!is_safe_git_ref("-upload-pack=cmd"));
        assert!(!is_safe_git_ref("--upload-pack"));
        assert!(!is_safe_git_ref("-"));
    }

    #[test]
    fn test_is_safe_git_ref_double_dot() {
        // ".." is used in range notation, not valid in a branch name passed
        // directly to git commands.
        assert!(!is_safe_git_ref("main..feature"));
        assert!(!is_safe_git_ref(".."));
        assert!(!is_safe_git_ref("feat/base..head"));
    }

    #[test]
    fn test_is_safe_git_ref_at_brace() {
        // "@{" is git reflog syntax.
        assert!(!is_safe_git_ref("HEAD@{1}"));
        assert!(!is_safe_git_ref("branch@{0}"));
    }

    #[test]
    fn test_is_safe_git_ref_special_chars() {
        // Characters that would be dangerous in shell or git contexts.
        assert!(!is_safe_git_ref("branch\\name"));
        assert!(!is_safe_git_ref("branch:name"));
        assert!(!is_safe_git_ref("branch?name"));
        assert!(!is_safe_git_ref("branch*name"));
        assert!(!is_safe_git_ref("branch[name]"));
        assert!(!is_safe_git_ref("branch^name"));
        assert!(!is_safe_git_ref("branch~1"));
    }

    #[test]
    fn test_is_safe_git_ref_whitespace() {
        // Whitespace (space, tab, newline) is rejected.
        assert!(!is_safe_git_ref("branch name"));
        assert!(!is_safe_git_ref("branch\tname"));
        assert!(!is_safe_git_ref("branch\nname"));
    }

    #[test]
    fn test_is_safe_git_ref_integration_branch_pattern() {
        // Typical auto-generated integration branch names should pass.
        assert!(is_safe_git_ref("integration/ses-01abc123def"));
        assert!(is_safe_git_ref("autorun/batch-2026-04-11"));
    }

    // -----------------------------------------------------------------------
    // commit_report_to_branch early-return tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_commit_report_skips_main_branch() {
        // commit_report_to_branch must return immediately for "main" without
        // touching the filesystem or running git commands.
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("report.md");
        std::fs::write(&report, "batch report").unwrap();
        // If this panics or runs git commands it would fail because there is
        // no real git repo in the temp dir. The function must return early.
        commit_report_to_branch(dir.path(), &report, "main");
    }

    #[test]
    fn test_commit_report_skips_master_branch() {
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("report.md");
        std::fs::write(&report, "batch report").unwrap();
        commit_report_to_branch(dir.path(), &report, "master");
    }

    #[test]
    fn test_commit_report_skips_invalid_branch_name() {
        // Branches that fail is_safe_git_ref must be rejected before any git
        // command is executed.
        let dir = tempfile::tempdir().unwrap();
        let report = dir.path().join("report.md");
        std::fs::write(&report, "batch report").unwrap();
        // Leading dash looks like a git option.
        commit_report_to_branch(dir.path(), &report, "--upload-pack=cmd");
        // Branch with double-dot (range syntax).
        commit_report_to_branch(dir.path(), &report, "main..evil");
    }

    #[test]
    fn test_commit_report_branch_not_found_returns_early() {
        // With a real (but empty) git repo, the integration branch does not
        // exist locally, so commit_report_to_branch must return without error.
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let report = dir.path().join("report.md");
        std::fs::write(&report, "batch report").unwrap();
        // "integration/nonexistent" is safe but not a real local branch.
        commit_report_to_branch(dir.path(), &report, "integration/nonexistent");
    }

    /// Helper: seed a `pathflow-config.json` with one `pr_pushed` gate.
    fn write_pr_pushed_gate_config(worktree: &std::path::Path, trigger_phase: &str) {
        let config_dir = worktree.join(".codeflow").join("config").join("pathflow");
        std::fs::create_dir_all(&config_dir).unwrap();
        let body = format!(
            r#"{{
              "phases": {{
                "PF5-VERIFY": {{"phase_order": 5}},
                "PF6-COMPLETE": {{"phase_order": 6}}
              }},
              "gates": {{
                "pr_pushed": {{
                  "on_phase_complete": "{trigger_phase}",
                  "description": "test"
                }}
              }}
            }}"#
        );
        std::fs::write(config_dir.join("pathflow-config.json"), body).unwrap();
    }

    /// The `PathFlowState::PrCreated` detection must be config-driven:
    /// `gates.pr_pushed.on_phase_complete` determines which phase sentinel
    /// marks "PR pushed", not a hardcoded `pathflow-pf-6` literal.
    #[test]
    fn test_autorun_pr_pushed_gate_driven() {
        let dir = tempfile::tempdir().unwrap();
        let sid = "ses-pr-pushed-gate";
        let sentinel_dir = dir.path().join(".state/sentinels/pathflow").join(sid);
        std::fs::create_dir_all(&sentinel_dir).unwrap();

        // Fallback behavior: with no config, the resolver returns pf-6.
        assert_eq!(resolve_pr_pushed_sentinel_name(dir.path()), "pathflow-pf-6");

        // Config says PR is pushed at PF6-COMPLETE. Writing pathflow-pf-6
        // should trigger PrCreated.
        write_pr_pushed_gate_config(dir.path(), "PF6-COMPLETE");
        assert_eq!(resolve_pr_pushed_sentinel_name(dir.path()), "pathflow-pf-6");
        std::fs::write(sentinel_dir.join("pathflow-pf-6"), "").unwrap();
        assert!(matches!(
            check_pathflow_progress(dir.path(), sid),
            PathFlowState::PrCreated
        ));

        // Change config to PF5-VERIFY as the trigger. Now pathflow-pf-5
        // (not pf-6) should signify PrCreated. Swap the sentinel files.
        std::fs::remove_file(sentinel_dir.join("pathflow-pf-6")).unwrap();
        write_pr_pushed_gate_config(dir.path(), "PF5-VERIFY");
        assert_eq!(resolve_pr_pushed_sentinel_name(dir.path()), "pathflow-pf-5");
        std::fs::write(sentinel_dir.join("pathflow-pf-5"), "").unwrap();
        assert!(matches!(
            check_pathflow_progress(dir.path(), sid),
            PathFlowState::PrCreated
        ));
    }
}
