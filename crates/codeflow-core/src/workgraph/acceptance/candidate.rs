//! A landing candidate judged member by member (TSK-1154, issue #488 and
//! TSK-1277's c2 shape).
//!
//! A landing candidate is a trusted tip plus no-ff merges of reviewed heads.
//! Judged as one pull request, every completion binds at the candidate head
//! and every own criteria change is frozen, so a clean batch is refused.
//! Here each commit on the head's first-parent chain from the tip is judged
//! on its own:
//!
//! - a two-parent merge must equal the clean remerge of its parents, and
//!   its second parent (the member head) is judged exactly as that member's
//!   own pull request into the receiving target is judged, with the merge's
//!   first parent (the tip it merged onto) as the base;
//! - a merge whose second parent is already on the published default
//!   target (a sync of the default target into the line) is not a member:
//!   it is judged as a range of its own with frozen criteria, as the whole
//!   candidate was judged before, so its records count only as proven
//!   imports from their home;
//! - any other commit is judged as a range of its own with frozen criteria.
//!
//! The member's task comes only from the records at the exact member head:
//! the one task whose completion that head adds over the tip it merged
//! onto. A branch name, a merge message or a flag supplies nothing. Its own
//! criteria change is judged as its own pull request's is only when the
//! caller's binding holds for that task at that head (for an epic line: the
//! task declares the line at the line's tip and at the head, and the line's
//! epic is bound there), and every changed criterion carries a dated
//! amendment note. The receiving target, its trusted tip and the binding
//! come from the caller, so another kind of tip (such as the default
//! target's) can use the same member judgment with its own anchor.
//!
//! A batch into the default target ([`Mode::MainBatch`], TSK-1154 AC-16)
//! is the same walk from the default target's fetched tip, with three
//! differences: a commit that is not a two-parent merge refuses; no merge
//! is read as a sync; and no existing criterion may change, the bound
//! task's own included. Records are matched across a member's two parents
//! by id and uid, never by path, so a record deleted and added back,
//! renamed or split is still judged against its first-parent criteria.
//! Each member binds as a single-task pull request into that target does:
//! the bound task's acceptance block names a reviewed commit that the
//! member head is, or holds with only default-target merges after it, with
//! the verdict at that head. A member that changes only what a planning
//! amendment may and adds no completion binds no task.

use super::{
    active_block, default_target_name, finding, history, imports::home_tip, is_ancestor_or_same,
    pull_request_findings_into, read_merge, task_criteria, Criteria, Finding, Graph, MergeReading,
    Oid, OwnLine, RecordKind, RecordView, Repository, Scope, BINDING_RULE, FROZEN_RULE,
};

/// How a candidate's members bind and what they may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A landing candidate of an epic line: a member's own criteria change
    /// is judged as its pull request's when the binding holds, and a merge
    /// of a commit already on the published default target is a sync.
    Line,
    /// A batch into the default target (TSK-1154 AC-16): only no-ff member
    /// merges, no sync reading, and no existing criterion may change.
    MainBatch,
}

/// What a candidate is judged against: its trusted tip, resolved once by
/// the caller and never taken from the head; the branch the members land
/// on; and the run's candidate authority (the pre-push hook's default
/// target tip), which stays the policy and history authority.
#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    /// The receiving target's trusted tip, as a full commit id.
    pub tip: &'a str,
    /// The receiving target's branch name (an epic line).
    pub target: &'a str,
    /// The run's candidate authority, when it names one.
    pub authority: Option<&'a str>,
    /// The judged policy's work branch prefixes.
    pub branch_prefixes: Option<&'a [String]>,
    /// How members bind and what they may change.
    pub mode: Mode,
}

/// One member of a candidate: the merge, the tip it merged onto (its first
/// parent) and the member head (its second parent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    /// The member merge on the candidate's first-parent chain.
    pub merge: Oid,
    /// The tip it merged onto, its first parent.
    pub onto: Oid,
    /// The member head, its second parent.
    pub head: Oid,
}

/// What a member's binding gives its judgment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// The member's task may take its own pull request's criteria rule
    /// (an epic line only; a batch into the default target never does).
    Own(String),
    /// Criteria stay as the tip holds them. `task` is the member's task,
    /// whose range the member is, when one binds; `note` says why the own
    /// rule does not apply, when it is worth saying.
    Frozen {
        task: Option<String>,
        note: Option<String>,
    },
}

