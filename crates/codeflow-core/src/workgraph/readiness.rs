//! Derived work state (SPC-013 R-27, R-40 to R-44, R-110): what is ready,
//! active, waiting, blocked and landed, computed from the records on each
//! target's tip and the branches git can see. Nothing here is written back;
//! `work next`, `work claim`, `status` and `orient` read the same answer, and
//! every verdict comes from the one readiness core that `work start` and CI
//! use ([`super::work_start`]).
//!
//! `work next` and `status` make no network call: they read the refs as last
//! fetched and name that snapshot. `work claim` fetches first.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use git2::{BranchType, Repository};

use super::work_start::{
    parse_record, records_from_tree, validate_anchored_task, work_prefixes, work_suffix, NotReady,
    Record, RecordKind,
};

/// The derived state of one task in the new-claim context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    /// The core holds and no visible branch carries the task.
    Ready,
    /// A visible, unlanded branch carries the task: claimed (advisory).
    Active,
    /// `todo` with an unmet dependency or a spec not yet approved.
    Waiting,
    /// A Blocker, an awaiting selection or a closed epic.
    Blocked,
    /// The records do not hold together on the target tip.
    Invalid,
}

impl State {
    /// The word used in output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Invalid => "invalid",
        }
    }
}

/// One target tip a verdict was read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The declared target (`main`, `integration/EPC-NNN-<slug>`).
    pub target: String,
    /// The ref read (`origin/main` or the local branch); `None` when the
    /// target resolves nowhere here.
    pub reference: Option<String>,
    /// The tip commit read.
    pub tip: Option<String>,
    /// Why the target could not be read, when it could not (a local branch
    /// diverged from its upstream).
    pub problem: Option<String>,
}

/// One open task and its derived state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub task_id: String,
    pub title: String,
    pub epic_id: Option<String>,
    pub target: String,
    pub state: State,
    /// Why it is not ready (empty when ready or active).
    pub reason: String,
    /// Visible, unlanded branches carrying the task id.
    pub branches: Vec<String>,
}

/// Epic progress: counts of its tasks by status on the target tips.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpicProgress {
    pub total: usize,
    pub complete: usize,
    pub cancelled: usize,
}

/// The derived view of the whole backlog.
#[derive(Debug, Clone, Default)]
pub struct Backlog {
    pub snapshots: Vec<Snapshot>,
    /// When the refs were last fetched (`FETCH_HEAD`), if ever.
    pub fetched_at: Option<String>,
    /// Open tasks (not complete or cancelled), ready first.
    pub entries: Vec<Entry>,
    /// Task ids that two or more visible branches carry.
    pub conflicts: BTreeMap<String, Vec<String>>,
    /// Branches carrying a task id whose tip is in its target (landed).
    pub landed: Vec<String>,
    pub epics: BTreeMap<String, EpicProgress>,
}

impl Backlog {
    /// Entries in `state`.
    pub fn in_state(&self, state: State) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.state == state)
    }

    /// Counts by derived state, in the order `work next` lists them.
    #[must_use]
    pub fn counts_line(&self) -> String {
        [
            State::Ready,
            State::Active,
            State::Waiting,
            State::Blocked,
            State::Invalid,
        ]
        .into_iter()
        .map(|state| format!("{} {}", state.as_str(), self.in_state(state).count()))
        .collect::<Vec<_>>()
        .join(" · ")
    }

    /// One line naming the snapshot the verdicts were read from (R-41).
    #[must_use]
    pub fn snapshot_line(&self) -> String {
        let tips = self
            .snapshots
            .iter()
            .map(
                |snapshot| match (&snapshot.reference, &snapshot.tip, &snapshot.problem) {
                    (Some(reference), Some(tip), _) => {
                        format!("{reference}@{}", &tip[..tip.len().min(9)])
                    }
                    (_, _, Some(problem)) => format!("{} ({problem})", snapshot.target),
                    _ => format!("{} (not found)", snapshot.target),
                },
            )
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "snapshot: {tips}; fetched {}",
            self.fetched_at
                .as_deref()
                .unwrap_or("never (local refs only)")
        )
    }
}

/// The identity of a target name, for comparing a branch with declared
/// targets: `main`, `refs/heads/main`, `origin/main` and
/// `refs/remotes/origin/main` name one line; a target on another remote
/// (`refs/remotes/upstream/main`) keeps its remote.
fn canonical_target(declared: &str) -> String {
    declared
        .strip_prefix("refs/heads/")
        .or_else(|| declared.strip_prefix("refs/remotes/origin/"))
        .or_else(|| declared.strip_prefix("origin/"))
        .unwrap_or(declared)
        .to_string()
}

/// A resolved target: the ref read, its tip, or why it cannot be read.
type Resolved = Result<Option<(String, git2::Oid)>, String>;

/// Resolve a declared target exactly as `work start` does
/// ([`super::resolve_work_target_checked`]): a bare name reads the local
/// branch, or its configured upstream when the local branch is strictly
/// behind it, and `origin/<name>` only where no local branch exists; an
/// explicit ref is read as written. One resolver, so `work next`,
/// `work claim` and `work start` read one line. `Ok(None)` when the target
/// resolves nowhere here; an error when the local branch and its upstream
/// have diverged.
fn resolve_target(repo_root: &Path, repo: &Repository, declared: &str) -> Resolved {
    let resolved = super::resolve_work_target_checked(repo_root, Some(declared))
        .map_err(|error| error.to_string())?;
    Ok(resolved.and_then(|resolved| {
        let commit = super::work_start::target_reference(repo, &resolved.target)?;
        Some((resolved.target, commit.id()))
    }))
}

/// A resolved target as output shows it (`upstream/main`, not
/// `refs/remotes/upstream/main`).
fn shown(target: &str) -> String {
    target
        .strip_prefix("refs/remotes/")
        .or_else(|| target.strip_prefix("refs/heads/"))
        .unwrap_or(target)
        .to_string()
}

/// The remote `work claim` fetches before it judges `declared`: the remote
/// an explicit remote-tracking ref names; for a bare name, the configured
/// upstream remote of its local branch, or `origin` when no local branch
/// exists (a clone that reads `origin/<name>`).
///
/// # Errors
///
/// Returns the reason when an explicit remote-tracking ref names no
/// configured remote.
fn target_fetch_remote(repo: &Repository, declared: &str) -> Result<Option<String>, String> {
    let owned = |buf: git2::Buf| std::str::from_utf8(&buf).ok().map(str::to_owned);
    if declared.starts_with("refs/remotes/") {
        return repo
            .branch_remote_name(declared)
            .ok()
            .and_then(owned)
            .map(Some)
            .ok_or_else(|| format!("target '{declared}' names a remote that is not configured"));
    }
    if declared.starts_with("origin/") {
        return Ok(Some("origin".to_string()));
    }
    let local = format!(
        "refs/heads/{}",
        declared.strip_prefix("refs/heads/").unwrap_or(declared)
    );
    if repo.find_reference(&local).is_ok() {
        return Ok(repo.branch_upstream_remote(&local).ok().and_then(owned));
    }
    Ok(Some("origin".to_string()))
}

/// Every visible branch carrying a task id on a sanctioned work prefix, with
/// its tip: local branches and `origin`'s by branch name (a branch and its
/// published copy are one claim), another remote's as `<remote>/<branch>`,
/// so a claim visible on any remote counts and distinct claims stay
/// distinct.
fn visible_work_branches(
    repo: &Repository,
    prefixes: &[String],
    ids: &BTreeSet<String>,
) -> BTreeMap<String, Vec<(String, git2::Oid)>> {
    let mut carried: BTreeMap<String, Vec<(String, git2::Oid)>> = BTreeMap::new();
    let Ok(branches) = repo.branches(None) else {
        return carried;
    };
    let mut seen = BTreeSet::new();
    for (branch, kind) in branches.flatten() {
        let Some(name) = branch.name().ok().flatten() else {
            continue;
        };
        let (name, short) = match kind {
            BranchType::Local => (name.to_string(), name.to_string()),
            BranchType::Remote => {
                let Some(remote) = repo
                    .branch_remote_name(&format!("refs/remotes/{name}"))
                    .ok()
                    .and_then(|buf| std::str::from_utf8(&buf).ok().map(str::to_owned))
                else {
                    continue;
                };
                let Some(short) = name
                    .strip_prefix(&format!("{remote}/"))
                    .filter(|short| *short != "HEAD")
                else {
                    continue;
                };
                let shown = if remote == "origin" {
                    short.to_string()
                } else {
                    name.to_string()
                };
                (shown, short.to_string())
            }
        };
        let Some(oid) = branch.get().peel_to_commit().ok().map(|commit| commit.id()) else {
            continue;
        };
        let Some(suffix) = work_suffix(prefixes, &short) else {
            continue;
        };
        let Some(id) = ids
            .iter()
            .filter(|id| suffix.starts_with(&format!("{id}-")))
            .max_by_key(|id| id.len())
        else {
            continue;
        };
        if seen.insert((name.clone(), oid)) {
            carried.entry(id.clone()).or_default().push((name, oid));
        }
    }
    carried
}

/// Whether a branch tip has landed on the target tip (R-27): it is an
/// ancestor of the target, or every commit it adds is patch-equivalent there
/// (`git cherry`, for squash and rebase landings). A tip on the target's own
/// first-parent line added nothing: that is a claim not yet started, not a
/// landing. Work lands with a merge commit, so a landed tip is off that line.
/// Anything unproven is not landed.
fn landed(repo: &Repository, repo_root: &Path, tip: git2::Oid, target: git2::Oid) -> bool {
    if tip == target || repo.graph_descendant_of(target, tip).unwrap_or(false) {
        return on_first_parent_line(repo, target, tip) == Some(false);
    }
    cherry_landed_in(repo, repo_root, target, tip)
}

