//! Work record lifecycle: the one judge of task, epic and spec status
//! (SPC-013 R-26 to R-35, R-51, R-83).
//!
//! Every edit is judged by the same functions, whoever made it: a status verb
//! judges its proposed content against the file it replaces, `codeflow ci`
//! and `validate --since` judge each changed record between two trees, and
//! `validate --docs` judges the whole tree against the migration baseline.
//! A verb is a safe editor, not the only writer.
//!
//! Rules apply from the recorded migration baseline (`work_records_baseline`
//! in `.codeflow/project.toml`) and by transition. A record whose bytes equal
//! its copy at the baseline is exempt (the exception is bound to that blob,
//! never to the id); a text-only edit of an older record warns; a status
//! change, a criteria change, a changed acceptance block or a record created
//! after the baseline applies the rules in full. Relationship errors are never
//! grandfathered. Without a recorded baseline every record is judged in full.
//! The baseline counts only when it is an ancestor of the judged commit, and
//! a range whose base predates it judges older records from their baseline
//! blobs.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use git2::Repository;

use crate::git::GitName;
use crate::remedy::{self, Finding};

use super::record_text::{
    acceptance_blocks, check_acceptance, check_block, historical_acceptance, outcome_word,
    parse_blocker, parse_cancellation, parse_criteria, reopen_reasons, AcceptanceBlock,
    CriteriaList, Criterion, FencedAcceptance,
};
use super::work_start::{record_kind_for_tree_path, RecordKind};
use super::{is_legacy_task_format_id, is_valid_epic_format_id, is_valid_spec_format_id};

/// The project-config key naming the migration baseline commit.
pub const BASELINE_KEY: &str = "work_records_baseline";

/// A record as the lifecycle rules read it.
#[derive(Debug, Clone)]
pub struct RecordView {
    pub kind: RecordKind,
    pub id: String,
    /// Repository-relative path with `/` separators.
    pub path: String,
    pub content: String,
    pub status: String,
    pub epic_id: Option<String>,
    pub specs: Vec<String>,
    pub supersedes: Vec<String>,
    pub superseded_by: Option<String>,
    pub integration_target: Option<String>,
    /// A task's `role` frontmatter value (SPC-013 R-120:
    /// `release-integration`), when set.
    pub role: Option<String>,
    pub body: String,
    pub criteria: CriteriaList,
    /// A spec's `open_questions` frontmatter list (`None` when absent), or
    /// why it is unreadable.
    pub open_questions: Result<Option<Vec<String>>, String>,
}

impl RecordView {
    /// Parse one record's text.
    ///
    /// # Errors
    ///
    /// Returns a message when the frontmatter is missing or unreadable, or
    /// the record has no supported id.
    pub fn parse(kind: RecordKind, path: &str, content: &str) -> Result<Self, String> {
        let (data, body) = crate::validate::parse_frontmatter(content.as_bytes())
            .map_err(|error| error.to_string())?;
        let field = |name: &str| {
            let value = crate::validate::get_string_field(&data, name);
            (!value.is_empty()).then_some(value)
        };
        let valid = |id: &str| match kind {
            RecordKind::Task => super::is_valid_task_format_id(id),
            RecordKind::Epic => is_valid_epic_format_id(id),
            RecordKind::Spec => is_valid_spec_format_id(id),
        };
        let id = field("id")
            .filter(|id| valid(id))
            .or_else(|| field("format_id").filter(|id| valid(id)))
            .ok_or_else(|| "record has no supported id".to_string())?;
        let list = |name: &str| -> Vec<String> {
            data.get(name)
                .and_then(serde_yaml::Value::as_sequence)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_yaml::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let body = String::from_utf8_lossy(&body).into_owned();
        Ok(Self {
            kind,
            criteria: parse_criteria(&body),
            open_questions: crate::validate::open_questions(&data),
            status: field("status").unwrap_or_default(),
            epic_id: field("epic_id"),
            specs: list("specs"),
            supersedes: list("supersedes"),
            superseded_by: field("superseded_by"),
            integration_target: field("integration_target"),
            role: field("role"),
            id,
            path: path.to_string(),
            content: content.to_string(),
            body,
        })
    }

    fn blocks(&self) -> Vec<FencedAcceptance> {
        acceptance_blocks(&self.body)
    }

    pub(crate) fn active_blocks(&self) -> Vec<FencedAcceptance> {
        self.blocks()
            .into_iter()
            .filter(|block| !block.is_superseded())
            .collect()
    }

    pub(super) fn superseded_blocks(&self) -> Vec<FencedAcceptance> {
        self.blocks()
            .into_iter()
            .filter(FencedAcceptance::is_superseded)
            .collect()
    }

    /// Legacy `TSK-NNN-NNN` records are exempt from the criteria and
    /// acceptance rules (R-100).
    fn is_legacy(&self) -> bool {
        self.kind == RecordKind::Task && is_legacy_task_format_id(&self.id)
    }
}

/// Every record of one tree, by id.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub records: BTreeMap<String, RecordView>,
}

impl Graph {
    /// Read the checked-out records under `project-management/`.
    ///
    /// # Errors
    ///
    /// Returns an error if an inventory or record cannot be read, decoded or parsed.
    pub fn from_worktree(repo_root: &Path) -> Result<Self, String> {
        let pm = repo_root.join("project-management");
        let mut graph = Self::default();
        for (kind, files) in [
            (RecordKind::Epic, super::layout::epic_record_files(&pm)),
            (RecordKind::Spec, super::layout::spec_record_files(&pm)),
            (RecordKind::Task, super::layout::task_record_files(&pm)),
        ] {
            for path in files.map_err(|error| error.to_string())? {
                let content = std::fs::read_to_string(&path)
                    .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
                let relative =
                    crate::portable_path::slashed(path.strip_prefix(repo_root).unwrap_or(&path));
                graph.insert(kind, &relative, &content)?;
            }
        }
        Ok(graph)
    }

    /// Read the records of a commit's tree.
    ///
    /// # Errors
    ///
    /// Returns a message when the revision or its tree cannot be read.
    pub fn from_revision(repo: &Repository, revision: &str) -> Result<Self, String> {
        let commit = repo
            .revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| format!("cannot resolve {revision}: {}", error.message()))?;
        let tree = commit
            .tree()
            .map_err(|error| format!("cannot read the tree of {revision}: {}", error.message()))?;
        let mut graph = Self::default();
        let mut failure = None;
        crate::git::walk_tree(repo, &tree, &mut |name, entry| {
            // A record path is valid text; any other name is not a record.
            let Ok(path) = name.rule_text() else {
                return crate::git::Walk::Continue;
            };
            let Some(kind) = record_kind_for_tree_path(path) else {
                return crate::git::Walk::Continue;
            };
            match repo.find_blob(entry.id()) {
                Ok(blob) => {
                    let loaded = std::str::from_utf8(blob.content())
                        .map_err(|error| error.to_string())
                        .and_then(|content| graph.insert(kind, path, content));
                    match loaded {
                        Ok(()) => crate::git::Walk::Continue,
                        Err(error) => {
                            failure = Some(format!("{path}: {error}"));
                            crate::git::Walk::Stop
                        }
                    }
                }
                Err(error) => {
                    failure = Some(format!("{path}: {}", error.message()));
                    crate::git::Walk::Stop
                }
            }
        })
        .map_err(|error| error.message().to_string())?;
        match failure {
            Some(failure) => Err(failure),
            None => Ok(graph),
        }
    }

    /// Every discovered record must be readable and have an unambiguous identity.
    fn insert(&mut self, kind: RecordKind, path: &str, content: &str) -> Result<(), String> {
        let record =
            RecordView::parse(kind, path, content).map_err(|error| format!("{path}: {error}"))?;
        if self.records.contains_key(&record.id) {
            return Err(format!("{path}: duplicate work id {}", record.id));
        }
        self.records.insert(record.id.clone(), record);
        Ok(())
    }

    fn get(&self, id: &str, kind: RecordKind) -> Option<&RecordView> {
        self.records.get(id).filter(|record| record.kind == kind)
    }

    fn tasks(&self) -> impl Iterator<Item = &RecordView> {
        self.records
            .values()
            .filter(|record| record.kind == RecordKind::Task)
    }

    /// Consumers of a spec: each epic or task whose own `specs` list names it
    /// (R-51, R-65).
    fn consumers(&self, spec_id: &str) -> Vec<&RecordView> {
        self.records
            .values()
            .filter(|record| record.kind != RecordKind::Spec)
            .filter(|record| record.specs.iter().any(|spec| spec == spec_id))
            .collect()
    }

    /// A copy with one record replaced, for judging a proposed edit.
    #[must_use]
    pub fn with(&self, record: RecordView) -> Self {
        let mut graph = self.clone();
        graph.records.insert(record.id.clone(), record);
        graph
    }
}

/// The migration baseline as a judge can see it: one commit per line of work
/// (`work_records_baseline`, a list of full 40-character commit ids; a single
/// string is read as a one-item list). List order carries no meaning.
///
/// A record whose bytes equal its copy in any listed baseline is legacy.
/// Otherwise it is judged from its ancestry-latest copies: of the baselines
/// that hold it, any whose commit is an ancestor of another holder is
/// dropped, the edit is judged as a transition from each remaining copy, and
/// any refusal refuses it.
#[derive(Debug, Clone)]
pub enum Baseline {
    /// No baseline recorded: every record is judged in full.
    NotRecorded,
    /// Recorded, but these commits are not in this clone's history (a
    /// shallow checkout): findings on records the range did not add warn.
    Unavailable(Vec<String>),
    /// Recorded but unusable (not a full commit id, not a commit, or not an
    /// ancestor of the judged commit): every record is judged in full and
    /// each reason is an error.
    Refused(Vec<String>),
    Available {
        commits: Vec<String>,
        /// The records of each baseline, in list order.
        graphs: Vec<Graph>,
        /// `ancestor[i][j]`: baseline `i` is a proper ancestor of baseline `j`.
        ancestor: Vec<Vec<bool>>,
    },
}

impl Baseline {
    /// Load the baseline named in `.codeflow/project.toml` for the checked-out
    /// `HEAD` (the verbs and `validate --docs`).
    #[must_use]
    pub fn load(repo_root: &Path) -> Self {
        let entries = match recorded_baseline(repo_root) {
            Ok(entries) => entries,
            Err(error) => return Self::Refused(vec![error]),
        };
        let Ok(repo) = Repository::discover(repo_root) else {
            return if entries.is_empty() {
                Self::NotRecorded
            } else {
                Self::Unavailable(entries)
            };
        };
        let head = resolve_commit(&repo, "HEAD");
        Self::from_entries(&repo, &entries, head)
    }

