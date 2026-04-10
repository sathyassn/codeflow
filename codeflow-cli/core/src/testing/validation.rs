//! Test validation module: shared code for test execution, coverage collection,
//! threshold validation, artifact management, and markdown formatting.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum TestError {
    #[error("command failed: {0}")]
    CommandFailed(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("artifact not found: {0}")]
    ArtifactNotFound(String),

    #[error("validation failed: {0}")]
    ValidationFailed(String),
}

// ── Configuration ──────────────────────────────────────────────────────────

/// Test configuration loaded from `codeflow-cli/config/testing/test-config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    pub coverage: CoverageConfig,
    pub conventions: ConventionsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageConfig {
    #[serde(default = "default_file_threshold", alias = "threshold")]
    pub file_threshold: u32,
    #[serde(default = "default_crate_threshold")]
    pub crate_threshold: u32,
    #[serde(default)]
    pub crate_overrides: HashMap<String, u32>,
    #[serde(default)]
    pub enforcement: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub business_packages: Vec<String>,
}

fn default_file_threshold() -> u32 {
    85
}

fn default_crate_threshold() -> u32 {
    85
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConventionsConfig {
    pub test_runner: String,
    pub coverage_tool: String,
    pub property_testing: String,
    pub snapshot_testing: String,
    #[serde(default)]
    pub exceptions: Vec<FileException>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileException {
    pub file: String,
    pub threshold: u32,
    pub reason: String,
    #[serde(default)]
    pub granularity: String,
    #[serde(default)]
    pub remove_when: String,
}

// ── Test Results ───────────────────────────────────────────────────────────

/// Result of running `cargo test --workspace --no-fail-fast`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestRunResult {
    pub passed: u32,
    pub failed: u32,
    pub ignored: u32,
    pub failures: Vec<String>,
    pub duration_secs: f64,
}

/// Per-file coverage data from `cargo llvm-cov --json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCoverage {
    pub filename: String,
    pub lines_count: u64,
    pub lines_covered: u64,
    pub percent: f64,
}

/// Per-crate aggregated coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateCoverage {
    pub name: String,
    pub lines_count: u64,
    pub lines_covered: u64,
    pub percent: f64,
    /// Per-crate threshold used during validation. Populated by `validate()`.
    #[serde(default)]
    pub threshold: u32,
}

