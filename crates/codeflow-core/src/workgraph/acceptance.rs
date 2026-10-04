//! Acceptance bound to the reviewed commit (SPC-013 R-52, R-53, R-60).
//!
//! The structural rules of an acceptance block live in
//! [`super::record_text`]; this module adds what needs git: the block names
//! the commit that was reviewed, and nothing but the record's status and
//! Closeout changed after it. Task landings and release imports identify
//! the source of that same reviewed span ([`bind_completion`]); `task status
//! complete` and `codeflow ci` share that judge; a waiver names the planning amendment on the
//! target or in its own reviewed range that changed that criterion. The
//! task may amend its own criteria until complete; reopened criteria stay
//! frozen. A range touching the
//! adopter-facing path set belongs to a task with a journey criterion.
//!
//! The checker proves structure and binding only ([`SCOPE_NOTE`]). It does
//! not prove the evidence is honest, and the review reference is
//! recheckable, not authenticated.

use git2::{Oid, Repository};

use super::classify::is_planning_path;
use super::lifecycle::{Graph, RecordView};
use super::record_text::{
    acceptance_blocks, frontmatter_len, outcome_word, scan_record, section_span, AcceptanceBlock,
    Criterion,
};
use super::work_start::RecordKind;

/// The acceptance block names the reviewed commit, and waivers name their
/// amendment (R-60). Level: `git.work_records`.
pub const BINDING_RULE: &str = "work.acceptance_binding";
/// Criteria changes outside the own-task or planning allowance, including
/// changes to reopened criteria (R-52). Always blocks.
pub const FROZEN_RULE: &str = "work.criteria_frozen";
/// An adopter-facing range belongs to a task with a journey criterion, and a
/// leaf serving the epic's journey says what ran (R-53). Level:
/// `git.work_records`.
pub const JOURNEY_RULE: &str = "work.journey_criterion";

/// What every report of these rules says about itself (R-60).
pub const SCOPE_NOTE: &str = "the acceptance check proves structure and binding only: it does not \
     prove that the evidence is honest, and the review reference is recheckable, not authenticated";

/// One finding of these rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub message: String,
    /// Informational evidence, never a refusal.
    pub note: bool,
    /// The record path of the epic whose own acceptance block this finding
    /// binds, so its remedy is the epic's, never a task's.
    pub epic_record: Option<String>,
}

pub(super) fn finding(rule: &'static str, message: String) -> Finding {
    Finding {
        rule,
        message,
        note: false,
        epic_record: None,
    }
}

/// The active (not superseded) acceptance block of a record, when exactly
/// one parses. Structure is judged elsewhere; binding needs a parsed block.
pub(super) fn active_block(record: &RecordView) -> Option<AcceptanceBlock> {
    let mut blocks = record
        .active_blocks()
        .into_iter()
        .filter_map(|block| block.parsed.ok());
    let block = blocks.next()?;
    blocks.next().is_none().then_some(block)
}

/// A commit named by its object id, never by a ref name: a branch named like
/// the id cannot stand in for the reviewed commit or the amendment.
pub(crate) fn commit_of(repo: &Repository, value: &str) -> Option<Oid> {
    super::work_start::commit_by_object_id(repo, value.trim())
        .ok()
        .map(|commit| commit.id())
}

fn is_first_parent_ancestor(repo: &Repository, ancestor: Oid, mut tip: Oid) -> bool {
    loop {
        if tip == ancestor {
            return true;
        }
        let Ok(parent) = repo.find_commit(tip).and_then(|commit| commit.parent_id(0)) else {
            return false;
        };
        tip = parent;
    }
}

fn is_ancestor_or_same(repo: &Repository, ancestor: Oid, of: Oid) -> bool {
    ancestor == of || repo.graph_descendant_of(of, ancestor).unwrap_or(false)
}

pub(super) fn blob_at(repo: &Repository, commit: Oid, path: &str) -> Option<String> {
    let tree = repo.find_commit(commit).ok()?.tree().ok()?;
    let entry = tree.get_path(std::path::Path::new(path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    Some(String::from_utf8_lossy(blob.content()).into_owned())
}

/// A record's text without its frontmatter `status:` field and its
/// `## Closeout` section: what must not change after the reviewed commit.
/// The Closeout is the section the record parser finds (the one the
/// acceptance block is read from), so a heading inside a comment, an HTML
/// block or a fence neither ends nor opens it.
fn reviewed_part(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let frontmatter = frontmatter_len(&lines);
    let scanned = scan_record(&lines);
    let closeout = section_span(&scanned, "## Closeout");
    lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            let status_field = *index > 0 && *index < frontmatter && line.starts_with("status:");
            let in_closeout = closeout.is_some_and(|(start, end)| (start..end).contains(index));
            !status_field && !in_closeout
        })
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// Where a completion lands: the commit that introduced the active block,
/// or the working tree `task status complete` is about to commit on top of
/// `head`, with every path it changes.
#[derive(Debug, Clone, Copy)]
pub enum Landing<'a> {
    /// A commit of the range (C in the binding rule).
    Commit(Oid),
    /// The working tree over `head`; `changed` lists staged, unstaged and
    /// untracked paths, the record's own included.
    Worktree { head: Oid, changed: &'a [String] },
}

impl Landing<'_> {
    /// The commit the completion sits on: C itself, or the working tree's
    /// `HEAD`.
    fn commit(&self) -> Oid {
        match *self {
            Self::Commit(oid) | Self::Worktree { head: oid, .. } => oid,
        }
    }
}

/// Which transport can carry a reviewed task onto the completion's line.
/// Release imports are attributed by `release_line` before entering this judge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// Direct work, including every direct release-line change.
    Direct,
    /// An ordinary task line may carry review through a clean task landing.
    TaskLanding,
}

/// Bind a completion to its reviewed source. Only this record's status and
/// Closeout may differ in the reviewed segment. A clean task landing can
/// transport that segment onto a line; a reopening range always reviews its
/// own work. `base` pins the range's anchored old review when supplied.
/// A waiver names the criterion's record-only amendment on its target;
/// `bind_completion_with_amendment` adds the task's own range.
#[must_use]
pub fn bind_completion(
    repo: &Repository,
    task: &RecordView,
    graph: &Graph,
    landing: Landing<'_>,
    default_target: Option<Oid>,
    transport: Transport,
    base: Option<Oid>,
) -> Vec<Finding> {
    bind_completion_with_amendment(
        repo,
        task,
        graph,
        landing,
        default_target,
        transport,
        base,
        None,
    )
}

/// [`bind_completion`] with the own-task waiver allowance (TSK-184): in the
/// task's own pull request, before the task was ever complete on its
/// target, a waiver may also name a record-only amendment commit of this
/// record in that range, after `own_range_base` and strictly before the
/// reviewed commit. Once the pull request lands, that commit is on the
/// target, so a line and the release judge accept the same waiver under
/// the target rule. `None`, or a range that reopens the task, keeps the
/// target rule alone.
#[allow(clippy::too_many_arguments)] // bind_completion's inputs and the own-range base.
pub(crate) fn bind_completion_with_amendment(
    repo: &Repository,
    task: &RecordView,
    graph: &Graph,
    landing: Landing<'_>,
    default_target: Option<Oid>,
    transport: Transport,
    base: Option<Oid>,
    own_range_base: Option<Oid>,
) -> Vec<Finding> {
    let Some(block) = active_block(task) else {
        return Vec::new();
    };
    let target = task_target(repo, task, default_target);
    // On the target itself there is no PR range to anchor. The status verb
    // still checks the transition and reviewed span, including uncommitted
    // changes; a named fix branch must instead review inside its range.
    let anchor =
        base.or_else(|| target.and_then(|tip| repo.merge_base(tip, landing.commit()).ok()));
    let on_target = base.is_none()
        && matches!(landing, Landing::Worktree { .. })
        && repo
            .head()
            .ok()
            .is_some_and(|head| head.shorthand().ok() == task.integration_target.as_deref());
    // A completion reopened before this range is recovered from the history
    // below. When that completion is on the target at the anchored base, a
    // separate pull request reopened it (R-119 keeps that valid): the range
    // does not reopen the task, and the fix lands as any task does, so a
    // task landing may carry its review. Otherwise this range completed and
    // reopened the task itself.
    let mut recovered = false;
    let mut unreadable = None;
    let reopen = (!on_target)
        .then_some(anchor)
        .flatten()
        .and_then(|anchor| {
            let content = blob_at(repo, anchor, &task.path)?;
            let old = RecordView::parse(RecordKind::Task, &task.path, &content).ok()?;
            let reopened = old.status == "complete"
                && (matches!(landing, Landing::Worktree { .. })
                    || super::lifecycle::is_recompletion(Some(&old), task)
                    || super::lifecycle::reopened_in_range(
                        repo,
                        &anchor.to_string(),
                        Some(&landing.commit().to_string()),
                        &Graph::default().with(task.clone()),
                    )
                    .contains(&task.id));
            reopened.then_some((anchor, old))
        })
        .or_else(|| {
            let (at, old, note) =
                recovered_completion(repo, task, &block, landing, anchor, default_target)?;
            unreadable = note;
            recovered = true;
            Some((at, old))
        });
    let before_range = |at: Oid| anchor.is_some_and(|base| is_ancestor_or_same(repo, at, base));
    // A reopening range reviews its own work: no task landing or clean line
    // merge stacks on its review, and no own-range waiver (R-60).
    let reopens_range = reopen
        .as_ref()
        .is_some_and(|(at, _)| !recovered || !before_range(*at));
    let own_range_base = own_range_base.filter(|_| !reopens_range);
    let mut findings = Vec::new();
    findings.extend(unreadable.map(|message| finding(BINDING_RULE, message)));
    if let Some((_, old)) = &reopen {
        findings.extend(
            super::lifecycle::reopen_problems(Some(old), task)
                .into_iter()
                .map(|message| finding(BINDING_RULE, format!("{}: {message}", task.id))),
        );
    }
    let mut bind = |message: String| findings.push(finding(BINDING_RULE, message));
    match commit_of(repo, &block.reviewed) {
        None => bind(format!(
            "{}: reviewed commit {} is not in this repository",
            task.id, block.reviewed
        )),
        Some(reviewed) => {
            // The review lies after the reopened completion. One exception:
            // a completion made and reopened inside this range (not on the
            // target at the range base) stood last at `at`, and a review of
            // `at` covers every change the range made before the reopen;
            // the binding below still refuses any later change but this
            // record's status and Closeout (AC-2).
            let completed_in_range = |at: Oid| recovered && anchor.is_some() && !before_range(at);
            let problem = if reopen.as_ref().is_some_and(|(at, _)| {
                (reviewed == *at && !completed_in_range(*at))
                    || !is_ancestor_or_same(repo, *at, reviewed)
            }) {
                Some(format!("a reopened task's reviewed commit {reviewed} must lie inside the fix range, after its anchored base"))
            } else if transport == Transport::TaskLanding && !reopens_range {
                binding_problem(repo, task, landing, reviewed, transport)
            } else {
                binding_problem(repo, task, landing, reviewed, Transport::Direct)
            };
            if let Some(problem) = problem {
                bind(format!("{}: {problem}", task.id));
            }
        }
    }
    for (id, result) in &block.criteria {
        if result.outcome == "waived" {
            if let Some(problem) = waiver_problem(
                repo,
                task,
                id,
                &result.evidence,
                landing,
                task_target(repo, task, default_target),
                own_range_base,
            ) {
                bind(format!("{}: {id} waiver {problem}", task.id));
            }
        }
    }
    findings.extend(leaf_journey(task, graph, &block));
    findings
}

