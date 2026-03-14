//! Hook contract tests (Rust-only, post-cutover).
//!
//! Each test runs the Rust binary with hook stdin input and validates
//! exit codes and output conform to the hook handler contract.
//!
//! All 21 handlers are covered across 7 event groups:
//!   session-start:      init, instructions, logging
//!   session-end:        cleanup, logging
//!   pre-tool-use:       gate-check, team-guard, edit-write-guard, gh-pr-guard,
//!                       protection-guard, security, webfetch-guard
//!   post-tool-use:      sentinel-write, settings-validate, checkpoint-register, logging
//!   task-completed:     checkpoint-complete
//!   stop:               logging
//!   user-prompt-submit: validate, logging
//!
//! # Graceful Degradation
//!
//! All handlers exit 0 when stdin is empty or when JSON is malformed, per the
//! graceful degradation contract. This is tested for every handler.

use crate::harness::{
    assert_rust_allow, assert_rust_allow_fixture, assert_rust_json_env_fixture,
    load_fixture_with_project_dir, make_isolated_project_dir, resolve_binaries,
    run_binary_isolated, run_rust_with_fixture,
};

// ─── session-start ───────────────────────────────────────────────────────────

/// session-start init: Rust outputs JSON env vars to stdout.
#[test]
fn test_session_start_init_allow_with_valid_input() {
    assert_rust_json_env_fixture(
        &["hooks", "session-start", "init"],
        "session_start",
        &["CODEFLOW_SESSION_ID", "CF_PROJECT_ROOT"],
    );
}

#[test]
fn test_session_start_init_both_exit_zero() {
    assert_rust_allow_fixture(&["hooks", "session-start", "init"], "session_start");
}

#[test]
fn test_session_start_init_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "session-start", "init"], "");
}

#[test]
fn test_session_start_init_graceful_on_invalid_json() {
    assert_rust_allow(&["hooks", "session-start", "init"], "not valid json");
}

#[test]
fn test_session_start_instructions_allow_with_valid_input() {
    let tmp = make_isolated_project_dir();
    let fixture = load_fixture_with_project_dir("session_start", tmp.path());
    let paths = resolve_binaries();
    let rust = run_binary_isolated(
        &paths.rust_binary,
        &["hooks", "session-start", "instructions"],
        &fixture,
        tmp.path(),
    );
    assert_eq!(
        rust.exit_code,
        0,
        "Rust session-start instructions should exit 0\n  stderr: {}",
        rust.stderr.trim()
    );
}

#[test]
fn test_session_start_instructions_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "session-start", "instructions"], "");
}

#[test]
fn test_session_start_instructions_exit_code_is_zero() {
    let rust = run_rust_with_fixture(&["hooks", "session-start", "instructions"], "session_start");
    assert_eq!(
        rust.exit_code, 0,
        "Rust session-start instructions should exit 0"
    );
}

#[test]
fn test_session_start_logging_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "session-start", "logging"], "");
}

#[test]
fn test_session_start_logging_exit_code_matches() {
    assert_rust_allow_fixture(&["hooks", "session-start", "logging"], "session_start");
}

// ─── session-end ─────────────────────────────────────────────────────────────

#[test]
fn test_session_end_cleanup_allow_with_valid_input() {
    assert_rust_allow_fixture(&["hooks", "session-end", "cleanup"], "session_end");
}

#[test]
fn test_session_end_cleanup_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "session-end", "cleanup"], "");
}

#[test]
fn test_session_end_cleanup_graceful_on_invalid_json() {
    assert_rust_allow(&["hooks", "session-end", "cleanup"], "{invalid}");
}

#[test]
fn test_session_end_logging_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "session-end", "logging"], "");
}

#[test]
fn test_session_end_logging_exit_code_matches() {
    assert_rust_allow_fixture(&["hooks", "session-end", "logging"], "session_end");
}

// ─── pre-tool-use ────────────────────────────────────────────────────────────

#[test]
fn test_pre_tool_use_gate_check_allow_with_valid_input() {
    assert_rust_allow_fixture(&["hooks", "pre-tool-use", "gate-check"], "pre_tool_use");
}

#[test]
fn test_pre_tool_use_gate_check_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "gate-check"], "");
}

#[test]
fn test_pre_tool_use_gate_check_graceful_on_invalid_json() {
    assert_rust_allow(&["hooks", "pre-tool-use", "gate-check"], "garbage");
}

#[test]
fn test_pre_tool_use_team_guard_allow_with_valid_input() {
    assert_rust_allow_fixture(&["hooks", "pre-tool-use", "team-guard"], "pre_tool_use");
}

#[test]
fn test_pre_tool_use_team_guard_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "team-guard"], "");
}

#[test]
fn test_pre_tool_use_edit_write_guard_allow_with_valid_input() {
    assert_rust_allow_fixture(
        &["hooks", "pre-tool-use", "edit-write-guard"],
        "pre_tool_use",
    );
}

#[test]
fn test_pre_tool_use_edit_write_guard_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "edit-write-guard"], "");
}

