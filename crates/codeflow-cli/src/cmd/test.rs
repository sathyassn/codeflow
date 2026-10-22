//! `codeflow test` — the generic test gate (charter §3.1, AC #7).

use clap::{ArgGroup, Args, Subcommand};
use codeflow_core::registry::codeflow_home;
use codeflow_core::testing::gate::{
    gate_uses_cargo, run_gate_with_options, CoverageReport, FailureReport, GateOptions, GateOutcome,
};
use codeflow_core::testing::gate_guard::{
    acquire_full_gate_lock, check_cargo_target_dir, lock_dirs, GateLock,
};
use codeflow_core::testing::setup::prompt::TerminalPromptProvider;
use codeflow_core::testing::setup::{self, SetupError, SetupResult};

#[derive(Args)]
pub struct TestArgs {
    /// Optional subcommand. When omitted, `codeflow test` runs the gate.
    #[command(subcommand)]
    pub command: Option<TestCommand>,

    /// Test mode: full (default), quick, or essential. `quick` is an alias
    /// for `essential` — the lighter mode shipped test-configs define.
    #[arg(long, default_value = "full", value_parser = ["full", "quick", "essential"])]
    pub mode: String,

    /// Exit non-zero when no targets executed or candidate evidence skipped a target.
    /// Explicit --only and mode exclusions are not skips.
    /// Use for scripted callers that require execution evidence.
    #[arg(long)]
    pub strict: bool,

    /// Compare the entire candidate delta with a recorded green base.
    /// Without --since, full mode executes every target.
    /// Default: the merge base of HEAD and the task's declared integration target.
    /// An unproven base conservatively runs every target.
    #[arg(long, conflicts_with = "all")]
    pub since: Option<String>,

    /// Run every target, including the binary determinism check at epic close.
    /// With `--only`, the named targets keep that full-strength check.
    #[arg(long)]
    pub all: bool,

    /// Run only these targets and their prerequisites, comma separated or
    /// repeated, to split one gate across parallel CI jobs. A limited run
    /// is never recorded as complete evidence, and an unknown name is
    /// refused before any target starts.
    #[arg(
        long,
        value_name = "TARGET",
        value_delimiter = ',',
        conflicts_with = "since"
    )]
    pub only: Vec<String>,
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// Configure `.codeflow/test-config.json` using root detection, an embedded
    /// template, or an appended target. Safe auto-detection is the default.
    Setup(SetupArgs),
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("setup-action")
        .args(["list_templates", "template", "add_target"])
        .multiple(false)
))]
pub struct SetupArgs {
    /// List the test-config templates embedded in this binary.
    #[arg(long)]
    pub list_templates: bool,

    /// Write an embedded test-config template by name.
    #[arg(long, value_name = "NAME")]
    pub template: Option<String>,

    /// Explicitly replace an existing config when applying --template.
    #[arg(long, requires = "template")]
    pub replace: bool,

    /// Interactively append one target to the existing config.
    #[arg(long)]
    pub add_target: bool,
}

pub fn run(args: &TestArgs) -> i32 {
    if let Some(TestCommand::Setup(setup_args)) = &args.command {
        return run_setup(setup_args);
    }

    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    let _lock = match guard_gate(&root, &args.mode) {
        Ok(lock) => lock,
        Err(message) => {
            eprintln!("codeflow test: refused: {message}");
            return 1;
        }
    };

    match run_gate_with_options(
        &root,
        &args.mode,
        &GateOptions {
            since: args.since.clone(),
            all: args.all,
            only: args.only.clone(),
        },
    ) {
        Ok(GateOutcome::NoTargets { reason }) => {
            // AC #7: no stack = loud no-op. The banner is unmissable in both
            // modes. Default exits 0 (bootstrap/early-setup stays green);
            // `--strict` exits non-zero so a scripted/unattended caller cannot
            // mistake "ran nothing" for "passed".
            eprintln!("==============================================================");
            eprintln!(
                "WARNING: codeflow test had nothing to run — {reason}.",
                reason = crate::cmd::shown(&reason)
            );
            eprintln!("test gate: executed 0; skipped 0; not complete");
            eprintln!("         No tests were executed. This is NOT a green test run.");
            eprintln!("         Configure .codeflow/test-config.json or add a");
            eprintln!("         supported stack (cargo, vitest/jest, go, pytest).");
            if args.strict {
                eprintln!("         --strict: exiting non-zero so no script reads this as green.");
            }
            eprintln!("==============================================================");
            // Distinct from a failed gate (exit 1): 2 == "nothing ran".
            if args.strict {
                2
            } else {
                0
            }
        }
        Ok(GateOutcome::Completed {
            results,
            passed,
            coverage,
            skipped,
            evidence_skipped,
            complete,
        }) => {
            for r in &results {
                let verdict = if r.passed() { "ok" } else { "FAILED" };
                // A target name is checkout text, shown as one line after a
                // fixed label (TSK-388).
                println!(
                    "target {}: {verdict} (exit {}, {} ms)",
                    codeflow_core::git::display_text(&r.name),
                    r.exit_code,
                    r.duration_ms
                );
                if !r.passed() {
                    print_failure(r);
                }
            }
            print_coverage_report(&coverage);
            let verdict = if !passed {
                "FAILED"
            } else if results.is_empty() || !skipped.is_empty() {
                "not complete"
            } else {
                "passed"
            };
            println!(
                "test gate: {verdict} (executed {}; skipped {}{})",
                results.len(),
                skipped.len(),
                evidence_suffix(results.len(), skipped.len(), complete)
            );
            if passed && args.strict && (results.is_empty() || evidence_skipped > 0) {
                2
            } else {
                i32::from(!passed)
            }
        }
        Err(e) => {
            eprintln!("codeflow test: {e}", e = crate::cmd::shown(&e));
            1
        }
    }
}

