//! Repository discovery for the hook plane.
//!
//! Hooks fire from arbitrary working directories (worktrees included); this
//! resolves the project root, the *common* git dir (runtime state under
//! `.git/codeflow/` is shared across worktrees, charter §4.1), and the
//! current branch.

use std::path::{Path, PathBuf};

use git2::Repository;

use crate::git::GitName;
use crate::hooks::policy::NON_UTF8_BRANCH;

/// Resolved repository context for a hook invocation.
pub struct RepoInfo {
    /// Working-tree root (the worktree's own root when in a worktree).
    pub root: PathBuf,
    /// The common git directory (`.git` of the main checkout).
    pub common_dir: PathBuf,
    /// Current branch name as the hook plane matches it: empty when detached
    /// or unreadable, [`NON_UTF8_BRANCH`] when the name is not valid UTF-8.
    pub branch: String,
    /// The same branch, exact, for output.
    pub branch_name: Option<GitName>,
    /// `true` when this checkout is a linked worktree.
    pub is_worktree: bool,
}

impl RepoInfo {
    /// Discover the repository containing `start`, walking parents.
    /// Returns `None` when `start` is not inside a git repository.
    #[must_use]
    pub fn discover(start: &Path) -> Option<Self> {
        let repo = Repository::discover(start).ok()?;
        let root = repo.workdir()?.to_path_buf();
        let common_dir = repo.commondir().to_path_buf();
        let branch = current_branch(&repo);
        let branch_name = current_branch_name(&repo);
        let is_worktree = repo.is_worktree();
        Some(Self {
            root,
            common_dir,
            branch,
            branch_name,
            is_worktree,
        })
    }

    /// Runtime state directory, shared across worktrees: `<common>/codeflow`.
    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.common_dir.join("codeflow")
    }

    /// Ledger directory under the state dir.
    #[must_use]
    pub fn ledger_dir(&self) -> PathBuf {
        self.state_dir().join("ledger")
    }
}

/// The checked-out branch of an open repository, exactly: `None` when HEAD is
/// detached or unreadable.
///
/// Handles the unborn-HEAD case (fresh repo before the first commit) by
/// reading the symbolic target, so branch protection applies from minute one.
#[must_use]
pub fn current_branch_name(repo: &Repository) -> Option<GitName> {
    if let Ok(head) = repo.head() {
        let name = crate::git::name::reference_shorthand(&head);
        if name.bytes() != b"HEAD" && !name.is_empty() {
            return Some(name);
        }
        return None; // detached
    }
    // Unborn branch: HEAD exists as a symbolic ref with no target commit.
    let head_ref = repo.find_reference("HEAD").ok()?;
    crate::git::name::symbolic_target(&head_ref)?.strip_prefix(b"refs/heads/")
}

/// Current branch name for the hook plane: its text, empty on a detached HEAD.
///
/// OS text rule (issue 79, `docs/architecture.md`): the name is matched
/// against protected-branch globs, which need text. A name that is not valid
/// UTF-8 cannot be matched, and reading it as detached or empty would drop a
/// protected-branch refusal, so it reads as [`NON_UTF8_BRANCH`], which every
/// branch rule treats as protected. Use [`current_branch_name`] to show it.
#[must_use]
pub fn current_branch(repo: &Repository) -> String {
    match current_branch_name(repo) {
        None => String::new(),
        Some(name) => name
            .rule_text()
            .map_or_else(|_| NON_UTF8_BRANCH.to_string(), str::to_string),
    }
}

/// Open the repository at (or above) `start` for direct git2 queries.
#[must_use]
pub fn open(start: &Path) -> Option<Repository> {
    Repository::discover(start).ok()
}

#[cfg(test)]
mod tests {

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-b", "main"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", "chore: init"]);
    }

    #[test]
    fn test_discover_none_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(RepoInfo::discover(dir.path()).is_none());
    }

    #[test]
    fn test_discover_basic_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let info = RepoInfo::discover(dir.path()).unwrap();
        assert_eq!(info.branch, "main");
        assert!(!info.is_worktree);
        assert!(info.state_dir().ends_with("codeflow"));
        assert!(info.common_dir.exists());
    }

    #[test]
    fn test_discover_from_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let sub = dir.path().join("src/deep");
        std::fs::create_dir_all(&sub).unwrap();
        let info = RepoInfo::discover(&sub).unwrap();
        assert_eq!(
            info.root.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_discover_unborn_head_reports_branch() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let info = RepoInfo::discover(dir.path()).unwrap();
        assert_eq!(info.branch, "main");
    }

    /// Review finding on issue 79: a branch whose name is not valid UTF-8 read
    /// as detached, so a protected pattern never saw it and the refusal was
    /// dropped. The hook plane now reads it as one sentinel that every branch
    /// rule treats as protected, and the exact name is kept for display.
    #[test]
    fn a_branch_that_is_not_utf8_is_not_detached_and_stays_protected() {
        let dir = tempfile::tempdir().unwrap();
        let git_repo = Repository::init(dir.path()).unwrap();
        let tree = git_repo
            .find_tree(git_repo.index().unwrap().write_tree().unwrap())
            .unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let commit = git_repo
            .commit(Some("HEAD"), &sig, &sig, "test: seed", &tree, &[])
            .unwrap();
        drop(tree);
        let git_dir = dir.path().join(".git");
        let mut packed = b"# pack-refs with: peeled fully-peeled sorted \n".to_vec();
        packed.extend_from_slice(format!("{commit} refs/heads/release/").as_bytes());
        packed.extend_from_slice(b"caf\xe9\n");
        std::fs::write(git_dir.join("packed-refs"), packed).unwrap();
        std::fs::write(git_dir.join("HEAD"), b"ref: refs/heads/release/caf\xe9\n").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let name = current_branch(&repo);
        assert_eq!(name, NON_UTF8_BRANCH);
        let policy = crate::hooks::policy::GitPolicy {
            protected_branches: vec!["main".to_string()],
            ..crate::hooks::policy::GitPolicy::default()
        };
        assert!(policy.branch_is_protected(&name));
        assert_eq!(
            current_branch_name(&repo).unwrap().display().to_string(),
            "release/caf\\xe9"
        );
        // The unborn case reads the symbolic target the same way.
        std::fs::write(git_dir.join("packed-refs"), b"").unwrap();
        assert_eq!(current_branch(&Repository::open(dir.path()).unwrap()), name);
    }

    #[test]
    fn test_worktree_shares_common_dir() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("repo");
        std::fs::create_dir_all(&main).unwrap();
        init_repo(&main);
        let wt = dir.path().join("wt");
        git(
            &main,
            &["worktree", "add", wt.to_str().unwrap(), "-b", "feat/x"],
        );
        let info = RepoInfo::discover(&wt).unwrap();
        assert_eq!(info.branch, "feat/x");
        assert!(info.is_worktree);
        let main_info = RepoInfo::discover(&main).unwrap();
        // State dirs resolve to the same place: shared across worktrees.
        assert_eq!(
            info.state_dir().parent().unwrap().canonicalize().unwrap(),
            main_info
                .state_dir()
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap(),
        );
    }
}
