//! What a planning amendment changes, reported per epic (ADR-0078,
//! SPC-013 R-52, R-70).
//!
//! One reviewed plan change may span several epics and carry its
//! instruction text, so its pull request names every epic it changes on one
//! `Task:` line. The amendment may change any task's criteria; the reviewer
//! sees each change here: per named epic, each task whose criteria change
//! with its delta, the records added or removed, the status transitions,
//! and the instruction and doc files the range touches. A record of an epic
//! the line does not name is refused; a standalone task or a spec belongs
//! to no epic and is listed, never refused. A criteria change to a task a
//! completion of which landed is flagged, since its acceptance block was
//! reviewed against the earlier criteria (R-119); whether one landed is read
//! from the judged history, not the tip (TSK-234), and a record the target
//! deleted is compared with, and owned as, its newest judged version. The
//! amendment's permission never depends on that answer: the reopen freeze
//! is checked before it. Before any of that, [`range_problem`] keeps
//! product, instruction and enforcement paths out of the range. The
//! amendment lands on
//! the target, and a line takes it by merging the target; a record that
//! changed on its line since the line last merged the target is flagged.

use crate::workgraph::history;

use std::collections::{BTreeMap, BTreeSet};

use git2::{Oid, Repository};

use super::acceptance::{blob_at, criteria_delta, finding, target_tips, Finding};
use super::classify::{amendment_path, instructions_block_unchanged, AmendmentPath, ProjectPaths};
use super::lifecycle::{Graph, RecordView};
use super::work_start::RecordKind;

/// The planning amendment's report (notes) and its epic scope (a
/// refusal). Always blocking when it refuses: the scope is never
/// adjustable.
pub const AMENDMENT_RULE: &str = "work.planning_amendment";

/// The group a change is reported under: an epic, or none.
const NO_EPIC: &str = "no epic";

