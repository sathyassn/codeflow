//! A landing candidate judged as its line's next tip (TSK-1351).
//!
//! A landing candidate is a branch named with the project's
//! `git.candidate_prefix`, cut from an epic line's tip, with the reviewed
//! pull request heads of one batch merged onto it by `--no-ff` merges. The
//! pre-push hook and `codeflow ci` judge it as the line's next tip, and the
//! line push that lands it, only when [`bind`] holds. Every input is one the
//! pusher cannot forge:
//!
//! - the name carries the prefix read from the default branch's policy;
//! - the candidate's first-parent chain reaches the fetched tip
//!   (`refs/remotes/origin/<line>`) of exactly one epic line, within
//!   `git.candidate_search_depth` commits, and that tip is the one the
//!   destination advertises for the line in the same push;
//! - the line is bound as an epic line by the records at both the default
//!   branch's tip and the line's fetched tip, never by the name shape
//!   `integration/<x>` alone;
//! - every commit after the line's tip on the first-parent chain is a
//!   two-parent merge whose tree is the clean merge of its parents: no
//!   direct commit, no octopus merge and no evil merge.
//!
//! Any condition that does not hold, or cannot be read, is the cause the
//! caller names in its refusal. The hook cannot prove offline that a member
//! head was reviewed: a clean merge of an unreviewed branch is admitted, the
//! same trust as a push to the line itself. A change enters the candidate
//! only through a member's head.

use std::collections::BTreeMap;
use std::path::Path;

use git2::{Oid, Repository};

use super::acceptance::{read_merge, MergeReading};
use super::history;
use super::line_adoption;
use super::work_start::{epic_line_bound_at, EpicLineBinding};
use crate::hooks::policy::GitPolicy;

/// The rule a refused landing candidate is reported under.
pub const RULE: &str = "work.landing_candidate";

/// How to clear a [`RULE`] refusal: the one route the pre-push hook and
/// `codeflow ci` both name.
pub const REMEDY: &str = "cut the candidate from the line's fetched tip (`git fetch origin`, then `git switch -C <candidate> origin/<line>`) and merge each reviewed pull request head onto it with `git merge --no-ff`, adding no other commit; see git-rules.md, \"Bodies of work\"";

/// The line a landing candidate is bound to by [`bind`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundLine {
    /// The epic line, `integration/EPC-NNN-<slug>`.
    pub line: String,
    /// The line's fetched tip, the commit the candidate was cut from.
    pub tip: String,
    /// How many member merges the candidate adds to the tip.
    pub merges: usize,
}

/// What [`bind`] reads besides the repository.
pub struct Inputs<'a> {
    /// The candidate's branch name, without `refs/heads/`.
    pub branch: &'a str,
    /// The candidate's head commit.
    pub head: &'a str,
    /// The policy at the default branch's tip, which names the prefix and
    /// the search depth.
    pub policy: &'a GitPolicy,
    /// The default branch's tip, whose records must bind the line.
    pub default_tip: &'a str,
    /// The branch tips the destination advertises in this push, by branch
    /// name; `None` when the run holds no advertisement (a run outside the
    /// pre-push hand-off), which then trusts the fetched tip alone.
    pub advertised: Option<&'a BTreeMap<String, String>>,
    /// The line the caller already knows the candidate must land on (the
    /// line a push updates); `None` to find it from the first-parent chain.
    pub line: Option<&'a str>,
}

/// Whether `branch` carries the landing candidate naming of `policy`. A
/// prefix that overlaps another branch name, does not end in `/` or holds
/// a glob character names no candidate at all.
#[must_use]
pub fn is_candidate_name(policy: &GitPolicy, branch: &str) -> bool {
    policy.is_candidate_branch(branch)
}

