//! Direct heavy commands share the host by the gate's capacity admission:
//! they take a heavy-run slot, never wait for other heavy runs, and use
//! fewer jobs when more runs share the host than its job budget holds.
use clap::Args;
use codeflow_core::testing::{
    capacity, gate_guard::acquire_project_gate_lock, runner::run_reserved_command,
};
use std::ffi::OsString;
use std::time::{Duration, Instant};

#[derive(Args)]
pub struct ReserveArgs {
    /// Maximum seconds to wait while a host-exclusive target holds the host.
    /// Zero refuses immediately. Other heavy runs never make a reserve wait.
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
    let execution = match codeflow_core::testing::config::load_test_config(&config_path) {
        Ok(config) => config.execution,
        Err(codeflow_core::testing::error::TestingError::ConfigNotFound(_)) => {
            codeflow_core::testing::config::ExecutionConfig::default()
        }
        Err(error) => {
            eprintln!("codeflow reserve: {error}");
            return 1;
        }
    };
    let start = Instant::now();
    let wait = Duration::from_secs(args.wait_seconds);
    let mut announced = false;
    let lock = loop {
        match acquire_project_gate_lock(&root, "reserve") {
            Ok(lock) => break lock,
            Err(message) => {
                if !message.contains("host-exclusive") || start.elapsed() >= wait {
                    eprintln!("codeflow reserve: refused: {message}");
                    return 1;
                }
                if !announced {
                    announced = true;
                    eprintln!(
                        "codeflow reserve: waiting up to {}s: {message}",
                        args.wait_seconds
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
    for note in &lock.notes {
        eprintln!("codeflow reserve: {note}");
    }
    let host = capacity::host_jobs(execution.host_jobs);
    // Always bounded by the host's job budget, the first run included
    // (R483-05); `worker_budget` only caps it further.
    let jobs = capacity::run_jobs(host, execution.worker_budget, lock.runs);
    if capacity::lowered(host, execution.worker_budget, lock.runs) {
        eprintln!(
            "codeflow reserve: {} heavy runs share this host's {host} jobs; this command uses {jobs}",
            lock.runs
        );
    }
    let workers = Some(jobs);
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
