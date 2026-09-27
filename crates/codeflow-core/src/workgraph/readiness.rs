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
            .map(|snapshot| match (&snapshot.reference, &snapshot.tip) {
                (Some(reference), Some(tip)) => format!("{reference}@{}", &tip[..tip.len().min(9)]),
                _ => format!("{} (not found)", snapshot.target),
            })
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

/// The identity of a declared target: `main`, `refs/heads/main`,
/// `origin/main` and `refs/remotes/origin/main` are one target; a target on
/// another remote (`refs/remotes/upstream/main`) keeps its remote.
fn canonical_target(declared: &str) -> String {
    declared
        .strip_prefix("refs/heads/")
        .or_else(|| declared.strip_prefix("refs/remotes/origin/"))
        .or_else(|| declared.strip_prefix("origin/"))
        .unwrap_or(declared)
        .to_string()
}

/// The remote a canonical target is read from, when it names one other than
/// `origin` (`refs/remotes/<remote>/<branch>`).
fn target_remote(canonical: &str) -> Option<&str> {
    canonical
        .strip_prefix("refs/remotes/")
        .and_then(|rest| rest.split_once('/'))
        .map(|(remote, _)| remote)
}

/// Resolve a canonical target to the ref the new-claim context reads. An
/// explicit ref is read exactly, never through another remote; a plain name
/// reads the fetched `origin` copy first and the local branch otherwise.
fn claim_reference(repo: &Repository, canonical: &str) -> Option<(String, git2::Oid)> {
    let candidates = if canonical.starts_with("refs/") {
        vec![canonical.to_string()]
    } else {
        vec![
            format!("refs/remotes/origin/{canonical}"),
            format!("refs/heads/{canonical}"),
        ]
    };
    candidates.into_iter().find_map(|name| {
        let oid = repo.find_reference(&name).ok()?.peel_to_commit().ok()?.id();
        Some((
            name.trim_start_matches("refs/remotes/")
                .trim_start_matches("refs/heads/")
                .to_string(),
            oid,
        ))
    })
}

/// Every visible branch name (local, and `origin/` stripped of its remote)
/// carrying a task id on a sanctioned work prefix, with its tip.
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
        let name = match kind {
            BranchType::Local => name.to_string(),
            BranchType::Remote => match name.strip_prefix("origin/") {
                Some(rest) if rest != "HEAD" => rest.to_string(),
                _ => continue,
            },
        };
        let Some(oid) = branch.get().peel_to_commit().ok().map(|commit| commit.id()) else {
            continue;
        };
        let Some(suffix) = work_suffix(prefixes, &name) else {
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
    cherry_landed(repo_root, &target.to_string(), &tip.to_string())
}

