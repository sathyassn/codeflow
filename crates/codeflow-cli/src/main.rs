//! codeflow — the AI-development discipline layer CLI.

// The output sink (TSK-388). These shadow `println!`, `print!`, `eprintln!`
// and `eprint!` for this crate: what one call prints goes through
// `codeflow_core::git::display_print`, which escapes each control, format
// and separator character other than a line break or tab, keeps
// backslashes, and indents each further line that does not already start
// indented. CodeFlow quotes branch names a remote chose, record titles,
// paths and policy values from a checkout, and errors that carry them;
// whatever a call interpolates, a line break in it cannot start a line at
// column 0. `term_write!` and `term_writeln!` apply the same rule to a
// terminal writer. Text that is several column-0 lines by design is a
// `codeflow_core::git::TextBlock`, built one `writeln!` per line under the
// same rule, and prints through `print_block!` or `println_block!`; JSON is
// a `TextBlock::json`. `codeflow-core` and `codeflow-present` shadow their
// own print macros the same way. Not covered here: the standard library's
// panic message, and clap's usage and error output.
macro_rules! println {
    () => {
        ::std::println!()
    };
    ($($arg:tt)*) => {
        ::std::println!("{}", ::codeflow_core::git::display_print(&::std::format!($($arg)*)))
    };
}
macro_rules! print {
    ($($arg:tt)*) => {
        ::std::print!("{}", ::codeflow_core::git::display_print(&::std::format!($($arg)*)))
    };
}
macro_rules! eprintln {
    () => {
        ::std::eprintln!()
    };
    ($($arg:tt)*) => {
        ::std::eprintln!("{}", ::codeflow_core::git::display_print(&::std::format!($($arg)*)))
    };
}
macro_rules! eprint {
    ($($arg:tt)*) => {
        ::std::eprint!("{}", ::codeflow_core::git::display_print(&::std::format!($($arg)*)))
    };
}

// A block prints as it stands: its lines were written under the sink rule.
macro_rules! print_block {
    ($block:expr) => {
        ::std::print!("{}", ::codeflow_core::git::TextBlock::as_str(&$block))
    };
}
macro_rules! println_block {
    ($block:expr) => {
        ::std::println!("{}", ::codeflow_core::git::TextBlock::as_str(&$block))
    };
}
// One write to a terminal writer under the sink rule.
macro_rules! term_write {
    ($out:expr, $($arg:tt)*) => {{
        // For a caller that has not imported it; harmless where it has.
        #[allow(unused_imports)]
        use ::std::io::Write as _;
        $out.write_all(
            ::codeflow_core::git::display_print(&::std::format!($($arg)*)).as_bytes(),
        )
    }};
}
macro_rules! term_writeln {
    ($out:expr) => {
        term_write!($out, "\n")
    };
    ($out:expr, $($arg:tt)*) => {
        term_write!($out, "{}\n", ::std::format!($($arg)*))
    };
}

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
        /// Write the report plus each applied change to a file. The export
        /// never shows a value, key name or line from the project's files or
        /// the recorded baseline: JSON, TOML and YAML list the changed key
        /// paths with the shipped value and a marker for the project's side;
        /// text shows the shipped lines written and counts the rest; any
        /// other format, or a file that does not parse, is withheld.
        #[arg(long, value_name = "FILE")]
        diff: Option<PathBuf>,
        /// Replace user-modified managed files instead of merging.
        #[arg(long)]
        force: bool,
        /// Only pin this codeflow release for CI: download its archives,
        /// check them against its sha256.sum and write `scaffold_version` and
        /// their digests (`[scaffold_sha256]`) to .codeflow/project.toml.
        /// Nothing else changes. The version must read as MAJOR.MINOR.PATCH
        /// with an optional -prerelease and +build; any other is refused
        /// before a download. Upgrade step one; land it before the update.
        #[arg(long, value_name = "VERSION", conflicts_with_all = ["diff", "force"])]
        pin: Option<String>,
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

    /// Run a direct heavy command under the shared gate lock.
    Reserve(cmd::reserve::ReserveArgs),
    /// Adopt or reconcile the opt-in documentation portal (utility craft over durable docs).
    Portal(cmd::portal::PortalArgs),
    /// Validate record frontmatter; --docs adds the doc-graph integrity lint.
    Validate(cmd::validate::ValidateArgs),
    /// Verify a commit range + branch name against policy — the portable,
    /// binary-sourced CI check (auto-detects the CI platform's range).
    Ci(cmd::ci::CiArgs),
    /// Generated status view: branch, worktrees, in-flight work, capabilities.
    Status(cmd::status::StatusArgs),
    /// Land a branch locally after gating a rebase or merge candidate.
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
    /// Operator feedback items: record (FB-NNN), move by the transition table, list.
    Feedback(cmd::feedback::FeedbackArgs),
    /// Follow-ups in the clone's coordination ledger (FU-NNN): add, check, close, list. The `CODEFLOW_COORD_*` role binding is cooperative ownership, not authentication.
    Follow(cmd::follow::FollowArgs),
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
            | Command::Follow(_)
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

