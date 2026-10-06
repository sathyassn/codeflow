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
    /// only; [`NON_UTF8_BRANCH`] when the name is not valid UTF-8.
    pub branch: String,
    /// The same branch, exact, for output.
    pub branch_name: Option<GitName>,
    /// `true` when this checkout is a linked worktree.
    pub is_worktree: bool,
}

impl RepoInfo {
    /// Discover the repository containing `start`, walking parents.
    /// Returns `None` when `start` is not inside a git repository.
    ///
    /// # Errors
    /// Repository discovery or HEAD cannot be read, or the repository has no worktree.
    pub fn discover(start: &Path) -> Result<Option<Self>, String> {
        let Some(repo) = open(start)? else {
            return Ok(None);
        };
        let root = repo
            .workdir()
            .ok_or("repository has no worktree")?
            .to_path_buf();
        let common_dir = repo.commondir().to_path_buf();
        let branch_name = current_branch_name(&repo)?;
        let branch = branch_rule_text(branch_name.as_ref());
        let is_worktree = repo.is_worktree();
        Ok(Some(Self {
            root,
            common_dir,
            branch,
            branch_name,
            is_worktree,
        }))
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

/// The checked-out branch exactly, or `None` only for a proven detached HEAD.
/// Unborn branches keep their symbolic name so protection applies before a commit.
///
/// # Errors
/// HEAD or its target cannot be read or is malformed.
pub fn current_branch_name(repo: &Repository) -> Result<Option<GitName>, String> {
    match crate::root_checkout::head(repo)? {
        crate::root_checkout::Head::Branch(name) => Ok(Some(name)),
        crate::root_checkout::Head::Detached(_) => Ok(None),
    }
}

/// Current branch rule text, empty only for a proven detached HEAD.
/// A non-UTF8 branch remains [`NON_UTF8_BRANCH`], which every branch rule
/// treats as protected; [`current_branch_name`] preserves its exact bytes.
///
/// # Errors
/// HEAD or its target cannot be read or is malformed.
pub fn current_branch(repo: &Repository) -> Result<String, String> {
    Ok(branch_rule_text(current_branch_name(repo)?.as_ref()))
}

fn branch_rule_text(name: Option<&GitName>) -> String {
    name.map_or_else(String::new, |name| {
        name.rule_text()
            .map_or_else(|_| NON_UTF8_BRANCH.to_string(), str::to_string)
    })
}

/// Open the repository at or above `start`; only a proven non-repository is absent.
///
/// # Errors
/// Discovery fails, including an existing repository marker with malformed metadata.
pub fn open(start: &Path) -> Result<Option<Repository>, String> {
    match Repository::discover(start) {
        Ok(repo) => {
            check_discovery_boundaries(start, &repo)?;
            Ok(Some(repo))
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            // libgit2 also reports NotFound for an existing .git with an invalid
            // HEAD. Only an empty directory is proven to contain no repository;
            // gitfiles, partial metadata and unreadable directories must refuse.
            for ancestor in start.ancestors() {
                let marker = ancestor.join(".git");
                if !crate::absence::proven_absent(&marker)
                    .map_err(|failure| format!("cannot inspect repository marker: {failure}"))?
                {
                    match std::fs::symlink_metadata(&marker) {
                        Ok(meta) if meta.is_dir() => {
                            let mut entries =
                                std::fs::read_dir(ancestor.join(".git")).map_err(|failure| {
                                    format!("cannot inspect repository directory: {failure}")
                                })?;
                            if entries
                                .next()
                                .transpose()
                                .map_err(|failure| {
                                    format!("cannot inspect repository entry: {failure}")
                                })?
                                .is_some()
                            {
                                return Err(format!(
                                    "cannot discover existing repository: {error}"
                                ));
                            }
                        }
                        Ok(_) => {
                            return Err(format!("cannot discover existing repository: {error}"))
                        }
                        Err(failure) => {
                            return Err(format!("cannot inspect repository marker: {failure}"))
                        }
                    }
                }
                if !crate::absence::proven_absent(&ancestor.join("objects"))
                    .map_err(|failure| failure.to_string())?
                    && ["HEAD", "config", "refs"]
                        .iter()
                        .try_fold(false, |found, name| {
                            crate::absence::proven_absent(&ancestor.join(name))
                                .map(|absent| found || !absent)
                        })
                        .map_err(|failure| failure.to_string())?
                {
                    return Err(format!("cannot discover existing bare repository: {error}"));
                }
            }
            Ok(None)
        }
        Err(error) => Err(format!("cannot discover repository: {error}")),
    }
}

/// libgit2 may ignore a broken inner marker and successfully discover an outer
/// repository. Each marker below the reported boundary must either resolve to
/// that repository (a gitfile can redirect outside this ancestry) or be empty.
fn check_discovery_boundaries(start: &Path, repo: &Repository) -> Result<(), String> {
    let start = start
        .canonicalize()
        .map_err(|error| format!("cannot resolve repository search directory: {error}"))?;
    let boundary = repo
        .workdir()
        .unwrap_or_else(|| repo.path())
        .canonicalize()
        .map_err(|error| format!("cannot resolve discovered repository directory: {error}"))?;
    let git_boundary = repo
        .path()
        .canonicalize()
        .map_err(|error| format!("cannot resolve discovered git directory: {error}"))?;
    for ancestor in start.ancestors() {
        if ancestor == boundary || ancestor == git_boundary {
            return Ok(());
        }
        let marker = ancestor.join(".git");
        if crate::absence::proven_absent(&marker)
            .map_err(|error| format!("cannot inspect repository marker: {error}"))?
        {
            continue;
        }
        let open_error = match Repository::open(ancestor) {
            Ok(local) => {
                let local_directory = local.path().canonicalize().map_err(|error| {
                    format!(
                        "cannot resolve repository marker {}: {error}",
                        marker.display()
                    )
                })?;
                if local_directory == git_boundary {
                    return Ok(());
                }
                return Err(format!(
                    "repository discovery skipped another repository at {}",
                    marker.display()
                ));
            }
            Err(error) => error,
        };
        let mut entries = std::fs::read_dir(&marker).map_err(|error| {
            format!(
                "cannot open repository marker {}: {open_error}; cannot inspect it: {error}",
                marker.display()
            )
        })?;
        if entries
            .next()
            .transpose()
            .map_err(|error| format!("cannot inspect repository entry: {error}"))?
            .is_some()
        {
            return Err(format!(
                "cannot open repository marker {}: {open_error}",
                marker.display()
            ));
        }
    }
    Err("discovered repository is outside the searched ancestry".to_string())
}

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn r22_broken_nested_marker_cannot_select_outer_repository() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        std::os::unix::fs::symlink(nested.join("missing"), nested.join(".git")).unwrap();
        assert!(super::open(&nested).is_err());
        std::fs::remove_file(nested.join(".git")).unwrap();
        std::fs::write(nested.join(".git"), "gitdir: missing\n").unwrap();
        assert!(super::open(&nested).is_err());
    }