/// The findings of a planning amendment from the target graph `target` (at
/// `target_tip`) to `head`, over the paths the range changes. `named` holds
/// the epics the pull request's `Task:` line names, or `None` for a push,
/// which has no body yet and is judged on its pull request.
#[must_use]
pub fn findings(
    repo: &Repository,
    target: &Graph,
    head: &Graph,
    changed_paths: &[String],
    named: Option<&[String]>,
    target_tip: Oid,
    judging: &mut super::acceptance::Judging<'_>,
) -> Vec<Finding> {
    let changed: BTreeSet<&str> = changed_paths.iter().map(String::as_str).collect();
    let ids: BTreeSet<&str> = head
        .records
        .values()
        .chain(target.records.values())
        .filter(|record| changed.contains(record.path.as_str()))
        .map(|record| record.id.as_str())
        .collect();
    let mut refused = Vec::new();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for id in ids {
        let after = head.records.get(id);
        let tip = target.records.get(id);
        let (recovered, landed) = judged(tip, after, judging);
        let before = tip.or(recovered.as_ref());
        let owners = owners(before, after);
        if let Some(named) = named {
            for owner in owners.iter().filter(|owner| !named.contains(owner)) {
                let which = if owner == id {
                    format!("{id} is an epic")
                } else {
                    format!("{id} belongs to {owner}")
                };
                refused.push(finding(
                    AMENDMENT_RULE,
                    format!(
                        "{which}, which the `Task:` line does not name; a planning amendment names every epic whose records it changes"
                    ),
                ));
            }
        }
        let mut lines = changes(id, before, after, &landed);
        if recovered.is_some() {
            lines.push(format!(
                "{id} is added again: the target deleted its record, so it is compared with its newest judged version"
            ));
        }
        if let Some(line) = before.and_then(|record| newer_on_line(repo, target_tip, record)) {
            lines.push(line);
        }
        let keys: Vec<String> = if owners.is_empty() {
            vec![NO_EPIC.to_string()]
        } else {
            owners.into_iter().collect()
        };
        for key in keys {
            groups.entry(key).or_default().extend(lines.iter().cloned());
        }
    }
    // range_problem has already admitted these paths. Outside the record
    // home they are plans, docs, declared indexes or root AGENTS.md.
    let files: Vec<&str> = changed_paths
        .iter()
        .map(String::as_str)
        .filter(|path| !path.starts_with("project-management/"))
        .collect();
    let mut found = refused;
    // Named epics first, in the order the line names them, then the rest.
    let order: Vec<String> = named
        .unwrap_or_default()
        .iter()
        .cloned()
        .chain(groups.keys().cloned())
        .collect();
    let mut seen = BTreeSet::new();
    for key in order {
        if !seen.insert(key.clone()) {
            continue;
        }
        for line in groups.get(&key).into_iter().flatten() {
            found.push(note(format!("{key}: {line}")));
        }
    }
    if !files.is_empty() {
        found.push(note(format!(
            "instruction and doc files: {}",
            files
                .iter()
                .map(|path| if *path == "AGENTS.md" {
                    "AGENTS.md (outside its managed block)".to_string()
                } else {
                    crate::git::display_key(path)
                })
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    found
}

fn note(message: String) -> Finding {
    Finding {
        rule: AMENDMENT_RULE,
        message,
        note: true,
        epic_record: None,
    }
}

/// The epics a changed record belongs to: an epic record is its own; a task
/// belongs to its epic on either side, so moving a task between epics
/// changes both; a standalone task and a spec belong to none.
fn owners(before: Option<&RecordView>, after: Option<&RecordView>) -> BTreeSet<String> {
    let mut owners = BTreeSet::new();
    for record in before.into_iter().chain(after) {
        match record.kind {
            RecordKind::Epic => {
                owners.insert(record.id.clone());
            }
            RecordKind::Task => {
                if let Some(epic) = record
                    .epic_id
                    .as_deref()
                    .filter(|epic| !epic.is_empty() && *epic != "null")
                {
                    owners.insert(epic.to_string());
                }
            }
            RecordKind::Spec => {}
        }
    }
    owners
}

fn kind_name(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Epic => "epic",
        RecordKind::Spec => "spec",
        RecordKind::Task => "task",
    }
}

/// For a task record whose criteria the amendment changes, or that the
/// target no longer holds, its judgement (TSK-234): the newest judged
/// version when the target deleted the record, which also names its epic,
/// and whether a completion of it landed.
fn judged(
    tip: Option<&RecordView>,
    after: Option<&RecordView>,
    judging: &mut super::acceptance::Judging<'_>,
) -> (Option<RecordView>, Landed) {
    let Some(record) = after.filter(|record| record.kind == RecordKind::Task) else {
        return (None, Landed::No);
    };
    if tip.is_some_and(|tip| tip.criteria.signature() == record.criteria.signature()) {
        return (None, Landed::No);
    }
    let judgement = judging.judge(record);
    let recovered = match (&judgement.criteria, tip) {
        (super::landing::Authority::Held(kept), None) => Some(kept.as_ref().clone()),
        _ => None,
    };
    let landed = match (&judgement.landing, judgement.landing_unknown()) {
        (super::landing::Landing::Landed { .. }, _) => Landed::Yes,
        (super::landing::Landing::Planned, _) => Landed::No,
        (_, reason) => Landed::Unknown(reason.unwrap_or_default()),
    };
    (recovered, landed)
}

/// Whether a completion of a task landed, for the R-119 flag.
enum Landed {
    No,
    Yes,
    Unknown(String),
}

/// What changed in one record, one line per change.
fn changes(
    id: &str,
    before: Option<&RecordView>,
    after: Option<&RecordView>,
    landed: &Landed,
) -> Vec<String> {
    let (before, after) = match (before, after) {
        (None, Some(after)) => {
            return vec![format!(
                "{id} added ({} record, {})",
                kind_name(after.kind),
                after.status
            )]
        }
        (Some(before), None) => {
            return vec![format!("{id} removed ({} record)", kind_name(before.kind))]
        }
        (Some(before), Some(after)) => (before, after),
        (None, None) => return Vec::new(),
    };
    let mut lines = Vec::new();
    if before.status != after.status {
        lines.push(format!("{id} status {} -> {}", before.status, after.status));
    }
    if before.epic_id != after.epic_id {
        let name = |epic: &Option<String>| epic.clone().unwrap_or_else(|| "none".to_string());
        lines.push(format!(
            "{id} moves from epic {} to {}",
            name(&before.epic_id),
            name(&after.epic_id)
        ));
    }
    if after.kind == RecordKind::Task && before.criteria.signature() != after.criteria.signature() {
        let delta = criteria_delta(&before.criteria.items, &after.criteria.items);
        if !delta.is_empty() {
            lines.push(format!("{id} criteria delta: {}", delta.join("; ")));
        }
        match landed {
            Landed::Yes if after.status == "complete" => lines.push(format!(
                "{id} is complete, and its acceptance block was reviewed against the earlier criteria: confirm that completion still holds, or reopen the task in its own pull request (SPC-013 R-119)"
            )),
            Landed::Yes => lines.push(format!(
                "{id} landed a completion earlier, so its next completion is held to these criteria: confirm they are what its fix must meet (SPC-013 R-119)"
            )),
            Landed::Unknown(reason) => lines.push(format!(
                "{id}: whether a completion of it landed cannot be told ({reason}): confirm any completion still holds, or reopen the task in its own pull request (SPC-013 R-119)"
            )),
            Landed::No => {}
        }
    }
    if lines.is_empty() {
        lines.push(format!("{id} changed, with no status or criteria change"));
    }
    lines
}

/// When `record` is a task bound for an integration line and its record
/// changed on that line since the line last merged the target, the note
/// that flags it: the amendment lands on the target and the line takes it
/// by merging the target, so the record there needs checking.
fn newer_on_line(repo: &Repository, target_tip: Oid, record: &RecordView) -> Option<String> {
    if record.kind != RecordKind::Task {
        return None;
    }
    let line = record.integration_target.as_deref()?;
    if !line.starts_with("integration/") {
        return None;
    }
    let tips = match target_tips(repo, line) {
        Ok(tips) => tips,
        Err(error) => return Some(format!("cannot read task target {line}: {error}")),
    };
    tips.into_iter().find_map(|(name, tip)| {
        let shared = match history::merge_base(repo, tip, target_tip) { Ok(shared) => shared, Err(error) => return Some(format!("cannot read task target history: {error}")) };
        let before = match blob_at(repo, shared, &record.path) { Ok(value) => value, Err(error) => return Some(error) };
        let after = match blob_at(repo, tip, &record.path) { Ok(value) => value, Err(error) => return Some(error) };
        (after != before).then(|| {
            format!(
                "{} is newer on {name} than on the target; this amendment lands on the target, so merge the target into {line} and check the record there",
                record.id
            )
        })
    })
}

/// Who judges a standalone cancellation, and with what it binds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationJudge<'a> {
    /// A pull request whose `Task:` line names this task on a `plan/`
    /// branch: the range must be exactly that one cancellation.
    PullRequest(&'a str),
    /// A push, which has no `Task:` line yet. The same range may be an
    /// epic's planning amendment, so only what no `Task:` line could make
    /// valid is refused.
    Push,
}

/// Identify a standalone cancellation carried by a planning branch: a
/// standalone task record absent from the merge-base and added at
/// `cancelled`. Any other change to a standalone record is an ordinary
/// planning amendment and returns `None`, so it keeps its existing route.
/// Callers restrict this check to `plan/` branches.
///
/// For a pull request ([`CancellationJudge::PullRequest`]) the range must
/// be that one record, inside `project-management/`, with its reason and
/// scope; the named task's record already on the target is refused toward
/// the task status route. A push ([`CancellationJudge::Push`]) refuses
/// only a new cancelled record without its reason or scope, or a range a
/// planning amendment cannot carry, and always returns `None`.
///
/// # Errors
/// Returns a diagnostic for unreadable history or an invalid cancellation
/// range, including a record that fails the full task schema.
pub fn cancelled_standalone(
    root: &std::path::Path,
    base: &str,
    head: &str,
    paths: &[crate::git::GitName],
    judge: CancellationJudge<'_>,
) -> Result<Option<String>, String> {
    let repo = Repository::discover(root).map_err(|error| error.to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| error.to_string())
    };
    let (base, head) = (oid(base)?, oid(head)?);
    let anchor = history::merge_base(&repo, base, head).map_err(|error| error.to_string())?;
    let before = Graph::from_revision(&repo, &anchor.to_string())?;
    let after = Graph::from_revision(&repo, &head.to_string())?;
    let changed = |record: &&RecordView| {
        paths
            .iter()
            .any(|path| path.bytes() == record.path.as_bytes())
    };
    let standalone =
        |record: &&RecordView| record.kind == RecordKind::Task && record.epic_id.is_none();
    let candidates: Vec<_> = after
        .records
        .values()
        .filter(changed)
        .filter(standalone)
        .filter(|record| !before.records.contains_key(&record.id) && record.status == "cancelled")
        .collect();
    let task = match judge {
        CancellationJudge::Push => {
            if candidates.is_empty() {
                return Ok(None);
            }
            for record in &candidates {
                cancellation_lines(record)?;
            }
            return match range_problem(&repo, base, head, &rule_texts(paths)?) {
                Some(problem) => Err(problem),
                None => Ok(None),
            };
        }
        CancellationJudge::PullRequest(task) => task,
    };
    if candidates.is_empty() {
        let landed = after
            .records
            .values()
            .filter(changed)
            .filter(standalone)
            .any(|record| {
                record.id == task
                    && record.status == "cancelled"
                    && before.records.contains_key(&record.id)
            });
        if landed {
            return Err(format!(
                "{task} is already on the target, so this is not a new cancelled standalone record; cancel it with `codeflow task status {task} cancelled --reason <why> --scope <where it went>` in its own task pull request"
            ));
        }
        return Ok(None);
    }
    let [record] = candidates.as_slice() else {
        return Err("a standalone cancellation amendment names and adds one task record".into());
    };
    let id = &record.id;
    cancellation_lines(record)?;
    let texts = rule_texts(paths)?;
    let history = history_paths(&repo, base, head)?;
    if texts
        .iter()
        .chain(&history)
        .any(|path| !path.starts_with("project-management/"))
    {
        return Err("a standalone cancellation amendment changes only project-management/ paths, in every commit as well as the final diff; keep product changes on the task branch".into());
    }
    schema_problem(record)?;
    if let Some(problem) = range_problem(&repo, base, head, &texts) {
        return Err(problem);
    }
    if before
        .records
        .values()
        .chain(after.records.values())
        .filter(changed)
        .any(|other| other.id != *id)
    {
        return Err(format!(
            "a standalone cancellation amendment for {id} cannot change another work record"
        ));
    }
    Ok(Some(id.clone()))
}

/// The full task schema of an incoming cancelled record (TSK-1001): the
/// lifecycle view carries no title or standalone reason, so the record is
/// validated as `codeflow validate` validates a task file.
fn schema_problem(record: &RecordView) -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    let name = record
        .path
        .rsplit('/')
        .next()
        .unwrap_or("record.md")
        .to_string();
    let folder = dir.path().join("project-management/tasks");
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let path = folder.join(name);
    std::fs::write(&path, &record.content).map_err(|error| error.to_string())?;
    let (errors, _) =
        crate::validate::validate_task(&path, &crate::validate::ValidateOptions::default())
            .map_err(|error| format!("{} is not a valid task record: {error}", record.id))?;
    if errors.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} is not a valid task record: {}",
        record.id,
        errors
            .iter()
            .map(|error| format!("{}: {}", error.field, error.message))
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

/// A cancelled record carries its reason and scope in the Closeout.
fn cancellation_lines(record: &RecordView) -> Result<(), String> {
    let missing = super::record_text::parse_cancellation(&record.body).missing();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} needs Closeout lines `- cancelled: <reason>` and `- scope: <where it went>` (missing {})",
        record.id,
        missing.join(", ")
    ))
}

