//! Recorded adoption of direct commits on an epic's first-parent line
//! (SPC-013 R-52, amended 2026-10-06 by TSK-248).
//!
//! An epic line holds only classified landings. A non-merge commit already
//! shared on the line is accepted only when the epic record at the head
//! lists it under `line_adoptions` and that entry is itself landed work: the
//! target tip already lists it, or the first commit on the line's
//! first-parent history whose record holds the entry is a merge.
use crate::workgraph::history;

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

/// The `work.epic_line` finding for direct commits `unadopted` that `line`
/// would receive, as `(message, remedy)`. `reset` is the ref the remedy's
/// `git reset --keep` returns the line to. The pre-push hook and
/// `codeflow ci` share it, so both name the same commits and the same
/// remedy (TSK-1155).
#[must_use]
pub fn direct_commit_refusal(line: &str, unadopted: &[String], reset: &str) -> (String, String) {
    let epic = epic_id(line).unwrap_or("its epic");
    let ids = unadopted
        .iter()
        .map(|id| id.get(..9).unwrap_or(id))
        .collect::<Vec<_>>()
        .join(", ");
    (
        format!(
            "{} would receive a commit made directly on the line ({ids})",
            crate::git::display_text(line)
        ),
        format!(
            "land it by a pull request: save it on a task or planning branch, then `git reset --keep {reset}` and open a task or planning pull request to the line; a commit already on the remote is adopted by a `line_adoptions` entry in {epic} (see git-rules.md, \"Bodies of work\")"
        ),
    )
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

/// The epic record field that declares the epic line the epic's tasks land
/// on, when that line is another epic's (TSK-1286). Its value is the line's
/// exact logical name, `integration/EPC-NNN-<slug>`. TSK-1011 extends the
/// same field to a child line that starts from that line.
pub const PARENT_LINE_FIELD: &str = "parent_line";

/// Parse a record's `parent_line` value. Absent or null is no declaration;
/// anything else must be a string naming an epic line exactly, with no
/// `origin/` or `refs/` prefix, so one spelling binds one line.
///
/// # Errors
/// Returns why the value is not an epic line name.
pub(crate) fn parse_parent_line(value: Option<&Value>) -> Result<Option<String>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => {
            let id = epic_id(text).map_err(|reason| format!("{PARENT_LINE_FIELD}: {reason}"))?;
            // `epic_id` proved the text is `integration/<id>-...`.
            let slug = text
                .get("integration/".len() + id.len() + 1..)
                .unwrap_or_default();
            if slug.is_empty() {
                return Err(format!(
                    "{PARENT_LINE_FIELD}: '{text}' has no slug after '{id}-'; an epic line is integration/EPC-NNN-<slug>"
                ));
            }
            if !super::work_start::is_stable_work_target(text) {
                return Err(format!(
                    "{PARENT_LINE_FIELD}: '{text}' is not a valid branch name"
                ));
            }
            Ok(Some(text.clone()))
        }
        Some(_) => Err(format!(
            "{PARENT_LINE_FIELD} must be an epic line name (integration/EPC-NNN-<slug>) or null"
        )),
    }
}