/// Bind an epic's own acceptance block (SPC-013 R-33, R-60), which proves
/// the criteria no live task serves: its reviewed commit is in this
/// repository and is the completion's commit or its ancestor, with only
/// this record's status and Closeout changed after it, and each waiver
/// names a planning amendment of that criterion of this epic that the
/// reviewed commit contains. The amendment rides in the epic's batched
/// amendment on its line, so the reviewed candidate, not one target branch,
/// is where it must be found. An epic without an own block binds nothing.
#[must_use]
pub fn bind_epic_completion(
    repo: &Repository,
    epic: &RecordView,
    landing: Landing<'_>,
) -> Vec<Finding> {
    let Some(block) = active_block(epic) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    let mut bind = |message: String| {
        findings.push(Finding {
            epic_record: Some(epic.path.clone()),
            ..finding(BINDING_RULE, format!("{}: {message}", epic.id))
        });
    };
    let reviewed = commit_of(repo, &block.reviewed);
    match reviewed {
        None => bind(format!(
            "reviewed commit {} is not in this repository",
            block.reviewed
        )),
        Some(reviewed) => {
            if let Some(problem) = reviewed_span_problem(repo, epic, landing, reviewed, false) {
                bind(problem);
            }
        }
    }
    for (id, result) in &block.criteria {
        if result.outcome == "waived" {
            if let Some(problem) = epic_waiver_problem(repo, epic, id, &result.evidence, reviewed) {
                bind(format!("{id} waiver {problem}"));
            }
        }
    }
    findings
}

/// Why a waiver in an epic's own block does not name the planning
/// amendment of criterion `id` of `epic` inside the reviewed commit, if it
/// does not.
fn epic_waiver_problem(
    repo: &Repository,
    epic: &RecordView,
    id: &str,
    evidence: &str,
    reviewed: Option<Oid>,
) -> Option<String> {
    let named = evidence.trim();
    let Some(amendment) = commit_of(repo, named) else {
        return Some(format!("names {named}, which is not a commit here"));
    };
    let Some(reviewed) = reviewed else {
        return Some(format!(
            "names {named}, which cannot be checked without the reviewed commit"
        ));
    };
    if !is_ancestor_or_same(repo, amendment, reviewed) {
        return Some(format!(
            "names {named}, which the reviewed commit {reviewed} does not contain; a waiver is a planning amendment the review saw"
        ));
    }
    let Ok(commit) = repo.find_commit(amendment) else {
        return Some(format!("names {named}, which cannot be read"));
    };
    let parent = commit.parent_id(0).ok();
    match non_planning_change(repo, parent, amendment) {
        Ok(None) => {}
        Ok(Some(path)) => {
            return Some(format!(
                "names {named}, which also changes {path}; a planning amendment changes planning records only"
            ));
        }
        Err(_) => return Some(format!("names {named}, whose change cannot be read")),
    }
    let criterion = |oid: Option<Oid>| {
        oid.and_then(|oid| blob_at(repo, oid, &epic.path))
            .and_then(|content| RecordView::parse(epic.kind, &epic.path, &content).ok())
            .and_then(|record| {
                record
                    .criteria
                    .items
                    .into_iter()
                    .find(|item| item.id == id)
                    .map(|item| item.text)
            })
    };
    match (criterion(parent), criterion(Some(amendment))) {
        (Some(before), Some(after)) if before != after => None,
        _ => Some(format!(
            "names {named}, which does not amend {id} of {}",
            epic.id
        )),
    }
}

/// The binding findings for every epic a range completes or whose active
/// block it changes. A planning or line range (`amendable`) binds each at
/// the commit that introduced its block; any other range owns all its
/// changes and binds at `head`.
pub(super) fn epic_completions_in_range(
    repo: &Repository,
    before: &Graph,
    after: &Graph,
    head: Oid,
    amendable: bool,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for epic in after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Epic && record.status == "complete")
    {
        let Some(block) = active_block(epic) else {
            continue;
        };
        let unchanged = before.records.get(&epic.id).is_some_and(|then| {
            then.status == "complete" && active_block(then).as_ref() == Some(&block)
        });
        if unchanged {
            continue;
        }
        let origin = if amendable {
            introduced_at(repo, epic, &block, head)
        } else {
            head
        };
        findings.extend(bind_epic_completion(repo, epic, Landing::Commit(origin)));
    }
    findings
}

/// Find the prior completed record when a range or target checkout starts
/// after the task was reopened. The range base can be `todo`, so it cannot
/// itself supply the acceptance block that the reopen must preserve.
fn previous_completion(
    repo: &Repository,
    task: &RecordView,
    block: &AcceptanceBlock,
    landing: Landing<'_>,
) -> Option<(Oid, RecordView)> {
    let mut at = landing.commit();
    match landing {
        Landing::Commit(head) => {
            let introduced = introduced_at(repo, task, block, head);
            at = repo.find_commit(introduced).ok()?.parent_id(0).ok()?;
        }
        Landing::Worktree { head, .. } => {
            let current = blob_at(repo, head, &task.path)
                .and_then(|content| RecordView::parse(RecordKind::Task, &task.path, &content).ok());
            if current.as_ref().is_some_and(|record| {
                record.status == "complete"
                    && active_block(record).as_ref() == Some(block)
                    && record.superseded_blocks() == task.superseded_blocks()
            }) {
                let introduced = introduced_at(repo, task, block, head);
                at = repo.find_commit(introduced).ok()?.parent_id(0).ok()?;
            }
        }
    }
    let mut crossed_reopen = false;
    loop {
        let content = blob_at(repo, at, &task.path)?;
        let record = RecordView::parse(RecordKind::Task, &task.path, &content).ok()?;
        if record.status == "complete" {
            return crossed_reopen.then_some((at, record));
        }
        crossed_reopen = true;
        at = repo.find_commit(at).ok()?.parent_id(0).ok()?;
    }
}

/// Every path the working tree changes against `HEAD`: staged, unstaged and
/// untracked, ignored files excepted. The reviewed result is a commit, so
/// these are what a completion made here adds to it.
///
/// # Errors
///
/// Returns the git error when the working tree's state cannot be read.
pub fn worktree_changes(repo: &Repository) -> Result<Vec<String>, git2::Error> {
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    Ok(repo
        .statuses(Some(&mut options))?
        .iter()
        .map(|entry| String::from_utf8_lossy(entry.path_bytes()).replace('\\', "/"))
        .collect())
}

/// Apply the one reviewed-span check, resolving task-landing transport only
/// when the source and completion do not share a directly reviewed span.
fn binding_problem(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    reviewed: Oid,
    transport: Transport,
) -> Option<String> {
    // A task landing's own span may stack (TSK-184): the task branch may
    // re-merge its integration target cleanly after the review. Direct
    // work, a reopening range and a completion bound at a head never stack.
    let direct = reviewed_span_problem(
        repo,
        task,
        landing,
        reviewed,
        transport == Transport::TaskLanding,
    )?;
    if transport == Transport::Direct {
        return Some(direct);
    }
    match task_landing(repo, task, landing, reviewed) {
        Landed::NoMerge => Some(direct),
        Landed::Span { merge, head } => reviewed_span_problem(repo, task, Landing::Commit(head), reviewed, false)
            .map(|problem| format!("the landing merge {merge} brings {head}, and {problem}; review the result that landed")),
        Landed::Refused(problem) => Some(problem),
    }
}

