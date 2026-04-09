//! Test command: run the codeflow test suite.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};

use crate::helpers;
use codeflow_core::testing::validation::TestValidator;

/// Test mode controlling which suites run.
#[derive(Debug, Clone, ValueEnum)]
pub enum TestMode {
    /// Essential tests only (default).
    Essential,
    /// Standard test suite.
    Standard,
    /// Full suite including integration and property tests.
    Full,
}

/// Arguments for the `test` subcommand.
#[derive(Debug, Clone, Args)]
pub struct TestArgs {
    /// Test mode: essential, standard, or full.
    #[arg(long, value_enum)]
    pub mode: Option<TestMode>,

    /// Run coverage collection and validation after tests.
    #[arg(long, default_value_t = false)]
    pub coverage: bool,

    /// Print the last saved test stats report (from `.state/runtime/test-stats.json`).
    #[arg(long, default_value_t = false)]
    pub report: bool,
}

pub fn run(args: Option<TestArgs>) -> Result<()> {
    let project_dir = helpers::detect_project_dir()?;
    let args = args.unwrap_or(TestArgs {
        mode: None,
        coverage: false,
        report: false,
    });
    run_with_dir(&project_dir, &args)
}

fn run_with_dir(project_dir: &Path, args: &TestArgs) -> Result<()> {
    // --report: print saved artifact and exit.
    if args.report {
        return run_report(project_dir);
    }

    // --mode full --coverage: full validation pipeline.
    if matches!(args.mode, Some(TestMode::Full)) && args.coverage {
        return run_full_coverage(project_dir);
    }

    // Default: run the shell-based test runner.
    run_legacy_tests(project_dir)
}

