//! Durable-work commands: `work next`, `work claim` and `work start`. All three
//! read the one readiness core (SPC-013 R-40, R-110). Claim and start fetch;
//! claim also writes a branch and its fixed base evidence. `work status` is the recovery
//! inventory: per task, the worktrees holding work found nowhere else.

use std::fmt::Write as _;

use clap::{Args, Subcommand};
use codeflow_core::hooks::policy::Policy;
use codeflow_core::hooks::PolicyLevel;
use codeflow_core::validate::validate_workgraph;
use codeflow_core::workgraph::readiness::{self, Backlog, Entry, State};
use codeflow_core::workgraph::work_start::{ReviewedPin, REVIEW_LOOKUP_TIMEOUT};
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
    /// List ready tasks first, then landed (work merged), waiting and blocked
    /// ones with their reasons, from the refs as last fetched. Checks review
    /// evidence for stack hints.
    Next {
        /// Only tasks of this epic.
        #[arg(long, value_name = "EPC-NNN")]
        epic: Option<String>,
        /// Print the same facts as JSON, each task with its `state`; a landed task adds `landed_branches` and `suggested_command`.
        #[arg(long)]
        json: bool,
    },
    /// Fetch, choose the live target tip or the single line holding a completed
    /// predecessor the target has not received, then create and push
    /// `task/<id>-<slug>` from that base. The pushed branch is an advisory mark
    /// others can see.
    Claim {
        /// Stable task id.
        task_id: String,
        /// Reviewed predecessor pin, repeat once per code dependency. A copy
        /// of the predecessor branch behind the pin is accepted; a copy ahead
        /// of it or diverged from it is refused.
        #[arg(long, value_name = "TSK-NNN@SHA")]
        on: Vec<String>,
    },
    /// Fetch, verify planning, and require HEAD to contain its recorded claim base or the current selected base.
    Start {
        /// Stable task id.
        task_id: String,
        /// Non-task branch/ref this task will merge into.
        #[arg(long = "into", value_name = "REF")]
        target: Option<String>,
        /// Reviewed predecessor pin already contained in HEAD. Its code
        /// dependency edge may be on the target or added by this task's own
        /// committed record.
        #[arg(long, value_name = "TSK-NNN@SHA")]
        on: Vec<String>,
    },
    /// List, per task, each worktree with uncommitted changes, unpushed
    /// commits or commits on no branch, read from git; work that cannot be
    /// recovered is named. Reads only.
    Status,
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
        WorkCommand::Status => status(),
    }
}

fn status() -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(error) => {
            eprintln!("work status: error: {error}");
            return 2;
        }
    };
    match codeflow_core::worktree_inventory::inventory(&cwd) {
        Ok(inventory) => {
            print_block!(codeflow_core::worktree_inventory::render(&inventory));
            0
        }
        Err(error) => {
            eprintln!(
                "work status: error: {error}",
                error = crate::cmd::shown(&error)
            );
            1
        }
    }
}

