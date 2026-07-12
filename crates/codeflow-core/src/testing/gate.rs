//! The test gate: one entry point shared by `codeflow test` and `integrate`.
//!
//! Resolution order (charter §3.1, AC #7):
//! 1. `.codeflow/test-config.json` present → run its targets as configured.
//! 2. No config → runtime stack detection ([`crate::testing::setup::detect`]);
//!    detected targets run with best-guess commands.
//! 3. Nothing detected → [`GateOutcome::NoTargets`] — the caller MUST surface
//!    this loudly (it is a no-op, not a green run), but it is not an error.

use std::path::Path;

use crate::testing::config::{load_test_config, TargetConfig};
use crate::testing::coverage::FileCoverage;
use crate::testing::error::TestingError;
use crate::testing::report::{CanonicalTestReport, CtrfStatus};
use crate::testing::runner::{run_all_targets, TargetRunResult};
use crate::testing::setup::detect::detect_stacks;
use crate::testing::validation;

/// Relative path of the test configuration inside a project.
pub const TEST_CONFIG_PATH: &str = ".codeflow/test-config.json";

/// Result of one target's run through the gate.
#[derive(Debug, Clone)]
pub struct GateTargetResult {
    pub name: String,
    /// Process exit code; `-1` when the command could not run at all.
    pub exit_code: i32,
    pub duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
    /// Set when the target failed before producing an exit code
    /// (spawn failure, missing mode, panic).
    pub error: Option<String>,
    /// Structured failure detail parsed from the target's JUnit/CTRF report
    /// artifact, when one was configured and produced. `None` when the target
    /// declares no report or the artifact is missing/unparseable — callers then
    /// fall back to the raw stdout/stderr scrollback. Populated for passing
    /// targets too (its `failures` list is then empty) so `integrate`/hook
    /// consumers get counts regardless of verdict.
    pub report: Option<FailureReport>,
}

impl GateTargetResult {
    /// Whether this target passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.exit_code == 0 && self.error.is_none()
    }
}

/// Pass/fail/skip counts plus the individual failing tests parsed from a
/// target's report artifact. Surfaced above the raw scrollback so a failed run
/// names *what* failed (and where) instead of only dumping tool output.
#[derive(Debug, Clone)]
pub struct FailureReport {
    pub passed: u64,
    pub failed: u64,
    pub skipped: u64,
    pub total: u64,
    /// One entry per failing test, in report order.
    pub failures: Vec<FailedTest>,
}

/// A single failing test extracted from the report.
#[derive(Debug, Clone)]
pub struct FailedTest {
    /// Test identifier: `suite::name` when a suite is known, else `name`.
    pub id: String,
    /// Best-effort `file:line` pulled from the failure message or trace, when
    /// the tool emitted a recognizable source location.
    pub location: Option<String>,
    /// First line of the failure message, when present.
    pub message: Option<String>,
}

/// Report-only coverage summary for one target (charter §3.1 coverage audit).
///
/// This never affects the gate verdict — it is surfaced so a full-mode run
/// makes coverage visible instead of silently dropping it. Threshold gating is
/// a future step; today the numbers are informational.
#[derive(Debug, Clone)]
pub struct CoverageReport {
    pub target: String,
    /// Overall line coverage percentage, or `None` when no coverage data was
    /// collected (missing or empty artifact).
    pub overall_percent: Option<f64>,
    pub thresholds_passed: usize,
    pub thresholds_failed: usize,
    pub exceptions_applied: usize,
    /// `(file, coverage_percent, threshold)` for each failing, non-excepted file.
    pub failing_files: Vec<(String, f64, u32)>,
    /// Set when coverage is configured but no usable data was produced.
    pub note: Option<String>,
}

/// Outcome of running the test gate.
#[derive(Debug)]
pub enum GateOutcome {
    /// Nothing to run: no config targets and no detectable stack (or no
    /// target defines the requested mode). Callers must report this loudly.
    NoTargets { reason: String },
    /// Targets ran; `passed` is the gate verdict. `coverage` is report-only
    /// (populated for full-mode runs of targets that configure coverage) and
    /// does not influence `passed`.
    Completed {
        results: Vec<GateTargetResult>,
        passed: bool,
        coverage: Vec<CoverageReport>,
    },
}

impl GateOutcome {
    /// Whether the gate allows proceeding (no-targets counts as allowed —
    /// AC #7: loud no-op, exit 0).
    #[must_use]
    pub fn allows_proceed(&self) -> bool {
        match self {
            Self::NoTargets { .. } => true,
            Self::Completed { passed, .. } => *passed,
        }
    }
}