/// The only binding predicate: the reviewed commit is the source head or
/// its ancestor, with only the task's status and Closeout changed afterward.
/// With `stacking`, a task pull request's own chain may also re-merge its
/// integration target cleanly after the review (TSK-184); direct work, a
/// reopening range and a completion bound at a head (SPC-013 R-120) never
/// stack.
fn reviewed_span_problem(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    reviewed: Oid,
    stacking: bool,
) -> Option<String> {
    if let Landing::Worktree { changed, .. } = landing {
        let outside: Vec<&str> = changed
            .iter()
            .map(String::as_str)
            .filter(|path| *path != task.path)
            .collect();
        if !outside.is_empty() {
            return Some(format!(
                "uncommitted changes outside the record were never reviewed ({}); commit, remove or ignore them, then review the result again",
                outside.join(", ")
            ));
        }
    }
    let at = landing.commit();
    // The record as the completion wrote it: a later planning amendment on
    // the line is not part of this completion.
    let completed = match landing {
        Landing::Commit(oid) => blob_at(repo, oid, &task.path),
        Landing::Worktree { .. } => None,
    }
    .unwrap_or_else(|| task.content.clone());
    if !is_ancestor_or_same(repo, reviewed, at) {
        return Some(format!(
            "reviewed commit {reviewed} is not the head or an ancestor of it; review the result that lands"
        ));
    }
    // Reviewed stacking: after the review, the task branch may re-merge
    // its integration target cleanly, and its own commits may change only
    // this record's status and Closeout. Anything else is judged as the
    // whole change from the reviewed commit to C.
    let stack = if stacking {
        stacked(repo, task, reviewed, at)
    } else {
        Stacked::No
    };
    match stack {
        Stacked::Clean => {
            let Some(then) = blob_at(repo, reviewed, &task.path) else {
                return Some(format!(
                    "{} is not in the reviewed commit, so its scope was never reviewed",
                    task.path
                ));
            };
            (reviewed_part(&then) != reviewed_part(&completed))
                .then(|| "the record changed outside its status and Closeout after the reviewed commit; review the result again".into())
        }
        Stacked::Unclean(merge) => Some(format!(
            "merge {merge} is not a clean re-merge from the task's integration target; review the result again"
        )),
        Stacked::No => later_change(repo, &task.path, &completed, reviewed, at).map(|problem| {
            format!("{problem} after the reviewed commit {reviewed}; review the result again")
        }),
    }
}

/// How C's first-parent chain back to the reviewed commit stacks on it.
enum Stacked {
    /// Every commit between is a clean re-merge from the task's integration
    /// target or changes only this record's status and Closeout.
    Clean,
    /// A merge from the integration target whose tree is not the clean
    /// re-merge of its parents.
    Unclean(Oid),
    /// The reviewed commit is not on the chain, or a commit between changes
    /// more than a clean re-merge or the record allows.
    No,
}

fn stacked(repo: &Repository, task: &RecordView, reviewed: Oid, at: Oid) -> Stacked {
    let target = task_target(repo, task, None);
    let mut cursor = at;
    while cursor != reviewed {
        let Ok(commit) = repo.find_commit(cursor) else {
            return Stacked::No;
        };
        let Ok(parent) = commit.parent_id(0) else {
            return Stacked::No;
        };
        if commit.parent_count() == 2 {
            let from_target = target.is_some_and(|tip| {
                commit
                    .parent_id(1)
                    .is_ok_and(|side| is_first_parent_ancestor(repo, side, tip))
            });
            if !from_target {
                return Stacked::No;
            }
            if !is_clean_remerge(repo, &commit).unwrap_or(false) {
                return Stacked::Unclean(cursor);
            }
        } else if blob_at(repo, cursor, &task.path).is_none_or(|content| {
            later_change(repo, &task.path, &content, parent, cursor).is_some()
        }) {
            return Stacked::No;
        }
        cursor = parent;
    }
    Stacked::Clean
}

/// Provenance of a possible task landing.
enum Landed {
    /// No merge on C's first-parent chain has `reviewed` as its second
    /// parent.
    NoMerge,
    /// The clean merge transports this task head; its reviewed span is
    /// checked by the same predicate as direct work.
    Span { merge: Oid, head: Oid },
    /// A landing merge exists, and this is why it does not carry the review.
    Refused(String),
}

/// Resolve the task-landing provenance. This checks transport structure,
/// never infers a line's parent and never grants a review verdict itself.
fn task_landing(
    repo: &Repository,
    task: &RecordView,
    landing: Landing<'_>,
    reviewed: Oid,
) -> Landed {
    let unreadable = |what: &str| Landed::Refused(format!("{what} cannot be read"));
    let (start, completion) = match landing {
        Landing::Worktree { head, changed } => (
            Some(head),
            changed
                .iter()
                .find(|path| **path != task.path && !is_planning_path(path))
                .cloned(),
        ),
        Landing::Commit(at) => {
            let Ok(commit) = repo.find_commit(at) else {
                return unreadable("the completing commit");
            };
            let parent = commit.parent_id(0).ok();
            match non_planning_change(repo, parent, at) {
                Ok(outside) => (parent, outside),
                Err(_) => return unreadable("the completing commit's change"),
            }
        }
    };
    let mut after_merge = None;
    let mut at = start;
    // The landing merge is the commit of C's first-parent chain that brought
    // `reviewed` onto it: its first parent does not hold `reviewed`.
    let merge = loop {
        let Some(oid) =
            at.filter(|oid| *oid != reviewed && is_ancestor_or_same(repo, reviewed, *oid))
        else {
            return Landed::NoMerge;
        };
        let Ok(commit) = repo.find_commit(oid) else {
            return unreadable("the first-parent chain");
        };
        let first = commit.parent_id(0).ok();
        if !first.is_some_and(|first| is_ancestor_or_same(repo, reviewed, first)) {
            if commit.parent_count() == 2 {
                break commit;
            }
            return Landed::NoMerge;
        }
        if commit.parent_count() < 2 && after_merge.is_none() {
            match non_planning_change(repo, commit.parent_id(0).ok(), oid) {
                Ok(None) => {}
                Ok(Some(path)) => after_merge = Some((oid, path)),
                Err(_) => return unreadable("the first-parent chain"),
            }
        }
        at = commit.parent_id(0).ok();
    };
    // The reviewed commit is the merge's second parent, or an ancestor of it
    // after which only this record's status and Closeout changed (R-60).
    let Ok(second) = merge.parent_id(1) else {
        return unreadable("the landing merge");
    };
    if let Some(path) = completion {
        return Landed::Refused(format!(
            "the completion also changes {path}; a completion after the landing merge {} of the reviewed commit {reviewed} changes planning records only",
            merge.id()
        ));
    }
    if let Some((commit, path)) = after_merge {
        return Landed::Refused(format!(
            "{commit} changes {path} after the landing merge {} of the reviewed commit {reviewed}; only merges and planning records may follow it",
            merge.id()
        ));
    }
    match is_clean_remerge(repo, &merge) {
        Ok(true) => Landed::Span { merge: merge.id(), head: second },
        Ok(false) => Landed::Refused(format!(
            "the landing merge {} of the reviewed commit {reviewed} is not the clean re-merge of its parents; review the result that landed",
            merge.id()
        )),
        Err(_) => unreadable("the landing merge"),
    }
}

/// Whether `merge`'s tree is what merging its two parents gives with no
/// conflict: a merge that added or dropped anything of its own (an evil
/// merge) or resolved a conflict landed a result nobody reviewed.
pub(super) fn is_clean_remerge(
    repo: &Repository,
    merge: &git2::Commit<'_>,
) -> Result<bool, git2::Error> {
    let merged = repo.merge_commits(&merge.parent(0)?, &merge.parent(1)?, None)?;
    if merged.has_conflicts() {
        return Ok(false);
    }
    let mut recorded = git2::Index::new()?;
    recorded.read_tree(&merge.tree()?)?;
    let entries = |index: &git2::Index| {
        let mut entries: Vec<(Vec<u8>, Oid, u32)> = index
            .iter()
            .map(|entry| (entry.path, entry.id, entry.mode))
            .collect();
        entries.sort();
        entries
    };
    Ok(entries(&merged) == entries(&recorded))
}

/// Why the diff from `reviewed` to `head` is more than the status and
/// Closeout of the record at `path`, which reads `completed` at `head`, if
/// it is.
fn later_change(
    repo: &Repository,
    path: &str,
    completed: &str,
    reviewed: Oid,
    head: Oid,
) -> Option<String> {
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree()).ok();
    let (Some(before), Some(after)) = (tree(reviewed), tree(head)) else {
        return Some("the trees cannot be read".to_string());
    };
    let Ok(diff) = repo.diff_tree_to_tree(Some(&before), Some(&after), None) else {
        return Some("the diff cannot be read".to_string());
    };
    let other: Vec<String> = diff
        .deltas()
        .flat_map(|delta| [delta.old_file().path(), delta.new_file().path()])
        .flatten()
        .map(|changed| changed.to_string_lossy().replace('\\', "/"))
        .filter(|changed| changed.as_str() != path)
        .collect();
    if let Some(changed) = other.first() {
        return Some(format!("{changed} changed"));
    }
    let Some(then) = blob_at(repo, reviewed, path) else {
        return Some(format!(
            "{path} is not in the reviewed commit, so its scope was never reviewed; it appeared"
        ));
    };
    (reviewed_part(&then) != reviewed_part(completed))
        .then(|| "the record changed outside its status and Closeout".to_string())
}

