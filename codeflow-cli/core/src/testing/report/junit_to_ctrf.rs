//! JUnit → CTRF conversion.
//!
//! Converts a `JunitReport` into a `CanonicalTestReport` (CTRF-shaped),
//! preserving all fields CodeFlow consumes: counts, durations, failure
//! messages, and suite names.

use super::{
    CanonicalTestReport, CtrfResults, CtrfStatus, CtrfSummary, CtrfTest, CtrfTool,
    junit::{JunitReport, JunitTestStatus},
};

impl From<JunitReport> for CanonicalTestReport {
    fn from(junit: JunitReport) -> Self {
        let mut tests = Vec::new();
        let mut total_time = 0.0f64;

        for suite in &junit.suites {
            for tc in &suite.test_cases {
                let status = match tc.status {
                    JunitTestStatus::Passed => CtrfStatus::Passed,
                    JunitTestStatus::Failed | JunitTestStatus::Error => CtrfStatus::Failed,
                    JunitTestStatus::Skipped => CtrfStatus::Skipped,
                };

                // Duration: JUnit time is in seconds, CTRF uses ms
                let duration_ms = tc.time * 1000.0;
                total_time += tc.time;

                let message = tc
                    .failure_message
                    .clone()
                    .or_else(|| tc.failure_text.clone());
                let trace = if tc.failure_message.is_some() {
                    tc.failure_text.clone()
                } else {
                    None
                };

                tests.push(CtrfTest {
                    name: tc.name.clone(),
                    status,
                    duration: duration_ms,
                    suite: Some(suite.name.clone()),
                    message,
                    trace,
                    tags: Vec::new(),
                    flaky: false,
                });
            }
        }

        let mut passed = 0u64;
        let mut failed = 0u64;
        let mut skipped = 0u64;

        for t in &tests {
            match t.status {
                CtrfStatus::Passed => passed += 1,
                CtrfStatus::Failed => failed += 1,
                CtrfStatus::Skipped => skipped += 1,
                CtrfStatus::Pending | CtrfStatus::Other => {}
            }
        }

        let total = passed + failed + skipped;

        Self {
            results: CtrfResults {
                tool: CtrfTool {
                    name: junit.tool_name,
                    version: None,
                },
                summary: CtrfSummary {
                    total,
                    passed,
                    failed,
                    skipped,
                    pending: 0,
                    other: 0,
                    start: 0,
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    stop: (total_time * 1000.0).max(0.0) as u64,
                },
                tests,
            },
            environment: None,
            extra: std::collections::BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::report::junit::{JunitTestCase, JunitTestSuite};

    fn make_case(name: &str, status: JunitTestStatus, time: f64) -> JunitTestCase {
        JunitTestCase {
            name: name.to_string(),
            classname: Some("test_class".to_string()),
            time,
            status,
            failure_message: None,
            failure_text: None,
            system_out: None,
            system_err: None,
        }
    }

    fn make_suite(name: &str, cases: Vec<JunitTestCase>) -> JunitTestSuite {
        let tests = cases.len() as u64;
        let failures = cases
            .iter()
            .filter(|c| c.status == JunitTestStatus::Failed)
            .count() as u64;
        let errors = cases
            .iter()
            .filter(|c| c.status == JunitTestStatus::Error)
            .count() as u64;
        let skipped = cases
            .iter()
            .filter(|c| c.status == JunitTestStatus::Skipped)
            .count() as u64;
        let time: f64 = cases.iter().map(|c| c.time).sum();
        JunitTestSuite {
            name: name.to_string(),
            tests,
            failures,
            errors,
            skipped,
            time,
            test_cases: cases,
        }
    }

    #[test]
    fn test_convert_simple_passing() {
        let junit = JunitReport {
            suites: vec![make_suite(
                "s1",
                vec![
                    make_case("t1", JunitTestStatus::Passed, 0.1),
                    make_case("t2", JunitTestStatus::Passed, 0.2),
                ],
            )],
            tool_name: "cargo-nextest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(ctrf.results.tool.name, "cargo-nextest");
        assert_eq!(ctrf.results.summary.total, 2);
        assert_eq!(ctrf.results.summary.passed, 2);
        assert_eq!(ctrf.results.summary.failed, 0);
        assert_eq!(ctrf.results.tests.len(), 2);
        assert_eq!(ctrf.results.tests[0].suite.as_deref(), Some("s1"));
    }

    #[test]
    fn test_convert_with_failures() {
        let mut case = make_case("broken", JunitTestStatus::Failed, 0.5);
        case.failure_message = Some("assertion failed".to_string());
        case.failure_text = Some("at line 42".to_string());

        let junit = JunitReport {
            suites: vec![make_suite(
                "tests",
                vec![make_case("passing", JunitTestStatus::Passed, 0.1), case],
            )],
            tool_name: "pytest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(ctrf.results.summary.passed, 1);
        assert_eq!(ctrf.results.summary.failed, 1);

        let failed_test = ctrf
            .results
            .tests
            .iter()
            .find(|t| t.status == CtrfStatus::Failed)
            .unwrap();
        assert_eq!(failed_test.message.as_deref(), Some("assertion failed"));
        assert_eq!(failed_test.trace.as_deref(), Some("at line 42"));
    }

    #[test]
    fn test_convert_with_skipped() {
        let junit = JunitReport {
            suites: vec![make_suite(
                "tests",
                vec![make_case("skip_me", JunitTestStatus::Skipped, 0.0)],
            )],
            tool_name: "jest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(ctrf.results.summary.skipped, 1);
        assert_eq!(ctrf.results.summary.total, 1);
        assert_eq!(ctrf.results.tests[0].status, CtrfStatus::Skipped);
    }

    #[test]
    fn test_convert_error_maps_to_failed() {
        let junit = JunitReport {
            suites: vec![make_suite(
                "tests",
                vec![make_case("crash", JunitTestStatus::Error, 0.01)],
            )],
            tool_name: "junit".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(ctrf.results.summary.failed, 1);
        assert_eq!(ctrf.results.tests[0].status, CtrfStatus::Failed);
    }

    #[test]
    fn test_convert_duration_seconds_to_ms() {
        let junit = JunitReport {
            suites: vec![make_suite(
                "tests",
                vec![make_case("slow", JunitTestStatus::Passed, 2.5)],
            )],
            tool_name: "cargo-nextest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        // 2.5 seconds = 2500 ms
        assert!((ctrf.results.tests[0].duration - 2500.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_convert_multiple_suites() {
        let junit = JunitReport {
            suites: vec![
                make_suite(
                    "suite_a",
                    vec![make_case("a1", JunitTestStatus::Passed, 0.1)],
                ),
                make_suite(
                    "suite_b",
                    vec![
                        make_case("b1", JunitTestStatus::Passed, 0.2),
                        make_case("b2", JunitTestStatus::Failed, 0.3),
                    ],
                ),
            ],
            tool_name: "vitest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(ctrf.results.summary.total, 3);
        assert_eq!(ctrf.results.summary.passed, 2);
        assert_eq!(ctrf.results.summary.failed, 1);
        assert_eq!(ctrf.results.tests[0].suite.as_deref(), Some("suite_a"));
        assert_eq!(ctrf.results.tests[1].suite.as_deref(), Some("suite_b"));
    }

    #[test]
    fn test_convert_empty_report() {
        let junit = JunitReport {
            suites: vec![],
            tool_name: "unknown".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(ctrf.results.summary.total, 0);
        assert!(ctrf.results.tests.is_empty());
    }

    #[test]
    fn test_convert_preserves_suite_names() {
        let junit = JunitReport {
            suites: vec![make_suite(
                "my::module::tests",
                vec![make_case("test_foo", JunitTestStatus::Passed, 0.1)],
            )],
            tool_name: "cargo-nextest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        assert_eq!(
            ctrf.results.tests[0].suite.as_deref(),
            Some("my::module::tests")
        );
    }

    #[test]
    fn test_convert_failure_text_only_no_message() {
        let mut case = make_case("t", JunitTestStatus::Failed, 0.1);
        case.failure_message = None;
        case.failure_text = Some("raw trace".to_string());

        let junit = JunitReport {
            suites: vec![make_suite("s", vec![case])],
            tool_name: "junit".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        // When message is None but text exists, message gets text, trace stays None
        assert_eq!(ctrf.results.tests[0].message.as_deref(), Some("raw trace"));
        assert!(ctrf.results.tests[0].trace.is_none());
    }

    #[test]
    fn test_serde_roundtrip_of_converted_report() {
        let junit = JunitReport {
            suites: vec![make_suite(
                "tests",
                vec![
                    make_case("t1", JunitTestStatus::Passed, 1.0),
                    make_case("t2", JunitTestStatus::Failed, 0.5),
                ],
            )],
            tool_name: "cargo-nextest".to_string(),
        };
        let ctrf: CanonicalTestReport = junit.into();
        let json = serde_json::to_string_pretty(&ctrf).unwrap();
        let reparsed: CanonicalTestReport = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed.results.summary.total, ctrf.results.summary.total);
        assert_eq!(reparsed.results.summary.passed, ctrf.results.summary.passed);
        assert_eq!(reparsed.results.summary.failed, ctrf.results.summary.failed);
        assert_eq!(reparsed.results.tests.len(), ctrf.results.tests.len());
    }
}
