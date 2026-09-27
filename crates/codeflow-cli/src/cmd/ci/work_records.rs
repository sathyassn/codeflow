//! Work-record transition check (TSK-102, SPC-013 R-30 to R-35): every
//! record the range changes is judged by the same lifecycle judge the status
//! verbs use, from the merge-base to the head. Only projects with durable
//! work tracking run it; a project without records sees no record rule.

use std::path::Path;

use codeflow_core::hooks::{PolicyLevel, Violation};
use codeflow_core::workgraph::durable_work_tracking_enabled;
use codeflow_core::workgraph::lifecycle::judge_pull_request;

/// Run the check for `codeflow ci` and record its findings and whether it ran.
pub(super) fn dispatch(
    root: &Path,
    base_candidates: &[String],
    head: &str,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let outcome = check(
        root,
        super::resolve_base(root, base_candidates).as_deref(),
        head,
    );
    if outcome.ran {
        ran.push("work-records");
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
    pub ran: bool,
}

/// Judge the records changed between the merge-base of `base` and `head`,
/// and `head`. `base` is `None` when the range could not be resolved.
pub(super) fn check(root: &Path, base: Option<&str>, head: &str) -> Outcome {
    match durable_work_tracking_enabled(root) {
        Ok(true) => {}
        Ok(false) => {
            return Outcome {
                violations: Vec::new(),
                ran: false,
            }
        }
        Err(error) => {
            return Outcome {
                violations: vec![block(format!(
                    "cannot determine durable-work tracking: {error}"
                ))],
                ran: true,
            }
        }
    }
    let Some(base) = base else {
        return Outcome {
            violations: Vec::new(),
            ran: false,
        };
    };
    match judge_pull_request(root, base, head) {
        Ok(verdict) => {
            let mut violations: Vec<Violation> = verdict.errors.into_iter().map(block).collect();
            violations.extend(verdict.warnings.into_iter().map(|warning| {
                Violation::new(
                    "work.records",
                    PolicyLevel::Warn,
                    warning,
                    "an older record keeps its baseline exemption; a transition applies the rules in full".to_string(),
                )
            }));
            Outcome {
                violations,
                ran: true,
            }
        }
        Err(error) => Outcome {
            violations: vec![block(format!("cannot read the record range: {error}"))],
            ran: true,
        },
    }
}

fn block(message: String) -> Violation {
    Violation::new(
        "work.records",
        PolicyLevel::Block,
        message,
        "change status with `codeflow task|epic|spec status`, which writes what the transition needs".to_string(),
    )
}