fn rule_texts(paths: &[crate::git::GitName]) -> Result<Vec<String>, String> {
    paths
        .iter()
        .map(|path| path.rule_text().map(str::to_string))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

/// Why a planning amendment judged at `head` on the target `base` cannot
/// name `epic`: it has no epic record at the head, or it was cancelled
/// before this range. Cancelling it in this range is a planning change of
/// its own, so the target's status decides.
#[must_use]
pub fn epic_problem(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    epic: &str,
) -> Option<String> {
    let status = |revision: &str| -> Result<Option<String>, String> {
        let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
        let graph = Graph::from_revision(&repo, revision)?;
        Ok(graph
            .records
            .get(epic)
            .filter(|record| record.kind == RecordKind::Epic)
            .map(|record| record.status.clone()))
    };
    match (status(base), status(head)) {
        (Err(error), _) | (_, Err(error)) => Some(format!(
            "cannot read the epic records to check {epic}: {error}"
        )),
        (_, Ok(None)) => Some(format!("`Task:` names {epic}, which has no epic record")),
        (Ok(Some(status)), _) if status == "cancelled" => {
            Some(format!("`Task:` names {epic}, which is cancelled"))
        }
        _ => None,
    }
}

/// Why a planning amendment that changes `AGENTS.md`'s managed block is
/// refused.
pub const MANAGED_BLOCK_CHANGED: &str = "a planning-only pull request changes the managed block of AGENTS.md (codeflow:managed:begin to codeflow:managed:end), which must stay byte-identical to the target's; `codeflow update` owns that block, and a planning amendment edits only the project section";

/// A Git tree entry's mode for a symbolic link.
const LINK_MODE: i32 = 0o120_000;

/// A Git tree entry's mode for a submodule (a gitlink).
const GITLINK_MODE: i32 = 0o160_000;

/// [`range_problem`] for revisions named as text, as `codeflow ci` holds
/// them: the target (`base`) and the head of a range whose changed paths
/// are `paths`, as git's exact bytes. A path that is not valid UTF-8 is
/// refused by name, as the entry check of [`range_problem`] refuses it, before any rule reads
/// the others as text.
#[must_use]
pub fn range_problem_at(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    paths: &[crate::git::GitName],
) -> Option<String> {
    let mut texts = Vec::with_capacity(paths.len());
    for path in paths {
        match path.rule_text() {
            Ok(text) => texts.push(text.to_string()),
            Err(odd) => return Some(odd_name_problem(odd.display())),
        }
    }
    let paths = texts.as_slice();
    let resolved = Repository::discover(repo_root)
        .map_err(|error| error.message().to_string())
        .and_then(|repo| {
            let oid = |revision: &str| {
                repo.revparse_single(revision)
                    .and_then(|object| object.peel_to_commit())
                    .map(|commit| commit.id())
                    .map_err(|error| format!("{revision}: {}", error.message()))
            };
            Ok(range_problem(&repo, oid(base)?, oid(head)?, paths))
        });
    resolved.unwrap_or_else(|error| Some(format!("cannot read the range to classify it: {error}")))
}

/// Why the change of `paths` from the target at `base` to `head` cannot
/// ride in a planning amendment (ADR-0078), or `None` when it can. The
/// project's product and watched paths come from the policy at `base`, the
/// target's, never from a working copy. Every path must be one
/// [`amendment_path`] admits, and, read from Git's own diff, none may be a symbolic link or a submodule on
/// either side or have a name that is not plain UTF-8. A range that
/// carries a doc or `AGENTS.md`
/// needs trees with no symbolic link or submodule at all, since either
/// could present that text at a path the amendment may not write; a range
/// of records and plans only is judged as before. `AGENTS.md` keeps the
/// target's managed block byte for byte.
#[must_use]
pub fn range_problem(repo: &Repository, base: Oid, head: Oid, paths: &[String]) -> Option<String> {
    range_problem_with(repo, base, head, paths, true)
}

/// [`range_problem`], with the per-commit history pass when `history`; the
/// per-path probe that names the path at fault judges the final paths only,
/// and names a history fault separately.
fn range_problem_with(
    repo: &Repository,
    base: Oid,
    head: Oid,
    paths: &[String],
    history: bool,
) -> Option<String> {
    let project = match project_at(repo, base) {
        Ok(project) => project,
        Err(error) => {
            return Some(format!(
                "cannot read the target's policy to classify the range: {error}"
            ))
        }
    };
    if let Some((_, problem)) = entry_problem(repo, base, head) {
        return Some(problem);
    }
    let mut instructions = false;
    let mut text = None;
    for path in paths {
        match amendment_path(path, &project) {
            None => {
                let path = crate::git::display_key(path);
                return Some(format!(
                    "a planning-only pull request touches a path outside the admitted set: {path}; admitted paths: project-management/** except templates, docs/plan/**, docs/** and git.document_indexes from the target policy ({}), all outside the adopter-facing set, hidden paths and harness instruction files; AGENTS.md only outside its managed block",
                    if project.document_indexes.is_empty() { "none declared".to_string() } else { project.document_indexes.join(", ") }
                ));
            }
            Some(AmendmentPath::Record) => {}
            Some(kind) => {
                instructions |= kind == AmendmentPath::Instructions;
                text.get_or_insert(path);
            }
        }
    }
    // The final diff can hide a change a commit made and a later one
    // reverted; that history still lands, so each commit is held to the
    // admitted paths, to regular entries only and to the target's managed
    // AGENTS.md block too (TSK-1001).
    if let Some(problem) = history
        .then(|| history_problem(repo, base, head, &project))
        .flatten()
    {
        return Some(problem);
    }
    if let Some(path) = text {
        let path = crate::git::display_key(path);
        for (side, at) in [("target", base), ("head", head)] {
            if let Some((kind, entry)) = first_special(repo, at) {
                return Some(format!(
                    "a planning-only pull request carries {path}, and the tree at the {side} holds the {kind} {entry}; a planning amendment carries docs and AGENTS.md only where the tree has no symbolic link or submodule, so carry records and plans only, or land {path} in a task pull request"
                ));
            }
        }
    }
    if instructions {
        let before = match entry_at(repo, base, "AGENTS.md") {
            Ok(entry) => entry,
            Err(error) => return Some(error),
        };
        let after = match entry_at(repo, head, "AGENTS.md") {
            Ok(entry) => entry,
            Err(error) => return Some(error),
        };
        if !instructions_block_unchanged(
            before.as_ref().map(|(_, bytes)| bytes.as_slice()),
            after.as_ref().map(|(_, bytes)| bytes.as_slice()),
        ) {
            return Some(MANAGED_BLOCK_CHANGED.to_string());
        }
    }
    None
}

/// One entry a commit of a range changes: its path, its mode and blob on
/// each side (zero when absent), and the commit (TSK-1001).
pub struct HistoryChange {
    pub commit: Oid,
    pub path: String,
    pub old_mode: i32,
    pub new_mode: i32,
    pub old_blob: Oid,
    pub new_blob: Oid,
}

/// Every entry a commit of the range (reachable from `head`, not from
/// `base`) changes: a commit's change against its one parent, a root
/// commit's whole tree (against the empty tree), and for a merge the
/// entries it changes against every parent, which is what the merge itself
/// introduced. The walk uses the guarded history reader, so a shallow or
/// overlaid history refuses. A name that is not UTF-8 is an error, never a
/// lookalike.
///
/// # Errors
/// Returns a message when the history or a tree cannot be read.
pub fn history_changes(
    repo: &Repository,
    base: Oid,
    head: Oid,
) -> Result<Vec<HistoryChange>, String> {
    let message = |error: git2::Error| error.message().to_string();
    let mut walk = history::revwalk(repo).map_err(message)?;
    walk.push(head).map_err(message)?;
    walk.hide(base).map_err(message)?;
    let deltas = |commit: Oid,
                  from: Option<&git2::Tree<'_>>,
                  to: &git2::Tree<'_>|
     -> Result<Vec<HistoryChange>, String> {
        let diff = repo
            .diff_tree_to_tree(from, Some(to), None)
            .map_err(message)?;
        let mut found = Vec::new();
        for delta in diff.deltas() {
            let (old, new) = (delta.old_file(), delta.new_file());
            let name = new
                .path_bytes()
                .or_else(|| old.path_bytes())
                .ok_or_else(|| "git reported a change with no path".to_string())?;
            let path = String::from_utf8(name.to_vec()).map_err(|error| {
                odd_name_problem(
                    &crate::git::GitName::from_bytes(error.as_bytes())
                        .display()
                        .to_string(),
                )
            })?;
            found.push(HistoryChange {
                commit,
                path,
                old_mode: i32::from(old.mode()),
                new_mode: i32::from(new.mode()),
                old_blob: old.id(),
                new_blob: new.id(),
            });
        }
        Ok(found)
    };
    let mut found = Vec::new();
    for oid in walk {
        let commit = repo.find_commit(oid.map_err(message)?).map_err(message)?;
        let tree = commit.tree().map_err(message)?;
        let parents: Vec<git2::Tree<'_>> = history::parents(repo, &commit)
            .map_err(message)?
            .into_iter()
            .map(|parent| repo.find_commit(parent).and_then(|parent| parent.tree()))
            .collect::<Result<_, _>>()
            .map_err(message)?;
        match parents.as_slice() {
            [] => found.extend(deltas(commit.id(), None, &tree)?),
            [parent] => found.extend(deltas(commit.id(), Some(parent), &tree)?),
            [first, rest @ ..] => {
                let mut changes = deltas(commit.id(), Some(first), &tree)?;
                for parent in rest {
                    let other: BTreeSet<String> = deltas(commit.id(), Some(parent), &tree)?
                        .into_iter()
                        .map(|change| change.path)
                        .collect();
                    changes.retain(|change| other.contains(&change.path));
                }
                found.extend(changes);
            }
        }
    }
    Ok(found)
}

/// Every path [`history_changes`] lists.
///
/// # Errors
/// Returns a message when the history or a tree cannot be read.
pub fn history_paths(repo: &Repository, base: Oid, head: Oid) -> Result<BTreeSet<String>, String> {
    Ok(history_changes(repo, base, head)?
        .into_iter()
        .map(|change| change.path)
        .collect())
}

/// Why a commit of a planning range carries what a planning amendment may
/// not, though the final diff may not show it: a path outside the admitted
/// set, a symbolic link or submodule on either side, or a change to the
/// managed block of `AGENTS.md` (TSK-1001).
fn history_problem(
    repo: &Repository,
    base: Oid,
    head: Oid,
    project: &ProjectPaths,
) -> Option<String> {
    let changes = match history_changes(repo, base, head) {
        Ok(changes) => changes,
        Err(error) => {
            return Some(format!(
                "cannot read the range's commits to classify it: {error}"
            ))
        }
    };
    let blob = |id: Oid| -> Result<Option<Vec<u8>>, String> {
        if id.is_zero() {
            return Ok(None);
        }
        repo.find_blob(id)
            .map(|blob| Some(blob.content().to_vec()))
            .map_err(|error| error.message().to_string())
    };
    for change in changes {
        let path = crate::git::display_key(&change.path);
        let commit = change.commit.to_string();
        let commit = &commit[..9];
        if amendment_path(&change.path, project).is_none() {
            return Some(format!(
                "commit {commit} of this planning-only pull request changes {path}, outside the admitted set, though the final diff may not show it; a planning amendment's history carries planning paths only, so drop that commit (rebase it out) and keep product changes on a task branch"
            ));
        }
        if let Some(kind) = [change.old_mode, change.new_mode]
            .into_iter()
            .find_map(special_entry_kind)
        {
            return Some(format!(
                "commit {commit} of this planning-only pull request changes the {kind} {path}, though the final diff may not show it; a planning amendment's history carries regular files only, so drop that commit (rebase it out)"
            ));
        }
        if change.path == "AGENTS.md" {
            let (before, after) = match (blob(change.old_blob), blob(change.new_blob)) {
                (Ok(before), Ok(after)) => (before, after),
                (Err(error), _) | (_, Err(error)) => {
                    return Some(format!("cannot read AGENTS.md in commit {commit}: {error}"))
                }
            };
            if !instructions_block_unchanged(before.as_deref(), after.as_deref()) {
                return Some(format!(
                    "commit {commit}: {MANAGED_BLOCK_CHANGED}; a later commit restoring it does not take the change out of the landed history"
                ));
            }
        }
    }
    None
}

/// Why an entry named `shown` cannot ride in a planning amendment: its name
/// is not plain UTF-8 or holds a backslash.
fn odd_name_problem(shown: &str) -> String {
    format!(
        "a planning-only pull request changes {shown}, whose name is not plain UTF-8 or holds a backslash; a planning amendment carries plainly named files only"
    )
}

/// What an entry of `mode` is when it is not a regular file or folder.
pub(crate) fn special_entry_kind(mode: i32) -> Option<&'static str> {
    match mode {
        LINK_MODE => Some("symbolic link"),
        GITLINK_MODE => Some("submodule"),
        _ => None,
    }
}

