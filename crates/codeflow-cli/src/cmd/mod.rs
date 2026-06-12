//! CLI subcommand modules and dispatch.
//!
//! `main.rs` stays a thin parser (it is a known merge hotspot across
//! parallel workstreams); everything else — the subcommand enum, argument
//! structs, and dispatch — lives here.

pub mod integrate;
pub mod status;
pub mod test;
pub mod validate;

use std::path::PathBuf;

use clap::Subcommand;

/// Subcommands wired by the g-flow workstream (charter §3.1).
#[derive(Subcommand)]
pub enum Command {
    /// Run the test gate (configured targets or runtime stack detection)
    Test(test::TestArgs),
    /// Validate record frontmatter; --docs adds the doc-graph integrity lint
    Validate(validate::ValidateArgs),
    /// Generated status view: branch, worktrees, in-flight work, capabilities
    Status(status::StatusArgs),
    /// Land a branch into a target: flock(rebase -> test -> ff-merge)
    Integrate(integrate::IntegrateArgs),
}

/// Dispatch a parsed subcommand; returns the process exit code.
pub fn run(command: Command) -> i32 {
    match command {
        Command::Test(args) => test::run(&args),
        Command::Validate(args) => validate::run(&args),
        Command::Status(args) => status::run(&args),
        Command::Integrate(args) => integrate::run(&args),
    }
}

/// Walk up from the current directory to the nearest git repository root.
/// Falls back to the current directory when none is found (commands that
/// don't need git still work there).
pub(crate) fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.clone();
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return cwd,
        }
    }
}
