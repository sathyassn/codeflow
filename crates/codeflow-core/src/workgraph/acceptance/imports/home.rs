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
//! the walk (AC-13). That adoption is the line's last change of the task's
//! home, found by identity, and the walk then starts after the commit where
//! the adopted version first stood on `main`, so neither a later edit on the
//! line nor a later recurrence of the same bytes on `main` moves it. A task
//! the range leaves unchanged is examined when the line changed its home or
//! introduced it at any commit since the merge base, a home it later
//! restored included.

use std::collections::BTreeSet;

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
    /// The line's records-only planning merge that changed the task's home
    /// to the exact version: its last home change, found by identity.
    pub line_landing: Oid,
    /// The default-branch commit where that exact version first stood since
    /// the line's merge base with the default tip. A later commit holding
    /// the same bytes never replaces it.
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

/// The oldest commit on `tip`'s first-parent history after `since`, held by
/// `head`, whose record of the task is `content`: where that exact version
/// first stood. A later commit holding the same bytes again never replaces
/// it, so a move away and an exact restoration after it stay in the walk.
fn first_source_version(
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
        if is_ancestor_or_same(repo, at, head)?
            && matches!(held_at(repo, at, who), Held::One(record) if record.content == content)
        {
            found = Some(at);
        }
        let Some(parent) = first_parent(repo, at)? else {
            break;
        };
        at = parent;
    }
    Ok(found)
}

/// The line's last change of the task's home on its first-parent history
/// after `start`, found by the task's identity.
#[derive(Clone, Copy)]
enum LineChange {
    /// The commit that changed the declared home, and its first parent.
    Moved(Oid, Oid),
    /// The commit that introduced the task.
    Introduced(Oid),
}

/// One commit of the line's first-parent history after `start`.
struct LineStep {
    at: Oid,
    /// Its actual first parent.
    parent: Oid,
    /// What it is compared with: its first parent, or `start` when that
    /// parent is outside the span. The line's last sync of the default
    /// branch is such a commit, so what it took from the default branch is
    /// no change of the line's own.
    before: Oid,
}

/// The commits of the line's first-parent history after `start`, newest
/// first, ending at `start` or at the first commit whose first parent is
/// outside the span.
///
/// # Errors
/// Unreadable history.
fn line_steps(repo: &Repository, receiver: Oid, start: Oid) -> Result<Vec<LineStep>, String> {
    let mut steps = Vec::new();
    let mut at = receiver;
    while at != start && is_ancestor_or_same(repo, start, at)? {
        let Some(parent) = first_parent(repo, at)? else {
            return Err(format!(
                "{MOVED}: cannot read the line's first-parent history at {at}"
            ));
        };
        let inside = is_ancestor_or_same(repo, start, parent)?;
        steps.push(LineStep {
            at,
            parent,
            before: if inside { parent } else { start },
        });
        if !inside {
            break;
        }
        at = parent;
    }
    Ok(steps)
}

/// Every path a commit of the line's first-parent history after `start`
/// changes, each compared as [`line_steps`] compares it: the files whose
/// tasks the line may have re-homed or introduced in that span, renames
/// included (TSK-1378 AC-13). Each path is kept as its exact bytes.
///
/// # Errors
/// Unreadable history or trees.
pub(super) fn line_paths(
    repo: &Repository,
    receiver: Oid,
    start: Oid,
) -> Result<BTreeSet<Vec<u8>>, String> {
    let tree = |at: Oid| {
        repo.find_commit(at)
            .and_then(|commit| commit.tree())
            .map_err(|error| format!("{MOVED}: cannot read the line's tree at {at}: {error}"))
    };
    let mut paths = BTreeSet::new();
    for step in line_steps(repo, receiver, start)? {
        let diff = repo
            .diff_tree_to_tree(Some(&tree(step.before)?), Some(&tree(step.at)?), None)
            .map_err(|error| {
                format!(
                    "{MOVED}: cannot compare the line's trees at {}: {error}",
                    step.at
                )
            })?;
        for delta in diff.deltas() {
            for file in [delta.old_file(), delta.new_file()] {
                if let Some(path) = file.path_bytes() {
                    paths.insert(path.to_vec());
                }
            }
        }
    }
    Ok(paths)
}

