//! A record's status judged one step per commit (TSK-1036, issues #400 and
//! #424).
//!
//! A range is judged on each commit's own change, never only on its
//! endpoints: every commit is compared with each of its parents. A
//! parent-to-commit change is accepted when it is one legal transition, or,
//! for a merge that took another parent's status, when that parent's
//! first-parent chain from the point it meets this parent's history is a
//! chain of legal steps and this parent still holds the status of that
//! meeting point. So a sync that brings `blocked -> todo -> complete` from
//! the target passes; the same history merged in one step or two gets the
//! same verdict; and a terminal status entered and left inside one range
//! (`todo -> cancelled -> todo`) is refused although its endpoints match.
//!
//! The record is followed by its id, at every path a record of its kind
//! may live at, so moving it between record paths is no gap. A record that
//! disappears (deleted, or parked off the record paths) and comes back is
//! judged from the status it last held before the gap, never as a new
//! record: `todo -> cancelled`, a deletion, then `todo` is refused like the
//! direct edit. Only a record no earlier commit of the range, nor the
//! boundary it starts from, holds is a creation.
//!
//! The judgment covers the history supplied and verified. History that
//! cannot be read (a shallow boundary, a missing object, an unparsable
//! record, an overlay) refuses with its cause; it never falls back to the
//! endpoints. No ref is consulted, so no remote-tracking ref or
//! default-branch tip exempts a step. A rebase or squash that erased a step
//! cannot be recovered and is not claimed. This proves transition legality
//! only, never review or authorship.

use std::collections::{HashMap, HashSet};

use git2::{Oid, Repository};

use super::history;
use super::lifecycle::RecordView;
use super::work_start::RecordKind;

/// The start of every refusal for status history that cannot be read.
pub const UNREADABLE: &str = "status history cannot be read";

/// Why the status history could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The clone is shallow inside the history the judgment reads.
    Shallow,
    /// A commit, tree or blob is missing from the object store.
    MissingObject,
    /// A version of the record does not parse.
    Unparsable,
    /// A graft or replace ref overlays the recorded history.
    Overlay,
}

impl Cause {
    /// The label printed in the refusal, which [`remedy_for`] reads back.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Shallow => "shallow history",
            Self::MissingObject => "missing object",
            Self::Unparsable => "unparsable record",
            Self::Overlay => "history overlay",
        }
    }

    fn all() -> [Self; 4] {
        [
            Self::Shallow,
            Self::MissingObject,
            Self::Unparsable,
            Self::Overlay,
        ]
    }
}

/// The step that clears an unreadable-history refusal, by its cause, or
/// `None` for a message this module did not write. Each names its cause, so
/// a fetch is never offered for a record that only an edit repairs.
#[must_use]
pub fn remedy_for(message: &str) -> Option<&'static str> {
    let cause = Cause::all()
        .into_iter()
        .find(|cause| message.contains(&format!("{UNREADABLE} ({}):", cause.label())))?;
    Some(match cause {
        Cause::Shallow => "the clone is shallow: fetch the full history (`git fetch --unshallow`, or a checkout with full depth), then judge the range again",
        Cause::MissingObject => "an object is missing: fetch it (`git fetch`) or repair the object store (`git fsck` names what is lost), then judge the range again",
        Cause::Unparsable => "a version of the record does not parse: fix that record in the commit the message names (rewrite the branch with `git rebase -i` before review); a fetch does not repair it",
        Cause::Overlay => "a graft or replace ref overlays the history: remove it (`git replace -d <object>`), or judge from a clean clone",
    })
}

/// History that cannot be read, with its cause.
#[derive(Debug, Clone)]
pub struct Unreadable {
    pub cause: Cause,
    pub detail: String,
}

impl Unreadable {
    fn new(cause: Cause, detail: impl std::fmt::Display) -> Self {
        Self {
            cause,
            detail: detail.to_string(),
        }
    }

    fn git(error: &git2::Error) -> Self {
        let text = error.message().to_string();
        if text.contains("shallow") {
            Self::new(Cause::Shallow, text)
        } else if text.contains("cannot verify history") {
            Self::new(Cause::Overlay, text)
        } else {
            Self::new(Cause::MissingObject, text)
        }
    }

    /// The refusal text.
    #[must_use]
    pub fn message(&self) -> String {
        format!("{UNREADABLE} ({}): {}", self.cause.label(), self.detail)
    }
}

