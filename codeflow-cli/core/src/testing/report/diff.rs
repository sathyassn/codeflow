//! Test report diff: compare two test runs and identify regressions.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{CanonicalTestReport, CtrfStatus};
use crate::testing::error::TestingError;

/// A single regression or improvement between two runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestDiffEntry {
    pub test_name: String,
    pub change: DiffChange,
    pub from_status: String,
    pub to_status: String,
}

/// Type of change between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffChange {
    NewFailure,
    Fixed,
    NewTest,
    Removed,
}

/// Coverage change for a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageDiffEntry {
    pub file: String,
    pub from_percent: f64,
    pub to_percent: f64,
    pub delta: f64,
}

/// Complete diff result between two runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffResult {
    pub from_run_id: String,
    pub to_run_id: String,
    pub test_changes: Vec<TestDiffEntry>,
    pub coverage_changes: Vec<CoverageDiffEntry>,
    pub total_regressions: usize,
}

/// Compute the diff between two `CanonicalTestReport` instances.
#[must_use]
pub fn diff_reports(
    from: &CanonicalTestReport,
    to: &CanonicalTestReport,
    from_run_id: &str,
    to_run_id: &str,
) -> DiffResult {
    let mut test_changes = Vec::new();

    // Build lookup maps: test_name -> status
    let from_tests: HashMap<&str, &CtrfStatus> = from
        .results
        .tests
        .iter()
        .map(|t| (t.name.as_str(), &t.status))
        .collect();
    let to_tests: HashMap<&str, &CtrfStatus> = to
        .results
        .tests
        .iter()
        .map(|t| (t.name.as_str(), &t.status))
        .collect();

    // Find new failures: passed in `from`, failed in `to`
    for (name, to_status) in &to_tests {
        match from_tests.get(name) {
            Some(from_status) => {
                let was_passing =
                    **from_status == CtrfStatus::Passed || **from_status == CtrfStatus::Skipped;
                let now_failing = **to_status == CtrfStatus::Failed;
                if was_passing && now_failing {
                    test_changes.push(TestDiffEntry {
                        test_name: (*name).to_string(),
                        change: DiffChange::NewFailure,
                        from_status: format!("{from_status:?}").to_lowercase(),
                        to_status: "failed".to_string(),
                    });
                }
                let was_failing = **from_status == CtrfStatus::Failed;
                let now_passing = **to_status == CtrfStatus::Passed;
                if was_failing && now_passing {
                    test_changes.push(TestDiffEntry {
                        test_name: (*name).to_string(),
                        change: DiffChange::Fixed,
                        from_status: "failed".to_string(),
                        to_status: "passed".to_string(),
                    });
                }
            }
            None => {
                // New test in `to` that wasn't in `from`
                if **to_status == CtrfStatus::Failed {
                    test_changes.push(TestDiffEntry {
                        test_name: (*name).to_string(),
                        change: DiffChange::NewTest,
                        from_status: "n/a".to_string(),
                        to_status: "failed".to_string(),
                    });
                }
            }
        }
    }

    // Find removed tests (in `from` but not in `to`)
    for (name, from_status) in &from_tests {
        if !to_tests.contains_key(name) && **from_status == CtrfStatus::Failed {
            test_changes.push(TestDiffEntry {
                test_name: (*name).to_string(),
                change: DiffChange::Removed,
                from_status: "failed".to_string(),
                to_status: "n/a".to_string(),
            });
        }
    }

    let total_regressions = test_changes
        .iter()
        .filter(|c| c.change == DiffChange::NewFailure)
        .count();

    DiffResult {
        from_run_id: from_run_id.to_string(),
        to_run_id: to_run_id.to_string(),
        test_changes,
        coverage_changes: Vec::new(),
        total_regressions,
    }
}

