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

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

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

/// Judge the record `id` of `kind` over `base..head`, followed by the id
/// its frontmatter states, at every record path of its kind.
#[must_use]
pub fn judge(repo: &Repository, base: Oid, head: Oid, kind: RecordKind, id: &str) -> Steps {
    let _judgment = history::judgment();
    let mut walker = Walker::new(repo, kind, id.to_string());
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
/// tree's step): the status of record `id` at `tip`, or, where `tip` holds
/// none, the status each nearest earlier holder in `base..tip` (or the
/// boundary) had; `None` alone for a record never held there.
///
/// # Errors
/// Returns the unreadable-history refusal.
pub fn starts_at(
    repo: &Repository,
    base: Oid,
    tip: Oid,
    kind: RecordKind,
    id: &str,
) -> Result<Vec<Option<String>>, String> {
    let _judgment = history::judgment();
    let mut walker = Walker::new(repo, kind, id.to_string());
    walker
        .range(base, tip)
        .map_err(|unreadable| unreadable.message())?;
    walker
        .starts(tip)
        .map_err(|unreadable| unreadable.message())
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

/// What a record file states: its id and status, or why it does not parse.
type Parsed = Result<(String, String), String>;

/// The record files of one kind at one commit, read once for a whole
/// read-only judgment (TSK-1306). Every record a range judges walks the
/// same commits, and each walk asks which file holds its id at each one;
/// reading and parsing every record file once per commit, instead of once
/// per record and commit, gives each walk the answer it read before.
#[derive(Default)]
struct CommitIndex {
    /// Each id the files state, with the files stating it, in tree order.
    holders: HashMap<String, Vec<(Oid, String)>>,
    /// For each file stem, the first file named for it that does not
    /// parse: its place in tree order, its path and the parse error.
    unparsable: HashMap<String, (usize, String, String)>,
    /// The first file whose blob cannot be read, with its place in tree
    /// order; reading stopped there, as the walk did.
    stop: Option<(usize, Unreadable)>,
}

impl CommitIndex {
    /// The file holding `id` at `commit`, with the refusals the walk gave
    /// when it read the files one by one: a file named for the id that does
    /// not parse, a blob it could not read before that, or two files
    /// stating the id. Whichever the tree order meets first wins.
    fn holder(&self, id: &str, commit: Oid) -> Result<Option<(Oid, String)>, Unreadable> {
        if let Some((at, path, error)) = self.unparsable.get(id) {
            if self.stop.as_ref().is_none_or(|(stop, _)| at < stop) {
                return Err(Unreadable::new(
                    Cause::Unparsable,
                    format!("{path} at {}: {error}", short(commit)),
                ));
            }
        }
        if let Some((_, unreadable)) = &self.stop {
            return Err(unreadable.clone());
        }
        let holders = self.holders.get(id).map_or(&[][..], Vec::as_slice);
        if holders.len() > 1 {
            let paths = holders
                .iter()
                .map(|(_, path)| path.as_str())
                .collect::<Vec<_>>();
            return Err(Unreadable::new(
                Cause::Unparsable,
                format!(
                    "{id} is held by more than one record file at {}: {}",
                    short(commit),
                    paths.join(", ")
                ),
            ));
        }
        Ok(holders.first().cloned())
    }
}

type MemoKey = (PathBuf, RecordKind, Oid);

thread_local! {
    /// Commit indexes by repository, kind and commit, kept while a
    /// judgment scope is open. An index that stopped at an unreadable blob
    /// is never kept, so a later read tries the object store again.
    static INDEXES: RefCell<HashMap<MemoKey, Rc<CommitIndex>>> = RefCell::new(HashMap::new());
    /// What each record blob states, by repository, kind and blob, kept
    /// while a judgment scope is open. A blob names its bytes, and the id
    /// and status a record states do not depend on its path.
    static PARSES: RefCell<HashMap<MemoKey, Parsed>> = RefCell::new(HashMap::new());
}

/// Forget every kept index and parse: the judgment scope closed, or an
/// object fetch may have changed what the store holds.
pub(super) fn clear_memo() {
    INDEXES.with(|memo| memo.borrow_mut().clear());
    PARSES.with(|memo| memo.borrow_mut().clear());
}

/// One tree entry: its name, object id and kind.
type Entry = (String, Oid, Option<git2::ObjectType>);

struct Walker<'r> {
    repo: &'r Repository,
    kind: RecordKind,
    id: String,
    /// The record file (blob and path) holding the id at each commit.
    found: HashMap<Oid, Option<(Oid, String)>>,
    /// What each record blob read states.
    parsed: HashMap<Oid, Parsed>,
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

/// The stem of a `.md` file name (any case of `md`), else `None`.
fn markdown_stem(name: &str) -> Option<&str> {
    let (stem, extension) = name.rsplit_once('.')?;
    extension.eq_ignore_ascii_case("md").then_some(stem)
}

/// The entries of `tree` (under `dir`). A name that is not UTF-8 cannot be
/// told from a record path, so it refuses, as the record inventory does.
fn entries(tree: &git2::Tree<'_>, dir: &str, commit: Oid) -> Result<Vec<Entry>, Unreadable> {
    let mut entries = Vec::new();
    for entry in tree {
        let name = crate::git::GitName::from_bytes(entry.name_bytes());
        let Ok(text) = name.rule_text() else {
            return Err(Unreadable::new(
                Cause::Unparsable,
                format!(
                    "a name under {dir} at {} is not UTF-8: {}",
                    short(commit),
                    name.display()
                ),
            ));
        };
        entries.push((text.to_string(), entry.id(), entry.kind()));
    }
    Ok(entries)
}

impl<'r> Walker<'r> {
    fn new(repo: &'r Repository, kind: RecordKind, id: String) -> Self {
        Self {
            repo,
            kind,
            id,
            found: HashMap::new(),
            parsed: HashMap::new(),
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

    /// The files at record paths of this kind in `commit`'s tree, as blob
    /// and path.
    fn record_files(&self, commit: Oid) -> Result<Vec<(Oid, String)>, Unreadable> {
        let missing = |error: git2::Error| Unreadable::new(Cause::MissingObject, error.message());
        let tree = self
            .repo
            .find_commit(commit)
            .and_then(|found| found.tree())
            .map_err(missing)?;
        let epics_dir = "project-management/epics";
        let mut dirs = vec![
            "project-management/tasks".to_string(),
            "project-management/specs".to_string(),
            epics_dir.to_string(),
        ];
        if let Some(epics) = self.subtree(&tree, epics_dir)? {
            for (name, _, kind) in entries(&epics, epics_dir, commit)? {
                if kind == Some(git2::ObjectType::Tree) {
                    dirs.push(format!("{epics_dir}/{name}"));
                    dirs.push(format!("{epics_dir}/{name}/tasks"));
                }
            }
        }
        let mut files = Vec::new();
        for dir in dirs {
            let Some(sub) = self.subtree(&tree, &dir)? else {
                continue;
            };
            for (name, blob, kind) in entries(&sub, &dir, commit)? {
                let path = format!("{dir}/{name}");
                if kind == Some(git2::ObjectType::Blob)
                    && super::work_start::record_kind_for_tree_path(&path) == Some(self.kind)
                {
                    files.push((blob, path));
                }
            }
        }
        Ok(files)
    }

    /// What the record blob at `path` states, read once per blob, and once
    /// per judgment where a judgment scope is open.
    fn parse(&mut self, blob: Oid, path: &str) -> Result<Parsed, Unreadable> {
        if let Some(parsed) = self.parsed.get(&blob) {
            return Ok(parsed.clone());
        }
        let key = (self.repo.path().to_path_buf(), self.kind, blob);
        let kept = history::in_judgment();
        let hit = kept
            .then(|| PARSES.with(|memo| memo.borrow().get(&key).cloned()))
            .flatten();
        let parsed = if let Some(hit) = hit {
            hit
        } else {
            let found = self
                .repo
                .find_blob(blob)
                .map_err(|error| Unreadable::new(Cause::MissingObject, error.message()))?;
            let parsed = std::str::from_utf8(found.content())
                .map_err(|error| error.to_string())
                .and_then(|text| RecordView::parse(self.kind, path, text))
                .map(|record| (record.id, record.status));
            if kept {
                PARSES.with(|memo| memo.borrow_mut().insert(key, parsed.clone()));
            }
            parsed
        };
        self.parsed.insert(blob, parsed.clone());
        Ok(parsed)
    }

    /// The record files of this kind at `commit`, indexed by the id each
    /// states (see [`CommitIndex`]), shared by every walk of the judgment.
    fn index(&mut self, commit: Oid) -> Result<Rc<CommitIndex>, Unreadable> {
        let key = (self.repo.path().to_path_buf(), self.kind, commit);
        let kept = history::in_judgment();
        if kept {
            if let Some(hit) = INDEXES.with(|memo| memo.borrow().get(&key).cloned()) {
                return Ok(hit);
            }
        }
        let mut index = CommitIndex::default();
        for (place, (blob, path)) in self.record_files(commit)?.into_iter().enumerate() {
            match self.parse(blob, &path) {
                Err(unreadable) => {
                    index.stop = Some((place, unreadable));
                    break;
                }
                Ok(Ok((id, _))) => index.holders.entry(id).or_default().push((blob, path)),
                Ok(Err(error)) => {
                    if let Some(stem) = path.rsplit('/').next().and_then(markdown_stem) {
                        let stem = stem.to_string();
                        index.unparsable.entry(stem).or_insert((place, path, error));
                    }
                }
            }
        }
        let index = Rc::new(index);
        if kept && index.stop.is_none() {
            INDEXES.with(|memo| memo.borrow_mut().insert(key, Rc::clone(&index)));
        }
        Ok(index)
    }

    /// The record file holding the id at `commit`: a file at a record path
    /// of this kind whose frontmatter states the id, as the record graph
    /// keys it; `None` when no file does. A file named for the id that does
    /// not parse, or two files stating one id, cannot be told apart from
    /// the record, so they refuse.
    fn locate(&mut self, commit: Oid) -> Result<Option<(Oid, String)>, Unreadable> {
        if let Some(found) = self.found.get(&commit) {
            return Ok(found.clone());
        }
        let found = self.index(commit)?.holder(&self.id, commit)?;
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
        match self.parse(blob, &path)? {
            Ok((_, status)) => Ok(Some(status)),
            Err(error) => Err(Unreadable::new(
                Cause::Unparsable,
                format!("{path} at {}: {error}", short(commit)),
            )),
        }
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
        judge(&repo, base, head, RecordKind::Task, "TSK-001")
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
        let judged = judge(&repo_at, base, head, RecordKind::Task, "TSK-001");
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
            "TSK-001",
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
            "TSK-001",
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
        let starts = starts_at(&repo, base, tip, RecordKind::Task, "TSK-001").unwrap();
        assert_eq!(starts, vec![Some("cancelled".to_string())]);
        let fresh = starts_at(&repo, tip, tip, RecordKind::Task, "TSK-001").unwrap();
        assert_eq!(fresh, vec![None]);
    }

    /// Commit `files` (path, content) after removing every path in `drop`.
    fn commit_files(root: &Path, drop: &[&str], files: &[(&str, &str)]) -> Oid {
        for path in drop {
            std::fs::remove_file(root.join(path)).unwrap();
        }
        for (path, text) in files {
            let file = root.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: files"]);
        Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap()
    }

    /// The record is the file whose frontmatter states the id, as the graph
    /// keys it, whatever the file is named; a file of another id is not it
    /// (TSK-1036 review 02).
    #[test]
    fn the_record_is_followed_by_its_stated_id() {
        const OTHER: &str = "project-management/tasks/TSK-002.md";
        // Cancelled only under another name, then restored as todo.
        let (dir, base) = repo("todo");
        commit_files(dir.path(), &[PATH], &[(OTHER, &record("cancelled"))]);
        let head = commit_files(dir.path(), &[OTHER], &[(PATH, &record("todo"))]);
        assert!(terminal(&steps(dir.path(), base, head)));

        // Cancelled, then moved to another name as todo.
        let (dir, base) = repo("todo");
        at(dir.path(), "cancelled");
        let head = commit_files(dir.path(), &[PATH], &[(OTHER, &record("todo"))]);
        assert!(terminal(&steps(dir.path(), base, head)));

        // A real TSK-002 beside TSK-001 is another record.
        let (dir, base) = repo("todo");
        let own = record("cancelled").replace("id: TSK-001", "id: TSK-002");
        commit_files(dir.path(), &[], &[(OTHER, &own)]);
        let head = at(dir.path(), "complete");
        let judged = steps(dir.path(), base, head);
        assert!(judged.proven && judged.problems.is_empty(), "{judged:?}");
    }

    /// A record directory whose name is not UTF-8 cannot be read, so the
    /// history refuses instead of reading the record as absent.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_refuses() {
        use std::os::unix::ffi::OsStrExt;
        let (dir, base) = repo("todo");
        // Some file systems refuse such a name, so the tree is built from
        // objects: project-management/epics/EPC-\xff/tasks/TSK-001.md.
        let repo = repo_at_path(dir.path());
        let blob = repo.blob(record("cancelled").as_bytes()).unwrap();
        let mut tasks = repo.treebuilder(None).unwrap();
        tasks.insert("TSK-001.md", blob, 0o100_644).unwrap();
        let tasks = tasks.write().unwrap();
        let mut epic = repo.treebuilder(None).unwrap();
        epic.insert("tasks", tasks, 0o040_000).unwrap();
        let epic = epic.write().unwrap();
        let mut epics = repo.treebuilder(None).unwrap();
        let name = std::path::Path::new(std::ffi::OsStr::from_bytes(b"EPC-\xff"));
        epics.insert(name, epic, 0o040_000).unwrap();
        let epics = epics.write().unwrap();
        let mut management = repo.treebuilder(None).unwrap();
        management.insert("epics", epics, 0o040_000).unwrap();
        let management = management.write().unwrap();
        let mut root = repo.treebuilder(None).unwrap();
        root.insert("project-management", management, 0o040_000)
            .unwrap();
        let tree = repo.find_tree(root.write().unwrap()).unwrap();
        let signature = git2::Signature::now("t", "t@example.test").unwrap();
        let parent = repo.find_commit(base).unwrap();
        let hidden = repo
            .commit(
                None,
                &signature,
                &signature,
                "chore: hide",
                &tree,
                &[&parent],
            )
            .unwrap();
        git(dir.path(), &["reset", "-q", "--soft", &hidden.to_string()]);
        let head = at(dir.path(), "todo");
        let judged = steps(dir.path(), base, head);
        assert!(
            judged.problems.iter().any(|problem| {
                problem.starts_with(UNREADABLE) && problem.contains("is not UTF-8")
            }),
            "{judged:?}"
        );
    }

    /// TSK-1306 AC-3: inside a judgment scope, where every walk shares the
    /// commit indexes and parses, each record's steps are exactly those the
    /// walk gives with nothing shared: a legal chain, an illegal step, an id
    /// two files state, a file named for an id that does not parse, and an
    /// id that appears inside the range.
    #[test]
    fn shared_indexes_give_every_record_the_steps_it_had_alone() {
        let (dir, _) = repo("todo");
        let root = dir.path();
        let write = |rel: &str, text: &str| {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        let other = |id: &str, status: &str| record(status).replace("TSK-001", id);
        write(
            "project-management/tasks/TSK-002.md",
            &other("TSK-002", "blocked"),
        );
        write(
            "project-management/tasks/TSK-003.md",
            &other("TSK-003", "todo"),
        );
        write(
            "project-management/tasks/TSK-004.md",
            &other("TSK-004", "todo"),
        );
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: more"]);
        let base = Oid::from_str(&git(root, &["rev-parse", "HEAD"])).unwrap();
        at(root, "blocked");
        write(
            "project-management/tasks/TSK-002.md",
            &other("TSK-002", "complete"),
        );
        write(
            "project-management/tasks/TSK-003.md",
            "---\nid: [broken\n---\n",
        );
        write(
            "project-management/tasks/TSK-004.md",
            &other("TSK-004", "blocked"),
        );
        write(
            "project-management/epics/EPC-001/tasks/TSK-004.md",
            &other("TSK-004", "blocked"),
        );
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "chore: change"]);
        let head = at(root, "todo");
        let repo = Repository::open(root).unwrap();
        let ids = ["TSK-001", "TSK-002", "TSK-003", "TSK-004", "TSK-005"];
        let alone: Vec<String> = ids
            .iter()
            .map(|id| format!("{:?}", judge(&repo, base, head, RecordKind::Task, id)))
            .collect();
        assert!(alone[1].contains("blocked -> complete"), "{alone:?}");
        assert!(alone[2].contains("TSK-003.md at"), "{alone:?}");
        assert!(alone[3].contains("more than one record file"), "{alone:?}");
        let _scope = history::judgment();
        for _ in 0..2 {
            let shared: Vec<String> = ids
                .iter()
                .map(|id| format!("{:?}", judge(&repo, base, head, RecordKind::Task, id)))
                .collect();
            assert_eq!(shared, alone);
        }
        assert!(INDEXES.with(|memo| !memo.borrow().is_empty()));
    }
}
