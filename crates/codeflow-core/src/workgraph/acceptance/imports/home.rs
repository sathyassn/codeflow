//! The home history of a task that a range takes from the default branch
//! (TSK-1378). A sync of the default branch into a line imports `main`'s
//! version of a record, and that version may sit after several home moves.
//! The check walks every home move of the task's identity on the trusted
//! default tip's first-parent history since the merge base of the line and
//! that tip, and accepts each move only when it is a reviewed planning
//! merge of records (AC-1, AC-6). The line's own copy is compared once,
//! with the last move's new home. A later change on `main` never clears an
//! earlier move (AC-10). Only the line's own records-only adoption of an
//! exact `main` version, on its first-parent history, moves the start of
//! the walk (AC-13).

use super::super::super::classify::AmendmentPath;
use super::super::super::work_start::logical_target;
use super::super::{
    common_base, first_parent, history, is_ancestor_or_same, is_same_task, record_uid, task_entries,
};
use super::{
    admissible_planning_landing_with, is_shared_planning, permitted_at, Oid, RecordKind,
    RecordView, Repository, PLANNING_LANDING_PATHS,
};

/// The refusal of a home move, as `codeflow ci` has always worded it.
pub(super) const MOVED: &str = "the landing changes the task's declared home";
/// The refusal of a record that names the receiving line as its home and
/// arrives with no proven history on the default branch (AC-12).
pub(super) const NO_SOURCE: &str =
    "the record arrives on its home line by a sync with no proven source history";

/// The point a receiving line's own adoption of a default-branch version
/// sets for one task (AC-13): the walk of that task's home moves starts
/// after `source_version`. It covers that home only, and grants nothing
/// for completions, criteria or later moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeAnchor {
    /// The line's records-only planning merge that took the exact version.
    pub line_landing: Oid,
    /// The default-branch commit where that exact version first stood.
    pub source_version: Oid,
}

/// A task's identity, and its record as the range holds it.
struct Identity<'a> {
    record: &'a RecordView,
    uid: Option<String>,
}

impl<'a> Identity<'a> {
    fn of(record: &'a RecordView) -> Self {
        Self {
            record,
            uid: record_uid(&record.content),
        }
    }
}

/// The task at one commit, found by identity.
enum Held {
    Absent,
    One(Box<RecordView>),
    /// Two records carry the identity, or a task record there cannot be
    /// read and may be a renamed copy of it.
    Ambiguous,
}

impl Held {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Absent, Self::Absent) => true,
            (Self::One(left), Self::One(right)) => left.content == right.content,
            _ => false,
        }
    }
}

fn held_at(repo: &Repository, at: Oid, who: &Identity<'_>) -> Held {
    let Ok(entries) = task_entries(repo, at) else {
        return Held::Ambiguous;
    };
    let mut found = Vec::new();
    for (path, blob) in entries {
        let Ok(blob) = repo.find_blob(blob) else {
            return Held::Ambiguous;
        };
        let Ok(content) = std::str::from_utf8(blob.content()) else {
            return Held::Ambiguous;
        };
        match RecordView::parse(RecordKind::Task, &path, content) {
            Ok(task) if is_same_task(&task, &who.record.id, who.uid.as_deref()) => {
                found.push(task);
            }
            Ok(_) => {}
            Err(_) => return Held::Ambiguous,
        }
    }
    match found.len() {
        0 => Held::Absent,
        1 => Held::One(Box::new(found.remove(0))),
        _ => Held::Ambiguous,
    }
}

/// The home a record declares, by its logical name.
fn declared(record: &RecordView) -> Option<&str> {
    record.integration_target.as_deref().map(logical_target)
}

/// The blob at `path` in the commit's tree, or `None` when it holds none.
pub(super) fn entry_at(repo: &Repository, at: Oid, path: &str) -> Result<Option<Oid>, String> {
    let tree = repo
        .find_commit(at)
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("cannot read {at}: {error}"))?;
    match tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => Ok(Some(entry.id())),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {path} at {at}: {error}")),
    }
}

