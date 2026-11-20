//! `codeflow policy baseline <show|init|refresh>`: the operator-confirmed
//! copy of the landed policy the guards hold the authority ref to
//! (TSK-1057). `show` reads it anywhere; `init` and `refresh` write it only
//! at an operator's terminal outside every agent session.
use std::path::Path;

use clap::{Args, Subcommand};
use codeflow_core::hooks::policy_baseline::{self as baseline, Lookup, Probe, Store};

#[derive(Debug, Args)]
pub struct BaselineArgs {
    #[command(subcommand)]
    pub command: BaselineCommand,
}

#[derive(Debug, Subcommand)]
pub enum BaselineCommand {
    /// Print this repository's policy baseline and how it bears on the
    /// policy read.
    Show,
    /// Record the baseline from a fresh read of the authority remote. Without
    /// --confirm it prints the commit and digest and writes nothing.
    Init {
        /// The commit printed by a run without --confirm.
        #[arg(long, value_name = "COMMIT")]
        confirm: Option<String>,
    },
    /// Move the baseline to the authority's current commit: a descendant
    /// whose policy is no weaker, or anything with --reset.
    Refresh {
        /// The commit printed by a run without --confirm.
        #[arg(long, value_name = "COMMIT")]
        confirm: Option<String>,
        /// Accept a commit the refresh rule refuses; the reason is recorded.
        #[arg(long, requires = "reason")]
        reset: bool,
        /// Why the operator resets the baseline.
        #[arg(long, value_name = "TEXT")]
        reason: Option<String>,
    },
}

/// Run `codeflow policy baseline`; returns the process exit code.
#[must_use]
pub fn run(args: &BaselineArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => return fail(&error),
    };
    let store = match Store::account() {
        Ok(Some(store)) => store,
        Ok(None) => return fail("policy baselines are not read on this platform"),
        Err(error) => return fail(&error),
    };
    let (confirm, mode) = match &args.command {
        BaselineCommand::Show => return show(&root, &store),
        BaselineCommand::Init { confirm } => (confirm, Mode::Init),
        BaselineCommand::Refresh {
            confirm,
            reset,
            reason,
        } => (
            confirm,
            Mode::Refresh {
                reset: reset.then(|| reason.clone().unwrap_or_default()),
            },
        ),
    };
    if let Err(error) = live_check() {
        return fail(&error);
    }
    write_checked(&root, &store, confirm.as_deref(), &mode)
}

/// Which writer runs.
enum Mode {
    Init,
    /// A refresh; `reset` holds the operator's reason for an explicit reset.
    Refresh {
        reset: Option<String>,
    },
}

fn fail(error: &str) -> i32 {
    eprintln!("codeflow: {}", crate::cmd::shown(&error));
    2
}

/// The writer's session checks against this process.
fn live_check() -> Result<(), String> {
    use std::io::IsTerminal as _;
    let env = codeflow_core::root_checkout::process_env;
    baseline::require_operator_terminal(&Probe {
        env: &env,
        ancestors: &baseline::process_ancestors,
        stdin_is_terminal: std::io::stdin().is_terminal(),
    })
}

fn show(root: &Path, store: &Store) -> i32 {
    let common = match codeflow_core::hooks::repo::open(root)
        .and_then(|repo| repo.ok_or_else(|| "not a repository with a commit".to_string()))
        .and_then(|repo| baseline::common_dir(&repo))
    {
        Ok(common) => common,
        Err(error) => return fail(&error),
    };
    println!("store: {}", store.dir().display());
    match store.lookup(&common) {
        Lookup::Never => println!("baseline: none for this repository"),
        Lookup::Missing => println!("baseline: missing, though this repository was initialized"),
        Lookup::Malformed(why) => println!("baseline: unreadable: {}", crate::cmd::shown(&why)),
        Lookup::Found(record) => {
            println!("baseline: {} at {}", record.authority, record.commit);
            println!(
                "remote: {} {}",
                record.remote,
                codeflow_core::git::remote_query::display_url(&record.remote_url)
            );
            println!("policy digest: {}", record.policy_digest);
            println!("updated: {}", record.updated);
            for reset in &record.resets {
                println!(
                    "reset {}: {} to {}: {}",
                    reset.at,
                    reset.from_commit,
                    reset.to_commit,
                    crate::cmd::shown(&reset.reason)
                );
            }
        }
    }
    match codeflow_core::hooks::landed_policy::load(root) {
        Ok(landed) => {
            println!("policy source: {}", crate::cmd::shown(&landed.source));
            if let Some(note) = landed.baseline.note() {
                println!("{note}");
            }
            0
        }
        Err(error) => {
            println!("policy check: blocked: {}", crate::cmd::shown(&error));
            1
        }
    }
}