fn evidence_suffix(executed: usize, skipped: usize, complete: bool) -> &'static str {
    if executed == 0 || skipped != 0 {
        "; not complete"
    } else if complete {
        ""
    } else {
        "; not full-gate evidence"
    }
}

/// TSK-134 guards, checked before any target runs: a gate that runs cargo
/// warns about a `CARGO_TARGET_DIR` outside the worktree, and a full gate
/// takes the machine-wide gate lock (held until the returned value drops) or
/// refuses naming the gate that holds it.
fn guard_gate(root: &std::path::Path, mode: &str) -> Result<Option<GateLock>, String> {
    if gate_uses_cargo(root, mode) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| root.to_path_buf());
        let value = std::env::var_os("CARGO_TARGET_DIR");
        if let Some(warning) = check_cargo_target_dir(root, &cwd, value.as_deref()) {
            eprintln!("codeflow test: warning: {warning}");
        }
    }
    if mode != "full" {
        return Ok(None);
    }
    let dirs = lock_dirs(root, codeflow_home().as_deref()).map_err(|error| {
        format!("gate lock unavailable: cannot discover repository: {error}; repair repository metadata, then rerun `codeflow doctor --check permissions`")
    })?;
    let lock = acquire_full_gate_lock(&dirs, root).map_err(|held| {
        let mut message = held.to_string();
        if message.contains("gate lock unavailable") {
            message.push_str(
                "; `codeflow doctor --check permissions` names each directory the gate must write, \
                 and inside the Claude Code sandbox the one to add to `sandbox.filesystem.allowWrite`",
            );
        }
        message
    })?;
    for note in &lock.notes {
        eprintln!("codeflow test: {note}");
    }
    Ok(Some(lock))
}

/// A failed target's detail: its error, the parsed failure summary FIRST
/// (failing test IDs, `file:line`, counts) so the signal sits above the raw
/// scrollback, then its captured output, every line indented (TSK-388).
fn print_failure(r: &codeflow_core::testing::gate::GateTargetResult) {
    if let Some(err) = &r.error {
        eprintln!("  error: {err}", err = crate::cmd::shown(&err));
    }
    // Parsed failure summary FIRST — failing test IDs + file:line
    // + counts — so the signal sits above the raw scrollback.
    // Absent when no report artifact exists (raw output is then
    // the only detail).
    if let Some(report) = &r.report {
        print_failure_report(report);
    }
    let stdout = r.stdout.trim();
    if !stdout.is_empty() {
        // Relayed output: every line indented (TSK-388).
        eprintln!("  stdout:\n{}", codeflow_core::git::display_relay(stdout));
    }
    let stderr = r.stderr.trim();
    if !stderr.is_empty() {
        eprintln!("  stderr:\n{}", codeflow_core::git::display_relay(stderr));
    }
}

/// Print the parsed failure summary (counts + failing test IDs + `file:line`)
/// for a failed target, above its raw stdout/stderr.
fn print_failure_report(report: &FailureReport) {
    eprintln!(
        "  {} passed, {} failed, {} skipped ({} total)",
        report.passed, report.failed, report.skipped, report.total
    );
    for f in &report.failures {
        match &f.location {
            Some(loc) => eprintln!(
                "  FAILED {} ({})",
                codeflow_core::git::display_text(&f.id),
                crate::cmd::shown(loc)
            ),
            None => eprintln!("  FAILED {}", codeflow_core::git::display_text(&f.id)),
        }
        if let Some(msg) = &f.message {
            eprintln!("         {}", crate::cmd::shown(msg));
        }
    }
}

