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
//! to no epic and is listed, never refused. A task complete on both sides
//! keeps the criteria its acceptance block was reviewed against (R-119), so
//! its changed criteria are listed as not admitted. The amendment lands on
//! the target, and a line takes it by merging the target; a record that
//! changed on its line since the line last merged the target is flagged.

use std::collections::{BTreeMap, BTreeSet};

use git2::{Oid, Repository};

use super::acceptance::{blob_at, criteria_delta, finding, target_tips, Finding};
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
        let before = target.records.get(id);
        let after = head.records.get(id);
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
        let mut lines = changes(id, before, after);
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
    let files: Vec<&str> = changed_paths
        .iter()
        .map(String::as_str)
        .filter(|path| {
            *path == "AGENTS.md" || (path.starts_with("docs/") && !path.starts_with("docs/plan/"))
        })
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
                    (*path).to_string()
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
                    .map(str::trim)
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

/// What changed in one record, one line per change.
fn changes(id: &str, before: Option<&RecordView>, after: Option<&RecordView>) -> Vec<String> {
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
        if before.status == "complete" && after.status == "complete" {
            lines.push(format!(
                "{id} is complete, so its criteria change is not admitted while its acceptance block stands (SPC-013 R-119)"
            ));
        } else {
            let delta = criteria_delta(&before.criteria.items, &after.criteria.items);
            if !delta.is_empty() {
                lines.push(format!("{id} criteria delta: {}", delta.join("; ")));
            }
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
    let line = record.integration_target.as_deref()?.trim();
    if !line.starts_with("integration/") {
        return None;
    }
    target_tips(repo, line).into_iter().find_map(|(name, tip)| {
        let shared = repo.merge_base(tip, target_tip).ok()?;
        (blob_at(repo, tip, &record.path) != blob_at(repo, shared, &record.path)).then(|| {
            format!(
                "{} is newer on {name} than on the target; this amendment lands on the target, so merge the target into {line} and check the record there",
                record.id
            )
        })
    })
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
