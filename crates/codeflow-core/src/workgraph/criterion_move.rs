//! Recorded transfers keep frozen task tags and bind the new serving relation
//! to the planning commit that introduced an identical destination criterion.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use git2::{Oid, Repository};

use super::acceptance::{commit_of, epic_waiver_problem, is_ancestor_or_same, record_at};
use super::history;
use super::lifecycle::{Graph, RecordView};
use super::record_text::{AcceptanceBlock, Criterion};
use super::work_start::RecordKind;
use crate::remedy::{self, Finding};

#[derive(Debug, Clone)]
pub(super) struct Transfer {
    destination: (String, String),
    amendment: Oid,
}

pub(super) type Transfers = BTreeMap<(String, String), Result<Transfer, String>>;

// One parse per historical record, shared by every note in this graph's
// resolution. The cache expires with read(), so later judgments read live
// history and never retain authority across repository mutations.
type RecordCopies = BTreeMap<(Oid, String), Result<Option<Rc<RecordView>>, String>>;

struct TransferHistory<'r> {
    repo: &'r Repository,
    records: RefCell<RecordCopies>,
}

impl TransferHistory<'_> {
    fn at(&self, oid: Oid, record: &RecordView) -> Result<Option<Rc<RecordView>>, String> {
        self.records
            .borrow_mut()
            .entry((oid, record.path.clone()))
            .or_insert_with(|| record_at(self.repo, oid, record).map(|copy| copy.map(Rc::new)))
            .clone()
    }
}

/// Reserve only this explicit use of an existing dated amendment note.
/// Malformed or conflicting transfer notes still demand proof and never
/// fall back to a tick or a task that happened to finish.
pub(super) fn has_note(criterion: &Criterion) -> bool {
    criterion
        .sub_items
        .iter()
        .any(|note| transfer_line(&note.line))
}

fn transfer_line(line: &str) -> bool {
    line.starts_with("- Amended ")
        && line
            .split([' ', '\t'])
            .filter(|word| !word.is_empty())
            .nth(3)
            == Some("transferred")
}

fn destination(criterion: &Criterion) -> Result<(String, String), String> {
    let notes: Vec<_> = criterion
        .sub_items
        .iter()
        .filter(|note| transfer_line(&note.line))
        .collect();
    let [note] = notes.as_slice() else {
        return Err("one readable transfer amendment note is required".into());
    };
    if !note.note {
        return Err("the transfer needs a dated Amended note".into());
    }
    let words: Vec<_> = note
        .line
        .split([' ', '\t'])
        .filter(|word| !word.is_empty())
        .collect();
    let ["-", "Amended", date, "transferred", "to", epic, criterion] = words.as_slice() else {
        return Err("use `- Amended YYYY-MM-DD: transferred to EPC-NNN AC-n.`".into());
    };
    let criterion = criterion.strip_suffix('.').unwrap_or(criterion);
    if !date.ends_with(':')
        || !super::is_valid_epic_format_id(epic)
        || !super::record_text::is_criterion_id(criterion)
    {
        return Err("the transfer destination is unreadable".into());
    }
    Ok((epic.to_string(), criterion.to_string()))
}

fn item<'a>(record: &'a RecordView, id: &str) -> Option<&'a Criterion> {
    record
        .criteria
        .items
        .iter()
        .find(|criterion| criterion.id == id)
}

fn open(record: &RecordView) -> bool {
    super::lifecycle::EPIC_OPEN.contains(&record.status.as_str())
}

