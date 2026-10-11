//! Post-test pipeline: parse reports, evaluate coverage, persist results.
//!
//! Coordinates the downstream steps after `runner::run_all_targets()`:
//! report parsing, coverage parsing, threshold evaluation, and report
//! persistence.

use std::io::Write;
use std::path::Path;

use crate::testing::config::{CoverageFormat, ReportFormat, TargetConfig};
use crate::testing::coverage::{self, FileCoverage};
use crate::testing::error::TestingError;
use crate::testing::report::CanonicalTestReport;
use crate::testing::runner::TargetRunResult;
use crate::testing::threshold;

/// Parsed post-test data for a single target.
pub struct TargetPostData {
    pub name: String,
    pub mode: String,
    pub run_result: TargetRunResult,
    pub report: Option<CanonicalTestReport>,
    pub file_coverages: Vec<FileCoverage>,
    pub threshold_results: Vec<threshold::ThresholdResult>,
    pub target_config: TargetConfig,
}

/// Parse test report for a single target.
///
/// Reads the report file produced by the test run and converts it to the
/// canonical CTRF-shaped model. Handles `JUnit` `derive_from` conversion.
///
/// # Errors
///
/// Returns `TestingError` if the report file cannot be read or parsed.
pub fn parse_target_report(
    target: &TargetConfig,
    run_result: &TargetRunResult,
) -> Result<Option<CanonicalTestReport>, TestingError> {
    let Some(report_config) = &target.report else {
        return Ok(None);
    };

    let report_path = match &run_result.report_path {
        Some(p) if p.exists() => p,
        _ => return Ok(None),
    };

    // If derive_from is "junit" or format is Junit, always parse as JUnit and convert
    let is_junit_source = report_config.derive_from.as_deref() == Some("junit")
        || report_config.format == ReportFormat::Junit;

    let report = if is_junit_source {
        let junit = crate::testing::report::junit::parse_junit(report_path)?;
        Some(CanonicalTestReport::from(junit))
    } else {
        Some(crate::testing::report::ctrf::parse_ctrf(report_path)?)
    };

    Ok(report)
}

/// Parse coverage data for a single target.
///
/// Reads the coverage artifact and dispatches to the correct parser
/// based on the target's `coverage.format`.
///
/// # Errors
///
/// Returns `TestingError` if the coverage file cannot be read or parsed.
pub fn parse_target_coverage(
    target: &TargetConfig,
    run_result: &TargetRunResult,
) -> Result<Vec<FileCoverage>, TestingError> {
    let Some(cov_config) = &target.coverage else {
        return Ok(Vec::new());
    };

    let cov_path = match &run_result.coverage_path {
        Some(p) if p.exists() => p,
        _ => return Ok(Vec::new()),
    };

    let mut coverages = match cov_config.format {
        CoverageFormat::Lcov => coverage::lcov::parse_lcov(cov_path)?,
        CoverageFormat::Cobertura => coverage::cobertura::parse_cobertura(cov_path)?,
        CoverageFormat::IstanbulSummary => coverage::istanbul::parse_istanbul(cov_path)?,
        CoverageFormat::GoCover => coverage::go_cover::parse_go_cover(cov_path)?,
    };

    // Normalize absolute paths to cwd-relative for exception matching.
    // Coverage tools (e.g., llvm-cov) report absolute paths; exceptions use
    // cwd-relative paths like "core/src/foo.rs". The coverage_path is resolved
    // by the runner as project_dir/cwd/coverage.path — its parent directory
    // is the cwd root, which we strip from each file path.
    if let Some(cwd_dir) = run_result.coverage_path.as_ref().and_then(|p| {
        // coverage_path = project_dir/cwd/lcov.info
        // We want project_dir/cwd/
        p.parent()
    }) {
        for cov in &mut coverages {
            if let Ok(relative) = std::path::Path::new(&cov.path).strip_prefix(cwd_dir) {
                cov.path = crate::git::GitName::from_os_str(relative.as_os_str()).storage_key();
            }
        }
    }

    Ok(coverages)
}