/// `codeflow update`: refresh the managed files, then the adopted portal;
/// exits 2 when either leaves a conflict.
fn update(
    assets: &embedded::EmbeddedAssets,
    cwd: &std::path::Path,
    diff: Option<PathBuf>,
    force: bool,
) -> anyhow::Result<()> {
    let options = scaffold::UpdateOptions {
        force,
        binary_version: BINARY_VERSION.to_string(),
        diff_out: diff,
    };
    let mut report = scaffold::update(assets, cwd, &options)?;
    for file in &mut report.files {
        file.notes
            .retain(|note| codeflow_core::hooks::policy_schema::first_unsupported_warning(note));
    }
    if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        decide_pr_template(cwd, &mut report)?;
    }
    print_block!(report.block());
    cmd::present::provision_state_root_or_warn();
    print_workspace_hint(cwd);
    let portal_report = scaffold::portal::update_adopted_portal(assets, cwd)?;
    if let Some(portal_report) = &portal_report {
        print_block!(portal_report.block());
    }
    if report.has_conflicts() || portal_report.is_some_and(|report| report.has_conflicts()) {
        std::process::exit(2);
    }
    Ok(())
}

/// `codeflow update --pin <version>`: pin that release and its archive
/// digests for CI (sathyassn/codeflow#47). Returns the exit code.
fn pin_release(root: &std::path::Path, version: &str) -> i32 {
    let base = match scaffold::release_pin::release_url() {
        Ok(base) => base,
        Err(error) => {
            eprintln!(
                "codeflow update --pin: error: {error}",
                error = crate::cmd::shown(&error)
            );
            return 1;
        }
    };
    match scaffold::release_pin::pin_release(
        root,
        version,
        &base,
        &scaffold::release_pin::fetch_with_curl,
    ) {
        Ok(report) => {
            println!("{report}");
            0
        }
        Err(error) => {
            eprintln!(
                "codeflow update --pin: error: {error}",
                error = crate::cmd::shown(&error)
            );
            1
        }
    }
}

/// Plain `init` and `update` switch nothing in a folder that holds nested
/// repositories; they name `codeflow init --workspace` instead.
fn print_workspace_hint(root: &std::path::Path) {
    let policy = match codeflow_core::hooks::policy::Policy::load(root) {
        Ok(policy) => policy.git,
        Err(error) => {
            eprintln!(
                "codeflow: cannot read workspace policy: {error}",
                error = crate::cmd::shown(&error)
            );
            return;
        }
    };
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
    let policy = codeflow_core::hooks::policy::Policy::load(root)
        .map_err(anyhow::Error::msg)?
        .git;
    Ok(Some(root_checkout::prepare_branch(root, &policy)?))
}

/// After `init`: what `--workspace` set up, or the hint for plain `init`.
fn report_workspace(
    root: &std::path::Path,
    step: Option<root_checkout::BranchStep>,
) -> anyhow::Result<()> {
    match step {
        Some(step) => println_block!(root_checkout::finish(root, step)?.block()),
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

/// The CLI boundary (TSK-388): an error `run` returns is printed here, its
/// whole chain in the form Rust's own termination prints (`Error: ...`
/// with `Caused by:`), escaped through `cmd::shown`, and the process exits
/// with status 1 as before. A path or name in the chain cannot write a
/// terminal escape sequence or start a line of its own.
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {}", cmd::shown(&format!("{error:?}")));
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
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
            print_block!(report.block());
            cmd::present::provision_state_root_or_warn();
            report_workspace(&cwd, workspace_step)?;
        }
        Command::Update { diff, force, pin } => match pin {
            Some(version) => std::process::exit(pin_release(&cwd, &version)),
            None => update(&assets, &cwd, diff, force)?,
        },
        Command::Hook(args) => std::process::exit(cmd::hook::run(&args)),
        Command::Delegate(args) => std::process::exit(cmd::delegate::run(&args)),
        Command::GitHook(args) => std::process::exit(cmd::git_hook::run(&args)),
        Command::Orient => std::process::exit(cmd::orient::run()),
        Command::Test(args) => std::process::exit(cmd::test::run(&args)),
        Command::Reserve(args) => std::process::exit(cmd::reserve::run(&args)),
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
        Command::Feedback(args) => std::process::exit(cmd::feedback::run(&args)),
        Command::Follow(args) => std::process::exit(cmd::follow::run(&args)),
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