/// The tip of the line `task` belongs to: its declared integration target
/// as a local or remote-tracking branch, or `default_target` when it
/// declares none. A declared target that does not resolve is `None`.
fn task_target(repo: &Repository, task: &RecordView, default_target: Option<Oid>) -> Option<Oid> {
    match task
        .integration_target
        .as_deref()
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        Some(target) => super::work_start::target_reference(repo, target).map(|commit| commit.id()),
        None => default_target,
    }
}

/// The completion this range reopened, recovered from the history below
/// the landing, with the criteria a re-completion must keep. The recovered
/// completion supplies the archive and the old review boundary. Its
/// criteria may predate a planning pull request that amended them on the
/// target after the reopen (R-52), so the criteria are the target's
/// record's, found by identity in any layout. A task no reference point of
/// its targets holds, and no branch outside the range records, is new in
/// this range: its own pull request may change its criteria with the
/// reopen (TSK-217). When a record that may be the task cannot be read, a
/// target it names does not resolve, or a branch outside the range records
/// it, the recovered criteria stay and a changed set gets the returned
/// refusal.
fn recovered_completion(
    repo: &Repository,
    task: &RecordView,
    block: &AcceptanceBlock,
    landing: Landing<'_>,
    anchor: Option<Oid>,
    default_target: Option<Oid>,
) -> Option<(Oid, RecordView, Option<String>)> {
    let (at, mut old) = previous_completion(repo, task, block, landing)?;
    let mut refusal = None;
    if let Some(anchor) = anchor {
        let range = Range {
            anchor,
            head: landing.commit(),
        };
        match target_record(repo, task, range, &old, default_target) {
            Presence::Present(record) => old.criteria = record.criteria,
            Presence::Absent => old.criteria = task.criteria.clone(),
            Presence::Unreadable(reason) => {
                if old.criteria.signature() != task.criteria.signature() {
                    refusal = Some(format!(
                        "{}: cannot tell whether a target holds this task, so its criteria stay as they were: {reason}",
                        task.id
                    ));
                }
            }
            Presence::Elsewhere { holder, commit } => {
                if old.criteria.signature() != task.criteria.signature() {
                    refusal = Some(format!(
                        "{}: `{holder}` records this task outside this range (commit {commit:.9} adds or changes its record), so the task is not new here and its criteria stay as they were; if `{holder}` is a stale copy, delete it, or merge it if it is newer work on this task, and run this again",
                        task.id
                    ));
                }
            }
        }
    }
    Some((at, old, refusal))
}

/// The commits a completion is judged over: those `head` reaches and
/// `anchor` does not, and the anchor itself.
#[derive(Clone, Copy)]
struct Range {
    anchor: Oid,
    head: Oid,
}

/// Whether a reference point holds a task, found by identity.
enum Presence {
    /// No record there carries the task's id or uid.
    Absent,
    /// The record that carries it.
    Present(Box<RecordView>),
    /// The answer is unknown: a required target does not resolve here, or
    /// a record at the task's own path does not parse; the reason.
    Unreadable(String),
    /// No target point holds it, but a branch or remote-tracking ref
    /// outside the range records it: `commit`, which `holder` reaches and
    /// the range's head does not, adds or changes the task's record.
    Elsewhere { holder: String, commit: Oid },
}

/// The target's record of `task`, read at every reference point the verb or
/// `codeflow ci` could judge it against, so neither an older range base, a
/// stale local branch, an upstream on another remote, nor a retargeted
/// record makes a task a target holds look new (TSK-217).
///
/// The targets are every integration target the task's record has named:
/// the declared one, the recovered completion's, and each version of the
/// record in the range, the anchor's version included. A record retargeted
/// in this pull request still names its former target in an earlier
/// version, and a branch cut from an integration line carries that line's
/// version, so the record's own history supplies its provenance.
///
/// The first point that holds the task supplies the criteria to keep, so
/// the order is deliberate: the range anchor first, the record this range
/// was cut from (R-52); then the tips of the declared target, remote ones
/// before the local branch, since a planning amendment landed there after
/// the reopen is the criteria that bind and a local branch may be stale;
/// then the tips of each former target; then `default_target` (the pull
/// request's base in CI, the resolved default target in the verb); then
/// the tips of the default target, which catch a record retargeted away
/// from it.
///
/// It fails closed: a required target (each one the record names, and the
/// default target) with no tip here makes the answer unreadable, never
/// absent. And when no point holds the task, it is new only if no branch
/// or remote-tracking ref outside the range records it ([`holder_outside`]):
/// the range's own history is the branch's to rewrite, other refs are not.
fn target_record(
    repo: &Repository,
    task: &RecordView,
    range: Range,
    recovered: &RecordView,
    default_target: Option<Oid>,
) -> Presence {
    let uid = record_uid(&task.content);
    let mut index = RecordIndex::new(repo, &task.id, uid.clone());
    let mut targets = Vec::new();
    let mut add = |name: Option<&str>| {
        let name = name.map(str::trim).filter(|name| !name.is_empty());
        if let Some(name) = name {
            if !targets.iter().any(|known| known == name) {
                targets.push(name.to_string());
            }
        }
    };
    add(task.integration_target.as_deref());
    add(recovered.integration_target.as_deref());
    match named_targets(repo, &mut index, range) {
        Ok(names) => names.iter().for_each(|name| add(Some(name))),
        Err(reason) => return Presence::Unreadable(reason),
    }
    let mut points = vec![range.anchor];
    for name in &targets {
        let tips = target_tips(repo, name);
        if tips.is_empty() {
            return Presence::Unreadable(format!(
                "the target `{name}`, which this task's record names, does not resolve here; fetch it (`git fetch origin {name}`)"
            ));
        }
        points.extend(tips);
    }
    points.extend(default_target);
    match default_target_name(repo) {
        Ok(name) => {
            let tips = target_tips(repo, &name.name);
            if tips.is_empty() {
                return Presence::Unreadable(format!(
                    "the default target `{0}`{1} does not resolve here; fetch it (`git fetch origin {0}`)",
                    name.name,
                    if name.from_origin_head { ", which `origin/HEAD` names," } else { "" }
                ));
            }
            points.extend(tips);
        }
        Err(reason) => return Presence::Unreadable(reason),
    }
    let mut seen = std::collections::HashSet::new();
    points.retain(|point| seen.insert(*point));
    let own_file = std::path::Path::new(&task.path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let mut unreadable = None;
    for point in points {
        match presence_at(repo, point, &task.id, uid.as_deref(), &own_file) {
            Presence::Unreadable(reason) => {
                unreadable.get_or_insert(reason);
            }
            present @ Presence::Present(_) => return present,
            Presence::Absent | Presence::Elsewhere { .. } => {}
        }
    }
    if let Some(reason) = unreadable {
        return Presence::Unreadable(reason);
    }
    match holder_outside(repo, &mut index, range.head) {
        Ok(Some((holder, commit))) => Presence::Elsewhere { holder, commit },
        Ok(None) => Presence::Absent,
        Err(reason) => Presence::Unreadable(reason),
    }
}

/// The versions of one task's record, found by parsed identity (`id` or
/// `uid`) in the task directories of every layout, read once per distinct
/// `project-management` tree and once per record file.
struct RecordIndex<'r> {
    repo: &'r Repository,
    id: String,
    uid: Option<String>,
    trees: std::collections::HashMap<Oid, Vec<Oid>>,
    blobs: std::collections::HashMap<Oid, Option<RecordView>>,
}

impl<'r> RecordIndex<'r> {
    fn new(repo: &'r Repository, id: &str, uid: Option<String>) -> Self {
        Self {
            repo,
            id: id.to_string(),
            uid,
            trees: std::collections::HashMap::new(),
            blobs: std::collections::HashMap::new(),
        }
    }

    /// The record files at `commit` that are this task, as sorted blob ids;
    /// `None` when the commit or its tree cannot be read.
    fn versions(&mut self, commit: Oid) -> Option<Vec<Oid>> {
        let tree = self.repo.find_commit(commit).and_then(|c| c.tree()).ok()?;
        let Some(records) = tree
            .get_name("project-management")
            .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
            .map(|entry| entry.id())
        else {
            return Some(Vec::new());
        };
        if let Some(found) = self.trees.get(&records) {
            return Some(found.clone());
        }
        let records_tree = self.repo.find_tree(records).ok()?;
        let mut found = Vec::new();
        for (path, blob) in task_entries_in(self.repo, &records_tree) {
            if !self.blobs.contains_key(&blob) {
                let record = self.read(&path, blob);
                self.blobs.insert(blob, record);
            }
            if self.blobs.get(&blob).is_some_and(Option::is_some) {
                found.push(blob);
            }
        }
        found.sort();
        found.dedup();
        self.trees.insert(records, found.clone());
        Some(found)
    }

