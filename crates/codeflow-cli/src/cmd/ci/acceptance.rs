//! Acceptance bound to the reviewed commit in `codeflow ci` (TSK-105,
//! SPC-013 R-52, R-60): a task pull request leaves its record's criteria as
//! the target has them, and every completion in the range is bound to its
//! reviewed commit (at the head of a task pull request, where the block was
//! introduced otherwise), with waivers that name their amendment on the
//! target.
//! The journey criterion for adopter-facing ranges (R-53) runs with the
//! classification of the pull request, which knows its task.

use std::path::Path;

use codeflow_core::hooks::{GitPolicy, Violation};
use codeflow_core::workgraph::acceptance::{
    pull_request_findings, Criteria, Finding, FROZEN_RULE, SCOPE_NOTE,
};
use codeflow_core::workgraph::classify::is_planning_path;
use codeflow_core::workgraph::release_line;
use codeflow_core::workgraph::{check_epic_line, task_id_from_branch};

use super::classification::{range_changes, root_branch_at, Class, Range};

/// Run the checks when durable work tracking is on at the target or at the
/// head, and a range resolves. `class` is the pull request's validated
/// class, when it was classified.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    range: Option<&Range<'_>>,
    names: &super::Names<'_>,
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
    let branch = names.branch;
    let found = match judge(root, range, names) {
        Some(judged) => judged,
        None => criteria(root, range, branch, class)
            .and_then(|criteria| pull_request_findings(root, range.base, range.head, &criteria)),
    };
    match found {
        Ok(found) => {
            for found in found {
                if found.note {
                    let remedy = if found.rule == FROZEN_RULE {
                        codeflow_core::remedy::CRITERIA_DELTA.remedy()
                    } else {
                        codeflow_core::remedy::ACCEPTANCE_BOUND.remedy()
                    };
                    let note = codeflow_core::remedy::Finding::new(
                        format!("{}: {}", found.rule, found.message),
                        remedy,
                    );
                    println!("{}", note.line("codeflow ci", "note"));
                } else {
                    tagged.push(violation(git, found));
                }
            }
        }
        Err(error) => tagged.push(violation(
            git,
            Finding {
                rule: FROZEN_RULE,
                note: false,
                message: format!("cannot read the range to check acceptance: {error}"),
            },
        )),
    }
}

/// The release-range judgement (SPC-013 R-120), or `None` for an ordinary
/// range. Scope comes from the head and target names under the policy at
/// the destination's default target; when that cannot be read the check
/// fails closed. A pull request into a release branch is judged as the
/// merge it would create.
fn judge(
    root: &Path,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Option<Result<Vec<Finding>, String>> {
    let (destination, scope) = match release_scope(root, names) {
        Ok(Some(release)) => release,
        Ok(None) => return None,
        Err(error) => return Some(Err(unscoped(error))),
    };
    // The scope this range is judged under: a result, not a finding. The
    // pre-push hook states a release push's scope itself.
    eprintln!(
        "codeflow ci: acceptance: release range ('{}'{}) under pattern '{}' ({}); each change is judged where it was introduced (SPC-013 R-120)",
        names.branch,
        names
            .into
            .map(|into| format!(" into '{into}'"))
            .unwrap_or_default(),
        scope.pattern,
        scope.source
    );
    let head = match release_head(root, range, scope) {
        Ok(head) => head,
        Err(error) => return Some(Err(error)),
    };
    Some(
        release_line::release_findings(root, destination, range.base, &head).map(|judged| {
            for step in &judged.path {
                println!("codeflow ci: acceptance: release path: {step}");
            }
            // A notice, so the pre-push hook shows it on a passing push too.
            for note in &judged.notes {
                let notice = codeflow_core::remedy::Finding::new(
                    format!("acceptance: {note}"),
                    codeflow_core::remedy::RELEASE_LEGACY_CHANGE.remedy(),
                );
                eprintln!("{}", notice.line("codeflow ci", "notice"));
            }
            judged.findings
        }),
    )
}

/// The destination and scope of a release range, asked once per run and
/// shared by the records rule and the acceptance check; `None` for an
/// ordinary range.
fn release_scope<'n>(
    root: &Path,
    names: &'n super::Names<'_>,
) -> Result<Option<&'n (release_line::Destination, release_line::Scope)>, &'n str> {
    names
        .release
        .get_or_init(|| {
            let destination = match (names.destination, names.advertisement) {
                (Some(url), Some(listed)) => release_line::from_advertisement(url, listed),
                _ => release_line::ask_destination(root, names.destination),
            }?;
            let scope = release_line::scope(root, &destination, names.branch, names.into)?;
            Ok(scope.release().then_some((destination, scope)))
        })
        .as_ref()
        .map(Option::as_ref)
        .map_err(String::as_str)
}

/// The head a release range is judged at: a pull request into a release
/// branch is judged as the merge it would create.
fn release_head(
    root: &Path,
    range: &Range<'_>,
    scope: &release_line::Scope,
) -> Result<String, String> {
    if scope.into {
        release_line::pull_request_merge(root, range.base, range.head)
            .map(|merge| merge.to_string())
    } else {
        Ok(range.head.to_string())
    }
}

/// On a release range, the records it brings from verified lines, for the
/// records rule to judge where each was introduced (SPC-013 R-120). `None`
/// on an ordinary range, and when the range cannot be judged, which the
/// acceptance check refuses.
pub(super) fn brought(
    root: &Path,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Option<codeflow_core::workgraph::lifecycle::Brought> {
    let (destination, scope) = release_scope(root, names).ok()??;
    let head = release_head(root, range, scope).ok()?;
    release_line::release_findings(root, destination, range.base, &head)
        .ok()
        .map(|judged| judged.brought)
}

fn unscoped(error: &str) -> String {
    format!("whether this is a release range cannot be decided, so nothing is judged under the ordinary rules instead (SPC-013 R-120): {error}")
}

/// Criteria may change only in a planning-only change, on a validated epic
/// line (R-52) or on the workspace root branch the target's policy names,
/// decided from the validated class, never from the branch prefix alone.
/// Without a class (no pull request body), the range must itself be
/// planning-only on a branch that carries no task, the root branch, or a
/// verified epic line.
fn criteria(
    root: &Path,
    range: &Range<'_>,
    branch: &str,
    class: Option<&Class>,
) -> Result<Criteria, String> {
    if let Some(Class::Tracked { task_id, .. }) = class {
        return codeflow_core::workgraph::acceptance::task_criteria(root, range.base, task_id);
    }
    let amendable = match class {
        Some(Class::PlanningOnly | Class::EpicLine(_) | Class::RootBranch(_)) => true,
        Some(_) => false,
        None => {
            let planning_only = task_id_from_branch(root, branch).is_none()
                && range_changes(root, range.base, range.head)?
                    .iter()
                    .all(|(_, path)| is_planning_path(path));
            planning_only
                || root_branch_at(root, range.base).as_deref() == Some(branch)
                || (branch.starts_with("integration/")
                    && check_epic_line(root, branch, range.target, range.base, range.head).is_ok())
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
    let violation = if found.rule == FROZEN_RULE {
        Violation::always_blocking(
            found.rule,
            found.message,
            "another task's criteria change by its own PR or the epic amendment; a reopened task keeps its criteria",
        )
    } else {
        Violation::new(
            found.rule,
            git.work_records_level(),
            found.message,
            codeflow_core::remedy::ACCEPTANCE_BINDING.with(&[("note", SCOPE_NOTE)]),
        )
    };
    super::TaggedViolation {
        sha: None,
        violation,
    }
}