/// Read the authority fresh and write the record `mode` asks for, once the
/// session checks have passed.
fn write_checked(root: &Path, store: &Store, confirm: Option<&str>, mode: &Mode) -> i32 {
    let limits = codeflow_core::git::remote_query::QueryLimits::from_authority_or_default(root);
    let fresh = match codeflow_core::git::baseline_fresh::fresh(root, limits) {
        Ok(fresh) => fresh,
        Err(error) => return fail(&error),
    };
    println!("authority: {} at {}", fresh.authority, fresh.commit);
    println!(
        "remote: {} {}",
        fresh.remote,
        codeflow_core::git::remote_query::display_url(&fresh.remote_url)
    );
    println!("policy digest: {}", fresh.policy_digest);
    let now = codeflow_core::hooks::rfc3339_utc_now();
    let record = match (store.lookup(&fresh.common_dir), mode) {
        (Lookup::Never, Mode::Init) => baseline::initial(&fresh, &now),
        (Lookup::Found(_) | Lookup::Missing | Lookup::Malformed(_), Mode::Init) => {
            return fail("this repository already has a policy baseline; the operator runs codeflow policy baseline refresh")
        }
        (Lookup::Never, Mode::Refresh { .. }) => {
            return fail("this repository has no policy baseline; the operator runs codeflow policy baseline init")
        }
        (Lookup::Found(record), Mode::Refresh { reset }) => {
            let reset = reset.as_deref();
            let repo = match codeflow_core::hooks::repo::open(root) {
                Ok(Some(repo)) => repo,
                Ok(None) => return fail("not a repository with a commit"),
                Err(error) => return fail(&error),
            };
            match baseline::refresh_refusal(&repo, &record, &fresh) {
                Ok(None) => baseline::refreshed(&record, &fresh, &now, reset),
                Ok(Some(why)) if reset.is_some() => {
                    println!("reset: {}", crate::cmd::shown(&why));
                    baseline::refreshed(&record, &fresh, &now, reset)
                }
                Ok(Some(why)) => {
                    return fail(&format!(
                        "{why}; the operator passes --reset --reason <text> to accept it"
                    ))
                }
                Err(error) => return fail(&error),
            }
        }
        (Lookup::Missing | Lookup::Malformed(_), Mode::Refresh { reset: None }) => {
            return fail("the policy baseline is missing or unreadable; the operator passes --reset --reason <text> to write a new one")
        }
        (Lookup::Missing | Lookup::Malformed(_), Mode::Refresh { reset: Some(reason) }) => {
            let mut record = baseline::initial(&fresh, &now);
            record.resets.push(baseline::Reset {
                at: now.clone(),
                from_commit: String::new(),
                to_commit: fresh.commit.clone(),
                reason: reason.to_owned(),
            });
            record
        }
    };
    match confirm {
        None => {
            println!(
                "nothing written; to record it, rerun with --confirm {}",
                fresh.commit
            );
            0
        }
        Some(commit) if commit != fresh.commit => fail(&format!(
            "--confirm names {commit}, and the authority is at {}; nothing written",
            fresh.commit
        )),
        Some(_) => match store.write(&record) {
            Ok(path) => {
                println!("written: {}", path.display());
                0
            }
            Err(error) => fail(&error),
        },
    }
}
