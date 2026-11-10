//! Read-only preflight for starting durable task implementation.
//!
//! Git history is the planning seal. The preflight inspects the merge-base
//! between the current task branch and its intended integration target and
//! requires the task, parent epic, applicable specs, and completed
//! dependencies to exist in that anchored snapshot. It never creates or
//! updates files, refs, branches, worktrees, or record status.

use crate::workgraph::history;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use git2::{Oid, Repository};
use thiserror::Error;

use crate::git::GitName;
use crate::workgraph::deps::{parse_dependencies, Dependency, DependencyKind};
use crate::workgraph::{
    is_canonical_task_format_id, is_valid_epic_format_id, is_valid_spec_format_id,
    is_valid_task_format_id,
};

/// Shared wording for a tracking state that cannot be determined.
pub const TRACKING_STATE_MESSAGE: &str = "cannot determine durable-work tracking";

/// Maximum provider wait for the review evidence of one predecessor pin.
pub const REVIEW_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Render the tracking-state refusal consistently across commands.
#[must_use]
pub fn tracking_state_message(error: impl std::fmt::Display) -> String {
    format!("{TRACKING_STATE_MESSAGE}: {error}")
}

/// Maximum task-home entries inspected by the activation probe. An exhausted
/// inventory is an error, never evidence that durable work is absent.
const MAX_ACTIVATION_ENTRIES: usize = 16_384;

#[derive(Debug, Error)]
pub enum DurableTrackingError {
    #[error("cannot inspect CodeFlow state at {}: {source}", path.display())]
    StateMetadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("CodeFlow state path {} is not a regular path (or is a symlink)", .0.display())]
    StatePath(PathBuf),
    #[error("cannot read existing CodeFlow state; repair .codeflow/project.toml")]
    StateRead(#[from] crate::scaffold::ScaffoldError),
    #[error("unsupported CodeFlow state schema version {0}")]
    UnsupportedStateVersion(u32),
    #[error("CodeFlow task-home inventory exceeds {MAX_ACTIVATION_ENTRIES} entries")]
    TaskInventoryLimit,
    #[error("cannot inspect CodeFlow task-home inventory")]
    TaskInventoryUnreadable,
}

#[derive(Debug, Error)]
pub enum WorkStartError {
    #[error("{0}")]
    Base(String),
    #[error("git repository unavailable: {0}")]
    Repository(String),
    /// A ref that exists and names no commit this clone can read, with its
    /// cause, what it points at and a remedy (TSK-416 AC-9).
    #[error("{0}")]
    UnresolvableTip(String),
    #[error("target ref '{0}' does not resolve to a commit")]
    Target(String),
    #[error("current branch '{branch}' does not carry {task_id} on a work prefix (<prefix>/{task_id}-<slug>)")]
    Branch { branch: String, task_id: String },
    #[error("no merge-base exists between HEAD and '{0}'")]
    MergeBase(String),
    #[error("anchored workgraph is invalid: {0}")]
    InvalidGraph(String),
    #[error("task {0} is not present at the merge-base; land its epic planning record, or carry its own standalone record on this task branch")]
    TaskNotAnchored(String),
    #[error("task {task_id} references missing epic {epic_id} at the merge-base")]
    MissingEpic { task_id: String, epic_id: String },
    #[error("task {0} is standalone but has no standalone_reason")]
    MissingStandaloneReason(String),
    #[error("task {task_id} depends on missing task {dependency} at the merge-base")]
    MissingDependency { task_id: String, dependency: String },
    #[error(
        "task {task_id} depends on {dependency}, whose anchored status is '{status}', not complete"
    )]
    DependencyIncomplete {
        task_id: String,
        dependency: String,
        status: String,
    },
    #[error("task {task_id} requires missing spec {spec_id} at the merge-base")]
    MissingSpec { task_id: String, spec_id: String },
    #[error("task {task_id} requires spec {spec_id}, whose anchored status is '{status}', not approved or implemented")]
    SpecNotReady {
        task_id: String,
        spec_id: String,
        status: String,
    },
    #[error("task {task_id} is blocked: {reason}")]
    Blocked { task_id: String, reason: String },
    #[error("task {task_id} is awaiting selection ({path}); a planning PR lands the selection")]
    AwaitingSelection { task_id: String, path: String },
    #[error("task {task_id} belongs to epic {epic_id}, which is {status}")]
    EpicClosed {
        task_id: String,
        epic_id: String,
        status: String,
    },
    #[error("task {task_id} depends on {dependency}, complete on '{line}' but not in this base; merge it or land a reviewed port")]
    DependencyOnOtherLine {
        task_id: String,
        dependency: String,
        line: String,
    },
    #[error("task {task_id} depends on {dependency} on '{line}', which is not fetched here: unknown, not satisfied")]
    DependencyLineUnknown {
        task_id: String,
        dependency: String,
        line: String,
    },
    #[error("task {task_id} has a {kind} dependency on {dependency} with no pin; the planner writes the pin, quoted: pin: \"<commit sha>\"")]
    DependencyUnpinned {
        task_id: String,
        dependency: String,
        kind: String,
    },
    #[error("task {task_id} pins {dependency} at {pin}: {reason}")]
    DependencyPin {
        task_id: String,
        dependency: String,
        pin: String,
        reason: String,
    },
    #[error("task {task_id} cannot start from status '{status}'; expected todo or in_progress")]
    TaskNotStartable { task_id: String, status: String },
    #[error("task {0} has no integration_target")]
    MissingIntegrationTarget(String),
    #[error("task {task_id} targets '{declared}', not requested target '{requested}'")]
    TargetMismatch {
        task_id: String,
        declared: String,
        requested: String,
    },
    #[error("target '{0}' is not a stable local or remote-tracking non-task branch")]
    UnstableTarget(String),
    #[error(
        "local branch '{local}' and '{remote}' have diverged ('{local}' at {local_tip} \
         with {ahead} commit(s) only there, '{remote}' at {remote_tip} with {behind} only \
         there), so the work target is ambiguous; judge against the fetched line by naming \
         it with `--into {}` where the command takes it, or {}; a plain `git fetch origin` \
         refreshes '{remote}' without moving '{local}'",
        crate::git::shell_arg(.remote),
        fresh_clone_retry(remote_of(.remote))
    )]
    DivergedTarget {
        local: String,
        remote: String,
        local_tip: String,
        remote_tip: String,
        ahead: usize,
        behind: usize,
    },
}

/// A work target resolved to a ref. A local branch strictly behind its
/// upstream is passed over for the upstream without a note: `work start`
/// reports the anchored ref, the stale branch needs no fast-forward, and the
/// guards refuse the ones an agent could run (sathyassn/codeflow#27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkTarget {
    /// The target to anchor on (`main`, `origin/main` or a full ref).
    pub target: String,
}

/// How a refusal of the readiness core reads in a derived view (R-27).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotReady {
    /// Complete or cancelled: no context starts it.
    Closed,
    /// A recorded Blocker, an awaiting selection or a closed epic.
    Blocked,
    /// `todo` with an unmet dependency or a spec not yet approved.
    Waiting,
    /// The records do not hold together at the base.
    Invalid,
}

impl WorkStartError {
    /// The catalogued step that clears this refusal of a task's admission:
    /// a task blocked on the target is unblocked in its own pull request
    /// (TSK-1036); any other refusal reconciles the planning anchor.
    #[must_use]
    pub fn admission_remedy(&self, target: &str, task_id: &str) -> crate::remedy::Remedy {
        match self {
            Self::Blocked { .. } => crate::remedy::WORK_START_UNBLOCK.with(&[("id", task_id)]),
            _ => crate::remedy::WORK_START_MERGE_PLANNING
                .with(&[("target", target), ("id", task_id)]),
        }
    }

    /// The derived state this refusal puts a task in.
    #[must_use]
    pub fn not_ready(&self) -> NotReady {
        match self {
            Self::TaskNotStartable { status, .. }
                if matches!(status.as_str(), "complete" | "cancelled") =>
            {
                NotReady::Closed
            }
            Self::Blocked { .. } | Self::AwaitingSelection { .. } | Self::EpicClosed { .. } => {
                NotReady::Blocked
            }
            Self::DependencyIncomplete { .. }
            | Self::DependencyOnOtherLine { .. }
            | Self::DependencyLineUnknown { .. }
            | Self::DependencyUnpinned { .. }
            | Self::DependencyPin { .. }
            | Self::SpecNotReady { .. } => NotReady::Waiting,
            _ => NotReady::Invalid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkStartReport {
    pub base_note: Option<String>,
    pub task_id: String,
    pub branch: String,
    pub target: String,
    pub merge_base: String,
    pub epic_id: Option<String>,
    pub specs: Vec<String>,
    pub dependencies: Vec<String>,
    /// The anchored task's `work_type` (`spike` selects the spike path rule).
    pub work_type: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Record {
    pub(crate) id: String,
    /// The hidden record identity, when the record carries one.
    pub(crate) uid: Option<String>,
    /// For a complete task whose active acceptance block passes the
    /// structural acceptance rules against its own criteria, the block's
    /// `reviewed` commit; its binding is judged separately.
    pub(crate) reviewed: Option<String>,
    /// The text of the record's one active acceptance block, valid or not.
    pub(crate) acceptance: Option<String>,
    pub(crate) kind: RecordKind,
    pub(crate) title: String,
    pub(crate) created: Option<String>,
    pub(crate) status: String,
    pub(crate) epic_id: Option<String>,
    standalone_reason: Option<String>,
    pub(crate) integration_target: Option<String>,
    specs: Vec<String>,
    pub(crate) depends_on: Vec<Dependency>,
    work_type: Option<String>,
    pub(crate) awaiting_selection: Option<String>,
    blocker_reason: Option<String>,
    pub(crate) line_adoptions: Vec<super::line_adoption::LineAdoption>,
    /// The epic line an epic declares its tasks land on (TSK-1286). It is read
    /// on every kind so a binding can refuse a task record that carries it.
    pub(crate) parent_line: Option<String>,
}

/// The three durable record kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Epic,
    Spec,
    Task,
}

#[derive(Debug)]
pub(crate) struct AnchoredTask {
    epic_id: Option<String>,
    specs: Vec<String>,
    dependencies: Vec<String>,
    work_type: Option<String>,
}

/// Pick the configured target convention without mutating repository state.
///
/// `origin/main` wins when present, then `origin/master`, `main`, and
/// `master`.
///
/// # Errors
///
/// Returns an error if an existing conventional target or its repository cannot be read.
pub fn default_work_target(repo_root: &Path) -> Result<Option<String>, WorkStartError> {
    Ok(default_target(repo_root)?.map(|(target, _)| target.to_string()))
}

/// The full ref of the target [`default_work_target`] picks, such as
/// `refs/remotes/origin/main`. Read the default target's commit through this
/// ref, never through the short name: Git resolves a short `origin/main` to a
/// local branch or tag of that name before the remote-tracking ref.
///
/// # Errors
///
/// Returns an error if an existing conventional target or its repository cannot be read.
pub fn default_work_target_ref(repo_root: &Path) -> Result<Option<String>, WorkStartError> {
    Ok(default_target(repo_root)?.map(|(_, reference)| reference))
}

fn default_target(repo_root: &Path) -> Result<Option<(&'static str, String)>, WorkStartError> {
    let Some(repo) = crate::hooks::repo::open(repo_root).map_err(WorkStartError::Repository)?
    else {
        return Ok(None);
    };
    for target in ["origin/main", "origin/master", "main", "master"] {
        let reference = if target.starts_with("origin/") {
            format!("refs/remotes/{target}")
        } else {
            format!("refs/heads/{target}")
        };
        if reference_commit(&repo, &reference)?.is_some() {
            return Ok(Some((target, reference)));
        }
    }
    Ok(None)
}

/// The commit the ref `name` names, `None` when no such ref exists. A ref
/// that exists and names no readable commit refuses with
/// [`unresolvable_tip`].
pub(crate) fn reference_commit(
    repo: &Repository,
    name: &str,
) -> Result<Option<git2::Oid>, WorkStartError> {
    let reference = match repo.find_reference(name) {
        Ok(reference) => reference,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(None),
        Err(error) => {
            return Err(WorkStartError::UnresolvableTip(unresolvable_tip(
                repo, name, &error,
            )))
        }
    };
    reference
        .peel_to_commit()
        .map(|commit| Some(commit.id()))
        .map_err(|error| WorkStartError::UnresolvableTip(unresolvable_tip(repo, name, &error)))
}

/// The unresolvable-tip refusal for the ref `reference` of the repository
/// at `repo_root`, when it exists and names no readable commit; `None` when it
/// resolves, is absent, or the repository cannot be opened.
#[must_use]
pub fn unresolvable_reference(repo_root: &Path, reference: &str) -> Option<String> {
    let repo = Repository::discover(repo_root).ok()?;
    let found = repo.find_reference(reference).ok()?;
    let error = found.peel_to_commit().err()?;
    Some(unresolvable_tip(&repo, reference, &error))
}

/// Why the ref `reference` names no commit this clone can read (TSK-416
/// AC-9): the cause, the ref and what it points at, and a remedy that runs
/// under the guard, never `update-ref` or a fetch with a destination.
/// - A fetched ref whose object is here but is not a commit is rewritten
///   by a plain fetch of its remote.
/// - A fetched ref whose object is missing stops every fetch (git refuses
///   the bad object while it negotiates), so the remedy is a fresh clone;
///   repairing this clone deletes a remote-tracking ref, which the guard
///   leaves to the operator.
/// - Any other ref is damage in this clone, which `git fsck` shows and a
///   fresh clone avoids.
pub(crate) fn unresolvable_tip(repo: &Repository, reference: &str, error: &git2::Error) -> String {
    let found = repo.find_reference(reference).ok();
    let target = found.as_ref().and_then(git2::Reference::target);
    let symbolic = found
        .as_ref()
        .and_then(|found| found.symbolic_target().ok().flatten());
    let points_at = match (target, symbolic) {
        (Some(oid), _) => oid.to_string(),
        (None, Some(symbolic)) => format!("the symbolic ref {symbolic}"),
        (None, None) => "a target git cannot read".to_string(),
    };
    let present = target.is_some_and(|oid| repo.find_object(oid, None).is_ok());
    let quoted = |operand: &str| crate::git::git_operands(&[operand]);
    let remedy = match reference
        .strip_prefix("refs/remotes/")
        .and_then(|rest| rest.split_once('/').map(|(remote, _)| (remote, rest)))
    {
        Some((remote, _)) if present => format!(
            "the fetched tip cannot be resolved, so the line it names cannot be judged; fetch it again with a plain `git fetch {}`, which rewrites {reference} from the remote and moves no local branch",
            quoted(remote)
        ),
        Some((remote, short)) => format!(
            "the fetched tip cannot be resolved, so the line it names cannot be judged, and a fetch cannot rewrite the ref while its object is missing (git stops on the bad object); {}; repairing this clone deletes the ref so a fetch can recreate it (`git branch -d -r {}`, then `git fetch {}`), which the guard leaves to the operator",
            fresh_clone(remote),
            quoted(short),
            quoted(remote)
        ),
        None => format!(
            "{reference} cannot be resolved, so the line it names cannot be judged; check this clone with `git fsck`, or {}",
            fresh_clone("origin")
        ),
    };
    format!(
        "{reference} points at {points_at}, which is not a commit this clone can read ({}); {remedy}",
        error.message()
    )
}

/// The remote a fetched ref (`refs/remotes/<remote>/…` or `<remote>/…`)
/// names, else `origin`.
pub(crate) fn remote_of(reference: &str) -> &str {
    reference
        .strip_prefix("refs/remotes/")
        .unwrap_or(reference)
        .split_once('/')
        .map_or("origin", |(remote, _)| remote)
}

/// The fresh-clone remedy for `remote`, naming the remote and never its
/// URL: a URL can carry a credential in its userinfo, query or fragment,
/// and a message reaches logs and transcripts (TSK-416 review). A clone has
/// its own refs, so what made this clone's judgment refuse is not there; a
/// worktree of this clone would share them. The remote is quoted as one
/// shell argument, after `--` in the command when it starts with `-`.
pub(crate) fn fresh_clone(remote: &str) -> String {
    let operand = crate::git::git_operands(&[remote]);
    let remote = crate::git::shell_arg(remote);
    format!(
        "work from a fresh clone of the remote {remote}, made from outside this clone (`git clone <url> <path>`, where `git remote get-url {operand}` prints the url)"
    )
}

/// The remedy when this clone's local copy of a line cannot be settled
/// against the fetched one (diverged, or its ancestry unreadable): rerun
/// the command from a fresh clone, whose local branch is the fetched
/// line, on the task branch pushed from here. A worktree of this clone
/// shares its branches, so it would refuse the same way (TSK-416 review).
pub(crate) fn fresh_clone_retry(remote: &str) -> String {
    format!(
        "a worktree of this clone shares its branches and would refuse the same way, so push the task branch, {}, switch to the task branch there and run this again",
        fresh_clone(remote)
    )
}

/// [`unresolvable_tip`] for the first ref under `refs/remotes/<remote>/`
/// that names no readable commit, so a failed fetch of `remote` can say
/// which ref stopped it. Symbolic refs (`<remote>/HEAD`) are skipped: they
/// fail through the ref they name.
pub(crate) fn unresolvable_remote_ref(repo: &Repository, remote: &str) -> Option<String> {
    let glob = format!("refs/remotes/{remote}/*");
    repo.references_glob(&glob)
        .ok()?
        .flatten()
        .find_map(|found| {
            found.target()?;
            let name = found.name().ok()?.to_string();
            let error = found.peel_to_commit().err()?;
            Some(unresolvable_tip(repo, &name, &error))
        })
}

/// Resolve a declared target to an available local or remote-tracking ref.
///
/// The declared logical name remains portable (`main`, for example) while CI
/// checkouts that expose only `origin/main` can verify the same task. A
/// diverged local branch resolves to the local branch here; callers that
/// anchor work use [`resolve_work_target_checked`], which refuses it.
/// Unreadable target state returns an error; a missing conventional default
/// is represented separately as `None`.
///
/// # Errors
///
/// Returns an error if the repository, target or configured upstream cannot be read.
pub fn resolve_work_target(
    repo_root: &Path,
    declared: Option<&str>,
) -> Result<Option<String>, WorkStartError> {
    match resolve_work_target_checked(repo_root, declared) {
        Ok(resolved) => Ok(resolved.map(|resolved| resolved.target)),
        // The local branch, by its exact ref: a bare name now reads the
        // fetched ref first, so it would not name the branch kept here.
        Err(WorkStartError::DivergedTarget { local, .. }) => {
            Ok(Some(format!("refs/heads/{local}")))
        }
        Err(error) => Err(error),
    }
}

/// Resolve a declared target as [`resolve_work_target`] does, but never
/// anchor on a stale local branch.
///
/// When a bare name (`main`) has a local branch, compared with the upstream
/// Git has configured for it (`main@{upstream}`):
/// - no configured upstream, equal, or the local branch ahead (unpushed
///   commits): the local branch;
/// - the local branch strictly behind: the upstream, since a stale local
///   branch anchors on an old snapshot;
/// - diverged: an error, since neither is clearly the target.
///
/// A same-named branch of another remote is never substituted. Without a
/// local branch, `origin/<name>` is used, as in a CI clone.
///
/// # Errors
///
/// Returns [`WorkStartError::DivergedTarget`] when the local branch and its
/// configured upstream have diverged.
pub fn resolve_work_target_checked(
    repo_root: &Path,
    declared: Option<&str>,
) -> Result<Option<ResolvedWorkTarget>, WorkStartError> {
    let Some(target) = declared.filter(|value| !value.is_empty()) else {
        return Ok(default_work_target(repo_root)?.map(|target| ResolvedWorkTarget { target }));
    };
    let plain = |target: String| Ok(Some(ResolvedWorkTarget { target }));
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let Some(candidates) = target_reference_names(target) else {
        return plain(target.to_string());
    };
    // A bare name with a local branch: that branch, unless it is strictly
    // behind the upstream Git has configured for it.
    let local_ref = format!("refs/heads/{target}");
    if candidates.contains(&local_ref) && !target.starts_with("refs/") {
        if let Some(local_id) = reference_commit(&repo, &local_ref)? {
            return local_or_upstream(&repo, target, &local_ref, local_id);
        }
    }
    // Otherwise the first candidate that resolves: an exact full ref, or
    // `origin/<name>` in a checkout with no local branch (a CI clone).
    for found in candidates {
        if reference_commit(&repo, &found)?.is_some() {
            return if target.starts_with("refs/") {
                plain(found)
            } else {
                plain(reference_display_name(&found))
            };
        }
    }
    plain(target.to_string())
}

/// The local branch `local`, or its configured upstream when the local
/// branch is strictly behind it. Only the upstream Git has configured
/// (`branch.<name>.remote` and `.merge`) may replace it: another remote's
/// same-named branch, such as a fork's `origin/main` beside an
/// `upstream/main` the branch tracks, is not the integration target. With no
/// configured upstream, a same-named origin tracking ref is the fetched target.
fn local_or_upstream(
    repo: &Repository,
    local: &str,
    local_ref: &str,
    local_id: git2::Oid,
) -> Result<Option<ResolvedWorkTarget>, WorkStartError> {
    // The kept local branch, named so that reading the name gives this
    // branch: the bare name when the ref a bare read takes holds this
    // commit, else its exact ref (TSK-416).
    let keep = || -> Result<Option<ResolvedWorkTarget>, WorkStartError> {
        let read = target_reference_read(repo, local)?.map(|(_, commit)| commit.id());
        let target = if read == Some(local_id) {
            local.to_string()
        } else {
            local_ref.to_string()
        };
        Ok(Some(ResolvedWorkTarget { target }))
    };
    // Only the same branch on a configured remote replaces it: an upstream
    // naming another branch, which git sets when a line is cut from
    // `origin/main` and which any config write can change, is never the
    // line (TSK-416).
    // A configured upstream git cannot decode is an error, never an absent
    // upstream (issue 79), whichever remote it names.
    match repo.branch_upstream_name(local_ref) {
        Ok(name) => {
            name.as_str().map_err(|_| {
                WorkStartError::Repository("configured upstream is not valid UTF-8".to_string())
            })?;
        }
        Err(error) if error.code() == git2::ErrorCode::NotFound => {}
        Err(error) => return Err(WorkStartError::Repository(error.to_string())),
    }
    let remotes = repo
        .remotes()
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let upstream_ref = if let Some(upstream) = configured_upstream(repo, local, &remotes)? {
        Some(upstream)
    } else {
        let remote = format!("refs/remotes/origin/{local}");
        reference_commit(repo, &remote)?.map(|_| remote)
    };
    let Some(upstream_ref) = upstream_ref else {
        return keep();
    };
    let Some(upstream_id) = reference_commit(repo, &upstream_ref)? else {
        return Err(WorkStartError::Repository(format!(
            "configured upstream {upstream_ref} does not resolve"
        )));
    };
    if upstream_id == local_id {
        return keep();
    }
    // The exact ref compared is the ref anchored on: a shortened name such
    // as `origin/main` could resolve to a different ref.
    let upstream = upstream_ref;
    let (ahead, behind) = history::ahead_behind(repo, local_id, upstream_id)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    match (ahead, behind) {
        (_, 0) => keep(),
        (0, _) => Ok(Some(ResolvedWorkTarget { target: upstream })),
        (ahead, behind) => Err(WorkStartError::DivergedTarget {
            local: local.to_string(),
            remote: upstream,
            local_tip: local_id.to_string(),
            remote_tip: upstream_id.to_string(),
            ahead,
            behind,
        }),
    }
}

/// Whether a ref may serve as a stable planning authority.
///
/// Task branches are implementation surfaces and cannot authorize their own
/// durable records.
#[must_use]
pub fn is_stable_work_target(target: &str) -> bool {
    target_reference_names(target).is_some()
        && !logical_target(target).starts_with('-')
        && !looks_like_full_object_id(target)
        && !logical_target(target).starts_with("task/")
}

fn looks_like_full_object_id(target: &str) -> bool {
    matches!(target.len(), 40 | 64) && target.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Whether full durable-work protection is active. Unknown relevant state is
/// returned as an error so hook and CI callers can block it explicitly.
///
/// Full tier remains authoritative without task files. Lower-tier or absent
/// state requires an actual supported TSK path in a flat or historical nested
/// task home; a foreign directory alone is not an opt-in signal.
///
/// # Errors
///
/// Returns a typed error for malformed, unsupported or unsafe state, and for
/// an unreadable or exhausted bounded task-home probe.
pub fn durable_work_tracking_enabled(repo_root: &Path) -> Result<bool, DurableTrackingError> {
    let state_dir = repo_root.join(crate::scaffold::state::CODEFLOW_DIR);
    match std::fs::symlink_metadata(&state_dir) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(DurableTrackingError::StatePath(state_dir));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(DurableTrackingError::StateMetadata {
                path: state_dir,
                source,
            });
        }
        Ok(_) => {}
    }

    let state_path = crate::scaffold::state::ProjectState::path(repo_root);
    match std::fs::symlink_metadata(&state_path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(DurableTrackingError::StatePath(state_path));
        }
        Ok(_) => {
            let state = crate::scaffold::state::ProjectState::load(repo_root)?;
            if state.schema_version != 1 {
                return Err(DurableTrackingError::UnsupportedStateVersion(
                    state.schema_version,
                ));
            }
            if state.tier == crate::scaffold::manifest::Tier::Full {
                return Ok(true);
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(DurableTrackingError::StateMetadata {
                path: state_path,
                source,
            });
        }
    }

    crate::workgraph::layout::has_task_record_path(
        &repo_root.join("project-management"),
        MAX_ACTIVATION_ENTRIES,
    )
    .map_err(|error| match error {
        crate::workgraph::layout::InventoryError::LimitExceeded => {
            DurableTrackingError::TaskInventoryLimit
        }
        crate::workgraph::layout::InventoryError::Unreadable => {
            DurableTrackingError::TaskInventoryUnreadable
        }
    })
}

/// The blob of `.codeflow/project.toml` in `tree`, walked one name at a
/// time: `None` only when a name is really absent (or `.codeflow` is not a
/// directory). An object this clone lacks, or a config path that is not a
/// file, is an error naming the revision and path, never "not present".
fn committed_state_entry(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    revision: &str,
) -> Result<Option<git2::Oid>, String> {
    let path = crate::scaffold::state::PROJECT_TOML;
    let Some(folder) = tree.get_name(".codeflow") else {
        return Ok(None);
    };
    if folder.kind() != Some(git2::ObjectType::Tree) {
        return Ok(None);
    }
    let folder = repo.find_tree(folder.id()).map_err(|error| {
        format!(
            "{revision}: .codeflow cannot be read, so {path} cannot be: {}",
            error.message()
        )
    })?;
    let Some(entry) = folder.get_name("project.toml") else {
        return Ok(None);
    };
    if entry.kind() != Some(git2::ObjectType::Blob) {
        return Err(format!("{revision}: {path} is not a file"));
    }
    Ok(Some(entry.id()))
}

/// Whether durable work tracking was on in the committed tree of `revision`:
/// a full-tier `.codeflow/project.toml`, or a supported task record path.
/// CI reads this at the target so a pull request cannot switch tracking off
/// for itself by deleting the records or the state file (SPC-013 R-70).
///
/// # Errors
///
/// Returns the reason when the revision, the state file or the tree cannot
/// be read.
pub fn durable_work_tracking_enabled_at(repo_root: &Path, revision: &str) -> Result<bool, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let tree = repo
        .revparse_single(revision)
        .and_then(|object| object.peel_to_tree())
        .map_err(|error| format!("{revision}: {error}"))?;
    if let Some(entry) = committed_state_entry(&repo, &tree, revision)? {
        let blob = repo.find_blob(entry).map_err(|error| {
            format!(
                "{revision}: {} cannot be read: {}",
                crate::scaffold::state::PROJECT_TOML,
                error.message()
            )
        })?;
        let text = std::str::from_utf8(blob.content())
            .map_err(|_| format!("{revision}: .codeflow/project.toml is not UTF-8"))?;
        let state: crate::scaffold::state::ProjectState = toml::from_str(text)
            .map_err(|error| format!("{revision}: .codeflow/project.toml: {error}"))?;
        // As the working tree's reader: an unsupported version is state
        // this binary cannot read, never an untracked project.
        if state.schema_version != 1 {
            return Err(format!(
                "{revision}: {}",
                DurableTrackingError::UnsupportedStateVersion(state.schema_version)
            ));
        }
        if state.tier == crate::scaffold::manifest::Tier::Full {
            return Ok(true);
        }
    }
    // Only a tree without the directory has no records; a read failure is
    // the error, never tracking off (issue 79).
    let entry = match tree.get_path(Path::new("project-management")) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(false),
        Err(error) => return Err(format!("{revision}: {}", error.message())),
    };
    let home = repo
        .find_tree(entry.id())
        .map_err(|_| format!("{revision}: project-management is not a directory"))?;
    let mut found = false;
    crate::git::walk_tree(&repo, &home, &mut |name, _| {
        // A record path is valid text; any other name is not a record.
        let Ok(text) = name.rule_text() else {
            return crate::git::Walk::Continue;
        };
        let path = format!("project-management/{text}");
        if record_kind_for_tree_path(&path) == Some(RecordKind::Task) {
            found = true;
            return crate::git::Walk::Stop;
        }
        crate::git::Walk::Continue
    })
    .map_err(|error| error.to_string())?;
    Ok(found)
}

/// Check that `branch` is an epic's integration line landing on its
/// destination, so the whole line may land as one pull request whose tasks
/// were each classified when they landed on it. A prefix alone grants
/// nothing: the name must carry an epic that exists and is not cancelled, a
/// task of that epic, or of an epic declaring the line as its
/// `parent_line`, must target this exact line, the pull request must go
/// to the project's default target, and the line's first-parent history
/// from the merge-base must hold only merges or direct commits with landed
/// adoption entries in the epic record. Returns the epic id.
///
/// # Errors
///
/// Returns why the head is not an epic line landing, or why git could not
/// be read.
pub fn check_epic_line(
    repo_root: &Path,
    branch: &str,
    base_ref: &str,
    base: &str,
    head: &str,
) -> Result<String, String> {
    check_epic_line_with_adoptions(repo_root, branch, base_ref, base, head).map(|(epic, _)| epic)
}

/// Check an epic line as [`check_epic_line`] does and also return the
/// adopted direct commits and the out-of-range entries to show its reviewer.
///
/// # Errors
/// Returns the same classification and history failures as [`check_epic_line`].
pub fn check_epic_line_with_adoptions(
    repo_root: &Path,
    branch: &str,
    base_ref: &str,
    base: &str,
    head: &str,
) -> Result<(String, super::line_adoption::AdoptionReport), String> {
    match epic_line_proof(repo_root, branch, base_ref, base, head)? {
        EpicLineProof::Verified(epic, adoptions) => Ok((epic, adoptions)),
        EpicLineProof::NotLine(reason) => Err(reason),
    }
}

/// A readable line can fail the epic criteria without a reader failing.
#[derive(Debug)]
pub enum EpicLineProof {
    /// The epic line, with the direct commits its landed entries adopt.
    Verified(String, super::line_adoption::AdoptionReport),
    NotLine(String),
}

/// Why the line `branch`'s own epic `epic_id` refuses the proof across
/// `record_sets` (each a record set and its place): it has no epic record
/// in any of them, or it is cancelled or archived in any one, whichever
/// set still has it active (TSK-1286).
fn line_owner_refusal(
    record_sets: &[(&BTreeMap<String, Record>, &str)],
    branch: &str,
    epic_id: &str,
) -> Option<String> {
    let owner = |records: &BTreeMap<String, Record>| {
        records
            .get(epic_id)
            .filter(|record| record.kind == RecordKind::Epic)
            .map(|record| record.status.clone())
    };
    if record_sets
        .iter()
        .all(|(records, _)| owner(records).is_none())
    {
        return Some(format!(
            "'{branch}' names {epic_id}, which has no epic record"
        ));
    }
    record_sets.iter().find_map(|(records, place)| {
        owner(records)
            .filter(|status| matches!(status.as_str(), "cancelled" | "archived"))
            .map(|status| format!("{epic_id} is {status} on {place}"))
    })
}