    /// The record in `blob` when it is this task. Only a file that names
    /// the id or uid literally, or holds an escape (a double-quoted YAML
    /// value can spell either one with `\x` escapes), is parsed.
    fn read(&self, path: &str, blob: Oid) -> Option<RecordView> {
        let blob = self.repo.find_blob(blob).ok()?;
        let content = String::from_utf8_lossy(blob.content());
        let mentions = content.contains(self.id.as_str())
            || self.uid.as_deref().is_some_and(|uid| content.contains(uid))
            || content.contains('\\');
        if !mentions {
            return None;
        }
        let record = RecordView::parse(RecordKind::Task, path, &content).ok()?;
        is_same_task(&record, &self.id, self.uid.as_deref()).then_some(record)
    }

    /// The task's record held in `blob`, once [`Self::versions`] found it.
    fn record(&self, blob: Oid) -> Option<&RecordView> {
        self.blobs.get(&blob).and_then(Option::as_ref)
    }
}

/// A branch or remote-tracking ref that records the task outside the
/// range ending at `head`: some commit it reaches and `head` does not adds
/// the task's record or changes it from every parent's version. A copy
/// inherited from the range's own commits (a branch stacked on this one,
/// or this branch's remote copy with unrelated commits on top) adds
/// nothing and does not count. Returns the ref's short name and the
/// commit; an unreadable history is the reason returned, so the caller
/// fails closed.
fn holder_outside(
    repo: &Repository,
    index: &mut RecordIndex<'_>,
    head: Oid,
) -> Result<Option<(String, Oid)>, String> {
    let unreadable =
        |error: git2::Error| format!("cannot read the branches of this clone: {error}");
    let mut refs = Vec::new();
    for reference in repo.references().map_err(unreadable)? {
        let reference = reference.map_err(unreadable)?;
        let Ok(name) = reference.name() else { continue };
        let tracked = name.starts_with("refs/heads/") || name.starts_with("refs/remotes/");
        if !tracked || reference.kind() == Some(git2::ReferenceType::Symbolic) {
            continue;
        }
        if let Ok(commit) = reference.peel_to_commit() {
            let short = reference.shorthand().unwrap_or(name).to_string();
            refs.push((name.to_string(), short, commit.id()));
        }
    }
    refs.sort();
    let mut walk = repo.revwalk().map_err(unreadable)?;
    walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)
        .map_err(unreadable)?;
    for (_, _, tip) in &refs {
        walk.push(*tip).map_err(unreadable)?;
    }
    walk.hide(head).map_err(unreadable)?;
    for commit in walk {
        let commit = commit.map_err(unreadable)?;
        let tree_error = || format!("cannot read the tree of {commit}");
        let versions = index.versions(commit).ok_or_else(tree_error)?;
        if versions.is_empty() {
            continue;
        }
        let parents: Vec<Oid> = repo
            .find_commit(commit)
            .map_err(unreadable)?
            .parent_ids()
            .collect();
        let mut inherited = std::collections::HashSet::new();
        for parent in parents {
            let parent_versions = index
                .versions(parent)
                .ok_or_else(|| format!("cannot read the tree of {parent}"))?;
            inherited.extend(parent_versions);
        }
        if versions.iter().all(|version| inherited.contains(version)) {
            continue;
        }
        let holder = refs
            .iter()
            .find(|(_, _, tip)| {
                *tip == commit || repo.graph_descendant_of(*tip, commit).unwrap_or(false)
            })
            .map_or_else(|| commit.to_string(), |(_, short, _)| short.clone());
        return Ok(Some((holder, commit)));
    }
    Ok(None)
}

/// The integration targets that versions of the task's record name in
/// `range`, the anchor's version included, in first-seen order from the
/// head back. A history that cannot be walked is the reason returned, so
/// the caller fails closed.
fn named_targets(
    repo: &Repository,
    index: &mut RecordIndex<'_>,
    range: Range,
) -> Result<Vec<String>, String> {
    let unwalkable = |error: git2::Error| {
        format!(
            "cannot read the history from {} to {}: {error}",
            range.anchor, range.head
        )
    };
    let mut walk = repo.revwalk().map_err(unwalkable)?;
    walk.push(range.head).map_err(unwalkable)?;
    walk.hide(range.anchor).map_err(unwalkable)?;
    let mut commits = walk.collect::<Result<Vec<_>, _>>().map_err(unwalkable)?;
    commits.push(range.anchor);
    let mut names = Vec::new();
    for commit in commits {
        let versions = index
            .versions(commit)
            .ok_or_else(|| format!("cannot read the tree of {commit}"))?;
        for blob in versions {
            let target = index
                .record(blob)
                .and_then(|record| record.integration_target.clone());
            if let Some(target) = target {
                if !names.contains(&target) {
                    names.push(target);
                }
            }
        }
    }
    Ok(names)
}

/// The default target this check reads, and whether `origin/HEAD` named it.
struct DefaultName {
    name: String,
    from_origin_head: bool,
}

/// The default target: the branch `origin/HEAD` names when this clone
/// records one, else the first of `main` and `master` that resolves
/// ([`super::work_start::default_work_target`]). Any other default branch is
/// found only through `origin/HEAD`, so its absence is the refusal returned.
fn default_target_name(repo: &Repository) -> Result<DefaultName, String> {
    let origin_head = repo
        .find_reference("refs/remotes/origin/HEAD")
        .ok()
        .and_then(|head| {
            head.symbolic_target()
                .ok()
                .flatten()
                .and_then(|target| target.strip_prefix("refs/remotes/origin/"))
                .map(str::to_string)
        });
    if let Some(name) = origin_head {
        return Ok(DefaultName {
            name,
            from_origin_head: true,
        });
    }
    repo.workdir()
        .and_then(super::work_start::default_work_target)
        .map(|name| DefaultName {
            name: name.strip_prefix("origin/").unwrap_or(&name).to_string(),
            from_origin_head: false,
        })
        .ok_or_else(|| {
            "no default target is found here: discovery reads `origin/HEAD`, then `main` and `master`, and none resolves. \
             If the default branch is `main` or `master`, fetch it (`git fetch origin main`); \
             for another default branch, record it (`git remote set-head origin --auto`)"
                .to_string()
        })
}

/// Every tip of the target `name` this clone knows: the configured
/// upstream of its local branch (on any remote), its `origin` tracking ref,
/// and the local branch, in that order. Empty when none resolves.
fn target_tips(repo: &Repository, name: &str) -> Vec<Oid> {
    let mut names = Vec::new();
    if let Ok(upstream) = repo.branch_upstream_name(&format!("refs/heads/{name}")) {
        if let Ok(upstream) = upstream.as_str() {
            names.push(upstream.to_string());
        }
    }
    let mut candidates = super::work_start::target_reference_names(name).unwrap_or_default();
    candidates.reverse();
    names.extend(candidates);
    names
        .iter()
        .filter_map(|reference| {
            repo.find_reference(reference)
                .and_then(|found| found.peel_to_commit())
                .ok()
                .map(|commit| commit.id())
        })
        .collect()
}

/// The task records in the tree at `at`, as path and blob, read only in the
/// task directories of the supported layouts (`project-management/tasks/`
/// and `project-management/epics/<EPC>/tasks/`). `None` when the tree
/// cannot be read.
fn task_entries(repo: &Repository, at: Oid) -> Option<Vec<(String, Oid)>> {
    let tree = repo.find_commit(at).and_then(|commit| commit.tree()).ok()?;
    let Some(records) = tree
        .get_name("project-management")
        .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
        .and_then(|entry| repo.find_tree(entry.id()).ok())
    else {
        return Some(Vec::new());
    };
    Some(task_entries_in(repo, &records))
}

/// The task records under one `project-management` tree, as path and blob.
fn task_entries_in(repo: &Repository, records: &git2::Tree<'_>) -> Vec<(String, Oid)> {
    let subtree = |tree: &git2::Tree<'_>, name: &str| {
        tree.get_name(name)
            .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
            .and_then(|entry| repo.find_tree(entry.id()).ok())
    };
    let mut directories = Vec::new();
    if let Some(tasks) = subtree(records, "tasks") {
        directories.push(("project-management/tasks".to_string(), tasks));
    }
    if let Some(epics) = subtree(records, "epics") {
        for epic in &epics {
            let Ok(name) = epic.name() else { continue };
            let Some(epic_tree) = subtree(&epics, name) else {
                continue;
            };
            if let Some(tasks) = subtree(&epic_tree, "tasks") {
                directories.push((format!("project-management/epics/{name}/tasks"), tasks));
            }
        }
    }
    let mut entries = Vec::new();
    for (directory, tasks) in directories {
        for entry in &tasks {
            let Ok(name) = entry.name() else { continue };
            let path = format!("{directory}/{name}");
            if super::work_start::record_kind_for_tree_path(&path) == Some(RecordKind::Task) {
                entries.push((path, entry.id()));
            }
        }
    }
    entries
}

/// Whether `record` is the task with `id` or `uid`, by parsed identity.
fn is_same_task(record: &RecordView, id: &str, uid: Option<&str>) -> bool {
    record.id == id || (uid.is_some() && record_uid(&record.content).as_deref() == uid)
}