    /// Resolve baseline entries for judging the commit `head`. An entry is
    /// read only as a full object id, never as a ref, tag, abbreviation or
    /// revision expression, so a snapshot cannot move while its string stays.
    fn from_entries(repo: &Repository, entries: &[String], head: Option<git2::Oid>) -> Self {
        if entries.is_empty() {
            return Self::NotRecorded;
        }
        let mut refused = Vec::new();
        let mut missing = Vec::new();
        let mut resolved: Vec<(String, git2::Oid)> = Vec::new();
        for entry in entries {
            let full = entry.len() == 40
                && entry
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            let Some(oid) = full.then(|| git2::Oid::from_str(entry).ok()).flatten() else {
                refused.push(format!(
                    "{BASELINE_KEY} entry `{entry}` is not a full 40-character lowercase commit id; tags, refs, abbreviations and revision expressions are refused"
                ));
                continue;
            };
            match repo.find_object(oid, None) {
                Err(_) => missing.push(entry.clone()),
                Ok(object) if object.kind() != Some(git2::ObjectType::Commit) => refused.push(format!(
                    "{BASELINE_KEY} entry {entry} is not a commit"
                )),
                Ok(_) if !head.is_some_and(|head| contains(repo, head, oid)) => refused.push(format!(
                    "{BASELINE_KEY} entry {entry} is not an ancestor of the commit being judged; every baseline must be a commit this history contains"
                )),
                Ok(_) => {
                    if !resolved.iter().any(|(_, seen)| *seen == oid) {
                        resolved.push((entry.clone(), oid));
                    }
                }
            }
        }
        if !refused.is_empty() {
            return Self::Refused(refused);
        }
        if !missing.is_empty() {
            return Self::Unavailable(missing);
        }
        let mut graphs = Vec::new();
        for (entry, oid) in &resolved {
            match Graph::from_revision(repo, &oid.to_string()) {
                Ok(graph) => graphs.push(graph),
                Err(_) => return Self::Unavailable(vec![entry.clone()]),
            }
        }
        let ancestor = resolved
            .iter()
            .map(|(_, older)| {
                resolved
                    .iter()
                    .map(|(_, newer)| older != newer && contains(repo, *newer, *older))
                    .collect()
            })
            .collect();
        Self::Available {
            commits: resolved.into_iter().map(|(entry, _)| entry).collect(),
            graphs,
            ancestor,
        }
    }

    /// The copies a record is judged from: those of the listed baselines
    /// holding it whose commit is not an ancestor of another holder.
    fn copies(&self, id: &str) -> Vec<&RecordView> {
        let Self::Available {
            graphs, ancestor, ..
        } = self
        else {
            return Vec::new();
        };
        let holders: Vec<usize> = (0..graphs.len())
            .filter(|&index| graphs[index].records.contains_key(id))
            .collect();
        holders
            .iter()
            .filter(|&&older| !holders.iter().any(|&newer| ancestor[older][newer]))
            .filter_map(|&index| graphs[index].records.get(id))
            .collect()
    }

    /// Whether the record's bytes equal its copy in any listed baseline,
    /// before or after `ids backfill` added its `uid` line. The backfill is
    /// part of the migration (R-3, R-25): it brings the record under the uid
    /// checks, which judge that line on their own (R-2), and leaves the
    /// exception bound to the rest of the blob (R-83).
    fn is_legacy_blob(&self, record: &RecordView) -> bool {
        match self {
            Self::Available { graphs, .. } => {
                let backfilled = without_backfilled_uid(&record.content);
                graphs.iter().any(|graph| {
                    graph.records.get(&record.id).is_some_and(|old| {
                        old.content == record.content
                            || backfilled.as_deref() == Some(old.content.as_str())
                    })
                })
            }
            _ => false,
        }
    }

    /// The refusals of an unusable baseline.
    #[must_use]
    pub fn errors(&self) -> Vec<String> {
        match self {
            Self::Refused(reasons) => reasons.clone(),
            _ => Vec::new(),
        }
    }

    /// Whether a baseline copy of the record is already complete: a task
    /// completed before the migration (R-83), whose Closeout may carry the
    /// historical form of R-101 instead of an acceptance block.
    fn completed_before(&self, record: &RecordView) -> bool {
        self.copies(&record.id)
            .iter()
            .any(|old| old.status == "complete")
    }

    /// Completed before the migration and reopened since. A reopen of a
    /// record without an acceptance block adds a `- reopened:` line (R-30),
    /// so more reopen lines than every complete baseline copy has means a
    /// reopen after the baseline: its re-completion applies the new rules
    /// in full (R-83).
    fn reopened_since(&self, record: &RecordView) -> bool {
        let reopens = reopen_reasons(&record.body).len();
        let copies: Vec<&RecordView> = self
            .copies(&record.id)
            .into_iter()
            .filter(|old| old.status == "complete")
            .collect();
        !copies.is_empty()
            && copies
                .iter()
                .all(|old| reopen_reasons(&old.body).len() < reopens)
    }

    /// Whether a record was created after every baseline.
    fn is_new(&self, record: &RecordView) -> bool {
        match self {
            Self::NotRecorded | Self::Refused(_) => true,
            Self::Unavailable(_) => false,
            Self::Available { .. } => self.copies(&record.id).is_empty(),
        }
    }

    /// The visible warning of a checkout that cannot see the baseline.
    fn warning(&self) -> Option<Finding> {
        match self {
            Self::Unavailable(commits) => Some(Finding::new(
                format!(
                    "work-records migration baseline {} is not in this clone's history; records the range did not add are judged leniently",
                    commits.join(", ")
                ),
                remedy::BASELINE_HISTORY.remedy(),
            )),
            _ => None,
        }
    }

    /// How fully the rules apply to a record that no transition touched.
    fn mode(&self, record: &RecordView) -> Mode {
        match self {
            Self::NotRecorded | Self::Refused(_) => Mode::Strict,
            Self::Unavailable(_) => Mode::Lenient,
            Self::Available { .. } if self.is_legacy_blob(record) => Mode::Exempt,
            Self::Available { .. } => {
                let copies = self.copies(&record.id);
                if !copies.is_empty() && copies.iter().all(|old| !significant_change(old, record)) {
                    Mode::Lenient
                } else {
                    Mode::Strict
                }
            }
        }
    }
}

/// `content` without the frontmatter `uid:` line `ids backfill` writes, or
/// `None` when its frontmatter has no such line.
pub(super) fn without_backfilled_uid(content: &str) -> Option<String> {
    let mut lines = content.split_inclusive('\n');
    let first = lines.next()?;
    if super::record_text::without_line_ending(first).trim_end_matches([' ', '\t']) != "---" {
        return None;
    }
    let mut out = String::from(first);
    let (mut removed, mut closed) = (false, false);
    for line in lines {
        if !closed {
            if super::record_text::without_line_ending(line).trim_end_matches([' ', '\t']) == "---"
            {
                closed = true;
            } else if !removed && line.starts_with("uid:") {
                removed = true;
                continue;
            }
        }
        out.push_str(line);
    }
    (removed && closed).then_some(out)
}

fn resolve_commit(repo: &Repository, revision: &str) -> Option<git2::Oid> {
    repo.revparse_single(revision)
        .and_then(|object| object.peel_to_commit())
        .map(|commit| commit.id())
        .ok()
}

/// Whether `commit` is `tip` or one of its ancestors.
fn contains(repo: &Repository, tip: git2::Oid, commit: git2::Oid) -> bool {
    tip == commit || repo.graph_descendant_of(tip, commit).unwrap_or(false)
}

