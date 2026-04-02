//! Parallel execution status command.
//!
//! Shows active worktrees, claims, merge queue, and recent coordination events.

use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::Result;
use clap::Subcommand;

use crate::helpers;

/// Parallel execution subcommands.
#[derive(Debug, Subcommand)]
pub enum ParallelCommand {
    /// Show parallel execution status dashboard
    Status,
}

#[allow(clippy::needless_pass_by_value)] // clap passes enum by value
pub fn run(command: Option<ParallelCommand>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    match command.as_ref() {
        Some(ParallelCommand::Status) | None => run_status(&project_dir),
    }
}

/// Maximum number of recent coordination events to display.
const MAX_RECENT_EVENTS: usize = 10;

#[allow(clippy::unnecessary_wraps)]
fn run_status(project_dir: &Path) -> Result<()> {
    let coord_dir = project_dir.join(".state").join("coordination");
    let registry_path = project_dir.join(".state/worktrees/worktrees.yaml");
    let events_path = project_dir
        .join(".state")
        .join("ledger")
        .join("coordination-events.jsonl");

    // -- Active worktrees from registry --
    println!("=== Active Worktrees ===");
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
                            entry.branch.as_deref().unwrap_or("(detached)"),
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

    // -- Active claims from state.loro --
    println!("\n=== Active Claims ===");
    let state_loro = coord_dir.join("state.loro");
    if state_loro.exists() {
        match codeflow_core::coordination::loro::LoroCoordinator::new(&state_loro) {
            Ok(coord) => {
                let claims = codeflow_core::coordination::claims::list_active(&coord);
                if claims.is_empty() {
                    println!("  (none)");
                } else {
                    println!("  {:<40} OWNER", "PATH");
                    for (path, claim) in &claims {
                        println!("  {:<40} {}", path, claim.owner);
                    }
                }
                println!("  Total claims: {}", claims.len());
            }
            Err(e) => println!("  Error reading state.loro: {e}"),
        }
    } else {
        println!("  (no state file)");
    }

    // -- Merge queue from state.loro --
    println!("\n=== Merge Queue ===");
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

    // -- Recent coordination events --
    println!("\n=== Recent Coordination Events ===");
    print_recent_events(&events_path);

    Ok(())
}