/// Judge `head` as `candidate`'s next tip, member by member. `bind` is asked
/// once per member merge and answers what the member binds, or
/// `Err(reason)` to refuse the member, naming it.
///
/// # Errors
///
/// Returns a message when a commit, a parent or a record cannot be read,
/// the history is shallow, or the head's first-parent chain does not reach
/// the tip: each refuses, never drops out.
pub fn candidate_findings(
    root: &std::path::Path,
    candidate: &Candidate<'_>,
    head: &str,
    bind: &mut dyn FnMut(Member) -> Result<Bound, String>,
) -> Result<Vec<Finding>, String> {
    let _judgment = history::judgment();
    let repo = Repository::discover(root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let tip = oid(candidate.tip)?;
    let head = oid(head)?;
    let mut found = Vec::new();
    let line = candidate.mode == Mode::Line;
    let default_tip = if line {
        published_default_tip(&repo)?
    } else {
        None
    };
    for (commit, parents) in first_parent_chain(&repo, tip, head)? {
        match parents.as_slice() {
            [onto, member] if line && is_sync(&repo, default_tip, *member)? => {
                let named = format!(
                    "sync {member:.9} (already on the published default target), merged by {commit:.9} onto {onto:.9}"
                );
                found.push(Finding {
                    epic_record: None,
                    rule: FROZEN_RULE,
                    message: format!("{named}: judged as a range of its own with frozen criteria; its records count only as imports proven from their home"),
                    note: true,
                });
                found.extend(range_findings(root, candidate, &named, commit, *onto)?);
            }
            [onto, member] => found.extend(member_findings(
                root,
                &repo,
                candidate,
                Member {
                    merge: commit,
                    onto: *onto,
                    head: *member,
                },
                bind,
            )?),
            [_] if !line => found.push(finding(
                BINDING_RULE,
                format!(
                    "candidate commit {commit:.9} is not a merge of a reviewed head; a batch into '{}' holds only two-parent no-ff merges of reviewed heads, so rebuild it from the tip {tip:.9}",
                    candidate.target
                ),
            )),
            [onto] => {
                let named = format!("candidate commit {commit:.9} (not a merge of a reviewed head)");
                found.extend(range_findings(root, candidate, &named, commit, *onto)?);
            }
            _ => found.push(finding(
                BINDING_RULE,
                format!(
                    "candidate commit {commit:.9} has {} parents; a landing candidate holds only two-parent no-ff merges of reviewed heads, so rebuild it from the tip {tip:.9}",
                    parents.len()
                ),
            )),
        }
    }
    Ok(found)
}

/// The published default target's tip, read once from its fetch remote;
/// `None` when no default target is published, so nothing is a sync.
fn published_default_tip(repo: &Repository) -> Result<Option<Oid>, String> {
    match default_target_name(repo) {
        Ok(default) => home_tip(repo, &default.name),
        Err(_) => Ok(None),
    }
}

/// Whether `member` is already on the published default target's tip, so
/// its merge is a sync of that target, not a member; with no published
/// default target, each merge is judged as a member.
fn is_sync(repo: &Repository, default_tip: Option<Oid>, member: Oid) -> Result<bool, String> {
    match default_tip {
        Some(tip) => is_ancestor_or_same(repo, member, tip),
        None => Ok(false),
    }
}

/// Each commit on `head`'s first-parent chain down to `tip`, oldest first,
/// with its parents.
fn first_parent_chain(
    repo: &Repository,
    tip: Oid,
    head: Oid,
) -> Result<Vec<(Oid, Vec<Oid>)>, String> {
    let mut chain = Vec::new();
    let mut at = head;
    while at != tip {
        let commit = repo
            .find_commit(at)
            .map_err(|error| format!("cannot read candidate commit {at}: {}", error.message()))?;
        let parents = history::parents(repo, &commit)
            .map_err(|error| format!("cannot read the parents of {at}: {}", error.message()))?;
        let Some(first) = parents.first().copied() else {
            return Err(format!(
                "the head's first-parent history does not reach the tip {tip:.9}, so it is not a landing candidate built on it"
            ));
        };
        chain.push((at, parents));
        at = first;
    }
    chain.reverse();
    Ok(chain)
}

/// Each task whose completion `member.head` adds over `member.onto`:
/// complete at the head, with an active block the tip does not hold.
fn completions(repo: &Repository, member: Member) -> Result<Vec<String>, String> {
    let at_head = Graph::from_revision(repo, &member.head.to_string())?;
    let at_onto = Graph::from_revision(repo, &member.onto.to_string())?;
    Ok(at_head
        .records
        .values()
        .filter(|record| {
            record.kind == RecordKind::Task
                && record.status == "complete"
                && active_block(record).is_some()
                && at_onto.records.get(&record.id).is_none_or(|before| {
                    before.status != "complete" || active_block(before) != active_block(record)
                })
        })
        .map(|record| record.id.clone())
        .collect())
}

/// The one task whose completion the member head adds over the tip it
/// merged onto, read from the records at those exact commits; `None` when
/// no task or more than one does.
///
/// # Errors
///
/// Returns a message when the repository or a record cannot be read.
pub fn completed_task(root: &std::path::Path, member: Member) -> Result<Option<String>, String> {
    let repo = Repository::discover(root).map_err(|error| error.message().to_string())?;
    Ok(match completions(&repo, member)?.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    })
}