/// Format a diff result as human-readable text.
#[must_use]
pub fn format_diff_human(diff: &DiffResult) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "Diff: {} -> {}\n", diff.from_run_id, diff.to_run_id);

    if diff.test_changes.is_empty() && diff.coverage_changes.is_empty() {
        let _ = writeln!(out, "No regressions detected.");
        return out;
    }

    if !diff.test_changes.is_empty() {
        let _ = writeln!(out, "Test Changes:");
        let header = format!("{:<12} {:<50} {:<10} To", "Change", "Test", "From");
        let _ = writeln!(out, "{header}");
        let _ = writeln!(out, "{}", "-".repeat(85));
        for entry in &diff.test_changes {
            let change_str = match entry.change {
                DiffChange::NewFailure => "REGRESSION",
                DiffChange::Fixed => "FIXED",
                DiffChange::NewTest => "NEW",
                DiffChange::Removed => "REMOVED",
            };
            let _ = writeln!(
                out,
                "{change_str:<12} {:<50} {:<10} {}",
                entry.test_name, entry.from_status, entry.to_status
            );
        }
        let _ = writeln!(out);
    }

    if !diff.coverage_changes.is_empty() {
        let _ = writeln!(out, "Coverage Changes:");
        let _ = writeln!(
            out,
            "{:<50} {:>8} {:>8} {:>8}",
            "File", "From", "To", "Delta"
        );
        let _ = writeln!(out, "{}", "-".repeat(80));
        for entry in &diff.coverage_changes {
            let _ = writeln!(
                out,
                "{:<50} {:>7.1}% {:>7.1}% {:>+7.1}%",
                entry.file, entry.from_percent, entry.to_percent, entry.delta
            );
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "Total regressions: {}", diff.total_regressions);
    out
}