fn ambiguous(at: Oid) -> String {
    format!("{MOVED}: the task's identity is ambiguous or unreadable at {at}")
}

/// The one merge base of the receiving line and the default tip: the
/// accepted home point every walk starts from.
///
/// # Errors
/// Several merge bases, none, or unreadable history.
pub(super) fn start_point(
    repo: &Repository,
    receiver: Oid,
    default_tip: Oid,
) -> Result<Oid, String> {
    let bases = history::merge_bases(repo, receiver, default_tip).map_err(|error| {
        format!("{MOVED}: cannot read the merge base of the line and the default branch: {error}")
    })?;
    match bases.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!(
            "{MOVED}: the line and the default branch share no history"
        )),
        _ => Err(format!(
            "{MOVED}: the line and the default branch have several merge bases"
        )),
    }
}

/// Whether `head` takes default-branch commits that `receiver` does not
/// already hold: a sync, or a range that merged the default branch.
///
/// # Errors
/// Unreadable history.
pub(super) fn takes_default(
    repo: &Repository,
    receiver: Oid,
    head: Oid,
    default_tip: Oid,
) -> Result<bool, String> {
    if is_ancestor_or_same(repo, default_tip, receiver)? {
        return Ok(false);
    }
    Ok(common_base(repo, head, default_tip)? != common_base(repo, receiver, default_tip)?)
}

/// How the task changed between a commit and its first parent.
enum Step {
    Unchanged,
    Moved,
    Introduced,
}

fn step(repo: &Repository, who: &Identity<'_>, at: Oid, parent: Oid) -> Result<Step, String> {
    // The record's own path holding the same blob on both sides is no
    // change; anything else is read by identity, renames included.
    let path = &who.record.path;
    let (now, then) = (entry_at(repo, at, path)?, entry_at(repo, parent, path)?);
    if now.is_some() && now == then {
        return Ok(Step::Unchanged);
    }
    match (held_at(repo, at, who), held_at(repo, parent, who)) {
        (Held::One(now), Held::One(then)) if declared(&now) != declared(&then) => Ok(Step::Moved),
        (Held::One(_), Held::One(_)) | (Held::Absent, Held::Absent) => Ok(Step::Unchanged),
        (Held::One(_), Held::Absent) => Ok(Step::Introduced),
        (Held::Absent, Held::One(_)) => Err(format!(
            "{MOVED}: the default branch removed the task at {at}, so its later home cannot be traced"
        )),
        _ => Err(ambiguous(at)),
    }
}

/// The task's home moves (each a commit and its first parent, oldest
/// first) and its introduction on the first-parent history of `tip` after
/// `since`, counting only commits that `head` holds.
struct Moves {
    moves: Vec<(Oid, Oid)>,
    introduced: Option<(Oid, Oid)>,
}

fn moves_of(
    repo: &Repository,
    who: &Identity<'_>,
    tip: Oid,
    since: Oid,
    head: Oid,
) -> Result<Moves, String> {
    let mut found = Moves {
        moves: Vec::new(),
        introduced: None,
    };
    let mut at = tip;
    while !is_ancestor_or_same(repo, at, since)? {
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        if is_ancestor_or_same(repo, at, head)? {
            match step(repo, who, at, parent)? {
                Step::Moved => found.moves.push((at, parent)),
                Step::Introduced => found.introduced = Some((at, parent)),
                Step::Unchanged => {}
            }
        }
        at = parent;
    }
    found.moves.reverse();
    Ok(found)
}