/// The baseline entries of a parsed project config: a list, or a single
/// string read as a one-item list. Entries are exact strings; their form is
/// checked when they are resolved.
fn baseline_entries(config: Option<&toml::Value>) -> Result<Vec<String>, String> {
    let values: Vec<&str> = match config.and_then(|config| config.get(BASELINE_KEY)) {
        None => Vec::new(),
        Some(toml::Value::String(value)) => vec![value.as_str()],
        Some(toml::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .ok_or_else(|| format!("{BASELINE_KEY} entries must be strings"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => {
            return Err(format!(
                "{BASELINE_KEY} must be a string or an array of strings"
            ))
        }
    };
    let mut entries: Vec<String> = Vec::new();
    for value in values {
        if !entries.iter().any(|seen| seen == value) {
            entries.push(value.to_string());
        }
    }
    Ok(entries)
}

/// The recorded migration baseline entries of the checked-out tree; empty
/// when none.
///
/// # Errors
///
/// Refuses an unreadable, non-UTF-8 or malformed project file, or a baseline
/// field that is not a string or list of strings. Only a missing file is absent.
pub fn recorded_baseline(repo_root: &Path) -> Result<Vec<String>, String> {
    let path = repo_root.join(".codeflow/project.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let config = text
        .parse::<toml::Value>()
        .map_err(|error| error.to_string())?;
    baseline_entries(Some(&config))
}

/// The baseline entries recorded in a commit's `.codeflow/project.toml`.
fn baseline_at(repo: &Repository, commit: git2::Oid) -> Result<Vec<String>, String> {
    let tree = repo
        .find_commit(commit)
        .and_then(|commit| commit.tree())
        .map_err(|error| error.to_string())?;
    let entry = match tree.get_path(Path::new(".codeflow/project.toml")) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let blob = repo
        .find_blob(entry.id())
        .map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(blob.content()).map_err(|error| error.to_string())?;
    let config = text
        .parse::<toml::Value>()
        .map_err(|error| error.to_string())?;
    baseline_entries(Some(&config))
}

/// The baseline that governs a range, and the notices about it. Trust comes
/// from the target (the pull request's base tip, or the `--since` revision),
/// never from the merge-base: when the target records a baseline, that list
/// exempts records and a change to it inside the range only takes effect
/// after it lands. A target list that cannot be evaluated against the
/// proposed history is refused, never replaced by the head's list. When the
/// target records none, the range introduces the migration (an adopter's
/// first update, or a release into a target that predates the rules); the
/// head's list governs, every entry must be an ancestor of the head, and
/// every entry is named for the human reviewer.
fn range_baseline(
    repo_root: &Path,
    repo: &Repository,
    target: git2::Oid,
    head: Option<&str>,
) -> (Baseline, Vec<Finding>) {
    let head_commit = resolve_commit(repo, head.unwrap_or("HEAD"));
    let refuse = |error: String| {
        (
            Baseline::Refused(vec![format!("cannot read migration baseline: {error}")]),
            Vec::new(),
        )
    };
    let base_list = match baseline_at(repo, target) {
        Ok(entries) => entries,
        Err(error) => return refuse(error),
    };
    let head_list = match head {
        Some(_) => match head_commit
            .map(|commit| baseline_at(repo, commit))
            .transpose()
        {
            Ok(entries) => entries.unwrap_or_default(),
            Err(error) => return refuse(error),
        },
        None => match recorded_baseline(repo_root) {
            Ok(entries) => entries,
            Err(error) => return refuse(error),
        },
    };
    let mut notices = Vec::new();
    if base_list.is_empty() {
        if !head_list.is_empty() {
            notices.push(Finding::new(
                format!(
                    "this change introduces {BASELINE_KEY}; every record unchanged from these snapshots is legacy, so the human reviewer approves each: {}",
                    head_list.join(", ")
                ),
                remedy::BASELINE_REVIEW.remedy(),
            ));
        }
        return (
            Baseline::from_entries(repo, &head_list, head_commit),
            notices,
        );
    }
    if head_list != base_list {
        let added: Vec<&str> = head_list
            .iter()
            .filter(|e| !base_list.contains(e))
            .map(String::as_str)
            .collect();
        let removed: Vec<&str> = base_list
            .iter()
            .filter(|e| !head_list.contains(e))
            .map(String::as_str)
            .collect();
        let describe = |entries: &[&str]| {
            if entries.is_empty() {
                "none".to_string()
            } else {
                entries.join(", ")
            }
        };
        notices.push(Finding::new(
            format!(
                "this change edits {BASELINE_KEY} (added {}; removed {}{}); the target's list judges this change, and the edit takes effect after it lands",
                describe(&added),
                describe(&removed),
                if added.is_empty() && removed.is_empty() { "; reordered" } else { "" }
            ),
            remedy::BASELINE_REVIEW.remedy(),
        ));
    }
    let baseline = match Baseline::from_entries(repo, &base_list, head_commit) {
        Baseline::Refused(reasons) => Baseline::Refused(
            reasons
                .into_iter()
                .map(|reason| {
                    let hint = if reason.contains("not an ancestor") {
                        "; bring the branch up to date with its target"
                    } else {
                        ""
                    };
                    format!("the target's {reason}{hint}")
                })
                .collect(),
        ),
        governed => governed,
    };
    (baseline, notices)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Bound to the exact baseline blob: the new rules are not applied.
    Exempt,
    /// An edit that is not a transition: findings warn.
    Lenient,
    /// Findings block.
    Strict,
}

/// The findings of one judgement.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    pub errors: Vec<String>,
    /// Warnings, each with the step that clears it (R-80).
    pub warnings: Vec<Finding>,
    /// Facts a reviewer should see that are neither errors nor warnings,
    /// such as a change to the baseline list.
    pub notices: Vec<Finding>,
}

impl Verdict {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    fn apply(&mut self, mode: Mode, path: &str, problems: Vec<String>) {
        let problems = problems
            .into_iter()
            .map(|problem| format!("{path}: {problem}"));
        match mode {
            Mode::Exempt => {}
            Mode::Lenient => self.warnings.extend(problems.map(|problem| {
                Finding::new(
                    problem,
                    remedy::RECORD_BASELINE_EXEMPT.with(&[("path", path)]),
                )
            })),
            Mode::Strict => self.errors.extend(problems),
        }
    }

    fn sort(&mut self) {
        self.errors.sort();
        self.errors.dedup();
        self.warnings.sort();
        self.warnings.dedup();
        self.notices.sort();
        self.notices.dedup();
    }
}

/// What a judge knows about the change beyond the two record states: the
/// tree it started from and the paths it touches. `None` means unknown, and
/// the rules that need it are left to the judge that knows it (CI and
/// `validate --since` know both).
#[derive(Debug, Clone, Copy, Default)]
pub struct ChangeContext<'a> {
    pub base: Option<&'a Graph>,
    pub changed_paths: Option<&'a [String]>,
    /// Records whose status left `complete` at some commit of the range,
    /// judged per commit so a reopen and re-completion inside one range is
    /// still seen (R-83).
    pub reopened: Option<&'a BTreeSet<String>>,
    /// On a release range, the records it brings from verified lines, each
    /// judged where it was introduced there (SPC-013 R-120).
    pub brought: Option<&'a Brought>,
    /// Specs implemented at some earlier commit of the base's history, so
    /// frozen though a later supersession or reopen left them open at the
    /// base (TSK-169).
    pub shipped: Option<&'a BTreeSet<String>>,
}

/// A release range's brought records, each judged where it was introduced
/// on its line instead of across the whole range (SPC-013 R-120, TSK-140
/// AC-11 to AC-13). Only a record every change of which the range brings
/// from a verified line is listed; a record the range also changes
/// directly is judged in full.
#[derive(Debug, Clone, Default)]
pub struct Brought {
    /// Records whose only change is the `uid` line `ids backfill` writes,
    /// landed on a line: the backfill was judged there, and the uid checks
    /// judge the line on their own.
    pub backfills: BTreeSet<String>,
    /// Specs brought `approved` or `superseded`: the problem of the landing
    /// that made the change on the line, `None` when it was planning-only.
    pub approvals: BTreeMap<String, Option<String>>,
    /// Complete tasks without an acceptance block whose record last changed
    /// on their line at or before its records cutoff: the notice that lists
    /// each as information.
    pub legacy: BTreeMap<String, String>,
}

/// The finding a legacy record's notice replaces (TSK-140 AC-13).
const NO_BLOCK: &str = "a complete record needs an acceptance block in its Closeout";

/// A change that re-applies the rules in full (R-83): a status change, a
/// criteria change or a changed acceptance block.
fn significant_change(before: &RecordView, after: &RecordView) -> bool {
    before.status != after.status
        || before.criteria.signature() != after.criteria.signature()
        || before
            .blocks()
            .iter()
            .map(|block| block.inner.clone())
            .collect::<Vec<_>>()
            != after
                .blocks()
                .iter()
                .map(|block| block.inner.clone())
                .collect::<Vec<_>>()
}

// ---------------------------------------------------------------------------
// Transitions
// ---------------------------------------------------------------------------

const TASK_TERMINAL: [&str; 2] = ["complete", "cancelled"];
const EPIC_OPEN: [&str; 4] = ["draft", "planning", "in_progress", "blocked"];

fn is_terminal(record: &RecordView, status: &str) -> bool {
    match record.kind {
        RecordKind::Task => TASK_TERMINAL.contains(&status),
        RecordKind::Epic => matches!(status, "complete" | "cancelled" | "archived"),
        RecordKind::Spec => false,
    }
}

/// At most one open task owns direct release work (SPC-013 R-120): a
/// second open `role: release-integration` holder is refused, naming the
/// other.
fn release_role_problem(record: &RecordView, graph: &Graph) -> Option<String> {
    let open = |task: &RecordView| {
        task.role.as_deref() == Some(super::release_line::RELEASE_ROLE)
            && !TASK_TERMINAL.contains(&task.status.as_str())
    };
    if !open(record) {
        return None;
    }
    let others: Vec<&str> = graph
        .tasks()
        .filter(|task| task.id != record.id && open(task))
        .map(|task| task.id.as_str())
        .collect();
    (!others.is_empty()).then(|| {
        format!(
            "{} and {} are both open with `role: {}`; one open task owns direct release work",
            record.id,
            others.join(", "),
            super::release_line::RELEASE_ROLE
        )
    })
}

/// A new completion of an already complete task must retain its old review.
/// An equal active block still counts when the archived history changes.
pub(super) fn is_recompletion(before: Option<&RecordView>, after: &RecordView) -> bool {
    let blocks = |record: &RecordView| {
        record
            .superseded_blocks()
            .into_iter()
            .map(|block| block.inner)
            .collect::<Vec<_>>()
    };
    after.kind == RecordKind::Task
        && after.status == "complete"
        && before.is_some_and(|old| {
            old.status == "complete"
                && (blocks(old) != blocks(after)
                    || reopen_reasons(&old.body) != reopen_reasons(&after.body))
        })
}

/// Whether moving `before` to `after` is a legal transition (R-30, R-32).
fn transition_problems(before: Option<&RecordView>, after: &RecordView) -> Vec<String> {
    let to = after.status.as_str();
    let from = before.map_or(
        match after.kind {
            RecordKind::Task => "todo",
            RecordKind::Epic | RecordKind::Spec => "draft",
        },
        |record| record.status.as_str(),
    );
    if from == to {
        return if is_recompletion(before, after) {
            reopen_problems(before, after)
        } else {
            Vec::new()
        };
    }
    let allowed = match after.kind {
        RecordKind::Task => task_transition_allowed(from, to),
        RecordKind::Epic => epic_transition_allowed(from, to),
        RecordKind::Spec => spec_transition_allowed(from, to),
    };
    let mut problems = match allowed {
        Ok(()) => Vec::new(),
        Err(reason) => vec![format!("status {from} -> {to} is refused: {reason}")],
    };
    if after.kind == RecordKind::Task && from == "complete" && to == "todo" {
        problems.extend(reopen_problems(before, after));
    }
    // Approval attests that the questions were reviewed and settled, so it
    // reads the structured list and never infers it from a missing field
    // (TSK-135). A spec approved before the field existed stays readable.
    if after.kind == RecordKind::Spec
        && to == "approved"
        && matches!(after.open_questions, Ok(None))
    {
        problems.push(
            "approving a spec needs its `open_questions` frontmatter list: resolve the questions the `## Open questions` prose still raises, then add `open_questions: []` (or list any that stay open)"
                .into(),
        );
    }
    problems
}

fn task_transition_allowed(from: &str, to: &str) -> Result<(), &'static str> {
    // `in_progress` stays readable; it is the legacy spelling of `todo`.
    let from = if from == "in_progress" { "todo" } else { from };
    match (from, to) {
        (_, "in_progress") => {
            Err("in_progress is no longer written; active work is derived from the branch")
        }
        ("todo", "todo" | "blocked" | "complete" | "cancelled")
        | ("blocked", "todo" | "cancelled")
        | ("complete", "todo") => Ok(()),
        ("cancelled", _) => Err("a cancelled task is terminal"),
        ("complete", _) => Err("a complete task only reopens to todo"),
        ("blocked", "complete") => Err("unblock to todo before completing"),
        _ => Err("not a task transition"),
    }
}

fn epic_transition_allowed(from: &str, to: &str) -> Result<(), &'static str> {
    let open = |status: &str| EPIC_OPEN.contains(&status);
    match (from, to) {
        (from, to) if open(from) && (open(to) || matches!(to, "complete" | "cancelled")) => Ok(()),
        ("complete" | "cancelled", "archived") => Ok(()),
        ("archived", _) => Err("an archived epic is final"),
        ("complete" | "cancelled", _) => Err("a closed epic only moves to archived"),
        _ => Err("not an epic transition"),
    }
}