/// Run the test gate for a project directory in the given mode
/// (`full` or `quick`).
///
/// # Errors
///
/// Returns `TestingError` when a present config file cannot be loaded —
/// a broken config is a failure, never a silent skip.
pub fn run_gate(project_dir: &Path, mode: &str) -> Result<GateOutcome, TestingError> {
    let config_path = project_dir.join(TEST_CONFIG_PATH);

    let (targets, parallel, fail_fast, effective_mode) = if config_path.exists() {
        let config = load_test_config(&config_path)?;
        if config.targets.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("{TEST_CONFIG_PATH} defines no targets"),
            });
        }
        let effective = resolve_mode(mode, &config.targets);
        (
            config.targets,
            config.execution.parallel,
            config.execution.fail_fast,
            effective,
        )
    } else {
        let detected = detect_stacks(project_dir);
        if detected.is_empty() {
            return Ok(GateOutcome::NoTargets {
                reason: format!("no {TEST_CONFIG_PATH} and no test stack detected"),
            });
        }
        let targets: Vec<TargetConfig> = detected.into_iter().map(|d| d.config).collect();
        let effective = resolve_mode(mode, &targets);
        (targets, false, false, effective)
    };

    let raw = run_all_targets(
        &targets,
        &effective_mode,
        project_dir,
        parallel,
        fail_fast,
        &[],
        &[],
        &[],
        &[],
    );

    if raw.is_empty() {
        return Ok(GateOutcome::NoTargets {
            reason: format!("no enabled target defines mode \"{effective_mode}\""),
        });
    }

    // Report-only coverage: for a full run, parse each covered target's artifact
    // and summarize it. This never gates (that is a future step) — it just makes
    // coverage visible rather than silently discarded.
    let coverage = if effective_mode == "full" {
        let ok_runs: Vec<TargetRunResult> =
            raw.iter().filter_map(|r| r.as_ref().ok().cloned()).collect();
        collect_coverage_reports(&targets, &ok_runs)
    } else {
        Vec::new()
    };

    let results: Vec<GateTargetResult> = raw
        .into_iter()
        .map(|r| match r {
            Ok(run) => {
                // Parse the target's report artifact (JUnit/CTRF) so a failed run
                // surfaces failing test IDs + file:line above the raw scrollback.
                // Best-effort: a missing config, missing artifact, or parse error
                // yields None and the caller falls back to raw output.
                let report = targets
                    .iter()
                    .find(|t| t.name == run.target_name)
                    .and_then(|t| validation::parse_target_report(t, &run).ok().flatten())
                    .map(|rep| summarize_report(&rep));
                GateTargetResult {
                    name: run.target_name,
                    exit_code: run.exit_code,
                    duration_ms: run.duration_ms,
                    stdout: run.stdout,
                    stderr: run.stderr,
                    error: None,
                    report,
                }
            }
            Err(e) => GateTargetResult {
                name: "(unrunnable)".to_string(),
                exit_code: -1,
                duration_ms: 0,
                stdout: String::new(),
                stderr: String::new(),
                error: Some(e.to_string()),
                report: None,
            },
        })
        .collect();

    let passed = results.iter().all(GateTargetResult::passed);
    Ok(GateOutcome::Completed {
        results,
        passed,
        coverage,
    })
}

/// Build report-only coverage summaries for the targets that ran successfully
/// and configure coverage. Reuses the post-test parsing/threshold machinery in
/// [`crate::testing::validation`]. Never blocks: a missing or unparseable
/// artifact yields a note, not an error.
fn collect_coverage_reports(
    targets: &[TargetConfig],
    runs: &[TargetRunResult],
) -> Vec<CoverageReport> {
    let mut reports = Vec::new();
    for run in runs {
        let Some(target) = targets.iter().find(|t| t.name == run.target_name) else {
            continue;
        };
        if target.coverage.is_none() {
            continue;
        }

        let coverages = match validation::parse_target_coverage(target, run) {
            Ok(c) => c,
            Err(e) => {
                reports.push(CoverageReport {
                    target: run.target_name.clone(),
                    overall_percent: None,
                    thresholds_passed: 0,
                    thresholds_failed: 0,
                    exceptions_applied: 0,
                    failing_files: Vec::new(),
                    note: Some(format!("coverage artifact could not be parsed: {e}")),
                });
                continue;
            }
        };
        if coverages.is_empty() {
            reports.push(CoverageReport {
                target: run.target_name.clone(),
                overall_percent: None,
                thresholds_passed: 0,
                thresholds_failed: 0,
                exceptions_applied: 0,
                failing_files: Vec::new(),
                note: Some("no coverage data collected".to_string()),
            });
            continue;
        }

        let total_found: u64 = coverages.iter().map(|c| c.lines_found).sum();
        let total_hit: u64 = coverages.iter().map(|c| c.lines_hit).sum();
        let overall_percent =
            (total_found > 0).then(|| FileCoverage::compute_percent(total_found, total_hit));

        let thresholds = validation::evaluate_target_thresholds(target, &coverages, &[]);
        let thresholds_passed = thresholds.iter().filter(|r| r.pass).count();
        let thresholds_failed = thresholds
            .iter()
            .filter(|r| !r.pass && !r.exception_applied)
            .count();
        let exceptions_applied = thresholds.iter().filter(|r| r.exception_applied).count();
        let failing_files = thresholds
            .iter()
            .filter(|r| !r.pass && !r.exception_applied)
            .map(|r| (r.file.clone(), r.coverage_percent, r.threshold))
            .collect();

        reports.push(CoverageReport {
            target: run.target_name.clone(),
            overall_percent,
            thresholds_passed,
            thresholds_failed,
            exceptions_applied,
            failing_files,
            note: None,
        });
    }
    reports
}

