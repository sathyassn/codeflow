//! A pushed landing candidate judged as its line's next tip (TSK-1351).
//!
//! The pre-push hook binds a branch named with `git.candidate_prefix` to
//! its line and hands `codeflow ci` the line's tip as `--base` and the line
//! as `--into`. This check never trusts that hand-off: it binds the pushed
//! branch again from its own reads, the fetched line tip, the records at
//! the default branch's tip and at that line tip, and its own read of the
//! destination, which must be `origin`'s push URL. The advertisement the
//! hand-off carries is required and is a claim: it must match that read
//! ref for ref. It refuses with the cause when the binding does not hold,
//! when the destination cannot be read or is not `origin`, when the two
//! reads differ (the destination moved, or the hand-off is not its), or
//! when a claim of the hand-off (the policy authority, `--into`, the base)
//! is missing or differs from what it established. A bound candidate is then judged as
//! a push of its line would be: the line's records into the line's own
//! target, the line's direct-commit rule, and each member merge as a
//! landing on the line.

use std::collections::BTreeMap;
use std::path::Path;

use codeflow_core::hooks::{GitPolicy, Violation};
use codeflow_core::workgraph::candidate_admission::{self, Member};
use codeflow_core::workgraph::landing_candidate::{self, BoundLine, Inputs};

/// A pushed landing candidate bound to its line.
pub(super) struct Bound {
    /// The line and the tip the candidate is cut from.
    pub line: BoundLine,
    /// The line's own target, the destination's default branch: a push of
    /// the line is judged into it, so the candidate is too.
    pub target: String,
}

/// The landing candidate the push hand-off names, bound to its line, or
/// `None` when the pushed ref is not named as a candidate, or when the
/// binding is refused (reported in `tagged`).
#[allow(clippy::too_many_arguments)] // The run's shared state, passed once.
pub(super) fn check(
    root: &Path,
    pushed_ref: Option<&str>,
    git: &GitPolicy,
    claims: (Option<&str>, Option<&str>),
    (base, head): (Option<&str>, &str),
    (destination, advertisement): (Option<&str>, Option<&str>),
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) -> Option<Bound> {
    let branch = pushed_ref?.strip_prefix("refs/heads/")?;
    if !git.is_candidate_branch(branch) {
        return None;
    }
    ran.push("landing-candidate");
    let shown = codeflow_core::git::display_text(branch);
    match bind(
        root,
        branch,
        git,
        claims,
        (base, head),
        (destination, advertisement),
    ) {
        Ok(bound) => {
            let pulls = bound
                .line
                .members
                .iter()
                .map(|member| format!("#{}", member.pull))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "codeflow ci: landing candidate: '{shown}' is bound to '{}' at {}, {} member merge(s): judged as the line's next tip, as a push of the line into '{}' is",
                codeflow_core::git::display_text(&bound.line.line),
                &bound.line.tip[..bound.line.tip.len().min(9)],
                bound.line.merges,
                codeflow_core::git::display_text(&bound.target)
            );
            println!(
                "codeflow ci: landing candidate: each member is the admitted head of its open pull request ({pulls}): admission at the head, not a person's review"
            );
            Some(bound)
        }
        Err(cause) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: Violation::always_blocking(
                    landing_candidate::RULE,
                    format!("'{shown}' is not judged as a landing candidate of a line: {cause}"),
                    landing_candidate::REMEDY,
                ),
            });
            None
        }
    }
}

/// The refusal of a hand-off that names a landing candidate with
/// `--pushed-ref` but no line with `--into`, made before the run resolves
/// its target, so the cause is named however the base resolves (a base
/// that several branches name would otherwise stop the run first). The
/// candidate name is read under the working copy's policy: this only
/// refuses sooner and never admits anything; [`check`] still binds under
/// the default branch's policy and refuses a missing `--into` there too.
pub(super) fn unnamed_line(
    pushed_ref: Option<&str>,
    git: &GitPolicy,
    into: Option<&str>,
) -> Option<super::TaggedViolation> {
    let branch = pushed_ref?.strip_prefix("refs/heads/")?;
    if into.is_some() || !git.is_candidate_branch(branch) {
        return None;
    }
    let shown = codeflow_core::git::display_text(branch);
    Some(super::TaggedViolation {
        sha: None,
        violation: Violation::always_blocking(
            landing_candidate::RULE,
            format!(
                "'{shown}' is not judged as a landing candidate of a line: the hand-off names no line (`--into`), so the line it lands on cannot be compared with the line the candidate binds to"
            ),
            landing_candidate::REMEDY,
        ),
    })
}

