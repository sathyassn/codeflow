//! Generated status views — never stored (charter §3.1, principle 3).
//!
//! `codeflow status` computes everything live: current branch + worktrees
//! from git, in-flight work from the workgraph `MarkdownStore`, and the
//! capability table from `docs/capabilities.md`. Every tier degrades
//! gracefully: absent layers become notes, never errors.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::capability::{parse_capabilities, CapabilityEntry};
use crate::models::{Epic, EpicFilter, Task, TaskFilter, TaskStatus};
use crate::workgraph::readiness::{self, Backlog, State};
use crate::workgraph::{durable_work_tracking_enabled, MarkdownStore, RecordStore};

/// A git worktree attached to the repository.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
}

/// Read-only closeout advice for a linked worktree or an unattached local
/// branch. `CodeFlow` reports evidence; it never removes either resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupInfo {
    pub kind: String,
    pub name: String,
    pub path: Option<String>,
    pub disposition: CleanupDisposition,
    pub proof: String,
}

/// The three outcomes used by the closeout inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupDisposition {
    /// Clean and proven landed by ancestry or patch equivalence.
    Removable,
    /// Contains local changes and must be preserved.
    PreserveDirty,
    /// Clean, but landing could not be proven from local Git evidence.
    RetainUnproven,
    /// An integration line an open task still targets (SPC-013 R-44), or a
    /// task branch holding an open claim (R-27).
    RetainLive,
}

impl CleanupDisposition {
    fn label(self) -> &'static str {
        match self {
            Self::Removable => "removable",
            Self::PreserveDirty => "preserve-dirty",
            Self::RetainUnproven => "retain-unproven",
            Self::RetainLive => "retain-live",
        }
    }
}

/// In-flight work, summarized from the workgraph store.
#[derive(Debug, Clone, Default)]
pub struct WorkSummary {
    /// Epic counts keyed by status string.
    pub epics_by_status: BTreeMap<String, usize>,
    /// Task counts keyed by status string.
    pub tasks_by_status: BTreeMap<String, usize>,
    /// `"<format_id> <title>"` for epics currently in progress.
    pub active_epics: Vec<String>,
    /// `"<format_id> <title>"` for tasks currently in progress.
    pub active_tasks: Vec<String>,
}

/// One epic under a capability, with its open/total task counts rolled up
/// from the epic ↔ task link.
#[derive(Debug, Clone)]
pub struct EpicDelivery {
    /// Epic format id (`EPC-###`) as referenced by the capability.
    pub format_id: String,
    pub title: String,
    /// Epic status string as written.
    pub status: String,
    /// Tasks not yet `complete` or `cancelled`.
    pub open_tasks: usize,
    /// All tasks linked to this epic.
    pub total_tasks: usize,
    /// Up to a few next actionable tasks (`todo`/`in_progress`), formatted
    /// `"<format_id> <title>"`.
    pub next_tasks: Vec<String>,
}

/// A capability's delivery rollup: the epics that build it and, transitively,
/// their open tasks. Generated purely from `capability.epics[]` → epic →
/// `task.epic_id`; never stored.
#[derive(Debug, Clone)]
pub struct CapabilityDelivery {
    /// Capability id (`CAP-###`).
    pub id: String,
    pub name: String,
    /// Capability status string as written.
    pub status: String,
    /// Epics referenced by the capability that resolve to a record.
    pub epics: Vec<EpicDelivery>,
    /// Epic ids the capability names that have no matching epic record — a
    /// dangling link surfaced rather than silently dropped.
    pub missing_epics: Vec<String>,
}

/// The complete generated view.
#[derive(Debug)]
pub struct StatusView {
    /// Current branch; `None` outside a git repo or on unborn HEAD.
    pub branch: Option<String>,
    pub worktrees: Vec<WorktreeInfo>,
    /// Local target ref used for cleanup proof, when available.
    pub cleanup_target: Option<String>,
    /// Linked worktrees and unattached local branches that need closeout.
    pub cleanup: Vec<CleanupInfo>,
    /// `None` when the project-management tier is absent.
    pub work: Option<WorkSummary>,
    /// Derived task states from the readiness core (SPC-013 R-27); `None`
    /// when durable work tracking is off or the refs cannot be read.
    pub derived: Option<Backlog>,
    /// `None` when `docs/capabilities.md` is absent.
    pub capabilities: Option<Vec<CapabilityEntry>>,
    /// Per-capability delivery rollup. `None` when either the registry or the
    /// project-management tier is absent (the view needs both links).
    pub delivery: Option<Vec<CapabilityDelivery>>,
    /// Tier/degradation notes (absent layers, parse problems).
    pub notes: Vec<String>,
}