/// Condense a parsed report into the counts + failing-test detail the gate
/// surfaces. Only failing tests are listed; passing/skipped ones are counted.
fn summarize_report(report: &CanonicalTestReport) -> FailureReport {
    let summary = &report.results.summary;
    let failures = report
        .results
        .tests
        .iter()
        .filter(|t| t.status == CtrfStatus::Failed)
        .map(|t| {
            let id = match &t.suite {
                Some(suite) if !suite.is_empty() => format!("{suite}::{}", t.name),
                _ => t.name.clone(),
            };
            let location = t
                .message
                .as_deref()
                .and_then(extract_location)
                .or_else(|| t.trace.as_deref().and_then(extract_location));
            let message = t.message.as_deref().and_then(first_line);
            FailedTest {
                id,
                location,
                message,
            }
        })
        .collect();
    FailureReport {
        passed: summary.passed,
        failed: summary.failed,
        skipped: summary.skipped,
        total: summary.total,
        failures,
    }
}

/// Best-effort extraction of a `file:line` source location from a failure
/// message or trace (e.g. `src/lib.rs:42`, `tests/test_api.py:10`). Returns the
/// first `path.ext:line` token found, or `None` when the tool emitted none.
fn extract_location(text: &str) -> Option<String> {
    // A path segment (word chars, `.`, `/`, `\`, `-`), then `.ext`, then `:line`.
    let re = regex::Regex::new(r"[\w./\\-]+\.[A-Za-z]\w*:\d+").ok()?;
    re.find(text).map(|m| m.as_str().to_string())
}

