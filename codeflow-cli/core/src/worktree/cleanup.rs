//! Worktree removal and deregistration.
//!
//! Supports force removal, dry-run mode, and git worktree pruning.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::error::WorktreeError;

use super::WorktreeManager;
use super::registry;

/// Options for worktree cleanup behavior.
#[derive(Debug, Clone, Default)]
pub struct CleanupOpts {
    /// Force removal even if the worktree has uncommitted changes
    /// or a `PathFlow` session is active.
    pub force: bool,
    /// Print what would be done without actually doing it.
    pub dry_run: bool,
    /// Run `git worktree prune` to clean stale references.
    /// When true, the `name` parameter is ignored.
    pub prune: bool,
}

/// Remove a worktree and deregister it from the registry.
///
/// This is the internal implementation called by `WorktreeManager::cleanup`.
pub(crate) fn cleanup_worktree(
    mgr: &WorktreeManager,
    name: &str,
    opts: &CleanupOpts,
) -> Result<(), WorktreeError> {
    // Prune mode: use git2 to prune stale worktree references.
    if opts.prune {
        return prune_worktrees(mgr, opts.dry_run);
    }

    if name.is_empty() {
        return Err(WorktreeError::InvalidName(
            "name cannot be empty".to_string(),
        ));
    }

    // PathFlow guard: block cleanup if active (unless force).
    if mgr.is_pathflow_active() && !opts.force {
        return Err(WorktreeError::PathFlowActive);
    }

    let wt_path = mgr.base_dir().join(name);

    if opts.dry_run {
        // Dry-run: just report what would happen, don't actually do anything.
        return Ok(());
    }

    // Rescue uncommitted/unpushed work before deletion (force mode only).
    if opts.force && wt_path.exists() {
        rescue_uncommitted_work(&wt_path, name)?;
    }

    // Attempt removal via git2.
    let removed_via_git = remove_via_git2(mgr, name, opts.force);

    // If git2 removal didn't fully clean up, remove the directory manually.
    if wt_path.exists() {
        if opts.force {
            fs::remove_dir_all(&wt_path)?;
        } else if !removed_via_git {
            return Err(WorktreeError::NotFound(name.to_string()));
        }
    }

    // Deregister from the YAML registry.
    let wt_path_str = wt_path.to_string_lossy();
    registry::deregister_worktree(mgr.registry_path(), &wt_path_str)?;

    Ok(())
}

/// Rescue uncommitted and unpushed work from a worktree before deletion.
///
/// 1. If the worktree has dirty files: stages all and creates a WIP commit.
/// 2. If the worktree has unpushed commits: attempts to push them.
/// 3. If push fails (no network): returns `WorktreeError::UnpushedWork`.
/// 4. If clean or push succeeds: returns `Ok(())` (safe to delete).
pub fn rescue_uncommitted_work(wt_path: &Path, session_hint: &str) -> Result<(), WorktreeError> {
    // Check if directory is a git repo at all.
    if !wt_path.join(".git").exists() {
        return Ok(());
    }

    // Step 1: Check for dirty files.
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(wt_path)
        .output()
        .map_err(|e| WorktreeError::Cleanup(format!("git status failed: {e}")))?;

    let dirty = !status_output.stdout.is_empty();

    if dirty {
        // Stage and commit all dirty files.
        let add_status = Command::new("git")
            .args(["add", "-A"])
            .current_dir(wt_path)
            .status()
            .map_err(|e| WorktreeError::Cleanup(format!("git add -A failed: {e}")))?;

        if !add_status.success() {
            crate::diagnostics::warn(
                "worktree",
                &format!("git add -A failed in {session_hint}, proceeding"),
            );
        }

        let commit_msg = format!("wip: auto-save from crashed session {session_hint}");
        let commit_status = Command::new("git")
            .args(["commit", "-m", &commit_msg, "--no-verify"])
            .current_dir(wt_path)
            .status()
            .map_err(|e| WorktreeError::Cleanup(format!("git commit failed: {e}")))?;

        if !commit_status.success() {
            crate::diagnostics::warn(
                "worktree",
                &format!("wip commit failed in {session_hint}, proceeding"),
            );
        }
    }

    // Step 2: Check for unpushed commits.
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(wt_path)
        .output()
        .map_err(|e| WorktreeError::Cleanup(format!("git rev-parse failed: {e}")))?;

    let branch = String::from_utf8_lossy(&branch_output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        // Detached HEAD — no branch to push. Work is only local.
        if dirty {
            return Err(WorktreeError::UnpushedWork(format!(
                "worktree '{session_hint}' has uncommitted work on detached HEAD"
            )));
        }
        return Ok(());
    }

    // Check if there are unpushed commits.
    let log_output = Command::new("git")
        .args(["log", "--oneline", &format!("origin/{branch}..HEAD")])
        .current_dir(wt_path)
        .output();

    let has_unpushed = match log_output {
        Ok(output) => !output.stdout.is_empty(),
        Err(_) => dirty, // If we can't check, assume unpushed if we just committed.
    };

    if !has_unpushed {
        return Ok(()); // Nothing to push — safe to delete.
    }

    // Step 3: Attempt to push.
    let push_status = Command::new("git")
        .args(["push", "origin", &branch])
        .current_dir(wt_path)
        .status();

    match push_status {
        Ok(status) if status.success() => Ok(()), // Push succeeded — safe to delete.
        _ => Err(WorktreeError::UnpushedWork(format!(
            "worktree '{session_hint}' has unpushed commits on branch '{branch}'"
        ))),
    }
}

