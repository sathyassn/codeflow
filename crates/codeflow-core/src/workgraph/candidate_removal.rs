//! `codeflow work remove-candidate`: remove a landing candidate branch whose
//! tree landed on its line by another commit (TSK-1308).
//!
//! A candidate is a merge of reviewed pull request heads that the full gate
//! runs on. When the line gets the same tree through a different commit (the
//! hosted merge of the same members, say), the candidate's own commits are
//! reachable from no other ref, so the ordinary merge proof fails and
//! `git branch -D` is refused by git-guard's `git.discard_uncommitted`. Its
//! tree is the proof instead: the branch tip's tree equals the tree of a
//! commit on the line's first-parent history, read from the commit objects
//! at a freshly fetched tip, so the line holds every file the candidate held.
//!
//! The removal is refused, naming the cause, when the name is not of the
//! project's candidate naming (`git.candidate_prefix`), the branch is
//! protected or is the line, the trees differ, the tree is only on a side
//! branch, the line or the branch cannot be read, a worktree has the branch
//! checked out, or it has an open pull request. A task branch keeps the
//! ordinary merge proof (`.codeflow/rules/worktrees.md` "Cleanup").

use std::collections::HashMap;
use std::path::Path;

use git2::{Oid, Repository};

use super::history;
use crate::git::GitName;

/// The proof of one removal, as it is printed and recorded in the ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    /// The branch removed, without `refs/heads/`.
    pub branch: String,
    /// Its tip when it was removed.
    pub tip: Oid,
    /// The line's remote-tracking ref, as fetched (`origin/<line>`).
    pub line: String,
    /// The line's tip at the fetch.
    pub line_tip: Oid,
    /// The commit on the line's first-parent history with the same tree.
    pub line_commit: Oid,
    /// The tree both hold.
    pub tree: Oid,
}

/// The event type of a removal in the ledger.
pub const EVENT: &str = "candidate_removed";

/// The open pull requests whose head is a branch: their numbers. An error
/// when the host cannot be asked, so the removal is refused.
pub type OpenPullRequests<'a> = &'a dyn Fn(&str) -> Result<Vec<u64>, String>;

/// Remove the local candidate `branch` when its tree landed on `line`
/// (a branch name on `origin`), then record the proof in the ledger.
///
/// # Errors
///
/// Returns the cause when the proof does not hold or cannot be read; the
/// branch is left as it was.
pub fn remove(
    repo_root: &Path,
    branch: &str,
    line: &str,
    open_pull_requests: OpenPullRequests<'_>,
) -> Result<Removed, String> {
    let proof = prove(repo_root, branch, line, open_pull_requests)?;
    // Compare and delete: the ref is removed only while it is still at the
    // tip the proof was made for.
    git(
        repo_root,
        &[
            "update-ref",
            "-d",
            &format!("refs/heads/{branch}"),
            &proof.tip.to_string(),
        ],
    )
    .map_err(|error| format!("`{branch}` was not removed: {error}"))?;
    record(repo_root, &proof).map_err(|error| {
        format!(
            "`{branch}` was removed (tip {}), but the ledger record failed: {error}; the proof is: line {} at {}, commit {}, tree {}",
            proof.tip, proof.line, proof.line_tip, proof.line_commit, proof.tree
        )
    })?;
    Ok(proof)
}

