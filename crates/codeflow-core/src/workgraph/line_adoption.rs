//! Recorded adoption of direct commits on an epic's first-parent line
//! (SPC-013 R-52, amended 2026-10-06 by TSK-248).
//!
//! An epic line holds only classified landings. A non-merge commit already
//! shared on the line is accepted only when the epic record at the head
//! lists it under `line_adoptions` and that entry is itself landed work: the
//! target tip already lists it, or the first commit on the line's
//! first-parent history whose record holds the entry is a merge.
use std::path::Path;

use git2::{Oid, Repository};
use serde_yaml::Value;

use super::work_start::{records_from_tree_matching, RecordKind};

/// A reviewed direct commit retained on an epic line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineAdoption {
    /// Full Git object id, normalized to lower-case hexadecimal.
    pub commit: String,
    /// Why this direct commit belongs on the line.
    pub reason: String,
    /// Where the adoption was reviewed.
    pub review: String,
}

/// What [`check_range`] found on a line's first-parent range.
#[derive(Debug, Default)]
pub struct AdoptionReport {
    /// Direct commits in the range with landed adoption entries, oldest first.
    pub adopted: Vec<LineAdoption>,
    /// Entries at the head naming commits outside the first-parent range.
    pub outside_range: Vec<LineAdoption>,
    /// Full ids of direct commits in the range with no landed adoption,
    /// oldest first.
    pub unadopted: Vec<String>,
    /// Why the epic record's `line_adoptions` could not be read at the head,
    /// when it could not.
    pub record_error: Option<String>,
}

impl AdoptionReport {
    /// The refusal for a line that holds unadopted direct commits, or `None`
    /// when every direct commit in the range is adopted.
    #[must_use]
    pub fn refusal(&self, branch: &str, epic: &str) -> Option<String> {
        if self.unadopted.is_empty() {
            return None;
        }
        let ids = self
            .unadopted
            .iter()
            .map(|id| &id[..9])
            .collect::<Vec<_>>()
            .join(", ");
        let noun = if self.unadopted.len() == 1 {
            "a commit"
        } else {
            "commits"
        };
        let unread = self
            .record_error
            .as_ref()
            .map(|error| format!("; {epic}'s line_adoptions cannot be read: {error}"))
            .unwrap_or_default();
        Some(format!(
            "'{branch}' has {noun} made directly on the line ({ids}); land work on the line by classified pull requests, or adopt a shared commit with a landed line_adoptions entry in {epic}{unread}"
        ))
    }
}

/// Parse and check the shape of an epic record's `line_adoptions` value.
pub(crate) fn parse(value: Option<&Value>) -> Result<Vec<LineAdoption>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value
        .as_sequence()
        .ok_or("line_adoptions must be a YAML list")?;
    entries
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let field = format!("line_adoptions[{index}]");
            let entry = value
                .as_mapping()
                .ok_or_else(|| format!("{field} must be a mapping"))?;
            let text = |name: &str| {
                entry
                    .get(Value::String(name.into()))
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| format!("{field}.{name} must be a non-empty string"))
            };
            let commit = text("commit")?;
            if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{field}.commit must be a full 40-hex commit id"));
            }
            Ok(LineAdoption {
                commit: commit.to_ascii_lowercase(),
                reason: text("reason")?,
                review: text("review")?,
            })
        })
        .collect()
}

/// Extract the epic id from an integration line's name.
///
/// # Errors
/// Returns the expected naming convention if the branch names no epic.
pub fn epic_id(branch: &str) -> Result<&str, String> {
    let suffix = branch
        .strip_prefix("integration/")
        .ok_or_else(|| format!("'{branch}' is not an integration line"))?;
    let digits = suffix.strip_prefix("EPC-").map_or(0, |rest| {
        rest.chars().take_while(char::is_ascii_digit).count()
    });
    let id = suffix.get(..4 + digits).unwrap_or_default();
    if digits == 0 || !super::is_valid_epic_format_id(id) || !suffix[id.len()..].starts_with('-') {
        return Err(format!(
            "'{branch}' names no epic; an epic line is integration/EPC-NNN-<slug>"
        ));
    }
    Ok(id)
}