/// The filing members of a batch into the default target `default` from
/// its fetched tip `tip` (TSK-1154 AC-16), each as its own change: the
/// merge, the merge base of its two parents as `onto`, and its head. The
/// caller runs on each the checks a one-pull-request filing into that
/// target gets, over that three-dot range. A member [`main_binding`]
/// refuses is not listed; its refusal is the acceptance check's.
///
/// # Errors
///
/// Returns a message when the history or a commit cannot be read.
pub fn filings(
    root: &std::path::Path,
    tip: &str,
    head: &str,
    default: &str,
) -> Result<Vec<Member>, String> {
    let _judgment = history::judgment();
    let repo = Repository::discover(root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let mut found = Vec::new();
    for (merge, parents) in first_parent_chain(&repo, oid(tip)?, oid(head)?)? {
        let [onto, member] = parents.as_slice() else {
            continue;
        };
        let member = Member {
            merge,
            onto: *onto,
            head: *member,
        };
        if matches!(main_binding(root, tip, member, default), Ok(None)) {
            found.push(own_change(&repo, member)?);
        }
    }
    Ok(found)
}

/// The member's own change, read from its two parents: `member` with
/// `onto` replaced by the merge base of the tip it merged onto and its
/// head. A later member merges onto a tip holding the members before it,
/// which its head need not hold; the clean remerge (checked per member)
/// makes the merge bring exactly this change onto that tip.
fn own_change(repo: &Repository, member: Member) -> Result<Member, String> {
    let base = history::merge_base(repo, member.onto, member.head).map_err(|error| {
        format!(
            "cannot read the merge base of {:.9} and {:.9}: {}",
            member.onto,
            member.head,
            error.message()
        )
    })?;
    Ok(Member {
        onto: base,
        ..member
    })
}

/// Whether a line member is a planning amendment (ADR-0078): its own
/// change adds no task completion and carries only what a planning
/// amendment may, judged against the target's policy at its merge base.
fn planning_member(repo: &Repository, member: Member) -> Result<bool, String> {
    let own = own_change(repo, member)?;
    if !completions(repo, own)?.is_empty() {
        return Ok(false);
    }
    let paths = super::super::lifecycle::changed_paths(
        repo,
        &own.onto.to_string(),
        Some(&own.head.to_string()),
    )?;
    Ok(super::super::amendment::range_problem(repo, own.onto, own.head, &paths).is_none())
}

/// What a member of a batch into the default target `default` binds
/// (TSK-1154 AC-16), read only from the trees at the member's two parents
/// and the batch's `anchor`: `Ok(None)` for a filing, a member that adds
/// no completion and only adds new record files (`filing_problem`),
/// admitted on the terms of a filing pull request into that target;
/// `Ok(Some(task))` for a task member, whose one changed task record
/// satisfies the binding to `default` ([`task_main_binding`]) and is the
/// one task whose completion it adds. Anything else is `Err(reason)`,
/// which refuses the member: any other member without a completion, no
/// such record or several, a completion of another task, or a record that
/// cannot be read.
///
/// [`task_main_binding`]: crate::workgraph::work_start::task_main_binding
///
/// # Errors
///
/// Returns the reason the member is refused.
pub fn main_binding(
    root: &std::path::Path,
    anchor: &str,
    member: Member,
    default: &str,
) -> Result<Option<String>, String> {
    let repo = Repository::discover(root).map_err(|error| error.message().to_string())?;
    let member = own_change(&repo, member)?;
    let (onto, head) = (member.onto.to_string(), member.head.to_string());
    let completed = completions(&repo, member)?;
    if completed.is_empty() {
        let paths = super::super::lifecycle::changed_paths(&repo, &onto, Some(&head))?;
        let problem = match filing_problem(&repo, member)? {
            Some(problem) => Some(problem),
            None => completion_shown(&repo, member)?.or_else(|| {
                super::super::amendment::range_problem(&repo, member.onto, member.head, &paths)
            }),
        };
        return match problem {
            None => Ok(None),
            Some(problem) => Err(format!(
                "it adds no task completion, and a member without one is admitted only as a filing that adds new record files and changes nothing else: {problem}"
            )),
        };
    }
    let before = Graph::from_revision(&repo, &onto)?;
    let after = Graph::from_revision(&repo, &head)?;
    let mut bound = Vec::new();
    let mut unbound = Vec::new();
    for record in after.records.values().filter(|record| {
        record.kind == RecordKind::Task
            && before
                .records
                .get(&record.id)
                .is_none_or(|then| then.content != record.content)
    }) {
        match crate::workgraph::work_start::task_main_binding(
            root, &record.id, &head, anchor, default,
        )
        .map_err(|error| format!("{}'s record cannot be read: {error}", record.id))?
        {
            Ok(()) => bound.push(record.id.clone()),
            Err(reason) => unbound.push(reason),
        }
    }
    let task = match bound.as_slice() {
        [one] => one,
        [] => {
            return Err(format!(
                "no task record it changes binds it to '{default}' ({}), so it is not a reviewed task head",
                if unbound.is_empty() {
                    "it changes none".to_string()
                } else {
                    unbound.join("; ")
                }
            ))
        }
        several => {
            return Err(format!(
                "{} each declare '{default}' in its range, and a task member binds exactly one task",
                several.join(", ")
            ))
        }
    };
    match completed.as_slice() {
        [done] if done == task => Ok(Some(task.clone())),
        [] => Err(format!(
            "it adds no completion of {task}, the task its records bind, so it is not a reviewed task head"
        )),
        done => Err(format!(
            "it adds the completion of {}, but {task} is the task its records bind",
            done.join(", ")
        )),
    }
}

/// The records of `other` that `record` matches: the one with its id, and
/// any other with its uid (a missing uid matches nothing).
fn matches<'g>(record: &RecordView, other: &'g Graph) -> Vec<&'g RecordView> {
    let mut found: Vec<&RecordView> = other.records.get(&record.id).into_iter().collect();
    if let Some(uid) = super::record_uid(&record.content) {
        found.extend(other.records.values().filter(|same| {
            same.id != record.id
                && super::record_uid(&same.content).as_deref() == Some(uid.as_str())
        }));
    }
    found
}