/// Print the last N events from coordination-events.jsonl.
fn print_recent_events(events_path: &Path) {
    if !events_path.exists() {
        println!("  (no events file)");
        return;
    }

    let file = match std::fs::File::open(events_path) {
        Ok(f) => f,
        Err(e) => {
            println!("  Error reading events: {e}");
            return;
        }
    };

    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    if lines.is_empty() {
        println!("  (no events)");
        return;
    }

    let start = lines.len().saturating_sub(MAX_RECENT_EVENTS);
    let recent = &lines[start..];
    println!(
        "  (showing last {} of {} events)",
        recent.len(),
        lines.len()
    );

    for line in recent {
        // Extract the event type and timestamp for display.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            let event_type = val
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let timestamp = val.get("timestamp").and_then(|v| v.as_str()).unwrap_or("-");
            let session = val
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let path = val.get("path").and_then(|v| v.as_str()).unwrap_or("-");
            println!("  [{timestamp}] {event_type}: {path} ({session})");
        } else {
            println!("  {line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parallel_command_exists() {
        let _: fn(Option<ParallelCommand>) -> Result<()> = run;
    }

    #[test]
    fn test_status_no_state() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_with_registry() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let registry_dir = state_dir.join("worktrees");
        std::fs::create_dir_all(&registry_dir).unwrap();
        let registry_path = registry_dir.join("worktrees.yaml");

        let mut reg = codeflow_core::worktree::WorktreeRegistry::new("2026-03-21T10:00:00Z");
        reg.worktrees.push(codeflow_core::worktree::WorktreeEntry {
            name: "ses-test".to_string(),
            path: "/tmp/wt/test".to_string(),
            branch: Some("feat/test".to_string()),
            created_at: "2026-03-21T10:00:00Z".to_string(),
            status: codeflow_core::worktree::WorktreeStatus::Active,
            session_id: Some("ses-001".to_string()),
            task_id: Some("TSK-001".to_string()),
            source: None,
            lead_pid: None,
        });
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_with_empty_registry() {
        let dir = tempfile::tempdir().unwrap();
        let state_dir = dir.path().join(".state");
        let registry_dir = state_dir.join("worktrees");
        std::fs::create_dir_all(&registry_dir).unwrap();
        let registry_path = registry_dir.join("worktrees.yaml");

        let reg = codeflow_core::worktree::WorktreeRegistry::new("2026-03-21T10:00:00Z");
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_with_events_file() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join(".state/ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();
        let events_path = ledger_dir.join("coordination-events.jsonl");

        std::fs::write(
            &events_path,
            r#"{"type":"claim_acquired","session_id":"ses-001","path":"src/a.rs","timestamp":"2026-03-21T10:00:00Z"}
{"type":"claim_conflict","session_id":"ses-002","path":"src/a.rs","held_by":"ses-001","timestamp":"2026-03-21T10:01:00Z"}
"#,
        )
        .unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_with_empty_events() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join(".state/ledger");
        std::fs::create_dir_all(&ledger_dir).unwrap();
        let events_path = ledger_dir.join("coordination-events.jsonl");
        std::fs::write(&events_path, "").unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_recent_events_no_file() {
        let path = Path::new("/nonexistent/events.jsonl");
        // Should not panic.
        print_recent_events(path);
    }

    #[test]
    fn test_print_recent_events_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        std::fs::write(&path, "").unwrap();
        print_recent_events(&path);
    }

    #[test]
    fn test_print_recent_events_limits_output() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");

        // Write more than MAX_RECENT_EVENTS lines.
        let mut content = String::new();
        for i in 0..15 {
            use std::fmt::Write;
            let _ = write!(
                content,
                r#"{{"type":"claim_acquired","session_id":"ses-{i:03}","path":"src/{i}.rs","timestamp":"2026-03-21T10:{i:02}:00Z"}}"#
            );
            content.push('\n');
        }
        std::fs::write(&path, content).unwrap();

        // Should show last 10 only (MAX_RECENT_EVENTS).
        print_recent_events(&path);
    }

    #[test]
    fn test_status_with_claims() {
        use codeflow_core::coordination::Coordinator;

        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state/coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        let state_path = coord_dir.join("state.loro");
        let mut coord = codeflow_core::coordination::loro::LoroCoordinator::in_memory();
        let sid = codeflow_core::types::SessionId::new_unchecked("ses-test");
        coord.acquire("src/main.rs", &sid).unwrap();
        let bytes = coord.export_bytes().unwrap();
        std::fs::write(&state_path, bytes).unwrap();

        // Also write a registry.
        let state_dir = dir.path().join(".state");
        let registry_dir = state_dir.join("worktrees");
        std::fs::create_dir_all(&registry_dir).unwrap();
        let registry_path = registry_dir.join("worktrees.yaml");
        let reg = codeflow_core::worktree::WorktreeRegistry::new("2026-03-21T10:00:00Z");
        codeflow_core::worktree::write_registry(&registry_path, &reg).unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_status_with_queue_entries() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state/coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        let state_path = coord_dir.join("state.loro");
        let coord = codeflow_core::coordination::loro::LoroCoordinator::in_memory();
        let entry = codeflow_core::coordination::merge_queue::MergeQueueEntry {
            session_id: codeflow_core::types::SessionId::new_unchecked("ses-001"),
            task_id: "TSK-001".to_string(),
            branch: "feat/test".to_string(),
            pr_ready_at: "2026-03-21T10:00:00Z".to_string(),
        };
        codeflow_core::coordination::merge_queue::enqueue(&coord, &entry).unwrap();
        let bytes = coord.export_bytes().unwrap();
        std::fs::write(&state_path, bytes).unwrap();

        let result = run_status(dir.path());
        assert!(result.is_ok());
    }
}