/// Only notes present at the review can transfer proof. Read each candidate
/// planning commit through the existing guarded history and record readers.
#[allow(clippy::too_many_lines)] // Keep review, planning ancestry and membership proofs in refusal order.
fn resolve(
    history: &TransferHistory<'_>,
    graph: &Graph,
    source: &RecordView,
    criterion: &Criterion,
) -> Result<Transfer, String> {
    let repo = history.repo;
    let target = destination(criterion)?;
    if target.0 == source.id {
        return Err("a criterion cannot transfer to its own epic".into());
    }
    let target_record = graph
        .get(&target.0, RecordKind::Epic)
        .ok_or_else(|| "the destination epic is missing".to_string())?;
    let target_item = item(target_record, &target.1)
        .ok_or_else(|| "the destination criterion is missing".to_string())?;
    let blocks = source.active_blocks();
    let reviewed = match blocks.as_slice() {
        [] => graph.revision,
        [block] => block
            .parsed
            .as_ref()
            .ok()
            .and_then(|block| commit_of(repo, &block.reviewed)),
        _ => None,
    }
    .ok_or_else(|| "the transfer needs a readable reviewed commit".to_string())?;
    if !is_ancestor_or_same(repo, reviewed, graph.revision.ok_or("no graph revision")?)? {
        return Err("the graph does not contain the transfer review".into());
    }
    let at_review = history
        .at(reviewed, source)?
        .ok_or("source missing at review")?;
    let reviewed_item =
        item(&at_review, &criterion.id).ok_or("source criterion missing at review")?;
    if reviewed_item != criterion {
        return Err("the transfer note or source criterion changed after review".into());
    }
    let reviewed_target = history
        .at(reviewed, target_record)?
        .ok_or("destination missing at review")?;
    // A completed destination retains its inherited task proof on later
    // validation. It cannot authorize closing the source: that requires a
    // source block whose own review still saw the destination open.
    let destination_completion = blocks.is_empty() && reviewed_target.status == "complete";
    // The destination keeps its obligation; its own dated notes may change.
    if (!open(&reviewed_target) && !destination_completion)
        || !item(&reviewed_target, &target.1)
            .is_some_and(|reviewed| same_obligation(reviewed, target_item))
    {
        return Err("the review needs an open destination with the same criterion".into());
    }
    let mut pending = vec![reviewed];
    let mut seen = BTreeSet::new();
    let mut found = None;
    while let Some(oid) = pending.pop() {
        if !seen.insert(oid) {
            continue;
        }
        let Some(after) = history.at(oid, source)? else {
            continue;
        };
        let Some(after_item) = item(&after, &criterion.id) else {
            continue;
        };
        if destination(after_item).ok().as_ref() != Some(&target) {
            continue;
        }
        let commit = repo.find_commit(oid).map_err(|error| error.to_string())?;
        let parents = history::parents(repo, &commit).map_err(|error| error.to_string())?;
        let mut inherited = false;
        for parent in &parents {
            let before = history.at(*parent, source)?;
            if before
                .as_ref()
                .and_then(|record| item(record, &criterion.id))
                .is_some_and(|before| destination(before).ok().as_ref() == Some(&target))
            {
                inherited = true;
            }
        }
        if inherited {
            pending.extend(parents);
            continue;
        }
        let [parent] = parents.as_slice() else {
            return Err(
                "a transfer must be introduced by one planning commit with one parent".into(),
            );
        };
        if epic_waiver_problem(
            repo,
            source,
            &criterion.id,
            &oid.to_string(),
            Some(reviewed),
        )
        .is_some()
        {
            return Err(
                "the transfer is not a planning-only amendment of the source criterion".into(),
            );
        }
        let before = history
            .at(*parent, source)?
            .ok_or("source missing before transfer")?;
        let before_item =
            item(&before, &criterion.id).ok_or("source criterion missing before transfer")?;
        let introduced = history
            .at(oid, target_record)?
            .ok_or("destination missing at transfer")?;
        let introduced_item =
            item(&introduced, &target.1).ok_or("destination criterion missing at transfer")?;
        let previous_target = history.at(*parent, target_record)?;
        if previous_target
            .as_ref()
            .and_then(|record| item(record, &target.1))
            .is_some()
        {
            return Err("the destination criterion existed before the transfer".into());
        }
        if !open(&introduced)
            || !same_obligation(before_item, after_item)
            || !same_obligation(before_item, introduced_item)
            || !same_obligation(before_item, criterion)
            || !same_obligation(introduced_item, target_item)
        {
            return Err("the transfer must preserve the obligation and journey/after-release tags in an open epic".into());
        }
        if found.replace(oid).is_some() {
            return Err("conflicting commits introduce this transfer".into());
        }
    }
    let amendment = found.ok_or("no planning commit introduces this transfer")?;
    let serving: Vec<_> = graph
        .tasks()
        .filter(|task| {
            task.criteria
                .items
                .iter()
                .any(|entry| entry.serves() == Some((source.id.clone(), criterion.id.clone())))
        })
        .collect();
    if serving.is_empty() {
        return Err("a transfer needs moved serving tasks".into());
    }
    for task in serving {
        let at_review = history
            .at(reviewed, task)?
            .ok_or("moved task missing at review")?;
        let cancelled_at_both = task.status == "cancelled" && at_review.status == "cancelled";
        // A discarded task supplies no proof and imposes no membership
        // constraint. Its reviewed identity and criteria still must exist.
        if at_review.criteria.signature() != task.criteria.signature()
            || (!cancelled_at_both
                && (task.epic_id.as_deref() != Some(target.0.as_str())
                    || at_review.epic_id != task.epic_id))
        {
            let cancellation = if task.status == "cancelled" && at_review.status != "cancelled" {
                "; cancelled after review, so membership is still required"
            } else {
                ""
            };
            return Err(format!(
                "{} is not a moved serving task in the destination at review{cancellation}",
                task.id
            ));
        }
    }
    Ok(Transfer {
        destination: target,
        amendment,
    })
}

fn same_obligation(left: &Criterion, right: &Criterion) -> bool {
    left.own_text() == right.own_text()
        && left.is_journey() == right.is_journey()
        && left.is_after_release() == right.is_after_release()
}

pub(super) fn read(graph: &Graph) -> Transfers {
    let _judgment = history::judgment();
    let repo = graph
        .repository
        .as_ref()
        .ok_or_else(|| "cannot read transfer history: no repository".to_string())
        .and_then(|path| Repository::open(path).map_err(|error| error.to_string()));
    let history = repo.as_ref().map(|repo| TransferHistory {
        repo,
        records: RefCell::default(),
    });
    let mut transfers = Transfers::new();
    for source in graph
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Epic)
    {
        for criterion in source
            .criteria
            .items
            .iter()
            .filter(|criterion| has_note(criterion))
        {
            let result = history
                .as_ref()
                .map_err(|error| (*error).clone())
                .and_then(|history| resolve(history, graph, source, criterion));
            transfers.insert((source.id.clone(), criterion.id.clone()), result);
        }
    }
    transfers
}

