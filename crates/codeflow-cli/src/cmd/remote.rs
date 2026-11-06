//! `codeflow remote protect [--provider github] [--dry-run] [--profile
//! <name>] [--check-app <id>] [--lines <pattern>]` — write the rulesets this
//! command owns to the remote (charter §6.1 plane 3, TSK-1080).
//! `codeflow remote setup-apps` creates the authority and agent GitHub Apps,
//! `codeflow remote agent-token` mints the agent App's token or prints its
//! committer values, and `codeflow remote import-agent-key` stores another
//! computer's key for that App (TSK-1348).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use clap::{Args, Subcommand};
use codeflow_core::hooks::policy_schema;
use codeflow_core::registry;
use codeflow_core::remote::agent_key::{self, os_store, CurlHttp, OpensslSigner};
use codeflow_core::remote::apps::{
    self, LoopbackBrowser, ManifestBrowser, PasteBrowser, SetupOptions,
};
use codeflow_core::remote::{
    gh_transport, provider_for, AuthorityPlan, Profile, ProtectionPlan, DEFAULT_LINES,
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
    /// Only rulesets with these names are written; every other ruleset is
    /// left as it is. An update never lowers an approval count, removes a
    /// rule, changes which app produces a check or widens a bypass list.
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

        /// Print the intended rules without applying anything.
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
        /// Actions (Mode B), a lower tier that does not close forgery by a
        /// later producer; only Mode A, the App authority, closes it.
        #[arg(long, value_name = "APP_ID")]
        check_app: Option<u64>,

        /// The integration-line branch pattern "codeflow line checks"
        /// targets.
        #[arg(long, value_name = "PATTERN", default_value = DEFAULT_LINES)]
        lines: String,
    },

    /// Create the two GitHub Apps of `CodeFlow` for this repository: the
    /// authority App (Checks write only), with the environment
    /// `codeflow-authority` restricted to the default branch holding its key
    /// secret `CODEFLOW_AUTHORITY_APP_KEY` and id variable
    /// `CODEFLOW_AUTHORITY_APP_ID`,
    /// and the agent App (Contents and Pull requests write only), whose key
    /// is kept in this computer's credential store.
    ///
    /// Run it as a repository administrator under your own `gh auth login`.
    /// It refuses a GitHub App token and a token without Administration
    /// write before it creates anything, and changes no git identity,
    /// commit template or trailer setting. Each App takes two clicks:
    /// "Create GitHub App" on GitHub, then "Install" on the printed link.
    /// Every write is listed with what undoes it. A second run creates
    /// nothing it finds: it reads the agent App with its own key, and
    /// reports a private authority App as present but not verified. The run
    /// prints the committer lines for each harness's project settings and
    /// edits no settings file.
    SetupApps {
        /// The authority App's name (default `codeflow-authority-<repo>`);
        /// GitHub App names are unique across GitHub, at most 34 characters.
        #[arg(long, value_name = "NAME")]
        authority_name: Option<String>,

        /// The agent App's name (default `codeflow-agent-<repo>`).
        #[arg(long, value_name = "NAME")]
        agent_name: Option<String>,

        /// Set up the authority App only.
        #[arg(long)]
        no_agent: bool,

        /// Write each App's page to a file and read back the address GitHub
        /// opens, instead of serving the page on a local port.
        #[arg(long)]
        paste: bool,
    },

    /// Print a GitHub installation token of this repository's agent App,
    /// valid for one hour, for `GH_TOKEN` or `git push`. The App's key is
    /// read from this computer's credential store and never printed.
    AgentToken {
        /// The repository, `owner/name` (default: the `origin` remote).
        #[arg(long, value_name = "OWNER/NAME")]
        repo: Option<String>,

        /// Answer as a git credential helper: `get` for `https://github.com`
        /// prints the token as the password of `x-access-token`; another
        /// host or operation prints nothing.
        #[arg(long, value_name = "OPERATION")]
        git_credential: Option<String>,

        /// Print the App's committer name or email instead of a token,
        /// confirmed with GitHub; nothing reaches standard output when it
        /// cannot be confirmed. No token is minted.
        #[arg(long, value_name = "FIELD", value_parser = ["name", "email"], conflicts_with = "git_credential")]
        committer: Option<String>,
    },

    /// Store this computer's own key for the repository's agent App, from a
    /// private key file generated on the App's settings page. Needs no
    /// administrator role and no `gh` login.
    ///
    /// The key is proved first: GitHub must answer as the App, show it as
    /// the repository's agent App, and show it installed there. A file
    /// others can read and an existing entry are refused. The file is left
    /// in place for you to delete.
    ImportAgentKey {
        /// The private key file; it must be readable only by you.
        #[arg(long, value_name = "PATH")]
        key_file: PathBuf,

        /// The App's Client ID, from its settings page.
        #[arg(long, value_name = "ID")]
        client_id: String,

        /// The repository, `owner/name` (default: the `origin` remote).
        #[arg(long, value_name = "OWNER/NAME")]
        repo: Option<String>,
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
    match &args.command {
        RemoteCommand::Protect {
            provider,
            dry_run,
            profile,
            check_app,
            lines,
        } => run_protect(provider, *dry_run, profile, *check_app, lines),
        RemoteCommand::SetupApps {
            authority_name,
            agent_name,
            no_agent,
            paste,
        } => run_setup_apps(
            &SetupOptions {
                authority_name: authority_name.clone(),
                agent_name: agent_name.clone(),
                agent: !no_agent,
            },
            *paste,
        ),
        RemoteCommand::AgentToken {
            repo,
            git_credential,
            committer,
        } => run_agent_token(
            repo.as_deref(),
            git_credential.as_deref(),
            committer.as_deref(),
        ),
        RemoteCommand::ImportAgentKey {
            key_file,
            client_id,
            repo,
        } => run_import_agent_key(key_file, client_id, repo.as_deref()),
    }
}