/// Why an entry the range changes cannot ride in a planning amendment,
/// read from the diff of the trees, from the merge-base of `base` and
/// `head` to `head` as `codeflow ci` reads the range, so no name decoding
/// stands between an entry and its check: a symbolic link or submodule on
/// either side, or a name that is not UTF-8 or holds a backslash (a
/// separator on Windows, a letter elsewhere). A range that cannot be read
/// is refused. The fault comes with the path of the entry at fault, empty
/// when the range cannot be read.
fn entry_problem(repo: &Repository, base: Oid, head: Oid) -> Option<(String, String)> {
    // A range with no common ancestor is read from the target; a history
    // that cannot be read is no range at all.
    let from = match super::acceptance::common_base(repo, base, head) {
        Ok(found) => found.unwrap_or(base),
        Err(error) => {
            return Some((
                String::new(),
                format!("cannot read the range to classify it: {error}"),
            ))
        }
    };
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree());
    let diff = tree(from)
        .and_then(|before| {
            let after = tree(head)?;
            repo.diff_tree_to_tree(Some(&before), Some(&after), None)
        })
        .map_err(|error| error.message().to_string());
    let diff = match diff {
        Ok(diff) => diff,
        Err(error) => {
            return Some((
                String::new(),
                format!("cannot read the range to classify it: {error}"),
            ))
        }
    };
    for delta in diff.deltas() {
        for file in [delta.old_file(), delta.new_file()] {
            let Some(name) = file.path_bytes() else {
                continue;
            };
            let shown = crate::git::GitName::from_bytes(name).display().to_string();
            if std::str::from_utf8(name).is_err() || name.contains(&b'\\') {
                let message = odd_name_problem(&shown);
                return Some((shown, message));
            }
            if let Some(kind) = special_entry_kind(i32::from(file.mode())) {
                let message = format!(
                    "a planning-only pull request changes the {kind} {shown}; a planning amendment carries regular files only"
                );
                return Some((shown, message));
            }
        }
    }
    None
}