/// Attempt to remove a worktree using git2.
///
/// Returns `true` if the removal succeeded (or the worktree was not found in git).
fn remove_via_git2(mgr: &WorktreeManager, name: &str, force: bool) -> bool {
    let Ok(repo) = git2::Repository::open(mgr.project_dir()) else {
        return false;
    };

    let Ok(wt) = repo.find_worktree(name) else {
        return false;
    };

    // Validate the worktree. If it's not valid, prune it.
    if wt.validate().is_err() {
        let _ = wt.prune(Some(
            git2::WorktreePruneOptions::new()
                .valid(false)
                .working_tree(true),
        ));
        return true;
    }

    // Try to prune the valid worktree.
    let mut prune_opts = git2::WorktreePruneOptions::new();
    prune_opts.valid(true).working_tree(true);
    if force {
        prune_opts.locked(true);
    }

    wt.prune(Some(&mut prune_opts)).is_ok()
}

/// Prune stale worktree references using git2.
fn prune_worktrees(mgr: &WorktreeManager, dry_run: bool) -> Result<(), WorktreeError> {
    let repo = git2::Repository::open(mgr.project_dir())?;

    let worktree_names = repo.worktrees()?;
    for name in worktree_names.iter().flatten() {
        if let Ok(wt) = repo.find_worktree(name) {
            if wt.validate().is_err() && !dry_run {
                let _ = wt.prune(Some(
                    git2::WorktreePruneOptions::new()
                        .valid(false)
                        .working_tree(true),
                ));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleanup_opts_default() {
        let opts = CleanupOpts::default();
        assert!(!opts.force);
        assert!(!opts.dry_run);
        assert!(!opts.prune);
    }

    #[test]
    fn test_cleanup_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts::default();

        let result = cleanup_worktree(&mgr, "", &opts);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::InvalidName(_)));
    }

    #[test]
    fn test_cleanup_pathflow_active_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path()).with_pathflow_guard(|| true);
        let opts = CleanupOpts::default();

        let result = cleanup_worktree(&mgr, "test-wt", &opts);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::PathFlowActive));
    }

    #[test]
    fn test_cleanup_pathflow_active_force_allows() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path()).with_pathflow_guard(|| true);
        let opts = CleanupOpts {
            force: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "nonexistent", &opts);
        if let Err(e) = &result {
            assert!(
                !matches!(e, WorktreeError::PathFlowActive),
                "force should bypass PathFlow guard"
            );
        }
    }

    #[test]
    fn test_cleanup_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        // Create a dummy directory to simulate a worktree.
        fs::create_dir_all(mgr.base_dir().join("dry-wt")).unwrap();

        let opts = CleanupOpts {
            dry_run: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "dry-wt", &opts);
        assert!(result.is_ok());

        // Directory should still exist (dry run).
        assert!(mgr.base_dir().join("dry-wt").exists());
    }

    #[test]
    fn test_cleanup_nonexistent_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts::default();

        let result = cleanup_worktree(&mgr, "nonexistent", &opts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cleanup_force_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        let wt_dir = mgr.base_dir().join("force-wt");
        fs::create_dir_all(&wt_dir).unwrap();
        fs::write(wt_dir.join("test.txt"), "data").unwrap();

        let opts = CleanupOpts {
            force: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "force-wt", &opts);
        assert!(result.is_ok());
        assert!(!wt_dir.exists(), "directory should be removed");
    }

    #[test]
    fn test_rescue_uncommitted_work_clean_worktree() {
        // A clean worktree should return Ok(()).
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let result = rescue_uncommitted_work(dir.path(), "test-session");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rescue_uncommitted_work_non_git_dir() {
        // A non-git directory should return Ok(()) (nothing to rescue).
        let dir = tempfile::tempdir().unwrap();
        let result = rescue_uncommitted_work(dir.path(), "test-session");
        assert!(result.is_ok());
    }

    #[test]
    fn test_rescue_uncommitted_work_dirty_detached_head() {
        // A dirty worktree on detached HEAD should return UnpushedWork error.
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // Detach HEAD
        repo.set_head_detached(oid).unwrap();

        // Create a dirty file
        fs::write(dir.path().join("dirty.txt"), "uncommitted data").unwrap();

        let result = rescue_uncommitted_work(dir.path(), "test-session");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorktreeError::UnpushedWork(_)));
    }

    #[test]
    fn test_cleanup_full_lifecycle() {
        let dir = tempfile::tempdir().unwrap();

        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let mgr = WorktreeManager::new(dir.path())
            .with_registry_path(dir.path().join(".state/worktrees.yaml"));

        let branch = crate::types::BranchName::new_unchecked("feat/cleanup-test");
        mgr.setup("cleanup-test", &branch).unwrap();
        assert!(mgr.base_dir().join("cleanup-test").exists());

        let opts = CleanupOpts {
            force: true,
            ..CleanupOpts::default()
        };
        mgr.cleanup("cleanup-test", &opts).unwrap();

        assert!(!mgr.base_dir().join("cleanup-test").exists());

        let entries = mgr.list(Some("removed")).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "cleanup-test");
    }

    #[test]
    fn test_prune_dry_run_on_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts {
            prune: true,
            dry_run: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "", &opts);
        assert!(result.is_ok());
    }

    #[test]
    fn test_prune_on_clean_repo() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let mgr = WorktreeManager::new(dir.path());
        let opts = CleanupOpts {
            prune: true,
            ..CleanupOpts::default()
        };

        let result = cleanup_worktree(&mgr, "", &opts);
        assert!(result.is_ok());
    }
}