/// First non-empty-trimmed line of a message, or `None` when it is blank.
fn first_line(text: &str) -> Option<String> {
    let line = text.lines().next().unwrap_or("").trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// Resolves the requested mode against the modes the targets actually define.
///
/// `quick` is an alias for `essential`. Every shipped test-config template and
/// runtime stack detection defines `essential`/`full`, never `quick`, while the
/// CLI default and the pre-push gate ([`crate::hooks::git_hook`]) speak
/// `quick`. When the targets define `essential` but no `quick`, `quick`
/// resolves to `essential`; otherwise the requested mode is passed through
/// unchanged, so an unsatisfiable mode still surfaces as a loud no-op
/// ([`GateOutcome::NoTargets`]) rather than silently mapping onto another mode.
fn resolve_mode(requested: &str, targets: &[TargetConfig]) -> String {
    let defines = |mode: &str| targets.iter().any(|t| t.modes.contains_key(mode));
    if requested == "quick" && defines("essential") && !defines("quick") {
        "essential".to_string()
    } else {
        requested.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_config(dir: &Path, targets_json: &str) {
        let codeflow = dir.join(".codeflow");
        std::fs::create_dir_all(&codeflow).unwrap();
        std::fs::write(
            codeflow.join("test-config.json"),
            format!(r#"{{"schema_version": "1.0", "targets": {targets_json}}}"#),
        )
        .unwrap();
    }

    fn echo_target(name: &str, command: &str) -> String {
        format!(
            r#"{{"name": "{name}", "runner": "custom", "modes": {{"full": {{"command": "{command}"}}, "quick": {{"command": "{command}"}}}}}}"#
        )
    }

    #[test]
    fn no_config_no_stack_is_loud_no_op() {
        let dir = tempfile::tempdir().unwrap();
        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::NoTargets { reason } => {
                assert!(reason.contains("no test stack detected"), "{reason}");
            }
            GateOutcome::Completed { .. } => panic!("expected NoTargets"),
        }
    }

    #[test]
    fn no_targets_allows_proceed() {
        let outcome = GateOutcome::NoTargets {
            reason: "x".into(),
        };
        assert!(outcome.allows_proceed());
    }

    #[test]
    fn configured_passing_target_runs_green() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("ok", "exit 0")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed { results, passed, .. } => {
                assert!(passed);
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].name, "ok");
                assert!(results[0].passed());
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn configured_failing_target_fails_gate() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("bad", "exit 3")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        assert!(!outcome.allows_proceed());
        match outcome {
            GateOutcome::Completed { results, passed, .. } => {
                assert!(!passed);
                assert_eq!(results[0].exit_code, 3);
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn failed_target_surfaces_parsed_report_above_raw() {
        // A failing target with a JUnit report artifact must surface parsed
        // counts + failing test IDs + file:line through the gate result, so the
        // CLI can print them above the raw scrollback (and integrate/hook
        // consumers get structured failures).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("junit.xml"),
            r#"<testsuites><testsuite name="mysuite" tests="2" failures="1">
                <testcase name="test_ok" classname="mysuite" time="0.1"/>
                <testcase name="test_bad" classname="mysuite" time="0.2">
                    <failure message="assertion failed at src/lib.rs:42">trace</failure>
                </testcase>
               </testsuite></testsuites>"#,
        )
        .unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "t", "runner": "custom",
                "report": {"format": "junit", "path": "junit.xml"},
                "modes": {"full": {"command": "exit 1"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                results, passed, ..
            } => {
                assert!(!passed);
                let rep = results[0]
                    .report
                    .as_ref()
                    .expect("failed target must carry a parsed report");
                assert_eq!(rep.passed, 1);
                assert_eq!(rep.failed, 1);
                assert_eq!(rep.failures.len(), 1);
                assert!(rep.failures[0].id.contains("test_bad"), "{:?}", rep.failures[0]);
                assert_eq!(rep.failures[0].location.as_deref(), Some("src/lib.rs:42"));
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn failed_target_without_report_falls_back_to_raw() {
        // No report block configured → report is None; the CLI falls back to
        // raw stdout/stderr. Guards the fallback path.
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("bad", "exit 3")));

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed { results, .. } => {
                assert!(results[0].report.is_none(), "no report artifact → None");
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn empty_targets_config_is_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "[]");

        let outcome = run_gate(dir.path(), "full").unwrap();
        assert!(matches!(outcome, GateOutcome::NoTargets { .. }));
    }

    #[test]
    fn broken_config_is_an_error_not_a_skip() {
        let dir = tempfile::tempdir().unwrap();
        let codeflow = dir.path().join(".codeflow");
        std::fs::create_dir_all(&codeflow).unwrap();
        std::fs::write(codeflow.join("test-config.json"), "{ not json").unwrap();

        let result = run_gate(dir.path(), "full");
        assert!(result.is_err(), "broken config must fail loud");
    }

    #[test]
    fn missing_mode_in_config_is_no_targets() {
        let dir = tempfile::tempdir().unwrap();
        // Target only defines "full"; ask for "quick".
        write_config(
            dir.path(),
            r#"[{"name": "only-full", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::NoTargets { reason } => {
                assert!(reason.contains("quick"), "{reason}");
            }
            GateOutcome::Completed { .. } => panic!("expected NoTargets"),
        }
    }

    fn essential_full_target(name: &str, command: &str) -> String {
        format!(
            r#"{{"name": "{name}", "runner": "custom", "modes": {{"essential": {{"command": "{command}"}}, "full": {{"command": "{command}"}}}}}}"#
        )
    }

    #[test]
    fn quick_resolves_to_essential_on_template_config() {
        // Shipped templates (and stack detection) define `essential`/`full`,
        // never `quick`. A `--mode quick` run must resolve to `essential` and
        // actually execute — the pre-push gate depends on this.
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            &format!("[{}]", essential_full_target("ok", "exit 0")),
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::Completed { results, passed, .. } => {
                assert!(passed, "quick must run the essential command set");
                assert_eq!(results.len(), 1);
                assert!(results[0].passed());
            }
            GateOutcome::NoTargets { reason } => {
                panic!("quick must resolve to essential and run, got NoTargets: {reason}")
            }
        }
    }

    #[test]
    fn essential_mode_is_accepted_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            &format!("[{}]", essential_full_target("ok", "exit 0")),
        );

        let outcome = run_gate(dir.path(), "essential").unwrap();
        assert!(
            matches!(outcome, GateOutcome::Completed { passed: true, .. }),
            "essential mode must run, got {outcome:?}"
        );
    }

    #[test]
    fn quick_kept_when_config_defines_quick() {
        // A config that actually defines `quick` keeps it — no aliasing.
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), &format!("[{}]", echo_target("ok", "exit 0")));

        let outcome = run_gate(dir.path(), "quick").unwrap();
        assert!(
            matches!(outcome, GateOutcome::Completed { passed: true, .. }),
            "explicit quick mode must run its own command, got {outcome:?}"
        );
    }

    #[test]
    fn quick_without_essential_is_loud_no_op() {
        // Only `full` is defined: `quick` has nothing to alias onto, so it
        // stays `quick` and surfaces loudly rather than silently mapping.
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "only-full", "runner": "custom", "modes": {"full": {"command": "exit 0"}}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::NoTargets { reason } => assert!(reason.contains("quick"), "{reason}"),
            GateOutcome::Completed { .. } => panic!("expected loud NoTargets, got Completed"),
        }
    }

    #[test]
    fn resolve_mode_aliases_quick_only_when_unambiguous() {
        let parse = |modes: &[&str]| -> TargetConfig {
            let entries: Vec<String> = modes
                .iter()
                .map(|m| format!(r#""{m}": {{"command": "exit 0"}}"#))
                .collect();
            serde_json::from_str(&format!(
                r#"{{"name": "t", "runner": "custom", "modes": {{{}}}}}"#,
                entries.join(", ")
            ))
            .unwrap()
        };

        let essential_full = [parse(&["essential", "full"])];
        assert_eq!(resolve_mode("quick", &essential_full), "essential");
        assert_eq!(resolve_mode("full", &essential_full), "full");
        assert_eq!(resolve_mode("essential", &essential_full), "essential");

        assert_eq!(resolve_mode("quick", &[parse(&["quick", "full"])]), "quick");
        assert_eq!(resolve_mode("quick", &[parse(&["full"])]), "quick");
    }

    #[test]
    fn full_mode_reports_coverage_but_does_not_gate_on_it() {
        // A target with coverage config + an lcov artifact below threshold must
        // still PASS the gate (report-only) while surfacing the shortfall.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("cov.lcov"),
            "SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n",
        )
        .unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "cov", "runner": "custom",
                "modes": {"full": {"command": "exit 0"}, "essential": {"command": "exit 0"}},
                "coverage": {"format": "lcov", "path": "cov.lcov",
                             "rules": [{"scope": "per_file", "minimum": 85}]}}]"#,
        );

        let outcome = run_gate(dir.path(), "full").unwrap();
        match outcome {
            GateOutcome::Completed {
                passed, coverage, ..
            } => {
                assert!(passed, "coverage is report-only; the gate must still pass");
                assert_eq!(coverage.len(), 1);
                let cov = &coverage[0];
                assert_eq!(cov.target, "cov");
                assert_eq!(cov.overall_percent, Some(50.0));
                assert_eq!(cov.thresholds_failed, 1);
                assert_eq!(cov.failing_files.len(), 1);
                assert_eq!(cov.failing_files[0].0, "src/lib.rs");
            }
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn quick_mode_collects_no_coverage() {
        // Coverage is a full-mode concern; quick(->essential) runs skip it.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("cov.lcov"),
            "SF:src/lib.rs\nDA:1,1\nend_of_record\n",
        )
        .unwrap();
        write_config(
            dir.path(),
            r#"[{"name": "cov", "runner": "custom",
                "modes": {"full": {"command": "exit 0"}, "essential": {"command": "exit 0"}},
                "coverage": {"format": "lcov", "path": "cov.lcov",
                             "rules": [{"scope": "per_file", "minimum": 85}]}}]"#,
        );

        let outcome = run_gate(dir.path(), "quick").unwrap();
        match outcome {
            GateOutcome::Completed { coverage, .. } => assert!(coverage.is_empty()),
            GateOutcome::NoTargets { reason } => panic!("expected run, got NoTargets: {reason}"),
        }
    }

    #[test]
    fn detected_stack_without_config_runs() {
        // A pyproject.toml with pytest makes detection fire; the pytest
        // command itself will fail in this environment, which is fine —
        // the point is that the gate RUNS rather than no-ops.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.pytest.ini_options]\nminversion = \"6.0\"\n",
        )
        .unwrap();

        let outcome = run_gate(dir.path(), "full").unwrap();
        assert!(
            matches!(outcome, GateOutcome::Completed { .. }),
            "detected stack must run, got {outcome:?}"
        );
    }
}
