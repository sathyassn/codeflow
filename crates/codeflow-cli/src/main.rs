//! codeflow — the AI-development discipline layer CLI.

mod cmd;
#[cfg(test)]
mod command_reference;
mod embedded;
mod prompts;

use std::path::PathBuf;

use clap::{ArgGroup, Parser, Subcommand};
use codeflow_core::root_checkout;
use codeflow_core::scaffold;

const BINARY_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "codeflow", version = concat!(env!("CARGO_PKG_VERSION"), " source=", env!("CODEFLOW_SOURCE_REVISION"), " dirty=", env!("CODEFLOW_SOURCE_DIRTY"), " inputs=", env!("CODEFLOW_HOOK_INPUT_DIGEST")), about = "AI-development discipline layer")]
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
        /// Set up an umbrella workspace: put the root checkout on its root branch
        /// (created from the default branch if missing), write `git.root_branch`,
        /// and ignore every nested git repository. Refuses over uncommitted changes.
        #[arg(long)]
        workspace: bool,
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
    /// Resolve catalog duties without launching models.
    Models(cmd::models::ModelsArgs),
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
    /// Create an ADR: number the next ADR-NNNN and write it as proposed.
    Adr(cmd::new::AdrArgs),
    /// The shared id registry: seed, backfill, sync, admit, retarget, restore, check.
    Ids(cmd::ids::IdsArgs),
    /// Check forecast allocations and compare outcomes derived from git, without writes.
    Estimate(cmd::estimate::EstimateArgs),
    /// Review this session on the utility presentation surface (catalog JSON, Comment).
    Present(cmd::present::PresentArgs),
    /// Read-only reports: `ceremony`, the process cost over merged pull requests.
    Report(cmd::report::ReportArgs),
}

/// Whether a command records its repository in the user registry. Hook
/// entry points, `ci` and the read-only checks (`validate`, `work`,
/// `estimate`, `report`) do not; `orient` and `status` do, since the session-start
/// orient is the main sign that a repository is in use.
fn touches_registry(command: &Command) -> bool {
    !matches!(
        command,
        Command::Hook(_)
            | Command::GitHook(_)
            | Command::Ci(_)
            | Command::Validate(_)
            | Command::Work(_)
            | Command::Estimate(_)
            | Command::Models(_)
            | Command::Report(_)
    )
}

/// Ask and record the decision for a kept PR template (SPC-013 R-84)
/// before the report says setup is done. End of input leaves it diagnosed.
fn decide_pr_template(root: &std::path::Path, report: &mut scaffold::Report) -> anyhow::Result<()> {
    let Some(kept) = report.pending_pr_template.clone() else {
        return Ok(());
    };
    if let Some(decision) = prompts::decide_pr_template(&mut std::io::stdin().lock(), &kept.path)? {
        let line = scaffold::pr_template::record_decision(
            root,
            &kept,
            decision,
            &scaffold::pr_template::today(),
        )?;
        report.notes.push(line);
        report.pending_pr_template = None;
    }
    Ok(())
}

/// Plain `init` and `update` switch nothing in a folder that holds nested
/// repositories; they name `codeflow init --workspace` instead.
fn print_workspace_hint(root: &std::path::Path) {
    let policy = codeflow_core::hooks::policy::Policy::load(root).git;
    if let Some(hint) = root_checkout::workspace_hint(root, &policy) {
        println!("{hint}");
    }
}

/// `init --workspace` switches the root checkout before anything is
/// written, so a refusal over uncommitted changes leaves the folder untouched.
fn prepare_workspace(
    root: &std::path::Path,
    workspace: bool,
) -> anyhow::Result<Option<root_checkout::BranchStep>> {
    if !workspace {
        return Ok(None);
    }
    let policy = codeflow_core::hooks::policy::Policy::load(root).git;
    Ok(Some(root_checkout::prepare_branch(root, &policy)?))
}

/// After `init`: what `--workspace` set up, or the hint for plain `init`.
fn report_workspace(
    root: &std::path::Path,
    step: Option<root_checkout::BranchStep>,
) -> anyhow::Result<()> {
    match step {
        Some(step) => println!("{}", root_checkout::finish(root, step)?),
        None => print_workspace_hint(root),
    }
    Ok(())
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

/// The tier `init` was asked for; none lets the scaffold choose.
fn selected_tier(minimal: bool, standard: bool, full: bool) -> Option<scaffold::Tier> {
    if minimal {
        Some(scaffold::Tier::Minimal)
    } else if full {
        Some(scaffold::Tier::Full)
    } else if standard {
        Some(scaffold::Tier::Standard)
    } else {
        None
    }
}

fn main() -> anyhow::Result<()> {
    // A hook git fires during this command runs this binary (SPC-013 R-85):
    // the path goes only into git children's environment, so an inherited
    // value is dropped here and no other child ever sees one.
    std::env::remove_var(codeflow_core::git::HOOK_BINARY_ENV);
    if let Ok(binary) = std::env::current_exe() {
        codeflow_core::git::designate_calling_binary(binary);
    }
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    // Update itself is the cure, so it skips the nag. Read-only resolvers
    // skip unrelated project-state reads to remain isolated.
    if !matches!(
        cli.command,
        Command::Update { .. } | Command::Estimate(_) | Command::Models(_)
    ) {
        if let Some(warning) = scaffold::version_skew_warning(&cwd, BINARY_VERSION) {
            eprintln!("{warning}");
        }
    }

    // Hooks, CI and read-only commands leave the user registry alone: they
    // run in sandboxes and CI, and forecast checking and model resolution are
    // strictly read-only.
    // Other commands keep their best-effort upkeep (charter §7).
    if touches_registry(&cli.command) {
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
            workspace,
        } => {
            let workspace_step = prepare_workspace(&cwd, workspace)?;
            let tier = selected_tier(minimal, standard, full);
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
            let mut report = scaffold::init(&assets, &cwd, &options)?;
            if !yes {
                decide_pr_template(&cwd, &mut report)?;
            }
            print!("{report}");
            cmd::present::provision_state_root_or_warn();
            report_workspace(&cwd, workspace_step)?;
        }
        Command::Update { diff, force } => {
            let options = scaffold::UpdateOptions {
                force,
                binary_version: BINARY_VERSION.to_string(),
                diff_out: diff,
            };
            let mut report = scaffold::update(&assets, &cwd, &options)?;
            if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                decide_pr_template(&cwd, &mut report)?;
            }
            print!("{report}");
            cmd::present::provision_state_root_or_warn();
            print_workspace_hint(&cwd);
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
        Command::Models(args) => std::process::exit(cmd::models::run(&args)),
        Command::Doctor(args) => std::process::exit(cmd::doctor::run(&args)),
        Command::Policy(args) => std::process::exit(cmd::policy::run(&args)),
        Command::Recall(args) => cmd::recall::run(&args)?,
        Command::Remote(args) => cmd::remote::run(&args)?,
        Command::Epic(args) => std::process::exit(cmd::new::run_epic(&args)),
        Command::Spec(args) => std::process::exit(cmd::new::run_spec(&args)),
        Command::Task(args) => std::process::exit(cmd::new::run_task(&args)),
        Command::Work(args) => std::process::exit(cmd::work::run(&args)),
        Command::Adr(args) => std::process::exit(cmd::new::run_adr(&args)),
        Command::Ids(args) => std::process::exit(cmd::ids::run(&args)),
        Command::Estimate(args) => std::process::exit(cmd::estimate::run(&args)),
        Command::Present(args) => std::process::exit(cmd::present::run(&args)),
        Command::Report(args) => std::process::exit(cmd::report::run(&args)),
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
