//! `codeflow epic new "<title>"` and `codeflow task new --epic EPC-NNN
//! "<title>"` — allocate the next free id and scaffold the record from the pm
//! template. Allocation scans `project-management/` for the highest existing
//! id (per epic for tasks), so parallel worktrees stop colliding on numbers.

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
    /// Allocate the next `TSK-NNN-MMM` under an epic and scaffold it.
    New {
        /// Parent epic id (e.g. `EPC-007`).
        #[arg(long, short, value_name = "EPC-NNN")]
        epic: String,
        /// Task title.
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
    let TaskCommand::New { epic, title } = &args.command;
    if !is_valid_epic_format_id(epic) {
        eprintln!("error: invalid epic id '{epic}' (expected EPC-NNN)");
        return 2;
    }
    let pm = super::repo_root().join("project-management");
    if !allocate::epic_exists(&pm, epic) {
        eprintln!("error: epic {epic} not found under {}", pm.display());
        return 1;
    }
    let Some(template) = load_template("base/pm/task.md.tmpl") else {
        eprintln!("error: task template unavailable");
        return 1;
    };
    match allocate::create_task(&pm, &template, epic, title) {
        Ok(rec) => report(&rec),
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Print the allocated id and the file written, then return exit code 0.
fn report(rec: &NewRecord) -> i32 {
    println!("{}  {}", rec.format_id, rec.path.display());
    0
}

/// Read an embedded pm template as UTF-8.
fn load_template(asset: &str) -> Option<String> {
    EmbeddedAssets
        .read(asset)
        .and_then(|bytes| String::from_utf8(bytes).ok())
}
