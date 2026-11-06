//! A landed planning move of a reviewed task's epic (TSK-1320, SPC-013 R-60).
//!
//! A reviewed task branch may re-merge its target cleanly after the review.
//! When that brings a planning pull request's move of the task to another
//! epic, the record differs from its reviewed part, and the binding would
//! refuse. This module admits that one edit and nothing else: the
//! `epic_id` line changes and plain lines are added outside the front
//! matter, the acceptance criteria, the Deliverables and the Closeout; one
//! commit on the first-parent line of a target tip the run is judged
//! against made exactly that edit; and the new epic lands its tasks on the
//! task's unchanged `integration_target` at that tip. Trust comes only from
//! those commits and the parsed records they hold, never from a branch
//! name, a title, a message or a date. Anything unreadable refuses.
//!
//! The head's own first-parent chain from the reviewed commit is never a
//! landed change, even where a caller trusts the head as a tip. The check
//! proves that the commit is on a trusted tip's first-parent line, not that
//! a reviewed planning pull request made it. On any amendable range (a line
//! pull request into `main`, a push to the line, a root-branch range or a
//! planning amendment) the range head itself is also a trusted tip
//! (`with_own_line`), so that head is the move's authority there; at a line
//! pull request into `main` the operator reviews and merges it.

use std::collections::{BTreeMap, HashMap, HashSet};

use git2::{Oid, Repository};

use super::super::line_adoption;
use super::super::record_text::{
    acceptance_blocks, frontmatter_len, parse_criteria, scan_record, section_span, LineKind,
    ScannedLine,
};
use super::super::work_start::{epic_facts_at, line_membership, logical_target, LineMembership};
use super::{blob_at, first_parent, history, is_ancestor_or_same, reviewed_part, StackingTips};

/// Sections an admitted edit leaves byte for byte.
const PROTECTED: [&str; 2] = ["## Acceptance Criteria", "## Deliverables"];

/// What part of a record two versions compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// The reviewed parts ([`reviewed_part`]): status and Closeout are the
    /// completion's to change.
    Reviewed,
    /// The whole record, as a planning commit on the target changed it:
    /// status, Closeout and acceptance blocks stay as they were.
    Whole,
}

/// An admitted edit: the epic it moves the task from and to, and the
/// lines it adds.
#[derive(Debug, PartialEq, Eq)]
struct Move {
    from: String,
    to: String,
    added: Vec<String>,
}

/// One commit on a target tip's first-parent line that changed the record.
struct Change {
    tip: Oid,
    commit: Oid,
    before: Option<String>,
    after: Option<String>,
}