fn next(epic: Option<&str>, as_json: bool) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    let mut backlog = match readiness::backlog(&root) {
        Ok(backlog) => backlog,
        Err(error) => {
            eprintln!(
                "work next: error: {error}",
                error = crate::cmd::shown(&error)
            );
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
        if let Ok(pins) = hints.hint(
            &root,
            &entry.task_id,
            &entry.target,
            &|branch, sha, covers| {
                let repository = repositories
                    .get_or_init(|| {
                        codeflow_core::workgraph::work_start::ReviewRepositories::open(&root)
                    })
                    .as_ref()
                    .map_err(Clone::clone)?
                    .repository(branch)?;
                reviewed(&root, &repository, branch, sha, covers)
            },
        ) {
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
        // JSON keeps each value exact; its bytes hold no hidden character
        // (`display_json`, TSK-388).
        println_block!(codeflow_core::git::TextBlock::json(
            &next_json(&backlog).to_string()
        ));
        return 0;
    }
    println!("{}", backlog.snapshot_line());
    let listed = [
        State::Ready,
        State::Landed,
        State::Waiting,
        State::Blocked,
        State::Invalid,
    ];
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
    use codeflow_core::git::{display_message, display_text};
    let scope = entry.epic_id.as_deref().unwrap_or("standalone");
    // A title is record text and a target a branch name: shown escaped,
    // with a further line of either indented (TSK-388).
    let mut line = format!(
        "{:<8} {} {} ({scope} -> {})",
        entry.state.as_str(),
        entry.task_id,
        display_message(&entry.title),
        display_text(&entry.target)
    );
    if !entry.reason.is_empty() {
        line.push_str("\n         ");
        line.push_str(&display_message(&entry.reason));
    }
    if !entry.branches.is_empty() {
        line.push_str("\n         branch: ");
        // The backlog holds claim branches in display form already.
        line.push_str(&entry.branches.join(", "));
    }
    if !entry.landed_branches.is_empty() {
        line.push_str("\n         landed branch: ");
        line.push_str(&entry.landed_branches.join(", "));
        line.push_str("\n         next: ");
        line.push_str(&readiness::landed_status_command(&entry.task_id));
    }
    for branch in &entry.informational_branches {
        line.push_str("\n         ");
        line.push_str(&codeflow_core::git::display_output(branch));
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
            // The names git stores; `display_json` escapes them on output.
            "branches": entry.branch_names,
            // Set on a `landed` task: the branches whose work reached the
            // target and the command that settles the record (TSK-1038).
            "landed_branches": entry.landed_branch_names,
            "suggested_command": (entry.state == State::Landed)
                .then(|| readiness::landed_status_command(&entry.task_id)),
            "informational_branches": entry.informational_branches,
        })).collect::<Vec<_>>(),
        "conflicts": backlog
            .conflicts
            .keys()
            .filter_map(|id| {
                backlog
                    .entries
                    .iter()
                    .find(|entry| &entry.task_id == id)
                    .map(|entry| (id.clone(), entry.branch_names.clone()))
            })
            .collect::<std::collections::BTreeMap<_, _>>(),
    })
}

fn claim(task_id: &str, on: &[String]) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    if !on.is_empty() {
        if let Err(error) = readiness::refresh_claim(&root, task_id) {
            eprintln!(
                "work claim: error: {error}",
                error = crate::cmd::shown(&error)
            );
            return 1;
        }
    }
    let pins = match resolve_pins(&root, on) {
        Ok(pins) => pins,
        Err(error) => {
            eprintln!(
                "work claim: error: {error}",
                error = crate::cmd::shown(&error)
            );
            return 1;
        }
    };
    match readiness::claim_on(&root, task_id, &pins) {
        Ok(claim) => {
            println!(
                "work claim: {task_id} -> {} from {}",
                claim.branch, claim.from
            );
            if !claim.omitted_words.is_empty() {
                println!(
                    "  {}",
                    codeflow_core::workgraph::light_paths::omitted_words_note(&claim.omitted_words)
                );
            }
            for information in &claim.informational_branches {
                println!("  {}", codeflow_core::git::display_output(information));
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
            eprintln!(
                "work claim: error: {error}",
                error = crate::cmd::shown(&error)
            );
            1
        }
    }
}

