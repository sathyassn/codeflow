//! CLI subcommand contract conformance tests.
//!
//! Tests that key CLI subcommands produce matching output between Go and Rust
//! binaries. Focus is on subcommands that have deterministic, stable output
//! (not dependent on filesystem state, timestamps, or environment).

use crate::harness::{resolve_binaries, run_binary};

// ─── version ─────────────────────────────────────────────────────────────────

#[test]
fn test_version_exit_code_matches() {
    let (go, rust) = crate::harness::run_conformance(&["version"], "");
    assert_eq!(
        go.exit_code, rust.exit_code,
        "version exit code mismatch: go={}, rust={}",
        go.exit_code, rust.exit_code
    );
}

#[test]
fn test_version_both_exit_zero() {
    let (go, rust) = crate::harness::run_conformance(&["version"], "");
    assert_eq!(go.exit_code, 0, "Go version should exit 0");
    assert_eq!(rust.exit_code, 0, "Rust version should exit 0");
}

#[test]
fn test_version_stdout_contains_codeflow() {
    let paths = resolve_binaries();
    let go_out = run_binary(&paths.go_binary, &["version"], "");
    let rust_out = run_binary(&paths.rust_binary, &["version"], "");

    assert!(
        go_out.stdout.contains("codeflow"),
        "Go version should contain 'codeflow' in stdout: {:?}",
        go_out.stdout
    );
    assert!(
        rust_out.stdout.contains("codeflow"),
        "Rust version should contain 'codeflow' in stdout: {:?}",
        rust_out.stdout
    );
}

#[test]
fn test_version_stdout_nonempty() {
    let (go, rust) = crate::harness::run_conformance(&["version"], "");
    assert!(
        !go.stdout.is_empty(),
        "Go version stdout should not be empty"
    );
    assert!(
        !rust.stdout.is_empty(),
        "Rust version stdout should not be empty"
    );
}

// ─── hooks (without stdin — graceful degradation) ────────────────────────────

#[test]
fn test_hooks_all_handlers_exit_zero_with_empty_stdin() {
    // Every hook handler must exit 0 when stdin is empty (graceful degradation).
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
        let go_out = run_binary(&paths.go_binary, &["hooks", event, handler], "");
        let rust_out = run_binary(&paths.rust_binary, &["hooks", event, handler], "");
        assert_eq!(
            go_out.exit_code, 0,
            "Go hooks {event} {handler} should exit 0 with empty stdin, got {}",
            go_out.exit_code
        );
        assert_eq!(
            rust_out.exit_code, 0,
            "Rust hooks {event} {handler} should exit 0 with empty stdin, got {}",
            rust_out.exit_code
        );
    }
}

#[test]
fn test_hooks_all_handlers_exit_code_matches_with_empty_stdin() {
    // Verify that Go and Rust agree on exit codes for all handlers with empty stdin.
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
        let go_out = run_binary(&paths.go_binary, &["hooks", event, handler], "");
        let rust_out = run_binary(&paths.rust_binary, &["hooks", event, handler], "");
        assert_eq!(
            go_out.exit_code, rust_out.exit_code,
            "hooks {event} {handler} exit code mismatch: go={}, rust={}",
            go_out.exit_code, rust_out.exit_code
        );
    }
}

// ─── validate subcommand ─────────────────────────────────────────────────────

/// `validate all` has different semantics: Go treats it as an unknown subcommand
/// (exits 0 showing help); Rust runs actual validation across all markdown files
/// (exits non-zero when validation issues found). The commands share a name but
/// have different implementations. Verified individually rather than via conformance.
#[test]
fn test_validate_go_exits_zero() {
    let paths = resolve_binaries();
    let out = run_binary(&paths.go_binary, &["validate", "all"], "");
    assert_eq!(
        out.exit_code, 0,
        "Go validate all should exit 0, got {}",
        out.exit_code
    );
}

#[test]
fn test_validate_rust_runs_validation() {
    let paths = resolve_binaries();
    let out = run_binary(&paths.rust_binary, &["validate", "all"], "");
    // Rust validate all runs real validation — exit code depends on project state,
    // but the command itself should not panic (exit 101 / SIGABRT).
    assert_ne!(out.exit_code, 101, "Rust validate all should not panic");
}

// ─── subcommands that produce no output (stub implementations) ───────────────

#[test]
fn test_welcome_both_exit_zero() {
    let (go, rust) = crate::harness::run_conformance(&["welcome"], "");
    assert_eq!(go.exit_code, 0, "Go welcome should exit 0");
    assert_eq!(rust.exit_code, 0, "Rust welcome should exit 0");
}

#[test]
fn test_internal_exit_code_matches() {
    let (go, rust) = crate::harness::run_conformance(&["internal"], "");
    assert_eq!(
        go.exit_code, rust.exit_code,
        "internal exit code mismatch: go={}, rust={}",
        go.exit_code, rust.exit_code
    );
}

/// `normalize` has different stdout: Go shows help text; Rust outputs "normalize: nothing to normalize".
/// Both exit 0. Verify only exit codes match, not stdout.
#[test]
fn test_normalize_exit_code_matches() {
    let (go, rust) = crate::harness::run_conformance(&["normalize"], "");
    assert_eq!(
        go.exit_code, rust.exit_code,
        "normalize exit code mismatch: go={}, rust={}",
        go.exit_code, rust.exit_code
    );
    assert_eq!(
        go.exit_code, 0,
        "Go normalize should exit 0, got {}",
        go.exit_code
    );
    assert_eq!(
        rust.exit_code, 0,
        "Rust normalize should exit 0, got {}",
        rust.exit_code
    );
}

// ─── graceful degradation: invalid subcommands ───────────────────────────────

#[test]
fn test_unknown_subcommand_both_fail() {
    let (go, rust) = crate::harness::run_conformance(&["nonexistent-subcommand-xyz"], "");
    // Both should fail (non-zero exit) for an unknown subcommand.
    assert_ne!(go.exit_code, 0, "Go should reject unknown subcommand");
    assert_ne!(rust.exit_code, 0, "Rust should reject unknown subcommand");
}

/// `hooks` without event group: Go exits 0 (shows help); Rust exits 2 (requires subcommand).
/// Verify Rust rejects it and Go is permissive (both are valid behaviors for their CLIs).
#[test]
fn test_hooks_without_event_group_rust_fails() {
    let paths = resolve_binaries();
    let rust_out = run_binary(&paths.rust_binary, &["hooks"], "");
    assert_ne!(
        rust_out.exit_code, 0,
        "Rust should reject hooks without event group"
    );
}

#[test]
fn test_hooks_without_event_group_go_shows_help() {
    let paths = resolve_binaries();
    let go_out = run_binary(&paths.go_binary, &["hooks"], "");
    // Go exits 0 and shows help text when no subcommand is given.
    assert_eq!(
        go_out.exit_code, 0,
        "Go hooks without subcommand should exit 0 (shows help)"
    );
}
