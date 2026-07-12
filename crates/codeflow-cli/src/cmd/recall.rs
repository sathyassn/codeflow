//! `codeflow recall [--all] "<query>"` — FTS5 search over ledgers, session
//! summaries, ADRs, epics/tasks, capabilities, and the product WHY docs
//! (`docs/product.md` + `docs/plan/`) (charter §8).

use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::Args;
use codeflow_core::recall::{recall, RecallOptions, RepoTarget};
use codeflow_core::registry;

/// Arguments for `codeflow recall`.
#[derive(Debug, Args)]
pub struct RecallArgs {
    /// Search every repo in the user registry, not just the current one.
    #[arg(long)]
    pub all: bool,

    /// Drop the index for the searched repos and re-sync (the index is a
    /// rebuildable cache).
    #[arg(long)]
    pub rebuild: bool,

    /// Maximum number of results (default 20, or `[recall].limit` from
    /// ~/.codeflow/config.toml).
    #[arg(long)]
    pub limit: Option<usize>,

    /// The query.
    pub query: String,
}

/// Run `codeflow recall`.
///
/// # Errors
///
/// Returns an error when no codeflow home can be resolved, the current
/// directory is not inside a codeflow repo (without `--all`), or the index
/// cannot be opened.
pub fn run(args: &RecallArgs) -> anyhow::Result<()> {
    let home = registry::codeflow_home()
        .context("cannot resolve ~/.codeflow (HOME unset; set CODEFLOW_HOME)")?;

    let user_config = registry::UserConfig::load(&home).unwrap_or_else(|e| {
        eprintln!("warning: {e}; using defaults");
        registry::UserConfig::default()
    });
    let limit = args.limit.or(user_config.recall.limit).unwrap_or(20);

    let registered = registry::list_repos(&home).map_err(|e| anyhow::anyhow!(e))?;

    let mut notes_extra: Vec<String> = Vec::new();
    let targets: Vec<RepoTarget> = if args.all {
        if registered.is_empty() {
            bail!(
                "no repos registered yet — run any codeflow command inside an \
                 initialized repo to register it"
            );
        }
        registered
            .iter()
            .map(|r| RepoTarget {
                name: r.name.clone(),
                root: PathBuf::from(&r.path),
            })
            .collect()
    } else {
        let cwd = std::env::current_dir().context("cannot resolve current directory")?;
        let Some(root) = registry::find_repo_root(&cwd) else {
            bail!(
                "not inside a codeflow repo (no .codeflow/ found); \
                 use --all to search registered repos"
            );
        };
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        let info = registry::read_project_info(&root);
        let others = registered
            .iter()
            .filter(|r| std::path::Path::new(&r.path) != root)
            .count();
        if others > 0 {
            notes_extra.push(format!(
                "{others} other registered repo(s) not searched — use --all"
            ));
        }
        vec![RepoTarget {
            name: info.name,
            root,
        }]
    };

    let options = RecallOptions {
        rebuild: args.rebuild,
        limit,
    };
    let db = registry::recall_db_path(&home);
    let report = recall(&db, &targets, &args.query, &options)?;

    if report.results.is_empty() {
        println!("no results for \"{}\"", args.query);
    } else {
        println!(
            "{} result(s) for \"{}\":\n",
            report.results.len(),
            args.query
        );
        for (i, r) in report.results.iter().enumerate() {
            println!("{}. [{}] {} ({}) — {}", i + 1, r.repo, r.path, r.kind, r.title);
            println!("   {}", r.snippet.replace('\n', " "));
        }
    }

    let all_notes: Vec<&String> = report.notes.iter().chain(notes_extra.iter()).collect();
    if !all_notes.is_empty() {
        println!("\ncoverage notes:");
        for note in all_notes {
            println!("  - {note}");
        }
    }
    println!(
        "\nindex: {} synced, {} unchanged, {} removed",
        report.stats.files_indexed, report.stats.files_skipped, report.stats.files_removed
    );

    Ok(())
}
