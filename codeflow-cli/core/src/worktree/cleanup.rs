//! Worktree removal and deregistration.
//!
//! Supports force removal, dry-run mode, git worktree pruning,
//! branch safety checks, and liveness verification.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::error::WorktreeError;

use super::WorktreeManager;
use super::registry;

/// Options for worktree cleanup behavior.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
pub struct CleanupOpts {
    /// Force removal even if the worktree has uncommitted changes
    /// or a `PathFlow` session is active.
    pub force: bool,
    /// Print what would be done without actually doing it.
    pub dry_run: bool,
    /// Run `git worktree prune` to clean stale references.
    /// When true, the `name` parameter is ignored.
    pub prune: bool,
    /// Show interactive prompts with branch/PR/commit status before removal.
    pub interactive: bool,
    /// Maximum number of "removed" entries to keep in registry during purge.
    /// Defaults to 5 if `None`.
    pub keep: Option<usize>,
    /// When set, force override applies only to entries whose names are in
    /// this list (exact match). When `None` and `force` is true, force
    /// applies to all eligible entries.
    pub force_names: Option<Vec<String>>,
}

/// Risk level for removing a worktree based on its branch state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BranchRisk {
    /// Branch merged into main or PR merged — safe to remove.
    None,
    /// Branch pushed with an open PR — safe to remove with note.
    Low,
    /// Branch pushed but no PR exists — warn but proceed.
    Medium,
    /// Branch NOT pushed but no uncommitted changes — block unless forced.
    High,
    /// Branch NOT pushed AND has uncommitted changes — refuse unless forced.
    Critical,
}

impl BranchRisk {
    /// Human-readable label for this risk level.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl std::fmt::Display for BranchRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of checking branch safety before worktree removal.
#[derive(Debug, Clone)]
pub struct BranchSafetyResult {
    /// Assessed risk level.
    pub risk: BranchRisk,
    /// Branch name (empty for detached HEAD).
    pub branch: String,
    /// Whether the branch has been pushed to remote.
    pub pushed: bool,
    /// PR number if one exists for this branch (0 if none or unknown).
    pub pr_number: u64,
    /// PR state (e.g., "open", "merged", "closed", or empty).
    pub pr_state: String,
    /// Count of unpushed commits.
    pub unpushed_commits: usize,
    /// Whether the worktree has uncommitted (dirty) files.
    pub has_uncommitted: bool,
    /// Human-readable summary message.
    pub message: String,
}

/// Check branch safety before removing a worktree.
///
/// Assesses the risk of data loss from removing the worktree by examining:
/// - Whether the branch has been pushed to remote
/// - Whether there are unpushed commits
/// - Whether there are uncommitted changes
/// - Whether a PR exists for the branch
///
/// This does NOT check PR state via `gh` to avoid network requirements.
/// The caller can enhance with PR information if `gh` is available.
#[must_use]
pub fn check_branch_safety(wt_path: &Path) -> BranchSafetyResult {
    // If not a git directory, it's safe to remove (orphaned).
    if !wt_path.join(".git").exists() {
        return BranchSafetyResult {
            risk: BranchRisk::None,
            branch: String::new(),
            pushed: false,
            pr_number: 0,
            pr_state: String::new(),
            unpushed_commits: 0,
            has_uncommitted: false,
            message: "not a git worktree — safe to remove".into(),
        };
    }

    // Get current branch name.
    let branch = get_branch_name(wt_path);

    // Detached HEAD or main/master — safe to remove.
    if branch.is_empty() || branch == "HEAD" || branch == "main" || branch == "master" {
        return BranchSafetyResult {
            risk: BranchRisk::None,
            branch,
            pushed: false,
            pr_number: 0,
            pr_state: String::new(),
            unpushed_commits: 0,
            has_uncommitted: false,
            message: "detached HEAD or default branch — safe to remove".into(),
        };
    }

    // Check for dirty/uncommitted files.
    let has_uncommitted = has_dirty_files(wt_path);

    // Check if branch is pushed to remote.
    let (pushed, unpushed_commits) = check_push_status(wt_path, &branch);

    // Determine risk level.
    let (risk, message) = if !pushed && has_uncommitted {
        (
            BranchRisk::Critical,
            format!("branch '{branch}' has unpushed commits AND uncommitted changes — BLOCKED"),
        )
    } else if !pushed {
        (
            BranchRisk::High,
            format!(
                "branch '{branch}' has {unpushed_commits} unpushed commit(s) — BLOCKED (push first or use --force)"
            ),
        )
    } else {
        // Branch is pushed — check if it's merged into main.
        if is_branch_merged(wt_path, &branch) {
            (
                BranchRisk::None,
                format!("branch '{branch}' is merged into main — safe to remove"),
            )
        } else {
            (
                BranchRisk::Medium,
                format!("branch '{branch}' is pushed but no PR found — removing with warning"),
            )
        }
    };

    BranchSafetyResult {
        risk,
        branch,
        pushed,
        pr_number: 0,
        pr_state: String::new(),
        unpushed_commits,
        has_uncommitted,
        message,
    }
}

/// Remove a worktree and deregister it from the registry.
///
/// This is the internal implementation called by `WorktreeManager::cleanup`.
///
/// Directory removal is unconditional when the directory exists. Deregistration
/// happens only after the directory is confirmed gone. If removal fails, the
/// entry stays in the registry for retry on the next run.
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

