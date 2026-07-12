//! Generated status views — never stored (charter §3.1, principle 3).
//!
//! `codeflow status` computes everything live: current branch + worktrees
//! from git, in-flight work from the workgraph `MarkdownStore`, and the
//! capability table from `docs/capabilities.md`. Every tier degrades
//! gracefully: absent layers become notes, never errors.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::capability::{CapabilityEntry, parse_capabilities};
use crate::models::{EpicFilter, TaskFilter};
use crate::workgraph::{MarkdownStore, RecordStore};

/// A git worktree attached to the repository.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub name: String,
    pub path: String,
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

/// The complete generated view.
#[derive(Debug)]
pub struct StatusView {
    /// Current branch; `None` outside a git repo or on unborn HEAD.
    pub branch: Option<String>,
    pub worktrees: Vec<WorktreeInfo>,
    /// `None` when the project-management tier is absent.
    pub work: Option<WorkSummary>,
    /// `None` when `docs/capabilities.md` is absent.
    pub capabilities: Option<Vec<CapabilityEntry>>,
    /// Tier/degradation notes (absent layers, parse problems).
    pub notes: Vec<String>,
}

/// Collect the status view for a repository root. Read-only; works at any
/// tier — absent layers are reported in `notes`.
#[must_use]
pub fn collect_status(repo_root: &Path) -> StatusView {
    let mut notes = Vec::new();

    let (branch, worktrees) = if let Ok(repo) = git2::Repository::open(repo_root) {
        let branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(ToString::to_string));
        (branch, list_worktrees(&repo))
    } else {
        notes.push("not a git repository — branch/worktree state unavailable".to_string());
        (None, Vec::new())
    };

    let work = collect_work(repo_root, &mut notes);
    let capabilities = collect_capabilities(repo_root, &mut notes);

    StatusView {
        branch,
        worktrees,
        work,
        capabilities,
        notes,
    }
}

fn list_worktrees(repo: &git2::Repository) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    if let Ok(names) = repo.worktrees() {
        for name in names.iter().flatten() {
            if let Ok(wt) = repo.find_worktree(name) {
                out.push(WorktreeInfo {
                    name: name.to_string(),
                    path: wt.path().display().to_string(),
                });
            }
        }
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
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
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
            view.notes.iter().any(|n| n.contains("not a git repository")),
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
                id: "epic-01a".into(),
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
                id: "task-01a".into(),
                format_id: "TSK-001-001".into(),
                epic_id: "epic-01a".into(),
                title: "Wire the CLI".into(),
                description: None,
                status: TaskStatus::Todo,
                work_type: "feat".into(),
                priority: "normal".into(),
                estimate: None,
                acceptance: vec![],
                tests: vec![],
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
    fn format_counts_empty_and_filled() {
        assert_eq!(format_counts(&BTreeMap::new()), "0");
        let mut counts = BTreeMap::new();
        counts.insert("draft".to_string(), 2);
        counts.insert("in_progress".to_string(), 1);
        assert_eq!(format_counts(&counts), "3 (draft 2, in_progress 1)");
    }
}