pub(super) fn close_source(
    source: &RecordView,
    criterion: &Criterion,
    graph: &Graph,
    block: Option<&AcceptanceBlock>,
) -> Result<(), String> {
    if criterion.is_after_release() {
        // Never substitute a waiver for today's after-release deferral path.
        let serving = super::lifecycle::serving_tasks(source, criterion, graph);
        return if super::lifecycle::deferred_to_open_follow_up(criterion, &serving, block) {
            Ok(())
        } else {
            Err(
                "an after-release transfer still needs deferral to an open serving follow-up"
                    .into(),
            )
        };
    }
    let transfer = graph
        .transfers()
        .get(&(source.id.clone(), criterion.id.clone()))
        .ok_or("no readable transfer")?
        .as_ref()
        .map_err(Clone::clone)?;
    let block = block.ok_or("a transferred criterion needs its own waiver")?;
    let result = block
        .criteria
        .iter()
        .find(|(id, _)| id == &criterion.id)
        .map(|(_, result)| result)
        .ok_or("a transferred criterion needs its own waiver")?;
    let repository = graph
        .repository
        .as_ref()
        .ok_or("cannot read transfer history")?;
    let repo = Repository::open(repository).map_err(|error| error.to_string())?;
    if result.outcome != "waived" {
        return Err(
            "a transferred criterion needs a waiver naming the commit that records the transfer"
                .into(),
        );
    }
    if let Some(problem) = epic_waiver_problem(
        &repo,
        source,
        &criterion.id,
        &result.evidence,
        commit_of(&repo, &block.reviewed),
    ) {
        return Err(format!("transfer waiver {problem}"));
    }
    if commit_of(&repo, &result.evidence) != Some(transfer.amendment) {
        return Err(
            "a transferred criterion needs a waiver naming the commit that records the transfer"
                .into(),
        );
    }
    Ok(())
}

pub(super) fn serves_destination(
    graph: &Graph,
    task: &RecordView,
    criterion: &Criterion,
    epic: &RecordView,
    destination: &Criterion,
) -> bool {
    criterion
        .serves()
        .and_then(|source| graph.transfers().get(&source))
        .is_some_and(|result| {
            result.as_ref().is_ok_and(|transfer| {
                transfer.destination == (epic.id.clone(), destination.id.clone())
                    && task.epic_id.as_deref() == Some(epic.id.as_str())
            })
        })
}

/// A destination cannot fall back to a tick or an own-block result when
/// transfer proof is invalid, or combine native and transferred servers.
pub(super) fn destination_problem(
    epic: &RecordView,
    criterion: &Criterion,
    graph: &Graph,
) -> Result<bool, String> {
    let mut incoming = false;
    for ((source, id), result) in graph.transfers() {
        match result {
            Ok(transfer) => {
                incoming |= transfer.destination == (epic.id.clone(), criterion.id.clone());
            }
            Err(reason) => {
                // Invalid transfers still refuse their intended destination.
                let intended = graph
                    .get(source, RecordKind::Epic)
                    .and_then(|record| item(record, id))
                    .and_then(|entry| destination(entry).ok());
                if intended == Some((epic.id.clone(), criterion.id.clone())) {
                    return Err(format!(
                        "the transfer from {source} {id} is invalid: {reason}"
                    ));
                }
            }
        }
    }
    if incoming
        && graph
            .tasks()
            .filter(|task| task.status != "cancelled")
            .any(|task| {
                task.criteria
                    .items
                    .iter()
                    .any(|entry| entry.serves() == Some((epic.id.clone(), criterion.id.clone())))
            })
    {
        return Err("transferred criteria must have only their moved serving tasks".into());
    }
    Ok(incoming)
}

pub(super) fn warnings(graph: &Graph) -> Vec<Finding> {
    let mut warnings = Vec::new();
    for task in graph.tasks() {
        for criterion in &task.criteria.items {
            let Some((epic, id)) = criterion.serves() else {
                continue;
            };
            let exists = graph
                .get(&epic, RecordKind::Epic)
                .and_then(|record| item(record, &id))
                .is_some();
            let moved = graph
                .transfers()
                .get(&(epic.clone(), id.clone()))
                .is_some_and(|result| {
                    result.as_ref().is_ok_and(|transfer| {
                        task.epic_id.as_deref() == Some(transfer.destination.0.as_str())
                    })
                });
            if !exists || (task.epic_id.as_deref() != Some(epic.as_str()) && !moved) {
                warnings.push(Finding::new(
                    format!("{} {}: (serves {epic} {id}) names a missing criterion or a foreign epic without a recorded transfer", task.id, criterion.id),
                    remedy::SERVING_TRANSFER.with(&[("path", task.path.as_str())]),
                ));
            }
        }
    }
    warnings
}