/// The first symbolic link or submodule in the tree of `commit`, by kind
/// and path, found by mode whatever its name; a tree that cannot be read
/// counts as holding one.
fn first_special(repo: &Repository, commit: Oid) -> Option<(&'static str, String)> {
    let unreadable = || {
        Some((
            "entry it cannot read in",
            commit.to_string()[..9].to_string(),
        ))
    };
    let Ok(tree) = repo.find_commit(commit).and_then(|commit| commit.tree()) else {
        return unreadable();
    };
    let mut found = None;
    let walked = crate::git::walk_tree(repo, &tree, &mut |path, entry| match special_entry_kind(
        entry.filemode(),
    ) {
        Some(kind) => {
            found = Some((kind, path.display().to_string()));
            crate::git::Walk::Stop
        }
        None => crate::git::Walk::Continue,
    });
    match (found, walked) {
        (Some(found), _) => Some(found),
        (None, Err(_)) => unreadable(),
        (None, Ok(())) => None,
    }
}

/// The mode and bytes of the entry at `path` in the tree of `commit`.
fn entry_at(repo: &Repository, commit: Oid, path: &str) -> Result<Option<(i32, Vec<u8>)>, String> {
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree())
        .map_err(|error| error.to_string())?;
    let entry = match tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| error.to_string())?;
    Ok(Some((entry.filemode(), blob.content().to_vec())))
}