/// The epic's adoption entries in the tree of `oid`; none when the record
/// is absent.
fn entries(repo: &Repository, oid: Oid, epic: &str) -> Result<Vec<LineAdoption>, String> {
    let tree = repo
        .find_commit(oid)
        .and_then(|commit| commit.tree())
        .map_err(|e| e.to_string())?;
    let records = records_from_tree_matching(repo, &tree, |path, kind| {
        kind == RecordKind::Epic && Path::new(path).file_stem().is_some_and(|name| name == epic)
    })
    .map_err(|e| e.to_string())?;
    Ok(records
        .get(epic)
        .filter(|record| record.kind == RecordKind::Epic)
        .map(|record| record.line_adoptions.clone())
        .unwrap_or_default())
}

fn first_parent_range(repo: &Repository, head: Oid, base: Oid) -> Result<Vec<Oid>, String> {
    let mut walk = repo.revwalk().map_err(|e| e.to_string())?;
    walk.push(head)
        .and_then(|()| walk.hide(base))
        .and_then(|()| walk.simplify_first_parent())
        .map_err(|e| e.to_string())?;
    walk.map(|oid| oid.map_err(|e| e.to_string())).collect()
}

/// Judge the direct commits on the first-parent range `base..head` of an
/// epic line against the epic's landed adoptions. `target` is the default
/// destination tip: it supplies entries already landed there and bounds the
/// line history searched for the merge that brought each entry.
///
/// # Errors
/// Returns an error only when the history cannot be read. Direct commits
/// without a landed adoption are listed in [`AdoptionReport::unadopted`].
pub fn check_range(
    root: &Path,
    branch: &str,
    target: &str,
    base: &str,
    head: &str,
) -> Result<AdoptionReport, String> {
    let epic = epic_id(branch)?;
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let resolve = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|e| format!("{revision}: {e}"))
    };
    let head = resolve(head)?;
    let target = resolve(target)?;
    let mut range = first_parent_range(&repo, head, resolve(base)?)?;
    range.reverse();
    let mut report = AdoptionReport::default();
    let adoptions = entries(&repo, head, epic).unwrap_or_else(|error| {
        report.record_error = Some(error);
        Vec::new()
    });
    let mut direct = Vec::new();
    for oid in &range {
        let parents = repo
            .find_commit(*oid)
            .map_err(|e| e.to_string())?
            .parent_count();
        if parents < 2 {
            let id = oid.to_string();
            direct.push((
                id.clone(),
                adoptions.iter().find(|entry| entry.commit == id),
            ));
        }
    }
    let at_target = if direct.iter().any(|(_, entry)| entry.is_some()) {
        entries(&repo, target, epic).unwrap_or_default()
    } else {
        Vec::new()
    };
    // The line's first-parent history from its fork, oldest first, with the
    // epic's entries at each commit. The fork commit is included, so an entry
    // the line merely inherited is never credited to a later merge.
    let history = if direct
        .iter()
        .any(|(_, entry)| entry.is_some_and(|entry| !at_target.contains(entry)))
    {
        let fork = repo.merge_base(target, head).map_err(|e| e.to_string())?;
        let mut history = first_parent_range(&repo, head, fork)?;
        history.push(fork);
        history.reverse();
        history
            .into_iter()
            .map(|oid| (oid, entries(&repo, oid, epic).unwrap_or_default()))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let arrived_by_merge = |entry: &LineAdoption| {
        history
            .iter()
            .find(|(_, entries)| entries.contains(entry))
            .is_some_and(|(first, _)| {
                repo.find_commit(*first)
                    .is_ok_and(|commit| commit.parent_count() >= 2)
            })
    };
    for (id, adoption) in direct {
        match adoption {
            Some(entry) if at_target.contains(entry) || arrived_by_merge(entry) => {
                report.adopted.push(entry.clone());
            }
            _ => report.unadopted.push(id),
        }
    }
    for entry in adoptions {
        if !range.iter().any(|oid| oid.to_string() == entry.commit) {
            report.outside_range.push(entry);
        }
    }
    Ok(report)
}