/// Collect the status view for a repository root. Read-only; works at any
/// tier — absent layers are reported in `notes`.
#[must_use]
pub fn collect_status(repo_root: &Path) -> StatusView {
    let mut notes = Vec::new();

    let (branch, worktrees, cleanup_target, cleanup) =
        if let Ok(repo) = git2::Repository::open(repo_root) {
            let branch = repo
                .head()
                .ok()
                .map(|h| crate::git::reference_shorthand(&h));
            let worktrees = list_worktrees(&repo);
            let target = cleanup_target(&repo);
            let cleanup = collect_cleanup(repo_root, &repo, target.as_ref());
            (
                branch,
                worktrees,
                target.as_ref().map(|item| item.name.clone()),
                cleanup,
            )
        } else {
            notes.push("not a git repository — branch/worktree state unavailable".to_string());
            (None, Vec::new(), None, Vec::new())
        };

    let work = collect_work(repo_root, &mut notes);
    let derived = match durable_work_tracking_enabled(repo_root) {
        Ok(true) => match readiness::backlog(repo_root) {
            Ok(backlog) => Some(backlog),
            Err(error) => {
                notes.push(format!("derived task states unavailable: {error}"));
                None
            }
        },
        _ => None,
    };
    let capabilities = collect_capabilities(repo_root, &mut notes);
    let delivery = collect_delivery(repo_root, capabilities.as_deref());

    StatusView {
        branch,
        worktrees,
        cleanup_target,
        cleanup,
        work,
        derived,
        capabilities,
        delivery,
        notes,
    }
}

#[derive(Debug)]
struct CleanupTarget {
    name: String,
    oid: git2::Oid,
}

fn cleanup_target(repo: &git2::Repository) -> Option<CleanupTarget> {
    let mut names = Vec::new();
    if let Ok(head) = repo.find_reference("refs/remotes/origin/HEAD") {
        if let Ok(Some(symbolic)) = head.symbolic_target() {
            names.push(symbolic.to_string());
        }
    }
    names.extend(
        [
            "refs/remotes/origin/main",
            "refs/remotes/origin/master",
            "refs/heads/main",
            "refs/heads/master",
        ]
        .into_iter()
        .map(ToString::to_string),
    );

    names.into_iter().find_map(|name| {
        let reference = repo.find_reference(&name).ok()?;
        let oid = reference.peel_to_commit().ok()?.id();
        Some(CleanupTarget {
            name: display_ref(&name),
            oid,
        })
    })
}

fn display_ref(name: &str) -> String {
    name.strip_prefix("refs/remotes/")
        .or_else(|| name.strip_prefix("refs/heads/"))
        .unwrap_or(name)
        .to_string()
}

fn collect_cleanup(
    repo_root: &Path,
    repo: &git2::Repository,
    target: Option<&CleanupTarget>,
) -> Vec<CleanupInfo> {
    let mut out = Vec::new();
    let mut checked_out = std::collections::BTreeSet::new();
    if let Ok(head) = repo.head() {
        checked_out.insert(crate::git::reference_shorthand(&head));
    }

    {
        for linked in crate::git::linked_worktrees(repo) {
            let name = linked.name.as_str();
            let path = linked.path.as_path();
            let Ok(worktree_repo) = git2::Repository::open(path) else {
                out.push(CleanupInfo {
                    kind: "worktree".to_string(),
                    name: name.to_string(),
                    path: Some(path.display().to_string()),
                    disposition: CleanupDisposition::RetainUnproven,
                    proof: "worktree state unavailable".to_string(),
                });
                continue;
            };
            let head = worktree_repo.head().ok();
            let branch = head.as_ref().map(crate::git::reference_shorthand);
            if let Some(branch) = &branch {
                checked_out.insert(branch.clone());
            }
            let oid = head.as_ref().and_then(git2::Reference::target);
            let (disposition, proof) = classify_cleanup(
                repo_root,
                repo,
                oid,
                branch.as_deref(),
                target,
                repository_dirty(path),
            );
            out.push(CleanupInfo {
                kind: "worktree".to_string(),
                name: branch.unwrap_or_else(|| format!("{name} (detached)")),
                path: Some(path.display().to_string()),
                disposition,
                proof,
            });
        }
    }

    if let Ok(branches) = repo.branches(Some(git2::BranchType::Local)) {
        for item in branches.flatten() {
            let (branch, _) = item;
            let Some(name) = branch.name().ok().flatten() else {
                continue;
            };
            if checked_out.contains(name) || target_matches_branch(target, name) {
                continue;
            }
            let oid = branch.get().peel_to_commit().ok().map(|commit| commit.id());
            let (disposition, proof) =
                classify_cleanup(repo_root, repo, oid, Some(name), target, Some(false));
            out.push(CleanupInfo {
                kind: "branch".to_string(),
                name: name.to_string(),
                path: None,
                disposition,
                proof,
            });
        }
    }

    out.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.name.cmp(&right.name))
    });
    out
}

fn target_matches_branch(target: Option<&CleanupTarget>, branch: &str) -> bool {
    target.is_some_and(|item| {
        item.name == branch
            || item
                .name
                .strip_prefix("origin/")
                .is_some_and(|name| name == branch)
    })
}

