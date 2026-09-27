//! Durable-work commands: `work next`, `work claim` and `work start`. All three
//! read the one readiness core (SPC-013 R-40, R-110); only `work claim`
//! writes, and what it writes is a branch.

use clap::{Args, Subcommand};
use codeflow_core::validate::validate_workgraph;
use codeflow_core::workgraph::readiness::{self, Backlog, Entry, State};
use codeflow_core::workgraph::{
    check_work_start, declared_work_target, resolve_work_target_checked,
};
use serde_json::json;

#[derive(Debug, Args)]
pub struct WorkArgs {
    #[command(subcommand)]
    pub command: WorkCommand,
}

#[derive(Debug, Subcommand)]
pub enum WorkCommand {
    /// List ready tasks first, then waiting and blocked ones with their
    /// reasons, from the refs as last fetched (no network call).
    Next {
        /// Only tasks of this epic.
        #[arg(long, value_name = "EPC-NNN")]
        epic: Option<String>,
        /// Print the same facts as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Fetch, check the task is ready and unclaimed on its target tip, then
    /// create and push `task/<id>-<slug>` from that tip. The pushed branch is
    /// an advisory mark others can see.
    Claim {
        /// Stable task id.
        task_id: String,
    },
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
    match &args.command {
        WorkCommand::Next { epic, json } => next(epic.as_deref(), *json),
        WorkCommand::Claim { task_id } => claim(task_id),
        WorkCommand::Start { task_id, target } => start(task_id, target.as_deref()),
    }
}

fn next(epic: Option<&str>, as_json: bool) -> i32 {
    let root = super::repo_root();
    let mut backlog = match readiness::backlog(&root) {
        Ok(backlog) => backlog,
        Err(error) => {
            eprintln!("work next: error: {error}");
            return 1;
        }
    };
    if let Some(epic) = epic {
        backlog
            .entries
            .retain(|entry| entry.epic_id.as_deref() == Some(epic));
    }
    if as_json {
        println!("{}", next_json(&backlog));
        return 0;
    }
    println!("{}", backlog.snapshot_line());
    let listed = [State::Ready, State::Waiting, State::Blocked, State::Invalid];
    if listed
        .iter()
        .all(|state| backlog.in_state(*state).next().is_none())
    {
        println!("no open task without a visible branch");
    }
    for state in listed {
        for entry in backlog.in_state(state) {
            println!("{}", entry_line(entry));
        }
    }
    let active = backlog.in_state(State::Active).count();
    if active > 0 {
        println!("{active} active (a visible branch carries the id); `codeflow status` lists them");
    }
    0
}

fn entry_line(entry: &Entry) -> String {
    let scope = entry.epic_id.as_deref().unwrap_or("standalone");
    let mut line = format!(
        "{:<8} {} {} ({scope} -> {})",
        entry.state.as_str(),
        entry.task_id,
        entry.title,
        entry.target
    );
    if !entry.reason.is_empty() {
        line.push_str("\n         ");
        line.push_str(&entry.reason);
    }
    line
}

fn next_json(backlog: &Backlog) -> serde_json::Value {
    json!({
        "snapshot": backlog.snapshots.iter().map(|snapshot| json!({
            "target": snapshot.target,
            "ref": snapshot.reference,
            "tip": snapshot.tip,
        })).collect::<Vec<_>>(),
        "fetched_at": backlog.fetched_at,
        "tasks": backlog.entries.iter().map(|entry| json!({
            "id": entry.task_id,
            "title": entry.title,
            "epic": entry.epic_id,
            "target": entry.target,
            "state": entry.state.as_str(),
            "reason": entry.reason,
            "branches": entry.branches,
        })).collect::<Vec<_>>(),
        "conflicts": backlog.conflicts,
    })
}

fn claim(task_id: &str) -> i32 {
    let root = super::repo_root();
    match readiness::claim(&root, task_id) {
        Ok(claim) => {
            println!(
                "work claim: {task_id} -> {} from {}",
                claim.branch, claim.from
            );
            if claim.pushed {
                println!("  pushed to origin; the branch is the visible mark of the claim");
            } else {
                println!("  no origin remote: the claim is visible only in this clone");
            }
            println!(
                "  next: check out {} and run `codeflow work start {task_id}`",
                claim.branch
            );
            0
        }
        Err(error) => {
            eprintln!("work claim: error: {error}");
            1
        }
    }
}

fn start(task_id: &str, target: Option<&str>) -> i32 {
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
        .map(str::to_string)
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
                println!("  dependencies met: {}", report.dependencies.join(", "));
            }
            let others = readiness::other_branches(&root, task_id, &report.branch);
            if !others.is_empty() {
                println!(
                    "  conflict: other visible branches carry {task_id}: {} (reported, not refused)",
                    others.join(", ")
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
