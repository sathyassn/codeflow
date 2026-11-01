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

/// Judge the record at `path` over `base..head`.
#[must_use]
pub fn judge(repo: &Repository, base: Oid, head: Oid, kind: RecordKind, path: &str) -> Steps {
    let _judgment = history::judgment();
    let mut walker = Walker::new(repo, kind, path);
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
    path: &'r str,
    blobs: HashMap<Oid, Option<Oid>>,
    statuses: HashMap<Oid, String>,
    steps: HashMap<(Oid, usize), bool>,
    /// Steps being judged, so a chain that reaches itself is refused.
    open: HashSet<(Oid, usize)>,
    absent: bool,
}

fn short(oid: Oid) -> String {
    oid.to_string()[..12].to_string()
}

impl<'r> Walker<'r> {
    fn new(repo: &'r Repository, kind: RecordKind, path: &'r str) -> Self {
        Self {
            repo,
            kind,
            path,
            blobs: HashMap::new(),
            statuses: HashMap::new(),
            steps: HashMap::new(),
            open: HashSet::new(),
            absent: false,
        }
    }

    fn guarded_parents(&self, commit: Oid) -> Result<Vec<Oid>, Unreadable> {
        let found = self
            .repo
            .find_commit(commit)
            .map_err(|error| Unreadable::new(Cause::MissingObject, error.message()))?;
        history::parents(self.repo, &found).map_err(|error| Unreadable::git(&error))
    }

    /// The record's blob at `commit`, `None` when the path is absent.
    fn blob(&mut self, commit: Oid) -> Result<Option<Oid>, Unreadable> {
        if let Some(blob) = self.blobs.get(&commit) {
            return Ok(*blob);
        }
        let missing = |error: git2::Error| Unreadable::new(Cause::MissingObject, error.message());
        let tree = self
            .repo
            .find_commit(commit)
            .and_then(|found| found.tree())
            .map_err(missing)?;
        let blob = match tree.get_path(std::path::Path::new(self.path)) {
            Ok(entry) => Some(entry.id()),
            Err(error) if error.code() == git2::ErrorCode::NotFound => None,
            Err(error) => return Err(missing(error)),
        };
        self.blobs.insert(commit, blob);
        Ok(blob)
    }

    /// The record's status at `commit`, `None` when absent there.
    fn status(&mut self, commit: Oid) -> Result<Option<String>, Unreadable> {
        let Some(blob) = self.blob(commit)? else {
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
                format!("{} at {}: {error}", self.path, short(commit)),
            )
        })?;
        let status = RecordView::parse(self.kind, self.path, text)
            .map_err(|error| {
                Unreadable::new(
                    Cause::Unparsable,
                    format!("{} at {}: {error}", self.path, short(commit)),
                )
            })?
            .status;
        self.statuses.insert(blob, status.clone());
        Ok(Some(status))
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
        let mut problems = Vec::new();
        for commit in commits {
            let parents = self.guarded_parents(commit)?;
            for (index, parent) in parents.iter().enumerate() {
                if self.blob(commit)? == self.blob(*parent)? {
                    continue;
                }
                if !self.step(commit, index)? {
                    let to = self.status(commit)?.unwrap_or_default();
                    let from = self.status(*parent)?;
                    let reason = step_allowed(self.kind, from.as_deref(), &to)
                        .err()
                        .unwrap_or("not a legal step");
                    problems.push(format!(
                        "status {} -> {to} is refused at commit {} (from parent {}): {reason}",
                        from.as_deref().unwrap_or(created(self.kind)),
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
        let from = self.status(parent)?;
        if step_allowed(self.kind, from.as_deref(), &to).is_ok() {
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
}
