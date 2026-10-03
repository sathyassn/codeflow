//! CLI subcommand handlers. Each module owns its arg types and run function
//! so `main.rs` stays a thin dispatcher (charter §3.1).

pub mod ci;
pub mod delegate;
pub mod doctor;
pub mod estimate;
pub mod git_hook;
pub mod hook;
pub mod ids;
pub mod integrate;
pub mod models;
pub mod new;
pub mod orient;
pub mod policy;
pub mod portal;
pub mod present;
mod push_set;
pub mod recall;
pub mod remote;
pub mod report;
pub mod status;
pub mod test;
pub mod validate;
pub mod work;

use std::path::{Path, PathBuf};

use codeflow_core::hooks::{any_blocking, Violation, HUMAN_OVERRIDE_ENV, INTEGRATE_TOKEN_ENV};
use codeflow_core::registry;

/// `true` when the `codeflow integrate` gate-context token is present in the
/// environment (charter §6.2 / D9).
#[must_use]
pub fn integrate_token_present() -> bool {
    std::env::var(INTEGRATE_TOKEN_ENV).is_ok_and(|v| !v.is_empty())
}

/// `true` when a human's `CODEFLOW_HUMAN_OVERRIDE=1` is present (ADR-0007).
/// Honored only by the git-client hook plane; the git-guard never consults it
/// and blocks in-session attempts to set it.
#[must_use]
pub fn human_override_present() -> bool {
    std::env::var(HUMAN_OVERRIDE_ENV).is_ok_and(|v| v == "1")
}

/// Project root for hook evaluation: the repo containing `start`, or `start`
/// itself when not in a repository (policy then falls back to defaults).
#[must_use]
pub fn project_root(start: &std::path::Path) -> PathBuf {
    codeflow_core::hooks::RepoInfo::discover(start).map_or_else(|| start.to_path_buf(), |i| i.root)
}

/// Print violations and notes for one enforcement plane run in `root`;
/// return the exit code (`block_code` when any violation is block-level,
/// else 0). A stopped operation is recorded in the refusals ledger.
#[must_use]
pub fn render_outcome(
    plane: &str,
    root: &Path,
    violations: &[Violation],
    notes: &[codeflow_core::remedy::Finding],
    block_code: i32,
) -> i32 {
    render_outcome_to(
        &mut std::io::stderr(),
        plane,
        Some(root),
        violations,
        notes,
        block_code,
    )
}

/// [`render_outcome`] into `err` in place of stderr: the doctor's in-process
/// guard canary reads the same lines a harness would (TSK-215). With no
/// `root` the refusal is not recorded in any ledger.
#[must_use]
pub fn render_outcome_to(
    err: &mut dyn std::io::Write,
    plane: &str,
    root: Option<&Path>,
    violations: &[Violation],
    notes: &[codeflow_core::remedy::Finding],
    block_code: i32,
) -> i32 {
    render_findings(err, plane, root, &[], violations, notes, &[], block_code)
}