fn repo_root() -> anyhow::Result<std::path::PathBuf> {
    let cwd = std::env::current_dir().context("cannot resolve current directory")?;
    Ok(registry::find_repo_root(&cwd)
        .context("cannot locate repository; repair .codeflow paths first")?
        .unwrap_or(cwd))
}

/// Seconds since 1970, for an App's JSON Web Token.
fn unix_now() -> anyhow::Result<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("the system clock is before 1970")?
        .as_secs())
}

/// `codeflow remote setup-apps`.
fn run_setup_apps(options: &SetupOptions, paste: bool) -> anyhow::Result<()> {
    let root = repo_root()?;
    let gh = gh_transport(&root).map_err(anyhow::Error::msg)?;
    let announce: Box<dyn Fn(&str)> = Box::new(|line| {
        eprintln!("{}", codeflow_core::git::display_print(line));
    });
    let mut browser: Box<dyn ManifestBrowser> = if paste {
        let read = Box::new(|| {
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .map_err(|error| format!("cannot read the address: {error}"))?;
            Ok(line)
        });
        Box::new(
            PasteBrowser::new(&std::env::temp_dir(), announce, read).map_err(anyhow::Error::msg)?,
        )
    } else {
        Box::new(
            LoopbackBrowser::bind(Duration::from_secs(60 * 60), announce)
                .map_err(anyhow::Error::msg)?,
        )
    };
    let store = os_store();
    // Each new App is asked, as itself, where its webhook points.
    let signer = OpensslSigner::discover(&std::env::temp_dir()).map_err(anyhow::Error::msg)?;
    let http = CurlHttp::discover().map_err(anyhow::Error::msg)?;
    let auth = apps::AppAuth {
        signer: &signer,
        http: &http,
        now: unix_now()?,
    };
    let report = apps::setup(&*gh, browser.as_mut(), store.as_ref(), &auth, options);
    print_block!(report.render());
    if let Some(failure) = report.failure {
        anyhow::bail!("remote setup-apps stopped: {failure}");
    }
    Ok(())
}

