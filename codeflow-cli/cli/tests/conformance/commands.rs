//! CLI subcommand contract tests (Rust-only, post-cutover).
//!
//! Tests that key CLI subcommands produce expected output from the Rust binary.

use crate::harness::{resolve_binaries, run_binary};

// ─── version (via --version flag) ────────────────────────────────────────────

#[test]
fn test_version_flag_exit_zero() {
    let rust = crate::harness::run_rust(&["--version"], "");
    assert_eq!(rust.exit_code, 0, "--version should exit 0");
}

#[test]
fn test_version_flag_stdout_contains_codeflow() {
    let paths = resolve_binaries();
    let rust_out = run_binary(&paths.rust_binary, &["--version"], "");
    assert!(
        rust_out.stdout.contains("codeflow"),
        "--version should contain 'codeflow' in stdout: {:?}",
        rust_out.stdout
    );
}

#[test]
fn test_version_flag_stdout_contains_git_hash() {
    let paths = resolve_binaries();
    let rust_out = run_binary(&paths.rust_binary, &["--version"], "");
    // Version format: "codeflow 0.1.0 (abcdef12 2026-03-23T05:40:47Z)"
    assert!(
        rust_out.stdout.contains('(') && rust_out.stdout.contains(')'),
        "--version should contain git hash in parentheses: {:?}",
        rust_out.stdout
    );
}

// ─── hooks (without stdin — graceful degradation) ────────────────────────────

#[test]
fn test_hooks_all_handlers_exit_zero_with_empty_stdin() {
    let handler_specs = &[
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

    let paths = resolve_binaries();
    for (event, handler) in handler_specs {
        let rust_out = run_binary(&paths.rust_binary, &["hooks", event, handler], "");
        assert_eq!(
            rust_out.exit_code, 0,
            "hooks {event} {handler} should exit 0 with empty stdin, got {}",
            rust_out.exit_code
        );
    }
}

// ─── validate subcommand ─────────────────────────────────────────────────────

#[test]
fn test_validate_rust_runs_validation() {
    let paths = resolve_binaries();
    let out = run_binary(&paths.rust_binary, &["validate", "all"], "");
    // validate all runs real validation — exit code depends on project state,
    // but the command itself should not panic (exit 101 / SIGABRT).
    assert_ne!(out.exit_code, 101, "validate all should not panic");
}

// ─── subcommands that produce no output (stub implementations) ───────────────

#[test]
fn test_welcome_exit_zero() {
    let rust = crate::harness::run_rust(&["welcome"], "");
    assert_eq!(rust.exit_code, 0, "welcome should exit 0");
}

#[test]
fn test_normalize_exit_zero() {
    let rust = crate::harness::run_rust(&["normalize"], "");
    assert_eq!(rust.exit_code, 0, "normalize should exit 0");
}

// ─── graceful degradation: invalid subcommands ───────────────────────────────

#[test]
fn test_unknown_subcommand_fails() {
    let rust = crate::harness::run_rust(&["nonexistent-subcommand-xyz"], "");
    assert_ne!(rust.exit_code, 0, "should reject unknown subcommand");
}

#[test]
fn test_hooks_without_event_group_fails() {
    let paths = resolve_binaries();
    let rust_out = run_binary(&paths.rust_binary, &["hooks"], "");
    assert_ne!(
        rust_out.exit_code, 0,
        "should reject hooks without event group"
    );
}
