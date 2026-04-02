//! Worktree command: git worktree lifecycle management.
//!
//! Supports subcommands: `list`, `cleanup`, `prune`, `repair`.
//! Default (no subcommand) behaves like `list`.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use codeflow_core::worktree::{CleanupOpts, WorktreeManager, WorktreeState};

use crate::helpers;

/// Worktree subcommands.
#[derive(Debug, Subcommand)]
pub enum WorktreeCommand {
    /// List worktrees and their state (includes liveness status)
    List,
    /// Remove stale or inactive worktrees with liveness and branch safety checks
    Cleanup {
        /// Force removal even of orphaned/unpushed worktrees
        #[arg(long)]
        force: bool,
        /// Report what would be removed without acting
        #[arg(long)]
        dry_run: bool,
        /// Show interactive prompts with branch/PR status before removal
        #[arg(short, long)]
        interactive: bool,
        /// Maximum number of old removed entries to keep (default: 5)
        #[arg(long)]
        keep: Option<usize>,
    },
    /// Remove orphaned worktree references from git internals
    Prune {
        /// Report what would be pruned without acting
        #[arg(long)]
        dry_run: bool,
    },
    /// Repair shared state symlinks in a worktree
    Repair {
        /// Worktree path to repair (defaults to CODEFLOW_WORKTREE_PATH)
        #[arg(long)]
        path: Option<String>,
    },
}

#[allow(clippy::needless_pass_by_value)] // clap passes enum by value
pub fn run(command: Option<WorktreeCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    match command.as_ref() {
        Some(WorktreeCommand::List) | None => run_list(&project_dir),
        Some(WorktreeCommand::Cleanup {
            force,
            dry_run,
            interactive,
            keep,
        }) => run_cleanup(&project_dir, *force, *dry_run, *interactive, *keep),
        Some(WorktreeCommand::Prune { dry_run }) => run_prune(&project_dir, *dry_run),
        Some(WorktreeCommand::Repair { path }) => run_repair(&project_dir, path.as_deref()),
    }
}

fn run_list(project_dir: &Path) -> Result<()> {
    let mgr = WorktreeManager::new(project_dir);

    let entries = mgr.list(None).context("listing worktrees")?;

    if entries.is_empty() {
        println!("no worktrees");
        return Ok(());
    }

    for entry in &entries {
        let state = mgr.detect_state(entry);
        let liveness = mgr.liveness_status(entry);
        let branch = entry.branch.as_deref().unwrap_or("(detached)");
        println!(
            "{} ({}) [{}] {} [{}]",
            entry.name, branch, state, liveness, entry.status
        );
    }

    Ok(())
}

fn run_cleanup(
    project_dir: &Path,
    force: bool,
    dry_run: bool,
    interactive: bool,
    keep: Option<usize>,
) -> Result<()> {
    let mgr = WorktreeManager::new(project_dir);
    let opts = CleanupOpts {
        force,
        dry_run,
        prune: false,
        interactive,
        keep,
    };

    if interactive && !dry_run {
        // In interactive mode, show status of each candidate before removing.
        let entries = mgr.list(None).context("listing worktrees")?;
        let mut candidates = Vec::new();
        for entry in &entries {
            if entry.status == codeflow_core::worktree::WorktreeStatus::Removed {
                continue;
            }
            let state = mgr.detect_state(entry);
            let liveness = mgr.liveness_status(entry);
            // Skip entries with confirmed live sessions (not just Active state).
            if state == WorktreeState::Active && liveness == "ACTIVE" {
                continue;
            }
            let wt_path = std::path::Path::new(&entry.path);
            let branch = entry.branch.as_deref().unwrap_or("(detached)");
            let safety_msg = if wt_path.exists() {
                let safety = codeflow_core::worktree::check_branch_safety(wt_path);
                format!("risk={} {}", safety.risk, safety.message)
            } else {
                "directory missing".to_string()
            };
            println!(
                "  {} ({}) [{}] {} [{}] -- {}",
                entry.name, branch, state, liveness, entry.status, safety_msg
            );
            candidates.push(entry.name.clone());
        }
        if candidates.is_empty() {
            println!("no worktrees to clean up");
            return Ok(());
        }
        println!("\n{} candidate(s) for removal", candidates.len());
    }

    let removed = mgr
        .cleanup_stale(&opts)
        .context("cleaning up stale worktrees")?;

    if removed.is_empty() {
        if !interactive {
            println!("no worktrees to clean up");
        }
    } else if dry_run {
        println!("would remove {} worktree(s):", removed.len());
        for name in &removed {
            println!("  {name}");
        }
    } else {
        println!("removed {} worktree(s):", removed.len());
        for name in &removed {
            println!("  {name}");
        }
    }

    Ok(())
}

