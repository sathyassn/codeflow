//! Work-record transition check (TSK-102, SPC-013 R-30 to R-35): every
//! record the range changes is judged by the same lifecycle judge the status
//! verbs use, from the merge-base to the head. Only projects with durable
//! work tracking run it; a project without records sees no record rule.

use std::path::Path;

use codeflow_core::hooks::{PolicyLevel, Violation};
use codeflow_core::workgraph::durable_work_tracking_enabled;
use codeflow_core::workgraph::lifecycle::judge_pull_request_under;

/// Run the check for `codeflow ci`, record its findings and whether it ran,
/// and print its notices. `authority` is the commit whose baseline list
/// governs; `None` means the resolved base.
pub(super) fn dispatch(
    root: &Path,
    base_candidates: &[String],
    head: &str,
    authority: Option<&str>,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let outcome = check(
        root,
        super::resolve_base(root, base_candidates).as_deref(),
        head,
        authority,
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
/// and `head`. `base` is `None` when the range could not be resolved.
pub(super) fn check(
    root: &Path,
    base: Option<&str>,
    head: &str,
    authority: Option<&str>,
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
                violations: vec![block(format!(
                    "cannot determine durable-work tracking: {error}"
                ))],
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
    match judge_pull_request_under(root, base, head, authority.unwrap_or(base)) {
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
