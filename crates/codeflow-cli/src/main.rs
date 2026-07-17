//! codeflow — the AI-development discipline layer CLI.

mod cmd;
mod embedded;
mod prompts;

use std::path::PathBuf;

use clap::{ArgGroup, Parser, Subcommand};
use codeflow_core::scaffold;

const BINARY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "codeflow", version, about = "AI-development discipline layer")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold this project (idempotent, non-destructive, offline).
    #[command(group(ArgGroup::new("tier").args(["minimal", "standard", "full"])))]
    Init {
        /// The discipline floor: full git-discipline enforcement (all hooks + CI + in-session guards + armed policy) and a lean agent contract — for any repo.
        #[arg(long)]
        minimal: bool,
        /// A code project (default): everything in minimal, plus the develop-loop skills, reviewer agents, the traceability spine (product/capabilities/architecture/ADRs), and harness integration.
        #[arg(long)]
        standard: bool,
        /// A program: everything in standard, plus project-management (epics, tasks, specs).
        #[arg(long)]
        full: bool,
        /// No prompts; sane defaults (standard tier).
        #[arg(long)]
        yes: bool,
        /// Overwrite existing files (never the default).
        #[arg(long)]
        force: bool,
    },
    /// Refresh managed scaffold files (3-way merge; never clobbers).
    Update {
        /// Write the report plus unified diffs of applied changes to a file.
        #[arg(long, value_name = "FILE")]
        diff: Option<PathBuf>,
        /// Replace user-modified managed files instead of merging.
        #[arg(long)]
        force: bool,
    },
    /// Claude-layer hooks, wired by the settings presets (charter §3.3).
    Hook(cmd::hook::HookArgs),
    /// Git client hook target — the .git/hooks shims exec this.
    GitHook(cmd::git_hook::GitHookArgs),
    /// Print the session-start digest (pointers, not content).
    Orient,
    /// Run the test gate (configured targets or runtime stack detection).
    Test(cmd::test::TestArgs),
    /// Validate record frontmatter; --docs adds the doc-graph integrity lint.
    Validate(cmd::validate::ValidateArgs),
    /// Verify a commit range + branch name against policy — the portable,
    /// binary-sourced CI check (auto-detects the CI platform's range).
    Ci(cmd::ci::CiArgs),
    /// Generated status view: branch, worktrees, in-flight work, capabilities.
    Status(cmd::status::StatusArgs),
    /// Land a branch into a target: flock(rebase -> test -> ff-merge).
    Integrate(cmd::integrate::IntegrateArgs),
    /// Health checks: hooks, Claude, Codex, config, permissions, network,
    /// delegates, repo integrity, CI perimeter, managed drift, customization,
    /// and test config — `doctor --list` names them all.
    Doctor(cmd::doctor::DoctorArgs),
    /// Inspect .codeflow/policy.json: `explain` the full key schema from the
    /// binary; `show` the effective values, their source, and invalid keys.
    Policy(cmd::policy::PolicyArgs),
    /// Search project memory: ledger, session summaries, ADRs, epics, capabilities.
    Recall(cmd::recall::RecallArgs),
    /// Remote provider operations (branch protection).
    Remote(cmd::remote::RemoteArgs),
    /// Create an epic: allocate the next EPC-NNN and scaffold it from the template.
    Epic(cmd::new::EpicArgs),
    /// Create a task under an epic: allocate the next TSK-NNN-MMM and scaffold it.
    Task(cmd::new::TaskArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    // Version-skew check on every invocation (charter §10): cheap, one line.
    // Update itself is the cure, so it skips the nag.
    if !matches!(cli.command, Command::Update { .. }) {
        if let Some(warning) = scaffold::version_skew_warning(&cwd, BINARY_VERSION) {
            eprintln!("{warning}");
        }
    }

    // Cross-repo registry upkeep is a side effect of every command (charter §7).
    cmd::touch_registry_best_effort();

    let assets = embedded::EmbeddedAssets;
    match cli.command {
        Command::Init {
            minimal,
            standard,
            full,
            yes,
            force,
        } => {
            let tier = if minimal {
                Some(scaffold::Tier::Minimal)
            } else if full {
                Some(scaffold::Tier::Full)
            } else if standard {
                Some(scaffold::Tier::Standard)
            } else {
                None
            };
            let answers = if yes {
                scaffold::InitAnswers::default()
            } else {
                prompts::gather_answers(&cwd)?
            };
            let options = scaffold::InitOptions {
                tier,
                force,
                binary_version: BINARY_VERSION.to_string(),
                answers,
            };
            let report = scaffold::init(&assets, &cwd, &options)?;
            print!("{report}");
        }
        Command::Update { diff, force } => {
            let options = scaffold::UpdateOptions {
                force,
                binary_version: BINARY_VERSION.to_string(),
                diff_out: diff,
            };
            let report = scaffold::update(&assets, &cwd, &options)?;
            print!("{report}");
            if report.has_conflicts() {
                std::process::exit(2);
            }
        }
        Command::Hook(args) => std::process::exit(cmd::hook::run(&args)),
        Command::GitHook(args) => std::process::exit(cmd::git_hook::run(&args)),
        Command::Orient => std::process::exit(cmd::orient::run()),
        Command::Test(args) => std::process::exit(cmd::test::run(&args)),
        Command::Validate(args) => std::process::exit(cmd::validate::run(&args)),
        Command::Ci(args) => std::process::exit(cmd::ci::run(&args)),
        Command::Status(args) => std::process::exit(cmd::status::run(&args)),
        Command::Integrate(args) => std::process::exit(cmd::integrate::run(&args)),
        Command::Doctor(args) => std::process::exit(cmd::doctor::run(&args)),
        Command::Policy(args) => std::process::exit(cmd::policy::run(&args)),
        Command::Recall(args) => cmd::recall::run(&args)?,
        Command::Remote(args) => cmd::remote::run(&args)?,
        Command::Epic(args) => std::process::exit(cmd::new::run_epic(&args)),
        Command::Task(args) => std::process::exit(cmd::new::run_task(&args)),
    }
    Ok(())
}
