//! CTRF native reader.
//!
//! Parses native CTRF JSON files into `CanonicalTestReport`.

use std::path::Path;

use crate::testing::error::TestingError;
use crate::testing::report::CanonicalTestReport;

/// Parse a native CTRF JSON file into a `CanonicalTestReport`.
///
/// # Errors
///
/// Returns `TestingError::CtrfParseError` on invalid JSON or missing
/// required fields.
pub fn parse_ctrf(path: &Path) -> Result<CanonicalTestReport, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::CtrfParseError {
        path: path.to_path_buf(),
        message: format!("failed to read file: {e}"),
    })?;

    parse_ctrf_str(&content, path)
}

/// Parse CTRF JSON from a string (for testing without file I/O).
pub(crate) fn parse_ctrf_str(
    json: &str,
    source_path: &Path,
) -> Result<CanonicalTestReport, TestingError> {
    serde_json::from_str::<CanonicalTestReport>(json).map_err(|e| TestingError::CtrfParseError {
        path: source_path.to_path_buf(),
        message: format!("{e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn test_parse_ctrf_valid() {
        let json = r#"{
            "results": {
                "tool": {"name": "jest"},
                "summary": {"total": 2, "passed": 2, "failed": 0, "skipped": 0},
                "tests": [
                    {"name": "test_a", "status": "passed", "duration": 10.0},
                    {"name": "test_b", "status": "passed", "duration": 20.0}
                ]
            }
        }"#;
        let report = parse_ctrf_str(json, &path("report.json")).unwrap();
        assert_eq!(report.results.tool.name, "jest");
        assert_eq!(report.results.summary.total, 2);
        assert_eq!(report.results.summary.passed, 2);
        assert_eq!(report.results.tests.len(), 2);
    }

    #[test]
    fn test_parse_ctrf_with_failures() {
        let json = r#"{
            "results": {
                "tool": {"name": "vitest"},
                "summary": {"total": 2, "passed": 1, "failed": 1, "skipped": 0},
                "tests": [
                    {"name": "pass", "status": "passed", "duration": 5.0},
                    {"name": "fail", "status": "failed", "duration": 3.0, "message": "expected 1, got 2"}
                ]
            }
        }"#;
        let report = parse_ctrf_str(json, &path("report.json")).unwrap();
        assert_eq!(report.results.summary.failed, 1);
        let failed = &report.results.tests[1];
        assert_eq!(failed.message.as_deref(), Some("expected 1, got 2"));
    }

    #[test]
    fn test_parse_ctrf_with_environment() {
        let json = r#"{
            "results": {
                "tool": {"name": "pytest", "version": "7.4"},
                "summary": {"total": 1, "passed": 1, "failed": 0, "skipped": 0},
                "tests": [{"name": "t", "status": "passed", "duration": 1.0}]
            },
            "environment": {
                "os": "linux",
                "os_version": "6.1"
            }
        }"#;
        let report = parse_ctrf_str(json, &path("report.json")).unwrap();
        assert_eq!(report.results.tool.version.as_deref(), Some("7.4"));
        let env = report.environment.unwrap();
        assert_eq!(env.os.as_deref(), Some("linux"));
    }

    #[test]
    fn test_parse_ctrf_invalid_json() {
        let result = parse_ctrf_str("not json", &path("bad.json"));
        assert!(result.is_err());
        match result.unwrap_err() {
            TestingError::CtrfParseError { path: p, message } => {
                assert_eq!(p, PathBuf::from("bad.json"));
                assert!(!message.is_empty());
            }
            other => panic!("expected CtrfParseError, got: {other}"),
        }
    }

    #[test]
    fn test_parse_ctrf_missing_results() {
        let json = r#"{"environment": {"os": "linux"}}"#;
        let result = parse_ctrf_str(json, &path("missing.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_ctrf_file_not_found() {
        let result = parse_ctrf(&PathBuf::from("/nonexistent/report.json"));
        assert!(result.is_err());
        match result.unwrap_err() {
            TestingError::CtrfParseError { message, .. } => {
                assert!(message.contains("failed to read file"));
            }
            other => panic!("expected CtrfParseError, got: {other}"),
        }
    }

    #[test]
    fn test_parse_ctrf_roundtrip() {
        let json = r#"{
            "results": {
                "tool": {"name": "cargo-nextest"},
                "summary": {"total": 3, "passed": 2, "failed": 1, "skipped": 0, "pending": 0, "other": 0, "start": 1000, "stop": 2000},
                "tests": [
                    {"name": "t1", "status": "passed", "duration": 10.0, "suite": "mod_a"},
                    {"name": "t2", "status": "passed", "duration": 20.0},
                    {"name": "t3", "status": "failed", "duration": 5.0, "message": "boom", "trace": "line 10"}
                ]
            }
        }"#;
        let report = parse_ctrf_str(json, &path("test.json")).unwrap();
        let re_json = serde_json::to_string_pretty(&report).unwrap();
        let reparsed = parse_ctrf_str(&re_json, &path("test.json")).unwrap();
        assert_eq!(reparsed.results.summary.total, 3);
        assert_eq!(reparsed.results.tests[2].message.as_deref(), Some("boom"));
        assert_eq!(reparsed.results.tests[2].trace.as_deref(), Some("line 10"));
    }

    #[test]
    fn test_parse_ctrf_with_extra_fields() {
        let json = r#"{
            "results": {
                "tool": {"name": "test"},
                "summary": {"total": 0, "passed": 0, "failed": 0, "skipped": 0},
                "tests": []
            },
            "extra": {"custom": "data"}
        }"#;
        let report = parse_ctrf_str(json, &path("test.json")).unwrap();
        assert_eq!(report.extra["custom"], "data");
    }
}
