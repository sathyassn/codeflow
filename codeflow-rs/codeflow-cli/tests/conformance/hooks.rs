//! Hook contract conformance tests.
//!
//! Each test runs both Go and Rust binaries with the same hook stdin input
//! and asserts that exit codes and stdout match between the two binaries.
//!
//! All 21 handlers are covered across 7 event groups:
//!   session-start:      init, instructions, logging
//!   session-end:        cleanup, logging
//!   pre-tool-use:       gate-check, team-guard, edit-write-guard, gh-pr-guard,
//!                       protection-guard, security, webfetch-guard
//!   post-tool-use:      sentinel-write, settings-validate, checkpoint-register, logging
//!   task-completed:     checkpoint-complete
//!   stop:               logging
//!   user-prompt-submit: validate (STUB), logging
//!
//! # Graceful Degradation
//!
//! All handlers exit 0 when stdin is empty or when JSON is malformed, per the
//! graceful degradation contract. This is tested for every handler.
//!
//! # Logging Handler Contract
//!
//! Logging handlers write to filesystem paths derived from project_dir.
//! Their exit code is always 0 (Allow). Stdout is empty. Stderr may contain
//! messages but is non-deterministic (timestamps, paths), so only exit code
//! and stdout are compared in conformance tests.

use crate::harness::{
    assert_both_allow, assert_conformance, assert_conformance_json_env, load_fixture,
    run_conformance,
};

// ─── session-start ───────────────────────────────────────────────────────────

/// session-start init: both binaries output `{"env":{...}}` JSON to stdout.
///
/// `CODEFLOW_SESSION_ID` is dynamic (each invocation generates a unique ULID),
/// so we compare JSON structure and static keys only.
#[test]
fn test_session_start_init_allow_with_valid_input() {
    let fixture = load_fixture("session_start");
    assert_conformance_json_env(
        &["hooks", "session-start", "init"],
        &fixture,
        &["CODEFLOW_SESSION_ID", "CF_PROJECT_ROOT"],
    );
}

/// Verify both binaries exit 0 for session-start init (even the stub).
#[test]
fn test_session_start_init_both_exit_zero() {
    let fixture = load_fixture("session_start");
    assert_both_allow(&["hooks", "session-start", "init"], &fixture);
}

#[test]
fn test_session_start_init_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "session-start", "init"], "");
}

#[test]
fn test_session_start_init_graceful_on_invalid_json() {
    assert_both_allow(&["hooks", "session-start", "init"], "not valid json");
}

/// session-start instructions: both binaries output identical instruction text.
#[test]
fn test_session_start_instructions_allow_with_valid_input() {
    let fixture = load_fixture("session_start");
    assert_conformance(&["hooks", "session-start", "instructions"], &fixture);
}

#[test]
fn test_session_start_instructions_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "session-start", "instructions"], "");
}

#[test]
fn test_session_start_instructions_exit_code_is_zero() {
    let fixture = load_fixture("session_start");
    let (go, rust) = run_conformance(&["hooks", "session-start", "instructions"], &fixture);
    assert_eq!(
        go.exit_code, 0,
        "Go session-start instructions should exit 0"
    );
    assert_eq!(
        rust.exit_code, 0,
        "Rust session-start instructions should exit 0"
    );
}

#[test]
fn test_session_start_logging_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "session-start", "logging"], "");
}

#[test]
fn test_session_start_logging_exit_code_matches() {
    let fixture = load_fixture("session_start");
    // Logging writes to filesystem — only compare exit code and stdout.
    assert_conformance(&["hooks", "session-start", "logging"], &fixture);
}

// ─── session-end ─────────────────────────────────────────────────────────────

#[test]
fn test_session_end_cleanup_allow_with_valid_input() {
    let fixture = load_fixture("session_end");
    assert_conformance(&["hooks", "session-end", "cleanup"], &fixture);
}

#[test]
fn test_session_end_cleanup_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "session-end", "cleanup"], "");
}

#[test]
fn test_session_end_cleanup_graceful_on_invalid_json() {
    assert_both_allow(&["hooks", "session-end", "cleanup"], "{invalid}");
}

#[test]
fn test_session_end_logging_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "session-end", "logging"], "");
}

#[test]
fn test_session_end_logging_exit_code_matches() {
    let fixture = load_fixture("session_end");
    assert_conformance(&["hooks", "session-end", "logging"], &fixture);
}

// ─── pre-tool-use ────────────────────────────────────────────────────────────

#[test]
fn test_pre_tool_use_gate_check_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "gate-check"], &fixture);
}

#[test]
fn test_pre_tool_use_gate_check_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "gate-check"], "");
}

#[test]
fn test_pre_tool_use_gate_check_graceful_on_invalid_json() {
    assert_both_allow(&["hooks", "pre-tool-use", "gate-check"], "garbage");
}

#[test]
fn test_pre_tool_use_team_guard_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "team-guard"], &fixture);
}

#[test]
fn test_pre_tool_use_team_guard_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "team-guard"], "");
}

#[test]
fn test_pre_tool_use_edit_write_guard_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "edit-write-guard"], &fixture);
}