/// Whether the tree at `at` holds a task record with `id` or `uid`, read
/// in the task directories ([`task_entries`]). Identity is the parsed `id`
/// and `uid` frontmatter values. A record that does not parse is this
/// task's only when it sits at the task's own file name, and then the
/// answer is unreadable, never absent.
fn presence_at(
    repo: &Repository,
    at: Oid,
    id: &str,
    uid: Option<&str>,
    own_file: &str,
) -> Presence {
    let Some(entries) = task_entries(repo, at) else {
        return Presence::Unreadable(format!("cannot read the tree of {at}"));
    };
    let mut unreadable = None;
    for (path, blob) in entries {
        let own = path.rsplit('/').next() == Some(own_file);
        let Ok(blob) = repo.find_blob(blob) else {
            if own {
                unreadable.get_or_insert(format!("cannot read {path} at {at}"));
            }
            continue;
        };
        let content = String::from_utf8_lossy(blob.content());
        match RecordView::parse(RecordKind::Task, &path, &content) {
            Ok(record) if is_same_task(&record, id, uid) => {
                return Presence::Present(Box::new(record));
            }
            Err(_) if own => {
                unreadable.get_or_insert(format!("{path} at {at} does not parse"));
            }
            Ok(_) | Err(_) => {}
        }
    }
    unreadable.map_or(Presence::Absent, Presence::Unreadable)
}

/// A record's `uid` as the frontmatter parser reads it, so every YAML
/// form of one value (plain, single or double quoted) is the same uid.
fn record_uid(content: &str) -> Option<String> {
    let (data, _) = crate::validate::parse_frontmatter(content.as_bytes()).ok()?;
    let uid = crate::validate::get_string_field(&data, "uid");
    let uid = uid.trim();
    (!uid.is_empty()).then(|| uid.to_string())
}

/// Why a waiver's commit is not the planning amendment for this record and
/// criterion on the target, if it is not.
fn waiver_problem(
    repo: &Repository,
    task: &RecordView,
    id: &str,
    evidence: &str,
    landing: Landing<'_>,
    target_tip: Option<Oid>,
    own_range_base: Option<Oid>,
) -> Option<String> {
    let Some(amendment) = commit_of(repo, evidence.trim()) else {
        return Some(format!(
            "names {}, which is not a commit here",
            evidence.trim()
        ));
    };
    // Only a real completion commit can be named as the head; the verb's
    // `HEAD` is the completion's parent and may itself be the amendment.
    if matches!(landing, Landing::Commit(completion) if completion == amendment) {
        return Some(
            "names the pull request head; a waiver is a planning amendment on the target".into(),
        );
    }
    let Some(tip) = target_tip else {
        return Some("cannot be checked: the target does not resolve here".into());
    };
    let own_range = !is_ancestor_or_same(repo, amendment, tip);
    if own_range {
        // Without the task's own range (a line, a release range, a
        // reopening range), the target route is the only one.
        let Some(own_range_base) = own_range_base else {
            return Some(format!(
                "names {}, which is not on the target",
                evidence.trim()
            ));
        };
        let reviewed = active_block(task).and_then(|block| commit_of(repo, &block.reviewed));
        if is_ancestor_or_same(repo, amendment, own_range_base)
            || reviewed.is_none_or(|reviewed| {
                amendment == reviewed || !is_ancestor_or_same(repo, amendment, reviewed)
            })
        {
            return Some("is not on the target or a record-only amendment strictly before the reviewed revision in this task's range".into());
        }
        let Ok(commit) = repo.find_commit(amendment) else {
            return Some("amendment cannot be read".into());
        };
        let paths = super::lifecycle::changed_paths(
            repo,
            &commit
                .parent_id(0)
                .map_or_else(|_| String::new(), |p| p.to_string()),
            Some(&amendment.to_string()),
        );
        if paths.is_err()
            || paths
                .is_ok_and(|paths| paths.is_empty() || paths.iter().any(|path| path != &task.path))
        {
            return Some("must change only this task's record".into());
        }
    }
    if !is_ancestor_or_same(repo, amendment, landing.commit()) {
        return Some(format!(
            "names {}, which the completion does not contain; rebase onto the amendment",
            evidence.trim()
        ));
    }
    let Ok(commit) = repo.find_commit(amendment) else {
        return Some(format!("names {}, which cannot be read", evidence.trim()));
    };
    let parent = commit.parent_id(0).ok();
    // What the amendment brought onto the target (for a merge, against the
    // target-side parent) changes planning records only (R-52, R-60).
    match non_planning_change(repo, parent, amendment) {
        Ok(None) => {}
        Ok(Some(path)) => {
            return Some(format!(
                "names {}, which also changes {path}; a planning amendment changes planning records only",
                evidence.trim()
            ));
        }
        Err(_) => {
            return Some(format!(
                "names {}, whose change cannot be read",
                evidence.trim()
            ));
        }
    }
    let criterion = |oid: Option<Oid>| {
        oid.and_then(|oid| blob_at(repo, oid, &task.path))
            .and_then(|content| RecordView::parse(task.kind, &task.path, &content).ok())
            .map(|record| {
                record
                    .criteria
                    .items
                    .into_iter()
                    .find(|item| item.id == id)
                    .map(|item| item.text)
            })
    };
    match (criterion(parent), criterion(Some(amendment))) {
        (Some(before), Some(after)) if before != after => None,
        _ => Some(format!(
            "names {}, which does not amend {id} of {}",
            evidence.trim(),
            task.id
        )),
    }
}

/// The first path the change from `parent` (none for a root commit) to
/// `commit` touches outside the planning records; an error when a tree or
/// the diff cannot be read.
pub(super) fn non_planning_change(
    repo: &Repository,
    parent: Option<Oid>,
    commit: Oid,
) -> Result<Option<String>, git2::Error> {
    let tree = |oid: Oid| repo.find_commit(oid).and_then(|commit| commit.tree());
    let after = tree(commit)?;
    let before = parent.map(tree).transpose()?;
    let diff = repo.diff_tree_to_tree(before.as_ref(), Some(&after), None)?;
    let outside = diff
        .deltas()
        .flat_map(|delta| [delta.old_file().path(), delta.new_file().path()])
        .flatten()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .find(|path| !is_planning_path(path));
    Ok(outside)
}

/// The epic journey criterion a task serves, when it has no journey of its
/// own.
fn served_journey<'g>(task: &RecordView, graph: &'g Graph) -> Option<(&'g str, &'g Criterion)> {
    if task.criteria.items.iter().any(Criterion::is_journey) {
        return None;
    }
    task.criteria.items.iter().find_map(|item| {
        let (epic_id, criterion_id) = item.serves()?;
        let epic = graph
            .records
            .get(&epic_id)
            .filter(|record| record.kind == RecordKind::Epic)?;
        epic.criteria
            .items
            .iter()
            .find(|criterion| criterion.id == criterion_id && criterion.is_journey())
            .map(|criterion| (epic.id.as_str(), criterion))
    })
}

/// A leaf serving the epic's journey links the evidence that ran
/// (`verified | <link>`) or states the narrower path it verified
/// (`narrower | <path>`); it never leaves the journey unsaid (R-53).
fn leaf_journey(task: &RecordView, graph: &Graph, block: &AcceptanceBlock) -> Option<Finding> {
    let (epic, criterion) = served_journey(task, graph)?;
    let detail = block
        .journey
        .split_once('|')
        .map_or("", |(_, detail)| detail.trim());
    let said =
        matches!(outcome_word(&block.journey), "verified" | "narrower") && !detail.is_empty();
    (!said).then(|| {
        finding(
            JOURNEY_RULE,
            format!(
                "{} serves {epic} {} (a journey); its `journey` links the evidence that ran (`verified | <link>`) or states the narrower path (`narrower | <path>`)",
                task.id, criterion.id
            ),
        )
    })
}

/// Why `task_id` may not carry an adopter-facing range: it has no journey
/// criterion and serves no epic journey criterion (R-53).
#[must_use]
pub fn journey_requirement(graph: &Graph, task_id: &str) -> Option<String> {
    let task = graph
        .records
        .get(task_id)
        .filter(|record| record.kind == RecordKind::Task)?;
    let own = task.criteria.items.iter().any(Criterion::is_journey);
    (!own && served_journey(task, graph).is_none()).then(|| {
        let inner: Vec<&str> = task
            .criteria
            .items
            .iter()
            .filter(|criterion| criterion.has_inner_journey())
            .map(|criterion| criterion.id.as_str())
            .collect();
        let note = if inner.is_empty() {
            String::new()
        } else {
            format!(
                "; {} {} `(journey)` inside its text, and the tag counts only where it opens or closes the criterion (R-50)",
                inner.join(", "),
                if inner.len() == 1 { "carries" } else { "carry" }
            )
        };
        format!(
            "{task_id} changes the adopter-facing path set but has no `(journey)` criterion and serves no epic journey criterion{note}"
        )
    })
}

/// Records of the range whose criteria differ from the target's (R-52).
#[must_use]
pub fn frozen_criteria(
    head: &Graph,
    target: &Graph,
    changed_paths: &[String],
    exempt: Option<&str>,
) -> Vec<Finding> {
    let mut found = Vec::new();
    for record in head
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task && changed_paths.contains(&record.path))
    {
        let before = target.records.get(&record.id);
        let old = before
            .map(|r| r.criteria.items.as_slice())
            .unwrap_or_default();
        let new = &record.criteria.items;
        if before.is_some_and(|r| r.criteria.signature() == record.criteria.signature()) {
            continue;
        }
        if exempt == Some(record.id.as_str()) {
            let mut delta = Vec::new();
            for item in old {
                match new.iter().find(|next| next.id == item.id) {
                    None => delta.push(format!(
                        "{} removed: {} (give the reason in the PR body)",
                        item.id, item.text
                    )),
                    Some(next) if next.text != item.text => delta.push(format!(
                        "{} changed: {} -> {}",
                        item.id, item.text, next.text
                    )),
                    _ => {}
                }
            }
            for item in new
                .iter()
                .filter(|item| !old.iter().any(|prior| prior.id == item.id))
            {
                delta.push(format!("{} added: {}", item.id, item.text));
            }
            if !delta.is_empty() {
                found.push(Finding {
                    epic_record: None,
                    rule: FROZEN_RULE,
                    message: format!("{} criteria delta: {}", record.id, delta.join("; ")),
                    note: true,
                });
            }
        } else if before.is_some() {
            found.push(finding(FROZEN_RULE, format!("{} changes its criteria on this branch; another task's criteria change by its own PR or the epic amendment", record.id)));
        }
    }
    found
}

