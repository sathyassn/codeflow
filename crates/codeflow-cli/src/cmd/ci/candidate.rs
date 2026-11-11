//! A pushed landing candidate judged as its line's next tip (TSK-1351).
//!
//! The pre-push hook binds a branch named with `git.candidate_prefix` to
//! its line and hands `codeflow ci` the line's tip as `--base` and the line
//! as `--into`. This check never trusts that hand-off: it binds the pushed
//! branch again from its own reads, the fetched line tip, the records at
//! the default branch's tip and at that line tip, and the destination's
//! advertisement, which it requires. It refuses with the cause when the
//! binding does not hold, when the advertisement is missing, or when a
//! claim of the hand-off (the policy authority, `--into`, the base) is
//! missing or differs from what it established. A bound candidate is then judged as
//! a push of its line would be: the line's records into the line's own
//! target, the line's direct-commit rule, and each member merge as a
//! landing on the line.

use std::collections::BTreeMap;
use std::path::Path;

use codeflow_core::hooks::{GitPolicy, Violation};
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
            println!(
                "codeflow ci: landing candidate: '{shown}' is bound to '{}' at {}, {} member merge(s): judged as the line's next tip, as a push of the line into '{}' is",
                codeflow_core::git::display_text(&bound.line.line),
                &bound.line.tip[..bound.line.tip.len().min(9)],
                bound.line.merges,
                codeflow_core::git::display_text(&bound.target)
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

/// Bind `branch` from this run's own reads; the cause when it does not
/// hold or cannot be read. Every destination tip comes from the
/// destination's own advertisement; each claim the hand-off makes (the
/// policy authority, `--into`, the base) must match what this run
/// establishes, and a missing claim is refused, never filled in.
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
    let asked = codeflow_core::workgraph::release_line::from_advertisement(url, listed)
        .map_err(|error| format!("the destination's advertisement cannot be read: {error}"))?;
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
    let line = landing_candidate::bind(
        root,
        &Inputs {
            branch,
            head,
            policy: git,
            default_tip: &default_tip,
            advertised: &advertised,
            line: None,
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
