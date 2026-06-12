//! `codeflow remote protect [--provider github] [--dry-run]` — apply the
//! `policy.json` protected-branch list to the remote (charter §6.1 plane 3).

use anyhow::Context;
use clap::{Args, Subcommand};
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
/// Returns an error only for environment problems (no repo, gh missing for
/// the github provider outside dry-run).
pub fn run(args: &RemoteArgs) -> anyhow::Result<()> {
    let RemoteCommand::Protect { provider, dry_run } = &args.command;

    let cwd = std::env::current_dir().context("cannot resolve current directory")?;
    let root = registry::find_repo_root(&cwd).unwrap_or(cwd);
    let policy_path = root.join(".codeflow/policy.json");
    if !policy_path.is_file() {
        eprintln!(
            "note: {} not found — using charter defaults (main, master)",
            policy_path.display()
        );
    }
    let plan = ProtectionPlan::from_policy_file(&policy_path);

    let report = if *dry_run {
        plan.dry_run_report(provider)
    } else {
        let adapter = provider_for(provider, &root).map_err(|e| anyhow::anyhow!(e))?;
        adapter.apply(&plan)
    };

    print!("{}", report.render());
    Ok(())
}
