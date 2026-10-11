//! `codeflow remote protect [--provider github] [--dry-run]` — apply the
//! `policy.json` protected-branch list to the remote (charter §6.1 plane 3).

use anyhow::Context;
use clap::{Args, Subcommand};
use codeflow_core::hooks::policy_schema;
use codeflow_core::registry;
use codeflow_core::remote::{provider_for, ProtectionPlan};

/// Arguments for `codeflow remote`.
#[derive(Debug, Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub command: RemoteCommand,
}

/// Remote subcommands.
#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Apply protected-branch policy to the remote provider.
    Protect {
        /// Remote provider (only `github` has an adapter; others print the
        /// manual checklist).
        #[arg(long, default_value = "github")]
        provider: String,

        /// Print the intended rules without applying anything.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Run `codeflow remote`.
///
/// Degradation is legible, not fatal: when rules cannot be applied (plan
/// limits, missing adapter) the precise limitation and a manual checklist
/// are printed and the command still exits 0 with status `degraded`.
///
/// # Errors
///
/// Returns an error for unreadable repository, policy or tracking state,
/// when the policy file fails the schema (before anything is planned or
/// sent), or for environment problems such as a missing gh outside dry-run.
pub fn run(args: &RemoteArgs) -> anyhow::Result<()> {
    let RemoteCommand::Protect { provider, dry_run } = &args.command;

    let cwd = std::env::current_dir().context("cannot resolve current directory")?;
    let root = registry::find_repo_root(&cwd)
        .context("cannot locate repository; repair .codeflow paths before applying protection")?
        .unwrap_or(cwd);
    let policy_path = root.join(".codeflow/policy.json");
    // The plan reads the protection fields and the schema judges the whole
    // file: a policy either one cannot read or refuses (a blank
    // `git.required_checks` entry included) is refused here, before any
    // request to the host, with every reason each gives.
    let read = ProtectionPlan::from_policy_file(&policy_path);
    let schema = policy_schema::validate_policy(&root);
    let mut plan = match (read, schema) {
        (Ok(plan), Ok(())) => plan,
        (read, schema) => {
            if let Err(error) = read {
                eprintln!("remote protect: {error}");
            }
            for error in schema.err().unwrap_or_default() {
                eprintln!("remote protect: policy error: {error}");
            }
            anyhow::bail!(
                "{} is invalid or cannot be read, so no rules were planned or applied (see `codeflow policy explain`)",
                policy_path.display()
            );
        }
    };
    // Only a policy proven missing gives the defaults; an unreadable one
    // was refused above.
    if ProtectionPlan::policy_absent(&policy_path).map_err(anyhow::Error::msg)? {
        eprintln!(
            "note: {} not found — using charter defaults (main, master)",
            policy_path.display()
        );
    }
    // The registry's data profile (SPC-013 R-6, R-22) joins the plan where
    // durable work is tracked; the rules of every other branch are unchanged.
    let tracked = codeflow_core::workgraph::durable_work_tracking_enabled(&root)?;
    if tracked {
        plan = plan.with_registry_profile();
    }

    let report = if *dry_run {
        plan.dry_run_report(provider)
    } else {
        let adapter = provider_for(provider, &root).map_err(|e| anyhow::anyhow!(e))?;
        adapter.apply(&plan)
    };

    print!("{}", report.render());
    let registry = codeflow_core::ids::REGISTRY_BRANCH;
    if tracked
        && !*dry_run
        && report
            .lines
            .iter()
            .any(|line| line.contains(&format!("for {registry}:")))
    {
        let git = codeflow_core::ids::Git::new(&root);
        let today = codeflow_core::ids::today();
        if let Err(error) = codeflow_core::ids::state::update(&git, |state| {
            state
                .data_profile
                .insert(codeflow_core::ids::AUTHORITY.to_string(), today);
        }) {
            eprintln!("warning: could not record the applied data profile: {error}");
        }
    }
    Ok(())
}
