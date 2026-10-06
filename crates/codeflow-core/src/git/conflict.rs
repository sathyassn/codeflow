//! Merge conflict detection using in-memory merge analysis.
//!
//! Performs a tree-level merge between the current HEAD and a target branch
//! to detect conflicts before PR creation, without modifying the working tree.

use std::path::Path;

use crate::error::GitError;

/// Result of a merge conflict check.
#[derive(Debug, Clone)]
pub struct ConflictResult {
    /// Whether conflicts were detected.
    pub has_conflicts: bool,
    /// List of file paths with conflicts (empty if clean).
    pub conflicting_files: Vec<String>,
    /// The target branch that was checked against.
    pub target_branch: String,
}

/// Check for merge conflicts between HEAD and a target branch.
///
/// Opens the repository at `repo_path`, finds the remote tracking ref
/// for `target_branch` (e.g., `refs/remotes/origin/main`), computes the
/// merge base, and performs an in-memory tree merge to detect conflicts.
///
/// # Returns
///
/// - `Ok(ConflictResult)` with `has_conflicts: false` if the merge is clean
///   or HEAD is already at or ahead of the merge base (fast-forward).
/// - `Ok(ConflictResult)` with `has_conflicts: true` and a list of
///   conflicting file paths if conflicts exist.
///
/// # Errors
///
/// - `GitError::RepoOpen` if the repository cannot be opened.
/// - `GitError::RefNotFound` if the target branch ref doesn't exist.
/// - `GitError::NoCommonAncestor` if no merge base exists.
/// - `GitError::Git2` for other git2 errors.
pub fn check_merge_conflicts(
    repo_path: &Path,
    target_branch: &str,
) -> Result<ConflictResult, GitError> {
    let repo = git2::Repository::open(repo_path)
        .map_err(|e| GitError::RepoOpen(format!("{}: {e}", repo_path.display())))?;

    let head_commit = repo
        .head()
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| GitError::RepoOpen(format!("cannot resolve HEAD: {e}")))?;

    let target_ref = find_target_ref(&repo, target_branch)?;
    let target_commit = target_ref.peel_to_commit()?;

    let merge_base_oid = repo
        .merge_base(head_commit.id(), target_commit.id())
        .map_err(|_| GitError::NoCommonAncestor(target_branch.to_string()))?;

    // Fast-forward: HEAD is at or behind the merge base.
    if head_commit.id() == merge_base_oid {
        return Ok(ConflictResult {
            has_conflicts: false,
            conflicting_files: Vec::new(),
            target_branch: target_branch.to_string(),
        });
    }

    // Already up-to-date: target is at merge base (HEAD is ahead).
    if target_commit.id() == merge_base_oid {
        return Ok(ConflictResult {
            has_conflicts: false,
            conflicting_files: Vec::new(),
            target_branch: target_branch.to_string(),
        });
    }

    // Perform in-memory tree merge.
    let ancestor_commit = repo.find_commit(merge_base_oid)?;
    let ancestor_tree = ancestor_commit.tree()?;
    let our_tree = head_commit.tree()?;
    let their_tree = target_commit.tree()?;

    let index = repo
        .merge_trees(&ancestor_tree, &our_tree, &their_tree, None)
        .map_err(|e| GitError::MergeFailed(e.to_string()))?;

    if !index.has_conflicts() {
        return Ok(ConflictResult {
            has_conflicts: false,
            conflicting_files: Vec::new(),
            target_branch: target_branch.to_string(),
        });
    }

    let conflicting_files = collect_conflict_paths(&index);

    Ok(ConflictResult {
        has_conflicts: true,
        conflicting_files,
        target_branch: target_branch.to_string(),
    })
}

/// Find the target branch reference.
///
/// Tries in order:
/// 1. `refs/remotes/origin/{target_branch}`
/// 2. `refs/heads/{target_branch}` (local branch)
pub(crate) fn find_target_ref<'r>(
    repo: &'r git2::Repository,
    target_branch: &str,
) -> Result<git2::Reference<'r>, GitError> {
    let remote_ref = format!("refs/remotes/origin/{target_branch}");
    if let Ok(reference) = repo.find_reference(&remote_ref) {
        return Ok(reference);
    }

    let local_ref = format!("refs/heads/{target_branch}");
    repo.find_reference(&local_ref)
        .map_err(|_| GitError::RefNotFound(format!("neither {remote_ref} nor {local_ref} exists")))
}

