//! Acceptance bound to the reviewed commit (SPC-013 R-52, R-53, R-60).
//!
//! The structural rules of an acceptance block live in
//! [`super::record_text`]; this module adds what needs git: the block names
//! the commit that was reviewed, and nothing but the record's status and
//! Closeout changed after it; a waiver names the planning amendment on the
//! target that changed that criterion; a task pull request leaves its
//! record's criteria as the target has them; and a range touching the
//! adopter-facing path set belongs to a task with a journey criterion.
//!
//! The checker proves structure and binding only ([`SCOPE_NOTE`]). It does
//! not prove the evidence is honest, and the review reference is
//! recheckable, not authenticated.

use git2::{Oid, Repository};

use super::lifecycle::{Graph, RecordView};
use super::record_text::{outcome_word, AcceptanceBlock, Criterion};
use super::work_start::RecordKind;

/// The acceptance block names the reviewed commit, and waivers name their
/// amendment (R-60). Level: `git.work_records`.
pub const BINDING_RULE: &str = "work.acceptance_binding";
/// A task pull request leaves its record's criteria as the target has them
/// (R-52). Always blocks.
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
}

fn finding(rule: &'static str, message: String) -> Finding {
    Finding { rule, message }
}

/// The active (not superseded) acceptance block of a record, when exactly
/// one parses. Structure is judged elsewhere; binding needs a parsed block.
fn active_block(record: &RecordView) -> Option<AcceptanceBlock> {
    let mut blocks = record
        .active_blocks()
        .into_iter()
        .filter_map(|block| block.parsed.ok());
    let block = blocks.next()?;
    blocks.next().is_none().then_some(block)
}

/// A commit named by its object id, never by a ref name: a branch named like
/// the id cannot stand in for the reviewed commit or the amendment.
fn commit_of(repo: &Repository, value: &str) -> Option<Oid> {
    super::work_start::commit_by_object_id(repo, value.trim())
        .ok()
        .map(|commit| commit.id())
}

fn is_ancestor_or_same(repo: &Repository, ancestor: Oid, of: Oid) -> bool {
    ancestor == of || repo.graph_descendant_of(of, ancestor).unwrap_or(false)
}

fn blob_at(repo: &Repository, commit: Oid, path: &str) -> Option<String> {
    let tree = repo.find_commit(commit).ok()?.tree().ok()?;
    let entry = tree.get_path(std::path::Path::new(path)).ok()?;
    let blob = repo.find_blob(entry.id()).ok()?;
    Some(String::from_utf8_lossy(blob.content()).into_owned())
}

/// A record's text without its `status:` line and its `## Closeout`
/// section: what must not change after the reviewed commit.
fn reviewed_part(content: &str) -> String {
    let mut out = Vec::new();
    let mut in_frontmatter = false;
    let mut in_closeout = false;
    let mut in_fence = false;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim_end();
        if index == 0 && trimmed == "---" {
            in_frontmatter = true;
            out.push(line);
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" {
                in_frontmatter = false;
            } else if line.starts_with("status:") {
                continue;
            }
            out.push(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if !in_fence && trimmed.starts_with("## ") {
            in_closeout = trimmed == "## Closeout";
        }
        if !in_closeout {
            out.push(line);
        }
    }
    out.join("\n").trim_end().to_string()
}

/// Bind a completed task's active acceptance block to the reviewed commit
/// (R-60): the reviewed commit is `head`, or an ancestor after which only
/// this record's status and Closeout changed; every waiver names a commit on
/// the target, not `head`, that amended this record's criterion. A leaf that
/// serves its epic's journey says what ran (R-53).
#[must_use]
pub fn bind_completion(
    repo: &Repository,
    task: &RecordView,
    graph: &Graph,
    head: Oid,
    target_tip: Option<Oid>,
) -> Vec<Finding> {
    let Some(block) = active_block(task) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    let mut bind = |message: String| findings.push(finding(BINDING_RULE, message));
    match commit_of(repo, &block.reviewed) {
        None => bind(format!(
            "{}: reviewed commit {} is not in this repository",
            task.id, block.reviewed
        )),
        Some(reviewed) if reviewed == head => {}
        Some(reviewed) if !is_ancestor_or_same(repo, reviewed, head) => bind(format!(
            "{}: reviewed commit {} is not the head or an ancestor of it; review the result that lands",
            task.id, block.reviewed
        )),
        Some(reviewed) => {
            if let Some(problem) = later_change(repo, task, reviewed, head) {
                bind(format!(
                    "{}: {problem} after the reviewed commit {}; review the result again",
                    task.id, block.reviewed
                ));
            }
        }
    }
    for (id, result) in &block.criteria {
        if result.outcome == "waived" {
            if let Some(problem) =
                waiver_problem(repo, task, id, &result.evidence, head, target_tip)
            {
                bind(format!("{}: {id} waiver {problem}", task.id));
            }
        }
    }
    findings.extend(leaf_journey(task, graph, &block));
    findings
}