/// Check every condition of a removal without changing anything.
///
/// # Errors
///
/// Returns the cause when a condition fails or cannot be read.
pub fn prove(
    repo_root: &Path,
    branch: &str,
    line: &str,
    open_pull_requests: OpenPullRequests<'_>,
) -> Result<Removed, String> {
    // The naming and depth come from the policy at its recorded authority
    // (the remote's default branch), never from the working copy.
    let landed = crate::hooks::landed_policy::load(repo_root)?;
    let git_policy = &landed.policy.git;
    let prefix = git_policy.candidate_prefix.as_str();
    let depth = git_policy.candidate_search_depth;
    let named = branch
        .strip_prefix(prefix)
        .is_some_and(|rest| !rest.is_empty());
    if prefix.is_empty() || !named {
        return Err(format!(
            "`{branch}` is not a landing candidate branch: its name does not start with `{prefix}` (git.candidate_prefix); a task branch keeps its merge proof (.codeflow/rules/worktrees.md \"Cleanup\")"
        ));
    }
    if depth == 0 {
        return Err("git.candidate_search_depth is 0, so no line commit can be searched".into());
    }
    if branch == line || git_policy.branch_is_protected_name(&GitName::from_text(branch)) {
        return Err(format!(
            "`{branch}` is a protected branch or the line itself, which is never removed"
        ));
    }
    // The line is a ref class the policy defines, never a name the requester
    // picks: a copy of the tree on an ordinary branch can be deleted later.
    let is_line = line.starts_with(crate::hooks::policy::INTEGRATION_BRANCH_PREFIX)
        || git_policy.branch_is_protected_name(&GitName::from_text(line));
    if !is_line {
        return Err(format!(
            "`{line}` is not a line: name a protected branch or an `{}` branch, so the proof holds only while the work is on a branch the policy protects or a line lands through",
            crate::hooks::policy::INTEGRATION_BRANCH_PREFIX
        ));
    }
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let local = format!("refs/heads/{branch}");
    let tip = repo
        .find_reference(&local)
        .and_then(|reference| reference.peel_to_commit())
        .map_err(|error| format!("the branch `{branch}` cannot be read: {error}"))?;
    let checked_out = super::local_branches::checked_out(&repo)
        .map_err(|error| format!("cannot tell whether a worktree uses `{branch}`: {error}"))?;
    if checked_out.contains(local.as_bytes()) {
        return Err(format!(
            "a worktree has `{branch}` checked out; remove that worktree first (git worktree remove, never --force on a dirty one)"
        ));
    }
    git(
        repo_root,
        &[
            "fetch",
            "--quiet",
            "--no-tags",
            "origin",
            &format!("+refs/heads/{line}:refs/remotes/origin/{line}"),
        ],
    )
    .map_err(|error| format!("the line `{line}` cannot be fetched from origin: {error}"))?;
    history::refresh(&repo);
    let _judgment = history::judgment();
    let line_ref = format!("refs/remotes/origin/{line}");
    let line_tip = repo
        .find_reference(&line_ref)
        .and_then(|reference| reference.peel_to_commit())
        .map_err(|error| format!("the line `origin/{line}` cannot be read: {error}"))?;
    history::check(&repo).map_err(|error| format!("the line cannot be read: {error}"))?;
    let tree = tip.tree_id();
    let unread = |error: git2::Error| format!("the line `origin/{line}` cannot be read: {error}");
    let searched = first_parent(&repo, &line_tip, depth).map_err(unread)?;
    let found = searched
        .iter()
        .find(|(_, at)| *at == tree)
        .map(|(commit, _)| *commit);
    let Some(line_commit) = found else {
        // Only to name the cause: the tree deeper on the first-parent
        // history, on a side branch, or nowhere near.
        let side = side_match(&repo, line_tip.id(), tree, depth).map_err(unread)?;
        return Err(match side {
            Some((commit, true)) => format!(
                "the tree {tree} of `{branch}` is on `origin/{line}`'s first-parent history at {commit}, deeper than the {depth} commits git.candidate_search_depth searches"
            ),
            Some((commit, false)) => format!(
                "the tree {tree} of `{branch}` is only on a side branch of `origin/{line}` (commit {commit}), not on its first-parent history, so the line did not land it"
            ),
            None => format!(
                "the trees differ: no commit in the last {depth} of `origin/{line}`'s first-parent history has the tree {tree} of `{branch}` (tip {})",
                tip.id()
            ),
        });
    };
    let open = open_pull_requests(branch).map_err(|error| {
        format!("cannot tell whether `{branch}` has an open pull request: {error}")
    })?;
    if !open.is_empty() {
        let shown: Vec<String> = open.iter().map(|number| format!("#{number}")).collect();
        return Err(format!(
            "`{branch}` has an open pull request ({}); close or land it first",
            shown.join(", ")
        ));
    }
    Ok(Removed {
        branch: branch.to_string(),
        tip: tip.id(),
        line: format!("origin/{line}"),
        line_tip: line_tip.id(),
        line_commit,
        tree,
    })
}