    let wt_path = mgr.base_dir().join(name);

    if opts.dry_run {
        // Dry-run: just report what would happen, don't actually do anything.
        return Ok(());
    }

    // Rescue uncommitted/unpushed work before deletion (force mode only).
    if opts.force && wt_path.exists() {
        rescue_uncommitted_work(&wt_path, name)?;
    }

    // Best-effort git metadata cleanup.
    let _ = remove_via_git2(mgr, name, opts.force);

    // Delete directory if it still exists (unconditional).
    // Catch NotFound to handle concurrent deletion (TOCTOU).
    if wt_path.exists() {
        match fs::remove_dir_all(&wt_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }

    // Deregister AFTER directory confirmed gone (locked to avoid TOCTOU).
    let wt_path_str = wt_path.to_string_lossy();
    registry::locked_deregister_worktree(mgr.registry_path(), &wt_path_str)?;

    // Defense-in-depth: also deregister by name for empty-path entries.
    registry::locked_deregister_by_name(mgr.registry_path(), name)?;

    Ok(())
}

/// Remove an orphaned worktree directory that has no registry entry.
///
/// Performs best-effort git metadata cleanup via `git2`, then removes
/// the directory. Catches `NotFound` for concurrent deletion safety.
///
/// # Errors
///
/// Returns `WorktreeError` on I/O or git metadata removal failures.
pub fn cleanup_orphan(
    project_dir: &Path,
    orphan_path: &Path,
    dry_run: bool,
) -> Result<(), WorktreeError> {
    if dry_run {
        return Ok(());
    }

    let mgr = WorktreeManager::new(project_dir);
    let name = orphan_path
        .file_name()
        .ok_or_else(|| WorktreeError::InvalidName("no filename".into()))?
        .to_string_lossy();

    // Best-effort git metadata cleanup.
    let _ = remove_via_git2(&mgr, &name, true);

    // Remove directory (catch NotFound for concurrent deletion).
    if orphan_path.exists() {
        match fs::remove_dir_all(orphan_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }

    Ok(())
}

/// Rescue uncommitted and unpushed work from a worktree before deletion.
///
/// 1. If the session's PR has already been pushed (pr_pushed=true in
///    pathflow-session-status.json), skip the rescue entirely — any dirty
///    files at this point are stray writes that would create noise commits
///    after the PR is open. The cleanup caller will proceed to delete.
/// 2. If the worktree has dirty files: stages all and creates a WIP commit.
/// 3. If the worktree has unpushed commits: attempts to push them.
/// 4. If push fails (no network): returns `WorktreeError::UnpushedWork`.
/// 5. If clean or push succeeds: returns `Ok(())` (safe to delete).
pub fn rescue_uncommitted_work(wt_path: &Path, session_hint: &str) -> Result<(), WorktreeError> {
    // Check if directory is a git repo at all.
    if !wt_path.join(".git").exists() {
        return Ok(());
    }

    // Step 0: If the PR has already been pushed for this session, skip rescue.
    // Post-PR writes are stray commits (worktree auto-save creates noise on
    // a branch that's already under review). Crashed sessions without the
    // pr_pushed flag still get the full rescue below.
    if session_pushed_pr(wt_path, session_hint) {
        crate::diagnostics::warn(
            "worktree",
            &format!("{session_hint}: skipping rescue — PR already pushed"),
        );
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

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();
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

/// Return true if the session's `pr_pushed` flag is set in
/// `pathflow-session-status.json`.
///
/// The flag is set atomically by the `SentinelWrite` PostToolUse hook after
/// a successful `git push` or `gh pr create`. The check is read-only and
/// tolerant: missing file, bad JSON, or missing field all return false —
/// callers must fall back to the full rescue path in those cases.
///
/// `session_hint` is the worktree directory name (e.g., `worktree-ses-abc123`);
/// the session ID is recovered by stripping the `worktree-` prefix.
///
/// # Path safety
///
/// The extracted session ID is validated against `[A-Za-z0-9_-]+` before
/// being joined into the filesystem path. Any value containing `..`,
/// path separators, null bytes, or other non-alphanumeric/`-_` characters
/// is rejected (returns `false`). Today the source is trusted
/// (`WorktreeManager` output), but the sanitization is cheap and forecloses
/// path-traversal attack surface if the helper is ever reused with
/// untrusted input.
fn session_pushed_pr(wt_path: &Path, session_hint: &str) -> bool {
    let session_id = session_hint
        .strip_prefix("worktree-")
        .unwrap_or(session_hint);
    if session_id.is_empty() {
        return false;
    }
    // Sanitize: only accept [A-Za-z0-9_-]. Reject anything else — this
    // includes "..", "/", "\\", null bytes, and any character that could
    // break path semantics.
    if !session_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return false;
    }

    let status_path = wt_path
        .join(".state/session")
        .join(session_id)
        .join("pathflow/pathflow-session-status.json");

    let Ok(contents) = std::fs::read_to_string(&status_path) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&contents)
        .ok()
        .and_then(|v| v.get("pr_pushed")?.as_bool())
        .unwrap_or(false)
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

/// Get the current branch name from a worktree, or empty string for detached HEAD.
fn get_branch_name(wt_path: &Path) -> String {
    Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(wt_path)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Check if a worktree has dirty (uncommitted) files.
pub(crate) fn has_dirty_files(wt_path: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(wt_path)
        .output()
        .ok()
        .is_some_and(|o| !o.stdout.is_empty())
}

/// Check if a branch has been pushed to remote and count unpushed commits.
///
/// Returns `(is_pushed, unpushed_count)`.
fn check_push_status(wt_path: &Path, branch: &str) -> (bool, usize) {
    // Check if remote tracking branch exists.
    let remote_ref = format!("origin/{branch}");
    let remote_exists = Command::new("git")
        .args(["rev-parse", "--verify", &remote_ref])
        .current_dir(wt_path)
        .output()
        .ok()
        .is_some_and(|o| o.status.success());

    if !remote_exists {
        // No remote tracking branch — check if any commits exist at all.
        let commit_count = Command::new("git")
            .args(["rev-list", "--count", "HEAD"])
            .current_dir(wt_path)
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<usize>()
                    .ok()
            })
            .unwrap_or(0);

        // No remote tracking: report at least 1 unpushed if commits exist.
        return (false, usize::from(commit_count > 0));
    }

    // Count commits ahead of remote.
    let log_range = format!("{remote_ref}..HEAD");
    let unpushed = Command::new("git")
        .args(["rev-list", "--count", &log_range])
        .current_dir(wt_path)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<usize>()
                .ok()
        })
        .unwrap_or(0);