fn spec_transition_allowed(from: &str, to: &str) -> Result<(), &'static str> {
    match (from, to) {
        ("draft", "approved") | ("approved" | "implemented", "superseded") => Ok(()),
        (_, "implemented") => Err("implemented is derived from the consumers and never written"),
        ("approved" | "implemented", "draft") => {
            Err("approved never returns to draft: amend it in place in a planning change until it is implemented; once it is implemented, a changed contract is a new spec")
        }
        ("draft", "superseded") => Err("only an approved spec is superseded"),
        ("superseded", _) => Err("a superseded spec is final"),
        _ => Err("not a spec transition"),
    }
}

/// Reopening keeps the old acceptance block, marked superseded with the
/// reopen reason (R-30). A task completed before the migration has no block
/// to keep; it records the reason as a Closeout line `- reopened: <reason>`
/// and no block is invented.
pub(super) fn reopen_problems(before: Option<&RecordView>, after: &RecordView) -> Vec<String> {
    let mut problems = Vec::new();
    if after.status != "complete" && !after.active_blocks().is_empty() {
        problems
            .push("a reopened task keeps no active acceptance block; mark it superseded".into());
    }
    if after.status == "complete"
        && before.is_some_and(|old| old.criteria.signature() != after.criteria.signature())
    {
        problems.push(super::acceptance::REOPENED_CRITERIA.into());
    }
    if before.is_some_and(|record| record.active_blocks().is_empty()) {
        let old = before.map_or(0, |record| reopen_reasons(&record.body).len());
        if reopen_reasons(&after.body).len() <= old {
            problems.push(
                "reopening a task completed without an acceptance block needs a Closeout line `- reopened: <reason>`"
                    .into(),
            );
        }
        return problems;
    }
    let old_count = before.map_or(0, |record| record.superseded_blocks().len());
    let superseded = after.superseded_blocks();
    let old_active = before.and_then(|record| record.active_blocks().into_iter().next());
    let kept = superseded.len() > old_count
        && superseded.last().is_some_and(|block| {
            block.parsed.is_ok()
                && old_active
                    .as_ref()
                    .is_none_or(|old| same_block_content(old, block))
        });
    if !kept {
        problems.push(
            "reopening needs the old acceptance block kept under `acceptance_superseded:` with the reopen reason"
                .into(),
        );
    }
    problems
}

fn same_block_content(old: &FencedAcceptance, new: &FencedAcceptance) -> bool {
    match (&old.parsed, &new.parsed) {
        (Ok(old), Ok(new)) => {
            let mut new = new.clone();
            new.superseded = false;
            new.reason = None;
            *old == new
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// State rules
// ---------------------------------------------------------------------------

/// What the record's current status requires (R-30, R-32, R-33, R-50, R-60).
///
/// `historical` says the record was already complete at the migration
/// baseline and has had no transition or criteria change since (R-83).
fn state_problems(
    record: &RecordView,
    graph: &Graph,
    is_new: bool,
    historical: bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    if !record.is_legacy() && record.kind != RecordKind::Spec {
        problems.extend(record.criteria.errors.iter().cloned());
        if is_new && record.criteria.uses_checkboxes() {
            problems.push(
                "new records list criteria as `- AC-n <criterion>` without a checkbox".into(),
            );
        }
    }
    match record.kind {
        RecordKind::Task => problems.extend(task_state_problems(record, graph, historical)),
        RecordKind::Epic => problems.extend(epic_state_problems(record, graph)),
        RecordKind::Spec => {
            match &record.open_questions {
                Err(message) => problems.push(message.clone()),
                Ok(Some(open)) if record.status == "approved" && !open.is_empty() => {
                    problems.push(format!(
                        "an approved spec leaves no open question; still open: {}",
                        open.join("; ")
                    ));
                }
                Ok(_) => {}
            }
            if record.status == "implemented" && is_new {
                problems.push(
                    "`implemented` is derived and is refused as a written value on a spec created after the migration baseline"
                        .into(),
                );
            }
        }
    }
    problems
}

fn task_state_problems(task: &RecordView, graph: &Graph, historical: bool) -> Vec<String> {
    let mut problems = Vec::new();
    match task.status.as_str() {
        "blocked" => match parse_blocker(&task.body) {
            None => problems
                .push("a blocked task needs a `## Blocker` with reason, owner and revisit".into()),
            Some(blocker) => {
                let missing = blocker.missing();
                if !missing.is_empty() {
                    problems.push(format!("the Blocker is missing {}", missing.join(", ")));
                }
            }
        },
        "cancelled" => problems.extend(cancellation_problems(task)),
        "complete" if !task.is_legacy() => {
            problems.extend(completion_problems(task, historical));
            for (spec, successor) in superseded_specs(task, graph) {
                problems.push(format!(
                    "acceptance cannot cite superseded spec {spec}; move the task to {successor}"
                ));
            }
        }
        _ => {}
    }
    for block in task.superseded_blocks() {
        if let Err(error) = &block.parsed {
            problems.push(format!("superseded acceptance block: {error}"));
        }
    }
    problems
}

fn cancellation_problems(record: &RecordView) -> Vec<String> {
    let missing = parse_cancellation(&record.body).missing();
    if missing.is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "a cancelled record needs Closeout lines `- cancelled: <reason>` and `- scope: <where the scope went>` (missing {})",
            missing.join(", ")
        )]
    }
}

/// A complete task carries one active acceptance block that passes the
/// structural rules of R-60. A task completed before the migration baseline
/// may instead carry the historical form of R-101: its evidence is gone, so
/// the Closeout says so and names the landing merge, and nothing is
/// reconstructed.
fn completion_problems(record: &RecordView, historical: bool) -> Vec<String> {
    let active = record.active_blocks();
    match active.as_slice() {
        [] if historical && historical_acceptance(&record.body).is_some() => Vec::new(),
        [] if historical => vec![
            "a complete record needs an acceptance block in its Closeout, or, completed before the migration baseline, an item `- acceptance: historical evidence unavailable; ...` that names the landing merge as merge `<sha>`"
                .into(),
        ],
        [] => vec!["a complete record needs an acceptance block in its Closeout".into()],
        [block] => match &block.parsed {
            Ok(parsed) => check_acceptance(parsed, &record.criteria.items),
            Err(error) => vec![error.to_string()],
        },
        _ => vec!["one acceptance block per completion; mark older blocks superseded".into()],
    }
}

fn consumed_specs(task: &RecordView, graph: &Graph) -> Vec<String> {
    let mut specs = task
        .epic_id
        .as_deref()
        .and_then(|epic| graph.get(epic, RecordKind::Epic))
        .map_or_else(Vec::new, |epic| epic.specs.clone());
    for spec in &task.specs {
        if !specs.contains(spec) {
            specs.push(spec.clone());
        }
    }
    specs
}

/// The consumed specs of a task that are superseded, with their successor.
/// Acceptance that cites one is invalidated (R-32) until the task is
/// reconciled with the successor.
fn superseded_specs(task: &RecordView, graph: &Graph) -> Vec<(String, String)> {
    consumed_specs(task, graph)
        .into_iter()
        .filter_map(|spec| {
            let record = graph.get(&spec, RecordKind::Spec)?;
            (record.status == "superseded").then(|| {
                let successor = record
                    .superseded_by
                    .clone()
                    .unwrap_or_else(|| "its successor".into());
                (spec, successor)
            })
        })
        .collect()
}

fn epic_state_problems(epic: &RecordView, graph: &Graph) -> Vec<String> {
    match epic.status.as_str() {
        "complete" => epic_close_problems(epic, graph),
        "cancelled" => cancellation_problems(epic),
        _ => Vec::new(),
    }
}

/// R-33: every task terminal, every epic criterion verified (on the journey
/// for a journey criterion), every consumed spec implemented in the
/// candidate state or still consumed by another open consumer.
fn epic_close_problems(epic: &RecordView, graph: &Graph) -> Vec<String> {
    let mut problems = Vec::new();
    let open: Vec<&str> = graph
        .tasks()
        .filter(|task| task.epic_id.as_deref() == Some(epic.id.as_str()))
        .filter(|task| !is_terminal(task, &task.status))
        .map(|task| task.id.as_str())
        .collect();
    if !open.is_empty() {
        problems.push(format!(
            "epic close needs every task terminal; open: {}",
            open.join(", ")
        ));
    }
    let own_block = epic_own_block(epic, graph, &mut problems);
    for criterion in &epic.criteria.items {
        if let Err(reason) = epic_criterion_verified(epic, criterion, graph, own_block.as_ref()) {
            problems.push(format!(
                "epic criterion {} is unverified: {reason}",
                criterion.id
            ));
        }
    }
    for spec_id in &epic.specs {
        let Some(spec) = graph.get(spec_id, RecordKind::Spec) else {
            continue;
        };
        if derived_spec_state(spec, graph, Some(&epic.id)) == SpecState::Implemented {
            continue;
        }
        let other_open = graph
            .consumers(spec_id)
            .into_iter()
            .any(|consumer| consumer.id != epic.id && !is_terminal(consumer, &consumer.status));
        if !other_open {
            problems.push(format!(
                "consumed spec {spec_id} would not be implemented and has no other open consumer"
            ));
        }
    }
    problems
}

fn serving_tasks<'g>(
    epic: &RecordView,
    criterion: &Criterion,
    graph: &'g Graph,
) -> Vec<(&'g RecordView, &'g Criterion)> {
    graph
        .tasks()
        .flat_map(|task| task.criteria.items.iter().map(move |item| (task, item)))
        .filter(|(_, item)| item.serves() == Some((epic.id.clone(), criterion.id.clone())))
        .collect()
}