/// The first `depth` commits of `tip`'s first-parent history, newest first,
/// each with its tree, read through the guarded parent reads.
fn first_parent(
    repo: &Repository,
    tip: &git2::Commit<'_>,
    depth: u32,
) -> Result<Vec<(Oid, Oid)>, git2::Error> {
    let mut chain = Vec::new();
    let mut current = tip.clone();
    for _ in 0..depth {
        chain.push((current.id(), current.tree_id()));
        if history::parents(repo, &current)?.is_empty() {
            break;
        }
        current = history::parent(repo, &current, 0)?;
    }
    Ok(chain)
}

/// A commit with `tree` among the newest commits `line_tip` reaches through
/// any parent, with whether it is on the first-parent history; read only
/// to name the cause of a refusal, over four times the searched depth.
fn side_match(
    repo: &Repository,
    line_tip: Oid,
    tree: Oid,
    depth: u32,
) -> Result<Option<(Oid, bool)>, git2::Error> {
    let limit = depth.saturating_mul(4);
    let tip = repo.find_commit(line_tip)?;
    let chain = first_parent(repo, &tip, limit)?;
    if let Some((commit, _)) = chain.iter().find(|(_, at)| *at == tree) {
        return Ok(Some((*commit, true)));
    }
    let mut walk = history::revwalk(repo)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL)?;
    walk.push(line_tip)?;
    for oid in walk.take(usize::try_from(limit).unwrap_or(usize::MAX)) {
        let oid = oid?;
        if repo.find_commit(oid)?.tree_id() == tree {
            return Ok(Some((oid, false)));
        }
    }
    Ok(None)
}

fn record(repo_root: &Path, removed: &Removed) -> Result<(), String> {
    use crate::ledger::LedgerWriter;
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let ledger = repo.commondir().join("codeflow").join("ledger");
    let mut data = HashMap::new();
    data.insert("branch".to_string(), serde_json::json!(removed.branch));
    data.insert(
        "tip".to_string(),
        serde_json::json!(removed.tip.to_string()),
    );
    data.insert("line".to_string(), serde_json::json!(removed.line));
    data.insert(
        "line_tip".to_string(),
        serde_json::json!(removed.line_tip.to_string()),
    );
    data.insert(
        "line_commit".to_string(),
        serde_json::json!(removed.line_commit.to_string()),
    );
    data.insert(
        "tree".to_string(),
        serde_json::json!(removed.tree.to_string()),
    );
    crate::ledger::JsonlWriter::new(ledger)
        .and_then(|writer| {
            writer.append_event(crate::ledger::Event {
                event_type: EVENT.to_string(),
                timestamp: crate::hooks::rfc3339_utc_now(),
                session_id: None,
                worktree: None,
                data,
            })
        })
        .map_err(|error| error.to_string())
}

fn git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let out = history::command()
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if out.status.success() {
        // Strict: the output names commits and trees, compared exactly.
        let text = String::from_utf8(out.stdout).map_err(|error| error.to_string())?;
        Ok(text.strip_suffix('\n').unwrap_or(&text).to_string())
    } else {
        Err(crate::git::remote_query::display_urls(
            String::from_utf8_lossy(&out.stderr).trim(),
        ))
    }
}

#[cfg(test)]
#[path = "candidate_removal_tests.rs"]
mod tests;
