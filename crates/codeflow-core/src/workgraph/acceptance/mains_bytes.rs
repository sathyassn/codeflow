//! Main's bytes (TSK-1185 AC-11 to AC-16, operator ruling 7): a line takes
//! the default branch's records as the default branch holds them.
//!
//! When one run has verified the default branch's tip (the caller's job,
//! AC-14), a record path whose tree entry at the head, blob and mode, equals
//! its tree entry at that trusted tip is the default branch's record. The
//! line does not re-prove it at import and does not freeze it, on any route
//! into the line. Home and "edited" are read from the line tip, the trusted
//! tip and their merge base, never from the head, so a head cannot make a
//! record look default-homed by copying it. Every record this module does
//! not admit keeps the existing checks, and whether a range is a sync is
//! still decided by `main_sync::sync_merges` alone.
use std::collections::BTreeSet;

use super::{
    common_base, first_parent, homed_on_line, is_ancestor_or_same, record_uid, Finding, Graph, Oid,
    RecordKind, RecordView, Repository,
};

/// The rule's note: which records a line takes by main's bytes, and which
/// match the default branch but keep the full checks, and why.
pub const MAINS_BYTES_NOTE: &str = "work.mains_bytes";

/// The trusted default-branch tip one run verified, and the epic line the
/// range lands on. Only `codeflow ci` builds it, after its own query of the
/// destination named this exact commit as the default branch's tip.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MainsBytes {
    /// The verified default-branch tip.
    pub tip: Oid,
    /// The receiving epic line.
    pub line: String,
}

/// The rule's answer for one range.
#[derive(Debug, Default)]
pub struct Judged {
    /// The record paths the line takes by main's bytes.
    pub admitted: BTreeSet<String>,
    /// One note per record the rule looked at.
    pub notes: Vec<Finding>,
}

/// The line tip's records, and each with its uid.
type LineRecords<'a> = (&'a Graph, &'a [(&'a RecordView, Option<String>)]);

/// One tree entry: its object and its mode.
type Entry = (Oid, i32);

fn tree_of(repo: &Repository, at: Oid) -> Result<git2::Tree<'_>, String> {
    repo.find_commit(at)
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("cannot read the tree of {at}: {error}"))
}

fn entry(tree: &git2::Tree<'_>, path: &str) -> Result<Option<Entry>, String> {
    match tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => Ok(Some((entry.id(), entry.filemode()))),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {path}: {error}")),
    }
}

/// A record's declared home, as it compares across versions: a task's
/// target, or its epic when it declares none; an epic's parent line.
fn home(record: &RecordView) -> (Option<String>, Option<String>) {
    let logical = |name: &String| super::super::work_start::logical_target(name).to_string();
    match record.kind {
        RecordKind::Task => match &record.integration_target {
            Some(target) => (Some(logical(target)), None),
            None => (None, record.epic_id.clone()),
        },
        _ => (record.parent_line.as_ref().map(logical), None),
    }
}

/// Judge the records at `head` against the trusted tip and the receiving
/// line's tip `line_tip`.
///
/// # Errors
/// A record graph, tree or history that cannot be read: the caller then
/// admits nothing, so every record keeps the full checks.
pub fn judge(
    repo: &Repository,
    mains: &MainsBytes,
    line_tip: Oid,
    head: Oid,
    at_head: &Graph,
) -> Result<Judged, String> {
    let epic = super::super::line_adoption::epic_id(&mains.line)?;
    let trusted = mains.tip;
    let fork = common_base(repo, line_tip, trusted)?
        .ok_or("the line and the default branch share no history")?;
    let at_line = Graph::from_revision(repo, &line_tip.to_string())?;
    let at_trusted = Graph::from_revision(repo, &trusted.to_string())?;
    let (head_tree, trusted_tree, line_tree, fork_tree) = (
        tree_of(repo, head)?,
        tree_of(repo, trusted)?,
        tree_of(repo, line_tip)?,
        tree_of(repo, fork)?,
    );
    // Each line record with its uid, read once.
    let line_index: Vec<(&RecordView, Option<String>)> = at_line
        .records
        .values()
        .map(|record| (record, record_uid(&record.content)))
        .collect();
    let mut judged = Judged::default();
    for record in at_head
        .records
        .values()
        .filter(|record| matches!(record.kind, RecordKind::Task | RecordKind::Epic))
    {
        let path = record.path.as_str();
        let carried = entry(&head_tree, path)?;
        if carried.is_none() || entry(&trusted_tree, path)? != carried {
            continue;
        }
        let on_line = entry(&line_tree, path)?;
        if on_line == carried {
            continue;
        }
        let declined = decline(
            repo,
            (record, (&at_line, &line_index), &at_trusted),
            (on_line.is_some(), entry(&fork_tree, path)? != on_line),
            (epic, &mains.line),
            (line_tip, trusted),
        )?;
        let message = match &declined {
            None => format!(
                "{} at {path} is the default branch's record at {trusted:.9}, entry for entry, so the line takes it without re-proof or freeze (TSK-1185)",
                record.id
            ),
            Some(why) => format!(
                "{} at {path} matches the default branch's record at {trusted:.9} but keeps the full checks: {why}",
                record.id
            ),
        };
        judged.notes.push(Finding {
            epic_record: None,
            rule: MAINS_BYTES_NOTE,
            message,
            note: true,
        });
        if declined.is_none() {
            judged.admitted.insert(path.to_string());
        }
    }
    Ok(judged)
}