/// A legacy ticked checkbox still verifies an ordinary criterion; a journey
/// criterion always needs journey evidence.
fn ticked_ordinary(criterion: &Criterion) -> bool {
    criterion.checkbox == Some(true) && !criterion.is_journey()
}

/// Whether every task serving `criterion` is cancelled. A cancelled child
/// never satisfies the epic's outcome (R-33), so such a criterion is proven
/// by the epic's own acceptance block, as an unserved one is.
fn served_only_by_cancelled(epic: &RecordView, criterion: &Criterion, graph: &Graph) -> bool {
    let serving = serving_tasks(epic, criterion, graph);
    !serving.is_empty() && serving.iter().all(|(task, _)| task.status == "cancelled")
}

/// The epic's own acceptance block, when it is the one active block and it
/// passes the structural rules of R-60 for the criteria it must prove: those
/// no task serves, unless a legacy tick verifies them, and those served only
/// by cancelled tasks, which a tick never verifies. Problems with it are
/// reported, and an invalid block proves nothing.
fn epic_own_block(
    epic: &RecordView,
    graph: &Graph,
    problems: &mut Vec<String>,
) -> Option<AcceptanceBlock> {
    let required: Vec<Criterion> = epic
        .criteria
        .items
        .iter()
        .filter(|criterion| {
            served_only_by_cancelled(epic, criterion, graph)
                || (serving_tasks(epic, criterion, graph).is_empty() && !ticked_ordinary(criterion))
        })
        .cloned()
        .collect();
    let active = epic.active_blocks();
    let block = match active.as_slice() {
        [] => return None,
        [block] => block,
        _ => {
            problems
                .push("one acceptance block per completion; mark older blocks superseded".into());
            return None;
        }
    };
    match &block.parsed {
        Ok(parsed) => {
            let issues = check_block(parsed, &required, &epic.criteria.items);
            if issues.is_empty() {
                Some(parsed.clone())
            } else {
                problems.extend(
                    issues
                        .into_iter()
                        .map(|issue| format!("epic acceptance block: {issue}")),
                );
                None
            }
        }
        Err(error) => {
            problems.push(error.to_string());
            None
        }
    }
}

fn epic_criterion_verified(
    epic: &RecordView,
    criterion: &Criterion,
    graph: &Graph,
    own_block: Option<&AcceptanceBlock>,
) -> Result<(), String> {
    let serving = serving_tasks(epic, criterion, graph);
    if !serving.is_empty() {
        if serving.iter().all(|(task, _)| task.status == "cancelled") {
            // A valid own block was checked against every criterion served
            // only by cancelled tasks; cancelling a task proves nothing.
            return if own_block.is_some() {
                Ok(())
            } else {
                Err("served only by cancelled tasks, and the epic has no valid acceptance block verifying it".into())
            };
        }
        let verified = serving.iter().any(|(task, item)| {
            task.status == "complete"
                && superseded_specs(task, graph).is_empty()
                && task.active_blocks().first().is_some_and(|block| {
                    block.parsed.as_ref().is_ok_and(|parsed| {
                        let result_ok = parsed.criteria.iter().any(|(id, result)| {
                            *id == item.id
                                && matches!(result.outcome.as_str(), "verified" | "waived")
                        });
                        let journey_ok =
                            !criterion.is_journey() || outcome_word(&parsed.journey) == "verified";
                        result_ok && journey_ok
                    })
                })
        });
        return if verified {
            Ok(())
        } else if serving
            .iter()
            .any(|(task, _)| task.status == "complete" && !superseded_specs(task, graph).is_empty())
        {
            Err("its serving task's acceptance cites a superseded spec; reconcile the task with the successor".into())
        } else {
            Err("no complete serving task verified it".into())
        };
    }
    if ticked_ordinary(criterion) || own_block.is_some() {
        // A valid own block was checked against every unserved criterion.
        return Ok(());
    }
    Err(if criterion.is_journey() {
        "no task serves it and the epic has no valid acceptance block verifying the journey".into()
    } else {
        "no task serves it and the epic has no valid acceptance block".into()
    })
}

/// The derived spec state of R-51.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecState {
    /// Approved, every consumer terminal and at least one complete.
    Implemented,
    /// No consumer, or every consumer cancelled.
    NoDeliveringConsumer,
    Open,
}

fn derived_spec_state(spec: &RecordView, graph: &Graph, closing_epic: Option<&str>) -> SpecState {
    let consumers = graph.consumers(&spec.id);
    let status = |consumer: &RecordView| -> String {
        if Some(consumer.id.as_str()) == closing_epic {
            "complete".to_string()
        } else {
            consumer.status.clone()
        }
    };
    if consumers
        .iter()
        .all(|consumer| status(consumer) == "cancelled")
    {
        return SpecState::NoDeliveringConsumer;
    }
    let all_terminal = consumers
        .iter()
        .all(|consumer| is_terminal(consumer, &status(consumer)));
    let any_complete = consumers
        .iter()
        .any(|consumer| status(consumer) == "complete");
    if matches!(spec.status.as_str(), "approved" | "implemented") && all_terminal && any_complete {
        SpecState::Implemented
    } else {
        SpecState::Open
    }
}

/// The derived state of a spec in a graph (R-51).
#[must_use]
pub fn spec_state(graph: &Graph, spec_id: &str) -> Option<SpecState> {
    graph
        .get(spec_id, RecordKind::Spec)
        .map(|spec| derived_spec_state(spec, graph, None))
}

/// Supersession links (R-32). Relationship errors are never grandfathered.
fn relationship_problems(record: &RecordView, graph: &Graph) -> Vec<String> {
    let mut problems = Vec::new();
    if record.kind == RecordKind::Task {
        problems.extend(release_role_problem(record, graph));
    }
    if record.kind != RecordKind::Spec {
        return problems;
    }
    if record.supersedes.contains(&record.id) {
        problems.push("a spec cannot supersede itself".into());
    }
    if record.superseded_by.as_deref() == Some(record.id.as_str()) {
        problems
            .push("a spec cannot be superseded by itself; a shipped contract is not reopened; a new spec carries changes".into());
    } else if supersession_cycle(record, graph) {
        problems.push(
            "the `superseded_by` chain returns to this spec; supersession cannot cycle".into(),
        );
    }
    for old in &record.supersedes {
        match graph.get(old, RecordKind::Spec) {
            None => problems.push(format!("supersedes missing spec {old}")),
            Some(previous) => {
                if previous.superseded_by.as_deref() != Some(record.id.as_str())
                    || previous.status != "superseded"
                {
                    problems.push(format!(
                        "supersedes {old}, which must be `superseded` with `superseded_by: {}` in the same change",
                        record.id
                    ));
                }
            }
        }
    }
    match (&record.superseded_by, record.status.as_str()) {
        (None, "superseded") => {
            problems.push("a superseded spec names its successor in `superseded_by`".into());
        }
        (Some(next), status) => {
            if status != "superseded" {
                problems.push(format!("`superseded_by` is set but status is {status}"));
            }
            match graph.get(next, RecordKind::Spec) {
                None => problems.push(format!("superseded_by names missing spec {next}")),
                Some(successor) if !successor.supersedes.contains(&record.id) => {
                    problems.push(format!(
                        "successor {next} does not list `supersedes: [{}]`",
                        record.id
                    ));
                }
                Some(_) => {}
            }
        }
        (None, _) => {}
    }
    problems
}

/// Whether following `superseded_by` from `record` returns to it.
fn supersession_cycle(record: &RecordView, graph: &Graph) -> bool {
    let mut next = record.superseded_by.clone();
    for _ in 0..=graph.records.len() {
        match next {
            Some(id) if id == record.id => return true,
            Some(id) => {
                next = graph
                    .get(&id, RecordKind::Spec)
                    .and_then(|spec| spec.superseded_by.clone());
            }
            None => return false,
        }
    }
    false
}

