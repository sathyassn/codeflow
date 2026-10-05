//! Outcome timings derived from git history, never recorded.
//!
//! The task records are read from `HEAD`. Each record's status history is
//! the sequence of non-merge commits, parents first, reachable from `HEAD`
//! or a task's integration target, whose copy of the record differs from
//! their parent's. A merge only carries edits made on its parents' lines,
//! so a record edited only in a merge's conflict resolution is not seen.
//! Times are author times, which a rebase keeps.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use git2::{Commit, Oid, Repository, Sort};
use serde::Serialize;

use crate::workgraph::record_text::acceptance_blocks;

/// One commit and its author time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Point {
    pub commit: String,
    pub epoch_seconds: i64,
    /// The same instant as RFC 3339 UTC.
    pub at: String,
}

/// A started time, or why none can be given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Started {
    Known(Point),
    Unknown { unknown: String },
}

/// A span in which the task's record said `blocked`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BlockedSpan {
    pub from: Point,
    /// The commit that cleared it; `None` while still blocked.
    pub to: Option<Point>,
}

/// A task record as read at `HEAD`.
#[derive(Debug, Clone)]
pub(super) struct Record {
    pub id: String,
    pub path: String,
    pub title: String,
    pub work_type: String,
    pub epic_id: Option<String>,
    pub status: String,
    pub target: String,
    /// The active acceptance block's reviewed commit, as written.
    pub reviewed: Option<String>,
}

/// The derived timings of one record.
#[derive(Debug, Clone, Default)]
pub(super) struct Timings {
    pub planned: Option<Point>,
    pub started: Option<Started>,
    pub blocked: Vec<BlockedSpan>,
    pub completed: Option<Point>,
    pub cancelled: Option<Point>,
    pub landed: Option<Point>,
}

/// The first-parent line of an integration target, oldest first, and for
/// every commit the line holds, the index of the first first-parent commit
/// that contains it.
struct Line {
    tip: Oid,
    first_parents: Vec<Oid>,
    entered: HashMap<Oid, usize>,
}

pub(super) struct History<'r> {
    repo: &'r Repository,
    /// Per record path: each changing commit with the status it wrote
    /// (empty when the record is absent), parents first.
    changes: HashMap<String, Vec<(Oid, String)>>,
    lines: HashMap<String, Option<Line>>,
}

const TASKS: &str = "project-management/tasks";
const EPICS: &str = "project-management/epics";