/// `work start`: the planning checks, once per task, at the
/// `git.work_planning` level (TSK-133). At `warn` a finding is reported and
/// the command succeeds; CI reports the same finding at the same level. An
/// unreadable tracking state or unverifiable explicit pin always blocks.
fn start(task_id: &str, target: Option<&str>, on: &[String]) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
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
    let (policy, _armed) = match Policy::load_effective(&root) {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "codeflow: cannot read policy: {}",
                codeflow_core::git::display_message(&error)
            );
            return 2;
        }
    };
    // Missing live base evidence blocks even at the project's warning level.
    // Fetch before resolving pins, as `work claim` does, so a replacement pin
    // is judged against the pushed predecessor head.
    if let Err(error) = readiness::refresh_claim(&root, task_id) {
        eprintln!("work start: error: {}", crate::cmd::shown(&error));
        return 1;
    }
    // Explicit review evidence is required even when planning findings warn.
    // Keep verification failures outside the policy-controlled findings.
    let pins = match resolve_pins(&root, on) {
        Ok(pins) => pins,
        Err(error) => {
            eprintln!(
                "work start: error: {error}",
                error = crate::cmd::shown(&error)
            );
            return 1;
        }
    };
    let level = policy.git.work_planning_level();
    let findings = match plan_check(&root, task_id, target, &pins) {
        Ok(verified) => {
            match codeflow_core::workgraph::claim_base::renew_reviewed(
                &root,
                task_id,
                &verified.branch,
                &verified.target,
                &pins,
            ) {
                Ok(Some(note)) => println!("  {}", crate::cmd::shown(&note)),
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "work start: error: recording reviewed claim base failed: {}",
                        crate::cmd::shown(&error)
                    );
                    return 1;
                }
            }
            return 0;
        }
        Err(findings) => findings,
    };
    let warn = level == PolicyLevel::Warn;
    let label = if warn { "warning" } else { "error" };
    for finding in &findings {
        eprintln!("work start: {label}: {}", crate::cmd::shown(finding));
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

/// The branch and target that passed `plan_check`, which `work start` uses
/// to renew a reviewed claim base.
struct VerifiedClaim {
    branch: String,
    target: String,
}

/// The planning checks behind `work start`: the anchor report printed on
/// success, or every finding.
fn plan_check(
    root: &std::path::Path,
    task_id: &str,
    target: Option<&str>,
    pins: &[ReviewedPin],
) -> Result<VerifiedClaim, Vec<String>> {
    let workgraph = validate_workgraph(root);
    if !workgraph.is_clean() {
        let mut findings = workgraph.issues;
        findings.push("current workgraph is invalid; run `codeflow validate --docs`".to_string());
        return Err(findings);
    }

    let declared = match target {
        Some(target) => Some(target.to_string()),
        None => declared_work_target(root, task_id).map_err(|error| vec![error.to_string()])?,
    };
    let target = match resolve_work_target_checked(root, declared.as_deref()) {
        Ok(resolved) => resolved.map_or_else(|| "main".to_string(), |r| r.target),
        Err(error) => return Err(vec![error.to_string()]),
    };
    let report =
        codeflow_core::workgraph::work_start::check_work_start_on(root, task_id, &target, pins)
            .map_err(|error| vec![error.to_string()])?;
    println!(
        "work start: {} anchored at {} for {} -> {}",
        report.task_id,
        report.merge_base,
        report.branch,
        // A fork's tracking ref holds its remote's name, shown only when
        // plain (TSK-416 review round 7).
        codeflow_core::security::git::without_remote_names(root, &report.target)
    );
    if let Some(note) = report.base_note {
        println!("  {}", crate::cmd::shown(&note));
    }
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
        Err(error) => println!(
            "  note: other branches were not checked: {error}",
            error = crate::cmd::shown(&error)
        ),
    }
    Ok(VerifiedClaim {
        branch: report.branch,
        target: report.target,
    })
}

fn resolve_pins(
    root: &std::path::Path,
    on: &[String],
) -> Result<Vec<codeflow_core::workgraph::work_start::ReviewedPin>, String> {
    codeflow_core::workgraph::work_start::reviewed_pins(root, on, &|branch, sha, covers| {
        review_lookup(root, branch, sha, covers)
    })
}

/// The review lookup `work claim`, `work start` and `codeflow ci` share
/// (SPC-013 R-42): the hosted repository of `branch`, then [`reviewed`].
pub(super) fn review_lookup(
    root: &std::path::Path,
    branch: &str,
    sha: &str,
    covers: &codeflow_core::workgraph::work_start::ReviewCovers<'_>,
) -> Result<bool, String> {
    let repository = codeflow_core::workgraph::work_start::review_repository(root, branch)?;
    reviewed(root, &repository, branch, sha, covers)
}

/// Whether the pull request of `branch` in `repository` has `sha` as its
/// head, from the same owner and repository, and an approving review row
/// naming a revision that `covers` accepts: the head itself, or a commit
/// the head follows only by the predecessor's status and Closeout. The
/// revisions are judged only once that pull request is found.
fn reviewed(
    root: &std::path::Path,
    repository: &str,
    branch: &str,
    sha: &str,
    covers: &codeflow_core::workgraph::work_start::ReviewCovers<'_>,
) -> Result<bool, String> {
    let proof = pr_review(root, branch, repository)?;
    // review_repository returns HOST/OWNER/NAME for gh's --repo argument.
    // GitHub owner and repository names are case-insensitive, so a remote URL
    // may differ in case from the API's canonical login and name.
    let mut parts = repository.rsplit('/');
    let name = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    if name.is_empty() || owner.is_empty() {
        return Err("predecessor has no repository identity".into());
    }
    if proof["headRefName"].as_str() != Some(branch)
        || proof["headRefOid"].as_str() != Some(sha)
        || proof["isCrossRepository"].as_bool() != Some(false)
        || !proof["headRepository"]["name"]
            .as_str()
            .is_some_and(|value| value.eq_ignore_ascii_case(name))
        || !proof["headRepositoryOwner"]["login"]
            .as_str()
            .is_some_and(|value| value.eq_ignore_ascii_case(owner))
    {
        return Err(format!("predecessor PR identity or tip differs from the pin {sha}; rebase on its new reviewed head and recheck"));
    }
    let (policy, _) = Policy::load_effective(root)?;
    let headings =
        codeflow_core::hooks::adoption::mapped_sections(&policy.git, &["Reviews".into()]);
    let body = proof["body"]
        .as_str()
        .ok_or("cannot read predecessor review body")?;
    let revisions = super::ci::pr_body::reviewed_revisions(body, &headings[0]);
    for revision in &revisions {
        if covers(revision).map_err(|error| {
            format!("cannot verify reviewed revision {revision} for pin {sha}: {error}")
        })? {
            return Ok(true);
        }
    }
    if !revisions.is_empty() {
        return Err(format!(
            "no review names pin {sha}, or a commit it follows only by the predecessor's status and Closeout; approved revisions {} do not cover it; cannot verify review for this pin",
            revisions.join(", ")
        ));
    }
    Ok(false)
}