fn classify_cleanup(
    repo_root: &Path,
    repo: &git2::Repository,
    oid: Option<git2::Oid>,
    branch: Option<&str>,
    target: Option<&CleanupTarget>,
    dirty: Option<bool>,
) -> (CleanupDisposition, String) {
    match dirty {
        Some(true) => {
            return (
                CleanupDisposition::PreserveDirty,
                "local changes present".to_string(),
            );
        }
        None => {
            return (
                CleanupDisposition::RetainUnproven,
                "working state unavailable".to_string(),
            );
        }
        Some(false) => {}
    }

    if branch.is_some_and(|name| readiness::is_live_integration_line(repo_root, name)) {
        return (
            CleanupDisposition::RetainLive,
            "live integration line: an open task targets it".to_string(),
        );
    }
    let (Some(oid), Some(target)) = (oid, target) else {
        return (
            CleanupDisposition::RetainUnproven,
            "landing target or revision unavailable".to_string(),
        );
    };
    if branch.is_some_and(|name| readiness::is_open_claim(repo_root, repo, name, oid, target.oid)) {
        return (
            CleanupDisposition::RetainLive,
            format!("open claim: nothing of it has landed in {}", target.name),
        );
    }
    if oid == target.oid || repo.graph_descendant_of(target.oid, oid).unwrap_or(false) {
        return (
            CleanupDisposition::Removable,
            format!("landed by ancestry in {}", target.name),
        );
    }
    if branch.is_some_and(|name| patch_equivalent(repo_root, &target.name, name)) {
        return (
            CleanupDisposition::Removable,
            format!("patch-equivalent in {}", target.name),
        );
    }
    (
        CleanupDisposition::RetainUnproven,
        format!("not proven landed in {}", target.name),
    )
}

fn repository_dirty(path: &Path) -> Option<bool> {
    let repo = git2::Repository::open(path).ok()?;
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);
    repo.statuses(Some(&mut options))
        .ok()
        .map(|statuses| !statuses.is_empty())
}

fn patch_equivalent(repo_root: &Path, target: &str, branch: &str) -> bool {
    readiness::cherry_landed(repo_root, target, branch)
}

fn list_worktrees(repo: &git2::Repository) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    for linked in crate::git::linked_worktrees(repo) {
        out.push(WorktreeInfo {
            name: linked.name,
            path: linked.path.display().to_string(),
        });
    }
    out
}

fn collect_work(repo_root: &Path, notes: &mut Vec<String>) -> Option<WorkSummary> {
    let pm = repo_root.join("project-management");
    if !pm.is_dir() {
        notes.push(
            "project-management/ absent — durable work tracking not enabled at this tier"
                .to_string(),
        );
        return None;
    }
    let store = match MarkdownStore::new(&pm) {
        Ok(s) => s,
        Err(e) => {
            notes.push(format!("project-management/ unreadable: {e}"));
            return None;
        }
    };

    let mut summary = WorkSummary::default();

    match store.list_epics(EpicFilter::default()) {
        Ok(epics) => {
            for epic in epics {
                let status = epic.status.to_string();
                *summary.epics_by_status.entry(status.clone()).or_insert(0) += 1;
                if status == "in_progress" {
                    summary
                        .active_epics
                        .push(format!("{} {}", epic.format_id, epic.title));
                }
            }
        }
        Err(e) => notes.push(format!("epics unreadable: {e}")),
    }

    match store.list_tasks(TaskFilter::default()) {
        Ok(tasks) => {
            for task in tasks {
                let status = task.status.to_string();
                *summary.tasks_by_status.entry(status.clone()).or_insert(0) += 1;
                if status == "in_progress" {
                    summary
                        .active_tasks
                        .push(format!("{} {}", task.format_id, task.title));
                }
            }
        }
        Err(e) => notes.push(format!("tasks unreadable: {e}")),
    }

    Some(summary)
}

fn collect_capabilities(repo_root: &Path, notes: &mut Vec<String>) -> Option<Vec<CapabilityEntry>> {
    let path = repo_root.join("docs/capabilities.md");
    let Ok(content) = std::fs::read_to_string(&path) else {
        notes.push("docs/capabilities.md absent — capability registry not present".to_string());
        return None;
    };
    let (entries, issues) = parse_capabilities(&content);
    for issue in issues {
        notes.push(format!(
            "docs/capabilities.md:{}: {}",
            issue.line, issue.message
        ));
    }
    Some(entries)
}

