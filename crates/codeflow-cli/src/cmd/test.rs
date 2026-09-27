//! `codeflow test` — the generic test gate (charter §3.1, AC #7).

use clap::{ArgGroup, Args, Subcommand};
use codeflow_core::registry::codeflow_home;
use codeflow_core::testing::gate::{
    gate_uses_cargo, run_gate, CoverageReport, FailureReport, GateOutcome,
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

    let root = super::repo_root();
    let _lock = match guard_gate(&root, &args.mode) {
        Ok(lock) => lock,
        Err(message) => {
            eprintln!("codeflow test: refused: {message}");
            return 1;
        }
    };

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

/// TSK-134 guards, checked before any target runs: a gate that runs cargo
/// refuses a `CARGO_TARGET_DIR` outside the worktree, and a full gate takes
/// the machine-wide gate lock (held until the returned value drops) or
/// refuses naming the gate that holds it.
fn guard_gate(root: &std::path::Path, mode: &str) -> Result<Option<GateLock>, String> {
    if gate_uses_cargo(root, mode) {
        let cwd = std::env::current_dir().unwrap_or_else(|_| root.to_path_buf());
        check_cargo_target_dir(root, &cwd, std::env::var_os("CARGO_TARGET_DIR").as_deref())?;
    }
    if mode != "full" {
        return Ok(None);
    }
    let dirs = lock_dirs(root, codeflow_home().as_deref());
    let lock = acquire_full_gate_lock(&dirs, root).map_err(|held| held.to_string())?;
    for note in &lock.notes {
        eprintln!("codeflow test: {note}");
    }
    Ok(Some(lock))
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

/// `codeflow test setup`: deterministic, offline config mechanics over root
/// detection, release-embedded templates, and explicit target append.
fn run_setup(args: &SetupArgs) -> i32 {
    let root = super::repo_root();
    if args.list_templates {
        for name in crate::embedded::test_template_names() {
            let description = crate::embedded::read_test_template(&name)
                .map(|content| setup::template_description(&content))
                .unwrap_or_default();
            println!("{name}  — {description}");
        }
        return 0;
    }

    if let Some(name) = &args.template {
        let Some(content) = crate::embedded::read_test_template(name) else {
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
                    path.display()
                );
                1
            }
            Err(error) => {
                eprintln!("codeflow test setup: {error}");
                1
            }
        };
    }

    if args.add_target {
        return match setup::run_add_target(&root, &TerminalPromptProvider) {
            Ok(SetupResult::Written | SetupResult::WrittenNoTargets | SetupResult::Aborted) => 0,
            Err(error) => {
                eprintln!("codeflow test setup: {error}");
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

/// Print the coverage summary. Configured thresholds and missing required data
/// contribute to the gate verdict in the testing engine.
fn print_coverage_report(reports: &[CoverageReport]) {
    if reports.is_empty() {
        return;
    }
    println!("coverage:");
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