/// The steps of one record over a range.
#[derive(Debug, Default)]
pub struct Steps {
    /// One refusal per illegal step, or the one unreadable-history refusal.
    pub problems: Vec<String>,
    /// Every step was read and legal, and the record was present at every
    /// commit and parent read: the range's status change is proven step by
    /// step, so a refusal of its endpoints alone may be lifted.
    pub proven: bool,
}

/// Judge the record `path` names over `base..head`, followed by its id (the
/// file stem) wherever a record of `kind` may live.
#[must_use]
pub fn judge(repo: &Repository, base: Oid, head: Oid, kind: RecordKind, path: &str) -> Steps {
    let _judgment = history::judgment();
    let mut walker = Walker::new(repo, kind, stem(path));
    match walker.range(base, head) {
        Ok(problems) => Steps {
            proven: problems.is_empty() && !walker.absent,
            problems,
        },
        Err(unreadable) => Steps {
            problems: vec![unreadable.message()],
            proven: false,
        },
    }
}

/// The statuses a change made on top of `tip` starts from (the working
/// tree's step): the record's status at `tip`, or, where `tip` holds none,
/// the status each nearest earlier holder in `base..tip` (or the boundary)
/// had; `None` alone for a record never held there.
///
/// # Errors
/// Returns the unreadable-history refusal.
pub fn starts_at(
    repo: &Repository,
    base: Oid,
    tip: Oid,
    kind: RecordKind,
    path: &str,
) -> Result<Vec<Option<String>>, String> {
    let _judgment = history::judgment();
    let mut walker = Walker::new(repo, kind, stem(path));
    walker
        .range(base, tip)
        .map_err(|unreadable| unreadable.message())?;
    walker
        .starts(tip)
        .map_err(|unreadable| unreadable.message())
}

fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default()
        .to_string()
}

/// The status `kind` records start from when created.
fn created(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Task => "todo",
        RecordKind::Epic | RecordKind::Spec => "draft",
    }
}

/// Whether `from -> to` is one legal step for `kind`.
///
/// # Errors
/// Returns the reason the transition is refused.
pub fn step_allowed(kind: RecordKind, from: Option<&str>, to: &str) -> Result<(), &'static str> {
    let from = from.unwrap_or(created(kind));
    if from == to {
        return Ok(());
    }
    match kind {
        RecordKind::Task => super::lifecycle::task_transition_allowed(from, to),
        RecordKind::Epic => super::lifecycle::epic_transition_allowed(from, to),
        RecordKind::Spec => super::lifecycle::spec_transition_allowed(from, to),
    }
}

struct Walker<'r> {
    repo: &'r Repository,
    kind: RecordKind,
    id: String,
    /// The record file (blob and path) holding the id at each commit.
    found: HashMap<Oid, Option<(Oid, String)>>,
    statuses: HashMap<Oid, String>,
    steps: HashMap<(Oid, usize), bool>,
    /// Steps being judged, so a chain that reaches itself is refused.
    open: HashSet<(Oid, usize)>,
    /// The commits of the range, which a gap's look-back walks through.
    in_range: HashSet<Oid>,
    absent: bool,
}

fn short(oid: Oid) -> String {
    oid.to_string()[..12].to_string()
}

/// Whether `name` is the record file of `id`: `<id>.md`, any case of `md`.
fn names_record(name: &str, id: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, extension)| stem == id && extension.eq_ignore_ascii_case("md"))
}

impl<'r> Walker<'r> {
    fn new(repo: &'r Repository, kind: RecordKind, id: String) -> Self {
        Self {
            repo,
            kind,
            id,
            found: HashMap::new(),
            statuses: HashMap::new(),
            steps: HashMap::new(),
            open: HashSet::new(),
            in_range: HashSet::new(),
            absent: false,
        }
    }

