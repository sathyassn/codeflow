//! Integration tests for Claude artifact generalization (INF-TSK-046-004).
//!
//! Verifies that all Claude-facing documentation and configuration artefacts
//! reference the generic `codeflow test` wrapper rather than stack-specific
//! commands (cargo, nextest, llvm-cov, pytest, jest, etc.).
//!
//! The tests read the actual files under test from the project root so that
//! any future regression (re-introducing stack-specific references) is caught
//! by the CI run.

use std::path::{Path, PathBuf};

/// Walk up from the current directory to find the project root (contains `.codeflow/`).
/// Uses `.codeflow/` as the sole marker because the workspace `Cargo.toml` lives
/// under `codeflow-cli/`, not at the project root.
fn find_project_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join(".codeflow").exists() && dir.join(".claude").exists() {
            return dir;
        }
        assert!(
            dir.pop(),
            "cannot find project root (.codeflow + .claude) from {}",
            std::env::current_dir().unwrap().display()
        );
    }
}

/// Read a file relative to the project root and return its content as a `String`.
fn read_project_file(root: &Path, rel_path: &str) -> String {
    let full = root.join(rel_path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("cannot read {}: {e}", full.display()))
}

// ---------------------------------------------------------------------------
// AC #11: cf-test.md uses `codeflow test --mode <mode>` syntax
// ---------------------------------------------------------------------------

#[test]
fn cf_test_uses_codeflow_test_wrapper_syntax() {
    let root = find_project_root();
    let content = read_project_file(&root, ".claude/commands/cf-test.md");

    assert!(
        content.contains("codeflow test"),
        "cf-test.md must reference 'codeflow test' wrapper"
    );
    assert!(
        content.contains("--mode"),
        "cf-test.md must document --mode flag"
    );
}

#[test]
fn cf_test_has_no_raw_cargo_test_references() {
    let root = find_project_root();
    let content = read_project_file(&root, ".claude/commands/cf-test.md");

    let lines_with_cargo_test: Vec<&str> = content
        .lines()
        .filter(|l| l.contains("cargo test") || l.contains("cargo nextest"))
        .collect();
    assert!(
        lines_with_cargo_test.is_empty(),
        "cf-test.md must not reference raw cargo test/nextest; found in lines: {lines_with_cargo_test:?}"
    );
}

#[test]
fn cf_test_has_no_raw_llvm_cov_references() {
    let root = find_project_root();
    let content = read_project_file(&root, ".claude/commands/cf-test.md");

    assert!(
        !content.contains("llvm-cov"),
        "cf-test.md must not reference llvm-cov directly"
    );
}

// ---------------------------------------------------------------------------
// AC #12: cf-doctor.md uses `codeflow test doctor` for test runner check
// ---------------------------------------------------------------------------

#[test]
fn cf_doctor_references_codeflow_test_doctor() {
    let root = find_project_root();
    let content = read_project_file(&root, ".claude/commands/cf-doctor.md");

    assert!(
        content.contains("codeflow test doctor"),
        "cf-doctor.md Check 3 must reference 'codeflow test doctor'"
    );
}

#[test]
fn cf_doctor_repair_table_uses_cargo_build() {
    let root = find_project_root();
    let content = read_project_file(&root, ".claude/commands/cf-doctor.md");

    // The repair table references `cargo build --release` for rebuilding the
    // CodeFlow CLI binary.  This is descriptive prose about CodeFlow's own
    // implementation (like CLAUDE.md §9), not stack-specific agent guidance.
    assert!(
        content.contains("cargo build --release"),
        "cf-doctor.md repair table must reference 'cargo build --release'"
    );
}

// ---------------------------------------------------------------------------
// AC #15: enforcement-policy.json uses canonical `## Test Results` section name
// ---------------------------------------------------------------------------

#[test]
fn enforcement_policy_has_test_results_section() {
    let root = find_project_root();
    let content = read_project_file(
        &root,
        ".codeflow/config/enforcement/enforcement-policy.json",
    );

    assert!(
        content.contains("## Test Results"),
        "enforcement-policy.json must reference '## Test Results'"
    );
}

#[test]
fn enforcement_policy_has_no_legacy_test_stats_section() {
    let root = find_project_root();
    let content = read_project_file(
        &root,
        ".codeflow/config/enforcement/enforcement-policy.json",
    );

    assert!(
        !content.contains("## Test Stats"),
        "enforcement-policy.json must not reference the legacy '## Test Stats'"
    );
}

