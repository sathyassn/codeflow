//! Read-only state inspection for local-work discard checks.
//! No mutation is performed by inspect. Tests create disposable Git fixtures.
use super::git_guard::Retarget;
use git2::{BranchType, Repository, Status, StatusOptions};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Normalized intent from `git_guard`'s existing option parser.
/// Dry runs/help/list operations must be excluded before constructing an intent.
#[derive(Debug)]
pub enum Intent {
    HardReset,
    RestorePaths(Vec<String>),
    CheckoutPaths(Vec<String>),
    StashDiscard,
    /// Only non-ignored candidates are judged here. -X is handled as a safe
    /// ignored-only operation by the caller; -x still has these candidates.
    /// The parser must not silently drop -e/pathspec-file/unknown semantics.
    CleanNonIgnored {
        paths: Vec<String>,
        directories: bool,
    },
    ForceDeleteBranches(Vec<String>),
    ForceRemoveWorktree(String),
}

/// Return the locally unique work at risk, or None when this intent is safe.
/// Errors are uncertainty, NEVER proof of a clean tree. The caller creates a
/// `git.discard_uncommitted` violation using the TARGET policy's level.
///
/// # Errors
/// Returns uncertainty for unreadable state or unsupported repository/path overrides.
pub fn inspect(
    cwd: &Path,
    target: Option<Retarget<'_>>,
    intent: &Intent,
) -> Result<Option<String>, String> {
    for name in [
        "GIT_INDEX_FILE",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_DIR",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(format!(
                "inherited {name} changes repository state outside this probe"
            ));
        }
    }
    // An explicit git dir without a modeled work-tree can make Git use the
    // command cwd as its work tree. Do not substitute libgit2's inferred tree.
    if target.is_some_and(|t| t.git_dir) {
        return Err(
            "cannot prove the working tree of an explicit git directory; use git -C <worktree>"
                .into(),
        );
    }
    let at = target.map_or_else(|| cwd.to_path_buf(), |t| cwd.join(t.path));
    let repo = Repository::discover(&at).map_err(|e| e.to_string())?;
    let root = repo.workdir().ok_or("repository has no working tree")?;
    let prefix = at
        .canonicalize()
        .map_err(|e| e.to_string())?
        .strip_prefix(root.canonicalize().map_err(|e| e.to_string())?)
        .map_err(|_| "command directory is outside its work tree")?
        .to_path_buf();
    match intent {
        Intent::HardReset => {
            let dirty = status_paths(&repo, false)?;
            Ok((!dirty.is_empty()).then(|| {
                format!(
                    "command would discard tracked changes: {}",
                    dirty.join(", ")
                )
            }))
        }
        Intent::CheckoutPaths(paths) => {
            let mut paths = paths.clone();
            if paths
                .first()
                .is_some_and(|p| repo.revparse_single(p).is_ok())
            {
                paths.remove(0);
            }
            inspect(cwd, target, &Intent::RestorePaths(paths))
        }
        Intent::RestorePaths(paths) => restore_paths(&repo, root, &prefix, paths),
        Intent::StashDiscard => match repo.find_reference("refs/stash") {
            Ok(_) => Ok(Some(
                "dropping or clearing the stash would discard locally saved work".into(),
            )),
            Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        },
        Intent::CleanNonIgnored { paths, directories } => {
            clean_paths(&repo, &prefix, paths, *directories)
        }
        Intent::ForceDeleteBranches(names) => delete_branches(&repo, names),
        Intent::ForceRemoveWorktree(path) => remove_worktree(&repo, &at, path),
    }
}

fn restore_paths(
    repo: &Repository,
    root: &Path,
    prefix: &Path,
    paths: &[String],
) -> Result<Option<String>, String> {
    let dirty = status_paths(repo, false)?;
    if dirty.is_empty() {
        return Ok(None);
    }
    let index = repo.index().map_err(|e| e.to_string())?;
    for path in paths {
        let rel = local_path(prefix, path)?;
        let text = portable(&rel)?;
        let directory = rel.as_os_str().is_empty()
            || root.join(&rel).symlink_metadata().is_ok_and(|m| m.is_dir())
            || index
                .iter()
                .any(|e| std::str::from_utf8(&e.path).is_ok_and(|p| under(p, &text) && p != text));
        // An explicit file restore is the accepted recoverable route.
        if directory && dirty.iter().any(|p| under(p, &text)) {
            return Ok(Some(format!(
                "restoring directory '{path}' would discard tracked changes"
            )));
        }
    }
    Ok(None)
}