    (true, unpushed)
}

/// Check if a branch has been merged into main/master.
fn is_branch_merged(wt_path: &Path, branch: &str) -> bool {
    // Check if origin/main or origin/master contains the branch tip.
    for main_ref in &["origin/main", "origin/master", "main", "master"] {
        let output = Command::new("git")
            .args(["branch", "--merged", main_ref])
            .current_dir(wt_path)
            .output();

        if let Ok(o) = output {
            if o.status.success() {
                let branches = String::from_utf8_lossy(&o.stdout);
                for line in branches.lines() {
                    let trimmed = line.trim().trim_start_matches("* ");
                    if trimmed == branch {
                        return true;
                    }
                }
            }
        }
    }
    false
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
        assert!(!opts.interactive);
        assert!(opts.keep.is_none());
    }

    #[test]
    fn test_branch_risk_ordering() {
        assert!(BranchRisk::None < BranchRisk::Low);
        assert!(BranchRisk::Low < BranchRisk::Medium);
        assert!(BranchRisk::Medium < BranchRisk::High);
        assert!(BranchRisk::High < BranchRisk::Critical);
    }

    #[test]
    fn test_branch_risk_display() {
        assert_eq!(BranchRisk::None.as_str(), "none");
        assert_eq!(BranchRisk::Low.as_str(), "low");
        assert_eq!(BranchRisk::Medium.as_str(), "medium");
        assert_eq!(BranchRisk::High.as_str(), "high");
        assert_eq!(BranchRisk::Critical.as_str(), "critical");
        assert_eq!(BranchRisk::Critical.to_string(), "critical");
    }

    #[test]
    fn test_check_branch_safety_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::None);
        assert!(result.message.contains("not a git worktree"));
    }

    #[test]
    fn test_check_branch_safety_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let oid = repo
            .commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        repo.set_head_detached(oid).unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::None);
        assert!(result.message.contains("detached HEAD"));
    }

    #[test]
    fn test_check_branch_safety_main_branch() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = git2::Repository::init(dir.path()).unwrap();
        // Set up git with initial commit on main
        Command::new("git")
            .args(["checkout", "-b", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::None);
    }

    #[test]
    fn test_check_branch_safety_unpushed_branch() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat/unpushed-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "feature work"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::High);
        assert!(!result.pushed);
        assert!(result.message.contains("BLOCKED"));
    }

    #[test]
    fn test_check_branch_safety_unpushed_with_dirty_files() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat/dirty-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Create a dirty file
        fs::write(dir.path().join("dirty.txt"), "uncommitted data").unwrap();

        let result = check_branch_safety(dir.path());
        assert_eq!(result.risk, BranchRisk::Critical);
        assert!(result.has_uncommitted);
        assert!(result.message.contains("BLOCKED"));
    }

    #[test]
    fn test_check_branch_safety_result_fields() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_branch_safety(dir.path());
        // Verify all fields are populated
        assert_eq!(result.pr_number, 0);
        assert!(result.pr_state.is_empty());
        assert!(!result.message.is_empty());
    }

    #[test]
    fn test_has_dirty_files_clean() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(!has_dirty_files(dir.path()));
    }

    #[test]
    fn test_has_dirty_files_dirty() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        fs::write(dir.path().join("new.txt"), "data").unwrap();
        assert!(has_dirty_files(dir.path()));
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
        assert!(matches!(
            result.unwrap_err(),
            WorktreeError::UnpushedWork(_)
        ));
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

        // Entry should be deleted from registry (not tombstoned as "removed").
        let entries = mgr.list(None).unwrap();
        assert!(
            !entries.iter().any(|e| e.name == "cleanup-test"),
            "entry should be deleted from registry after cleanup"
        );
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

    #[test]
    fn test_check_branch_safety_merged_branch() {
        // Set up a git repo with main, create a feature branch, merge it,
        // then check that check_branch_safety reports BranchRisk::None.
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Create and commit on feature branch.
        Command::new("git")
            .args(["checkout", "-b", "feat/merged-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "feature work"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Merge into main.
        Command::new("git")
            .args(["checkout", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["merge", "feat/merged-test", "--no-ff", "-m", "merge feat"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        // Go back to feature branch to check safety.
        Command::new("git")
            .args(["checkout", "feat/merged-test"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        assert!(is_branch_merged(dir.path(), "feat/merged-test"));
    }

    #[test]
    fn test_is_branch_merged_not_merged() {
        let dir = tempfile::tempdir().unwrap();
        Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["checkout", "-b", "feat/not-merged"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", "diverged"])
            .current_dir(dir.path())
            .output()
            .unwrap();

        assert!(!is_branch_merged(dir.path(), "feat/not-merged"));
    }

    // -- TOCTOU and cleanup_orphan tests --

    #[test]
    fn test_cleanup_worktree_concurrent_delete() {
        // If the directory disappears between exists() and remove_dir_all(),
        // cleanup_worktree should not error (NotFound caught).
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        // Create .git-worktrees base dir but NOT the worktree subdir.
        fs::create_dir_all(mgr.base_dir()).unwrap();

        // Create a registry so deregister has something to work with.
        let registry_dir = dir.path().join(".state").join("worktrees");
        fs::create_dir_all(&registry_dir).unwrap();

        let opts = CleanupOpts {
            force: true,
            ..Default::default()
        };

        // The worktree dir doesn't exist — should succeed without error.
        let result = cleanup_worktree(&mgr, "nonexistent-wt", &opts);
        assert!(
            result.is_ok(),
            "cleanup should succeed even if dir doesn't exist: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_cleanup_orphan_removes_directory() {
        let dir = tempfile::tempdir().unwrap();
        let orphan_dir = dir.path().join(".git-worktrees").join("worktree-orphan");
        fs::create_dir_all(&orphan_dir).unwrap();
        fs::write(orphan_dir.join("marker.txt"), "test").unwrap();

        let result = cleanup_orphan(dir.path(), &orphan_dir, false);
        assert!(
            result.is_ok(),
            "cleanup_orphan should succeed: {:?}",
            result.err()
        );
        assert!(!orphan_dir.exists(), "orphan directory should be removed");
    }

    #[test]
    fn test_cleanup_orphan_dry_run_preserves() {
        let dir = tempfile::tempdir().unwrap();
        let orphan_dir = dir.path().join(".git-worktrees").join("worktree-orphan");
        fs::create_dir_all(&orphan_dir).unwrap();

        let result = cleanup_orphan(dir.path(), &orphan_dir, true);
        assert!(result.is_ok());
        assert!(
            orphan_dir.exists(),
            "orphan directory should be preserved in dry-run"
        );
    }

    #[test]
    fn test_cleanup_orphan_nonexistent_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let orphan_dir = dir.path().join(".git-worktrees").join("worktree-gone");

        let result = cleanup_orphan(dir.path(), &orphan_dir, false);
        assert!(
            result.is_ok(),
            "cleanup_orphan should succeed for nonexistent path: {:?}",
            result.err()
        );
    }

    // -- session_pushed_pr tests --

    fn write_session_status(
        wt_path: &Path,
        session_id: &str,
        contents: &str,
    ) -> std::path::PathBuf {
        let dir = wt_path
            .join(".state/session")
            .join(session_id)
            .join("pathflow");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pathflow-session-status.json");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_session_pushed_pr_true() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(
            dir.path(),
            "ses-abc",
            r#"{"pr_pushed": true, "status": "pf-in-progress"}"#,
        );
        assert!(session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_false() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", r#"{"pr_pushed": false}"#);
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_missing_field() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", r#"{"status": "pf-in-progress"}"#);
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-missing"));
    }

    #[test]
    fn test_session_pushed_pr_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", "not valid json {");
        assert!(!session_pushed_pr(dir.path(), "worktree-ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_session_hint_without_prefix() {
        // session_hint may be passed as "ses-abc" directly (no "worktree-" prefix).
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc", r#"{"pr_pushed": true}"#);
        assert!(session_pushed_pr(dir.path(), "ses-abc"));
    }

    #[test]
    fn test_session_pushed_pr_empty_hint() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!session_pushed_pr(dir.path(), ""));
        // Just a "worktree-" prefix with nothing after.
        assert!(!session_pushed_pr(dir.path(), "worktree-"));
    }

    #[test]
    fn test_session_pushed_pr_rejects_path_traversal() {
        // Session IDs containing ".." or path separators must be rejected
        // even if the attacker-controlled path would happen to land on a
        // real status file with pr_pushed=true. Defense-in-depth (REV F6).
        let dir = tempfile::tempdir().unwrap();
        // Plant a status file at a location the traversal attempt might reach.
        write_session_status(dir.path(), "ses-real", r#"{"pr_pushed": true}"#);

        // Attempts that must all be rejected by the sanitizer:
        assert!(!session_pushed_pr(
            dir.path(),
            "worktree-../../../etc/passwd"
        ));
        assert!(!session_pushed_pr(
            dir.path(),
            "worktree-ses-real/../ses-real"
        ));
        assert!(!session_pushed_pr(dir.path(), "worktree-..%2Fses-real"));
        assert!(!session_pushed_pr(dir.path(), "worktree-ses real")); // space
        assert!(!session_pushed_pr(dir.path(), "worktree-ses\0real")); // null byte
        assert!(!session_pushed_pr(dir.path(), "worktree-a/b/c"));
        assert!(!session_pushed_pr(dir.path(), "worktree-a\\b\\c"));
    }

    #[test]
    fn test_session_pushed_pr_accepts_valid_ids() {
        // Happy-path IDs that match [A-Za-z0-9_-]+ must still work.
        let dir = tempfile::tempdir().unwrap();
        write_session_status(dir.path(), "ses-abc-123_XYZ", r#"{"pr_pushed": true}"#);
        assert!(session_pushed_pr(dir.path(), "worktree-ses-abc-123_XYZ"));
    }

    #[test]
    fn test_rescue_skips_when_pr_pushed() {
        // Setup: worktree with dirty files + pr_pushed=true status.
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().join("worktree-ses-pushed");
        fs::create_dir_all(&wt_path).unwrap();

        // Init a real git repo (rescue_uncommitted_work short-circuits if .git missing).
        let init = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&wt_path)
            .status();
        assert!(init.is_ok() && init.unwrap().success());

        // Create a dirty file.
        fs::write(wt_path.join("dirty.txt"), "uncommitted").unwrap();

        // Write pr_pushed=true status.
        write_session_status(
            &wt_path,
            "ses-pushed",
            r#"{"pr_pushed": true, "status": "pf-in-progress"}"#,
        );

        // Should return Ok immediately without staging/committing.
        let result = rescue_uncommitted_work(&wt_path, "worktree-ses-pushed");
        assert!(
            result.is_ok(),
            "rescue should skip when PR already pushed: {:?}",
            result.err()
        );

        // Verify nothing was committed (git log should be empty — no commits).
        let log_output = Command::new("git")
            .args(["log", "--oneline"])
            .current_dir(&wt_path)
            .output()
            .unwrap();
        assert!(
            log_output.stdout.is_empty(),
            "no commits should have been created; got: {}",
            String::from_utf8_lossy(&log_output.stdout)
        );
    }

    #[test]
    fn test_rescue_proceeds_when_no_status() {
        // No pathflow-session-status.json at all — rescue should proceed normally.
        let dir = tempfile::tempdir().unwrap();
        let wt_path = dir.path().join("worktree-ses-nostatus");
        fs::create_dir_all(&wt_path).unwrap();

        // Not a git repo — rescue returns Ok immediately (existing behavior).
        let result = rescue_uncommitted_work(&wt_path, "worktree-ses-nostatus");
        assert!(result.is_ok());
    }
}