/// Obtain the epic-line evidence, preserving a negative proof as a value.
///
/// # Errors
/// Repository, revision, record or history evidence could not be read,
/// including the epic's `line_adoptions` when a direct commit needs them.
pub fn epic_line_proof(
    repo_root: &Path,
    branch: &str,
    base_ref: &str,
    base: &str,
    head: &str,
) -> Result<EpicLineProof, String> {
    // The name grammar reads nothing, so its refusal is a negative proof.
    let epic_id = match super::line_adoption::epic_id(branch) {
        Ok(epic_id) => epic_id,
        Err(reason) => return Ok(EpicLineProof::NotLine(reason)),
    };
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| format!("{revision}: {error}"))
    };
    let head_commit = commit(head)?;
    let target_tip = commit(base)?.id();
    let merge_base = history::merge_base(&repo, target_tip, head_commit.id())
        .map_err(|error| error.to_string())?;
    let records_at = |oid: git2::Oid| {
        repo.find_commit(oid)
            .and_then(|commit| commit.tree())
            .map_err(|error| error.to_string())
            .and_then(|tree| records_from_tree(&repo, &tree).map_err(|error| error.to_string()))
    };
    // The target's current records decide, so an epic closed on the target
    // after the line forked is closed here; the merge-base only bounds the
    // range and the first-parent walk.
    let at_base = records_at(target_tip)?;
    let at_head = records_at(head_commit.id())?;
    // The line's fetched tip is a trusted anchor beside the target whenever
    // it exists, on every path, so a cancellation there is never skipped
    // (TSK-1286).
    let fetched = super::line_adoption::fetched_tip(repo_root, branch)?;
    let at_line = match &fetched {
        Some(tip) => Some(records_at(
            git2::Oid::from_str(tip).map_err(|error| error.to_string())?,
        )?),
        None => None,
    };
    let line_place = format!("'origin/{branch}'");
    let mut anchors = vec![(&at_base, "the target")];
    if let Some(records) = &at_line {
        anchors.push((records, line_place.as_str()));
    }
    let everywhere: Vec<(&BTreeMap<String, Record>, &str)> = anchors
        .iter()
        .copied()
        .chain([(&at_head, "the head")])
        .collect();
    if let Some(refusal) = line_owner_refusal(&everywhere, branch, epic_id) {
        return Ok(EpicLineProof::NotLine(refusal));
    }
    let own_task = |records: &BTreeMap<String, Record>| {
        records.values().any(|record| {
            record.kind == RecordKind::Task
                && record.epic_id.as_deref() == Some(epic_id)
                && record
                    .integration_target
                    .as_deref()
                    .is_some_and(|target| logical_target(target) == branch)
        })
    };
    // A task of an epic that declares this line binds it only from the
    // trusted anchors, the target's tip and the line's fetched tip, never
    // from the head's records. Every declaration refusal runs first, even
    // when a task of the line's own epic binds it: a declaring epic
    // cancelled at an anchor or the head, or a head that changes or removes
    // a trusted declaration or deletes the declaring epic (TSK-1286). A
    // task of the line's own epic only lifts the need for a declared task.
    let declared = match declared_line_tasks(&anchors, &at_head, branch) {
        Err(refusal) => return Ok(EpicLineProof::NotLine(refusal)),
        Ok(declared) => declared,
    };
    if !declared && !own_task(&at_head) && !own_task(&at_base) {
        return Ok(EpicLineProof::NotLine(format!(
            "no task of {epic_id} targets '{branch}', nor a task of an epic declaring {} '{branch}' on the target or the line's fetched tip",
            super::line_adoption::PARENT_LINE_FIELD
        )));
    }
    let destination = default_work_target(repo_root)
        .map_err(|error| error.to_string())?
        .ok_or("no main or master branch to land the line on")?;
    if logical_target(base_ref) != logical_target(&destination) {
        return Ok(EpicLineProof::NotLine(format!(
            "'{branch}' lands on '{}', not '{base_ref}'",
            logical_target(&destination)
        )));
    }
    let adoptions = super::line_adoption::check_range(
        repo_root,
        branch,
        &target_tip.to_string(),
        &merge_base.to_string(),
        &head_commit.id().to_string(),
    )?;
    if let Some(refusal) = adoptions.refusal(branch, epic_id) {
        // Entries that cannot be read leave the direct commits unproven,
        // which is a reader failure, never a line proven not to qualify.
        if adoptions.record_error.is_some() {
            return Err(refusal);
        }
        return Ok(EpicLineProof::NotLine(refusal));
    }
    Ok(EpicLineProof::Verified(epic_id.to_string(), adoptions))
}

/// Whether a stable target resolves to a real local or remote-tracking branch.
#[must_use]
pub fn work_target_resolves(repo_root: &Path, target: &str) -> bool {
    if !is_stable_work_target(target) {
        return false;
    }
    Repository::discover(repo_root)
        .ok()
        .is_some_and(|repo| target_reference(&repo, target).is_ok_and(|target| target.is_some()))
}

/// Read a task's declared integration target from the visible checkout.
///
/// This is intentionally read-only. It accepts canonical flat records and the
/// historical nested task layout so hooks and CI can share the same target
/// selection without owning harness- or branch-management state.
///
/// # Errors
///
/// Returns an error if the task inventory or matching record cannot be read.
pub fn declared_work_target(
    repo_root: &Path,
    task_id: &str,
) -> Result<Option<String>, WorkStartError> {
    let pm_root = repo_root.join("project-management");
    let files = crate::workgraph::layout::task_record_files(&pm_root)
        .map_err(|error| WorkStartError::InvalidGraph(error.to_string()))?;
    match files.into_iter().find(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem == task_id)
    }) {
        Some(path) => declared_work_target_at(&path),
        None => Ok(None),
    }
}

/// Read a work branch's target from the pushed revision, not the checkout.
/// Uses readiness's bounded reader and supported layouts for the matching
/// task only. Returns a logical branch name for destination resolution.
///
/// # Errors
///
/// Returns an error if the revision or matching task cannot be read, so a
/// caller can distinguish an unavailable target from an absent declaration.
pub fn declared_work_target_at_revision(
    repo_root: &Path,
    branch: &str,
    revision: &str,
) -> Result<Option<String>, WorkStartError> {
    Ok(
        declared_work_record_at_revision(repo_root, branch, revision)?
            .and_then(|record| record.integration_target)
            .filter(|target| !target.is_empty())
            .map(|target| logical_target(&target).to_owned()),
    )
}

/// The task record a work branch claims, read at `revision`.
fn declared_work_record_at_revision(
    repo_root: &Path,
    branch: &str,
    revision: &str,
) -> Result<Option<Record>, WorkStartError> {
    let Some(suffix) = work_branch_suffix(repo_root, branch)? else {
        return Ok(None);
    };
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let tree = repo
        .revparse_single(revision)
        .and_then(|object| object.peel_to_tree())
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let records = records_from_tree_matching(&repo, &tree, |path, kind| {
        kind == RecordKind::Task
            && path
                .rsplit('/')
                .next()
                .and_then(markdown_stem)
                .is_some_and(|id| suffix.starts_with(&format!("{id}-")))
    })?;
    Ok(records
        .into_values()
        .filter(|record| suffix.starts_with(&format!("{}-", record.id)))
        .max_by_key(|record| record.id.len()))
}

/// Whether the records at one revision bind a declared epic line.
#[derive(Debug, PartialEq, Eq)]
pub enum EpicLineBinding {
    /// The records bind the line as a target the branch may declare.
    Bound,
    /// A refusal no other revision can lift: the name is not an epic line,
    /// the branch cannot target its epic, or the epic is cancelled or
    /// archived.
    Refused(String),
    /// These records do not bind the line; another trusted revision may.
    Unbound(String),
}

/// Whether trusted records at `anchor` (named `place` in messages) bind
/// `line` as an epic line that `branch` may declare. The task record at
/// `head` is author-controlled routing input, not authorization: it must
/// name the line's epic, name an epic whose record at the anchor declares
/// this line as its `parent_line` (TSK-1286), or be standalone with a reason
/// and this target. A planning branch must name the line's own epic.
///
/// Only the trusted anchor records bind the line: the epic must exist there
/// without being cancelled or archived, and a task of that epic, or of an
/// epic declaring the line, there must name this exact line, as
/// [`check_epic_line`] requires of a landing line. A declaration counts
/// only from the anchor, and a head whose copy of the declaring epic
/// declares anything else is refused. The caller selects the protected tip
/// or the remote line's tip as the anchor. Head-record membership never
/// replaces this anchor check.
///
/// # Errors
///
/// Returns why the repository, a revision or a record could not be read.
pub fn epic_line_binding(
    repo_root: &Path,
    branch: &str,
    head: &str,
    line: &str,
    anchor: &str,
    place: &str,
) -> Result<EpicLineBinding, WorkStartError> {
    let line = logical_target(line);
    let epic_id = match super::line_adoption::epic_id(line) {
        Ok(epic_id) => epic_id,
        Err(reason) => return Ok(EpicLineBinding::Refused(reason)),
    };
    let not_own = format!("names {epic_id}, which is not the epic of branch '{branch}'");
    // A planning branch must name the line's own epic; it never binds
    // through a declaration (TSK-1286 keeps the planning rule as it was).
    let claim = if let Some(plan) = branch.strip_prefix("plan/") {
        if super::line_adoption::epic_id(&format!("integration/{plan}")) == Ok(epic_id) {
            Claim::Line
        } else {
            return Ok(EpicLineBinding::Refused(not_own));
        }
    } else if let Some(record) = declared_work_record_at_revision(repo_root, branch, head)? {
        if let Some(refusal) = task_parent_line_refusal(&record) {
            return Ok(EpicLineBinding::Refused(refusal));
        }
        if let Some(own) = record.epic_id.as_deref() {
            if own == epic_id {
                Claim::Line
            } else {
                Claim::Declared(own.to_string())
            }
        } else {
            if record
                .standalone_reason
                .as_deref()
                .is_none_or(|reason| reason.trim().is_empty())
            {
                return Ok(EpicLineBinding::Refused(format!(
                    "standalone task on branch '{branch}' has no nonblank standalone_reason"
                )));
            }
            let target = record.integration_target.as_deref().unwrap_or_default();
            if logical_target(target) != line {
                return Ok(EpicLineBinding::Refused(format!(
                    "standalone task on branch '{branch}' declares target '{target}', not this line '{line}'"
                )));
            }
            Claim::Line
        }
    } else {
        return Ok(EpicLineBinding::Refused(not_own));
    };
    let head_value = match head_refusal(repo_root, head, epic_id, &claim, &not_own)? {
        Err(refusal) => return Ok(EpicLineBinding::Refused(refusal)),
        Ok(head_value) => head_value,
    };
    let records = anchor_records(repo_root, anchor)?;
    let Claim::Declared(own) = claim else {
        return Ok(bound_in(&records, line, epic_id, place));
    };
    Ok(declared_binding(
        &records,
        line,
        epic_id,
        &own,
        head_value.as_deref(),
        place,
        &not_own,
    ))
}

/// What a branch's task record claims about the line it targets.
enum Claim {
    /// The task is of the line's own epic, standalone for this line, or a
    /// planning branch of the line's epic.
    Line,
    /// The task is of this other epic, which must declare the line in the
    /// trusted records.
    Declared(String),
}

/// The refusal for a task record that carries `parent_line`: only an epic
/// record declares the line its tasks land on (TSK-1286).
fn task_parent_line_refusal(record: &Record) -> Option<String> {
    record.parent_line.as_ref().map(|_| {
        format!(
            "task {} carries {}, which only an epic record may declare; remove it from the task record and declare the line on the task's epic through a planning pull request",
            record.id,
            super::line_adoption::PARENT_LINE_FIELD
        )
    })
}

/// What the exact `head` says about the epics a binding relies on, read
/// before any anchor so no positive result can skip it (TSK-1286): the
/// line's own epic `epic_id`, or the declaring epic a [`Claim::Declared`]
/// names, cancelled or archived on the head is a refusal (`Err`).
/// Otherwise `Ok` carries the head's `parent_line` for the declaring epic
/// (`None` for [`Claim::Line`], or when the head removed the field or the
/// record), which [`declared_binding`] compares with each anchor.
fn head_refusal(
    repo_root: &Path,
    head: &str,
    epic_id: &str,
    claim: &Claim,
    not_own: &str,
) -> Result<Result<Option<String>, String>, WorkStartError> {
    let closed = |record: &Record| matches!(record.status.as_str(), "cancelled" | "archived");
    // A head record that does not parse refuses, so the binding stays
    // closed and the check that owns the malformed field names it.
    let read = |epic: &str| match epic_record_at_revision(repo_root, epic, head) {
        Ok(record) => Ok(Ok(record)),
        Err(WorkStartError::InvalidGraph(reason)) => Ok(Err(format!(
            "names {epic_id}, but {epic}'s record on the head cannot be read: {reason}"
        ))),
        Err(error) => Err(error),
    };
    let owner = match read(epic_id)? {
        Ok(owner) => owner,
        Err(refusal) => return Ok(Err(refusal)),
    };
    if let Some(owner) = owner.filter(|owner| closed(owner)) {
        return Ok(Err(format!(
            "names {epic_id}, which is {} on the head",
            owner.status
        )));
    }
    let Claim::Declared(own) = claim else {
        return Ok(Ok(None));
    };
    let declaring = match read(own)? {
        Ok(declaring) => declaring,
        Err(refusal) => return Ok(Err(refusal)),
    };
    match declaring {
        Some(record) if closed(&record) => Ok(Err(format!(
            "{not_own}: {own} is {} on the head",
            record.status
        ))),
        Some(record) => Ok(Ok(record.parent_line)),
        None => Ok(Ok(None)),
    }
}

/// The records of the tree at `anchor`, an exact trusted revision.
fn anchor_records(
    repo_root: &Path,
    anchor: &str,
) -> Result<BTreeMap<String, Record>, WorkStartError> {
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let tree = repo
        .revparse_single(anchor)
        .and_then(|object| object.peel_to_tree())
        .map_err(|error| WorkStartError::Repository(format!("{anchor}: {error}")))?;
    records_from_tree(&repo, &tree)
}

/// The epic record `epic` at `revision`, found by its file name and id.
fn epic_record_at_revision(
    repo_root: &Path,
    epic: &str,
    revision: &str,
) -> Result<Option<Record>, WorkStartError> {
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let tree = repo
        .revparse_single(revision)
        .and_then(|object| object.peel_to_tree())
        .map_err(|error| WorkStartError::Repository(format!("{revision}: {error}")))?;
    let records = records_from_tree_matching(&repo, &tree, |path, kind| {
        kind == RecordKind::Epic && path.rsplit('/').next().and_then(markdown_stem) == Some(epic)
    })?;
    Ok(records
        .into_iter()
        .find(|(id, record)| id == epic && record.kind == RecordKind::Epic)
        .map(|(_, record)| record))
}

/// The status and `parent_line` of the epic record `epic` at `revision`,
/// `None` when that revision has no such record (TSK-1320). A record that
/// cannot be read is an error, never absent.
pub(crate) fn epic_facts_at(
    repo_root: &Path,
    epic: &str,
    revision: &str,
) -> Result<Option<EpicFacts>, String> {
    epic_record_at_revision(repo_root, epic, revision)
        .map(|record| {
            record.map(|record| EpicFacts {
                status: record.status,
                parent_line: record.parent_line,
            })
        })
        .map_err(|error| error.to_string())
}

/// Whether the trusted `records` (named `place`) let a task of `own`, an
/// epic other than the line's, target `line`: `own`'s record there must
/// declare `parent_line` as exactly this line, and the head's copy of that
/// record (`head_value`) must declare the same line. The declaration is
/// trusted only from `records`; a head that adds it gains nothing, and a
/// head that changes or removes it is refused, so a branch never rebinds
/// its own epic (TSK-1286). A cancelled or archived `own` there refuses
/// before anything else, so a caller that reads several anchors lets it
/// outweigh a declaration at another; then the line itself must be bound
/// there ([`bound_in`]).
fn declared_binding(
    records: &BTreeMap<String, Record>,
    line: &str,
    epic_id: &str,
    own: &str,
    head_value: Option<&str>,
    place: &str,
    not_own: &str,
) -> EpicLineBinding {
    let field = super::line_adoption::PARENT_LINE_FIELD;
    let membership = line_membership(line, own, &record_facts(records), place);
    // Refusals first, in this order, so no positive result can skip one:
    // the declaring epic closed here, then a head that changes or removes
    // the declaration found here, then the line's own epic closed here
    // (inside [`bound_in`]).
    if let LineMembership::Refused(reason) = &membership {
        return EpicLineBinding::Refused(format!("{not_own}: {reason}"));
    }
    if membership == LineMembership::Declared && head_value != Some(line) {
        let shown = head_value.map_or_else(|| "none".to_string(), |value| format!("'{value}'"));
        return EpicLineBinding::Refused(format!(
            "{not_own}: {own} declares {field} '{line}' on {place}, but the head's {own} record declares {shown}; a branch cannot change the line its own epic lands on, so restore the field and change it through a planning pull request into the line"
        ));
    }
    let binding = bound_in(records, line, epic_id, place);
    if binding != EpicLineBinding::Bound {
        return binding;
    }
    if let LineMembership::Not(reason) = &membership {
        let head_only = if head_value == Some(line) {
            format!(
                "; the head's own {field} for {own} is not trusted, so land the declaration on the line through a planning pull request first"
            )
        } else {
            String::new()
        };
        return EpicLineBinding::Unbound(format!("{not_own}, and {reason}{head_only}"));
    }
    EpicLineBinding::Bound
}

/// Whether the trusted records at `anchor` (named `place` in messages) bind
/// `line` as an epic line, whatever branch asks: the epic exists there
/// without being cancelled or archived, and a task of that epic, or of an
/// epic declaring the line as its `parent_line` there, names this exact
/// line. [`epic_line_binding`] adds the asking branch's own
/// claim; `codeflow ci` uses this alone to confirm the line a landing
/// candidate's head would become the tip of (TSK-1155).
///
/// # Errors
///
/// Returns why the repository, the anchor or a record could not be read.
pub fn epic_line_bound_at(
    repo_root: &Path,
    line: &str,
    anchor: &str,
    place: &str,
) -> Result<EpicLineBinding, WorkStartError> {
    let line = logical_target(line);
    let epic_id = match super::line_adoption::epic_id(line) {
        Ok(epic_id) => epic_id,
        Err(reason) => return Ok(EpicLineBinding::Refused(reason)),
    };
    let records = anchor_records(repo_root, anchor)?;
    Ok(bound_in(&records, line, epic_id, place))
}

/// Whether the trusted records at commit `tip` bind `line`, judged by
/// [`bound_in`] as every other reader judges it, so `codeflow task status`
/// picks a receiving line exactly as `codeflow ci` would bind it (TSK-1286).
///
/// # Errors
///
/// Returns why the commit or a record at it could not be read.
pub(crate) fn line_bound_at_tip(
    repo: &Repository,
    tip: git2::Oid,
    line: &str,
) -> Result<EpicLineBinding, String> {
    let line = logical_target(line);
    let epic_id = match super::line_adoption::epic_id(line) {
        Ok(epic_id) => epic_id,
        Err(reason) => return Ok(EpicLineBinding::Refused(reason)),
    };
    let tree = repo
        .find_commit(tip)
        .and_then(|commit| commit.tree())
        .map_err(|error| format!("{tip}: {error}"))?;
    let records = records_from_tree(repo, &tree).map_err(|error| error.to_string())?;
    Ok(bound_in(&records, line, epic_id, "the tip"))
}

/// Whether one trusted record set (named `place`) binds `line`, whose name
/// carries `epic_id`: that epic exists there without being cancelled or
/// archived, and a task there targets this exact line as a task of that
/// epic or of an epic that declares the line as its `parent_line` there.
fn bound_in(
    records: &BTreeMap<String, Record>,
    line: &str,
    epic_id: &str,
    place: &str,
) -> EpicLineBinding {
    let Some(epic) = records
        .get(epic_id)
        .filter(|record| record.kind == RecordKind::Epic)
    else {
        return EpicLineBinding::Unbound(format!(
            "names {epic_id}, which has no epic record on {place}"
        ));
    };
    if matches!(epic.status.as_str(), "cancelled" | "archived") {
        return EpicLineBinding::Refused(format!(
            "names {epic_id}, which is {} on {place}",
            epic.status
        ));
    }
    if let Some(reason) = chained_owner(records, line, place) {
        return EpicLineBinding::Refused(reason);
    }
    if !line_has_member_task(records, line, place) {
        return EpicLineBinding::Unbound(format!(
            "is not the target of any task of {epic_id} on {place}, nor of a task of an epic declaring {} '{line}' there",
            super::line_adoption::PARENT_LINE_FIELD
        ));
    }
    EpicLineBinding::Bound
}

/// How an epic's tasks relate to an epic line in one record set (TSK-1286).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LineMembership {
    /// The epic is the one the line's name carries.
    Own,
    /// The epic's record there declares `parent_line` as exactly this line,
    /// is neither cancelled nor archived, and the line's own epic has a
    /// record there.
    Declared,
    /// The epic's record there is cancelled or archived, or the line's own
    /// epic there itself declares a `parent_line`: a refusal, which a
    /// declaration at another anchor never outweighs.
    Refused(String),
    /// Neither; the reason names what is missing or different.
    Not(String),
}

/// What one record set says about an epic: its status and `parent_line`.
pub(crate) struct EpicFacts {
    pub(crate) status: String,
    pub(crate) parent_line: Option<String>,
}

/// Whether tasks of `epic` land on `line` according to one record set,
/// read through `lookup` (an epic's facts there, `None` when it has no epic
/// record there) and named `place` in messages. One level only: the
/// declaring epic names this exact line, so a chain through a third epic
/// never binds and no prefix or case spelling of the line matches, and a
/// line whose own epic itself declares a `parent_line` refuses.
pub(crate) fn line_membership(
    line: &str,
    epic: &str,
    lookup: &dyn Fn(&str) -> Option<EpicFacts>,
    place: &str,
) -> LineMembership {
    let field = super::line_adoption::PARENT_LINE_FIELD;
    let line_epic = match super::line_adoption::epic_id(line) {
        Ok(line_epic) => line_epic,
        Err(reason) => return LineMembership::Not(reason),
    };
    if epic == line_epic {
        return LineMembership::Own;
    }
    let Some(facts) = lookup(epic) else {
        return LineMembership::Not(format!("{epic} has no epic record on {place}"));
    };
    if matches!(facts.status.as_str(), "cancelled" | "archived") {
        return LineMembership::Refused(format!("{epic} is {} on {place}", facts.status));
    }
    match facts.parent_line.as_deref() {
        None => LineMembership::Not(format!("{epic} declares no {field} on {place}")),
        Some(declared) if declared != line => LineMembership::Not(format!(
            "{epic} declares {field} '{declared}' on {place}, not '{line}'"
        )),
        Some(_) => match lookup(line_epic) {
            None => LineMembership::Not(format!(
                "{epic}'s {field} names {line_epic}, which has no epic record on {place}"
            )),
            Some(owner) => match owner.parent_line.as_deref() {
                Some(parent) => {
                    LineMembership::Refused(owner_parent_refusal(line, line_epic, parent, place))
                }
                None => LineMembership::Declared,
            },
        },
    }
}

/// The refusal when `line`'s own epic `line_epic` itself declares a
/// `parent_line` (`parent`) on `place`: one level only (TSK-1286 AC-3).
fn owner_parent_refusal(line: &str, line_epic: &str, parent: &str, place: &str) -> String {
    let field = super::line_adoption::PARENT_LINE_FIELD;
    format!(
        "{line_epic}, the epic of '{line}', itself declares {field} '{parent}' on {place}; one level only, so no other epic's {field} binds this line"
    )
}

/// One level only (TSK-1286 AC-3): when the line's own epic itself declares
/// a `parent_line` in `records` (named `place`) and a task there targets
/// the line as a task of an epic declaring it, the refusal naming the cause.
/// A line with only its own epic's tasks is untouched.
fn chained_owner(records: &BTreeMap<String, Record>, line: &str, place: &str) -> Option<String> {
    let line_epic = super::line_adoption::epic_id(line).ok()?;
    let epic_record = |id: &str| {
        records
            .get(id)
            .filter(|record| record.kind == RecordKind::Epic)
    };
    let parent = epic_record(line_epic)?.parent_line.as_deref()?;
    let declaring = records.values().any(|record| {
        record.kind == RecordKind::Task
            && record
                .integration_target
                .as_deref()
                .is_some_and(|target| logical_target(target) == line)
            && record.epic_id.as_deref().is_some_and(|epic| {
                epic != line_epic
                    && epic_record(epic)
                        .is_some_and(|declarer| declarer.parent_line.as_deref() == Some(line))
            })
    });
    declaring.then(|| owner_parent_refusal(line, line_epic, parent, place))
}

/// The epic facts of a parsed record set, for [`line_membership`].
fn record_facts(records: &BTreeMap<String, Record>) -> impl Fn(&str) -> Option<EpicFacts> + '_ {
    move |id: &str| {
        records
            .get(id)
            .filter(|record| record.kind == RecordKind::Epic)
            .map(|record| EpicFacts {
                status: record.status.clone(),
                parent_line: record.parent_line.clone(),
            })
    }
}

/// Whether a task in `records` targets exactly `line` as a task of the
/// line's epic or of an epic that declares the line there.
fn line_has_member_task(records: &BTreeMap<String, Record>, line: &str, place: &str) -> bool {
    records.values().any(|record| {
        record.kind == RecordKind::Task
            && record
                .integration_target
                .as_deref()
                .is_some_and(|target| logical_target(target) == line)
            && record.epic_id.as_deref().is_some_and(|epic| {
                matches!(
                    line_membership(line, epic, &record_facts(records), place),
                    LineMembership::Own | LineMembership::Declared
                )
            })
    })
}

/// Whether a task in one of the trusted `anchors` (each a record set and
/// its place) targets `line` as a task of an epic that declares the line
/// in that same set (TSK-1286). Every anchor is read before an answer, and
/// a declaring epic that is cancelled or archived at any anchor or at the
/// `head` refuses, naming where. The `head` must also keep each trusted
/// declaration as it is, as [`declared_binding`] requires: a head that
/// changes or removes the epic's `parent_line`, or deletes the epic,
/// refuses.
///
/// # Errors
///
/// Returns the refusal for a cancelled or archived declaring epic, or for
/// a head that changes, removes or deletes a trusted declaration.
fn declared_line_tasks(
    anchors: &[(&BTreeMap<String, Record>, &str)],
    head: &BTreeMap<String, Record>,
    line: &str,
) -> Result<bool, String> {
    let field = super::line_adoption::PARENT_LINE_FIELD;
    let mut declared = BTreeMap::new();
    for (records, place) in anchors {
        if let Some(reason) = chained_owner(records, line, place) {
            return Err(reason);
        }
        for record in records.values() {
            let Some(epic) = record.epic_id.as_deref() else {
                continue;
            };
            if record.kind == RecordKind::Task
                && record
                    .integration_target
                    .as_deref()
                    .is_some_and(|target| logical_target(target) == line)
                && line_membership(line, epic, &record_facts(records), place)
                    == LineMembership::Declared
            {
                declared.entry(epic.to_string()).or_insert(*place);
            }
        }
    }
    for (epic, trusted_at) in &declared {
        for (records, place) in anchors.iter().copied().chain([(head, "the head")]) {
            if let LineMembership::Refused(reason) =
                line_membership(line, epic, &record_facts(records), place)
            {
                return Err(format!("{epic} declares {field} '{line}', but {reason}"));
            }
        }
        if let LineMembership::Not(reason) =
            line_membership(line, epic, &record_facts(head), "the head")
        {
            return Err(format!(
                "{epic} declares {field} '{line}' on {trusted_at}, but {reason}; a branch cannot change or remove the line an epic lands on, so change it through a planning pull request into the line"
            ));
        }
    }
    Ok(!declared.is_empty())
}

/// The record of task `task` at `revision`, found by its file name and id,
/// with no branch name or policy involved (TSK-350 AC-1, AC-6).
///
/// # Errors
///
/// Returns an error if the revision or the task's record cannot be read.
pub(crate) fn task_record_at_revision(
    repo_root: &Path,
    task: &str,
    revision: &str,
) -> Result<Option<Record>, WorkStartError> {
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let tree = repo
        .revparse_single(revision)
        .and_then(|object| object.peel_to_tree())
        .map_err(|error| WorkStartError::Repository(format!("{revision}: {error}")))?;
    let records = records_from_tree_matching(&repo, &tree, |path, kind| {
        kind == RecordKind::Task && path.rsplit('/').next().and_then(markdown_stem) == Some(task)
    })?;
    Ok(records
        .into_iter()
        .find(|(id, _)| id == task)
        .map(|(_, record)| record))
}

/// The `integration_target` task `task`'s record declares at `revision`,
/// as a logical branch name, read by the task's id with no branch name or
/// policy involved (TSK-350 AC-1, AC-6).
///
/// # Errors
///
/// Returns an error if the revision or the task's record cannot be read.
pub fn task_target_at_revision(
    repo_root: &Path,
    task: &str,
    revision: &str,
) -> Result<Option<String>, WorkStartError> {
    Ok(task_record_at_revision(repo_root, task, revision)?
        .and_then(|record| record.integration_target)
        .filter(|target| !target.is_empty())
        .map(|target| logical_target(&target).to_owned()))
}

/// Whether trusted records bind `line` as the epic line task `task` may
/// declare (TSK-350 AC-6, AC-8), judged at exact revisions only: the task's
/// record at `head` must name the line's epic, name an epic that declares
/// the line at an anchor with the same declaration at `head` (TSK-1286), or be
/// standalone with a reason and this target; then each `(anchor, place)` is
/// read, and a
/// refusal at any anchor (a cancelled or archived epic) wins over a binding
/// at another. No local or remote-tracking ref is read, so a stale ref can
/// neither bind the line nor hide its epic's cancellation.
///
/// # Errors
///
/// Returns why a revision or a record could not be read.
pub fn task_line_binding(
    repo_root: &Path,
    task: &str,
    head: &str,
    line: &str,
    anchors: &[(&str, &str)],
) -> Result<EpicLineBinding, WorkStartError> {
    let line = logical_target(line);
    let epic_id = match super::line_adoption::epic_id(line) {
        Ok(epic_id) => epic_id,
        Err(reason) => return Ok(EpicLineBinding::Refused(reason)),
    };
    let Some(record) = task_record_at_revision(repo_root, task, head)? else {
        return Ok(EpicLineBinding::Refused(format!(
            "{task} has no record at the judged head"
        )));
    };
    if let Some(refusal) = task_parent_line_refusal(&record) {
        return Ok(EpicLineBinding::Refused(refusal));
    }
    let not_own = format!("names {epic_id}, which {task} may not target");
    let claim = match record.epic_id.as_deref() {
        Some(own) if own == epic_id => Claim::Line,
        Some(own) => Claim::Declared(own.to_string()),
        None => {
            let standalone = record
                .standalone_reason
                .as_deref()
                .is_some_and(|reason| reason.bytes().any(|byte| !byte.is_ascii_whitespace()))
                && logical_target(record.integration_target.as_deref().unwrap_or_default()) == line;
            if !standalone {
                return Ok(EpicLineBinding::Refused(not_own));
            }
            Claim::Line
        }
    };
    // The head's copies of the line's epic and the declaring epic, read
    // once at the exact head before any anchor.
    let head_value = match head_refusal(repo_root, head, epic_id, &claim, &not_own)? {
        Err(refusal) => return Ok(EpicLineBinding::Refused(refusal)),
        Ok(head_value) => head_value,
    };
    let mut bound = false;
    let mut unbound = Vec::new();
    for (anchor, place) in anchors {
        let records = anchor_records(repo_root, anchor)?;
        let binding = match &claim {
            Claim::Declared(own) => declared_binding(
                &records,
                line,
                epic_id,
                own,
                head_value.as_deref(),
                place,
                &not_own,
            ),
            Claim::Line => bound_in(&records, line, epic_id, place),
        };
        match binding {
            EpicLineBinding::Bound => bound = true,
            EpicLineBinding::Refused(reason) => return Ok(EpicLineBinding::Refused(reason)),
            EpicLineBinding::Unbound(reason) => unbound.push(reason),
        }
    }
    Ok(if bound {
        EpicLineBinding::Bound
    } else {
        EpicLineBinding::Unbound(unbound.join("; "))
    })
}