/// Evaluate coverage thresholds for a target.
#[must_use]
pub fn evaluate_target_thresholds(
    target: &TargetConfig,
    file_coverages: &[FileCoverage],
    changed_files: &[String],
) -> Vec<threshold::ThresholdResult> {
    let Some(cov_config) = &target.coverage else {
        return Vec::new();
    };

    let mut results = threshold::evaluate_file_thresholds(
        &cov_config.rules,
        file_coverages,
        changed_files,
        &cov_config.exceptions,
    );
    results.extend(
        threshold::evaluate_aggregate_thresholds(&cov_config.rules, file_coverages)
            .into_iter()
            .map(|aggregate| threshold::ThresholdResult {
                file: if aggregate.measured_lines == 0 {
                    format!(
                        "<{:?}: no measured lines; check coverage includes>",
                        aggregate.scope
                    )
                    .to_lowercase()
                } else {
                    format!("<{:?}>", aggregate.scope).to_lowercase()
                },
                coverage_percent: aggregate.coverage_percent,
                threshold: aggregate.threshold,
                pass: aggregate.pass,
                rule_scope: aggregate.scope,
                exception_applied: false,
            }),
    );
    results
}

/// Normalise `changed_files` (project-root-relative per `git diff`) to the
/// target's cwd-relative format so the filter against `ThresholdResult.file`
/// (which `parse_target_coverage` already stripped to cwd-relative paths)
/// produces real matches rather than an empty intersection.
///
/// Behaviour:
/// - `target_cwd = None` (target runs at project root) → pass-through.
/// - File lives under `target_cwd/…` → returned with prefix stripped.
/// - File lives OUTSIDE `target_cwd/…` → dropped (not in target's scope).
///
/// The ledger's modified-file audit is per-target by design: a docs-only
/// file changed at the repo root is not a covered artifact for the rust-core
/// target, so excluding it here is semantically correct.
fn strip_target_cwd_prefix(changed_files: &[String], target_cwd: Option<&str>) -> Vec<String> {
    let Some(cwd_rel) = target_cwd else {
        return changed_files.to_vec();
    };
    if cwd_rel.is_empty() || cwd_rel == "." {
        return changed_files.to_vec();
    }
    let prefix = if cwd_rel.ends_with('/') {
        cwd_rel.to_string()
    } else {
        format!("{cwd_rel}/")
    };
    changed_files
        .iter()
        .filter_map(|f| f.strip_prefix(&prefix).map(String::from))
        .collect()
}

/// Write the canonical test report to `report_dir`.
///
/// The caller chooses the destination (the CLI wires the project's runtime
/// state directory, e.g. `.git/codeflow/test-reports/`). Creates the
/// directory if needed. Filename: `{run_id}.json`.
///
/// # Errors
///
/// Returns `TestingError` if the report directory cannot be created or the
/// report file cannot be written.
pub fn write_test_report(
    report_dir: &Path,
    run_id: &str,
    targets: &[TargetPostData],
) -> Result<(), TestingError> {
    std::fs::create_dir_all(report_dir).map_err(|e| TestingError::CoverageParseError {
        path: report_dir.to_path_buf(),
        message: format!("failed to create report dir: {e}"),
    })?;

    // Aggregate all reports into a summary JSON
    let mut targets_arr = Vec::new();
    for t in targets {
        let mut target_json = serde_json::json!({
            "name": t.name,
            "mode": t.mode,
            "exit_code": t.run_result.exit_code,
            "duration_ms": t.run_result.duration_ms,
        });
        if let Some(ref report) = t.report {
            target_json["summary"] = serde_json::json!({
                "passed": report.results.summary.passed,
                "failed": report.results.summary.failed,
                "skipped": report.results.summary.skipped,
                "total": report.results.summary.total,
            });
        }
        if !t.file_coverages.is_empty() {
            let total_found: u64 = t.file_coverages.iter().map(|c| c.lines_found).sum();
            let total_hit: u64 = t.file_coverages.iter().map(|c| c.lines_hit).sum();
            let pct = if total_found > 0 {
                FileCoverage::compute_percent(total_found, total_hit)
            } else {
                0.0
            };
            target_json["coverage_percent"] = serde_json::json!(pct);
        }
        let failing = t.threshold_results.iter().filter(|r| !r.pass).count();
        target_json["threshold_failures"] = serde_json::json!(failing);
        targets_arr.push(target_json);
    }

    let summary = serde_json::json!({
        "run_id": run_id,
        "timestamp": chrono_timestamp(),
        "targets": targets_arr,
    });

    let report_path = report_dir.join(format!("{run_id}.json"));
    let json =
        serde_json::to_string_pretty(&summary).map_err(|e| TestingError::CoverageParseError {
            path: report_path.clone(),
            message: format!("failed to serialize report: {e}"),
        })?;
    std::fs::write(&report_path, format!("{json}\n")).map_err(|e| {
        TestingError::CoverageParseError {
            path: report_path,
            message: format!("failed to write report: {e}"),
        }
    })?;

    Ok(())
}