#[test]
fn enforcement_policy_has_no_exempted_files_section() {
    let root = find_project_root();
    let content = read_project_file(
        &root,
        ".codeflow/config/enforcement/enforcement-policy.json",
    );

    assert!(
        !content.contains("#### Exempted Files"),
        "enforcement-policy.json must not include '#### Exempted Files' as a required section"
    );
}

#[test]
fn enforcement_policy_required_sections_has_six_entries() {
    let root = find_project_root();
    let content = read_project_file(
        &root,
        ".codeflow/config/enforcement/enforcement-policy.json",
    );

    // Parse required_sections array from the "pr" object.
    let policy: serde_json::Value =
        serde_json::from_str(&content).expect("enforcement-policy.json must be valid JSON");

    let sections = policy["git_format"]["pr"]["required_sections"]
        .as_array()
        .expect("required_sections must be an array");

    assert_eq!(
        sections.len(),
        6,
        "enforcement-policy.json required_sections must have exactly 6 entries; got {sections:?}"
    );
}

// ---------------------------------------------------------------------------
// AC #20: gh_pr_guard.rs module exists and exports validate_test_results_section
// ---------------------------------------------------------------------------

#[test]
fn gh_pr_guard_module_file_exists() {
    let root = find_project_root();
    let path = root.join("codeflow-cli/core/src/hooks/gh_pr_guard.rs");
    assert!(
        path.exists(),
        "gh_pr_guard.rs must exist at codeflow-cli/core/src/hooks/gh_pr_guard.rs"
    );
}

#[test]
fn gh_pr_guard_module_validates_canonical_test_results() {
    // Verify the function is reachable via the crate's public path and
    // that a canonical body passes without errors.
    let canonical = "\
## Summary\n- Add feature\n\
## Testing\n- Full suite run\n\
## Test Results\n\
### 1. Overall Test Pass Status\n- Result: 200 passed, 0 failed\n\
### 2. Overall Coverage\n- Workspace: 90%\n\
### 3. Modified File Coverage\n| File | Coverage | Threshold | Status |\n|---|---|---|---|\n| src/lib.rs | 95% | 85% | PASS |\n";

    let errors = codeflow_core::hooks::gh_pr_guard::validate_test_results_section(canonical);
    assert!(
        errors.is_empty(),
        "canonical ## Test Results body should pass; errors: {errors:?}"
    );
}

#[test]
fn gh_pr_guard_module_rejects_legacy_test_stats() {
    let legacy = "\
## Summary\n- Add feature\n\
## Test Stats\n\
### 1. Overall Test Pass Status\n- Result: pass\n\
### 2. Overall Coverage\n- 90%\n\
#### Exempted Files\n| f | c | t | r |\n|-|-|-|-|\n\
### 3. Modified File Coverage\n| f | c | t | s |\n|-|-|-|-|\n";

    let errors = codeflow_core::hooks::gh_pr_guard::validate_test_results_section(legacy);
    assert!(
        !errors.is_empty(),
        "legacy ## Test Stats body must produce errors"
    );
    assert!(
        errors[0].contains("codeflow test --mode full --coverage"),
        "error must reference regeneration command; got: {:?}",
        errors[0]
    );
}

#[test]
fn gh_pr_guard_module_accepts_fresh_project_body() {
    let fresh = "## Test Results\nNo test targets configured.\n";
    let errors = codeflow_core::hooks::gh_pr_guard::validate_test_results_section(fresh);
    assert!(
        errors.is_empty(),
        "fresh-project body should pass; errors: {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// AC #21: pre_tool_use.rs re-exports GhPrGuard and helpers from gh_pr_guard
// ---------------------------------------------------------------------------

#[test]
fn pre_tool_use_reexports_gh_pr_guard_types() {
    // Verify the re-exported is_emoji and strip_zero_width helpers are reachable.
    use codeflow_core::hooks::pre_tool_use::{is_emoji, strip_zero_width};
    assert!(!is_emoji('A'), "letter A should not be emoji");
    assert!(!is_emoji('z'), "letter z should not be emoji");
    let clean = strip_zero_width("hello\u{200B}world");
    assert_eq!(clean, "helloworld", "zero-width space should be stripped");
}
