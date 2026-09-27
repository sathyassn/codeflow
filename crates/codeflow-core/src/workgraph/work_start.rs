//! Read-only preflight for starting durable task implementation.
//!
//! Git history is the planning seal. The preflight inspects the merge-base
//! between the current task branch and its intended integration target and
//! requires the task, parent epic, applicable specs, and completed
//! dependencies to exist in that anchored snapshot. It never creates or
//! updates files, refs, branches, worktrees, or record status.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use git2::{Repository, TreeWalkMode, TreeWalkResult};
use thiserror::Error;

use crate::workgraph::{
    is_canonical_task_format_id, is_valid_epic_format_id, is_valid_spec_format_id,
    is_valid_task_format_id,
};

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
    #[error("git repository unavailable: {0}")]
    Repository(String),
    #[error("target ref '{0}' does not resolve to a commit")]
    Target(String),
    #[error("current branch '{branch}' does not carry {task_id} on a work prefix (<prefix>/{task_id}-<slug>)")]
    Branch { branch: String, task_id: String },
    #[error("no merge-base exists between HEAD and '{0}'")]
    MergeBase(String),
    #[error("anchored workgraph is invalid: {0}")]
    InvalidGraph(String),
    #[error("task {0} is not present at the merge-base; merge its planning record before implementation")]
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
        "local branch '{local}' and '{remote}' have diverged ({ahead} commit(s) only on \
         '{local}', {behind} only on '{remote}'), so the work target is ambiguous; \
         reconcile them (fetch, then rebase '{local}' onto '{remote}' or reset it), or \
         name one with `--into`"
    )]
    DivergedTarget {
        local: String,
        remote: String,
        ahead: usize,
        behind: usize,
    },
}

/// A work target resolved to a ref, with a note when a stale local branch was
/// passed over for its remote-tracking ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkTarget {
    /// The target to anchor on (`main`, `origin/main` or a full ref).
    pub target: String,
    /// Why the remote-tracking ref was chosen, for the caller to print.
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkStartReport {
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
struct Record {
    id: String,
    kind: RecordKind,
    status: String,
    epic_id: Option<String>,
    standalone_reason: Option<String>,
    integration_target: Option<String>,
    specs: Vec<String>,
    depends_on: Vec<String>,
    work_type: Option<String>,
}

/// The three durable record kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Epic,
    Spec,
    Task,
}

struct AnchoredTask {
    epic_id: Option<String>,
    specs: Vec<String>,
    dependencies: Vec<String>,
    work_type: Option<String>,
}

/// Pick the configured target convention without mutating repository state.
///
/// `origin/main` wins when present, then `origin/master`, `main`, and
/// `master`.
#[must_use]
pub fn default_work_target(repo_root: &Path) -> Option<String> {
    let repo = Repository::discover(repo_root).ok()?;
    for target in ["origin/main", "origin/master", "main", "master"] {
        if target_reference(&repo, target).is_some() {
            return Some(target.to_string());
        }
    }
    None
}

/// Resolve a declared target to an available local or remote-tracking ref.
///
/// The declared logical name remains portable (`main`, for example) while CI
/// checkouts that expose only `origin/main` can verify the same task. A
/// diverged local branch resolves to the local branch here; callers that
/// anchor work use [`resolve_work_target_checked`], which refuses it.
#[must_use]
pub fn resolve_work_target(repo_root: &Path, declared: Option<&str>) -> Option<String> {
    match resolve_work_target_checked(repo_root, declared) {
        Ok(resolved) => resolved.map(|resolved| resolved.target),
        Err(_) => declared.map(str::to_owned),
    }
}

