//! `codeflow ids`: the shared id registry (SPC-013 R-5 to R-25, R-108 to
//! R-111; ADR-0072). Maintainer commands (`seed`, `admit`, `restore`) need
//! push rights to `codeflow/registry`; CI only ever runs `check`.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use codeflow_core::ids::check::{self, Report};
use codeflow_core::ids::entry::frontmatter_value;
use codeflow_core::ids::{issue, record_id_from_path, seed, Git, RegId};

/// Arguments for `codeflow ids`.
#[derive(Debug, Args)]
pub struct IdsArgs {
    #[command(subcommand)]
    pub command: IdsCommand,
}

/// Registry subcommands.
#[derive(Debug, Subcommand)]
pub enum IdsCommand {
    /// Register every id on every ref and in retained history, once (maintainer).
    Seed {
        /// Map copies of one record that provenance cannot decide.
        #[arg(long, value_name = "FILE")]
        map: Option<PathBuf>,
    },
    /// Copy each registered uid into the records of the current line.
    Backfill,
    /// Publish pending reservations made offline.
    Sync,
    /// Reserve the number of a fork or hand-written record for its uid (maintainer).
    Admit {
        /// The record: a path in the working tree, or `<rev>:<path>`.
        record: String,
    },
    /// Renumber an unmerged record whose number another record holds.
    Retarget {
        /// The record's current id.
        id: String,
    },
    /// Return registry files to the bytes of their first addition (maintainer).
    Restore {
        /// The ids to restore.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Check the registry and reconcile it with every ref (read only).
    Check {
        /// Registry ref to read (default: the fetched authority copy, else local).
        #[arg(long, value_name = "REF")]
        registry: Option<String>,
        /// With --head: also apply the merge rule and uniqueness scan to base...head.
        #[arg(long, value_name = "REF", requires = "head")]
        base: Option<String>,
        /// Head of the range to judge.
        #[arg(long, value_name = "REF", requires = "base")]
        head: Option<String>,
    },
}

/// Run `codeflow ids`.
pub fn run(args: &IdsArgs) -> i32 {
    let root = super::repo_root();
    match &args.command {
        IdsCommand::Seed { map } => run_seed(&root, map.as_ref()),
        IdsCommand::Backfill => match seed::backfill(&root) {
            Ok(report) => {
                println!("ids backfill: wrote {} uid(s)", report.written.len());
                for refused in &report.refused {
                    eprintln!("refused: {refused}");
                }
                i32::from(!report.refused.is_empty())
            }
            Err(error) => fail(&error),
        },
        IdsCommand::Sync => match issue::sync(&root) {
            Ok(report) => {
                if let Some(note) = &report.note {
                    println!("ids sync: {note}");
                }
                println!(
                    "ids sync: published {}; already reserved {}",
                    list(&report.published),
                    list(&report.already)
                );
                0
            }
            Err(error) => fail(&error),
        },
        IdsCommand::Admit { record } => run_admit(&root, record),
        IdsCommand::Retarget { id } => {
            let Some(id) = parse_id(id) else { return 2 };
            match seed::retarget(&root, &id) {
                Ok(done) => {
                    println!("{}  ->  {}  {}", done.from, done.to, done.path.display());
                    println!("registry: {}", done.standing.describe());
                    println!("links rewritten in {} file(s)", done.rewritten.len());
                    0
                }
                Err(error) => fail(&error),
            }
        }
        IdsCommand::Restore { ids } => {
            let mut parsed = Vec::new();
            for id in ids {
                let Some(id) = parse_id(id) else { return 2 };
                parsed.push(id);
            }
            match issue::restore(&root, &parsed) {
                Ok(restored) => {
                    println!(
                        "restore: {} returned to their first addition",
                        list(&restored)
                    );
                    0
                }
                Err(error) => fail(&error),
            }
        }
        IdsCommand::Check {
            registry,
            base,
            head,
        } => {
            match codeflow_core::workgraph::durable_work_tracking_enabled(&root) {
                Ok(true) => {}
                Ok(false) => {
                    println!("codeflow ids check: durable work tracking is off; no registry rule applies");
                    return 0;
                }
                Err(error) => {
                    eprintln!(
                        "codeflow ids check: cannot determine durable-work tracking: {error}"
                    );
                    return 2;
                }
            }
            let git = Git::new(&root);
            let mut report = match check::check(&git, registry.as_deref()) {
                Ok(report) => report,
                Err(error) => {
                    eprintln!("codeflow ids check: could not run: {error}");
                    return 2;
                }
            };
            if let (Some(base), Some(head)) = (base, head) {
                match check::merge_rule(&git, base, head) {
                    Ok(range) => merge(&mut report, range),
                    Err(error) => {
                        eprintln!("codeflow ids check: could not run the merge rule: {error}");
                        return 2;
                    }
                }
            }
            render(&report)
        }
    }
}

fn run_seed(root: &std::path::Path, map: Option<&PathBuf>) -> i32 {
    let map = match map {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => match seed::SeedMap::parse(&text) {
                Ok(map) => Some(map),
                Err(error) => return fail(&error),
            },
            Err(error) => {
                eprintln!("error: cannot read {}: {error}", path.display());
                return 1;
            }
        },
        None => None,
    };
    match seed::seed(root, map.as_ref()) {
        Ok(report) => {
            println!(
                "ids seed: registered {} new id(s); {} already registered",
                report.registered.len(),
                report.already
            );
            if let Some(standing) = report.standing {
                println!("registry: {}", standing.describe());
            }
            if let Some(map) = &map {
                println!(
                    "map sha256:{} (cite it in the verification record)",
                    map.digest
                );
            }
            0
        }
        Err(error) => fail(&error),
    }
}