/// The oldest commit of the newest run on `tip`'s first-parent history
/// after `since`, held by `head`, whose record of the task is `content`.
fn source_version(
    repo: &Repository,
    who: &Identity<'_>,
    content: &str,
    tip: Oid,
    since: Oid,
    head: Oid,
) -> Result<Option<Oid>, String> {
    let mut found = None;
    let mut at = tip;
    while !is_ancestor_or_same(repo, at, since)? {
        let holds = is_ancestor_or_same(repo, at, head)?
            && matches!(held_at(repo, at, who), Held::One(record) if record.content == content);
        if holds {
            found = Some(at);
        } else if found.is_some() {
            break;
        }
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        at = parent;
    }
    Ok(found)
}

/// Whether the line's copy of the task at `receiver` is the copy at
/// `since`, at every commit of the line's first-parent history after it.
fn line_unchanged(
    repo: &Repository,
    who: &Identity<'_>,
    receiver: Oid,
    since: Oid,
) -> Result<bool, String> {
    let reference = held_at(repo, since, who);
    if matches!(reference, Held::Ambiguous) {
        return Ok(false);
    }
    let mut at = receiver;
    while at != since && is_ancestor_or_same(repo, since, at)? {
        if !held_at(repo, at, who).same(&reference) {
            return Ok(false);
        }
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        at = parent;
    }
    Ok(true)
}

/// The common proof of one move (AC-1's closing sentence): a reviewed
/// planning merge with its exact first parent, holding the identity once
/// on each side.
fn common_proof(repo: &Repository, who: &Identity<'_>, at: Oid, parent: Oid) -> Result<(), String> {
    if admissible_planning_landing_with(repo, at, PLANNING_LANDING_PATHS)? != Some(parent) {
        return Err(format!("{MOVED}: the move at {at} is not a reviewed planning merge of records on the default branch's first-parent history"));
    }
    for side in [at, parent] {
        if !matches!(held_at(repo, side, who), Held::One(_)) {
            return Err(ambiguous(side));
        }
    }
    Ok(())
}

/// Judge `moves` against the line's copy (AC-1, AC-6, AC-10): every move
/// passes the common proof, every move before the last is made in a
/// permitted state at both ends, and the last one is either a home the
/// line already names or, in a permitted state, one the line has not
/// diverged from since `line_since`.
fn judge_moves(
    repo: &Repository,
    who: &Identity<'_>,
    line: &Held,
    moves: &[(Oid, Oid)],
    receiver: Oid,
    line_since: Oid,
) -> Result<(), String> {
    let Some(((last, last_parent), earlier)) = moves.split_last() else {
        return Ok(());
    };
    for (at, parent) in moves {
        common_proof(repo, who, *at, *parent)?;
    }
    let permitted = |at: Oid| permitted_at(repo, at, who.record);
    for (at, parent) in earlier {
        if !permitted(*at) || !permitted(*parent) {
            return Err(format!("{MOVED}: the task moved at {at} while it was not todo or blocked, or while it held an acceptance block, before its last move"));
        }
    }
    let Held::One(arrived) = held_at(repo, *last, who) else {
        return Err(ambiguous(*last));
    };
    if matches!(line, Held::One(copy) if declared(copy) == declared(&arrived)) {
        return Ok(());
    }
    if !permitted(*last) || !permitted(*last_parent) {
        return Err(format!("{MOVED}: the line's copy names another home, and the task was not todo or blocked with no acceptance block when it moved at {last}"));
    }
    if !line_unchanged(repo, who, receiver, line_since)? {
        return Err(format!("{MOVED}: the line's copy names another home and changed since the line last took the default branch, so the line did not agree to the move at {last}"));
    }
    Ok(())
}