/// Resolve a declared target as [`resolve_work_target`] does, but never
/// anchor on a stale local branch.
///
/// When a bare name (`main`) has a local branch, compared with the upstream
/// Git has configured for it (`main@{upstream}`):
/// - no configured upstream, equal, or the local branch ahead (unpushed
///   commits): the local branch;
/// - the local branch strictly behind: the upstream, with a note saying so,
///   since a stale local branch anchors on an old snapshot;
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
    let Some(target) = declared.filter(|value| !value.trim().is_empty()) else {
        return Ok(
            default_work_target(repo_root).map(|target| ResolvedWorkTarget { target, note: None })
        );
    };
    let plain = |target: String| Ok(Some(ResolvedWorkTarget { target, note: None }));
    let Ok(repo) = Repository::discover(repo_root) else {
        return plain(target.to_string());
    };
    let Some(candidates) = target_reference_names(target) else {
        return plain(target.to_string());
    };
    let resolves = |name: &str| {
        repo.find_reference(name)
            .and_then(|reference| reference.peel_to_commit())
            .ok()
            .map(|commit| commit.id())
    };
    // A bare name with a local branch: that branch, unless it is strictly
    // behind the upstream Git has configured for it.
    let local_ref = format!("refs/heads/{target}");
    if candidates.first() == Some(&local_ref) {
        if let Some(local_id) = resolves(&local_ref) {
            return local_or_upstream(&repo, target, &local_ref, local_id);
        }
    }
    // Otherwise the first candidate that resolves: an exact full ref, or
    // `origin/<name>` in a checkout with no local branch (a CI clone).
    match candidates
        .iter()
        .find(|candidate| resolves(candidate).is_some())
    {
        Some(found) if target.starts_with("refs/") => plain(found.clone()),
        Some(found) => plain(reference_display_name(found)),
        None => plain(target.to_string()),
    }
}

/// The local branch `local`, or its configured upstream when the local
/// branch is strictly behind it. Only the upstream Git has configured
/// (`branch.<name>.remote` and `.merge`) may replace it: another remote's
/// same-named branch, such as a fork's `origin/main` beside an
/// `upstream/main` the branch tracks, is not the integration target. With no
/// configured upstream the local branch is kept.
fn local_or_upstream(
    repo: &Repository,
    local: &str,
    local_ref: &str,
    local_id: git2::Oid,
) -> Result<Option<ResolvedWorkTarget>, WorkStartError> {
    let keep = || {
        Ok(Some(ResolvedWorkTarget {
            target: local.to_string(),
            note: None,
        }))
    };
    let Some(upstream_ref) = repo
        .branch_upstream_name(local_ref)
        .ok()
        .and_then(|name| name.as_str().ok().map(str::to_owned))
    else {
        return keep();
    };
    let Some(upstream_id) = repo
        .find_reference(&upstream_ref)
        .and_then(|reference| reference.peel_to_commit())
        .ok()
        .map(|commit| commit.id())
    else {
        return keep();
    };
    if upstream_id == local_id {
        return keep();
    }
    let upstream = upstream_target(&upstream_ref);
    let (ahead, behind) = repo
        .graph_ahead_behind(local_id, upstream_id)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    match (ahead, behind) {
        (_, 0) => keep(),
        (0, behind) => Ok(Some(ResolvedWorkTarget {
            note: Some(format!(
                "local branch '{local}' is {behind} commit(s) behind its upstream \
                 '{upstream}'; anchoring on '{upstream}' (fast-forward '{local}' to \
                 silence this)"
            )),
            target: upstream,
        })),
        (ahead, behind) => Err(WorkStartError::DivergedTarget {
            local: local.to_string(),
            remote: upstream,
            ahead,
            behind,
        }),
    }
}

/// A configured upstream ref as a work target that resolves back to it:
/// `main` for a local branch, `origin/main` for origin, else the full ref.
fn upstream_target(reference: &str) -> String {
    if let Some(branch) = reference.strip_prefix("refs/heads/") {
        return branch.to_string();
    }
    if let Some(branch) = reference.strip_prefix("refs/remotes/origin/") {
        return format!("origin/{branch}");
    }
    reference.to_string()
}