/// The line's last home change or introduction of the task after `start`,
/// or `None` when its first-parent history since `start` holds neither.
/// Each commit is compared as [`line_steps`] compares it, so a commit the
/// line later restored still counts, and an earlier sync's import does not.
fn line_change(
    repo: &Repository,
    who: &Identity<'_>,
    receiver: Oid,
    start: Oid,
) -> Result<Option<LineChange>, String> {
    let path = &who.record.path;
    for step in line_steps(repo, receiver, start)? {
        let now = entry_at(repo, step.at, path)?;
        if now.is_some() && now == entry_at(repo, step.before, path)? {
            continue;
        }
        match (held_at(repo, step.at, who), held_at(repo, step.before, who)) {
            (Held::One(_), Held::Absent) => return Ok(Some(LineChange::Introduced(step.at))),
            (Held::One(now), Held::One(then)) if declared(&now) != declared(&then) => {
                return Ok(Some(LineChange::Moved(step.at, step.parent)));
            }
            (Held::Ambiguous, _) | (_, Held::Ambiguous) => return Err(ambiguous(step.at)),
            _ => {}
        }
    }
    Ok(None)
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

/// The anchor the line's own adoption sets (AC-13). The adoption is the
/// line's last home change of the task, found by identity, never the
/// latest commit that wrote the copy's bytes. It anchors only when it is a
/// records-only planning merge with its exact first parent, made after the
/// line last took the default branch, of the task while it was todo or
/// blocked with no acceptance block on both sides, and its record there is
/// exactly a default-branch version since `start` that `start` itself does
/// not hold. The source is where that version first stood. Anything else
/// sets no anchor, so every default-branch move since `start` stays in the
/// walk. Adopting the version the merge base already held agrees to no move
/// of the default branch, so a move away and back on `main` stays judged.
fn adoption_anchor(
    repo: &Repository,
    who: &Identity<'_>,
    change: LineChange,
    start: Oid,
    default_tip: Oid,
    head: Oid,
) -> Result<Option<HomeAnchor>, String> {
    let LineChange::Moved(landing, parent) = change else {
        // An introduction is judged as shared planning, never as an adoption.
        return Ok(None);
    };
    let Held::One(adopted) = held_at(repo, landing, who) else {
        return Err(ambiguous(landing));
    };
    let proved = is_ancestor_or_same(repo, start, parent)?
        && admissible_planning_landing_with(repo, landing, &[AmendmentPath::Record])?
            == Some(parent)
        && permitted_at(repo, landing, &adopted)
        && permitted_at(repo, parent, &adopted);
    if !proved {
        return Ok(None);
    }
    match held_at(repo, start, who) {
        Held::One(base) if base.content == adopted.content => return Ok(None),
        Held::Ambiguous => return Ok(None),
        _ => {}
    }
    let source = first_source_version(repo, who, &adopted.content, default_tip, start, head)?;
    Ok(source.map(|source_version| HomeAnchor {
        line_landing: landing,
        source_version,
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
    let anchor = match line_change(repo, &who, receiver, start)? {
        Some(change) => adoption_anchor(repo, &who, change, start, default_tip, head)?,
        None => None,
    };
    let (since, line_since) = anchor.map_or((start, start), |anchor| {
        (anchor.source_version, anchor.line_landing)
    });
    let found = moves_of(repo, &who, default_tip, since, head)?;
    judge_moves(repo, &who, &line, &found.moves, receiver, line_since)?;
    Ok(anchor)
}

/// Judge a record the range leaves unchanged whose home the line changed,
/// or which the line introduced, at any commit of its first-parent history
/// since `start`, a home it later restored included, when the default
/// branch moved the same task in that span (AC-13). The line's last home
/// change or introduction, found by identity, decides: an exact adoption
/// passes from its anchor; otherwise the change must be the line's own
/// admitted change, and every default-branch move is still judged. A line
/// with no such change since `start` is not examined.
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
    if matches!(line, Held::Ambiguous) {
        return Err(ambiguous(receiver));
    }
    let Some(change) = line_change(repo, &who, receiver, start)? else {
        return Ok(());
    };
    if let Some(anchor) = adoption_anchor(repo, &who, change, start, default_tip, head)? {
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
    own_change(repo, &who, change, head)?;
    judge_moves(repo, &who, &line, &found.moves, receiver, start)
}

/// The line's own change to the task's home is admitted: its last home
/// change is a planning merge of the task while it was todo or blocked on
/// both sides, or its introduction is shared planning. It grants no anchor
/// and removes no default-branch move from the walk.
fn own_change(
    repo: &Repository,
    who: &Identity<'_>,
    change: LineChange,
    head: Oid,
) -> Result<(), String> {
    let (at, admitted) = match change {
        LineChange::Introduced(at) => (at, is_shared_planning(repo, who.record, at, head)?),
        LineChange::Moved(at, parent) => (
            at,
            admissible_planning_landing_with(repo, at, PLANNING_LANDING_PATHS)? == Some(parent)
                && permitted_at(repo, at, who.record)
                && permitted_at(repo, parent, who.record),
        ),
    };
    if admitted {
        Ok(())
    } else {
        Err(format!("{MOVED}: the line's own home change at {at} is not a reviewed planning merge of the task while it was todo or blocked"))
    }
}

/// The line landing the walk binds for `record`, and the anchor it sets,
/// apart from whether the record is admitted: for fixtures only.
#[cfg(test)]
pub(super) fn line_landing(
    repo: &Repository,
    record: &RecordView,
    default_tip: Oid,
    receiver: Oid,
    head: Oid,
) -> Result<(Option<Oid>, Option<HomeAnchor>), String> {
    let who = Identity::of(record);
    let start = start_point(repo, receiver, default_tip)?;
    let Some(change) = line_change(repo, &who, receiver, start)? else {
        return Ok((None, None));
    };
    let anchor = adoption_anchor(repo, &who, change, start, default_tip, head)?;
    let (LineChange::Moved(at, _) | LineChange::Introduced(at)) = change;
    Ok((Some(at), anchor))
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
    if first_source_version(repo, &who, &record.content, default_tip, start, head)?.is_none() {
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
