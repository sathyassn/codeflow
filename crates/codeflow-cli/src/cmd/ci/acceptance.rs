//! Acceptance bound to the reviewed commit in `codeflow ci` (TSK-105,
//! SPC-013 R-52, R-60): a task pull request leaves its record's criteria as
//! the target has them, and every completion in the range is bound to the
//! pull request head, with waivers that name their amendment on the target.
//! The journey criterion for adopter-facing ranges (R-53) runs with the
//! classification of the pull request, which knows its task.

use std::path::Path;

use codeflow_core::hooks::{GitPolicy, PolicyLevel, Violation};
use codeflow_core::workgraph::acceptance::{
    pull_request_findings, Finding, FROZEN_RULE, SCOPE_NOTE,
};

/// Run the checks when durable work tracking is on at the target or at the
/// head, and a range resolves.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    range: Option<&super::classification::Range<'_>>,
    branch: &str,
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let Some(range) = range else {
        return;
    };
    if !super::classification::tracking_on(root, Some(range)).unwrap_or(true) {
        return;
    }
    ran.push("acceptance");
    println!("codeflow ci: acceptance: {SCOPE_NOTE}");
    match pull_request_findings(root, range.base, range.head, branch) {
        Ok(found) => tagged.extend(found.into_iter().map(|found| violation(git, found))),
        Err(error) => tagged.push(violation(
            git,
            Finding {
                rule: FROZEN_RULE,
                message: format!("cannot read the range to check acceptance: {error}"),
            },
        )),
    }
}

/// Criteria frozen always blocks (R-80); the binding and journey rules take
/// the `git.work_records` level.
fn violation(git: &GitPolicy, found: Finding) -> super::TaggedViolation {
    let (level, remedy) = if found.rule == FROZEN_RULE {
        (
            PolicyLevel::Block,
            "change criteria by a planning pull request on the target, then rebase".to_string(),
        )
    } else {
        (
            git.work_records_level(),
            format!(
                "review the pull request head and record it in the acceptance block; a waiver names the planning amendment commit on the target ({SCOPE_NOTE})"
            ),
        )
    };
    super::TaggedViolation {
        sha: None,
        violation: Violation::new(found.rule, level, found.message, remedy),
    }
}