/// Whether a ref may serve as a stable planning authority.
///
/// Task branches are implementation surfaces and cannot authorize their own
/// durable records.
#[must_use]
pub fn is_stable_work_target(target: &str) -> bool {
    let target = target.trim();
    target_reference_names(target).is_some()
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
    if let Ok(entry) = tree.get_path(Path::new(crate::scaffold::state::PROJECT_TOML)) {
        let blob = repo
            .find_blob(entry.id())
            .map_err(|_| format!("{revision}: .codeflow/project.toml is not a file"))?;
        let text = std::str::from_utf8(blob.content())
            .map_err(|_| format!("{revision}: .codeflow/project.toml is not UTF-8"))?;
        let state: crate::scaffold::state::ProjectState = toml::from_str(text)
            .map_err(|error| format!("{revision}: .codeflow/project.toml: {error}"))?;
        if state.tier == crate::scaffold::manifest::Tier::Full {
            return Ok(true);
        }
    }
    let Ok(entry) = tree.get_path(Path::new("project-management")) else {
        return Ok(false);
    };
    let home = repo
        .find_tree(entry.id())
        .map_err(|_| format!("{revision}: project-management is not a directory"))?;
    let mut found = false;
    home.walk(TreeWalkMode::PreOrder, |root, entry| {
        let path = format!("project-management/{root}{}", entry.name().unwrap_or(""));
        if record_kind_for_tree_path(&path) == Some(RecordKind::Task) {
            found = true;
            return TreeWalkResult::Abort;
        }
        TreeWalkResult::Ok
    })
    .or_else(|error| if found { Ok(()) } else { Err(error) })
    .map_err(|error| error.to_string())?;
    Ok(found)
}

/// Check that `branch` is an epic's integration line landing on its
/// destination, so the whole line may land as one pull request whose tasks
/// were each classified when they landed on it. A prefix alone grants
/// nothing: the name must carry an epic that exists and is not cancelled, a
/// task of that epic must target this exact line, the pull request must go
/// to the project's default target, and the line's first-parent history
/// from the merge-base must hold only merges (no work committed directly on
/// the line). Returns the epic id.
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
    let suffix = branch
        .strip_prefix("integration/")
        .ok_or_else(|| format!("'{branch}' is not an integration line"))?;
    let digits = suffix.strip_prefix("EPC-").map_or(0, |rest| {
        rest.chars().take_while(char::is_ascii_digit).count()
    });
    let epic_id = suffix.get(..4 + digits).unwrap_or_default();
    if digits == 0 || !is_valid_epic_format_id(epic_id) || !suffix[epic_id.len()..].starts_with('-')
    {
        return Err(format!(
            "'{branch}' names no epic; an epic line is integration/EPC-NNN-<slug>"
        ));
    }
    let repo = Repository::discover(repo_root).map_err(|error| error.to_string())?;
    let commit = |revision: &str| {
        repo.revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| format!("{revision}: {error}"))
    };
    let head_commit = commit(head)?;
    let target_tip = commit(base)?.id();
    let merge_base = repo
        .merge_base(target_tip, head_commit.id())
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
    let epic = at_base
        .get(epic_id)
        .or_else(|| at_head.get(epic_id))
        .filter(|record| record.kind == RecordKind::Epic)
        .ok_or_else(|| format!("'{branch}' names {epic_id}, which has no epic record"))?;
    if matches!(epic.status.as_str(), "cancelled" | "archived") {
        return Err(format!("{epic_id} is {}", epic.status));
    }
    let bound = at_head.values().chain(at_base.values()).any(|record| {
        record.kind == RecordKind::Task
            && record.epic_id.as_deref() == Some(epic_id)
            && record
                .integration_target
                .as_deref()
                .is_some_and(|target| logical_target(target) == branch)
    });
    if !bound {
        return Err(format!("no task of {epic_id} targets '{branch}'"));
    }
    let destination =
        default_work_target(repo_root).ok_or("no main or master branch to land the line on")?;
    if logical_target(base_ref) != logical_target(&destination) {
        return Err(format!(
            "'{branch}' lands on '{}', not '{base_ref}'",
            logical_target(&destination)
        ));
    }
    let mut walk = repo.revwalk().map_err(|error| error.to_string())?;
    walk.push(head_commit.id())
        .and_then(|()| walk.hide(merge_base))
        .and_then(|()| walk.simplify_first_parent())
        .map_err(|error| error.to_string())?;
    for oid in walk {
        let oid = oid.map_err(|error| error.to_string())?;
        let parents = repo
            .find_commit(oid)
            .map_err(|error| error.to_string())?
            .parent_count();
        if parents < 2 {
            return Err(format!(
                "'{branch}' has a commit made directly on the line ({}); land work on the line by classified pull requests",
                &oid.to_string()[..9]
            ));
        }
    }
    Ok(epic_id.to_string())
}

/// Whether a stable target resolves to a real local or remote-tracking branch.
#[must_use]
pub fn work_target_resolves(repo_root: &Path, target: &str) -> bool {
    if !is_stable_work_target(target) {
        return false;
    }
    Repository::discover(repo_root)
        .ok()
        .is_some_and(|repo| target_reference(&repo, target).is_some())
}