fn clean_paths(
    repo: &Repository,
    prefix: &Path,
    paths: &[String],
    directories: bool,
) -> Result<Option<String>, String> {
    let candidates = untracked_paths(repo, directories || !paths.is_empty())?;
    let specs: Vec<String> = if paths.is_empty() {
        vec![portable(prefix)?]
    } else {
        paths
            .iter()
            .map(|p| local_path(prefix, p).and_then(|p| portable(&p)))
            .collect::<Result<_, _>>()?
    };
    for path in candidates {
        if specs.iter().any(|spec| under(&path, spec)) {
            // libgit2 collapses only wholly untracked directories when not
            // recursing. Files below tracked directories remain candidates.
            if !path.ends_with('/') {
                return Ok(Some(format!(
                    "clean would discard untracked non-ignored work: {path}"
                )));
            }
        }
    }
    Ok(None)
}

fn delete_branches(repo: &Repository, names: &[String]) -> Result<Option<String>, String> {
    let removed: BTreeSet<_> = names.iter().map(|n| format!("refs/heads/{n}")).collect();
    let mut kept = Vec::new();
    for reference in repo.references().map_err(|e| e.to_string())? {
        let reference = reference.map_err(|e| e.to_string())?;
        let name = reference.name().map_err(|e| e.to_string())?;
        if !removed.contains(name) {
            // Only commit-bearing refs establish another live reachability path.
            if let Ok(commit) = reference.peel_to_commit() {
                kept.push(commit.id());
            }
        }
    }
    for name in names {
        let branch = match repo.find_branch(name, BranchType::Local) {
            Ok(branch) => branch,
            Err(e) if e.code() == git2::ErrorCode::NotFound => continue,
            Err(e) => return Err(e.to_string()),
        };
        let tip = branch
            .get()
            .peel_to_commit()
            .map_err(|e| e.to_string())?
            .id();
        let mut backed = false;
        for other in &kept {
            if *other == tip
                || repo
                    .graph_descendant_of(*other, tip)
                    .map_err(|e| e.to_string())?
            {
                backed = true;
                break;
            }
        }
        if !backed {
            return Ok(Some(format!(
                "deleting '{name}' would leave its commits without another live ref"
            )));
        }
    }
    Ok(None)
}

fn remove_worktree(repo: &Repository, at: &Path, path: &str) -> Result<Option<String>, String> {
    let wanted = at.join(path);
    let wanted = wanted.canonicalize().map_err(|e| e.to_string())?;
    let mut matched = None;
    for name in &repo.worktrees().map_err(|e| e.to_string())? {
        let name = name
            .map_err(|e| e.to_string())?
            .ok_or("non-UTF8 worktree name")?;
        let tree = repo.find_worktree(name).map_err(|e| e.to_string())?;
        if tree.path().canonicalize().map_err(|e| e.to_string())? == wanted {
            matched = Some(tree);
            break;
        }
    }
    let tree = matched.ok_or("requested path is not a resolved linked worktree")?;
    let other = Repository::open(tree.path()).map_err(|e| e.to_string())?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    let states = other
        .statuses(Some(&mut options))
        .map_err(|e| e.to_string())?;
    Ok((!states.is_empty())
        .then(|| format!("force-removing worktree '{path}' would discard its dirty work")))
}

