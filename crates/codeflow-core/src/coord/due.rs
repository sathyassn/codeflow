//! Due times and target-aware fulfilment.
//!
//! A follow-up is due when its next check has passed, when a prerequisite
//! changed state since it was last observed (a task claimed, completed,
//! blocked or cancelled; a pull request merged, closed or its checks
//! finished; a lane reporting, going idle or stale; another follow-up
//! closing; a time passing), or when a task it links to was cancelled.
//! Due never closes anything.
//!
//! Only a `fulfils` link closes a follow-up, and only when its target is
//! done on the target branch: a task `complete` in the records on its
//! declared landing line's tip (read by [`crate::workgraph::readiness::target_tasks`],
//! the same resolution `work start` uses), a pull request merged, a commit
//! contained in the target, a branch shown deleted by its remote. A
//! completion on a feature branch never counts, and nothing unknown counts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::observe::Cache;
use super::reference::{Link, LinkKind, Target};
use super::schema::{Evidence, EvidenceKind, Observation};
use super::store::Liveness;
use super::transition::{Item, ItemState, Ledger};

/// A task as its target holds it.
pub type TaskView = crate::workgraph::readiness::TargetTask;

/// A pull request as its host reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullView {
    /// `open`, `merged` or `closed`.
    pub state: String,
    /// `pending`, `passed`, `failed` or `none`.
    pub checks: String,
    /// The branch it merges into.
    pub base: String,
    pub merge_commit: Option<String>,
}

/// What due derivation reads from outside the ledger.
pub trait Observer {
    /// The task on its target tip.
    fn task(&mut self, id: &str) -> Option<TaskView>;
    /// The pull request; `None` when it cannot be observed (offline).
    fn pull_request(&mut self, repository: &str, number: u64) -> Option<PullView>;
    /// Whether the remote shows the branch deleted; `None` when unknown.
    fn branch_deleted(&mut self, remote: &str, name: &str) -> Option<bool>;
    /// Whether the commit is in the default work target, and the target
    /// with its tip; `None` when unknown.
    fn commit_on_target(&mut self, sha: &str) -> Option<(bool, String, String)>;
    /// The current time, seconds since the epoch.
    fn now(&self) -> u64;
    /// This clone's repository identity, when known; a link to another
    /// repository is then never judged against this one.
    fn repository(&self) -> Option<String> {
        None
    }
    /// Whether a pull request's base (a bare branch name) is a landing line
    /// this project recognises.
    fn landing_base(&mut self, base: &str) -> bool {
        matches!(base, "main" | "master")
    }
}

/// The observer over a checkout: records on target tips, git, and, when
/// `online`, `gh` and `git ls-remote`. Every observation goes through the
/// bounded interface in [`super::observe`], each kind within its own limit,
/// and is kept for the freshness window; a timeout or a failed read is
/// unknown.
pub struct RepoObserver {
    root: PathBuf,
    online: bool,
    now: u64,
    limits: super::observe::Limits,
    repository: Option<String>,
    tasks: Cache<(), Option<BTreeMap<String, TaskView>>>,
    pulls: Cache<(String, u64), Option<PullView>>,
    branches: Cache<(String, String), Option<bool>>,
    commits: Cache<String, Option<(bool, String, String)>>,
    lines: Cache<(), Option<std::collections::BTreeSet<String>>>,
}

impl RepoObserver {
    /// An observer over the checkout at `root` with the default limits;
    /// network observations only when `online`.
    #[must_use]
    pub fn new(root: &Path, online: bool) -> Self {
        Self::with_limits(root, online, super::observe::Limits::default())
    }

    /// An observer with its own limits and freshness window. The clone's
    /// repository identity is read here, within the Git limit; unknown when
    /// it does not finish.
    #[must_use]
    pub fn with_limits(root: &Path, online: bool, limits: super::observe::Limits) -> Self {
        let identity_root = root.to_path_buf();
        Self {
            root: root.to_path_buf(),
            online,
            now: super::now_secs(),
            limits,
            repository: super::observe::within(limits.git, move || {
                super::reference::repository_identity(&identity_root)
            }),
            tasks: Cache::new(limits.freshness),
            pulls: Cache::new(limits.freshness),
            branches: Cache::new(limits.freshness),
            commits: Cache::new(limits.freshness),
            lines: Cache::new(limits.freshness),
        }
    }
}