/// The `integration_target` the task record at `path` declares.
pub(crate) fn declared_work_target_at(path: &Path) -> Result<Option<String>, WorkStartError> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        WorkStartError::InvalidGraph(format!("cannot read {}: {error}", path.display()))
    })?;
    let record = parse_record(&content, RecordKind::Task).map_err(WorkStartError::InvalidGraph)?;
    Ok(record
        .integration_target
        .filter(|target| !target.is_empty()))
}

/// Branch prefixes that never carry a task id: planning branches create
/// records and integration branches carry many tasks.
const NON_WORK_PREFIXES: [&str; 2] = ["plan/", "integration/"];

/// The part of `branch` after a sanctioned work prefix: any policy branch
/// prefix except `plan/` and `integration/` (SPC-013 R-110).
fn work_branch_suffix<'b>(
    repo_root: &Path,
    branch: &'b str,
) -> Result<Option<&'b str>, WorkStartError> {
    Ok(work_suffix(&work_prefixes(repo_root)?, branch))
}

/// The sanctioned work prefixes of the project's policy: every
/// `git.branch_prefixes` entry except `plan/` and `integration/`.
pub(crate) fn work_prefixes(repo_root: &Path) -> Result<Vec<String>, WorkStartError> {
    let (policy, _) = crate::hooks::policy::Policy::load_effective(repo_root)
        .map_err(WorkStartError::InvalidGraph)?;
    Ok(policy
        .git
        .branch_prefixes
        .into_iter()
        .filter(|prefix| !NON_WORK_PREFIXES.contains(&prefix.as_str()))
        .collect())
}

/// The part of `branch` after one of `prefixes`.
pub(crate) fn work_suffix<'b>(prefixes: &[String], branch: &'b str) -> Option<&'b str> {
    prefixes
        .iter()
        .find_map(|prefix| branch.strip_prefix(prefix.as_str()))
}

/// [`work_suffix`] over exact bytes: the part of `branch` after one of
/// `prefixes`, whatever the rest of the name holds.
pub(crate) fn work_suffix_name(prefixes: &[String], branch: &GitName) -> Option<GitName> {
    prefixes
        .iter()
        .find_map(|prefix| branch.strip_prefix(prefix.as_bytes()))
}

/// The longest of `ids` that `suffix` starts with, followed by `-`, read from
/// bytes: `task/TSK-1-caf\xe9` still claims `TSK-1`.
pub(crate) fn carried_task_id_name(
    suffix: &GitName,
    ids: &std::collections::BTreeSet<String>,
) -> Option<String> {
    ids.iter()
        .filter(|id| suffix.starts_with(format!("{id}-").as_bytes()))
        .max_by_key(|id| id.len())
        .cloned()
}

/// Resolve the durable task id a work branch carries,
/// `<prefix>/TSK-NNN-<slug>` on any sanctioned work prefix (`task/`, `fix/`,
/// `feat/`, `spike/` and the rest of `git.branch_prefixes`).
///
/// The visible workgraph is the source of truth, which avoids baking legacy
/// or future id shapes into branch parsing.
///
/// # Errors
///
/// Returns an error if work prefixes or the task inventory cannot be read.
pub fn task_id_from_branch(
    repo_root: &Path,
    branch: &str,
) -> Result<Option<String>, WorkStartError> {
    let Some(suffix) = work_branch_suffix(repo_root, branch)? else {
        return Ok(None);
    };
    Ok(carried_task_id(suffix, &visible_task_ids(repo_root)?))
}

/// The task ids of the record files visible in the checkout.
fn visible_task_ids(
    repo_root: &Path,
) -> Result<std::collections::BTreeSet<String>, WorkStartError> {
    Ok(
        crate::workgraph::layout::task_record_files(&repo_root.join("project-management"))
            .map_err(|error| WorkStartError::InvalidGraph(error.to_string()))?
            .into_iter()
            .filter_map(|path| {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .filter(|id| is_valid_task_format_id(id))
            .collect(),
    )
}

/// The longest of `ids` that `suffix` starts with, followed by `-`.
fn carried_task_id(suffix: &str, ids: &std::collections::BTreeSet<String>) -> Option<String> {
    suffix
        .match_indices('-')
        .map(|(end, _)| &suffix[..end])
        .filter(|candidate| ids.contains(*candidate))
        .max_by_key(|candidate| candidate.len())
        .map(str::to_owned)
}

/// The task records one graph holds, by id with their paths, or why that
/// graph cannot be read.
type TaskPaths = Result<BTreeMap<String, String>, String>;

/// The visible work branches, local and remote-tracking, grouped by the task
/// id each carries: read once, so a caller judging many pins or tasks never
/// walks the refs and the record files again per branch.
pub(crate) struct PinBranches {
    /// Task id, then branch name without its remote, then the tips it has.
    tips: BTreeMap<String, BTreeMap<GitName, std::collections::BTreeSet<Oid>>>,
    /// A branch of the task whose tip could not be read.
    unreadable: BTreeMap<String, String>,
    /// The task records the graph at a revision holds, by id with their
    /// paths, parsed once per revision however many pins name it and
    /// whichever check asks.
    task_paths: std::cell::RefCell<std::collections::HashMap<Oid, TaskPaths>>,
}

impl PinBranches {
    /// Read every branch of `repo`, resolving task ids from `root`'s policy
    /// and record files.
    ///
    /// # Errors
    /// Returns the reason the branches cannot be listed.
    pub(crate) fn read(repo: &Repository, root: &Path) -> Result<Self, String> {
        let prefixes = work_prefixes(root).map_err(|error| error.to_string())?;
        let ids = visible_task_ids(root).map_err(|error| error.to_string())?;
        let mut tips =
            BTreeMap::<String, BTreeMap<GitName, std::collections::BTreeSet<Oid>>>::new();
        let mut unreadable = BTreeMap::new();
        let remotes = crate::git::name::remote_names(repo).map_err(|e| e.to_string())?;
        for branch in repo.branches(None).map_err(|e| e.to_string())? {
            let (branch, kind) = branch.map_err(|e| e.to_string())?;
            // OS text rule (issue 79): the name is exact bytes, matched
            // against work prefixes and task ids by prefix, so a branch that
            // is not valid UTF-8 neither fails the listing nor hides a claim
            // on its task.
            let name = crate::git::name::branch_name(&branch).map_err(|e| e.to_string())?;
            let name = if kind == git2::BranchType::Remote {
                // `<remote>/<branch>`: a remote name may hold `/`, so the
                // configured remotes decide where the branch starts.
                crate::git::tracking_branch(&name, &remotes).unwrap_or(name)
            } else {
                name
            };
            let Some(task_id) = work_suffix_name(&prefixes, &name)
                .and_then(|suffix| carried_task_id_name(&suffix, &ids))
            else {
                continue;
            };
            match branch.get().peel_to_commit() {
                Ok(commit) => {
                    tips.entry(task_id)
                        .or_default()
                        .entry(name)
                        .or_default()
                        .insert(commit.id());
                }
                Err(error) => {
                    unreadable
                        .entry(task_id)
                        .or_insert_with(|| error.to_string());
                }
            }
        }
        Ok(Self {
            tips,
            unreadable,
            task_paths: std::cell::RefCell::default(),
        })
    }

    /// The branches in `slot`, read from `repo`'s working tree on first use.
    ///
    /// # Errors
    /// Returns the reason the working tree or its branches cannot be read.
    pub(crate) fn once<'s>(
        slot: &'s mut Option<Self>,
        repo: &Repository,
    ) -> Result<&'s Self, String> {
        if slot.is_none() {
            let root = repo
                .workdir()
                .ok_or_else(|| "no working tree".to_string())?;
            *slot = Some(Self::read(repo, root)?);
        }
        slot.as_ref().ok_or_else(|| "no working tree".to_string())
    }

    /// Each branch carrying `task_id`, with its tips.
    fn of(
        &self,
        task_id: &str,
    ) -> Result<impl Iterator<Item = (&GitName, &std::collections::BTreeSet<Oid>)>, String> {
        if let Some(error) = self.unreadable.get(task_id) {
            return Err(error.clone());
        }
        Ok(self.tips.get(task_id).into_iter().flatten())
    }

    /// Every tip of a branch carrying `task_id`.
    pub(crate) fn tips_of(&self, task_id: &str) -> Result<std::collections::BTreeSet<Oid>, String> {
        Ok(self
            .of(task_id)?
            .flat_map(|(_, tips)| tips.iter().copied())
            .collect())
    }

    /// The one branch whose tip is the pin, the branch not advanced past it.
    ///
    /// Each copy of that branch (the local branch and its remote-tracking
    /// refs) is judged against the pin by ancestry alone (TSK-416): a copy
    /// behind the pin, a strict ancestor of it, is a stale copy and does not
    /// refuse; a copy ahead of the pin or diverged from it refuses, naming
    /// the branch, the pin, the copy and its shape; ancestry that cannot be
    /// read refuses with that cause.
    ///
    /// # Errors
    /// Refuses a pin that is not the unique predecessor branch tip, a copy
    /// ahead of or diverged from it, and unreadable ancestry.
    pub(crate) fn pin_name(&self, repo: &Repository, pin: &ReviewedPin) -> Result<GitName, String> {
        let visible: Vec<_> = self.of(&pin.task_id)?.collect();
        let heads = visible
            .iter()
            .flat_map(|(name, tips)| {
                tips.iter()
                    .map(move |tip| format!("{}@{tip}", name.display()))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let matching: Vec<_> = visible
            .iter()
            .copied()
            .filter(|(_, tips)| tips.contains(&pin.revision))
            .collect();
        let [(name, tips)] = matching.as_slice() else {
            // Issue 603: every visible copy behind the pin means the
            // reviewed head is not pushed or fetched yet, not that the
            // branch advanced.
            if matching.is_empty() && !visible.is_empty() {
                let mut behind = true;
                for tip in visible.iter().flat_map(|(_, tips)| tips.iter()) {
                    behind &= history::descendant_of(repo, pin.revision, *tip).map_err(|error| {
                        format!(
                            "{} predecessor branch: cannot read the ancestry of its copy at {tip} against the pin {}: {}; fetch the branch again and recheck",
                            pin.task_id,
                            pin.revision,
                            error.message()
                        )
                    })?;
                }
                if behind {
                    return Err(format!("{} predecessor branch is behind the pin {}; visible heads: {heads}; push the reviewed head to the predecessor's branch or wait for its push, then fetch and recheck", pin.task_id, pin.revision));
                }
            }
            return Err(format!("{} pin is not the unique predecessor branch tip; visible heads: {heads}; rebase on its new reviewed head and recheck", pin.task_id));
        };
        for other in tips.iter().filter(|tip| **tip != pin.revision) {
            let unreadable = |error: git2::Error| {
                format!(
                    "{} predecessor branch {}: cannot read the ancestry of its copy at {other} against the pin {}: {}; fetch the branch again and recheck",
                    pin.task_id,
                    name.display(),
                    pin.revision,
                    error.message()
                )
            };
            if history::descendant_of(repo, pin.revision, *other).map_err(unreadable)? {
                continue;
            }
            let shape = if history::descendant_of(repo, *other, pin.revision).map_err(unreadable)? {
                "is ahead of"
            } else {
                "has diverged from"
            };
            return Err(format!(
                "{} predecessor branch {} has a copy at {other} that {shape} the pin {}; visible heads: {heads}; rebase on its new reviewed head and recheck, or delete or reset the stray copy",
                pin.task_id,
                name.display(),
                pin.revision
            ));
        }
        Ok((*name).clone())
    }

    /// Whether the pinned revision holds the pinned task's record, the
    /// revision's graph read once.
    ///
    /// # Errors
    /// Refuses a pin whose tree cannot be read or lacks the record.
    pub(crate) fn pin_holds_record(
        &self,
        repo: &Repository,
        pin: &ReviewedPin,
    ) -> Result<(), String> {
        if self.record_path(repo, pin)?.is_none() {
            return Err(format!(
                "{} pin does not hold that task's record",
                pin.task_id
            ));
        }
        Ok(())
    }

    /// The path of the pinned task's record in the graph at the pin, `None`
    /// when that graph holds no such task. The graph is parsed once per
    /// revision for every pin and check that asks (R-103).
    ///
    /// # Errors
    /// Returns why the graph at the pin cannot be read.
    pub(crate) fn record_path(
        &self,
        repo: &Repository,
        pin: &ReviewedPin,
    ) -> Result<Option<String>, String> {
        self.task_paths
            .borrow_mut()
            .entry(pin.revision)
            .or_insert_with(|| {
                super::lifecycle::Graph::from_revision(repo, &pin.revision.to_string()).map(
                    |graph| {
                        graph
                            .records
                            .into_iter()
                            .filter(|(_, record)| record.kind == RecordKind::Task)
                            .map(|(id, record)| (id, record.path))
                            .collect()
                    },
                )
            })
            .as_ref()
            .map_err(Clone::clone)
            .map(|paths| paths.get(&pin.task_id).cloned())
    }

    /// The branch a reviewed pin names: [`Self::pin_name`], then
    /// [`Self::pin_holds_record`].
    ///
    /// # Errors
    /// Refuses as those two do.
    pub(crate) fn pin_branch(
        &self,
        repo: &Repository,
        pin: &ReviewedPin,
    ) -> Result<GitName, String> {
        let name = self.pin_name(repo, pin)?;
        self.pin_holds_record(repo, pin)?;
        Ok(name)
    }
}

/// The task id a work branch carries, read from the records at `head`
/// rather than the checkout: CI judges a pull request head from a base
/// checkout, where the head's own standalone record is not on disk.
///
/// # Errors
///
/// Returns an error if work prefixes, the revision or its task records cannot be read.
pub fn task_id_from_branch_at(
    repo_root: &Path,
    branch: &str,
    head: &str,
) -> Result<Option<String>, WorkStartError> {
    let Some(suffix) = work_branch_suffix(repo_root, branch)? else {
        return Ok(None);
    };
    task_id_in_head(repo_root, suffix, head)
}

/// The task id a work branch carries, judged as `codeflow ci` judges a
/// range (TSK-350 AC-1, AC-2): the work prefixes come from `prefixes`, the
/// `git.branch_prefixes` of the policy the run judges with, and the id from
/// the task records at the judged `head`. The checkout supplies neither, so
/// a record only the checkout holds grants no own-task authority, and one
/// the checkout lacks takes none away.
///
/// # Errors
///
/// Returns an error if the revision or its task records cannot be read.
pub fn judged_task_id(
    repo_root: &Path,
    prefixes: &[String],
    branch: &str,
    head: &str,
) -> Result<Option<String>, WorkStartError> {
    let prefixes: Vec<String> = prefixes
        .iter()
        .filter(|prefix| !NON_WORK_PREFIXES.contains(&prefix.as_str()))
        .cloned()
        .collect();
    let Some(suffix) = work_suffix(&prefixes, branch) else {
        return Ok(None);
    };
    task_id_in_head(repo_root, suffix, head)
}

/// The longest task id at `head` that `suffix` starts with, followed by `-`.
fn task_id_in_head(
    repo_root: &Path,
    suffix: &str,
    head: &str,
) -> Result<Option<String>, WorkStartError> {
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let tree = repo
        .revparse_single(head)
        .and_then(|object| object.peel_to_commit())
        .and_then(|commit| commit.tree())
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    // The task record files at `head`, by name as the checkout lookup reads
    // them: no record is parsed, so another record that does not parse
    // keeps the refusal that names it rather than this lookup's.
    let ids = std::cell::RefCell::new(std::collections::BTreeSet::new());
    records_from_tree_matching(&repo, &tree, |path, kind| {
        if kind == RecordKind::Task {
            if let Some(id) = path
                .rsplit('/')
                .next()
                .and_then(markdown_stem)
                .filter(|id| is_valid_task_format_id(id))
            {
                ids.borrow_mut().insert(id.to_owned());
            }
        }
        false
    })?;
    Ok(carried_task_id(suffix, &ids.into_inner()))
}

/// Whether a work branch names a task id by shape (`<prefix>/TSK-<digits>-`),
/// whether or not a record for it is visible. A branch that claims an id the
/// workgraph does not hold is refused rather than treated as untracked.
///
/// # Errors
///
/// Returns an error if work prefixes or the task inventory cannot be read.
pub fn branch_claims_task_id(repo_root: &Path, branch: &str) -> Result<bool, WorkStartError> {
    Ok(
        work_branch_suffix(repo_root, branch)?.is_some_and(|suffix| {
            suffix
                .strip_prefix("TSK-")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        }),
    )
}

/// A reviewed predecessor pin. The lookup names its exact branch and commit;
/// matching this evidence is structural, not authentication of the verdict.
/// Only [`reviewed_pins`] and [`stacked_pins`] build one, after a review row
/// named it, so a branch tip or any other movable ref cannot stand in for
/// it (issue #69).
#[derive(Debug, Clone)]
pub struct ReviewedPin {
    task_id: String,
    revision: git2::Oid,
}

impl ReviewedPin {
    /// The predecessor task the pin is for.
    #[must_use]
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// The reviewed head the pin names.
    #[must_use]
    pub fn revision(&self) -> git2::Oid {
        self.revision
    }
}

/// Whether a revision an approving review row names covers a pin: the pin
/// itself, or an ancestor the pin follows only by changes to the pinned
/// record's status and Closeout. A lookup calls it only once it has found the
/// pull request with the pin as its head, so a lookup that finds none
/// never reads the predecessor's history.
pub type ReviewCovers<'a> = dyn Fn(&str) -> Result<bool, String> + 'a;

/// How a pin's review is looked up: the predecessor's branch, the pin (its
/// tip), and [`ReviewCovers`] for that pin. It answers whether the pull
/// request of that branch has the pin as its head and an approving review
/// row naming a revision that covers it.
pub type ReviewLookup<'a> = dyn Fn(&str, &str, &ReviewCovers<'_>) -> Result<bool, String> + 'a;

/// Resolve explicit pins against predecessor branches and review evidence.
///
/// # Errors
/// Refuses malformed, repeated, unknown, advanced or unreviewed pins.
pub fn reviewed_pins(
    root: &Path,
    values: &[String],
    lookup: &ReviewLookup<'_>,
) -> Result<Vec<ReviewedPin>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    let mut branches = None;
    reviewed_pins_in(&repo, &mut branches, values, lookup, false)
}

/// [`reviewed_pins`] over branches read at most once. `review_first` asks
/// for review evidence before the pinned tree is read, for a caller that
/// needs only whether every pin holds, not which refusal comes first.
pub(crate) fn reviewed_pins_in(
    repo: &Repository,
    branches: &mut Option<PinBranches>,
    values: &[String],
    lookup: &ReviewLookup<'_>,
    review_first: bool,
) -> Result<Vec<ReviewedPin>, String> {
    let mut pins = Vec::new();
    for value in values {
        let (task_id, sha) = value
            .split_once('@')
            .ok_or_else(|| "--on requires TSK-NNN@<full reviewed sha>".to_string())?;
        if !is_valid_task_format_id(task_id)
            || sha.len() != 40
            || !sha.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("--on requires TSK-NNN@<full reviewed sha>".into());
        }
        if pins.iter().any(|pin: &ReviewedPin| pin.task_id == task_id) {
            return Err(format!("duplicate --on pin for {task_id}"));
        }
        let revision = git2::Oid::from_str(sha).map_err(|e| e.to_string())?;
        let pin = ReviewedPin {
            task_id: task_id.into(),
            revision,
        };
        let branches = PinBranches::once(branches, repo)?;
        let branch = branches.pin_name(repo, &pin)?;
        if !review_first {
            branches.pin_holds_record(repo, &pin)?;
        }
        // The hosted review is found by branch name, which needs text.
        let branch_text = branch.rule_text().map_err(|error| error.to_string())?;
        // Read only when a pull request with this head is found. A record
        // path that cannot be read covers only the pin itself, the strict side.
        let covers = |reviewed: &str| match branches.record_path(repo, &pin) {
            Ok(Some(path)) => review_covers_pin(repo, &pin, &path, reviewed),
            _ => Ok(resolve_reviewed(repo, reviewed) == Some(pin.revision)),
        };
        if !lookup(branch_text, &revision.to_string(), &covers)? {
            return Err(format!(
                "no review names {task_id}@{sha}, or a commit it follows only by {task_id}'s status and Closeout; cannot verify review for this pin"
            ));
        }
        if review_first {
            branches.pin_holds_record(repo, &pin)?;
        }
        pins.push(pin);
    }
    Ok(pins)
}

/// The commit a review row names by `reviewed`, a full revision or a
/// uniquely resolved hexadecimal abbreviation of at least 7 characters;
/// `None` when it names none or several.
fn resolve_reviewed(repo: &Repository, reviewed: &str) -> Option<Oid> {
    if !(7..=40).contains(&reviewed.len()) || !reviewed.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    repo.find_commit_by_prefix(reviewed)
        .ok()
        .map(|commit| commit.id())
}