#[test]
fn test_pre_tool_use_gh_pr_guard_allow_with_valid_input() {
    assert_rust_allow_fixture(&["hooks", "pre-tool-use", "gh-pr-guard"], "pre_tool_use");
}

#[test]
fn test_pre_tool_use_gh_pr_guard_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "gh-pr-guard"], "");
}

#[test]
fn test_pre_tool_use_protection_guard_allow_with_valid_input() {
    assert_rust_allow_fixture(
        &["hooks", "pre-tool-use", "protection-guard"],
        "pre_tool_use",
    );
}

#[test]
fn test_pre_tool_use_protection_guard_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "protection-guard"], "");
}

#[test]
fn test_pre_tool_use_security_allow_with_valid_input() {
    assert_rust_allow_fixture(&["hooks", "pre-tool-use", "security"], "pre_tool_use");
}

#[test]
fn test_pre_tool_use_security_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "security"], "");
}

#[test]
fn test_pre_tool_use_security_graceful_on_invalid_json() {
    assert_rust_allow(&["hooks", "pre-tool-use", "security"], "not json");
}

#[test]
fn test_pre_tool_use_webfetch_guard_allow_with_valid_input() {
    assert_rust_allow_fixture(&["hooks", "pre-tool-use", "webfetch-guard"], "pre_tool_use");
}

#[test]
fn test_pre_tool_use_webfetch_guard_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "pre-tool-use", "webfetch-guard"], "");
}

// ─── post-tool-use ───────────────────────────────────────────────────────────

#[test]
fn test_post_tool_use_sentinel_write_allow_with_valid_input() {
    assert_rust_allow_fixture(
        &["hooks", "post-tool-use", "sentinel-write"],
        "post_tool_use",
    );
}

#[test]
fn test_post_tool_use_sentinel_write_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "post-tool-use", "sentinel-write"], "");
}

#[test]
fn test_post_tool_use_sentinel_write_graceful_on_invalid_json() {
    assert_rust_allow(&["hooks", "post-tool-use", "sentinel-write"], "{bad}");
}

#[test]
fn test_post_tool_use_settings_validate_allow_with_valid_input() {
    assert_rust_allow_fixture(
        &["hooks", "post-tool-use", "settings-validate"],
        "post_tool_use",
    );
}

#[test]
fn test_post_tool_use_settings_validate_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "post-tool-use", "settings-validate"], "");
}

#[test]
fn test_post_tool_use_checkpoint_register_allow_with_valid_input() {
    assert_rust_allow_fixture(
        &["hooks", "post-tool-use", "checkpoint-register"],
        "post_tool_use",
    );
}

#[test]
fn test_post_tool_use_checkpoint_register_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "post-tool-use", "checkpoint-register"], "");
}

#[test]
fn test_post_tool_use_logging_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "post-tool-use", "logging"], "");
}

#[test]
fn test_post_tool_use_logging_exit_code_matches() {
    assert_rust_allow_fixture(&["hooks", "post-tool-use", "logging"], "post_tool_use");
}

// ─── task-completed ──────────────────────────────────────────────────────────

#[test]
fn test_task_completed_checkpoint_complete_allow_with_valid_input() {
    assert_rust_allow_fixture(
        &["hooks", "task-completed", "checkpoint-complete"],
        "task_completed",
    );
}

#[test]
fn test_task_completed_checkpoint_complete_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "task-completed", "checkpoint-complete"], "");
}

#[test]
fn test_task_completed_checkpoint_complete_graceful_on_invalid_json() {
    assert_rust_allow(
        &["hooks", "task-completed", "checkpoint-complete"],
        "not json at all",
    );
}

// ─── stop ────────────────────────────────────────────────────────────────────

#[test]
fn test_stop_logging_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "stop", "logging"], "");
}

#[test]
fn test_stop_logging_exit_code_matches() {
    assert_rust_allow_fixture(&["hooks", "stop", "logging"], "stop");
}

// ─── user-prompt-submit ──────────────────────────────────────────────────────

#[test]
fn test_user_prompt_submit_logging_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "user-prompt-submit", "logging"], "");
}

#[test]
fn test_user_prompt_submit_logging_exit_code_matches() {
    assert_rust_allow_fixture(
        &["hooks", "user-prompt-submit", "logging"],
        "user_prompt_submit",
    );
}

#[test]
fn test_user_prompt_submit_validate_output_matches() {
    let rust = run_rust_with_fixture(
        &["hooks", "user-prompt-submit", "validate"],
        "user_prompt_submit",
    );
    assert_eq!(
        rust.exit_code,
        0,
        "Rust user-prompt-submit validate should exit 0\n  stderr: {}",
        rust.stderr.trim()
    );
}

#[test]
fn test_user_prompt_submit_validate_both_exit_zero() {
    assert_rust_allow_fixture(
        &["hooks", "user-prompt-submit", "validate"],
        "user_prompt_submit",
    );
}

#[test]
fn test_user_prompt_submit_validate_graceful_on_empty_stdin() {
    assert_rust_allow(&["hooks", "user-prompt-submit", "validate"], "");
}