fn status_paths(repo: &Repository, untracked: bool) -> Result<Vec<String>, String> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(untracked)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    repo.statuses(Some(&mut options))
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|e| !e.status().is_empty() && !e.status().contains(Status::IGNORED))
        .map(|e| e.path().map(str::to_owned).map_err(|e| e.to_string()))
        .collect()
}
fn untracked_paths(repo: &Repository, recurse: bool) -> Result<Vec<String>, String> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(recurse)
        .include_ignored(false);
    repo.statuses(Some(&mut options))
        .map_err(|e| e.to_string())?
        .iter()
        .filter(|e| e.status().contains(Status::WT_NEW))
        .map(|e| e.path().map(str::to_owned).map_err(|e| e.to_string()))
        .collect()
}
fn portable(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(|p| p.replace('\\', "/"))
        .ok_or_else(|| "non-UTF8 pathspec".into())
}
fn under(path: &str, directory: &str) -> bool {
    directory.is_empty()
        || path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|s| s.starts_with('/'))
}
fn local_path(prefix: &Path, text: &str) -> Result<PathBuf, String> {
    if text.starts_with(':') || text.contains(['*', '?', '[']) {
        return Err("pathspec magic or a pattern needs an explicit discard preview".into());
    }
    let path = Path::new(text);
    if path.is_absolute() {
        return Err("absolute pathspec needs an explicit discard preview".into());
    }
    let mut result = prefix.to_path_buf();
    for part in path.components() {
        match part {
            Component::Normal(p) => result.push(p),
            Component::CurDir => {}
            Component::ParentDir if result.pop() => {}
            _ => return Err("pathspec is outside the repository".into()),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    // These tests MUTATE ONLY newly created disposable test repositories.
    // They are a proposal; they have not been compiled or run by the worker.
    use super::*;
    fn fixture() -> (tempfile::TempDir, Repository) {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/a"), "base\n").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        commit(&repo);
        (dir, repo)
    }
    fn commit(repo: &Repository) {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let oid = index.write_tree().unwrap();
        let tree = repo.find_tree(oid).unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, "test: snapshot", &tree, &parents)
            .unwrap();
    }
    #[test]
    fn reset_and_directory_restore_dirty_clean_pairs_and_file_control() {
        let (dir, _repo) = fixture();
        assert!(inspect(dir.path(), None, &Intent::HardReset)
            .unwrap()
            .is_none());
        std::fs::write(dir.path().join("src/a"), "local\n").unwrap();
        assert!(inspect(dir.path(), None, &Intent::HardReset)
            .unwrap()
            .is_some());
        assert!(
            inspect(dir.path(), None, &Intent::RestorePaths(vec!["src".into()]))
                .unwrap()
                .is_some()
        );
        assert!(inspect(
            dir.path(),
            None,
            &Intent::RestorePaths(vec!["src/a".into()])
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn f4_clean_files_in_tracked_directories() {
        let (dir, _repo) = fixture();
        let intent = Intent::CleanNonIgnored {
            paths: vec![],
            directories: false,
        };
        std::fs::create_dir(dir.path().join("untracked")).unwrap();
        std::fs::write(dir.path().join("untracked/notes"), "keep").unwrap();
        assert!(inspect(dir.path(), None, &intent).unwrap().is_none());
        std::fs::write(dir.path().join("src/notes"), "keep").unwrap();
        assert!(inspect(dir.path(), None, &intent).unwrap().is_some());
    }

    #[test]
    fn clean_ignored_and_nonignored_pair() {
        let (dir, _repo) = fixture();
        std::fs::create_dir(dir.path().join("target")).unwrap();
        std::fs::write(dir.path().join("target/out"), "generated\n").unwrap();
        let intent = Intent::CleanNonIgnored {
            paths: vec![],
            directories: true,
        };
        assert!(inspect(dir.path(), None, &intent).unwrap().is_none());
        std::fs::write(dir.path().join("notes"), "unique\n").unwrap();
        assert!(inspect(dir.path(), None, &intent).unwrap().is_some());
    }
    #[test]
    fn retarget_uses_the_other_repository() {
        let (clean, _repo) = fixture();
        let (dirty, _repo2) = fixture();
        std::fs::write(dirty.path().join("src/a"), "unique\n").unwrap();
        let target = Retarget {
            path: dirty.path().to_str().unwrap(),
            git_dir: false,
        };
        assert!(inspect(clean.path(), Some(target), &Intent::HardReset)
            .unwrap()
            .is_some());
        assert!(inspect(clean.path(), None, &Intent::HardReset)
            .unwrap()
            .is_none());
    }
    #[test]
    fn deleting_all_backing_branches_is_not_backed_by_each_other() {
        let (dir, repo) = fixture();
        let base = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("a", &base, false).unwrap();
        repo.set_head("refs/heads/a").unwrap();
        std::fs::write(dir.path().join("src/a"), "second\n").unwrap();
        commit(&repo);
        let tip = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("b", &tip, false).unwrap();
        assert!(inspect(
            dir.path(),
            None,
            &Intent::ForceDeleteBranches(vec!["a".into()])
        )
        .unwrap()
        .is_none());
        assert!(inspect(
            dir.path(),
            None,
            &Intent::ForceDeleteBranches(vec!["a".into(), "b".into()])
        )
        .unwrap()
        .is_some());
    }
    #[test]
    fn stash_empty_and_saved_pair() {
        let (dir, mut repo) = fixture();
        assert!(inspect(dir.path(), None, &Intent::StashDiscard)
            .unwrap()
            .is_none());
        std::fs::write(dir.path().join("src/a"), "saved work\n").unwrap();
        let sig = git2::Signature::now("Test", "test@example.invalid").unwrap();
        repo.stash_save(&sig, "saved work", None).unwrap();
        assert!(inspect(dir.path(), None, &Intent::StashDiscard)
            .unwrap()
            .is_some());
    }
    #[test]
    fn forced_worktree_removal_clean_and_dirty_pair() {
        let (dir, repo) = fixture();
        let at = dir.path().join("other");
        repo.worktree("other", &at, None).unwrap();
        let intent = Intent::ForceRemoveWorktree(at.to_string_lossy().into_owned());
        assert!(inspect(dir.path(), None, &intent).unwrap().is_none());
        std::fs::write(at.join("src/a"), "dirty\n").unwrap();
        assert!(inspect(dir.path(), None, &intent).unwrap().is_some());
    }
    #[test]
    fn directory_restore_is_relative_to_actual_command_cwd() {
        let (dir, _repo) = fixture();
        std::fs::write(dir.path().join("src/a"), "dirty\n").unwrap();
        assert!(inspect(
            &dir.path().join("src"),
            None,
            &Intent::RestorePaths(vec![".".into()])
        )
        .unwrap()
        .is_some());
        assert!(inspect(
            &dir.path().join("src"),
            None,
            &Intent::RestorePaths(vec!["a".into()])
        )
        .unwrap()
        .is_none());
    }
}
