//! File I/O contract tests (Rust-only, post-cutover).
//!
//! Tests that the Rust binary correctly handles file I/O for logging handlers:
//! - Exit 0 when filesystem is available
//! - Exit 0 when filesystem path is missing (graceful degradation)
//! - Logging handlers write to files, not stdout

use std::path::PathBuf;

use crate::harness::{resolve_binaries, run_binary};

fn make_project_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let logs_dir = tmp.path().join(".state").join("logs");
    std::fs::create_dir_all(&logs_dir).expect("create .state/logs");
    let project_dir = tmp.path().to_path_buf();
    (tmp, project_dir)
}

fn session_start_fixture_with_dir(project_dir: &str) -> String {
    format!(
        r#"{{
  "event": "session_start",
  "session_id": "ses-fileiotest000000000001",
  "project_dir": "{project_dir}",
  "source": "startup"
}}"#
    )
}

fn session_end_fixture_with_dir(project_dir: &str) -> String {
    format!(
        r#"{{
  "event": "session_end",
  "session_id": "ses-fileiotest000000000002",
  "project_dir": "{project_dir}",
  "source": "compact"
}}"#
    )
}

fn post_tool_use_fixture_with_dir(project_dir: &str) -> String {
    format!(
        r#"{{
  "event": "post_tool_use",
  "tool_name": "Bash",
  "tool_input": {{"command": "echo test"}},
  "session_id": "ses-fileiotest000000000003",
  "project_dir": "{project_dir}"
}}"#
    )
}

fn stop_fixture_with_dir(project_dir: &str) -> String {
    format!(
        r#"{{
  "event": "stop",
  "session_id": "ses-fileiotest000000000004",
  "project_dir": "{project_dir}"
}}"#
    )
}

// ─── logging handlers: exit 0 with valid project dir ─────────────────────────

#[test]
fn test_session_start_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let fixture = session_start_fixture_with_dir(&project_dir.to_string_lossy());
    let paths = resolve_binaries();
    let out = run_binary(
        &paths.rust_binary,
        &["hooks", "session-start", "logging"],
        &fixture,
    );
    assert_eq!(out.exit_code, 0, "session-start logging should exit 0");
}

#[test]
fn test_session_end_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let fixture = session_end_fixture_with_dir(&project_dir.to_string_lossy());
    let paths = resolve_binaries();
    let out = run_binary(
        &paths.rust_binary,
        &["hooks", "session-end", "logging"],
        &fixture,
    );
    assert_eq!(out.exit_code, 0, "session-end logging should exit 0");
}

#[test]
fn test_post_tool_use_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let fixture = post_tool_use_fixture_with_dir(&project_dir.to_string_lossy());
    let paths = resolve_binaries();
    let out = run_binary(
        &paths.rust_binary,
        &["hooks", "post-tool-use", "logging"],
        &fixture,
    );
    assert_eq!(out.exit_code, 0, "post-tool-use logging should exit 0");
}

#[test]
fn test_stop_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let fixture = stop_fixture_with_dir(&project_dir.to_string_lossy());
    let paths = resolve_binaries();
    let out = run_binary(&paths.rust_binary, &["hooks", "stop", "logging"], &fixture);
    assert_eq!(out.exit_code, 0, "stop logging should exit 0");
}

// ─── logging handlers: graceful degradation with missing project dir ─────────

#[test]
fn test_session_start_logging_graceful_with_missing_dir() {
    let fixture = session_start_fixture_with_dir("/nonexistent/path/xyz123");
    let paths = resolve_binaries();
    let out = run_binary(
        &paths.rust_binary,
        &["hooks", "session-start", "logging"],
        &fixture,
    );
    assert_eq!(
        out.exit_code, 0,
        "session-start logging should exit 0 even with missing dir"
    );
}

#[test]
fn test_stop_logging_graceful_with_missing_dir() {
    let fixture = stop_fixture_with_dir("/nonexistent/path/xyz456");
    let paths = resolve_binaries();
    let out = run_binary(&paths.rust_binary, &["hooks", "stop", "logging"], &fixture);
    assert_eq!(
        out.exit_code, 0,
        "stop logging should exit 0 even with missing dir"
    );
}

// ─── logging handlers: stdout is empty ───────────────────────────────────────

#[test]
fn test_logging_handlers_produce_no_stdout() {
    let (_tmp, project_dir) = make_project_dir();
    let project_dir_str = project_dir.to_string_lossy();
    let paths = resolve_binaries();

    let logging_cases: &[(&str, &str, String)] = &[
        (
            "session-start",
            "logging",
            session_start_fixture_with_dir(&project_dir_str),
        ),
        (
            "session-end",
            "logging",
            session_end_fixture_with_dir(&project_dir_str),
        ),
        (
            "post-tool-use",
            "logging",
            post_tool_use_fixture_with_dir(&project_dir_str),
        ),
        ("stop", "logging", stop_fixture_with_dir(&project_dir_str)),
    ];

    for (event, handler, fixture) in logging_cases {
        let out = run_binary(&paths.rust_binary, &["hooks", event, handler], fixture);
        assert!(
            out.stdout.is_empty(),
            "hooks {event} {handler}: expected empty stdout, got {:?}",
            out.stdout
        );
    }
}
