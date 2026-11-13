//! A landing candidate judged as its line's next tip (TSK-1351).
//!
//! A landing candidate is a branch named with the project's
//! `git.candidate_prefix`, cut from an epic line's tip, with the reviewed
//! pull request heads of one batch merged onto it by `--no-ff` merges. The
//! pre-push hook and `codeflow ci` judge it as the line's next tip, and the
//! line push that lands it, only when [`bind`] holds. Its inputs come from
//! the default branch, the destination and the forge, never from the
//! candidate's own claims:
//!
//! - the name carries the prefix read from the default branch's policy;
//! - the candidate's first-parent chain, `git.candidate_search_depth`
//!   commits of it, holds the tip of exactly one epic line, counting every
//!   epic line the destination advertises in the same push and every one
//!   fetched here, at whatever point of the chain each tip lies; that line's
//!   advertised tip and its fetched tip (`refs/remotes/origin/<line>`) are
//!   that same commit;
//! - the line is bound as an epic line by the records at both the default
//!   branch's tip and the line's fetched tip, never by the name shape
//!   `integration/<x>` alone;
//! - every commit after the line's tip on the first-parent chain is a
//!   two-parent merge whose tree is the clean merge of its parents: no
//!   direct commit, no octopus merge and no evil merge;
//! - every member merge's second parent is the exact head of an open pull
//!   request into the line, with the default branch's trusted admission
//!   checks green there, each written by the policy workflow's own
//!   `pull_request_target` run for that pull request, read from the forge
//!   ([`crate::workgraph::candidate_admission`]).
//!
//! Any condition that does not hold, or cannot be read, is the cause the
//! caller names in its refusal. The member binding proves admission at each
//! head, the hosted verdicts and record checks run there, not that a person
//! reviewed it. A change enters the candidate only through a member's head.

use std::collections::BTreeMap;
use std::path::Path;

use git2::{Oid, Repository};

use super::acceptance::{read_merge, MergeReading};
use super::candidate_admission::{Admitted, Member};
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
    /// Each member head, oldest merge first, with the open pull request
    /// into the line whose admitted head it is.
    pub members: Vec<Admitted>,
}

/// Binds the member heads of a candidate on `line` to admitted pull
/// request heads: [`crate::workgraph::candidate_admission::admit_at_forge`] in the hook and
/// `codeflow ci`, a recorded forge in tests.
pub type Admission<'a> = &'a dyn Fn(&str, &[Member]) -> Result<Vec<Admitted>, String>;

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
    /// name. Always required: the destination's line inventory decides
    /// which line the candidate is cut from, and a fetched tip alone never
    /// does.
    pub advertised: &'a BTreeMap<String, String>,
    /// The line the caller already knows the candidate must land on (the
    /// line a push updates); `None` to find it from the first-parent chain.
    pub line: Option<&'a str>,
    /// Binds each member head to an admitted pull request head (AC-1).
    pub admission: Admission<'a>,
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
    let depth = policy.candidate_search_depth;
    let chain = first_parent_chain(&repo, head, depth)?;
    let fetched = fetched_lines(&repo)?;
    let (line, at) = line_on_chain(&chain, &fetched, inputs.advertised, depth)?;
    let tip = chain[at];
    let chain: Vec<Oid> = chain[..at].to_vec();
    if let Some(expected) = inputs.line {
        if line != expected {
            return Err(format!(
                "its first-parent chain reaches the tip of '{}', not of '{}', the line this push updates",
                crate::git::display_text(&line),
                crate::git::display_text(expected)
            ));
        }
    }
    let tip_text = tip.to_string();
    let shown = crate::git::display_text(&line);
    // The line's tip is confirmed by both reads: the destination's
    // advertisement in this push and this clone's fetched ref.
    let fetched_tip = fetched
        .iter()
        .find(|(name, _)| name.as_deref() == Some(line.as_str()))
        .map(|(_, oid)| *oid);
    match (inputs.advertised.get(&line), fetched_tip) {
        (None, _) => {
            return Err(format!(
                "the destination does not advertise '{shown}', so its fetched tip {tip:.9} cannot be confirmed as the line's tip"
            ))
        }
        (Some(advertised), _) if !advertised.eq_ignore_ascii_case(&tip_text) => {
            return Err(format!(
                "it is cut from 'origin/{shown}' at {tip:.9}, but the destination advertises '{shown}' at {}: the fetched tip is stale, so fetch the line and rebuild the candidate on its tip",
                advertised.get(..9).unwrap_or(advertised)
            ))
        }
        (Some(_), None) => {
            return Err(format!(
                "'{shown}' is not fetched here ('origin/{shown}' is missing), so the records at its tip {tip:.9} cannot be read: fetch the line"
            ))
        }
        (Some(_), Some(fetched)) if fetched != tip => {
            return Err(format!(
                "the destination advertises '{shown}' at {tip:.9}, but 'origin/{shown}' is at {fetched:.9}: fetch the line and rebuild the candidate on its tip"
            ))
        }
        (Some(_), Some(_)) => {}
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
    let members = member_merges(&repo, &chain, tip)?;
    // Last, since it reads the forge: each member head is the exact head of
    // an admitted pull request into the line (AC-1).
    let members = (inputs.admission)(&line, &members)?;
    Ok(BoundLine {
        line,
        tip: tip_text,
        merges: chain.len(),
        members,
    })
}