/// Why the review provider failed, from the first line it wrote to stderr
/// (issue 547): the cause (no pull request, authentication or network),
/// then the provider's own words, redacted (URL userinfo and secret-shaped
/// text) and cut to one bounded terminal line.
fn provider_failure(stderr: &str) -> String {
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    let lower = first.to_ascii_lowercase();
    let cause = if lower.contains("no pull requests found")
        || lower.contains("no open pull requests")
        || lower.contains("could not resolve to a pullrequest")
    {
        "no pull request has this branch as its head"
    } else if [
        "auth",
        "401",
        "403",
        "credential",
        "token",
        "logged in",
        "login",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        "the provider could not authenticate; check `gh auth status`"
    } else if [
        "could not connect",
        "connecting",
        "connection",
        "network",
        "timeout",
        "timed out",
        "dial tcp",
        "no such host",
        "resolve host",
        "unreachable",
    ]
    .iter()
    .any(|word| lower.contains(word))
    {
        "the provider could not reach the host"
    } else {
        "the provider gave another error"
    };
    // A secret-shaped line becomes a marker, then the text is one terminal
    // line. Built from the scanner and git's display escaping, so this gate
    // command does not name the coordination module (ledger_free_cli).
    let urls = codeflow_core::git::remote_query::display_urls(first);
    let flat = urls
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .map(|line| {
            if codeflow_core::hooks::scan::scan_line(line).is_some() {
                "[redacted]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\t', " ");
    let shown = codeflow_core::git::display_message(&flat);
    let shown: String = shown.chars().take(300).collect();
    if shown.is_empty() {
        format!("cannot verify review for this pin: provider failed: {cause} (it wrote no error message)")
    } else {
        format!("cannot verify review for this pin: provider failed: {cause}: {shown}")
    }
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
    let mut errors = tempfile::tempfile().map_err(|error| error.to_string())?;
    let mut command = Command::new("gh");
    command
        .args([
            "pr",
            "view",
            branch,
            "--repo",
            repository,
            "--json",
            "body,headRefName,headRefOid,isCrossRepository,headRepository,headRepositoryOwner",
        ])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|error| error.to_string())?)
        .stderr(errors.try_clone().map_err(|error| error.to_string())?);
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
        if start.elapsed() >= REVIEW_LOOKUP_TIMEOUT {
            break Err(format!(
                "cannot verify review for this pin: provider timed out after {} seconds",
                REVIEW_LOOKUP_TIMEOUT.as_secs()
            ));
        }
        match output.metadata() {
            Ok(metadata) if metadata.len() <= 1_048_576 => {}
            Ok(_) => {
                break Err(
                    "cannot verify review for this pin: review response too large".to_string(),
                );
            }
            Err(error) => {
                break Err(format!(
                    "cannot verify review for this pin: cannot inspect provider response: {error}"
                ));
            }
        }
        if errors
            .metadata()
            .is_ok_and(|metadata| metadata.len() > 1_048_576)
        {
            break Err(
                "cannot verify review for this pin: provider error output too large".to_string(),
            );
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                break Err(format!(
                    "cannot verify review for this pin: cannot wait for provider: {error}"
                ));
            }
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
        let mut said = Vec::new();
        let _ = errors.rewind();
        let _ = (&mut errors).take(4096).read_to_end(&mut said);
        return Err(provider_failure(&String::from_utf8_lossy(&said)));
    }
    output.rewind().map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    output
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("cannot verify review for this pin: review response too large".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse predecessor review: {error}"))
}
