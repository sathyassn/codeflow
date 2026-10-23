//! Direct heavy commands share the gate's machine and repository admission.
use clap::Args;
use codeflow_core::testing::{gate_guard::acquire_project_gate_lock, runner::run_reserved_command};
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
    let workers = match config_path.try_exists() {
        Ok(false) => None,
        Ok(true) => match codeflow_core::testing::config::load_test_config(&config_path) {
            Ok(config) => config.execution.worker_budget,
            Err(error) => {
                eprintln!("codeflow reserve: {error}");
                return 1;
            }
        },
        Err(error) => {
            eprintln!("codeflow reserve: cannot read test config: {error}");
            return 1;
        }
    };
    let start = Instant::now();
    let lock = loop {
        match acquire_project_gate_lock(&root, "reserve") {
            Ok(lock) => break lock,
            Err(message) => {
                if message.contains("gate lock unavailable")
                    || start.elapsed() >= Duration::from_secs(args.wait_seconds)
                {
                    eprintln!("codeflow reserve: refused: {message}");
                    return 1;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
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