/// Why `record` keeps the full checks, or `None` when the line takes it.
/// `held` says whether the line tip holds its path, and `edited` whether
/// the line tip's entry there differs from the merge base's.
fn decline(
    repo: &Repository,
    (record, (at_line, line_index), at_trusted): (&RecordView, LineRecords<'_>, &Graph),
    (held, edited): (bool, bool),
    (epic, line): (&str, &str),
    (line_tip, trusted): (Oid, Oid),
) -> Result<Option<String>, String> {
    let Some(main) = at_trusted
        .records
        .get(&record.id)
        .filter(|main| main.path == record.path)
    else {
        return Ok(Some(
            "the default branch holds this path under another record".into(),
        ));
    };
    let uid = record_uid(&record.content);
    // The line's own copy, by id or uid, wherever it is.
    let copies: Vec<&RecordView> = line_index
        .iter()
        .filter(|(other, other_uid)| {
            other.id == record.id
                || (uid.is_some() && *other_uid == uid)
                || other.path == record.path
        })
        .map(|(other, _)| *other)
        .collect();
    if !held {
        // The fourth arm (AC-13): a record the line never held, under any
        // path, id or uid.
        return Ok((!copies.is_empty())
            .then(|| "the line tip holds this record at another path or under another id".into()));
    }
    let [own] = copies.as_slice() else {
        return Ok(Some(
            "the line tip holds more than one record with this path, id or uid".into(),
        ));
    };
    if own.path != record.path || own.id != record.id || own.kind != record.kind {
        return Ok(Some(
            "the line tip holds another record at this path".into(),
        ));
    }
    if edited && homed_on_line(own, at_line, epic, line) {
        return Ok(Some(format!(
            "its home is {line} and the line has edited it since its merge base with the default branch"
        )));
    }
    if home(own) != home(main) && !moved_on_trusted_chain(repo, own, line_tip, trusted)? {
        return Ok(Some(
            "the line tip and the default branch declare different homes for it, and no planning landing on the default branch's first-parent chain moved it".into(),
        ));
    }
    Ok(None)
}

/// Whether the default branch, not the line, moved this task's home: on
/// the trusted tip's first-parent chain back to the line's history, every
/// commit that changed the declared home is an admissible planning landing
/// with the task in a permitted state at it and at its first parent
/// (TSK-1004 AC-8 as excepted by TSK-1185 AC-16), at least one did, and
/// the version the line already held declares the line tip's home.
fn moved_on_trusted_chain(
    repo: &Repository,
    own: &RecordView,
    line_tip: Oid,
    trusted: Oid,
) -> Result<bool, String> {
    if own.kind != RecordKind::Task {
        return Ok(false);
    }
    let parse = |at: Oid| -> Result<Option<RecordView>, String> {
        super::blob_at(repo, at, &own.path)?
            .map(|content| RecordView::parse(own.kind, &own.path, &content))
            .transpose()
    };
    let mut at = trusted;
    let mut now = parse(at)?.ok_or("the trusted tip has no record at this path")?;
    let mut moved = false;
    loop {
        if is_ancestor_or_same(repo, at, line_tip)? {
            return Ok(moved && now.id == own.id && home(&now) == home(own));
        }
        let Some(parent) = first_parent(repo, at)? else {
            return Ok(false);
        };
        let Some(then) = parse(parent)? else {
            return Ok(false);
        };
        if then.id != now.id {
            return Ok(false);
        }
        if home(&then) != home(&now) {
            if !super::imports::is_trusted_planning_move(repo, &now, at, parent)? {
                return Ok(false);
            }
            moved = true;
        }
        at = parent;
        now = then;
    }
}
