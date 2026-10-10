//! Outcome timings derived from git history, never recorded.
//!
//! The task records are read from `HEAD`. A record's lifecycle is the
//! sequence of non-merge commits in `HEAD`'s history, parents first, whose
//! status for that task id differs from their parent's, so a text edit or a
//! move between record layouts is no transition, and a merge, which only
//! carries edits made on its parents' lines, is never one. A record whose
//! status changed only in a merge's conflict resolution is not seen. Each
//! task's integration target is read only to place landings on its
//! first-parent line. Times are author times, which a rebase keeps.

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
    /// The code landed before the completion reached the target (a later
    /// records change wrote it), so active work ends at the landing.
    pub ended_at_landing: bool,
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
    /// Per task id: each non-merge commit whose status differs from its
    /// parent's, with the status before and after (empty when the record is
    /// absent), parents first.
    changes: HashMap<String, Vec<(Oid, String, String)>>,
    lines: HashMap<String, Option<Line>>,
}

const TASKS: &str = "project-management/tasks";
const EPICS: &str = "project-management/epics";

fn message(error: &git2::Error) -> String {
    error.message().to_string()
}

/// The tree at `path` under `tree`: `None` when nothing is there or the
/// entry is not a directory; an error when the read fails (issue 79).
fn subtree_id(tree: &git2::Tree<'_>, path: &str) -> Result<Option<Oid>, String> {
    match tree.get_path(std::path::Path::new(path)) {
        Ok(entry) => Ok((entry.kind() == Some(git2::ObjectType::Tree)).then(|| entry.id())),
        Err(error) if error.code() == git2::ErrorCode::NotFound => Ok(None),
        Err(error) => Err(format!("cannot read {path}: {}", error.message())),
    }
}

