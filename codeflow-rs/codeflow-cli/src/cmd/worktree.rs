//! Worktree command: git worktree lifecycle management.

use anyhow::{Context, Result};
use codeflow_core::worktree::WorktreeManager;

use crate::helpers;

pub fn run() -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let mgr = WorktreeManager::new(&project_dir);

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

    #[test]
    fn test_worktree_command_exists() {
        let _: fn() -> Result<()> = run;
    }

    #[test]
    fn test_worktree_manager_construction() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = WorktreeManager::new(dir.path());
        // Manager should construct even without a git repo.
        let _ = mgr;
    }
}
