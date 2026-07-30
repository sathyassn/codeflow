//! Durable work-record creation. The CLI writes independent `EPC-NNN`,
//! `SPC-NNN`, and `TSK-NNN` ids in the canonical flat layout.

use clap::{Args, Subcommand};
use codeflow_core::scaffold::AssetSource;
use codeflow_core::workgraph::{allocate, is_valid_epic_format_id, NewRecord};

use crate::embedded::EmbeddedAssets;

/// Arguments for `codeflow epic`.
#[derive(Debug, Args)]
pub struct EpicArgs {
    #[command(subcommand)]
    pub command: EpicCommand,
}

/// Epic subcommands.
#[derive(Debug, Subcommand)]
pub enum EpicCommand {
    /// Allocate the next `EPC-NNN` and scaffold the epic from the template.
    New {
        /// Epic title.
        title: String,
    },
}

/// Arguments for `codeflow task`.
#[derive(Debug, Args)]
pub struct TaskArgs {
    #[command(subcommand)]
    pub command: TaskCommand,
}

/// Task subcommands.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// Allocate the next independent `TSK-NNN` and scaffold it.
    New {
        /// Parent epic id. Mutually exclusive with --standalone-reason.
        #[arg(long, short, value_name = "EPC-NNN")]
        epic: Option<String>,
        /// Why this durable task does not belong to an epic.
        #[arg(long, value_name = "REASON")]
        standalone_reason: Option<String>,
        /// Existing local or remote-tracking non-task branch this task will integrate into.
        #[arg(long = "into", value_name = "BRANCH")]
        integration_target: Option<String>,
        /// Task title.
        title: String,
    },
}

/// Arguments for `codeflow spec`.
#[derive(Debug, Args)]
pub struct SpecArgs {
    #[command(subcommand)]
    pub command: SpecCommand,
}

/// Spec subcommands.
#[derive(Debug, Subcommand)]
pub enum SpecCommand {
    /// Allocate the next `SPC-NNN`, scaffold it, and link the consuming work item.
    New {
        /// Epic or task that consumes this specification.
        #[arg(long = "for", value_name = "EPC-NNN|TSK-NNN")]
        work_item: String,
        /// Specification title.
        title: String,
    },
}

/// Run `codeflow epic`.
pub fn run_epic(args: &EpicArgs) -> i32 {
    let EpicCommand::New { title } = &args.command;
    let pm = super::repo_root().join("project-management");
    let Some(template) = load_template("base/pm/epic.md.tmpl") else {
        eprintln!("error: epic template unavailable");
        return 1;
    };
    match allocate::create_epic(&pm, &template, title) {
        Ok(rec) => report(&rec),
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Run `codeflow task`.
pub fn run_task(args: &TaskArgs) -> i32 {
    let TaskCommand::New {
        epic,
        standalone_reason,
        integration_target,
        title,
    } = &args.command;
    if epic
        .as_deref()
        .is_some_and(|value| !is_valid_epic_format_id(value))
    {
        eprintln!(
            "error: invalid epic id '{}' (expected EPC-NNN)",
            epic.as_deref().unwrap_or_default()
        );
        return 2;
    }
    match (epic.as_deref(), standalone_reason.as_deref()) {
        (Some(_), Some(_)) => {
            eprintln!("error: --epic and --standalone-reason are mutually exclusive");
            return 2;
        }
        (None, None) => {
            eprintln!("error: task new requires --epic or --standalone-reason");
            return 2;
        }
        _ => {}
    }
    let pm = super::repo_root().join("project-management");
    if epic
        .as_deref()
        .is_some_and(|value| !allocate::epic_exists(&pm, value))
    {
        eprintln!(
            "error: epic {} not found under {}",
            epic.as_deref().unwrap_or_default(),
            pm.display()
        );
        return 1;
    }
    let Some(template) = load_template("base/pm/task.md.tmpl") else {
        eprintln!("error: task template unavailable");
        return 1;
    };
    match allocate::create_task(
        &pm,
        &template,
        epic.as_deref(),
        standalone_reason.as_deref(),
        integration_target.as_deref(),
        title,
    ) {
        Ok(rec) => report(&rec),
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Run `codeflow spec`.
pub fn run_spec(args: &SpecArgs) -> i32 {
    let SpecCommand::New { work_item, title } = &args.command;
    let pm = super::repo_root().join("project-management");
    if !allocate::work_item_exists(&pm, work_item) {
        eprintln!(
            "error: work item {work_item} not found under {}",
            pm.display()
        );
        return 1;
    }
    let Some(template) = load_template("base/pm/spec.md.tmpl") else {
        eprintln!("error: spec template unavailable");
        return 1;
    };
    match allocate::create_spec(&pm, &template, work_item, title) {
        Ok(rec) => report(&rec),
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

/// Print the allocated id and the file written, then return exit code 0.
fn report(rec: &NewRecord) -> i32 {
    println!("{}  {}", rec.id, rec.path.display());
    0
}

/// Read an embedded pm template as UTF-8.
fn load_template(asset: &str) -> Option<String> {
    EmbeddedAssets
        .read(asset)
        .and_then(|bytes| String::from_utf8(bytes).ok())
}