/// Roll up capability delivery from the existing links: each capability's
/// `epics[]` (by `EPC-###` format id) → epic record → tasks (by
/// `task.epic_id`, the epic's internal id). Read-only and generated; needs
/// both the registry and the project-management tier, so returns `None` when
/// either is absent (their absence is already noted by the other collectors).
fn collect_delivery(
    repo_root: &Path,
    capabilities: Option<&[CapabilityEntry]>,
) -> Option<Vec<CapabilityDelivery>> {
    let capabilities = capabilities?;
    let pm = repo_root.join("project-management");
    if !pm.is_dir() {
        return None;
    }
    let store = MarkdownStore::new(&pm).ok()?;
    let epics = store.list_epics(EpicFilter::default()).ok()?;
    let tasks = store.list_tasks(TaskFilter::default()).ok()?;

    // Capabilities reference epics by format id; tasks reference their epic by
    // its internal id. Index both so the join is a lookup, not a scan.
    let epic_by_format: BTreeMap<&str, &Epic> =
        epics.iter().map(|e| (e.format_id.as_str(), e)).collect();
    let mut tasks_by_epic: BTreeMap<&str, Vec<&Task>> = BTreeMap::new();
    for task in &tasks {
        if let Some(epic_id) = task.epic_id.as_deref() {
            tasks_by_epic.entry(epic_id).or_default().push(task);
        }
    }

    let rollup = capabilities
        .iter()
        .map(|cap| {
            let mut epic_deliveries = Vec::new();
            let mut missing_epics = Vec::new();
            for epic_fid in &cap.epics {
                let Some(epic) = epic_by_format.get(epic_fid.as_str()) else {
                    missing_epics.push(epic_fid.clone());
                    continue;
                };
                let epic_tasks: &[&Task] = tasks_by_epic
                    .get(epic.id.as_str())
                    .map_or(&[], Vec::as_slice);
                let open_tasks = epic_tasks
                    .iter()
                    .filter(|t| !matches!(t.status, TaskStatus::Complete | TaskStatus::Cancelled))
                    .count();
                let next_tasks = epic_tasks
                    .iter()
                    .filter(|t| matches!(t.status, TaskStatus::Todo | TaskStatus::InProgress))
                    .take(3)
                    .map(|t| format!("{} {}", t.format_id, t.title))
                    .collect();
                epic_deliveries.push(EpicDelivery {
                    format_id: epic.format_id.clone(),
                    title: epic.title.clone(),
                    status: epic.status.to_string(),
                    open_tasks,
                    total_tasks: epic_tasks.len(),
                    next_tasks,
                });
            }
            CapabilityDelivery {
                id: cap.id.clone(),
                name: cap.name.clone(),
                status: cap.status.clone(),
                epics: epic_deliveries,
                missing_epics,
            }
        })
        .collect();

    Some(rollup)
}

/// Render the view as the human-readable status report.
///
/// With `capabilities_table`, every capability entry is listed; otherwise
/// only counts by status appear.
#[must_use]
pub fn render_status(view: &StatusView, capabilities_table: bool) -> String {
    let mut out = String::new();

    match &view.branch {
        Some(b) => {
            let _ = writeln!(out, "branch: {b}");
        }
        None => {
            let _ = writeln!(out, "branch: (none)");
        }
    }

    if view.worktrees.is_empty() {
        let _ = writeln!(out, "worktrees: none");
    } else {
        let _ = writeln!(out, "worktrees: {}", view.worktrees.len());
        for wt in &view.worktrees {
            let _ = writeln!(out, "  {} -> {}", wt.name, wt.path);
        }
    }

    render_cleanup(&mut out, view);

    match &view.work {
        Some(work) => {
            let _ = writeln!(
                out,
                "work: epics {} · tasks {}",
                format_counts(&work.epics_by_status),
                format_counts(&work.tasks_by_status)
            );
            for epic in &work.active_epics {
                let _ = writeln!(out, "  in flight (epic): {epic}");
            }
            for task in &work.active_tasks {
                let _ = writeln!(out, "  in flight (task): {task}");
            }
        }
        None => {
            let _ = writeln!(out, "work: (no project-management tier)");
        }
    }
    if let Some(backlog) = &view.derived {
        render_derived(&mut out, backlog);
    }

    match &view.capabilities {
        Some(entries) => {
            let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
            for entry in entries {
                *by_status.entry(entry.status.clone()).or_insert(0) += 1;
            }
            let detail = by_status
                .iter()
                .map(|(status, n)| format!("{status} {n}"))
                .collect::<Vec<_>>()
                .join(", ");
            if detail.is_empty() {
                let _ = writeln!(out, "capabilities: 0");
            } else {
                let _ = writeln!(out, "capabilities: {} ({detail})", entries.len());
            }
            if capabilities_table {
                for entry in entries {
                    let _ = writeln!(
                        out,
                        "  {}  {:<10}  {:<10}  {}  [{}]",
                        entry.id,
                        entry.status,
                        entry.area,
                        entry.name,
                        entry.verified_by.join(", ")
                    );
                }
            }
        }
        None => {
            let _ = writeln!(out, "capabilities: (no registry)");
        }
    }

    for note in &view.notes {
        let _ = writeln!(out, "note: {note}");
    }

    out
}