/// Whether every commit `tip` adds over `target` is patch-equivalent there.
/// `git cherry` skips merge commits, so a range holding a merge (whose
/// resolution may carry its own change) is never proven this way.
pub(crate) fn cherry_landed(repo_root: &Path, target: &str, tip: &str) -> bool {
    if target.starts_with('-') || tip.starts_with('-') {
        return false;
    }
    let resolved = Repository::open(repo_root).ok().and_then(|repo| {
        let oid = |name: &str| {
            repo.revparse_single(name)
                .ok()?
                .peel_to_commit()
                .ok()
                .map(|c| c.id())
        };
        Some((oid(target)?, oid(tip)?, repo))
    });
    match resolved {
        Some((target, tip, repo)) => cherry_landed_in(&repo, repo_root, target, tip),
        None => spawned_cherry_landed(repo_root, target, tip),
    }
}

/// [`cherry_landed`] on resolved commits. Only `git cherry` proves a
/// landing; an in-process read first rules out the ranges it could never
/// prove, so thousands of stale branches cost no process each (R-103).
fn cherry_landed_in(
    repo: &Repository,
    repo_root: &Path,
    target: git2::Oid,
    tip: git2::Oid,
) -> bool {
    let (target_name, tip_name) = (target.to_string(), tip.to_string());
    match could_be_patch_equivalent(repo, target, tip) {
        Some(false) => false,
        Some(true) => cherry_all_equivalent(repo_root, &target_name, &tip_name),
        None => spawned_cherry_landed(repo_root, &target_name, &tip_name),
    }
}

fn spawned_cherry_landed(repo_root: &Path, target: &str, tip: &str) -> bool {
    let range = format!("{target}..{tip}");
    let merges = git(
        repo_root,
        &["rev-list", "--merges", "--max-count=1", &range],
    );
    if !merges.is_ok_and(|out| out.trim().is_empty()) {
        return false;
    }
    cherry_all_equivalent(repo_root, target, tip)
}

fn cherry_all_equivalent(repo_root: &Path, target: &str, tip: &str) -> bool {
    git(repo_root, &["cherry", target, tip])
        .is_ok_and(|out| !out.trim().is_empty() && out.lines().all(|line| line.starts_with("- ")))
}

/// Whether `git cherry target tip` could find every commit of `tip`
/// patch-equivalent on `target`, from what each commit changes: a patch id
/// hashes the name of every changed path with its whitespace removed, so
/// equal patch ids need equal changed paths under that same normalisation,
/// and a `tip` commit whose paths no `target` commit changes can never be
/// matched. `Some(false)` also covers a merge in
/// the range and a range that adds nothing, where the spawned check says
/// the same. `None` when the commits cannot be read here, or a commit changes
/// nothing: the spawned check then decides alone.
fn could_be_patch_equivalent(repo: &Repository, target: git2::Oid, tip: git2::Oid) -> Option<bool> {
    let mut added = Vec::new();
    let mut walk = repo.revwalk().ok()?;
    walk.push(tip).ok()?;
    walk.hide(target).ok()?;
    for oid in walk {
        let commit = repo.find_commit(oid.ok()?).ok()?;
        if commit.parent_count() > 1 {
            return Some(false);
        }
        added.push(changed_paths(repo, &commit)?);
    }
    if added.is_empty() {
        return Some(false);
    }
    let mut upstream = std::collections::HashSet::new();
    let mut walk = repo.revwalk().ok()?;
    walk.push(target).ok()?;
    walk.hide(tip).ok()?;
    for oid in walk {
        let commit = repo.find_commit(oid.ok()?).ok()?;
        // `git cherry` compares against non-merge commits only.
        if commit.parent_count() <= 1 {
            if let Some(paths) = changed_paths(repo, &commit) {
                upstream.insert(paths);
            }
        }
    }
    Some(added.iter().all(|paths| upstream.contains(paths)))
}

thread_local! {
    /// Changed-path digests by commit id. A commit id names its content, so
    /// the memo holds across repositories; it is cleared when large.
    static CHANGED_PATHS: std::cell::RefCell<std::collections::HashMap<git2::Oid, Option<u64>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// A digest of the paths `commit` changes against its first parent (the
/// diff `git cherry` takes a patch id of), each normalised as the patch id
/// normalises it: every whitespace byte removed, so `a b` and `ab` are one
/// path, as they are to `git cherry` after a rename. Both names of a changed
/// pair count, as both enter the patch id. `None` when the commit changes
/// nothing or cannot be read.
fn changed_paths(repo: &Repository, commit: &git2::Commit<'_>) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    if let Some(known) = CHANGED_PATHS.with(|memo| memo.borrow().get(&commit.id()).copied()) {
        return known;
    }
    let digest = (|| {
        let tree = commit.tree().ok()?;
        let parent = match commit.parent_count() {
            0 => None,
            _ => Some(commit.parent(0).ok()?.tree().ok()?),
        };
        let diff = repo
            .diff_tree_to_tree(parent.as_ref(), Some(&tree), None)
            .ok()?;
        let mut paths: Vec<Vec<u8>> = diff
            .deltas()
            .flat_map(|delta| [delta.old_file().path_bytes(), delta.new_file().path_bytes()])
            .flatten()
            .map(patch_id_path)
            .collect();
        if paths.is_empty() {
            return None;
        }
        paths.sort_unstable();
        paths.dedup();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        paths.hash(&mut hasher);
        Some(hasher.finish())
    })();
    CHANGED_PATHS.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.len() > 200_000 {
            memo.clear();
        }
        memo.insert(commit.id(), digest);
    });
    digest
}

/// A path as a patch id hashes it: without the bytes C's `isspace` matches
/// (git's `remove_space`).
fn patch_id_path(path: &[u8]) -> Vec<u8> {
    path.iter()
        .copied()
        .filter(|byte| !matches!(byte, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
        .collect()
}

/// Whether `tip` is on the first-parent line of `target`, by parent links
/// alone (commit dates need not follow ancestry). `None` when the walk
/// cannot be completed, which proves nothing.
fn on_first_parent_line(repo: &Repository, target: git2::Oid, tip: git2::Oid) -> Option<bool> {
    let mut current = repo.find_commit(target).ok()?;
    loop {
        if current.id() == tip {
            return Some(true);
        }
        if current.parent_count() == 0 {
            return Some(false);
        }
        current = current.parent(0).ok()?;
    }
}

/// Split the branches carrying one task into open claims and landed
/// branches, by name (a local branch and its `origin/` copy are one name,
/// open when any of its tips has not landed).
fn split_landed(
    repo: &Repository,
    repo_root: &Path,
    branches: &[(String, git2::Oid)],
    target: git2::Oid,
) -> (Vec<String>, Vec<String>) {
    let mut open = BTreeSet::new();
    let mut done = BTreeSet::new();
    for (name, tip) in branches {
        if landed(repo, repo_root, *tip, target) {
            done.insert(name.clone());
        } else {
            open.insert(name.clone());
        }
    }
    let done = done.difference(&open).cloned().collect();
    (open.into_iter().collect(), done)
}

fn fetched_at(repo: &Repository) -> Option<String> {
    let modified = std::fs::metadata(repo.commondir().join("FETCH_HEAD"))
        .and_then(|meta| meta.modified())
        .ok()?;
    Some(super::rfc3339_at(modified))
}

/// The declared targets of the task records in the working tree, as
/// written, and the default target for records that declare none.
fn declared_targets(repo_root: &Path) -> BTreeSet<String> {
    // One listing; the first record of each id speaks for it, as
    // `declared_work_target` finds it (R-103: no scan per record).
    let mut ids = BTreeSet::new();
    let mut targets: BTreeSet<String> =
        crate::workgraph::layout::task_record_files(&repo_root.join("project-management"))
            .into_iter()
            .filter(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .is_some_and(|stem| ids.insert(stem.to_string()))
            })
            .filter_map(|path| super::work_start::declared_work_target_at(&path))
            .collect();
    if let Some(default) = super::default_work_target(repo_root) {
        targets.insert(default);
    }
    targets
}

/// Declared targets resolved once each, as `work start` resolves them.
struct Targets<'a> {
    repo_root: &'a Path,
    repo: &'a Repository,
    seen: BTreeMap<String, Resolved>,
}

impl<'a> Targets<'a> {
    fn new(repo_root: &'a Path, repo: &'a Repository) -> Self {
        Self {
            repo_root,
            repo,
            seen: BTreeMap::new(),
        }
    }

    fn resolve(&mut self, declared: &str) -> Resolved {
        self.seen
            .entry(declared.to_string())
            .or_insert_with(|| resolve_target(self.repo_root, self.repo, declared))
            .clone()
    }
}

/// One target tip read: the declared target, the resolved ref, its tip and
/// the records there, shared with every other tip on the same tree.
type Tip = (String, String, git2::Oid, Rc<BTreeMap<String, Record>>);

