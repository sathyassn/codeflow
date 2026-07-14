//! `JUnit` XML parser.
//!
//! Parses `JUnit` XML reports from cargo-nextest, pytest, jest-junit, and vitest
//! into a `JunitReport` intermediate representation.

use std::path::Path;

use crate::testing::error::TestingError;

/// A parsed `JUnit` report containing one or more test suites.
#[derive(Debug, Clone)]
pub struct JunitReport {
    /// The test suites in this report.
    pub suites: Vec<JunitTestSuite>,
    /// Name of the tool that produced this report.
    pub tool_name: String,
}

/// A single test suite from a `JUnit` XML report.
#[derive(Debug, Clone)]
pub struct JunitTestSuite {
    pub name: String,
    pub tests: u64,
    pub failures: u64,
    pub errors: u64,
    pub skipped: u64,
    pub time: f64,
    pub test_cases: Vec<JunitTestCase>,
}

/// A single test case from a `JUnit` XML test suite.
#[derive(Debug, Clone)]
pub struct JunitTestCase {
    pub name: String,
    pub classname: Option<String>,
    pub time: f64,
    pub status: JunitTestStatus,
    pub failure_message: Option<String>,
    pub failure_text: Option<String>,
    pub system_out: Option<String>,
    pub system_err: Option<String>,
}

/// Status of a `JUnit` test case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JunitTestStatus {
    Passed,
    Failed,
    Error,
    Skipped,
}

/// Parse a `JUnit` XML file into a `JunitReport`.
///
/// Handles dialect variations from cargo-nextest, pytest-junitxml,
/// jest-junit, and vitest.
///
/// # Errors
///
/// Returns `TestingError::JunitParseError` on malformed XML with file path
/// and byte offset.
pub fn parse_junit(path: &Path) -> Result<JunitReport, TestingError> {
    let content = std::fs::read_to_string(path).map_err(|e| TestingError::JunitParseError {
        path: path.to_path_buf(),
        offset: 0,
        message: format!("failed to read file: {e}"),
    })?;

    parse_junit_str(&content, path)
}