/// The member merges on `chain`, the candidate's first-parent commits after
/// the line's `tip`, oldest first: each must be a two-parent merge whose
/// tree is the clean merge of its parents, and its second parent is the
/// member head; the cause when one is not.
fn member_merges(repo: &Repository, chain: &[Oid], tip: Oid) -> Result<Vec<Member>, String> {
    let mut members = Vec::new();
    // Oldest merge first, the order the members were merged in.
    for oid in chain.iter().rev() {
        let commit = repo.find_commit(*oid).map_err(|e| e.to_string())?;
        let parents = history::parents(repo, &commit).map_err(|e| e.to_string())?;
        if parents.len() < 2 {
            return Err(format!(
                "{oid:.9} on its first-parent chain after the line's tip {tip:.9} is a direct commit, not a merge of a member head; a candidate holds only --no-ff merges of reviewed heads"
            ));
        }
        if parents.len() > 2 {
            return Err(format!(
                "merge {oid:.9} has {} parents: a candidate merges one member head per merge",
                parents.len()
            ));
        }
        members.push(Member {
            merge: oid.to_string(),
            head: parents[1].to_string(),
        });
        match read_merge(repo, &commit) {
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
    Ok(members)
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

/// `head`'s first-parent chain, newest first: `head` and at most `depth`
/// commits before it, ending early at a root commit.
fn first_parent_chain(repo: &Repository, head: Oid, depth: u32) -> Result<Vec<Oid>, String> {
    let mut chain = vec![head];
    let mut at = head;
    for _ in 0..depth {
        let commit = repo.find_commit(at).map_err(|e| e.to_string())?;
        match history::parents(repo, &commit)
            .map_err(|e| e.to_string())?
            .first()
        {
            Some(first) => {
                at = *first;
                chain.push(at);
            }
            None => break,
        }
    }
    Ok(chain)
}

/// The one epic line whose tip lies on `chain`, and the tip's position in
/// it. The inventory is every epic line the destination advertises and
/// every epic line fetched here, so a missing tracking ref hides no line
/// the destination holds. The whole bounded chain is searched, never only
/// up to the first tip: two lines whose tips lie on the chain, at one
/// commit or at different points, leave the line ambiguous and refuse.
/// Where a line's advertised and fetched tips lie at different points, the
/// advertised one is returned and the caller refuses the mismatch.
fn line_on_chain(
    chain: &[Oid],
    fetched: &[(Option<String>, Oid)],
    advertised: &BTreeMap<String, String>,
    depth: u32,
) -> Result<(String, usize), String> {
    let place = |tip: Oid| chain.iter().position(|at| *at == tip);
    // Each line on the chain, with the position of its advertised tip and
    // of its fetched tip.
    let mut found: BTreeMap<String, (Option<usize>, Option<usize>)> = BTreeMap::new();
    for (name, tip) in advertised {
        if line_adoption::epic_id(name).is_err() {
            continue;
        }
        let tip = Oid::from_str(tip).map_err(|e| {
            format!(
                "the destination advertises '{}' at '{}', which is not a commit ID: {e}",
                crate::git::display_text(name),
                crate::git::display_text(tip)
            )
        })?;
        if let Some(at) = place(tip) {
            found.entry(name.clone()).or_default().0 = Some(at);
        }
    }
    for (name, tip) in fetched {
        let Some(at) = place(*tip) else {
            continue;
        };
        let Some(name) = name else {
            return Err(format!(
                "a fetched line at {:.9} has a name that is not UTF-8",
                chain[at]
            ));
        };
        found.entry(name.clone()).or_default().1 = Some(at);
    }
    let mut lines = found
        .into_iter()
        .filter_map(|(name, (advertised, fetched))| advertised.or(fetched).map(|at| (name, at)));
    match (lines.next(), lines.next()) {
        (None, _) => Err(format!(
            "its first-parent chain reaches no epic line tip ('integration/EPC-*') the destination advertises or this clone fetched, within {depth} commits (git.candidate_search_depth): cut it from a line's fetched tip, or fetch the line it was cut from"
        )),
        (Some(line), None) => Ok(line),
        (Some(first), Some(second)) => {
            let names = [first, second]
                .into_iter()
                .chain(lines)
                .map(|(name, at)| {
                    format!("'{}' at {:.9}", crate::git::display_text(&name), chain[at])
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "its first-parent chain holds the tips of several epic lines ({names}), so the line it lands on is ambiguous"
            ))
        }
    }
}

#[cfg(test)]
#[path = "landing_candidate_tests.rs"]
mod tests;