/// A record the member adds that shows a completion (an acceptance block,
/// a complete status or a reopen) without being a completion it adds: such
/// a record binds as a task member, or the member refuses.
fn completion_shown(repo: &Repository, member: Member) -> Result<Option<String>, String> {
    let before = Graph::from_revision(repo, &member.onto.to_string())?;
    let after = Graph::from_revision(repo, &member.head.to_string())?;
    Ok(after
        .records
        .values()
        .find(|record| !before.records.contains_key(&record.id) && super::shows_completion(record))
        .map(|record| {
            format!(
                "it adds {} ({}), which shows a completion; a new record with an acceptance block binds as a task member",
                record.id, record.path
            )
        }))
}

/// Why the member is not a filing (TSK-1154 AC-16): every change from
/// `member.onto` to its head must add a new regular file at a record
/// path; a changed, removed, renamed or linked path, or any other added
/// path, is the reason. `None` when it is one.
fn filing_problem(repo: &Repository, member: Member) -> Result<Option<String>, String> {
    let tree = |oid: Oid| {
        repo.find_commit(oid)
            .and_then(|commit| commit.tree())
            .map_err(|error| format!("cannot read the tree of {oid}: {}", error.message()))
    };
    let (before, after) = (tree(member.onto)?, tree(member.head)?);
    let diff = repo
        .diff_tree_to_tree(Some(&before), Some(&after), None)
        .map_err(|error| format!("cannot diff the member: {}", error.message()))?;
    for delta in diff.deltas() {
        let name = crate::git::GitName::from_bytes(
            delta
                .new_file()
                .path_bytes()
                .or_else(|| delta.old_file().path_bytes())
                .unwrap_or_default(),
        );
        if delta.status() != git2::Delta::Added {
            return Ok(Some(format!("it changes or removes {}", name.display())));
        }
        let regular = matches!(
            delta.new_file().mode(),
            git2::FileMode::Blob | git2::FileMode::BlobExecutable
        );
        // A name that is not UTF-8 is no record path, so it refuses.
        let record = name
            .rule_text()
            .ok()
            .and_then(crate::workgraph::work_start::record_kind_for_tree_path)
            .is_some();
        if !regular || !record {
            return Ok(Some(format!(
                "it adds {}, which is not a record file",
                name.display()
            )));
        }
    }
    Ok(None)
}

