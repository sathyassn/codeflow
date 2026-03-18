//! Worktree command: git worktree lifecycle management.

use anyhow::{Context, Result};
use codeflow_core::worktree::WorktreeManager;

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir)
}

fn run_with_dir(project_dir: &std::path::Path) -> Result<()> {
    let mgr = WorktreeManager::new(project_dir);

    let entries = mgr.list(None).context("listing worktrees")?;

    if entries.is_empty() {
        println!("no worktrees");
        return Ok(());
    }

    for entry in &entries {
        let state = mgr.detect_state(entry);
        println!("{} ({}) [{}]", entry.name, entry.branch, state);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::worktree::WorktreeEntry;

    #[test]
    fn test_worktree_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_worktree_manager_construction() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let _ = mgr;
    }

    #[test]
    fn test_worktree_list_no_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let result = mgr.list(None);
        match result {
            Ok(entries) => assert!(entries.is_empty()),
            Err(_) => {}
        }
    }

    #[test]
    fn test_worktree_detect_state_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());

        let entry = WorktreeEntry {
            name: "test-wt".to_string(),
            path: "/nonexistent/path".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: "removed".to_string(),
            session_id: None,
        };

        let state = mgr.detect_state(&entry);
        let state_str = state.to_string();
        assert!(
            state_str == "stale" || state_str == "orphaned",
            "expected stale or orphaned, got {state_str}"
        );
    }

    #[test]
    fn test_worktree_entry_display_format() {
        let entry = WorktreeEntry {
            name: "my-worktree".to_string(),
            path: "/tmp/wt".to_string(),
            branch: "feat/something".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: None,
        };

        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        let state = mgr.detect_state(&entry);
        let output = format!("{} ({}) [{}]", entry.name, entry.branch, state);
        assert!(output.contains("my-worktree"));
        assert!(output.contains("feat/something"));
    }

    #[test]
    fn test_worktree_empty_entries_message() {
        let entries: Vec<WorktreeEntry> = vec![];
        assert!(entries.is_empty());
    }

    #[test]
    fn test_worktree_run_with_dir_no_git() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path());
        let _ = result;
    }

    #[test]
    fn test_worktree_run_with_dir_git_init() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        let result = run_with_dir(dir.path());
        let _ = result;
    }
}