/// The anchor the line's own adoption sets (AC-13), when the line's copy
/// is a task it re-homed since `start` whose exact bytes are a default
/// version. A claimed adoption that fails its proof refuses; a copy that is
/// no default version, or a line introduction, sets no anchor.
fn adoption_anchor(
    repo: &Repository,
    who: &Identity<'_>,
    line: &Held,
    receiver: Oid,
    start: Oid,
    default_tip: Oid,
    head: Oid,
) -> Result<Option<HomeAnchor>, String> {
    let Held::One(copy) = line else {
        return Ok(None);
    };
    match held_at(repo, start, who) {
        Held::One(then) if declared(&then) == declared(copy) => return Ok(None),
        Held::Ambiguous => return Err(ambiguous(start)),
        _ => {}
    }
    let Some(source) = source_version(repo, who, &copy.content, default_tip, start, head)? else {
        return Ok(None);
    };
    // The line landing that wrote these bytes: the oldest commit of the
    // run on the line's first-parent history that holds them.
    let mut landing = receiver;
    while let Some(parent) = first_parent(repo, landing)? {
        if parent == start
            || !is_ancestor_or_same(repo, start, parent)?
            || !matches!(held_at(repo, parent, who), Held::One(old) if old.content == copy.content)
        {
            break;
        }
        landing = parent;
    }
    let parent = first_parent(repo, landing)?;
    let held_before = parent.map(|parent| held_at(repo, parent, who));
    if matches!(held_before, Some(Held::Absent)) {
        // An introduction is judged as shared planning, never as an adoption.
        return Ok(None);
    }
    let adopted = parent.is_some()
        && matches!(held_before, Some(Held::One(_)))
        && admissible_planning_landing_with(repo, landing, &[AmendmentPath::Record])? == parent
        && permitted_at(repo, landing, copy)
        && parent.is_some_and(|parent| permitted_at(repo, parent, copy));
    if !adopted {
        return Err(format!("{MOVED}: the line's copy is the default branch's version at {source}, but the line took it at {landing}, which is not a reviewed records-only planning merge of the task while it was todo or blocked"));
    }
    Ok(Some(HomeAnchor {
        line_landing: landing,
        source_version: source,
    }))
}

/// Judge a record the range imports from the default tip (AC-1, AC-6,
/// AC-10, AC-13), against the line's copy at `receiver`.
///
/// # Errors
/// The refusal names the move and its cause.
pub(super) fn judge_home(
    repo: &Repository,
    record: &RecordView,
    default_tip: Oid,
    receiver: Oid,
    head: Oid,
) -> Result<Option<HomeAnchor>, String> {
    let who = Identity::of(record);
    let start = start_point(repo, receiver, default_tip)?;
    let line = held_at(repo, receiver, &who);
    if matches!(line, Held::Ambiguous) {
        return Err(ambiguous(receiver));
    }
    let anchor = adoption_anchor(repo, &who, &line, receiver, start, default_tip, head)?;
    let (since, line_since) = anchor.map_or((start, start), |anchor| {
        (anchor.source_version, anchor.line_landing)
    });
    let found = moves_of(repo, &who, default_tip, since, head)?;
    judge_moves(repo, &who, &line, &found.moves, receiver, line_since)?;
    Ok(anchor)
}

/// Judge a record the range leaves unchanged whose home the line changed,
/// or which the line introduced, since `start`, when the default branch
/// moved the same task in that span (AC-13). It passes as the line's
/// exact adoption, from its anchor, or as the line's own admitted change
/// with every default-branch move still judged.
///
/// # Errors
/// The refusal names the move and its cause.
pub(super) fn judge_unchanged(
    repo: &Repository,
    record: &RecordView,
    default_tip: Oid,
    receiver: Oid,
    head: Oid,
) -> Result<(), String> {
    let who = Identity::of(record);
    let start = start_point(repo, receiver, default_tip)?;
    let found = moves_of(repo, &who, default_tip, start, head)?;
    if found.moves.is_empty() {
        return Ok(());
    }
    let line = held_at(repo, receiver, &who);
    let changed = match (held_at(repo, start, &who), &line) {
        (Held::Absent, Held::One(_)) => true,
        (Held::One(then), Held::One(copy)) => declared(&then) != declared(copy),
        (Held::Ambiguous, _) | (_, Held::Ambiguous) => return Err(ambiguous(receiver)),
        _ => false,
    };
    if !changed {
        return Ok(());
    }
    if let Some(anchor) = adoption_anchor(repo, &who, &line, receiver, start, default_tip, head)? {
        let after = moves_of(repo, &who, default_tip, anchor.source_version, head)?;
        return judge_moves(
            repo,
            &who,
            &line,
            &after.moves,
            receiver,
            anchor.line_landing,
        );
    }
    own_change(repo, &who, receiver, start, head)?;
    judge_moves(repo, &who, &line, &found.moves, receiver, start)
}