/// Render a git hook stage: the findings another plane printed for it, its
/// progress lines, then its own notes and violations and the closing line.
pub fn render_stage(
    plane: &str,
    root: &Path,
    report: &codeflow_core::hooks::git_hook::StageReport,
    block_code: i32,
) -> i32 {
    for status in &report.status {
        eprintln!("codeflow {plane}: {status}");
    }
    render_findings(
        &mut std::io::stderr(),
        plane,
        Some(root),
        &report.relayed,
        &report.violations,
        &report.notes,
        &report.refused_by,
        block_code,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_findings(
    err: &mut dyn std::io::Write,
    plane: &str,
    root: Option<&Path>,
    relayed: &[String],
    violations: &[Violation],
    notes: &[codeflow_core::remedy::Finding],
    refused_by: &[String],
    block_code: i32,
) -> i32 {
    for finding in relayed {
        let _ = writeln!(err, "codeflow {plane}: {finding}");
    }
    for note in notes {
        let _ = writeln!(err, "{}", note.line(&format!("codeflow {plane}"), "note"));
    }
    for v in violations {
        let _ = writeln!(err, "{}", v.render(plane));
    }
    let stopped = any_blocking(violations);
    if let Some(root) = root {
        record_refusal(root, plane, violations, refused_by, stopped);
    }
    // The closing line says whether the operation was stopped (R-80).
    if !relayed.is_empty() || !violations.is_empty() || !notes.is_empty() {
        let operation = operation_of(plane);
        let verdict = if stopped { "stopped" } else { "not stopped" };
        let _ = writeln!(err, "codeflow {plane}: {operation} {verdict}");
    }
    let _ = err.flush();
    if stopped {
        block_code
    } else {
        0
    }
}

/// Record a stopped operation in the refusals ledger of the repository at
/// `root` (TSK-149): the plane, the effective level and the blocking rules,
/// never the command. A finding at warn stops nothing and is not written.
/// A pre-push that stopped on its `codeflow ci` check also names the rules
/// that check blocked on. Every run first marks that recording began, so the ceremony report can
/// tell "none recorded" from "not recorded"; a marker that cannot be
/// written is left out silently, since the report then says the refusals
/// are unknown. A refusal that cannot be written is a warning; the
/// operation stays refused.
fn record_refusal(
    root: &Path,
    plane: &str,
    violations: &[Violation],
    refused_by: &[String],
    stopped: bool,
) {
    use codeflow_core::hooks::{rfc3339_utc_now, session_summary, PolicyLevel};
    use codeflow_core::ledger::refusal;
    let Some(info) = codeflow_core::hooks::RepoInfo::discover(root) else {
        return;
    };
    let ledger = info.ledger_dir();
    let now = rfc3339_utc_now();
    let _ = refusal::mark_recording(&ledger, &now);
    if !stopped {
        return;
    }
    let mut rules: Vec<&str> = Vec::new();
    let blocking = violations
        .iter()
        .filter(|v| v.level == PolicyLevel::Block)
        .map(|v| v.rule.as_str());
    for rule in blocking.chain(refused_by.iter().map(String::as_str)) {
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    if let Err(error) = refusal::record(&ledger, plane, &rules, &now) {
        let unwritten = session_summary::unwritten(&ledger, error);
        let path = unwritten.path.display().to_string();
        let finding = codeflow_core::remedy::Finding::new(
            format!("refusal not recorded ({})", unwritten.cause),
            codeflow_core::remedy::REFUSAL_UNRECORDED
                .with(&[("repair", unwritten.repair.words()), ("path", &path)]),
        );
        eprintln!("{}", finding.line(&format!("codeflow {plane}"), "warning"));
    }
}

/// The operation a plane's verdict stops or lets through.
fn operation_of(plane: &str) -> &'static str {
    match plane {
        "pre-push" => "push",
        "pre-commit" | "commit-msg" => "commit",
        "pre-merge-commit" => "merge",
        "reference-transaction" => "ref update",
        _ => "command",
    }
}

/// Cheap per-command registry touch (charter §7 layer 3): when the current
/// working directory is inside an initialized codeflow repo, upsert its
/// entry in `~/.codeflow/registry.json`. Never blocks or fails the actual
/// command — registry maintenance is a side effect, not a gate.
pub fn touch_registry_best_effort() {
    let Some(home) = registry::codeflow_home() else {
        return;
    };
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    if let Some(root) = registry::find_repo_root(&cwd) {
        if let Err(e) = registry::touch_registry(&home, &root) {
            let path = registry::registry_path(&home).display().to_string();
            let finding = codeflow_core::remedy::Finding::new(
                format!("registry touch failed: {e}"),
                codeflow_core::remedy::REGISTRY_UNWRITTEN.with(&[("path", &path)]),
            );
            eprintln!("{}", finding.line("codeflow", "warning"));
        }
    }
}

/// Walk up from the current directory to the nearest git repository root.
/// Falls back to the current directory when none is found (commands that
/// don't need git still work there).
pub(crate) fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.clone();
    loop {
        if dir.join(".git").exists() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return cwd,
        }
    }
}