/// Result of a rebase attempt.
#[derive(Debug, Clone)]
pub enum RebaseResult {
    /// Rebase completed successfully.
    Success,
    /// Rebase aborted due to conflicts.
    ConflictAborted {
        /// Files that had conflicts during rebase.
        conflicting_files: Vec<String>,
    },
}

/// Attempt to rebase the current branch onto the target branch.
///
/// Runs `git rebase <target_branch>` as a subprocess. If rebase fails due
/// to conflicts, aborts the rebase and returns `ConflictAborted`.
///
/// # Arguments
///
/// * `repo_path` - Path to the git repository.
/// * `target_branch` - Branch to rebase onto (e.g., "main").
///
/// # Errors
///
/// Returns `GitError` if git commands fail for non-conflict reasons.
pub fn attempt_rebase(repo_path: &Path, target_branch: &str) -> Result<RebaseResult, GitError> {
    let mut args: Vec<String> = fallback_identity_args(repo_path)?;
    args.push("rebase".to_string());
    args.push(target_branch.to_string());
    let output = crate::git::command()
        .args(&args)
        .current_dir(repo_path)
        .output()
        .map_err(|e| GitError::MergeFailed(format!("failed to run git rebase: {e}")))?;

    if output.status.success() {
        return Ok(RebaseResult::Success);
    }

    // Rebase failed — collect conflict info from status and abort.
    let status_output = crate::git::command()
        .args(["diff", "--name-only", "-z", "--diff-filter=U"])
        .current_dir(repo_path)
        .output()
        .ok();

    let conflicting_files = status_output
        .as_ref()
        .map(|o| {
            // The list is shown to a person, so each exact path goes through
            // its display form (OS text rule, issue 79).
            o.stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| crate::git::GitName::from_bytes(path).display().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Abort the in-progress rebase.
    let _ = crate::git::command()
        .args(["rebase", "--abort"])
        .current_dir(repo_path)
        .output();

    Ok(RebaseResult::ConflictAborted { conflicting_files })
}

/// `-c user.name=… -c user.email=…` arguments supplying a fallback committer
/// identity, but ONLY when the repository (and machine) has none configured.
///
/// A rebase writes new commit objects and so needs a committer identity;
/// without one `git rebase` aborts with "Committer identity unknown". Fresh
/// machines — CI runners, freshly provisioned dev boxes, consumer repos that
/// never set a global identity — hit exactly that. When a real identity IS
/// configured (local, global, or system) it is left untouched so the rebase is
/// attributed correctly.
fn fallback_identity_args(repo_path: &Path) -> Result<Vec<String>, GitError> {
    Ok(identity_args_for(has_git_identity(repo_path)?))
}

/// Whether a committer identity (both `user.name` and `user.email`) is
/// resolvable for `repo_path` via any config scope (local, global, system).
fn has_git_identity(repo_path: &Path) -> Result<bool, GitError> {
    let configured = |key: &str| -> Result<bool, GitError> {
        let out = crate::git::command()
            .args(["config", "--null", "--get", key])
            .current_dir(repo_path)
            .output()
            .map_err(|error| GitError::MergeFailed(format!("cannot read {key}: {error}")))?;
        if out.status.code() == Some(1) {
            return Ok(false);
        }
        if !out.status.success() {
            return Err(GitError::MergeFailed(format!(
                "cannot read {key}: git config failed ({})",
                out.status
            )));
        }
        let value = out
            .stdout
            .strip_suffix(&[0])
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                GitError::MergeFailed(format!("cannot read {key}: empty or unframed identity"))
            })?;
        Ok(!value.is_empty())
    };
    let name = configured("user.name")?;
    let email = configured("user.email")?;
    Ok(name && email)
}

/// The `-c` argument list: empty when an identity already exists, or a fallback
/// identity otherwise. Split from the config probe so it is testable directly.
fn identity_args_for(has_identity: bool) -> Vec<String> {
    if has_identity {
        Vec::new()
    } else {
        vec![
            "-c".to_string(),
            "user.name=codeflow".to_string(),
            "-c".to_string(),
            "user.email=codeflow@localhost.invalid".to_string(),
        ]
    }
}