fn run_admit(root: &std::path::Path, record: &str) -> i32 {
    let (text, path) = match record.split_once(':') {
        Some((rev, path)) if !rev.is_empty() && !std::path::Path::new(record).exists() => {
            match Git::new(root).run(&["show", record]) {
                Ok(text) => (text, path.to_string()),
                Err(error) => return fail(&error),
            }
        }
        _ => match std::fs::read_to_string(root.join(record)) {
            Ok(text) => (text, record.to_string()),
            Err(error) => {
                eprintln!("error: cannot read {record}: {error}");
                return 1;
            }
        },
    };
    let Some(id) = record_id_from_path(&path)
        .or_else(|| frontmatter_value(&text, "id").and_then(|id| RegId::parse(&id)))
    else {
        eprintln!("error: {record} is not a record path with an id");
        return 2;
    };
    let Some(uid) = frontmatter_value(&text, "uid").filter(|uid| codeflow_core::ids::is_uid(uid))
    else {
        eprintln!(
            "error: {record} carries no valid uid; add `uid: {}` after its id line and admit again",
            codeflow_core::ids::new_uid()
        );
        return 1;
    };
    let title = frontmatter_value(&text, "title").unwrap_or_default();
    let target =
        frontmatter_value(&text, "integration_target").unwrap_or_else(|| "none".to_string());
    match issue::admit(root, &id, &uid, &title, &target) {
        Ok(admission) if admission.already => {
            println!("{} is already admitted as {}", id, admission.reserved);
            0
        }
        Ok(admission) if admission.reserved == admission.requested => {
            println!(
                "admitted: {} -> {uid} ({})",
                admission.reserved,
                admission.standing.describe()
            );
            0
        }
        Ok(admission) => {
            println!(
                "{} is held by another record; reserved {} for uid {uid} ({})",
                admission.requested,
                admission.reserved,
                admission.standing.describe()
            );
            println!(
                "the record's author retargets it: `codeflow ids retarget {}`",
                admission.requested
            );
            0
        }
        Err(error) => fail(&error),
    }
}

fn merge(into: &mut Report, from: Report) {
    into.info.extend(from.info);
    into.warns.extend(from.warns);
    into.blocks.extend(from.blocks);
}

/// Print a report; exit 1 when it blocks.
pub fn render(report: &Report) -> i32 {
    for line in &report.info {
        println!("codeflow ids check: {line}");
    }
    for line in &report.warns {
        eprintln!("codeflow ids check: warn: {line}");
    }
    for line in &report.blocks {
        eprintln!("codeflow ids check: block: {line}");
    }
    if report.passed() {
        println!("codeflow ids check: pass");
        0
    } else {
        eprintln!(
            "codeflow ids check: FAIL ({} finding(s))",
            report.blocks.len()
        );
        1
    }
}

fn parse_id(id: &str) -> Option<RegId> {
    let parsed = RegId::parse(id);
    if parsed.is_none() {
        eprintln!("error: '{id}' is not a record id (EPC-NNN, SPC-NNN, TSK-NNN, ADR-NNNN)");
    }
    parsed
}

fn list(ids: &[RegId]) -> String {
    if ids.is_empty() {
        "none".to_string()
    } else {
        ids.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn fail(error: &codeflow_core::ids::IdsError) -> i32 {
    eprintln!("error: {error}");
    1
}
