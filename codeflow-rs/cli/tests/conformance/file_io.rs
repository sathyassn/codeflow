//! File I/O contract conformance tests.
//!
//! Tests that both binaries agree on whether file I/O operations succeed (exit code)
//! when given a valid project directory. Logging handlers write JSONL to state files;
//! this suite verifies they produce the same exit code for the same operation.
//!
//! Note: Actual JSONL content comparison is excluded because:
//! - Timestamps in log entries are non-deterministic.
//! - Binary version strings may differ between Go and Rust.
//! - Log format verification belongs to unit tests in the logging module.
//!
//! What IS verified:
//! - Exit codes match when filesystem is available.
//! - Exit codes match when filesystem path is missing/inaccessible.
//! - Both binaries exit 0 (allow) for logging handlers — never block.

use std::path::PathBuf;

use crate::harness::{resolve_binaries, run_binary};

/// Create a minimal project directory structure for testing logging handlers.
///
/// Returns a `tempfile::TempDir` that auto-cleans on drop, and the path to
/// the `.state/logs` directory.
fn make_project_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let logs_dir = tmp.path().join(".state").join("logs");
    std::fs::create_dir_all(&logs_dir).expect("create .state/logs");
    let project_dir = tmp.path().to_path_buf();
    (tmp, project_dir)
}

/// Build a `session_start` fixture with a specific `project_dir` embedded.
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

/// Build a `session_end` fixture with a specific `project_dir` embedded.
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

/// Build a `post_tool_use` fixture with a specific `project_dir` embedded.
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

/// Build a stop fixture with a specific `project_dir` embedded.
fn stop_fixture_with_dir(project_dir: &str) -> String {
    format!(
        r#"{{
  "event": "stop",
  "session_id": "ses-fileiotest000000000004",
  "project_dir": "{project_dir}"
}}"#
    )
}

// ─── session-start logging: with valid project dir ───────────────────────────

#[test]
fn test_session_start_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let project_dir_str = project_dir.to_string_lossy();
    let fixture = session_start_fixture_with_dir(&project_dir_str);

    let paths = resolve_binaries();
    let go_out = run_binary(
        &paths.go_binary,
        &["hooks", "session-start", "logging"],
        &fixture,
    );
    let rust_out = run_binary(
        &paths.rust_binary,
        &["hooks", "session-start", "logging"],
        &fixture,
    );

    assert_eq!(
        go_out.exit_code, rust_out.exit_code,
        "session-start logging exit code mismatch with valid dir: go={}, rust={}",
        go_out.exit_code, rust_out.exit_code
    );
    // Both should allow (exit 0) — logging never blocks.
    assert_eq!(go_out.exit_code, 0, "session-start logging should exit 0");
    assert_eq!(rust_out.exit_code, 0, "session-start logging should exit 0");
}

// ─── session-end logging: with valid project dir ─────────────────────────────

#[test]
fn test_session_end_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let project_dir_str = project_dir.to_string_lossy();
    let fixture = session_end_fixture_with_dir(&project_dir_str);

    let paths = resolve_binaries();
    let go_out = run_binary(
        &paths.go_binary,
        &["hooks", "session-end", "logging"],
        &fixture,
    );
    let rust_out = run_binary(
        &paths.rust_binary,
        &["hooks", "session-end", "logging"],
        &fixture,
    );

    assert_eq!(
        go_out.exit_code, rust_out.exit_code,
        "session-end logging exit code mismatch with valid dir: go={}, rust={}",
        go_out.exit_code, rust_out.exit_code
    );
    assert_eq!(go_out.exit_code, 0, "session-end logging should exit 0");
}

// ─── post-tool-use logging: with valid project dir ───────────────────────────

