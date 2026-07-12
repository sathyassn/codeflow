//! Canonical test report model (CTRF-shaped).
//!
//! This module defines the internal representation of test results using
//! a shape inspired by the Common Test Report Format (CTRF). All downstream
//! consumers (PR body, ledger, diff, external reporters) read this model.

pub mod ctrf;
pub mod junit;
pub mod junit_to_ctrf;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Status of an individual test case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CtrfStatus {
    Passed,
    Failed,
    Skipped,
    Pending,
    Other,
}

/// An individual test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrfTest {
    /// Fully qualified test name.
    pub name: String,

    /// Pass/fail/skip status.
    pub status: CtrfStatus,

    /// Duration in milliseconds.
    #[serde(default)]
    pub duration: f64,

    /// Test suite name (e.g., module path, class name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suite: Option<String>,

    /// Failure or error message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Stack trace or detailed output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,

    /// Arbitrary tags for filtering/grouping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Whether this test is known to be flaky.
    #[serde(default)]
    pub flaky: bool,
}

/// Aggregate summary of a test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrfSummary {
    pub total: u64,
    pub passed: u64,
    pub failed: u64,
    pub skipped: u64,
    #[serde(default)]
    pub pending: u64,
    #[serde(default)]
    pub other: u64,

    /// Run start time as epoch milliseconds.
    #[serde(default)]
    pub start: u64,

    /// Run stop time as epoch milliseconds.
    #[serde(default)]
    pub stop: u64,
}

/// Environment metadata for the test run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CtrfEnvironment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_number: Option<String>,
}

/// The results section of a CTRF report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrfResults {
    /// Tool that produced the results.
    pub tool: CtrfTool,

    /// Aggregate counts.
    pub summary: CtrfSummary,

    /// Individual test results.
    pub tests: Vec<CtrfTest>,
}

