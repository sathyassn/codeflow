//! Guarded parent and ancestry reads for local judgments (TSK-287).
//!
//! Every read refuses grafts and replace refs. Walks refuse a shallow
//! boundary when they reach it; a boundary hidden by the range's base does
//! not truncate that range. Errors retain the caller's ordinary refusal path.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

struct Metadata {
    boundary: Rc<HashSet<Oid>>,
    graph: Rc<super::history_graph::Verified>,
}
type Snapshot = Result<Rc<Metadata>, String>;
thread_local! {
    static JUDGMENT: RefCell<Option<HashMap<PathBuf, Snapshot>>> = const { RefCell::new(None) };
}

/// Reuse one overlay and shallow-boundary snapshot during a read-only judgment.
/// Nested scopes share their caller's snapshot. Dropping the outer scope clears
/// it, so later judgments see metadata changes. This guard stays on its thread.
#[must_use]
pub struct Judgment {
    owner: bool,
    _thread: std::marker::PhantomData<Rc<()>>,
}

/// Begin a read-only judgment. Do not retain this scope across Git mutations.
pub fn judgment() -> Judgment {
    let owner = JUDGMENT.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.is_some() {
            false
        } else {
            *memo = Some(HashMap::new());
            true
        }
    });
    Judgment {
        owner,
        _thread: std::marker::PhantomData,
    }
}

impl Drop for Judgment {
    fn drop(&mut self) {
        if self.owner {
            JUDGMENT.with(|memo| *memo.borrow_mut() = None);
        }
    }
}

/// Discard a snapshot after an object fetch may have updated shallow metadata.
pub(super) fn refresh(repo: &Repository) {
    JUDGMENT.with(|memo| {
        if let Some(memo) = memo.borrow_mut().as_mut() {
            memo.remove(repo.path());
        }
    });
}

fn snapshot(repo: &Repository) -> Snapshot {
    JUDGMENT.with(|memo| {
        let mut memo = memo.borrow_mut();
        if let Some(memo) = memo.as_mut() {
            memo.entry(repo.path().to_path_buf())
                .or_insert_with(|| read_snapshot(repo))
                .clone()
        } else {
            read_snapshot(repo)
        }
    })
}

#[cfg(test)]
thread_local! { static SNAPSHOT_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn read_snapshot(repo: &Repository) -> Snapshot {
    #[cfg(test)]
    SNAPSHOT_READS.with(|reads| reads.set(reads.get() + 1));
    if let Some(overlay) = super::release_line::history_overlay(repo)? {
        return Err(format!(
            "this clone overlays its recorded history with {overlay}; remove it or judge from a clean clone"
        ));
    }
    let graph = Rc::new(super::history_graph::verify(repo)?);
    let boundary = Rc::new(super::release_line::shallow_boundary(repo)?);
    Ok(Rc::new(Metadata { boundary, graph }))
}

use git2::{Commit, Error, ErrorCode, Oid, Repository, Sort};

fn unverified(why: impl std::fmt::Display) -> Error {
    Error::from_str(&format!("cannot verify history: {why}"))
}

/// Refuse a local history overlay before reading any parent or ancestry.
///
/// # Errors
/// Returns `cannot verify` for a graft, replacement or unreadable overlay.
pub fn check(repo: &Repository) -> Result<(), Error> {
    snapshot(repo)
        .and_then(|metadata| metadata.graph.check())
        .map_err(unverified)
}

fn boundary(repo: &Repository) -> Result<Rc<HashSet<Oid>>, Error> {
    // Raw commit-object parent reads cannot consume libgit2's graph cache.
    // Graph-based ancestry calls and Walk check its identity separately.
    let metadata = snapshot(repo).map_err(unverified)?;
    Ok(Rc::clone(&metadata.boundary))
}

fn complete(boundary: &HashSet<Oid>, oid: Oid) -> Result<(), Error> {
    if boundary.contains(&oid) {
        return Err(unverified(format!(
            "this clone is shallow at {oid}; fetch its full history"
        )));
    }
    Ok(())
}

/// A commit's parents, including a genuine root's empty list.
///
/// # Errors
/// Refuses overlays and a shallow boundary at this commit.
pub fn parents(repo: &Repository, commit: &Commit<'_>) -> Result<Vec<Oid>, Error> {
    complete(boundary(repo)?.as_ref(), commit.id())?;
    Ok(commit.parent_ids().collect())
}

/// One parent id. Only an out-of-range index is `NotFound`.
///
/// # Errors
/// Refuses unverified parents or an absent index.
pub fn parent_id(repo: &Repository, commit: &Commit<'_>, index: usize) -> Result<Oid, Error> {
    parents(repo, commit)?.get(index).copied().ok_or_else(|| {
        Error::new(
            ErrorCode::NotFound,
            git2::ErrorClass::Object,
            "no such parent",
        )
    })
}

/// One parent commit.
///
/// # Errors
/// Refuses unverified parents, an absent index or a missing object.
pub fn parent<'r>(
    repo: &'r Repository,
    commit: &Commit<'_>,
    index: usize,
) -> Result<Commit<'r>, Error> {
    repo.find_commit(parent_id(repo, commit, index)?)
}

