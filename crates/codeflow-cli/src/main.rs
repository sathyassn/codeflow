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
    /// Transport-neutral lifecycle for interactive delegate turns.
    Delegate(cmd::delegate::DelegateArgs),
    #[command(about = git_hook_help())]
    GitHook(cmd::git_hook::GitHookArgs),
    /// Print the session-start digest (pointers, not content).
    Orient,
    /// Run the test gate (configured targets or runtime stack detection).
    Test(cmd::test::TestArgs),
    /// Adopt or reconcile the opt-in documentation portal (utility craft over durable docs).
    Portal(cmd::portal::PortalArgs),
    /// Validate record frontmatter; --docs adds the doc-graph integrity lint.
    Validate(cmd::validate::ValidateArgs),
    /// Verify a commit range + branch name against policy — the portable,
    /// binary-sourced CI check (auto-detects the CI platform's range).
    Ci(cmd::ci::CiArgs),
    /// Generated status view: branch, worktrees, in-flight work, capabilities.
    Status(cmd::status::StatusArgs),
    /// Land a branch into a target: flock(rebase -> test -> ff-merge).
    Integrate(cmd::integrate::IntegrateArgs),
    #[command(about = doctor_help())]
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
    /// Create a spec: allocate the next SPC-NNN and link its consuming work item.
    Spec(cmd::new::SpecArgs),
    /// Create an epic-linked or reasoned standalone task.
    Task(cmd::new::TaskArgs),
    /// Durable-work lifecycle checks.
    Work(cmd::work::WorkArgs),
    /// Check explicit forecast allocations and pinned evidence without writes.
    Estimate(cmd::estimate::EstimateArgs),
    /// Review this session on the utility presentation surface (catalog JSON, Comment).
    Present(cmd::present::PresentArgs),
}

/// Render the `doctor` about-line from a list of check names. Split out from
/// [`doctor_help`] so the derivation itself is testable against a fixture
/// list: a hand-maintained help string drifts from the registry, this cannot.
fn doctor_help_for(names: &[&str]) -> String {
    format!("Health checks: {}. See `doctor --list`", names.join(", "))
}

/// `doctor`'s about-line, derived from the check registry — never hand-listed.
fn doctor_help() -> &'static str {
    static HELP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HELP.get_or_init(|| doctor_help_for(&codeflow_core::doctor::check_names()))
}

/// `git-hook`'s about-line, derived from the same path constant the install
/// code writes to, so the help can never name a path nothing installs at.
fn git_hook_help() -> &'static str {
    static HELP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HELP.get_or_init(|| {
        format!(
            "Git client hook target — the {}/ shims exec this",
            scaffold::detect::CODEFLOW_HOOKS_PATH
        )
    })
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    // Update itself is the cure, so it skips the nag. Forecast checking also
    // skips this unrelated, unbounded project-state read to remain isolated.
    if !matches!(cli.command, Command::Update { .. } | Command::Estimate(_)) {
        if let Some(warning) = scaffold::version_skew_warning(&cwd, BINARY_VERSION) {
            eprintln!("{warning}");
        }
    }

    // Forecast checking is strictly read-only, including the user registry.
    // Other commands retain their existing best-effort upkeep (charter §7).
    if !matches!(cli.command, Command::Estimate(_)) {
        cmd::touch_registry_best_effort();
    }

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
            let portal_report = scaffold::portal::update_adopted_portal(&assets, &cwd)?;
            if let Some(portal_report) = &portal_report {
                print!("{portal_report}");
            }
            if report.has_conflicts() || portal_report.is_some_and(|report| report.has_conflicts())
            {
                std::process::exit(2);
            }
        }
        Command::Hook(args) => std::process::exit(cmd::hook::run(&args)),
        Command::Delegate(args) => std::process::exit(cmd::delegate::run(&args)),
        Command::GitHook(args) => std::process::exit(cmd::git_hook::run(&args)),
        Command::Orient => std::process::exit(cmd::orient::run()),
        Command::Test(args) => std::process::exit(cmd::test::run(&args)),
        Command::Portal(args) => std::process::exit(cmd::portal::run(&args, &assets)),
        Command::Validate(args) => std::process::exit(cmd::validate::run(&args)),
        Command::Ci(args) => std::process::exit(cmd::ci::run(&args)),
        Command::Status(args) => std::process::exit(cmd::status::run(&args)),
        Command::Integrate(args) => std::process::exit(cmd::integrate::run(&args)),
        Command::Doctor(args) => std::process::exit(cmd::doctor::run(&args)),
        Command::Policy(args) => std::process::exit(cmd::policy::run(&args)),
        Command::Recall(args) => cmd::recall::run(&args)?,
        Command::Remote(args) => cmd::remote::run(&args)?,
        Command::Epic(args) => std::process::exit(cmd::new::run_epic(&args)),
        Command::Spec(args) => std::process::exit(cmd::new::run_spec(&args)),
        Command::Task(args) => std::process::exit(cmd::new::run_task(&args)),
        Command::Work(args) => std::process::exit(cmd::work::run(&args)),
        Command::Estimate(args) => std::process::exit(cmd::estimate::run(&args)),
        Command::Present(args) => std::process::exit(cmd::present::run(&args)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DEFECT 5: the `doctor` about-line is derived, not hand-maintained —
    /// a different registry produces different help, with every name present.
    #[test]
    fn doctor_help_is_derived_from_the_check_names_it_is_given() {
        let fixture = doctor_help_for(&["alpha", "beta"]);
        assert_eq!(fixture, "Health checks: alpha, beta. See `doctor --list`");

        // Adding a fixture check changes the help.
        let extended = doctor_help_for(&["alpha", "beta", "gamma"]);
        assert_ne!(fixture, extended);
        assert!(extended.contains("gamma"));

        // The shipped help names exactly the registry's checks.
        let registered = codeflow_core::doctor::check_names();
        assert_eq!(doctor_help(), doctor_help_for(&registered));
        for name in registered {
            assert!(
                doctor_help().contains(name),
                "doctor help omits registered check {name}"
            );
        }
    }

    /// DEFECT 9: the `git-hook` about-line names the path the install code
    /// actually writes the shims to.
    #[test]
    fn git_hook_help_names_the_installed_shim_path() {
        let installed = scaffold::detect::CODEFLOW_HOOKS_PATH;
        assert!(
            git_hook_help().contains(installed),
            "git-hook help does not name {installed}"
        );
        assert!(
            !git_hook_help().contains(".git/hooks"),
            "git-hook help still names the path codeflow does not install to"
        );
    }
}