/// Enforce reopen equality before any range-class amendment exemption.
/// The prospective graph may include an uncommitted status transition.
///
/// # Errors
/// Returns an error if the range cannot be read.
pub fn reopened_criteria(
    repo: &Repository,
    base: &str,
    head: &str,
    after: &Graph,
) -> Result<Vec<Finding>, String> {
    Ok(reopened_ids(repo, base, head, after)?
        .into_iter()
        .map(|id| finding(FROZEN_RULE, reopened_message(&id)))
        .collect())
}

/// Why a reopened task's changed criteria are refused: one statement for
/// the reopen transition (R-119) and the range freeze (R-52).
pub(super) const REOPENED_CRITERIA: &str = "a reopened task keeps its criteria as the anchored target has them; change them in the epic's batched amendment, a planning pull request on the target";

/// The frozen finding's message for a reopened task whose criteria changed.
pub(super) fn reopened_message(id: &str) -> String {
    format!("{id}: {REOPENED_CRITERIA}")
}

/// The tasks complete at `base` whose criteria differ at `head` and which
/// the range reopened: no longer complete, a block superseded, or a commit
/// of the range moving them from complete.
///
/// # Errors
/// Returns an error if the range cannot be read.
pub(super) fn reopened_ids(
    repo: &Repository,
    base: &str,
    head: &str,
    after: &Graph,
) -> Result<std::collections::BTreeSet<String>, String> {
    let target = Graph::from_revision(repo, base)?;
    let candidates: Vec<_> = after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task)
        .filter(|record| {
            target.records.get(&record.id).is_some_and(|old| {
                old.status == "complete" && old.criteria.signature() != record.criteria.signature()
            })
        })
        .collect();
    if candidates.is_empty() {
        return Ok(std::collections::BTreeSet::new());
    }
    let oid = |rev: &str| {
        repo.revparse_single(rev)
            .and_then(|o| o.peel_to_commit())
            .map(|c| c.id())
            .map_err(|e| e.to_string())
    };
    let mut walk = repo.revwalk().map_err(|e| e.to_string())?;
    walk.push(oid(head)?).map_err(|e| e.to_string())?;
    walk.hide(oid(base)?).map_err(|e| e.to_string())?;
    let mut reopened = std::collections::BTreeSet::new();
    for record in &candidates {
        let old = &target.records[&record.id];
        if record.status != "complete"
            || acceptance_blocks(&record.body)
                .iter()
                .filter(|block| block.is_superseded())
                .count()
                > acceptance_blocks(&old.body)
                    .iter()
                    .filter(|block| block.is_superseded())
                    .count()
        {
            reopened.insert(record.id.clone());
        }
    }
    for oid in walk {
        let oid = oid.map_err(|e| e.to_string())?;
        for record in &candidates {
            if let Some(content) = blob_at(repo, oid, &record.path) {
                let then = RecordView::parse(RecordKind::Task, &record.path, &content)?;
                if then.status != "complete" {
                    let commit = repo.find_commit(oid).map_err(|e| e.to_string())?;
                    let transitioned = commit.parent_ids().any(|parent| {
                        blob_at(repo, parent, &record.path)
                            .and_then(|text| {
                                RecordView::parse(RecordKind::Task, &record.path, &text).ok()
                            })
                            .is_some_and(|prior| prior.status == "complete")
                    });
                    if transitioned {
                        reopened.insert(record.id.clone());
                    }
                }
            }
        }
    }
    Ok(reopened)
}

/// The binding findings of a range: every task record it completes, or
/// whose active block or archived review it changes, with provenance read
/// before applying the shared binding rule.
/// `default_target` stands in for a task that declares no integration
/// target when its waivers are judged.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
pub fn completions_in_range(
    repo: &Repository,
    base: &str,
    head: &str,
    default_target: Option<Oid>,
    criteria: &Criteria,
) -> Result<Vec<Finding>, String> {
    let amendable = *criteria == Criteria::Amendable;
    // The task a task pull request is for may waive a criterion by a
    // record-only amendment in its own range (TSK-184); no other record may.
    let own_task = match criteria {
        Criteria::OwnTask(id) => Some(id.as_str()),
        _ => None,
    };
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let head_oid = oid(head)?;
    let anchor = repo
        .merge_base(oid(base)?, head_oid)
        .map_err(|error| error.message().to_string())?;
    let before = Graph::from_revision(repo, &anchor.to_string())?;
    let after = Graph::from_revision(repo, &head_oid.to_string())?;
    let reopened_in_history =
        super::lifecycle::reopened_in_range(repo, &anchor.to_string(), Some(head), &after);
    let mut findings = Vec::new();
    for task in after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task && record.status == "complete")
    {
        let block = active_block(task);
        let reopened = super::lifecycle::is_recompletion(before.records.get(&task.id), task)
            || reopened_in_history.contains(&task.id);
        let unchanged = before
            .records
            .get(&task.id)
            .is_some_and(|then| then.status == "complete" && active_block(then) == block);
        if !unchanged || reopened {
            // A task range owns all its changes. A planning/line range
            // carries each completion from its introduction on that line.
            // Recompletion owns the entire fix range even if its block is
            // byte-identical to an earlier review.
            let introduced = block
                .as_ref()
                .map_or(head_oid, |block| introduced_at(repo, task, block, head_oid));
            let source_base = amendable
                .then(|| source_landing_base(repo, head_oid, introduced))
                .flatten();
            let origin = if amendable && (!reopened || source_base.is_some()) {
                introduced
            } else {
                head_oid
            };
            if amendable {
                findings.push(Finding {
                    epic_record: None,
                    rule: BINDING_RULE,
                    message: format!("{} completion bound at {origin}", task.id),
                    note: true,
                });
            }
            findings.extend(bind_completion_with_amendment(
                repo,
                task,
                &after,
                Landing::Commit(origin),
                default_target,
                Transport::TaskLanding,
                Some(source_base.unwrap_or(anchor)),
                (own_task == Some(task.id.as_str())).then_some(anchor),
            ));
        }
    }
    findings.extend(epic_completions_in_range(
        repo, &before, &after, head_oid, amendable,
    ));
    Ok(findings)
}

/// The anchored base of the task landing that carried `introduced` onto
/// the source's first-parent chain. The caller supplies an ordinary line or
/// an already verified import source; this never qualifies a release import.
pub(super) fn source_landing_base(repo: &Repository, source: Oid, introduced: Oid) -> Option<Oid> {
    let mut at = source;
    while at != introduced {
        let commit = repo.find_commit(at).ok()?;
        let first = commit.parent_id(0).ok()?;
        if !is_ancestor_or_same(repo, introduced, first) {
            return (commit.parent_count() == 2
                && is_ancestor_or_same(repo, introduced, commit.parent_id(1).ok()?))
            .then(|| repo.merge_base(first, introduced).ok())
            .flatten();
        }
        at = first;
    }
    None
}

/// The commit of the range that introduced `task`'s completion with
/// `block`: walk from `head` while a parent still holds it, the first
/// parent before the second. It ends at the first commit whose parents do
/// not hold it; the range's merge-base never does, or the range would not
/// bind this completion.
pub(super) fn introduced_at(
    repo: &Repository,
    task: &RecordView,
    block: &AcceptanceBlock,
    head: Oid,
) -> Oid {
    let holds = |oid: Oid| {
        blob_at(repo, oid, &task.path)
            .and_then(|content| RecordView::parse(task.kind, &task.path, &content).ok())
            .is_some_and(|record| {
                record.status == "complete"
                    && active_block(&record).as_ref() == Some(block)
                    && record.superseded_blocks() == task.superseded_blocks()
            })
    };
    let mut at = head;
    while let Some(parent) = repo
        .find_commit(at)
        .ok()
        .and_then(|commit| commit.parent_ids().take(2).find(|parent| holds(*parent)))
    {
        at = parent;
    }
    at
}

/// [`journey_requirement`] for the task as the `head` revision has it.
///
/// # Errors
///
/// Returns a message when the repository or the revision cannot be read.
pub fn journey_requirement_at(
    repo_root: &std::path::Path,
    head: &str,
    task_id: &str,
) -> Result<Option<String>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let graph = Graph::from_revision(&repo, head)?;
    Ok(journey_requirement(&graph, task_id))
}

/// Whether a range may change task criteria (R-52): a planning-only change
/// or a validated epic integration line. The caller decides it from the
/// pull request's validated class, never from the branch prefix alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Criteria {
    /// Criteria stay as the target has them.
    Frozen,
    /// Only the named, not previously complete task may amend its criteria.
    OwnTask(String),
    /// A planning-only range or a validated epic line may change them.
    Amendable,
}