/// Collect file paths from merge conflicts in the index.
fn collect_conflict_paths(index: &git2::Index) -> Vec<String> {
    let mut names: Vec<crate::git::GitName> = Vec::new();
    if let Ok(conflicts) = index.conflicts() {
        for conflict in conflicts.flatten() {
            // A conflict entry has ancestor, our, and their sides.
            // Extract the path from whichever side is available.
            let path = conflict
                .our
                .as_ref()
                .or(conflict.their.as_ref())
                .or(conflict.ancestor.as_ref())
                .map(|entry| crate::git::GitName::from_bytes(&entry.path));

            if let Some(p) = path {
                if !names.contains(&p) {
                    names.push(p);
                }
            }
        }
    }
    // Shown to a person (OS text rule, issue 79): each exact path by its
    // display form.
    names
        .iter()
        .map(|name| name.display().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a git repo with an initial commit containing a file.
    fn init_repo_with_file(dir: &Path, filename: &str, content: &str) -> git2::Repository {
        // Pin the initial branch to `main` so the `set_head("refs/heads/main")`
        // calls below hold regardless of the host's `init.defaultBranch`
        // (GitHub runners default to `master`, not `main`).
        let mut init_opts = git2::RepositoryInitOptions::new();
        init_opts.initial_head("main");
        let repo = git2::Repository::init_opts(dir, &init_opts).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();

        // Write file to disk and add to index.
        fs::write(dir.join(filename), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(filename)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();

        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "initial commit", &tree, &[])
                .unwrap();
        }
        repo
    }

    /// Helper: commit a file change on the current branch.
    fn commit_file(
        repo: &git2::Repository,
        dir: &Path,
        filename: &str,
        content: &str,
        message: &str,
    ) -> git2::Oid {
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        fs::write(dir.join(filename), content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new(filename)).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&head])
            .unwrap()
    }

    #[test]
    fn test_clean_merge_no_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "shared.txt", "initial content\n");

        // Create target branch at initial commit.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &head, false).unwrap();
        }

        // Commit a non-conflicting change on HEAD (different file).
        commit_file(
            &repo,
            dir.path(),
            "feature.txt",
            "new feature\n",
            "add feature",
        );

        let result = check_merge_conflicts(dir.path(), "target").unwrap();
        assert!(!result.has_conflicts);
        assert!(result.conflicting_files.is_empty());
        assert_eq!(result.target_branch, "target");
    }

    #[test]
    fn test_conflicting_merge_detected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "base\n");

        // Create target branch from initial commit.
        {
            let initial = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &initial, false).unwrap();
        }

        // Switch to target and commit a conflicting change.
        repo.set_head("refs/heads/target").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        commit_file(
            &repo,
            dir.path(),
            "file.txt",
            "target change\n",
            "target edit",
        );

        // Go back to main and commit a different conflicting change.
        repo.set_head("refs/heads/main").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        commit_file(&repo, dir.path(), "file.txt", "main change\n", "main edit");

        let result = check_merge_conflicts(dir.path(), "target").unwrap();
        assert!(result.has_conflicts);
        assert!(!result.conflicting_files.is_empty());
    }

    #[test]
    fn test_conflict_file_list_correct() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "a.txt", "base a\n");

        // Add another file in initial commit.
        {
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            fs::write(dir.path().join("b.txt"), "base b\n").unwrap();
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("b.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "add b.txt", &tree, &[&head])
                .unwrap();
        }

        // Create target branch, modify both files.
        {
            let base = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &base, false).unwrap();
        }
        repo.set_head("refs/heads/target").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        // Commit both files changed on target.
        commit_file(&repo, dir.path(), "a.txt", "target a\n", "target a");
        commit_file(&repo, dir.path(), "b.txt", "target b\n", "target b");

        // Go back to main, modify both files differently.
        repo.set_head("refs/heads/main").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        commit_file(&repo, dir.path(), "a.txt", "main a\n", "main a");
        commit_file(&repo, dir.path(), "b.txt", "main b\n", "main b");

        let result = check_merge_conflicts(dir.path(), "target").unwrap();
        assert!(result.has_conflicts);
        assert!(result.conflicting_files.contains(&"a.txt".to_string()));
        assert!(result.conflicting_files.contains(&"b.txt".to_string()));
        assert_eq!(result.conflicting_files.len(), 2);
    }

    #[test]
    fn test_fast_forward_no_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "content\n");

        // Create target branch at same commit as HEAD.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &head, false).unwrap();
        }

        // Target is at same commit as HEAD — this is the fast-forward case
        // where HEAD == merge_base.
        let result = check_merge_conflicts(dir.path(), "target").unwrap();
        assert!(!result.has_conflicts);
        assert!(result.conflicting_files.is_empty());
    }

    #[test]
    fn test_no_common_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "content\n");

        // Create an orphan branch with no common ancestor.
        // git2 doesn't have a simple orphan API, so create a commit with no parents.
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        fs::write(dir.path().join("orphan.txt"), "orphan\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("orphan.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let orphan_oid = repo
            .commit(None, &sig, &sig, "orphan commit", &tree, &[])
            .unwrap();

        // Create orphan branch ref pointing to that commit.
        let orphan_commit = repo.find_commit(orphan_oid).unwrap();
        repo.branch("orphan-branch", &orphan_commit, false).unwrap();

        let result = check_merge_conflicts(dir.path(), "orphan-branch");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, GitError::NoCommonAncestor(_)),
            "expected NoCommonAncestor, got: {err}"
        );
    }

    #[test]
    fn test_target_branch_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_file(dir.path(), "file.txt", "content\n");

        let result = check_merge_conflicts(dir.path(), "nonexistent-branch");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, GitError::RefNotFound(_)),
            "expected RefNotFound, got: {err}"
        );
    }

    #[test]
    fn test_repo_open_failure() {
        let dir = tempfile::tempdir().unwrap();
        // No git repo initialized — should fail.
        let result = check_merge_conflicts(dir.path(), "main");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, GitError::RepoOpen(_)),
            "expected RepoOpen, got: {err}"
        );
    }

    #[test]
    fn test_unborn_head_cannot_resolve() {
        let dir = tempfile::tempdir().unwrap();
        // Repo is initialized but has NO commit — HEAD is unborn, so it cannot
        // be peeled to a commit even though the repository itself opens fine.
        let _repo = git2::Repository::init(dir.path()).unwrap();

        let result = check_merge_conflicts(dir.path(), "main");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(&err, GitError::RepoOpen(msg) if msg.contains("cannot resolve HEAD")),
            "expected RepoOpen(cannot resolve HEAD), got: {err}"
        );
    }

    #[test]
    fn test_conflict_result_fields() {
        let result = ConflictResult {
            has_conflicts: true,
            conflicting_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            target_branch: "main".to_string(),
        };
        assert!(result.has_conflicts);
        assert_eq!(result.conflicting_files.len(), 2);
        assert_eq!(result.target_branch, "main");
    }

    #[test]
    fn test_conflict_result_debug() {
        let result = ConflictResult {
            has_conflicts: false,
            conflicting_files: Vec::new(),
            target_branch: "develop".to_string(),
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("ConflictResult"));
        assert!(debug.contains("develop"));
    }

    #[test]
    fn test_conflict_result_clone() {
        let result = ConflictResult {
            has_conflicts: true,
            conflicting_files: vec!["x.rs".to_string()],
            target_branch: "main".to_string(),
        };
        let cloned = result.clone();
        assert_eq!(cloned.has_conflicts, result.has_conflicts);
        assert_eq!(cloned.conflicting_files, result.conflicting_files);
        assert_eq!(cloned.target_branch, result.target_branch);
    }

    #[test]
    fn test_head_ahead_of_target_no_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "v1\n");

        // Create target at initial commit.
        {
            let initial = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &initial, false).unwrap();
        }

        // Add commits on main (HEAD moves ahead of target).
        commit_file(&repo, dir.path(), "file.txt", "v2\n", "update to v2");

        // HEAD is ahead — target_commit.id() == merge_base => no conflicts.
        let result = check_merge_conflicts(dir.path(), "target").unwrap();
        assert!(!result.has_conflicts);
    }

    #[test]
    fn test_find_target_ref_local() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "content\n");
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("local-target", &head, false).unwrap();
        }

        let reference = find_target_ref(&repo, "local-target");
        assert!(reference.is_ok());
    }

    #[test]
    fn test_find_target_ref_remote() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "content\n");
        let head_oid = repo.head().unwrap().peel_to_commit().unwrap().id();

        // Create a remote-tracking ref with no matching local branch, so a
        // successful lookup can only come from the remote early-return path
        // (`refs/remotes/origin/{target}`), tried before the local fallback.
        repo.reference(
            "refs/remotes/origin/remote-target",
            head_oid,
            false,
            "create remote-tracking ref",
        )
        .unwrap();

        let reference = find_target_ref(&repo, "remote-target").unwrap();
        assert_eq!(
            reference.name().unwrap(),
            "refs/remotes/origin/remote-target"
        );
    }

    #[test]
    fn test_find_target_ref_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "content\n");

        let reference = find_target_ref(&repo, "does-not-exist");
        assert!(reference.is_err());
        let err = reference.map(|_| ()).unwrap_err();
        assert!(
            matches!(err, GitError::RefNotFound(_)),
            "expected RefNotFound, got: {err}"
        );
    }

    // -- RebaseResult tests --

    #[test]
    fn test_rebase_result_success_debug() {
        let result = RebaseResult::Success;
        let debug = format!("{result:?}");
        assert!(debug.contains("Success"));
    }

    #[test]
    fn test_rebase_result_conflict_aborted_debug() {
        let result = RebaseResult::ConflictAborted {
            conflicting_files: vec!["a.rs".to_string()],
        };
        let debug = format!("{result:?}");
        assert!(debug.contains("ConflictAborted"));
        assert!(debug.contains("a.rs"));
    }

    #[test]
    fn test_rebase_result_clone() {
        let result = RebaseResult::ConflictAborted {
            conflicting_files: vec!["b.rs".to_string()],
        };
        let cloned = result.clone();
        if let RebaseResult::ConflictAborted { conflicting_files } = cloned {
            assert_eq!(conflicting_files, vec!["b.rs".to_string()]);
        } else {
            panic!("expected ConflictAborted");
        }
    }

    #[test]
    fn r20_failed_identity_query_never_selects_scaffold_actor() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(repo.path().join("config"), "[broken\n").unwrap();
        assert!(format!("{:?}", has_git_identity(dir.path())).starts_with("Err("));
    }

    // -- attempt_rebase tests --

    #[test]
    fn test_attempt_rebase_clean() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "base.txt", "base\n");

        // Create target branch at initial commit.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &head, false).unwrap();
        }

        // Add a non-conflicting commit on main.
        commit_file(
            &repo,
            dir.path(),
            "feature.txt",
            "new file\n",
            "add feature",
        );

        // Add a commit on target (different file).
        repo.set_head("refs/heads/target").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        commit_file(&repo, dir.path(), "other.txt", "other file\n", "add other");

        // Switch back to main.
        repo.set_head("refs/heads/main").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();

        let result = attempt_rebase(dir.path(), "target").unwrap();
        assert!(matches!(result, RebaseResult::Success));
    }

    #[test]
    fn identity_args_supply_fallback_only_when_unconfigured() {
        // Configured → no override, so the rebase is attributed correctly.
        assert!(identity_args_for(true).is_empty());
        // Unconfigured → a fallback committer identity is injected, which is
        // what lets `git rebase` create commits on a machine with no identity
        // (fresh CI runners, freshly provisioned boxes, consumer repos).
        let args = identity_args_for(false);
        assert_eq!(args.iter().filter(|a| *a == "-c").count(), 2);
        assert!(args.iter().any(|a| a.starts_with("user.name=")));
        assert!(args.iter().any(|a| a.starts_with("user.email=")));
    }

    #[test]
    fn has_git_identity_true_when_repo_local_identity_set() {
        // A repo-local identity is seen regardless of ambient global config,
        // so no fallback is injected.
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "f.txt", "x\n");
        let mut cfg = repo.config().unwrap();
        cfg.set_str("user.name", "Real Dev").unwrap();
        cfg.set_str("user.email", "real@example.com").unwrap();
        assert!(has_git_identity(dir.path()).unwrap());
        assert!(fallback_identity_args(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_attempt_rebase_with_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let repo = init_repo_with_file(dir.path(), "file.txt", "base\n");

        // Create target branch.
        {
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("target", &head, false).unwrap();
        }

        // Modify file on target.
        repo.set_head("refs/heads/target").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        commit_file(
            &repo,
            dir.path(),
            "file.txt",
            "target change\n",
            "target edit",
        );

        // Modify same file on main (conflicting).
        repo.set_head("refs/heads/main").unwrap();
        repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        commit_file(&repo, dir.path(), "file.txt", "main change\n", "main edit");

        let result = attempt_rebase(dir.path(), "target").unwrap();
        assert!(
            matches!(result, RebaseResult::ConflictAborted { .. }),
            "expected ConflictAborted, got {result:?}"
        );
    }

    #[test]
    fn test_attempt_rebase_nonexistent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = init_repo_with_file(dir.path(), "file.txt", "content\n");

        // Rebase onto a branch that doesn't exist — should fail.
        let result = attempt_rebase(dir.path(), "nonexistent-branch-xyz");
        assert!(result.is_ok());
        // git rebase on a nonexistent branch returns an error exit code,
        // which we treat as ConflictAborted.
        assert!(
            matches!(
                result.as_ref().unwrap(),
                RebaseResult::ConflictAborted { .. }
            ),
            "expected ConflictAborted for nonexistent branch, got {result:?}"
        );
    }

    #[test]
    fn test_collect_conflict_paths_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let index = repo.index().unwrap();
        let paths = collect_conflict_paths(&index);
        assert!(paths.is_empty());
    }
}