/// A walk whose yielded commits cannot conceal a shallow boundary.
pub struct Walk<'r> {
    inner: git2::Revwalk<'r>,
    boundary: Rc<HashSet<Oid>>,
    graph: Rc<super::history_graph::Verified>,
}

/// Begin a guarded walk. Push and hide retain libgit2's range semantics.
///
/// # Errors
/// Refuses overlays or an unreadable shallow boundary.
pub fn revwalk(repo: &Repository) -> Result<Walk<'_>, Error> {
    let metadata = snapshot(repo).map_err(unverified)?;
    metadata.graph.check().map_err(unverified)?;
    Ok(Walk {
        inner: repo.revwalk()?,
        boundary: Rc::clone(&metadata.boundary),
        graph: Rc::clone(&metadata.graph),
    })
}

impl Walk<'_> {
    /// Add a starting commit.
    /// # Errors
    /// Returns a Git error if the commit cannot be read.
    pub fn push(&mut self, oid: Oid) -> Result<(), Error> {
        self.graph.check().map_err(unverified)?;
        self.inner.push(oid)
    }
    /// Exclude a commit and its ancestors.
    /// # Errors
    /// Returns a Git error if the commit cannot be read.
    pub fn hide(&mut self, oid: Oid) -> Result<(), Error> {
        self.graph.check().map_err(unverified)?;
        self.inner.hide(oid)
    }
    /// Select the traversal order.
    /// # Errors
    /// Returns a Git error when sorting cannot be set.
    pub fn set_sorting(&mut self, sorting: Sort) -> Result<(), Error> {
        self.graph.check().map_err(unverified)?;
        self.inner.set_sorting(sorting)
    }
    /// Follow only first parents.
    /// # Errors
    /// Returns a Git error when the walk cannot be configured.
    pub fn simplify_first_parent(&mut self) -> Result<(), Error> {
        self.graph.check().map_err(unverified)?;
        self.inner.simplify_first_parent()
    }
}

impl Iterator for Walk<'_> {
    type Item = Result<Oid, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if let Err(error) = self.graph.check() {
            return Some(Err(unverified(error)));
        }
        self.inner.next().map(|oid| {
            let oid = oid?;
            complete(&self.boundary, oid)?;
            Ok(oid)
        })
    }
}

/// Verify the reachable history outside an already established common base.
fn verify_range(repo: &Repository, tips: &[Oid], hide: Option<Oid>) -> Result<(), Error> {
    if boundary(repo)?.is_empty() {
        return Ok(());
    }
    let mut walk = revwalk(repo)?;
    for tip in tips {
        walk.push(*tip)?;
    }
    if let Some(hide) = hide {
        walk.hide(hide)?;
    }
    for oid in walk {
        oid?;
    }
    Ok(())
}