/// Bind the candidate `inputs` names to its line, or say why it is not a
/// landing candidate of a line.
///
/// # Errors
/// Returns the cause: a condition that does not hold, or an input that
/// cannot be read. The caller refuses with it, and never falls back to
/// judging the branch against the default branch.
pub fn bind(root: &Path, inputs: &Inputs<'_>) -> Result<BoundLine, String> {
    let _judgment = history::judgment();
    let policy = inputs.policy;
    if let Some(problem) = policy.candidate_prefix_problem() {
        return Err(problem);
    }
    if !is_candidate_name(policy, inputs.branch) {
        return Err(format!(
            "its name does not start with `{}` (git.candidate_prefix at the default branch)",
            policy.candidate_prefix
        ));
    }
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let resolve = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|e| format!("cannot read {revision}: {e}"))
    };
    let head = resolve(inputs.head)?;
    let lines = fetched_lines(&repo)?;
    let (line, tip, chain) = walk(&repo, head, &lines, policy.candidate_search_depth)?;
    if let Some(expected) = inputs.line {
        if line != expected {
            return Err(format!(
                "its first-parent chain reaches the fetched tip of '{}', not of '{}', the line this push updates",
                crate::git::display_text(&line),
                crate::git::display_text(expected)
            ));
        }
    }
    let tip_text = tip.to_string();
    let shown = crate::git::display_text(&line);
    if let Some(advertised) = inputs.advertised {
        match advertised.get(&line) {
            None => {
                return Err(format!(
                    "the destination does not advertise '{shown}', so its fetched tip {tip:.9} cannot be confirmed as the line's tip"
                ))
            }
            Some(at) if !at.eq_ignore_ascii_case(&tip_text) => {
                return Err(format!(
                    "it is cut from 'origin/{shown}' at {tip:.9}, but the destination advertises '{shown}' at {}: the fetched tip is stale, so fetch the line and rebuild the candidate on its tip",
                    at.get(..9).unwrap_or(at)
                ))
            }
            Some(_) => {}
        }
    }
    for (anchor, place) in [
        (inputs.default_tip, "the default branch".to_string()),
        (tip_text.as_str(), format!("'origin/{shown}'")),
    ] {
        match epic_line_bound_at(root, &line, anchor, &place).map_err(|e| e.to_string())? {
            EpicLineBinding::Bound => {}
            EpicLineBinding::Refused(reason) | EpicLineBinding::Unbound(reason) => {
                return Err(format!(
                    "'{shown}' {reason}, so it is not a bound epic line"
                ));
            }
        }
    }
    for oid in &chain {
        let commit = repo.find_commit(*oid).map_err(|e| e.to_string())?;
        let parents = history::parents(&repo, &commit).map_err(|e| e.to_string())?;
        if parents.len() < 2 {
            return Err(format!(
                "{oid:.9} on its first-parent chain after the line's tip {tip:.9} is a direct commit, not a merge of a member head; a candidate holds only --no-ff merges of reviewed heads"
            ));
        }
        match read_merge(&repo, &commit) {
            MergeReading::Clean => {}
            MergeReading::Changed(paths) => {
                let names = paths
                    .iter()
                    .take(5)
                    .map(|path| crate::git::GitName::from_bytes(path).display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "merge {oid:.9} is not the clean merge of its parents (it changes {names}): an evil or hand-resolved merge lands work no member head carries"
                ));
            }
            MergeReading::Unknown(reason) => return Err(reason),
        }
    }
    Ok(BoundLine {
        line,
        tip: tip_text,
        merges: chain.len(),
    })
}

/// Every fetched epic line, `refs/remotes/origin/integration/EPC-*`, and its
/// tip. A fetched line whose name is not text is kept as `None`: one at a
/// commit the walk reaches refuses rather than being skipped.
fn fetched_lines(repo: &Repository) -> Result<Vec<(Option<String>, Oid)>, String> {
    let mut lines = Vec::new();
    let references = repo
        .references_glob("refs/remotes/origin/integration/*")
        .map_err(|e| format!("cannot list the fetched epic lines: {e}"))?;
    for reference in references {
        let reference = reference.map_err(|e| format!("cannot read a fetched epic line: {e}"))?;
        let full = crate::git::GitName::from_bytes(reference.name_bytes());
        let name = match full.rule_text() {
            Ok(text) => match text.strip_prefix("refs/remotes/origin/") {
                Some(name) if line_adoption::epic_id(name).is_ok() => Some(name.to_string()),
                _ => continue,
            },
            Err(_) => None,
        };
        let tip = reference
            .peel_to_commit()
            .map_err(|e| format!("cannot read the tip of '{}': {e}", full.display()))?;
        lines.push((name, tip.id()));
    }
    Ok(lines)
}

/// Walk `head`'s first-parent chain, `depth` commits at most, to the first
/// commit that is a fetched epic line's tip: that line, its tip, and the
/// chain's commits after the tip, newest first.
fn walk(
    repo: &Repository,
    head: Oid,
    lines: &[(Option<String>, Oid)],
    depth: u32,
) -> Result<(String, Oid, Vec<Oid>), String> {
    let mut chain = Vec::new();
    let mut at = head;
    for _ in 0..=depth {
        let here: Vec<&Option<String>> = lines
            .iter()
            .filter(|(_, tip)| *tip == at)
            .map(|(name, _)| name)
            .collect();
        match here.as_slice() {
            [] => {}
            [Some(name)] => return Ok((name.clone(), at, chain)),
            [None] => {
                return Err(format!(
                    "a fetched line at {at:.9} has a name that is not UTF-8"
                ))
            }
            several => {
                let names = several
                    .iter()
                    .map(|name| name.as_deref().unwrap_or("<not UTF-8>"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "its first-parent chain reaches {at:.9}, the fetched tip of several epic lines ({names}), so the line it lands on is ambiguous"
                ));
            }
        }
        chain.push(at);
        let commit = repo.find_commit(at).map_err(|e| e.to_string())?;
        match history::parents(repo, &commit)
            .map_err(|e| e.to_string())?
            .first()
        {
            Some(first) => at = *first,
            None => break,
        }
    }
    Err(format!(
        "its first-parent chain reaches no fetched epic line tip ('origin/integration/EPC-*') within {depth} commits (git.candidate_search_depth): cut it from a line's fetched tip, or fetch the line it was cut from"
    ))
}

#[cfg(test)]
#[path = "landing_candidate_tests.rs"]
mod tests;