impl Observer for RepoObserver {
    fn task(&mut self, id: &str) -> Option<TaskView> {
        let root = self.root.clone();
        let limit = self.limits.records;
        // Every task's history, acceptance and binding are read at once,
        // within one records bound: a stalled Git read makes every task
        // unknown, never a late answer.
        self.tasks
            .get((), || {
                super::observe::within(limit, move || {
                    crate::workgraph::readiness::target_tasks(&root).ok()
                })
                .flatten()
            })
            .and_then(|tasks| tasks.get(id).cloned())
    }

    fn pull_request(&mut self, repository: &str, number: u64) -> Option<PullView> {
        if !self.online {
            return None;
        }
        let root = self.root.clone();
        let limit = self.limits.network;
        self.pulls.get((repository.to_string(), number), || {
            gh_pull(&root, repository, number, limit)
        })
    }

    fn branch_deleted(&mut self, remote: &str, name: &str) -> Option<bool> {
        if !self.online {
            return None;
        }
        let root = self.root.clone();
        let limits = self.limits;
        self.branches
            .get((remote.to_string(), name.to_string()), || {
                // Only a remote this clone configures is contacted: a name
                // replayed from the log is checked again here, and Git
                // resolves its URL from the clone's own configuration.
                let check = root.clone();
                let wanted = remote.to_string();
                let configured = super::observe::within(limits.git, move || {
                    git2::Repository::discover(&check)
                        .is_ok_and(|repo| repo.find_remote(&wanted).is_ok())
                })?;
                if !configured {
                    return None;
                }
                let mut command = crate::git::command();
                command
                    .current_dir(&root)
                    .env("GIT_TERMINAL_PROMPT", "0")
                    // The stock ssh client: no configured or inherited
                    // command runs in its place.
                    .env_remove("GIT_SSH")
                    .env_remove("GIT_SSH_COMMAND")
                    // Only https and ssh transports, and the stock upload
                    // pack: a remote's configured command is never run.
                    .args([
                        "-c",
                        "protocol.allow=never",
                        "-c",
                        "protocol.https.allow=always",
                        "-c",
                        "protocol.ssh.allow=always",
                        "-c",
                        "core.sshCommand=ssh",
                        "ls-remote",
                        "--upload-pack=git-upload-pack",
                        "--heads",
                        "--",
                        remote,
                    ])
                    .arg(format!("refs/heads/{name}"));
                let output = super::observe::command(command, limits.network)?;
                output
                    .status
                    .success()
                    .then(|| output.stdout.iter().all(u8::is_ascii_whitespace))
            })
    }

    fn commit_on_target(&mut self, sha: &str) -> Option<(bool, String, String)> {
        let root = self.root.clone();
        let limit = self.limits.git;
        self.commits.get(sha.to_string(), || {
            let sha = sha.to_string();
            super::observe::within(limit, move || commit_on_target(&root, &sha)).flatten()
        })
    }

    fn now(&self) -> u64 {
        self.now
    }

    fn repository(&self) -> Option<String> {
        self.repository.clone()
    }

    fn landing_base(&mut self, base: &str) -> bool {
        let root = self.root.clone();
        let limit = self.limits.git;
        let lines = self.lines.get((), || {
            super::observe::within(limit, move || {
                crate::workgraph::readiness::landing_lines(&root).ok()
            })
            .flatten()
        });
        // Unknown landing lines leave only the default lines recognised.
        crate::workgraph::readiness::is_recognised_landing(base, &lines.unwrap_or_default())
    }
}

/// Whether `sha` is in the default work target, with the target and its tip.
fn commit_on_target(root: &Path, sha: &str) -> Option<(bool, String, String)> {
    let repo = git2::Repository::discover(root).ok()?;
    let target = crate::workgraph::default_work_target(root).ok()??;
    let resolved = crate::workgraph::resolve_work_target_checked(root, Some(&target)).ok()??;
    let tip = crate::workgraph::work_start::target_reference(&repo, &resolved.target)
        .ok()??
        .id();
    // The guarded ancestry reader: no replace refs, grafts or commit graph.
    let contained = crate::git::commit_reachable_from(root, sha, &[tip.to_string()]).ok()?;
    Some((contained, target, tip.to_string()))
}

