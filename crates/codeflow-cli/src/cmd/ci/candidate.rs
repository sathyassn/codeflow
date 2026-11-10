//! A pushed landing candidate judged as its line's next tip (TSK-1351).
//!
//! The pre-push hook binds a branch named with `git.candidate_prefix` to
//! its line and hands `codeflow ci` the line's tip as `--base` and the line
//! as `--into`. This check never trusts that hand-off: it binds the pushed
//! branch again from its own reads, the fetched line tip, the records at
//! the default branch's tip and at that line tip, and the advertisement the
//! hook passed, and refuses with the cause when the binding does not hold
//! or the base is not the line's tip. A bound candidate is then judged as
//! a push of its line would be: the line's records into the line's own
//! target, the line's direct-commit rule, and each member merge as a
//! landing on the line.

use std::collections::BTreeMap;
use std::path::Path;

use codeflow_core::hooks::{GitPolicy, Violation};
use codeflow_core::workgraph::landing_candidate::{self, BoundLine, Inputs};
use codeflow_core::workgraph::line_adoption;

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
    authority: Option<&str>,
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
        authority,
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
/// hold or cannot be read.
fn bind(
    root: &Path,
    branch: &str,
    git: &GitPolicy,
    authority: Option<&str>,
    (base, head): (Option<&str>, &str),
    (destination, advertisement): (Option<&str>, Option<&str>),
) -> Result<Bound, String> {
    // The default branch's tip: the authority the hook names, else the
    // default target's fetched tip, never a local branch.
    let default_tip = if let Some(tip) = authority {
        tip.to_string()
    } else {
        let target = codeflow_core::workgraph::default_work_target_ref(root)
            .map_err(|error| format!("the default branch cannot be read: {error}"))?
            .ok_or("no default branch is fetched here to read the line's binding from")?;
        line_adoption::reference_tip(root, &target)?
            .ok_or_else(|| format!("'{target}' cannot be read"))?
    };
    let asked = match (destination, advertisement) {
        (Some(url), Some(listed)) => Some(
            codeflow_core::workgraph::release_line::from_advertisement(url, listed).map_err(
                |error| format!("the destination's advertisement cannot be read: {error}"),
            )?,
        ),
        _ => None,
    };
    let advertised: Option<BTreeMap<String, String>> = asked.as_ref().map(|asked| {
        asked
            .heads
            .iter()
            .map(|(name, tip)| (name.clone(), tip.to_string()))
            .collect()
    });
    // The line's own target: the default branch the destination names,
    // else the one fetched here.
    let target = match asked.as_ref().and_then(|asked| asked.default.as_ref()) {
        Some((name, _)) => name.clone(),
        None => codeflow_core::workgraph::default_work_target(root)
            .map_err(|error| format!("the default branch cannot be read: {error}"))?
            .ok_or("no default branch is known here to judge the line against")?,
    };
    let line = landing_candidate::bind(
        root,
        &Inputs {
            branch,
            head,
            policy: git,
            default_tip: &default_tip,
            advertised: advertised.as_ref(),
            line: None,
        },
    )?;
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