fn render_derived(out: &mut String, backlog: &Backlog) {
    let _ = writeln!(out, "tasks: {}", backlog.counts_line());
    let _ = writeln!(out, "  {}", backlog.snapshot_line());
    for entry in backlog.in_state(State::Active) {
        let _ = writeln!(
            out,
            "  active: {} {} ({})",
            entry.task_id,
            entry.title,
            entry.branches.join(", ")
        );
    }
    for entry in backlog.in_state(State::Ready) {
        let _ = writeln!(out, "  ready: {} {}", entry.task_id, entry.title);
    }
    // A Blocker or an invalid record outranks a claim; the branch stays shown.
    for state in [State::Blocked, State::Invalid] {
        for entry in backlog
            .in_state(state)
            .filter(|entry| !entry.branches.is_empty())
        {
            let _ = writeln!(
                out,
                "  {}: {} ({}; {})",
                state.as_str(),
                entry.task_id,
                entry.reason,
                entry.branches.join(", ")
            );
        }
    }
    for (task_id, branches) in &backlog.conflicts {
        let _ = writeln!(
            out,
            "  conflict: {task_id} is carried by {} (claims are advisory; settle one owner)",
            branches.join(", ")
        );
    }
    for branch in &backlog.landed {
        let _ = writeln!(out, "  landed: {branch}");
    }
    for (epic, progress) in &backlog.epics {
        let _ = writeln!(
            out,
            "  epic {epic}: {}/{} complete, {} cancelled",
            progress.complete, progress.total, progress.cancelled
        );
    }
}

fn render_cleanup(out: &mut String, view: &StatusView) {
    match &view.cleanup_target {
        Some(target) if view.cleanup.is_empty() => {
            let _ = writeln!(out, "cleanup: none (target {target})");
        }
        Some(target) => {
            let _ = writeln!(
                out,
                "cleanup: {} candidate(s) against {target}",
                view.cleanup.len()
            );
            for item in &view.cleanup {
                let path = item
                    .path
                    .as_deref()
                    .map(|path| format!(" -> {path}"))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "  {} {} {}{} ({})",
                    item.disposition.label(),
                    item.kind,
                    item.name,
                    path,
                    item.proof
                );
            }
        }
        None if !view.cleanup.is_empty() => {
            let _ = writeln!(
                out,
                "cleanup: {} candidate(s); landing target unavailable",
                view.cleanup.len()
            );
            for item in &view.cleanup {
                let _ = writeln!(
                    out,
                    "  {} {} {} ({})",
                    item.disposition.label(),
                    item.kind,
                    item.name,
                    item.proof
                );
            }
        }
        None => {
            let _ = writeln!(out, "cleanup: unavailable");
        }
    }
}

/// Render the capability-delivery rollup: for each capability, its status and
/// the epics that build it, each with open/total task counts and the next
/// actionable tasks. Generated from the existing links, so it degrades the
/// same way the base view does.
#[must_use]
pub fn render_delivery(view: &StatusView) -> String {
    let mut out = String::new();
    match &view.delivery {
        Some(caps) if !caps.is_empty() => {
            let _ = writeln!(out, "capability delivery:");
            for cap in caps {
                let _ = writeln!(out, "  {}  {:<10}  {}", cap.id, cap.status, cap.name);
                if cap.epics.is_empty() && cap.missing_epics.is_empty() {
                    let _ = writeln!(out, "    (no epics linked)");
                }
                for epic in &cap.epics {
                    let _ = writeln!(
                        out,
                        "    {} {}  [{}]  tasks {}/{} open",
                        epic.format_id, epic.title, epic.status, epic.open_tasks, epic.total_tasks
                    );
                    for task in &epic.next_tasks {
                        let _ = writeln!(out, "      next: {task}");
                    }
                }
                for missing in &cap.missing_epics {
                    let _ = writeln!(out, "    {missing} (no epic record)");
                }
            }
        }
        Some(_) => {
            let _ = writeln!(out, "capability delivery: (no capabilities in registry)");
        }
        None => {
            let _ = writeln!(
                out,
                "capability delivery: (unavailable — needs both the registry and project-management tier)"
            );
        }
    }
    out
}

