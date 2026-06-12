//! `codeflow test` — the generic test gate (charter §3.1, AC #7).

use clap::Args;
use codeflow_core::testing::gate::{GateOutcome, run_gate};

#[derive(Args)]
pub struct TestArgs {
    /// Test mode: full (default) or quick
    #[arg(long, default_value = "full", value_parser = ["full", "quick"])]
    pub mode: String,
}

pub fn run(args: &TestArgs) -> i32 {
    let root = super::repo_root();

    match run_gate(&root, &args.mode) {
        Ok(GateOutcome::NoTargets { reason }) => {
            // AC #7: no stack = loud no-op, exit 0. Loud means unmissable.
            eprintln!("==============================================================");
            eprintln!("WARNING: codeflow test had nothing to run — {reason}.");
            eprintln!("         No tests were executed. This is NOT a green test run.");
            eprintln!("         Configure .codeflow/test-config.json or add a");
            eprintln!("         supported stack (cargo, vitest/jest, go, pytest).");
            eprintln!("==============================================================");
            0
        }
        Ok(GateOutcome::Completed { results, passed }) => {
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
