//! Claim-time bases live in Git, independently of the coordination ledger.
//!
//! The private ref points to an annotated object whose target is the base
//! commit. Its message binds the source and pins to a task, branch and target.
//! This is local evidence shared by linked worktrees, not a planning authority.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use git2::{Oid, Repository};
use serde::{Deserialize, Serialize};

use super::deps::DependencyKind;
use super::history;
use super::work_start::{self, Record, ReviewedPin};

const CLAIM_VERSION: u32 = 1;
const REVIEWED_SOURCE: &str = "reviewed pins";
const CLAIM_TAGGER_NAME: &str = "CodeFlow";
const CLAIM_TAGGER_EMAIL: &str = "codeflow@localhost";

/// The fixed base a claim selected, or the live base an unclaimed start needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    /// Exact stable source ref, or `reviewed pins` for an unlanded stack.
    pub source: String,
    /// Full commit identity, never a rev expression.
    pub revision: String,
    /// Reviewed pins at claim or its last explicit validated renewal.
    /// Replacement pins must name these same predecessors.
    pub pins: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    version: u32,
    task: String,
    branch: String,
    target: String,
    base: Base,
}

fn records(repo: &Repository, oid: Oid) -> Result<BTreeMap<String, Record>, String> {
    let tree = repo
        .find_commit(oid)
        .and_then(|c| c.tree())
        .map_err(|e| e.to_string())?;
    work_start::records_from_tree(repo, &tree).map_err(|e| e.to_string())
}

fn contains(repo: &Repository, head: Oid, base: Oid) -> Result<bool, String> {
    if head == base {
        return Ok(true);
    }
    history::descendant_of(repo, head, base).map_err(|e| e.to_string())
}

/// Whether the named head contains the exact base commit.
///
/// # Errors
/// Refuses missing objects, incomplete history and local history overlays.
pub fn head_contains(root: &Path, head: &str, base: &str) -> Result<bool, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    history::check(&repo).map_err(|e| e.to_string())?;
    let head = Oid::from_str(head).map_err(|e| e.to_string())?;
    let base = Oid::from_str(base).map_err(|e| e.to_string())?;
    repo.find_commit(head).map_err(|e| e.to_string())?;
    repo.find_commit(base).map_err(|e| e.to_string())?;
    contains(&repo, head, base)
}