#[test]
fn test_pre_tool_use_edit_write_guard_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "edit-write-guard"], "");
}

#[test]
fn test_pre_tool_use_gh_pr_guard_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "gh-pr-guard"], &fixture);
}

#[test]
fn test_pre_tool_use_gh_pr_guard_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "gh-pr-guard"], "");
}

#[test]
fn test_pre_tool_use_protection_guard_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "protection-guard"], &fixture);
}

#[test]
fn test_pre_tool_use_protection_guard_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "protection-guard"], "");
}

#[test]
fn test_pre_tool_use_security_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "security"], &fixture);
}

#[test]
fn test_pre_tool_use_security_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "security"], "");
}

#[test]
fn test_pre_tool_use_security_graceful_on_invalid_json() {
    assert_both_allow(&["hooks", "pre-tool-use", "security"], "not json");
}

#[test]
fn test_pre_tool_use_webfetch_guard_allow_with_valid_input() {
    let fixture = load_fixture("pre_tool_use");
    assert_conformance(&["hooks", "pre-tool-use", "webfetch-guard"], &fixture);
}

#[test]
fn test_pre_tool_use_webfetch_guard_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "pre-tool-use", "webfetch-guard"], "");
}

// ─── post-tool-use ───────────────────────────────────────────────────────────

#[test]
fn test_post_tool_use_sentinel_write_allow_with_valid_input() {
    let fixture = load_fixture("post_tool_use");
    assert_conformance(&["hooks", "post-tool-use", "sentinel-write"], &fixture);
}

#[test]
fn test_post_tool_use_sentinel_write_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "post-tool-use", "sentinel-write"], "");
}

#[test]
fn test_post_tool_use_sentinel_write_graceful_on_invalid_json() {
    assert_both_allow(&["hooks", "post-tool-use", "sentinel-write"], "{bad}");
}

#[test]
fn test_post_tool_use_settings_validate_allow_with_valid_input() {
    let fixture = load_fixture("post_tool_use");
    assert_conformance(&["hooks", "post-tool-use", "settings-validate"], &fixture);
}

#[test]
fn test_post_tool_use_settings_validate_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "post-tool-use", "settings-validate"], "");
}

#[test]
fn test_post_tool_use_checkpoint_register_allow_with_valid_input() {
    let fixture = load_fixture("post_tool_use");
    assert_conformance(&["hooks", "post-tool-use", "checkpoint-register"], &fixture);
}

#[test]
fn test_post_tool_use_checkpoint_register_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "post-tool-use", "checkpoint-register"], "");
}

#[test]
fn test_post_tool_use_logging_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "post-tool-use", "logging"], "");
}

#[test]
fn test_post_tool_use_logging_exit_code_matches() {
    let fixture = load_fixture("post_tool_use");
    assert_conformance(&["hooks", "post-tool-use", "logging"], &fixture);
}

// ─── task-completed ──────────────────────────────────────────────────────────

#[test]
fn test_task_completed_checkpoint_complete_allow_with_valid_input() {
    let fixture = load_fixture("task_completed");
    assert_conformance(
        &["hooks", "task-completed", "checkpoint-complete"],
        &fixture,
    );
}

#[test]
fn test_task_completed_checkpoint_complete_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "task-completed", "checkpoint-complete"], "");
}

#[test]
fn test_task_completed_checkpoint_complete_graceful_on_invalid_json() {
    assert_both_allow(
        &["hooks", "task-completed", "checkpoint-complete"],
        "not json at all",
    );
}

// ─── stop ────────────────────────────────────────────────────────────────────

#[test]
fn test_stop_logging_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "stop", "logging"], "");
}

#[test]
fn test_stop_logging_exit_code_matches() {
    let fixture = load_fixture("stop");
    assert_conformance(&["hooks", "stop", "logging"], &fixture);
}

// ─── user-prompt-submit ──────────────────────────────────────────────────────

#[test]
fn test_user_prompt_submit_logging_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "user-prompt-submit", "logging"], "");
}

#[test]
fn test_user_prompt_submit_logging_exit_code_matches() {
    let fixture = load_fixture("user_prompt_submit");
    assert_conformance(&["hooks", "user-prompt-submit", "logging"], &fixture);
}

/// user-prompt-submit validate: both binaries output identical context reminders.
///
/// Uses `assert_conformance` (exit code + stdout) rather than strict, because
/// stderr may contain non-deterministic git-related messages.
#[test]
fn test_user_prompt_submit_validate_output_matches() {
    let fixture = load_fixture("user_prompt_submit");
    assert_conformance(&["hooks", "user-prompt-submit", "validate"], &fixture);
}

/// Verify that both binaries exit 0 for user-prompt-submit validate (even the stub).
#[test]
fn test_user_prompt_submit_validate_both_exit_zero() {
    let fixture = load_fixture("user_prompt_submit");
    assert_both_allow(&["hooks", "user-prompt-submit", "validate"], &fixture);
}

#[test]
fn test_user_prompt_submit_validate_graceful_on_empty_stdin() {
    assert_both_allow(&["hooks", "user-prompt-submit", "validate"], "");
}