/// Parse `JUnit` XML from a string (for testing without file I/O).
///
/// Single-pass XML event loop imported from v1; splitting the state machine
/// across functions would obscure the parser states it walks through.
#[allow(clippy::too_many_lines)]
pub(crate) fn parse_junit_str(xml: &str, source_path: &Path) -> Result<JunitReport, TestingError> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);

    let mut suites: Vec<JunitTestSuite> = Vec::new();
    let mut current_suite: Option<JunitTestSuite> = None;
    let mut current_case: Option<JunitTestCase> = None;
    let mut in_failure = false;
    let mut in_error = false;
    let mut in_system_out = false;
    let mut in_system_err = false;
    let mut text_buf = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.local_name().as_ref() {
                b"testsuite" => {
                    let mut name = String::new();
                    let mut tests = 0u64;
                    let mut failures = 0u64;
                    let mut errors = 0u64;
                    let mut skipped = 0u64;
                    let mut time = 0.0f64;

                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                            b"tests" => {
                                tests = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0);
                            }
                            b"failures" => {
                                failures =
                                    String::from_utf8_lossy(&attr.value).parse().unwrap_or(0);
                            }
                            b"errors" => {
                                errors = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0);
                            }
                            b"skipped" => {
                                skipped = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0);
                            }
                            b"time" => {
                                time = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0.0);
                            }
                            _ => {}
                        }
                    }

                    current_suite = Some(JunitTestSuite {
                        name,
                        tests,
                        failures,
                        errors,
                        skipped,
                        time,
                        test_cases: Vec::new(),
                    });
                }
                b"testcase" => {
                    let mut name = String::new();
                    let mut classname = None;
                    let mut time = 0.0f64;

                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                            b"classname" => {
                                classname = Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                            b"time" => {
                                time = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0.0);
                            }
                            _ => {}
                        }
                    }

                    current_case = Some(JunitTestCase {
                        name,
                        classname,
                        time,
                        status: JunitTestStatus::Passed,
                        failure_message: None,
                        failure_text: None,
                        system_out: None,
                        system_err: None,
                    });
                }
                b"failure" => {
                    if let Some(ref mut case) = current_case {
                        case.status = JunitTestStatus::Failed;
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"message" {
                                case.failure_message =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                    in_failure = true;
                    text_buf.clear();
                }
                b"error" => {
                    if let Some(ref mut case) = current_case {
                        case.status = JunitTestStatus::Error;
                        for attr in e.attributes().flatten() {
                            if attr.key.local_name().as_ref() == b"message" {
                                case.failure_message =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                    in_error = true;
                    text_buf.clear();
                }
                b"skipped" => {
                    if let Some(ref mut case) = current_case {
                        case.status = JunitTestStatus::Skipped;
                    }
                }
                b"system-out" => {
                    in_system_out = true;
                    text_buf.clear();
                }
                b"system-err" => {
                    in_system_err = true;
                    text_buf.clear();
                }
                _ => {}
            },
            Ok(Event::Empty(ref e)) => match e.local_name().as_ref() {
                b"testcase" => {
                    let mut name = String::new();
                    let mut classname = None;
                    let mut time = 0.0f64;

                    for attr in e.attributes().flatten() {
                        match attr.key.local_name().as_ref() {
                            b"name" => name = String::from_utf8_lossy(&attr.value).to_string(),
                            b"classname" => {
                                classname = Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                            b"time" => {
                                time = String::from_utf8_lossy(&attr.value).parse().unwrap_or(0.0);
                            }
                            _ => {}
                        }
                    }

                    let case = JunitTestCase {
                        name,
                        classname,
                        time,
                        status: JunitTestStatus::Passed,
                        failure_message: None,
                        failure_text: None,
                        system_out: None,
                        system_err: None,
                    };

                    if let Some(ref mut suite) = current_suite {
                        suite.test_cases.push(case);
                    }
                }
                b"skipped" => {
                    if let Some(ref mut case) = current_case {
                        case.status = JunitTestStatus::Skipped;
                    }
                }
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if in_failure || in_error || in_system_out || in_system_err {
                    let decoded = e.decode().unwrap_or_default();
                    text_buf.push_str(&quick_xml::escape::unescape(&decoded).unwrap_or_default());
                }
            }
            Ok(Event::CData(ref e)) => {
                if in_failure || in_error || in_system_out || in_system_err {
                    text_buf.push_str(&String::from_utf8_lossy(e.as_ref()));
                }
            }
            Ok(Event::End(ref e)) => match e.local_name().as_ref() {
                b"testsuite" => {
                    if let Some(suite) = current_suite.take() {
                        suites.push(suite);
                    }
                }
                b"testcase" => {
                    if let Some(case) = current_case.take() {
                        if let Some(ref mut suite) = current_suite {
                            suite.test_cases.push(case);
                        }
                    }
                }
                b"failure" => {
                    if in_failure {
                        if let Some(ref mut case) = current_case {
                            if !text_buf.is_empty() {
                                case.failure_text = Some(text_buf.clone());
                            }
                        }
                        in_failure = false;
                    }
                }
                b"error" => {
                    if in_error {
                        if let Some(ref mut case) = current_case {
                            if !text_buf.is_empty() {
                                case.failure_text = Some(text_buf.clone());
                            }
                        }
                        in_error = false;
                    }
                }
                b"system-out" => {
                    if in_system_out {
                        if let Some(ref mut case) = current_case {
                            if !text_buf.is_empty() {
                                case.system_out = Some(text_buf.clone());
                            }
                        }
                        in_system_out = false;
                    }
                }
                b"system-err" if in_system_err => {
                    if let Some(ref mut case) = current_case {
                        if !text_buf.is_empty() {
                            case.system_err = Some(text_buf.clone());
                        }
                    }
                    in_system_err = false;
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(TestingError::JunitParseError {
                    path: source_path.to_path_buf(),
                    #[allow(clippy::cast_possible_truncation)]
                    offset: reader.error_position() as usize,
                    message: format!("{e}"),
                });
            }
            _ => {}
        }
    }

    // Detect the tool from suite names or heuristics
    let tool_name = detect_tool_name(&suites);

    Ok(JunitReport { suites, tool_name })
}

/// Detect the `JUnit` dialect from testsuite metadata.
fn detect_tool_name(suites: &[JunitTestSuite]) -> String {
    if suites.is_empty() {
        return "unknown".to_string();
    }

    // cargo-nextest uses the crate name as the suite name and has "::" in test names
    let has_rust_paths = suites
        .iter()
        .any(|s| s.test_cases.iter().any(|tc| tc.name.contains("::")));
    if has_rust_paths {
        return "cargo-nextest".to_string();
    }

    // pytest uses dotted classnames (module.Class.method)
    let has_pytest_classnames = suites.iter().any(|s| {
        s.test_cases.iter().any(|tc| {
            tc.classname
                .as_ref()
                .is_some_and(|c| c.contains('.') && !c.contains('/'))
        })
    });
    if has_pytest_classnames {
        return "pytest".to_string();
    }

    "junit".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn test_parse_simple_junit() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="math" tests="2" failures="0" errors="0" skipped="0" time="0.5">
    <testcase name="test_add" classname="math" time="0.2"/>
    <testcase name="test_sub" classname="math" time="0.3"/>
  </testsuite>
</testsuites>"#;
        let report = parse_junit_str(xml, &path("test.xml")).unwrap();
        assert_eq!(report.suites.len(), 1);
        assert_eq!(report.suites[0].name, "math");
        assert_eq!(report.suites[0].test_cases.len(), 2);
        assert_eq!(report.suites[0].test_cases[0].name, "test_add");
        assert_eq!(
            report.suites[0].test_cases[0].status,
            JunitTestStatus::Passed
        );
    }

    #[test]
    fn test_parse_junit_with_failure() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="tests" tests="1" failures="1">
  <testcase name="test_broken" time="0.1">
    <failure message="assertion failed">expected 4, got 5</failure>
  </testcase>
</testsuite>"#;
        let report = parse_junit_str(xml, &path("test.xml")).unwrap();
        assert_eq!(
            report.suites[0].test_cases[0].status,
            JunitTestStatus::Failed
        );
        assert_eq!(
            report.suites[0].test_cases[0].failure_message.as_deref(),
            Some("assertion failed")
        );
        assert_eq!(
            report.suites[0].test_cases[0].failure_text.as_deref(),
            Some("expected 4, got 5")
        );
    }

    #[test]
    fn test_parse_junit_with_error() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="tests" tests="1" errors="1">
  <testcase name="test_crash" time="0.01">
    <error message="NullPointer">stack trace here</error>
  </testcase>
</testsuite>"#;
        let report = parse_junit_str(xml, &path("test.xml")).unwrap();
        assert_eq!(
            report.suites[0].test_cases[0].status,
            JunitTestStatus::Error
        );
        assert_eq!(
            report.suites[0].test_cases[0].failure_message.as_deref(),
            Some("NullPointer")
        );
    }

    #[test]
    fn test_parse_junit_with_skipped() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="tests" tests="1" skipped="1">
  <testcase name="test_skip" time="0.0">
    <skipped/>
  </testcase>
</testsuite>"#;
        let report = parse_junit_str(xml, &path("test.xml")).unwrap();
        assert_eq!(
            report.suites[0].test_cases[0].status,
            JunitTestStatus::Skipped
        );
    }

    #[test]
    fn test_parse_malformed_xml() {
        // Use XML with mismatched tags that quick-xml actually rejects
        let xml = "<testsuite><testcase></testsuite></testcase>";
        let result = parse_junit_str(xml, &path("bad.xml"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            TestingError::JunitParseError { path: p, .. } => {
                assert_eq!(p, PathBuf::from("bad.xml"));
            }
            other => panic!("expected JunitParseError, got: {other}"),
        }
    }

    #[test]
    fn test_parse_nextest_dialect() {
        let xml = r#"<?xml version="1.0"?>
<testsuites>
  <testsuite name="codeflow-core" tests="1" failures="0">
    <testcase name="testing::report::tests::test_add" classname="codeflow-core" time="0.01"/>
  </testsuite>
</testsuites>"#;
        let report = parse_junit_str(xml, &path("nextest.xml")).unwrap();
        assert_eq!(report.tool_name, "cargo-nextest");
    }

    #[test]
    fn test_parse_pytest_dialect() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="pytest" tests="1">
  <testcase name="test_hello" classname="tests.test_main.TestHello" time="0.05"/>
</testsuite>"#;
        let report = parse_junit_str(xml, &path("pytest.xml")).unwrap();
        assert_eq!(report.tool_name, "pytest");
    }

    #[test]
    fn test_parse_empty_testsuite() {
        let xml = r#"<testsuite name="empty" tests="0" failures="0"></testsuite>"#;
        let report = parse_junit_str(xml, &path("empty.xml")).unwrap();
        assert_eq!(report.suites.len(), 1);
        assert!(report.suites[0].test_cases.is_empty());
    }

    #[test]
    fn test_parse_multiple_suites() {
        let xml = r#"<?xml version="1.0"?>
<testsuites>
  <testsuite name="a" tests="1"><testcase name="t1" time="0.1"/></testsuite>
  <testsuite name="b" tests="1"><testcase name="t2" time="0.2"/></testsuite>
</testsuites>"#;
        let report = parse_junit_str(xml, &path("multi.xml")).unwrap();
        assert_eq!(report.suites.len(), 2);
        assert_eq!(report.suites[0].name, "a");
        assert_eq!(report.suites[1].name, "b");
    }

    #[test]
    fn test_parse_file_not_found() {
        let result = parse_junit(&PathBuf::from("/nonexistent/report.xml"));
        assert!(result.is_err());
        match result.unwrap_err() {
            TestingError::JunitParseError {
                path: p, message, ..
            } => {
                assert_eq!(p, PathBuf::from("/nonexistent/report.xml"));
                assert!(message.contains("failed to read file"));
            }
            other => panic!("expected JunitParseError, got: {other}"),
        }
    }

    #[test]
    fn test_parse_system_out_err() {
        let xml = r#"<?xml version="1.0"?>
<testsuite name="tests" tests="1">
  <testcase name="test_verbose" time="0.1">
    <system-out>hello stdout</system-out>
    <system-err>hello stderr</system-err>
  </testcase>
</testsuite>"#;
        let report = parse_junit_str(xml, &path("verbose.xml")).unwrap();
        assert_eq!(
            report.suites[0].test_cases[0].system_out.as_deref(),
            Some("hello stdout")
        );
        assert_eq!(
            report.suites[0].test_cases[0].system_err.as_deref(),
            Some("hello stderr")
        );
    }
}
