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

use super::{
    active_block, default_target_name, finding, history, imports::home_tip,
    is_ancestor_or_same, pull_request_findings_into, read_merge, task_criteria, Criteria, Finding,
    Graph, MergeReading, Oid, OwnLine, RecordKind, Repository, Scope, BINDING_RULE, FROZEN_RULE,
};

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
}

/// One member of a candidate: the merge, the tip it merged onto (its first
/// parent) and the member head (its second parent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Member {
    merge: Oid,
    onto: Oid,
    head: Oid,
}

/// Judge `head` as `candidate`'s next tip, member by member. `bind` is asked
/// once per member task, with the task id and the member head, and answers
/// `Err(reason)` when that task may not take its own pull request's
/// criteria rule at that head; the member is then judged with frozen
/// criteria and the reason is reported.
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
    bind: &mut dyn FnMut(&str, &str) -> Result<(), String>,
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
    let mut default_tip = None;
    for (commit, parents) in first_parent_chain(&repo, tip, head)? {
        match parents.as_slice() {
            [onto, member] if is_sync(&repo, &mut default_tip, *member)? => {
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

/// Whether `member` is already on the published default target's tip, so
/// its merge is a sync of that target, not a member. The tip is read once,
/// from the default target's fetch remote; with none published, nothing is
/// a sync and each merge is judged as a member.
fn is_sync(
    repo: &Repository,
    default_tip: &mut Option<Option<Oid>>,
    member: Oid,
) -> Result<bool, String> {
    if default_tip.is_none() {
        let published = match default_target_name(repo) {
            Ok(default) => home_tip(repo, &default.name)?,
            Err(_) => None,
        };
        *default_tip = Some(published);
    }
    match default_tip {
        Some(Some(tip)) => is_ancestor_or_same(repo, member, *tip),
        _ => Ok(false),
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

/// The one task whose completion `member.head` adds over `member.onto`:
/// complete at the head, with an active block the tip does not hold.
/// `None` when no task or more than one does.
fn member_task(repo: &Repository, member: Member) -> Result<Option<String>, String> {
    let at_head = Graph::from_revision(repo, &member.head.to_string())?;
    let at_onto = Graph::from_revision(repo, &member.onto.to_string())?;
    let mut completed = at_head.records.values().filter(|record| {
        record.kind == RecordKind::Task
            && record.status == "complete"
            && active_block(record).is_some()
            && at_onto.records.get(&record.id).is_none_or(|before| {
                before.status != "complete" || active_block(before) != active_block(record)
            })
    });
    let first = completed.next().map(|record| record.id.clone());
    Ok(if completed.next().is_some() {
        None
    } else {
        first
    })
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
    bind: &mut dyn FnMut(&str, &str) -> Result<(), String>,
) -> Result<Vec<Finding>, String> {
    let task = member_task(repo, member)?;
    let named = format!(
        "member {:.9} ({}), merged by {:.9} onto {:.9}",
        member.head,
        task.as_deref().unwrap_or("no task completion"),
        member.merge,
        member.onto
    );
    let mut found = vec![Finding {
        epic_record: None,
        rule: BINDING_RULE,
        message: format!("{named}: judged at its own reviewed head against the tip it merged onto"),
        note: true,
    }];
    let merge = repo
        .find_commit(member.merge)
        .map_err(|error| format!("cannot read merge {}: {}", member.merge, error.message()))?;
    match read_merge(repo, &merge) {
        MergeReading::Clean => {}
        MergeReading::Changed(paths) => found.push(finding(
            BINDING_RULE,
            format!(
                "{named}: the merge differs from the clean remerge of its parents in {}; that hand-resolved hunk is the candidate's own work, so the member's whole-unit review reopens: review the resolved result as the member's pull request, then rebuild the candidate with it",
                paths
                    .iter()
                    .map(|path| crate::git::GitName::from_bytes(path).display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
        MergeReading::Unknown(reason) => found.push(finding(BINDING_RULE, format!("{named}: {reason}"))),
    }
    let base = member.onto.to_string();
    let member_head = member.head.to_string();
    let mut own_line = None;
    let criteria = match task.as_deref() {
        None => Criteria::Frozen,
        Some(id) => match bind(id, &member_head) {
            Ok(()) => {
                let criteria = task_criteria(root, &base, id, true)?;
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
                criteria
            }
            Err(reason) => {
                found.push(Finding {
                    epic_record: None,
                    rule: FROZEN_RULE,
                    message: format!("{named}: the own-task criteria rule does not apply to {id}: {reason}, so its criteria stay as the tip holds them"),
                    note: true,
                });
                Criteria::Frozen
            }
        },
    };
    let judged = pull_request_findings_into(
        root,
        &base,
        &member_head,
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
