//! Whether a local branch is live work or a leftover copy only this clone
//! holds (TSK-1308).
//!
//! A local branch is this clone's own state: the requester can create,
//! move or keep it, and no one else sees it. In a clone with a remote, the
//! checks that look outside a pushed range for a completion of a task
//! ([`super::landing`]) and the claim check of `work claim`
//! ([`super::readiness`]) therefore count a local branch only when it is
//! live work:
//!
//! - a remote-tracking ref of the same branch name exists for a configured
//!   remote (the branch was published, so others can see it), or
//! - a worktree of this clone has it checked out (someone is working on it).
//!
//! Any other local branch is a leftover copy: a landing candidate that was
//! replaced, or a task branch left from an earlier use of an id. It is
//! named as information, never as a completion or a claim. A clone with no
//! remote has only local branches, so every one of them still counts.
//!
//! Every doubt counts the branch: a ref list or a worktree that cannot be
//! read makes each local branch live, so the checks refuse as before.

use std::collections::BTreeSet;

use git2::Repository;

use crate::git::GitName;

/// The facts that decide whether a local branch is live, read once.
pub(crate) struct LocalBranches {
    /// `None` when every local branch counts: the clone has no remote, or
    /// the refs, remotes or worktrees could not be read.
    known: Option<Known>,
}

struct Known {
    remotes: Vec<GitName>,
    /// Every ref name in the clone, as exact bytes.
    refs: BTreeSet<Vec<u8>>,
    /// The full ref names (`refs/heads/...`) a worktree has checked out.
    checked_out: BTreeSet<Vec<u8>>,
}

impl LocalBranches {
    /// Read the remotes, the ref names and every worktree's checked-out
    /// branch of `repo`'s clone.
    pub(crate) fn read(repo: &Repository) -> Self {
        Self {
            known: read_known(repo),
        }
    }

    /// Whether the local branch `full` (`refs/heads/<name>`) counts as live
    /// work. A name outside `refs/heads/` is not a local branch and counts.
    pub(crate) fn is_live(&self, full: &[u8]) -> bool {
        let Some(known) = &self.known else {
            return true;
        };
        let Some(short) = full.strip_prefix(b"refs/heads/") else {
            return true;
        };
        if known.checked_out.contains(full) {
            return true;
        }
        known.remotes.iter().any(|remote| {
            let mut tracking = b"refs/remotes/".to_vec();
            tracking.extend_from_slice(remote.bytes());
            tracking.push(b'/');
            tracking.extend_from_slice(short);
            known.refs.contains(&tracking)
        })
    }
}

fn read_known(repo: &Repository) -> Option<Known> {
    let remotes = crate::git::name::remote_names(repo).ok()?;
    if remotes.is_empty() {
        return None;
    }
    let mut refs = BTreeSet::new();
    for reference in repo.references().ok()? {
        refs.insert(reference.ok()?.name_bytes().to_vec());
    }
    Some(Known {
        remotes,
        refs,
        checked_out: checked_out(repo).ok()?,
    })
}

/// The full branch ref names that a worktree of `repo`'s clone has checked
/// out: the main worktree's and every linked worktree's `HEAD`, read from
/// the repository metadata, so a worktree whose folder is gone still holds
/// its branch.
///
/// # Errors
///
/// Returns why a worktree or its `HEAD` cannot be read.
pub(crate) fn checked_out(repo: &Repository) -> Result<BTreeSet<Vec<u8>>, String> {
    let mut branches = BTreeSet::new();
    let common = repo.commondir();
    let mut heads = vec![common.join("HEAD")];
    let names = repo.worktrees().map_err(|error| error.to_string())?;
    for name in &names {
        // Kept strict (issue 79): a worktree whose name cannot be read may
        // hold the branch, so the read stops instead of skipping it.
        let name = name
            .map_err(|error| error.to_string())?
            .ok_or("a worktree name is not valid UTF-8")?;
        heads.push(common.join("worktrees").join(name).join("HEAD"));
    }
    for head in heads {
        let text = match std::fs::read(&head) {
            Ok(text) => text,
            // A bare common directory has a HEAD; a missing one in a linked
            // worktree's metadata is a broken registration.
            Err(error) => return Err(format!("cannot read {}: {error}", head.display())),
        };
        if let Some(target) = text.strip_prefix(b"ref: ") {
            let target = target.strip_suffix(b"\n").unwrap_or(target);
            let target = target.strip_suffix(b"\r").unwrap_or(target);
            branches.insert(target.to_vec());
        }
    }
    Ok(branches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn git(dir: &Path, args: &[&str]) {
        let status = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        git(root, &["commit", "-q", "--allow-empty", "-m", "base"]);
        git(root, &["branch", "leftover"]);
        dir
    }

    fn live(root: &Path, name: &str) -> bool {
        let repo = Repository::open(root).unwrap();
        LocalBranches::read(&repo).is_live(format!("refs/heads/{name}").as_bytes())
    }

    #[test]
    fn every_local_branch_counts_in_a_clone_with_no_remote() {
        let dir = repo();
        assert!(live(dir.path(), "leftover"));
    }

    #[test]
    fn a_local_branch_no_remote_holds_and_no_worktree_uses_is_a_leftover() {
        let dir = repo();
        let root = dir.path();
        git(
            root,
            &["remote", "add", "origin", "file:///nonexistent/origin"],
        );
        assert!(!live(root, "leftover"));
        assert!(live(root, "main"), "the checked-out branch is live");
    }

    #[test]
    fn a_published_or_checked_out_local_branch_is_live() {
        let dir = repo();
        let root = dir.path();
        git(
            root,
            &["remote", "add", "origin", "file:///nonexistent/origin"],
        );
        git(
            root,
            &["update-ref", "refs/remotes/origin/leftover", "HEAD"],
        );
        assert!(live(root, "leftover"), "a remote holds it");
        git(root, &["branch", "other"]);
        let tree = tempfile::tempdir().unwrap();
        let at = tree.path().join("other");
        git(
            root,
            &["worktree", "add", "-q", at.to_str().unwrap(), "other"],
        );
        assert!(live(root, "other"), "a worktree has it checked out");
        // Read from a linked worktree too: the facts are the clone's.
        let repo = Repository::open(&at).unwrap();
        let read = LocalBranches::read(&repo);
        assert!(read.is_live(b"refs/heads/other"));
        assert!(read.is_live(b"refs/heads/main"));
        assert!(read.is_live(b"refs/heads/leftover"));
        git(root, &["branch", "gone"]);
        assert!(!live(root, "gone"), "a new local-only branch is a leftover");
    }
}
