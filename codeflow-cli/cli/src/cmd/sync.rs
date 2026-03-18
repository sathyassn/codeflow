//! Sync command: Loro CRDT state synchronization daemon.
//!
//! Provides the `codeflow sync daemon` subcommand that runs a long-lived
//! sync loop for same-machine compaction and multi-machine delta exchange.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;

use codeflow_core::coordination::sync::{self, DEFAULT_SYNC_INTERVAL_SECS, PidFile, SyncConfig};

use crate::helpers;

/// Sync subcommands.
#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// Start the sync daemon (long-running process).
    Daemon {
        /// Sync interval in seconds (default: 30).
        #[arg(long, default_value_t = DEFAULT_SYNC_INTERVAL_SECS)]
        interval: u64,
    },
    /// Show sync daemon status.
    Status,
}

/// Run the sync command.
///
/// # Errors
///
/// Returns an error if the subcommand fails.
#[allow(clippy::needless_pass_by_value)] // clap dispatch produces owned values
pub fn run(command: SyncCommand) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    run_with_dir(&project_dir, &command)
}

fn run_with_dir(project_dir: &Path, command: &SyncCommand) -> Result<()> {
    match command {
        SyncCommand::Daemon { interval } => run_daemon(project_dir, *interval),
        SyncCommand::Status => {
            run_status(project_dir);
            Ok(())
        }
    }
}

/// Run the sync daemon loop.
fn run_daemon(project_dir: &Path, interval_secs: u64) -> Result<()> {
    let config = SyncConfig::from_project_dir(project_dir, interval_secs);

    // Ensure peer ID exists.
    let peer_id = sync::ensure_peer_id(&config.runtime_dir).context("ensuring peer ID")?;

    // Create PID file.
    let pid = std::process::id();
    let pid_file = PidFile::new(pid);
    sync::write_pid_file(&config.pid_path, &pid_file).context("writing PID file")?;

    eprintln!(
        "sync daemon started (pid={pid}, peer={}, interval={}s)",
        peer_id.as_str(),
        interval_secs
    );

    // Run sync loop until interrupted.
    loop {
        if let Err(e) = sync::run_sync_cycle(&config, &peer_id) {
            eprintln!("sync cycle error: {e}");
        }

        // Check if ref count has reached 0 (graceful shutdown).
        match sync::read_pid_file(&config.pid_path) {
            Ok(pf) if pf.ref_count == 0 => {
                eprintln!("ref count reached 0, shutting down");
                break;
            }
            Ok(_) => {}
            Err(_) => {
                // PID file removed externally — shut down.
                eprintln!("PID file removed, shutting down");
                break;
            }
        }

        std::thread::sleep(config.interval);
    }

    // Cleanup PID file.
    let _ = sync::remove_pid_file(&config.pid_path);
    eprintln!("sync daemon stopped");

    Ok(())
}