/// How long one network observation may take before it counts as unknown.
pub const OBSERVATION_TIMEOUT: std::time::Duration = super::observe::NETWORK_TIMEOUT;

fn gh_pull(
    root: &Path,
    repository: &str,
    number: u64,
    limit: std::time::Duration,
) -> Option<PullView> {
    let mut command = crate::git::process("gh");
    command
        .current_dir(root)
        .env("GH_PROMPT_DISABLED", "1")
        .args([
            "pr",
            "view",
            &number.to_string(),
            "--repo",
            repository,
            "--json",
            "state,baseRefName,mergeCommit,statusCheckRollup",
        ]);
    let output = super::observe::command(command, limit)?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let state = value.get("state")?.as_str()?.to_ascii_lowercase();
    let checks = value
        .get("statusCheckRollup")
        .and_then(serde_json::Value::as_array)
        .map_or("none".to_string(), |checks| {
            let conclusions: Vec<String> = checks
                .iter()
                .map(|check| {
                    check
                        .get("conclusion")
                        .or_else(|| check.get("state"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase()
                })
                .collect();
            if conclusions.is_empty() {
                "none".into()
            } else if conclusions.iter().any(|c| c.is_empty() || c == "pending") {
                "pending".into()
            } else if conclusions
                .iter()
                .all(|c| matches!(c.as_str(), "success" | "skipped" | "neutral"))
            {
                "passed".into()
            } else {
                "failed".into()
            }
        });
    Some(PullView {
        state,
        checks,
        base: value.get("baseRefName")?.as_str()?.to_string(),
        merge_commit: value
            .get("mergeCommit")
            .and_then(|commit| commit.get("oid"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

/// Observe one link's current state.
pub fn observe(
    link: &Link,
    ledger: &Ledger,
    liveness: &[Liveness],
    observer: &mut dyn Observer,
) -> Observation {
    let (state, version) = match &link.target {
        Target::Record { id, uid, .. } => {
            if super::reference::is_task_id(id) {
                match observer.task(id) {
                    Some(view) if uid.is_some() && view.uid != *uid => {
                        ("uid-changed".to_string(), Some(view.tip))
                    }
                    Some(view) => {
                        let state = if view.status == "todo" && view.claimed {
                            "claimed".to_string()
                        } else {
                            view.status
                        };
                        (state, Some(view.tip))
                    }
                    None => ("unknown".into(), None),
                }
            } else {
                ("unobserved".into(), None)
            }
        }
        Target::PullRequest { repository, number } => {
            match observer.pull_request(repository, *number) {
                Some(view) if view.state == "open" => {
                    (format!("open, checks {}", view.checks), None)
                }
                Some(view) => (view.state, view.merge_commit),
                None => ("unknown".into(), None),
            }
        }
        Target::Commit { sha, .. } => match observer.commit_on_target(sha) {
            Some((true, _, tip)) => ("on-target".into(), Some(tip)),
            Some((false, _, tip)) => ("not-on-target".into(), Some(tip)),
            None => ("unknown".into(), None),
        },
        Target::Branch { name, remote, .. } => match observer.branch_deleted(remote, name) {
            Some(true) => ("deleted".into(), None),
            Some(false) => ("present".into(), None),
            None => ("unknown".into(), None),
        },
        Target::FollowUp { id } => match ledger.items.get(id) {
            Some(item) => (
                item.state.as_str().to_string(),
                Some(item.revision.to_string()),
            ),
            None => ("missing".into(), None),
        },
        Target::Lane { id } => lane_state(ledger, liveness, id, observer.now()),
        Target::Time { at } => {
            let passed = super::parse_time(at).is_ok_and(|at| at <= observer.now());
            (if passed { "passed" } else { "pending" }.to_string(), None)
        }
    };
    Observation {
        link: link.display(),
        state,
        version,
    }
}

fn lane_state(
    ledger: &Ledger,
    liveness: &[Liveness],
    id: &str,
    now: u64,
) -> (String, Option<String>) {
    let Some(lane) = ledger.lanes.get(id) else {
        return ("missing".into(), None);
    };
    if lane.closed.is_some() {
        return ("closed".into(), Some(lane.last_sequence.to_string()));
    }
    let seen = lane.seat.as_ref().and_then(|seat| {
        liveness
            .iter()
            .filter(|live| &live.recorded_by.actor_instance == seat)
            .filter_map(|live| super::parse_time(&live.last_seen).ok())
            .max()
    });
    if seen.is_some_and(|seen| now.saturating_sub(seen) > super::HEARTBEAT_TIMEOUT_SECS) {
        return ("stale".into(), None);
    }
    match &lane.reported_at {
        Some(at) => ("reported".into(), Some(at.clone())),
        None => ("idle".into(), None),
    }
}

/// Observe every prerequisite (and each `waits_on` id) of an item.
pub fn observe_item(
    item: &Item,
    ledger: &Ledger,
    liveness: &[Liveness],
    observer: &mut dyn Observer,
) -> Vec<Observation> {
    let mut links: Vec<Link> = item
        .record
        .links
        .iter()
        .filter(|link| link.kind != LinkKind::Context)
        .cloned()
        .collect();
    for id in &item.record.waits_on {
        let target = if super::actor::is_lane_id(id) {
            Target::Lane { id: id.clone() }
        } else {
            Target::FollowUp { id: id.clone() }
        };
        links.push(Link {
            kind: LinkKind::Prerequisite,
            target,
        });
    }
    // A task link of any kind is observed, so a cancelled task makes the
    // follow-up due whatever the link's kind.
    for link in item.record.links.iter().filter(|link| {
        link.kind == LinkKind::Context
            && matches!(&link.target, Target::Record { id, .. } if super::reference::is_task_id(id))
    }) {
        links.push(link.clone());
    }
    links
        .iter()
        .map(|link| observe(link, ledger, liveness, observer))
        .collect()
}

/// Why an open item is due now, if it is.
#[must_use]
pub fn due_reasons(item: &Item, now: u64, observed: &[Observation]) -> Vec<String> {
    let mut reasons = Vec::new();
    if item.state != ItemState::Open {
        return reasons;
    }
    if super::parse_time(&item.record.next_check).is_ok_and(|next| next <= now) {
        reasons.push(format!("check due since {}", item.record.next_check));
    }
    let baseline = item.baseline();
    for current in observed {
        if current.state == "cancelled" {
            reasons.push(format!(
                "{} was cancelled: relink the follow-up or drop it",
                current.link
            ));
            continue;
        }
        if current.state == "uid-changed" {
            reasons.push(format!(
                "{} now names a different record: relink the follow-up",
                current.link
            ));
            continue;
        }
        let prerequisite = !current.link.starts_with("context:");
        if !prerequisite {
            continue;
        }
        let before = baseline.iter().find(|seen| seen.link == current.link);
        let changed = match before {
            Some(before) => before.state != current.state || before.version != current.version,
            None => false,
        };
        if changed && current.state != "unknown" {
            let before = before.map_or("unrecorded", |before| before.state.as_str());
            reasons.push(format!(
                "{} changed: {before} -> {}",
                current.link, current.state
            ));
        }
    }
    reasons
}

/// The machine-verified proof that closes an item: every `fulfils` link is
/// done on its target, and every prerequisite and `waits_on` id the item
/// declares beside it is met too, so a condition that asks for more than
/// the fulfilled work keeps the item open. `None` when the item has no
/// `fulfils` link, or anything is unmet or unknown.
pub fn fulfilment(item: &Item, ledger: &Ledger, observer: &mut dyn Observer) -> Option<Evidence> {
    let fulfils: Vec<&Link> = item
        .record
        .links
        .iter()
        .filter(|link| link.kind == LinkKind::Fulfils)
        .collect();
    if fulfils.is_empty() || !prerequisites_met(item, ledger, observer) {
        return None;
    }
    let mut proofs = Vec::new();
    for link in fulfils {
        match judge(link, observer) {
            Judged::Met(evidence) => proofs.push(*evidence),
            Judged::Unmet(_) | Judged::Unknown => return None,
        }
    }
    let first = proofs.first()?.clone();
    if proofs.len() == 1 {
        return Some(first);
    }
    Some(Evidence {
        reference: proofs
            .iter()
            .map(|proof| proof.reference.as_str())
            .collect::<Vec<_>>()
            .join("; "),
        ..first
    })
}

/// Whether every prerequisite link and `waits_on` id is met: a task
/// accepted complete on its target, a pull request merged into a landing
/// line, a commit in the target, a branch deleted, a follow-up closed or
/// dropped, a lane closed, a time passed. Context links do not count.
fn prerequisites_met(item: &Item, ledger: &Ledger, observer: &mut dyn Observer) -> bool {
    let local = |id: &str| -> bool {
        if super::actor::is_lane_id(id) {
            ledger
                .lanes
                .get(id)
                .is_some_and(|lane| lane.closed.is_some())
        } else {
            ledger
                .items
                .get(id)
                .is_some_and(|item| item.state != ItemState::Open)
        }
    };
    for link in &item.record.links {
        if link.kind != LinkKind::Prerequisite {
            continue;
        }
        let met = match &link.target {
            Target::FollowUp { id } | Target::Lane { id } => local(id),
            Target::Time { at } => super::parse_time(at).is_ok_and(|at| at <= observer.now()),
            _ => matches!(judge(link, observer), Judged::Met(_)),
        };
        if !met {
            return false;
        }
    }
    item.record.waits_on.iter().all(|id| local(id))
}

/// What one fulfils condition shows now.
enum Judged {
    /// Done on its target, with the proof.
    Met(Box<Evidence>),
    /// Definitely not done, and why.
    Unmet(String),
    /// Not observable now (offline, unreadable, not found).
    Unknown,
}

fn repository_matches(repository: &str, observer: &dyn Observer) -> bool {
    observer
        .repository()
        .is_none_or(|current| current == repository)
}

#[allow(clippy::too_many_lines)]
fn judge(link: &Link, observer: &mut dyn Observer) -> Judged {
    let now = super::format_time(observer.now());
    match &link.target {
        Target::Record {
            id,
            uid,
            repository,
        } => {
            if !super::reference::is_task_id(id) || !repository_matches(repository, observer) {
                return Judged::Unknown;
            }
            let Some(view) = observer.task(id) else {
                return Judged::Unknown;
            };
            // A pinned identity must be established on the target: a
            // missing or different UID there is another record.
            if uid.is_some() && view.uid != *uid {
                return Judged::Unmet(format!(
                    "{id} on {} is not the record the link names (its UID differs)",
                    view.reference
                ));
            }
            if view.status != "complete" {
                return Judged::Unmet(format!(
                    "{id} is {} on {} at {}",
                    view.status,
                    view.reference,
                    short(&view.tip)
                ));
            }
            if !view.accepted {
                return Judged::Unmet(format!(
                    "{id} is complete on {} without a passing acceptance block",
                    view.reference
                ));
            }
            Judged::Met(Box::new(Evidence {
                kind: EvidenceKind::MachineVerified,
                reference: format!(
                    "{id} complete and accepted on {} at {}",
                    view.reference,
                    short(&view.tip)
                ),
                source: format!("task record and acceptance block on {}", view.reference),
                repository: Some(repository.clone()),
                target: Some(view.target),
                revision: Some(view.tip),
                observed_at: now,
            }))
        }
        Target::PullRequest { repository, number } => {
            if !repository_matches(repository, observer) {
                return Judged::Unknown;
            }
            let Some(view) = observer.pull_request(repository, *number) else {
                return Judged::Unknown;
            };
            if view.state != "merged" {
                return Judged::Unmet(format!("PR#{number} is {}", view.state));
            }
            if !observer.landing_base(&view.base) {
                return Judged::Unmet(format!(
                    "PR#{number} merged into a work branch, not a landing line"
                ));
            }
            Judged::Met(Box::new(Evidence {
                kind: EvidenceKind::MachineVerified,
                reference: format!("https://{repository}/pull/{number}"),
                source: "gh pr view".into(),
                repository: Some(repository.clone()),
                target: Some(view.base),
                revision: Some(view.merge_commit.unwrap_or_else(|| "unknown".into())),
                observed_at: now,
            }))
        }
        Target::Commit { sha, repository } => {
            if !repository_matches(repository, observer) {
                return Judged::Unknown;
            }
            match observer.commit_on_target(sha) {
                Some((true, target, tip)) => Judged::Met(Box::new(Evidence {
                    kind: EvidenceKind::MachineVerified,
                    reference: format!("commit {sha} contained in {target}"),
                    source: "git history".into(),
                    repository: Some(repository.clone()),
                    target: Some(target),
                    revision: Some(tip),
                    observed_at: now,
                })),
                Some((false, target, _)) => {
                    Judged::Unmet(format!("commit {} is not in {target}", short(sha)))
                }
                None => Judged::Unknown,
            }
        }
        Target::Branch {
            name,
            remote,
            repository,
        } => {
            if !repository_matches(repository, observer) {
                return Judged::Unknown;
            }
            match observer.branch_deleted(remote, name) {
                Some(true) => Judged::Met(Box::new(Evidence {
                    kind: EvidenceKind::MachineVerified,
                    reference: format!("branch {name} absent from {remote}"),
                    source: "git ls-remote".into(),
                    repository: Some(repository.clone()),
                    target: Some(remote.clone()),
                    revision: Some("absent".into()),
                    observed_at: now,
                })),
                Some(false) => Judged::Unmet(format!("branch {name} is present on {remote}")),
                None => Judged::Unknown,
            }
        }
        Target::FollowUp { .. } | Target::Lane { .. } | Target::Time { .. } => Judged::Unknown,
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

/// Why a machine-verified closure no longer holds: any part of the
/// condition that authorized it is now definitely unmet. That is a `fulfils`
/// condition (a task reverted, replaced or no longer accepted, a pull
/// request reopened, a commit gone from the target, a branch recreated), a
/// prerequisite link, or a `waits_on` follow-up or lane reopened. An
/// observation that cannot be made is not invalidation.
pub fn reverted(item: &Item, ledger: &Ledger, observer: &mut dyn Observer) -> Option<String> {
    let ItemState::Closed(evidence) = &item.state else {
        return None;
    };
    if evidence.kind != EvidenceKind::MachineVerified {
        return None;
    }
    let local_open = |id: &str| -> Option<String> {
        if super::actor::is_lane_id(id) {
            ledger
                .lanes
                .get(id)
                .filter(|lane| lane.closed.is_none())
                .map(|_| format!("{id} is open again"))
        } else {
            ledger
                .items
                .get(id)
                .filter(|other| other.state == ItemState::Open)
                .map(|_| format!("{id} is open again"))
        }
    };
    for link in &item.record.links {
        let unmet = match (&link.kind, &link.target) {
            (LinkKind::Context, _) | (_, Target::Time { .. }) => None,
            (_, Target::FollowUp { id } | Target::Lane { id }) => local_open(id),
            _ => match judge(link, observer) {
                Judged::Unmet(reason) => Some(reason),
                Judged::Met(_) | Judged::Unknown => None,
            },
        };
        if let Some(reason) = unmet {
            return Some(format!("the proof no longer holds: {reason}"));
        }
    }
    item.record
        .waits_on
        .iter()
        .find_map(|id| local_open(id))
        .map(|reason| format!("the proof no longer holds: {reason}"))
}

/// The schedule of a check: the requested time, capped for an item that
/// waits on a prerequisite so the wait is rechecked at least daily.
#[must_use]
pub fn capped_next_check(item: &Item, requested: u64, now: u64) -> u64 {
    let waits = !item.record.waits_on.is_empty()
        || item
            .record
            .links
            .iter()
            .any(|link| link.kind == LinkKind::Prerequisite);
    if waits {
        requested.min(now + super::MAX_RECHECK_SECS)
    } else {
        requested
    }
}