/// Read each declared target's tip once (two spellings of one ref are one
/// tip) and name every snapshot, including the ones that cannot be read.
/// Each distinct tree is parsed once (R-103): refs on one tree keep their
/// own identity and are judged apart, over one shared set of records.
fn read_tips(
    repo: &Repository,
    targets: &mut Targets<'_>,
    declared: BTreeSet<String>,
    snapshots: &mut Vec<Snapshot>,
) -> Result<Vec<Tip>, String> {
    let mut tips: Vec<Tip> = Vec::new();
    let mut parsed: BTreeMap<git2::Oid, Rc<BTreeMap<String, Record>>> = BTreeMap::new();
    for target in declared {
        let (reference, tip, problem) = match targets.resolve(&target) {
            Ok(Some((reference, oid))) => (Some(reference), Some(oid), None),
            Ok(None) => (None, None, None),
            Err(problem) => (None, None, Some(problem)),
        };
        if let (Some(reference), Some(oid)) = (&reference, tip) {
            if tips
                .iter()
                .any(|(_, seen, tip, _)| seen == reference && *tip == oid)
            {
                continue;
            }
            let tree = repo
                .find_commit(oid)
                .and_then(|commit| commit.tree())
                .map_err(|error| error.to_string())?;
            let records = if let Some(records) = parsed.get(&tree.id()) {
                Rc::clone(records)
            } else {
                let records =
                    Rc::new(records_from_tree(repo, &tree).map_err(|error| error.to_string())?);
                parsed.insert(tree.id(), Rc::clone(&records));
                records
            };
            tips.push((target.clone(), reference.clone(), oid, records));
        }
        snapshots.push(Snapshot {
            target,
            reference: reference.as_deref().map(shown),
            tip: tip.map(|oid| oid.to_string()),
            problem,
        });
    }
    Ok(tips)
}

/// Compute the backlog from the last-fetched refs. Makes no network call.
///
/// # Errors
///
/// Returns the reason when the repository or a target tip cannot be read.
pub fn backlog(repo_root: &Path) -> Result<Backlog, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let prefixes = work_prefixes(repo_root);
    let default = super::default_work_target(repo_root);
    let mut out = Backlog {
        fetched_at: fetched_at(&repo),
        ..Backlog::default()
    };
    // Each declared target resolved once, as `work start` resolves it.
    let mut resolved = Targets::new(repo_root, &repo);
    let tips = read_tips(
        &repo,
        &mut resolved,
        declared_targets(repo_root),
        &mut out.snapshots,
    )?;
    let mut resolve = |declared: &str| resolved.resolve(declared);

    let ids: BTreeSet<String> = tips
        .iter()
        .flat_map(|(_, _, _, records)| records.keys().cloned())
        .collect();
    let carried = visible_work_branches(&repo, &prefixes, &ids);
    let mut judged = BTreeSet::new();
    for (_, reference, tip, records) in &tips {
        for record in records
            .values()
            .filter(|record| record.kind == RecordKind::Task)
        {
            // A record belongs to this tip when its own declared target
            // resolves to this ref, whatever spelling it uses.
            let Some(declared) = record
                .integration_target
                .clone()
                .or_else(|| default.clone())
            else {
                continue;
            };
            let here = matches!(resolve(&declared), Ok(Some((seen, oid))) if seen == *reference && oid == *tip);
            if !here || !judged.insert(record.id.clone()) {
                continue;
            }
            if let Some(epic) = &record.epic_id {
                let progress = out.epics.entry(epic.clone()).or_default();
                progress.total += 1;
                progress.complete += usize::from(record.status == "complete");
                progress.cancelled += usize::from(record.status == "cancelled");
            }
            let (names, done) = split_landed(
                &repo,
                repo_root,
                carried
                    .get(&record.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                *tip,
            );
            out.landed.extend(done);
            if names.len() > 1 {
                out.conflicts.insert(record.id.clone(), names.clone());
            }
            let (state, reason) =
                match validate_anchored_task(&repo, records, &record.id, reference) {
                    Ok(_) if names.is_empty() => (State::Ready, String::new()),
                    Ok(_) => (State::Active, String::new()),
                    Err(error) => match error.not_ready() {
                        NotReady::Closed => continue,
                        NotReady::Blocked => (State::Blocked, error.to_string()),
                        NotReady::Waiting => (State::Waiting, error.to_string()),
                        NotReady::Invalid => (State::Invalid, error.to_string()),
                    },
                };
            // A claim on a waiting task makes it active; a Blocker or an
            // invalid record keeps precedence over any branch, which is still
            // listed with the entry.
            let state = if state == State::Waiting && !names.is_empty() {
                State::Active
            } else {
                state
            };
            out.entries.push(Entry {
                task_id: record.id.clone(),
                title: record.title.clone(),
                epic_id: record.epic_id.clone(),
                target: declared.clone(),
                state,
                reason,
                branches: names,
            });
        }
    }
    out.entries
        .sort_by(|left, right| (left.state, &left.task_id).cmp(&(right.state, &right.task_id)));
    out.landed.sort();
    out.landed.dedup();
    Ok(out)
}

/// What `work claim` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub branch: String,
    pub from: String,
    pub pushed: bool,
}

fn git(repo_root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .map_err(|error| error.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Claim `task_id` (R-23): fetch the remote its target resolves through,
/// and `origin`, then resolve the target again exactly as `work start` does
/// and judge the task in the new-claim context on that tip; refuse when a
/// branch already carries its id (visible here, or listed on `origin` or the
/// target's remote); then create `task/<id>-<slug>` from that tip and
/// publish it to `origin` create-only. The pushed branch is an advisory mark
/// that others see; nothing depends on it being exclusive.
///
/// # Errors
///
/// Returns the reason the task cannot be claimed or git refused a step.
pub fn claim(repo_root: &Path, task_id: &str) -> Result<Claim, String> {
    let declared = super::declared_work_target(repo_root, task_id)
        .ok_or_else(|| format!("{task_id} has no visible record with an integration_target"))?;
    let remotes: Vec<String> = git(repo_root, &["remote"])?
        .lines()
        .map(str::to_string)
        .collect();
    let has_origin = remotes.iter().any(|remote| remote == "origin");
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    // Fetch the remote the target resolves through (never substituting
    // origin for it), and origin, where claims are published.
    let target_remote = target_fetch_remote(&repo, &declared)?
        .filter(|remote| remotes.iter().any(|known| known == remote));
    let mut fetch: Vec<&str> = target_remote.as_deref().into_iter().collect();
    if has_origin && !fetch.contains(&"origin") {
        fetch.push("origin");
    }
    for remote in fetch {
        git(repo_root, &["fetch", "--prune", "--quiet", remote])
            .map_err(|error| format!("fetch {remote} failed: {error}"))?;
    }
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let (reference, tip) = resolve_target(repo_root, &repo, &declared)?
        .ok_or_else(|| format!("target '{declared}' does not resolve here"))?;
    let tree = repo
        .find_commit(tip)
        .and_then(|commit| commit.tree())
        .map_err(|error| error.to_string())?;
    let records = records_from_tree(&repo, &tree).map_err(|error| error.to_string())?;
    let reference_shown = shown(&reference);
    validate_anchored_task(&repo, &records, task_id, &reference)
        .map_err(|error| format!("not ready on {reference_shown}: {error}"))?;
    let ids = BTreeSet::from([task_id.to_string()]);
    let prefixes = work_prefixes(repo_root);
    let mut carried = visible_work_branches(&repo, &prefixes, &ids)
        .remove(task_id)
        .unwrap_or_default();
    let mut listed: Vec<&str> = Vec::new();
    if has_origin {
        listed.push("origin");
    }
    if let Some(remote) = target_remote
        .as_deref()
        .filter(|remote| *remote != "origin")
    {
        listed.push(remote);
    }
    for remote in listed {
        carried.extend(remote_claims(&repo, repo_root, &prefixes, task_id, remote)?);
    }
    let (open, _) = split_landed(&repo, repo_root, &carried, tip);
    if !open.is_empty() {
        return Err(format!(
            "{task_id} is already claimed by a visible branch: {}",
            open.join(", ")
        ));
    }
    let title = records
        .get(task_id)
        .map_or("", |record| record.title.as_str());
    let branch = format!("task/{task_id}-{}", super::light_paths::slug(title));
    git(repo_root, &["branch", &branch, &tip.to_string()])?;
    if has_origin {
        // Create only: an existing remote branch of that name is never moved.
        let destination = format!("refs/heads/{branch}");
        git(
            repo_root,
            &[
                "push",
                "--quiet",
                "-u",
                &format!("--force-with-lease={destination}:"),
                "origin",
                &format!("{branch}:{destination}"),
            ],
        )
        .map_err(|error| format!("created {branch} locally; push to origin failed: {error}"))?;
    }
    Ok(Claim {
        branch,
        from: format!("{reference_shown}@{}", &tip.to_string()[..9]),
        pushed: has_origin,
    })
}

/// Branches on `remote` carrying `task_id`, read from the remote itself so
/// a narrow fetch refspec cannot hide a claim. A tip this clone does not
/// have is kept as an open claim (its landing cannot be proven here).
/// `origin`'s are named by branch, another remote's as `<remote>/<branch>`,
/// as [`visible_work_branches`] names them.
fn remote_claims(
    repo: &Repository,
    repo_root: &Path,
    prefixes: &[String],
    task_id: &str,
    remote: &str,
) -> Result<Vec<(String, git2::Oid)>, String> {
    let listed = git(repo_root, &["ls-remote", "--heads", remote])
        .map_err(|error| format!("cannot list {remote}'s branches: {error}"))?;
    Ok(listed
        .lines()
        .filter_map(|line| {
            let (sha, name) = line.split_once('\t')?;
            let name = name.strip_prefix("refs/heads/")?;
            let suffix = work_suffix(prefixes, name)?;
            if !suffix.starts_with(&format!("{task_id}-")) {
                return None;
            }
            let oid = git2::Oid::from_str(sha).ok()?;
            let known = repo.find_commit(oid).is_ok();
            let shown = if remote == "origin" {
                name.to_string()
            } else {
                format!("{remote}/{name}")
            };
            // An unknown tip is never landed: keep it open with a null id.
            Some((shown, if known { oid } else { git2::Oid::ZERO_SHA1 }))
        })
        .collect())
}

/// Whether `branch` (local, or `origin/<branch>`) holds an open claim: it
/// carries a task id on a sanctioned work prefix and its tip has not landed
/// on `target` (R-27). `work next` calls such a task active, so cleanup
/// keeps its branch, even when its tip is contained in the target because
/// nothing has been committed on it yet.
pub(crate) fn is_open_claim(
    repo_root: &Path,
    repo: &Repository,
    branch: &str,
    tip: git2::Oid,
    target: git2::Oid,
) -> bool {
    let name = branch.strip_prefix("origin/").unwrap_or(branch);
    let carries_task = work_suffix(&work_prefixes(repo_root), name).is_some_and(|suffix| {
        let mut parts = suffix.splitn(3, '-');
        match (parts.next(), parts.next()) {
            (Some(kind), Some(number)) => {
                super::is_valid_task_format_id(&format!("{kind}-{number}"))
            }
            _ => false,
        }
    });
    carries_task && !landed(repo, repo_root, tip, target)
}

/// Other open (unlanded) visible branches carrying `task_id` than `own`: the
/// own-branch context reports them as a conflict and does not refuse (R-110).
#[must_use]
pub fn other_branches(repo_root: &Path, task_id: &str, own: &str) -> Vec<String> {
    let Ok(repo) = Repository::discover(repo_root) else {
        return Vec::new();
    };
    let ids = BTreeSet::from([task_id.to_string()]);
    let carried = visible_work_branches(&repo, &work_prefixes(repo_root), &ids)
        .remove(task_id)
        .unwrap_or_default();
    let target_tip = super::declared_work_target(repo_root, task_id)
        .and_then(|target| resolve_target(repo_root, &repo, &target).ok().flatten())
        .map(|(_, oid)| oid);
    let open = match target_tip {
        Some(tip) => split_landed(&repo, repo_root, &carried, tip).0,
        None => carried.into_iter().map(|(name, _)| name).collect(),
    };
    open.into_iter()
        .filter(|name| name != own)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Tasks whose `awaiting_selection` the range from the merge-base of `base`
/// and `head` to `head` removes. A selection lands only by a planning pull
/// request (R-43), so CI refuses this on any other branch.
///
/// # Errors
///
/// Returns the reason when a revision or its records cannot be read.
pub fn selections_in_range(
    repo_root: &Path,
    base: &str,
    head: &str,
) -> Result<Vec<String>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| format!("{revision}: {error}"))
    };
    let head = commit(head)?;
    let base = repo
        .merge_base(commit(base)?.id(), head.id())
        .and_then(|oid| repo.find_commit(oid))
        .map_err(|error| error.to_string())?;
    let read = |commit: &git2::Commit<'_>| {
        commit
            .tree()
            .map_err(|error| error.to_string())
            .and_then(|tree| records_from_tree(&repo, &tree).map_err(|error| error.to_string()))
    };
    let before = read(&base)?;
    let after = read(&head)?;
    Ok(before
        .values()
        .filter(|record| record.kind == RecordKind::Task && record.awaiting_selection.is_some())
        .filter(|record| {
            after
                .get(&record.id)
                .is_some_and(|now| now.awaiting_selection.is_none())
        })
        .map(|record| record.id.clone())
        .collect())
}

