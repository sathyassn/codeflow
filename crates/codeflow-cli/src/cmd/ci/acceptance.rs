//! Acceptance bound to the reviewed commit in `codeflow ci` (TSK-105,
//! SPC-013 R-52, R-60): a task pull request leaves its record's criteria as
//! the target has them, and every completion in the range is bound to its
//! reviewed commit (at the head of a task pull request, where the block was
//! introduced otherwise), with waivers that name their amendment on the
//! target.
//! The journey criterion for adopter-facing ranges (R-53) runs with the
//! classification of the pull request, which knows its task.

use std::path::Path;

use codeflow_core::hooks::{GitPolicy, PolicyLevel, Violation};
use codeflow_core::workgraph::acceptance::{
    pull_request_findings, Criteria, Finding, FROZEN_RULE, SCOPE_NOTE,
};
use codeflow_core::workgraph::classify::is_planning_path;
use codeflow_core::workgraph::{check_epic_line, task_id_from_branch};

use super::classification::{range_changes, Class, Range};

/// Run the checks when durable work tracking is on at the target or at the
/// head, and a range resolves. `class` is the pull request's validated
/// class, when it was classified.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    range: Option<&Range<'_>>,
    branch: &str,
    class: Option<&Class>,
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
    let found = criteria(root, range, branch, class)
        .and_then(|criteria| pull_request_findings(root, range.base, range.head, criteria));
    match found {
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

/// Criteria may change only in a planning-only change or on a validated
/// epic line (R-52), decided from the validated class, never from the
/// branch prefix alone. Without a class (no pull request body), the range
/// must itself be planning-only on a branch that carries no task, or a
/// verified epic line.
fn criteria(
    root: &Path,
    range: &Range<'_>,
    branch: &str,
    class: Option<&Class>,
) -> Result<Criteria, String> {
    let amendable = match class {
        Some(Class::PlanningOnly | Class::EpicLine(_)) => true,
        Some(_) => false,
        None => {
            let planning_only = task_id_from_branch(root, branch).is_none()
                && range_changes(root, range.base, range.head)?
                    .iter()
                    .all(|(_, path)| is_planning_path(path));
            planning_only
                || (branch.starts_with("integration/")
                    && check_epic_line(root, branch, range.base_ref, range.base, range.head)
                        .is_ok())
        }
    };
    Ok(if amendable {
        Criteria::Amendable
    } else {
        Criteria::Frozen
    })
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
