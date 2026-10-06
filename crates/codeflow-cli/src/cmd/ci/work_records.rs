//! Work-record transition check (TSK-102, SPC-013 R-30 to R-35): every
//! record the range changes is judged by the same lifecycle judge the status
//! verbs use, from the merge-base to the head. Only projects with durable
//! work tracking run it; a project without records sees no record rule.

use std::path::Path;

use codeflow_core::hooks::{PolicyLevel, Violation};
use codeflow_core::workgraph::durable_work_tracking_enabled;
use codeflow_core::workgraph::lifecycle::{
    judge_line_under, judge_pull_request_under, judge_release_range, Brought,
};

/// Run the check for `codeflow ci`, record its findings and whether it ran,
/// and print its notices. `authority` is the commit whose baseline list
/// governs; `None` means the resolved base. `on_line` says the range is a
/// verified epic line, whose landings are judged one by one; `brought`, on a
/// release range, names the records judged where each was introduced.
#[allow(clippy::too_many_arguments)] // The run's shared state, passed once.
pub(super) fn dispatch(
    root: &Path,
    base_candidates: &[String],
    head: &str,
    authority: Option<&str>,
    on_line: bool,
    brought: Option<&Brought>,
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
    let outcome = check(
        root,
        resolved_base.as_deref(),
        head,
        authority,
        on_line,
        brought,
    );
    if outcome.ran {
        ran.push("work-records");
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
            }
        }
        Err(error) => {
            return Outcome {
                violations: vec![super::tracking_state_violation(error)],
                notices: Vec::new(),
                ran: true,
            }
        }
    }
    let Some(base) = base else {
        return Outcome {
            violations: Vec::new(),
            notices: Vec::new(),
            ran: false,
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
            }
        }
        Err(error) => Outcome {
            violations: vec![block(format!("cannot read the record range: {error}"))],
            notices: Vec::new(),
            ran: true,
        },
    }
}

fn block(message: String) -> Violation {
    Violation::always_blocking(
        "work.records",
        message,
        "change status with `codeflow task|epic|spec status`, which writes what the transition needs",
    )
}
