//! Durable-work commands: `work next`, `work claim` and `work start`. All three
//! read the one readiness core (SPC-013 R-40, R-110); only `work claim`
//! writes, and what it writes is a branch.

use std::fmt::Write as _;

use clap::{Args, Subcommand};
use codeflow_core::hooks::policy::Policy;
use codeflow_core::hooks::PolicyLevel;
use codeflow_core::validate::validate_workgraph;
use codeflow_core::workgraph::readiness::{self, Backlog, Entry, State};
use codeflow_core::workgraph::{
    declared_work_target, durable_work_tracking_enabled, resolve_work_target_checked,
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
    /// reasons, from the refs as last fetched. Checks review evidence for stack hints.
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
        /// Reviewed predecessor pin, repeat once per code dependency.
        #[arg(long, value_name = "TSK-NNN@SHA")]
        on: Vec<String>,
    },
    /// Verify that a durable task was planned and anchored before implementation.
    Start {
        /// Stable task id.
        task_id: String,
        /// Non-task branch/ref this task will merge into.
        #[arg(long = "into", value_name = "REF")]
        target: Option<String>,
        /// Reviewed predecessor pin already contained in HEAD.
        #[arg(long, value_name = "TSK-NNN@SHA")]
        on: Vec<String>,
    },
}