#[test]
fn test_post_tool_use_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let project_dir_str = project_dir.to_string_lossy();
    let fixture = post_tool_use_fixture_with_dir(&project_dir_str);

    let paths = resolve_binaries();
    let go_out = run_binary(
        &paths.go_binary,
        &["hooks", "post-tool-use", "logging"],
        &fixture,
    );
    let rust_out = run_binary(
        &paths.rust_binary,
        &["hooks", "post-tool-use", "logging"],
        &fixture,
    );

    assert_eq!(
        go_out.exit_code, rust_out.exit_code,
        "post-tool-use logging exit code mismatch with valid dir: go={}, rust={}",
        go_out.exit_code, rust_out.exit_code
    );
    assert_eq!(go_out.exit_code, 0, "post-tool-use logging should exit 0");
}

// ─── stop logging: with valid project dir ────────────────────────────────────

#[test]
fn test_stop_logging_exit_code_with_valid_dir() {
    let (_tmp, project_dir) = make_project_dir();
    let project_dir_str = project_dir.to_string_lossy();
    let fixture = stop_fixture_with_dir(&project_dir_str);

    let paths = resolve_binaries();
    let go_out = run_binary(&paths.go_binary, &["hooks", "stop", "logging"], &fixture);
    let rust_out = run_binary(&paths.rust_binary, &["hooks", "stop", "logging"], &fixture);

    assert_eq!(
        go_out.exit_code, rust_out.exit_code,
        "stop logging exit code mismatch with valid dir: go={}, rust={}",
        go_out.exit_code, rust_out.exit_code
    );
    assert_eq!(go_out.exit_code, 0, "stop logging should exit 0");
}

// ─── logging handlers: graceful degradation with missing project dir ─────────

#[test]
fn test_session_start_logging_graceful_with_missing_dir() {
    let fixture = session_start_fixture_with_dir("/nonexistent/path/xyz123");

    let paths = resolve_binaries();
    let go_out = run_binary(
        &paths.go_binary,
        &["hooks", "session-start", "logging"],
        &fixture,
    );
    let rust_out = run_binary(
        &paths.rust_binary,
        &["hooks", "session-start", "logging"],
        &fixture,
    );

    // Both must gracefully degrade (exit 0) even when project dir doesn't exist.
    assert_eq!(
        go_out.exit_code, 0,
        "Go session-start logging should exit 0 even with missing dir"
    );
    assert_eq!(
        rust_out.exit_code, 0,
        "Rust session-start logging should exit 0 even with missing dir"
    );
    assert_eq!(
        go_out.exit_code, rust_out.exit_code,
        "exit codes should match: go={}, rust={}",
        go_out.exit_code, rust_out.exit_code
    );
}

#[test]
fn test_stop_logging_graceful_with_missing_dir() {
    let fixture = stop_fixture_with_dir("/nonexistent/path/xyz456");

    let paths = resolve_binaries();
    let go_out = run_binary(&paths.go_binary, &["hooks", "stop", "logging"], &fixture);
    let rust_out = run_binary(&paths.rust_binary, &["hooks", "stop", "logging"], &fixture);

    assert_eq!(
        go_out.exit_code, 0,
        "Go stop logging should exit 0 even with missing dir"
    );
    assert_eq!(
        rust_out.exit_code, 0,
        "Rust stop logging should exit 0 even with missing dir"
    );
}

// ─── logging handlers: stdout is empty (logging writes to files, not stdout) ─

#[test]
fn test_logging_handlers_produce_no_stdout() {
    // All logging handlers write to state files, not to stdout.
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
        let go_out = run_binary(&paths.go_binary, &["hooks", event, handler], fixture);
        let rust_out = run_binary(&paths.rust_binary, &["hooks", event, handler], fixture);

        assert!(
            go_out.stdout.is_empty(),
            "Go hooks {event} {handler}: expected empty stdout, got {:?}",
            go_out.stdout
        );
        assert!(
            rust_out.stdout.is_empty(),
            "Rust hooks {event} {handler}: expected empty stdout, got {:?}",
            rust_out.stdout
        );
    }
}
