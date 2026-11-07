//! `codeflow remote protect [--provider github] [--dry-run] [--profile
//! <name>] [--check-app <id>] [--lines <pattern>] [--branch <name>]
//! [--adopt <ruleset>]` — write the rulesets this command owns to the
//! remote (charter §6.1 plane 3, TSK-1080, TSK-383).

use anyhow::Context;
use clap::{Args, Subcommand};
use codeflow_core::hooks::policy_schema;
use codeflow_core::registry;
use codeflow_core::remote::{
    provider_for, Adopt, AuthorityPlan, Profile, ProtectionPlan, Scope, DEFAULT_LINES,
};

/// Arguments for `codeflow remote`.
#[derive(Debug, Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub command: RemoteCommand,
}

/// Remote subcommands.
#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Write the rulesets codeflow owns: "codeflow authority updates" and
    /// "codeflow authority checks" on the default branch, "codeflow line
    /// checks" on the integration lines, and `codeflow protect <pattern>`
    /// for each other protected branch in policy.json.
    ///
    /// Only rulesets with these names, and one `--adopt` names, are
    /// written; every other ruleset is left as it is. An update never
    /// lowers an approval count, removes a rule, changes which app produces
    /// a check or widens a bypass list.
    /// The authority and line checks are written with an empty bypass
    /// list, which closes a recovery window. To reduce a rule, edit it on
    /// the host as an administrator; there is no command for it. The checks
    /// are refused until the default branch's policy workflow declares the
    /// names they require.
    Protect {
        /// Remote provider (only `github` has an adapter; others print the
        /// manual checklist).
        #[arg(long, default_value = "github")]
        provider: String,

        /// Read the host and print the exact requests a run would send,
        /// writing nothing.
        #[arg(long)]
        dry_run: bool,

        /// Who reviews and who moves the default branch:
        /// solo-administrator (no approval; only the administrator role
        /// updates the default branch) or independent-review (one
        /// approval, stale approvals dismissed, the last push approved by
        /// someone else; only the maintain role updates it).
        #[arg(long, default_value = "solo-administrator", value_parser = Profile::NAMES.to_vec())]
        profile: String,

        /// The id of the GitHub App that creates the authority: checks
        /// (Mode A). Without it, the trusted: checks are pinned to GitHub
        /// Actions (Mode B).
        #[arg(long, value_name = "APP_ID")]
        check_app: Option<u64>,

        /// The integration-line branch pattern "codeflow line checks"
        /// targets.
        #[arg(long, value_name = "PATTERN", default_value = DEFAULT_LINES)]
        lines: String,

        /// Limit the run to the owned rulesets that target this branch. A
        /// literal policy branch the host does not have is reported and
        /// never planned, with or without this flag.
        #[arg(long, value_name = "NAME")]
        branch: Option<String>,

        /// Bring an existing ruleset, by id or exact name, under the owned
        /// name of the ruleset that targets exactly the same branches; add
        /// `=<owned name>` where two owned rulesets target them. It is
        /// renamed and only gains rules. Repeat for more than one.
        #[arg(long, value_name = "RULESET")]
        adopt: Vec<String>,
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
    let RemoteCommand::Protect {
        provider,
        dry_run,
        profile,
        check_app,
        lines,
        branch,
        adopt,
    } = &args.command;
    // The flags are judged before any file or host is read.
    let authority = AuthorityPlan::new(
        Profile::parse(profile).map_err(anyhow::Error::msg)?,
        *check_app,
        lines,
    )
    .map_err(|error| anyhow::anyhow!("remote protect: {error}"))?;
    let adopt = adopt
        .iter()
        .map(|text| Adopt::parse(text))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| anyhow::anyhow!("remote protect: {error}"))?;
    if branch
        .as_deref()
        .is_some_and(|b| b.trim().is_empty() || b.starts_with("refs/"))
    {
        anyhow::bail!(
            "remote protect: --branch must name one branch below refs/heads/, such as main"
        );
    }
    let scope = Scope {
        branch: branch.clone(),
        adopt,
    };

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
                eprintln!("remote protect: {error}", error = crate::cmd::shown(&error));
            }
            for error in schema.err().unwrap_or_default() {
                eprintln!(
                    "remote protect: policy error: {error}",
                    error = crate::cmd::shown(&error)
                );
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
            codeflow_core::git::display_path(&policy_path)
        );
    }
    // The registry's data profile (SPC-013 R-6, R-22) joins the plan where
    // durable work is tracked; the rules of every other branch are unchanged.
    let tracked = codeflow_core::workgraph::durable_work_tracking_enabled(&root)?;
    if tracked {
        plan = plan.with_registry_profile();
    }
    plan = plan.with_authority(authority).with_scope(scope);

    // The dry run reads the host, never writes it, and prints the exact
    // requests a run would send; without `gh` it prints the intended rules
    // and says it read no host.
    let report = if *dry_run {
        match provider_for(provider, &root) {
            Ok(adapter) => adapter.preview(&plan),
            Err(why) => {
                let mut report = plan.dry_run_report(provider);
                report.limitations.push(format!(
                    "the host was not read ({why}), so these are the intended rules, not the exact requests; --branch, --adopt and absent branches are judged only against the host"
                ));
                report
            }
        }
    } else {
        let adapter = provider_for(provider, &root).map_err(|e| anyhow::anyhow!(e))?;
        adapter.apply(&plan)
    };

    print_block!(report.render());
    let registry = codeflow_core::ids::REGISTRY_BRANCH;
    if tracked
        && !*dry_run
        && report
            .lines
            .iter()
            .any(|line| line.contains(&format!("'codeflow protect {registry}'")))
    {
        let git = codeflow_core::ids::Git::new(&root);
        let today = codeflow_core::ids::today();
        if let Err(error) = codeflow_core::ids::state::update(&git, |state| {
            state
                .data_profile
                .insert(codeflow_core::ids::AUTHORITY.to_string(), today);
        }) {
            eprintln!(
                "warning: could not record the applied data profile: {error}",
                error = crate::cmd::shown(&error)
            );
        }
    }
    Ok(())
}