    /// The subtree at `path` under `tree`, `None` when absent or not a tree.
    fn subtree(
        &self,
        tree: &git2::Tree<'_>,
        path: &str,
    ) -> Result<Option<git2::Tree<'r>>, Unreadable> {
        let missing = |error: git2::Error| Unreadable::new(Cause::MissingObject, error.message());
        let entry = match tree.get_path(std::path::Path::new(path)) {
            Ok(entry) => entry,
            Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
            Err(error) => return Err(missing(error)),
        };
        if entry.kind() != Some(git2::ObjectType::Tree) {
            return Ok(None);
        }
        self.repo.find_tree(entry.id()).map(Some).map_err(missing)
    }

    /// Every record file of the id in `tree` (under `dir`), as blob and path.
    fn files_in(&self, tree: &git2::Tree<'_>, dir: &str, into: &mut Vec<(Oid, String)>) {
        for entry in tree {
            if entry.kind() != Some(git2::ObjectType::Blob) {
                continue;
            }
            if let Some(name) = entry
                .name()
                .ok()
                .filter(|name| names_record(name, &self.id))
            {
                into.push((entry.id(), format!("{dir}/{name}")));
            }
        }
    }

    /// The record file holding the id at `commit`, at any path a record of
    /// this kind may live at; `None` when no such file exists. Two files
    /// holding one id cannot be told apart, so they refuse.
    fn locate(&mut self, commit: Oid) -> Result<Option<(Oid, String)>, Unreadable> {
        if let Some(found) = self.found.get(&commit) {
            return Ok(found.clone());
        }
        let missing = |error: git2::Error| Unreadable::new(Cause::MissingObject, error.message());
        let tree = self
            .repo
            .find_commit(commit)
            .and_then(|found| found.tree())
            .map_err(missing)?;
        let mut files = Vec::new();
        match self.kind {
            RecordKind::Task => {
                let dir = "project-management/tasks";
                if let Some(tasks) = self.subtree(&tree, dir)? {
                    self.files_in(&tasks, dir, &mut files);
                }
            }
            RecordKind::Spec => {
                let dir = "project-management/specs";
                if let Some(specs) = self.subtree(&tree, dir)? {
                    self.files_in(&specs, dir, &mut files);
                }
            }
            RecordKind::Epic => {}
        }
        let epics_dir = "project-management/epics";
        if let Some(epics) = self.subtree(&tree, epics_dir)? {
            if self.kind == RecordKind::Epic {
                self.files_in(&epics, epics_dir, &mut files);
            }
            let epic_dirs = epics
                .iter()
                .filter(|entry| entry.kind() == Some(git2::ObjectType::Tree))
                .filter_map(|entry| entry.name().ok().map(str::to_string))
                .collect::<Vec<_>>();
            for epic in epic_dirs {
                let (dir, wanted) = match self.kind {
                    RecordKind::Task => (format!("{epics_dir}/{epic}/tasks"), true),
                    RecordKind::Epic => (format!("{epics_dir}/{epic}"), epic == self.id),
                    RecordKind::Spec => (String::new(), false),
                };
                if !wanted {
                    continue;
                }
                if let Some(sub) = self.subtree(&tree, &dir)? {
                    self.files_in(&sub, &dir, &mut files);
                }
            }
        }
        if files.len() > 1 {
            let paths = files
                .iter()
                .map(|(_, path)| path.as_str())
                .collect::<Vec<_>>();
            return Err(Unreadable::new(
                Cause::Unparsable,
                format!(
                    "{} is held by more than one record file at {}: {}",
                    self.id,
                    short(commit),
                    paths.join(", ")
                ),
            ));
        }
        let found = files.pop();
        self.found.insert(commit, found.clone());
        Ok(found)
    }

    fn guarded_parents(&self, commit: Oid) -> Result<Vec<Oid>, Unreadable> {
        let found = self
            .repo
            .find_commit(commit)
            .map_err(|error| Unreadable::new(Cause::MissingObject, error.message()))?;
        history::parents(self.repo, &found).map_err(|error| Unreadable::git(&error))
    }

    /// The record's blob at `commit`, `None` when no record holds the id.
    fn blob(&mut self, commit: Oid) -> Result<Option<Oid>, Unreadable> {
        Ok(self.locate(commit)?.map(|(blob, _)| blob))
    }

    /// The record's status at `commit`, `None` when absent there.
    fn status(&mut self, commit: Oid) -> Result<Option<String>, Unreadable> {
        let Some((blob, path)) = self.locate(commit)? else {
            self.absent = true;
            return Ok(None);
        };
        if let Some(status) = self.statuses.get(&blob) {
            return Ok(Some(status.clone()));
        }
        let found = self
            .repo
            .find_blob(blob)
            .map_err(|error| Unreadable::new(Cause::MissingObject, error.message()))?;
        let text = std::str::from_utf8(found.content()).map_err(|error| {
            Unreadable::new(
                Cause::Unparsable,
                format!("{path} at {}: {error}", short(commit)),
            )
        })?;
        let status = RecordView::parse(self.kind, &path, text)
            .map_err(|error| {
                Unreadable::new(
                    Cause::Unparsable,
                    format!("{path} at {}: {error}", short(commit)),
                )
            })?
            .status;
        self.statuses.insert(blob, status.clone());
        Ok(Some(status))
    }

    /// The statuses a change from `parent` starts from: the record's status
    /// at `parent`; where `parent` holds no record, the status each nearest
    /// earlier holder had, through the commits of the range and at the
    /// boundary it starts from; `None` only when no earlier commit holds
    /// one, a creation.
    fn starts(&mut self, parent: Oid) -> Result<Vec<Option<String>>, Unreadable> {
        if let Some(status) = self.status(parent)? {
            return Ok(vec![Some(status)]);
        }
        let mut held = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![parent];
        while let Some(commit) = stack.pop() {
            if !seen.insert(commit) {
                continue;
            }
            if let Some(status) = self.status(commit)? {
                if !held.contains(&status) {
                    held.push(status);
                }
                continue;
            }
            if self.in_range.contains(&commit) {
                stack.extend(self.guarded_parents(commit)?);
            }
        }
        if held.is_empty() {
            return Ok(vec![None]);
        }
        Ok(held.into_iter().map(Some).collect())
    }

    /// The first start of `starts` from which `to` is not one legal step.
    fn refused_start(&self, starts: &[Option<String>], to: &str) -> Option<(String, &'static str)> {
        starts.iter().find_map(|from| {
            step_allowed(self.kind, from.as_deref(), to)
                .err()
                .map(|reason| {
                    (
                        from.clone()
                            .unwrap_or_else(|| created(self.kind).to_string()),
                        reason,
                    )
                })
        })
    }

    /// Every illegal step of `base..head`, judged per commit and parent.
    fn range(&mut self, base: Oid, head: Oid) -> Result<Vec<String>, Unreadable> {
        let mut walk = history::revwalk(self.repo).map_err(|error| Unreadable::git(&error))?;
        walk.push(head)
            .and_then(|()| walk.hide(base))
            .map_err(|error| Unreadable::git(&error))?;
        let commits = walk
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| Unreadable::git(&error))?;
        self.in_range = commits.iter().copied().collect();
        let mut problems = Vec::new();
        for commit in commits {
            let parents = self.guarded_parents(commit)?;
            for (index, parent) in parents.iter().enumerate() {
                if self.blob(commit)? == self.blob(*parent)? {
                    continue;
                }
                if !self.step(commit, index)? {
                    let to = self.status(commit)?.unwrap_or_default();
                    let starts = self.starts(*parent)?;
                    let (from, reason) = self.refused_start(&starts, &to).unwrap_or_else(|| {
                        let from = starts.first().cloned().flatten();
                        (
                            from.unwrap_or_else(|| created(self.kind).to_string()),
                            "not a legal step",
                        )
                    });
                    problems.push(format!(
                        "status {from} -> {to} is refused at commit {} (from parent {}): {reason}",
                        short(commit),
                        short(*parent)
                    ));
                }
            }
        }
        Ok(problems)
    }

    /// Whether `commit`'s change from its parent `index` is legal: one legal
    /// step, or, for a merge, the other side's chain of legal steps.
    fn step(&mut self, commit: Oid, index: usize) -> Result<bool, Unreadable> {
        if let Some(known) = self.steps.get(&(commit, index)) {
            return Ok(*known);
        }
        if !self.open.insert((commit, index)) {
            return Ok(false);
        }
        let judged = self.judge_step(commit, index);
        self.open.remove(&(commit, index));
        let legal = judged?;
        self.steps.insert((commit, index), legal);
        Ok(legal)
    }

    fn judge_step(&mut self, commit: Oid, index: usize) -> Result<bool, Unreadable> {
        let parents = self.guarded_parents(commit)?;
        let parent = parents[index];
        // A deletion is no status step; the record rules judge it.
        let Some(to) = self.status(commit)? else {
            return Ok(true);
        };
        let starts = self.starts(parent)?;
        if self.refused_start(&starts, &to).is_none() {
            return Ok(true);
        }
        for (other, side) in parents.iter().enumerate() {
            if other == index || self.status(*side)?.as_deref() != Some(to.as_str()) {
                continue;
            }
            if self.side_chain(parent, *side)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether the merge of `side` into `ours` brought its status by legal
    /// steps: `side`'s first-parent chain, back to the first commit `ours`
    /// already contains, is legal step by step, and `ours` still holds that
    /// commit's status, so its own side did not change it.
    fn side_chain(&mut self, ours: Oid, side: Oid) -> Result<bool, Unreadable> {
        let mut chain = Vec::new();
        let mut point = side;
        loop {
            let reached = point == ours
                || history::descendant_of(self.repo, ours, point)
                    .map_err(|error| Unreadable::git(&error))?;
            if reached {
                break;
            }
            chain.push(point);
            let parents = self.guarded_parents(point)?;
            let Some(first) = parents.first() else {
                return Ok(false);
            };
            point = *first;
        }
        if self.status(point)? != self.status(ours)? {
            return Ok(false);
        }
        for step in chain {
            if self.blob(step)? == self.blob(self.guarded_parents(step)?[0])? {
                continue;
            }
            if !self.step(step, 0)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const PATH: &str = "project-management/tasks/TSK-001.md";

    fn git(root: &Path, args: &[&str]) -> String {
        let out = crate::git::command()
            .arg("-C")
            .arg(root)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.test")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.test")
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn record(status: &str) -> String {
        format!(
            "---\nid: TSK-001\nepic_id: null\nstandalone_reason: unit\nintegration_target: main\ntitle: unit\nstatus: {status}\nwork_type: fix\nspecs: []\ndepends_on: []\ncreated: 2026-11-01\n---\n\n## Description\nUnit.\n"
        )
    }

    /// Commit TSK-001 at `status` (or `text` verbatim) and return the commit.
    fn at(root: &Path, status: &str) -> Oid {
        let text = if status.contains('\n') {
            status.to_string()
        } else {
            record(status)
        };
        let path = root.join(PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: step", "--allow-empty"]);
        Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap()
    }

    fn repo(first: &str) -> (tempfile::TempDir, Oid) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-qb", "main"]);
        let base = at(dir.path(), first);
        (dir, base)
    }

    fn steps(root: &Path, base: Oid, head: Oid) -> Steps {
        let repo = Repository::open(root).unwrap();
        judge(&repo, base, head, RecordKind::Task, PATH)
    }

    #[test]
    fn a_chain_of_legal_steps_is_proven_and_a_direct_illegal_step_is_refused() {
        let (dir, base) = repo("blocked");
        at(dir.path(), "todo");
        let head = at(dir.path(), "complete");
        let judged = steps(dir.path(), base, head);
        assert!(judged.proven && judged.problems.is_empty(), "{judged:?}");

        let (dir, base) = repo("blocked");
        let head = at(dir.path(), "complete");
        let judged = steps(dir.path(), base, head);
        assert!(!judged.proven);
        assert!(
            judged.problems[0].contains("status blocked -> complete is refused at commit")
                && judged.problems[0].contains("unblock to todo before completing"),
            "{judged:?}"
        );
    }

    #[test]
    fn a_terminal_status_left_inside_the_range_is_refused() {
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        let head = at(dir.path(), "todo");
        let judged = steps(dir.path(), base, head);
        assert_eq!(judged.problems.len(), 1, "{judged:?}");
        assert!(
            judged.problems[0].contains("a cancelled task is terminal"),
            "{judged:?}"
        );
    }

    /// A merge that takes the other side's status is judged by that side's
    /// first-parent chain; a nested merge on that chain is judged the same
    /// way. A merge that revives a cancellation from the unchanged side is
    /// refused.
    #[test]
    fn a_merge_is_judged_by_the_chain_it_takes() {
        let (dir, base) = repo("blocked");
        let root = dir.path();
        git(root, &["switch", "-qc", "side"]);
        at(root, "todo");
        at(root, "complete");
        // The side lands on `main` by a merge, as a pull request does.
        git(root, &["switch", "-qc", "target", &base.to_string()]);
        git(
            root,
            &[
                "merge",
                "-q",
                "--no-ff",
                "side",
                "-m",
                "merge: land the side",
            ],
        );
        // A line still at `blocked` takes the target in one merge.
        git(root, &["switch", "-qc", "line", &base.to_string()]);
        at(root, &(record("blocked") + "\nLine note.\n"));
        let line = Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap();
        git(
            root,
            &[
                "merge",
                "-q",
                "--no-ff",
                "target",
                "-X",
                "theirs",
                "-m",
                "merge: sync",
            ],
        );
        let head = Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap();
        let judged = steps(root, line, head);
        assert!(judged.problems.is_empty(), "{judged:?}");

        // A cancellation on one side, revived by taking the other side's todo.
        let (dir, base) = repo("todo");
        let root = dir.path();
        git(root, &["switch", "-qc", "side"]);
        at(root, "cancelled");
        git(root, &["switch", "-q", "main"]);
        std::fs::write(root.join("other.txt"), "x\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: other"]);
        git(root, &["merge", "-q", "--no-ff", "--no-commit", "side"]);
        std::fs::write(root.join(PATH), record("todo")).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "merge: revive"]);
        let head = Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap();
        let judged = steps(root, base, head);
        assert!(
            judged
                .problems
                .iter()
                .any(|p| p.contains("status cancelled -> todo is refused")),
            "{judged:?}"
        );
    }

    #[test]
    fn unreadable_history_refuses_with_its_cause_and_remedy() {
        // A shallow boundary inside the range.
        let (dir, base) = repo("todo");
        let middle = at(dir.path(), "blocked");
        let head = at(dir.path(), "todo");
        std::fs::write(dir.path().join(".git/shallow"), format!("{middle}\n")).unwrap();
        let judged = steps(dir.path(), base, head);
        assert!(!judged.proven);
        assert!(
            judged.problems[0].starts_with("status history cannot be read (shallow history):"),
            "{judged:?}"
        );
        assert!(remedy_for(&judged.problems[0])
            .unwrap()
            .contains("git fetch --unshallow"));

        // An unparsable version.
        let (dir, base) = repo("todo");
        at(dir.path(), "not a record\n");
        let head = at(dir.path(), "todo");
        let judged = steps(dir.path(), base, head);
        assert!(
            judged.problems[0].contains("(unparsable record)"),
            "{judged:?}"
        );
        let remedy = remedy_for(&judged.problems[0]).unwrap();
        assert!(remedy.contains("a fetch does not repair it") && !remedy.contains("--unshallow"));

        // A missing blob.
        let (dir, base) = repo("todo");
        let middle = at(dir.path(), "blocked");
        let head = at(dir.path(), "todo");
        let blob = git(dir.path(), &["rev-parse", &format!("{middle}:{PATH}")]);
        std::fs::remove_file(dir.path().join(format!(
            ".git/objects/{}/{}",
            &blob[..2],
            &blob[2..]
        )))
        .unwrap();
        let judged = steps(dir.path(), base, head);
        assert!(
            judged.problems[0].contains("(missing object)"),
            "{judged:?}"
        );
        assert!(remedy_for(&judged.problems[0])
            .unwrap()
            .contains("git fetch"));

        assert!(remedy_for("status todo -> complete is refused").is_none());
    }

    #[test]
    fn a_record_absent_at_a_parent_is_a_creation_not_unreadable_history() {
        let (dir, base) = repo("todo");
        std::fs::remove_file(dir.path().join(PATH)).unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-qm", "chore: drop"]);
        let head = at(dir.path(), "cancelled");
        let judged = steps(dir.path(), base, head);
        assert!(judged.problems.is_empty(), "{judged:?}");
        // Absence is legal but never proves the endpoints' change.
        assert!(!judged.proven);
    }

    /// Commit the tree as it stands, with TSK-001 moved to `path` (or
    /// removed from every record path when `path` is `None`) at `status`.
    fn moved(root: &Path, path: Option<&str>, status: &str) -> Oid {
        for old in [PATH, EPIC_PATH, PARKED] {
            let _ = std::fs::remove_file(root.join(old));
        }
        let target = path.unwrap_or(PARKED);
        let file = root.join(target);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, record(status)).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: move"]);
        Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap()
    }

    const EPIC_PATH: &str = "project-management/epics/EPC-001/tasks/TSK-001.md";
    const PARKED: &str = "notes/TSK-001.md";

    fn drop_record(root: &Path) -> Oid {
        std::fs::remove_file(root.join(PATH)).unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: drop"]);
        Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap()
    }

    fn terminal(judged: &Steps) -> bool {
        judged
            .problems
            .iter()
            .any(|problem| problem.contains("a cancelled task is terminal"))
    }

    /// A record that disappears and comes back is judged from the status it
    /// last held, wherever it went: a deletion, a move to another record
    /// path, or a park off the record paths (TSK-1036 review 01).
    #[test]
    fn a_record_that_comes_back_is_judged_from_its_last_status() {
        // Deleted, then recreated as todo.
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        drop_record(dir.path());
        let head = at(dir.path(), "todo");
        assert!(terminal(&steps(dir.path(), base, head)));

        // Blocked, cancelled, deleted, then todo.
        let (dir, base) = repo("blocked");
        at(dir.path(), "cancelled");
        drop_record(dir.path());
        let head = at(dir.path(), "todo");
        assert!(terminal(&steps(dir.path(), base, head)));

        // Moved to its epic's task path as todo.
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        let head = moved(dir.path(), Some(EPIC_PATH), "todo");
        let repo_at = Repository::open(dir.path()).unwrap();
        let judged = judge(&repo_at, base, head, RecordKind::Task, EPIC_PATH);
        assert!(terminal(&judged), "{judged:?}");

        // Parked off the record paths, then restored as todo.
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        moved(dir.path(), None, "cancelled");
        let head = moved(dir.path(), Some(PATH), "todo");
        assert!(terminal(&steps(dir.path(), base, head)));

        // Blocked, deleted, then complete: still one step too far.
        let (dir, base) = repo("blocked");
        drop_record(dir.path());
        let head = at(dir.path(), "complete");
        let judged = steps(dir.path(), base, head);
        assert!(
            judged
                .problems
                .iter()
                .any(|problem| problem.contains("unblock to todo before completing")),
            "{judged:?}"
        );

        // Controls: todo, deleted, then cancelled is legal; a legal move
        // between record paths keeps the walk legal.
        let (dir, base) = repo("todo");
        drop_record(dir.path());
        let head = at(dir.path(), "cancelled");
        assert!(steps(dir.path(), base, head).problems.is_empty());
        let (dir, base) = repo("blocked");
        at(dir.path(), "todo");
        let head = moved(dir.path(), Some(EPIC_PATH), "complete");
        let judged = judge(
            &repo_at_path(dir.path()),
            base,
            head,
            RecordKind::Task,
            EPIC_PATH,
        );
        assert!(judged.problems.is_empty(), "{judged:?}");
    }

    fn repo_at_path(root: &Path) -> Repository {
        Repository::open(root).unwrap()
    }

    /// The record is followed by its id at any case of the `.md` extension,
    /// and two files holding one id refuse as unreadable.
    #[test]
    fn the_record_is_followed_by_id_and_a_duplicate_refuses() {
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        std::fs::remove_file(dir.path().join(PATH)).unwrap();
        let upper = "project-management/tasks/TSK-001.MD";
        std::fs::write(dir.path().join(upper), record("todo")).unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-qm", "chore: case"]);
        let head = Oid::from_str(&git(dir.path(), &["rev-parse", "HEAD"])).unwrap();
        let judged = judge(
            &repo_at_path(dir.path()),
            base,
            head,
            RecordKind::Task,
            upper,
        );
        assert!(terminal(&judged), "{judged:?}");

        let (dir, base) = repo("todo");
        let second = dir.path().join(EPIC_PATH);
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(second, record("todo")).unwrap();
        let head = at(dir.path(), "complete");
        let judged = steps(dir.path(), base, head);
        assert!(
            judged.problems.len() == 1
                && judged.problems[0].starts_with(UNREADABLE)
                && judged.problems[0].contains("more than one record file"),
            "{judged:?}"
        );
    }

    /// The checkout's step starts from the status the record last held
    /// when `HEAD` dropped it.
    #[test]
    fn the_working_tree_step_starts_from_the_last_held_status() {
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        let tip = drop_record(dir.path());
        let repo = repo_at_path(dir.path());
        let starts = starts_at(&repo, base, tip, RecordKind::Task, PATH).unwrap();
        assert_eq!(starts, vec![Some("cancelled".to_string())]);
        let fresh = starts_at(&repo, tip, tip, RecordKind::Task, PATH).unwrap();
        assert_eq!(fresh, vec![None]);
    }
}
