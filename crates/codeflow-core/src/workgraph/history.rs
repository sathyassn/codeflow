//! Guarded parent and ancestry reads for local judgments (TSK-287).
//!
//! Every read refuses grafts and replace refs. Walks refuse a shallow
//! boundary when they reach it; a boundary hidden by the range's base does
//! not truncate that range. Errors retain the caller's ordinary refusal path.
use std::collections::HashSet;

use git2::{Commit, Error, ErrorCode, Oid, Repository, Sort};

fn unverified(why: impl std::fmt::Display) -> Error {
    Error::from_str(&format!("cannot verify history: {why}"))
}

/// Refuse a local history overlay before reading any parent or ancestry.
///
/// # Errors
/// Returns `cannot verify` for a graft, replacement or unreadable overlay.
pub fn check(repo: &Repository) -> Result<(), Error> {
    match super::release_line::history_overlay(repo).map_err(unverified)? {
        None => Ok(()),
        Some(overlay) => Err(unverified(format!(
            "this clone overlays its recorded history with {overlay}; remove it or judge from a clean clone"
        ))),
    }
}

fn boundary(repo: &Repository) -> Result<HashSet<Oid>, Error> {
    super::release_line::shallow_boundary(repo).map_err(unverified)
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
    check(repo)?;
    complete(&boundary(repo)?, commit.id())?;
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
    boundary: HashSet<Oid>,
}

/// Begin a guarded walk. Push and hide retain libgit2's range semantics.
///
/// # Errors
/// Refuses overlays or an unreadable shallow boundary.
pub fn revwalk(repo: &Repository) -> Result<Walk<'_>, Error> {
    check(repo)?;
    Ok(Walk {
        inner: repo.revwalk()?,
        boundary: boundary(repo)?,
    })
}

impl Walk<'_> {
    /// Add a starting commit.
    /// # Errors
    /// Returns a Git error if the commit cannot be read.
    pub fn push(&mut self, oid: Oid) -> Result<(), Error> {
        self.inner.push(oid)
    }
    /// Exclude a commit and its ancestors.
    /// # Errors
    /// Returns a Git error if the commit cannot be read.
    pub fn hide(&mut self, oid: Oid) -> Result<(), Error> {
        self.inner.hide(oid)
    }
    /// Select the traversal order.
    /// # Errors
    /// Returns a Git error when sorting cannot be set.
    pub fn set_sorting(&mut self, sorting: Sort) -> Result<(), Error> {
        self.inner.set_sorting(sorting)
    }
    /// Follow only first parents.
    /// # Errors
    /// Returns a Git error when the walk cannot be configured.
    pub fn simplify_first_parent(&mut self) -> Result<(), Error> {
        self.inner.simplify_first_parent()
    }
}

impl Iterator for Walk<'_> {
    type Item = Result<Oid, Error>;
    fn next(&mut self) -> Option<Self::Item> {
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

/// Guard a spawned history query whose range is supplied as Git arguments.
/// These callers require complete history; they cannot distinguish a cut
/// from the root of an unrelated line.
///
/// # Errors
/// Refuses overlays, shallow clones or unreadable repository metadata.
pub fn check_command(root: &std::path::Path) -> Result<(), String> {
    let repo = Repository::discover(root).map_err(|error| unverified(error).to_string())?;
    check(&repo).map_err(|error| error.to_string())?;
    if let Some(file) = std::env::var_os("GIT_SHALLOW_FILE") {
        let file = root.join(file);
        let bytes = std::fs::read(&file).map_err(|error| {
            unverified(format!(
                "cannot read the shallow file {}: {error}",
                file.display()
            ))
            .to_string()
        })?;
        if !bytes.is_empty() {
            return Err(unverified(format!(
                "Git uses the shallow file {}; fetch the full history",
                file.display()
            ))
            .to_string());
        }
    }
    if !boundary(&repo)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err(unverified("this clone is shallow; fetch its full history").to_string());
    }
    Ok(())
}

/// A commit's first parent as its object records it, read from the object
/// database: grafts, replace refs and shallow boundaries never apply.
///
/// # Errors
///
/// Returns a message when the object is missing or is not a commit.
pub(super) fn recorded_first_parent(
    repo: &Repository,
    odb: &git2::Odb<'_>,
    commit: Oid,
) -> Result<Option<Oid>, String> {
    check(repo).map_err(|error| error.to_string())?;
    complete(&boundary(repo).map_err(|error| error.to_string())?, commit)
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
    repo.merge_commits(ours, theirs, None)
}