/// Read a `test_result_recorded` event from the ledger by run_id.
///
/// Scans all JSONL files in the work-graph ledger directory for events
/// with `event: "test_result_recorded"` and matching `run_id`.
///
/// # Errors
///
/// Returns `TestingError::ReportNotFound` if no event with the given run_id exists.
pub fn read_run_from_ledger(
    ledger_dir: &Path,
    run_id: &str,
) -> Result<serde_json::Value, TestingError> {
    let wg_dir = ledger_dir.join("work-graph");
    if !wg_dir.exists() {
        return Err(TestingError::ReportNotFound(
            format!("run {run_id} not found in ledger").into(),
        ));
    }

    // Scan all .jsonl files in work-graph/
    let entries = std::fs::read_dir(&wg_dir).map_err(TestingError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let content = std::fs::read_to_string(&path).map_err(TestingError::Io)?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                if val.get("event").and_then(|e| e.as_str()) == Some("test_result_recorded")
                    && val.get("run_id").and_then(|r| r.as_str()) == Some(run_id)
                {
                    return Ok(val);
                }
            }
        }
    }

    Err(TestingError::ReportNotFound(
        format!("run {run_id} not found in ledger").into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::report::{CanonicalTestReport, CtrfTest};

    fn make_test(name: &str, status: CtrfStatus) -> CtrfTest {
        CtrfTest {
            name: name.to_string(),
            status,
            duration: 1.0,
            suite: None,
            message: None,
            trace: None,
            tags: vec![],
            flaky: false,
        }
    }

    #[test]
    fn test_diff_no_regressions() {
        let from = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Passed),
            ],
        );
        let to = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Passed),
            ],
        );
        let diff = diff_reports(&from, &to, "run-1", "run-2");
        assert!(diff.test_changes.is_empty());
        assert_eq!(diff.total_regressions, 0);
    }

    #[test]
    fn test_diff_new_failure_detected() {
        let from = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Passed),
            ],
        );
        let to = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Failed),
            ],
        );
        let diff = diff_reports(&from, &to, "run-1", "run-2");
        assert_eq!(diff.test_changes.len(), 1);
        assert_eq!(diff.test_changes[0].test_name, "b");
        assert_eq!(diff.test_changes[0].change, DiffChange::NewFailure);
        assert_eq!(diff.total_regressions, 1);
    }

    #[test]
    fn test_diff_fixed_test_detected() {
        let from = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Failed),
            ],
        );
        let to = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Passed),
            ],
        );
        let diff = diff_reports(&from, &to, "run-1", "run-2");
        assert_eq!(diff.test_changes.len(), 1);
        assert_eq!(diff.test_changes[0].change, DiffChange::Fixed);
        assert_eq!(diff.total_regressions, 0);
    }

    #[test]
    fn test_diff_new_failing_test() {
        let from = CanonicalTestReport::new("test", vec![make_test("a", CtrfStatus::Passed)]);
        let to = CanonicalTestReport::new(
            "test",
            vec![
                make_test("a", CtrfStatus::Passed),
                make_test("b", CtrfStatus::Failed),
            ],
        );
        let diff = diff_reports(&from, &to, "run-1", "run-2");
        let new_test = diff
            .test_changes
            .iter()
            .find(|c| c.change == DiffChange::NewTest);
        assert!(new_test.is_some());
        assert_eq!(new_test.unwrap().test_name, "b");
    }

    #[test]
    fn test_diff_run_id_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        std::fs::create_dir_all(ledger_dir.join("work-graph")).unwrap();
        std::fs::write(
            ledger_dir.join("work-graph/work-graph.jsonl"),
            r#"{"event":"task_created","timestamp":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let result = read_run_from_ledger(&ledger_dir, "nonexistent-run");
        assert!(result.is_err());
        match result.unwrap_err() {
            TestingError::ReportNotFound(p) => {
                assert!(p.to_string_lossy().contains("not found"));
            }
            other => panic!("expected ReportNotFound, got: {other}"),
        }
    }

    #[test]
    fn test_diff_run_found_in_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let ledger_dir = dir.path().join("ledger");
        std::fs::create_dir_all(ledger_dir.join("work-graph")).unwrap();
        std::fs::write(
            ledger_dir.join("work-graph/work-graph.jsonl"),
            r#"{"event":"test_result_recorded","timestamp":"2026-01-01T00:00:00Z","run_id":"run-abc","overall_pass":true,"total_passed":10}"#,
        )
        .unwrap();

        let result = read_run_from_ledger(&ledger_dir, "run-abc");
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["run_id"], "run-abc");
        assert_eq!(val["total_passed"], 10);
    }

    #[test]
    fn test_diff_ledger_dir_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = read_run_from_ledger(&dir.path().join("nonexistent"), "run-1");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_diff_no_regressions() {
        let diff = DiffResult {
            from_run_id: "run-1".to_string(),
            to_run_id: "run-2".to_string(),
            test_changes: vec![],
            coverage_changes: vec![],
            total_regressions: 0,
        };
        let output = format_diff_human(&diff);
        assert!(output.contains("No regressions detected"));
    }

    #[test]
    fn test_format_diff_with_regressions() {
        let diff = DiffResult {
            from_run_id: "run-1".to_string(),
            to_run_id: "run-2".to_string(),
            test_changes: vec![TestDiffEntry {
                test_name: "test_broken".to_string(),
                change: DiffChange::NewFailure,
                from_status: "passed".to_string(),
                to_status: "failed".to_string(),
            }],
            coverage_changes: vec![],
            total_regressions: 1,
        };
        let output = format_diff_human(&diff);
        assert!(output.contains("REGRESSION"));
        assert!(output.contains("test_broken"));
        assert!(output.contains("Total regressions: 1"));
    }

    #[test]
    fn test_diff_result_serde_roundtrip() {
        let diff = DiffResult {
            from_run_id: "run-1".to_string(),
            to_run_id: "run-2".to_string(),
            test_changes: vec![TestDiffEntry {
                test_name: "t".to_string(),
                change: DiffChange::NewFailure,
                from_status: "passed".to_string(),
                to_status: "failed".to_string(),
            }],
            coverage_changes: vec![CoverageDiffEntry {
                file: "src/main.rs".to_string(),
                from_percent: 90.0,
                to_percent: 80.0,
                delta: -10.0,
            }],
            total_regressions: 1,
        };
        let json = serde_json::to_string(&diff).unwrap();
        let parsed: DiffResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_regressions, 1);
        assert_eq!(parsed.test_changes[0].test_name, "t");
        assert_eq!(parsed.coverage_changes[0].file, "src/main.rs");
    }
}