/// Tool metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrfTool {
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Canonical test report — CTRF-shaped.
///
/// This is the single data structure every downstream consumer reads.
/// Input adapters (`JUnit`, native CTRF) convert into this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTestReport {
    /// The results section containing tool, summary, and tests.
    pub results: CtrfResults,

    /// Environment metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<CtrfEnvironment>,

    /// Extra fields for forward compatibility.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl CanonicalTestReport {
    /// Create a new report with the given tool name and test results.
    #[must_use]
    pub fn new(tool_name: &str, tests: Vec<CtrfTest>) -> Self {
        let mut passed = 0u64;
        let mut failed = 0u64;
        let mut skipped = 0u64;
        let mut pending = 0u64;
        let mut other = 0u64;

        for t in &tests {
            match t.status {
                CtrfStatus::Passed => passed += 1,
                CtrfStatus::Failed => failed += 1,
                CtrfStatus::Skipped => skipped += 1,
                CtrfStatus::Pending => pending += 1,
                CtrfStatus::Other => other += 1,
            }
        }

        let total = passed + failed + skipped + pending + other;

        Self {
            results: CtrfResults {
                tool: CtrfTool {
                    name: tool_name.to_string(),
                    version: None,
                },
                summary: CtrfSummary {
                    total,
                    passed,
                    failed,
                    skipped,
                    pending,
                    other,
                    start: 0,
                    stop: 0,
                },
                tests,
            },
            environment: None,
            extra: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ctrf_status_serde_roundtrip() {
        for status in &[
            CtrfStatus::Passed,
            CtrfStatus::Failed,
            CtrfStatus::Skipped,
            CtrfStatus::Pending,
            CtrfStatus::Other,
        ] {
            let json = serde_json::to_string(status).unwrap();
            let parsed: CtrfStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, status);
        }
    }

    #[test]
    fn test_ctrf_status_serialize_values() {
        assert_eq!(
            serde_json::to_string(&CtrfStatus::Passed).unwrap(),
            "\"passed\""
        );
        assert_eq!(
            serde_json::to_string(&CtrfStatus::Failed).unwrap(),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&CtrfStatus::Skipped).unwrap(),
            "\"skipped\""
        );
        assert_eq!(
            serde_json::to_string(&CtrfStatus::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&CtrfStatus::Other).unwrap(),
            "\"other\""
        );
    }

    #[test]
    fn test_ctrf_test_minimal() {
        let test = CtrfTest {
            name: "test_add".to_string(),
            status: CtrfStatus::Passed,
            duration: 12.5,
            suite: None,
            message: None,
            trace: None,
            tags: Vec::new(),
            flaky: false,
        };
        let json = serde_json::to_string(&test).unwrap();
        let parsed: CtrfTest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "test_add");
        assert_eq!(parsed.status, CtrfStatus::Passed);
        assert!((parsed.duration - 12.5).abs() < f64::EPSILON);
        assert!(parsed.suite.is_none());
        assert!(parsed.message.is_none());
        assert!(parsed.trace.is_none());
        assert!(parsed.tags.is_empty());
        assert!(!parsed.flaky);
    }

    #[test]
    fn test_ctrf_test_with_failure() {
        let test = CtrfTest {
            name: "test_broken".to_string(),
            status: CtrfStatus::Failed,
            duration: 1.0,
            suite: Some("math::tests".to_string()),
            message: Some("assertion failed: 2 + 2 != 5".to_string()),
            trace: Some("at line 42".to_string()),
            tags: vec!["unit".to_string()],
            flaky: false,
        };
        let json = serde_json::to_string(&test).unwrap();
        let parsed: CtrfTest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, CtrfStatus::Failed);
        assert_eq!(parsed.suite.as_deref(), Some("math::tests"));
        assert!(
            parsed
                .message
                .as_ref()
                .unwrap()
                .contains("assertion failed")
        );
        assert_eq!(parsed.tags.len(), 1);
    }

    #[test]
    fn test_ctrf_summary_serde_roundtrip() {
        let summary = CtrfSummary {
            total: 100,
            passed: 90,
            failed: 5,
            skipped: 3,
            pending: 1,
            other: 1,
            start: 1_000_000,
            stop: 1_100_000,
        };
        let json = serde_json::to_string(&summary).unwrap();
        let parsed: CtrfSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total, 100);
        assert_eq!(parsed.passed, 90);
        assert_eq!(parsed.failed, 5);
        assert_eq!(parsed.skipped, 3);
        assert_eq!(parsed.pending, 1);
        assert_eq!(parsed.other, 1);
        assert_eq!(parsed.start, 1_000_000);
        assert_eq!(parsed.stop, 1_100_000);
    }

    #[test]
    fn test_canonical_report_new_computes_summary() {
        let tests = vec![
            CtrfTest {
                name: "a".to_string(),
                status: CtrfStatus::Passed,
                duration: 1.0,
                suite: None,
                message: None,
                trace: None,
                tags: Vec::new(),
                flaky: false,
            },
            CtrfTest {
                name: "b".to_string(),
                status: CtrfStatus::Failed,
                duration: 2.0,
                suite: None,
                message: Some("fail".to_string()),
                trace: None,
                tags: Vec::new(),
                flaky: false,
            },
            CtrfTest {
                name: "c".to_string(),
                status: CtrfStatus::Skipped,
                duration: 0.0,
                suite: None,
                message: None,
                trace: None,
                tags: Vec::new(),
                flaky: false,
            },
        ];
        let report = CanonicalTestReport::new("cargo-nextest", tests);
        assert_eq!(report.results.tool.name, "cargo-nextest");
        assert_eq!(report.results.summary.total, 3);
        assert_eq!(report.results.summary.passed, 1);
        assert_eq!(report.results.summary.failed, 1);
        assert_eq!(report.results.summary.skipped, 1);
        assert_eq!(report.results.summary.pending, 0);
        assert_eq!(report.results.summary.other, 0);
        assert_eq!(report.results.tests.len(), 3);
    }

    #[test]
    fn test_canonical_report_serde_roundtrip() {
        let report = CanonicalTestReport::new(
            "jest",
            vec![CtrfTest {
                name: "test_1".to_string(),
                status: CtrfStatus::Passed,
                duration: 5.5,
                suite: Some("suite_a".to_string()),
                message: None,
                trace: None,
                tags: vec!["integration".to_string()],
                flaky: true,
            }],
        );
        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: CanonicalTestReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.results.tool.name, "jest");
        assert_eq!(parsed.results.summary.total, 1);
        assert_eq!(parsed.results.summary.passed, 1);
        assert_eq!(parsed.results.tests[0].name, "test_1");
        assert!(parsed.results.tests[0].flaky);
        assert_eq!(parsed.results.tests[0].tags, vec!["integration"]);
    }

    #[test]
    fn test_canonical_report_with_environment() {
        let mut report = CanonicalTestReport::new("pytest", vec![]);
        report.environment = Some(CtrfEnvironment {
            os: Some("linux".to_string()),
            os_version: Some("6.1".to_string()),
            build_name: None,
            build_number: None,
        });
        let json = serde_json::to_string(&report).unwrap();
        let parsed: CanonicalTestReport = serde_json::from_str(&json).unwrap();
        let env = parsed.environment.unwrap();
        assert_eq!(env.os.as_deref(), Some("linux"));
        assert_eq!(env.os_version.as_deref(), Some("6.1"));
        assert!(env.build_name.is_none());
    }

    #[test]
    fn test_canonical_report_with_extra() {
        let mut report = CanonicalTestReport::new("vitest", vec![]);
        report
            .extra
            .insert("custom_field".to_string(), serde_json::json!("value"));
        let json = serde_json::to_string(&report).unwrap();
        let parsed: CanonicalTestReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.extra["custom_field"], "value");
    }

    #[test]
    fn test_canonical_report_empty_tests() {
        let report = CanonicalTestReport::new("rspec", vec![]);
        assert_eq!(report.results.summary.total, 0);
        assert_eq!(report.results.summary.passed, 0);
        assert_eq!(report.results.summary.failed, 0);
        assert!(report.results.tests.is_empty());
    }

    #[test]
    fn test_ctrf_environment_default() {
        let env = CtrfEnvironment::default();
        assert!(env.os.is_none());
        assert!(env.os_version.is_none());
        assert!(env.build_name.is_none());
        assert!(env.build_number.is_none());
    }

    #[test]
    fn test_ctrf_tool_serde_roundtrip() {
        let tool = CtrfTool {
            name: "cargo-nextest".to_string(),
            version: Some("0.9.72".to_string()),
        };
        let json = serde_json::to_string(&tool).unwrap();
        let parsed: CtrfTool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "cargo-nextest");
        assert_eq!(parsed.version.as_deref(), Some("0.9.72"));
    }

    #[test]
    fn test_ctrf_test_optional_fields_omitted_in_json() {
        let test = CtrfTest {
            name: "t".to_string(),
            status: CtrfStatus::Passed,
            duration: 0.0,
            suite: None,
            message: None,
            trace: None,
            tags: Vec::new(),
            flaky: false,
        };
        let json = serde_json::to_string(&test).unwrap();
        // Optional None fields should not appear in output
        assert!(!json.contains("suite"));
        assert!(!json.contains("message"));
        assert!(!json.contains("trace"));
        // Empty tags should not appear
        assert!(!json.contains("tags"));
    }

    #[test]
    fn test_canonical_report_all_status_types() {
        let tests = vec![
            CtrfTest {
                name: "p".into(),
                status: CtrfStatus::Passed,
                duration: 0.0,
                suite: None,
                message: None,
                trace: None,
                tags: vec![],
                flaky: false,
            },
            CtrfTest {
                name: "f".into(),
                status: CtrfStatus::Failed,
                duration: 0.0,
                suite: None,
                message: None,
                trace: None,
                tags: vec![],
                flaky: false,
            },
            CtrfTest {
                name: "s".into(),
                status: CtrfStatus::Skipped,
                duration: 0.0,
                suite: None,
                message: None,
                trace: None,
                tags: vec![],
                flaky: false,
            },
            CtrfTest {
                name: "pe".into(),
                status: CtrfStatus::Pending,
                duration: 0.0,
                suite: None,
                message: None,
                trace: None,
                tags: vec![],
                flaky: false,
            },
            CtrfTest {
                name: "o".into(),
                status: CtrfStatus::Other,
                duration: 0.0,
                suite: None,
                message: None,
                trace: None,
                tags: vec![],
                flaky: false,
            },
        ];
        let report = CanonicalTestReport::new("test", tests);
        assert_eq!(report.results.summary.total, 5);
        assert_eq!(report.results.summary.passed, 1);
        assert_eq!(report.results.summary.failed, 1);
        assert_eq!(report.results.summary.skipped, 1);
        assert_eq!(report.results.summary.pending, 1);
        assert_eq!(report.results.summary.other, 1);
    }
}