/// The common base of two commits, from verified ancestry.
///
/// # Errors
/// Refuses overlays, a shallow cut in the compared history, unreadable
/// history, or (`NotFound`) histories proven unrelated.
pub fn merge_base(repo: &Repository, left: Oid, right: Oid) -> Result<Oid, Error> {
    check(repo)?;
    let result = repo.merge_base(left, right);
    match &result {
        Ok(base) => verify_range(repo, &[left, right], Some(*base))?,
        Err(error) if error.code() == ErrorCode::NotFound => {
            verify_range(repo, &[left, right], None)?;
        }
        Err(_) => {}
    }
    result
}

/// Whether `descendant` strictly descends from `ancestor`.
///
/// # Errors
/// Refuses overlays, incomplete ancestry or a reachable shallow boundary
/// before the named ancestor.
pub fn descendant_of(repo: &Repository, descendant: Oid, ancestor: Oid) -> Result<bool, Error> {
    check(repo)?;
    let result = repo.graph_descendant_of(descendant, ancestor)?;
    verify_range(repo, &[descendant], result.then_some(ancestor))?;
    Ok(result)
}

/// Ahead and behind counts over verified ancestry.
///
/// # Errors
/// Refuses unverified ancestry or unreadable commits.
pub fn ahead_behind(repo: &Repository, local: Oid, upstream: Oid) -> Result<(usize, usize), Error> {
    let base = match merge_base(repo, local, upstream) {
        Ok(base) => Some(base),
        Err(error) if error.code() == ErrorCode::NotFound => None,
        Err(error) => return Err(error),
    };
    verify_range(repo, &[local, upstream], base)?;
    check(repo)?;
    repo.graph_ahead_behind(local, upstream)
}

/// A common base for the octopus import calculation.
///
/// # Errors
/// Refuses overlays, incomplete ancestry or unreadable commits.
pub fn merge_base_many(repo: &Repository, commits: &[Oid]) -> Result<Oid, Error> {
    check(repo)?;
    let result = repo.merge_base_many(commits);
    match &result {
        Ok(base) => verify_range(repo, commits, Some(*base))?,
        Err(error) if error.code() == ErrorCode::NotFound => verify_range(repo, commits, None)?,
        Err(_) => {}
    }
    result
}

/// Build a judging Git subprocess bound to its explicit directory.
/// Call `check_command` before history queries; this constructor also serves
/// their supporting object and ref lookups. Never inherit an object store
/// or repository redirect that the reader did not check.
#[must_use]
pub fn command() -> std::process::Command {
    let mut command = crate::git::command();
    command
        .args(["-c", "core.commitGraph=false"])
        .env_remove("GIT_DIR")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_WORK_TREE");
    command
}

/// Guard a spawned history query whose range is supplied as Git arguments.
/// These callers require complete history; they cannot distinguish a cut
/// from the root of an unrelated line.
///
/// # Errors
/// Refuses overlays, shallow clones or unreadable repository metadata.
pub fn check_command(root: &std::path::Path) -> Result<(), String> {
    let repo = Repository::discover(root).map_err(|error| unverified(error).to_string())?;
    check(&repo).map_err(|error| error.to_string())?;
    if !boundary(&repo)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err(crate::remedy::Finding::new(
            unverified("this clone is shallow").to_string(),
            crate::remedy::CI_RANGE_UNREADABLE.remedy(),
        )
        .to_string());
    }
    if let Some(file) = std::env::var_os("GIT_SHALLOW_FILE") {
        // Git treats an empty override as an explicitly complete history.
        if file.is_empty() {
            return Ok(());
        }
        // These Git queries run with `current_dir(root)` or `git -C root`.
        // Git resolves a relative override after that directory change.
        let file = root.join(file);
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => {
                return Err(unverified(format!(
                    "cannot read the shallow file {}: {error}",
                    file.display()
                ))
                .to_string())
            }
        };
        if !bytes.is_empty() {
            return Err(crate::remedy::Finding::new(
                unverified(format!("Git uses the shallow file {}", file.display())).to_string(),
                crate::remedy::CI_RANGE_UNREADABLE.remedy(),
            )
            .to_string());
        }
        return Ok(());
    }
    Ok(())
}