/// Emit a `test_result_recorded` event to the ledger.
///
/// Appends a single JSONL line to `{ledger_dir}/testing-events.jsonl`. The
/// caller chooses the ledger directory (the CLI wires the project's runtime
/// state directory, e.g. `.git/codeflow/ledger/`). Non-fatal: logs warning
/// on failure but does not propagate errors.
///
/// The event payload includes a per-target coverage audit: overall coverage
/// percentage, per-rule threshold pass/fail counts, exception applications,
/// and per-file results for modified files. This provides a complete audit
/// trail for coverage decisions — not just test pass/fail.
pub fn emit_ledger_event(
    ledger_dir: &Path,
    run_id: &str,
    targets: &[TargetPostData],
    changed_files: &[String],
) {
    if std::fs::create_dir_all(ledger_dir).is_err() {
        eprintln!("warning: cannot create ledger dir");
        return;
    }

    let overall_pass = targets
        .iter()
        .all(|t| t.run_result.exit_code == 0 && t.threshold_results.iter().all(|r| r.pass));

    let total_duration_ms: u64 = targets.iter().map(|t| t.run_result.duration_ms).sum();

    let target_summaries: Vec<serde_json::Value> = targets
        .iter()
        .map(|t| build_target_ledger_summary(t, changed_files))
        .collect();

    let event = serde_json::json!({
        "event": "test_result_recorded",
        "run_id": run_id,
        "timestamp": chrono_timestamp(),
        "overall_pass": overall_pass,
        "targets": target_summaries,
        "duration_ms": total_duration_ms,
        "changed_files": changed_files,
    });

    let ledger_path = ledger_dir.join("testing-events.jsonl");
    let line = match serde_json::to_string(&event) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("warning: failed to serialize ledger event: {e}");
            return;
        }
    };

    // Locked append via open + write
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ledger_path)
    {
        Ok(mut f) => {
            if writeln!(f, "{line}").is_err() {
                eprintln!("warning: failed to write ledger event");
            }
        }
        Err(e) => {
            eprintln!("warning: cannot open ledger file: {e}");
        }
    }
}

/// Build a per-target ledger summary with coverage audit fields.
fn build_target_ledger_summary(
    target: &TargetPostData,
    changed_files: &[String],
) -> serde_json::Value {
    let mut s = serde_json::json!({
        "name": target.name,
        "exit_code": target.run_result.exit_code,
        "duration_ms": target.run_result.duration_ms,
    });
    if let Some(ref report) = target.report {
        s["passed"] = serde_json::json!(report.results.summary.passed);
        s["failed"] = serde_json::json!(report.results.summary.failed);
        s["skipped"] = serde_json::json!(report.results.summary.skipped);
    }

    // Coverage audit: per-target overall percentage, per-file threshold pass/fail.
    let total_lines_found: u64 = target.file_coverages.iter().map(|c| c.lines_found).sum();
    let total_lines_hit: u64 = target.file_coverages.iter().map(|c| c.lines_hit).sum();
    let coverage_percent = if total_lines_found > 0 {
        Some(FileCoverage::compute_percent(
            total_lines_found,
            total_lines_hit,
        ))
    } else {
        None
    };
    s["coverage_percent"] =
        coverage_percent.map_or(serde_json::Value::Null, serde_json::Value::from);

    let threshold_passes = target.threshold_results.iter().filter(|r| r.pass).count();
    // `pass` already reflects the exception-lowered threshold, so every `!pass`
    // is a real failure — matching the gate verdict (codex round-2: don't waive
    // a file below its lowered bar just because an exception was applied).
    let threshold_failures = target.threshold_results.iter().filter(|r| !r.pass).count();
    let exception_applications = target
        .threshold_results
        .iter()
        .filter(|r| r.exception_applied)
        .count();
    s["threshold_passes"] = serde_json::json!(threshold_passes);
    s["threshold_failures"] = serde_json::json!(threshold_failures);
    s["exception_applications"] = serde_json::json!(exception_applications);

    // Per-file results for the modified-file audit trail.
    // `changed_files` arrives as project-root-relative paths from `git diff`;
    // `ThresholdResult.file` is cwd-relative. Strip the target's cwd prefix
    // so the two path formats align and the filter produces real matches.
    let scoped_changed =
        strip_target_cwd_prefix(changed_files, target.target_config.cwd.as_deref());
    let modified_file_results: Vec<serde_json::Value> = target
        .threshold_results
        .iter()
        .filter(|r| scoped_changed.iter().any(|f| f == &r.file))
        .map(|r| {
            serde_json::json!({
                "file": r.file,
                "coverage_percent": r.coverage_percent,
                "threshold": r.threshold,
                "pass": r.pass,
                "exception_applied": r.exception_applied,
            })
        })
        .collect();
    s["modified_file_results"] = serde_json::json!(modified_file_results);

    s
}