/// Whether a review that approves `reviewed` covers `pin` (SPC-013 R-42,
/// issue #69, TSK-250): `reviewed` resolves to the pin, or to an ancestor
/// of it that the pin follows only by single-parent commits that change
/// the pinned task's own record and nothing in it but its status and
/// Closeout. A completion recorded after the review, as cf-ship asks, so
/// needs no second review before a successor stacks on it. Each commit in
/// the span is judged against its parent, so a change followed by its
/// revert still refuses. A merge in the span carries no review: the
/// predecessor's own acceptance binding (R-60) accepts a merge after its
/// reviewed commit only as a clean re-merge of its target, which changes
/// other files and so never passes this check either, and a pin that
/// binding refuses would pass `work claim` and then fail its own push. A
/// root commit in the span refuses, since the record appears in it
/// unreviewed. A history overlay (grafts or replace refs) could fake the
/// parents walked, and a shallow boundary cuts the walk, so either stops
/// it, as in the acceptance binding's own walk. `path` is the pinned
/// record's path at the pin ([`PinBranches::record_path`]).
///
/// # Errors
/// Returns why the ancestry or the history walk between the review and the
/// pin cannot be read or trusted. A record or tree that cannot be read
/// inside the span is no proof of coverage and returns `Ok(false)`.
pub(crate) fn review_covers_pin(
    repo: &Repository,
    pin: &ReviewedPin,
    path: &str,
    reviewed: &str,
) -> Result<bool, String> {
    let Some(review) = resolve_reviewed(repo, reviewed) else {
        return Ok(false);
    };
    if review == pin.revision {
        return Ok(true);
    }
    if !history::descendant_of(repo, pin.revision, review).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    let mut walk = history::revwalk(repo).map_err(|e| e.to_string())?;
    walk.push(pin.revision).map_err(|e| e.to_string())?;
    walk.hide(review).map_err(|e| e.to_string())?;
    for revision in walk {
        let revision = revision.map_err(|e| e.to_string())?;
        let commit = repo.find_commit(revision).map_err(|e| e.to_string())?;
        // An unreadable record is an error, never a covered revision.
        let Some(content) = super::acceptance::blob_at(repo, revision, path)? else {
            return Ok(false);
        };
        let Ok(parent) = history::parent_id(repo, &commit, 0) else {
            return Ok(false);
        };
        if history::parents(repo, &commit)
            .map_err(|error| error.to_string())?
            .len()
            != 1
            || super::acceptance::later_change(repo, path, &content, parent, revision).is_some()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn pin_branch(repo: &Repository, pin: &ReviewedPin) -> Result<GitName, String> {
    PinBranches::once(&mut None, repo)?.pin_branch(repo, pin)
}

/// The prospective base is the reviewed pin that contains all other pins.
///
/// # Errors
/// Refuses advanced or incomparable pins before a claim creates anything.
pub fn stack_base(root: &Path, pins: &[ReviewedPin]) -> Result<Option<git2::Oid>, String> {
    let repo = Repository::discover(root).map_err(|e| e.to_string())?;
    stack_base_in(&repo, &mut None, pins)
}

/// [`stack_base`] over branches read at most once.
pub(crate) fn stack_base_in(
    repo: &Repository,
    branches: &mut Option<PinBranches>,
    pins: &[ReviewedPin],
) -> Result<Option<git2::Oid>, String> {
    history::check(repo).map_err(|error| error.to_string())?;
    for pin in pins {
        PinBranches::once(branches, repo)?.pin_branch(repo, pin)?;
    }
    if pins.is_empty() {
        return Ok(None);
    }
    for candidate in pins {
        let mut contains_all = true;
        for pin in pins {
            if candidate.revision != pin.revision
                && !history::descendant_of(repo, candidate.revision, pin.revision)
                    .map_err(|error| error.to_string())?
            {
                contains_all = false;
                break;
            }
        }
        if contains_all {
            return Ok(Some(candidate.revision));
        }
    }
    Err("no reviewed base contains all pins; land or stack one first".into())
}

/// Start a branch already stacked on the supplied reviewed predecessors.
///
/// # Errors
/// Refuses pins outside HEAD and every ordinary structural/start failure.
pub fn check_work_start_on(
    root: &Path,
    task_id: &str,
    target: &str,
    pins: &[ReviewedPin],
) -> Result<WorkStartReport, WorkStartError> {
    let _judgment = history::judgment();
    let repo = Repository::discover(root).map_err(|e| WorkStartError::Repository(e.to_string()))?;
    let head = repo
        .head()
        .map_err(|e| WorkStartError::Repository(e.to_string()))?;
    let branch = head_branch_text(&head, task_id)?;
    if task_id_from_branch(root, &branch)?.as_deref() != Some(task_id) {
        return Err(WorkStartError::Branch {
            branch: branch.clone(),
            task_id: task_id.into(),
        });
    }
    let head = head
        .peel_to_commit()
        .map_err(|e| WorkStartError::Repository(e.to_string()))?
        .id();
    for pin in pins {
        pin_branch(&repo, pin).map_err(WorkStartError::InvalidGraph)?;
        if pin.revision != head
            && !history::descendant_of(&repo, head, pin.revision)
                .map_err(|error| WorkStartError::Repository(error.to_string()))?
        {
            return Err(WorkStartError::InvalidGraph(format!(
                "{} pin is not an ancestor of HEAD",
                pin.task_id
            )));
        }
    }
    // The report names the branch whose identity was checked, as the plain
    // start does, so the caller can tell it from other carriers.
    let base_note = super::claim_base::check(root, task_id, &branch, target, pins)
        .map_err(WorkStartError::Base)?;
    let mut report =
        check_task_anchor(root, task_id, target, &branch, Admission::Start, pins, None)?;
    report.base_note = base_note;
    report.branch = branch;
    Ok(report)
}

/// The checked-out branch as text for the work-start rules.
///
/// OS text rule (issue 79): the rules match the branch against sanctioned work
/// prefixes and the task id, which need text. A branch that is not valid UTF-8
/// is refused as not carrying the task, with its escaped name shown, instead
/// of being matched on a lossy spelling.
fn head_branch_text(head: &git2::Reference<'_>, task_id: &str) -> Result<String, WorkStartError> {
    let name = crate::git::name::reference_shorthand(head);
    match name.rule_text() {
        Ok(text) => Ok(text.to_string()),
        Err(_) => Err(WorkStartError::Branch {
            branch: name.display().to_string(),
            task_id: task_id.to_string(),
        }),
    }
}

/// Validate that `task_id` is safe to begin on the current branch.
///
/// # Errors
///
/// Returns a specific error for branch identity, target resolution, missing
/// planning anchors, invalid relationships, incomplete dependencies, draft
/// specs, or closed tasks.
pub fn check_work_start(
    repo_root: &Path,
    task_id: &str,
    target: &str,
) -> Result<WorkStartReport, WorkStartError> {
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let head = repo
        .head()
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let branch = head_branch_text(&head, task_id)?;
    check_work_start_for_branch(repo_root, task_id, target, &branch)
}

/// Validate a durable task against an explicit branch identity.
///
/// CI commonly checks out a detached commit, so its trusted CI branch name is
/// supplied explicitly while all Git history and workgraph checks remain
/// identical to the interactive preflight.
///
/// # Errors
///
/// Returns a specific error for branch identity, target resolution, missing
/// planning anchors, invalid relationships, incomplete dependencies, draft
/// specs, or closed tasks.
pub fn check_work_start_for_branch(
    repo_root: &Path,
    task_id: &str,
    target: &str,
    branch: &str,
) -> Result<WorkStartReport, WorkStartError> {
    if !is_valid_task_format_id(task_id) {
        return Err(WorkStartError::InvalidGraph(format!(
            "task id '{task_id}' is malformed"
        )));
    }
    let expected = format!("{task_id}-");
    if !work_branch_suffix(repo_root, branch)?.is_some_and(|suffix| suffix.starts_with(&expected)) {
        return Err(WorkStartError::Branch {
            branch: branch.to_string(),
            task_id: task_id.to_string(),
        });
    }
    let mut report = check_work_start_anchored(repo_root, task_id, target)?;
    report.branch = branch.to_string();
    Ok(report)
}

/// Validate a durable task at the merge-base of `HEAD` and `target`, whatever
/// the branch is called: the pull request context of SPC-013 R-110, where a
/// `Task: TSK-NNN` line names the task (a `fix/` pull request naming the
/// release task, for example). The returned report's `branch` is empty.
///
/// # Errors
///
/// Returns the same errors as [`check_work_start_for_branch`] apart from
/// branch identity.
pub fn check_work_start_anchored(
    repo_root: &Path,
    task_id: &str,
    target: &str,
) -> Result<WorkStartReport, WorkStartError> {
    let repo =
        Repository::discover(repo_root).map_err(|e| WorkStartError::Repository(e.to_string()))?;
    let branch = repo
        .head()
        .ok()
        .map(|head| head_branch_text(&head, task_id))
        .transpose()?
        .unwrap_or_default();
    check_task_anchor(
        repo_root,
        task_id,
        target,
        &branch,
        Admission::Start,
        &[],
        None,
    )
}

/// CI admission shares structural readiness but does not try to start a closed task.
/// A new standalone record must already exist in the revision its review names.
/// `head` is the pull request head CI judges; the hosted workflows check out
/// the base and pass the head as data, so the records are read from that
/// revision, never from the checkout.
///
/// # Errors
/// Returns an identity, anchor or structural readiness refusal.
pub fn check_work_admission(
    repo_root: &Path,
    task_id: &str,
    target: &str,
    branch: &str,
    head: &str,
) -> Result<WorkStartReport, WorkStartError> {
    check_task_anchor(
        repo_root,
        task_id,
        target,
        branch,
        Admission::Work,
        &[],
        Some(head),
    )
}

/// [`check_work_admission`] for a push of a branch stacked on reviewed
/// predecessor heads ([`stacked_pins`]): each pinned code dependency is met
/// by its pin, as `work claim --on` and `work start --on` meet it, instead
/// of by its status at the merge-base (SPC-013 R-42, issue #69).
///
/// # Errors
/// Returns an identity, anchor or structural readiness refusal.
pub fn check_work_admission_on(
    repo_root: &Path,
    task_id: &str,
    target: &str,
    branch: &str,
    head: &str,
    pins: &[ReviewedPin],
) -> Result<WorkStartReport, WorkStartError> {
    let _judgment = history::judgment();
    check_task_anchor(
        repo_root,
        task_id,
        target,
        branch,
        Admission::Work,
        pins,
        Some(head),
    )
}

/// The reviewed predecessor heads a push of `task_id`'s branch stacks on
/// (SPC-013 R-42, issue #69). For each code dependency that is not
/// complete at the merge-base of `head` with `target`, the one visible
/// branch tip of that predecessor that `head` contains and the merge-base
/// does not is its candidate pin. A candidate is honoured only as `work
/// claim --on` honours it ([`reviewed_pins`]): the unique tip of the
/// predecessor's branch, holding its record, with a review row naming it
/// or a commit it follows only by the predecessor's status and Closeout.
/// Branch tips only nominate; the review row decides. Returns the honoured
/// pins, none when nothing is stacked.
///
/// # Errors
/// Returns why a candidate is not honoured, or why the range cannot be read.
pub fn stacked_pins(
    repo_root: &Path,
    task_id: &str,
    target: &str,
    head: &str,
    lookup: &ReviewLookup<'_>,
) -> Result<Vec<ReviewedPin>, String> {
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let head_id = head_commit(&repo, Some(head)).map_err(|error| error.to_string())?;
    let (merge_base, mut records) =
        anchored_records(&repo, target, head_id).map_err(|error| error.to_string())?;
    let merge_base = Oid::from_str(&merge_base).map_err(|error| error.to_string())?;
    if !records.contains_key(task_id) {
        // A standalone record new on this branch carries its edges at head.
        let tree = repo
            .find_commit(head_id)
            .and_then(|commit| commit.tree())
            .map_err(|error| error.to_string())?;
        if let Some(task) = records_from_tree(&repo, &tree)
            .map_err(|error| error.to_string())?
            .remove(task_id)
        {
            records.insert(task_id.to_string(), task);
        }
    }
    let Some(task) = records.get(task_id) else {
        return Ok(Vec::new());
    };
    let mut branches = None;
    let mut values = Vec::new();
    for dependency in &task.depends_on {
        if dependency.kind != DependencyKind::Code
            || records
                .get(&dependency.id)
                .is_some_and(|record| record.status == "complete")
        {
            continue;
        }
        // A tip the head contains and the target does not. An ancestry
        // that cannot be read refuses (issue 79), never reads as "no".
        let descends = |of: Oid, ancestor: Oid| {
            history::descendant_of(&repo, of, ancestor).map_err(|error| {
                format!("cannot read the ancestry of {ancestor} from {of}: {error}")
            })
        };
        // Each branch of the predecessor with the tips the head contains
        // and the target does not.
        let mut by_branch: Vec<Vec<Oid>> = Vec::new();
        for (_, tips) in PinBranches::once(&mut branches, &repo)?.of(&dependency.id)? {
            let mut inside = Vec::new();
            for tip in tips {
                if (*tip == head_id || descends(head_id, *tip)?)
                    && *tip != merge_base
                    && !descends(merge_base, *tip)?
                {
                    inside.push(*tip);
                }
            }
            by_branch.push(inside);
        }
        // A copy of one branch that another contained copy of the same
        // branch descends from is a stale copy behind it, never a second
        // stack (TSK-416); `pin_name` then judges every copy against the pin.
        let mut contained = std::collections::BTreeSet::new();
        for inside in &by_branch {
            for tip in inside {
                let mut behind = false;
                for other in inside.iter().filter(|other| *other != tip) {
                    if descends(*other, *tip)? {
                        behind = true;
                        break;
                    }
                }
                if !behind {
                    contained.insert(*tip);
                }
            }
        }
        match contained.len() {
            0 => {}
            1 => values.push(format!(
                "{}@{}",
                dependency.id,
                contained.first().copied().unwrap_or(merge_base)
            )),
            _ => {
                return Err(format!(
                "this branch contains several tips of {}'s branches; stack on one reviewed head",
                dependency.id
            ))
            }
        }
    }
    if values.is_empty() {
        return Ok(Vec::new());
    }
    let pins = reviewed_pins_in(&repo, &mut branches, &values, lookup, false)?;
    stack_base_in(&repo, &mut branches, &pins)?;
    Ok(pins)
}

/// CI admission for a validated secret-scan exemption class. It keeps the
/// task's identity, planning anchor, graph, specs and start gate, but does not
/// require its dependencies to be ready before the exemption reaches the base.
/// The caller must first prove the range changes only `.gitleaksignore` and
/// that the Task line identifies this task.
///
/// # Errors
/// Returns the ordinary admission refusals except dependency readiness.
pub fn check_secret_scan_exemption_admission(
    repo_root: &Path,
    task_id: &str,
    target: &str,
    branch: &str,
    head: &str,
) -> Result<WorkStartReport, WorkStartError> {
    let _judgment = history::judgment();
    check_task_anchor(
        repo_root,
        task_id,
        target,
        branch,
        Admission::SecretScanExemption,
        &[],
        Some(head),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Admission {
    Start,
    Work,
    SecretScanExemption,
}

fn check_task_anchor(
    repo_root: &Path,
    task_id: &str,
    target: &str,
    branch: &str,
    admission: Admission,
    pins: &[ReviewedPin],
    head: Option<&str>,
) -> Result<WorkStartReport, WorkStartError> {
    if !is_valid_task_format_id(task_id) {
        return Err(WorkStartError::InvalidGraph(format!(
            "task id '{task_id}' is malformed"
        )));
    }
    if !is_stable_work_target(target) {
        return Err(WorkStartError::UnstableTarget(target.to_string()));
    }
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let head_id = head_commit(&repo, head)?;
    let (merge_base, mut records) = anchored_records(&repo, target, head_id)?;
    let at_base = records.contains_key(task_id);
    standalone_at_head(
        &repo,
        &mut records,
        task_id,
        branch,
        admission != Admission::Start,
        head_id,
    )?;
    // A named fix may reopen a complete task in this range (TSK-140). Only
    // its status is projected onto the anchored readiness core; every
    // planning input and dependency still comes from the merge-base. The
    // reopened record is read where the check reads the change: at the
    // named head (CI admission), else in the checkout.
    if records
        .get(task_id)
        .is_some_and(|task| task.status == "complete")
    {
        let base = super::lifecycle::Graph::from_revision(&repo, &merge_base)
            .map_err(WorkStartError::InvalidGraph)?;
        let current = match head {
            Some(_) => super::lifecycle::Graph::from_revision(&repo, &head_id.to_string())
                .map_err(WorkStartError::InvalidGraph)?,
            None => super::lifecycle::Graph::from_worktree(repo_root)
                .map_err(WorkStartError::InvalidGraph)?,
        };
        if let (Some(old), Some(now)) = (base.records.get(task_id), current.records.get(task_id)) {
            if now.status == "todo" || super::lifecycle::is_recompletion(Some(old), now) {
                let problems = super::lifecycle::reopen_problems(Some(old), now);
                if !problems.is_empty() {
                    return Err(WorkStartError::InvalidGraph(problems.join("; ")));
                }
                if let Some(task) = records.get_mut(task_id) {
                    task.status = "todo".into();
                }
            }
        }
    }
    // A task blocked at the merge-base may be unblocked in its own range
    // (TSK-1036, issue #365): only its status is projected, as a reopen is,
    // and only when the range proves the unblock step by step. Every other
    // planning input and every dependency still comes from the merge-base,
    // so an unblock never admits work the graph does not.
    if at_base
        && records
            .get(task_id)
            .is_some_and(|task| task.status == "blocked" || task.blocker_reason.is_some())
        && unblocked_in_range(&repo, repo_root, &merge_base, head, head_id, task_id)?
    {
        if let Some(task) = records.get_mut(task_id) {
            task.status = "todo".into();
            task.blocker_reason = None;
        }
    }
    super::claim_base::project_dependencies(
        &repo,
        &mut records,
        task_id,
        Oid::from_str(&merge_base).map_err(|e| WorkStartError::Repository(e.to_string()))?,
        head_id,
        pins,
    )
    .map_err(WorkStartError::Repository)?;
    own_pin_edges(&repo, &mut records, task_id, head_id, pins)?;
    let anchored = validate_task_structure(
        &repo,
        &records,
        task_id,
        target,
        pins,
        &mut None,
        admission != Admission::SecretScanExemption,
    )?;
    // The start gate judges the record as the merge base has it, with a
    // reopen or an unblock in this range projected above: a task complete
    // or cancelled there takes no further change. A standalone record arriving with its
    // code (admission) has no base record; `standalone_at_head` judged it.
    if admission == Admission::Start || at_base {
        start_gate(&records[task_id], task_id)?;
    }

    Ok(WorkStartReport {
        base_note: None,
        task_id: task_id.to_string(),
        branch: String::new(),
        target: target.to_string(),
        merge_base,
        epic_id: anchored.epic_id,
        specs: anchored.specs,
        dependencies: anchored.dependencies,
        work_type: anchored.work_type,
    })
}

/// Read a task's approved planning snapshot, never its working-tree contents.
/// The committed HEAD declaration locates the target; the shared work-start
/// rule then validates the task and dependencies at their merge-base.
///
/// # Errors
/// Rejects malformed ids, unstable targets and unanchored or invalid plans.
pub fn anchored_task_content(repo_root: &Path, task_id: &str) -> Result<String, String> {
    if !super::is_canonical_task_format_id(task_id) {
        return Err("override requires a canonical task id".into());
    }
    let repo = Repository::discover(repo_root).map_err(|e| e.to_string())?;
    let head = repo
        .head()
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| e.to_string())?;
    let path = format!("project-management/tasks/{task_id}.md");
    let read = |tree: &git2::Tree<'_>| -> Result<String, String> {
        let entry = tree
            .get_path(Path::new(&path))
            .map_err(|_| format!("task {task_id} not committed"))?;
        if entry.filemode() != 0o100_644 && entry.filemode() != 0o100_755 {
            return Err("task record must be a regular file".into());
        }
        let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
        String::from_utf8(blob.content().to_vec()).map_err(|e| e.to_string())
    };
    let content = read(&head.tree().map_err(|e| e.to_string())?)?;
    let declared = parse_record(&content, RecordKind::Task)?;
    let target = declared
        .integration_target
        .ok_or("task has no integration_target")?;
    if !is_stable_work_target(&target) {
        return Err("override target must be a stable non-task branch".into());
    }
    let (base, records) = anchored_records(&repo, &target, head.id()).map_err(|e| e.to_string())?;
    validate_anchored_task(&repo, &records, task_id, &target).map_err(|e| e.to_string())?;
    let commit = repo
        .find_commit(git2::Oid::from_str(&base).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;
    read(&tree)
}

/// A project file as committed on a task's integration target.
#[derive(Debug, Clone)]
pub struct AnchoredFile {
    /// The task's declared integration target.
    pub target: String,
    /// The merge-base of `HEAD` and the target, where the file was read.
    pub base: String,
    /// The file's bytes there; `None` when the target does not carry it.
    pub content: Option<Vec<u8>>,
}

/// Read `path` as committed at the merge-base of `HEAD` and the integration
/// target the task's committed `HEAD` record declares. Neither the working
/// tree nor the task branch's own commits confer what this returns: only a
/// file already on the target does.
///
/// # Errors
/// Rejects malformed ids, an uncommitted task, an unstable or unresolved
/// target, a standalone record the target does not carry whose history
/// declares another target or cannot be read, a non-regular file and a
/// file over 1 MiB.
pub fn anchored_project_file(
    repo_root: &Path,
    task_id: &str,
    path: &str,
) -> Result<AnchoredFile, String> {
    const MAX_BYTES: usize = 1024 * 1024;
    if !is_canonical_task_format_id(task_id) {
        return Err("design authority requires a canonical task id".into());
    }
    let repo = Repository::discover(repo_root).map_err(|e| e.to_string())?;
    let head = repo
        .head()
        .and_then(|r| r.peel_to_commit())
        .map_err(|e| e.to_string())?;
    let record = format!("project-management/tasks/{task_id}.md");
    // Only a record the head does not hold is uncommitted (issue 79): a
    // tree that cannot be read is an error.
    let entry = match head
        .tree()
        .map_err(|e| e.to_string())?
        .get_path(Path::new(&record))
    {
        Ok(entry) => entry,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            return Err(format!("task {task_id} not committed"))
        }
        Err(e) => return Err(format!("cannot read task {task_id} at HEAD: {e}")),
    };
    if entry.filemode() != 0o100_644 && entry.filemode() != 0o100_755 {
        return Err("task record must be a regular file".into());
    }
    let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
    let content = String::from_utf8(blob.content().to_vec()).map_err(|e| e.to_string())?;
    let target = parse_record(&content, RecordKind::Task)?
        .integration_target
        .ok_or("task has no integration_target")?;
    if !is_stable_work_target(&target) {
        return Err("task integration target must be a stable non-task branch".into());
    }
    // The task must pass the planning anchor rule that `codeflow work start`
    // applies on its own task branch: a task on the anchor must declare the
    // target it declares there, so a rewritten target cannot borrow another
    // line's file, and a standalone record arriving with its own pull
    // request is judged at the head. Only the target's file is then read.
    // The branch name is read exactly (issue 79): a head that cannot be read
    // is an error, and a name that is not UTF-8 is refused by its bytes,
    // never read as a lookalike branch.
    let reference = repo.head().map_err(|e| e.to_string())?;
    if !reference.is_branch() {
        return Err("design authority is read only on the task's own branch".into());
    }
    let shorthand = crate::git::name::reference_shorthand(&reference);
    let branch = shorthand
        .rule_text()
        .map_err(|_| {
            format!(
                "design authority is read only on the task's own branch, and {} is not UTF-8",
                shorthand.display()
            )
        })?
        .to_owned();
    let base = check_work_start_for_branch(repo_root, task_id, &target, &branch)
        .map_err(|e| e.to_string())?
        .merge_base;
    let base = Oid::from_str(&base).map_err(|e| e.to_string())?;
    let tree = repo
        .find_commit(base)
        .and_then(|commit| commit.tree())
        .map_err(|e| e.to_string())?;
    // Accepted as a standalone record new at the head: the target does not
    // carry it, so only a record that never declared another target counts.
    // A retarget is not a new arrival.
    // Absence is git's not-found answer only (issue 79): a tree that cannot
    // be read refuses, never reads as a target without the file.
    match tree.get_path(Path::new(&record)) {
        Ok(_) => {}
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            refuse_retarget(&repo, head.id(), &record, task_id, &target)?;
        }
        Err(e) => return Err(format!("cannot read {record} on {target}: {e}")),
    }
    let content = match tree.get_path(Path::new(path)) {
        Err(e) if e.code() == git2::ErrorCode::NotFound => None,
        Err(e) => return Err(format!("cannot read {path} on {target}: {e}")),
        Ok(entry) => {
            if entry.filemode() != 0o100_644 && entry.filemode() != 0o100_755 {
                return Err(format!("{path} on {target} must be a regular file"));
            }
            let blob = repo.find_blob(entry.id()).map_err(|e| e.to_string())?;
            if blob.size() > MAX_BYTES {
                return Err(format!("{path} on {target} exceeds {MAX_BYTES} bytes"));
            }
            Some(blob.content().to_vec())
        }
    };
    Ok(AnchoredFile {
        target,
        base: base.to_string(),
        content,
    })
}

/// The tail of a retarget refusal, which design authority reports even when
/// the working tree carries no block of its own.
pub const RETARGET_REFUSAL: &str = "a rewritten target confers no authority";

/// Refuse when any version of `record` that `head` reaches declares an
/// integration target other than `target`. One walk reads that one path per
/// commit and parses each distinct version once. A history that cannot be
/// read in full (a shallow clone, a graft or replace ref, an unreadable
/// commit or version) refuses.
fn refuse_retarget(
    repo: &Repository,
    head: Oid,
    record: &str,
    task_id: &str,
    target: &str,
) -> Result<(), String> {
    let unreadable =
        |reason: String| format!("the history of task {task_id} cannot be read: {reason}");
    let mut walk = history::revwalk(repo).map_err(|e| unreadable(e.to_string()))?;
    walk.push(head).map_err(|e| unreadable(e.to_string()))?;
    let mut seen = std::collections::HashSet::new();
    for commit in walk {
        let commit = commit
            .and_then(|id| repo.find_commit(id))
            .map_err(|e| unreadable(e.to_string()))?;
        let tree = commit.tree().map_err(|e| unreadable(e.to_string()))?;
        let entry = match tree.get_path(Path::new(record)) {
            Ok(entry) => entry,
            Err(e) if e.code() == git2::ErrorCode::NotFound => continue,
            Err(e) => return Err(unreadable(e.to_string())),
        };
        if !seen.insert(entry.id()) {
            continue;
        }
        let version = repo
            .find_blob(entry.id())
            .map_err(|e| e.to_string())
            .and_then(|blob| String::from_utf8(blob.content().to_vec()).map_err(|e| e.to_string()))
            .and_then(|text| parse_record(&text, RecordKind::Task))
            .map_err(|e| unreadable(format!("{record} at {}: {e}", commit.id())))?;
        if let Some(declared) = version
            .integration_target
            .as_deref()
            // Spaces and tabs only (issue 79): a value of another whitespace
            // character is a declared target, never an unset one.
            .filter(|declared| !declared.trim_matches([' ', '\t']).is_empty())
            .filter(|declared| logical_target(declared) != logical_target(target))
        {
            return Err(format!(
                "an earlier version of task {task_id} declares integration target \
                 '{declared}', not '{target}'; {RETARGET_REFUSAL}"
            ));
        }
    }
    Ok(())
}

/// The commit the check reads: `head` when a caller names one (CI's pull
/// request head), else the checkout's `HEAD`.
/// Whether the change this check reads (the named head, else the checkout)
/// unblocks `task_id`, blocked at `merge_base` (TSK-1036): the record there
/// is `todo` or `complete` with no Blocker, or `cancelled`, at the same
/// path, and every status step from the merge-base is read and legal, so a
/// direct `blocked -> complete` or unreadable history projects nothing.
fn unblocked_in_range(
    repo: &Repository,
    repo_root: &Path,
    merge_base: &str,
    head: Option<&str>,
    head_id: Oid,
    task_id: &str,
) -> Result<bool, WorkStartError> {
    use super::lifecycle::Graph;
    use super::status_steps::{judge, step_allowed};
    let base = Graph::from_revision(repo, merge_base).map_err(WorkStartError::InvalidGraph)?;
    let Some(old) = base.records.get(task_id) else {
        return Ok(false);
    };
    let current = match head {
        Some(_) => Graph::from_revision(repo, &head_id.to_string()),
        None => Graph::from_worktree(repo_root),
    }
    .map_err(WorkStartError::InvalidGraph)?;
    let Some(now) = current.records.get(task_id) else {
        return Ok(false);
    };
    let cleared = match now.status.as_str() {
        "todo" | "complete" => super::record_text::parse_blocker(&now.body).is_none(),
        "cancelled" => true,
        _ => false,
    };
    if now.path != old.path || !cleared {
        return Ok(false);
    }
    let base_id = Oid::from_str(merge_base)
        .map_err(|error| WorkStartError::Repository(error.message().to_string()))?;
    if !judge(repo, base_id, head_id, RecordKind::Task, &now.id).proven {
        return Ok(false);
    }
    if head.is_none() {
        // The checkout's uncommitted change is the last step.
        let committed = Graph::from_revision(repo, &head_id.to_string())
            .map_err(WorkStartError::InvalidGraph)?;
        let Some(at_head) = committed.records.get(task_id) else {
            return Ok(false);
        };
        if at_head.path != now.path
            || step_allowed(RecordKind::Task, Some(&at_head.status), &now.status).is_err()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn head_commit(repo: &Repository, head: Option<&str>) -> Result<Oid, WorkStartError> {
    match head {
        Some(revision) => repo
            .revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map(|commit| commit.id())
            .map_err(|error| {
                WorkStartError::Repository(format!("head {revision}: {}", error.message()))
            }),
        None => repo
            .head()
            .map_err(|error| WorkStartError::Repository(error.to_string()))?
            .target()
            .ok_or_else(|| WorkStartError::Repository("HEAD has no commit".to_string())),
    }
}

fn anchored_records(
    repo: &Repository,
    target: &str,
    head_id: Oid,
) -> Result<(String, BTreeMap<String, Record>), WorkStartError> {
    let target_commit = target_reference(repo, target)?
        .ok_or_else(|| WorkStartError::Target(target.to_string()))?;
    // Only git's "not found" means the two have no common ancestor; any
    // other error is a history that could not be read.
    let merge_base = match history::merge_base(repo, head_id, target_commit.id()) {
        Ok(merge_base) => merge_base,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Err(WorkStartError::MergeBase(target.to_string()))
        }
        Err(error) => return Err(WorkStartError::Repository(error.to_string())),
    };
    let commit = repo
        .find_commit(merge_base)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let records = records_from_tree(
        repo,
        &commit
            .tree()
            .map_err(|error| WorkStartError::Repository(error.to_string()))?,
    )?;
    Ok((merge_base.to_string(), records))
}

fn standalone_at_head(
    repo: &Repository,
    records: &mut BTreeMap<String, Record>,
    task_id: &str,
    branch: &str,
    admission: bool,
    head_id: Oid,
) -> Result<(), WorkStartError> {
    if records.contains_key(task_id) {
        return Ok(());
    }
    let root = repo
        .workdir()
        .ok_or_else(|| WorkStartError::Repository("no working tree".into()))?;
    // The branch names the task by shape; the record itself is read at the
    // head, which need not be the checkout (CI judges from the base).
    let carries = work_branch_suffix(root, branch)?
        .is_some_and(|suffix| suffix.starts_with(&format!("{task_id}-")));
    if !carries {
        return Err(WorkStartError::TaskNotAnchored(task_id.into()));
    }
    let head = repo
        .find_commit(head_id)
        .map_err(|e| WorkStartError::Repository(e.to_string()))?;
    let at_head = records_from_tree(
        repo,
        &head
            .tree()
            .map_err(|e| WorkStartError::Repository(e.to_string()))?,
    )?;
    let task = at_head
        .get(task_id)
        .filter(|r| r.kind == RecordKind::Task && r.epic_id.is_none())
        .ok_or_else(|| WorkStartError::TaskNotAnchored(task_id.into()))?;
    if task
        .standalone_reason
        .as_deref()
        .is_none_or(|s| s.trim().is_empty())
    {
        return Err(WorkStartError::MissingStandaloneReason(task_id.into()));
    }
    let task = if admission && task.status == "complete" {
        let graph = super::lifecycle::Graph::from_revision(repo, &head_id.to_string())
            .map_err(WorkStartError::InvalidGraph)?;
        let view = &graph.records[task_id];
        let blocks = view.active_blocks();
        let reviewed = blocks
            .first()
            .and_then(|b| b.parsed.as_ref().ok())
            .filter(|_| blocks.len() == 1)
            .ok_or_else(|| {
                WorkStartError::InvalidGraph(format!(
                    "{task_id} requires one valid acceptance block"
                ))
            })?;
        let commit = super::acceptance::commit_of(repo, &reviewed.reviewed)
            .and_then(|id| repo.find_commit(id).ok())
            .ok_or_else(|| {
                WorkStartError::InvalidGraph("reviewed revision does not resolve".into())
            })?;
        let pinned = records_from_tree(
            repo,
            &commit
                .tree()
                .map_err(|e| WorkStartError::Repository(e.to_string()))?,
        )?;
        pinned
            .get(task_id)
            .filter(|r| r.epic_id.is_none())
            .cloned()
            .ok_or_else(|| WorkStartError::TaskNotAnchored(task_id.into()))?
    } else {
        task.clone()
    };
    records.insert(task_id.to_string(), task);
    Ok(())
}

fn start_gate(task: &Record, task_id: &str) -> Result<(), WorkStartError> {
    let closed = matches!(task.status.as_str(), "complete" | "cancelled");
    if !closed && (task.status == "blocked" || task.blocker_reason.is_some()) {
        return Err(WorkStartError::Blocked {
            task_id: task_id.into(),
            reason: task
                .blocker_reason
                .clone()
                .filter(|r| !r.trim().is_empty())
                .unwrap_or_else(|| "no Blocker reason recorded".into()),
        });
    }
    if !matches!(task.status.as_str(), "todo" | "in_progress") {
        return Err(WorkStartError::TaskNotStartable {
            task_id: task_id.into(),
            status: task.status.clone(),
        });
    }
    if let Some(path) = &task.awaiting_selection {
        return Err(WorkStartError::AwaitingSelection {
            task_id: task_id.into(),
            path: path.clone(),
        });
    }
    Ok(())
}

/// The one readiness core of SPC-013 R-40, over the records of one base
/// commit: `work start`, CI, `work next`, `work claim` and
/// `status` all call it and differ only in which base they read (R-110).
pub(crate) fn validate_anchored_task(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
) -> Result<AnchoredTask, WorkStartError> {
    validate_anchored_task_on(repo, records, task_id, target, &[])
}

pub(crate) fn validate_anchored_task_on(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
    pins: &[ReviewedPin],
) -> Result<AnchoredTask, WorkStartError> {
    validate_anchored_task_in(repo, records, task_id, target, pins, &mut None)
}

/// [`validate_anchored_task_on`] over pin branches read at most once.
pub(crate) fn validate_anchored_task_in(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
    pins: &[ReviewedPin],
    branches: &mut Option<PinBranches>,
) -> Result<AnchoredTask, WorkStartError> {
    let result = validate_task_structure(repo, records, task_id, target, pins, branches, true)?;
    start_gate(&records[task_id], task_id)?;
    Ok(result)
}

/// Issue 603: a reviewed pin may name a code dependency that the task's
/// own change adds. The edge is read from the task's record in the judged
/// head commit, never the working tree, and projected onto the anchored
/// record only for a pinned predecessor the merge-base record does not name
/// at all. The pin itself is then checked as every pinned code dependency
/// is; every other planning input keeps the merge-base's authority.
///
/// # Errors
/// Returns why the head's records cannot be read.
fn own_pin_edges(
    repo: &Repository,
    records: &mut BTreeMap<String, Record>,
    task_id: &str,
    head: Oid,
    pins: &[ReviewedPin],
) -> Result<(), WorkStartError> {
    let Some(task) = records.get_mut(task_id) else {
        return Ok(());
    };
    let unnamed: Vec<&str> = pins
        .iter()
        .map(|pin| pin.task_id.as_str())
        .filter(|id| !task.depends_on.iter().any(|dep| dep.id == *id))
        .collect();
    if unnamed.is_empty() {
        return Ok(());
    }
    let tree = repo
        .find_commit(head)
        .and_then(|commit| commit.tree())
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let at_head = records_from_tree_matching(repo, &tree, |path, kind| {
        kind == RecordKind::Task && path.rsplit('/').next().and_then(markdown_stem) == Some(task_id)
    })?;
    if let Some(own) = at_head.get(task_id) {
        task.depends_on.extend(
            own.depends_on
                .iter()
                .filter(|dep| {
                    dep.kind == DependencyKind::Code && unnamed.contains(&dep.id.as_str())
                })
                .cloned(),
        );
    }
    Ok(())
}

fn validate_task_structure(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
    pins: &[ReviewedPin],
    branches: &mut Option<PinBranches>,
    dependencies_required: bool,
) -> Result<AnchoredTask, WorkStartError> {
    let task = records
        .get(task_id)
        .filter(|record| record.kind == RecordKind::Task)
        .ok_or_else(|| WorkStartError::TaskNotAnchored(task_id.to_string()))?;
    match task
        .integration_target
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(declared_target) if logical_target(declared_target) != logical_target(target) => {
            return Err(WorkStartError::TargetMismatch {
                task_id: task_id.to_string(),
                declared: declared_target.to_string(),
                requested: target.to_string(),
            });
        }
        None if is_canonical_task_format_id(task_id) => {
            return Err(WorkStartError::MissingIntegrationTarget(
                task_id.to_string(),
            ));
        }
        _ => {}
    }
    let epic = if let Some(epic_id) = task.epic_id.as_deref() {
        let epic = records
            .get(epic_id)
            .filter(|record| record.kind == RecordKind::Epic)
            .ok_or_else(|| WorkStartError::MissingEpic {
                task_id: task_id.to_string(),
                epic_id: epic_id.to_string(),
            })?;
        if matches!(epic.status.as_str(), "complete" | "cancelled" | "archived") {
            return Err(WorkStartError::EpicClosed {
                task_id: task_id.to_string(),
                epic_id: epic_id.to_string(),
                status: epic.status.clone(),
            });
        }
        Some(epic)
    } else {
        if task
            .standalone_reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
        {
            return Err(WorkStartError::MissingStandaloneReason(task_id.to_string()));
        }
        None
    };

    let mut specs = epic.map_or_else(Vec::new, |record| record.specs.clone());
    for spec in &task.specs {
        if !specs.contains(spec) {
            specs.push(spec.clone());
        }
    }
    validate_specs(records, task_id, &specs)?;
    if let Some(pin) = pins.iter().find(|pin| {
        !task
            .depends_on
            .iter()
            .any(|dep| dep.id == pin.task_id && dep.kind == DependencyKind::Code)
    }) {
        return Err(WorkStartError::InvalidGraph(format!(
            "--on pin does not name a code dependency of this task: {task_id} does not depend on {} at the target or in its own committed record; add {} to depends_on in {task_id}'s record on its branch, commit it and rerun",
            pin.task_id, pin.task_id
        )));
    }
    if dependencies_required {
        validate_dependencies(
            repo,
            records,
            task_id,
            target,
            &task.depends_on,
            pins,
            branches,
        )?;
    }
    Ok(AnchoredTask {
        epic_id: task.epic_id.clone(),
        specs,
        dependencies: task.depends_on.iter().map(|dep| dep.id.clone()).collect(),
        work_type: task.work_type.clone(),
    })
}

fn validate_specs(
    records: &BTreeMap<String, Record>,
    task_id: &str,
    specs: &[String],
) -> Result<(), WorkStartError> {
    for spec_id in specs {
        let spec = records
            .get(spec_id)
            .filter(|record| record.kind == RecordKind::Spec)
            .ok_or_else(|| WorkStartError::MissingSpec {
                task_id: task_id.to_string(),
                spec_id: spec_id.clone(),
            })?;
        if !matches!(spec.status.as_str(), "approved" | "implemented") {
            return Err(WorkStartError::SpecNotReady {
                task_id: task_id.to_string(),
                spec_id: spec_id.clone(),
                status: spec.status.clone(),
            });
        }
    }
    Ok(())
}

fn validate_dependencies(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
    dependencies: &[Dependency],
    pins: &[ReviewedPin],
    branches: &mut Option<PinBranches>,
) -> Result<(), WorkStartError> {
    for dependency in dependencies {
        match dependency.kind {
            DependencyKind::Code => {
                if let Some(pin) = pins.iter().find(|pin| pin.task_id == dependency.id) {
                    PinBranches::once(branches, repo)
                        .and_then(|branches| branches.pin_branch(repo, pin))
                        .map_err(WorkStartError::InvalidGraph)?;
                } else {
                    code_dependency(repo, records, task_id, target, &dependency.id)?;
                }
            }
            DependencyKind::Research | DependencyKind::Decision => {
                pinned_dependency(repo, records, task_id, target, dependency)?;
            }
        }
    }
    Ok(())
}

/// A code dependency is met only when the predecessor is complete in this
/// base (R-42): complete on another line is reported as such, and a line
/// that is not fetched is unknown, never satisfied.
fn code_dependency(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
    dependency: &str,
) -> Result<(), WorkStartError> {
    let predecessor = records
        .get(dependency)
        .filter(|record| record.kind == RecordKind::Task)
        .ok_or_else(|| WorkStartError::MissingDependency {
            task_id: task_id.to_string(),
            dependency: dependency.to_string(),
        })?;
    if predecessor.status == "complete" {
        return Ok(());
    }
    let incomplete = || WorkStartError::DependencyIncomplete {
        task_id: task_id.to_string(),
        dependency: dependency.to_string(),
        status: predecessor.status.clone(),
    };
    let Some(line) = predecessor
        .integration_target
        .as_deref()
        .filter(|line| logical_target(line) != logical_target(target))
    else {
        return Err(incomplete());
    };
    let Some(tip) = target_reference(repo, line)? else {
        return Err(WorkStartError::DependencyLineUnknown {
            task_id: task_id.to_string(),
            dependency: dependency.to_string(),
            line: line.to_string(),
        });
    };
    let there = tip
        .tree()
        .map_err(|error| WorkStartError::Repository(error.to_string()))
        .and_then(|tree| records_from_tree(repo, &tree))?;
    if there
        .get(dependency)
        .is_some_and(|record| record.status == "complete")
    {
        Err(WorkStartError::DependencyOnOtherLine {
            task_id: task_id.to_string(),
            dependency: dependency.to_string(),
            line: line.to_string(),
        })
    } else {
        Err(incomplete())
    }
}

/// A research or decision dependency is met when its pin is a commit that
/// holds the predecessor's record as complete and is on a verified target
/// that still exists: the predecessor's target, this task's target or the
/// default branch, each read as [`tracking_reference`] reads it (R-112). The
/// predecessor need not be in this base, and a deleted predecessor line does
/// not unmeet a pin that landed (sathyassn/codeflow#236).
fn pinned_dependency(
    repo: &Repository,
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
    dependency: &Dependency,
) -> Result<(), WorkStartError> {
    let Some(pin) = dependency.pin.as_deref() else {
        return Err(WorkStartError::DependencyUnpinned {
            task_id: task_id.to_string(),
            dependency: dependency.id.clone(),
            kind: dependency.kind.as_str().to_string(),
        });
    };
    let refuse = |reason: String| WorkStartError::DependencyPin {
        task_id: task_id.to_string(),
        dependency: dependency.id.clone(),
        pin: pin.to_string(),
        reason,
    };
    let commit = commit_by_object_id(repo, pin).map_err(refuse)?;
    let line = records
        .get(&dependency.id)
        .and_then(|record| record.integration_target.clone());
    let mut names = line
        .into_iter()
        .chain([target.to_string()])
        .collect::<Vec<_>>();
    for default in ["main", "master"] {
        if tracking_reference(repo, default)?.is_some() {
            names.push(default.to_string());
            break;
        }
    }
    let (mut read, mut unfetched) = (Vec::<String>::new(), Vec::<String>::new());
    let mut landed = false;
    for name in names {
        match tracking_reference(repo, &name)? {
            Some((shown, _)) if read.contains(&shown) => {}
            Some((shown, tip)) => {
                if tip == commit.id()
                    || history::descendant_of(repo, tip, commit.id())
                        .map_err(|error| refuse(error.to_string()))?
                {
                    landed = true;
                    break;
                }
                read.push(shown);
            }
            None if unfetched.contains(&name) => {}
            None => unfetched.push(name),
        }
    }
    if !landed {
        return Err(refuse(not_landed_reason(&read, &unfetched)));
    }
    let at_pin = commit
        .tree()
        .map_err(|error| WorkStartError::Repository(error.to_string()))
        .and_then(|tree| records_from_tree(repo, &tree))?;
    match at_pin
        .get(&dependency.id)
        .map(|record| record.status.as_str())
    {
        Some("complete") => Ok(()),
        Some(status) => Err(refuse(format!("the record there is '{status}'"))),
        None => Err(refuse("the record is not there".to_string())),
    }
}

/// The landing ref readiness reads for a target, with its tip, as output
/// shows it (`origin/main`). In a repository with any remote, a landing ref
/// is always a remote-tracking ref, so no local ref decides readiness
/// (sathyassn/codeflow#236):
/// - a branch, bare (`main`) or spelled `refs/heads/main`, reads the upstream
///   configured for it only when that upstream is the same branch on a
///   configured remote, `refs/remotes/<remote>/<branch>` (never remote `.`, a
///   local branch, a tag or another branch), else
///   `refs/remotes/origin/<branch>` on the authority remote;
/// - an explicit remote-tracking ref (`origin/main`, `refs/remotes/...`) counts
///   only when a configured remote's fetch mapping owns it.
///
/// The local branch is a landing ref only in a repository with no remote at
/// all. `None` when the target is not fetched here.
pub(crate) fn tracking_reference(
    repo: &Repository,
    name: &str,
) -> Result<Option<(String, Oid)>, WorkStartError> {
    let Some(candidates) = target_reference_names(name) else {
        return Ok(None);
    };
    // A bare name is the branch of that name; any other spelling names one ref.
    let exact = candidates
        .iter()
        .find(|candidate| candidate.starts_with("refs/heads/"))
        .unwrap_or(&candidates[0]);
    // A landing ref counts only under the exact spelling git lists
    // (TSK-381 SF-4).
    let read = |reference: &str| {
        crate::hooks::repo::listed_ref_commit(repo, reference)
            .map(|tip| tip.map(|tip| (reference_display_name(reference), tip)))
            .map_err(|error| {
                // The listing reads every ref; only this ref's own failure
                // is reported as its unresolvable tip.
                let own = repo
                    .find_reference(reference)
                    .map(|found| found.peel_to_commit().err());
                match own {
                    Ok(Some(own)) => {
                        WorkStartError::UnresolvableTip(unresolvable_tip(repo, reference, &own))
                    }
                    _ => WorkStartError::Repository(error.to_string()),
                }
            })
    };
    let remotes = repo
        .remotes()
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let branch = exact.strip_prefix("refs/heads/");
    if remotes.is_empty() {
        return branch.map_or(Ok(None), |_| read(exact));
    }
    let Some(branch) = branch else {
        return if repo.branch_remote_name(exact).is_ok() {
            read(exact)
        } else {
            Ok(None)
        };
    };
    let landing = configured_upstream(repo, branch, &remotes)?
        .unwrap_or_else(|| format!("refs/remotes/{}/{branch}", crate::ids::AUTHORITY));
    read(&landing)
}

/// [`configured_upstream`] for a caller outside this module.
pub(crate) fn same_branch_upstream(
    repo: &Repository,
    branch: &str,
) -> Result<Option<String>, WorkStartError> {
    let remotes = repo
        .remotes()
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    configured_upstream(repo, branch, &remotes)
}

/// The upstream configured for local branch `branch`, when it is
/// `refs/remotes/<remote>/<branch>`, the same branch on a configured remote
/// other than `.` (the fork case). Any other upstream, including another
/// branch of that remote and one git cannot map, is not honoured, and the
/// caller reads the authority remote's tracking ref instead. A configured
/// name that is not valid UTF-8 is an error, never an absent upstream (issue
/// 79).
fn configured_upstream(
    repo: &Repository,
    branch: &str,
    remotes: &git2::string_array::StringArray,
) -> Result<Option<String>, WorkStartError> {
    let local = format!("refs/heads/{branch}");
    let not_utf8 = |what: &str| {
        WorkStartError::Repository(format!(
            "the configured upstream {what} of '{branch}' is not valid UTF-8"
        ))
    };
    let Ok(remote) = repo.branch_upstream_remote(&local) else {
        return Ok(None);
    };
    let remote = remote.as_str().map_err(|_| not_utf8("remote"))?;
    if remote == "." || !remotes.iter_bytes().any(|name| name == remote.as_bytes()) {
        return Ok(None);
    }
    let Ok(upstream) = repo.branch_upstream_name(&local) else {
        return Ok(None);
    };
    let upstream = upstream.as_str().map_err(|_| not_utf8("ref"))?;
    Ok(upstream
        .strip_prefix(&format!("refs/remotes/{remote}/"))
        .is_some_and(|rest| rest == branch)
        .then(|| upstream.to_owned()))
}

/// Why a pin is on no verified target: the refs read, then the targets that
/// are not fetched here.
fn not_landed_reason(read: &[String], unfetched: &[String]) -> String {
    let quoted = |names: &[String], conjunction: &str| {
        let names = names
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>();
        match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => {
                format!("{} {conjunction} {last}", rest.join(", "))
            }
            _ => names.concat(),
        }
    };
    let mut parts = Vec::new();
    if !read.is_empty() {
        parts.push(format!("not on {}", quoted(read, "or")));
    }
    if !unfetched.is_empty() {
        let verb = if unfetched.len() == 1 { "is" } else { "are" };
        parts.push(format!(
            "{} {verb} not fetched here",
            quoted(unfetched, "and")
        ));
    }
    parts.join("; ")
}