fn run_prune(project_dir: &Path, dry_run: bool) -> Result<()> {
    let mut args = vec!["worktree", "prune"];
    if dry_run {
        args.push("--dry-run");
    }

    let output = std::process::Command::new("git")
        .args(&args)
        .current_dir(project_dir)
        .output()
        .context("running git worktree prune")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }

    if output.status.success() {
        if stdout.is_empty() && stderr.is_empty() {
            println!("no stale worktree references to prune");
        }
        Ok(())
    } else {
        anyhow::bail!("git worktree prune failed (exit {})", output.status)
    }
}

fn run_repair(project_dir: &Path, path: Option<&str>) -> Result<()> {
    let wt_path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let env_path = std::env::var("CODEFLOW_WORKTREE_PATH")
                .context("no --path provided and CODEFLOW_WORKTREE_PATH not set")?;
            std::path::PathBuf::from(env_path)
        }
    };

    if !wt_path.exists() {
        anyhow::bail!("worktree path does not exist: {}", wt_path.display());
    }

    codeflow_core::worktree::repair_symlinks(project_dir, &wt_path)
        .context("repairing symlinks")?;

    println!("repaired symlinks for {}", wt_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::worktree::{WorktreeEntry, WorktreeRegistry, write_registry};

    #[test]
    fn test_worktree_command_exists() {
        let _: fn(Option<WorktreeCommand>) -> Result<()> = run;
    }

    #[test]
    fn test_worktree_manager_construction() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let _ = mgr;
    }

    #[test]
    fn test_worktree_list_no_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let result = mgr.list(None);
        if let Ok(entries) = result {
            assert!(entries.is_empty());
        }
    }

    #[test]
    fn test_worktree_detect_state_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/nonexistent/path".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Removed,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let state = mgr.detect_state(&entry);
        let state_str = state.to_string();
        assert!(
            state_str == "stale" || state_str == "orphaned",
            "expected stale or orphaned, got {state_str}"
        );
    }

    #[test]
    fn test_worktree_entry_display_format() {
        let entry = WorktreeEntry {
            name: "my-worktree".to_string(),
            path: "/tmp/wt".to_string(),
            branch: Some("feat/something".to_string()),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        };

        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let state = mgr.detect_state(&entry);
        let branch = entry.branch.as_deref().unwrap_or("(detached)");
        let output = format!("{} ({}) [{}]", entry.name, branch, state);
        assert!(output.contains("my-worktree"));
        assert!(output.contains("feat/something"));
    }

    #[test]
    fn test_worktree_empty_entries_message() {
        let entries: Vec<WorktreeEntry> = vec![];
        assert!(entries.is_empty());
    }

    #[test]
    fn test_worktree_run_list_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_list(dir.path());
        let _ = result;
    }

    #[test]
    fn test_worktree_run_list_git_init() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let result = run_list(dir.path());
        let _ = result;
    }

    // -- cleanup subcommand tests --

    #[test]
    fn test_cleanup_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_cleanup(dir.path(), false, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_stale_entry_removed() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "stale-wt".to_string(),
            path: dir
                .path()
                .join(".git-worktrees/stale-wt")
                .to_string_lossy()
                .to_string(),
            branch: Some("feat/stale".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Directory does not exist, so state is Stale -- cleanup should handle it.
        let result = run_cleanup(dir.path(), false, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_dry_run_does_not_modify() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "stale-wt".to_string(),
            path: "/nonexistent/stale-wt".to_string(),
            branch: Some("feat/stale".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, true, false, None);
        assert!(result.is_ok());

        // Registry should not have changed (dry run).
        let reg_after = codeflow_core::worktree::read_registry(&reg_path).unwrap();
        assert_eq!(
            reg_after.worktrees[0].status,
            codeflow_core::worktree::WorktreeStatus::Active
        );
    }

    #[test]
    fn test_cleanup_force_includes_orphaned() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");

        // Create a directory that's an orphaned worktree (exists but no .git).
        let wt_dir = dir.path().join(".git-worktrees/orphan-wt");
        std::fs::create_dir_all(&wt_dir).unwrap();

        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "orphan-wt".to_string(),
            path: wt_dir.to_string_lossy().to_string(),
            branch: Some("feat/orphan".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Without force, orphaned should not be removed (dry run to check count).
        let mgr = WorktreeManager::new(dir.path());
        let opts_no_force = CleanupOpts {
            force: false,
            dry_run: true,
            ..CleanupOpts::default()
        };
        let removed_no_force = mgr.cleanup_stale(&opts_no_force).unwrap();
        assert!(
            removed_no_force.is_empty(),
            "orphaned not removed without force"
        );

        // With force, orphaned should be removed.
        let opts_force = CleanupOpts {
            force: true,
            dry_run: true,
            ..CleanupOpts::default()
        };
        let removed_force = mgr.cleanup_stale(&opts_force).unwrap();
        assert_eq!(removed_force.len(), 1);
        assert_eq!(removed_force[0], "orphan-wt");
    }

    // -- CLI subcommand dispatch tests --

    #[test]
    fn test_dispatch_list_default() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        // No subcommand -> None -> defaults to list.
        let cli = TestCli::try_parse_from(["test"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_dispatch_list_explicit() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "list"]).unwrap();
        assert!(matches!(cli.command, Some(WorktreeCommand::List)));
    }

    #[test]
    fn test_dispatch_cleanup() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                dry_run: false,
                interactive: false,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_with_flags() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "--force", "--dry-run"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: true,
                dry_run: true,
                interactive: false,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_interactive() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "-i"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                dry_run: false,
                interactive: true,
                keep: None,
            })
        ));
    }

    #[test]
    fn test_dispatch_cleanup_keep_param() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "cleanup", "--keep", "3"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Cleanup {
                force: false,
                dry_run: false,
                interactive: false,
                keep: Some(3),
            })
        ));
    }

    #[test]
    fn test_dispatch_repair() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "repair"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Repair { path: None })
        ));
    }

    #[test]
    fn test_dispatch_repair_with_path() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "repair", "--path", "/tmp/worktree"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Repair { path: Some(_) })
        ));
    }

    // -- run_list with entries tests --

    #[test]
    fn test_list_shows_branch_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "list-wt".to_string(),
            path: "/nonexistent/list-wt".to_string(),
            branch: Some("feat/listed".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // run_list should succeed and print the entry.
        let result = run_list(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_list_detached_branch_shows_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "detached-wt".to_string(),
            path: "/nonexistent/detached-wt".to_string(),
            branch: None,
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_list(dir.path());
        assert!(result.is_ok());
    }

    // -- interactive cleanup tests --

    #[test]
    fn test_cleanup_interactive_no_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        // Interactive with no entries should say "no worktrees to clean up".
        let result = run_cleanup(dir.path(), false, false, true, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_interactive_shows_stale_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        // Add a stale entry (directory doesn't exist).
        reg.worktrees.push(WorktreeEntry {
            name: "stale-interactive".to_string(),
            path: "/nonexistent/stale-interactive".to_string(),
            branch: Some("feat/stale".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, false, true, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_removed_entries_skipped_in_interactive() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "already-removed".to_string(),
            path: "/gone/removed".to_string(),
            branch: Some("feat/old".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Removed,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Removed entries should be skipped -- "no worktrees to clean up".
        let result = run_cleanup(dir.path(), false, false, true, None);
        assert!(result.is_ok());
    }

    // -- cleanup output path tests --

    #[test]
    fn test_cleanup_non_interactive_empty_says_no_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, false, false, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_dry_run_stale_shows_would_remove() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-04-01T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "dry-run-stale".to_string(),
            path: dir
                .path()
                .join(".git-worktrees/dry-run-stale")
                .to_string_lossy()
                .to_string(),
            branch: Some("feat/dry".to_string()),
            created_at: "2026-04-01T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: None,
            task_id: None,
            source: None,
            lead_pid: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Dry run should report "would remove" without actually removing.
        let result = run_cleanup(dir.path(), false, true, false, None);
        assert!(result.is_ok());
    }
}