/// The rules of R-32 that need the change itself: approval and supersession
/// travel in a planning-only change, and supersession adds its successor.
fn context_problems(
    before: Option<&RecordView>,
    after: &RecordView,
    context: ChangeContext<'_>,
) -> Vec<String> {
    let mut problems = Vec::new();
    let to = after.status.as_str();
    let moved = before.is_none_or(|record| record.status != to);
    if after.kind != RecordKind::Spec || !moved || !matches!(to, "approved" | "superseded") {
        return problems;
    }
    if let Some(landing) = context
        .brought
        .and_then(|brought| brought.approvals.get(&after.id))
    {
        // Brought from a line: judged where it landed there.
        problems.extend(landing.iter().cloned());
    } else if let Some(paths) = context.changed_paths {
        let product: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|path| !super::classify::is_planning_path(path))
            .collect();
        if !product.is_empty() {
            problems.push(format!(
                "a spec becomes {to} only in a planning-only change (project-management/ and docs/plan/); this change also touches {}",
                product
                    .iter()
                    .map(|path| crate::git::display_key(path))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if to == "superseded" {
        if let (Some(base), Some(successor)) = (context.base, after.superseded_by.as_deref()) {
            if base.records.contains_key(successor) {
                problems.push(format!(
                    "the successor {successor} already existed before this change; supersession adds a new spec revision in the same change"
                ));
            }
        }
    }
    problems
}

/// Whether two record texts say the same thing. Line endings do not count:
/// a Windows checkout with `core.autocrlf` writes CRLF where the commit
/// holds LF, and that changes no word of a contract.
fn same_text(a: &str, b: &str) -> bool {
    a == b || a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

/// Whether `spec` is implemented in `graph`: written `implemented` by a
/// legacy record, or derived from its consumers (R-51).
fn implemented_in(spec: &RecordView, graph: &Graph) -> bool {
    spec.status == "implemented" || derived_spec_state(spec, graph, None) == SpecState::Implemented
}

/// A spec that shipped is frozen (TSK-169): until it is implemented an
/// approved spec is amended in place; after that its text never changes.
/// It shipped when it is implemented in the tree before the change, so
/// reopening a consumer in the same change does not thaw it, or when a
/// judge that read its history found it implemented at an earlier commit
/// (`context.shipped`), so a later supersession or reopen does not either.
/// Only the text below the frontmatter is compared: status, supersession
/// links and a `uid` backfill are judged by their own rules.
fn frozen_spec_problems(
    before: Option<&RecordView>,
    after: &RecordView,
    context: ChangeContext<'_>,
) -> Vec<String> {
    let Some(before) = before.filter(|record| record.kind == RecordKind::Spec) else {
        return Vec::new();
    };
    if same_text(&before.body, &after.body) {
        return Vec::new();
    }
    let at_base = before.status == "implemented"
        || context.base.is_some_and(|base| {
            base.get(&before.id, RecordKind::Spec)
                .is_some_and(|spec| implemented_in(spec, base))
        });
    let in_history = context
        .shipped
        .is_some_and(|shipped| shipped.contains(&before.id));
    if !at_base && !in_history {
        return Vec::new();
    }
    vec![format!(
        "{id} was implemented, so its text is frozen: a changed contract is a new spec that lists `supersedes: [{id}]`, or an explicit superseding record",
        id = before.id
    )]
}

/// Whether `spec_id` was implemented at any commit reachable from `tip`
/// (TSK-169). The state is derived only from the spec and the records that
/// name it, and changes only at a commit that changes one of them. One
/// `git log --raw -z` lists every record blob each commit wrote under
/// `project-management/`; each distinct blob is parsed once, and a path is
/// relevant when any version of it is the spec or lists it in `specs`, read
/// as the judge reads it, never by searching the text for the id (an
/// escaped YAML string names the spec too). The state is then derived at
/// each commit that changed a relevant path, from those paths alone, and
/// the walk stops at the first implemented state. Side branches count
/// (`--full-history`); a shallow clone reads only the history it has.
fn shipped_in_history(repo: &Repository, tip: git2::Oid, spec_id: &str) -> Result<bool, String> {
    let message = |error: git2::Error| error.message().to_string();
    let output = crate::git::command()
        .arg("--git-dir")
        .arg(repo.path())
        .args([
            "log",
            "--no-renames",
            "--full-history",
            "--diff-merges=first-parent",
            "--raw",
            "-z",
            "--no-abbrev",
            "--format=commit %H",
        ])
        .arg(tip.to_string())
        .args(["--", "project-management/"])
        .output()
        .map_err(|error| format!("git log: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let mut commits: Vec<(git2::Oid, Vec<String>)> = Vec::new();
    let mut names: HashMap<git2::Oid, bool> = HashMap::new();
    let mut relevant: BTreeSet<String> = BTreeSet::new();
    // With `-z` every field ends in NUL and paths are never quoted:
    // `commit <hash>`, then per change `:<old mode> <new mode> <old blob>
    // <new blob> <status>` and its path as the next field. The first raw
    // field of a commit starts with the newline that ends its header.
    let mut fields = output.stdout.split(|byte| *byte == 0);
    while let Some(field) = fields.next() {
        // Header fields are git's own ASCII (hashes, modes, status).
        let field = std::str::from_utf8(field)
            .map_err(|error| format!("cannot decode git log metadata: {error}"))?;
        let field = field.strip_prefix('\n').unwrap_or(field);
        if let Some(hash) = field.strip_prefix("commit ") {
            commits.push((
                git2::Oid::from_str(hash.strip_suffix('\n').unwrap_or(hash)).map_err(message)?,
                Vec::new(),
            ));
            continue;
        }
        let Some(meta) = field.strip_prefix(':') else {
            continue;
        };
        let Some(path) = fields.next() else {
            break;
        };
        // A record path is valid text; a path with an invalid byte is not
        // one, and is never read as a lossy lookalike of one (issue 79).
        let path = GitName::from_bytes(path);
        let Ok(path) = path.rule_text() else {
            continue;
        };
        let Some(kind) = record_kind_for_tree_path(path) else {
            continue;
        };
        if let Some((_, paths)) = commits.last_mut() {
            paths.push(path.to_string());
        }
        let hash = meta.split(' ').nth(3).ok_or("git log omitted a blob id")?;
        let blob = git2::Oid::from_str(hash).map_err(message)?;
        if blob.is_zero() {
            continue;
        }
        let names_spec = if let Some(names_spec) = names.get(&blob) {
            *names_spec
        } else {
            let object = repo.find_blob(blob).map_err(message)?;
            let content =
                std::str::from_utf8(object.content()).map_err(|error| error.to_string())?;
            let record = RecordView::parse(kind, path, content)?;
            let names_spec =
                record.id == spec_id || record.specs.iter().any(|spec| spec == spec_id);
            names.insert(blob, names_spec);
            names_spec
        };
        if names_spec {
            relevant.insert(path.to_string());
        }
    }
    implemented_at_a_commit(repo, &commits, &relevant, spec_id)
}

/// Whether `spec_id` is implemented at one of `commits` that changed a
/// `relevant` path, derived from those paths alone and stopping at the
/// first implemented state (TSK-169, see [`shipped_in_history`]).
fn implemented_at_a_commit(
    repo: &Repository,
    commits: &[(git2::Oid, Vec<String>)],
    relevant: &BTreeSet<String>,
    spec_id: &str,
) -> Result<bool, String> {
    let message = |error: git2::Error| error.message().to_string();
    let mut parsed: HashMap<git2::Oid, RecordView> = HashMap::new();
    for (oid, paths) in commits {
        if !paths.iter().any(|path| relevant.contains(path)) {
            continue;
        }
        let tree = repo
            .find_commit(*oid)
            .and_then(|commit| commit.tree())
            .map_err(message)?;
        let mut graph = Graph::default();
        for path in relevant {
            let Some(kind) = record_kind_for_tree_path(path) else {
                continue;
            };
            let entry = match tree.get_path(Path::new(path)) {
                Ok(entry) => entry,
                Err(error) if error.code() == git2::ErrorCode::NotFound => continue,
                Err(error) => return Err(message(error)),
            };
            let view = match parsed.entry(entry.id()) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(cached) => {
                    let blob = repo.find_blob(entry.id()).map_err(message)?;
                    let content =
                        std::str::from_utf8(blob.content()).map_err(|error| error.to_string())?;
                    cached.insert(RecordView::parse(kind, path, content)?)
                }
            };
            graph
                .records
                .entry(view.id.clone())
                .or_insert_with(|| view.clone());
        }
        if graph
            .get(spec_id, RecordKind::Spec)
            .is_some_and(|spec| implemented_in(spec, &graph))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The specs a range changes the text of that shipped at an earlier commit
/// of `base`'s history but are not implemented at `base` itself, with a
/// problem for each spec whose history cannot be read (TSK-169).
fn shipped_specs(
    repo: &Repository,
    base: &str,
    before: &Graph,
    after: &Graph,
) -> (BTreeSet<String>, Vec<(String, String)>) {
    let mut shipped = BTreeSet::new();
    let mut problems = Vec::new();
    let Some(tip) = resolve_commit(repo, base) else {
        return (shipped, problems);
    };
    for record in after.records.values() {
        let Some(old) = before.get(&record.id, RecordKind::Spec) else {
            continue;
        };
        if record.kind != RecordKind::Spec
            || same_text(&old.body, &record.body)
            || implemented_in(old, before)
        {
            continue;
        }
        match shipped_in_history(repo, tip, &record.id) {
            Ok(true) => {
                shipped.insert(record.id.clone());
            }
            Ok(false) => {}
            Err(error) => problems.push((
                record.path.clone(),
                format!(
                    "cannot read the history of {} to check whether it shipped: {error}",
                    record.id
                ),
            )),
        }
    }
    (shipped, problems)
}

// ---------------------------------------------------------------------------
// Judgements
// ---------------------------------------------------------------------------

/// Judge one record change: the verdict a verb and a hand edit share.
///
/// `before` is the record as it was (`None` when the change adds it), `graph`
/// is the whole tree after the change, and `context` what is known about the
/// change itself.
#[must_use]
pub fn judge_change(
    before: Option<&RecordView>,
    after: &RecordView,
    graph: &Graph,
    baseline: &Baseline,
    context: ChangeContext<'_>,
) -> Verdict {
    let mut verdict = Verdict::default();
    verdict.apply(
        Mode::Strict,
        &after.path,
        transition_problems(before, after),
    );
    verdict.apply(
        Mode::Strict,
        &after.path,
        context_problems(before, after, context),
    );
    if let Some(old) = context.base.and_then(|base| base.records.get(&after.id)) {
        if old.status == "complete"
            && after.kind == RecordKind::Task
            && (after.status == "todo"
                || (after.status == "complete"
                    && before.is_some_and(|record| record.status == "todo"))
                || is_recompletion(Some(old), after)
                || context.reopened.is_some_and(|ids| ids.contains(&after.id)))
        {
            verdict.apply(Mode::Strict, &after.path, reopen_problems(Some(old), after));
        }
    }
    verdict.apply(
        Mode::Strict,
        &after.path,
        frozen_spec_problems(before, after, context),
    );
    let reopened = baseline.reopened_since(after)
        || context
            .reopened
            .is_some_and(|reopened| reopened.contains(&after.id));
    let mode = if reopened || before.is_none_or(|old| significant_change(old, after)) {
        Mode::Strict
    } else {
        baseline.mode(after)
    };
    // A record the change adds is new whatever history the checkout has.
    let mut state = state_problems(
        after,
        graph,
        before.is_none() || baseline.is_new(after),
        mode != Mode::Strict && baseline.completed_before(after),
    );
    if context
        .brought
        .is_some_and(|brought| brought.legacy.contains_key(&after.id))
    {
        // Listed as information instead (SPC-013 R-120, TSK-140 AC-13).
        state.retain(|problem| !problem.starts_with(NO_BLOCK));
    }
    verdict.apply(mode, &after.path, state);
    verdict.apply(
        Mode::Strict,
        &after.path,
        relationship_problems(after, graph),
    );
    verdict.sort();
    verdict
}

/// Judge every record a range changes, from `base` to `head` (a revision, or
/// the working tree when `None`). A record `base` lacks but a migration
/// baseline has is judged as a transition from each of its ancestry-latest
/// baseline copies, so a range whose base predates a baseline treats older
/// records as that baseline does. The baseline list is read from `base`
/// (see `range_baseline`).
///
/// # Errors
///
/// Returns a message when the repository or a revision cannot be read.
pub fn judge_range(repo_root: &Path, base: &str, head: Option<&str>) -> Result<Verdict, String> {
    judge_range_against(repo_root, base, base, head, false, None)
}

/// [`judge_range`] with the records diffed from `base` and the governing
/// baseline list read from `target`, which differ for a pull request whose
/// branch forked before the target's current tip. `on_line` says the range
/// is a verified integration line (the caller proved it), whose landings
/// are judged one by one; any other range is judged whole.
fn judge_range_against(
    repo_root: &Path,
    base: &str,
    target: &str,
    head: Option<&str>,
    on_line: bool,
    brought: Option<&Brought>,
) -> Result<Verdict, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let target_commit = resolve_commit(&repo, target)
        .ok_or_else(|| format!("cannot resolve {target} to a commit"))?;
    let (baseline, notices) = range_baseline(repo_root, &repo, target_commit, head);
    let base_graph = Graph::from_revision(&repo, base)?;
    let after = match head {
        Some(head) => Graph::from_revision(&repo, head)?,
        None => Graph::from_worktree(repo_root)?,
    };
    // The tree before the change, with each record the base lacks taken
    // from a baseline copy, for the rules that read the surrounding records.
    let mut before = base_graph.clone();
    for id in after.records.keys() {
        if !before.records.contains_key(id) {
            if let Some(copy) = baseline.copies(id).first() {
                before.records.insert(id.clone(), (*copy).clone());
            }
        }
    }
    let paths = changed_paths(&repo, base, head)?;
    let reopened = reopened_in_range(&repo, base, head, &after)?;
    let (shipped, history_problems) = shipped_specs(&repo, base, &before, &after);
    let context = ChangeContext {
        base: Some(&before),
        changed_paths: Some(&paths),
        reopened: Some(&reopened),
        brought,
        shipped: Some(&shipped),
    };
    let mut verdict = Verdict {
        notices,
        ..Verdict::default()
    };
    if let Some(brought) = brought {
        verdict.notices.extend(
            brought
                .legacy
                .values()
                .map(|notice| Finding::new(notice.clone(), remedy::RELEASE_LEGACY_RECORD.remedy())),
        );
    }
    for (path, problem) in history_problems {
        verdict.apply(Mode::Strict, &path, vec![problem]);
    }
    verdict.warnings.extend(baseline.warning());
    verdict.errors.extend(baseline.errors());
    for record in after.records.values() {
        if brought.is_some_and(|brought| brought.backfills.contains(&record.id)) {
            // A `uid` backfill landed on a line was judged there.
            continue;
        }
        let olds: Vec<&RecordView> = match base_graph.records.get(&record.id) {
            Some(old) if old.content == record.content && !reopened.contains(&record.id) => {
                continue
            }
            Some(old) => vec![old],
            None if baseline.is_legacy_blob(record) => continue,
            None => baseline.copies(&record.id),
        };
        // On a verified line, a spec approval landed by a merge is judged
        // by that landing's change, never by the whole line (R-32 on a
        // line). Any other range is judged whole, so a private merge inside
        // a task branch cannot manufacture the exception.
        let landing = if on_line {
            landing_paths(&repo, base, head, record)?
        } else {
            None
        };
        let context = match &landing {
            Some(paths) => ChangeContext {
                changed_paths: Some(paths),
                ..context
            },
            None => context,
        };
        let judged: Vec<Verdict> = if olds.is_empty() {
            vec![judge_change(None, record, &after, &baseline, context)]
        } else {
            olds.into_iter()
                .map(|old| judge_change(Some(old), record, &after, &baseline, context))
                .collect()
        };
        for one in judged {
            verdict.errors.extend(one.errors);
            verdict.warnings.extend(one.warnings);
        }
    }
    verdict.sort();
    Ok(verdict)
}

/// On a verified integration line, the paths of the landing that brought a
/// spec's approval or supersession into the range, when that landing is a
/// merge on the line's first-parent chain from `head` back to `base`: the
/// landed pull request's own change, judged as the planning-only change the
/// transition must travel in. `None` when the transition arrived by a plain
/// commit, or on the working tree, so the whole range is judged as before.
fn landing_paths(
    repo: &Repository,
    base: &str,
    head: Option<&str>,
    record: &RecordView,
) -> Result<Option<Vec<String>>, String> {
    let to = record.status.as_str();
    if record.kind != RecordKind::Spec || !matches!(to, "approved" | "superseded") {
        return Ok(None);
    }
    let Some(head) = head else {
        return Ok(None);
    };
    let base = resolve_commit(repo, base).ok_or("cannot resolve spec landing base")?;
    let tip = resolve_commit(repo, head).ok_or("cannot resolve spec landing head")?;
    let mut commit = repo.find_commit(tip).map_err(|error| error.to_string())?;
    let status_at = |commit: &git2::Commit<'_>| -> Result<Option<String>, String> {
        let tree = commit.tree().map_err(|error| error.to_string())?;
        let entry = match tree.get_path(Path::new(&record.path)) {
            Ok(entry) => entry,
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let blob = repo
            .find_blob(entry.id())
            .map_err(|error| error.to_string())?;
        let text = std::str::from_utf8(blob.content())
            .map_err(|error| format!("spec landing record is not valid UTF-8: {error}"))?;
        RecordView::parse(record.kind, &record.path, text).map(|view| Some(view.status))
    };
    while commit.id() != base {
        if commit.parent_count() == 0 {
            return Ok(None);
        }
        let parent = commit.parent(0).map_err(|error| error.to_string())?;
        let arrived = status_at(&commit)?.as_deref() == Some(to)
            && status_at(&parent)?.as_deref() != Some(to);
        if arrived {
            if commit.parent_count() < 2 {
                return Ok(None);
            }
            return changed_paths(
                repo,
                &parent.id().to_string(),
                Some(&commit.id().to_string()),
            )
            .map(Some);
        }
        commit = parent;
    }
    Ok(None)
}

/// Complete tasks whose status left `complete` inside this range, including
/// when its endpoint block is unchanged. Historical completions without a
/// block are also walked so they cannot re-enter the migration exception.
pub(super) fn reopened_in_range(
    repo: &Repository,
    base: &str,
    head: Option<&str>,
    after: &Graph,
) -> Result<BTreeSet<String>, String> {
    let candidates: Vec<&RecordView> = after
        .records
        .values()
        .filter(|record| record.kind == RecordKind::Task && record.status == "complete")
        .collect();
    let mut reopened = BTreeSet::new();
    if candidates.is_empty() {
        return Ok(reopened);
    }
    let resolve = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("cannot read reopen history at {revision}: {error}"))
    };
    let base = resolve(base)?;
    let tip = resolve(head.unwrap_or("HEAD"))?;
    let mut walk = repo.revwalk().map_err(|error| error.to_string())?;
    walk.push(tip)
        .and_then(|()| walk.hide(base))
        .map_err(|error| error.to_string())?;
    for oid in walk {
        let oid = oid.map_err(|error| error.to_string())?;
        let commit = repo.find_commit(oid).map_err(|error| error.to_string())?;
        for record in &candidates {
            let Some(content) = super::acceptance::blob_at(repo, oid, &record.path)? else {
                continue;
            };
            let status = RecordView::parse(record.kind, &record.path, &content)?.status;
            if status != "complete" {
                if let Some(parent) = commit.parent_ids().next() {
                    if let Some(content) = super::acceptance::blob_at(repo, parent, &record.path)? {
                        if RecordView::parse(record.kind, &record.path, &content)?.status
                            == "complete"
                        {
                            reopened.insert(record.id.clone());
                        }
                    }
                }
            }
        }
    }
    Ok(reopened)
}

/// Every path a change touches, from `base` to `head` (a revision), or to
/// the working tree with its index and untracked files when `head` is `None`.
///
/// # Errors
///
/// Returns a message when a revision or the diff cannot be read.
pub fn changed_paths(
    repo: &Repository,
    base: &str,
    head: Option<&str>,
) -> Result<Vec<String>, String> {
    let tree = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_tree())
            .map_err(|error| format!("cannot read the tree of {revision}: {}", error.message()))
    };
    let base_tree = tree(base)?;
    let diff = if let Some(head) = head {
        repo.diff_tree_to_tree(Some(&base_tree), Some(&tree(head)?), None)
    } else {
        let mut options = git2::DiffOptions::new();
        options.include_untracked(true).recurse_untracked_dirs(true);
        repo.diff_tree_to_workdir_with_index(Some(&base_tree), Some(&mut options))
    }
    .map_err(|error| format!("cannot diff from {base}: {}", error.message()))?;
    // OS text rule (issue 79): a path is kept as its storage key, so a path
    // that is not valid UTF-8 stays its own path (valid text is unchanged and
    // no valid path equals a key with an invalid byte), never a lossy lookalike.
    let mut paths: Vec<String> = crate::git::diff_paths(&diff)
        .iter()
        .map(GitName::storage_key)
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// The change context of a status verb run in `repo_root`: the paths of the
/// uncommitted work (the verb cannot know the pull request's base; CI and
/// `validate --since` judge the whole range) and the tree at the merge-base
/// of `HEAD` and the default work target, when one resolves.
///
/// # Errors
///
/// Returns an error if changed paths, target history or contextual records cannot be read.
pub fn working_context(repo_root: &Path) -> Result<(Option<Graph>, Option<Vec<String>>), String> {
    let Some(repo) = crate::hooks::repo::open(repo_root)? else {
        return Ok((None, None));
    };
    match repo.head() {
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => return Ok((None, None)),
        Err(error) => return Err(format!("cannot read working context HEAD: {error}")),
        Ok(_) => {}
    }
    let paths = changed_paths(&repo, "HEAD", None)?;
    let base = match super::work_start::default_work_target(repo_root)
        .map_err(|error| error.to_string())?
    {
        Some(target) => {
            let resolve = |name: &str| {
                repo.revparse_single(name)
                    .and_then(|object| object.peel_to_commit())
                    .map(|commit| commit.id())
                    .map_err(|error| error.to_string())
            };
            let anchor = repo
                .merge_base(resolve("HEAD")?, resolve(&target)?)
                .map_err(|error| error.to_string())?;
            Some(Graph::from_revision(&repo, &anchor.to_string())?)
        }
        None => None,
    };
    Ok((base, Some(paths)))
}

/// Judge a pull request: the records changed from the merge-base of `base`
/// and `head` to `head`, so work that landed on the target after the branch
/// point is not mistaken for a change of this range. The governing baseline
/// list is read from `base` itself, not from the merge-base, so a branch
/// forked before the target adopted its list cannot re-enter migration.
///
/// # Errors
///
/// Returns a message when a revision or the merge-base cannot be resolved.
pub fn judge_pull_request(repo_root: &Path, base: &str, head: &str) -> Result<Verdict, String> {
    judge_pull_request_under(repo_root, base, head, base)
}

/// [`judge_pull_request_under`] for a verified integration line judged as
/// one pull request (R-60): each planning landing on the line's
/// first-parent chain is judged by its own change. The caller proves the
/// line (`check_epic_line`); an unverified range is judged whole.
///
/// # Errors
///
/// Returns a message when a revision or the merge-base cannot be resolved.
pub fn judge_line_under(
    repo_root: &Path,
    base: &str,
    head: &str,
    authority: &str,
) -> Result<Verdict, String> {
    judge_pull_request_shaped(repo_root, base, head, authority, true)
}

/// [`judge_pull_request`] with the governing baseline list read from
/// `authority` instead of `base`. The pre-push hook passes the destination
/// branch's current tip here, because its `base` is a boundary of every
/// destination tip, which can belong to another line with another list.
///
/// # Errors
///
/// Returns a message when a revision or the merge-base cannot be resolved.
pub fn judge_pull_request_under(
    repo_root: &Path,
    base: &str,
    head: &str,
    authority: &str,
) -> Result<Verdict, String> {
    judge_pull_request_shaped(repo_root, base, head, authority, false)
}

fn judge_pull_request_shaped(
    repo_root: &Path,
    base: &str,
    head: &str,
    authority: &str,
    on_line: bool,
) -> Result<Verdict, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let target = commit(authority)?;
    let anchor = repo
        .merge_base(commit(base)?, commit(head)?)
        .map_err(|error| format!("no merge-base of {base} and {head}: {}", error.message()))?;
    judge_range_against(
        repo_root,
        &anchor.to_string(),
        &target.to_string(),
        Some(head),
        on_line,
        None,
    )
}

/// [`judge_pull_request_under`] for a release range: each record in
/// `brought` is judged where it was introduced on its line (SPC-013 R-120).
///
/// # Errors
///
/// Returns a message when a revision or the merge-base cannot be resolved.
pub fn judge_release_range(
    repo_root: &Path,
    base: &str,
    head: &str,
    authority: &str,
    brought: &Brought,
) -> Result<Verdict, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| format!("{revision}: {}", error.message()))
    };
    let target = commit(authority)?;
    let anchor = repo
        .merge_base(commit(base)?, commit(head)?)
        .map_err(|error| format!("no merge-base of {base} and {head}: {}", error.message()))?;
    judge_range_against(
        repo_root,
        &anchor.to_string(),
        &target.to_string(),
        Some(head),
        false,
        Some(brought),
    )
}