/// Why an object id does not resolve to a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommitLookup {
    /// It is not an object id, names no object here, is ambiguous, or names
    /// an object that is not a commit.
    Unresolved(String),
    /// The object is here but cannot be read (issue 79): no answer about it.
    Unreadable(String),
}

impl CommitLookup {
    /// The reason, as the lookup's callers show it.
    pub(crate) fn reason(self) -> String {
        match self {
            Self::Unresolved(reason) | Self::Unreadable(reason) => reason,
        }
    }
}

/// Resolve a pin through the object database, never through refs: a tag or
/// branch named like the pin cannot redirect it. An abbreviated id must be
/// unambiguous and name a commit.
pub(crate) fn commit_by_object_id<'repo>(
    repo: &'repo Repository,
    pin: &str,
) -> Result<git2::Commit<'repo>, String> {
    lookup_commit_by_object_id(repo, pin).map_err(CommitLookup::reason)
}

/// [`commit_by_object_id`], telling an id that names no commit here from an
/// object that cannot be read, which git reports alike when only the
/// commit lookup is asked.
pub(crate) fn lookup_commit_by_object_id<'repo>(
    repo: &'repo Repository,
    pin: &str,
) -> Result<git2::Commit<'repo>, CommitLookup> {
    let unreadable = |error: git2::Error| {
        CommitLookup::Unreadable(format!("cannot read object {pin}: {}", error.message()))
    };
    let prefix = git2::Oid::from_str(pin)
        .map_err(|_| CommitLookup::Unresolved("not an object id".to_string()))?;
    let odb = repo.odb().map_err(unreadable)?;
    let full = if pin.len() == 40 {
        prefix
    } else {
        match odb.exists_prefix(prefix, pin.len()) {
            Ok(full) => full,
            Err(error) if error.code() == git2::ErrorCode::Ambiguous => {
                return Err(CommitLookup::Unresolved(
                    "an ambiguous abbreviated object id".to_string(),
                ))
            }
            Err(error) if error.code() == git2::ErrorCode::NotFound => {
                return Err(CommitLookup::Unresolved(
                    "not an object in this repository".to_string(),
                ))
            }
            Err(error) => return Err(unreadable(error)),
        }
    };
    // Only an object the database proves absent is unresolved; one it
    // holds but cannot read is no answer.
    let kind = match odb.read_header(full) {
        Ok((_, kind)) => kind,
        Err(error) if error.code() == git2::ErrorCode::NotFound => {
            return Err(CommitLookup::Unresolved(
                "not a commit in this repository".to_string(),
            ))
        }
        Err(error) => return Err(unreadable(error)),
    };
    if kind != git2::ObjectType::Commit {
        return Err(CommitLookup::Unresolved(
            "not a commit in this repository".to_string(),
        ));
    }
    repo.find_commit(full).map_err(unreadable)
}

pub(crate) fn record_kind_for_tree_path(path: &str) -> Option<RecordKind> {
    let parts = path.split('/').collect::<Vec<_>>();
    match parts.as_slice() {
        ["project-management", "tasks", file]
        | ["project-management", "epics", _, "tasks", file]
            if markdown_stem(file).is_some_and(is_valid_task_format_id) =>
        {
            Some(RecordKind::Task)
        }
        ["project-management", "specs", file]
            if markdown_stem(file).is_some_and(is_valid_spec_format_id) =>
        {
            Some(RecordKind::Spec)
        }
        ["project-management", "epics", file]
            if markdown_stem(file).is_some_and(is_valid_epic_format_id) =>
        {
            Some(RecordKind::Epic)
        }
        ["project-management", "epics", epic, file]
            if is_valid_epic_format_id(epic) && markdown_stem(file) == Some(*epic) =>
        {
            Some(RecordKind::Epic)
        }
        _ => None,
    }
}

fn markdown_stem(file: &str) -> Option<&str> {
    let (stem, extension) = file.rsplit_once('.')?;
    extension.eq_ignore_ascii_case("md").then_some(stem)
}

/// The tree every work record lives under.
const RECORDS_ROOT: &str = "project-management";

/// The largest work record a tree read accepts. The largest real record is
/// about 100 KiB; the bound matches the portal's per-file source limit.
const MAX_RECORD_BYTES: u64 = 4 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    /// How many trees `records_from_tree` has parsed on this thread.
    pub(crate) static TREE_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn records_from_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
) -> Result<BTreeMap<String, Record>, WorkStartError> {
    records_from_tree_matching(repo, tree, |_, _| true)
}

pub(crate) fn records_from_tree_matching(
    repo: &Repository,
    tree: &git2::Tree<'_>,
    include: impl Fn(&str, RecordKind) -> bool,
) -> Result<BTreeMap<String, Record>, WorkStartError> {
    #[cfg(test)]
    TREE_PARSES.with(|parses| parses.set(parses.get() + 1));
    let mut records = BTreeMap::new();
    let mut failure = None;
    let odb = repo
        .odb()
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let walk_result = crate::git::walk_tree(repo, tree, &mut |name, entry| {
        if failure.is_some() {
            return crate::git::Walk::Stop;
        }
        // A record path is valid text; a name that is not UTF-8 is never one,
        // and a tree with such a name is not walked into for records.
        let Ok(path) = name.rule_text() else {
            return if entry.kind() == Some(git2::ObjectType::Tree) {
                crate::git::Walk::SkipTree
            } else {
                crate::git::Walk::Continue
            };
        };
        // Only the record paths are read (SPC-013 R-103): a tree elsewhere
        // is never loaded, so a partial clone that lacks it still reads.
        if entry.kind() == Some(git2::ObjectType::Tree) {
            return if path == RECORDS_ROOT || path.starts_with(&format!("{RECORDS_ROOT}/")) {
                crate::git::Walk::Continue
            } else {
                crate::git::Walk::SkipTree
            };
        }
        let Some(kind) = record_kind_for_tree_path(path) else {
            return crate::git::Walk::Continue;
        };
        if !include(path, kind) {
            return crate::git::Walk::Continue;
        }
        // A record's size is read from its object header before its bytes,
        // so a hostile tip cannot make every reader load it.
        let size = match odb.read_header(entry.id()) {
            Ok((size, _)) => size,
            Err(error) => {
                failure = Some(format!("{path}: cannot read record size: {error}"));
                return crate::git::Walk::Stop;
            }
        };
        if u64::try_from(size).map_or(true, |size| size > MAX_RECORD_BYTES) {
            failure = Some(format!(
                "{path}: record exceeds the 4 MiB bound for a work record"
            ));
            return crate::git::Walk::Stop;
        }
        let Ok(blob) = repo.find_blob(entry.id()) else {
            failure = Some(format!("{path}: cannot read blob"));
            return crate::git::Walk::Stop;
        };
        let Ok(content) = std::str::from_utf8(blob.content()) else {
            failure = Some(format!("{path}: record is not UTF-8"));
            return crate::git::Walk::Stop;
        };
        match parse_record(content, kind) {
            Ok(record) => {
                if records.insert(record.id.clone(), record).is_some() {
                    failure = Some(format!("{path}: duplicate work id"));
                    crate::git::Walk::Stop
                } else {
                    crate::git::Walk::Continue
                }
            }
            Err(error) => {
                failure = Some(format!("{path}: {error}"));
                crate::git::Walk::Stop
            }
        }
    });
    if let Some(error) = failure {
        return Err(WorkStartError::InvalidGraph(error));
    }
    walk_result.map_err(|error| WorkStartError::Repository(error.to_string()))?;
    Ok(records)
}

pub(crate) fn parse_record(content: &str, kind: RecordKind) -> Result<Record, String> {
    let yaml = frontmatter(content).ok_or_else(|| "missing frontmatter".to_string())?;
    let data = serde_yaml::from_str::<serde_yaml::Mapping>(yaml)
        .map_err(|error| format!("invalid YAML: {error}"))?;
    let raw_id = string(&data, "id").ok_or_else(|| "missing id".to_string())?;
    let alias = string(&data, "format_id");
    let id_is_valid = match kind {
        RecordKind::Epic => is_valid_epic_format_id(&raw_id),
        RecordKind::Spec => is_valid_spec_format_id(&raw_id),
        RecordKind::Task => is_valid_task_format_id(&raw_id),
    };
    let id = if id_is_valid {
        if alias.as_deref().is_some_and(|value| value != raw_id) {
            return Err("id and format_id conflict".to_string());
        }
        raw_id
    } else {
        alias.ok_or_else(|| "id is malformed and no supported format_id exists".to_string())?
    };
    let valid = match kind {
        RecordKind::Epic => is_valid_epic_format_id(&id),
        RecordKind::Spec => is_valid_spec_format_id(&id),
        RecordKind::Task => is_valid_task_format_id(&id),
    };
    if !valid {
        return Err(format!("unsupported id {id}"));
    }
    let status = string(&data, "status").unwrap_or_default();
    let reviewed = if kind == RecordKind::Task && status == "complete" {
        accepted_block(content)
    } else {
        None
    };
    Ok(Record {
        id,
        uid: string(&data, "uid"),
        reviewed,
        acceptance: active_block_text(content),
        kind,
        status,
        epic_id: string(&data, "epic_id"),
        standalone_reason: string(&data, "standalone_reason"),
        title: string(&data, "title").unwrap_or_default(),
        created: string(&data, "created").or_else(|| string(&data, "created_at")),
        integration_target: string(&data, "integration_target"),
        specs: strings(&data, "specs")?,
        depends_on: dependencies(&data)?,
        work_type: string(&data, "work_type"),
        awaiting_selection: string(&data, "awaiting_selection").filter(|path| !path.is_empty()),
        blocker_reason: blocker_reason(content),
        line_adoptions: if kind == RecordKind::Epic {
            super::line_adoption::parse(data.get(key("line_adoptions")))?
        } else {
            Vec::new()
        },
        parent_line: super::line_adoption::parse_parent_line(
            data.get(key(super::line_adoption::PARENT_LINE_FIELD)),
        )?,
    })
}

/// The text of the record's one active acceptance block.
fn active_block_text(content: &str) -> Option<String> {
    let blocks = super::record_text::acceptance_blocks(content);
    let mut active = blocks.iter().filter(|block| !block.is_superseded());
    let block = active.next()?;
    active.next().is_none().then(|| block.inner.clone())
}

/// The `reviewed` commit of the record's one active acceptance block, when
/// that block parses and passes the structural acceptance rules for its
/// criteria. Two active blocks are ambiguous and give none.
fn accepted_block(content: &str) -> Option<String> {
    use super::record_text::{acceptance_blocks, check_acceptance, parse_criteria};
    let blocks = acceptance_blocks(content);
    let mut active = blocks.iter().filter(|block| !block.is_superseded());
    let block = active.next()?;
    if active.next().is_some() {
        return None;
    }
    match &block.parsed {
        Ok(parsed) if check_acceptance(parsed, &parse_criteria(content).items).is_empty() => {
            Some(parsed.reviewed.clone())
        }
        _ => None,
    }
}

/// The commit a target names, read through [`target_reference_names`]: for
/// a bare name the fetched `origin/<name>` when it exists, else the local
/// branch.
pub(crate) fn target_reference<'repo>(
    repo: &'repo Repository,
    target: &str,
) -> Result<Option<git2::Commit<'repo>>, WorkStartError> {
    Ok(target_reference_read(repo, target)?.map(|(_, commit)| commit))
}

/// The full ref [`target_reference`] reads for `target`, with its commit,
/// so a caller can say which reference decided.
pub(crate) fn target_reference_read<'repo>(
    repo: &'repo Repository,
    target: &str,
) -> Result<Option<(String, git2::Commit<'repo>)>, WorkStartError> {
    let Some(mut candidates) = target_reference_names(target) else {
        return Ok(None);
    };
    // A bare name whose local branch tracks the same branch on another
    // configured remote reads that ref first: a fork's `origin/<name>` is not
    // the line the branch follows. An upstream naming another branch is never
    // read, so a config write cannot point the line elsewhere (TSK-416).
    if candidates.len() > 1 {
        let remotes = repo
            .remotes()
            .map_err(|error| WorkStartError::Repository(error.to_string()))?;
        if let Some(upstream) = configured_upstream(repo, target, &remotes)? {
            candidates.insert(0, upstream);
        }
    }
    for name in candidates {
        if let Some(oid) = reference_commit(repo, &name)? {
            return repo
                .find_commit(oid)
                .map(|commit| Some((name, commit)))
                .map_err(|error| WorkStartError::Repository(error.to_string()));
        }
    }
    Ok(None)
}

/// The refs a target name may be read through, in the order they are tried.
///
/// A full ref (`refs/heads/...`, `refs/remotes/...`) and an `origin/<name>`
/// spelling name one ref. A bare name tries the fetched
/// `refs/remotes/origin/<name>` first and the local `refs/heads/<name>` only
/// after it, so a stale local branch of the same name never decides where
/// the fetched line exists (TSK-416). `None` for a name that is not a
/// target.
pub(crate) fn target_reference_names(target: &str) -> Option<Vec<String>> {
    if target.is_empty() || logical_target(target) == "HEAD" {
        return None;
    }
    let candidates = if target.starts_with("refs/heads/") || target.starts_with("refs/remotes/") {
        vec![target.to_string()]
    } else if target.starts_with("refs/") {
        return None;
    } else if let Some(remote_branch) = target.strip_prefix("origin/") {
        vec![format!("refs/remotes/origin/{remote_branch}")]
    } else {
        vec![
            format!("refs/remotes/origin/{target}"),
            format!("refs/heads/{target}"),
        ]
    };
    candidates
        .iter()
        .all(|candidate| git2::Reference::is_valid_name(candidate))
        .then_some(candidates)
}

fn reference_display_name(reference: &str) -> String {
    reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/"))
        .unwrap_or(reference)
        .to_string()
}

pub(crate) fn logical_target(target: &str) -> &str {
    target
        .strip_prefix("refs/heads/")
        .or_else(|| {
            target
                .strip_prefix("refs/remotes/")
                .and_then(|value| value.split_once('/').map(|(_, branch)| branch))
        })
        .or_else(|| target.strip_prefix("origin/"))
        .unwrap_or(target)
}

fn frontmatter(content: &str) -> Option<&str> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if super::record_text::without_line_ending(line) == "---" {
            return Some(&rest[..offset]);
        }
        offset += line.len();
    }
    None
}

fn key(name: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(name.to_string())
}

fn string(data: &serde_yaml::Mapping, name: &str) -> Option<String> {
    data.get(key(name))
        .and_then(serde_yaml::Value::as_str)
        .map(str::to_owned)
}

fn strings(data: &serde_yaml::Mapping, name: &str) -> Result<Vec<String>, String> {
    let Some(value) = data.get(key(name)) else {
        return Ok(Vec::new());
    };
    let sequence = value
        .as_sequence()
        .ok_or_else(|| format!("{name} must be a YAML list"))?;
    sequence
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{name} must contain only ids"))
        })
        .collect()
}

fn dependencies(data: &serde_yaml::Mapping) -> Result<Vec<Dependency>, String> {
    let canonical = data.get(key("depends_on"));
    let legacy = data.get(key("dependencies"));
    if canonical.is_some() && legacy.is_some() {
        return Err("defines both depends_on and dependencies".to_string());
    }
    parse_dependencies(canonical.or(legacy))
}

/// The reason of a record's `## Blocker` section, if any.
/// The reason of a visible `## Blocker` section; `Some("")` when the section
/// is there without a reason, so an incomplete Blocker still holds the task.
fn blocker_reason(content: &str) -> Option<String> {
    let body = content
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---").map(|(_, body)| body))
        .unwrap_or(content);
    crate::workgraph::record_text::parse_blocker(body).map(|blocker| blocker.reason)
}

/// Resolve the repository from the branch's configured remote, falling back
/// to origin. Explicit selection prevents `GH_REPO` from supplying another PR.
///
/// # Errors
/// Refuses missing or ambiguous hosted repository identity.
pub fn review_repository(root: &std::path::Path, branch: &str) -> Result<String, String> {
    let _judgment = history::judgment();
    ReviewRepositories::open(root)?.repository(branch)
}

/// [`review_repository`] for many branches in one invocation: the
/// remote-tracking branch names are listed once, not once per branch.
pub struct ReviewRepositories {
    repo: Repository,
    remote_branches: Result<Vec<GitName>, String>,
}

impl ReviewRepositories {
    /// Open the repository at `root` and list its remote-tracking branches.
    ///
    /// # Errors
    /// Returns the reason the repository cannot be opened.
    pub fn open(root: &std::path::Path) -> Result<Self, String> {
        let repo = git2::Repository::discover(root).map_err(|error| error.to_string())?;
        let remote_branches = (|| {
            let mut names = Vec::new();
            for candidate in repo
                .branches(Some(git2::BranchType::Remote))
                .map_err(|error| error.to_string())?
            {
                let (candidate, _) = candidate.map_err(|error| error.to_string())?;
                // OS text rule (issue 79): exact names, compared with a branch
                // byte for byte.
                names.push(
                    crate::git::name::branch_name(&candidate).map_err(|error| error.to_string())?,
                );
            }
            Ok(names)
        })();
        Ok(Self {
            repo,
            remote_branches,
        })
    }

    /// The hosted repository `branch`'s review lives in, as
    /// [`review_repository`] resolves it.
    ///
    /// # Errors
    /// Refuses missing or ambiguous hosted repository identity.
    pub fn repository(&self, branch: &str) -> Result<String, String> {
        let repo = &self.repo;
        // `get_bytes` reads a snapshot, not the live configuration.
        let config = repo
            .config()
            .and_then(|mut config| config.snapshot())
            .map_err(|error| error.to_string())?;
        let mut names = std::collections::BTreeSet::new();
        // OS text rule (issue 79): an unset setting is absent, but a value
        // that is not valid UTF-8 names a remote this check cannot look up,
        // so it is a refusal, never a fall back to `origin`.
        match config.get_bytes(&format!("branch.{branch}.remote")) {
            Ok(bytes) => {
                let name = GitName::from_bytes(bytes);
                let text = name.rule_text().map_err(|error| {
                    format!(
                        "cannot verify review for this pin: the configured remote is not valid UTF-8 ({})",
                        error.display()
                    )
                })?;
                names.insert(text.to_string());
            }
            Err(error) if error.code() == git2::ErrorCode::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let suffix = format!("/{branch}");
        for candidate in self.remote_branches.as_ref().map_err(Clone::clone)? {
            if let Some(remote) = candidate.bytes().strip_suffix(suffix.as_bytes()) {
                // OS text rule (issue 79): the remote names the hosted
                // repository, an identity. One that is not valid UTF-8 cannot
                // be looked up, so the answer is a refusal, never another
                // remote's repository.
                let remote = GitName::from_bytes(remote);
                let remote = remote.rule_text().map_err(|error| {
                    format!(
                        "cannot verify review for this pin: a remote name is not valid UTF-8 ({})",
                        error.display()
                    )
                })?;
                names.insert(remote.to_string());
            }
        }
        if names.is_empty() {
            names.insert("origin".into());
        }
        let identities = names
            .into_iter()
            .map(|name| hosted_remote(repo, &name))
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        if identities.len() != 1 {
            return Err(
                "cannot verify review for this pin: conflicting predecessor repositories".into(),
            );
        }
        identities
            .into_iter()
            .next()
            .ok_or_else(|| "predecessor has no repository identity".into())
    }
}

fn hosted_remote(repo: &Repository, remote_name: &str) -> Result<String, String> {
    let remote = repo
        .find_remote(remote_name)
        .map_err(|_| "cannot verify review for this pin: predecessor has no repository remote")?;
    let url = remote.url().map_err(|_| "predecessor remote has no URL")?;
    let address = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://git@"))
        .map(str::to_string)
        .or_else(|| url.strip_prefix("git@").map(|s| s.replacen(':', "/", 1)))
        .ok_or("cannot verify review for this pin: remote is not a hosted repository URL")?;
    let address = address.trim_end_matches('/').trim_end_matches(".git");
    let parts: Vec<_> = address.split('/').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || part.contains(['?', '#', '@', ':']))
    {
        return Err("cannot verify review for this pin: ambiguous repository URL".into());
    }
    Ok(address.to_string())
}