#[must_use]
pub fn run(args: &WorkArgs) -> i32 {
    match &args.command {
        WorkCommand::Next { epic, json } => next(epic.as_deref(), *json),
        WorkCommand::Claim { task_id, on } => claim(task_id, on),
        WorkCommand::Start {
            task_id,
            target,
            on,
        } => start(task_id, target.as_deref(), on),
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
    // One reader for every waiting task: the refs, records and review
    // answers are read once per invocation, never once per task.
    let mut hints = readiness::StackHints::new(&root);
    // The hint is advisory, so it may read the remote-tracking branches
    // once; claim and start resolve each pin's repository afresh.
    let repositories = std::cell::OnceCell::new();
    for entry in &mut backlog.entries {
        if entry.state != State::Waiting {
            continue;
        }
        let Ok(hints) = hints.as_mut() else {
            break;
        };
        if let Ok(pins) = hints.hint(&root, &entry.task_id, &entry.target, &|branch, sha| {
            let repository = repositories
                .get_or_init(|| {
                    codeflow_core::workgraph::work_start::ReviewRepositories::open(&root)
                })
                .as_ref()
                .map_err(Clone::clone)?
                .repository(branch)?;
            reviewed(&root, &repository, branch, sha)
        }) {
            let noun = if pins.len() == 1 {
                "that pin is"
            } else {
                "those pins are"
            };
            let _ = write!(
                entry.reason,
                "; startable on {} once {noun} named",
                pins.join(", ")
            );
        }
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
    if !entry.branches.is_empty() {
        line.push_str("\n         branch: ");
        line.push_str(&entry.branches.join(", "));
    }
    for branch in &entry.informational_branches {
        line.push_str("\n         ");
        line.push_str(branch);
    }
    line
}

fn next_json(backlog: &Backlog) -> serde_json::Value {
    json!({
        "snapshot": backlog.snapshots.iter().map(|snapshot| json!({
            "target": snapshot.target,
            "ref": snapshot.reference,
            "tip": snapshot.tip,
            "problem": snapshot.problem,
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
            "informational_branches": entry.informational_branches,
        })).collect::<Vec<_>>(),
        "conflicts": backlog.conflicts,
    })
}

fn claim(task_id: &str, on: &[String]) -> i32 {
    let root = super::repo_root();
    if !on.is_empty() {
        if let Err(error) = readiness::refresh_claim(&root, task_id) {
            eprintln!("work claim: error: {error}");
            return 1;
        }
    }
    let pins = match resolve_pins(&root, on) {
        Ok(pins) => pins,
        Err(error) => {
            eprintln!("work claim: error: {error}");
            return 1;
        }
    };
    match readiness::claim_on(&root, task_id, &pins) {
        Ok(claim) => {
            println!(
                "work claim: {task_id} -> {} from {}",
                claim.branch, claim.from
            );
            for information in &claim.informational_branches {
                println!("  {information}");
            }
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

/// `work start`: the planning checks, once per task, at the
/// `git.work_planning` level (TSK-133). At `warn` a finding is reported and
/// the command succeeds; CI reports the same finding at the same level. An
/// unreadable tracking state always blocks.
fn start(task_id: &str, target: Option<&str>, on: &[String]) -> i32 {
    let root = super::repo_root();
    // An undeterminable tracking state blocks whatever the level says, as in
    // CI's `work.tracking_state`.
    if let Err(error) = durable_work_tracking_enabled(&root) {
        eprintln!(
            "work start: error: {}",
            codeflow_core::workgraph::work_start::tracking_state_message(error)
        );
        eprintln!(
            "work start: repair CodeFlow state or task-home access; this blocks whatever git.work_planning says"
        );
        return 1;
    }
    let (policy, _armed) = Policy::load_effective(&root);
    let level = policy.git.work_planning_level();
    let Err(findings) = plan_check(&root, task_id, target, on) else {
        return 0;
    };
    let warn = level == PolicyLevel::Warn;
    let label = if warn { "warning" } else { "error" };
    for finding in &findings {
        eprintln!("work start: {label}: {finding}");
    }
    if warn {
        eprintln!(
            "work start: continuing: git.work_planning is warn, so this finding does not stop the work; CI reports it too"
        );
        0
    } else {
        1
    }
}

/// The planning checks behind `work start`: the anchor report printed on
/// success, or every finding.
fn plan_check(
    root: &std::path::Path,
    task_id: &str,
    target: Option<&str>,
    on: &[String],
) -> Result<(), Vec<String>> {
    let workgraph = validate_workgraph(root);
    if !workgraph.is_clean() {
        let mut findings = workgraph.issues;
        findings.push("current workgraph is invalid; run `codeflow validate --docs`".to_string());
        return Err(findings);
    }

    let declared = target
        .map(str::to_string)
        .or_else(|| declared_work_target(root, task_id));
    let target = match resolve_work_target_checked(root, declared.as_deref()) {
        Ok(resolved) => resolved.map_or_else(|| "main".to_string(), |r| r.target),
        Err(error) => return Err(vec![error.to_string()]),
    };
    let pins = resolve_pins(root, on).map_err(|error| vec![error])?;
    let report =
        codeflow_core::workgraph::work_start::check_work_start_on(root, task_id, &target, &pins)
            .map_err(|error| vec![error.to_string()])?;
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
    match readiness::other_branches(root, task_id, &report.branch) {
        Ok(others) if others.is_empty() => {}
        Ok(others) => println!(
            "  conflict: other visible branches carry {task_id}: {} (reported, not refused)",
            others.join(", ")
        ),
        Err(error) => println!("  note: other branches were not checked: {error}"),
    }
    Ok(())
}

fn resolve_pins(
    root: &std::path::Path,
    on: &[String],
) -> Result<Vec<codeflow_core::workgraph::work_start::ReviewedPin>, String> {
    codeflow_core::workgraph::work_start::reviewed_pins(root, on, &|branch, sha| {
        let repository = codeflow_core::workgraph::work_start::review_repository(root, branch)?;
        reviewed(root, &repository, branch, sha)
    })
}

/// Whether the pull request of `branch` in `repository` names `sha` reviewed.
fn reviewed(
    root: &std::path::Path,
    repository: &str,
    branch: &str,
    sha: &str,
) -> Result<bool, String> {
    let proof = pr_review(root, branch, repository)?;
    if proof["headRefName"].as_str() != Some(branch)
        || proof["headRefOid"].as_str() != Some(sha)
        || proof["isCrossRepository"].as_bool() != Some(false)
        || proof["headRepository"]["nameWithOwner"].as_str()
            != repository.split_once('/').map(|(_, repo)| repo)
    {
        return Err("predecessor PR identity or tip differs from the pin; rebase on its new reviewed head and recheck".into());
    }
    let (policy, _) = Policy::load_effective(root);
    let headings =
        codeflow_core::hooks::adoption::mapped_sections(&policy.git, &["Reviews".into()]);
    Ok(super::ci::pr_body::review_names_revision(
        proof["body"].as_str().unwrap_or_default(),
        &headings[0],
        sha,
    ))
}

/// Bound provider lifetime and response size; a regular output file means a
/// descendant retaining stdout cannot hold an output-reader thread open.
fn pr_review(
    root: &std::path::Path,
    branch: &str,
    repository: &str,
) -> Result<serde_json::Value, String> {
    use std::io::{Read as _, Seek as _};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut output = tempfile::tempfile().map_err(|error| error.to_string())?;
    let mut command = Command::new("gh");
    command
        .args([
            "pr",
            "view",
            branch,
            "--repo",
            repository,
            "--json",
            "body,headRefName,headRefOid,isCrossRepository,headRepository",
        ])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|error| error.to_string())?)
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot verify review for this pin: {error}"))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None)
                if start.elapsed() < Duration::from_secs(5)
                    && output.metadata().is_ok_and(|m| m.len() <= 1_048_576) =>
            {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => break Err(
                "cannot verify review for this pin: provider unavailable, too large or timed out"
                    .to_string(),
            ),
        }
    };
    // Terminate descendants too, including those holding the output file open.
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
    if !status?.success() {
        return Err("cannot verify review for this pin: provider failed".into());
    }
    output.rewind().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    output
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("review response too large".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse predecessor review: {error}"))
}