/// Why the diff from `reviewed` to `head` is more than this record's status
/// and Closeout, if it is.
fn later_change(repo: &Repository, task: &RecordView, reviewed: Oid, head: Oid) -> Option<String> {
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
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|path| *path != task.path)
        .collect();
    if let Some(path) = other.first() {
        return Some(format!("{path} changed"));
    }
    let then = blob_at(repo, reviewed, &task.path)?;
    (reviewed_part(&then) != reviewed_part(&task.content))
        .then(|| "the record changed outside its status and Closeout".to_string())
}

/// Why a waiver's commit is not the planning amendment for this record and
/// criterion on the target, if it is not.
fn waiver_problem(
    repo: &Repository,
    task: &RecordView,
    id: &str,
    evidence: &str,
    head: Oid,
    target_tip: Option<Oid>,
) -> Option<String> {
    let Some(amendment) = commit_of(repo, evidence.trim()) else {
        return Some(format!(
            "names {}, which is not a commit here",
            evidence.trim()
        ));
    };
    if amendment == head {
        return Some(
            "names the pull request head; a waiver is a planning amendment on the target".into(),
        );
    }
    let Some(tip) = target_tip else {
        return Some("cannot be checked: the target does not resolve here".into());
    };
    if !is_ancestor_or_same(repo, amendment, tip) {
        return Some(format!(
            "names {}, which is not on the target",
            evidence.trim()
        ));
    }
    let commit = repo.find_commit(amendment).ok()?;
    let parent = commit.parent_id(0).ok();
    let criterion = |oid: Option<Oid>| {
        oid.and_then(|oid| blob_at(repo, oid, &task.path))
            .and_then(|content| RecordView::parse(RecordKind::Task, &task.path, &content).ok())
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
        format!(
            "{task_id} changes the adopter-facing path set but has no `(journey)` criterion and serves no epic journey criterion"
        )
    })
}

/// Records of the range whose criteria differ from the target's (R-52).
#[must_use]
pub fn frozen_criteria(head: &Graph, target: &Graph, changed_paths: &[String]) -> Vec<Finding> {
    head.records
        .values()
        .filter(|record| record.kind == RecordKind::Task)
        .filter(|record| changed_paths.contains(&record.path))
        .filter_map(|record| {
            let there = target.records.get(&record.id)?;
            (there.criteria.signature() != record.criteria.signature()).then(|| {
                finding(
                    FROZEN_RULE,
                    format!(
                        "{} changes its criteria on this branch; criteria change only by a planning pull request on the target, then this branch rebases",
                        record.id
                    ),
                )
            })
        })
        .collect()
}

/// The binding findings of a pull request: every task record the range
/// completes, or whose active block it changes, bound to `head`.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
pub fn completions_in_range(
    repo: &Repository,
    base: &str,
    head: &str,
    target_tip: Option<Oid>,
) -> Result<Vec<Finding>, String> {
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
    let mut findings = Vec::new();
    for task in after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task && record.status == "complete")
    {
        let block = active_block(task);
        let unchanged = before
            .records
            .get(&task.id)
            .is_some_and(|then| then.status == "complete" && active_block(then) == block);
        if !unchanged {
            findings.extend(bind_completion(repo, task, &after, head_oid, target_tip));
        }
    }
    Ok(findings)
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

/// The findings of a pull request from `base` (the target tip) to `head`:
/// criteria frozen for any branch but a planning branch or an epic line,
/// and every completion in the range bound to `head`.
///
/// # Errors
///
/// Returns a message when a revision, the merge-base or a tree cannot be
/// read.
pub fn pull_request_findings(
    repo_root: &std::path::Path,
    base: &str,
    head: &str,
    branch: &str,
) -> Result<Vec<Finding>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let oid = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let target_tip = oid(base)?;
    let mut found = Vec::new();
    if !branch.starts_with("plan/") && !branch.starts_with("integration/") {
        let anchor = repo
            .merge_base(target_tip, oid(head)?)
            .map_err(|error| error.message().to_string())?;
        let paths = super::lifecycle::changed_paths(&repo, &anchor.to_string(), Some(head))?;
        let at_head = Graph::from_revision(&repo, head)?;
        let at_target = Graph::from_revision(&repo, base)?;
        found.extend(frozen_criteria(&at_head, &at_target, &paths));
    }
    found.extend(completions_in_range(&repo, base, head, Some(target_tip))?);
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
}
