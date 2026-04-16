//! Post-test pipeline: parse reports, evaluate coverage, render PR body.
//!
//! Coordinates the downstream steps after `runner::run_all_targets()`:
//! report parsing, coverage parsing, threshold evaluation, PR body rendering,
//! and report persistence.

use std::path::Path;

use crate::testing::config::{CoverageFormat, ReportFormat, TargetConfig};
use crate::testing::coverage::{self, FileCoverage};
use crate::testing::error::TestingError;
use crate::testing::pr_body::{self, TargetPrData};
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
/// canonical CTRF-shaped model. Handles JUnit `derive_from` conversion.
///
/// # Errors
///
/// Returns `TestingError` if the report file cannot be read or parsed.
pub fn parse_target_report(
    target: &TargetConfig,
    run_result: &TargetRunResult,
) -> Result<Option<CanonicalTestReport>, TestingError> {
    let report_config = match &target.report {
        Some(r) => r,
        None => return Ok(None),
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
    let cov_config = match &target.coverage {
        Some(c) => c,
        None => return Ok(Vec::new()),
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
                cov.path = relative.to_string_lossy().to_string();
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
    let cov_config = match &target.coverage {
        Some(c) => c,
        None => return Vec::new(),
    };

    threshold::evaluate_file_thresholds(
        &cov_config.rules,
        file_coverages,
        changed_files,
        &cov_config.exceptions,
    )
}

/// Build PR body data for a target.
#[must_use]
pub fn build_target_pr_data(post: &TargetPostData) -> TargetPrData {
    let exceptions = post
        .target_config
        .coverage
        .as_ref()
        .map(|c| c.exceptions.clone())
        .unwrap_or_default();

    let total_lines_found: u64 = post.file_coverages.iter().map(|c| c.lines_found).sum();
    let total_lines_hit: u64 = post.file_coverages.iter().map(|c| c.lines_hit).sum();
    let workspace_coverage = if total_lines_found > 0 {
        Some(total_lines_hit as f64 / total_lines_found as f64 * 100.0)
    } else {
        None
    };

    let coverage_na_reason = if post.target_config.coverage.is_none() {
        Some("no coverage configured".to_string())
    } else if post.file_coverages.is_empty() {
        Some("no coverage data collected".to_string())
    } else {
        None
    };

    let failing_count = post
        .threshold_results
        .iter()
        .filter(|r| !r.pass && !r.exception_applied)
        .count();
    let passing_count = post.threshold_results.iter().filter(|r| r.pass).count();
    let coverage_summary = if post.file_coverages.is_empty() {
        None
    } else {
        Some(format!(
            "{} files evaluated, {} pass, {} fail",
            post.threshold_results.len(),
            passing_count,
            failing_count,
        ))
    };

    TargetPrData {
        name: post.name.clone(),
        mode: post.mode.clone(),
        report: post.report.clone(),
        run_result: Some(post.run_result.clone()),
        file_coverages: post.file_coverages.clone(),
        threshold_results: post.threshold_results.clone(),
        exceptions,
        workspace_coverage,
        coverage_summary,
        coverage_na_reason,
    }
}

/// Render the full PR body markdown from post-test data.
pub fn render_full_pr_body(
    targets: &[TargetPostData],
    changed_files: &[String],
    consecutive_clean_runs: u32,
) -> String {
    let pr_targets: Vec<TargetPrData> = targets.iter().map(build_target_pr_data).collect();

    let modified_file_results: Vec<(String, String, f64, u32, bool)> = targets
        .iter()
        .flat_map(|t| {
            t.threshold_results
                .iter()
                .filter(|r| changed_files.contains(&r.file))
                .map(|r| {
                    (
                        t.name.clone(),
                        r.file.clone(),
                        r.coverage_percent,
                        r.threshold,
                        r.pass,
                    )
                })
        })
        .collect();

    pr_body::render_pr_body(&pr_targets, &modified_file_results, consecutive_clean_runs)
}

/// Write the canonical test report to `.state/test-reports/`.
///
/// Creates the directory if needed. Filename: `{run_id}.json`.
///
/// # Errors
///
/// Returns `TestingError` if the report directory cannot be created or the
/// report file cannot be written.
pub fn write_test_report(
    project_dir: &Path,
    run_id: &str,
    targets: &[TargetPostData],
) -> Result<(), TestingError> {
    let report_dir = project_dir.join(".state").join("test-reports");
    std::fs::create_dir_all(&report_dir).map_err(|e| TestingError::CoverageParseError {
        path: report_dir.clone(),
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
                total_hit as f64 / total_found as f64 * 100.0
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
/// Appends a single JSONL line to `.state/ledger/testing-events.jsonl`.
/// Non-fatal: logs warning on failure but does not propagate errors.
pub fn emit_ledger_event(project_dir: &Path, run_id: &str, targets: &[TargetPostData]) {
    let ledger_dir = project_dir.join(".state").join("ledger");
    if std::fs::create_dir_all(&ledger_dir).is_err() {
        eprintln!("warning: cannot create ledger dir");
        return;
    }

    let overall_pass = targets
        .iter()
        .all(|t| t.run_result.exit_code == 0 && t.threshold_results.iter().all(|r| r.pass));

    let total_duration_ms: u64 = targets.iter().map(|t| t.run_result.duration_ms).sum();

    let target_summaries: Vec<serde_json::Value> = targets
        .iter()
        .map(|t| {
            let mut s = serde_json::json!({
                "name": t.name,
                "exit_code": t.run_result.exit_code,
                "duration_ms": t.run_result.duration_ms,
            });
            if let Some(ref report) = t.report {
                s["passed"] = serde_json::json!(report.results.summary.passed);
                s["failed"] = serde_json::json!(report.results.summary.failed);
            }
            s
        })
        .collect();

    let event = serde_json::json!({
        "event": "test_result_recorded",
        "run_id": run_id,
        "timestamp": chrono_timestamp(),
        "overall_pass": overall_pass,
        "targets": target_summaries,
        "duration_ms": total_duration_ms,
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
    use std::io::Write;
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
        }
    }

    fn make_run_result(name: &str) -> TargetRunResult {
        TargetRunResult {
            target_name: name.to_string(),
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
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
    fn test_build_target_pr_data_with_coverage() {
        let target = make_target("rust", true);
        let post = TargetPostData {
            name: "rust".to_string(),
            mode: "full".to_string(),
            run_result: make_run_result("rust"),
            report: Some(make_report()),
            file_coverages: vec![
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
            ],
            threshold_results: vec![threshold::ThresholdResult {
                file: "src/main.rs".to_string(),
                coverage_percent: 90.0,
                threshold: 85,
                pass: true,
                rule_scope: CoverageScope::PerFile,
                exception_applied: false,
            }],
            target_config: target,
        };

        let pr_data = build_target_pr_data(&post);
        assert_eq!(pr_data.name, "rust");
        assert!(pr_data.workspace_coverage.is_some());
        assert_eq!(pr_data.exceptions.len(), 1);
    }

    #[test]
    fn test_build_target_pr_data_no_coverage() {
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

        let pr_data = build_target_pr_data(&post);
        assert_eq!(
            pr_data.coverage_na_reason.as_deref(),
            Some("no coverage configured")
        );
    }

    #[test]
    fn test_render_full_pr_body_produces_sections() {
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
            threshold_results: vec![threshold::ThresholdResult {
                file: "src/main.rs".to_string(),
                coverage_percent: 90.0,
                threshold: 85,
                pass: true,
                rule_scope: CoverageScope::PerFile,
                exception_applied: false,
            }],
            target_config: target,
        };

        let md = render_full_pr_body(&[post], &["src/main.rs".to_string()], 1);
        assert!(
            md.contains("### 1. Overall Test Pass Status"),
            "missing section 1"
        );
        assert!(md.contains("### 2. Overall Coverage"), "missing section 2");
        assert!(
            md.contains("### 3. Modified File Coverage"),
            "missing section 3"
        );
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

        let result = write_test_report(dir.path(), "run-001", &[post]);
        assert!(result.is_ok());

        let report_path = dir.path().join(".state/test-reports/run-001.json");
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
}