/// The complete and cancelled task records at `HEAD`.
///
/// # Errors
///
/// A tree, record or name the history needs that cannot be read: the
/// report then says so instead of leaving tasks out (issue 79).
pub(super) fn records(repo: &Repository) -> Result<Vec<Record>, String> {
    // An unborn HEAD has no history and so no outcome yet.
    let head = match repo.head() {
        Ok(head) => head,
        Err(error) if error.code() == git2::ErrorCode::UnbornBranch => return Ok(Vec::new()),
        Err(error) => return Err(format!("cannot read HEAD: {}", error.message())),
    };
    let head = head.peel_to_commit().map_err(|error| message(&error))?;
    let tree = head.tree().map_err(|error| message(&error))?;
    let mut paths = Vec::new();
    if let Some(tasks) = subtree_id(&tree, TASKS)? {
        paths.extend(
            markdown_names(repo, tasks)?
                .into_iter()
                .map(|name| format!("{TASKS}/{name}")),
        );
    }
    if let Some(epics) = subtree_id(&tree, EPICS)? {
        let epics = repo.find_tree(epics).map_err(|error| message(&error))?;
        for epic in &epics {
            if epic.kind() != Some(git2::ObjectType::Tree) {
                continue;
            }
            let nested = repo.find_tree(epic.id()).map_err(|error| message(&error))?;
            let Some(tasks) = subtree_id(&nested, "tasks")? else {
                continue;
            };
            // An epic directory that holds task records must be named in
            // text, or its records would drop out of the report.
            let epic_name = crate::git::GitName::from_bytes(epic.name_bytes());
            let name = epic_name.rule_text().map_err(|_| {
                format!(
                    "the epic directory {} has a name that is not UTF-8, so its task records cannot be read",
                    epic_name.display()
                )
            })?;
            paths.extend(
                markdown_names(repo, tasks)?
                    .into_iter()
                    .map(|file| format!("{EPICS}/{name}/tasks/{file}")),
            );
        }
    }
    let mut out = Vec::new();
    for path in paths {
        let blob = tree
            .get_path(std::path::Path::new(&path))
            .and_then(|entry| repo.find_blob(entry.id()))
            .map_err(|error| format!("cannot read {path}: {}", error.message()))?;
        if let Some(record) = parse_record(&path, blob.content())? {
            if matches!(record.status.as_str(), "complete" | "cancelled") {
                out.push(record);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// The task record file names in `tree`.
///
/// # Errors
///
/// The tree cannot be read.
fn markdown_names(repo: &Repository, tree: Oid) -> Result<Vec<String>, String> {
    let tree = repo.find_tree(tree).map_err(|error| message(&error))?;
    Ok(tree
        .iter()
        .filter(|entry| entry.kind() == Some(git2::ObjectType::Blob))
        // A task record's file name is its id (`TSK-NNN.md`), which is
        // ASCII, so a name that is not UTF-8 is no record.
        .filter_map(|entry| {
            crate::git::GitName::from_bytes(entry.name_bytes())
                .rule_text()
                .ok()
                .map(str::to_string)
        })
        .filter(|name: &String| {
            name.strip_suffix(".md")
                .is_some_and(crate::workgraph::is_valid_task_format_id)
        })
        .collect())
}

/// The record at `path`, or `None` when its frontmatter names no task.
///
/// # Errors
///
/// A body that is not UTF-8, whose acceptance block cannot be read.
fn parse_record(path: &str, bytes: &[u8]) -> Result<Option<Record>, String> {
    let Ok((data, body)) = crate::validate::parse_frontmatter(bytes) else {
        return Ok(None);
    };
    let field = |key: &str| {
        // A value is trimmed of YAML's own blanks only, so an id, status
        // or target padded with another space character stays itself.
        let value = crate::validate::get_string_field(&data, key);
        let value = value.trim_matches([' ', '\t']);
        (!value.is_empty()).then(|| value.to_string())
    };
    let Some(id) = field("id")
        .or_else(|| field("format_id"))
        .filter(|id| crate::workgraph::is_valid_task_format_id(id))
    else {
        return Ok(None);
    };
    let body = std::str::from_utf8(&body).map_err(|_| format!("{path} is not UTF-8"))?;
    let mut active = acceptance_blocks(body)
        .into_iter()
        .filter(|block| !block.is_superseded())
        .filter_map(|block| block.parsed.ok());
    let reviewed = match (active.next(), active.next()) {
        (Some(block), None) => Some(block.reviewed),
        _ => None,
    };
    Ok(Some(Record {
        id,
        path: path.to_string(),
        title: field("title").unwrap_or_default(),
        work_type: field("work_type").unwrap_or_else(|| "unset".to_string()),
        epic_id: field("epic_id"),
        status: field("status").unwrap_or_default(),
        target: field("integration_target").unwrap_or_else(|| "main".to_string()),
        reviewed,
    }))
}

/// A record blob's task id and status, when its frontmatter names a task.
fn id_and_status(bytes: &[u8]) -> Option<(String, String)> {
    let (data, _) = crate::validate::parse_frontmatter(bytes).ok()?;
    let field = |key: &str| {
        crate::validate::get_string_field(&data, key)
            .trim_matches([' ', '\t'])
            .to_string()
    };
    let id = Some(field("id"))
        .filter(|id| crate::workgraph::is_valid_task_format_id(id))
        .or_else(|| {
            Some(field("format_id")).filter(|id| crate::workgraph::is_valid_task_format_id(id))
        })?;
    Some((id, field("status")))
}

impl<'r> History<'r> {
    /// Walk once from `HEAD` and every record's target, collecting each
    /// record's changes.
    pub(super) fn read(repo: &'r Repository, records: &[Record]) -> Result<Self, String> {
        let mut lines = HashMap::new();
        for record in records {
            if !lines.contains_key(&record.target) {
                lines.insert(record.target.clone(), line(repo, &record.target)?);
            }
        }
        let ids: BTreeSet<&str> = records.iter().map(|record| record.id.as_str()).collect();
        // Only HEAD's history: the records were read at HEAD, so their
        // lifecycle is the one HEAD holds. A target ahead of HEAD may carry
        // another lifecycle (a reopen and re-completion); its line is used
        // only to place landings, never to read transitions.
        let mut walk = repo
            .revwalk()
            .map_err(|error| error.message().to_string())?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
            .map_err(|error| error.message().to_string())?;
        walk.push_head()
            .map_err(|error| error.message().to_string())?;
        let mut changes: HashMap<String, Vec<(Oid, String, String)>> = HashMap::new();
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
            let now = homes.subtrees(&commit)?;
            let before = if commit.parent_count() == 0 {
                (None, None)
            } else {
                let parent = commit.parent(0).map_err(|error| message(&error))?;
                homes.subtrees(&parent)?
            };
            if now == before {
                continue;
            }
            // Statuses by task id, so a record moved between the flat and
            // nested layouts keeps its history.
            let now = homes.statuses(repo, now)?;
            let before = homes.statuses(repo, before)?;
            for id in &ids {
                let empty = String::new();
                let to = now.get(*id).unwrap_or(&empty);
                let from = before.get(*id).unwrap_or(&empty);
                // A text-only edit or a move keeps the status; it is no
                // transition, whichever line it is on.
                if to != from {
                    changes.entry((*id).to_string()).or_default().push((
                        oid,
                        from.clone(),
                        to.clone(),
                    ));
                }
            }
        }
        Ok(Self {
            repo,
            changes,
            lines,
        })
    }

    /// Derive one record's timings.
    ///
    /// # Errors
    ///
    /// A commit the history walked that can no longer be read.
    pub(super) fn timings(&self, record: &Record) -> Result<Timings, String> {
        let mut timings = Timings::default();
        let empty = Vec::new();
        let changes = self.changes.get(&record.id).unwrap_or(&empty);
        let mut added = None;
        let mut open_block: Option<Point> = None;
        // Completions since the last reopen; parallel lines (a squash on the
        // target and the task branch itself) can each carry one.
        let mut completions: Vec<Oid> = Vec::new();
        for (commit, from, to) in changes {
            if from == "complete" {
                completions.clear();
            }
            if to == "complete" {
                completions.push(*commit);
            }
            if added.is_none() && from.is_empty() && !to.is_empty() {
                added = Some(*commit);
            }
            let point = Some(self.point(*commit)?);
            if from == "blocked" {
                if let Some(from) = open_block.take() {
                    timings.blocked.push(BlockedSpan {
                        from,
                        to: point.clone(),
                    });
                }
            }
            match to.as_str() {
                "blocked" => open_block = point,
                "cancelled" => timings.cancelled = point,
                _ => {}
            }
        }
        if let Some(from) = open_block {
            timings.blocked.push(BlockedSpan { from, to: None });
        }
        let line = self.lines.get(&record.target).and_then(Option::as_ref);
        // The completion that reached the target first; else the latest.
        let completion = completions
            .iter()
            .filter_map(|oid| Some((line?.entered.get(oid).copied()?, *oid)))
            .min()
            .map(|(_, oid)| oid)
            .or_else(|| completions.last().copied());
        timings.completed = completion.map(|oid| self.point(oid)).transpose()?;
        // A reviewed commit this clone does not have is no landing evidence;
        // one it has but cannot read refuses the report (issue 79).
        let reviewed = match record.reviewed.as_deref().map(|value| {
            crate::workgraph::work_start::lookup_commit_by_object_id(
                self.repo,
                value.trim_matches([' ', '\t']),
            )
        }) {
            None | Some(Err(crate::workgraph::work_start::CommitLookup::Unresolved(_))) => None,
            Some(Ok(commit)) => Some(commit.id()),
            Some(Err(crate::workgraph::work_start::CommitLookup::Unreadable(reason))) => {
                return Err(format!("{}: {reason}", record.path));
            }
        };
        // Where on the target's first-parent line the reviewed commit and the
        // completion first arrive, by ancestry. The code lands with the
        // reviewed commit; a completion that arrives later was written by a
        // records change. A completion that arrives first means the task
        // landed without its reviewed commit (squash or rebase), even if
        // that commit reaches the target later through another branch.
        let at = |oid: Option<Oid>| {
            line.and_then(|line| oid.and_then(|oid| line.entered.get(&oid).copied()))
        };
        let (reviewed_at, completion_at) = (at(reviewed), at(completion));
        let (landing_at, squashed) = if record.status == "complete" {
            match (reviewed_at, completion_at) {
                (Some(code), Some(done)) if done < code => (Some(done), true),
                (Some(code), _) => (Some(code), false),
                (None, Some(done)) => (Some(done), true),
                (None, None) => (None, false),
            }
        } else {
            (None, false)
        };
        let landing = landing_at.and_then(|index| line.map(|line| line.first_parents[index]));
        timings.ended_at_landing = !squashed
            && completion.is_some()
            && landing_at.is_some()
            && completion_at.is_none_or(|done| Some(done) > landing_at);
        timings.landed = landing.map(|oid| self.point(oid)).transpose()?;
        timings.planned = added
            .map(|added| {
                let on_target = line.and_then(|line| Self::first_containing(line, added));
                match on_target {
                    Some(merge) if Some(merge) != landing => self.point(merge),
                    _ => self.point(added),
                }
            })
            .transpose()?;
        if record.status == "complete" {
            timings.started = Some(self.started(record, line, landing, reviewed, squashed));
        }
        Ok(timings)
    }

    fn started(
        &self,
        record: &Record,
        line: Option<&Line>,
        landing: Option<Oid>,
        reviewed: Option<Oid>,
        squashed: bool,
    ) -> Started {
        let unknown = |reason: &str| Started::Unknown {
            unknown: reason.to_string(),
        };
        if record.reviewed.is_none() {
            return unknown(
                "the record has no single readable acceptance block naming its reviewed commit",
            );
        }
        let Some(reviewed) = reviewed else {
            return unknown("the reviewed commit is not in this clone");
        };
        let base = match (landing, line) {
            (Some(landing), Some(_)) => {
                if squashed {
                    return unknown(
                        "landed without its reviewed commit (squash or rebase), so the task branch is not on the target",
                    );
                }
                let Ok(merge) = self.repo.find_commit(landing) else {
                    return unknown("the landing commit cannot be read");
                };
                if landing == reviewed || merge.parent_count() < 2 {
                    return unknown(
                        "landed without a merge commit (squash, rebase or fast-forward), so the task branch is not on the target",
                    );
                }
                match merge.parent_id(0) {
                    Ok(base) => base,
                    Err(_) => return unknown("the landing commit has no first parent"),
                }
            }
            (None, Some(line)) => line.tip,
            _ => {
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
        // A commit the walk cannot read leaves the start unknown, never a
        // shorter branch (issue 79).
        let Ok(commits) = walk.collect::<Result<BTreeSet<Oid>, _>>() else {
            return unknown("the task branch history cannot be read");
        };
        if commits.is_empty() {
            return unknown("the reviewed commit was on the target before the task branch began");
        }
        if commits.iter().all(|commit| *commit == reviewed) {
            return unknown("no commit before the reviewed one on the task branch");
        }
        // The branch begins at a commit with no parent inside the range; by
        // ancestry, not by clock. Several roots (a branch built on others)
        // take the earliest.
        let mut first: Option<Point> = None;
        for oid in &commits {
            let Ok(commit) = self.repo.find_commit(*oid) else {
                return unknown("the task branch history cannot be read");
            };
            if commit.parent_ids().any(|parent| commits.contains(&parent)) {
                continue;
            }
            let Ok(point) = self.point(*oid) else {
                return unknown("the task branch history cannot be read");
            };
            if first
                .as_ref()
                .is_none_or(|known| point.epoch_seconds < known.epoch_seconds)
            {
                first = Some(point);
            }
        }
        let Some(first) = first else {
            return unknown("the task branch history cannot be read");
        };
        match self.point(reviewed) {
            Ok(end) if first.epoch_seconds <= end.epoch_seconds => Started::Known(first),
            Ok(_) => unknown(
                "author times on the task branch run backwards, so its first commit gives no reliable start",
            ),
            Err(_) => unknown("the task branch history cannot be read"),
        }
    }

    /// The oldest first-parent commit of `line` that contains `commit`.
    fn first_containing(line: &Line, commit: Oid) -> Option<Oid> {
        line.entered
            .get(&commit)
            .map(|index| line.first_parents[*index])
    }

    fn point(&self, oid: Oid) -> Result<Point, String> {
        let commit = self
            .repo
            .find_commit(oid)
            .map_err(|error| format!("cannot read commit {oid}: {}", error.message()))?;
        let seconds = commit.author().when().seconds();
        Ok(Point {
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
/// branch of that name, preferring whichever contains the other; `None`
/// when neither resolves.
///
/// # Errors
///
/// A ref, commit or ancestry that cannot be read (issue 79): the landing
/// places then cannot be told.
fn line(repo: &Repository, target: &str) -> Result<Option<Line>, String> {
    let Some(names) = crate::workgraph::work_start::target_reference_names(target) else {
        return Ok(None);
    };
    let unreadable = |error: git2::Error| {
        format!(
            "cannot read the integration target `{target}`: {}",
            error.message()
        )
    };
    let mut tips: Vec<Oid> = Vec::new();
    for name in &names {
        let reference = match repo.find_reference(name) {
            Ok(reference) => reference,
            Err(error) if error.code() == git2::ErrorCode::NotFound => continue,
            Err(error) => return Err(unreadable(error)),
        };
        tips.push(reference.peel_to_commit().map_err(unreadable)?.id());
    }
    let tip = match tips.as_slice() {
        [] => return Ok(None),
        [one] => *one,
        [first, second, ..] => {
            if repo
                .graph_descendant_of(*second, *first)
                .map_err(unreadable)?
            {
                *second
            } else {
                *first
            }
        }
    };
    let mut first_parents = Vec::new();
    let mut current = repo.find_commit(tip).map_err(unreadable)?;
    loop {
        first_parents.push(current.id());
        if current.parent_count() == 0 {
            break;
        }
        current = current.parent(0).map_err(unreadable)?;
    }
    first_parents.reverse();
    let mut entered = HashMap::new();
    for (index, commit) in first_parents.iter().enumerate() {
        let mut walk = repo.revwalk().map_err(unreadable)?;
        walk.push(*commit).map_err(unreadable)?;
        if index > 0 {
            walk.hide(first_parents[index - 1]).map_err(unreadable)?;
        }
        for oid in walk {
            entered.entry(oid.map_err(unreadable)?).or_insert(index);
        }
    }
    Ok(Some(Line {
        tip,
        first_parents,
        entered,
    }))
}

/// The record homes of one commit: the `tasks` and `epics` subtree ids.
type Subtrees = (Option<Oid>, Option<Oid>);

/// Caches of record homes by commit, of each record blob's id and status,
/// and of the status map of each pair of homes, so each tree and blob is
/// read once however many commits share it.
#[derive(Default)]
struct Homes {
    by_commit: HashMap<Oid, Subtrees>,
    blobs: HashMap<Oid, Option<(String, String)>>,
    statuses: HashMap<Subtrees, std::rc::Rc<BTreeMap<String, String>>>,
}

impl Homes {
    fn subtrees(&mut self, commit: &Commit<'_>) -> Result<Subtrees, String> {
        if let Some(found) = self.by_commit.get(&commit.id()) {
            return Ok(*found);
        }
        let tree = commit.tree().map_err(|error| {
            format!(
                "cannot read the tree of {}: {}",
                commit.id(),
                error.message()
            )
        })?;
        let found = (subtree_id(&tree, TASKS)?, subtree_id(&tree, EPICS)?);
        self.by_commit.insert(commit.id(), found);
        Ok(found)
    }

    /// Each task id in these homes with its status; the first path in
    /// layout order wins when two records claim one id.
    ///
    /// # Errors
    ///
    /// A tree or record blob that cannot be read (issue 79), which would
    /// otherwise read as a record removed and invent a transition.
    fn statuses(
        &mut self,
        repo: &Repository,
        homes: Subtrees,
    ) -> Result<std::rc::Rc<BTreeMap<String, String>>, String> {
        if let Some(map) = self.statuses.get(&homes) {
            return Ok(map.clone());
        }
        let mut map = BTreeMap::new();
        for blob in record_blobs(repo, homes)? {
            let parsed = if let Some(parsed) = self.blobs.get(&blob) {
                parsed.clone()
            } else {
                let content = repo.find_blob(blob).map_err(|error| {
                    format!("cannot read record blob {blob}: {}", error.message())
                })?;
                let parsed = id_and_status(content.content());
                self.blobs.insert(blob, parsed.clone());
                parsed
            };
            if let Some((id, status)) = parsed {
                map.entry(id).or_insert(status);
            }
        }
        let map = std::rc::Rc::new(map);
        self.statuses.insert(homes, map.clone());
        Ok(map)
    }
}

/// The blob of every task record in these homes, flat layout first.
///
/// # Errors
///
/// A directory that cannot be read.
fn record_blobs(repo: &Repository, homes: Subtrees) -> Result<Vec<Oid>, String> {
    let read = |tree: Oid| {
        repo.find_tree(tree)
            .map_err(|error| format!("cannot read records tree {tree}: {}", error.message()))
    };
    let mut out = Vec::new();
    let mut entries = |tree: &git2::Tree<'_>| {
        for entry in tree {
            // A task record's file name is its id (`TSK-NNN.md`), which is
            // ASCII, so a name that is not UTF-8 is no record.
            let is_record = entry.kind() == Some(git2::ObjectType::Blob)
                && crate::git::GitName::from_bytes(entry.name_bytes())
                    .rule_text()
                    .is_ok_and(|name| {
                        name.strip_suffix(".md")
                            .is_some_and(crate::workgraph::is_valid_task_format_id)
                    });
            if is_record {
                out.push(entry.id());
            }
        }
    };
    if let Some(tasks) = homes.0 {
        entries(&read(tasks)?);
    }
    if let Some(epics) = homes.1 {
        for epic in &read(epics)? {
            if epic.kind() != Some(git2::ObjectType::Tree) {
                continue;
            }
            if let Some(tasks) = subtree_id(&read(epic.id())?, "tasks")? {
                entries(&read(tasks)?);
            }
        }
    }
    Ok(out)
}