/// Resolve only a fetched ref owned by the source's configured remote.
pub(crate) fn source(repo: &Repository, target: &str) -> Result<(String, Oid), String> {
    if !work_start::is_stable_work_target(target) {
        return Err(format!("'{target}' is not a stable work source"));
    }
    if let Some((shown, oid)) =
        work_start::tracking_reference(repo, target).map_err(|e| e.to_string())?
    {
        let full = if repo.remotes().map_err(|e| e.to_string())?.is_empty() {
            format!("refs/heads/{shown}")
        } else {
            format!("refs/remotes/{shown}")
        };
        return Ok((full, oid));
    }
    // A review-only remote does not turn an otherwise local target into
    // an origin target. Only the declared source's remote owns its history.
    let origin = match repo.find_remote("origin") {
        Ok(_) => true,
        Err(e) if e.code() == git2::ErrorCode::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    if super::readiness::target_fetch_remote(repo, target)?.is_none() && !origin {
        if let Some(commit) =
            work_start::target_reference(repo, target).map_err(|e| e.to_string())?
        {
            return Ok((
                format!("refs/heads/{}", work_start::logical_target(target)),
                commit.id(),
            ));
        }
    }
    Err(format!(
        "source '{target}' is not fetched here; fetch its remote"
    ))
}

/// Choose the source holding all required code, keeping the task's target.
///
/// # Errors
/// Refuses unreadable history, missing sources and two unmerged sources.
pub fn select(root: &Path, task: &str, target: &str, pins: &[ReviewedPin]) -> Result<Base, String> {
    let _judgment = history::judgment();
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    history::check(&repo).map_err(|e| e.to_string())?;
    let (target_ref, tip) = source(&repo, target)?;
    let at_target = records(&repo, tip)?;
    let mut sources = BTreeMap::new();
    if let Some(record) = at_target.get(task) {
        for dep in &record.depends_on {
            if dep.kind != DependencyKind::Code || pins.iter().any(|p| p.task_id() == dep.id) {
                continue;
            }
            let Some(predecessor) = at_target.get(&dep.id) else {
                continue;
            };
            if predecessor.status == "complete" {
                continue;
            }
            let Some(line) = predecessor.integration_target.as_deref().filter(|line| {
                work_start::logical_target(line) != work_start::logical_target(target)
            }) else {
                continue;
            };
            let (reference, landed) = source(&repo, line)?;
            if records(&repo, landed)?
                .get(&dep.id)
                .is_some_and(|r| r.status == "complete")
                && !contains(&repo, tip, landed)?
            {
                sources.insert(reference, landed);
            }
        }
    }
    // A source is a tracking ref that holds its remote's name, shown only
    // when plain (TSK-416 review round 7).
    let shown = |reference: &str| crate::security::git::shown_ref(root, reference);
    if sources.len() > 1 {
        return Err(format!("predecessors require different unmerged sources: {}; use the line sync pull request route (issue #280) to bring them onto the target first", sources.keys().map(|source| shown(source)).collect::<Vec<_>>().join(", ")));
    }
    let (mut reference, mut base) = sources.into_iter().next().unwrap_or((target_ref, tip));
    if base != tip && pins.is_empty() {
        let common = history::merge_base(&repo, base, tip).map_err(|e| e.to_string())?;
        let mut planning = records(&repo, common)?;
        project_dependencies(&repo, &mut planning, task, common, base, pins)?;
        work_start::validate_anchored_task_on(&repo, &planning, task, target, pins)
            .map_err(|error| format!("source {}@{base} lacks the target's required planning anchor: {error}; use the line sync pull request route (issue #280) first", shown(&reference)))?;
    }
    if let Some(stack) = work_start::stack_base(root, pins)? {
        if base != tip && !contains(&repo, stack, base)? {
            return Err(format!("reviewed stack does not contain {}@{base}; use the line sync pull request route (issue #280) first", shown(&reference)));
        }
        reference = REVIEWED_SOURCE.into();
        base = stack;
    }
    Ok(Base {
        source: reference,
        revision: base.to_string(),
        pins: pins
            .iter()
            .map(|p| format!("{}@{}", p.task_id(), p.revision()))
            .collect(),
    })
}

fn claim_ref(branch: &str) -> String {
    format!("refs/codeflow/claims/{branch}")
}

fn pin_tasks(values: &[String]) -> Result<BTreeSet<String>, String> {
    let mut tasks = BTreeSet::new();
    for value in values {
        let (task, revision) = value.split_once('@').ok_or("malformed claim pin")?;
        if !super::is_valid_task_format_id(task)
            || revision.len() != 40
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !tasks.insert(task.to_string())
        {
            return Err("malformed or repeated claim pin".into());
        }
    }
    Ok(tasks)
}

/// Persist a new claim or an explicitly validated reviewed-claim renewal.
///
/// # Errors
/// Refuses mismatched old metadata, an invalid base or a failed Git write.
pub(crate) fn record(
    root: &Path,
    task: &str,
    branch: &str,
    target: &str,
    base: &Base,
) -> Result<(), String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    // Branch allocation and readiness already refused an active claim.
    // A ref left after deleting an old branch belongs to the previous claim.
    let replace_old = recorded(root, task, branch, target)?.is_some();
    let oid = Oid::from_str(&base.revision).map_err(|e| e.to_string())?;
    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
    let message = serde_json::to_string(&Claim {
        version: CLAIM_VERSION,
        task: task.into(),
        branch: branch.into(),
        target: work_start::logical_target(target).into(),
        base: base.clone(),
    })
    .map_err(|e| e.to_string())?;
    // The tag is CodeFlow plumbing, never authored work: its tagger is a fixed
    // identity, so a claim needs no `user.name` or `user.email` (a hosted
    // runner or a fresh machine has neither) and never reads or writes them.
    let signature =
        git2::Signature::now(CLAIM_TAGGER_NAME, CLAIM_TAGGER_EMAIL).map_err(|e| e.to_string())?;
    let tag = repo
        .tag_annotation_create(branch, commit.as_object(), &signature, &message)
        .map_err(|e| e.to_string())?;
    repo.reference(
        &claim_ref(branch),
        tag,
        replace_old,
        "record work claim base",
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Read local claim evidence; a malformed ref is never treated as absent.
///
/// # Errors
/// Refuses corrupt objects and metadata for another task, branch or target.
pub fn recorded(
    root: &Path,
    task: &str,
    branch: &str,
    target: &str,
) -> Result<Option<Base>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let reference = match repo.find_reference(&claim_ref(branch)) {
        Ok(r) => r,
        Err(e) if e.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read claim base: {e}")),
    };
    let tag = reference
        .peel_to_tag()
        .map_err(|e| format!("cannot read claim base: {e}"))?;
    let claim: Claim = serde_json::from_str(
        tag.message()
            .map_err(|e| e.to_string())?
            .ok_or("claim base has no message")?,
    )
    .map_err(|e| format!("cannot read claim base: {e}"))?;
    let base_oid = Oid::from_str(&claim.base.revision).map_err(|e| e.to_string())?;
    if claim.version != CLAIM_VERSION
        || claim.task != task
        || claim.branch != branch
        || claim.target != work_start::logical_target(target)
        || tag.target_id() != base_oid
    {
        return Err("claim base does not match this task, branch, target and commit".into());
    }
    repo.find_commit(tag.target_id())
        .map_err(|e| e.to_string())?;
    pin_tasks(&claim.base.pins)?;
    if (claim.base.source == REVIEWED_SOURCE) == claim.base.pins.is_empty() {
        return Err("claim source and reviewed pins disagree".into());
    }
    Ok(Some(claim.base))
}

struct Decision {
    base: Base,
    note: Option<String>,
    renew: bool,
}

/// Check containment and report a source advancing after a fixed claim.
///
/// # Errors
/// Refuses a wrong base or different pinned predecessors; callers choose policy level.
pub fn check(
    root: &Path,
    task: &str,
    branch: &str,
    target: &str,
    pins: &[ReviewedPin],
) -> Result<Option<String>, String> {
    decide(root, task, branch, target, pins).map(|decision| decision.note)
}

fn decide(
    root: &Path,
    task: &str,
    branch: &str,
    target: &str,
    pins: &[ReviewedPin],
) -> Result<Decision, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let recorded = recorded(root, task, branch, target)?;
    let fixed = recorded.is_some();
    let mut base = match recorded {
        Some(base) => base,
        None => select(root, task, target, pins)?,
    };
    let actual: BTreeSet<_> = pins.iter().map(|p| p.task_id().to_string()).collect();
    // Once the predecessor lands, start can omit --on. The ordinary
    // dependency checks after containment must then prove its completion;
    // a deleted review branch cannot keep a landed dependency unstartable.
    if !pins.is_empty() && pin_tasks(&base.pins)? != actual {
        return Err("claim's reviewed dependencies differ; supply --on pins for the same predecessors and verify their current review".into());
    }
    let supplied: BTreeSet<_> = pins
        .iter()
        .map(|p| format!("{}@{}", p.task_id(), p.revision()))
        .collect();
    let renew =
        fixed && !pins.is_empty() && base.pins.iter().cloned().collect::<BTreeSet<_>>() != supplied;
    if renew {
        // The caller's explicit new pins have current native review evidence.
        // This proposal remains read-only until all planning checks pass.
        base = select(root, task, target, pins)?;
    }
    let oid = Oid::from_str(&base.revision).map_err(|e| e.to_string())?;
    let head = repo
        .head()
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| e.to_string())?
        .id();
    if !contains(&repo, head, oid)? {
        return Err(format!("HEAD does not contain required base {}@{}; start new work with `codeflow work claim {task}`", base.source, base.revision));
    }
    if fixed && base.pins.is_empty() {
        let (_, now) = source(&repo, &base.source)?;
        if now != oid {
            if !contains(&repo, now, oid)? {
                return Err(format!("source {} no longer contains the recorded base {oid}; reconcile its history before starting", base.source));
            }
            let note = Some(format!(
                "source {} advanced to {now}; recorded base {oid} remains valid",
                base.source
            ));
            return Ok(Decision { base, note, renew });
        }
    }
    Ok(Decision {
        base,
        note: None,
        renew,
    })
}