/// Read a task's declared integration target from the visible checkout.
///
/// This is intentionally read-only. It accepts canonical flat records and the
/// historical nested task layout so hooks and CI can share the same target
/// selection without owning harness- or branch-management state.
#[must_use]
pub fn declared_work_target(repo_root: &Path, task_id: &str) -> Option<String> {
    let pm_root = repo_root.join("project-management");
    crate::workgraph::layout::task_record_files(&pm_root)
        .into_iter()
        .find(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem == task_id)
        })
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|content| parse_record(&content, RecordKind::Task).ok())
        .and_then(|record| record.integration_target)
        .filter(|target| !target.trim().is_empty())
}

/// Branch prefixes that never carry a task id: planning branches create
/// records and integration branches carry many tasks.
const NON_WORK_PREFIXES: [&str; 2] = ["plan/", "integration/"];

/// The part of `branch` after a sanctioned work prefix: any policy branch
/// prefix except `plan/` and `integration/` (SPC-013 R-110).
fn work_branch_suffix<'b>(repo_root: &Path, branch: &'b str) -> Option<&'b str> {
    let (policy, _) = crate::hooks::policy::Policy::load_effective(repo_root);
    policy
        .git
        .branch_prefixes
        .iter()
        .filter(|prefix| !NON_WORK_PREFIXES.contains(&prefix.as_str()))
        .find_map(|prefix| branch.strip_prefix(prefix.as_str()))
}

/// Resolve the durable task id a work branch carries,
/// `<prefix>/TSK-NNN-<slug>` on any sanctioned work prefix (`task/`, `fix/`,
/// `feat/`, `spike/` and the rest of `git.branch_prefixes`).
///
/// The visible workgraph is the source of truth, which avoids baking legacy
/// or future id shapes into branch parsing.
#[must_use]
pub fn task_id_from_branch(repo_root: &Path, branch: &str) -> Option<String> {
    let suffix = work_branch_suffix(repo_root, branch)?;
    let pm_root = repo_root.join("project-management");
    crate::workgraph::layout::task_record_files(&pm_root)
        .into_iter()
        .filter_map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .filter(|id| is_valid_task_format_id(id))
        .filter(|id| suffix.starts_with(&format!("{id}-")))
        .max_by_key(String::len)
}