/// Read the first parent recorded in a commit object after checking the
/// judgment's overlay snapshot and this commit's shallow boundary.
///
/// # Errors
///
/// Refuses overlays, a reached shallow boundary, a missing object or a non-commit.
pub(super) fn recorded_first_parent(
    repo: &Repository,
    odb: &git2::Odb<'_>,
    commit: Oid,
) -> Result<Option<Oid>, String> {
    complete(
        boundary(repo).map_err(|error| error.to_string())?.as_ref(),
        commit,
    )
    .map_err(|error| error.to_string())?;
    let object = odb.read(commit).map_err(|error| {
        format!(
            "commit {commit} is not in this clone ({}); fetch the default target's full history, then retry (SPC-013 R-120)",
            error.message()
        )
    })?;
    if object.kind() != git2::ObjectType::Commit {
        return Err(format!("{commit} is not a commit"));
    }
    for line in object.data().split(|byte| *byte == b'\n') {
        if line.is_empty() {
            break;
        }
        if let Some(parent) = line.strip_prefix(b"parent ") {
            let hex = std::str::from_utf8(parent)
                .map_err(|_| format!("commit {commit} records a malformed parent"))?;
            let parent = Oid::from_str(hex)
                .map_err(|_| format!("commit {commit} records a malformed parent"))?;
            return Ok(Some(parent));
        }
    }
    Ok(None)
}

/// Compute a merge only after its ancestry has been verified.
///
/// # Errors
/// Refuses overlays, incomplete ancestry or an unreadable merge input.
pub fn merge_commits(
    repo: &Repository,
    ours: &Commit<'_>,
    theirs: &Commit<'_>,
) -> Result<git2::Index, Error> {
    match merge_base(repo, ours.id(), theirs.id()) {
        Ok(_) => {}
        Err(error) if error.code() == ErrorCode::NotFound => {}
        Err(error) => return Err(error),
    }
    check(repo)?;
    repo.merge_commits(ours, theirs, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_judgment_reads_metadata_once_and_the_next_judgment_refreshes_it() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(dir.path(), &[(b"source.rs", b"source")]);
        let commit = repo.find_commit(head).unwrap();
        SNAPSHOT_READS.with(|reads| reads.set(0));
        {
            let _judgment = judgment();
            for _ in 0..100 {
                let _nested = judgment();
                assert!(parents(&repo, &commit).unwrap().is_empty());
                check(&repo).unwrap();
                assert!(boundary(&repo).unwrap().is_empty());
            }
            SNAPSHOT_READS.with(|reads| assert_eq!(reads.get(), 1));
        }
        std::fs::create_dir_all(repo.path().join("info")).unwrap();
        std::fs::write(repo.path().join("info/grafts"), format!("{head}\n")).unwrap();
        let _judgment = judgment();
        assert!(check(&repo)
            .unwrap_err()
            .message()
            .contains("cannot verify"));
        assert!(parents(&repo, &commit).is_err());
        SNAPSHOT_READS.with(|reads| assert_eq!(reads.get(), 2));
        std::fs::remove_file(repo.path().join("info/grafts")).unwrap();
        refresh(&repo);
        check(&repo).unwrap();
        SNAPSHOT_READS.with(|reads| assert_eq!(reads.get(), 3));
    }

    #[test]
    fn a_judgment_keeps_repository_boundaries_separate() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let (first_repo, first_head) = crate::git::repo_with_tree(first.path(), &[]);
        let (second_repo, second_head) = crate::git::repo_with_tree(second.path(), &[]);
        std::fs::write(
            second_repo.path().join("shallow"),
            format!("{second_head}\n"),
        )
        .unwrap();
        let second_repo = Repository::open(second.path()).unwrap();
        let _judgment = judgment();
        assert!(parents(&first_repo, &first_repo.find_commit(first_head).unwrap()).is_ok());
        assert!(
            parents(&second_repo, &second_repo.find_commit(second_head).unwrap())
                .unwrap_err()
                .message()
                .contains("cannot verify")
        );
    }
}