/// Check an epic record's `parent_line` value against the epic's own id:
/// the shape `parse_parent_line` reads, and never the epic's own line.
///
/// # Errors
/// Returns why the declaration is not one the binding can use.
pub fn check_parent_line(own_epic: &str, value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(line) = parse_parent_line(value)? else {
        return Ok(None);
    };
    if epic_id(&line) == Ok(own_epic) {
        return Err(format!(
            "{PARENT_LINE_FIELD} names {own_epic}'s own line '{line}'; an epic lands on its own line without declaring it"
        ));
    }
    Ok(Some(line))
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
    let mut walk = history::revwalk(repo).map_err(|e| e.to_string())?;
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
/// Returns an error when the history or its adoption entries cannot be read.
/// Malformed head entries are reported in [`AdoptionReport::record_error`]. Direct commits
/// without a landed adoption are listed in [`AdoptionReport::unadopted`].
pub fn check_range(
    root: &Path,
    branch: &str,
    target: &str,
    base: &str,
    head: &str,
) -> Result<AdoptionReport, String> {
    let _judgment = history::judgment();
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
        let parents = history::parents(&repo, &repo.find_commit(*oid).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
            .len();
        if parents < 2 {
            let id = oid.to_string();
            direct.push((
                id.clone(),
                adoptions.iter().find(|entry| entry.commit == id),
            ));
        }
    }
    let at_target = if direct.iter().any(|(_, entry)| entry.is_some()) {
        entries(&repo, target, epic)?
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
        let fork = history::merge_base(&repo, target, head).map_err(|e| e.to_string())?;
        let mut history = first_parent_range(&repo, head, fork)?;
        history.push(fork);
        history.reverse();
        history
            .into_iter()
            .map(|oid| entries(&repo, oid, epic).map(|entries| (oid, entries)))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let arrived_by_merge = |entry: &LineAdoption| -> Result<bool, String> {
        let Some((first, _)) = history.iter().find(|(_, entries)| entries.contains(entry)) else {
            return Ok(false);
        };
        repo.find_commit(*first)
            .and_then(|commit| history::parents(&repo, &commit))
            .map(|parents| parents.len() >= 2)
            .map_err(|error| error.to_string())
    };
    for (id, adoption) in direct {
        match adoption {
            Some(entry) if at_target.contains(entry) || arrived_by_merge(entry)? => {
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

/// The commit `origin/<line>` names as fetched, or `None` when the line has
/// no remote-tracking ref. A local branch never counts: the fetched tip is
/// the one trusted authority for the line (TSK-1155).
///
/// # Errors
/// Returns an error when the repository or the ref cannot be read.
pub fn fetched_tip(root: &Path, line: &str) -> Result<Option<String>, String> {
    reference_tip(root, &format!("refs/remotes/origin/{line}"))
}

/// The commit the full reference `name` names, or `None` when it does not
/// exist.
///
/// # Errors
/// Returns an error when the repository or the reference cannot be read.
pub fn reference_tip(root: &Path, name: &str) -> Result<Option<String>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let tip = match repo.find_reference(name) {
        Ok(reference) => reference
            .peel_to_commit()
            .map(|commit| Some(commit.id().to_string()))
            .map_err(|e| format!("{name}: {e}")),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(format!("{name}: {error}")),
    };
    tip
}

/// The epic lines (`integration/EPC-*`) whose fetched tip is exactly
/// `commit`, sorted.
///
/// # Errors
/// Returns an error when the repository, `commit` or a fetched line cannot
/// be read.
pub fn fetched_lines_at(root: &Path, commit: &str) -> Result<Vec<String>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let commit = repo
        .revparse_single(commit)
        .and_then(|object| object.peel_to_commit())
        .map(|commit| commit.id())
        .map_err(|e| format!("{commit}: {e}"))?;
    let mut lines = Vec::new();
    let references = repo
        .references_glob("refs/remotes/origin/integration/*")
        .map_err(|e| format!("cannot list the fetched epic lines: {e}"))?;
    for reference in references {
        let reference = reference.map_err(|e| format!("cannot read a fetched epic line: {e}"))?;
        let full = crate::git::GitName::from_bytes(reference.name_bytes());
        let line = match full.rule_text() {
            Ok(text) => match text.strip_prefix("refs/remotes/origin/") {
                Some(name) if epic_id(name).is_ok() => Some(name.to_string()),
                _ => continue,
            },
            // A name that is not text cannot be read as a line: one at the
            // commit refuses rather than being skipped.
            Err(_) => None,
        };
        let tip = reference
            .peel_to_commit()
            .map_err(|e| format!("cannot read the tip of '{}': {e}", full.display()))?;
        if tip.id() != commit {
            continue;
        }
        match line {
            Some(name) => lines.push(name),
            None => {
                return Err(format!(
                    "a fetched line at {commit} has a name that is not UTF-8: '{}'",
                    full.display()
                ))
            }
        }
    }
    lines.sort();
    Ok(lines)
}

/// Whether `head` contains `tip` in its history, so the line could move
/// from `tip` to `head` without losing a commit.
///
/// # Errors
/// Returns an error when either revision or their history cannot be read.
pub fn descends_from(root: &Path, tip: &str, head: &str) -> Result<bool, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let resolve = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|e| format!("{revision}: {e}"))
    };
    let tip = resolve(tip)?;
    let head = resolve(head)?;
    Ok(history::merge_base(&repo, tip, head).map_err(|e| e.to_string())? == tip)
}