/// The repository named by `--repo`, or the `origin` remote.
fn resolve_repository(repo: Option<&str>) -> anyhow::Result<String> {
    let repository = match repo {
        Some(repo) => repo.to_string(),
        None => codeflow_core::remote::github_origin(&repo_root()?)
            .map_err(anyhow::Error::msg)?
            .context(
                "the origin remote is not a github.com repository; name it with --repo owner/name",
            )?,
    };
    if !apps::valid_repository(&repository) {
        anyhow::bail!("'{repository}' is not owner/name");
    }
    Ok(repository)
}

/// `codeflow remote import-agent-key`.
fn run_import_agent_key(
    key_file: &Path,
    client_id: &str,
    repo: Option<&str>,
) -> anyhow::Result<()> {
    let repository = resolve_repository(repo)?;
    let signer = OpensslSigner::discover(&std::env::temp_dir()).map_err(anyhow::Error::msg)?;
    let http = CurlHttp::discover().map_err(anyhow::Error::msg)?;
    let store = os_store();
    let import = agent_key::KeyImport {
        repository: &repository,
        client_id,
        key_file,
    };
    let lines = agent_key::import_agent_key(store.as_ref(), &signer, &http, &import, unix_now()?)
        .map_err(|error| anyhow::anyhow!("remote import-agent-key: {error}"))?;
    for line in lines {
        println!("{}", codeflow_core::git::display_print(&line));
    }
    Ok(())
}

/// `codeflow remote agent-token`.
fn run_agent_token(
    repo: Option<&str>,
    git_credential: Option<&str>,
    committer: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(operation) = git_credential {
        // Git sends the request on standard input; only `get` for
        // https://github.com is answered, and nothing is minted otherwise.
        let mut request = String::new();
        let _ = std::io::stdin().read_to_string(&mut request);
        if operation != "get" || !agent_key::credential_request_is_github(&request) {
            return Ok(());
        }
    }
    let repository = resolve_repository(repo)?;
    let signer = OpensslSigner::discover(&std::env::temp_dir()).map_err(anyhow::Error::msg)?;
    let http = CurlHttp::discover().map_err(anyhow::Error::msg)?;
    let store = os_store();
    if let Some(field) = committer {
        // Computed first: nothing reaches standard output on a failure.
        let committer =
            agent_key::bot_committer(store.as_ref(), &signer, &http, &repository, unix_now()?)
                .map_err(|error| anyhow::anyhow!("remote agent-token: {error}"))?;
        let value = if field == "email" {
            committer.email
        } else {
            committer.name
        };
        writeln!(std::io::stdout().lock(), "{value}")?;
        return Ok(());
    }
    let minted = agent_key::mint(store.as_ref(), &signer, &http, &repository, unix_now()?)
        .map_err(|error| anyhow::anyhow!("remote agent-token: {error}"))?;
    let mut out = std::io::stdout().lock();
    if git_credential.is_some() {
        out.write_all(agent_key::git_credential_answer(&minted.token).as_bytes())?;
    } else {
        writeln!(out, "{}", minted.token)?;
        eprintln!(
            "agent App token for {repository}, expires {}",
            codeflow_core::git::display_print(&minted.expires_at)
        );
    }
    Ok(())
}

/// `codeflow remote protect`.
fn run_protect(
    provider: &str,
    dry_run: bool,
    profile: &str,
    check_app: Option<u64>,
    lines: &str,
) -> anyhow::Result<()> {
    // The flags are judged before any file or host is read.
    let authority = AuthorityPlan::new(
        Profile::parse(profile).map_err(anyhow::Error::msg)?,
        check_app,
        lines,
    )
    .map_err(|error| anyhow::anyhow!("remote protect: {error}"))?;

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
    plan = plan.with_authority(authority);

    let report = if dry_run {
        plan.dry_run_report(provider)
    } else {
        let adapter = provider_for(provider, &root).map_err(|e| anyhow::anyhow!(e))?;
        adapter.apply(&plan)
    };

    print_block!(report.render());
    let registry = codeflow_core::ids::REGISTRY_BRANCH;
    if tracked
        && !dry_run
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