/// The project's product and watched paths from the policy at `commit`: its
/// `git.product_paths`, else the default for the stack its project file
/// records, its `git.breaking_watch_paths` and its `git.document_indexes`.
pub(super) fn project_at(repo: &Repository, commit: Oid) -> Result<ProjectPaths, String> {
    let list =
        |value: Option<&serde_json::Value>, key: &str| -> Result<Option<Vec<String>>, String> {
            let Some(value) = value else { return Ok(None) };
            let items = value
                .as_array()
                .ok_or_else(|| format!("git.{key} is not a list"))?;
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| format!("git.{key} contains a non-string path"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some)
        };
    let policy: serde_json::Value = match entry_at(repo, commit, ".codeflow/policy.json")? {
        Some((_, bytes)) => serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
        None => serde_json::Value::Null,
    };
    let git = &policy["git"];
    let product = if let Some(paths) = list(git.get("product_paths"), "product_paths")? {
        paths
    } else {
        let project = entry_at(repo, commit, ".codeflow/project.toml")?
            .map(|(_, bytes)| {
                let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
                text.parse::<toml::Table>()
                    .map_err(|error| error.to_string())
            })
            .transpose()?;
        let stack = project
            .as_ref()
            .and_then(|table| table.get("stack"))
            .map(|stack| {
                stack
                    .as_str()
                    .ok_or_else(|| "project stack is not a string".to_string())
            })
            .transpose()?
            .unwrap_or_default();
        super::classify::stack_product_paths(stack)
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    let paths = ProjectPaths {
        product,
        watched: list(git.get("breaking_watch_paths"), "breaking_watch_paths")?.unwrap_or_default(),
        document_indexes: list(git.get("document_indexes"), "document_indexes")?
            .unwrap_or_default(),
    };
    paths.validate()?;
    Ok(paths)
}

/// The path that keeps a criteria change landed by `landing` (on top of
/// `parent`) from being a planning change, or `None` when it is one: a
/// range that [`range_problem`] admits as a planning amendment (ADR-0078),
/// records-only ranges included, so a release imports nothing its
/// planning pull request would have refused.
///
/// # Errors
///
/// Returns the Git error when a tree cannot be read.
pub(super) fn landing_problem(
    repo: &Repository,
    parent: Option<Oid>,
    landing: Oid,
) -> Result<Option<String>, git2::Error> {
    let Some(parent) = parent else {
        return super::acceptance::non_planning_change(repo, None, landing);
    };
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree());
    let diff = repo.diff_tree_to_tree(Some(&tree(parent)?), Some(&tree(landing)?), None)?;
    let names: BTreeSet<crate::git::GitName> = crate::git::diff_paths(&diff).into_iter().collect();
    // A planning amendment carries plainly named files only, and a name that
    // is not valid UTF-8 is not one (see `entry_problem`).
    if let Some(odd) = names.iter().find(|name| name.rule_text().is_err()) {
        return Ok(Some(odd.display().to_string()));
    }
    let paths: Vec<String> = names
        .iter()
        .filter_map(|name| name.rule_text().ok().map(str::to_string))
        .collect();
    if range_problem(repo, parent, landing, &paths).is_none() {
        return Ok(None);
    }
    // An entry at fault is named as Git holds it; the per-path probe below
    // would find the whole-range fault on every path.
    if let Some((path, _)) = entry_problem(repo, parent, landing) {
        return Ok(Some(path)
            .filter(|path| !path.is_empty())
            .or_else(|| paths.first().cloned()));
    }
    // Name the path at fault; a problem no single path shows (the policy
    // cannot be read) names the first.
    let single = |path: &String| {
        range_problem_with(repo, parent, landing, std::slice::from_ref(path), false)
    };
    if let Some(path) = paths.iter().find(|path| single(path).is_some()) {
        return Ok(Some(path.clone()));
    }
    // No final path is at fault, so a commit of the landing's history is
    // (TSK-1001): name the first entry it changes that a planning
    // amendment may not carry.
    let project = project_at(repo, parent).ok();
    let historical = history_changes(repo, parent, landing)
        .ok()
        .and_then(|changes| {
            changes.into_iter().find_map(|change| {
                let admitted = project
                    .as_ref()
                    .is_some_and(|project| amendment_path(&change.path, project).is_some());
                let special = [change.old_mode, change.new_mode]
                    .into_iter()
                    .any(|mode| special_entry_kind(mode).is_some());
                (!admitted || special || change.path == "AGENTS.md").then_some(change.path)
            })
        });
    Ok(historical.or_else(|| paths.first().cloned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_refusal_escapes_control_characters_in_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, base) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        for (path, escaped) in [
            ("index\x1b.md", r"index\x1b.md"),
            ("index\n.md", r"index\x0a.md"),
        ] {
            let head = crate::git::add_commit(&repo, &[(path.as_bytes(), b"y")]);
            let problem = range_problem(&repo, base, head, &[path.to_string()]).unwrap();
            assert!(problem.contains(escaped), "{problem:?}");
            assert!(
                !problem.contains(path),
                "raw control character: {problem:?}"
            );
        }
    }

    #[test]
    fn special_tree_refusal_escapes_the_document_path() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, _) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        let base = crate::git::add_commit_modes(&repo, &[(b"link", b"target", 0o120_000)]);
        for (path, escaped) in [
            ("docs/index\x1b.md", r"docs/index\x1b.md"),
            ("docs/index\n.md", r"docs/index\x0a.md"),
        ] {
            let head = crate::git::add_commit(&repo, &[(path.as_bytes(), b"y")]);
            let problem = range_problem(&repo, base, head, &[path.to_string()]).unwrap();
            assert!(problem.contains("symbolic link"), "{problem:?}");
            assert_eq!(problem.matches(escaped).count(), 2, "{problem:?}");
            assert!(
                !problem.contains(path),
                "raw control character: {problem:?}"
            );
        }
    }

    /// Round 23: a changed path that is not valid UTF-8 is refused by name
    /// before any rule reads the paths as text, with or without a readable
    /// repository; a plainly named path reaches the repository read.
    #[test]
    fn r23_range_problem_at_refuses_an_odd_name_by_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let odd = crate::git::GitName::from_bytes(b"docs/caf\xe9.md");
        let problem = range_problem_at(dir.path(), "HEAD", "HEAD", &[odd]).unwrap();
        assert!(problem.contains(r"docs/caf\xe9.md"), "{problem}");
        assert!(problem.contains("not plain UTF-8"), "{problem}");
        let plain = crate::git::GitName::from_text("docs/cafe.md");
        let problem = range_problem_at(dir.path(), "HEAD", "HEAD", &[plain]).unwrap();
        assert!(problem.contains("cannot read the range"), "{problem}");
    }

    /// A history whose shared ancestor cannot be read is refused; it is
    /// never read as a range with no common ancestor and diffed from the
    /// target.
    #[test]
    fn r23_an_unreadable_shared_ancestor_refuses_the_range() {
        let dir = tempfile::tempdir().unwrap();
        let record = "project-management/tasks/TSK-002.md".to_string();
        let (repo, first) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"1")]);
        let base = crate::git::add_commit(&repo, &[(b"a", b"2")]);
        let who = git2::Signature::now("Test", "test@example.com").unwrap();
        let (head, orphan) = {
            let parent = repo.find_commit(first).unwrap();
            let mut index = repo.index().unwrap();
            index.read_tree(&parent.tree().unwrap()).unwrap();
            let blob = repo.blob(b"y").unwrap();
            index
                .add(&git2::IndexEntry {
                    ctime: git2::IndexTime::new(0, 0),
                    mtime: git2::IndexTime::new(0, 0),
                    dev: 0,
                    ino: 0,
                    mode: 0o100_644,
                    uid: 0,
                    gid: 0,
                    file_size: 1,
                    id: blob,
                    flags: 0,
                    flags_extended: 0,
                    path: record.as_bytes().to_vec(),
                })
                .unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let head = repo
                .commit(None, &who, &who, "record", &tree, &[&parent])
                .unwrap();
            let orphan = repo.commit(None, &who, &who, "orphan", &tree, &[]).unwrap();
            (head, orphan)
        };
        let paths = std::slice::from_ref(&record);
        // The control: an ordinary planning range passes. A disjoint
        // history's root commit lands its whole tree, so its product file
        // `a` is refused even though the range's own paths omit it
        // (TSK-1001).
        assert_eq!(range_problem(&repo, base, head, paths), None);
        let orphaned = range_problem(&repo, base, orphan, paths).expect("refused");
        assert!(
            orphaned.contains("changes a, outside the admitted set"),
            "{orphaned}"
        );
        drop(repo);

        let hex = first.to_string();
        std::fs::remove_file(
            dir.path()
                .join(".git")
                .join("objects")
                .join(&hex[..2])
                .join(&hex[2..]),
        )
        .unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let problem = range_problem(&repo, base, head, paths).expect("refused");
        assert!(problem.contains("cannot read the range"), "{problem}");
    }

    #[test]
    fn r22_historical_project_paths_reject_invalid_globs_and_keep_absence() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, first) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        assert!(project_at(&repo, first).is_ok());
        for key in ["product_paths", "breaking_watch_paths"] {
            let policy = serde_json::json!({"git": {key: ["src/["]}}).to_string();
            let oid =
                crate::git::add_commit(&repo, &[(b".codeflow/policy.json", policy.as_bytes())]);
            assert!(project_at(&repo, oid).is_err(), "{key}");
        }
    }

    #[test]
    fn r22_historical_project_paths_reject_malformed_lists() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, _) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        for key in ["product_paths", "breaking_watch_paths"] {
            for value in [serde_json::json!(42), serde_json::json!([42])] {
                let policy = serde_json::json!({"git": {key: value}}).to_string();
                let oid =
                    crate::git::add_commit(&repo, &[(b".codeflow/policy.json", policy.as_bytes())]);
                assert!(project_at(&repo, oid).is_err(), "{key}");
            }
        }
    }

    #[test]
    fn unreadable_project_never_uses_default_product_paths() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, oid) = crate::git::repo_with_tree(
            dir.path(),
            &[(b".codeflow/project.toml", b"stack = \"rust\"\n#\xff")],
        );
        assert!(project_at(&repo, oid).is_err());
    }

    /// Issue 79: a landing that changes a path that is not valid UTF-8 is not
    /// a plain planning amendment, and the path is named by its exact bytes.
    #[test]
    fn a_landing_that_changes_a_path_that_is_not_utf8_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, first) = crate::git::repo_with_tree(
            dir.path(),
            &[(b"project-management/tasks/TSK-001.md", b"x")],
        );
        let second =
            crate::git::add_commit(&repo, &[(b"project-management/tasks/TSK-002\xff.md", b"y")]);
        let problem = landing_problem(&repo, Some(first), second).unwrap();
        assert_eq!(
            problem.as_deref(),
            Some(r"project-management/tasks/TSK-002\xff.md")
        );
    }

    /// A symbolic link below a directory whose name is not valid UTF-8 is
    /// found: git2's own walk stopped at the directory.
    #[test]
    fn a_symlink_below_a_directory_that_is_not_utf8_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, _) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        let second =
            crate::git::add_commit_modes(&repo, &[(b"dir\xff/link", b"target", 0o120_000)]);
        let found = first_special(&repo, second).expect("the link is found");
        assert_eq!(found.1, r"dir\xff/link");
    }
}