/// Detect files changed relative to a base git ref.
///
/// Runs `git diff --name-only {base_ref}` in `project_dir` and returns the
/// resulting file list (one path per entry, relative to the repo root).
/// Returns an empty vec on any git error — the caller treats "no changed
/// files" as a non-fatal condition (e.g., fresh repo, detached HEAD).
///
/// The `base_ref` is typically `HEAD~1` but can be any ref (branch, tag,
/// merge-base marker) the caller configures.
#[must_use]
pub fn detect_changed_files(project_dir: &Path, base_ref: &str) -> Vec<String> {
    let output = crate::git::command()
        .args(["diff", "--name-only", "-z", base_ref])
        .current_dir(project_dir)
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    name_only_paths(&output.stdout)
}

/// The paths of `git diff --name-only -z` output. OS text rule (issue 79):
/// each is its storage key, so a path that is not valid UTF-8 stays its own
/// path and is never a lossy lookalike.
fn name_only_paths(stdout: &[u8]) -> Vec<String> {
    stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| crate::git::GitName::from_bytes(path).storage_key())
        .collect()
}

/// Resolve the default base ref for branch-wide changed-file detection.
///
/// Returns `git merge-base HEAD origin/main` when the command succeeds and
/// emits a non-empty SHA. Falls back to `HEAD~1` when:
/// - `origin/main` is not available (fresh clone, disconnected work, fork).
/// - `git` is missing from PATH.
/// - The merge-base SHA is empty (detached HEAD with no common ancestor).
///
/// The merge-base is preferred because it represents the divergence point
/// from the default branch — i.e., the set of files that have changed
/// on the working branch since it forked off. `HEAD~1` only compares
/// against the immediately preceding commit, which under-reports on any
/// branch longer than a single commit.
///
/// Callers can override this by setting `CODEFLOW_COVERAGE_BASE_REF`.
#[must_use]
pub fn default_base_ref(project_dir: &Path) -> String {
    let output = crate::git::command()
        .args(["merge-base", "HEAD", "origin/main"])
        .current_dir(project_dir)
        .output();

    let Ok(out) = output else {
        return "HEAD~1".to_string();
    };
    if !out.status.success() {
        return "HEAD~1".to_string();
    }

    let sha =
        String::from_utf8_lossy(out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout)).to_string();
    if sha.is_empty() {
        "HEAD~1".to_string()
    } else {
        sha
    }
}

fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    // ISO 8601 approximation without chrono crate
    let secs = now.as_secs();
    let days = secs / 86400;
    let remaining = secs % 86400;
    let hours = remaining / 3600;
    let minutes = (remaining % 3600) / 60;
    let seconds = remaining % 60;
    // Approximate date calculation (good enough for filenames)
    let year = 1970 + days / 365;
    let day_of_year = days % 365;
    let month = day_of_year / 30 + 1;
    let day = day_of_year % 30 + 1;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::config::{
        CoverageConfig, CoverageException, CoverageRule, CoverageScope, ModeCommand, ReportConfig,
        RunnerType, TargetConfig,
    };
    use crate::testing::coverage::FileCoverage;
    use crate::testing::report::{CanonicalTestReport, CtrfStatus, CtrfTest};
    use crate::testing::runner::TargetRunResult;
    use std::collections::BTreeMap;

    fn make_target(name: &str, has_coverage: bool) -> TargetConfig {
        let mut modes = BTreeMap::new();
        modes.insert(
            "full".to_string(),
            ModeCommand {
                command: "echo pass".to_string(),
            },
        );
        TargetConfig {
            name: name.to_string(),
            enabled: true,
            cwd: None,
            env: BTreeMap::new(),
            shell: crate::testing::config::CommandShell::Auto,
            requires: Vec::new(),
            outputs: Vec::new(),
            narrow: Vec::new(),
            exclusive: false,
            runner: RunnerType::Custom,
            modes,
            report: Some(ReportConfig {
                format: ReportFormat::Junit,
                path: "junit.xml".to_string(),
                derive_from: Some("junit".to_string()),
            }),
            coverage: if has_coverage {
                Some(CoverageConfig {
                    format: CoverageFormat::Lcov,
                    path: "lcov.info".to_string(),
                    transform: None,
                    rules: vec![CoverageRule {
                        scope: CoverageScope::PerFile,
                        include: Vec::new(),
                        exclude: Vec::new(),
                        minimum: 85,
                    }],
                    exceptions: vec![CoverageException {
                        file: "untestable.rs".to_string(),
                        threshold: 0,
                        reason: "test".to_string(),
                        remove_when: "later".to_string(),
                    }],
                })
            } else {
                None
            },
            ci_skip: None,
            ci_skip_reason: None,
            timeout_seconds: None,
            structural: None,
            tags: Vec::new(),
            test_files: Vec::new(),
        }
    }

    fn make_run_result(name: &str) -> TargetRunResult {
        TargetRunResult {
            target_name: name.to_string(),
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1000,
            report_path: None,
            coverage_path: None,
        }
    }

    fn make_report() -> CanonicalTestReport {
        CanonicalTestReport::new(
            "test",
            vec![
                CtrfTest {
                    name: "test_one".to_string(),
                    status: CtrfStatus::Passed,
                    duration: 100.0,
                    suite: None,
                    message: None,
                    trace: None,
                    tags: vec![],
                    flaky: false,
                },
                CtrfTest {
                    name: "test_two".to_string(),
                    status: CtrfStatus::Passed,
                    duration: 200.0,
                    suite: None,
                    message: None,
                    trace: None,
                    tags: vec![],
                    flaky: false,
                },
            ],
        )
    }

    #[test]
    fn test_write_test_report() {
        let dir = tempfile::tempdir().unwrap();
        let post = TargetPostData {
            name: "rust".to_string(),
            mode: "full".to_string(),
            run_result: make_run_result("rust"),
            report: Some(make_report()),
            file_coverages: Vec::new(),
            threshold_results: Vec::new(),
            target_config: make_target("rust", false),
        };

        let report_dir = dir.path().join("test-reports");
        let result = write_test_report(&report_dir, "run-001", &[post]);
        assert!(result.is_ok());

        let report_path = report_dir.join("run-001.json");
        assert!(report_path.exists());

        let content = std::fs::read_to_string(&report_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["run_id"], "run-001");
        assert_eq!(parsed["targets"][0]["name"], "rust");
    }

    #[test]
    fn test_evaluate_target_thresholds_with_exception() {
        let target = make_target("rust", true);
        let coverages = vec![
            FileCoverage {
                path: "src/main.rs".to_string(),
                lines_found: 100,
                lines_hit: 90,
                percent: 90.0,
            },
            FileCoverage {
                path: "untestable.rs".to_string(),
                lines_found: 50,
                lines_hit: 0,
                percent: 0.0,
            },
        ];

        let results = evaluate_target_thresholds(&target, &coverages, &[]);
        assert_eq!(results.len(), 2);

        let main_result = results.iter().find(|r| r.file == "src/main.rs").unwrap();
        assert!(main_result.pass);
        assert!(!main_result.exception_applied);

        let exc_result = results.iter().find(|r| r.file == "untestable.rs").unwrap();
        assert!(exc_result.pass);
        assert!(exc_result.exception_applied);
    }

    #[test]
    fn test_parse_target_coverage_missing_file_returns_empty() {
        // AC 23: essential-mode graceful degradation — a missing coverage
        // artifact must NOT be an error; callers rely on the empty vec to
        // render "N/A" in Section 2 instead of aborting the run.
        let target = make_target("rust", true);
        let run_result = make_run_result("rust"); // coverage_path is None
        let result = parse_target_coverage(&target, &run_result);
        assert!(result.is_ok(), "missing coverage must not error");
        assert!(
            result.unwrap().is_empty(),
            "missing coverage yields empty vec"
        );
    }

    #[test]
    fn test_parse_target_coverage_nonexistent_path_returns_empty() {
        let target = make_target("rust", true);
        let mut run_result = make_run_result("rust");
        run_result.coverage_path = Some(std::path::PathBuf::from(
            "/nonexistent/path/that/cannot/exist.info",
        ));
        let result = parse_target_coverage(&target, &run_result);
        assert!(result.is_ok(), "nonexistent coverage path must not error");
        assert!(
            result.unwrap().is_empty(),
            "nonexistent path yields empty vec"
        );
    }

    #[test]
    fn test_build_target_ledger_summary_includes_coverage_audit() {
        // AC 26: ledger event per-target payload must include coverage_percent,
        // threshold_passes, threshold_failures, exception_applications, and
        // per-file modified_file_results for changed files.
        let target = make_target("rust", true);
        let post = TargetPostData {
            name: "rust".to_string(),
            mode: "full".to_string(),
            run_result: make_run_result("rust"),
            report: Some(make_report()),
            file_coverages: vec![FileCoverage {
                path: "src/main.rs".to_string(),
                lines_found: 100,
                lines_hit: 90,
                percent: 90.0,
            }],
            threshold_results: vec![
                threshold::ThresholdResult {
                    file: "src/main.rs".to_string(),
                    coverage_percent: 90.0,
                    threshold: 85,
                    pass: true,
                    rule_scope: CoverageScope::PerFile,
                    exception_applied: false,
                },
                threshold::ThresholdResult {
                    file: "untestable.rs".to_string(),
                    coverage_percent: 0.0,
                    threshold: 0,
                    pass: true,
                    rule_scope: CoverageScope::PerFile,
                    exception_applied: true,
                },
                threshold::ThresholdResult {
                    file: "failing.rs".to_string(),
                    coverage_percent: 50.0,
                    threshold: 85,
                    pass: false,
                    rule_scope: CoverageScope::PerFile,
                    exception_applied: false,
                },
            ],
            target_config: target,
        };

        let changed_files = vec!["src/main.rs".to_string()];
        let summary = build_target_ledger_summary(&post, &changed_files);

        assert_eq!(summary["name"], "rust");
        assert_eq!(summary["passed"], 2);
        assert_eq!(summary["failed"], 0);
        assert!(
            (summary["coverage_percent"].as_f64().unwrap() - 90.0).abs() < f64::EPSILON,
            "coverage_percent mismatch"
        );
        assert_eq!(summary["threshold_passes"], 2);
        assert_eq!(summary["threshold_failures"], 1);
        assert_eq!(summary["exception_applications"], 1);
        let modified = summary["modified_file_results"].as_array().unwrap();
        assert_eq!(modified.len(), 1, "only src/main.rs is in changed_files");
        assert_eq!(modified[0]["file"], "src/main.rs");
        assert_eq!(modified[0]["pass"], true);
    }

    #[test]
    fn test_build_target_ledger_summary_no_coverage() {
        let target = make_target("shell", false);
        let post = TargetPostData {
            name: "shell".to_string(),
            mode: "full".to_string(),
            run_result: make_run_result("shell"),
            report: Some(make_report()),
            file_coverages: Vec::new(),
            threshold_results: Vec::new(),
            target_config: target,
        };

        let summary = build_target_ledger_summary(&post, &[]);
        assert_eq!(summary["name"], "shell");
        assert!(summary["coverage_percent"].is_null());
        assert_eq!(summary["threshold_passes"], 0);
        assert_eq!(summary["threshold_failures"], 0);
        assert_eq!(summary["exception_applications"], 0);
    }

    #[test]
    fn test_emit_ledger_event_writes_file_with_coverage_fields() {
        // End-to-end: the ledger file should contain the new audit fields.
        let dir = tempfile::tempdir().unwrap();
        let target = make_target("rust", true);
        let post = TargetPostData {
            name: "rust".to_string(),
            mode: "full".to_string(),
            run_result: make_run_result("rust"),
            report: Some(make_report()),
            file_coverages: vec![FileCoverage {
                path: "src/lib.rs".to_string(),
                lines_found: 100,
                lines_hit: 92,
                percent: 92.0,
            }],
            threshold_results: vec![threshold::ThresholdResult {
                file: "src/lib.rs".to_string(),
                coverage_percent: 92.0,
                threshold: 85,
                pass: true,
                rule_scope: CoverageScope::PerFile,
                exception_applied: false,
            }],
            target_config: target,
        };

        let ledger_dir = dir.path().join("ledger");
        emit_ledger_event(&ledger_dir, "run-xyz", &[post], &["src/lib.rs".to_string()]);

        let ledger_path = ledger_dir.join("testing-events.jsonl");
        assert!(ledger_path.exists(), "ledger file must be written");
        let content = std::fs::read_to_string(&ledger_path).unwrap();
        let event: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(event["event"], "test_result_recorded");
        assert_eq!(event["run_id"], "run-xyz");
        assert_eq!(event["overall_pass"], true);
        let targets = event["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 1);
        assert!(
            (targets[0]["coverage_percent"].as_f64().unwrap() - 92.0).abs() < f64::EPSILON,
            "coverage_percent must be present in ledger event"
        );
        assert_eq!(targets[0]["threshold_passes"], 1);
        assert_eq!(targets[0]["threshold_failures"], 0);
        let modified = targets[0]["modified_file_results"].as_array().unwrap();
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0]["file"], "src/lib.rs");
        let changed = event["changed_files"].as_array().unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0], "src/lib.rs");
    }

    #[test]
    fn test_detect_changed_files_invalid_ref_returns_empty() {
        // AC 22: detect_changed_files must degrade gracefully on git errors
        // (invalid ref, not a repo, etc.) instead of panicking or erroring.
        let dir = tempfile::tempdir().unwrap();
        // Not a git repo, so any ref errors — expect empty vec, not panic.
        let files = detect_changed_files(dir.path(), "HEAD~1");
        assert!(files.is_empty(), "non-repo directory yields empty vec");
    }

    #[test]
    fn test_detect_changed_files_in_real_repo() {
        // Run against the actual project dir — if HEAD~1 exists, we should
        // get something (or empty if there are no changes). Either way,
        // must not panic.
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _files = detect_changed_files(&cwd, "HEAD~1");
        // No assertion on content — just that the call completes without panic.
    }

    #[test]
    fn test_default_base_ref_falls_back_outside_repo() {
        // AC 22: outside a git repo (no `origin/main`, no `git`), the helper
        // must fall back to the literal `HEAD~1` string rather than panicking
        // or returning an empty string that would break callers.
        let dir = tempfile::tempdir().unwrap();
        let resolved = default_base_ref(dir.path());
        assert_eq!(
            resolved, "HEAD~1",
            "non-repo directory must fall back to HEAD~1"
        );
    }

    #[test]
    fn test_default_base_ref_in_real_repo() {
        // End-to-end smoke test: when invoked in the actual repo, the helper
        // either returns the merge-base SHA (40-char hex) or falls back to
        // "HEAD~1" if `origin/main` isn't configured (fresh clone). Either
        // outcome is non-empty and must be accepted by downstream callers.
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let resolved = default_base_ref(&cwd);
        assert!(!resolved.is_empty(), "resolved base ref must not be empty");
        // Either a 40-char hex SHA (merge-base success) or the literal fallback.
        let is_sha = resolved.len() == 40 && resolved.chars().all(|c| c.is_ascii_hexdigit());
        let is_fallback = resolved == "HEAD~1";
        assert!(
            is_sha || is_fallback,
            "resolved base ref must be 40-char hex SHA or \"HEAD~1\"; got: {resolved}"
        );
    }

    /// AC 22 regression: `changed_files` arrives project-root-relative from
    /// `git diff`; `ThresholdResult.file` is cwd-relative; the filter must
    /// align them via `strip_target_cwd_prefix`. Without the prefix strip,
    /// Section 3 (Modified File Coverage) renders empty even when the branch
    /// changed 20+ Rust files, because the two path formats never intersect.
    #[test]
    fn test_strip_target_cwd_prefix_aligns_path_formats() {
        let changed = vec![
            "backend/cli/src/cmd/test.rs".to_string(),
            "backend/core/src/testing/validation.rs".to_string(),
            ".claude/CLAUDE.md".to_string(),
            "docs/notes/foo.md".to_string(),
        ];

        // Target cwd = "backend" (a monorepo sub-tree target). Files under
        // that tree must be returned with the prefix stripped; files outside
        // must be dropped.
        let scoped = strip_target_cwd_prefix(&changed, Some("backend"));
        assert_eq!(
            scoped,
            vec![
                "cli/src/cmd/test.rs".to_string(),
                "core/src/testing/validation.rs".to_string(),
            ],
            "in-scope files must be returned cwd-relative; out-of-scope must be dropped"
        );

        // Target with no cwd (runs at project root) → pass-through, no strip.
        let passthrough = strip_target_cwd_prefix(&changed, None);
        assert_eq!(
            passthrough, changed,
            "None cwd must pass through without modification"
        );

        // Empty string and "." both mean "project root".
        assert_eq!(strip_target_cwd_prefix(&changed, Some("")), changed);
        assert_eq!(strip_target_cwd_prefix(&changed, Some(".")), changed);
    }

    /// Regression guard for the shape that once produced an empty modified-file
    /// audit: a target whose `cwd = "backend"` holds a cwd-relative threshold
    /// result, while `changed_files` is project-root-relative. The per-target
    /// filter in the ledger summary must still find the match.
    #[test]
    fn test_ledger_summary_finds_modified_files_under_target_cwd() {
        use crate::testing::config::{
            CoverageConfig, CoverageFormat, CoverageScope, ModeCommand, RunnerType, TargetConfig,
        };
        use crate::testing::runner::TargetRunResult;
        use std::collections::BTreeMap;

        let mut modes = BTreeMap::new();
        modes.insert(
            "full".to_string(),
            ModeCommand {
                command: "cargo test".to_string(),
            },
        );

        let target_cfg = TargetConfig {
            name: "rust-core".to_string(),
            enabled: true,
            cwd: Some("backend".to_string()),
            env: BTreeMap::new(),
            shell: crate::testing::config::CommandShell::Auto,
            requires: Vec::new(),
            outputs: Vec::new(),
            narrow: Vec::new(),
            exclusive: false,
            runner: RunnerType::Cargo,
            modes,
            report: None,
            coverage: Some(CoverageConfig {
                format: CoverageFormat::Lcov,
                path: "lcov.info".to_string(),
                transform: None,
                rules: Vec::new(),
                exceptions: Vec::new(),
            }),
            ci_skip: None,
            ci_skip_reason: None,
            timeout_seconds: None,
            structural: None,
            tags: Vec::new(),
            test_files: Vec::new(),
        };

        let post = TargetPostData {
            name: "rust-core".to_string(),
            mode: "full".to_string(),
            run_result: TargetRunResult {
                target_name: "rust-core".to_string(),
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                duration_ms: 1000,
                report_path: None,
                coverage_path: None,
            },
            report: None,
            file_coverages: Vec::new(),
            threshold_results: vec![threshold::ThresholdResult {
                file: "cli/src/cmd/test.rs".to_string(),
                coverage_percent: 92.5,
                threshold: 85,
                pass: true,
                rule_scope: CoverageScope::PerFile,
                exception_applied: false,
            }],
            target_config: target_cfg,
        };

        let changed_files = vec![
            "backend/cli/src/cmd/test.rs".to_string(),
            ".claude/CLAUDE.md".to_string(),
        ];

        let summary = build_target_ledger_summary(&post, &changed_files);
        let modified = summary["modified_file_results"].as_array().unwrap();
        assert_eq!(
            modified.len(),
            1,
            "ledger summary must include exactly the in-scope modified file; got: {modified:?}"
        );
        assert_eq!(modified[0]["file"], "cli/src/cmd/test.rs");
    }

    /// Issue 79: a changed path that is not valid UTF-8 keeps its exact bytes
    /// in its key, so it is never a lossy lookalike of another path.
    #[test]
    fn a_changed_path_that_is_not_utf8_keeps_its_own_key() {
        let files = name_only_paths(b"caf\xe9\0caf\xef\xbf\xbd\0\0");
        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files.contains(&"caf\u{fffd}".to_string()));
        assert!(files.contains(&crate::git::GitName::from_bytes(b"caf\xe9").storage_key()));
    }
}
