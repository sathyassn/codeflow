//! A pushed landing candidate judged as its line's next tip (TSK-1351).
//!
//! The pre-push hook binds a branch named with `git.candidate_prefix` to
//! its line and hands `codeflow ci` the line's tip as `--base` and the line
//! as `--into`. This check never trusts that hand-off: it binds the pushed
//! branch again from its own reads, the fetched line tip, the records at
//! the default branch's tip and at that line tip, and its own read of the
//! destination, which must be `origin`'s push URL. On the hook's path the
//! policy at the default branch tip of that read decides whether the
//! pushed ref is a candidate, never the policy the caller names. The
//! advertisement the hand-off carries is required and is a claim: it must
//! match that read at HEAD's target and every branch tip. It refuses with
//! the cause when the binding does not hold, when the destination cannot be read or is not `origin`, when the two
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
    let shown = codeflow_core::git::display_text(branch);
    let refuse = |tagged: &mut Vec<super::TaggedViolation>, cause: String| {
        tagged.push(super::TaggedViolation {
            sha: None,
            violation: Violation::always_blocking(
                landing_candidate::RULE,
                format!("'{shown}' is not judged as a landing candidate of a line: {cause}"),
                landing_candidate::REMEDY,
            ),
        });
    };
    // On the pre-push hook's path (a pushed ref with the destination's
    // advertisement), whether the ref is a candidate at all is decided by
    // the policy at the default branch's tip as this run reads the
    // destination, never by the policy the caller names: a `--policy-from`
    // with another prefix would otherwise skip the binding. The caller's
    // authority is compared with that tip in [`bind`]. The hook's push asks
    // the destination once for every other ref, so this run reads it again
    // only for a ref that is a candidate under the caller's policy or under
    // the policy at the default tip the hand-off claims, or whose claimed
    // tip this clone cannot read; a ref that is a candidate under neither
    // is judged as the ordinary hand-off it claims to be.
    let at_destination: codeflow_core::hooks::policy::Policy;
    let (git, read) = if let (Some(url), Some(listed)) = (destination, advertisement) {
        let claimed = codeflow_core::workgraph::release_line::from_advertisement(url, listed)
            .ok()
            .and_then(|claimed| claimed.default)
            .map(|(_, tip)| {
                policy_at(
                    root,
                    "the default branch the hand-off claims",
                    &tip.to_string(),
                )
            });
        let claimed_candidate = match claimed {
            Some(Ok(policy)) => Some(policy.git.is_candidate_branch(branch)),
            Some(Err(_)) | None => None,
        };
        if !git.is_candidate_branch(branch) && claimed_candidate == Some(false) {
            return None;
        }
        match destination_policy(root, git, url) {
            Ok((read, policy)) => {
                // Once read, the destination is compared with the hand-off
                // before any return, whatever its policy says about this
                // ref: a destination that moved after the hook's read, even
                // to a policy that no longer names candidates this way, is
                // refused, never judged as an ordinary hand-off.
                let compared = matching_claim(root, &policy.git, url, listed, &read)
                    .and_then(|()| authority_is_tip(root, claims.0, &read))
                    .and_then(|()| {
                        if claimed_candidate == Some(true) && !policy.git.is_candidate_branch(branch)
                        {
                            Err("the policy at the destination's default branch tip no longer names it a candidate, as the policy the hand-off was judged under does: the designation changed after the hook's read; fetch and push again".to_string())
                        } else {
                            Ok(())
                        }
                    });
                if let Err(cause) = compared {
                    ran.push("landing-candidate");
                    refuse(tagged, cause);
                    return None;
                }
                at_destination = policy;
                (&at_destination.git, Some(read))
            }
            Err(cause) => {
                ran.push("landing-candidate");
                refuse(
                    tagged,
                    format!("whether it is one is decided by the policy at the destination's default branch tip, and {cause}"),
                );
                return None;
            }
        }
    } else {
        (git, None)
    };
    if !git.is_candidate_branch(branch) {
        return None;
    }
    ran.push("landing-candidate");
    match bind(
        root,
        branch,
        git,
        claims,
        (base, head),
        (destination, advertisement, read.as_ref()),
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
            refuse(tagged, cause);
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
/// at HEAD's target and every branch tip; `git` is the policy at the
/// default branch tip of that read. Each other claim the hand-off makes
/// (the policy authority, `--into`, the base) must match what this run
/// establishes, and a missing claim is refused, never filled in.
fn bind(
    root: &Path,
    branch: &str,
    git: &GitPolicy,
    (authority, into): (Option<&str>, Option<&str>),
    (base, head): (Option<&str>, &str),
    (destination, advertisement, read): (
        Option<&str>,
        Option<&str>,
        Option<&codeflow_core::workgraph::release_line::Destination>,
    ),
) -> Result<Bound, String> {
    let (Some(url), Some(listed), Some(read)) = (destination, advertisement, read) else {
        return Err("the run holds no destination advertisement (`--destination` with `--advertisement-stdin`, as the pre-push hook hands over), so the line's tip and the default branch's tip cannot be established independently".to_string());
    };
    matching_claim(root, git, url, listed, read)?;
    let asked = read;
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

/// The destination `url` as this run reads it, and the policy at the tip
/// of the default branch it advertises, read from this clone's objects.
/// The read uses the timeout and attempts of `git`, the policy the caller
/// names; that can only make the read fail, which refuses.
///
/// # Errors
///
/// Returns why the destination cannot be read, why it names no default
/// branch, or why that tip's policy cannot be read here (a tip this clone
/// has not fetched names the remedy), never falling back to `git`.
fn destination_policy(
    root: &Path,
    git: &GitPolicy,
    url: &str,
) -> Result<
    (
        codeflow_core::workgraph::release_line::Destination,
        codeflow_core::hooks::policy::Policy,
    ),
    String,
> {
    use codeflow_core::git::remote_query::QueryLimits;
    use codeflow_core::workgraph::release_line;
    let limits = QueryLimits::from_git(git);
    let read = release_line::advertisement(root, url, limits)
        .and_then(|text| release_line::from_advertisement(url, &text))
        .map_err(|error| format!("this run cannot read the destination itself: {error}"))?;
    let Some((name, tip)) = read.default.clone() else {
        return Err(
            "the destination advertises no default branch whose policy could decide it".into(),
        );
    };
    let shown = format!("'{}'", codeflow_core::git::display_text(&name));
    let policy = policy_at(root, &shown, &tip.to_string())?;
    Ok((read, policy))
}

/// Whether the policy authority the hand-off names, when it names one, is
/// the default branch tip of the destination `read`; a hand-off with none
/// is refused later only when the ref binds as a candidate.
///
/// # Errors
///
/// Returns the mismatch, with the moved-destination remedy.
fn authority_is_tip(
    root: &Path,
    authority: Option<&str>,
    read: &codeflow_core::workgraph::release_line::Destination,
) -> Result<(), String> {
    let (Some(authority), Some((target, tip))) = (authority, read.default.as_ref()) else {
        return Ok(());
    };
    let tip = tip.to_string();
    let named = super::rev_parse(root, authority)?.unwrap_or_else(|| authority.to_string());
    if named.eq_ignore_ascii_case(&tip) {
        return Ok(());
    }
    Err(format!(
        "the hand-off's policy authority {} is not the tip of '{}' the destination advertises, {}: the destination moved after the hook's read, or the hand-off is not the destination's; fetch and push again",
        &named[..named.len().min(9)],
        codeflow_core::git::display_text(target),
        &tip[..tip.len().min(9)]
    ))
}

/// The policy at `tip`, the tip of the branch `what` names, read from this
/// clone's objects; the shipped defaults when that tree proves the policy
/// file absent, as the judging policy does.
///
/// # Errors
///
/// Returns why `tip` is not in this clone (with the remedy to fetch) or
/// why its policy cannot be read or is invalid.
fn policy_at(
    root: &Path,
    what: &str,
    tip: &str,
) -> Result<codeflow_core::hooks::policy::Policy, String> {
    let short = &tip[..tip.len().min(9)];
    if super::rev_parse(root, tip)?.is_none() {
        return Err(format!(
            "the tip of {what} the destination advertises, {short}, is not in this clone; fetch and push again"
        ));
    }
    let unreadable =
        |why: &str| format!("the policy at the tip of {what}, {short}, cannot be read: {why}");
    match codeflow_core::hooks::landed_policy::policy_text_at(root, tip)
        .map_err(|why| unreadable(&why))?
    {
        None => Ok(codeflow_core::hooks::policy::Policy::default()),
        Some(text) => {
            codeflow_core::hooks::policy_schema::validate_policy_str(&text).map_err(|errors| {
                let shown: Vec<_> = errors.iter().map(ToString::to_string).collect();
                unreadable(&shown.join("; "))
            })?;
            serde_json::from_str(&text).map_err(|error| unreadable(&error.to_string()))
        }
    }
}

/// The destination `read`, once `url` is shown to be `origin`'s push URL
/// and the hand-off's advertisement `listed` is shown to match the read at
/// HEAD's target and every branch tip. Tags are not compared: nothing the
/// binding reads comes from a tag.
///
/// # Errors
///
/// Returns why `url` is not `origin`'s push URL, why the hand-off's
/// advertisement cannot be read, or which refs differ.
fn matching_claim(
    root: &Path,
    git: &GitPolicy,
    url: &str,
    listed: &str,
    read: &codeflow_core::workgraph::release_line::Destination,
) -> Result<(), String> {
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
    let (claimed_tips, read_tips) = (tips(&claimed), tips(read));
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
    Ok(())
}