#[cfg(test)]
mod tests {
    /// Issue 79: an object id that names no commit is told apart from a
    /// commit object that cannot be read, which the lookup used to report
    /// alike as "not a commit in this repository".
    #[test]
    fn r24_an_unreadable_commit_is_not_reported_absent() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, head) = crate::git::repo_with_tree(dir.path(), &[(b"a.txt", b"a\n")]);
        let blob = repo.blob(b"not a commit").unwrap();
        let id = head.to_string();
        assert!(super::commit_by_object_id(&repo, &id).is_ok());
        assert!(super::commit_by_object_id(&repo, &id[..12]).is_ok());
        assert_eq!(
            super::commit_by_object_id(&repo, &blob.to_string())
                .err()
                .as_deref(),
            Some("not a commit in this repository")
        );
        assert_eq!(
            super::commit_by_object_id(&repo, &"1".repeat(40))
                .err()
                .as_deref(),
            Some("not a commit in this repository")
        );
        assert_eq!(
            super::commit_by_object_id(&repo, &"1".repeat(12))
                .err()
                .as_deref(),
            Some("not an object in this repository")
        );
        let object = repo.path().join("objects").join(&id[..2]).join(&id[2..]);
        std::fs::remove_file(&object).unwrap();
        std::fs::write(&object, b"not zlib data").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        for pin in [id.as_str(), &id[..12]] {
            let error = super::commit_by_object_id(&repo, pin)
                .err()
                .unwrap_or_default();
            assert!(error.starts_with("cannot read object"), "{pin}: {error}");
        }
    }

    use super::*;

    /// Round 23: only git's "not found" from the merge-base walk is a
    /// missing common ancestor; a history object that cannot be read is a
    /// repository error, which blocks as unreadable tracking state.
    #[test]
    fn r23_merge_base_reader_errors_are_repository_errors() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, first) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"1")]);
        crate::git::add_commit(&repo, &[(b"a", b"2")]);
        let who = git2::Signature::now("Test", "test@example.com").unwrap();
        let (side, orphan) = {
            let parent = repo.find_commit(first).unwrap();
            let tree = parent.tree().unwrap();
            let side = repo
                .commit(None, &who, &who, "side", &tree, &[&parent])
                .unwrap();
            let orphan = repo.commit(None, &who, &who, "orphan", &tree, &[]).unwrap();
            (side, orphan)
        };
        drop(repo);

        // The control: no common ancestor is the semantic merge-base error.
        let repo = Repository::open(dir.path()).unwrap();
        assert!(matches!(
            anchored_records(&repo, "main", orphan),
            Err(WorkStartError::MergeBase(_))
        ));
        drop(repo);

        // The shared parent's loose object is damaged, so the walk cannot
        // read it.
        let hex = first.to_string();
        let object = dir
            .path()
            .join(".git")
            .join("objects")
            .join(&hex[..2])
            .join(&hex[2..]);
        let mut permissions = fs::metadata(&object).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&object, permissions).unwrap();
        fs::write(&object, b"not a zlib stream").unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        match anchored_records(&repo, "main", side) {
            Err(WorkStartError::Repository(_)) => {}
            other => panic!("expected a repository error, got {other:?}"),
        }
        drop(repo);
        // A missing ancestor object is also an unreadable history, never a
        // missing common ancestor.
        fs::remove_file(&object).unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        match anchored_records(&repo, "main", side) {
            Err(WorkStartError::Repository(_)) => {}
            other => panic!("expected a repository error, got {other:?}"),
        }
    }

    #[test]
    fn r16_declared_target_does_not_hide_unreadable_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project-management/tasks/TSK-001.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"---\nid: TSK-001\n---\n#\xff").unwrap();
        assert!(declared_work_target(dir.path(), "TSK-001").is_err());
    }

    use std::fs;
    #[cfg(unix)]
    use std::process::Command;

    #[cfg(unix)]
    struct RestorePermissions {
        path: PathBuf,
        original: fs::Permissions,
    }

    #[cfg(unix)]
    impl RestorePermissions {
        fn deny(path: &Path) -> Self {
            use std::os::unix::fs::PermissionsExt;

            let original = fs::metadata(path).unwrap().permissions();
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            Self {
                path: path.to_path_buf(),
                original,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.path, self.original.clone());
        }
    }

    const TASK_TEXT: &[u8] = b"---\nid: TSK-003\nepic_id: null\nstandalone_reason: bounded outcome\nintegration_target: main\ntitle: late\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n";

    /// Issue 79: git2's `Tree::walk` gives the parent path as text and aborts
    /// the whole walk at a directory whose name is not UTF-8. A record next to
    /// such a directory, at the root or under `project-management`, is still
    /// read, and a directory with that name is never taken for a record path.
    #[test]
    fn records_are_read_beside_a_directory_that_is_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, commit) = crate::git::repo_with_tree(
            dir.path(),
            &[
                (b"project-management/tasks/TSK-003.md", TASK_TEXT),
                (b"project-management/notes\xff/TSK-004.md", TASK_TEXT),
                (b"src/dir\xff/f.rs", b"x"),
                (b"project-management/tasks/TSK-005\xff.md", TASK_TEXT),
            ],
        );
        let tree = repo.find_commit(commit).unwrap().tree().unwrap();
        let records = records_from_tree(&repo, &tree).expect("the walk reaches the record");
        assert_eq!(records.keys().collect::<Vec<_>>(), ["TSK-003"]);
        let home = tree.get_path(Path::new("project-management")).unwrap();
        assert!(home.kind() == Some(git2::ObjectType::Tree));
    }

    #[test]
    fn tree_path_classifier_matches_only_supported_record_layouts() {
        assert_eq!(
            record_kind_for_tree_path("project-management/tasks/TSK-001.md"),
            Some(RecordKind::Task)
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/epics/EPC-001/tasks/TSK-001-001.md"),
            Some(RecordKind::Task)
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/epics/EPC-001.md"),
            Some(RecordKind::Epic)
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/epics/EPC-001/EPC-001.md"),
            Some(RecordKind::Epic)
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/specs/SPC-001.md"),
            Some(RecordKind::Spec)
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/epics/EPC-001/notes.md"),
            None
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/epics/notes.md"),
            None
        );
        assert_eq!(
            record_kind_for_tree_path("project-management/tasks/notes.md"),
            None
        );
        assert_eq!(record_kind_for_tree_path("docs/tasks/TSK-001.md"), None);
    }

    /// A committed config whose `.codeflow` tree or blob this clone lacks
    /// is an error naming the revision and path, never an absent state.
    #[test]
    fn a_missing_committed_state_object_is_never_absent() {
        for missing in [".codeflow", ".codeflow/project.toml"] {
            let dir = tempfile::tempdir().unwrap();
            git(dir.path(), &["init", "-q", "-b", "main"]);
            fs::create_dir_all(dir.path().join(".codeflow")).unwrap();
            fs::write(
                dir.path().join(".codeflow/project.toml"),
                "schema_version = 1\ntier = \"full\"\nscaffold_version = \"3.0.0\"\nstack = \"rust\"\nareas = []\npolicy_armed = true\ngit_hooks = \"wired\"\npermission_preset = \"default\"\n",
            )
            .unwrap();
            git(dir.path(), &["add", "-A"]);
            git(
                dir.path(),
                &[
                    "-c",
                    "user.name=T",
                    "-c",
                    "user.email=t@example.com",
                    "commit",
                    "-q",
                    "-m",
                    "state",
                ],
            );
            assert_eq!(
                durable_work_tracking_enabled_at(dir.path(), "HEAD"),
                Ok(true)
            );
            let repo = Repository::open(dir.path()).unwrap();
            let tree = repo.head().unwrap().peel_to_tree().unwrap();
            let id = tree.get_path(Path::new(missing)).unwrap().id().to_string();
            fs::remove_file(
                dir.path()
                    .join(".git/objects")
                    .join(&id[..2])
                    .join(&id[2..]),
            )
            .unwrap();
            let error = durable_work_tracking_enabled_at(dir.path(), "HEAD").unwrap_err();
            assert!(
                error.starts_with("HEAD: ") && error.contains("cannot be read"),
                "{missing}: {error}"
            );
            assert!(error.contains(".codeflow"), "{missing}: {error}");
        }
    }

    fn git(root: &Path, args: &[&str]) {
        let status = crate::git::command()
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-b", "main"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        for child in ["epics", "specs", "tasks"] {
            fs::create_dir_all(dir.path().join("project-management").join(child)).unwrap();
        }
        fs::write(
            dir.path().join("project-management/epics/EPC-001.md"),
            "---\nid: EPC-001\ntitle: outcome\nstatus: planning\nwork_type: feat\nspecs: [SPC-001]\ncreated: 2026-07-29\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("project-management/specs/SPC-001.md"),
            "---\nid: SPC-001\ntitle: contract\nstatus: approved\ncreated: 2026-07-29\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("project-management/tasks/TSK-001.md"),
            "---\nid: TSK-001\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: predecessor\nstatus: complete\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("project-management/tasks/TSK-002.md"),
            "---\nid: TSK-002\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: work\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: [TSK-001]\ncreated: 2026-07-29\n---\n",
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "plan"]);
        git(dir.path(), &["switch", "-c", "task/TSK-002-work"]);
        dir
    }

    #[test]
    fn epic_line_binding_requires_an_explicit_standalone_target() {
        let dir = fixture();
        let root = dir.path();
        let line = "integration/EPC-001-outcome";
        git(root, &["switch", "main"]);
        let path = root.join("project-management/tasks/TSK-001.md");
        fs::write(
            &path,
            fs::read_to_string(&path).unwrap().replace(
                "integration_target: main",
                &format!("integration_target: {line}"),
            ),
        )
        .unwrap();
        git(root, &["commit", "-am", "bind epic line"]);
        git(root, &["switch", "task/TSK-002-work"]);
        let path = root.join("project-management/tasks/TSK-002.md");
        let missing_reason =
            "standalone task on branch 'task/TSK-002-work' has no nonblank standalone_reason";
        let wrong_target = "standalone task on branch 'task/TSK-002-work' declares target 'main', not this line 'integration/EPC-001-outcome'";
        // Another epic may declare the line at another trusted anchor
        // (TSK-1286), so an epic with no record here is unbound, not refused.
        let other_epic = EpicLineBinding::Unbound(
            "names EPC-001, which is not the epic of branch 'task/TSK-002-work', and EPC-002 has no epic record on protected branch".to_string(),
        );
        let refused = |reason: &str| EpicLineBinding::Refused(reason.to_string());
        for (epic, reason, target, expected) in [
            ("null", Some("bounded repair"), line, EpicLineBinding::Bound),
            (
                "null",
                Some("bounded repair"),
                "refs/heads/integration/EPC-001-outcome",
                EpicLineBinding::Bound,
            ),
            ("EPC-001", Some("null"), line, EpicLineBinding::Bound),
            ("EPC-002", Some("null"), line, other_epic),
            ("null", None, line, refused(missing_reason)),
            ("null", Some("null"), line, refused(missing_reason)),
            ("null", Some("' '"), line, refused(missing_reason)),
            (
                "null",
                Some("bounded repair"),
                "main",
                refused(wrong_target),
            ),
        ] {
            let reason_field =
                reason.map_or_else(String::new, |value| format!("standalone_reason: {value}\n"));
            fs::write(&path, format!("---\nid: TSK-002\nepic_id: {epic}\n{reason_field}integration_target: {target}\nstatus: todo\n---\n")).unwrap();
            git(root, &["commit", "-am", "declare target"]);
            let binding = epic_line_binding(
                root,
                "task/TSK-002-work",
                "HEAD",
                line,
                "main",
                "protected branch",
            )
            .unwrap();
            assert_eq!(binding, expected, "{epic}, {reason:?}, {target}");
        }
        for branch in [
            "task/TSK-099-missing",
            "fix/untracked",
            "plan/EPC-002-other",
        ] {
            assert!(
                matches!(
                    epic_line_binding(root, branch, "HEAD", line, "main", "protected branch")
                        .unwrap(),
                    EpicLineBinding::Refused(_)
                ),
                "{branch}"
            );
        }
    }

    const DECLARED_LINE: &str = "integration/EPC-001-outcome";

    /// An epic record for EPC-002 with an optional `parent_line` and status.
    fn second_epic(parent_line: Option<&str>, status: &str) -> String {
        let field = parent_line.map_or_else(String::new, |line| format!("parent_line: {line}\n"));
        format!("---\nid: EPC-002\ntitle: feature\nstatus: {status}\nwork_type: feat\nspecs: []\n{field}created: 2026-07-29\n---\n")
    }

    /// TSK-1286: `main` (the trusted anchor) binds the line through TSK-001 of
    /// EPC-001 and holds EPC-002 as `anchor_epic` (none: no record); the
    /// task branch carries TSK-002 of EPC-002 targeting the line and its own
    /// copy of EPC-002 as `head_epic` (none: deleted).
    fn declared_fixture(anchor_epic: Option<&str>, head_epic: Option<&str>) -> tempfile::TempDir {
        let dir = fixture();
        let root = dir.path();
        git(root, &["switch", "main"]);
        let first = root.join("project-management/tasks/TSK-001.md");
        fs::write(
            &first,
            fs::read_to_string(&first).unwrap().replace(
                "integration_target: main",
                &format!("integration_target: {DECLARED_LINE}"),
            ),
        )
        .unwrap();
        let epic = root.join("project-management/epics/EPC-002.md");
        if let Some(text) = anchor_epic {
            fs::write(&epic, text).unwrap();
        }
        git(root, &["add", "."]);
        git(root, &["commit", "-m", "anchor records"]);
        git(root, &["switch", "task/TSK-002-work"]);
        git(root, &["merge", "--ff-only", "-q", "main"]);
        match head_epic {
            Some(text) => fs::write(&epic, text).unwrap(),
            None => {
                let _ = fs::remove_file(&epic);
            }
        }
        fs::write(
            root.join("project-management/tasks/TSK-002.md"),
            format!("---\nid: TSK-002\nepic_id: EPC-002\nstandalone_reason: null\nintegration_target: {DECLARED_LINE}\nstatus: todo\n---\n"),
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "task of the declaring epic"]);
        dir
    }

    fn bind_declared(root: &Path) -> EpicLineBinding {
        epic_line_binding(
            root,
            "task/TSK-002-work",
            "HEAD",
            DECLARED_LINE,
            "main",
            "protected branch",
        )
        .unwrap()
    }

    /// TSK-1286: a task of an epic whose record at the trusted anchor declares
    /// the line as its `parent_line` binds the line. Before the change the
    /// same records were refused as "not the epic of branch".
    #[test]
    fn declared_parent_line_binds_a_task_of_another_epic() {
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let dir = declared_fixture(Some(declared.as_str()), Some(declared.as_str()));
        assert_eq!(bind_declared(dir.path()), EpicLineBinding::Bound);
        assert_eq!(
            task_line_binding(
                dir.path(),
                "TSK-002",
                "HEAD",
                DECLARED_LINE,
                &[("main", "protected branch")],
            )
            .unwrap(),
            EpicLineBinding::Bound
        );
    }

    /// TSK-1286 fail-closed cases: each binds nothing, with the reason named.
    #[test]
    fn declared_parent_line_fails_closed() {
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let plain = second_epic(None, "planning");
        let elsewhere = second_epic(Some("integration/EPC-009-elsewhere"), "planning");
        let cancelled = second_epic(Some(DECLARED_LINE), "cancelled");
        // (case, EPC-002 at the anchor, EPC-002 at the head, refused, reason)
        let cases = [
            // The head adds the declaration the anchor lacks: not trusted.
            (
                "head adds",
                Some(plain.as_str()),
                Some(declared.as_str()),
                false,
                "is not trusted",
            ),
            (
                "anchor lacks",
                Some(plain.as_str()),
                Some(plain.as_str()),
                false,
                "declares no parent_line",
            ),
            (
                "other line",
                Some(elsewhere.as_str()),
                Some(elsewhere.as_str()),
                false,
                "not 'integration/EPC-001-outcome'",
            ),
            (
                "missing epic",
                None,
                Some(declared.as_str()),
                false,
                "EPC-002 has no epic record on protected branch",
            ),
            // A cancelled declaring epic is a refusal, which no other anchor
            // can outweigh.
            (
                "cancelled",
                Some(cancelled.as_str()),
                Some(cancelled.as_str()),
                true,
                "EPC-002 is cancelled",
            ),
            // The head changes or removes the anchor's declaration: refused.
            (
                "head changes",
                Some(declared.as_str()),
                Some(elsewhere.as_str()),
                true,
                "a branch cannot change the line",
            ),
            (
                "head removes field",
                Some(declared.as_str()),
                Some(plain.as_str()),
                true,
                "declares none",
            ),
            (
                "head deletes epic",
                Some(declared.as_str()),
                None,
                true,
                "declares none",
            ),
        ];
        for (case, anchor, head, refused, text) in cases {
            let dir = declared_fixture(anchor, head);
            let binding = bind_declared(dir.path());
            let matched = match &binding {
                EpicLineBinding::Refused(reason) => refused && reason.contains(text),
                EpicLineBinding::Unbound(reason) => !refused && reason.contains(text),
                EpicLineBinding::Bound => false,
            };
            assert!(matched, "{case}: {binding:?}");
            let task = task_line_binding(
                dir.path(),
                "TSK-002",
                "HEAD",
                DECLARED_LINE,
                &[("main", "protected branch")],
            )
            .unwrap();
            assert_ne!(task, EpicLineBinding::Bound, "{case}: {task:?}");
        }
    }

    /// TSK-1286: only an epic record carries `parent_line`; a task record that
    /// carries it is refused even when its epic's declaration would bind.
    #[test]
    fn parent_line_on_a_task_record_is_refused() {
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let dir = declared_fixture(Some(declared.as_str()), Some(declared.as_str()));
        let root = dir.path();
        let task = root.join("project-management/tasks/TSK-002.md");
        fs::write(
            &task,
            fs::read_to_string(&task).unwrap().replacen(
                "---\n",
                &format!("---\nparent_line: {DECLARED_LINE}\n"),
                1,
            ),
        )
        .unwrap();
        git(root, &["commit", "-am", "task carries the field"]);
        for binding in [
            bind_declared(root),
            task_line_binding(
                root,
                "TSK-002",
                "HEAD",
                DECLARED_LINE,
                &[("main", "protected branch")],
            )
            .unwrap(),
        ] {
            assert!(
                matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains("only an epic record may declare")),
                "{binding:?}"
            );
        }
    }

    /// TSK-1286: the line stays bound when every task on it belongs to a
    /// declaring epic, and a declaration through a third epic's line never
    /// chains to it.
    #[test]
    fn declared_tasks_alone_bind_the_line_one_hop_only() {
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let dir = declared_fixture(Some(declared.as_str()), Some(declared.as_str()));
        let root = dir.path();
        git(root, &["switch", "main"]);
        let first = root.join("project-management/tasks/TSK-001.md");
        fs::write(
            &first,
            fs::read_to_string(&first)
                .unwrap()
                .replace("epic_id: EPC-001", "epic_id: EPC-002"),
        )
        .unwrap();
        git(root, &["commit", "-am", "move the line's only task"]);
        assert_eq!(
            epic_line_bound_at(root, DECLARED_LINE, "main", "protected branch").unwrap(),
            EpicLineBinding::Bound
        );
        // EPC-003 declares EPC-002's line, not EPC-001's: no chain.
        fs::write(
            root.join("project-management/epics/EPC-002.md"),
            second_epic(None, "planning"),
        )
        .unwrap();
        fs::write(
            root.join("project-management/epics/EPC-003.md"),
            "---\nid: EPC-003\ntitle: chained\nstatus: planning\nwork_type: feat\nspecs: []\nparent_line: integration/EPC-002-feature\ncreated: 2026-07-29\n---\n",
        )
        .unwrap();
        fs::write(
            &first,
            fs::read_to_string(&first)
                .unwrap()
                .replace("epic_id: EPC-002", "epic_id: EPC-003"),
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-m", "chain through a third epic"]);
        assert!(matches!(
            epic_line_bound_at(root, DECLARED_LINE, "main", "protected branch").unwrap(),
            EpicLineBinding::Unbound(_)
        ));
    }

    /// TSK-1286 review F1: once the anchor holds a line epic that itself
    /// declares a `parent_line` and a declaring epic's task on the line, the
    /// whole line refuses, its own epic's tasks included, until the chain or
    /// the declaring tasks are removed through a planning pull request.
    #[test]
    fn a_chained_line_refuses_its_own_epics_tasks_too() {
        let cause =
            "EPC-001, the epic of 'integration/EPC-001-outcome', itself declares parent_line";
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let dir = declared_fixture(Some(declared.as_str()), Some(declared.as_str()));
        let root = dir.path();
        git(root, &["switch", "main"]);
        fs::write(
            root.join("project-management/tasks/TSK-003.md"),
            format!("---\nid: TSK-003\nepic_id: EPC-002\nstandalone_reason: null\nintegration_target: {DECLARED_LINE}\nstatus: todo\n---\n"),
        )
        .unwrap();
        let owner = root.join("project-management/epics/EPC-001.md");
        fs::write(
            &owner,
            fs::read_to_string(&owner).unwrap().replacen(
                "---\n",
                "---\nparent_line: integration/EPC-009-elsewhere\n",
                1,
            ),
        )
        .unwrap();
        git(root, &["add", "-A"]);
        git(
            root,
            &["commit", "-qm", "a chained line with a declaring task"],
        );
        git(root, &["switch", "-q", "task/TSK-002-work"]);
        git(root, &["merge", "-q", "--no-edit", "main"]);
        // The branch's task now belongs to the line's own epic.
        let task = root.join("project-management/tasks/TSK-002.md");
        fs::write(
            &task,
            fs::read_to_string(&task)
                .unwrap()
                .replace("epic_id: EPC-002", "epic_id: EPC-001"),
        )
        .unwrap();
        git(root, &["commit", "-qam", "an own-epic task"]);
        for (reader, binding) in [
            ("own-epic branch", bind_declared(root)),
            (
                "line tip",
                epic_line_bound_at(root, DECLARED_LINE, "main", "protected branch").unwrap(),
            ),
        ] {
            assert!(
                matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains(cause)),
                "{reader}: {binding:?}"
            );
        }
    }

    /// TSK-1286 AC-3: one level only. When the line's own epic itself
    /// declares a `parent_line` at the trusted anchor, a task of an epic
    /// that declares the line is refused, naming the cause, in every reader:
    /// the branch binding, the task binding, the line's binding at its tip
    /// and the line proof. Before this check all four bound or verified.
    #[test]
    fn an_anchor_epic_that_declares_a_parent_line_refuses_declared_tasks() {
        let cause = "EPC-001, the epic of 'integration/EPC-001-outcome', itself declares parent_line 'integration/EPC-009-elsewhere' on";
        let chained = |path: &Path| {
            fs::write(
                path,
                fs::read_to_string(path).unwrap().replacen(
                    "---\n",
                    "---\nparent_line: integration/EPC-009-elsewhere\n",
                    1,
                ),
            )
            .unwrap();
        };
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let dir = declared_fixture(Some(declared.as_str()), Some(declared.as_str()));
        let root = dir.path();
        git(root, &["switch", "main"]);
        // The line's only task on the anchor belongs to the declaring epic,
        // and the line's own epic declares a parent line of its own.
        let first = root.join("project-management/tasks/TSK-001.md");
        fs::write(
            &first,
            fs::read_to_string(&first)
                .unwrap()
                .replace("epic_id: EPC-001", "epic_id: EPC-002"),
        )
        .unwrap();
        chained(&root.join("project-management/epics/EPC-001.md"));
        git(
            root,
            &["commit", "-qam", "the line's epic declares a parent"],
        );
        git(root, &["switch", "-q", "task/TSK-002-work"]);
        git(root, &["merge", "-q", "--no-edit", "main"]);
        for (reader, binding) in [
            ("branch", bind_declared(root)),
            (
                "task",
                task_line_binding(
                    root,
                    "TSK-002",
                    "HEAD",
                    DECLARED_LINE,
                    &[("main", "protected branch")],
                )
                .unwrap(),
            ),
            (
                "line tip",
                epic_line_bound_at(root, DECLARED_LINE, "main", "protected branch").unwrap(),
            ),
        ] {
            assert!(
                matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains(cause)),
                "{reader}: {binding:?}"
            );
        }
        let proof_dir = proof_fixture(true, Some(&declared));
        let proof_root = proof_dir.path();
        git(proof_root, &["switch", "-q", "main"]);
        chained(&proof_root.join("project-management/epics/EPC-001.md"));
        git(
            proof_root,
            &["commit", "-qam", "the line's epic declares a parent"],
        );
        git(proof_root, &["switch", "-q", DECLARED_LINE]);
        git(proof_root, &["merge", "-q", "--no-edit", "main"]);
        let proof =
            epic_line_proof(proof_root, DECLARED_LINE, "main", "main", DECLARED_LINE).unwrap();
        assert!(
            matches!(&proof, EpicLineProof::NotLine(reason) if reason.contains(cause)),
            "line proof: {proof:?}"
        );
    }

    /// TSK-1286 review: a declaring epic cancelled at one trusted anchor
    /// refuses even when another anchor still declares the line, in either
    /// order. At 5dcc8d8ce the declaring anchor bound the line.
    #[test]
    fn a_cancelled_declaring_epic_at_any_anchor_refuses() {
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let cancelled = second_epic(Some(DECLARED_LINE), "cancelled");
        let dir = declared_fixture(Some(cancelled.as_str()), Some(declared.as_str()));
        let root = dir.path();
        git(root, &["switch", "-q", "-c", "anchor/declaring", "main"]);
        fs::write(root.join("project-management/epics/EPC-002.md"), &declared).unwrap();
        git(root, &["commit", "-qam", "an anchor that still declares"]);
        git(root, &["switch", "-q", "task/TSK-002-work"]);
        for anchors in [
            [
                ("main", "the candidate authority"),
                ("anchor/declaring", "the line's tip"),
            ],
            [
                ("anchor/declaring", "the line's tip"),
                ("main", "the candidate authority"),
            ],
        ] {
            let binding =
                task_line_binding(root, "TSK-002", "HEAD", DECLARED_LINE, &anchors).unwrap();
            assert!(
                matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains("EPC-002 is cancelled on the candidate authority")),
                "{anchors:?}: {binding:?}"
            );
        }
        assert!(matches!(bind_declared(root), EpicLineBinding::Refused(_)));
    }

    /// A line `integration/EPC-001-outcome` merged from a `work` branch that
    /// holds TSK-002 of EPC-002 targeting the line and its copy of EPC-002
    /// as `head_epic` (none: deleted). With `on_target`, `main` holds
    /// the same task and an active declaring EPC-002 too.
    fn proof_fixture(on_target: bool, head_epic: Option<&str>) -> tempfile::TempDir {
        let dir = fixture();
        let root = dir.path();
        let epic = root.join("project-management/epics/EPC-002.md");
        let task = root.join("project-management/tasks/TSK-002.md");
        let declared_task = format!("---\nid: TSK-002\nepic_id: EPC-002\nstandalone_reason: null\nintegration_target: {DECLARED_LINE}\nstatus: todo\n---\n");
        git(root, &["switch", "-q", "main"]);
        if on_target {
            fs::write(&epic, second_epic(Some(DECLARED_LINE), "planning")).unwrap();
            fs::write(&task, &declared_task).unwrap();
            git(root, &["add", "-A"]);
            git(root, &["commit", "-qm", "the target declares"]);
        }
        git(root, &["switch", "-q", "-c", DECLARED_LINE]);
        git(root, &["switch", "-q", "-c", "work"]);
        match head_epic {
            Some(content) => fs::write(&epic, content).unwrap(),
            None if epic.exists() => fs::remove_file(&epic).unwrap(),
            None => {}
        }
        fs::write(&task, &declared_task).unwrap();
        fs::write(root.join("work.txt"), "work\n").unwrap();
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "work on the line"]);
        git(root, &["switch", "-q", DECLARED_LINE]);
        git(root, &["merge", "-q", "--no-ff", "-m", "land work", "work"]);
        dir
    }

    /// TSK-1286 review: the proof that lands a line on `main` trusts a
    /// declaration only from the target or the line's fetched tip, never
    /// from the head alone, and a cancellation at the head refuses. At
    /// 5dcc8d8ce the head-only declaration was verified.
    #[test]
    fn epic_line_proof_trusts_only_anchor_declarations() {
        let proof = |root: &Path| {
            epic_line_proof(root, DECLARED_LINE, "main", "main", DECLARED_LINE).unwrap()
        };
        let declaring = second_epic(Some(DECLARED_LINE), "planning");
        let head_only = proof_fixture(false, Some(&declaring));
        assert!(
            matches!(proof(head_only.path()), EpicLineProof::NotLine(reason) if reason.contains("nor a task of an epic declaring parent_line")),
            "head only"
        );
        let trusted = proof_fixture(true, Some(&declaring));
        assert!(
            matches!(proof(trusted.path()), EpicLineProof::Verified(..)),
            "declared on the target"
        );
        let cancelled = proof_fixture(true, Some(&second_epic(Some(DECLARED_LINE), "cancelled")));
        assert!(
            matches!(proof(cancelled.path()), EpicLineProof::NotLine(reason) if reason.contains("EPC-002 is cancelled on the head")),
            "cancelled at the head"
        );
    }

    /// TSK-1286 review of fac11e366: with a matching task and declaration
    /// on the target, a head that changes the declaration, removes it or
    /// deletes the declaring epic refuses, as the task binding does. At
    /// fac11e366 all three were verified.
    #[test]
    fn epic_line_proof_refuses_a_head_that_changes_a_trusted_declaration() {
        let changed = second_epic(Some("integration/EPC-001-elsewhere"), "planning");
        let removed = second_epic(None, "planning");
        for (case, head_epic) in [
            ("changed", Some(changed.as_str())),
            ("removed", Some(removed.as_str())),
            ("deleted", None),
        ] {
            let dir = proof_fixture(true, head_epic);
            let proof =
                epic_line_proof(dir.path(), DECLARED_LINE, "main", "main", DECLARED_LINE).unwrap();
            assert!(
                matches!(&proof, EpicLineProof::NotLine(reason) if reason.contains("EPC-002 declares parent_line 'integration/EPC-001-outcome' on the target, but") && reason.contains("a branch cannot change or remove")),
                "{case}: {proof:?}"
            );
        }
    }

    /// Point `origin/<DECLARED_LINE>` in `root` at a commit off the line
    /// whose `epic` (an epic id) is cancelled, leaving the line unchanged.
    fn cancel_at_fetched_tip(root: &Path, epic: &str) {
        let record = root.join(format!("project-management/epics/{epic}.md"));
        git(root, &["switch", "-q", "-c", "fetched", DECLARED_LINE]);
        fs::write(
            &record,
            fs::read_to_string(&record)
                .unwrap()
                .replace("status: planning", "status: cancelled"),
        )
        .unwrap();
        git(
            root,
            &["commit", "-qam", "cancel an epic at the fetched tip"],
        );
        git(
            root,
            &[
                "update-ref",
                &format!("refs/remotes/origin/{DECLARED_LINE}"),
                "fetched",
            ],
        );
        git(root, &["switch", "-q", DECLARED_LINE]);
    }

    /// Give the line in `root` a task of its own epic: TSK-001 of EPC-001
    /// targets the line on `main`, and the line merges `main`.
    fn add_own_task(root: &Path) {
        git(root, &["switch", "-q", "main"]);
        let first = root.join("project-management/tasks/TSK-001.md");
        fs::write(
            &first,
            fs::read_to_string(&first).unwrap().replace(
                "integration_target: main",
                &format!("integration_target: {DECLARED_LINE}"),
            ),
        )
        .unwrap();
        git(root, &["commit", "-qam", "the line's own task"]);
        git(root, &["switch", "-q", DECLARED_LINE]);
        git(root, &["merge", "-q", "--no-ff", "-m", "take main", "main"]);
    }

    fn proof_of(root: &Path) -> EpicLineProof {
        epic_line_proof(root, DECLARED_LINE, "main", "main", DECLARED_LINE).unwrap()
    }

    /// TSK-1286 review of fac11e366: the line's own epic cancelled at the
    /// line's fetched tip refuses the proof while the target and the head
    /// keep it active, both through a declaring epic and through a task of
    /// the line's own epic. At fac11e366 both were verified.
    #[test]
    fn epic_line_proof_refuses_an_owner_cancelled_at_the_fetched_tip() {
        let expected = format!("EPC-001 is cancelled on 'origin/{DECLARED_LINE}'");
        let declared = proof_fixture(true, Some(&second_epic(Some(DECLARED_LINE), "planning")));
        assert!(matches!(
            proof_of(declared.path()),
            EpicLineProof::Verified(..)
        ));
        cancel_at_fetched_tip(declared.path(), "EPC-001");
        assert!(
            matches!(proof_of(declared.path()), EpicLineProof::NotLine(ref reason) if reason == &expected),
            "declared path"
        );
        let own = proof_fixture(false, None);
        let root = own.path();
        add_own_task(root);
        assert!(
            matches!(proof_of(root), EpicLineProof::Verified(..)),
            "own task"
        );
        cancel_at_fetched_tip(root, "EPC-001");
        assert!(
            matches!(proof_of(root), EpicLineProof::NotLine(ref reason) if reason == &expected),
            "own-task path"
        );
    }

    /// TSK-1286 review of 7b07c2f57: a task of the line's own epic never
    /// skips a declaration refusal. With both an own-epic task and a task of
    /// a declaring epic on the target, a head that changes, removes or
    /// deletes the declaration, a declaring epic cancelled at the head, and
    /// one cancelled at the line's fetched tip each refuse. At 7b07c2f57 all
    /// were verified through the own-task shortcut.
    #[test]
    fn epic_line_proof_checks_declarations_beside_an_own_task() {
        let declaring = second_epic(Some(DECLARED_LINE), "planning");
        let baseline = proof_fixture(true, Some(&declaring));
        add_own_task(baseline.path());
        assert!(
            matches!(proof_of(baseline.path()), EpicLineProof::Verified(..)),
            "mixed tasks, declaration kept"
        );
        let changed = second_epic(Some("integration/EPC-001-elsewhere"), "planning");
        let removed = second_epic(None, "planning");
        let cancelled = second_epic(Some(DECLARED_LINE), "cancelled");
        let mutation =
            "EPC-002 declares parent_line 'integration/EPC-001-outcome' on the target, but";
        for (case, head_epic, text) in [
            ("changed", Some(changed.as_str()), mutation),
            ("removed", Some(removed.as_str()), mutation),
            ("deleted", None, mutation),
            (
                "cancelled at the head",
                Some(cancelled.as_str()),
                "EPC-002 is cancelled on the head",
            ),
        ] {
            let dir = proof_fixture(true, head_epic);
            add_own_task(dir.path());
            let proof = proof_of(dir.path());
            assert!(
                matches!(&proof, EpicLineProof::NotLine(reason) if reason.contains(text)),
                "{case}: {proof:?}"
            );
        }
        cancel_at_fetched_tip(baseline.path(), "EPC-002");
        let proof = proof_of(baseline.path());
        assert!(
            matches!(&proof, EpicLineProof::NotLine(reason) if reason.contains(&format!("EPC-002 is cancelled on 'origin/{DECLARED_LINE}'"))),
            "cancelled at the fetched tip: {proof:?}"
        );
    }

    /// TSK-1286 review of 7b07c2f57: the task bindings read the head before
    /// any anchor. The line's own epic cancelled on the head refuses for a
    /// task of a declaring epic and for a task of the line's own epic, and
    /// a head that removes a trusted declaration refuses even at an anchor
    /// that holds no task of the line. At 7b07c2f57 the declaring-epic task
    /// bound and the taskless anchor was only unbound.
    #[test]
    fn task_bindings_refuse_head_changes_before_any_anchor() {
        let declared = second_epic(Some(DECLARED_LINE), "planning");
        let task = |root: &Path| {
            task_line_binding(
                root,
                "TSK-002",
                "HEAD",
                DECLARED_LINE,
                &[("main", "protected branch")],
            )
            .unwrap()
        };
        let owner_text = "names EPC-001, which is cancelled on the head";
        let dir = declared_fixture(Some(declared.as_str()), Some(declared.as_str()));
        let root = dir.path();
        let owner = root.join("project-management/epics/EPC-001.md");
        fs::write(
            &owner,
            fs::read_to_string(&owner)
                .unwrap()
                .replace("status: planning", "status: cancelled"),
        )
        .unwrap();
        git(
            root,
            &["commit", "-qam", "the head cancels the line's epic"],
        );
        for (case, binding) in [
            ("declared", bind_declared(root)),
            ("declared task", task(root)),
        ] {
            assert!(
                matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains(owner_text)),
                "{case}: {binding:?}"
            );
        }
        let path = root.join("project-management/tasks/TSK-002.md");
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("epic_id: EPC-002", "epic_id: EPC-001"),
        )
        .unwrap();
        git(root, &["commit", "-qam", "a task of the line's own epic"]);
        for (case, binding) in [("own", bind_declared(root)), ("own task", task(root))] {
            assert!(
                matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains(owner_text)),
                "{case}: {binding:?}"
            );
        }
        let plain = second_epic(None, "planning");
        let dir = declared_fixture(Some(declared.as_str()), Some(plain.as_str()));
        let root = dir.path();
        git(root, &["switch", "-q", "-c", "anchor/no-task", "main"]);
        let first = root.join("project-management/tasks/TSK-001.md");
        fs::write(
            &first,
            fs::read_to_string(&first).unwrap().replace(
                &format!("integration_target: {DECLARED_LINE}"),
                "integration_target: main",
            ),
        )
        .unwrap();
        git(
            root,
            &["commit", "-qam", "an anchor with no task of the line"],
        );
        git(root, &["switch", "-q", "task/TSK-002-work"]);
        let binding = epic_line_binding(
            root,
            "task/TSK-002-work",
            "HEAD",
            DECLARED_LINE,
            "anchor/no-task",
            "the line's tip",
        )
        .unwrap();
        assert!(
            matches!(&binding, EpicLineBinding::Refused(reason) if reason.contains("declares none")),
            "removed at a taskless anchor: {binding:?}"
        );
    }

    /// TSK-1286: the record reader takes `parent_line` only as an exact epic
    /// line name, on any record kind, so a binding can refuse it on a task.
    #[test]
    fn parent_line_is_read_only_as_an_exact_epic_line() {
        let epic = |value: &str| {
            parse_record(
                &format!("---\nid: EPC-002\nstatus: planning\nparent_line: {value}\n---\n"),
                RecordKind::Epic,
            )
        };
        assert_eq!(
            epic(DECLARED_LINE).unwrap().parent_line.as_deref(),
            Some(DECLARED_LINE)
        );
        assert_eq!(epic("null").unwrap().parent_line, None);
        for bad in [
            "origin/integration/EPC-001-outcome",
            "refs/heads/integration/EPC-001-outcome",
            "EPC-001",
            "main",
            "[integration/EPC-001-outcome]",
            "7",
            // An empty slug, and slugs that are not valid branch names.
            "integration/EPC-001-",
            "'integration/EPC-001-bad name'",
            "integration/EPC-001-a..b",
            "integration/EPC-001-a~b",
        ] {
            assert!(epic(bad).is_err(), "{bad}");
        }
        let task = parse_record(
            &format!("---\nid: TSK-002\nstatus: todo\nparent_line: {DECLARED_LINE}\n---\n"),
            RecordKind::Task,
        )
        .unwrap();
        assert_eq!(task.parent_line.as_deref(), Some(DECLARED_LINE));
    }

    /// Pins of different tasks at one revision read that revision's graph
    /// once: a stack of many reviewed predecessors stays linear.
    #[test]
    fn pins_at_one_revision_parse_its_graph_once() {
        let dir = fixture();
        git(dir.path(), &["branch", "task/TSK-001-work"]);
        let repo = Repository::open(dir.path()).unwrap();
        let revision = repo.head().unwrap().peel_to_commit().unwrap().id();
        let branches = PinBranches::read(&repo, dir.path()).unwrap();
        for (task_id, branch) in [
            ("TSK-001", "task/TSK-001-work"),
            ("TSK-002", "task/TSK-002-work"),
        ] {
            let pin = ReviewedPin {
                task_id: task_id.into(),
                revision,
            };
            assert_eq!(
                branches.pin_branch(&repo, &pin).unwrap(),
                GitName::from_text(branch)
            );
        }
        assert_eq!(branches.task_paths.borrow().len(), 1);
    }

    fn replace_on_main(
        dir: &tempfile::TempDir,
        relative: &str,
        from: &str,
        to: &str,
        message: &str,
        branch: &str,
    ) {
        git(dir.path(), &["switch", "main"]);
        let path = dir.path().join(relative);
        let body = fs::read_to_string(&path).unwrap().replace(from, to);
        fs::write(path, body).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", message]);
        git(dir.path(), &["switch", "-c", branch]);
    }

    #[test]
    fn anchored_graph_passes_without_mutating_repo() {
        let dir = fixture();
        let before_head = fs::read_to_string(dir.path().join(".git/HEAD")).expect("HEAD readable");
        let before_status = crate::git::command()
            .arg("-C")
            .arg(dir.path())
            .args(["status", "--porcelain"])
            .output()
            .unwrap()
            .stdout;
        let report = check_work_start(dir.path(), "TSK-002", "main").unwrap();
        assert_eq!(report.epic_id.as_deref(), Some("EPC-001"));
        assert_eq!(report.specs, ["SPC-001"]);
        assert_eq!(report.dependencies, ["TSK-001"]);
        assert_eq!(
            fs::read_to_string(dir.path().join(".git/HEAD")).unwrap(),
            before_head
        );
        let after_status = crate::git::command()
            .arg("-C")
            .arg(dir.path())
            .args(["status", "--porcelain"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(before_status, after_status);
    }

    /// TSK-104 AC-2: the preflight runs on every sanctioned work prefix that
    /// carries the task id, keyed on that id.
    #[test]
    fn preflight_runs_on_every_work_prefix_carrying_the_id() {
        let dir = fixture();
        for branch in [
            "task/TSK-002-work",
            "fix/TSK-002-repair",
            "feat/TSK-002-thing",
            "spike/TSK-002-probe",
        ] {
            assert_eq!(
                task_id_from_branch(dir.path(), branch).unwrap().as_deref(),
                Some("TSK-002"),
                "{branch}"
            );
            let report = check_work_start_for_branch(dir.path(), "TSK-002", "main", branch)
                .unwrap_or_else(|error| panic!("{branch}: {error}"));
            assert_eq!(report.branch, branch);
            assert_eq!(report.work_type.as_deref(), Some("feat"));
        }
        for branch in ["plan/TSK-002-graph", "integration/TSK-002-line", "fix/typo"] {
            assert_eq!(
                task_id_from_branch(dir.path(), branch).unwrap(),
                None,
                "{branch}"
            );
        }
        assert!(branch_claims_task_id(dir.path(), "fix/TSK-404-ghost").unwrap());
        assert!(!branch_claims_task_id(dir.path(), "fix/typo").unwrap());
        assert!(!branch_claims_task_id(dir.path(), "plan/TSK-404-new").unwrap());
        assert!(matches!(
            check_work_start_for_branch(dir.path(), "TSK-002", "main", "fix/TSK-001-other"),
            Err(WorkStartError::Branch { .. })
        ));
    }

    /// The pull request context: a `fix/` branch without an id may still
    /// name a task, whose anchored state is checked all the same.
    #[test]
    fn anchored_check_needs_no_branch_identity() {
        let dir = fixture();
        git(dir.path(), &["switch", "-c", "fix/release-notes"]);
        let report = check_work_start_anchored(dir.path(), "TSK-002", "main").unwrap();
        assert!(report.branch.is_empty());
        assert!(matches!(
            check_work_start_anchored(dir.path(), "TSK-001", "main"),
            Err(WorkStartError::TaskNotStartable { .. })
        ));
    }

    #[test]
    fn standalone_record_at_head_starts_but_closed_record_does_not() {
        let dir = fixture();
        let path = dir.path().join("project-management/tasks/TSK-003.md");
        let text = "---\nid: TSK-003\nepic_id: null\nstandalone_reason: bounded outcome\nintegration_target: main\ntitle: late\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n";
        fs::write(&path, text).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "standalone"]);
        git(dir.path(), &["branch", "-m", "task/TSK-003-late"]);
        assert!(check_work_start(dir.path(), "TSK-003", "main").is_ok());
        fs::write(&path, text.replace("status: todo", "status: complete")).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "complete"]);
        assert!(matches!(
            check_work_start(dir.path(), "TSK-003", "main"),
            Err(WorkStartError::TaskNotStartable { .. })
        ));
    }

    #[test]
    fn task_created_only_on_feature_branch_is_not_anchored() {
        let dir = fixture();
        fs::write(
            dir.path().join("project-management/tasks/TSK-003.md"),
            "---\nid: TSK-003\nepic_id: EPC-001\nstandalone_reason: null\nintegration_target: main\ntitle: late\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: []\ncreated: 2026-07-29\n---\n",
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "late task"]);
        git(dir.path(), &["branch", "-m", "task/TSK-003-late"]);
        assert!(matches!(
            check_work_start(dir.path(), "TSK-003", "main"),
            Err(WorkStartError::TaskNotAnchored(_))
        ));
    }

    #[test]
    fn draft_spec_blocks_start() {
        let dir = fixture();
        git(dir.path(), &["switch", "main"]);
        let path = dir.path().join("project-management/specs/SPC-001.md");
        let body = fs::read_to_string(&path)
            .unwrap()
            .replace("status: approved", "status: draft");
        fs::write(path, body).unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "draft spec"]);
        git(dir.path(), &["switch", "-c", "task/TSK-002-draft"]);
        assert!(matches!(
            check_work_start(dir.path(), "TSK-002", "main"),
            Err(WorkStartError::SpecNotReady { .. })
        ));
    }

    #[test]
    fn declared_target_is_read_from_current_record() {
        let dir = fixture();
        assert_eq!(
            declared_work_target(dir.path(), "TSK-002")
                .unwrap()
                .as_deref(),
            Some("main")
        );
    }

    /// Configure `remote` with the usual fetch refspec, and optionally make
    /// it `main`'s upstream.
    fn add_remote(dir: &Path, remote: &str, upstream_of_main: bool) {
        git(
            dir,
            &["config", &format!("remote.{remote}.url"), "/nowhere"],
        );
        git(
            dir,
            &[
                "config",
                &format!("remote.{remote}.fetch"),
                &format!("+refs/heads/*:refs/remotes/{remote}/*"),
            ],
        );
        if upstream_of_main {
            git(dir, &["config", "branch.main.remote", remote]);
            git(dir, &["config", "branch.main.merge", "refs/heads/main"]);
        }
    }

    /// Point `refs/remotes/<remote>/main` one commit past `main`.
    fn advance_remote_main(dir: &Path, remote: &str) {
        git(dir, &["checkout", "-q", "-b", "ahead"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", remote]);
        git(
            dir,
            &["update-ref", &format!("refs/remotes/{remote}/main"), "HEAD"],
        );
        git(dir, &["checkout", "-q", "main"]);
        git(dir, &["branch", "-q", "-D", "ahead"]);
    }

    /// A fixture whose `main` tracks `origin/main`, with a commit that only
    /// one of them has when `local_extra` / `remote_extra` are set.
    fn tracking_fixture(local_extra: bool, remote_extra: bool) -> tempfile::TempDir {
        let dir = fixture();
        add_remote(dir.path(), "origin", true);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        if remote_extra {
            advance_remote_main(dir.path(), "origin");
        }
        if local_extra {
            git(
                dir.path(),
                &["commit", "-q", "--allow-empty", "-m", "local"],
            );
        }
        dir
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_configured_upstream_never_falls_back_to_origin() {
        let dir = tracking_fixture(false, true);
        let repo = Repository::open(dir.path()).unwrap();
        let oid = repo.head().unwrap().peel_to_commit().unwrap().id();
        let mut packed = format!("{oid} refs/heads/upstream-").into_bytes();
        packed.extend_from_slice(b"\xff\n");
        std::fs::write(repo.path().join("packed-refs"), packed).unwrap();
        let config_path = repo.path().join("config");
        let mut config = std::fs::read(&config_path).unwrap();
        config.extend_from_slice(
            b"\n[branch \"main\"]\nremote = .\nmerge = refs/heads/upstream-\xff\n",
        );
        std::fs::write(config_path, config).unwrap();
        let repo = Repository::open(dir.path()).unwrap();
        let upstream = repo.branch_upstream_name("refs/heads/main").unwrap();
        assert!(
            upstream.as_str().is_err(),
            "fixture must exercise failed decoding"
        );
        assert!(resolve_work_target_checked(dir.path(), Some("main")).is_err());
        assert!(resolve_work_target(dir.path(), Some("main")).is_err());
    }

    #[test]
    fn target_names_preserve_unicode_whitespace() {
        let dir = fixture();
        for name in ["release\u{a0}", "\u{a0}"] {
            git(dir.path(), &["branch", name]);
            assert!(is_stable_work_target(name));
            let resolved = resolve_work_target_checked(dir.path(), Some(name))
                .unwrap()
                .unwrap();
            assert_eq!(resolved.target, name);
            assert!(
                target_reference(&Repository::open(dir.path()).unwrap(), name)
                    .unwrap()
                    .is_some()
            );
        }
    }

    #[test]
    fn a_target_without_a_configured_upstream_uses_fetched_origin() {
        let dir = fixture();
        add_remote(dir.path(), "origin", false);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        advance_remote_main(dir.path(), "origin");
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "refs/remotes/origin/main");
    }

    #[test]
    fn a_fork_origin_never_replaces_the_configured_upstream() {
        // SR-1: `main` tracks `upstream/main` (equal); the fork's newer
        // `origin/main` holds planning that never reached upstream.
        let dir = fixture();
        add_remote(dir.path(), "upstream", true);
        add_remote(dir.path(), "origin", false);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/upstream/main", "HEAD"],
        );
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        advance_remote_main(dir.path(), "origin");
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        // The bare name, which reads the configured `upstream/main`, never
        // the fork's `origin/main` (TSK-416).
        assert_eq!(resolved.target, "main");
        let repo = Repository::open(dir.path()).unwrap();
        assert_eq!(
            target_reference(&repo, &resolved.target)
                .unwrap()
                .unwrap()
                .id(),
            repo.find_reference("refs/remotes/upstream/main")
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
        );
    }

    #[test]
    fn a_local_upstream_is_anchored_by_its_exact_ref() {
        // SR-2: `main` tracks the local branch `origin/main`
        // (`branch.main.remote = .`), while a remote `origin/main` exists
        // too. The ref compared must be the ref anchored on. A local
        // upstream is not a fetched line, so the fetched `origin/main` is
        // read, by its exact ref, never the author-made local branch
        // (TSK-416).
        let dir = fixture();
        add_remote(dir.path(), "origin", false);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        advance_remote_main(dir.path(), "origin");
        git(dir.path(), &["branch", "-q", "origin/main", "main"]);
        git(dir.path(), &["checkout", "-q", "origin/main"]);
        git(
            dir.path(),
            &["commit", "-q", "--allow-empty", "-m", "local upstream"],
        );
        git(dir.path(), &["checkout", "-q", "main"]);
        git(dir.path(), &["config", "branch.main.remote", "."]);
        git(
            dir.path(),
            &["config", "branch.main.merge", "refs/heads/origin/main"],
        );
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "refs/remotes/origin/main");
        let repo = Repository::discover(dir.path()).unwrap();
        let anchored = target_reference(&repo, &resolved.target)
            .unwrap()
            .unwrap()
            .id();
        let fetched = repo
            .find_reference("refs/remotes/origin/main")
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id();
        assert_eq!(anchored, fetched);
        assert!(is_stable_work_target(&resolved.target));
    }

    #[test]
    fn a_target_behind_its_configured_upstream_moves_to_it() {
        let dir = fixture();
        add_remote(dir.path(), "upstream", true);
        add_remote(dir.path(), "origin", false);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        git(
            dir.path(),
            &["update-ref", "refs/remotes/upstream/main", "HEAD"],
        );
        advance_remote_main(dir.path(), "upstream");
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "refs/remotes/upstream/main");
        assert!(is_stable_work_target(&resolved.target));
        assert!(work_target_resolves(dir.path(), &resolved.target));
    }

    #[test]
    fn a_target_equal_to_its_tracking_ref_resolves_to_the_local_branch() {
        let dir = tracking_fixture(false, false);
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "main");
    }

    #[test]
    fn a_local_target_strictly_behind_resolves_to_the_tracking_ref() {
        let dir = tracking_fixture(false, true);
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "refs/remotes/origin/main");
        assert_eq!(
            resolve_work_target(dir.path(), Some("main"))
                .unwrap()
                .as_deref(),
            Some("refs/remotes/origin/main")
        );
    }

    #[test]
    fn a_local_target_only_ahead_resolves_to_the_local_branch() {
        let dir = tracking_fixture(true, false);
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "main");
    }

    #[test]
    fn a_diverged_target_is_refused_naming_both_sides() {
        let dir = tracking_fixture(true, true);
        let error = resolve_work_target_checked(dir.path(), Some("main")).unwrap_err();
        assert!(
            matches!(
                &error,
                WorkStartError::DivergedTarget {
                    ahead: 1,
                    behind: 1,
                    ..
                }
            ),
            "{error:?}"
        );
        let message = error.to_string();
        assert!(
            message.contains("'main' and 'refs/remotes/origin/main' have diverged"),
            "{message}"
        );
        // TSK-416 AC-9: both tips by commit, and a remedy that runs under the
        // guard, never a rebase or reset of the local branch.
        let repo = Repository::open(dir.path()).unwrap();
        for reference in ["refs/heads/main", "refs/remotes/origin/main"] {
            let tip = repo
                .find_reference(reference)
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string();
            assert!(message.contains(&tip), "{reference}: {message}");
        }
        // TSK-416 review: a worktree shares the diverged branch, so the
        // remedy is `--into` or a fresh clone, named by its remote.
        for part in [
            "`--into refs/remotes/origin/main`",
            "a worktree of this clone shares its branches",
            "a fresh clone of the remote origin",
            "`git remote get-url origin`",
            "switch to the task branch there",
        ] {
            assert!(message.contains(part), "{part}: {message}");
        }
        assert!(!message.contains("worktree add"), "{message}");
        assert!(
            !message.contains("rebase") && !message.contains("reset"),
            "{message}"
        );
        // The unchecked resolver keeps its old answer, the local branch, by
        // its exact ref, for non-anchoring callers.
        assert_eq!(
            resolve_work_target(dir.path(), Some("main"))
                .unwrap()
                .as_deref(),
            Some("refs/heads/main")
        );
    }

    /// TSK-416 AC-9: a fetched tip that names no readable commit, a missing
    /// object or a blob, refuses in every reader of the target, naming the
    /// cause, the ref, the object it points at and a remedy that runs under
    /// the guard, never `update-ref` or a fetch with a destination.
    #[test]
    fn an_unresolvable_fetched_tip_refuses_naming_the_ref_and_a_guarded_remedy() {
        let dir = tracking_fixture(false, false);
        let repo = Repository::open(dir.path()).unwrap();
        let blob = repo.blob(b"not a commit").unwrap().to_string();
        let missing = "1".repeat(40);
        // A missing object stops every fetch, so the remedy is a fresh
        // clone; an object that is here and is not a commit is rewritten by
        // a plain fetch.
        let fresh_clone = [
            "a fetch cannot rewrite the ref while its object is missing",
            "a fresh clone of the remote origin, made from outside this clone (`git clone <url> <path>`, where `git remote get-url origin` prints the url)",
            "`git branch -d -r origin/main`, then `git fetch origin`",
            "which the guard leaves to the operator",
        ];
        let fetch = ["plain `git fetch origin`, which rewrites refs/remotes/origin/main"];
        for (points_at, remedy) in [(missing, &fresh_clone[..]), (blob, &fetch[..])] {
            fs::write(
                dir.path().join(".git/refs/remotes/origin/main"),
                format!("{points_at}\n"),
            )
            .unwrap();
            let repo = Repository::open(dir.path()).unwrap();
            for (reader, error) in [
                (
                    "resolve_work_target_checked",
                    resolve_work_target_checked(dir.path(), Some("main")).unwrap_err(),
                ),
                (
                    "target_reference_read",
                    target_reference_read(&repo, "main").unwrap_err(),
                ),
                (
                    "tracking_reference",
                    tracking_reference(&repo, "main").unwrap_err(),
                ),
            ] {
                let message = error.to_string();
                for part in [
                    "refs/remotes/origin/main points at",
                    points_at.as_str(),
                    "not a commit this clone can read",
                    "the fetched tip cannot be resolved",
                ]
                .iter()
                .chain(remedy)
                {
                    assert!(message.contains(part), "{reader}: {part}: {message}");
                }
                assert!(!message.contains("update-ref"), "{reader}: {message}");
                assert!(!message.contains("unavailable"), "{reader}: {message}");
            }
        }
    }

    /// TSK-416 review: the remedy names the remote and never prints its
    /// URL, whose userinfo, query or path a reader must not see or paste,
    /// and each command it prints reaches a shell as the arguments it
    /// names, whatever the remote's name holds. Each command without a
    /// `<placeholder>` runs here under a shell whose `git` prints its
    /// arguments.
    #[test]
    #[cfg(unix)]
    fn an_unresolvable_tip_remedy_prints_no_url_and_quotes_each_argument() {
        for (remote, url) in [
            (
                "origin",
                "https://u:FAKE_TOKEN_3b9@h.invalid/x.git?access_token=FAKE_TOKEN_3b9#FAKE_TOKEN_3b9",
            ),
            ("origin", "/srv/with space/x.git"),
            // A backtick would split the message's own code spans, which
            // this test reads; `shell_arg`'s test covers it.
            ("o$(id);x", "/nowhere"),
            // Git reads an operand that starts with `-` as an option
            // wherever it stands, unless `--` ends the options first.
            ("-x", "/nowhere"),
            ("--upload-pack=touch_pwned", "/nowhere"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            Repository::init(root).unwrap();
            git(root, &["config", &format!("remote.{remote}.url"), url]);
            let refs = root.join(".git/refs/remotes").join(remote);
            fs::create_dir_all(&refs).unwrap();
            fs::write(refs.join("main"), format!("{}\n", "1".repeat(40))).unwrap();
            let message =
                unresolvable_reference(root, &format!("refs/remotes/{remote}/main")).unwrap();
            let commands: Vec<&str> = message
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|span| span.starts_with("git ") && !span.contains('<'))
                .collect();
            for command in &commands {
                let out = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("git() {{ printf '%s\\n' \"$@\"; }}; {command}"))
                    .output()
                    .unwrap();
                assert!(out.status.success(), "{command}");
                let printed = String::from_utf8_lossy(&out.stdout);
                let last = printed.lines().last().unwrap_or_default();
                assert!(
                    last == remote || last == format!("{remote}/main"),
                    "{command} gave {printed}"
                );
                assert!(!printed.contains("uid="), "{command} ran id: {printed}");
                // Before `--`, the only dashed arguments are the command's
                // own options.
                for argument in printed.lines().take_while(|argument| *argument != "--") {
                    assert!(
                        !argument.starts_with('-') || ["-d", "-r"].contains(&argument),
                        "{command} passes {argument} where git reads options: {printed}"
                    );
                }
            }
            for hidden in ["FAKE_TOKEN", "with space", "/nowhere", "h.invalid"] {
                assert!(!message.contains(hidden), "{hidden}: {message}");
            }
            // The fetch, the ref deletion and the URL lookup.
            assert_eq!(commands.len(), 3, "{message}");
        }
    }

    /// TSK-416 review: for a remote named like an option, git runs each
    /// command the remedy prints as written, and following them in order
    /// repairs the fetched ref: the URL lookup prints the remote's URL, the
    /// deletion removes the ref whose object is missing and the fetch
    /// recreates it from the remote.
    #[test]
    #[cfg(unix)]
    fn a_dashed_remote_remedy_runs_with_git_as_printed() {
        for remote in ["-x", "--upload-pack=touch_pwned"] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let source = root.to_str().unwrap();
            git(root, &["init", "-q", "-b", "main"]);
            git(
                root,
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.test",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "tip",
                ],
            );
            git(root, &["config", &format!("remote.{remote}.url"), source]);
            git(
                root,
                &[
                    "config",
                    &format!("remote.{remote}.fetch"),
                    &format!("+refs/heads/*:refs/remotes/{remote}/*"),
                ],
            );
            let refs = root.join(".git/refs/remotes").join(remote);
            fs::create_dir_all(&refs).unwrap();
            fs::write(refs.join("main"), format!("{}\n", "1".repeat(40))).unwrap();
            let fetched = format!("refs/remotes/{remote}/main");
            let message = unresolvable_reference(root, &fetched).unwrap();
            let commands: Vec<&str> = message
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|span| span.starts_with("git ") && !span.contains('<'))
                .collect();
            assert_eq!(commands.len(), 3, "{message}");
            for command in commands {
                let out = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(format!("git() {{ printf '%s\\n' \"$@\"; }}; {command}"))
                    .output()
                    .unwrap();
                let printed = String::from_utf8_lossy(&out.stdout).into_owned();
                let arguments: Vec<&str> = printed.lines().collect();
                let ran = crate::git::command()
                    .arg("-C")
                    .arg(root)
                    .args(&arguments)
                    .output()
                    .unwrap();
                assert!(
                    ran.status.success(),
                    "{command}: {}",
                    String::from_utf8_lossy(&ran.stderr)
                );
                if arguments.contains(&"get-url") {
                    assert_eq!(String::from_utf8_lossy(&ran.stdout).trim(), source);
                }
            }
            let repo = Repository::open(root).unwrap();
            let head = repo.head().unwrap().target().unwrap();
            assert_eq!(
                repo.find_reference(&fetched).unwrap().target(),
                Some(head),
                "{message}"
            );
        }
    }

    #[test]
    fn declared_target_can_resolve_through_remote_tracking_ref() {
        let dir = fixture();
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/release", "HEAD"],
        );
        assert_eq!(
            resolve_work_target(dir.path(), Some("release"))
                .unwrap()
                .as_deref(),
            Some("origin/release")
        );
        git(
            dir.path(),
            &["update-ref", "refs/remotes/upstream/release", "HEAD"],
        );
        assert_eq!(
            resolve_work_target(dir.path(), Some("refs/remotes/upstream/release"))
                .unwrap()
                .as_deref(),
            Some("refs/remotes/upstream/release")
        );
        assert!(work_target_resolves(dir.path(), "release"));
        assert!(work_target_resolves(
            dir.path(),
            "refs/remotes/upstream/release"
        ));
        assert!(!work_target_resolves(dir.path(), "missing"));
    }

    #[test]
    fn mismatched_target_is_rejected() {
        let dir = fixture();
        git(dir.path(), &["branch", "release"]);
        assert!(matches!(
            check_work_start(dir.path(), "TSK-002", "release"),
            Err(WorkStartError::TargetMismatch { .. })
        ));
    }

    #[test]
    fn malformed_identity_and_task_branch_target_fail_before_graph_read() {
        let dir = fixture();
        assert!(matches!(
            check_work_start_for_branch(dir.path(), "not-a-task", "main", "task/not-a-task-work"),
            Err(WorkStartError::InvalidGraph(_))
        ));
        assert!(matches!(
            check_work_start_for_branch(dir.path(), "TSK-002", "main", "fix/wrong"),
            Err(WorkStartError::Branch { .. })
        ));
        assert!(matches!(
            check_work_start(dir.path(), "TSK-002", "refs/heads/task/TSK-002-work"),
            Err(WorkStartError::UnstableTarget(_))
        ));
        for target in ["HEAD", "origin/HEAD", "HEAD~1", "refs/tags/v1.0.0"] {
            assert!(
                !is_stable_work_target(target),
                "{target} must not be accepted as a branch authority"
            );
            assert!(matches!(
                check_work_start(dir.path(), "TSK-002", target),
                Err(WorkStartError::UnstableTarget(_))
            ));
        }

        let object_id = "0123456789abcdef0123456789abcdef01234567";
        assert!(!is_stable_work_target(object_id));
        assert!(!work_target_resolves(dir.path(), object_id));
        assert!(matches!(
            check_work_start(dir.path(), "TSK-002", object_id),
            Err(WorkStartError::UnstableTarget(_))
        ));
    }

    #[test]
    fn hexadecimal_branch_name_can_be_a_real_target() {
        let dir = fixture();
        let target = "deadbeef";
        git(dir.path(), &["branch", target, "main"]);
        assert!(is_stable_work_target(target));
        assert!(work_target_resolves(dir.path(), target));
    }

    #[test]
    fn durable_work_tracking_is_tier_and_layout_aware() {
        let absent = tempfile::tempdir().unwrap();
        assert!(!durable_work_tracking_enabled(absent.path()).unwrap());

        std::fs::create_dir_all(absent.path().join("project-management/tasks")).unwrap();
        assert!(!durable_work_tracking_enabled(absent.path()).unwrap());
        std::fs::write(
            absent.path().join("project-management/tasks/notes.md"),
            "foreign notes",
        )
        .unwrap();
        assert!(!durable_work_tracking_enabled(absent.path()).unwrap());
        std::fs::write(
            absent.path().join("project-management/tasks/TSK-001.md"),
            "malformed CodeFlow-shaped record",
        )
        .unwrap();
        assert!(durable_work_tracking_enabled(absent.path()).unwrap());

        let full = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(full.path().join(".codeflow")).unwrap();
        std::fs::write(
            full.path().join(".codeflow/project.toml"),
            r#"schema_version = 1
tier = "full"
scaffold_version = "3.0.0"
stack = "rust"
areas = []
policy_armed = true
git_hooks = "wired"
permission_preset = "strict"
"#,
        )
        .unwrap();
        assert!(durable_work_tracking_enabled(full.path()).unwrap());
    }

    #[test]
    fn durable_work_tracking_rejects_indeterminate_state_and_keeps_legacy_records() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("project.toml");
        std::fs::write(&state_path, "tier = [malformed").unwrap();
        assert!(matches!(
            durable_work_tracking_enabled(dir.path()),
            Err(DurableTrackingError::StateRead(_))
        ));
        std::fs::write(
            &state_path,
            r#"schema_version = 9
tier = "minimal"
scaffold_version = "3.0.0"
stack = "rust"
areas = []
policy_armed = true
git_hooks = "wired"
permission_preset = "strict"
"#,
        )
        .unwrap();
        assert!(matches!(
            durable_work_tracking_enabled(dir.path()),
            Err(DurableTrackingError::UnsupportedStateVersion(9))
        ));
        std::fs::write(
            &state_path,
            r#"schema_version = 1
tier = "standard"
scaffold_version = "3.0.0"
stack = "rust"
areas = []
policy_armed = true
git_hooks = "wired"
permission_preset = "strict"
"#,
        )
        .unwrap();
        assert!(!durable_work_tracking_enabled(dir.path()).unwrap());
        std::fs::create_dir_all(dir.path().join("project-management/epics/EPC-001/tasks")).unwrap();
        std::fs::write(
            dir.path()
                .join("project-management/epics/EPC-001/tasks/TSK-001-001.md"),
            "malformed CodeFlow-shaped record",
        )
        .unwrap();
        assert!(durable_work_tracking_enabled(dir.path()).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn durable_work_tracking_rejects_nonregular_and_dangling_state_paths() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".codeflow");
        std::fs::create_dir_all(&state_dir).unwrap();
        symlink("absent.toml", state_dir.join("project.toml")).unwrap();
        assert!(matches!(
            durable_work_tracking_enabled(dir.path()),
            Err(DurableTrackingError::StatePath(_))
        ));
        std::fs::remove_file(state_dir.join("project.toml")).unwrap();
        std::fs::create_dir(state_dir.join("project.toml")).unwrap();
        assert!(matches!(
            durable_work_tracking_enabled(dir.path()),
            Err(DurableTrackingError::StatePath(_))
        ));
        std::fs::remove_dir(state_dir.join("project.toml")).unwrap();
        let fifo = state_dir.join("project.toml");
        let created = Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(created.success());
        assert!(matches!(
            durable_work_tracking_enabled(dir.path()),
            Err(DurableTrackingError::StatePath(_))
        ));
        std::fs::remove_file(&fifo).unwrap();
        std::fs::remove_dir(&state_dir).unwrap();
        symlink("missing-state-dir", &state_dir).unwrap();
        assert!(matches!(
            durable_work_tracking_enabled(dir.path()),
            Err(DurableTrackingError::StatePath(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn durable_work_tracking_reports_unreadable_state_and_task_home() {
        use std::io::ErrorKind;

        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".codeflow");
        fs::create_dir_all(&state_dir).unwrap();
        let state_path = state_dir.join("project.toml");
        fs::write(
            &state_path,
            r#"schema_version = 1
tier = "minimal"
scaffold_version = "3.0.0"
stack = "rust"
areas = []
policy_armed = true
git_hooks = "wired"
permission_preset = "strict"
"#,
        )
        .unwrap();
        let denied_state = RestorePermissions::deny(&state_path);
        match fs::read(&state_path) {
            Ok(_) => eprintln!("EACCES state probe unavailable under this test identity"),
            Err(error) => {
                assert_eq!(error.kind(), ErrorKind::PermissionDenied);
                assert!(matches!(
                    durable_work_tracking_enabled(dir.path()),
                    Err(DurableTrackingError::StateRead(_))
                ));
            }
        }
        drop(denied_state);
        assert!(!durable_work_tracking_enabled(dir.path()).unwrap());
        fs::remove_file(&state_path).unwrap();

        let tasks = dir.path().join("project-management/tasks");
        fs::create_dir_all(&tasks).unwrap();
        let denied_tasks = RestorePermissions::deny(&tasks);
        match fs::read_dir(&tasks) {
            Ok(_) => eprintln!("EACCES task-home probe unavailable under this test identity"),
            Err(error) => {
                assert_eq!(error.kind(), ErrorKind::PermissionDenied);
                assert!(matches!(
                    durable_work_tracking_enabled(dir.path()),
                    Err(DurableTrackingError::TaskInventoryUnreadable)
                ));
            }
        }
        drop(denied_tasks);
        assert!(!durable_work_tracking_enabled(dir.path()).unwrap());
    }

    #[test]
    fn canonical_task_requires_target_and_open_status() {
        let missing_target = fixture();
        replace_on_main(
            &missing_target,
            "project-management/tasks/TSK-002.md",
            "integration_target: main\n",
            "",
            "remove target",
            "task/TSK-002-no-target",
        );
        assert!(matches!(
            check_work_start(missing_target.path(), "TSK-002", "main"),
            Err(WorkStartError::MissingIntegrationTarget(_))
        ));

        let closed = fixture();
        replace_on_main(
            &closed,
            "project-management/tasks/TSK-002.md",
            "status: todo",
            "status: cancelled",
            "cancel task",
            "task/TSK-002-closed",
        );
        assert!(matches!(
            check_work_start(closed.path(), "TSK-002", "main"),
            Err(WorkStartError::TaskNotStartable { .. })
        ));

        let blocked = fixture();
        replace_on_main(
            &blocked,
            "project-management/tasks/TSK-002.md",
            "status: todo",
            "status: blocked",
            "block task",
            "task/TSK-002-blocked",
        );
        assert!(matches!(
            check_work_start(blocked.path(), "TSK-002", "main"),
            Err(WorkStartError::Blocked { .. })
        ));
    }

    #[test]
    fn parent_and_standalone_contracts_are_anchored() {
        let missing_epic = fixture();
        replace_on_main(
            &missing_epic,
            "project-management/tasks/TSK-002.md",
            "epic_id: EPC-001",
            "epic_id: EPC-999",
            "break parent",
            "task/TSK-002-parent",
        );
        assert!(matches!(
            check_work_start(missing_epic.path(), "TSK-002", "main"),
            Err(WorkStartError::MissingEpic { .. })
        ));

        let unjustified = fixture();
        replace_on_main(
            &unjustified,
            "project-management/tasks/TSK-002.md",
            "epic_id: EPC-001",
            "epic_id: null",
            "remove parent",
            "task/TSK-002-standalone",
        );
        assert!(matches!(
            check_work_start(unjustified.path(), "TSK-002", "main"),
            Err(WorkStartError::MissingStandaloneReason(_))
        ));
    }

    #[test]
    fn missing_spec_and_dependency_fail_closed() {
        let missing_spec = fixture();
        replace_on_main(
            &missing_spec,
            "project-management/epics/EPC-001.md",
            "specs: [SPC-001]",
            "specs: [SPC-999]",
            "break spec",
            "task/TSK-002-spec",
        );
        assert!(matches!(
            check_work_start(missing_spec.path(), "TSK-002", "main"),
            Err(WorkStartError::MissingSpec { .. })
        ));

        let missing_dependency = fixture();
        replace_on_main(
            &missing_dependency,
            "project-management/tasks/TSK-002.md",
            "depends_on: [TSK-001]",
            "depends_on: [TSK-999]",
            "break dependency",
            "task/TSK-002-dependency",
        );
        assert!(matches!(
            check_work_start(missing_dependency.path(), "TSK-002", "main"),
            Err(WorkStartError::MissingDependency { .. })
        ));

        let incomplete = fixture();
        replace_on_main(
            &incomplete,
            "project-management/tasks/TSK-001.md",
            "status: complete",
            "status: in-progress",
            "reopen dependency",
            "task/TSK-002-incomplete",
        );
        assert!(matches!(
            check_work_start(incomplete.path(), "TSK-002", "main"),
            Err(WorkStartError::DependencyIncomplete { .. })
        ));
    }

    #[test]
    fn unrelated_markdown_outside_project_management_is_ignored() {
        let dir = fixture();
        git(dir.path(), &["switch", "main"]);
        fs::create_dir_all(dir.path().join("docs/tasks")).unwrap();
        fs::write(
            dir.path().join("docs/tasks/TSK-NOTE.md"),
            "---\nid: definitely-not-a-task\n---\n",
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "docs"]);
        git(dir.path(), &["switch", "-c", "task/TSK-002-docs"]);
        assert!(check_work_start(dir.path(), "TSK-002", "main").is_ok());
    }

    #[test]
    fn duplicate_anchored_identity_is_a_specific_graph_error() {
        let dir = fixture();
        git(dir.path(), &["switch", "main"]);
        fs::create_dir_all(dir.path().join("project-management/epics/EPC-001")).unwrap();
        fs::copy(
            dir.path().join("project-management/epics/EPC-001.md"),
            dir.path()
                .join("project-management/epics/EPC-001/EPC-001.md"),
        )
        .unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-m", "duplicate epic"]);
        git(dir.path(), &["switch", "-c", "task/TSK-002-duplicate"]);

        let error = check_work_start(dir.path(), "TSK-002", "main").unwrap_err();
        assert!(matches!(error, WorkStartError::InvalidGraph(_)));
        assert!(error.to_string().contains("duplicate work id"));
    }

    /// Review finding on issue 79: a remote whose name is not valid UTF-8 and
    /// that carries the branch cannot name a hosted repository, so the lookup
    /// refuses instead of choosing another remote.
    #[test]
    fn a_remote_that_is_not_utf8_has_no_hosted_repository_to_guess() {
        let dir = tempfile::tempdir().unwrap();
        crate::git::repo_with_refs(dir.path(), &[b"refs/remotes/caf\xe9/task/TSK-001-x"]);
        let repositories = ReviewRepositories::open(dir.path()).unwrap();
        let error = repositories.repository("task/TSK-001-x").unwrap_err();
        assert!(error.contains("not valid UTF-8"), "{error}");
    }

    /// Round six on issue 79: a `branch.<name>.remote` that is not valid UTF-8
    /// is a refusal, never a fall back to `origin` (another repository).
    #[test]
    fn a_configured_remote_that_is_not_utf8_is_refused_not_replaced_by_origin() {
        use std::io::Write as _;
        let dir = tempfile::tempdir().unwrap();
        crate::git::repo_with_refs(dir.path(), &[b"refs/heads/task/TSK-001-x"]);
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(".git").join("config"))
            .unwrap();
        config
            .write_all(
                b"[remote \"origin\"]\n\turl = https://github.com/wrong/owner.git\n\
                  [branch \"task/TSK-001-x\"]\n\tremote = caf\xff\n",
            )
            .unwrap();
        let repositories = ReviewRepositories::open(dir.path()).unwrap();
        let error = repositories.repository("task/TSK-001-x").unwrap_err();
        assert!(error.contains("not valid UTF-8"), "{error}");
    }

    /// Review finding on issue 79: one branch whose name is not valid UTF-8
    /// failed the whole listing of work branches. It is read as `GitName` bytes.
    #[test]
    fn a_branch_that_is_not_utf8_does_not_fail_the_branch_listing() {
        let dir = tempfile::tempdir().unwrap();
        let repo = crate::git::repo_with_refs(
            dir.path(),
            &[b"refs/heads/task/TSK-001-caf\xe9", b"refs/heads/other\xff"],
        );
        assert!(PinBranches::read(&repo, dir.path()).is_ok());
    }
    #[test]
    fn r16_unreadable_local_target_cannot_fall_back_to_origin() {
        let dir = tempfile::tempdir().unwrap();
        let (repo, oid) = crate::git::repo_with_tree(dir.path(), &[(b"a", b"x")]);
        repo.reference("refs/remotes/origin/main", oid, true, "fixture")
            .unwrap();
        std::fs::create_dir_all(repo.path().join("refs/heads")).unwrap();
        std::fs::write(
            repo.path().join("refs/heads/main"),
            "1111111111111111111111111111111111111111\n",
        )
        .unwrap();
        // A bare name reads the fetched ref first (TSK-416), so the broken
        // local branch is never read there; anchoring work still compares
        // the local branch with it and refuses.
        assert_eq!(
            target_reference(&repo, "main")
                .unwrap()
                .map(|commit| commit.id()),
            Some(oid)
        );
        assert!(target_reference(&repo, "refs/heads/main").is_err());
        assert!(resolve_work_target_checked(dir.path(), Some("main")).is_err());
    }

    /// TSK-248 AC-1 (issue 85): a direct commit on an epic line is accepted
    /// once its adoption lands by merge or is on the target; an entry still
    /// on an unmerged direct commit leaves the line refused.
    #[test]
    fn epic_line_adoption_accepts_merge_or_target_and_refuses_direct_entry() {
        for on_target in [false, true] {
            let dir = fixture();
            let root = dir.path();
            let line = "integration/EPC-001-outcome";
            git(root, &["switch", "main"]);
            let task = root.join("project-management/tasks/TSK-002.md");
            fs::write(
                &task,
                fs::read_to_string(&task).unwrap().replace(
                    "integration_target: main",
                    &format!("integration_target: {line}"),
                ),
            )
            .unwrap();
            git(root, &["commit", "-am", "bind line"]);
            git(root, &["switch", "-c", line]);
            assert!(check_epic_line(root, line, "main", "main", "HEAD").is_ok());
            fs::write(root.join("repair.txt"), "repair").unwrap();
            git(root, &["add", "."]);
            git(root, &["commit", "-m", "direct repair"]);
            let repo = Repository::open(root).unwrap();
            let direct = repo
                .head()
                .unwrap()
                .peel_to_commit()
                .unwrap()
                .id()
                .to_string();
            assert!(check_epic_line(root, line, "main", "main", "HEAD")
                .unwrap_err()
                .contains(&direct[..9]));
            let epic = root.join("project-management/epics/EPC-001.md");
            let record = fs::read_to_string(&epic).unwrap().replacen("---\n", &format!("---\nline_adoptions:\n  - commit: '{direct}'\n    reason: repair\n    review: https://example.test/pr/1\n"), 1);
            if on_target {
                git(root, &["switch", "main"]);
            } else {
                git(root, &["switch", "-c", "plan/adopt"]);
            }
            fs::write(&epic, record).unwrap();
            git(root, &["commit", "-am", "adopt repair"]);
            if !on_target {
                // The entry arrived by a second direct commit, so both the
                // repaired commit and the record change stay unadopted.
                let error = check_epic_line(root, line, "main", "main", "HEAD").unwrap_err();
                assert!(
                    error.contains("commits made directly on the line")
                        && error.contains(&direct[..9]),
                    "{error}"
                );
            }
            git(root, &["switch", line]);
            git(
                root,
                &[
                    "merge",
                    "--no-ff",
                    "-m",
                    "land adoption",
                    if on_target { "main" } else { "plan/adopt" },
                ],
            );
            assert_eq!(
                check_epic_line(root, line, "main", "main", "HEAD"),
                Ok("EPC-001".into())
            );
        }
    }
    #[test]
    fn pin_containment_refuses_grafts_and_replace_refs() {
        for replace in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let record = |id| {
                format!("---\nid: {id}\nepic_id: null\nstandalone_reason: bounded\nintegration_target: main\nstatus: todo\ndepends_on: []\n---\n")
            };
            let first_record = record("TSK-001");
            let second_record = record("TSK-002");
            let records_dir = dir.path().join("project-management/tasks");
            std::fs::create_dir_all(&records_dir).unwrap();
            std::fs::write(records_dir.join("TSK-001.md"), &first_record).unwrap();
            std::fs::write(records_dir.join("TSK-002.md"), &second_record).unwrap();
            let (repo, base) = crate::git::repo_with_tree(
                dir.path(),
                &[
                    (
                        b"project-management/tasks/TSK-001.md",
                        first_record.as_bytes(),
                    ),
                    (
                        b"project-management/tasks/TSK-002.md",
                        second_record.as_bytes(),
                    ),
                    (b"source.rs", b"original"),
                ],
            );
            crate::git::add_commit(&repo, &[(b"source.rs", b"unreviewed")]);
            let head = crate::git::add_commit(&repo, &[(b"source.rs", b"original")]);
            repo.branch(
                "task/TSK-001-first",
                &repo.find_commit(base).unwrap(),
                false,
            )
            .unwrap();
            repo.branch(
                "task/TSK-002-second",
                &repo.find_commit(head).unwrap(),
                false,
            )
            .unwrap();
            let pins = [
                ReviewedPin {
                    task_id: "TSK-001".into(),
                    revision: base,
                },
                ReviewedPin {
                    task_id: "TSK-002".into(),
                    revision: head,
                },
            ];
            assert_eq!(stack_base(dir.path(), &pins).unwrap(), Some(head));
            if replace {
                repo.reference(&format!("refs/replace/{head}"), base, false, "test")
                    .unwrap();
            } else {
                std::fs::create_dir_all(repo.path().join("info")).unwrap();
                std::fs::write(repo.path().join("info/grafts"), format!("{head} {base}\n"))
                    .unwrap();
            }
            let error = stack_base(dir.path(), &pins).unwrap_err();
            assert!(error.contains("cannot verify"), "{error}");
        }
    }
}