/// Whether a work branch names a task id by shape (`<prefix>/TSK-<digits>-`),
/// whether or not a record for it is visible. A branch that claims an id the
/// workgraph does not hold is refused rather than treated as untracked.
#[must_use]
pub fn branch_claims_task_id(repo_root: &Path, branch: &str) -> bool {
    work_branch_suffix(repo_root, branch).is_some_and(|suffix| {
        suffix
            .strip_prefix("TSK-")
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
    })
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
    let branch = head
        .shorthand()
        .map(str::to_owned)
        .map_err(|_| WorkStartError::Repository("HEAD is detached".to_string()))?;
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
    if !work_branch_suffix(repo_root, branch).is_some_and(|suffix| suffix.starts_with(&expected)) {
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
    if !is_valid_task_format_id(task_id) {
        return Err(WorkStartError::InvalidGraph(format!(
            "task id '{task_id}' is malformed"
        )));
    }
    if !is_stable_work_target(target) {
        return Err(WorkStartError::UnstableTarget(target.to_string()));
    }
    let (merge_base, records) = anchored_records(repo_root, target)?;
    let anchored = validate_anchored_task(&records, task_id, target)?;

    Ok(WorkStartReport {
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

fn anchored_records(
    repo_root: &Path,
    target: &str,
) -> Result<(String, BTreeMap<String, Record>), WorkStartError> {
    let repo = Repository::discover(repo_root)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let head_id = repo
        .head()
        .map_err(|error| WorkStartError::Repository(error.to_string()))?
        .target()
        .ok_or_else(|| WorkStartError::Repository("HEAD has no commit".to_string()))?;
    let target_commit = target_reference(&repo, target)
        .ok_or_else(|| WorkStartError::Target(target.to_string()))?;
    let merge_base = repo
        .merge_base(head_id, target_commit.id())
        .map_err(|_| WorkStartError::MergeBase(target.to_string()))?;
    let commit = repo
        .find_commit(merge_base)
        .map_err(|error| WorkStartError::Repository(error.to_string()))?;
    let records = records_from_tree(
        &repo,
        &commit
            .tree()
            .map_err(|error| WorkStartError::Repository(error.to_string()))?,
    )?;
    Ok((merge_base.to_string(), records))
}

fn validate_anchored_task(
    records: &BTreeMap<String, Record>,
    task_id: &str,
    target: &str,
) -> Result<AnchoredTask, WorkStartError> {
    let task = records
        .get(task_id)
        .filter(|record| record.kind == RecordKind::Task)
        .ok_or_else(|| WorkStartError::TaskNotAnchored(task_id.to_string()))?;
    match task
        .integration_target
        .as_deref()
        .filter(|value| !value.trim().is_empty())
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
    if !matches!(task.status.as_str(), "todo" | "in_progress") {
        return Err(WorkStartError::TaskNotStartable {
            task_id: task_id.to_string(),
            status: task.status.clone(),
        });
    }

    let epic = if let Some(epic_id) = task.epic_id.as_deref() {
        Some(
            records
                .get(epic_id)
                .filter(|record| record.kind == RecordKind::Epic)
                .ok_or_else(|| WorkStartError::MissingEpic {
                    task_id: task_id.to_string(),
                    epic_id: epic_id.to_string(),
                })?,
        )
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
    validate_dependencies(records, task_id, &task.depends_on)?;
    Ok(AnchoredTask {
        epic_id: task.epic_id.clone(),
        specs,
        dependencies: task.depends_on.clone(),
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
    records: &BTreeMap<String, Record>,
    task_id: &str,
    dependencies: &[String],
) -> Result<(), WorkStartError> {
    for dependency in dependencies {
        let predecessor = records
            .get(dependency)
            .filter(|record| record.kind == RecordKind::Task)
            .ok_or_else(|| WorkStartError::MissingDependency {
                task_id: task_id.to_string(),
                dependency: dependency.clone(),
            })?;
        if predecessor.status != "complete" {
            return Err(WorkStartError::DependencyIncomplete {
                task_id: task_id.to_string(),
                dependency: dependency.clone(),
                status: predecessor.status.clone(),
            });
        }
    }
    Ok(())
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

fn records_from_tree(
    repo: &Repository,
    tree: &git2::Tree<'_>,
) -> Result<BTreeMap<String, Record>, WorkStartError> {
    let mut records = BTreeMap::new();
    let mut failure = None;
    let walk_result = tree.walk(TreeWalkMode::PreOrder, |root, entry| {
        if failure.is_some() {
            return TreeWalkResult::Abort;
        }
        let Ok(name) = entry.name() else {
            return TreeWalkResult::Ok;
        };
        let path = format!("{root}{name}");
        let Some(kind) = record_kind_for_tree_path(&path) else {
            return TreeWalkResult::Ok;
        };
        let Ok(blob) = repo.find_blob(entry.id()) else {
            failure = Some(format!("{path}: cannot read blob"));
            return TreeWalkResult::Abort;
        };
        let Ok(content) = std::str::from_utf8(blob.content()) else {
            failure = Some(format!("{path}: record is not UTF-8"));
            return TreeWalkResult::Abort;
        };
        match parse_record(content, kind) {
            Ok(record) => {
                if records.insert(record.id.clone(), record).is_some() {
                    failure = Some(format!("{path}: duplicate work id"));
                    TreeWalkResult::Abort
                } else {
                    TreeWalkResult::Ok
                }
            }
            Err(error) => {
                failure = Some(format!("{path}: {error}"));
                TreeWalkResult::Abort
            }
        }
    });
    if let Some(error) = failure {
        return Err(WorkStartError::InvalidGraph(error));
    }
    walk_result.map_err(|error| WorkStartError::Repository(error.to_string()))?;
    Ok(records)
}

fn parse_record(content: &str, kind: RecordKind) -> Result<Record, String> {
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
    Ok(Record {
        id,
        kind,
        status: string(&data, "status").unwrap_or_default(),
        epic_id: string(&data, "epic_id"),
        standalone_reason: string(&data, "standalone_reason"),
        integration_target: string(&data, "integration_target"),
        specs: strings(&data, "specs")?,
        depends_on: strings_alias(&data, "depends_on", "dependencies")?,
        work_type: string(&data, "work_type"),
    })
}

fn target_reference<'repo>(repo: &'repo Repository, target: &str) -> Option<git2::Commit<'repo>> {
    target_reference_names(target)?
        .into_iter()
        .find_map(|name| repo.find_reference(&name).ok()?.peel_to_commit().ok())
}

fn target_reference_names(target: &str) -> Option<Vec<String>> {
    if target.is_empty() || target.trim() != target || logical_target(target) == "HEAD" {
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
            format!("refs/heads/{target}"),
            format!("refs/remotes/origin/{target}"),
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

fn logical_target(target: &str) -> &str {
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
        if line.trim_end_matches(['\r', '\n']) == "---" {
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

fn strings_alias(
    data: &serde_yaml::Mapping,
    canonical: &str,
    legacy: &str,
) -> Result<Vec<String>, String> {
    if data.contains_key(key(canonical)) && data.contains_key(key(legacy)) {
        return Err(format!("defines both {canonical} and {legacy}"));
    }
    if data.contains_key(key(canonical)) {
        strings(data, canonical)
    } else {
        strings(data, legacy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
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
        let before_status = Command::new("git")
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
        let after_status = Command::new("git")
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
                task_id_from_branch(dir.path(), branch).as_deref(),
                Some("TSK-002"),
                "{branch}"
            );
            let report = check_work_start_for_branch(dir.path(), "TSK-002", "main", branch)
                .unwrap_or_else(|error| panic!("{branch}: {error}"));
            assert_eq!(report.branch, branch);
            assert_eq!(report.work_type.as_deref(), Some("feat"));
        }
        for branch in ["plan/TSK-002-graph", "integration/TSK-002-line", "fix/typo"] {
            assert_eq!(task_id_from_branch(dir.path(), branch), None, "{branch}");
        }
        assert!(branch_claims_task_id(dir.path(), "fix/TSK-404-ghost"));
        assert!(!branch_claims_task_id(dir.path(), "fix/typo"));
        assert!(!branch_claims_task_id(dir.path(), "plan/TSK-404-new"));
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
            declared_work_target(dir.path(), "TSK-002").as_deref(),
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

    #[test]
    fn a_target_without_a_configured_upstream_stays_local() {
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
        assert_eq!(resolved.target, "main");
        assert_eq!(resolved.note, None);
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
        assert_eq!(resolved.target, "main");
        assert_eq!(resolved.note, None);
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
        assert!(resolved
            .note
            .unwrap()
            .contains("behind its upstream 'refs/remotes/upstream/main'"));
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
        assert_eq!(resolved.note, None);
    }

    #[test]
    fn a_local_target_strictly_behind_resolves_to_the_tracking_ref_and_says_so() {
        let dir = tracking_fixture(false, true);
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "origin/main");
        let note = resolved.note.unwrap();
        assert!(
            note.contains("'main' is 1 commit(s) behind its upstream 'origin/main'"),
            "{note}"
        );
        assert_eq!(
            resolve_work_target(dir.path(), Some("main")).as_deref(),
            Some("origin/main")
        );
    }

    #[test]
    fn a_local_target_only_ahead_resolves_to_the_local_branch() {
        let dir = tracking_fixture(true, false);
        let resolved = resolve_work_target_checked(dir.path(), Some("main"))
            .unwrap()
            .unwrap();
        assert_eq!(resolved.target, "main");
        assert_eq!(resolved.note, None);
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
            message.contains("'main' and 'origin/main' have diverged"),
            "{message}"
        );
        // The unchecked resolver keeps its old answer for non-anchoring callers.
        assert_eq!(
            resolve_work_target(dir.path(), Some("main")).as_deref(),
            Some("main")
        );
    }

    #[test]
    fn declared_target_can_resolve_through_remote_tracking_ref() {
        let dir = fixture();
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/release", "HEAD"],
        );
        assert_eq!(
            resolve_work_target(dir.path(), Some("release")).as_deref(),
            Some("origin/release")
        );
        git(
            dir.path(),
            &["update-ref", "refs/remotes/upstream/release", "HEAD"],
        );
        assert_eq!(
            resolve_work_target(dir.path(), Some("refs/remotes/upstream/release")).as_deref(),
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
            Err(WorkStartError::TaskNotStartable { .. })
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
}