/// Whether `branch` is an integration line that an open task still targets
/// (such a branch is never removable, R-44).
#[must_use]
pub fn is_live_integration_line(repo_root: &Path, branch: &str) -> bool {
    if !branch.starts_with("integration/") {
        return false;
    }
    crate::workgraph::layout::task_record_files(&repo_root.join("project-management"))
        .into_iter()
        .any(|path| {
            // An unreadable record may be the one that keeps the line live.
            let Ok(content) = std::fs::read_to_string(&path) else {
                return true;
            };
            let Ok(record) = parse_record(&content, RecordKind::Task) else {
                return true;
            };
            !matches!(record.status.as_str(), "complete" | "cancelled")
                && record
                    .integration_target
                    .as_deref()
                    .is_some_and(|target| canonical_target(target) == branch)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workgraph::work_start::{AnchoredTask, WorkStartError};
    use std::fs;

    fn run(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
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

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        run(root, &["init", "-q", "-b", "main"]);
        run(root, &["config", "user.email", "test@example.com"]);
        run(root, &["config", "user.name", "Test"]);
        for child in ["epics", "specs", "tasks"] {
            fs::create_dir_all(root.join("project-management").join(child)).unwrap();
        }
        epic(root, "EPC-001", "in_progress");
        fs::write(
            root.join("project-management/specs/SPC-001.md"),
            "---\nid: SPC-001\ntitle: contract\nstatus: approved\ncreated: 2026-09-27\n---\n",
        )
        .unwrap();
        dir
    }

    fn epic(root: &Path, id: &str, status: &str) {
        fs::write(
            root.join(format!("project-management/epics/{id}.md")),
            format!("---\nid: {id}\ntitle: outcome\nstatus: {status}\nwork_type: feat\nspecs: []\ncreated: 2026-09-27\n---\n"),
        )
        .unwrap();
    }

    /// A task on `main` in EPC-001 (or standalone when `epic` is None).
    fn task(root: &Path, id: &str, status: &str, deps: &str, extra: &str) {
        task_in(root, id, Some("EPC-001"), "main", status, deps, extra);
    }

    fn task_in(
        root: &Path,
        id: &str,
        epic: Option<&str>,
        target: &str,
        status: &str,
        deps: &str,
        extra: &str,
    ) {
        let (epic, reason) = match epic {
            Some(epic) => (epic.to_string(), "null".to_string()),
            None => ("null".to_string(), "\"a one-off fix\"".to_string()),
        };
        fs::write(
            root.join(format!("project-management/tasks/{id}.md")),
            format!(
                "---\nid: {id}\nepic_id: {epic}\nstandalone_reason: {reason}\nintegration_target: {target}\ntitle: \"work {id}\"\nstatus: {status}\nwork_type: feat\nspecs: []\ndepends_on: {deps}\n{extra}created: 2026-09-27\n---\n\n# {id}\n"
            ),
        )
        .unwrap();
    }

    fn commit(root: &Path, message: &str) -> String {
        run(root, &["add", "-A"]);
        run(root, &["commit", "-q", "-m", message]);
        run(root, &["rev-parse", "HEAD"])
    }

    fn verdict(root: &Path, id: &str) -> Result<AnchoredTask, WorkStartError> {
        let repo = Repository::open(root).unwrap();
        let tree = repo.head().unwrap().peel_to_tree().unwrap();
        let records = records_from_tree(&repo, &tree).unwrap();
        validate_anchored_task(&repo, &records, id, "main")
    }

    fn state(root: &Path, id: &str) -> Option<NotReady> {
        verdict(root, id).err().map(|error| error.not_ready())
    }

    #[test]
    fn the_core_holds_only_when_every_condition_holds() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "complete", "[]", "");
        task(root, "TSK-002", "todo", "[TSK-001]", "");
        task_in(root, "TSK-003", None, "main", "todo", "[]", "");
        task(root, "TSK-004", "in_progress", "[]", "");
        task(root, "TSK-005", "cancelled", "[]", "");
        task(root, "TSK-006", "blocked", "[]", "");
        task(
            root,
            "TSK-007",
            "todo",
            "[]",
            "awaiting_selection: docs/plan/choice.md\n",
        );
        task(root, "TSK-008", "todo", "[TSK-002]", "");
        task(root, "TSK-009", "todo", "[]", "");
        fs::write(
            root.join("project-management/specs/SPC-002.md"),
            "---\nid: SPC-002\ntitle: open\nstatus: draft\ncreated: 2026-09-27\n---\n",
        )
        .unwrap();
        let text = fs::read_to_string(root.join("project-management/tasks/TSK-009.md"))
            .unwrap()
            .replace("specs: []", "specs: [SPC-002]");
        fs::write(root.join("project-management/tasks/TSK-009.md"), text).unwrap();
        epic(root, "EPC-002", "complete");
        task_in(root, "TSK-010", Some("EPC-002"), "main", "todo", "[]", "");
        commit(root, "plan");

        assert!(verdict(root, "TSK-002").is_ok(), "all conditions hold");
        assert!(
            verdict(root, "TSK-003").is_ok(),
            "a standalone task with a reason"
        );
        assert!(
            verdict(root, "TSK-004").is_ok(),
            "in_progress stays readable"
        );
        assert_eq!(state(root, "TSK-001"), Some(NotReady::Closed));
        assert_eq!(state(root, "TSK-005"), Some(NotReady::Closed));
        assert_eq!(state(root, "TSK-006"), Some(NotReady::Blocked));
        assert!(matches!(
            verdict(root, "TSK-007"),
            Err(WorkStartError::AwaitingSelection { .. })
        ));
        assert_eq!(state(root, "TSK-007"), Some(NotReady::Blocked));
        assert!(matches!(
            verdict(root, "TSK-008"),
            Err(WorkStartError::DependencyIncomplete { .. })
        ));
        assert_eq!(state(root, "TSK-008"), Some(NotReady::Waiting));
        assert!(matches!(
            verdict(root, "TSK-009"),
            Err(WorkStartError::SpecNotReady { .. })
        ));
        assert_eq!(state(root, "TSK-009"), Some(NotReady::Waiting));
        assert!(matches!(
            verdict(root, "TSK-010"),
            Err(WorkStartError::EpicClosed { .. })
        ));
        assert_eq!(state(root, "TSK-010"), Some(NotReady::Blocked));
    }

    #[test]
    fn a_standalone_task_needs_its_reason() {
        let dir = repo();
        let root = dir.path();
        task_in(root, "TSK-001", None, "main", "todo", "[]", "");
        let text = fs::read_to_string(root.join("project-management/tasks/TSK-001.md"))
            .unwrap()
            .replace(
                "standalone_reason: \"a one-off fix\"",
                "standalone_reason: null",
            );
        fs::write(root.join("project-management/tasks/TSK-001.md"), text).unwrap();
        commit(root, "plan");
        assert_eq!(state(root, "TSK-001"), Some(NotReady::Invalid));
    }

    #[test]
    fn a_code_dependency_on_another_line_waits_for_the_change_here() {
        let dir = repo();
        let root = dir.path();
        let line = "integration/EPC-001-other";
        task_in(root, "TSK-001", Some("EPC-001"), line, "todo", "[]", "");
        task(root, "TSK-002", "todo", "[TSK-001]", "");
        commit(root, "plan");
        assert!(
            matches!(
                verdict(root, "TSK-002"),
                Err(WorkStartError::DependencyLineUnknown { .. })
            ),
            "an unfetched line is unknown, never satisfied"
        );
        run(root, &["switch", "-q", "-c", line]);
        task_in(root, "TSK-001", Some("EPC-001"), line, "complete", "[]", "");
        commit(root, "land TSK-001 on its line");
        run(root, &["switch", "-q", "main"]);
        let error = verdict(root, "TSK-002").unwrap_err();
        assert!(
            matches!(error, WorkStartError::DependencyOnOtherLine { .. }),
            "{error}"
        );
        assert_eq!(error.not_ready(), NotReady::Waiting);
        run(root, &["merge", "-q", "--no-ff", "-m", "port", line]);
        assert!(verdict(root, "TSK-002").is_ok(), "merged into this base");
    }

    #[test]
    fn a_research_dependency_is_met_at_its_pin_only() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        let open = commit(root, "plan");
        task(root, "TSK-001", "complete", "[]", "");
        let done = commit(root, "findings");
        run(root, &["switch", "-q", "-c", "side"]);
        fs::write(root.join("note.md"), "side\n").unwrap();
        let side = commit(root, "side");
        run(root, &["switch", "-q", "main"]);
        let consumer = |pin: &str| {
            let dep = if pin.is_empty() {
                "[{id: TSK-001, kind: research}]".to_string()
            } else {
                format!("[{{id: TSK-001, kind: research, pin: \"{pin}\"}}]")
            };
            task(root, "TSK-002", "todo", &dep, "");
            // The same predecessor may be a code dependency of another task.
            task(root, "TSK-003", "todo", "[TSK-001]", "");
            commit(root, "consumer");
            verdict(root, "TSK-002")
        };
        assert!(matches!(
            consumer(""),
            Err(WorkStartError::DependencyUnpinned { .. })
        ));
        let error = consumer(&open).unwrap_err();
        assert!(error.to_string().contains("'todo'"), "{error}");
        let error = consumer(&side).unwrap_err();
        assert!(error.to_string().contains("not on 'main'"), "{error}");
        assert!(consumer(&done).is_ok());
        assert!(verdict(root, "TSK-003").is_ok());
    }

    #[test]
    fn the_backlog_derives_every_state_from_the_same_core() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "complete", "[]", "");
        task(root, "TSK-002", "todo", "[TSK-001]", "");
        task(root, "TSK-003", "todo", "[TSK-004]", "");
        task(root, "TSK-004", "todo", "[]", "");
        task(root, "TSK-005", "blocked", "[]", "");
        task_in(root, "TSK-006", None, "main", "todo", "[]", "");
        task(root, "TSK-007", "todo", "[]", "");
        commit(root, "plan");
        // A claim that has not started: the branch sits on main's tip.
        run(root, &["branch", "task/TSK-004-claimed"]);
        // Two claims on one task: a conflict, reported not refused.
        run(root, &["branch", "task/TSK-007-one"]);
        run(root, &["switch", "-q", "-c", "fix/TSK-007-two"]);
        fs::write(root.join("fix.txt"), "fix\n").unwrap();
        commit(root, "fix");
        // A landed branch of the complete task.
        run(root, &["switch", "-q", "-c", "task/TSK-001-done", "main"]);
        fs::write(root.join("done.txt"), "done\n").unwrap();
        commit(root, "done");
        run(root, &["switch", "-q", "main"]);
        run(
            root,
            &["merge", "-q", "--no-ff", "-m", "land", "task/TSK-001-done"],
        );
        // A squash landing: patch-equivalent on main, not an ancestor.
        run(root, &["switch", "-q", "-c", "fix/TSK-001-squash", "main"]);
        fs::write(root.join("squash.txt"), "squash\n").unwrap();
        let squashed = commit(root, "squash");
        run(root, &["switch", "-q", "main"]);
        // Main moves on first, so the squash is a new commit, not the tip.
        fs::write(root.join("other.txt"), "other\n").unwrap();
        commit(root, "other");
        run(root, &["cherry-pick", &squashed]);

        let backlog = backlog(root).unwrap();
        let of = |id: &str| backlog.entries.iter().find(|entry| entry.task_id == id);
        assert_eq!(of("TSK-002").unwrap().state, State::Ready);
        assert_eq!(
            of("TSK-006").unwrap().state,
            State::Ready,
            "standalone included"
        );
        assert_eq!(of("TSK-003").unwrap().state, State::Waiting);
        assert!(of("TSK-003").unwrap().reason.contains("TSK-004"));
        assert_eq!(of("TSK-004").unwrap().state, State::Active);
        assert_eq!(of("TSK-004").unwrap().branches, ["task/TSK-004-claimed"]);
        assert_eq!(of("TSK-005").unwrap().state, State::Blocked);
        assert_eq!(of("TSK-007").unwrap().state, State::Active);
        assert!(of("TSK-001").is_none(), "closed tasks are not listed");
        assert_eq!(
            backlog.conflicts.get("TSK-007").unwrap(),
            &["fix/TSK-007-two", "task/TSK-007-one"]
        );
        assert_eq!(backlog.landed, ["fix/TSK-001-squash", "task/TSK-001-done"]);
        let progress = backlog.epics.get("EPC-001").unwrap();
        assert_eq!((progress.total, progress.complete), (6, 1));
        assert_eq!(backlog.entries[0].state, State::Ready, "ready first");
        let line = backlog.snapshot_line();
        assert!(line.starts_with("snapshot: main@"), "{line}");
        assert!(line.contains("fetched never"), "{line}");
        assert_eq!(
            backlog.counts_line(),
            "ready 2 · active 2 · waiting 1 · blocked 1 · invalid 0"
        );
    }

    fn with_origin(root: &Path) -> tempfile::TempDir {
        let bare = tempfile::tempdir().unwrap();
        run(bare.path(), &["init", "-q", "--bare"]);
        run(
            root,
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        run(root, &["push", "-q", "-u", "origin", "main"]);
        bare
    }

    #[test]
    fn a_claim_pushes_one_advisory_branch_and_refuses_a_second() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "complete", "[]", "");
        task(root, "TSK-002", "todo", "[TSK-001]", "");
        task(root, "TSK-003", "todo", "[TSK-002]", "");
        commit(root, "plan");
        let bare = with_origin(root);

        let claimed = claim(root, "TSK-002").unwrap();
        assert_eq!(claimed.branch, "task/TSK-002-work-tsk-002");
        assert!(claimed.pushed);
        // `main` tracks `origin/main` and equals it: the local branch is read,
        // as `work start` reads it.
        assert!(claimed.from.starts_with("main@"), "{}", claimed.from);
        let remote = run(bare.path(), &["branch", "--list"]);
        assert!(remote.contains("task/TSK-002-work-tsk-002"), "{remote}");

        let second = claim(root, "TSK-002").unwrap_err();
        assert!(second.contains("already claimed"), "{second}");
        assert!(!second.to_lowercase().contains("lock"), "{second}");
        let waiting = claim(root, "TSK-003").unwrap_err();
        assert!(waiting.contains("not ready on main"), "{waiting}");
        assert!(backlog(root).unwrap().fetched_at.is_some(), "claim fetched");
        assert_eq!(
            other_branches(root, "TSK-002", "task/TSK-002-work-tsk-002"),
            Vec::<String>::new()
        );
        assert_eq!(
            other_branches(root, "TSK-002", "fix/TSK-002-mine"),
            ["task/TSK-002-work-tsk-002"]
        );
    }

    #[test]
    fn a_claim_without_origin_stays_local() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        commit(root, "plan");
        let claimed = claim(root, "TSK-001").unwrap();
        assert!(!claimed.pushed);
        assert!(claimed.from.starts_with("main@"));
    }

    #[test]
    fn a_selection_removed_in_the_range_is_named() {
        let dir = repo();
        let root = dir.path();
        task(
            root,
            "TSK-001",
            "blocked",
            "[]",
            "awaiting_selection: docs/plan/choice.md\n",
        );
        commit(root, "plan");
        run(root, &["switch", "-q", "-c", "task/TSK-002-x"]);
        task(root, "TSK-001", "todo", "[]", "");
        commit(root, "select");
        assert_eq!(
            selections_in_range(root, "main", "HEAD").unwrap(),
            ["TSK-001"]
        );
        assert!(selections_in_range(root, "main", "main")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn only_an_integration_line_an_open_task_targets_is_live() {
        let dir = repo();
        let root = dir.path();
        task_in(
            root,
            "TSK-001",
            Some("EPC-001"),
            "integration/EPC-001-a",
            "todo",
            "[]",
            "",
        );
        task_in(
            root,
            "TSK-002",
            Some("EPC-001"),
            "integration/EPC-001-b",
            "complete",
            "[]",
            "",
        );
        assert!(is_live_integration_line(root, "integration/EPC-001-a"));
        assert!(!is_live_integration_line(root, "integration/EPC-001-b"));
        assert!(!is_live_integration_line(root, "task/TSK-001-x"));
    }

    // Review round 1 (Codex, TSK-103): each probe pinned as a test.

    fn entry<'b>(backlog: &'b Backlog, id: &str) -> &'b Entry {
        backlog
            .entries
            .iter()
            .find(|entry| entry.task_id == id)
            .unwrap()
    }

    fn commit_dated(root: &Path, message: &str, date: &str) -> String {
        run(root, &["add", "-A"]);
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["commit", "-q", "-m", message])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(out.status.success());
        run(root, &["rev-parse", "HEAD"])
    }

    /// T103-1: a tag or branch named like a pin cannot redirect it. The pin
    /// is `open`'s full object id and each ref is named exactly that, so no
    /// part of the case depends on how a prefix happens to look (TSK-156
    /// review T156-2).
    #[test]
    fn a_pin_is_an_object_id_never_a_ref_name() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        let open = commit(root, "plan");
        task(root, "TSK-001", "complete", "[]", "");
        let done = commit(root, "findings");
        pinned_consumer(root, &open);
        run(root, &["tag", &open, &done]);
        run(root, &["branch", &open, &done]);
        let error = verdict(root, "TSK-002").unwrap_err();
        assert!(error.to_string().contains("'todo'"), "{error}");
        assert_eq!(
            entry(&backlog(root).unwrap(), "TSK-002").state,
            State::Waiting
        );
    }

    /// An abbreviated pin resolves as the object it abbreviates, never as a
    /// ref of the same name. The abbreviation is the one git reports as
    /// unambiguous in this repository (`rev-parse --short=12`), so it cannot
    /// collide with another object, and it is quoted.
    #[test]
    fn an_abbreviated_pin_resolves_as_its_object_never_a_ref_name() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        let open = commit(root, "plan");
        task(root, "TSK-001", "complete", "[]", "");
        let done = commit(root, "findings");
        let short = run(root, &["rev-parse", "--short=12", &open]);
        assert!(open.starts_with(&short) && short.len() >= 12, "{short}");
        pinned_consumer(root, &short);
        run(root, &["tag", &short, &done]);
        run(root, &["branch", &short, &done]);
        let error = verdict(root, "TSK-002").unwrap_err();
        assert!(error.to_string().contains("'todo'"), "{error}");
        // The same abbreviation of the completing commit is met.
        let met = run(root, &["rev-parse", "--short=12", &done]);
        pinned_consumer(root, &met);
        assert!(verdict(root, "TSK-002").is_ok());
    }

    /// TSK-002 depends on TSK-001's research at the quoted `pin`.
    fn pinned_consumer(root: &Path, pin: &str) {
        task(
            root,
            "TSK-002",
            "todo",
            &format!("[{{id: TSK-001, kind: research, pin: \"{pin}\"}}]"),
            "",
        );
        commit(root, "consumer");
    }

    /// TSK-156 AC-3 and TSK-142 AC-4: fixed pins, never a random commit
    /// prefix. Unquoted, a digit or exponent pin YAML reads as a number
    /// leaves the graph unreadable with the quote remedy; quoted, or kept as
    /// text, each is read as an object id and judged as one.
    #[test]
    fn a_pin_is_judged_the_same_for_fixed_digit_exponent_and_letter_cases() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "complete", "[]", "");
        commit(root, "findings");
        let graph = |pin: &str| {
            task(
                root,
                "TSK-002",
                "todo",
                &format!("[{{id: TSK-001, kind: research, pin: {pin}}}]"),
                "",
            );
            commit(root, "consumer");
            let repo = Repository::open(root).unwrap();
            let tree = repo.head().unwrap().peel_to_tree().unwrap();
            records_from_tree(&repo, &tree).map(|_| ())
        };
        for pin in ["70283613", "12345678", "949894e0"] {
            let error = graph(pin).unwrap_err().to_string();
            assert!(
                error.contains("pin reads as a YAML number"),
                "{pin}: {error}"
            );
            assert!(error.contains("must be quoted"), "{pin}: {error}");
            assert!(!error.contains("not a commit id"), "{pin}: {error}");
        }
        for pin in [
            "\"70283613\"",
            "\"12345678\"",
            "\"949894e0\"",
            "\"12e45678\"",
            // Too large for a float, so YAML keeps it as text as written.
            "12e45678",
            "0123abcd",
            "\"0123abcd\"",
        ] {
            graph(pin).unwrap();
            let error = verdict(root, "TSK-002").unwrap_err().to_string();
            assert!(
                error.contains("not an object in this repository"),
                "{pin}: {error}"
            );
        }
    }

    /// T103-2: a target on another remote is read there, never from origin.
    #[test]
    fn a_declared_remote_target_is_never_read_from_origin() {
        let dir = repo();
        let root = dir.path();
        let blocked =
            "## Blocker\n\n- reason: awaiting operator\n- owner: operator\n- revisit: approved\n";
        task_in(
            root,
            "TSK-001",
            Some("EPC-001"),
            "refs/remotes/upstream/main",
            "blocked",
            "[]",
            "",
        );
        let path = root.join("project-management/tasks/TSK-001.md");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("{text}\n{blocked}")).unwrap();
        commit(root, "central");
        let central = tempfile::tempdir().unwrap();
        run(
            Path::new("."),
            &[
                "clone",
                "-q",
                "--bare",
                root.to_str().unwrap(),
                central.path().to_str().unwrap(),
            ],
        );
        run(
            root,
            &[
                "remote",
                "add",
                "upstream",
                central.path().to_str().unwrap(),
            ],
        );
        run(root, &["fetch", "-q", "upstream"]);
        task_in(
            root,
            "TSK-001",
            Some("EPC-001"),
            "refs/remotes/upstream/main",
            "todo",
            "[]",
            "",
        );
        commit(root, "fork");
        let fork = with_origin(root);
        run(root, &["fetch", "-q", "origin"]);
        let backlog = backlog(root).unwrap();
        assert_eq!(entry(&backlog, "TSK-001").state, State::Blocked);
        assert!(
            backlog.snapshot_line().contains("upstream/main@"),
            "{}",
            backlog.snapshot_line()
        );
        let refused = claim(root, "TSK-001").unwrap_err();
        assert!(refused.contains("not ready on upstream/main"), "{refused}");
        drop(fork);
    }

    /// T103-3: a visible Blocker holds a task whatever its status; one in a
    /// comment does not.
    #[test]
    fn a_visible_blocker_holds_a_todo_task() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        task(root, "TSK-002", "todo", "[]", "");
        task(root, "TSK-003", "todo", "[]", "");
        for (id, section) in [
            (
                "TSK-001",
                "## Blocker\n\n- reason: vendor reply\n- owner: ops\n- revisit: reply\n",
            ),
            ("TSK-002", "<!--\n## Blocker\n\n- reason: example\n-->\n"),
            ("TSK-003", "## Blocker\n\n- owner: ops\n"),
        ] {
            let path = root.join(format!("project-management/tasks/{id}.md"));
            let text = fs::read_to_string(&path).unwrap();
            fs::write(&path, format!("{text}\n{section}")).unwrap();
        }
        commit(root, "plan");
        let error = verdict(root, "TSK-001").unwrap_err();
        assert!(error.to_string().contains("vendor reply"), "{error}");
        assert!(
            verdict(root, "TSK-002").is_ok(),
            "a commented Blocker is not operative"
        );
        assert_eq!(
            state(root, "TSK-003"),
            Some(NotReady::Blocked),
            "an incomplete Blocker still holds"
        );
    }

    /// T103-4: a claim on origin that a narrow fetch refspec hides still
    /// counts.
    #[test]
    fn a_remote_claim_hidden_by_the_refspec_still_counts() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        commit(root, "plan");
        let origin = with_origin(root);
        run(
            root,
            &["push", "-q", "origin", "main:refs/heads/task/TSK-001-other"],
        );
        run(
            root,
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/main:refs/remotes/origin/main",
            ],
        );
        run(root, &["fetch", "-q", "--prune", "origin"]);
        // Another clone pushed it: this clone never tracked it.
        run(
            root,
            &["update-ref", "-d", "refs/remotes/origin/task/TSK-001-other"],
        );
        assert!(run(root, &["branch", "-r"])
            .lines()
            .all(|line| !line.contains("TSK-001")));
        let refused = claim(root, "TSK-001").unwrap_err();
        assert!(refused.contains("task/TSK-001-other"), "{refused}");
        let remote = run(origin.path(), &["branch", "--list"]);
        assert!(!remote.contains("task/TSK-001-work"), "{remote}");
    }

    /// T103-5: a merge commit's own change is never proven landed by
    /// patch equivalence.
    #[test]
    fn an_unmatched_merge_is_never_proof_of_landing() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        commit(root, "plan");
        run(root, &["switch", "-q", "-c", "side"]);
        fs::write(root.join("side.txt"), "side\n").unwrap();
        let side = commit(root, "side");
        run(root, &["switch", "-q", "-c", "task/TSK-001-work", "main"]);
        fs::write(root.join("work.txt"), "work\n").unwrap();
        let work = commit(root, "work");
        run(root, &["merge", "-q", "--no-ff", "--no-commit", "side"]);
        fs::write(root.join("merge-only.txt"), "only in the merge\n").unwrap();
        commit(root, "merge side");
        run(root, &["switch", "-q", "main"]);
        fs::write(root.join("other.txt"), "other\n").unwrap();
        commit(root, "other");
        // Every ordinary commit is patch-equivalent on main; the merge's own
        // file is not there.
        run(root, &["cherry-pick", &work]);
        run(root, &["cherry-pick", &side]);
        assert!(!root.join("merge-only.txt").exists());
        let backlog = backlog(root).unwrap();
        assert_eq!(entry(&backlog, "TSK-001").state, State::Active);
        assert!(backlog.landed.is_empty(), "{:?}", backlog.landed);
    }

    /// T103-6: first-parent membership follows parent links, not dates.
    #[test]
    fn an_older_dated_child_does_not_land_a_claim() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        commit_dated(root, "plan", "2026-09-27T12:00:00Z");
        run(root, &["branch", "task/TSK-001-claim"]);
        fs::write(root.join("later.txt"), "later\n").unwrap();
        commit_dated(root, "child dated earlier", "2026-09-26T12:00:00Z");
        let backlog = backlog(root).unwrap();
        assert_eq!(entry(&backlog, "TSK-001").state, State::Active);
        assert!(backlog.landed.is_empty(), "{:?}", backlog.landed);
    }

    /// T103-7: the live line is read through the record parser.
    #[test]
    fn the_live_line_is_read_through_the_record_parser() {
        let dir = repo();
        let root = dir.path();
        let line = "integration/EPC-001-live";
        task_in(root, "TSK-001", Some("EPC-001"), line, "todo", "[]", "");
        let path = root.join("project-management/tasks/TSK-001.md");
        let text = fs::read_to_string(&path).unwrap().replace(
            &format!("integration_target: {line}"),
            &format!("integration_target: '{line}'"),
        );
        fs::write(&path, format!("{text}\nAn example:\nstatus: complete\n")).unwrap();
        assert!(is_live_integration_line(root, line));
        fs::write(
            root.join("project-management/tasks/TSK-002.md"),
            "not a record",
        )
        .unwrap();
        assert!(
            is_live_integration_line(root, "integration/EPC-001-other"),
            "unreadable fails safe"
        );
    }

    /// T103-8: a Blocker outranks a claim, and the branch stays listed.
    #[test]
    fn a_blocked_task_with_a_branch_stays_blocked() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "blocked", "[]", "");
        let path = root.join("project-management/tasks/TSK-001.md");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            format!("{text}\n## Blocker\n\n- reason: vendor\n- owner: ops\n- revisit: reply\n"),
        )
        .unwrap();
        commit(root, "plan");
        run(root, &["branch", "task/TSK-001-work"]);
        let backlog = backlog(root).unwrap();
        let blocked = entry(&backlog, "TSK-001");
        assert_eq!(blocked.state, State::Blocked);
        assert_eq!(blocked.branches, ["task/TSK-001-work"]);
        assert!(blocked.reason.contains("vendor"));
        assert_eq!(backlog.in_state(State::Active).count(), 0);
    }

    const HOLD: &str =
        "\n## Blocker\n\n- reason: upstream hold\n- owner: operator\n- revisit: approved\n";

    /// TSK-001 on `main`, held by a Blocker when `held`.
    fn main_task(root: &Path, held: bool) {
        task(
            root,
            "TSK-001",
            if held { "blocked" } else { "todo" },
            "[]",
            "",
        );
        if held {
            let path = root.join("project-management/tasks/TSK-001.md");
            let text = fs::read_to_string(&path).unwrap();
            fs::write(&path, format!("{text}{HOLD}")).unwrap();
        }
    }

    fn bare_from(root: &Path, source: &str) -> tempfile::TempDir {
        let bare = tempfile::tempdir().unwrap();
        run(bare.path(), &["init", "-q", "--bare"]);
        run(
            root,
            &[
                "push",
                "-q",
                bare.path().to_str().unwrap(),
                &format!("{source}:refs/heads/main"),
            ],
        );
        bare
    }

    /// A clone whose `main` tracks `upstream/main` (TSK-001 held when
    /// `upstream_held`), with `origin` a fork one commit ahead of upstream
    /// that flips the hold. Every remote is fetched. Returns the remotes.
    fn forked(root: &Path, upstream_held: bool) -> (tempfile::TempDir, tempfile::TempDir) {
        main_task(root, upstream_held);
        commit(root, "upstream plan");
        let upstream = bare_from(root, "main");
        run(
            root,
            &[
                "remote",
                "add",
                "upstream",
                upstream.path().to_str().unwrap(),
            ],
        );
        run(root, &["fetch", "-q", "upstream"]);
        run(root, &["branch", "-q", "-u", "upstream/main", "main"]);
        run(root, &["switch", "-q", "-c", "fork", "main"]);
        main_task(root, !upstream_held);
        commit(root, "fork flips the hold");
        let origin = bare_from(root, "fork");
        run(
            root,
            &["remote", "add", "origin", origin.path().to_str().unwrap()],
        );
        run(root, &["switch", "-q", "main"]);
        run(root, &["branch", "-q", "-D", "fork"]);
        run(root, &["fetch", "-q", "origin"]);
        (upstream, origin)
    }

    /// `work start` on a fresh task branch cut from `from`, with the target
    /// resolved as the CLI resolves it.
    fn start_from(root: &Path, from: &str) -> Result<(), String> {
        run(root, &["switch", "-q", "-c", "task/TSK-001-probe", from]);
        let result = crate::workgraph::resolve_work_target_checked(root, Some("main"))
            .map_err(|error| error.to_string())
            .and_then(|resolved| {
                let target = resolved.map(|resolved| resolved.target).unwrap();
                crate::workgraph::check_work_start(root, "TSK-001", &target)
                    .map(drop)
                    .map_err(|error| error.to_string())
            });
        run(root, &["switch", "-q", "main"]);
        run(root, &["branch", "-q", "-D", "task/TSK-001-probe"]);
        result
    }

    /// R2-1: `main` tracks `upstream/main`; `origin` is a fork that removes
    /// the upstream Blocker. Next, claim and start all read `main` (the
    /// configured upstream line) and all refuse.
    #[test]
    fn a_fork_origin_never_stands_in_for_the_configured_upstream() {
        let dir = repo();
        let root = dir.path();
        let (_upstream, _origin) = forked(root, true);
        let backlog = backlog(root).unwrap();
        assert_eq!(entry(&backlog, "TSK-001").state, State::Blocked);
        assert!(
            backlog.snapshot_line().contains(" main@"),
            "{}",
            backlog.snapshot_line()
        );
        let refused = claim(root, "TSK-001").unwrap_err();
        assert!(refused.contains("not ready on main"), "{refused}");
        assert!(refused.contains("upstream hold"), "{refused}");
        let started = start_from(root, "main").unwrap_err();
        assert!(started.contains("upstream hold"), "{started}");
    }

    /// R2-1, the other direction: upstream is ready and the fork adds a
    /// Blocker. All three accept, and the claim is cut from `main`.
    #[test]
    fn a_ready_upstream_is_ready_whatever_the_fork_says() {
        let dir = repo();
        let root = dir.path();
        let (_upstream, origin) = forked(root, false);
        let backlog = backlog(root).unwrap();
        assert_eq!(entry(&backlog, "TSK-001").state, State::Ready);
        let claimed = claim(root, "TSK-001").unwrap();
        assert!(claimed.from.starts_with("main@"), "{}", claimed.from);
        let published = run(origin.path(), &["branch", "--list"]);
        assert!(published.contains(&claimed.branch), "{published}");
        start_from(root, &claimed.branch).unwrap();
    }

    /// R2-1 controls: a local `main` ahead of its upstream is read as it is;
    /// one diverged from it is refused by all three with one reason.
    #[test]
    fn a_local_target_ahead_is_read_and_a_diverged_one_is_refused() {
        let dir = repo();
        let root = dir.path();
        let (upstream, _origin) = forked(root, true);
        main_task(root, false);
        commit(root, "local release of the hold");
        let ahead = backlog(root).unwrap();
        assert_eq!(entry(&ahead, "TSK-001").state, State::Ready);
        start_from(root, "main").unwrap();

        // Upstream moves on too: the local branch and its upstream diverge.
        let other = tempfile::tempdir().unwrap();
        run(
            Path::new("."),
            &[
                "clone",
                "-q",
                upstream.path().to_str().unwrap(),
                other.path().to_str().unwrap(),
            ],
        );
        run(other.path(), &["config", "user.email", "test@example.com"]);
        run(other.path(), &["config", "user.name", "Test"]);
        fs::write(other.path().join("elsewhere.txt"), "x\n").unwrap();
        commit(other.path(), "upstream moves");
        run(other.path(), &["push", "-q", "origin", "main"]);
        run(root, &["fetch", "-q", "upstream"]);
        let diverged = backlog(root).unwrap();
        assert!(diverged
            .entries
            .iter()
            .all(|entry| entry.task_id != "TSK-001"));
        let line = diverged.snapshot_line();
        assert!(line.contains("main (local branch 'main' and"), "{line}");
        assert!(line.contains("have diverged"), "{line}");
        let refused = claim(root, "TSK-001").unwrap_err();
        assert!(refused.contains("have diverged"), "{refused}");
        let started = start_from(root, "main").unwrap_err();
        assert!(started.contains("have diverged"), "{started}");
    }

    /// R2-2: a claim visible on the declared target's remote counts, fetched
    /// or listed only by that remote; it is named with its remote.
    #[test]
    fn a_claim_on_the_target_remote_counts() {
        let dir = repo();
        let root = dir.path();
        task_in(
            root,
            "TSK-001",
            Some("EPC-001"),
            "refs/remotes/upstream/main",
            "todo",
            "[]",
            "",
        );
        commit(root, "plan");
        let upstream = bare_from(root, "main");
        run(
            upstream.path(),
            &["branch", "task/TSK-001-already-working", "main"],
        );
        run(
            root,
            &[
                "remote",
                "add",
                "upstream",
                upstream.path().to_str().unwrap(),
            ],
        );
        run(root, &["fetch", "-q", "upstream"]);
        let _origin = with_origin(root);

        let backlog = backlog(root).unwrap();
        let claimed = entry(&backlog, "TSK-001");
        assert_eq!(claimed.state, State::Active);
        assert_eq!(claimed.branches, ["upstream/task/TSK-001-already-working"]);
        let refused = claim(root, "TSK-001").unwrap_err();
        assert!(
            refused.contains("upstream/task/TSK-001-already-working"),
            "{refused}"
        );

        // A refspec that fetches only `main` hides it locally; listing the
        // remote still finds it.
        run(
            root,
            &[
                "config",
                "remote.upstream.fetch",
                "+refs/heads/main:refs/remotes/upstream/main",
            ],
        );
        run(
            root,
            &[
                "update-ref",
                "-d",
                "refs/remotes/upstream/task/TSK-001-already-working",
            ],
        );
        let refused = claim(root, "TSK-001").unwrap_err();
        assert!(
            refused.contains("upstream/task/TSK-001-already-working"),
            "{refused}"
        );
    }

    /// R2-3: a just-created claim is active in `work next` and retained,
    /// never removable, in the same `status` output.
    #[test]
    fn a_fresh_claim_is_active_and_never_removable() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        commit(root, "plan");
        let _origin = with_origin(root);
        let claimed = claim(root, "TSK-001").unwrap();
        let view = crate::status::collect_status(root);
        let rendered = crate::status::render_status(&view, false);
        let line = rendered
            .lines()
            .find(|line| line.contains(&claimed.branch) && line.contains("retain"))
            .unwrap_or_else(|| panic!("no cleanup line for the claim:\n{rendered}"));
        assert!(line.contains("retain-live"), "{line}");
        assert!(line.contains("open claim"), "{line}");
        assert!(
            !rendered
                .lines()
                .any(|line| line.contains(&claimed.branch) && line.contains("removable")),
            "{rendered}"
        );
        assert!(rendered.contains("active"), "{rendered}");
    }

    /// TSK-110 AC-2: the in-process read decides no landing by itself. On
    /// each shape it agrees with `git cherry` alone, and it spares the
    /// process only where `git cherry` could never prove a landing.
    #[test]
    fn the_in_process_landing_read_agrees_with_git_cherry() {
        let dir = repo();
        let root = dir.path();
        fs::write(root.join("a.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
        commit(root, "base");
        let branch = |name: &str, file: &str, text: &str| {
            run(root, &["switch", "-q", "-c", name, "main"]);
            fs::write(root.join(file), text).unwrap();
            commit(root, name)
        };
        // Squash-landed: the same change is on main as a new commit.
        let squashed = branch("task/TSK-001-squash", "s.txt", "s\n");
        // Rebase-landed with shifted context: the patch still matches.
        let shifted = branch(
            "task/TSK-002-shift",
            "a.txt",
            "1\n2\n3\n4\n5\n6\n7\n8\nnine\n",
        );
        // Same path, another change: a candidate `git cherry` refuses.
        branch("task/TSK-003-other", "s.txt", "other\n");
        // Never landed, and partly landed.
        branch("task/TSK-004-open", "open.txt", "open\n");
        let part = branch("task/TSK-005-part", "p1.txt", "p1\n");
        fs::write(root.join("p2.txt"), "p2\n").unwrap();
        commit(root, "part two");
        // A merge in the range, and a commit that changes nothing.
        run(root, &["switch", "-q", "-c", "task/TSK-006-merge", "main"]);
        run(
            root,
            &["merge", "-q", "--no-ff", "-m", "merge", "task/TSK-004-open"],
        );
        run(root, &["switch", "-q", "-c", "task/TSK-007-empty", "main"]);
        run(root, &["commit", "-q", "--allow-empty", "-m", "empty"]);
        run(root, &["switch", "-q", "main"]);
        fs::write(root.join("a.txt"), "zero\n1\n2\n3\n4\n5\n6\n7\n8\n9\n").unwrap();
        commit(root, "main moves");
        run(root, &["cherry-pick", &squashed]);
        run(root, &["cherry-pick", &shifted]);
        run(root, &["cherry-pick", &part]);

        let repo = Repository::open(root).unwrap();
        let oid = |name: &str| {
            repo.revparse_single(name)
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
        };
        let main = oid("main");
        for (name, landed, spared) in [
            ("task/TSK-001-squash", true, Some(true)),
            ("task/TSK-002-shift", true, Some(true)),
            ("task/TSK-003-other", false, Some(true)),
            ("task/TSK-004-open", false, Some(false)),
            ("task/TSK-005-part", false, Some(false)),
            ("task/TSK-006-merge", false, Some(false)),
            ("task/TSK-007-empty", false, None),
        ] {
            let alone = spawned_cherry_landed(root, "main", name);
            assert_eq!(alone, landed, "{name}: git cherry alone");
            assert_eq!(cherry_landed(root, "main", name), alone, "{name}");
            assert_eq!(
                could_be_patch_equivalent(&repo, main, oid(name)),
                spared,
                "{name}: in-process read"
            );
        }
    }

    /// Lines cut at one tree, as one commit or as commits that differ only
    /// in metadata, are parsed once and still judged each on its own line.
    #[test]
    fn refs_on_one_tree_parse_it_once() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        for (id, line) in [
            ("TSK-002", "integration/EPC-001-a"),
            ("TSK-003", "integration/EPC-001-b"),
        ] {
            task_in(root, id, Some("EPC-001"), line, "todo", "[]", "");
        }
        commit(root, "plan");
        run(root, &["branch", "integration/EPC-001-a"]);
        run(root, &["switch", "-q", "-c", "integration/EPC-001-b"]);
        run(root, &["commit", "-q", "--allow-empty", "-m", "cut"]);
        run(root, &["switch", "-q", "main"]);

        super::super::work_start::TREE_PARSES.with(|parses| parses.set(0));
        let backlog = backlog(root).unwrap();
        let parses = super::super::work_start::TREE_PARSES.with(std::cell::Cell::get);
        assert_eq!(parses, 1, "three refs on one tree");
        let judged: Vec<(&str, &str, State)> = backlog
            .entries
            .iter()
            .map(|entry| (entry.task_id.as_str(), entry.target.as_str(), entry.state))
            .collect();
        assert_eq!(
            judged,
            [
                ("TSK-001", "main", State::Ready),
                ("TSK-002", "integration/EPC-001-a", State::Ready),
                ("TSK-003", "integration/EPC-001-b", State::Ready),
            ]
        );
        let tips: BTreeSet<_> = backlog.snapshots.iter().map(|shot| &shot.tip).collect();
        assert_eq!(tips.len(), 2, "each line keeps its own tip");
    }

    /// Renames, copies and mode changes are where changed paths could part
    /// from a patch id: the in-process read never rules out a branch
    /// `git cherry` alone finds landed, under default rename handling and
    /// with copies detected.
    #[test]
    fn the_in_process_landing_read_agrees_with_git_cherry_across_renames() {
        let dir = repo();
        let root = dir.path();
        run(root, &["config", "core.fileMode", "false"]);
        for (file, text) in [
            ("a b", "space\n"),
            ("cd", "joined\n"),
            ("dir x/f.txt", "in a dir\n"),
            ("x.txt", "plain\n"),
            ("m n", "renamed by the task\n"),
            ("c.txt", "copied\n"),
            ("run.sh", "echo run\n"),
        ] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        commit(root, "base");
        let edit = |name: &str, file: &str| {
            run(root, &["switch", "-q", "-c", name, "main"]);
            fs::write(root.join(file), format!("{name}\n")).unwrap();
            commit(root, name)
        };
        let space_removed = edit("task/TSK-011-space-removed", "a b");
        let space_added = edit("task/TSK-012-space-added", "cd");
        let dir_space = edit("task/TSK-013-dir-space", "dir x/f.txt");
        let renamed = edit("task/TSK-014-renamed", "x.txt");
        run(
            root,
            &["switch", "-q", "-c", "task/TSK-015-task-renames", "main"],
        );
        run(root, &["mv", "m n", "mn"]);
        let task_renames = commit(root, "task renames");
        run(root, &["switch", "-q", "-c", "task/TSK-016-copy", "main"]);
        fs::write(root.join("c copy.txt"), "copied\n").unwrap();
        let copy = commit(root, "copy");
        run(root, &["switch", "-q", "-c", "task/TSK-017-mode", "main"]);
        run(root, &["update-index", "--chmod=+x", "run.sh"]);
        run(root, &["commit", "-q", "-m", "mode"]);
        let mode = run(root, &["rev-parse", "HEAD"]);

        run(root, &["switch", "-q", "main"]);
        for (from, to, pick) in [
            ("a b", "ab", &space_removed),
            ("cd", "c d", &space_added),
            ("dir x", "dirx", &dir_space),
            ("x.txt", "y.txt", &renamed),
        ] {
            run(root, &["mv", from, to]);
            commit(root, &format!("rename {from}"));
            run(root, &["cherry-pick", pick]);
        }
        for pick in [&task_renames, &copy, &mode] {
            run(root, &["cherry-pick", pick]);
        }

        let repo = Repository::open(root).unwrap();
        let oid = |name: &str| {
            repo.revparse_single(name)
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
        };
        let main = oid("main");
        for copies in [false, true] {
            if copies {
                run(root, &["config", "diff.renames", "copies"]);
            }
            for (name, landed) in [
                ("task/TSK-011-space-removed", true),
                ("task/TSK-012-space-added", true),
                ("task/TSK-013-dir-space", true),
                ("task/TSK-014-renamed", false),
                ("task/TSK-015-task-renames", true),
                ("task/TSK-016-copy", true),
                ("task/TSK-017-mode", true),
            ] {
                let alone = spawned_cherry_landed(root, "main", name);
                assert_eq!(alone, landed, "{name} (copies {copies}): git cherry alone");
                assert_eq!(
                    cherry_landed(root, "main", name),
                    alone,
                    "{name} (copies {copies})"
                );
                if alone {
                    assert_ne!(
                        could_be_patch_equivalent(&repo, main, oid(name)),
                        Some(false),
                        "{name} (copies {copies}): in-process read ruled out a landing"
                    );
                }
            }
        }
    }
}