/// TSK-416: a stale copy of a branch, local or remote-tracking, never
/// decides a target read or a reviewed pin; ancestry alone judges a copy.
#[cfg(test)]
mod stale_copy_tests {
    use super::*;
    use std::fs;

    fn git(root: &Path, args: &[&str]) -> String {
        let out = crate::git::command()
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

    fn commit(root: &Path, message: &str) -> String {
        git(root, &["commit", "-q", "--allow-empty", "-m", message]);
        git(root, &["rev-parse", "HEAD"])
    }

    /// `main` holding TSK-001 (todo) and TSK-002, which depends on it, with
    /// an `origin` remote configured but nothing fetched.
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["config", "remote.origin.url", "/nowhere"]);
        git(
            root,
            &[
                "config",
                "remote.origin.fetch",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        );
        fs::create_dir_all(root.join("project-management/tasks")).unwrap();
        for (id, deps) in [("TSK-001", "[]"), ("TSK-002", "[TSK-001]")] {
            fs::write(
                root.join(format!("project-management/tasks/{id}.md")),
                format!("---\nid: {id}\nepic_id: null\nstandalone_reason: bounded\nintegration_target: main\ntitle: work {id}\nstatus: todo\nwork_type: feat\nspecs: []\ndepends_on: {deps}\ncreated: 2026-11-05\n---\n"),
            )
            .unwrap();
        }
        git(root, &["add", "."]);
        git(root, &["commit", "-q", "-m", "plan"]);
        dir
    }

