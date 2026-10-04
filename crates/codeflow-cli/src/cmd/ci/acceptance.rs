//! Acceptance bound to the reviewed commit in `codeflow ci` (TSK-105,
//! SPC-013 R-52, R-60): a task pull request leaves its record's criteria as
//! the target has them, and every completion in the range is bound to its
//! reviewed commit (at the head of a task pull request, where the block was
//! introduced otherwise), with waivers that name their amendment on the
//! target.
//! The journey criterion for adopter-facing ranges (R-53) runs with the
//! classification of the pull request, which knows its task. A planning
//! amendment (ADR-0078) may change criteria, and each change is reported per
//! epic for its reviewer.

use std::path::Path;

use codeflow_core::hooks::{GitPolicy, Violation};
use codeflow_core::workgraph::acceptance::{
    pull_request_findings_judged, Criteria, Finding, FROZEN_RULE, SCOPE_NOTE,
};
use codeflow_core::workgraph::amendment::AMENDMENT_RULE;
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
        None => criteria(root, range, branch, class).and_then(|criteria| {
            pull_request_findings_judged(
                root,
                range.base,
                range.head,
                &criteria,
                names.candidate,
                Some(names.branch),
            )
        }),
    };
    match found {
        Ok(found) => {
            for found in found {
                if found.note {
                    let remedy = if found.rule == FROZEN_RULE {
                        codeflow_core::remedy::CRITERIA_DELTA.remedy()
                    } else if found.rule == AMENDMENT_RULE {
                        codeflow_core::remedy::PLANNING_AMENDMENT.remedy()
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
                epic_record: None,
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
pub(super) fn release_scope<'n>(
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

/// On a release range, the one task the release checks select as the owner
/// of its direct work (SPC-013 R-120), judged at the same head as the
/// acceptance check; `None` when none or several are eligible, or the
/// range cannot be read, which the acceptance check refuses.
pub(super) fn release_owner(
    root: &Path,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Option<String> {
    let (destination, scope) = release_scope(root, names).ok()??;
    let head = release_head(root, range, scope).ok()?;
    release_line::release_owner(root, destination, range.base, &head)
        .ok()
        .flatten()
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

/// Criteria may change only in a planning amendment (ADR-0078), on a
/// validated epic line (R-52) or on the workspace root branch the target's
/// policy names, decided from the validated class, never from the branch
/// prefix alone. Without a class (no pull request body, as the pre-push
/// hook runs), the range must itself carry only what a planning amendment
/// may, `AGENTS.md`'s managed block read from the target at the base, on a
/// branch that carries no task; or be the root branch or a verified epic
/// line. A push names no epics yet, so its pull request judges their scope.
fn criteria(
    root: &Path,
    range: &Range<'_>,
    branch: &str,
    class: Option<&Class>,
) -> Result<Criteria, String> {
    // A release range is judged by `judge` and does not reach here; were it
    // to, the release pull request keeps its task's criteria as tracked work.
    if let Some(Class::Tracked { task_id } | Class::ReleaseIntegration { task_id }) = class {
        return codeflow_core::workgraph::acceptance::task_criteria(root, range.base, task_id);
    }
    // Without a pull request body (a push), a task branch's only valid class
    // is its own task, so it may change that task's criteria as its PR may.
    if class.is_none() {
        if let Some(task_id) = task_id_from_branch(root, branch) {
            return codeflow_core::workgraph::acceptance::task_criteria(root, range.base, &task_id);
        }
    }
    let amendable = match class {
        Some(Class::PlanningOnly { epics }) => return Ok(Criteria::Amendment(Some(epics.clone()))),
        Some(Class::EpicLine(_) | Class::RootBranch(_)) => true,
        Some(_) => false,
        None => {
            if root_branch_at(root, range.base).as_deref() == Some(branch)
                || (branch.starts_with("integration/")
                    && check_epic_line(root, branch, range.target, range.base, range.head).is_ok())
            {
                true
            } else if task_id_from_branch(root, branch).is_none()
                && planning_amendment_range(root, range)?
            {
                return Ok(Criteria::Amendment(None));
            } else {
                false
            }
        }
    };
    Ok(if amendable {
        Criteria::Amendable
    } else {
        Criteria::Frozen
    })
}

/// Whether the range carries only what a planning amendment may, read
/// against the target at the base (ADR-0078): the push-side twin of the
/// planning class.
fn planning_amendment_range(root: &Path, range: &Range<'_>) -> Result<bool, String> {
    let paths: Vec<String> = range_changes(root, range.base, range.head)?
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    Ok(
        codeflow_core::workgraph::amendment::range_problem_at(root, range.base, range.head, &paths)
            .is_none(),
    )
}

/// Criteria frozen and a planning amendment's epic scope always block
/// (R-80, ADR-0078); the binding and journey rules take the
/// `git.work_records` level.
fn violation(git: &GitPolicy, found: Finding) -> super::TaggedViolation {
    let violation = if found.rule == FROZEN_RULE {
        Violation::always_blocking(
            found.rule,
            found.message,
            "another task's criteria change by its own PR or by a planning amendment that names its epic; a reopened task keeps its criteria",
        )
    } else if found.rule == AMENDMENT_RULE {
        Violation::always_blocking(
            found.rule,
            found.message,
            "name every epic the amendment changes on its one `Task:` line (`Task: EPC-001, EPC-002`), or move that change to its own epic's amendment",
        )
    } else {
        // An epic's own block has its own route: an epic is never reopened.
        let remedy = match &found.epic_record {
            Some(path) => codeflow_core::remedy::EPIC_ACCEPTANCE_BINDING
                .with(&[("path", path), ("note", SCOPE_NOTE)]),
            None => codeflow_core::remedy::ACCEPTANCE_BINDING.with(&[("note", SCOPE_NOTE)]),
        };
        Violation::new(found.rule, git.work_records_level(), found.message, remedy)
    };
    super::TaggedViolation {
        sha: None,
        violation,
    }
}
