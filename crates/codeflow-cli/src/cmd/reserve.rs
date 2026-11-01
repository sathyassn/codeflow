//! Direct heavy commands share the gate's machine and repository admission.
use clap::Args;
use codeflow_core::testing::{
    gate_guard::{acquire_project_gate_lock, WaitTicket},
    runner::run_reserved_command,
};
use std::ffi::OsString;
use std::time::{Duration, Instant};

#[derive(Args)]
pub struct ReserveArgs {
    /// Maximum seconds to wait for another heavy run. Zero refuses immediately.
    #[arg(long, default_value_t = 0)]
    pub wait_seconds: u64,
    /// Stop the command and its descendants after this many seconds.
    #[arg(long, default_value_t = 3600)]
    pub timeout_seconds: u64,
    /// Executable and exact arguments, following --.
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<OsString>,
}

pub fn run(args: &ReserveArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow reserve: {error}");
            return 1;
        }
    };
    let config_path = root.join(".codeflow/test-config.json");
    let workers = match codeflow_core::testing::config::load_test_config(&config_path) {
        Ok(config) => config.execution.worker_budget,
        Err(codeflow_core::testing::error::TestingError::ConfigNotFound(_)) => None,
        Err(error) => {
            eprintln!("codeflow reserve: {error}");
            return 1;
        }
    };
    let start = Instant::now();
    let wait = Duration::from_secs(args.wait_seconds);
    // A waiter queues before its first attempt, so waiters take the lock in
    // arrival order and none loses a lock that frees before it queued.
    let ticket = if wait.is_zero() {
        None
    } else {
        match WaitTicket::take(&root, wait) {
            Ok(taken) => Some(taken),
            Err(error) => {
                eprintln!("codeflow reserve: refused: {error}");
                return 1;
            }
        }
    };
    let mut announced = false;
    let lock = loop {
        match acquire_project_gate_lock(&root, "reserve", ticket.as_ref()) {
            Ok(lock) => break lock,
            Err(message) => {
                if message.contains("gate lock unavailable") || start.elapsed() >= wait {
                    eprintln!("codeflow reserve: refused: {message}");
                    return 1;
                }
                if !announced {
                    announced = true;
                    eprintln!(
                        "codeflow reserve: waiting up to {}s in arrival order: {message}",
                        args.wait_seconds
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
    drop(ticket);
    for note in &lock.notes {
        eprintln!("codeflow reserve: {note}");
    }
    let cwd = std::env::current_dir().unwrap_or(root);
    match run_reserved_command(
        &args.command,
        &cwd,
        Duration::from_secs(args.timeout_seconds),
        workers,
    ) {
        Ok(result) => {
            print!("{}", result.stdout);
            eprint!("{}", result.stderr);
            if result.exit_code < 0 {
                1
            } else {
                result.exit_code
            }
        }
        Err(error) => {
            eprintln!("codeflow reserve: {error}");
            1
        }
    }
}
