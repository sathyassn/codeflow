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
    is_correction, pull_request_findings_into, Criteria, Finding, OwnLine, Replaced, Scope,
    AUTHORITY_NOTE, COPIES_NOTE, FROZEN_RULE, IMPORT_RULE, REMOVAL_RULE, SCOPE_NOTE,
};
use codeflow_core::workgraph::amendment::AMENDMENT_RULE;
use codeflow_core::workgraph::release_line;
use codeflow_core::workgraph::{check_epic_line, judged_task_id};

use super::classification::{range_changes, root_branch_at, Class, Range};

/// Run the checks when durable work tracking is on at the target or at the
/// head, and a range resolves. `class` is the pull request's validated
/// class, when it was classified. `body` is the pull request body, which
/// gives the reason for each criterion a task removes from its own record.
/// `push` is true for the pre-push hook's hand-off, never for a pull
/// request check.
#[allow(clippy::too_many_arguments)] // The run's shared state and the stacked heads, passed once.
pub(super) fn dispatch(
    root: &Path,
    git: &GitPolicy,
    push: bool,
    body: Option<&str>,
    range: Option<&Range<'_>>,
    names: &super::Names<'_>,
    class: Option<&Class>,
    stacked: &[codeflow_core::workgraph::work_start::ReviewedPin],
    tagged: &mut Vec<super::TaggedViolation>,
    ran: &mut Vec<&str>,
) {
    let Some(range) = range else {
        return;
    };
    match super::classification::tracking_on(root, Some(range)) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tagged.push(super::TaggedViolation {
                sha: None,
                violation: super::tracking_state_violation(error),
            });
            return;
        }
    }
    ran.push("acceptance");
    println!("codeflow ci: acceptance: {SCOPE_NOTE}");
    let branch = names.branch;
    let found = match judged_apart(root, git, range, names) {
        Some(judged) => judged,
        None => criteria(root, git, range, branch, class).and_then(|criteria| {
            println!(
                "codeflow ci: acceptance binding: {} (branch '{}', target '{}')",
                if criteria.amends() {
                    "amendable"
                } else {
                    "frozen"
                },
                codeflow_core::git::display_text(branch),
                codeflow_core::git::display_text(range.target)
            );
            // A landing candidate, whose line and fetched tip the line-tip
            // check established, is judged member by member (TSK-1154)
            // wherever it would not be judged as an amendment: a task its
            // branch name carries is never the candidate's own, so each
            // member's task comes only from the records at its head (AC-5).
            if let (false, None, Some(landing)) = (criteria.amends(), class, names.landing.as_ref())
            {
                return candidate(root, git, names, range.head, landing);
            }
            let sync_tip = if push {
                line_sync_tip(root, git, range, names)
            } else {
                None
            };
            let own_line = own_line(root, git, range, names, &criteria);
            let replaced = replaced_ref(root, git, range, names, push);
            pull_request_findings_into(
                root,
                range.base,
                range.head,
                &criteria,
                names.candidate,
                (Some(names.branch), Some(range.target), sync_tip.as_deref()),
                stacked,
                Scope {
                    own_line: own_line.as_ref(),
                    replaced: replaced.as_ref(),
                    branch_prefixes: Some(&git.branch_prefixes),
                    member: None,
                    stacking: None,
                },
            )
        }),
    };
    match found {
        Ok(found) => {
            for found in found {
                if found.rule == REMOVAL_RULE {
                    if let Some(refused) = removal(body, &found) {
                        tagged.push(refused);
                    }
                } else if found.note {
                    let remedy = if found.rule == AUTHORITY_NOTE || found.rule == COPIES_NOTE {
                        codeflow_core::remedy::CRITERIA_AUTHORITY.remedy()
                    } else if is_correction(&found) {
                        codeflow_core::remedy::CRITERIA_CORRECTION.remedy()
                    } else if found.rule == FROZEN_RULE {
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

/// A range judged apart from the criteria classes: a batch into the
/// default target, member by member from that target's fetched tip
/// (TSK-1154 AC-16), never as one range and never by a class its body
/// names; or a release range ([`judge`]). `None` for any other range.
fn judged_apart(
    root: &Path,
    git: &GitPolicy,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Option<Result<Vec<Finding>, String>> {
    match names.main_batch() {
        Some(landing) => Some(candidate(root, git, names, range.head, landing)),
        None => judge(root, range, names),
    }
}

/// The target line's tip as the destination advertises it, for the pre-push
/// hand-off of a task branch (TSK-1149). Pushing keeps work durable; the
/// review of a hand-resolved sync of the branch's own line belongs to the
/// pull request check at ready and at landing, which never asks for this
/// tip. `None`, which keeps every merge judged, when the pushed branch is
/// protected or folds onto a protected name, carries no task, or the
/// destination does not advertise the target under that exact name.
fn line_sync_tip(
    root: &Path,
    git: &GitPolicy,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Option<String> {
    let (Some(url), Some(listed)) = (names.destination, names.advertisement) else {
        return None;
    };
    let branch = names.branch;
    if git.branch_is_protected_any_case(branch) || git.branch_folds_onto_protected(branch) {
        return None;
    }
    if !matches!(judged_task(root, git, branch, range.head), Ok(Some(_))) {
        return None;
    }
    let destination = release_line::from_advertisement(url, listed)
        .ok()?
        .with_limits(names.limits);
    release_line::advertised_tip_here(root, &destination, range.target)
        .ok()
        .map(|tip| tip.to_string())
}

/// The task `branch` carries, read from the task records at the judged
/// `head` under the work prefixes of the policy the run judges with
/// (TSK-350 AC-1, AC-2), never from the checkout's records or policy.
fn judged_task(
    root: &Path,
    git: &GitPolicy,
    branch: &str,
    head: &str,
) -> Result<Option<String>, String> {
    judged_task_id(root, &git.branch_prefixes, branch, head).map_err(|error| error.to_string())
}

/// The bounded own-line exception for the task's own criteria change
/// (TSK-350 AC-6, SPC-013 R-52), when every condition holds; `None` keeps
/// today's criteria points. It matters only when the run names a candidate
/// authority (the pre-push hook's `--policy-from`), the one point it would
/// set aside, so no other run asks the destination for it. A condition
/// that cannot be proved is a refusal of the exception, never a grant.
fn own_line(
    root: &Path,
    git: &GitPolicy,
    range: &Range<'_>,
    names: &super::Names<'_>,
    criteria: &Criteria,
) -> Option<OwnLine> {
    let Criteria::OwnTask(task) = criteria else {
        return None;
    };
    names.candidate?;
    // Only an explicit `--into` earns the exception: a target inferred from
    // the head's own record keeps today's rule (TSK-350 Description).
    let into = names.into_flag.and(names.into)?;
    match own_line_tip(root, git, range, names, task, into) {
        Ok(line) => {
            println!(
                "codeflow ci: acceptance: {task} changes its own criteria on its bound line '{}' (at {:.9}): the line's tip and the range base are its criteria points; the candidate authority stays the policy and history authority (SPC-013 R-52)",
                codeflow_core::git::display_text(into),
                line.tip
            );
            Some(line)
        }
        Err(reason) => {
            if codeflow_core::workgraph::line_adoption::epic_id(into).is_ok() {
                println!(
                    "codeflow ci: acceptance: the own-line criteria exception does not apply to {task} on '{}': {reason}",
                    codeflow_core::git::display_text(into)
                );
            }
            None
        }
    }
}

/// The confirmed tip of `into` when the exception's binding holds for
/// `task`: `into` is an epic line, not a protected branch; the branch
/// carries `task` at the judged head; the destination advertises `into`
/// and its tip is here; the task's record declares `into` at that tip and
/// at the head; and the records at the pinned candidate authority and at
/// that tip bind the line, with no refusal at either.
fn own_line_tip(
    root: &Path,
    git: &GitPolicy,
    range: &Range<'_>,
    names: &super::Names<'_>,
    task: &str,
    into: &str,
) -> Result<OwnLine, String> {
    use codeflow_core::workgraph::EpicLineBinding;
    if codeflow_core::workgraph::line_adoption::epic_id(into).is_err()
        || git.branch_is_protected_any_case(into)
        || git.branch_folds_onto_protected(into)
    {
        return Err("the target is not an epic integration line".into());
    }
    let branch = names.branch;
    if judged_task(root, git, branch, range.head)?.as_deref() != Some(task) {
        return Err(format!(
            "the branch does not carry {task} at the judged head"
        ));
    }
    let destination = match (names.destination, names.advertisement) {
        (Some(url), Some(listed)) => release_line::from_advertisement(url, listed)
            .map(|destination| destination.with_limits(names.limits)),
        _ => release_line::ask_destination(root, names.destination, names.limits),
    }
    .map_err(|error| format!("the destination cannot confirm the line: {error}"))?;
    let tip = release_line::advertised_tip_here(root, &destination, into)
        .map_err(|error| format!("the destination cannot confirm the line: {error}"))?;
    // Every read below is at an exact revision and by the task's id: the
    // declarations at the confirmed line tip and the head, and the binding
    // at the pinned candidate authority and that tip, where a refusal at
    // either wins. No checkout policy and no local or remote-tracking ref
    // takes part (TSK-350 review 01, findings 1 and 2).
    let tip_text = tip.to_string();
    for (revision, place) in [
        (tip_text.as_str(), "the line's tip"),
        (range.head, "the head"),
    ] {
        let declared = codeflow_core::workgraph::task_target_at_revision(root, task, revision)
            .map_err(|error| error.to_string())?;
        if declared.as_deref() != Some(into) {
            return Err(format!(
                "{task}'s record at {place} does not declare this line as its integration_target"
            ));
        }
    }
    let candidate = names
        .candidate
        .ok_or_else(|| "no pinned candidate authority".to_string())?;
    let anchors = [
        (candidate, "the candidate authority"),
        (tip_text.as_str(), "the line's tip"),
    ];
    match codeflow_core::workgraph::task_line_binding(root, task, range.head, into, &anchors)
        .map_err(|error| error.to_string())?
    {
        EpicLineBinding::Bound => Ok(OwnLine {
            task: task.to_string(),
            tip,
        }),
        EpicLineBinding::Refused(reason) | EpicLineBinding::Unbound(reason) => {
            Err(format!("the line {reason}, so it is not a bound epic line"))
        }
    }
}

/// A landing candidate judged member by member (TSK-1154): each merge on
/// the head's first-parent chain from the fetched tip is judged at its
/// member head against the tip it merged onto, as that member's own pull
/// request into the receiving branch is judged. The whole tip-to-head
/// chain is judged whatever base the run was given, so a base after the
/// tip hides no member.
///
/// On an epic line, a member's own criteria change is judged as its pull
/// request's only when its record declares the line at the line's tip and
/// at the member head, and the trusted records at that tip (and at the
/// candidate authority, when the run names one) bind the line for it. In
/// a batch into the default target (AC-16), each member binds from the
/// records at its two parents and the target's fetched tip
/// ([`main_binding`]), and no existing criterion may change.
///
/// [`main_binding`]: codeflow_core::workgraph::acceptance::candidate::main_binding
fn candidate(
    root: &Path,
    git: &GitPolicy,
    names: &super::Names<'_>,
    head: &str,
    landing: &super::line_tip::Landing,
) -> Result<Vec<Finding>, String> {
    use codeflow_core::workgraph::acceptance::candidate::{
        candidate_findings, completed_task, main_binding, Bound, Candidate, Member, Mode,
    };
    use codeflow_core::workgraph::EpicLineBinding;
    let line = landing.line.as_str();
    let tip = landing.tip.as_str();
    let mode = match landing.kind {
        super::line_tip::Kind::Line => Mode::Line,
        super::line_tip::Kind::MainBatch => Mode::MainBatch,
    };
    let candidate = Candidate {
        tip,
        target: line,
        authority: names.candidate,
        branch_prefixes: Some(&git.branch_prefixes),
        mode,
    };
    if mode == Mode::MainBatch {
        println!(
            "codeflow ci: acceptance: batch into '{}' from its fetched tip {:.9}: each member merge is judged at its own reviewed head against the tip it merged onto, and no existing criterion may change",
            codeflow_core::git::display_text(line),
            tip
        );
        let mut bind = |member: Member| -> Result<Bound, String> {
            main_binding(root, tip, member, line).map(|task| Bound::Frozen { task, note: None })
        };
        return candidate_findings(root, &candidate, head, &mut bind);
    }
    println!(
        "codeflow ci: acceptance: landing candidate of '{}' from its fetched tip {:.9}: each member merge is judged at its own reviewed head against the tip it merged onto",
        codeflow_core::git::display_text(line),
        tip
    );
    let line_binding = |task: &str, member: &str| -> Result<(), String> {
        for (revision, place) in [(tip, "the line's tip"), (member, "the member head")] {
            let declared = codeflow_core::workgraph::task_target_at_revision(root, task, revision)
                .map_err(|error| error.to_string())?;
            if declared.as_deref() != Some(line) {
                return Err(format!(
                    "its record at {place} does not declare this line as its integration_target"
                ));
            }
        }
        let mut anchors = vec![(tip, "the line's tip")];
        if let Some(authority) = names.candidate {
            anchors.push((authority, "the candidate authority"));
        }
        match codeflow_core::workgraph::task_line_binding(root, task, member, line, &anchors)
            .map_err(|error| error.to_string())?
        {
            EpicLineBinding::Bound => Ok(()),
            EpicLineBinding::Refused(reason) | EpicLineBinding::Unbound(reason) => {
                Err(format!("the line {reason}, so it is not a bound epic line"))
            }
        }
    };
    let mut bind = |member: Member| -> Result<Bound, String> {
        let Some(task) = completed_task(root, member)? else {
            return Ok(Bound::Frozen {
                task: None,
                note: None,
            });
        };
        Ok(match line_binding(&task, &member.head.to_string()) {
            Ok(()) => Bound::Own(task),
            Err(note) => Bound::Frozen {
                task: Some(task),
                note: Some(note),
            },
        })
    };
    candidate_findings(root, &candidate, head, &mut bind)
}

/// The remote-tracking ref of the judged branch that this run replaces
/// (TSK-350 AC-4, AC-5), by full name and its exact tip: the destination's
/// copy of a task branch, `refs/remotes/<remote>/<branch>`, where
/// `<remote>` is the one configured remote whose URL is the destination.
/// On the pre-push hand-off its tip must also be the old head the
/// destination advertises for the branch, the head the push replaces. A
/// protected branch, a branch carrying no task at the head, an ambiguous
/// or missing remote and a tracking ref behind or ahead of the destination
/// leave nothing out.
fn replaced_ref(
    root: &Path,
    git: &GitPolicy,
    range: &Range<'_>,
    names: &super::Names<'_>,
    push: bool,
) -> Option<Replaced> {
    let branch = names.branch;
    if git.branch_is_protected_any_case(branch) || git.branch_folds_onto_protected(branch) {
        return None;
    }
    if !matches!(judged_task(root, git, branch, range.head), Ok(Some(_))) {
        return None;
    }
    let url = names.destination?;
    // A read that fails sets nothing aside, the strict side.
    let replaced = Replaced::tracking(root, url, branch).ok().flatten()?;
    if push {
        let destination = release_line::from_advertisement(url, names.advertisement?)
            .ok()?
            .with_limits(names.limits);
        let advertised = destination
            .heads
            .iter()
            .find(|(head, _)| head == branch)
            .map(|(_, tip)| *tip)?;
        if advertised != replaced.tip {
            return None;
        }
    }
    Some(replaced)
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
        Err(error) => return Some(Err(unscoped(error.message()))),
    };
    // The scope this range is judged under: a result, not a finding. The
    // pre-push hook states a release push's scope itself.
    eprintln!(
        "codeflow ci: acceptance: release range ('{}'{}) under pattern '{}' ({}); each change is judged where it was introduced (SPC-013 R-120)",
        codeflow_core::git::display_text(names.branch),
        names
            .into
            .map(|into| format!(" into '{}'", codeflow_core::git::display_text(into)))
            .unwrap_or_default(),
        codeflow_core::git::display_text(&scope.pattern),
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

/// Acquisition failures stay separate from policy and history failures.
pub(super) enum ScopeError {
    Destination(String),
    Scope(String),
}

impl ScopeError {
    fn message(&self) -> &str {
        match self {
            Self::Destination(message) | Self::Scope(message) => message,
        }
    }
}

/// The destination and scope of a release range, asked once per run and
/// shared by the records rule and the acceptance check; `None` for an
/// ordinary range.
pub(super) fn release_scope<'n>(
    root: &Path,
    names: &'n super::Names<'_>,
) -> Result<Option<&'n (release_line::Destination, release_line::Scope)>, &'n ScopeError> {
    names
        .release
        .get_or_init(|| {
            let destination = match (names.destination, names.advertisement) {
                (Some(url), Some(listed)) => release_line::from_advertisement(url, listed)
                    .map(|destination| destination.with_limits(names.limits)),
                _ => release_line::ask_destination(root, names.destination, names.limits),
            }
            .map_err(|why| ScopeError::Destination(credential_hint(why)))?;
            let scope = release_line::scope(root, &destination, names.branch, names.into)
                .map_err(ScopeError::Scope)?;
            Ok(scope.release().then_some((destination, scope)))
        })
        .as_ref()
        .map(Option::as_ref)
}

/// `why` the destination could not be asked, with the fix when it refused
/// for want of a credential (TSK-1267). Release scope is unknown until the
/// destination answers, so the message names no kind of range.
fn credential_hint(why: String) -> String {
    if !why.contains(codeflow_core::git::remote_query::AUTHENTICATION_FAILED) {
        return why;
    }
    format!("{why}: {CREDENTIAL_NEEDED}")
}

/// The fix for a destination lookup refused for want of a credential.
const CREDENTIAL_NEEDED: &str = "The destination requires a credential this run does not have. Codeflow needs its default branch and policy to determine release scope. Give the job read access to the destination.";

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
/// acceptance check; `None` when none or several are eligible. An unreadable
/// range returns an error that classification refuses.
pub(super) fn release_owner(
    root: &Path,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Result<Option<String>, String> {
    let Some((destination, scope)) =
        release_scope(root, names).map_err(|error| error.message().to_string())?
    else {
        return Ok(None);
    };
    let head = release_head(root, range, scope)?;
    release_line::release_owner(root, destination, range.base, &head)
}

/// On a release range, the records it brings from verified lines, for the
/// records rule to judge where each was introduced (SPC-013 R-120). `None`
/// on an ordinary range. An unreadable range returns an error that the
/// record checks refuse.
pub(super) fn brought(
    root: &Path,
    range: &Range<'_>,
    names: &super::Names<'_>,
) -> Result<Option<codeflow_core::workgraph::lifecycle::Brought>, String> {
    let Some((destination, scope)) =
        release_scope(root, names).map_err(|error| unscoped(error.message()))?
    else {
        return Ok(None);
    };
    let head = release_head(root, range, scope)?;
    release_line::release_findings(root, destination, range.base, &head)
        .map(|judged| Some(judged.brought))
}

pub(super) fn scope_refusal(error: &ScopeError) -> Violation {
    if let ScopeError::Destination(message) = error {
        return Violation::always_blocking(
            release_line::DESTINATION_RULE,
            unscoped(message),
            &codeflow_core::remedy::PUSH_DESTINATION_SILENT.remedy(),
        );
    }
    Violation::always_blocking(
        FROZEN_RULE,
        unscoped(error.message()),
        "repair the destination or policy named above, then rerun codeflow ci",
    )
}

fn unscoped(error: &str) -> String {
    format!("whether this is a release range cannot be decided, so nothing is judged under the ordinary rules instead (SPC-013 R-120): {error}")
}

/// Criteria may change only in a planning amendment (ADR-0078), on a
/// validated epic line (R-52) or on the workspace root branch the target's
/// policy names, decided from the validated class, never from the branch
/// prefix alone. A tracked range may also correct the criteria text of a
/// completed standalone task it names or follows up (ADR-0080), off that
/// task's own branch. Without a class (no pull request body, as the pre-push
/// hook runs), the range must itself carry only what a planning amendment
/// may, `AGENTS.md`'s managed block read from the target at the base, on a
/// branch that carries no task; or be the root branch or a verified epic
/// line. A push names no epics yet, so its pull request judges their scope.
fn criteria(
    root: &Path,
    git: &GitPolicy,
    range: &Range<'_>,
    branch: &str,
    class: Option<&Class>,
) -> Result<Criteria, String> {
    // The branch's task, from the judged head under the run's policy
    // (TSK-350 AC-1): the same head gets the same answer from any checkout.
    let carried = judged_task(root, git, branch, range.head)?;
    // A release range is judged by `judge` and does not reach here; were it
    // to, the release pull request keeps its task's criteria as tracked work.
    if let Some(task_id) = class.and_then(Class::task_id) {
        let own_branch = carried.as_deref() == Some(task_id);
        return codeflow_core::workgraph::acceptance::task_criteria(
            root, range.base, task_id, own_branch,
        );
    }
    // Without a pull request body (a push), a task branch's only valid class
    // is its own task, so it may change that task's criteria as its PR may.
    if class.is_none() {
        if let Some(task_id) = carried.clone() {
            return codeflow_core::workgraph::acceptance::task_criteria(
                root, range.base, &task_id, true,
            );
        }
    }
    let amendable = match class {
        Some(Class::PlanningOnly { epics }) => return Ok(Criteria::Amendment(Some(epics.clone()))),
        Some(Class::EpicLine(_) | Class::RootBranch(_)) => true,
        Some(_) => false,
        None => {
            let root_branch = root_branch_at(root, range.base)?.as_deref() == Some(branch);
            let line = (!root_branch && branch.starts_with("integration/"))
                .then(|| check_epic_line(root, branch, range.target, range.base, range.head));
            if root_branch || matches!(line, Some(Ok(_))) {
                true
            } else if carried.is_none() && planning_amendment_range(root, range)? {
                return Ok(Criteria::Amendment(None));
            } else {
                // Only now are the criteria frozen, so only now say why.
                if let Some(Err(reason)) = line {
                    frozen_line_note(branch, &reason);
                }
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

/// Print once why `branch`, an `integration/` head judged without a pull
/// request body, is judged as frozen task work: it is not a verified epic
/// line, so each completion binds at the range head (TSK-433 AC-7, issue
/// #341). Printed only once the criteria are frozen, never when the range
/// falls back to a planning amendment; a pull request's classification
/// already names the reason.
fn frozen_line_note(branch: &str, reason: &str) {
    let note = codeflow_core::remedy::Finding::new(
        format!(
            "'{}' is not a verified epic line, so its completions bind at the range head with frozen criteria: {reason}",
            codeflow_core::git::display_text(branch)
        ),
        codeflow_core::remedy::UNVERIFIED_LINE_FROZEN.remedy(),
    );
    println!("{}", note.line("codeflow ci: acceptance", "note"));
}

/// Whether the range carries only what a planning amendment may, read
/// against the target at the base (ADR-0078): the push-side twin of the
/// planning class.
fn planning_amendment_range(root: &Path, range: &Range<'_>) -> Result<bool, String> {
    let paths: Vec<codeflow_core::git::GitName> = range_changes(root, range.base, range.head)?
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
/// A criterion `found` names (`<task> <criterion>`) that a task removes
/// from its own record (TSK-451): the pull request body gives the reason on
/// a visible line naming the criterion and its removal, or the removal is
/// refused, so weakening a criterion stays visible to the review. A push
/// has no body yet and is judged on its pull request.
fn removal(body: Option<&str>, found: &Finding) -> Option<super::TaggedViolation> {
    let (task, criterion) = found.message.split_once(' ')?;
    let Some(body) = body else {
        let note = codeflow_core::remedy::Finding::new(
            format!("{REMOVAL_RULE}: {task} removes {criterion}; its pull request body gives the reason"),
            codeflow_core::remedy::CRITERIA_DELTA.remedy(),
        );
        println!("{}", note.line("codeflow ci", "note"));
        return None;
    };
    if super::pr_body::gives_removal_reason(body, criterion) {
        println!("codeflow ci: {task} removes {criterion}, with the reason in the PR body");
        return None;
    }
    Some(super::TaggedViolation {
        sha: None,
        violation: Violation::always_blocking(
            REMOVAL_RULE,
            format!("{task} removes its criterion {criterion}, and the PR body gives no reason for it"),
            "give the reason in the PR body on a line naming the criterion, for example `- AC-2 removed: <why the remaining criteria suffice>`, or keep the criterion",
        ),
    })
}

fn violation(git: &GitPolicy, found: Finding) -> super::TaggedViolation {
    let violation = if found.rule == FROZEN_RULE {
        Violation::always_blocking(
            found.rule,
            found.message,
            &codeflow_core::remedy::CRITERIA_FROZEN.remedy(),
        )
    } else if found.rule == IMPORT_RULE {
        Violation::always_blocking(
            found.rule,
            found.message,
            "fetch the named source history; repair its proof through the source task's pull request, then merge the reviewed result",
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