/// Why a member of a batch into the default target changes an existing
/// criterion (TSK-1154 AC-16), each reason once; empty when it changes
/// none. Records match across the member's two parents by id and by uid,
/// never by path: an id or uid on two records in one tree, or a record
/// that matches two records in the other, is a reason; a first-parent
/// record with no match at the member head is a change; and criteria are
/// compared on every matched pair. A record whose id and uid are both new
/// is an addition and passes.
fn record_changes(repo: &Repository, member: Member) -> Result<Vec<String>, String> {
    let before = Graph::from_revision(repo, &member.onto.to_string())?;
    let after = Graph::from_revision(repo, &member.head.to_string())?;
    let mut changes = Vec::new();
    for (graph, place) in [
        (&before, "the member's merge base"),
        (&after, "the member head"),
    ] {
        let mut seen = std::collections::BTreeMap::new();
        for record in graph.records.values() {
            if let Some(uid) = super::record_uid(&record.content) {
                if let Some(other) = seen.insert(uid.clone(), record.id.clone()) {
                    changes.push(format!(
                        "uid {uid} is on two records at {place} ({other} and {})",
                        record.id
                    ));
                }
            }
        }
    }
    if !changes.is_empty() {
        return Ok(changes);
    }
    let mut compared = std::collections::BTreeSet::new();
    let mut compare = |then: &RecordView, now: &RecordView, changes: &mut Vec<String>| {
        if compared.insert((then.id.clone(), now.id.clone()))
            && then.criteria.signature() != now.criteria.signature()
        {
            changes.push(if then.id == now.id {
                format!("{} changes its criteria", then.id)
            } else {
                format!(
                    "{} changes the criteria {} held at the member's merge base (matched by uid)",
                    now.id, then.id
                )
            });
        }
    };
    for then in before.records.values() {
        match matches(then, &after).as_slice() {
            [] => changes.push(format!(
                "{} ({}) has no record with its id or uid at the member head, so its criteria change",
                then.id, then.path
            )),
            [now] => compare(then, now, &mut changes),
            several => changes.push(format!(
                "{} matches {} at the member head, by id and by uid",
                then.id,
                several
                    .iter()
                    .map(|now| now.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ")
            )),
        }
    }
    for now in after.records.values() {
        match matches(now, &before).as_slice() {
            [] => {}
            [then] => compare(then, now, &mut changes),
            several => changes.push(format!(
                "{} matches {} at the member's merge base, by id and by uid",
                now.id,
                several
                    .iter()
                    .map(|then| then.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" and ")
            )),
        }
    }
    Ok(changes)
}

/// Each changed criterion of `task` between `onto` and `head` that carries
/// no new dated amendment note (`- Amended YYYY-MM-DD: ...`).
fn undated_changes(repo: &Repository, task: &str, member: Member) -> Result<Vec<String>, String> {
    let at_head = Graph::from_revision(repo, &member.head.to_string())?;
    let at_onto = Graph::from_revision(repo, &member.onto.to_string())?;
    let (Some(after), Some(before)) = (at_head.records.get(task), at_onto.records.get(task)) else {
        return Ok(Vec::new());
    };
    let mut undated = Vec::new();
    for item in &after.criteria.items {
        let Some(old) = before.criteria.items.iter().find(|old| old.id == item.id) else {
            continue;
        };
        if super::CriterionKey::of(old) == super::CriterionKey::of(item) {
            continue;
        }
        let noted = item
            .sub_items
            .iter()
            .any(|note| note.note && !old.sub_items.iter().any(|kept| kept.line == note.line));
        if !noted {
            undated.push(item.id.clone());
        }
    }
    Ok(undated)
}

/// The criteria `task` changes between `onto` and `head`, by id.
fn changed_criteria(repo: &Repository, task: &str, member: Member) -> Result<Vec<String>, String> {
    let at_head = Graph::from_revision(repo, &member.head.to_string())?;
    let at_onto = Graph::from_revision(repo, &member.onto.to_string())?;
    let (Some(after), Some(before)) = (at_head.records.get(task), at_onto.records.get(task)) else {
        return Ok(Vec::new());
    };
    Ok(after
        .criteria
        .items
        .iter()
        .filter(|item| {
            before
                .criteria
                .items
                .iter()
                .find(|old| old.id == item.id)
                .is_none_or(|old| super::CriterionKey::of(old) != super::CriterionKey::of(item))
        })
        .map(|item| item.id.clone())
        .collect())
}

fn member_findings(
    root: &std::path::Path,
    repo: &Repository,
    candidate: &Candidate<'_>,
    member: Member,
    bind: &mut dyn FnMut(Member) -> Result<Bound, String>,
) -> Result<Vec<Finding>, String> {
    let merged = format!("merged by {:.9} onto {:.9}", member.merge, member.onto);
    // A batch into the default target never takes the own-task rule,
    // whatever the binding answers: no existing criterion may change there.
    let main = candidate.mode == Mode::MainBatch;
    let bound = bind(member);
    let (task, own, note) = match &bound {
        Ok(Bound::Own(task)) => (Some(task.clone()), !main, None),
        Ok(Bound::Frozen { task, note }) => (task.clone(), false, note.clone()),
        Err(_) => (None, false, None),
    };
    let named = match (&bound, task.as_deref()) {
        (Err(_), _) => format!("member {:.9}, {merged}", member.head),
        (Ok(_), Some(task)) => format!("member {:.9} ({task}), {merged}", member.head),
        (Ok(_), None) if main => format!(
            "member {:.9} (a filing of new records), {merged}",
            member.head
        ),
        (Ok(_), None) => format!("member {:.9} (no task completion), {merged}", member.head),
    };
    let unchanged = if main {
        main_changes(repo, candidate, member, &named)
    } else {
        Vec::new()
    };
    if let Err(reason) = bound {
        let mut refused = unchanged;
        refused.push(finding(
            BINDING_RULE,
            format!("{named}: {reason}; a member is the reviewed head of one task, or a filing of new records"),
        ));
        return Ok(refused);
    }
    let mut found = vec![Finding {
        epic_record: None,
        rule: BINDING_RULE,
        message: format!("{named}: judged at its own reviewed head against the tip it merged onto"),
        note: true,
    }];
    found.extend(merge_findings(repo, member, &named)?);
    found.extend(unchanged);
    let (criteria, own_line) = member_criteria(
        root,
        repo,
        candidate,
        member,
        (&named, task.as_deref(), own, note),
        &mut found,
    )?;
    let judged = pull_request_findings_into(
        root,
        &member.onto.to_string(),
        &member.head.to_string(),
        &criteria,
        candidate.authority,
        (None, Some(candidate.target), None),
        &[],
        Scope {
            own_line: own_line.as_ref(),
            replaced: None,
            branch_prefixes: candidate.branch_prefixes,
            member: task.as_deref(),
        },
    )?;
    found.extend(judged.into_iter().map(|mut each| {
        each.message = format!("{named}: {}", each.message);
        each
    }));
    Ok(found)
}

/// The member merge against the clean remerge of its parents: a
/// hand-resolved hunk is the candidate's own work and reopens the member's
/// whole-unit review.
fn merge_findings(repo: &Repository, member: Member, named: &str) -> Result<Vec<Finding>, String> {
    let merge = repo
        .find_commit(member.merge)
        .map_err(|error| format!("cannot read merge {}: {}", member.merge, error.message()))?;
    Ok(match read_merge(repo, &merge) {
        MergeReading::Clean => Vec::new(),
        MergeReading::Changed(paths) => vec![finding(
            BINDING_RULE,
            format!(
                "{named}: the merge differs from the clean remerge of its parents in {}; that hand-resolved hunk is the candidate's own work, so the member's whole-unit review reopens: review the resolved result as the member's pull request, then rebuild the candidate with it",
                paths
                    .iter()
                    .map(|path| crate::git::GitName::from_bytes(path).display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )],
        MergeReading::Unknown(reason) => vec![finding(BINDING_RULE, format!("{named}: {reason}"))],
    })
}

/// Each existing criterion a member of a batch into the default target
/// changes, read from its own change ([`record_changes`]), as a refusal.
fn main_changes(
    repo: &Repository,
    candidate: &Candidate<'_>,
    member: Member,
    named: &str,
) -> Vec<Finding> {
    match own_change(repo, member).and_then(|own| record_changes(repo, own)) {
        Ok(changes) => changes
            .into_iter()
            .map(|change| {
                finding(
                    FROZEN_RULE,
                    format!("{named}: {change}; no existing criterion may change in a batch into '{}', so land such a change in its own pull request", candidate.target),
                )
            })
            .collect(),
        Err(reason) => vec![finding(
            FROZEN_RULE,
            format!("{named}: the records at its two parents cannot be read, so its criteria cannot be judged unchanged: {reason}"),
        )],
    }
}

/// The criteria rule a member is judged with, and the own-line exception
/// when its task takes its own pull request's rule; notes and refusals
/// about the choice go to `found`. `(named, task, own, note)`: the member
/// as messages name it, its task, whether that task takes the own rule,
/// and why it does not when the binding said so.
fn member_criteria(
    root: &std::path::Path,
    repo: &Repository,
    candidate: &Candidate<'_>,
    member: Member,
    (named, task, own, note): (&str, Option<&str>, bool, Option<String>),
    found: &mut Vec<Finding>,
) -> Result<(Criteria, Option<OwnLine>), String> {
    let main = candidate.mode == Mode::MainBatch;
    Ok(match (task, own) {
        (Some(id), true) => {
            let criteria = task_criteria(root, &member.onto.to_string(), id, true)?;
            let mut own_line = None;
            if criteria == Criteria::OwnTask(id.to_string()) {
                for criterion in undated_changes(repo, id, member)? {
                    found.push(finding(
                        FROZEN_RULE,
                        format!("{named}: {id} {criterion} changes without a dated amendment note; a task's own criteria change lands through its candidate only with the note inside the criterion (`- Amended YYYY-MM-DD: <what changed and why>`), which travels with the content"),
                    ));
                }
                let changed = changed_criteria(repo, id, member)?;
                if !changed.is_empty() {
                    found.push(Finding {
                        epic_record: None,
                        rule: FROZEN_RULE,
                        message: format!(
                            "{named}: {id} {} changed in its own reviewed pull request, judged at that head as its pull request judges it",
                            changed.join(", ")
                        ),
                        note: true,
                    });
                }
                own_line = candidate.authority.map(|_| OwnLine {
                    task: id.to_string(),
                    tip: member.onto,
                });
            }
            (criteria, own_line)
        }
        (None, _) if !main && planning_member(repo, member)? => {
            found.push(Finding {
                epic_record: None,
                rule: super::super::amendment::AMENDMENT_RULE,
                message: format!("{named}: adds no completion and changes only what a planning amendment may, so it is judged as a planning amendment, as its own pull request into the line is"),
                note: true,
            });
            (Criteria::Amendment(None), None)
        }
        _ => {
            if let (Some(id), Some(reason)) = (task, note) {
                found.push(Finding {
                    epic_record: None,
                    rule: FROZEN_RULE,
                    message: format!("{named}: the own-task criteria rule does not apply to {id}: {reason}, so its criteria stay as the tip holds them"),
                    note: true,
                });
            }
            (Criteria::Frozen, None)
        }
    })
}

/// A commit on the candidate that is not a member merge (a direct commit
/// or a sync), judged as a range of its own with frozen criteria: it may
/// change no criteria, a completion it makes binds at it, and a record it
/// brings counts only as a proven import. The direct-commit rule
/// (`work.epic_line`) judges a direct commit itself.
fn range_findings(
    root: &std::path::Path,
    candidate: &Candidate<'_>,
    named: &str,
    commit: Oid,
    onto: Oid,
) -> Result<Vec<Finding>, String> {
    let judged = pull_request_findings_into(
        root,
        &onto.to_string(),
        &commit.to_string(),
        &Criteria::Frozen,
        candidate.authority,
        (None, Some(candidate.target), None),
        &[],
        Scope {
            branch_prefixes: candidate.branch_prefixes,
            ..Scope::default()
        },
    )?;
    Ok(judged
        .into_iter()
        .map(|mut each| {
            each.message = format!("{named}: {}", each.message);
            each
        })
        .collect())
}