/// `codeflow test setup`: deterministic, offline config mechanics over root
/// detection, release-embedded templates, and explicit target append.
fn run_setup(args: &SetupArgs) -> i32 {
    let root = match super::repo_root() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("codeflow: {error}", error = crate::cmd::shown(&error));
            return 2;
        }
    };
    if args.list_templates {
        for name in crate::embedded::test_template_names() {
            let content = match crate::embedded::read_test_template(&name) {
                Ok(Some(content)) => content,
                Ok(None) => {
                    eprintln!("codeflow test setup: listed template {name} is absent");
                    return 1;
                }
                Err(error) => {
                    eprintln!(
                        "codeflow test setup: {error}",
                        error = crate::cmd::shown(&error)
                    );
                    return 1;
                }
            };
            let description = setup::template_description(&content);
            println!("{name}  — {description}");
        }
        return 0;
    }

    if let Some(name) = &args.template {
        let loaded = match crate::embedded::read_test_template(name) {
            Ok(loaded) => loaded,
            Err(error) => {
                eprintln!(
                    "codeflow test setup: {error}",
                    error = crate::cmd::shown(&error)
                );
                return 1;
            }
        };
        let Some(content) = loaded else {
            eprintln!(
                "codeflow test setup: template {name:?} not found; run `codeflow test setup --list-templates`"
            );
            return 1;
        };
        return match setup::run_template_content(&root, name, &content, args.replace) {
            Ok(SetupResult::Written) => {
                println!("Next: run `codeflow test --mode essential` to try it.");
                0
            }
            Ok(SetupResult::WrittenNoTargets | SetupResult::Aborted) => 0,
            Err(SetupError::ConfigExists(path)) => {
                eprintln!(
                    "codeflow test setup: {} already exists; inspect it or rerun this template with --replace",
                    codeflow_core::git::display_path(&path)
                );
                1
            }
            Err(error) => {
                eprintln!(
                    "codeflow test setup: {error}",
                    error = crate::cmd::shown(&error)
                );
                1
            }
        };
    }

    if args.add_target {
        return match setup::run_add_target(&root, &TerminalPromptProvider) {
            Ok(SetupResult::Written | SetupResult::WrittenNoTargets | SetupResult::Aborted) => 0,
            Err(error) => {
                eprintln!(
                    "codeflow test setup: {error}",
                    error = crate::cmd::shown(&error)
                );
                1
            }
        };
    }

    match setup::run_auto(&root) {
        Ok(SetupResult::Written) => {
            println!("Next: run `codeflow test --mode essential` to try it.");
            0
        }
        // Zero-detection: run_auto already printed the honest "no stack
        // detected" line; nothing to run yet, so don't suggest it.
        // (`run_auto` never aborts, but the arm stays exhaustive.)
        Ok(SetupResult::WrittenNoTargets | SetupResult::Aborted) => 0,
        Err(SetupError::ConfigExists(path)) => {
            // Idempotent: a real, populated config is never overwritten.
            println!(
                "{} already has targets — leaving it unchanged.",
                codeflow_core::git::display_path(&path)
            );
            0
        }
        Err(e) => {
            eprintln!("codeflow test setup: {e}", e = crate::cmd::shown(&e));
            1
        }
    }
}

/// Print the coverage summary. Configured thresholds and missing required data
/// contribute to the gate verdict in the testing engine.
fn print_coverage_report(reports: &[CoverageReport]) {
    if reports.is_empty() {
        return;
    }
    println!("coverage:");
    for r in reports {
        if let Some(note) = &r.note {
            println!(
                "  {}: {}",
                codeflow_core::git::display_text(&r.target),
                codeflow_core::git::display_line(note)
            );
            continue;
        }
        let overall = r
            .overall_percent
            .map_or_else(|| "n/a".to_string(), |p| format!("{p:.1}%"));
        let exceptions = if r.exceptions_applied > 0 {
            format!(" ({} exception(s))", r.exceptions_applied)
        } else {
            String::new()
        };
        println!(
            "  {}: {overall} overall — {} pass, {} fail{exceptions}",
            codeflow_core::git::display_text(&r.target),
            r.thresholds_passed,
            r.thresholds_failed,
        );
        for (file, pct, threshold) in &r.failing_files {
            let file = codeflow_core::git::display_line(file);
            println!("    below threshold: {file} {pct:.1}% < {threshold}");
        }
    }
}