/// Persist an explicit reviewed-pin renewal after the caller's planning check.
///
/// # Errors
/// Refuses a changed branch, invalid base or failed metadata write. The CLI
/// treats a write failure as missing admission evidence at every policy level.
pub fn renew_reviewed(
    root: &Path,
    task: &str,
    branch: &str,
    target: &str,
    pins: &[ReviewedPin],
) -> Result<Option<String>, String> {
    let decision = decide(root, task, branch, target, pins)?;
    if !decision.renew {
        return Ok(None);
    }
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let head = repo.head().map_err(|e| e.to_string())?;
    let current = crate::git::name::reference_shorthand(&head);
    if current.rule_text().map_err(|e| e.to_string())? != branch {
        return Err("branch changed before recording the reviewed claim".into());
    }
    record(root, task, branch, target, &decision.base)?;
    Ok(Some(format!(
        "reviewed claim renewed at {}",
        decision.base.revision
    )))
}

/// Project only landed predecessor records from verified shared history.
/// The task, epic and specs retain the target's planning authority.
pub(crate) fn project_dependencies(
    repo: &Repository,
    anchored: &mut BTreeMap<String, Record>,
    task: &str,
    anchor: Oid,
    head: Oid,
    pins: &[ReviewedPin],
) -> Result<(), String> {
    let Some(record) = anchored.get(task) else {
        return Ok(());
    };
    let dependencies = record.depends_on.clone();
    let target = record.integration_target.clone();
    for dep in dependencies
        .into_iter()
        .filter(|d| d.kind == DependencyKind::Code && !pins.iter().any(|p| p.task_id() == d.id))
    {
        let Some(predecessor) = anchored.get(&dep.id) else {
            continue;
        };
        if predecessor.status == "complete" {
            continue;
        }
        let Some(line) = predecessor.integration_target.as_deref() else {
            continue;
        };
        if target.as_deref().is_some_and(|target| {
            work_start::logical_target(target) == work_start::logical_target(line)
        }) {
            continue;
        }
        let (_, tip) = source(repo, line)?;
        let common = history::merge_base(repo, head, tip).map_err(|e| e.to_string())?;
        if let Some(complete) = records(repo, common)?
            .remove(&dep.id)
            .filter(|r| r.status == "complete")
        {
            // A target that received this completion and then reopened the
            // predecessor remains incomplete. An unrelated source advance
            // must not restore the old completion over that newer decision.
            let shared = history::merge_base(repo, anchor, tip).map_err(|e| e.to_string())?;
            if records(repo, shared)?.get(&dep.id).is_some_and(|r| {
                r.status == "complete"
                    && r.reviewed == complete.reviewed
                    && r.acceptance == complete.acceptance
            }) {
                continue;
            }
            anchored.insert(dep.id, complete);
        }
    }
    Ok(())
}