fn format_counts(counts: &BTreeMap<String, usize>) -> String {
    if counts.is_empty() {
        return "0".to_string();
    }
    let total: usize = counts.values().sum();
    let detail = counts
        .iter()
        .map(|(status, n)| format!("{status} {n}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{total} ({detail})")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Epic, EpicStatus, Task, TaskStatus};

    fn git(dir: &Path, args: &[&str]) {
        let output = crate::git::command()
            .args(args)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(dir: &Path) {
        git(dir, &["init", "-b", "main"]);
        std::fs::write(dir.join("README.md"), "hello\n").unwrap();
        git(dir, &["add", "README.md"]);
        git(dir, &["commit", "-m", "chore: initial commit"]);
    }

    fn task_fixture(
        _legacy_id: &str,
        format_id: &str,
        epic_id: &str,
        title: &str,
        status: TaskStatus,
    ) -> Task {
        Task {
            id: format_id.into(),
            format_id: format_id.into(),
            epic_id: Some(epic_id.into()),
            standalone_reason: None,
            specs: vec![],
            title: title.into(),
            description: None,
            status,
            work_type: "feat".into(),
            priority: "normal".into(),
            estimate: None,
            acceptance: vec![],
            tests: vec![],
            depends_on: vec![],
            integration_target: None,
            branch: None,
            pr_number: None,
            created_at: "2026-07-05T00:00:00Z".into(),
            updated_at: "2026-07-05T00:00:00Z".into(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn minimal_tier_is_graceful_with_notes() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());

        let view = collect_status(dir.path());
        assert_eq!(view.branch.as_deref(), Some("main"));
        assert!(view.work.is_none());
        assert!(view.capabilities.is_none());
        assert!(
            view.notes.iter().any(|n| n.contains("project-management")),
            "notes: {:?}",
            view.notes
        );
        assert!(
            view.notes.iter().any(|n| n.contains("capabilities.md")),
            "notes: {:?}",
            view.notes
        );

        let rendered = render_status(&view, false);
        assert!(rendered.contains("branch: main"));
        assert!(rendered.contains("(no project-management tier)"));
        assert!(rendered.contains("(no registry)"));
    }

    #[test]
    fn non_repo_directory_is_graceful() {
        let dir = tempfile::tempdir().unwrap();
        let view = collect_status(dir.path());
        assert!(view.branch.is_none());
        assert!(
            view.notes
                .iter()
                .any(|n| n.contains("not a git repository")),
            "notes: {:?}",
            view.notes
        );
    }

    #[test]
    fn full_tier_counts_work_and_capabilities() {
        use crate::models::{Epic, EpicStatus, Task, TaskStatus};

        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());

        // Workgraph records via the production store.
        let store = MarkdownStore::new(dir.path().join("project-management")).unwrap();
        store
            .create_epic(&Epic {
                id: "EPC-001".into(),
                format_id: "EPC-001".into(),
                title: "Build the flow".into(),
                summary: None,
                status: EpicStatus::InProgress,
                work_type: "feat".into(),
                priority: "high".into(),
                pr_number: None,
                created_at: "2026-06-11T00:00:00Z".into(),
                updated_at: "2026-06-11T00:00:00Z".into(),
            })
            .unwrap();
        store
            .create_task(&Task {
                id: "TSK-001-001".into(),
                format_id: "TSK-001-001".into(),
                epic_id: Some("EPC-001".into()),
                standalone_reason: None,
                specs: vec![],
                title: "Wire the CLI".into(),
                description: None,
                status: TaskStatus::Todo,
                work_type: "feat".into(),
                priority: "normal".into(),
                estimate: None,
                acceptance: vec![],
                tests: vec![],
                depends_on: vec![],
                integration_target: None,
                branch: None,
                pr_number: None,
                created_at: "2026-06-11T00:00:00Z".into(),
                updated_at: "2026-06-11T00:00:00Z".into(),
                started_at: None,
                completed_at: None,
            })
            .unwrap();

        // Capability registry.
        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/capabilities.md"),
            "# caps\n\n```yaml\nid: CAP-001\nname: integrate flow\narea: engine\nstatus: building\nverified_by: []\nepics: [EPC-001]\nadrs: []\n```\n",
        )
        .unwrap();

        let view = collect_status(dir.path());
        let work = view.work.as_ref().expect("work tier present");
        assert_eq!(work.epics_by_status.get("in_progress"), Some(&1));
        assert_eq!(work.tasks_by_status.get("todo"), Some(&1));
        assert_eq!(work.active_epics, vec!["EPC-001 Build the flow"]);
        assert!(work.active_tasks.is_empty());

        let caps = view.capabilities.as_ref().expect("registry present");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].id, "CAP-001");

        let rendered = render_status(&view, true);
        assert!(rendered.contains("in flight (epic): EPC-001 Build the flow"));
        assert!(rendered.contains("capabilities: 1 (building 1)"));
        assert!(rendered.contains("CAP-001"));
        assert!(rendered.contains("integrate flow"));
    }

    #[test]
    fn trimmed_records_without_priority_are_counted() {
        // Records authored from the trimmed templates omit priority (and, for
        // tasks, estimate/acceptance/tests). They must still parse and be
        // counted by status — not silently skipped by the store as unreadable,
        // which would undercount in-flight work.
        let dir = tempfile::tempdir().unwrap();
        let pm = dir.path().join("project-management");
        std::fs::create_dir_all(pm.join("epics")).unwrap();
        std::fs::create_dir_all(pm.join("tasks")).unwrap();

        std::fs::write(
            pm.join("epics/EPC-060.md"),
            "---\nid: epic-060\nformat_id: EPC-060\ntitle: Trimmed epic\nstatus: in_progress\nwork_type: feat\ncreated_at: 2026-07-05T00:00:00Z\nupdated_at: 2026-07-05T00:00:00Z\n---\n## Summary\nTrimmed.\n",
        )
        .unwrap();
        std::fs::write(
            pm.join("tasks/TSK-060-001.md"),
            "---\nid: task-060-001\nformat_id: TSK-060-001\nepic_id: epic-060\ntitle: Trimmed task\nstatus: todo\nwork_type: feat\ncreated_at: 2026-07-05T00:00:00Z\nupdated_at: 2026-07-05T00:00:00Z\n---\n## Description\nTrimmed.\n\n## Acceptance Criteria\n\n- [ ]\n",
        )
        .unwrap();

        let view = collect_status(dir.path());
        let work = view.work.as_ref().expect("work tier present");
        assert_eq!(
            work.epics_by_status.get("in_progress"),
            Some(&1),
            "trimmed epic must be counted, not skipped: {:?}",
            view.notes
        );
        assert_eq!(
            work.tasks_by_status.get("todo"),
            Some(&1),
            "trimmed task must be counted, not skipped: {:?}",
            view.notes
        );
    }

    #[test]
    fn delivery_groups_tasks_under_capability_via_epic_link() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());

        let store = MarkdownStore::new(dir.path().join("project-management")).unwrap();
        let mk_epic = |_legacy_id: &str, fid: &str, title: &str| Epic {
            id: fid.into(),
            format_id: fid.into(),
            title: title.into(),
            summary: None,
            status: EpicStatus::InProgress,
            work_type: "feat".into(),
            priority: "high".into(),
            pr_number: None,
            created_at: "2026-07-05T00:00:00Z".into(),
            updated_at: "2026-07-05T00:00:00Z".into(),
        };
        // Epic linked to the capability, plus an unrelated epic whose tasks
        // must NOT roll up under the capability.
        store
            .create_epic(&mk_epic("epic-01a", "EPC-001", "Deliver flow"))
            .unwrap();
        store
            .create_epic(&mk_epic("epic-02b", "EPC-002", "Unrelated"))
            .unwrap();
        // Two tasks under the linked epic (one open, one done) and one task
        // under the unrelated epic.
        store
            .create_task(&task_fixture(
                "task-01a",
                "TSK-001-001",
                "EPC-001",
                "Wire CLI",
                TaskStatus::Todo,
            ))
            .unwrap();
        store
            .create_task(&task_fixture(
                "task-01b",
                "TSK-001-002",
                "EPC-001",
                "Ship it",
                TaskStatus::Complete,
            ))
            .unwrap();
        store
            .create_task(&task_fixture(
                "task-02a",
                "TSK-002-001",
                "EPC-002",
                "Elsewhere",
                TaskStatus::Todo,
            ))
            .unwrap();

        std::fs::create_dir_all(dir.path().join("docs")).unwrap();
        std::fs::write(
            dir.path().join("docs/capabilities.md"),
            "# caps\n\n```yaml\nid: CAP-001\nname: flow delivery\narea: engine\nstatus: building\nverified_by: []\nepics: [EPC-001, EPC-404]\nadrs: []\n```\n",
        )
        .unwrap();

        let view = collect_status(dir.path());
        let delivery = view.delivery.as_ref().expect("delivery present");
        assert_eq!(delivery.len(), 1);
        let cap = &delivery[0];
        assert_eq!(
            (cap.id.as_str(), cap.status.as_str()),
            ("CAP-001", "building")
        );

        // Only EPC-001 resolves; its two tasks group here (one open of two),
        // while the unrelated epic's task does not leak in.
        assert_eq!(cap.epics.len(), 1);
        let epic = &cap.epics[0];
        assert_eq!(epic.format_id, "EPC-001");
        assert_eq!(epic.total_tasks, 2);
        assert_eq!(epic.open_tasks, 1);
        assert_eq!(epic.next_tasks, vec!["TSK-001-001 Wire CLI"]);

        // A referenced-but-missing epic is surfaced, not silently dropped.
        assert_eq!(cap.missing_epics, vec!["EPC-404"]);

        let rendered = render_delivery(&view);
        assert!(rendered.contains("capability delivery:"), "{rendered}");
        assert!(rendered.contains("CAP-001"), "{rendered}");
        assert!(
            rendered.contains("EPC-001 Deliver flow  [in_progress]  tasks 1/2 open"),
            "{rendered}"
        );
        assert!(
            rendered.contains("next: TSK-001-001 Wire CLI"),
            "{rendered}"
        );
        assert!(rendered.contains("EPC-404 (no epic record)"), "{rendered}");
        // The unrelated epic and its task never appear in the rollup.
        assert!(!rendered.contains("Elsewhere"), "{rendered}");
        assert!(!rendered.contains("EPC-002"), "{rendered}");
    }

    #[test]
    fn worktrees_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let wt_path = dir.path().join("wt-feature");
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                wt_path.to_str().unwrap(),
                "-b",
                "feat/wt",
            ],
        );

        let view = collect_status(dir.path());
        assert_eq!(view.worktrees.len(), 1);
        assert_eq!(view.worktrees[0].name, "wt-feature");

        let rendered = render_status(&view, false);
        assert!(rendered.contains("worktrees: 1"));
        assert!(rendered.contains("wt-feature"));
    }

    #[test]
    fn cleanup_preserves_dirty_and_unproven_worktrees() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let dirty_path = dir.path().join("wt-dirty");
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                dirty_path.to_str().unwrap(),
                "-b",
                "feat/dirty",
            ],
        );
        std::fs::write(dirty_path.join("draft.txt"), "uncommitted\n").unwrap();

        let pending_path = dir.path().join("wt-pending");
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                pending_path.to_str().unwrap(),
                "-b",
                "feat/pending",
            ],
        );
        std::fs::write(pending_path.join("pending.txt"), "not landed\n").unwrap();
        git(&pending_path, &["add", "pending.txt"]);
        git(&pending_path, &["commit", "-m", "feat: add pending work"]);

        let view = collect_status(dir.path());
        let dirty = view
            .cleanup
            .iter()
            .find(|item| item.name == "feat/dirty")
            .unwrap();
        assert_eq!(
            dirty.disposition,
            CleanupDisposition::PreserveDirty,
            "{:?}",
            view.cleanup
        );
        let pending = view
            .cleanup
            .iter()
            .find(|item| item.name == "feat/pending")
            .unwrap();
        assert_eq!(
            pending.disposition,
            CleanupDisposition::RetainUnproven,
            "{:?}",
            view.cleanup
        );
    }

    #[test]
    fn cleanup_recognizes_squash_landing_by_patch_equivalence() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        let wt_path = dir.path().join("wt-squashed");
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                wt_path.to_str().unwrap(),
                "-b",
                "feat/squashed",
            ],
        );
        std::fs::write(wt_path.join("squashed.txt"), "landed once\n").unwrap();
        git(&wt_path, &["add", "squashed.txt"]);
        git(&wt_path, &["commit", "-m", "feat: add squashed change"]);

        git(dir.path(), &["merge", "--squash", "feat/squashed"]);
        git(dir.path(), &["commit", "-m", "feat: land squashed change"]);

        let view = collect_status(dir.path());
        let squashed = view
            .cleanup
            .iter()
            .find(|item| item.name == "feat/squashed")
            .unwrap();
        assert_eq!(
            squashed.disposition,
            CleanupDisposition::Removable,
            "{:?}",
            view.cleanup
        );
        assert!(squashed.proof.contains("patch-equivalent"));
        let rendered = render_status(&view, false);
        let rendered_path = squashed.path.as_deref().expect("worktree has a path");
        assert_eq!(
            std::fs::canonicalize(rendered_path).unwrap(),
            std::fs::canonicalize(&wt_path).unwrap(),
            "reported path must resolve to the linked worktree"
        );
        assert!(
            rendered.contains(&format!(
                "removable worktree feat/squashed -> {rendered_path} (patch-equivalent in main)"
            )),
            "{rendered}"
        );
    }

    #[test]
    fn cleanup_rejects_option_shaped_refs_before_git_cherry() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!patch_equivalent(dir.path(), "main", "--unsafe"));
        assert!(!patch_equivalent(dir.path(), "--unsafe", "feat/safe"));
    }

    #[test]
    fn cleanup_does_not_apply_root_dirtiness_to_unattached_branches() {
        let dir = tempfile::tempdir().unwrap();
        init_repo(dir.path());
        git(dir.path(), &["branch", "feat/already-landed"]);
        std::fs::write(dir.path().join("local-draft.txt"), "root-only\n").unwrap();

        let view = collect_status(dir.path());
        let branch = view
            .cleanup
            .iter()
            .find(|item| item.name == "feat/already-landed")
            .unwrap();
        assert_eq!(
            branch.disposition,
            CleanupDisposition::Removable,
            "{:?}",
            view.cleanup
        );
        assert!(branch.proof.contains("ancestry"));
        let rendered = render_status(&view, false);
        assert!(
            rendered.contains(
                "cleanup: 1 candidate(s) against main\n  removable branch \
                 feat/already-landed (landed by ancestry in main)"
            ),
            "{rendered}"
        );
    }

    #[test]
    fn format_counts_empty_and_filled() {
        assert_eq!(format_counts(&BTreeMap::new()), "0");
        let mut counts = BTreeMap::new();
        counts.insert("draft".to_string(), 2);
        counts.insert("in_progress".to_string(), 1);
        assert_eq!(format_counts(&counts), "3 (draft 2, in_progress 1)");
    }
}