/// Bind `branch` from this run's own reads; the cause when it does not
/// hold or cannot be read. Every destination tip comes from this run's own
/// read of the destination, which must be `origin`'s push URL; the
/// advertisement the hand-off carries is a claim that read must match ref
/// for ref. Each other claim the hand-off makes (the policy authority,
/// `--into`, the base) must match what this run establishes, and a missing
/// claim is refused, never filled in.
fn bind(
    root: &Path,
    branch: &str,
    git: &GitPolicy,
    (authority, into): (Option<&str>, Option<&str>),
    (base, head): (Option<&str>, &str),
    (destination, advertisement): (Option<&str>, Option<&str>),
) -> Result<Bound, String> {
    let (Some(url), Some(listed)) = (destination, advertisement) else {
        return Err("the run holds no destination advertisement (`--destination` with `--advertisement-stdin`, as the pre-push hook hands over), so the line's tip and the default branch's tip cannot be established independently".to_string());
    };
    let asked = destination_read(root, git, url, listed)?;
    // The default branch and its tip, as the destination advertises them:
    // the line's own target, and the records that must bind the line.
    let (target, default_tip) = asked
        .default
        .as_ref()
        .map(|(name, tip)| (name.clone(), tip.to_string()))
        .ok_or("the destination advertises no default branch, so the line's binding cannot be read at its tip")?;
    // The policy authority the hand-off names must be that advertised tip.
    let authority = authority.ok_or("the hand-off names no policy authority (`--policy-from`), so the default branch's tip it judged under cannot be compared with the destination's")?;
    let named = super::rev_parse(root, authority)?.unwrap_or_else(|| authority.to_string());
    if !named.eq_ignore_ascii_case(&default_tip) {
        return Err(format!(
            "the hand-off's policy authority {} is not the tip of '{}' the destination advertises, {}",
            &named[..named.len().min(9)],
            codeflow_core::git::display_text(&target),
            &default_tip[..default_tip.len().min(9)]
        ));
    }
    let advertised: BTreeMap<String, String> = asked
        .heads
        .iter()
        .map(|(name, tip)| (name.clone(), tip.to_string()))
        .collect();
    // Each member head is bound again to its admitted pull request, read
    // from the forge by this run (AC-1).
    let admission = |line: &str, members: &[Member]| {
        candidate_admission::admit_at_forge(root, url, line, &default_tip, members)
    };
    let line = landing_candidate::bind(
        root,
        &Inputs {
            branch,
            head,
            policy: git,
            default_tip: &default_tip,
            advertised: &advertised,
            line: None,
            admission: &admission,
        },
    )?;
    // The line the hand-off names with `--into` must be the bound line.
    let shown = codeflow_core::git::display_text(&line.line);
    match into {
        None => {
            return Err(format!(
                "the hand-off names no line (`--into`), so it cannot be compared with '{shown}', the line the candidate is bound to"
            ))
        }
        Some(into) if into != line.line => {
            return Err(format!(
                "the hand-off names '{}' with `--into`, but the candidate is bound to '{shown}'",
                codeflow_core::git::display_text(into)
            ))
        }
        Some(_) => {}
    }
    let base = base.ok_or("the range has no base, so it cannot start at the line's tip")?;
    let base = super::rev_parse(root, base)?.unwrap_or_else(|| base.to_string());
    if !base.eq_ignore_ascii_case(&line.tip) {
        return Err(format!(
            "the range's base {} is not the line's fetched tip {}, so the push is not judged as the line's next tip",
            &base[..base.len().min(9)],
            &line.tip[..line.tip.len().min(9)]
        ));
    }
    Ok(Bound { line, target })
}

/// The destination `url` as this run reads it, once `url` is shown to be
/// `origin`'s push URL and the hand-off's advertisement `listed` is shown
/// to match the read ref for ref. The read uses the timeout and attempts
/// of the policy that judges the push, as the hook's own read does.
///
/// # Errors
///
/// Returns why `url` is not `origin`'s push URL, why the destination or
/// the hand-off's advertisement cannot be read, or which refs differ.
fn destination_read(
    root: &Path,
    git: &GitPolicy,
    url: &str,
    listed: &str,
) -> Result<codeflow_core::workgraph::release_line::Destination, String> {
    use codeflow_core::git::remote_query::{display_urls, QueryLimits};
    use codeflow_core::workgraph::release_line;
    let shown = display_urls(url);
    let limits = QueryLimits::from_git(git);
    let pushes = codeflow_core::git::remote_identity::effective_urls(root, "origin", true, limits)
        .map_err(|why| {
            format!(
                "`origin`'s push URL cannot be read ({}), so the destination {shown} cannot be shown to be the repository this run reads",
                display_urls(&why)
            )
        })?;
    if !pushes.iter().any(|push| push == url) {
        return Err(format!(
            "the destination {shown} is not `origin`'s push URL, so this run cannot read it as the destination; push to `origin`"
        ));
    }
    let claimed = release_line::from_advertisement(url, listed).map_err(|error| {
        format!("the hand-off's advertisement of the destination cannot be read: {error}")
    })?;
    let read = release_line::advertisement(root, url, limits)
        .and_then(|text| release_line::from_advertisement(url, &text))
        .map_err(|error| {
            format!("this run cannot read the destination itself, so the hand-off's advertisement cannot be checked: {error}")
        })?;
    let mut moved: Vec<String> = Vec::new();
    if claimed.default.as_ref().map(|(name, _)| name) != read.default.as_ref().map(|(name, _)| name)
    {
        moved.push("HEAD".to_string());
    }
    let tips = |destination: &release_line::Destination| -> BTreeMap<String, String> {
        destination
            .heads
            .iter()
            .map(|(name, tip)| (name.clone(), tip.to_string()))
            .collect()
    };
    let (claimed_tips, read_tips) = (tips(&claimed), tips(&read));
    for name in claimed_tips.keys().chain(read_tips.keys()) {
        let shown_name = codeflow_core::git::display_text(name);
        if claimed_tips.get(name) != read_tips.get(name) && !moved.contains(&shown_name) {
            moved.push(shown_name);
        }
    }
    if !moved.is_empty() {
        let count = moved.len();
        moved.truncate(5);
        return Err(format!(
            "the advertisement the hand-off carries differs from this run's own read of the destination at {count} ref(s) ({}{}): the destination moved after the hook read it, or the hand-off is not the destination's, so the candidate is bound to neither read; fetch, rebuild the candidate on its line's current tip if the line moved, and push again",
            moved.join(", "),
            if count > 5 { ", ..." } else { "" }
        ));
    }
    Ok(read)
}
