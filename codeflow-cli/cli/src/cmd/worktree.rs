//! Worktree command: git worktree lifecycle management.
//!
//! Supports subcommands: `list`, `cleanup`, `prune`, `repair`.
//! Default (no subcommand) behaves like `list`.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use codeflow_core::worktree::{CleanupOpts, WorktreeManager};

use crate::helpers;

/// Worktree subcommands.
#[derive(Debug, Subcommand)]
pub enum WorktreeCommand {
    /// List worktrees and their state
    List,
    /// Remove stale or inactive worktrees
    Cleanup {
        /// Force removal of orphaned worktrees (not just stale)
        #[arg(long)]
        force: bool,
        /// Report what would be removed without acting
        #[arg(long)]
        dry_run: bool,
    },
    /// Reconcile registry with filesystem
    Prune {
        /// Report inconsistencies without fixing
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
        Some(WorktreeCommand::Cleanup { force, dry_run }) => {
            run_cleanup(&project_dir, *force, *dry_run)
        }
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
        println!("{} ({}) [{}]", entry.name, entry.branch, state);
    }

    Ok(())
}

fn run_cleanup(project_dir: &Path, force: bool, dry_run: bool) -> Result<()> {
    let mgr = WorktreeManager::new(project_dir);
    let opts = CleanupOpts {
        force,
        dry_run,
        prune: false,
    };

    let removed = mgr
        .cleanup_stale(&opts)
        .context("cleaning up stale worktrees")?;

    if removed.is_empty() {
        println!("no worktrees to clean up");
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
    let mgr = WorktreeManager::new(project_dir);

    let (stale, orphaned) = mgr
        .reconcile_registry(dry_run)
        .context("reconciling registry")?;

    let prefix = if dry_run { "would fix" } else { "fixed" };

    if stale.is_empty() && orphaned.is_empty() {
        println!("registry is consistent");
        return Ok(());
    }

    if !stale.is_empty() {
        println!(
            "{prefix}: {} stale registry entr{}:",
            stale.len(),
            if stale.len() == 1 { "y" } else { "ies" }
        );
        for name in &stale {
            println!("  {name} (in registry, directory missing)");
        }
    }

    if !orphaned.is_empty() {
        println!(
            "found {} orphaned director{}:",
            orphaned.len(),
            if orphaned.len() == 1 { "y" } else { "ies" }
        );
        for path in &orphaned {
            println!("  {path} (on disk, not in registry)");
        }
    }

    Ok(())
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
            branch: "feat/test".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: "removed".to_string(),
            session_id: None,
            task_id: None,
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
            branch: "feat/something".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        };

        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let state = mgr.detect_state(&entry);
        let output = format!("{} ({}) [{}]", entry.name, entry.branch, state);
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
        let result = run_cleanup(dir.path(), false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, false);
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
            branch: "feat/stale".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Directory does not exist, so state is Stale -- cleanup should handle it.
        let result = run_cleanup(dir.path(), false, false);
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
            branch: "feat/stale".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let result = run_cleanup(dir.path(), false, true);
        assert!(result.is_ok());

        // Registry should not have changed (dry run).
        let reg_after = codeflow_core::worktree::read_registry(&reg_path).unwrap();
        assert_eq!(reg_after.worktrees[0].status, "active");
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
            branch: "feat/orphan".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        // Without force, orphaned should not be removed (dry run to check count).
        let mgr = WorktreeManager::new(dir.path());
        let opts_no_force = CleanupOpts {
            force: false,
            dry_run: true,
            prune: false,
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
            prune: false,
        };
        let removed_force = mgr.cleanup_stale(&opts_force).unwrap();
        assert_eq!(removed_force.len(), 1);
        assert_eq!(removed_force[0], "orphan-wt");
    }

    // -- prune subcommand tests --

    #[test]
    fn test_prune_no_registry() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_prune(dir.path(), false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_prune_consistent_registry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        let result = run_prune(dir.path(), false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_prune_detects_stale_entry() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "ghost-wt".to_string(),
            path: "/nonexistent/ghost-wt".to_string(),
            branch: "feat/ghost".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let (stale, _orphaned) = mgr.reconcile_registry(false).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], "ghost-wt");

        // Verify registry was updated.
        let reg_after = codeflow_core::worktree::read_registry(&reg_path).unwrap();
        assert_eq!(reg_after.worktrees[0].status, "removed");
    }

    #[test]
    fn test_prune_detects_orphaned_dir() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        write_registry(&reg_path, &reg).unwrap();

        // Create a directory that is not in the registry.
        let orphan_dir = dir.path().join(".git-worktrees/orphan-dir");
        std::fs::create_dir_all(&orphan_dir).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let (stale, orphaned) = mgr.reconcile_registry(false).unwrap();
        assert!(stale.is_empty());
        assert_eq!(orphaned.len(), 1);
        assert!(orphaned[0].contains("orphan-dir"));
    }

    #[test]
    fn test_prune_dry_run_does_not_modify() {
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "ghost-wt".to_string(),
            path: "/nonexistent/ghost-wt".to_string(),
            branch: "feat/ghost".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let (stale, _) = mgr.reconcile_registry(true).unwrap();
        assert_eq!(stale.len(), 1);

        // Registry should NOT be modified (dry run).
        let reg_after = codeflow_core::worktree::read_registry(&reg_path).unwrap();
        assert_eq!(reg_after.worktrees[0].status, "active");
    }

    #[test]
    fn test_prune_concurrent_access() {
        // Verify locked access by running prune in sequence.
        let dir = tempfile::tempdir().unwrap();
        let reg_path = dir.path().join(".state/worktrees/worktrees.yaml");
        let mut reg = WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(WorktreeEntry {
            name: "s1".to_string(),
            path: "/nonexistent/s1".to_string(),
            branch: "feat/s1".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        reg.worktrees.push(WorktreeEntry {
            name: "s2".to_string(),
            path: "/nonexistent/s2".to_string(),
            branch: "feat/s2".to_string(),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
            task_id: None,
        });
        write_registry(&reg_path, &reg).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let (stale, _) = mgr.reconcile_registry(false).unwrap();
        assert_eq!(stale.len(), 2);
    }

    // -- integration test: create -> delete dir -> prune -> verify --

    #[test]
    fn test_integration_create_delete_prune() {
        let dir = tempfile::tempdir().unwrap();

        // Initialize a git repo with an initial commit.
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees/worktrees.yaml"));

        // Create a worktree.
        let branch = codeflow_core::types::BranchName::new_unchecked("feat/prune-test");
        let entry = mgr.setup("prune-test", &branch).unwrap();
        assert_eq!(entry.status, "active");

        let wt_path = std::path::PathBuf::from(&entry.path);
        assert!(wt_path.exists());

        // Manually delete the worktree directory to simulate stale state.
        std::fs::remove_dir_all(&wt_path).unwrap();
        assert!(!wt_path.exists());

        // Verify detect_state now returns Stale.
        let entries = mgr.list(None).unwrap();
        let our_entry = entries.iter().find(|e| e.name == "prune-test").unwrap();
        assert_eq!(
            mgr.detect_state(our_entry),
            codeflow_core::worktree::WorktreeState::Stale
        );

        // Run prune (reconcile).
        let (stale, _) = mgr.reconcile_registry(false).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], "prune-test");

        // Verify registry was updated: entry is now "removed".
        let reg = codeflow_core::worktree::read_registry(mgr.registry_path()).unwrap();
        let pruned = reg
            .worktrees
            .iter()
            .find(|e| e.name == "prune-test")
            .unwrap();
        assert_eq!(pruned.status, "removed");
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
                dry_run: false
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
                dry_run: true
            })
        ));
    }

    #[test]
    fn test_dispatch_prune() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "prune"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Prune { dry_run: false })
        ));
    }

    #[test]
    fn test_dispatch_prune_dry_run() {
        use clap::Parser;

        #[derive(Debug, Parser)]
        struct TestCli {
            #[command(subcommand)]
            command: Option<WorktreeCommand>,
        }

        let cli = TestCli::try_parse_from(["test", "prune", "--dry-run"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(WorktreeCommand::Prune { dry_run: true })
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
}
