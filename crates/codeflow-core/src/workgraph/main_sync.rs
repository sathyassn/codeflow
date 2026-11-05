//! A sync of the default branch into an integration line (TSK-1312).
//!
//! The route is proven from the history, never claimed by a name: every
//! commit the head brings beyond the line's base and the default branch's
//! trusted commit is a two-parent merge whose second parent is on that
//! commit and whose tree is the clean automatic merge of its parents
//! ([`super::acceptance::MergeReading::Clean`]). Such a range holds nothing
//! but the line's history and the default branch's reviewed history, so no
//! change of its own can ride in it. Anything else is refused, naming the
//! commit and the cause; a history that cannot be read is refused too.

use std::path::Path;

use git2::{Oid, Repository};

use super::acceptance::{read_merge, MergeReading};
use super::history;

/// The merges a proven sync brings, newest first, as full commit ids.
///
/// `trusted` is the default branch's commit as the run's authority names it
/// (the hosted policy workflow passes the default branch's own commit), read
/// by id. A ref named like the default branch proves nothing here.
///
/// # Errors
///
/// The reason the range is not a sync of `trusted`: no merge at all, a
/// commit that is not a two-parent merge, a second parent that is not on
/// `trusted`, a merge whose tree differs from the clean merge of its
/// parents, or a history that cannot be read.
pub fn sync_merges(
    repo_root: &Path,
    base: &str,
    head: &str,
    trusted: &str,
) -> Result<Vec<String>, String> {
    let _judgment = history::judgment();
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let unreadable = |error: git2::Error| format!("cannot read the range: {}", error.message());
    let trusted = oid(trusted)?;
    let mut walk = history::revwalk(&repo).map_err(unreadable)?;
    // Newest first, so a refusal names the commit nearest the head.
    walk.set_sorting(git2::Sort::TOPOLOGICAL)
        .map_err(unreadable)?;
    walk.push(oid(head)?).map_err(unreadable)?;
    walk.hide(oid(base)?).map_err(unreadable)?;
    walk.hide(trusted).map_err(unreadable)?;
    let mut merges = Vec::new();
    for id in walk {
        let commit = repo
            .find_commit(id.map_err(unreadable)?)
            .map_err(unreadable)?;
        let parents = history::parents(&repo, &commit).map_err(unreadable)?;
        let [_, second] = parents.as_slice() else {
            return Err(if parents.len() > 2 {
                format!(
                    "commit {:.9} is an octopus merge; a sync carries only two-parent merges of the default branch",
                    commit.id()
                )
            } else {
                format!(
                    "commit {:.9} is not a merge; a sync carries only merges of the default branch, so other work lands through its own task",
                    commit.id()
                )
            });
        };
        if !on_trusted(&repo, *second, trusted).map_err(unreadable)? {
            return Err(format!(
                "merge {:.9} takes {:.9}, which is not on the default branch's commit {:.9}",
                commit.id(),
                second,
                trusted
            ));
        }
        match read_merge(&repo, &commit) {
            MergeReading::Clean => {}
            MergeReading::Changed(paths) => {
                let named: Vec<String> = paths
                    .iter()
                    .map(|path| crate::git::GitName::from_bytes(path).display().to_string())
                    .collect();
                return Err(format!(
                    "merge {:.9} changes {} beyond the clean merge of its parents; a sync with a resolved conflict or a change of its own lands through a task",
                    commit.id(),
                    named.join(", ")
                ));
            }
            MergeReading::Unknown(reason) => return Err(reason),
        }
        merges.push(commit.id().to_string());
    }
    if merges.is_empty() {
        return Err("the range brings no merge of the default branch".into());
    }
    Ok(merges)
}

/// Whether `commit` is `trusted` or one of its ancestors.
fn on_trusted(repo: &Repository, commit: Oid, trusted: Oid) -> Result<bool, git2::Error> {
    Ok(commit == trusted || history::descendant_of(repo, trusted, commit)?)
}
