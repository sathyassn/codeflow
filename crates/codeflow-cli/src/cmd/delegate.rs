//! `codeflow delegate` — transport-neutral delegate lifecycle commands.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Args, Subcommand, ValueEnum};
use codeflow_core::delegate::{self, ErrorKind, WaitUntil};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Delegate lifecycle command.
#[derive(Debug, Args)]
pub struct DelegateArgs {
    /// Lifecycle operation.
    #[command(subcommand)]
    pub command: DelegateCommand,
}

/// Delegate lifecycle operations.
#[derive(Debug, Subcommand)]
pub enum DelegateCommand {
    /// Create an owner-only run directory and task hook settings.
    Init {
        /// Unique run identifier.
        #[arg(long, value_name = "ID")]
        run_id: String,
        /// Absolute path for the owner-only protocol state.
        #[arg(long, value_name = "DIR")]
        state_dir: PathBuf,
    },
    /// Arm one prompt for a delegate turn.
    Arm {
        /// Unique run identifier.
        #[arg(long, value_name = "ID")]
        run_id: String,
        /// Absolute path for the owner-only protocol state.
        #[arg(long, value_name = "DIR")]
        state_dir: PathBuf,
        /// Unique turn identifier within the run.
        #[arg(long, value_name = "ID")]
        turn_id: String,
        /// File containing the exact prompt bytes that the host will deliver.
        #[arg(long, value_name = "FILE")]
        prompt_file: PathBuf,
    },
    /// Wait for a durable lifecycle state.
    Wait {
        /// Unique run identifier.
        #[arg(long, value_name = "ID")]
        run_id: String,
        /// Absolute path for the owner-only protocol state.
        #[arg(long, value_name = "DIR")]
        state_dir: PathBuf,
        /// State to observe.
        #[arg(long, value_enum)]
        until: WaitState,
        /// Turn identifier; required for accepted and terminal waits.
        #[arg(long, value_name = "ID")]
        turn_id: Option<String>,
        /// Bounded wait duration in seconds.
        #[arg(long, value_name = "N")]
        timeout_seconds: u64,
    },
}

/// CLI representation of a wait state.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum WaitState {
    /// Harness startup is ready.
    Ready,
    /// Armed prompt was accepted.
    Accepted,
    /// Accepted turn completed or failed.
    Terminal,
}

/// Run a delegate lifecycle command.
#[must_use]
pub fn run(args: &DelegateArgs) -> i32 {
    match &args.command {
        DelegateCommand::Init { run_id, state_dir } => match delegate::init(run_id, state_dir) {
            Ok(settings) => {
                println!("{}", settings.display());
                0
            }
            Err(error) => fail("init", &error, 1),
        },
        DelegateCommand::Arm {
            run_id,
            state_dir,
            turn_id,
            prompt_file,
        } => {
            let prompt = match read_prompt(prompt_file) {
                Ok(prompt) => prompt,
                Err(error) => {
                    eprintln!(
                        "codeflow delegate arm: cannot read {}: {error}",
                        prompt_file.display()
                    );
                    return 1;
                }
            };
            match delegate::arm(run_id, state_dir, turn_id, &prompt) {
                Ok(()) => 0,
                Err(error) => fail("arm", &error, 1),
            }
        }
        DelegateCommand::Wait {
            run_id,
            state_dir,
            until,
            turn_id,
            timeout_seconds,
        } => {
            INTERRUPTED.store(false, Ordering::SeqCst);
            install_interrupt_handler();
            let until = match until {
                WaitState::Ready => WaitUntil::Ready,
                WaitState::Accepted => WaitUntil::Accepted,
                WaitState::Terminal => WaitUntil::Terminal,
            };
            match delegate::wait(
                run_id,
                state_dir,
                turn_id.as_deref(),
                until,
                Duration::from_secs(*timeout_seconds),
                || INTERRUPTED.load(Ordering::SeqCst),
            ) {
                Ok(result) => {
                    print!("{}", result.json);
                    if result.failed {
                        10
                    } else {
                        0
                    }
                }
                Err(error) => {
                    let code = match error.kind {
                        ErrorKind::Timeout => 124,
                        ErrorKind::Interrupted => 130,
                        ErrorKind::Unsafe | ErrorKind::Invalid => 11,
                    };
                    fail("wait", &error, code)
                }
            }
        }
    }
}

fn read_prompt(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    let limit = u64::try_from(delegate::MAX_PROMPT_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut prompt = Vec::with_capacity(delegate::MAX_PROMPT_BYTES.min(64 * 1024));
    file.take(limit).read_to_end(&mut prompt)?;
    Ok(prompt)
}

fn fail(operation: &str, error: &delegate::DelegateError, code: i32) -> i32 {
    eprintln!("codeflow delegate {operation}: {error}");
    code
}

#[cfg(unix)]
fn install_interrupt_handler() {
    unsafe extern "C" fn handle_interrupt(_signal: i32) {
        INTERRUPTED.store(true, Ordering::SeqCst);
    }
    unsafe extern "C" {
        fn signal(signal: i32, handler: unsafe extern "C" fn(i32)) -> usize;
    }
    // SAFETY: SIGINT is a standard Unix signal and the handler only performs
    // an atomic store, which is signal-safe.
    unsafe {
        let _ = signal(2, handle_interrupt);
    }
}

#[cfg(not(unix))]
fn install_interrupt_handler() {}