/// The line's own change to the task's home since `start` is admitted:
/// its last home change on the line's first-parent history is the line's
/// own planning move, or its introduction is shared planning. It grants no
/// anchor and removes no default-branch move from the walk.
fn own_change(
    repo: &Repository,
    who: &Identity<'_>,
    receiver: Oid,
    start: Oid,
    head: Oid,
) -> Result<(), String> {
    let refused = |at: Oid| {
        format!("{MOVED}: the line's own home change at {at} is not a reviewed planning merge of the task while it was todo or blocked")
    };
    let mut at = receiver;
    while at != start && is_ancestor_or_same(repo, start, at)? {
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        match (held_at(repo, at, who), held_at(repo, parent, who)) {
            (Held::One(_), Held::Absent) => {
                return if is_shared_planning(repo, who.record, at, head)? {
                    Ok(())
                } else {
                    Err(refused(at))
                };
            }
            (Held::One(now), Held::One(then)) if declared(&now) != declared(&then) => {
                let admitted = admissible_planning_landing_with(repo, at, PLANNING_LANDING_PATHS)?
                    == Some(parent)
                    && permitted_at(repo, at, who.record)
                    && permitted_at(repo, parent, who.record);
                return if admitted { Ok(()) } else { Err(refused(at)) };
            }
            (Held::Ambiguous, _) | (_, Held::Ambiguous) => return Err(ambiguous(at)),
            _ => at = parent,
        }
    }
    Err(format!(
        "{MOVED}: the line's own home change for the task is not on its first-parent history"
    ))
}

/// Judge a record that names the receiving line as its home and that the
/// line does not already hold homed there (AC-12): its exact version must
/// stand on the default branch, its introduction there since `start` must
/// be a records-only planning merge of a todo or blocked task, and every
/// move must pass as any home move does. A record the line already homes
/// stays under the criteria freeze, and this grants nothing against it.
///
/// # Errors
/// The refusal names what could not be proven.
pub(super) fn judge_arrival(
    repo: &Repository,
    record: &RecordView,
    default_tip: Oid,
    receiver: Oid,
    head: Oid,
) -> Result<(), String> {
    let who = Identity::of(record);
    let line = held_at(repo, receiver, &who);
    match &line {
        Held::One(copy) if declared(copy) == declared(record) => return Ok(()),
        Held::Ambiguous => return Err(ambiguous(receiver)),
        _ => {}
    }
    let start = start_point(repo, receiver, default_tip)?;
    if source_version(repo, &who, &record.content, default_tip, start, head)?.is_none() {
        return Err(format!(
            "{NO_SOURCE}: no commit on the default branch's first-parent history holds this version"
        ));
    }
    let found = moves_of(repo, &who, default_tip, start, head)?;
    if let Some((at, parent)) = found.introduced {
        let admitted = admissible_planning_landing_with(repo, at, &[AmendmentPath::Record])?
            == Some(parent)
            && permitted_at(repo, at, record);
        if !admitted {
            return Err(format!("{NO_SOURCE}: it was introduced at {at}, which is not a reviewed records-only planning merge of a todo or blocked task"));
        }
    } else if found.moves.is_empty() {
        return Err(format!("{NO_SOURCE}: no planning merge on the default branch introduced it or moved it there since the line last took the default branch"));
    }
    judge_moves(repo, &who, &line, &found.moves, receiver, start)
}
