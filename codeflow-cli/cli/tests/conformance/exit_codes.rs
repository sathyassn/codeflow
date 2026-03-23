//! Exit code contract tests (Rust-only, post-cutover).
//!
//! Verifies that the Rust binary conforms to exit code contracts:
//! - Exit 0: Allow (normal processing, graceful degradation)
//! - Exit 2: Block (hook blocks an operation)

use crate::harness::{load_fixture, resolve_binaries, run_binary};

const ALL_HANDLERS: &[(&str, &str)] = &[
    ("session-start", "init"),
    ("session-start", "instructions"),
    ("session-start", "logging"),
    ("session-end", "cleanup"),
    ("session-end", "logging"),
    ("pre-tool-use", "gate-check"),
    ("pre-tool-use", "team-guard"),
    ("pre-tool-use", "edit-write-guard"),
    ("pre-tool-use", "gh-pr-guard"),
    ("pre-tool-use", "protection-guard"),
    ("pre-tool-use", "security"),
    ("pre-tool-use", "webfetch-guard"),
    ("post-tool-use", "sentinel-write"),
    ("post-tool-use", "settings-validate"),
    ("post-tool-use", "checkpoint-register"),
    ("post-tool-use", "logging"),
    ("task-completed", "checkpoint-complete"),
    ("stop", "logging"),
    ("user-prompt-submit", "validate"),
    ("user-prompt-submit", "logging"),
];

// ─── Exit 0: empty stdin (graceful degradation) ──────────────────────────────

#[test]
fn test_exit_zero_all_handlers_empty_stdin() {
    let paths = resolve_binaries();
    for (event, handler) in ALL_HANDLERS {
        let out = run_binary(&paths.rust_binary, &["hooks", event, handler], "");
        assert_eq!(
            out.exit_code, 0,
            "hooks {event} {handler}: expected exit 0 on empty stdin, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_all_handlers_invalid_json() {
    let paths = resolve_binaries();
    for (event, handler) in ALL_HANDLERS {
        let out = run_binary(
            &paths.rust_binary,
            &["hooks", event, handler],
            "this is not json",
        );
        assert_eq!(
            out.exit_code, 0,
            "hooks {event} {handler}: expected exit 0 on invalid JSON, got {}",
            out.exit_code
        );
    }
}

// ─── Exit 0: valid fixture input (allow path) ────────────────────────────────

#[test]
fn test_exit_zero_session_start_handlers_with_fixture() {
    let fixture = load_fixture("session_start");
    let paths = resolve_binaries();

    for handler in &["init", "instructions", "logging"] {
        let out = run_binary(
            &paths.rust_binary,
            &["hooks", "session-start", handler],
            &fixture,
        );
        assert_eq!(
            out.exit_code, 0,
            "session-start {handler}: expected exit 0 with valid input, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_session_end_handlers_with_fixture() {
    let fixture = load_fixture("session_end");
    let paths = resolve_binaries();

    for handler in &["cleanup", "logging"] {
        let out = run_binary(
            &paths.rust_binary,
            &["hooks", "session-end", handler],
            &fixture,
        );
        assert_eq!(
            out.exit_code, 0,
            "session-end {handler}: expected exit 0 with valid input, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_pre_tool_use_with_benign_bash_input() {
    let fixture = r#"{"event":"pre_tool_use","tool_name":"Bash","tool_input":{"command":"ls -la"},"session_id":"ses-conformance000000000001"}"#;
    let paths = resolve_binaries();
    let handlers = &[
        "gate-check",
        "team-guard",
        "edit-write-guard",
        "gh-pr-guard",
        "protection-guard",
        "security",
        "webfetch-guard",
    ];

    for handler in handlers {
        let out = run_binary(
            &paths.rust_binary,
            &["hooks", "pre-tool-use", handler],
            fixture,
        );
        assert_eq!(
            out.exit_code, 0,
            "pre-tool-use {handler}: expected exit 0 for benign bash input, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_post_tool_use_with_fixture() {
    let fixture = load_fixture("post_tool_use");
    let paths = resolve_binaries();

    for handler in &[
        "sentinel-write",
        "settings-validate",
        "checkpoint-register",
        "logging",
    ] {
        let out = run_binary(
            &paths.rust_binary,
            &["hooks", "post-tool-use", handler],
            &fixture,
        );
        assert_eq!(
            out.exit_code, 0,
            "post-tool-use {handler}: expected exit 0, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_task_completed_with_fixture() {
    let fixture = load_fixture("task_completed");
    let paths = resolve_binaries();

    let out = run_binary(
        &paths.rust_binary,
        &["hooks", "task-completed", "checkpoint-complete"],
        &fixture,
    );
    assert_eq!(
        out.exit_code, 0,
        "task-completed checkpoint-complete: expected exit 0, got {}",
        out.exit_code
    );
}

#[test]
fn test_exit_zero_stop_logging_with_fixture() {
    let fixture = load_fixture("stop");
    let paths = resolve_binaries();

    let out = run_binary(&paths.rust_binary, &["hooks", "stop", "logging"], &fixture);
    assert_eq!(
        out.exit_code, 0,
        "stop logging: expected exit 0, got {}",
        out.exit_code
    );
}

// ─── Version flag exit code ──────────────────────────────────────────────────

#[test]
fn test_exit_zero_version_flag() {
    let paths = resolve_binaries();
    let out = run_binary(&paths.rust_binary, &["--version"], "");
    assert_eq!(
        out.exit_code, 0,
        "--version should exit 0, got {}",
        out.exit_code
    );
}