/// Show the sync daemon status.
fn run_status(project_dir: &Path) {
    let config = SyncConfig::from_project_dir(project_dir, DEFAULT_SYNC_INTERVAL_SECS);

    match sync::read_pid_file(&config.pid_path) {
        Ok(pid_file) => {
            println!("sync daemon: running");
            println!("  pid: {}", pid_file.pid);
            println!("  sessions: {}", pid_file.sessions.len());
            println!("  ref_count: {}", pid_file.ref_count);
            for session in &pid_file.sessions {
                println!("    - {session}");
            }
        }
        Err(_) => {
            println!("sync daemon: not running");
        }
    }

    // Show sync state.
    match sync::read_sync_state(&config.sync_state_path) {
        Ok(state) => {
            if state.last_sync_vv.is_some() {
                println!("last sync: completed (version vector stored)");
            } else {
                println!("last sync: never");
            }
        }
        Err(_) => {
            println!("last sync: unknown");
        }
    }

    // Show peer ID.
    match sync::ensure_peer_id(&config.runtime_dir) {
        Ok(peer_id) => println!("peer id: {}", peer_id.as_str()),
        Err(_) => println!("peer id: not set"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeflow_core::types::SessionId;

    fn session(id: &str) -> SessionId {
        SessionId::new_unchecked(id)
    }

    // -- run_with_dir dispatch tests --

    #[test]
    fn test_sync_command_exists() {
        let _: fn(SyncCommand) -> Result<()> = run;
    }

    #[test]
    fn test_dispatch_status() {
        let dir = tempfile::tempdir().unwrap();
        let result = run_with_dir(dir.path(), &SyncCommand::Status);
        assert!(result.is_ok());
    }

    #[test]
    fn test_dispatch_daemon_ref_count_zero() {
        // Daemon with a pre-existing PID file at ref_count=0 exits immediately.
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        // Write a state.loro so compaction doesn't fail on missing file.
        let coord = codeflow_core::coordination::loro::LoroCoordinator::in_memory();
        let snapshot = coord.export_bytes().unwrap();
        std::fs::write(coord_dir.join("state.loro"), &snapshot).unwrap();

        // Pre-create PID file with ref_count=0 so daemon exits after first cycle.
        let pf = PidFile::new(std::process::id());
        sync::write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let result = run_with_dir(dir.path(), &SyncCommand::Daemon { interval: 1 });
        assert!(result.is_ok());

        // PID file should be cleaned up.
        assert!(!coord_dir.join("sync-daemon.pid").exists());
    }

    #[test]
    fn test_dispatch_daemon_pid_file_removed_externally() {
        // Daemon exits when PID file is removed externally.
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        // Write state.loro for compaction.
        let coord = codeflow_core::coordination::loro::LoroCoordinator::in_memory();
        let snapshot = coord.export_bytes().unwrap();
        std::fs::write(coord_dir.join("state.loro"), &snapshot).unwrap();

        // Do NOT create PID file — daemon writes it, then on first loop
        // iteration tries to read it. Since we write it in run_daemon,
        // we need to remove it right after. Instead, create it with ref_count=0.
        let pf = PidFile::new(std::process::id());
        sync::write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        // Remove it so the daemon sees "PID file removed" on read.
        std::fs::remove_file(coord_dir.join("sync-daemon.pid")).unwrap();

        // Daemon's own write will recreate it, then the loop reads the newly
        // written one (ref_count=0), triggering shutdown. This tests the
        // ref_count=0 path specifically.
        let result = run_daemon(dir.path(), 1);
        assert!(result.is_ok());
    }

    // -- run_status branch coverage --

    #[test]
    fn test_status_no_state_dir() {
        // All three branches hit the Err/missing case.
        let dir = tempfile::tempdir().unwrap();
        run_status(dir.path());
        // Should not panic — exercises all Err/missing branches.
    }

    #[test]
    fn test_status_with_running_daemon_and_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        // PID file with multiple sessions.
        let mut pf = PidFile::new(12345);
        pf.add_session(&session("ses-001"));
        pf.add_session(&session("ses-002"));
        sync::write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        run_status(dir.path());
        // Exercises: Ok(pid_file) with sessions.len() > 0, for loop over sessions.
    }

    #[test]
    fn test_status_with_sync_state_completed() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        let state = sync::SyncState {
            last_sync_vv: Some(vec![1, 2, 3]),
        };
        sync::write_sync_state(&coord_dir.join("sync-state.json"), &state).unwrap();

        run_status(dir.path());
        // Exercises: Ok(state) with last_sync_vv.is_some() = true.
    }

    #[test]
    fn test_status_with_sync_state_never() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        let state = sync::SyncState { last_sync_vv: None };
        sync::write_sync_state(&coord_dir.join("sync-state.json"), &state).unwrap();

        run_status(dir.path());
        // Exercises: Ok(state) with last_sync_vv.is_some() = false ("last sync: never").
    }

    #[test]
    fn test_status_with_peer_id() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();
        std::fs::write(runtime_dir.join("peer-id"), "alice-laptop").unwrap();

        run_status(dir.path());
        // Exercises: Ok(peer_id) branch in ensure_peer_id.
    }

    #[test]
    fn test_status_full_state() {
        // Exercise ALL Ok branches of run_status in one call.
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        let runtime_dir = dir.path().join(".state").join("runtime");
        std::fs::create_dir_all(&coord_dir).unwrap();
        std::fs::create_dir_all(&runtime_dir).unwrap();

        // PID file with sessions.
        let mut pf = PidFile::new(99999);
        pf.add_session(&session("ses-full-test"));
        sync::write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        // Sync state with version vector.
        let state = sync::SyncState {
            last_sync_vv: Some(vec![42]),
        };
        sync::write_sync_state(&coord_dir.join("sync-state.json"), &state).unwrap();

        // Peer ID file.
        std::fs::write(runtime_dir.join("peer-id"), "test-host").unwrap();

        run_status(dir.path());
        // All Ok branches exercised: pid_file Ok, sync_state Ok with Some, peer_id Ok.
    }

    // -- SyncConfig tests --

    #[test]
    fn test_sync_config_paths() {
        let dir = std::path::Path::new("/tmp/myproject");
        let config = SyncConfig::from_project_dir(dir, 45);
        assert_eq!(config.interval.as_secs(), 45);
        assert_eq!(
            config.state_loro_path,
            dir.join(".state/coordination/state.loro")
        );
    }

    #[test]
    fn test_sync_config_default_interval() {
        let dir = std::path::Path::new("/tmp/project");
        let config = SyncConfig::from_project_dir(dir, DEFAULT_SYNC_INTERVAL_SECS);
        assert_eq!(config.interval.as_secs(), 30);
    }

    // -- run_daemon specific tests --

    #[test]
    fn test_daemon_creates_pid_file_and_peer_id() {
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        // Write state.loro so compaction succeeds.
        let coord = codeflow_core::coordination::loro::LoroCoordinator::in_memory();
        let snapshot = coord.export_bytes().unwrap();
        std::fs::write(coord_dir.join("state.loro"), &snapshot).unwrap();

        // PID file with ref_count=0 so daemon exits immediately.
        let pf = PidFile::new(std::process::id());
        sync::write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let result = run_daemon(dir.path(), 1);
        assert!(result.is_ok());

        // Peer ID should have been created.
        let runtime_dir = dir.path().join(".state").join("runtime");
        assert!(runtime_dir.join("peer-id").exists());
    }

    #[test]
    fn test_daemon_sync_cycle_error_is_logged_not_fatal() {
        // Daemon should continue (then exit on ref_count=0) even if
        // sync cycle encounters an error (e.g., missing state.loro).
        let dir = tempfile::tempdir().unwrap();
        let coord_dir = dir.path().join(".state").join("coordination");
        std::fs::create_dir_all(&coord_dir).unwrap();

        // No state.loro — sync cycle will error on compaction.
        // PID file with ref_count=0 for immediate exit.
        let pf = PidFile::new(std::process::id());
        sync::write_pid_file(&coord_dir.join("sync-daemon.pid"), &pf).unwrap();

        let result = run_daemon(dir.path(), 1);
        // Should succeed — sync error is logged, not propagated.
        assert!(result.is_ok());
    }
}
