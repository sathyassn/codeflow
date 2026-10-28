//! Work-record transition check (TSK-102, SPC-013 R-30 to R-35): every
//! record the range changes is judged by the same lifecycle judge the status
//! verbs use, from the merge-base to the head. Only projects with durable
//! work tracking run it; a project without records sees no record rule.
//! The Outcome adoption marker (TSK-435) is judged in every project,
//! against the tip of the branch the range lands on.

use std::path::Path;

use codeflow_core::hooks::{PolicyLevel, Violation};
use codeflow_core::workgraph::durable_work_tracking_enabled;
use codeflow_core::workgraph::lifecycle::{
    judge_line_under, judge_pull_request_under, judge_release_range, outcome_marker_errors, Brought,
};

/// Run the check for `codeflow ci`, record its findings and whether it ran,
/// and print its notices. `authority` is the commit whose baseline list
/// governs; `None` means the resolved base. `on_line` says the range is a
/// verified epic line, whose landings are judged one by one; `brought`, on a
/// release range, names the records judged where each was introduced.
/// `target_tip` is the tip of the branch the range lands on, which the
/// Outcome marker is judged against (see [`super::target_tip`]).
#[allow(clippy::too_many_arguments)] // The run's shared state, passed once.
pub(super) fn dispatch(
    root: &Path,
    base_candidates: &[String],
    head: &str,
    authority: Option<&str>,
    on_line: bool,
    brought: Option<&Brought>,
    target_tip: &Result<Option<String>, String>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let resolved_base = match super::resolve_base(root, base_candidates) {
        Ok(value) => value,
        Err(error) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: super::tracking_state_violation(error),
            });
            return;
        }
    };
    let mut outcome = check(
        root,
        resolved_base.as_deref(),
        head,
        authority,
        on_line,
        brought,
    );
    // The marker is judged wherever the range resolved, whatever the
    // tracking state.
    if resolved_base.is_some() {
        outcome.violations.extend(marker(root, target_tip, head));
        outcome.marker_ran = true;
    }
    if outcome.ran {
        ran.push("work-records");
    }
    if outcome.marker_ran {
        ran.push("outcome-marker");
    }
    for notice in &outcome.notices {
        eprintln!(
            "{}",
            notice
                .clone()
                .prefixed("work.records")
                .line("codeflow ci", "notice")
        );
    }
    tagged.extend(
        outcome
            .violations
            .into_iter()
            .map(|violation| super::TaggedViolation {
                sha: None,
                violation,
            }),
    );
}

/// What the check found and whether it ran.
pub(super) struct Outcome {
    pub violations: Vec<Violation>,
    /// Facts to show without blocking, such as an edited baseline list.
    pub notices: Vec<codeflow_core::remedy::Finding>,
    pub ran: bool,
    /// Whether the Outcome adoption marker was judged.
    pub marker_ran: bool,
}

/// Judge the records changed between the merge-base of `base` and `head`,
/// and `head`. `base` is `None` when the range could not be resolved. On a
/// release range, `brought` names the records judged where each was
/// introduced on its line.
pub(super) fn check(
    root: &Path,
    base: Option<&str>,
    head: &str,
    authority: Option<&str>,
    on_line: bool,
    brought: Option<&Brought>,
) -> Outcome {
    match durable_work_tracking_enabled(root) {
        Ok(true) => {}
        Ok(false) => {
            return Outcome {
                violations: Vec::new(),
                notices: Vec::new(),
                ran: false,
                marker_ran: false,
            }
        }
        Err(error) => {
            return Outcome {
                violations: vec![super::tracking_state_violation(error)],
                notices: Vec::new(),
                ran: true,
                marker_ran: false,
            }
        }
    }
    let Some(base) = base else {
        return Outcome {
            violations: Vec::new(),
            notices: Vec::new(),
            ran: false,
            marker_ran: false,
        };
    };
    // A release range judges each brought record where it was introduced
    // on its line (SPC-013 R-120); a verified epic line judges each landing
    // on its own (R-60); any other range is judged whole.
    let authority = authority.unwrap_or(base);
    let judged = match brought {
        Some(brought) => judge_release_range(root, base, head, authority, brought),
        None if on_line => judge_line_under(root, base, head, authority),
        None => judge_pull_request_under(root, base, head, authority),
    };
    match judged {
        Ok(verdict) => {
            let mut violations: Vec<Violation> = verdict.errors.into_iter().map(block).collect();
            violations.extend(verdict.warnings.into_iter().map(|warning| {
                Violation::new(
                    "work.records",
                    PolicyLevel::Warn,
                    warning.text,
                    warning.remedy,
                )
            }));
            Outcome {
                violations,
                notices: verdict.notices,
                ran: true,
                marker_ran: false,
            }
        }
        Err(error) => Outcome {
            violations: vec![block(format!("cannot read the record range: {error}"))],
            notices: Vec::new(),
            ran: true,
            marker_ran: false,
        },
    }
}

/// The Outcome adoption marker's refusals (TSK-435): the head's list
/// judged against `target_tip`, never against the range's base, which the
/// pre-push hook sets to a merge-base or another boundary. A project with
/// only epics, or no records yet, is judged too. A target the destination
/// proves absent (`Ok(None)`) has no list to keep; a tip or marker that
/// cannot be read refuses.
fn marker(root: &Path, target_tip: &Result<Option<String>, String>, head: &str) -> Vec<Violation> {
    let judged = match target_tip {
        Ok(None) => return Vec::new(),
        Ok(Some(tip)) => outcome_marker_errors(root, tip, Some(head)),
        Err(error) => Err(error.clone()),
    };
    match judged {
        Ok(errors) => errors.into_iter().map(marker_block).collect(),
        Err(error) => vec![marker_block(format!(
            "cannot judge the Outcome adoption marker in .codeflow/project.toml: {error}"
        ))],
    }
}

fn marker_block(message: String) -> Violation {
    Violation::always_blocking(
        "work.records",
        message,
        "keep `outcome_present_uids` as the target lists it, or drop uids from it; a record made after adoption states its Outcome instead",
    )
}

pub(super) fn block(message: String) -> Violation {
    Violation::always_blocking(
        "work.records",
        message,
        "change status with `codeflow task|epic|spec status`, which writes what the transition needs",
    )
}
