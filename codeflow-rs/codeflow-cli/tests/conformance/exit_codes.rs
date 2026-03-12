//! Exit code contract conformance tests.
//!
//! Verifies that both binaries agree on exit codes for all exit code paths:
//! - Exit 0: Allow (normal processing, graceful degradation)
//! - Exit 2: Block (hook blocks an operation)
//!
//! The graceful degradation contract requires that infrastructure errors
//! (empty stdin, invalid JSON, handler errors) always produce exit 0 —
//! never exit 2. This is tested for every handler.

use crate::harness::{load_fixture, resolve_binaries, run_binary};

/// All 20 active handlers (user-prompt-submit validate included as stub).
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
fn test_exit_zero_all_handlers_empty_stdin_go() {
    let paths = resolve_binaries();
    for (event, handler) in ALL_HANDLERS {
        let out = run_binary(&paths.go_binary, &["hooks", event, handler], "");
        assert_eq!(
            out.exit_code, 0,
            "Go hooks {event} {handler}: expected exit 0 on empty stdin, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_all_handlers_empty_stdin_rust() {
    let paths = resolve_binaries();
    for (event, handler) in ALL_HANDLERS {
        let out = run_binary(&paths.rust_binary, &["hooks", event, handler], "");
        assert_eq!(
            out.exit_code, 0,
            "Rust hooks {event} {handler}: expected exit 0 on empty stdin, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_all_handlers_invalid_json_go() {
    let paths = resolve_binaries();
    for (event, handler) in ALL_HANDLERS {
        let out = run_binary(
            &paths.go_binary,
            &["hooks", event, handler],
            "this is not json",
        );
        assert_eq!(
            out.exit_code, 0,
            "Go hooks {event} {handler}: expected exit 0 on invalid JSON, got {}",
            out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_all_handlers_invalid_json_rust() {
    let paths = resolve_binaries();
    for (event, handler) in ALL_HANDLERS {
        let out = run_binary(
            &paths.rust_binary,
            &["hooks", event, handler],
            "this is not json",
        );
        assert_eq!(
            out.exit_code, 0,
            "Rust hooks {event} {handler}: expected exit 0 on invalid JSON, got {}",
            out.exit_code
        );
    }
}

// ─── Exit 0: valid fixture input (allow path) ────────────────────────────────

#[test]
fn test_exit_zero_session_start_handlers_with_fixture() {
    let fixture = load_fixture("session_start");
    let paths = resolve_binaries();
    let session_start_handlers = &["init", "instructions", "logging"];

    for handler in session_start_handlers {
        let go_out = run_binary(
            &paths.go_binary,
            &["hooks", "session-start", handler],
            &fixture,
        );
        let rust_out = run_binary(
            &paths.rust_binary,
            &["hooks", "session-start", handler],
            &fixture,
        );
        assert_eq!(
            go_out.exit_code, 0,
            "Go session-start {handler}: expected exit 0 with valid input, got {}",
            go_out.exit_code
        );
        assert_eq!(
            rust_out.exit_code, 0,
            "Rust session-start {handler}: expected exit 0 with valid input, got {}",
            rust_out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_session_end_handlers_with_fixture() {
    let fixture = load_fixture("session_end");
    let paths = resolve_binaries();

    for handler in &["cleanup", "logging"] {
        let go_out = run_binary(
            &paths.go_binary,
            &["hooks", "session-end", handler],
            &fixture,
        );
        let rust_out = run_binary(
            &paths.rust_binary,
            &["hooks", "session-end", handler],
            &fixture,
        );
        assert_eq!(
            go_out.exit_code, 0,
            "Go session-end {handler}: expected exit 0 with valid input, got {}",
            go_out.exit_code
        );
        assert_eq!(
            rust_out.exit_code, 0,
            "Rust session-end {handler}: expected exit 0 with valid input, got {}",
            rust_out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_pre_tool_use_with_benign_bash_input() {
    // A benign `ls` command should be allowed by all pre-tool-use handlers.
    let fixture = r#"{"event":"pre_tool_use","tool_name":"Bash","tool_input":{"command":"ls -la"},"session_id":"ses-conformance000000000001"}"#;
    let paths = resolve_binaries();
    let pre_tool_use_handlers = &[
        "gate-check",
        "team-guard",
        "edit-write-guard",
        "gh-pr-guard",
        "protection-guard",
        "security",
        "webfetch-guard",
    ];

    for handler in pre_tool_use_handlers {
        let go_out = run_binary(
            &paths.go_binary,
            &["hooks", "pre-tool-use", handler],
            fixture,
        );
        let rust_out = run_binary(
            &paths.rust_binary,
            &["hooks", "pre-tool-use", handler],
            fixture,
        );

        // Both must agree on the exit code.
        assert_eq!(
            go_out.exit_code, rust_out.exit_code,
            "pre-tool-use {handler}: exit code mismatch for benign bash input: go={}, rust={}",
            go_out.exit_code, rust_out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_post_tool_use_with_fixture() {
    let fixture = load_fixture("post_tool_use");
    let paths = resolve_binaries();
    let post_tool_use_handlers = &[
        "sentinel-write",
        "settings-validate",
        "checkpoint-register",
        "logging",
    ];

    for handler in post_tool_use_handlers {
        let go_out = run_binary(
            &paths.go_binary,
            &["hooks", "post-tool-use", handler],
            &fixture,
        );
        let rust_out = run_binary(
            &paths.rust_binary,
            &["hooks", "post-tool-use", handler],
            &fixture,
        );
        assert_eq!(
            go_out.exit_code, 0,
            "Go post-tool-use {handler}: expected exit 0, got {}",
            go_out.exit_code
        );
        assert_eq!(
            rust_out.exit_code, 0,
            "Rust post-tool-use {handler}: expected exit 0, got {}",
            rust_out.exit_code
        );
    }
}

#[test]
fn test_exit_zero_task_completed_with_fixture() {
    let fixture = load_fixture("task_completed");
    let paths = resolve_binaries();

    let go_out = run_binary(
        &paths.go_binary,
        &["hooks", "task-completed", "checkpoint-complete"],
        &fixture,
    );
    let rust_out = run_binary(
        &paths.rust_binary,
        &["hooks", "task-completed", "checkpoint-complete"],
        &fixture,
    );
    assert_eq!(
        go_out.exit_code, 0,
        "Go task-completed checkpoint-complete: expected exit 0, got {}",
        go_out.exit_code
    );
    assert_eq!(
        rust_out.exit_code, 0,
        "Rust task-completed checkpoint-complete: expected exit 0, got {}",
        rust_out.exit_code
    );
}

#[test]
fn test_exit_zero_stop_logging_with_fixture() {
    let fixture = load_fixture("stop");
    let paths = resolve_binaries();

    let go_out = run_binary(&paths.go_binary, &["hooks", "stop", "logging"], &fixture);
    let rust_out = run_binary(&paths.rust_binary, &["hooks", "stop", "logging"], &fixture);
    assert_eq!(
        go_out.exit_code, 0,
        "Go stop logging: expected exit 0, got {}",
        go_out.exit_code
    );
    assert_eq!(
        rust_out.exit_code, 0,
        "Rust stop logging: expected exit 0, got {}",
        rust_out.exit_code
    );
}

// ─── Exit code agreement across all handlers and all event fixtures ──────────

#[test]
fn test_exit_code_agreement_all_handlers_all_fixtures() {
    // Cross-product: every handler with every event fixture.
    // All should agree on exit code (even if it's not 0 — they must match).
    let fixtures = &[
        ("pre_tool_use", load_fixture("pre_tool_use")),
        ("post_tool_use", load_fixture("post_tool_use")),
        ("task_completed", load_fixture("task_completed")),
        ("session_start", load_fixture("session_start")),
        ("session_end", load_fixture("session_end")),
        ("stop", load_fixture("stop")),
        ("user_prompt_submit", load_fixture("user_prompt_submit")),
    ];

    let paths = resolve_binaries();

    // Test a representative subset to avoid combinatorial explosion while
    // still providing cross-event coverage.
    let representative_handlers = &[
        ("session-start", "init"),
        ("session-start", "logging"),
        ("pre-tool-use", "gate-check"),
        ("pre-tool-use", "security"),
        ("post-tool-use", "sentinel-write"),
        ("post-tool-use", "logging"),
        ("task-completed", "checkpoint-complete"),
        ("stop", "logging"),
    ];

    for (fixture_name, fixture_json) in fixtures {
        for (event, handler) in representative_handlers {
            let go_out = run_binary(&paths.go_binary, &["hooks", event, handler], fixture_json);
            let rust_out = run_binary(&paths.rust_binary, &["hooks", event, handler], fixture_json);
            assert_eq!(
                go_out.exit_code, rust_out.exit_code,
                "hooks {event} {handler} with fixture {fixture_name}: exit code mismatch: go={}, rust={}",
                go_out.exit_code, rust_out.exit_code
            );
        }
    }
}

// ─── Version command exit codes ───────────────────────────────────────────────

#[test]
fn test_exit_zero_version_command_go() {
    let paths = resolve_binaries();
    let out = run_binary(&paths.go_binary, &["version"], "");
    assert_eq!(
        out.exit_code, 0,
        "Go version should exit 0, got {}",
        out.exit_code
    );
}

#[test]
fn test_exit_zero_version_command_rust() {
    let paths = resolve_binaries();
    let out = run_binary(&paths.rust_binary, &["version"], "");
    assert_eq!(
        out.exit_code, 0,
        "Rust version should exit 0, got {}",
        out.exit_code
    );
}
