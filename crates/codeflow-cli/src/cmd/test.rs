//! `codeflow test` — the generic test gate (charter §3.1, AC #7).

use clap::{Args, Subcommand};
use codeflow_core::testing::gate::{run_gate, CoverageReport, FailureReport, GateOutcome};
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

    /// Treat "nothing to run" as a failure (exit non-zero) instead of a loud
    /// no-op. For scripted/unattended callers — CI, the pipeline verify gate —
    /// where a run that executed zero tests must NOT read as green. The default
    /// (no `--strict`) keeps the loud-no-op-exit-0 behavior so the
    /// bootstrap/early-setup path of a brand-new repo without tests is not
    /// broken.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// Detect the stack and write `.codeflow/test-config.json` (offline,
    /// idempotent). Never overwrites a populated config.
    Setup,
}

pub fn run(args: &TestArgs) -> i32 {
    if matches!(args.command, Some(TestCommand::Setup)) {
        return run_setup();
    }

    let root = super::repo_root();

    match run_gate(&root, &args.mode) {
        Ok(GateOutcome::NoTargets { reason }) => {
            // AC #7: no stack = loud no-op. The banner is unmissable in both
            // modes. Default exits 0 (bootstrap/early-setup stays green);
            // `--strict` exits non-zero so a scripted/unattended caller cannot
            // mistake "ran nothing" for "passed".
            eprintln!("==============================================================");
            eprintln!("WARNING: codeflow test had nothing to run — {reason}.");
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
        }) => {
            for r in &results {
                let verdict = if r.passed() { "ok" } else { "FAILED" };
                println!(
                    "{}: {verdict} (exit {}, {} ms)",
                    r.name, r.exit_code, r.duration_ms
                );
                if !r.passed() {
                    if let Some(err) = &r.error {
                        eprintln!("  error: {err}");
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
                        eprintln!("  stdout:\n{stdout}");
                    }
                    let stderr = r.stderr.trim();
                    if !stderr.is_empty() {
                        eprintln!("  stderr:\n{stderr}");
                    }
                }
            }
            print_coverage_report(&coverage);
            if passed {
                println!("test gate: passed ({} target(s))", results.len());
                0
            } else {
                eprintln!("test gate: FAILED");
                1
            }
        }
        Err(e) => {
            eprintln!("codeflow test: {e}");
            1
        }
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
            Some(loc) => eprintln!("  FAILED {} ({loc})", f.id),
            None => eprintln!("  FAILED {}", f.id),
        }
        if let Some(msg) = &f.message {
            eprintln!("         {msg}");
        }
    }
}

/// `codeflow test setup`: deterministic, offline stack detection that writes
/// `.codeflow/test-config.json`. Idempotent — a populated config is left
/// untouched (reported as success, not an error).
fn run_setup() -> i32 {
    let root = super::repo_root();
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
                path.display()
            );
            0
        }
        Err(e) => {
            eprintln!("codeflow test setup: {e}");
            1
        }
    }
}

/// Print the report-only coverage summary. Explicitly does NOT influence the
/// gate verdict — threshold gating is a future step (see
/// `codeflow_core::testing::gate::CoverageReport`).
fn print_coverage_report(reports: &[CoverageReport]) {
    if reports.is_empty() {
        return;
    }
    println!("coverage (report-only — does not affect the gate verdict):");
    for r in reports {
        if let Some(note) = &r.note {
            println!("  {}: {note}", r.target);
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
            r.target, r.thresholds_passed, r.thresholds_failed,
        );
        for (file, pct, threshold) in &r.failing_files {
            println!("    below threshold: {file} {pct:.1}% < {threshold}");
        }
    }
}
