//! Read-only durable-work lifecycle commands.

use clap::{Args, Subcommand};
use codeflow_core::validate::validate_workgraph;
use codeflow_core::workgraph::{
    check_work_start, declared_work_target, resolve_work_target_checked,
};

#[derive(Debug, Args)]
pub struct WorkArgs {
    #[command(subcommand)]
    pub command: WorkCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkCommand {
    /// Verify that a durable task was planned and anchored before implementation.
    Start {
        /// Stable task id.
        task_id: String,
        /// Non-task branch/ref this task will merge into.
        #[arg(long = "into", value_name = "REF")]
        target: Option<String>,
    },
}

#[must_use]
pub fn run(args: &WorkArgs) -> i32 {
    let WorkCommand::Start { task_id, target } = &args.command;
    let root = super::repo_root();

    let workgraph = validate_workgraph(&root);
    if !workgraph.is_clean() {
        for issue in workgraph.issues {
            eprintln!("work start: error: {issue}");
        }
        eprintln!("work start: current workgraph is invalid; run `codeflow validate --docs`");
        return 1;
    }

    let declared = target
        .clone()
        .or_else(|| declared_work_target(&root, task_id));
    let target = match resolve_work_target_checked(&root, declared.as_deref()) {
        Ok(resolved) => {
            let resolved =
                resolved.unwrap_or_else(|| codeflow_core::workgraph::ResolvedWorkTarget {
                    target: "main".to_string(),
                    note: None,
                });
            if let Some(note) = &resolved.note {
                eprintln!("work start: note: {note}");
            }
            resolved.target
        }
        Err(error) => {
            eprintln!("work start: error: {error}");
            return 1;
        }
    };
    match check_work_start(&root, task_id, &target) {
        Ok(report) => {
            println!(
                "work start: {} anchored at {} for {} -> {}",
                report.task_id, report.merge_base, report.branch, report.target
            );
            if let Some(epic) = report.epic_id {
                println!("  epic: {epic}");
            } else {
                println!("  epic: standalone (reason verified)");
            }
            if !report.specs.is_empty() {
                println!("  specs: {}", report.specs.join(", "));
            }
            if !report.dependencies.is_empty() {
                println!(
                    "  dependencies complete: {}",
                    report.dependencies.join(", ")
                );
            }
            0
        }
        Err(error) => {
            eprintln!("work start: error: {error}");
            1
        }
    }
}