/// Whether every commit `tip` adds over `target` is patch-equivalent there.
/// `git cherry` skips merge commits, so a range holding a merge (whose
/// resolution may carry its own change) is never proven this way.
pub(crate) fn cherry_landed(repo_root: &Path, target: &str, tip: &str) -> bool {
    if target.starts_with('-') || tip.starts_with('-') {
        return false;
    }
    let range = format!("{target}..{tip}");
    let merges = git(
        repo_root,
        &["rev-list", "--merges", "--max-count=1", &range],
    );
    if !merges.is_ok_and(|out| out.trim().is_empty()) {
        return false;
    }
    git(repo_root, &["cherry", target, tip])
        .is_ok_and(|out| !out.trim().is_empty() && out.lines().all(|line| line.starts_with("- ")))
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

/// The declared targets of the task records in the working tree, and the
/// default target for records that declare none.
fn declared_targets(repo_root: &Path) -> BTreeSet<String> {
    let mut targets: BTreeSet<String> =
        crate::workgraph::layout::task_record_files(&repo_root.join("project-management"))
            .into_iter()
            .filter_map(|path| {
                let stem = path.file_stem()?.to_str()?.to_string();
                super::declared_work_target(repo_root, &stem)
            })
            .map(|target| canonical_target(&target))
            .collect();
    if let Some(default) = super::default_work_target(repo_root) {
        targets.insert(canonical_target(&default));
    }
    targets
}

/// Compute the backlog from the last-fetched refs. Makes no network call.
///
/// # Errors
///
/// Returns the reason when the repository or a target tip cannot be read.
pub fn backlog(repo_root: &Path) -> Result<Backlog, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let prefixes = work_prefixes(repo_root);
    let default = super::default_work_target(repo_root).map(|target| canonical_target(&target));
    let mut out = Backlog {
        fetched_at: fetched_at(&repo),
        ..Backlog::default()
    };
    let mut tips: Vec<(String, git2::Oid, BTreeMap<String, Record>)> = Vec::new();
    for target in declared_targets(repo_root) {
        match claim_reference(&repo, &target) {
            Some((reference, oid)) => {
                let tree = repo
                    .find_commit(oid)
                    .and_then(|commit| commit.tree())
                    .map_err(|error| error.to_string())?;
                let records = records_from_tree(&repo, &tree).map_err(|error| error.to_string())?;
                out.snapshots.push(Snapshot {
                    target: target.clone(),
                    reference: Some(reference),
                    tip: Some(oid.to_string()),
                });
                tips.push((target, oid, records));
            }
            None => out.snapshots.push(Snapshot {
                target,
                reference: None,
                tip: None,
            }),
        }
    }

    let ids: BTreeSet<String> = tips
        .iter()
        .flat_map(|(_, _, records)| records.keys().cloned())
        .collect();
    let carried = visible_work_branches(&repo, &prefixes, &ids);
    let mut judged = BTreeSet::new();
    for (target, tip, records) in &tips {
        for record in records
            .values()
            .filter(|record| record.kind == RecordKind::Task)
        {
            let declared = record
                .integration_target
                .as_deref()
                .map(canonical_target)
                .or_else(|| default.clone());
            if declared.as_deref() != Some(target.as_str()) || !judged.insert(record.id.clone()) {
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
            let (state, reason) = match validate_anchored_task(&repo, records, &record.id, target) {
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
                target: target.clone(),
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

/// Claim `task_id` (R-23): fetch `origin` when it exists, judge the task in
/// the new-claim context on the fetched target tip, refuse when a visible
/// branch already carries its id, then create `task/<id>-<slug>` from that
/// tip and push it. The pushed branch is an advisory mark that others see;
/// nothing depends on it being exclusive.
///
/// # Errors
///
/// Returns the reason the task cannot be claimed or git refused a step.
pub fn claim(repo_root: &Path, task_id: &str) -> Result<Claim, String> {
    let declared = super::declared_work_target(repo_root, task_id)
        .ok_or_else(|| format!("{task_id} has no visible record with an integration_target"))?;
    let target = canonical_target(&declared);
    let remotes: Vec<String> = git(repo_root, &["remote"])?
        .lines()
        .map(str::to_string)
        .collect();
    let has_origin = remotes.iter().any(|remote| remote == "origin");
    // Fetch the remote the target names (never substituting origin for it)
    // and origin, where claims are published.
    let mut fetch: Vec<&str> = target_remote(&target).into_iter().collect();
    if has_origin && !fetch.contains(&"origin") {
        fetch.push("origin");
    }
    for remote in fetch {
        if !remotes.iter().any(|known| known == remote) {
            return Err(format!(
                "target '{target}' names remote '{remote}', which is not configured"
            ));
        }
        git(repo_root, &["fetch", "--prune", "--quiet", remote])
            .map_err(|error| format!("fetch {remote} failed: {error}"))?;
    }
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let (reference, tip) = claim_reference(&repo, &target)
        .ok_or_else(|| format!("target '{target}' does not resolve here"))?;
    let tree = repo
        .find_commit(tip)
        .and_then(|commit| commit.tree())
        .map_err(|error| error.to_string())?;
    let records = records_from_tree(&repo, &tree).map_err(|error| error.to_string())?;
    validate_anchored_task(&repo, &records, task_id, &target)
        .map_err(|error| format!("not ready on {reference}: {error}"))?;
    let ids = BTreeSet::from([task_id.to_string()]);
    let prefixes = work_prefixes(repo_root);
    let mut carried = visible_work_branches(&repo, &prefixes, &ids)
        .remove(task_id)
        .unwrap_or_default();
    if has_origin {
        carried.extend(remote_claims(&repo, repo_root, &prefixes, task_id)?);
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
        from: format!("{reference}@{}", &tip.to_string()[..9]),
        pushed: has_origin,
    })
}

/// Branches on `origin` carrying `task_id`, read from the remote itself so
/// a narrow fetch refspec cannot hide a claim. A tip this clone does not
/// have is kept as an open claim (its landing cannot be proven here).
fn remote_claims(
    repo: &Repository,
    repo_root: &Path,
    prefixes: &[String],
    task_id: &str,
) -> Result<Vec<(String, git2::Oid)>, String> {
    let listed = git(repo_root, &["ls-remote", "--heads", "origin"])
        .map_err(|error| format!("cannot list origin's branches: {error}"))?;
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
            // An unknown tip is never landed: keep it open with a null id.
            Some((
                name.to_string(),
                if known { oid } else { git2::Oid::ZERO_SHA1 },
            ))
        })
        .collect())
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
        .and_then(|target| claim_reference(&repo, &canonical_target(&target)))
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
                format!("[{{id: TSK-001, kind: research, pin: {pin}}}]")
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
        assert!(claimed.from.starts_with("origin/main@"));
        let remote = run(bare.path(), &["branch", "--list"]);
        assert!(remote.contains("task/TSK-002-work-tsk-002"), "{remote}");

        let second = claim(root, "TSK-002").unwrap_err();
        assert!(second.contains("already claimed"), "{second}");
        assert!(!second.to_lowercase().contains("lock"), "{second}");
        let waiting = claim(root, "TSK-003").unwrap_err();
        assert!(waiting.contains("not ready on origin/main"), "{waiting}");
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

    /// T103-1: a tag or branch named like a pin cannot redirect it.
    #[test]
    fn a_pin_is_an_object_id_never_a_ref_name() {
        let dir = repo();
        let root = dir.path();
        task(root, "TSK-001", "todo", "[]", "");
        let open = commit(root, "plan");
        task(root, "TSK-001", "complete", "[]", "");
        let done = commit(root, "findings");
        let pin = &open[..8];
        task(
            root,
            "TSK-002",
            "todo",
            &format!("[{{id: TSK-001, kind: research, pin: {pin}}}]"),
            "",
        );
        commit(root, "consumer");
        run(root, &["tag", pin, &done]);
        run(root, &["branch", pin, &done]);
        let error = verdict(root, "TSK-002").unwrap_err();
        assert!(error.to_string().contains("'todo'"), "{error}");
        assert_eq!(
            entry(&backlog(root).unwrap(), "TSK-002").state,
            State::Waiting
        );
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
}