/// Select the own-task amendment only for a task not complete at the target.
///
/// # Errors
/// Returns an error when the target records cannot be read.
pub fn task_criteria(
    root: &std::path::Path,
    base: &str,
    task_id: &str,
) -> Result<Criteria, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let graph = Graph::from_revision(&repo, base)?;
    Ok(
        if graph
            .records
            .get(task_id)
            .is_some_and(|record| record.status == "complete")
        {
            Criteria::Frozen
        } else {
            Criteria::OwnTask(task_id.to_string())
        },
    )
}

/// The findings of a pull request from `base` (the target tip) to `head`:
/// criteria frozen unless `criteria` is [`Criteria::Amendable`], and every
/// completion in the range bound. A frozen range is a task pull request and
/// owns its changes; an amendable one carries completions from their source
/// on the line. Both apply the same binding rule.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
pub fn pull_request_findings(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    criteria: &Criteria,
) -> Result<Vec<Finding>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let target_tip = oid(base)?;
    let at_head = Graph::from_revision(&repo, head)?;
    // Resolution 43: a reopened task keeps its criteria, whatever the
    // range's class allows.
    let mut found = reopened_criteria(&repo, base, head, &at_head)?;
    if *criteria != Criteria::Amendable {
        let anchor = repo
            .merge_base(target_tip, oid(head)?)
            .map_err(|error| error.message().to_string())?;
        let paths = super::lifecycle::changed_paths(&repo, &anchor.to_string(), Some(head))?;
        let at_target = Graph::from_revision(&repo, base)?;
        let exempt = match criteria {
            Criteria::OwnTask(id) => Some(id.as_str()),
            _ => None,
        };
        found.extend(frozen_criteria(&at_head, &at_target, &paths, exempt));
    }
    found.extend(completions_in_range(
        &repo,
        base,
        head,
        Some(target_tip),
        criteria,
    )?);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_status_line_and_the_closeout_are_outside_review() {
        let base = "---\nid: TSK-001\nstatus: todo  # x\n---\n\n# TSK-001\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n";
        let done = base
            .replace("status: todo  # x", "status: complete  # x")
            .replace("Pending.", "```yaml\nacceptance:\n## not a heading\n```\n");
        assert_eq!(reviewed_part(base), reviewed_part(&done));
        let criteria = base.replace("shall work", "shall work well");
        assert_ne!(reviewed_part(base), reviewed_part(&criteria));
        let title = base.replace("id: TSK-001", "id: TSK-001\ntitle: other");
        assert_ne!(reviewed_part(base), reviewed_part(&title));
        let after_closeout = format!("{base}\n## Notes\n\nNew.\n");
        assert_ne!(reviewed_part(base), reviewed_part(&after_closeout));
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let out = crate::git::command()
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A task is found by its id or uid in any supported layout; a task
    /// path that names it but does not parse is unreadable, never absent.
    #[test]
    fn presence_finds_a_task_by_identity_in_any_layout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let uid = "6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d";
        let record = |id: &str| {
            format!(
                "---\nid: {id}\nuid: \"{uid}\"  # written once\nstatus: todo\n---\n\n# {id}: work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n"
            )
        };
        // Every YAML form of one value is the same uid.
        for form in [uid.to_string(), format!("'{uid}'"), format!("\"{uid}\"")] {
            let text = format!("---\nid: TSK-001\nuid: {form}\nstatus: todo\n---\n");
            assert_eq!(record_uid(&text).as_deref(), Some(uid), "{form}");
        }
        let commit = |path: &str, text: &str| {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
            git(root, &["add", "-A"]);
            git(root, &["commit", "-qm", "record"]);
            Repository::open(root)
                .unwrap()
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
        };
        let legacy = commit(
            "project-management/epics/EPC-001/tasks/TSK-001.md",
            &record("TSK-001"),
        );
        let repo = Repository::open(root).unwrap();
        let at = |commit, id, uid, own| presence_at(&repo, commit, id, uid, own);
        assert!(matches!(
            at(legacy, "TSK-001", None, "TSK-001.md"),
            Presence::Present(_)
        ));
        // The same uid under another number is the same record.
        assert!(matches!(
            at(legacy, "TSK-009", Some(uid), "TSK-009.md"),
            Presence::Present(_)
        ));
        assert!(matches!(
            at(legacy, "TSK-002", None, "TSK-002.md"),
            Presence::Absent
        ));
        // A broken record that only mentions the task is not the task; one
        // at the task's own file name is unreadable.
        let mentions = commit(
            "project-management/tasks/TSK-099.md",
            "---\nid: TSK-099\nstatus: [unclosed\n---\nSee TSK-002.\n",
        );
        assert!(matches!(
            at(mentions, "TSK-002", None, "TSK-002.md"),
            Presence::Absent
        ));
        let broken = commit(
            "project-management/tasks/TSK-002.md",
            "---\nid: TSK-002\nstatus: [unclosed\n---\n",
        );
        assert!(matches!(
            at(broken, "TSK-002", None, "TSK-002.md"),
            Presence::Unreadable(_)
        ));
    }

    /// The record index finds a task whose id and uid are spelled with YAML
    /// escapes, so no spelling hides a version from the history walks.
    #[test]
    fn the_record_index_reads_an_escaped_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        let path = root.join("project-management/tasks/TSK-003.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "---\nid: \"\\x54SK-003\"\nuid: \"\\x36f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d\"\nstatus: todo\nintegration_target: main\n---\n\n# Work\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n",
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "record"]);
        let repo = Repository::open(root).unwrap();
        let head = repo.head().unwrap().peel_to_commit().unwrap().id();
        for (id, uid) in [
            ("TSK-003", None),
            (
                "TSK-777",
                Some("6f1c2b8e-3d4a-4f5b-9c6d-7e8f9a0b1c2d".to_string()),
            ),
        ] {
            let mut index = RecordIndex::new(&repo, id, uid);
            let versions = index.versions(head).unwrap();
            assert_eq!(versions.len(), 1, "{id}");
            assert_eq!(
                index
                    .record(versions[0])
                    .and_then(|r| r.integration_target.clone()),
                Some("main".to_string())
            );
        }
    }

    /// The merge rule takes a landing merge only when its tree is the clean
    /// re-merge of its parents: a plain merge is; a merge that adds a file
    /// of its own (an evil merge) or resolves a conflict is not.
    #[test]
    fn a_landing_merge_is_its_clean_re_merge() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |path: &str, text: &str| std::fs::write(root.join(path), text).unwrap();
        git(root, &["init", "-q", "-b", "line"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
        write("a.txt", "a\n");
        write("b.txt", "b\n");
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "base"]);
        git(root, &["switch", "-qc", "task"]);
        write("a.txt", "task\n");
        git(root, &["commit", "-qam", "task"]);
        git(root, &["switch", "-q", "line"]);
        write("b.txt", "line\n");
        git(root, &["commit", "-qam", "line"]);
        let repo = Repository::open(root).unwrap();
        let clean = || {
            let tip = repo.head().unwrap().peel_to_commit().unwrap();
            is_clean_remerge(&repo, &tip).unwrap()
        };

        git(root, &["merge", "-q", "--no-ff", "-m", "clean", "task"]);
        assert!(clean(), "a plain merge");

        git(root, &["reset", "-q", "--hard", "HEAD^"]);
        git(root, &["merge", "-q", "--no-ff", "--no-commit", "task"]);
        write("evil.txt", "evil\n");
        git(root, &["add", "evil.txt"]);
        git(root, &["commit", "-qm", "evil"]);
        assert!(!clean(), "an evil merge");

        git(root, &["reset", "-q", "--hard", "HEAD^"]);
        write("a.txt", "line too\n");
        git(root, &["commit", "-qam", "conflicting"]);
        let merged = crate::git::command()
            .args(["merge", "-q", "--no-ff", "task"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(!merged.status.success(), "the merge conflicts");
        write("a.txt", "resolved\n");
        git(root, &["commit", "-qam", "resolved"]);
        assert!(!clean(), "a resolved conflict");
    }

    /// A `## Closeout` line inside a comment or a fence is not the Closeout,
    /// so the text after it stays under review; a `status:` line outside the
    /// frontmatter is reviewed text.
    #[test]
    fn only_the_parsed_closeout_is_outside_review() {
        let base = "---\nid: TSK-001\nstatus: todo\n---\n\n# TSK-001\n\n## Description\n\n{hidden}\n\nVisible scope.\n\n## Acceptance Criteria\n\n- AC-1 When run, the system shall work.\n\n## Closeout\n\nPending.\n";
        for hidden in [
            "<!--\n## Closeout\n-->",
            "````text\n```\n## Closeout\n```\n````",
            "~~~\n## Closeout\n~~~",
        ] {
            let before = base.replace("{hidden}", hidden);
            let narrowed = before.replace("Visible scope.", "Narrower scope.");
            assert_ne!(reviewed_part(&before), reviewed_part(&narrowed), "{hidden}");
            let done = before
                .replace("status: todo", "status: complete")
                .replace("Pending.", "Done.");
            assert_eq!(reviewed_part(&before), reviewed_part(&done), "{hidden}");
        }
        let body_status = base
            .replace("{hidden}", "status: todo")
            .replace("status: todo\n\nVisible", "status: done\n\nVisible");
        assert_ne!(
            reviewed_part(&base.replace("{hidden}", "status: todo")),
            reviewed_part(&body_status)
        );
    }
}
