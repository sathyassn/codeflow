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

use std::collections::BTreeMap;
use std::path::Path;

use git2::{Repository, TreeWalkMode, TreeWalkResult};

use super::record_text::{
    acceptance_blocks, check_acceptance, check_block, outcome_word, parse_blocker,
    parse_cancellation, parse_criteria, reopen_reasons, AcceptanceBlock, CriteriaList, Criterion,
    FencedAcceptance,
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
    pub body: String,
    pub criteria: CriteriaList,
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
            (!value.trim().is_empty()).then_some(value)
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
            status: field("status").unwrap_or_default(),
            epic_id: field("epic_id"),
            specs: list("specs"),
            supersedes: list("supersedes"),
            superseded_by: field("superseded_by"),
            integration_target: field("integration_target"),
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

    fn superseded_blocks(&self) -> Vec<FencedAcceptance> {
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
    #[must_use]
    pub fn from_worktree(repo_root: &Path) -> Self {
        let pm = repo_root.join("project-management");
        let mut graph = Self::default();
        for (kind, files) in [
            (RecordKind::Epic, super::layout::epic_record_files(&pm)),
            (RecordKind::Spec, super::layout::spec_record_files(&pm)),
            (RecordKind::Task, super::layout::task_record_files(&pm)),
        ] {
            for path in files {
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let relative = path
                    .strip_prefix(repo_root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                graph.insert(kind, &relative, &content);
            }
        }
        graph
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
        tree.walk(TreeWalkMode::PreOrder, |root, entry| {
            let Ok(name) = entry.name() else {
                return TreeWalkResult::Ok;
            };
            let path = format!("{root}{name}");
            let Some(kind) = record_kind_for_tree_path(&path) else {
                return TreeWalkResult::Ok;
            };
            match repo.find_blob(entry.id()) {
                Ok(blob) => {
                    graph.insert(kind, &path, &String::from_utf8_lossy(blob.content()));
                    TreeWalkResult::Ok
                }
                Err(error) => {
                    failure = Some(format!("{path}: {}", error.message()));
                    TreeWalkResult::Abort
                }
            }
        })
        .map_err(|error| {
            failure
                .clone()
                .unwrap_or_else(|| error.message().to_string())
        })?;
        match failure {
            Some(failure) => Err(failure),
            None => Ok(graph),
        }
    }

    /// Unparseable records and duplicate ids are left to the structural
    /// validator, which reports them.
    fn insert(&mut self, kind: RecordKind, path: &str, content: &str) {
        if let Ok(record) = RecordView::parse(kind, path, content) {
            self.records.entry(record.id.clone()).or_insert(record);
        }
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
        let Ok(repo) = Repository::discover(repo_root) else {
            let entries = recorded_baseline(repo_root);
            return if entries.is_empty() {
                Self::NotRecorded
            } else {
                Self::Unavailable(entries)
            };
        };
        let head = resolve_commit(&repo, "HEAD");
        Self::from_entries(&repo, &recorded_baseline(repo_root), head)
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

    /// Whether the record's bytes equal its copy in any listed baseline.
    fn is_legacy_blob(&self, record: &RecordView) -> bool {
        match self {
            Self::Available { graphs, .. } => graphs.iter().any(|graph| {
                graph
                    .records
                    .get(&record.id)
                    .is_some_and(|old| old.content == record.content)
            }),
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

    /// Whether a record was created after every baseline.
    fn is_new(&self, record: &RecordView) -> bool {
        match self {
            Self::NotRecorded | Self::Refused(_) => true,
            Self::Unavailable(_) => false,
            Self::Available { .. } => self.copies(&record.id).is_empty(),
        }
    }

    /// The visible warning of a checkout that cannot see the baseline.
    fn warning(&self) -> Option<String> {
        match self {
            Self::Unavailable(commits) => Some(format!(
                "work-records migration baseline {} is not in this clone's history; records the range did not add are judged leniently (fetch full history to enforce the rules in full)",
                commits.join(", ")
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
/// string read as a one-item list. Entries are trimmed; their form is
/// checked when they are resolved.
fn baseline_entries(config: Option<&toml::Value>) -> Vec<String> {
    let values: Vec<&str> = match config.and_then(|config| config.get(BASELINE_KEY)) {
        Some(toml::Value::String(value)) => vec![value.as_str()],
        Some(toml::Value::Array(items)) => items.iter().filter_map(toml::Value::as_str).collect(),
        _ => Vec::new(),
    };
    let mut entries: Vec<String> = Vec::new();
    for value in values
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !entries.iter().any(|seen| seen == value) {
            entries.push(value.to_string());
        }
    }
    entries
}

/// The recorded migration baseline entries of the checked-out tree; empty
/// when none.
#[must_use]
pub fn recorded_baseline(repo_root: &Path) -> Vec<String> {
    baseline_entries(crate::hooks::policy::read_project_toml(repo_root).as_ref())
}

/// The baseline entries recorded in a commit's `.codeflow/project.toml`.
fn baseline_at(repo: &Repository, commit: git2::Oid) -> Vec<String> {
    let config = repo
        .find_commit(commit)
        .ok()
        .and_then(|commit| commit.tree().ok())
        .and_then(|tree| tree.get_path(Path::new(".codeflow/project.toml")).ok())
        .and_then(|entry| repo.find_blob(entry.id()).ok())
        .and_then(|blob| String::from_utf8(blob.content().to_vec()).ok())
        .and_then(|text| text.parse::<toml::Value>().ok());
    baseline_entries(config.as_ref())
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
) -> (Baseline, Vec<String>) {
    let head_commit = resolve_commit(repo, head.unwrap_or("HEAD"));
    let base_list = baseline_at(repo, target);
    let head_list = match head {
        Some(_) => head_commit
            .map(|commit| baseline_at(repo, commit))
            .unwrap_or_default(),
        None => recorded_baseline(repo_root),
    };
    let mut notices = Vec::new();
    if base_list.is_empty() {
        if !head_list.is_empty() {
            notices.push(format!(
                "this change introduces {BASELINE_KEY}; every record unchanged from these snapshots is legacy, so the human reviewer approves each: {}",
                head_list.join(", ")
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
        notices.push(format!(
            "this change edits {BASELINE_KEY} (added {}; removed {}{}); the target's list judges this change, and the edit takes effect after it lands",
            describe(&added),
            describe(&removed),
            if added.is_empty() && removed.is_empty() { "; reordered" } else { "" }
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
    pub warnings: Vec<String>,
    /// Facts a reviewer should see that are neither errors nor warnings,
    /// such as a change to the baseline list.
    pub notices: Vec<String>,
}

impl Verdict {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }

    fn apply(&mut self, mode: Mode, path: &str, problems: Vec<String>) {
        let target = match mode {
            Mode::Exempt => return,
            Mode::Lenient => &mut self.warnings,
            Mode::Strict => &mut self.errors,
        };
        target.extend(
            problems
                .into_iter()
                .map(|problem| format!("{path}: {problem}")),
        );
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
}

/// Paths a planning-only change may touch (R-70).
const PLANNING_PATHS: [&str; 2] = ["project-management/", "docs/plan/"];

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

/// Whether moving `before` to `after` is a legal transition (R-30, R-32 and
/// the epic terminal acts of R-26). A record added in the change moves from
/// the initial state: `todo`, `draft` or an open epic.
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
        return Vec::new();
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
            Err("approved never returns to draft; a changed contract is a new spec")
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
fn reopen_problems(before: Option<&RecordView>, after: &RecordView) -> Vec<String> {
    let mut problems = Vec::new();
    if !after.active_blocks().is_empty() {
        problems
            .push("a reopened task keeps no active acceptance block; mark it superseded".into());
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
fn state_problems(record: &RecordView, graph: &Graph, is_new: bool) -> Vec<String> {
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
        RecordKind::Task => problems.extend(task_state_problems(record, graph)),
        RecordKind::Epic => problems.extend(epic_state_problems(record, graph)),
        RecordKind::Spec => {
            if record.status == "approved"
                && crate::validate::section_has_unresolved_questions(record.body.as_bytes())
            {
                problems.push("an approved spec leaves no open question".into());
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

fn task_state_problems(task: &RecordView, graph: &Graph) -> Vec<String> {
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
            problems.extend(completion_problems(task));
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
/// structural rules of R-60.
fn completion_problems(record: &RecordView) -> Vec<String> {
    let active = record.active_blocks();
    match active.as_slice() {
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

/// The epic's own acceptance block, when it is the one active block and it
/// passes the structural rules of R-60 for the criteria no task serves.
/// Problems with it are reported, and an invalid block proves nothing.
fn epic_own_block(
    epic: &RecordView,
    graph: &Graph,
    problems: &mut Vec<String>,
) -> Option<AcceptanceBlock> {
    let required: Vec<Criterion> = epic
        .criteria
        .items
        .iter()
        .filter(|criterion| serving_tasks(epic, criterion, graph).is_empty())
        .filter(|criterion| !ticked_ordinary(criterion))
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
            return Err("served only by cancelled tasks".into());
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
    if record.kind != RecordKind::Spec {
        return problems;
    }
    if record.supersedes.contains(&record.id) {
        problems.push("a spec cannot supersede itself".into());
    }
    if record.superseded_by.as_deref() == Some(record.id.as_str()) {
        problems
            .push("a spec cannot be superseded by itself; a changed contract is a new spec".into());
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
    if let Some(paths) = context.changed_paths {
        let product: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|path| !PLANNING_PATHS.iter().any(|prefix| path.starts_with(prefix)))
            .collect();
        if !product.is_empty() {
            problems.push(format!(
                "a spec becomes {to} only in a planning-only change (project-management/ and docs/plan/); this change also touches {}",
                product.join(", ")
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
    let mode = if before.is_none_or(|old| significant_change(old, after)) {
        Mode::Strict
    } else {
        baseline.mode(after)
    };
    verdict.apply(
        mode,
        &after.path,
        // A record the change adds is new whatever history the checkout has.
        state_problems(after, graph, before.is_none() || baseline.is_new(after)),
    );
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
/// (see [`range_baseline`]).
///
/// # Errors
///
/// Returns a message when the repository or a revision cannot be read.
pub fn judge_range(repo_root: &Path, base: &str, head: Option<&str>) -> Result<Verdict, String> {
    judge_range_against(repo_root, base, base, head)
}

/// [`judge_range`] with the records diffed from `base` and the governing
/// baseline list read from `target`, which differ for a pull request whose
/// branch forked before the target's current tip.
fn judge_range_against(
    repo_root: &Path,
    base: &str,
    target: &str,
    head: Option<&str>,
) -> Result<Verdict, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.message().to_string())?;
    let target_commit = resolve_commit(&repo, target)
        .ok_or_else(|| format!("cannot resolve {target} to a commit"))?;
    let (baseline, notices) = range_baseline(repo_root, &repo, target_commit, head);
    let base_graph = Graph::from_revision(&repo, base)?;
    let after = match head {
        Some(head) => Graph::from_revision(&repo, head)?,
        None => Graph::from_worktree(repo_root),
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
    let context = ChangeContext {
        base: Some(&before),
        changed_paths: Some(&paths),
    };
    let mut verdict = Verdict {
        notices,
        ..Verdict::default()
    };
    verdict.warnings.extend(baseline.warning());
    verdict.errors.extend(baseline.errors());
    for record in after.records.values() {
        let olds: Vec<&RecordView> = match base_graph.records.get(&record.id) {
            Some(old) if old.content == record.content => continue,
            Some(old) => vec![old],
            None if baseline.is_legacy_blob(record) => continue,
            None => baseline.copies(&record.id),
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
    let mut paths: Vec<String> = diff
        .deltas()
        .flat_map(|delta| [delta.old_file().path(), delta.new_file().path()])
        .flatten()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// The change context of a status verb run in `repo_root`: the paths of the
/// uncommitted work (the verb cannot know the pull request's base; CI and
/// `validate --since` judge the whole range) and the tree at the merge-base
/// of `HEAD` and the default work target, when one resolves.
#[must_use]
pub fn working_context(repo_root: &Path) -> (Option<Graph>, Option<Vec<String>>) {
    let Ok(repo) = Repository::discover(repo_root) else {
        return (None, None);
    };
    let paths = changed_paths(&repo, "HEAD", None).ok();
    let base = super::work_start::default_work_target(repo_root)
        .and_then(|target| {
            let head = repo
                .revparse_single("HEAD")
                .ok()?
                .peel_to_commit()
                .ok()?
                .id();
            let target = repo
                .revparse_single(&target)
                .ok()?
                .peel_to_commit()
                .ok()?
                .id();
            repo.merge_base(head, target).ok()
        })
        .and_then(|anchor| Graph::from_revision(&repo, &anchor.to_string()).ok());
    (base, paths)
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
    )
}

/// Judge the whole checked-out tree against the migration baseline, and add
/// the stale-word warnings of R-28 and R-80.
#[must_use]
pub fn validate_lifecycle(repo_root: &Path) -> Verdict {
    let graph = Graph::from_worktree(repo_root);
    let baseline = Baseline::load(repo_root);
    let mut verdict = Verdict::default();
    verdict.warnings.extend(baseline.warning());
    verdict.errors.extend(baseline.errors());
    for record in graph.records.values() {
        verdict.apply(
            baseline.mode(record),
            &record.path,
            state_problems(record, &graph, baseline.is_new(record)),
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

fn stale_warnings(repo_root: &Path, graph: &Graph) -> Vec<String> {
    let mut warnings = Vec::new();
    let repo = Repository::discover(repo_root).ok();
    let prefixes = crate::hooks::policy::Policy::load_effective(repo_root)
        .0
        .git
        .branch_prefixes;
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
                    warnings.push(format!(
                        "{}: stale status: {} epic has complete tasks ({})",
                        record.path,
                        record.status,
                        done.join(", ")
                    ));
                }
            }
            RecordKind::Task if record.status == "in_progress" => {
                let active = repo
                    .as_ref()
                    .is_some_and(|repo| has_active_branch(repo, record, &prefixes));
                if !active {
                    warnings.push(format!(
                        "{}: stale status: in_progress with no active branch carrying {}",
                        record.path, record.id
                    ));
                }
            }
            RecordKind::Task if record.status == "complete" => {
                for spec in consumed_specs(record, graph) {
                    if graph
                        .get(&spec, RecordKind::Spec)
                        .is_some_and(|spec| spec.status == "superseded")
                    {
                        warnings.push(format!(
                            "{}: acceptance cited superseded spec {spec}; recheck it against the successor",
                            record.path
                        ));
                    }
                }
            }
            RecordKind::Spec => {
                let state = derived_spec_state(record, graph, None);
                match (record.status.as_str(), state) {
                    ("approved", SpecState::Implemented) => warnings.push(format!(
                        "{}: stale status: approved spec whose consumers are all accepted; its derived state is implemented",
                        record.path
                    )),
                    ("approved", SpecState::NoDeliveringConsumer) => {
                        warnings.push(format!("{}: no delivering consumer", record.path));
                    }
                    ("implemented", SpecState::Implemented) => {}
                    ("implemented", _) => warnings.push(format!(
                        "{}: written `implemented` disagrees with the derived state",
                        record.path
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
    references.flatten().any(|reference| {
        let Ok(name) = reference.name() else {
            return false;
        };
        let short = if let Some(local) = name.strip_prefix("refs/heads/") {
            local
        } else if let Some(remote) = name.strip_prefix("refs/remotes/") {
            match remote.split_once('/') {
                Some((_, branch)) => branch,
                None => return false,
            }
        } else {
            return false;
        };
        let carries = prefixes.iter().any(|prefix| {
            short
                .strip_prefix(prefix.as_str())
                .is_some_and(|rest| rest.starts_with(&marker))
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