/// Print the last saved test stats report.
fn run_report(project_dir: &Path) -> Result<()> {
    let result = TestValidator::read_artifact(project_dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let md = TestValidator::format_markdown(&result);
    println!("{md}");
    Ok(())
}

/// Full validation: run tests, coverage, validate, write artifact, print report.
fn run_full_coverage(project_dir: &Path) -> Result<()> {
    println!("Running full test validation...");

    let result = TestValidator::full_validate(project_dir)?;

    TestValidator::write_artifact(project_dir, &result)?;

    let md = TestValidator::format_markdown(&result);
    println!("{md}");

    if !result.overall_pass {
        eprintln!(
            "FAIL: {} test failures, {} modified files below threshold",
            result.test_run.failed,
            result
                .modified_file_results
                .iter()
                .filter(|m| !m.pass)
                .count()
        );
        std::process::exit(1);
    }

    println!("PASS: All tests pass, all coverage thresholds met.");
    Ok(())
}

/// Legacy mode: run the shell-based `run-all-tests.sh` script.
fn run_legacy_tests(project_dir: &Path) -> Result<()> {
    let test_script = project_dir
        .join(".codeflow")
        .join("testing")
        .join("run-all-tests.sh");

    if !test_script.exists() {
        anyhow::bail!("test runner not found at {}", test_script.display());
    }

    let status = Command::new("bash")
        .arg(&test_script)
        .current_dir(project_dir)
        .status()
        .context("executing test runner")?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_no_script() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("test runner not found"), "got: {msg}");
    }

    #[test]
    fn test_test_with_passing_script() {
        let dir = tempfile::tempdir().unwrap();
        let script_dir = dir.path().join(".codeflow").join("testing");
        std::fs::create_dir_all(&script_dir).unwrap();
        std::fs::write(script_dir.join("run-all-tests.sh"), "#!/bin/bash\nexit 0\n").unwrap();
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok());
    }

    #[test]
    fn test_test_no_script_error_message() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test runner not found"),
            "expected 'test runner not found', got: {msg}"
        );
        assert!(
            msg.contains("run-all-tests.sh"),
            "expected path to contain run-all-tests.sh, got: {msg}"
        );
    }

    #[test]
    fn test_test_with_passing_script_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let script_dir = dir.path().join(".codeflow").join("testing");
        std::fs::create_dir_all(&script_dir).unwrap();
        let script_path = script_dir.join("run-all-tests.sh");
        std::fs::write(&script_path, "#!/bin/bash\nexit 0\n").unwrap();
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok(), "passing script should return Ok(())");
    }

    #[test]
    fn test_test_script_path_includes_codeflow_testing() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains(".codeflow"),
            "error should mention .codeflow directory: {msg}"
        );
        assert!(
            msg.contains("testing"),
            "error should mention testing directory: {msg}"
        );
    }

    #[test]
    fn test_report_flag_missing_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: true,
        };
        // run_with_dir routes to run_report, which returns Err for missing artifact.
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("test stats artifact not found"), "got: {msg}");
    }

    #[test]
    fn test_report_flag_with_artifact() {
        use codeflow_core::testing::validation::{TestRunResult, TestValidationResult};

        let dir = tempfile::tempdir().unwrap();
        let artifact = TestValidationResult {
            overall_pass: true,
            test_run: TestRunResult {
                passed: 50,
                failed: 0,
                ignored: 0,
                failures: Vec::new(),
                duration_secs: 2.0,
            },
            file_coverages: Vec::new(),
            crate_coverages: Vec::new(),
            workspace_coverage: 90.0,
            modified_file_results: Vec::new(),
            exempted_files: Vec::new(),
            new_tests_added: 0,
            consecutive_clean_runs: 1,
        };

        TestValidator::write_artifact(dir.path(), &artifact).unwrap();

        // run_with_dir routes to run_report, which reads and formats the artifact.
        let args = TestArgs {
            mode: None,
            coverage: false,
            report: true,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_ok(), "report with valid artifact should succeed");
    }

    #[test]
    fn test_report_flag_failing_artifact() {
        use codeflow_core::testing::validation::{TestRunResult, TestValidationResult};

        let dir = tempfile::tempdir().unwrap();
        let artifact = TestValidationResult {
            overall_pass: false,
            test_run: TestRunResult {
                passed: 9,
                failed: 1,
                ignored: 0,
                failures: vec!["tests::broken".to_string()],
                duration_secs: 1.0,
            },
            file_coverages: Vec::new(),
            crate_coverages: Vec::new(),
            workspace_coverage: 90.0,
            modified_file_results: Vec::new(),
            exempted_files: Vec::new(),
            new_tests_added: 0,
            consecutive_clean_runs: 1,
        };

        TestValidator::write_artifact(dir.path(), &artifact).unwrap();

        let args = TestArgs {
            mode: None,
            coverage: false,
            report: true,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err(), "report with failing artifact should error");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("overall_pass=false"), "got: {msg}");
    }

    #[test]
    fn test_report_takes_priority_over_mode() {
        // When both --report and --mode full --coverage are set, --report wins.
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: Some(TestMode::Full),
            coverage: true,
            report: true,
        };
        // report=true routes to run_report, which fails on missing artifact
        // (not to run_full_coverage which would fail differently on missing cargo).
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test stats artifact not found"),
            "should route to report path, got: {msg}"
        );
    }

    #[test]
    fn test_mode_full_without_coverage_runs_legacy() {
        // --mode full without --coverage should fall through to legacy test runner.
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: Some(TestMode::Full),
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test runner not found"),
            "should fall to legacy, got: {msg}"
        );
    }

    #[test]
    fn test_mode_essential_runs_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: Some(TestMode::Essential),
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test runner not found"),
            "essential should run legacy, got: {msg}"
        );
    }

    #[test]
    fn test_mode_standard_runs_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let args = TestArgs {
            mode: Some(TestMode::Standard),
            coverage: false,
            report: false,
        };
        let result = run_with_dir(dir.path(), &args);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("test runner not found"),
            "standard should run legacy, got: {msg}"
        );
    }
}