    /// A commit object whose parent is missing, so its ancestry cannot be read.
    fn unreadable_commit(root: &Path) -> String {
        let tree = git(root, &["rev-parse", "HEAD^{tree}"]);
        let body = format!(
            "tree {tree}\nparent {}\nauthor T <t@example.test> 1700000000 +0000\ncommitter T <t@example.test> 1700000000 +0000\n\nbroken\n",
            "1".repeat(40)
        );
        let path = root.join(".git/broken-commit");
        fs::write(&path, body).unwrap();
        let path = path.to_str().unwrap().to_string();
        git(
            root,
            &["hash-object", "-t", "commit", "-w", "--literally", &path],
        )
    }

    #[test]
    fn a_bare_target_reads_the_fetched_ref_before_a_stale_local_branch() {
        let dir = fixture();
        let root = dir.path();
        let stale = git(root, &["rev-parse", "HEAD"]);
        let fetched = commit(root, "fetched");
        git(root, &["update-ref", "refs/remotes/origin/main", &fetched]);
        git(root, &["update-ref", "refs/heads/main", &stale]);
        let repo = Repository::open(root).unwrap();
        let (name, tip) = target_reference_read(&repo, "main").unwrap().unwrap();
        assert_eq!(name, "refs/remotes/origin/main");
        assert_eq!(tip.id().to_string(), fetched);
        assert_eq!(
            target_reference(&repo, "main")
                .unwrap()
                .unwrap()
                .id()
                .to_string(),
            fetched
        );
        // Only when nothing of that name is fetched does the local branch decide.
        git(root, &["update-ref", "-d", "refs/remotes/origin/main"]);
        let (name, tip) = target_reference_read(&repo, "main").unwrap().unwrap();
        assert_eq!(name, "refs/heads/main");
        assert_eq!(tip.id().to_string(), stale);
    }

    /// TSK-416 AC-3: the other readers of a bare target read the fetched
    /// line beside a stale local copy: `work_target_resolves`,
    /// `anchored_records` (readiness and the pre-push range),
    /// `tracking_reference` and `resolve_work_target` (the redaction and
    /// reopen resolver in `status_verb.rs`).
    #[test]
    fn the_other_bare_target_readers_read_the_fetched_line() {
        let dir = fixture();
        let root = dir.path();
        let stale = git(root, &["rev-parse", "HEAD"]);
        let fetched = commit(root, "fetched");
        let head = commit(root, "task work");
        git(root, &["update-ref", "refs/remotes/origin/main", &fetched]);
        git(root, &["update-ref", "refs/heads/main", &stale]);
        let repo = Repository::open(root).unwrap();
        assert!(work_target_resolves(root, "main"));
        let head = Oid::from_str(&head).unwrap();
        let (base, _) = anchored_records(&repo, "main", head).unwrap();
        assert_eq!(base, fetched);
        let (name, tip) = tracking_reference(&repo, "main").unwrap().unwrap();
        assert_eq!(name, "origin/main");
        assert_eq!(tip.to_string(), fetched);
        assert_eq!(
            resolve_work_target(root, Some("main")).unwrap().as_deref(),
            Some("refs/remotes/origin/main")
        );
    }

    /// An upstream naming another branch is never read for the line: a
    /// config write (`git branch -u`, `branch.<b>.merge`) or a line cut from
    /// `origin/main` and pushed without `-u` cannot point it elsewhere.
    #[test]
    fn an_upstream_naming_another_branch_is_never_read_for_the_line() {
        let dir = fixture();
        let root = dir.path();
        let base = git(root, &["rev-parse", "HEAD"]);
        let other = commit(root, "another branch");
        git(
            root,
            &["update-ref", "refs/remotes/origin/feat/fake", &other],
        );
        git(root, &["update-ref", "refs/heads/main", &base]);
        git(root, &["update-ref", "refs/remotes/origin/main", &base]);
        git(root, &["config", "branch.main.remote", "origin"]);
        git(
            root,
            &["config", "branch.main.merge", "refs/heads/feat/fake"],
        );
        let repo = Repository::open(root).unwrap();
        let (name, tip) = target_reference_read(&repo, "main").unwrap().unwrap();
        assert_eq!(name, "refs/remotes/origin/main");
        assert_eq!(tip.id().to_string(), base);
        let resolved = resolve_work_target_checked(root, Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "main");

        // A line cut from `origin/main` tracks it; pushed without `-u`, its
        // own fetched copy is the one read, and a local copy behind that
        // copy resolves to it, never to `origin/main`.
        let line = "integration/x";
        git(root, &["update-ref", &format!("refs/heads/{line}"), &base]);
        git(
            root,
            &["update-ref", &format!("refs/remotes/origin/{line}"), &other],
        );
        git(
            root,
            &["config", &format!("branch.{line}.remote"), "origin"],
        );
        git(
            root,
            &["config", &format!("branch.{line}.merge"), "refs/heads/main"],
        );
        let (name, tip) = target_reference_read(&repo, line).unwrap().unwrap();
        assert_eq!(name, format!("refs/remotes/origin/{line}"));
        assert_eq!(tip.id().to_string(), other);
        let resolved = resolve_work_target_checked(root, Some(line))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, format!("refs/remotes/origin/{line}"));
        // Equal copies keep the bare name, which reads the same commit.
        git(root, &["update-ref", &format!("refs/heads/{line}"), &other]);
        let resolved = resolve_work_target_checked(root, Some(line))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, line);
    }

    /// A fork's `origin/main` never replaces the upstream `main` tracks.
    #[test]
    fn a_bare_target_reads_the_configured_upstream_before_a_fork_origin() {
        let dir = fixture();
        let root = dir.path();
        let upstream = git(root, &["rev-parse", "HEAD"]);
        let fork = commit(root, "fork");
        git(root, &["update-ref", "refs/remotes/origin/main", &fork]);
        git(
            root,
            &["update-ref", "refs/remotes/upstream/main", &upstream],
        );
        git(root, &["update-ref", "refs/heads/main", &upstream]);
        git(root, &["config", "remote.upstream.url", "/nowhere"]);
        git(
            root,
            &[
                "config",
                "remote.upstream.fetch",
                "+refs/heads/*:refs/remotes/upstream/*",
            ],
        );
        git(root, &["config", "branch.main.remote", "upstream"]);
        git(root, &["config", "branch.main.merge", "refs/heads/main"]);
        let repo = Repository::open(root).unwrap();
        let (name, tip) = target_reference_read(&repo, "main").unwrap().unwrap();
        assert_eq!(name, "refs/remotes/upstream/main");
        assert_eq!(tip.id().to_string(), upstream);
    }

    /// AC-11: the pre-push hook's default target (`git_hook.rs`
    /// `epic_line_push`) is read through `default_work_target_ref`, which
    /// names `refs/remotes/origin/main` before the local `main`, so a stale
    /// local `main` never decides the push check.
    #[test]
    fn the_pre_push_default_target_is_the_fetched_main_beside_a_stale_local_one() {
        let dir = fixture();
        let root = dir.path();
        let stale = git(root, &["rev-parse", "HEAD"]);
        let fetched = commit(root, "fetched");
        git(root, &["update-ref", "refs/remotes/origin/main", &fetched]);
        git(root, &["update-ref", "refs/heads/main", &stale]);
        assert_eq!(
            default_work_target_ref(root).unwrap().as_deref(),
            Some("refs/remotes/origin/main")
        );
    }

    /// AC-4: every other spelling keeps its one ref, and a name that is not
    /// a target stays refused.
    #[test]
    fn other_target_spellings_keep_their_one_ref() {
        assert_eq!(
            target_reference_names("main").unwrap(),
            ["refs/remotes/origin/main", "refs/heads/main"]
        );
        for (name, read) in [
            ("origin/main", "refs/remotes/origin/main"),
            ("refs/heads/main", "refs/heads/main"),
            ("refs/remotes/upstream/main", "refs/remotes/upstream/main"),
        ] {
            assert_eq!(target_reference_names(name).unwrap(), [read], "{name}");
        }
        for bad in ["", "HEAD", "refs/tags/v1", "bad..name"] {
            assert!(target_reference_names(bad).is_none(), "{bad}");
        }
    }

    /// The pin `P`, a commit `A` before it and one `C` after it, each on
    /// `task/TSK-001-work` history, and a sibling `D` of `P`.
    struct Shapes {
        before: String,
        pin: String,
        after: String,
        sibling: String,
    }

    fn shapes(root: &Path) -> Shapes {
        git(root, &["switch", "-q", "-c", "task/TSK-001-work"]);
        let before = commit(root, "before");
        let pin = commit(root, "pin");
        let after = commit(root, "after");
        git(root, &["switch", "-q", "--detach", &before]);
        let sibling = commit(root, "sibling");
        git(root, &["switch", "-q", "main"]);
        Shapes {
            before,
            pin,
            after,
            sibling,
        }
    }

    fn copies(root: &Path, local: &str, remote: &str) {
        git(root, &["update-ref", "refs/heads/task/TSK-001-work", local]);
        git(
            root,
            &[
                "update-ref",
                "refs/remotes/origin/task/TSK-001-work",
                remote,
            ],
        );
    }

    fn pin_name_of(root: &Path, pin: &str) -> Result<GitName, String> {
        let repo = Repository::open(root).unwrap();
        let branches = PinBranches::read(&repo, root).unwrap();
        branches.pin_name(
            &repo,
            &ReviewedPin {
                task_id: "TSK-001".into(),
                revision: Oid::from_str(pin).unwrap(),
            },
        )
    }

    /// AC-12: a copy behind the pin, local or remote-tracking, is accepted.
    #[test]
    fn pin_name_accepts_a_copy_behind_the_pin() {
        let dir = fixture();
        let root = dir.path();
        let s = shapes(root);
        for (local, remote) in [(&s.before, &s.pin), (&s.pin, &s.before)] {
            copies(root, local, remote);
            assert_eq!(
                pin_name_of(root, &s.pin).unwrap(),
                GitName::from_text("task/TSK-001-work")
            );
        }
    }

    /// AC-13: a copy ahead of the pin or diverged from it still refuses,
    /// naming the branch, the pin, the copy and its shape.
    #[test]
    fn pin_name_refuses_a_copy_ahead_of_or_diverged_from_the_pin() {
        let dir = fixture();
        let root = dir.path();
        let s = shapes(root);
        for (other, shape) in [(&s.after, "is ahead of"), (&s.sibling, "has diverged from")] {
            copies(root, &s.pin, other);
            let error = pin_name_of(root, &s.pin).unwrap_err();
            for part in [
                "TSK-001 predecessor branch task/TSK-001-work",
                other.as_str(),
                shape,
                s.pin.as_str(),
                "rebase on its new reviewed head",
                "delete or reset the stray copy",
            ] {
                assert!(error.contains(part), "{part}: {error}");
            }
        }
    }

    /// AC-14: a copy whose ancestry cannot be read refuses with that cause.
    #[test]
    fn pin_name_refuses_a_copy_whose_ancestry_cannot_be_read() {
        let dir = fixture();
        let root = dir.path();
        let s = shapes(root);
        let broken = unreadable_commit(root);
        copies(root, &s.pin, &broken);
        let error = pin_name_of(root, &s.pin).unwrap_err();
        assert!(error.contains("cannot read the ancestry"), "{error}");
        assert!(error.contains(&broken), "{error}");
    }

    /// Issue 603: when every copy of the predecessor branch is behind the
    /// pin, the refusal says so and names the push, never "advanced" or a
    /// rebase; a copy beside the pin that is not behind it keeps the old
    /// refusal.
    #[test]
    fn pin_name_says_behind_when_every_copy_is_behind_the_pin() {
        let dir = fixture();
        let root = dir.path();
        let s = shapes(root);
        copies(root, &s.before, &s.before);
        let error = pin_name_of(root, &s.pin).unwrap_err();
        for part in [
            "TSK-001 predecessor branch is behind the pin",
            s.pin.as_str(),
            s.before.as_str(),
            "push the reviewed head",
        ] {
            assert!(error.contains(part), "{part}: {error}");
        }
        assert!(!error.contains("rebase"), "{error}");
        copies(root, &s.before, &s.sibling);
        let error = pin_name_of(root, &s.pin).unwrap_err();
        assert!(
            error.contains("not the unique predecessor branch tip"),
            "{error}"
        );
    }

    /// The task branch of TSK-002 with one commit on the pin, and its head.
    fn stacked_head(root: &Path, pin: &str) -> String {
        git(root, &["switch", "-q", "-c", "task/TSK-002-work", pin]);
        let head = commit(root, "successor");
        git(root, &["switch", "-q", "main"]);
        head
    }

    fn stacked(root: &Path, head: &str) -> Result<Vec<ReviewedPin>, String> {
        stacked_pins(root, "TSK-002", "main", head, &|_, _, _| Ok(true))
    }

    /// AC-15: the push check honours the pin when a copy is behind it, and
    /// still refuses a copy ahead of it or one whose ancestry cannot be read.
    #[test]
    fn stacked_pins_honour_a_copy_behind_the_pin_and_refuse_the_rest() {
        let dir = fixture();
        let root = dir.path();
        let s = shapes(root);
        let head = stacked_head(root, &s.pin);
        for (local, remote) in [(&s.before, &s.pin), (&s.pin, &s.before)] {
            copies(root, local, remote);
            let pins = stacked(root, &head).unwrap();
            assert_eq!(pins.len(), 1);
            assert_eq!(pins[0].revision.to_string(), s.pin);
        }
        for (other, cause) in [(&s.after, "is ahead of"), (&s.sibling, "has diverged from")] {
            copies(root, &s.pin, other);
            let error = stacked(root, &head).unwrap_err();
            assert!(error.contains(cause), "{error}");
        }
        let broken = unreadable_commit(root);
        copies(root, &s.pin, &broken);
        let error = stacked(root, &head).unwrap_err();
        assert!(error.contains("cannot read the ancestry"), "{error}");
    }
}