/// The complete and cancelled task records at `HEAD`.
pub(super) fn records(repo: &Repository) -> Result<Vec<Record>, String> {
    // An unborn HEAD has no history and so no outcome yet.
    let Ok(head) = repo.head().and_then(|head| head.peel_to_commit()) else {
        return Ok(Vec::new());
    };
    let tree = head.tree().map_err(|error| error.message().to_string())?;
    let mut paths = Vec::new();
    if let Ok(entry) = tree.get_path(std::path::Path::new(TASKS)) {
        paths.extend(markdown_names(repo, entry.id()).map(|name| format!("{TASKS}/{name}")));
    }
    if let Ok(entry) = tree.get_path(std::path::Path::new(EPICS)) {
        if let Ok(epics) = repo.find_tree(entry.id()) {
            for epic in &epics {
                let (Some(name), Some(git2::ObjectType::Tree)) = (epic.name().ok(), epic.kind())
                else {
                    continue;
                };
                let Ok(nested) = repo
                    .find_tree(epic.id())
                    .and_then(|nested| nested.get_path(std::path::Path::new("tasks")))
                else {
                    continue;
                };
                paths.extend(
                    markdown_names(repo, nested.id())
                        .map(|file| format!("{EPICS}/{name}/tasks/{file}")),
                );
            }
        }
    }
    let mut out = Vec::new();
    for path in paths {
        let Ok(blob) = tree
            .get_path(std::path::Path::new(&path))
            .and_then(|entry| repo.find_blob(entry.id()))
        else {
            continue;
        };
        if let Some(record) = parse_record(&path, blob.content()) {
            if matches!(record.status.as_str(), "complete" | "cancelled") {
                out.push(record);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn markdown_names(repo: &Repository, tree: Oid) -> impl Iterator<Item = String> {
    let names: Vec<String> = repo
        .find_tree(tree)
        .map(|tree| {
            tree.iter()
                .filter(|entry| entry.kind() == Some(git2::ObjectType::Blob))
                .filter_map(|entry| entry.name().ok().map(str::to_string))
                .filter(|name: &String| {
                    name.strip_suffix(".md")
                        .is_some_and(crate::workgraph::is_valid_task_format_id)
                })
                .collect()
        })
        .unwrap_or_default();
    names.into_iter()
}

fn parse_record(path: &str, bytes: &[u8]) -> Option<Record> {
    let (data, body) = crate::validate::parse_frontmatter(bytes).ok()?;
    let field = |key: &str| {
        let value = crate::validate::get_string_field(&data, key);
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    };
    let id = field("id")
        .or_else(|| field("format_id"))
        .filter(|id| crate::workgraph::is_valid_task_format_id(id))?;
    let body = String::from_utf8_lossy(&body);
    let mut active = acceptance_blocks(&body)
        .into_iter()
        .filter(|block| !block.is_superseded())
        .filter_map(|block| block.parsed.ok());
    let reviewed = match (active.next(), active.next()) {
        (Some(block), None) => Some(block.reviewed),
        _ => None,
    };
    Some(Record {
        id,
        path: path.to_string(),
        title: field("title").unwrap_or_default(),
        work_type: field("work_type").unwrap_or_else(|| "unset".to_string()),
        epic_id: field("epic_id"),
        status: field("status").unwrap_or_default(),
        target: field("integration_target").unwrap_or_else(|| "main".to_string()),
        reviewed,
    })
}

fn status_of(bytes: &[u8]) -> String {
    crate::validate::parse_frontmatter(bytes)
        .map(|(data, _)| {
            crate::validate::get_string_field(&data, "status")
                .trim()
                .to_string()
        })
        .unwrap_or_default()
}

impl<'r> History<'r> {
    /// Walk once from `HEAD` and every record's target, collecting each
    /// record's changes.
    pub(super) fn read(repo: &'r Repository, records: &[Record]) -> Result<Self, String> {
        let mut lines = HashMap::new();
        for record in records {
            lines
                .entry(record.target.clone())
                .or_insert_with(|| line(repo, &record.target));
        }
        let paths: BTreeSet<&str> = records.iter().map(|record| record.path.as_str()).collect();
        let mut walk = repo
            .revwalk()
            .map_err(|error| error.message().to_string())?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
            .map_err(|error| error.message().to_string())?;
        walk.push_head()
            .map_err(|error| error.message().to_string())?;
        for line in lines.values().flatten() {
            walk.push(line.tip)
                .map_err(|error| error.message().to_string())?;
        }
        let mut changes: HashMap<String, Vec<(Oid, String)>> = HashMap::new();
        let mut statuses: HashMap<Oid, String> = HashMap::new();
        let mut homes = Homes::default();
        for oid in walk {
            let oid = oid.map_err(|error| error.message().to_string())?;
            let commit = repo
                .find_commit(oid)
                .map_err(|error| error.message().to_string())?;
            // A merge only carries edits made on its parents' lines; counting
            // it would replay an older line's status as a new transition.
            if commit.parent_count() > 1 {
                continue;
            }
            let now = homes.subtrees(&commit);
            let before = commit
                .parent(0)
                .ok()
                .map(|parent| homes.subtrees(&parent))
                .unwrap_or_default();
            if now == before {
                continue;
            }
            let now = homes.blobs(repo, now, &paths);
            let before = homes.blobs(repo, before, &paths);
            for path in &paths {
                let (after, prior) = (now.get(*path), before.get(*path));
                if after == prior {
                    continue;
                }
                let status = after.map_or_else(String::new, |blob| {
                    statuses
                        .entry(*blob)
                        .or_insert_with(|| {
                            repo.find_blob(*blob)
                                .map(|blob| status_of(blob.content()))
                                .unwrap_or_default()
                        })
                        .clone()
                });
                changes
                    .entry((*path).to_string())
                    .or_default()
                    .push((oid, status));
            }
        }
        Ok(Self {
            repo,
            changes,
            lines,
        })
    }

    /// Derive one record's timings.
    pub(super) fn timings(&self, record: &Record) -> Timings {
        let mut timings = Timings::default();
        let empty = Vec::new();
        let changes = self.changes.get(&record.path).unwrap_or(&empty);
        let mut previous = String::new();
        let mut added = None;
        let mut open_block: Option<Point> = None;
        for (commit, status) in changes {
            if added.is_none() && !status.is_empty() {
                added = Some(*commit);
            }
            if *status == previous {
                continue;
            }
            let point = self.point(*commit);
            if previous == "blocked" {
                if let Some(from) = open_block.take() {
                    timings.blocked.push(BlockedSpan {
                        from,
                        to: point.clone(),
                    });
                }
            }
            match status.as_str() {
                "blocked" => open_block = point,
                "complete" => timings.completed = point,
                "cancelled" => timings.cancelled = point,
                _ => {}
            }
            previous.clone_from(status);
        }
        if let Some(from) = open_block {
            timings.blocked.push(BlockedSpan { from, to: None });
        }
        let line = self.lines.get(&record.target).and_then(Option::as_ref);
        let completion = timings
            .completed
            .as_ref()
            .and_then(|point| Oid::from_str(&point.commit).ok());
        let landing = match (line, completion) {
            (Some(line), Some(done)) if record.status == "complete" => {
                Self::first_containing(line, done)
            }
            _ => None,
        };
        timings.landed = landing.and_then(|oid| self.point(oid));
        timings.planned = added.and_then(|added| {
            let on_target = line.and_then(|line| Self::first_containing(line, added));
            match on_target {
                Some(merge) if Some(merge) != landing => self.point(merge),
                _ => self.point(added),
            }
        });
        if record.status == "complete" {
            timings.started = Some(self.started(record, line, landing));
        }
        timings
    }

    fn started(&self, record: &Record, line: Option<&Line>, landing: Option<Oid>) -> Started {
        let unknown = |reason: &str| Started::Unknown {
            unknown: reason.to_string(),
        };
        let Some(reviewed) = record.reviewed.as_deref() else {
            return unknown(
                "the record has no single readable acceptance block naming its reviewed commit",
            );
        };
        let Ok(reviewed) =
            crate::workgraph::work_start::commit_by_object_id(self.repo, reviewed.trim())
                .map(|commit| commit.id())
        else {
            return unknown("the reviewed commit is not in this clone");
        };
        let base = match (landing, line) {
            (Some(landing), _) => {
                let Ok(merge) = self.repo.find_commit(landing) else {
                    return unknown("the landing commit cannot be read");
                };
                if merge.parent_count() < 2 {
                    return unknown(
                        "landed without a merge commit (squash, rebase or fast-forward), so the task branch is not on the target",
                    );
                }
                let entered = line.and_then(|line| {
                    let landed_at = line.entered.get(&landing)?;
                    line.entered.get(&reviewed).map(|at| at <= landed_at)
                });
                if entered != Some(true) {
                    return unknown(
                        "the reviewed commit is not in the landed history (squash or rebase)",
                    );
                }
                match merge.parent_id(0) {
                    Ok(base) => base,
                    Err(_) => return unknown("the landing commit has no first parent"),
                }
            }
            (None, Some(line)) => line.tip,
            (None, None) => {
                return unknown(&format!(
                    "the integration target `{}` does not resolve in this clone",
                    record.target
                ));
            }
        };
        let Ok(mut walk) = self.repo.revwalk() else {
            return unknown("the task branch history cannot be read");
        };
        if walk.push(reviewed).is_err() || walk.hide(base).is_err() {
            return unknown("the task branch history cannot be read");
        }
        let commits: Vec<Oid> = walk.filter_map(Result::ok).collect();
        if commits.is_empty() {
            return unknown(
                "the reviewed commit was on the target before the record was completed there",
            );
        }
        if commits.iter().all(|commit| *commit == reviewed) {
            return unknown("no commit before the reviewed one on the task branch");
        }
        commits
            .iter()
            .filter_map(|commit| self.point(*commit))
            .min_by_key(|point| point.epoch_seconds)
            .map_or_else(
                || unknown("the task branch history cannot be read"),
                Started::Known,
            )
    }

    /// The oldest first-parent commit of `line` that contains `commit`.
    fn first_containing(line: &Line, commit: Oid) -> Option<Oid> {
        line.entered
            .get(&commit)
            .map(|index| line.first_parents[*index])
    }

    fn point(&self, oid: Oid) -> Option<Point> {
        let commit = self.repo.find_commit(oid).ok()?;
        let seconds = commit.author().when().seconds();
        Some(Point {
            commit: oid.to_string(),
            epoch_seconds: seconds,
            at: rfc3339(seconds),
        })
    }
}

/// RFC 3339 UTC for epoch seconds; times before 1970 print as the epoch.
pub(super) fn rfc3339(seconds: i64) -> String {
    let seconds = u64::try_from(seconds).unwrap_or(0);
    crate::workgraph::rfc3339_at(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds))
}

/// Resolve an integration target: the local branch and the remote-tracking
/// branch of that name, preferring whichever contains the other.
fn line(repo: &Repository, target: &str) -> Option<Line> {
    let names = crate::workgraph::work_start::target_reference_names(target)?;
    let tips: Vec<Oid> = names
        .iter()
        .filter_map(|name| repo.find_reference(name).ok()?.peel_to_commit().ok())
        .map(|commit| commit.id())
        .collect();
    let tip = match tips.as_slice() {
        [] => return None,
        [one] => *one,
        [first, second, ..] => {
            if repo.graph_descendant_of(*second, *first).unwrap_or(false) {
                *second
            } else {
                *first
            }
        }
    };
    let mut first_parents = Vec::new();
    let mut current = repo.find_commit(tip).ok();
    while let Some(commit) = current {
        first_parents.push(commit.id());
        current = commit.parent(0).ok();
    }
    first_parents.reverse();
    let mut entered = HashMap::new();
    for (index, commit) in first_parents.iter().enumerate() {
        let Ok(mut walk) = repo.revwalk() else {
            return None;
        };
        if walk.push(*commit).is_err() {
            return None;
        }
        if index > 0 && walk.hide(first_parents[index - 1]).is_err() {
            return None;
        }
        for oid in walk.filter_map(Result::ok) {
            entered.entry(oid).or_insert(index);
        }
    }
    Some(Line {
        tip,
        first_parents,
        entered,
    })
}

/// The record homes of one commit: the `tasks` and `epics` subtree ids.
type Subtrees = (Option<Oid>, Option<Oid>);

/// Caches of record homes by commit and of record blobs by home, so each
/// tree is read once however many commits share it.
#[derive(Default)]
struct Homes {
    by_commit: HashMap<Oid, Subtrees>,
    blobs: HashMap<Subtrees, std::rc::Rc<BTreeMap<String, Oid>>>,
}

impl Homes {
    fn subtrees(&mut self, commit: &Commit<'_>) -> Subtrees {
        *self.by_commit.entry(commit.id()).or_insert_with(|| {
            let Ok(tree) = commit.tree() else {
                return (None, None);
            };
            let id = |path: &str| {
                tree.get_path(std::path::Path::new(path))
                    .ok()
                    .map(|entry| entry.id())
            };
            (id(TASKS), id(EPICS))
        })
    }

    fn blobs(
        &mut self,
        repo: &Repository,
        homes: Subtrees,
        paths: &BTreeSet<&str>,
    ) -> std::rc::Rc<BTreeMap<String, Oid>> {
        self.blobs
            .entry(homes)
            .or_insert_with(|| std::rc::Rc::new(record_blobs(repo, homes, paths)))
            .clone()
    }
}

/// The blob of each wanted record path in these homes.
fn record_blobs(
    repo: &Repository,
    homes: Subtrees,
    paths: &BTreeSet<&str>,
) -> BTreeMap<String, Oid> {
    let mut out = BTreeMap::new();
    let mut entries = |tree: Oid, prefix: &str| {
        if let Ok(tree) = repo.find_tree(tree) {
            for entry in &tree {
                let Ok(name) = entry.name() else {
                    continue;
                };
                let path = format!("{prefix}/{name}");
                if paths.contains(path.as_str()) {
                    out.insert(path, entry.id());
                }
            }
        }
    };
    if let Some(tasks) = homes.0 {
        entries(tasks, TASKS);
    }
    let nested: Vec<(String, Oid)> = homes
        .1
        .and_then(|epics| repo.find_tree(epics).ok())
        .map(|epics| {
            epics
                .iter()
                .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
                .filter_map(|entry| Some((entry.name().ok()?.to_string(), entry.id())))
                .collect()
        })
        .unwrap_or_default();
    for (epic, tree) in nested {
        let prefix = format!("{EPICS}/{epic}/tasks");
        if !paths.iter().any(|path| path.starts_with(&prefix)) {
            continue;
        }
        let Ok(tasks) = repo
            .find_tree(tree)
            .and_then(|tree| tree.get_path(std::path::Path::new("tasks")))
        else {
            continue;
        };
        entries(tasks.id(), &prefix);
    }
    out
}