/// Judge the whole checked-out tree against the migration baseline, and add
/// the stale-word warnings of R-28 and R-80.
#[must_use]
pub fn validate_lifecycle(repo_root: &Path) -> Verdict {
    let graph = match Graph::from_worktree(repo_root) {
        Ok(graph) => graph,
        Err(error) => {
            return Verdict {
                errors: vec![error],
                ..Verdict::default()
            }
        }
    };
    let baseline = Baseline::load(repo_root);
    let mut verdict = Verdict::default();
    verdict.warnings.extend(baseline.warning());
    verdict.errors.extend(baseline.errors());
    for record in graph.records.values() {
        // A reopen after the baseline is a transition: the record is judged
        // in full from then on (R-83).
        let mode = if baseline.reopened_since(record) {
            Mode::Strict
        } else {
            baseline.mode(record)
        };
        verdict.apply(
            mode,
            &record.path,
            state_problems(
                record,
                &graph,
                baseline.is_new(record),
                mode != Mode::Strict && baseline.completed_before(record),
            ),
        );
        verdict.apply(
            Mode::Strict,
            &record.path,
            relationship_problems(record, &graph),
        );
    }
    verdict.warnings.extend(stale_warnings(repo_root, &graph));
    verdict.sort();
    verdict
}

fn stale_warnings(repo_root: &Path, graph: &Graph) -> Vec<Finding> {
    let mut warnings = Vec::new();
    let repo = Repository::discover(repo_root).ok();
    let prefixes = match crate::hooks::policy::Policy::load_effective(repo_root) {
        Ok((policy, _)) => policy.git.branch_prefixes,
        Err(error) => {
            return vec![Finding::new(
                format!("cannot read work policy: {error}"),
                remedy::BASELINE_REVIEW.remedy(),
            )]
        }
    };
    for record in graph.records.values() {
        match record.kind {
            RecordKind::Epic if matches!(record.status.as_str(), "draft" | "planning") => {
                let done: Vec<&str> = graph
                    .tasks()
                    .filter(|task| task.epic_id.as_deref() == Some(record.id.as_str()))
                    .filter(|task| task.status == "complete")
                    .map(|task| task.id.as_str())
                    .collect();
                if !done.is_empty() {
                    warnings.push(Finding::new(
                        format!(
                            "{}: stale status: {} epic has complete tasks ({})",
                            record.path,
                            record.status,
                            done.join(", ")
                        ),
                        remedy::EPIC_STATUS.with(&[("path", &record.path), ("id", &record.id)]),
                    ));
                }
            }
            RecordKind::Task if record.status == "in_progress" => {
                let active = repo
                    .as_ref()
                    .is_some_and(|repo| has_active_branch(repo, record, &prefixes));
                if !active {
                    warnings.push(Finding::new(
                        format!(
                            "{}: stale status: in_progress with no active branch carrying {}",
                            record.path, record.id
                        ),
                        remedy::TASK_STATUS.with(&[("id", &record.id)]),
                    ));
                }
            }
            RecordKind::Task if record.status == "complete" => {
                for spec in consumed_specs(record, graph) {
                    if graph
                        .get(&spec, RecordKind::Spec)
                        .is_some_and(|spec| spec.status == "superseded")
                    {
                        warnings.push(Finding::new(
                            format!(
                                "{}: acceptance cited superseded spec {spec}; recheck it against the successor",
                                record.path
                            ),
                            remedy::SUPERSEDED_CITATION
                                .with(&[("id", &record.id), ("spec", &spec)]),
                        ));
                    }
                }
            }
            RecordKind::Spec => {
                let state = derived_spec_state(record, graph, None);
                // An approved spec whose consumers are all done is healthy:
                // `implemented` is its derived state, never written (R-32,
                // R-51). Only states that disagree with the consumers warn.
                match (record.status.as_str(), state) {
                    ("approved", SpecState::NoDeliveringConsumer) => {
                        warnings.push(Finding::new(
                            format!("{}: no delivering consumer", record.path),
                            remedy::SPEC_NO_CONSUMER.with(&[("id", &record.id)]),
                        ));
                    }
                    ("implemented", SpecState::Implemented) => {}
                    ("implemented", _) => warnings.push(Finding::new(
                        format!(
                            "{}: written `implemented` disagrees with the derived state",
                            record.path
                        ),
                        remedy::SPEC_WRITTEN_IMPLEMENTED.with(&[("path", &record.path)]),
                    )),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    warnings
}

/// A visible branch `<prefix>/<TSK-id>-<slug>`, local or remote-tracking,
/// whose tip has not landed on the task's target (R-27).
fn has_active_branch(repo: &Repository, task: &RecordView, prefixes: &[String]) -> bool {
    let target = task
        .integration_target
        .as_deref()
        .and_then(|target| {
            super::resolve_work_target(repo.workdir().unwrap_or_else(|| repo.path()), Some(target))
                .ok()
                .flatten()
        })
        .and_then(|target| {
            ["refs/heads/", "refs/remotes/"]
                .iter()
                .find_map(|prefix| repo.find_reference(&format!("{prefix}{target}")).ok())
                .and_then(|reference| reference.peel_to_commit().ok())
                .map(|commit| commit.id())
        });
    let Ok(references) = repo.references() else {
        return false;
    };
    let marker = format!("{}-", task.id);
    let remotes = crate::git::name::remote_names(repo).unwrap_or_default();
    references.flatten().any(|reference| {
        // OS text rule (issue 79): a branch that is not valid UTF-8 still
        // counts as active work when its prefix and task id match, so the name
        // is exact bytes and the prefix and marker are tested as bytes.
        let name = crate::git::name::reference_name(&reference);
        let short = if let Some(local) = name.strip_prefix(b"refs/heads/") {
            local
        } else if let Some(remote) = name.strip_prefix(b"refs/remotes/") {
            match crate::git::tracking_branch(&remote, &remotes) {
                Some(branch) => branch,
                None => return false,
            }
        } else {
            return false;
        };
        let carries = prefixes.iter().any(|prefix| {
            short
                .strip_prefix(prefix.as_bytes())
                .is_some_and(|rest| rest.starts_with(marker.as_bytes()))
        });
        if !carries {
            return false;
        }
        let Some(tip) = reference.target() else {
            return false;
        };
        match target {
            Some(target) => {
                tip != target && !repo.graph_descendant_of(target, tip).unwrap_or(false)
            }
            None => true,
        }
    })
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;

#[cfg(test)]
mod r21_tests {
    #[test]
    fn r21_landing_paths_refuse_undecodable_parent_record() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let path = "project-management/specs/SPC-001.md";
        let base = crate::git::add_commit(&repo, &[(b"seed", b"base")]);
        let parent = crate::git::add_commit(
            &repo,
            &[(
                path.as_bytes(),
                b"---\nid: SPC-001\nstatus: draft\nintegration_target: caf\xff\n---\n",
            )],
        );
        let valid = "---\nid: SPC-001\nstatus: approved\nintegration_target: main\n---\n";
        let next = crate::git::add_commit(&repo, &[(path.as_bytes(), valid.as_bytes())]);
        let tree = repo.find_commit(next).unwrap().tree().unwrap();
        let signature = git2::Signature::now("test", "test@example.invalid").unwrap();
        let head = repo
            .commit(
                None,
                &signature,
                &signature,
                "merge approval",
                &tree,
                &[
                    &repo.find_commit(parent).unwrap(),
                    &repo.find_commit(base).unwrap(),
                ],
            )
            .unwrap();
        let record = super::RecordView::parse(super::RecordKind::Spec, path, valid).unwrap();
        let answer =
            super::landing_paths(&repo, &base.to_string(), Some(&head.to_string()), &record);
        assert!(
            answer.is_err(),
            "unreadable parent must not prove a narrowed landing: {answer:?}"
        );
    }
}