    #[test]
    fn r22_absent_and_empty_nested_markers_keep_parent_discovery() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        for empty_marker in [false, true] {
            if empty_marker {
                std::fs::create_dir(nested.join(".git")).unwrap();
            }
            let repo = super::open(&nested).unwrap().unwrap();
            assert_eq!(
                repo.workdir().unwrap().canonicalize().unwrap(),
                dir.path().canonicalize().unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn r22_nested_bare_repository_stops_before_broken_outer_marker() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join(".git")).unwrap();
        let nested = dir.path().join("nested.git");
        Repository::init_bare(&nested).unwrap();
        let repo = super::open(&nested).unwrap().unwrap();
        assert!(repo.is_bare());
        assert_eq!(
            repo.path().canonicalize().unwrap(),
            nested.canonicalize().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn r22_repository_discovery_from_symlinked_subdirectory_keeps_real_ancestry() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        Repository::init(&root).unwrap();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&nested, &link).unwrap();
        let repo = super::open(&link).unwrap().unwrap();
        assert_eq!(
            repo.workdir().unwrap().canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn r22_repository_discovery_accepts_separate_git_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("checkout");
        let metadata = dir.path().join("metadata");
        git(
            dir.path(),
            &[
                "init",
                "--separate-git-dir",
                metadata.to_str().unwrap(),
                root.to_str().unwrap(),
            ],
        );
        for start in [&root, &metadata, &metadata.join("objects")] {
            let repo = super::open(start).unwrap().unwrap();
            assert_eq!(
                repo.path().canonicalize().unwrap(),
                metadata.canonicalize().unwrap()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn r18_repository_beneath_dangling_ancestor_is_not_absent() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("broken");
        std::os::unix::fs::symlink(dir.path().join("missing"), &link).unwrap();
        assert!(super::open(&link.join("child")).is_err());
    }

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
        assert!(RepoInfo::discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn test_discover_basic_repo() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let info = RepoInfo::discover(dir.path()).unwrap().unwrap();
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
        let info = RepoInfo::discover(&sub).unwrap().unwrap();
        assert_eq!(
            info.root.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn test_discover_unborn_head_reports_branch() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        let info = RepoInfo::discover(dir.path()).unwrap().unwrap();
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
        let name = current_branch(&repo).unwrap();
        assert_eq!(name, NON_UTF8_BRANCH);
        let policy = crate::hooks::policy::GitPolicy {
            protected_branches: vec!["main".to_string()],
            ..crate::hooks::policy::GitPolicy::default()
        };
        assert!(policy.branch_is_protected(&name));
        assert_eq!(
            current_branch_name(&repo)
                .unwrap()
                .unwrap()
                .display()
                .to_string(),
            "release/caf\\xe9"
        );
        // The unborn case reads the symbolic target the same way.
        std::fs::write(git_dir.join("packed-refs"), b"").unwrap();
        assert_eq!(
            current_branch(&Repository::open(dir.path()).unwrap()).unwrap(),
            name
        );
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
        let info = RepoInfo::discover(&wt).unwrap().unwrap();
        assert_eq!(info.branch, "feat/x");
        assert!(info.is_worktree);
        let main_info = RepoInfo::discover(&main).unwrap().unwrap();
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

#[cfg(test)]
mod r16_obtaining_regressions {
    #[test]
    fn r17_empty_git_directory_is_not_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert!(super::open(dir.path()).unwrap().is_none());
        assert!(super::RepoInfo::discover(dir.path()).unwrap().is_none());
    }

    #[test]
    fn r17_bare_repository_without_head_is_not_absent() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(dir.path()).unwrap();
        std::fs::remove_file(dir.path().join("HEAD")).unwrap();
        assert!(super::open(dir.path()).is_err());
    }

    #[test]
    fn r16_invalid_existing_head_is_not_detached_or_nonrepo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join(".git/HEAD"), b"invalid HEAD\n").unwrap();
        assert!(super::current_branch_name(&repo).is_err());
        assert!(super::current_branch(&repo).is_err());
        assert!(super::RepoInfo::discover(dir.path()).is_err());
        // libgit2 may still open a repository handle with malformed HEAD.
        // It must never become absent; the head consumer must still refuse.
        match super::open(dir.path()) {
            Ok(Some(reopened)) => assert!(super::current_branch_name(&reopened).is_err()),
            Err(_) => {}
            Ok(None) => panic!("an existing repository was discarded as absent"),
        }
        std::fs::remove_file(dir.path().join(".git/HEAD")).unwrap();
        assert!(super::open(dir.path()).is_err());
    }
}