/// Final validation result combining test execution and coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestValidationResult {
    pub overall_pass: bool,
    pub test_run: TestRunResult,
    pub file_coverages: Vec<FileCoverage>,
    pub crate_coverages: Vec<CrateCoverage>,
    pub workspace_coverage: f64,
    pub modified_file_results: Vec<ModifiedFileResult>,
    pub exempted_files: Vec<ExemptedFile>,
    /// Number of new tests added in this changeset. Default 0; WS-QA fills actual value.
    #[serde(default)]
    pub new_tests_added: u32,
    /// Number of consecutive clean test runs. Default 1; WS-QA fills actual value.
    #[serde(default = "default_consecutive_runs")]
    pub consecutive_clean_runs: u32,
    /// Validation warnings (non-fatal issues detected during validation).
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn default_consecutive_runs() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifiedFileResult {
    pub file: String,
    pub coverage: f64,
    pub threshold: u32,
    pub pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExemptedFile {
    pub file: String,
    pub coverage: f64,
    pub configured_threshold: u32,
    pub reason: String,
}

// ── TestValidator ──────────────────────────────────────────────────────────

pub struct TestValidator;

impl TestValidator {
    /// Load test config from `codeflow-cli/config/testing/test-config.json`.
    /// Returns defaults if the file is missing.
    #[must_use]
    pub fn load_config(project_dir: &Path) -> TestConfig {
        let config_path = project_dir
            .join("codeflow-cli")
            .join("config")
            .join("testing")
            .join("test-config.json");

        if let Ok(contents) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str::<TestConfig>(&contents) {
                return config;
            }
        }

        // Defaults when config is missing or unparseable.
        TestConfig {
            coverage: CoverageConfig {
                file_threshold: 85,
                crate_threshold: 85,
                crate_overrides: HashMap::new(),
                enforcement: "per_file".to_string(),
                tool: "cargo-llvm-cov".to_string(),
                business_packages: vec!["codeflow-core".to_string()],
            },
            conventions: ConventionsConfig {
                test_runner: "cargo-nextest".to_string(),
                coverage_tool: "cargo-llvm-cov".to_string(),
                property_testing: "proptest".to_string(),
                snapshot_testing: "insta".to_string(),
                exceptions: Vec::new(),
            },
        }
    }

    /// Run `cargo test --workspace --no-fail-fast` and parse results.
    ///
    /// # Errors
    /// Returns `TestError::CommandFailed` if the cargo command cannot be spawned.
    /// Returns `TestError::Parse` if the output cannot be parsed.
    pub fn run_tests(project_dir: &Path) -> Result<TestRunResult, TestError> {
        let output = Command::new("cargo")
            .args(["test", "--workspace", "--no-fail-fast"])
            .current_dir(project_dir.join("codeflow-cli"))
            .output()
            .map_err(|e| TestError::CommandFailed(format!("failed to run cargo test: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");

        Self::parse_test_output(&combined)
    }

    /// Parse `cargo test` output to extract pass/fail/ignored counts.
    ///
    /// # Errors
    /// Returns `TestError::Parse` if the result regex cannot be compiled.
    pub fn parse_test_output(output: &str) -> Result<TestRunResult, TestError> {
        let mut total_passed: u32 = 0;
        let mut total_failed: u32 = 0;
        let mut total_ignored: u32 = 0;
        let mut duration_secs: f64 = 0.0;
        let mut failures = Vec::new();

        // Parse "test result:" lines — there is one per test binary.
        let result_re = regex::Regex::new(
            r"test result: (?:ok|FAILED)\.\s+(\d+) passed;\s+(\d+) failed;\s+(\d+) ignored",
        )
        .map_err(|e| TestError::Parse(format!("regex compilation failed: {e}")))?;

        for cap in result_re.captures_iter(output) {
            total_passed += cap[1].parse::<u32>().unwrap_or(0);
            total_failed += cap[2].parse::<u32>().unwrap_or(0);
            total_ignored += cap[3].parse::<u32>().unwrap_or(0);
        }

        // Parse "finished in Ns" for duration.
        if let Ok(duration_re) = regex::Regex::new(r"finished in (\d+\.?\d*)s") {
            for cap in duration_re.captures_iter(output) {
                let d: f64 = cap[1].parse().unwrap_or(0.0);
                if d > duration_secs {
                    duration_secs = d;
                }
            }
        }

        // Extract failure names from "failures:" sections.
        let mut in_failures = false;
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed == "failures:" {
                in_failures = true;
                continue;
            }
            if in_failures {
                if trimmed.is_empty() || trimmed.starts_with("test result:") {
                    in_failures = false;
                    continue;
                }
                if !trimmed.starts_with("----") {
                    failures.push(trimmed.to_string());
                }
            }
        }

        Ok(TestRunResult {
            passed: total_passed,
            failed: total_failed,
            ignored: total_ignored,
            failures,
            duration_secs,
        })
    }

    /// Run `cargo llvm-cov --workspace --json` and parse coverage data.
    ///
    /// # Errors
    /// Returns `TestError::CommandFailed` if cargo llvm-cov cannot run or exits non-zero.
    /// Returns `TestError::Parse` or `TestError::Json` if output is malformed.
    pub fn run_coverage(
        project_dir: &Path,
    ) -> Result<(Vec<FileCoverage>, Vec<CrateCoverage>, f64), TestError> {
        let output = Command::new("cargo")
            .args(["llvm-cov", "--workspace", "--json"])
            .current_dir(project_dir.join("codeflow-cli"))
            .output()
            .map_err(|e| TestError::CommandFailed(format!("failed to run cargo llvm-cov: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TestError::CommandFailed(format!(
                "cargo llvm-cov failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Self::parse_coverage_json(&stdout)
    }

    /// Parse the JSON output from `cargo llvm-cov --json`.
    ///
    /// # Errors
    /// Returns `TestError::Json` if JSON is invalid.
    /// Returns `TestError::Parse` if `data[0].files` is missing.
    pub fn parse_coverage_json(
        json_str: &str,
    ) -> Result<(Vec<FileCoverage>, Vec<CrateCoverage>, f64), TestError> {
        let value: serde_json::Value = serde_json::from_str(json_str)?;

        let files = value
            .pointer("/data/0/files")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                TestError::Parse("missing data[0].files in coverage JSON".to_string())
            })?;

        let mut file_coverages = Vec::new();
        let mut crate_lines: HashMap<String, (u64, u64)> = HashMap::new();
        let mut total_lines: u64 = 0;
        let mut total_covered: u64 = 0;

        for file_val in files {
            let filename = file_val
                .get("filename")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let lines_count = file_val
                .pointer("/summary/lines/count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let lines_covered = file_val
                .pointer("/summary/lines/covered")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let percent = file_val
                .pointer("/summary/lines/percent")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0);

            file_coverages.push(FileCoverage {
                filename: filename.to_string(),
                lines_count,
                lines_covered,
                percent,
            });

            total_lines += lines_count;
            total_covered += lines_covered;

            // Group by crate: core/src/ -> codeflow-core, cli/src/ -> codeflow-cli.
            let crate_name = if filename.contains("core/src/") {
                "codeflow-core"
            } else if filename.contains("cli/src/") {
                "codeflow-cli"
            } else {
                continue;
            };

            let entry = crate_lines.entry(crate_name.to_string()).or_insert((0, 0));
            entry.0 += lines_count;
            entry.1 += lines_covered;
        }

        let workspace_coverage = if total_lines > 0 {
            (total_covered as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        };

        let crate_coverages: Vec<CrateCoverage> = crate_lines
            .into_iter()
            .map(|(name, (count, covered))| {
                let percent = if count > 0 {
                    (covered as f64 / count as f64) * 100.0
                } else {
                    0.0
                };
                CrateCoverage {
                    name,
                    lines_count: count,
                    lines_covered: covered,
                    percent,
                    threshold: 0, // Populated by validate() with config-driven values.
                }
            })
            .collect();

        Ok((file_coverages, crate_coverages, workspace_coverage))
    }

    /// Validate test results and coverage against thresholds.
    #[must_use]
    pub fn validate(
        test_run: &TestRunResult,
        all_coverage: &[FileCoverage],
        crate_coverage: &[CrateCoverage],
        workspace_coverage: f64,
        modified_files: &[String],
        config: &TestConfig,
    ) -> TestValidationResult {
        let mut modified_file_results = Vec::new();
        let mut all_modified_pass = true;
        let warnings = Vec::new();

        // Build exception lookup by file path suffix.
        let exceptions: HashMap<&str, &FileException> = config
            .conventions
            .exceptions
            .iter()
            .map(|e| (e.file.as_str(), e))
            .collect();

        for modified in modified_files {
            // Find coverage for this file in the all_coverage list.
            // Use exact path matching: the coverage filename must either equal
            // the modified path or end with "/{modified}" to prevent false
            // matches (e.g., "foo/bar.rs" matching "bar.rs").
            let file_cov = all_coverage.iter().find(|fc| {
                fc.filename == *modified || fc.filename.ends_with(&format!("/{modified}"))
            });

            // -1.0 sentinel means file not found in coverage data (rendered as "N/A").
            let coverage = file_cov.map_or(-1.0, |fc| fc.percent);

            // Determine threshold: per-file exception overrides, otherwise use configured file threshold.
            let threshold = if let Some(exc) = exceptions.get(modified.as_str()) {
                exc.threshold
            } else {
                config.coverage.file_threshold
            };

            // Missing coverage (sentinel -1.0) always fails.
            let pass = coverage >= 0.0 && coverage >= f64::from(threshold);

            if !pass {
                all_modified_pass = false;
            }

            modified_file_results.push(ModifiedFileResult {
                file: modified.clone(),
                coverage,
                threshold,
                pass,
            });
        }

        // Build exempted_files from ALL config exceptions, not just modified files.
        let exempted_files: Vec<ExemptedFile> = config
            .conventions
            .exceptions
            .iter()
            .map(|exc| {
                let coverage = all_coverage
                    .iter()
                    .find(|fc| {
                        fc.filename == exc.file || fc.filename.ends_with(&format!("/{}", exc.file))
                    })
                    .map_or(-1.0, |fc| fc.percent);

                ExemptedFile {
                    file: exc.file.clone(),
                    coverage,
                    configured_threshold: exc.threshold,
                    reason: exc.reason.clone(),
                }
            })
            .collect();

        // Check workspace coverage.
        let workspace_pass = workspace_coverage >= f64::from(config.coverage.file_threshold);

        // Populate per-crate thresholds and check crate coverages.
        let enriched_crates: Vec<CrateCoverage> = crate_coverage
            .iter()
            .map(|c| {
                let thresh = if let Some(&t) = config.coverage.crate_overrides.get(&c.name) {
                    t
                } else {
                    config.coverage.crate_threshold
                };
                CrateCoverage {
                    threshold: thresh,
                    ..c.clone()
                }
            })
            .collect();

        let crates_pass = enriched_crates
            .iter()
            .all(|c| c.percent >= f64::from(c.threshold));

        let overall_pass =
            test_run.failed == 0 && all_modified_pass && workspace_pass && crates_pass;

        TestValidationResult {
            overall_pass,
            test_run: test_run.clone(),
            file_coverages: all_coverage.to_vec(),
            crate_coverages: enriched_crates,
            workspace_coverage,
            modified_file_results,
            exempted_files,
            new_tests_added: 0,
            consecutive_clean_runs: 1,
            warnings,
        }
    }

    /// Full end-to-end validation: run tests, coverage, get modified files, validate.
    /// Also prunes stale exceptions (files that now exceed the global 85% threshold).
    ///
    /// # Errors
    /// Returns errors from test execution, coverage collection, or git diff.
    pub fn full_validate(project_dir: &Path) -> Result<TestValidationResult, TestError> {
        let config = Self::load_config(project_dir);
        let test_run = Self::run_tests(project_dir)?;
        let (file_coverages, crate_coverages, workspace_coverage) =
            Self::run_coverage(project_dir)?;

        // Auto-remove stale exceptions: any file now at or above the global
        // threshold (85%) no longer needs an exception.
        let config = Self::prune_stale_exceptions(project_dir, config, &file_coverages);

        let modified_files = Self::get_modified_files(project_dir)?;

        Ok(Self::validate(
            &test_run,
            &file_coverages,
            &crate_coverages,
            workspace_coverage,
            &modified_files,
            &config,
        ))
    }

    /// Write validation result to `.state/runtime/test-stats.json`.
    ///
    /// # Errors
    /// Returns `TestError::Io` on filesystem failures.
    /// Returns `TestError::Json` on serialization failures.
    pub fn write_artifact(
        project_dir: &Path,
        result: &TestValidationResult,
    ) -> Result<(), TestError> {
        let artifact_dir = project_dir.join(".state").join("runtime");
        std::fs::create_dir_all(&artifact_dir)?;

        let artifact_path = artifact_dir.join("test-stats.json");
        let json = serde_json::to_string_pretty(result)?;
        std::fs::write(&artifact_path, json)?;

        Ok(())
    }

    /// Read validation result from `.state/runtime/test-stats.json`.
    /// Returns error if missing or if `overall_pass` is false.
    ///
    /// # Errors
    /// Returns `TestError::ArtifactNotFound` if the file is missing.
    /// Returns `TestError::Json` if the file is malformed.
    /// Returns `TestError::ValidationFailed` if `overall_pass` is false.
    pub fn read_artifact(project_dir: &Path) -> Result<TestValidationResult, TestError> {
        let artifact_path = Self::artifact_path(project_dir);

        let contents = std::fs::read_to_string(&artifact_path).map_err(|_| {
            TestError::ArtifactNotFound(format!(
                "test stats artifact not found at {}",
                artifact_path.display()
            ))
        })?;

        let result: TestValidationResult = serde_json::from_str(&contents)?;

        if !result.overall_pass {
            return Err(TestError::ValidationFailed(format!(
                "test stats artifact shows overall_pass=false ({} test failures, {} modified files below threshold)",
                result.test_run.failed,
                result
                    .modified_file_results
                    .iter()
                    .filter(|m| !m.pass)
                    .count()
            )));
        }

        Ok(result)
    }

    /// Format validation result as the standardized 3-section markdown.
    #[must_use]
    pub fn format_markdown(result: &TestValidationResult) -> String {
        let mut out = String::new();

        // Section 1: Overall Test Pass Status (bullet format per CLAUDE.md)
        out.push_str("### 1. Overall Test Pass Status\n");
        let _ = writeln!(out, "- Suite: `cargo test --workspace --no-fail-fast`");
        let _ = writeln!(
            out,
            "- Result: {} passed, {} failed, {} skipped",
            result.test_run.passed, result.test_run.failed, result.test_run.ignored
        );
        let _ = writeln!(out, "- New tests added: {}", result.new_tests_added);
        let _ = writeln!(
            out,
            "- Runs: {} consecutive clean",
            result.consecutive_clean_runs
        );

        out.push('\n');

        // Section 2: Overall Coverage
        out.push_str("### 2. Overall Coverage\n");
        out.push_str("| Crate | Coverage | Threshold | Status |\n");
        out.push_str("|-------|----------|-----------|--------|\n");

        // Sort crates for deterministic output: core before cli.
        let mut sorted_crates = result.crate_coverages.clone();
        sorted_crates.sort_by(|a, b| a.name.cmp(&b.name));

        for crate_cov in &sorted_crates {
            let status = if crate_cov.percent >= f64::from(crate_cov.threshold) {
                "PASS"
            } else {
                "FAIL"
            };
            let _ = writeln!(
                out,
                "| {} | {:.1}% | {}% | {} |",
                crate_cov.name, crate_cov.percent, crate_cov.threshold, status
            );
        }

        let ws_status = if result.workspace_coverage >= 85.0 {
            "PASS"
        } else {
            "FAIL"
        };
        let _ = writeln!(
            out,
            "| **Workspace** | **{:.1}%** | **85%** | **{}** |",
            result.workspace_coverage, ws_status
        );

        // Exempted files sub-table: always render all project-wide exceptions
        // from test-config.json, regardless of whether they appeared in coverage data.
        out.push('\n');
        out.push_str("#### Exempted Files (below 85%)\n");
        out.push_str("All project-wide coverage exceptions from test-config.json conventions.exceptions[].\n\n");
        if result.exempted_files.is_empty() {
            out.push_str("No exceptions configured.\n");
        } else {
            out.push_str("| File | Coverage | Configured Threshold | Reason |\n");
            out.push_str("|------|----------|---------------------|--------|\n");
            for ef in &result.exempted_files {
                // coverage < 0 means the file was not found in coverage data.
                let coverage_str = if ef.coverage < 0.0 {
                    "N/A".to_string()
                } else {
                    format!("{:.1}%", ef.coverage)
                };
                let _ = writeln!(
                    out,
                    "| {} | {} | {}% | {} |",
                    ef.file, coverage_str, ef.configured_threshold, ef.reason
                );
            }
        }

        out.push('\n');

        // Section 3: Modified File Coverage
        out.push_str("### 3. Modified File Coverage\n");
        out.push_str("| File | Coverage | Threshold | Status |\n");
        out.push_str("|------|----------|-----------|--------|\n");

        for mfr in &result.modified_file_results {
            let status = if mfr.pass { "PASS" } else { "FAIL" };
            let coverage_str = if mfr.coverage < 0.0 {
                "N/A".to_string()
            } else {
                format!("{:.1}%", mfr.coverage)
            };
            let _ = writeln!(
                out,
                "| {} | {} | {}% | {} |",
                mfr.file, coverage_str, mfr.threshold, status
            );
        }

        out
    }

    /// Get list of modified `.rs` files under `codeflow-cli/` relative to `origin/main`.
    ///
    /// # Errors
    /// Returns `TestError::CommandFailed` if git diff cannot be spawned.
    pub fn get_modified_files(project_dir: &Path) -> Result<Vec<String>, TestError> {
        let output = Command::new("git")
            .args(["diff", "--name-only", "origin/main...HEAD"])
            .current_dir(project_dir)
            .output()
            .map_err(|e| TestError::CommandFailed(format!("failed to run git diff: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let files: Vec<String> = stdout
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && Path::new(trimmed)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                    && trimmed.starts_with("codeflow-cli/")
            })
            .map(|line| {
                // Strip the "codeflow-cli/" prefix to get the crate-relative path.
                line.trim()
                    .strip_prefix("codeflow-cli/")
                    .unwrap_or(line.trim())
                    .to_string()
            })
            .collect();

        Ok(files)
    }

    /// Determine the artifact path for the test stats file.
    #[must_use]
    pub fn artifact_path(project_dir: &Path) -> PathBuf {
        project_dir
            .join(".state")
            .join("runtime")
            .join("test-stats.json")
    }

    /// Path to the test-config.json file.
    #[must_use]
    pub fn config_path(project_dir: &Path) -> PathBuf {
        project_dir
            .join("codeflow-cli")
            .join("config")
            .join("testing")
            .join("test-config.json")
    }

    /// Remove stale exceptions: any exception whose file now meets or exceeds the
    /// global coverage threshold (85%) is removed from the config and the config
    /// file is rewritten. Returns the updated config for use in validation.
    #[must_use]
    pub fn prune_stale_exceptions(
        project_dir: &Path,
        mut config: TestConfig,
        file_coverages: &[FileCoverage],
    ) -> TestConfig {
        let global_threshold = f64::from(config.coverage.file_threshold);
        let original_count = config.conventions.exceptions.len();

        config.conventions.exceptions.retain(|exc| {
            let actual = file_coverages
                .iter()
                .find(|fc| {
                    fc.filename == exc.file || fc.filename.ends_with(&format!("/{}", exc.file))
                })
                .map_or(0.0, |fc| fc.percent);

            // Keep the exception only if the file is still below the global threshold.
            actual < global_threshold
        });

        if config.conventions.exceptions.len() < original_count {
            // Rewrite the config file with stale exceptions removed.
            let config_path = Self::config_path(project_dir);
            if let Ok(json) = serde_json::to_string_pretty(&config) {
                // Best-effort write; failure is non-fatal (config is still usable in memory).
                let _ = std::fs::write(&config_path, json + "\n");
            }
        }

        config
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join("codeflow-cli")
            .join("config")
            .join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(
            config_dir.join("test-config.json"),
            r#"{
                "coverage": {
                    "file_threshold": 90,
                    "crate_threshold": 90,
                    "enforcement": "per_file",
                    "tool": "cargo-llvm-cov",
                    "business_packages": ["codeflow-core", "codeflow-cli"]
                },
                "conventions": {
                    "test_runner": "cargo-nextest",
                    "coverage_tool": "cargo-llvm-cov",
                    "property_testing": "proptest",
                    "snapshot_testing": "insta",
                    "exceptions": []
                }
            }"#,
        )
        .unwrap();

        let config = TestValidator::load_config(dir.path());
        assert_eq!(config.coverage.file_threshold, 90);
        assert_eq!(config.coverage.business_packages.len(), 2);
    }

    #[test]
    fn test_load_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = TestValidator::load_config(dir.path());
        assert_eq!(config.coverage.file_threshold, 85);
        assert_eq!(config.coverage.business_packages, vec!["codeflow-core"]);
    }

    #[test]
    fn test_parse_test_output_all_pass() {
        let output = "\
running 5 tests
test tests::test_one ... ok
test tests::test_two ... ok
test tests::test_three ... ok
test tests::test_four ... ok
test tests::test_five ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.5s
";
        let result = TestValidator::parse_test_output(output).unwrap();
        assert_eq!(result.passed, 5);
        assert_eq!(result.failed, 0);
        assert_eq!(result.ignored, 0);
        assert!(result.failures.is_empty());
        assert!((result.duration_secs - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_test_output_with_failures() {
        let output = "\
running 3 tests
test tests::test_one ... ok
test tests::test_two ... FAILED
test tests::test_three ... ok

failures:
    tests::test_two

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.8s
";
        let result = TestValidator::parse_test_output(output).unwrap();
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
        assert_eq!(result.ignored, 0);
        assert_eq!(result.failures, vec!["tests::test_two"]);
    }

    #[test]
    fn test_parse_test_output_multiple_suites() {
        let output = "\
test result: ok. 10 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 1.0s

test result: ok. 5 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 2.0s
";
        let result = TestValidator::parse_test_output(output).unwrap();
        assert_eq!(result.passed, 15);
        assert_eq!(result.failed, 0);
        assert_eq!(result.ignored, 3);
        assert!((result.duration_secs - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_validate_all_pass() {
        let config = make_default_config();
        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/lib.rs", 90.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert!(result.overall_pass);
        assert_eq!(result.modified_file_results.len(), 1);
        assert!(result.modified_file_results[0].pass);
    }

    #[test]
    fn test_validate_one_file_fails() {
        let config = make_default_config();
        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/lib.rs", 50.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 50.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 50.0, &modified, &config);
        assert!(!result.overall_pass);
        assert!(!result.modified_file_results[0].pass);
    }

    #[test]
    fn test_validate_exempted_file_passes() {
        let mut config = make_default_config();
        config.conventions.exceptions.push(FileException {
            file: "core/src/autorun/worker.rs".to_string(),
            threshold: 80,
            reason: "async process spawning".to_string(),
            granularity: "per_file".to_string(),
            remove_when: "refactored".to_string(),
        });

        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/autorun/worker.rs", 82.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/autorun/worker.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert!(result.overall_pass);
        assert!(result.modified_file_results[0].pass);
        assert_eq!(result.exempted_files.len(), 1);
        assert_eq!(result.exempted_files[0].reason, "async process spawning");
    }

    #[test]
    fn test_validate_exempted_file_below_exception() {
        let mut config = make_default_config();
        config.conventions.exceptions.push(FileException {
            file: "core/src/autorun/worker.rs".to_string(),
            threshold: 80,
            reason: "async process spawning".to_string(),
            granularity: "per_file".to_string(),
            remove_when: "refactored".to_string(),
        });

        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/autorun/worker.rs", 70.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/autorun/worker.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert!(!result.overall_pass);
        assert!(!result.modified_file_results[0].pass);
    }

    #[test]
    fn test_validate_modified_files_filter() {
        let config = make_default_config();
        let test_run = make_passing_run(5);
        let all_cov = vec![
            make_file_cov("core/src/lib.rs", 90.0),
            make_file_cov("core/src/hooks/session.rs", 90.0),
        ];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert!(result.overall_pass);
        assert_eq!(result.modified_file_results.len(), 1);
        assert_eq!(result.modified_file_results[0].file, "core/src/lib.rs");
    }

    #[test]
    fn test_validate_test_failure_overrides_coverage() {
        let config = make_default_config();
        let test_run = TestRunResult {
            passed: 9,
            failed: 1,
            ignored: 0,
            failures: vec!["tests::broken".to_string()],
            duration_secs: 1.0,
        };
        let all_cov = vec![make_file_cov("core/src/lib.rs", 95.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 95.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 95.0, &modified, &config);
        assert!(!result.overall_pass);
    }

    #[test]
    fn test_validate_test_pass_but_coverage_fail() {
        let config = make_default_config();
        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/lib.rs", 90.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 70.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 70.0, &modified, &config);
        assert!(!result.overall_pass);
    }

    #[test]
    fn test_format_markdown_structure() {
        let result = TestValidationResult {
            overall_pass: true,
            test_run: TestRunResult {
                passed: 100,
                failed: 0,
                ignored: 2,
                failures: Vec::new(),
                duration_secs: 5.0,
            },
            file_coverages: Vec::new(),
            crate_coverages: vec![
                make_crate_cov("codeflow-core", 90.0),
                make_crate_cov("codeflow-cli", 85.0),
            ],
            workspace_coverage: 88.3,
            modified_file_results: vec![ModifiedFileResult {
                file: "core/src/testing/validation.rs".to_string(),
                coverage: 92.0,
                threshold: 85,
                pass: true,
            }],
            exempted_files: Vec::new(),
            new_tests_added: 5,
            consecutive_clean_runs: 2,
            warnings: Vec::new(),
        };

        let md = TestValidator::format_markdown(&result);

        // Heading names per CLAUDE.md PR template.
        assert!(
            md.contains("### 1. Overall Test Pass Status"),
            "missing section 1"
        );
        assert!(md.contains("### 2. Overall Coverage"), "missing section 2");
        assert!(
            md.contains("### 3. Modified File Coverage"),
            "missing section 3"
        );
        // Section 1 uses bullet format, not table.
        assert!(md.contains("- Suite: `cargo test --workspace --no-fail-fast`"));
        assert!(md.contains("- Result: 100 passed, 0 failed, 2 skipped"));
        assert!(md.contains("- New tests added: 5"));
        assert!(md.contains("- Runs: 2 consecutive clean"));
        // Section 2 tables.
        assert!(md.contains("codeflow-cli"));
        assert!(md.contains("codeflow-core"));
        assert!(md.contains("**Workspace**"));
        // Section 3 file entry.
        assert!(md.contains("core/src/testing/validation.rs"));
        // Exempted files section always present, even when empty.
        assert!(
            md.contains("#### Exempted Files (below 85%)"),
            "missing exempted files header"
        );
        assert!(
            md.contains("No exceptions configured."),
            "missing empty exceptions message"
        );
    }

    #[test]
    fn test_format_markdown_exempted_files_table() {
        let result = TestValidationResult {
            overall_pass: true,
            test_run: make_passing_run(50),
            file_coverages: Vec::new(),
            crate_coverages: vec![make_crate_cov("codeflow-core", 90.0)],
            workspace_coverage: 90.0,
            modified_file_results: vec![ModifiedFileResult {
                file: "core/src/autorun/worker.rs".to_string(),
                coverage: 81.0,
                threshold: 80,
                pass: true,
            }],
            exempted_files: vec![ExemptedFile {
                file: "core/src/autorun/worker.rs".to_string(),
                coverage: 81.0,
                configured_threshold: 80,
                reason: "async process spawning".to_string(),
            }],
            new_tests_added: 0,
            consecutive_clean_runs: 1,
            warnings: Vec::new(),
        };

        let md = TestValidator::format_markdown(&result);
        assert!(
            md.contains("#### Exempted Files (below 85%)"),
            "missing exempted files header"
        );
        assert!(md.contains("async process spawning"));
        assert!(md.contains("80%"));
    }

    #[test]
    fn test_write_and_read_artifact() {
        let dir = tempfile::tempdir().unwrap();

        let result = TestValidationResult {
            overall_pass: true,
            test_run: make_passing_run(10),
            file_coverages: Vec::new(),
            crate_coverages: Vec::new(),
            workspace_coverage: 90.0,
            modified_file_results: Vec::new(),
            exempted_files: Vec::new(),
            new_tests_added: 0,
            consecutive_clean_runs: 1,
            warnings: Vec::new(),
        };

        TestValidator::write_artifact(dir.path(), &result).unwrap();
        let read_back = TestValidator::read_artifact(dir.path()).unwrap();
        assert!(read_back.overall_pass);
        assert_eq!(read_back.test_run.passed, 10);
    }

    #[test]
    fn test_read_artifact_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = TestValidator::read_artifact(dir.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("test stats artifact not found"), "got: {msg}");
    }

    #[test]
    fn test_read_artifact_failing() {
        let dir = tempfile::tempdir().unwrap();

        let result = TestValidationResult {
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
            warnings: Vec::new(),
        };

        TestValidator::write_artifact(dir.path(), &result).unwrap();
        let read_back = TestValidator::read_artifact(dir.path());
        assert!(read_back.is_err());
        let msg = read_back.unwrap_err().to_string();
        assert!(msg.contains("overall_pass=false"), "got: {msg}");
    }

    #[test]
    fn test_get_modified_files() {
        // Verify the filter logic with simulated git output.
        let simulated_output = "\
codeflow-cli/core/src/testing/validation.rs
codeflow-cli/cli/src/cmd/test.rs
README.md
.claude/CLAUDE.md
codeflow-cli/core/src/lib.rs
";
        let files: Vec<String> = simulated_output
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && Path::new(trimmed)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                    && trimmed.starts_with("codeflow-cli/")
            })
            .map(|line| {
                line.trim()
                    .strip_prefix("codeflow-cli/")
                    .unwrap_or(line.trim())
                    .to_string()
            })
            .collect();

        assert_eq!(files.len(), 3);
        assert_eq!(files[0], "core/src/testing/validation.rs");
        assert_eq!(files[1], "cli/src/cmd/test.rs");
        assert_eq!(files[2], "core/src/lib.rs");
    }

    #[test]
    fn test_parse_coverage_json() {
        let json = r#"{
            "data": [{
                "files": [
                    {
                        "filename": "core/src/lib.rs",
                        "summary": {
                            "lines": {"count": 100, "covered": 90, "percent": 90.0}
                        }
                    },
                    {
                        "filename": "cli/src/main.rs",
                        "summary": {
                            "lines": {"count": 50, "covered": 40, "percent": 80.0}
                        }
                    }
                ]
            }]
        }"#;

        let (files, crates, ws) = TestValidator::parse_coverage_json(json).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].filename, "core/src/lib.rs");
        assert!((files[0].percent - 90.0).abs() < 0.01);
        assert_eq!(crates.len(), 2);
        // Workspace coverage: (90+40)/(100+50) = 130/150 = 86.67%.
        assert!((ws - 86.666).abs() < 0.1);
    }

    #[test]
    fn test_parse_coverage_json_missing_files() {
        let json = r#"{"data": [{}]}"#;
        let result = TestValidator::parse_coverage_json(json);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing data[0].files"), "got: {msg}");
    }

    #[test]
    fn test_auto_remove_stale_exception() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join("codeflow-cli")
            .join("config")
            .join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();

        // Config with one exception at 80% threshold.
        let config = TestConfig {
            coverage: CoverageConfig {
                file_threshold: 85,
                crate_threshold: 85,
                crate_overrides: HashMap::new(),
                enforcement: "per_file".to_string(),
                tool: "cargo-llvm-cov".to_string(),
                business_packages: vec!["codeflow-core".to_string()],
            },
            conventions: ConventionsConfig {
                test_runner: "cargo-nextest".to_string(),
                coverage_tool: "cargo-llvm-cov".to_string(),
                property_testing: "proptest".to_string(),
                snapshot_testing: "insta".to_string(),
                exceptions: vec![FileException {
                    file: "core/src/autorun/orchestrator.rs".to_string(),
                    threshold: 80,
                    reason: "async macro expansion".to_string(),
                    granularity: "per_file".to_string(),
                    remove_when: "refactored".to_string(),
                }],
            },
        };

        // Write original config to disk.
        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(config_dir.join("test-config.json"), &json).unwrap();

        // Coverage data shows the file at 90% — above 85% global threshold.
        let file_coverages = vec![FileCoverage {
            filename: "core/src/autorun/orchestrator.rs".to_string(),
            lines_count: 100,
            lines_covered: 90,
            percent: 90.0,
        }];

        let updated = TestValidator::prune_stale_exceptions(dir.path(), config, &file_coverages);

        // Exception should be removed from the returned config.
        assert!(
            updated.conventions.exceptions.is_empty(),
            "stale exception should be pruned, got: {:?}",
            updated.conventions.exceptions
        );

        // Config file on disk should be rewritten without the exception.
        let reloaded = TestValidator::load_config(dir.path());
        assert!(
            reloaded.conventions.exceptions.is_empty(),
            "reloaded config should have no exceptions"
        );
    }

    #[test]
    fn test_prune_keeps_valid_exceptions() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir
            .path()
            .join("codeflow-cli")
            .join("config")
            .join("testing");
        std::fs::create_dir_all(&config_dir).unwrap();

        let config = TestConfig {
            coverage: CoverageConfig {
                file_threshold: 85,
                crate_threshold: 85,
                crate_overrides: HashMap::new(),
                enforcement: "per_file".to_string(),
                tool: "cargo-llvm-cov".to_string(),
                business_packages: vec!["codeflow-core".to_string()],
            },
            conventions: ConventionsConfig {
                test_runner: "cargo-nextest".to_string(),
                coverage_tool: "cargo-llvm-cov".to_string(),
                property_testing: "proptest".to_string(),
                snapshot_testing: "insta".to_string(),
                exceptions: vec![FileException {
                    file: "core/src/session/process.rs".to_string(),
                    threshold: 78,
                    reason: "platform-conditional code".to_string(),
                    granularity: "per_file".to_string(),
                    remove_when: "CI on both platforms".to_string(),
                }],
            },
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(config_dir.join("test-config.json"), &json).unwrap();

        // File still below 85% — exception should be kept.
        let file_coverages = vec![FileCoverage {
            filename: "core/src/session/process.rs".to_string(),
            lines_count: 100,
            lines_covered: 80,
            percent: 80.0,
        }];

        let updated = TestValidator::prune_stale_exceptions(dir.path(), config, &file_coverages);
        assert_eq!(
            updated.conventions.exceptions.len(),
            1,
            "valid exception should be kept"
        );
    }

    #[test]
    fn test_validate_exempted_files_all_config_exceptions() {
        // Verify that ALL config exceptions appear in exempted_files,
        // even when none of them are in the modified files list.
        let mut config = make_default_config();
        config.conventions.exceptions = vec![
            FileException {
                file: "core/src/autorun/worker.rs".to_string(),
                threshold: 80,
                reason: "async process spawning".to_string(),
                granularity: "per_file".to_string(),
                remove_when: "refactored".to_string(),
            },
            FileException {
                file: "cli/src/cmd/interactive.rs".to_string(),
                threshold: 80,
                reason: "exec(2) replaces process".to_string(),
                granularity: "per_file".to_string(),
                remove_when: "injectable exec".to_string(),
            },
            FileException {
                file: "core/src/tui/mod.rs".to_string(),
                threshold: 0,
                reason: "pure module declaration".to_string(),
                granularity: "per_file".to_string(),
                remove_when: "gains code".to_string(),
            },
        ];

        let test_run = make_passing_run(10);
        // Only one exception file appears in coverage data.
        let all_cov = vec![
            make_file_cov("core/src/autorun/worker.rs", 82.0),
            make_file_cov("core/src/lib.rs", 95.0),
        ];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        // No modified files at all — exempted_files should still list ALL 3 exceptions.
        let modified: Vec<String> = vec![];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert_eq!(
            result.exempted_files.len(),
            3,
            "all 3 config exceptions should appear in exempted_files"
        );
        assert_eq!(result.exempted_files[0].file, "core/src/autorun/worker.rs");
        assert!((result.exempted_files[0].coverage - 82.0).abs() < 0.01);
        assert_eq!(result.exempted_files[1].file, "cli/src/cmd/interactive.rs");
        assert!(
            result.exempted_files[1].coverage < 0.0,
            "file not in coverage data should have coverage < 0 (N/A)"
        );
        assert_eq!(result.exempted_files[2].file, "core/src/tui/mod.rs");
        assert!(
            result.exempted_files[2].coverage < 0.0,
            "file not in coverage data should have coverage < 0 (N/A)"
        );
    }

    #[test]
    fn test_format_markdown_all_exceptions_including_na() {
        // Verify format_markdown renders all exceptions including N/A coverage.
        let result = TestValidationResult {
            overall_pass: true,
            test_run: make_passing_run(50),
            file_coverages: Vec::new(),
            crate_coverages: vec![make_crate_cov("codeflow-core", 90.0)],
            workspace_coverage: 90.0,
            modified_file_results: Vec::new(),
            exempted_files: vec![
                ExemptedFile {
                    file: "core/src/autorun/worker.rs".to_string(),
                    coverage: 82.0,
                    configured_threshold: 80,
                    reason: "async process spawning".to_string(),
                },
                ExemptedFile {
                    file: "core/src/tui/mod.rs".to_string(),
                    coverage: -1.0, // Not in coverage data.
                    configured_threshold: 0,
                    reason: "pure module declaration".to_string(),
                },
            ],
            new_tests_added: 0,
            consecutive_clean_runs: 1,
            warnings: Vec::new(),
        };

        let md = TestValidator::format_markdown(&result);
        assert!(
            md.contains("#### Exempted Files (below 85%)"),
            "missing exempted files header"
        );
        assert!(
            md.contains("All project-wide coverage exceptions"),
            "missing description text"
        );
        assert!(md.contains("82.0%"), "should show actual coverage");
        assert!(md.contains("N/A"), "should show N/A for missing coverage");
        assert!(md.contains("pure module declaration"));
        assert!(md.contains("async process spawning"));
    }

    #[test]
    fn test_validate_warnings_reserved_field_empty() {
        // Warnings field is reserved for future validation warnings; currently always empty.
        let mut config = make_default_config();
        config.conventions.exceptions = vec![FileException {
            file: "core/src/autorun/worker.rs".to_string(),
            threshold: 80,
            reason: "async".to_string(),
            granularity: "per_file".to_string(),
            remove_when: "later".to_string(),
        }];

        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/autorun/worker.rs", 82.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified: Vec<String> = vec![];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_validate_empty_exceptions_empty_exempted() {
        // Empty exceptions list → empty exempted files, no warning.
        let config = make_default_config();
        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/lib.rs", 90.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);
        assert!(
            result.exempted_files.is_empty(),
            "no exceptions = no exempted files"
        );
        assert!(result.warnings.is_empty(), "no warnings for empty config");
    }

    #[test]
    fn test_format_markdown_empty_exceptions() {
        // When no exceptions configured, show "No exceptions configured."
        let result = TestValidationResult {
            overall_pass: true,
            test_run: make_passing_run(10),
            file_coverages: Vec::new(),
            crate_coverages: vec![make_crate_cov("codeflow-core", 90.0)],
            workspace_coverage: 90.0,
            modified_file_results: Vec::new(),
            exempted_files: Vec::new(),
            new_tests_added: 0,
            consecutive_clean_runs: 1,
            warnings: Vec::new(),
        };

        let md = TestValidator::format_markdown(&result);
        assert!(
            md.contains("#### Exempted Files (below 85%)"),
            "header always present"
        );
        assert!(
            md.contains("No exceptions configured."),
            "empty state message"
        );
        // The exempted files section should not contain its own table header.
        // (Modified File Coverage section has its own "| File | Coverage |" table.)
        assert!(
            !md.contains("| File | Coverage | Configured Threshold | Reason |"),
            "no exempted files table when empty"
        );
    }

    #[test]
    fn test_validate_missing_coverage_consistent_sentinel() {
        // A file in both modified_files AND exceptions but NOT in coverage data
        // should get consistent -1.0 sentinel in both tables, rendered as "N/A".
        let mut config = make_default_config();
        config.conventions.exceptions = vec![FileException {
            file: "core/src/tui/mod.rs".to_string(),
            threshold: 0,
            reason: "pure module declaration".to_string(),
            granularity: "per_file".to_string(),
            remove_when: "gains code".to_string(),
        }];

        let test_run = make_passing_run(10);
        let all_cov = vec![]; // No coverage data at all.
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/tui/mod.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);

        // Modified file should have -1.0 sentinel.
        assert_eq!(result.modified_file_results.len(), 1);
        assert!(
            result.modified_file_results[0].coverage < 0.0,
            "modified file missing from coverage should be -1.0, got {}",
            result.modified_file_results[0].coverage
        );

        // Exempted file should also have -1.0 sentinel.
        assert_eq!(result.exempted_files.len(), 1);
        assert!(
            result.exempted_files[0].coverage < 0.0,
            "exempted file missing from coverage should be -1.0, got {}",
            result.exempted_files[0].coverage
        );

        // Both sentinels are the same value.
        assert!(
            (result.modified_file_results[0].coverage - result.exempted_files[0].coverage).abs()
                < f64::EPSILON,
            "sentinel values must be consistent"
        );

        // format_markdown should render "N/A" in both tables.
        let md = TestValidator::format_markdown(&result);
        let na_count = md.matches("N/A").count();
        assert!(
            na_count >= 2,
            "expected N/A in both modified and exempted tables, found {na_count} occurrences"
        );
    }

    #[test]
    fn test_validate_crate_override_lowers_threshold() {
        let mut config = make_default_config();
        config
            .coverage
            .business_packages
            .push("codeflow-cli".to_string());
        config
            .coverage
            .crate_overrides
            .insert("codeflow-cli".to_string(), 80);

        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("cli/src/main.rs", 90.0)];
        // CLI crate at 82% — below crate_threshold (85) but above override (80).
        let crate_cov = vec![
            make_crate_cov("codeflow-core", 90.0),
            make_crate_cov("codeflow-cli", 82.0),
        ];
        let modified = vec!["cli/src/main.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);

        let cli_crate = result
            .crate_coverages
            .iter()
            .find(|c| c.name == "codeflow-cli")
            .expect("should have codeflow-cli crate");
        assert_eq!(
            cli_crate.threshold, 80,
            "crate override should set threshold to 80"
        );
        assert!(
            cli_crate.percent >= f64::from(cli_crate.threshold),
            "82% should pass 80% override threshold"
        );
        assert!(
            result.overall_pass,
            "overall should pass with crate override"
        );
    }

    #[test]
    fn test_validate_crate_override_does_not_affect_other_crates() {
        let mut config = make_default_config();
        config
            .coverage
            .crate_overrides
            .insert("codeflow-cli".to_string(), 80);

        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/lib.rs", 90.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);

        let core_crate = result
            .crate_coverages
            .iter()
            .find(|c| c.name == "codeflow-core")
            .expect("should have codeflow-core crate");
        assert_eq!(
            core_crate.threshold, 85,
            "core should use crate_threshold, not cli override"
        );
    }

    #[test]
    fn test_validate_no_crate_override_uses_crate_threshold() {
        let config = make_default_config();
        let test_run = make_passing_run(10);
        let all_cov = vec![make_file_cov("core/src/lib.rs", 90.0)];
        let crate_cov = vec![make_crate_cov("codeflow-core", 90.0)];
        let modified = vec!["core/src/lib.rs".to_string()];

        let result =
            TestValidator::validate(&test_run, &all_cov, &crate_cov, 90.0, &modified, &config);

        let core_crate = result
            .crate_coverages
            .iter()
            .find(|c| c.name == "codeflow-core")
            .expect("should have codeflow-core crate");
        assert_eq!(
            core_crate.threshold, 85,
            "no override should use crate_threshold from config"
        );
    }

    // ── Test helpers ───────────────────────────────────────────────────────

    fn make_default_config() -> TestConfig {
        TestConfig {
            coverage: CoverageConfig {
                file_threshold: 85,
                crate_threshold: 85,
                crate_overrides: HashMap::new(),
                enforcement: "per_file".to_string(),
                tool: "cargo-llvm-cov".to_string(),
                business_packages: vec!["codeflow-core".to_string()],
            },
            conventions: ConventionsConfig {
                test_runner: "cargo-nextest".to_string(),
                coverage_tool: "cargo-llvm-cov".to_string(),
                property_testing: "proptest".to_string(),
                snapshot_testing: "insta".to_string(),
                exceptions: Vec::new(),
            },
        }
    }

    fn make_passing_run(passed: u32) -> TestRunResult {
        TestRunResult {
            passed,
            failed: 0,
            ignored: 0,
            failures: Vec::new(),
            duration_secs: 1.0,
        }
    }

    fn make_file_cov(filename: &str, percent: f64) -> FileCoverage {
        let lines_count: u64 = 100;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let lines_covered = (percent * lines_count as f64 / 100.0) as u64;
        FileCoverage {
            filename: filename.to_string(),
            lines_count,
            lines_covered,
            percent,
        }
    }

    fn make_crate_cov(name: &str, percent: f64) -> CrateCoverage {
        let lines_count: u64 = 100;
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let lines_covered = (percent * lines_count as f64 / 100.0) as u64;
        let threshold = 85; // matches CoverageConfig.crate_threshold default
        CrateCoverage {
            name: name.to_string(),
            lines_count,
            lines_covered,
            percent,
            threshold,
        }
    }
}
