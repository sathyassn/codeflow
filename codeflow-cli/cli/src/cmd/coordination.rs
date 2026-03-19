//! Coordination command: coordination operations for multi-agent workflows.

use std::path::Path;

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

#[derive(Debug, Subcommand)]
pub enum CoordinationCommand {
    /// Show active workers and merge queue status
    Status,
}

#[allow(clippy::needless_pass_by_value)] // clap passes enum by value
pub fn run(command: Option<CoordinationCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    match command.as_ref() {
        Some(CoordinationCommand::Status) | None => run_status(&project_dir),
    }
}

#[allow(clippy::unnecessary_wraps)]
fn run_status(project_dir: &Path) -> Result<()> {
    let coord_dir = project_dir.join(".state").join("coordination");
    let registry_path = project_dir.join(".state").join("worktrees.yaml");

    // -- Active workers from worktree registry --
    println!("=== Active Workers ===");
    if registry_path.exists() {
        match codeflow_core::worktree::read_registry(&registry_path) {
            Ok(reg) => {
                let active = codeflow_core::worktree::list_active(&reg);
                if active.is_empty() {
                    println!("  (none)");
                } else {
                    println!("  {:<20} {:<30} {:<20} TASK", "NAME", "BRANCH", "SESSION");
                    for entry in &active {
                        println!(
                            "  {:<20} {:<30} {:<20} {}",
                            entry.name,
                            if entry.branch.is_empty() {
                                "(detached)"
                            } else {
                                &entry.branch
                            },
                            entry.session_id.as_deref().unwrap_or("-"),
                            entry.task_id.as_deref().unwrap_or("-"),
                        );
                    }
                }
                println!("  Total active: {}/{}", active.len(), 3);
            }
            Err(e) => println!("  Error reading registry: {e}"),
        }
    } else {
        println!("  (no registry)");
    }

    // -- Merge queue from state.loro --
    println!("\n=== Merge Queue ===");
    let state_loro = coord_dir.join("state.loro");
    if state_loro.exists() {
        match codeflow_core::coordination::loro::LoroCoordinator::new(&state_loro) {
            Ok(coord) => {
                let len = codeflow_core::coordination::merge_queue::queue_len(&coord);
                if len == 0 {
                    println!("  (empty)");
                } else {
                    println!("  {:<5} {:<20} {:<20} BRANCH", "POS", "SESSION", "TASK");
                    for i in 0..len {
                        match codeflow_core::coordination::merge_queue::peek_at(&coord, i) {
                            Ok(Some(entry)) => {
                                println!(
                                    "  {:<5} {:<20} {:<20} {}",
                                    i,
                                    entry.session_id.as_str(),
                                    entry.task_id,
                                    entry.branch,
                                );
                            }
                            _ => println!("  {i:<5} (unreadable)"),
                        }
                    }
                }
                println!("  Queue depth: {len}");
            }
            Err(e) => println!("  Error reading state.loro: {e}"),
        }
    } else {
        println!("  (no state file)");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordination_command_exists() {
        let _: fn(Option<CoordinationCommand>) -> Result<()> = run;
    }

    #[test]
    fn test_coordination_status_no_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_coordination_status_with_registry() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let registry_path = state_dir.join("worktrees.yaml");

        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-03-19T10:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "ses-test".to_string(),
            path: "/tmp/wt/test".to_string(),
            branch: "feat/test".to_string(),
            created_at: "2026-03-19T10:00:00Z".to_string(),
            status: "active".to_string(),
            session_id: Some("ses-001".to_string()),
            task_id: Some("TSK-001".to_string()),
        });
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_coordination_status_with_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let registry_path = state_dir.join("worktrees.yaml");

        let reg = codeflow_core::worktree::WorktreeRegistry::new("2026-03-19T10:00:00Z");
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }
}