/// Why the difference between the reviewed record `then` and the completed
/// record is not one landed planning move of its epic, or `None` when it
/// is. Called only where the chain from `reviewed` to `at` stacks, so every
/// change to the reviewed part came through clean merges of `tips`.
pub(super) fn refusal(
    repo: &Repository,
    path: &str,
    reviewed: Oid,
    at: Oid,
    then: &str,
    completed: &str,
    tips: &StackingTips,
) -> Option<String> {
    let changes = match target_changes(repo, path, reviewed, at, tips) {
        Ok(changes) => changes,
        Err(error) => return Some(format!("{error}, so no landed planning move can be proved")),
    };
    let edit = shape(
        &reviewed_part(then),
        &reviewed_part(completed),
        Scope::Reviewed,
    );
    let change = match changes.as_slice() {
        [] => {
            let part = edit
                .err()
                .map_or_else(String::new, |part| format!("{part}; "));
            let tips = tips
                .tips()
                .iter()
                .map(Oid::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Some(format!(
                "{part}no commit on the first-parent line of the target tip this run is judged against ({tips}) changed the record"
            ));
        }
        [change] => change,
        many => {
            let commits = many
                .iter()
                .map(|change| format!("{} (target tip {})", change.commit, change.tip))
                .collect::<Vec<_>>()
                .join(", ");
            return Some(format!(
                "commits {commits} on the first-parent lines of the target tips this run is judged against changed the record; only one landed planning move is admitted"
            ));
        }
    };
    let commit = change.commit;
    let on = format!("on the first-parent line of the target tip {}", change.tip);
    let (Some(before), Some(after)) = (&change.before, &change.after) else {
        return Some(format!("commit {commit} {on} adds or removes the record"));
    };
    let landed = match shape(before, after, Scope::Whole) {
        Ok(landed) => landed,
        Err(part) => return Some(format!("commit {commit} {on} changed the record: {part}")),
    };
    let edit = match edit {
        Ok(edit) => edit,
        Err(part) => {
            return Some(format!(
                "{part}, which commit {commit} {on} does not account for"
            ))
        }
    };
    if (&edit.from, &edit.to) != (&landed.from, &landed.to) {
        return Some(format!(
            "the head moves the task from {} to {}, but commit {commit} {on} moves it from {} to {}",
            edit.from, edit.to, landed.from, landed.to
        ));
    }
    let mut landed_lines = landed.added;
    for line in &edit.added {
        match landed_lines.iter().position(|landed| landed == line) {
            Some(index) => {
                landed_lines.swap_remove(index);
            }
            None => {
                return Some(format!(
                    "the line `{line}` was added, and commit {commit} {on} did not add it"
                ))
            }
        }
    }
    landing_line_problem(repo, completed, &edit.to, commit, change.tip)
}

/// Why tasks of `epic` do not land on the record's `integration_target`
/// at the target tip `tip`, if they do not: the line's own epic, or an epic
/// whose record there declares the line as its `parent_line`.
fn landing_line_problem(
    repo: &Repository,
    completed: &str,
    epic: &str,
    commit: Oid,
    tip: Oid,
) -> Option<String> {
    let target = match front_matter(completed) {
        Ok((data, _)) => crate::validate::get_string_field(&data, "integration_target"),
        Err(error) => return Some(error),
    };
    if target.is_empty() {
        return Some(format!(
            "commit {commit} moves the task to {epic}, but the record names no integration_target, so its landing line cannot be proved"
        ));
    }
    let line = logical_target(&target);
    let line_epic = match line_adoption::epic_id(line) {
        Ok(line_epic) => line_epic,
        Err(why) => {
            return Some(format!(
                "commit {commit} moves the task to {epic}, but its landing line cannot be proved: {why}"
            ))
        }
    };
    let root = repo.workdir().unwrap_or_else(|| repo.path());
    let mut facts = BTreeMap::new();
    for id in [epic, line_epic] {
        match epic_facts_at(root, id, &tip.to_string()) {
            Ok(found) => {
                facts.insert(
                    id.to_string(),
                    found.map(|found| (found.status, found.parent_line)),
                );
            }
            Err(error) => {
                return Some(format!(
                    "{id}'s record at the target tip {tip} cannot be read: {error}"
                ))
            }
        }
    }
    let lookup = |id: &str| {
        facts
            .get(id)
            .cloned()
            .flatten()
            .map(
                |(status, parent_line)| super::super::work_start::EpicFacts {
                    status,
                    parent_line,
                },
            )
    };
    match line_membership(line, epic, &lookup, &format!("the target tip {tip}")) {
        LineMembership::Own | LineMembership::Declared => None,
        LineMembership::Refused(why) | LineMembership::Not(why) => Some(format!(
            "commit {commit} moves the task to {epic}, whose tasks do not land on {line} ({why})"
        )),
    }
}

/// Every commit on the first-parent line of a run tip, after the last one
/// the reviewed commit holds, that `at` holds and whose record at `path`
/// differs from its first parent's. The head's own first-parent chain from
/// `reviewed` to `at`, which [`super::stacked`] has judged, is never a
/// target change, even where a caller passes the head as a tip (readiness
/// does). History that cannot be read, or a line that never reaches the
/// reviewed commit's history, is an error.
fn target_changes(
    repo: &Repository,
    path: &str,
    reviewed: Oid,
    at: Oid,
    tips: &StackingTips,
) -> Result<Vec<Change>, String> {
    let _judgment = history::judgment();
    history::check(repo).map_err(|error| error.to_string())?;
    let own = own_span(repo, reviewed, at)?;
    let mut changes: Vec<Change> = Vec::new();
    for &tip in tips.tips() {
        let mut cursor = tip;
        while !is_ancestor_or_same(repo, cursor, reviewed)? {
            let Some(parent) = first_parent(repo, cursor)? else {
                return Err(format!(
                    "the first-parent line of the target tip {tip} never reaches the reviewed commit {reviewed}'s history"
                ));
            };
            if !own.contains(&cursor)
                && !changes.iter().any(|change| change.commit == cursor)
                && is_ancestor_or_same(repo, cursor, at)?
            {
                let before = blob_at(repo, parent, path)?;
                let after = blob_at(repo, cursor, path)?;
                if before != after {
                    changes.push(Change {
                        tip,
                        commit: cursor,
                        before,
                        after,
                    });
                }
            }
            cursor = parent;
        }
    }
    Ok(changes)
}

/// The commits on `at`'s first-parent chain after `reviewed`: the task
/// branch's own completion and its merges of the target. The chain must
/// reach `reviewed`, as [`super::stacked`] requires; otherwise it is an
/// error.
fn own_span(repo: &Repository, reviewed: Oid, at: Oid) -> Result<HashSet<Oid>, String> {
    let mut own = HashSet::new();
    let mut cursor = at;
    while cursor != reviewed {
        own.insert(cursor);
        cursor = first_parent(repo, cursor)?.ok_or_else(|| {
            format!("the first-parent chain of {at} never reaches the reviewed commit {reviewed}")
        })?;
    }
    Ok(own)
}

/// The admitted edit from `before` to `after`, or which part of the record
/// it changed beyond it.
fn shape(before: &str, after: &str, scope: Scope) -> Result<Move, String> {
    let old = record_lines(before);
    let new = record_lines(after);
    let (old_end, new_end) = (frontmatter_len(&old), frontmatter_len(&new));
    if old_end == 0 || new_end == 0 {
        return Err("the front matter cannot be read".into());
    }
    if old_end != new_end {
        return Err("the front matter gained or lost a line".into());
    }
    for (was, now) in old[..old_end].iter().zip(&new[..new_end]) {
        if was != now && !(is_epic_line(was) && is_epic_line(now)) {
            return Err(format!(
                "the front matter field `{}` changed",
                field_name(was)
            ));
        }
    }
    let (mut old_data, old_body) = front_matter(before)?;
    let (mut new_data, new_body) = front_matter(after)?;
    let from = crate::validate::get_string_field(&old_data, "epic_id");
    let to = crate::validate::get_string_field(&new_data, "epic_id");
    old_data.remove("epic_id");
    new_data.remove("epic_id");
    if old_data != new_data {
        return Err("the front matter changed as parsed".into());
    }
    let old_scan = scan_record(&old);
    let new_scan = scan_record(&new);
    let mut compared: Vec<&str> = PROTECTED.to_vec();
    if scope == Scope::Whole {
        compared.push("## Closeout");
    }
    for heading in compared {
        if section_text(&old, &old_scan, heading) != section_text(&new, &new_scan, heading) {
            return Err(format!("the {} section changed", &heading[3..]));
        }
    }
    let added = added_lines(
        (&old, &old_scan, old_end),
        (&new, &new_scan, new_end),
        scope,
    )?;
    if parse_criteria(&old_body) != parse_criteria(&new_body) {
        return Err("the acceptance criteria changed as parsed".into());
    }
    if scope == Scope::Whole {
        let inner = |body: &str| {
            acceptance_blocks(body)
                .into_iter()
                .map(|block| block.inner)
                .collect::<Vec<_>>()
        };
        if inner(&old_body) != inner(&new_body) {
            return Err("an acceptance block changed".into());
        }
    }
    if from.is_empty() {
        return Err("the reviewed record names no epic".into());
    }
    if to.is_empty() {
        return Err("epic_id is null or missing".into());
    }
    if from == to {
        return Err(
            "lines were added, but epic_id is unchanged, so this is no planning move".into(),
        );
    }
    Ok(Move { from, to, added })
}

/// The lines `new` adds to `old`, where `old`'s body is a subsequence of
/// `new`'s and every matched line keeps how it parses. Each added line must
/// be visible text that is no heading and not empty, outside the protected
/// sections. Its visible text is the whole line, so it holds no inline
/// HTML either.
fn added_lines(
    (old, old_scan, old_end): (&[&str], &[ScannedLine], usize),
    (new, new_scan, new_end): (&[&str], &[ScannedLine], usize),
    scope: Scope,
) -> Result<Vec<String>, String> {
    let mut protected: Vec<&str> = PROTECTED.to_vec();
    if scope == Scope::Whole {
        protected.push("## Closeout");
    }
    let spans: Vec<(usize, usize)> = protected
        .iter()
        .filter_map(|heading| section_span(new_scan, heading))
        .collect();
    let mut matched = old_end;
    let mut added = Vec::new();
    for (index, line) in new.iter().enumerate().skip(new_end) {
        let same_parse = |at: usize| {
            old_scan[at].kind == new_scan[index].kind
                && old_scan[at].heading == new_scan[index].heading
        };
        if matched < old.len() && old[matched] == *line && same_parse(matched) {
            matched += 1;
            continue;
        }
        // An old line that no later new line repeats was removed or
        // changed; otherwise this new line is an addition before it.
        if let Some(was) = old.get(matched) {
            if !new[index + 1..].contains(was) {
                return Err(format!(
                    "the line `{was}` in {} was removed or changed",
                    section_of(old_scan, matched)
                ));
            }
        }
        let section = section_of(new_scan, index);
        let scanned = &new_scan[index];
        if scanned.kind != LineKind::Text
            || scanned.visible != *line
            || scanned.heading.is_some()
            || line.trim_matches([' ', '\t']).is_empty()
            || line.trim_start_matches([' ', '\t']).starts_with('#')
        {
            return Err(format!(
                "the line `{line}` added in {section} changes how the record parses"
            ));
        }
        if spans
            .iter()
            .any(|(start, end)| (*start..*end).contains(&index))
        {
            return Err(format!("the line `{line}` was added to {section}"));
        }
        added.push((*line).to_string());
    }
    if let Some(line) = old.get(matched) {
        return Err(format!(
            "the line `{line}` in {} was removed or changed",
            section_of(old_scan, matched)
        ));
    }
    Ok(added)
}

/// Whether a front matter line is the `epic_id` field.
fn is_epic_line(line: &str) -> bool {
    line.starts_with("epic_id:")
}

/// The key a front matter line sets, for messages.
fn field_name(line: &str) -> &str {
    line.split_once(':')
        .map_or(line, |(key, _)| key)
        .trim_matches([' ', '\t'])
}

/// The record's lines as [`reviewed_part`] splits them: at LF, with one
/// trailing CR removed from each line and no empty line after a final LF.
fn record_lines(text: &str) -> Vec<&str> {
    let text = text.strip_suffix('\n').unwrap_or(text);
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// The parsed front matter fields and the body of a record. A body that
/// is not UTF-8 is an error, never decoded lossily.
fn front_matter(content: &str) -> Result<(HashMap<String, serde_yaml::Value>, String), String> {
    let (data, body) = crate::validate::parse_frontmatter(content.as_bytes())
        .map_err(|error| format!("the front matter cannot be read: {error}"))?;
    let body =
        String::from_utf8(body).map_err(|error| format!("the body cannot be read: {error}"))?;
    Ok((data, body))
}

/// The lines of the level-two section `heading`, when the record has it.
fn section_text(lines: &[&str], scanned: &[ScannedLine], heading: &str) -> Option<String> {
    section_span(scanned, heading).map(|(start, end)| lines[start..end].join("\n"))
}

/// The level-two section line `at` sits in, for messages.
fn section_of(scanned: &[ScannedLine], at: usize) -> String {
    scanned[..=at]
        .iter()
        .rev()
        .find_map(|line| line.heading.as_deref())
        .map_or_else(
            || "the text before the first section".to_string(),
            |heading| format!("the {heading} section"),
        )
}

#[cfg(test)]
mod tests;
