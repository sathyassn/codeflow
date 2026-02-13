#!/usr/bin/env bash
# Purpose:   Test cf-worktree-cleanup.sh functionality
# Usage:     bash test-cf-worktree-cleanup.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-cleanup.sh"

# Source test helpers
source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"

# Create isolated test area
TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/cf-test-cleanup-XXXXXX")

_cleanup() {
    rm -rf "$TEST_TMPDIR" 2>/dev/null || true
}
trap _cleanup EXIT

# =============================================================================
# BASIC TESTS
# =============================================================================

test_section "Basic Script Tests"

test_script_exists_and_executable() {
    if [[ -x "$SCRIPT_UNDER_TEST" ]]; then
        test_pass "Script exists and is executable"
    else
        test_fail "Script not found or not executable"
    fi
}

test_script_syntax() {
    if bash -n "$SCRIPT_UNDER_TEST" 2>/dev/null; then
        test_pass "Script has valid syntax"
    else
        test_fail "Script has syntax errors"
    fi
}

test_help_option() {
    local output
    output=$("$SCRIPT_UNDER_TEST" --help 2>&1)

    assert_contains "$output" "cf-worktree-cleanup.sh" "Help shows script name"
    assert_contains "$output" "--path" "Help shows --path option"
    assert_contains "$output" "--dry-run" "Help shows --dry-run option"
    assert_contains "$output" "--force" "Help shows --force option"
    assert_contains "$output" "--status" "Help shows --status option"
    assert_contains "$output" "--days" "Help shows --days option"
    assert_contains "$output" "--prune" "Help shows --prune option"
    assert_contains "$output" "PATHFLOW" "Help shows PathFlow section"
}

test_version_option() {
    local output
    output=$("$SCRIPT_UNDER_TEST" --version 2>&1)

    assert_contains "$output" "version" "Version output contains 'version'"
    assert_contains "$output" "1.1.0" "Version shows 1.1.0"
}

test_script_exists_and_executable
test_script_syntax
test_help_option
test_version_option

# =============================================================================
# ARGUMENT VALIDATION TESTS
# =============================================================================

test_section "Argument Validation"

test_unknown_option() {
    local output exit_code=0
    output=$("$SCRIPT_UNDER_TEST" --bogus 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "Unknown option rejected"
    else
        test_fail "Unknown option should be rejected"
    fi
    assert_contains "$output" "Unknown option" "Error message mentions unknown option"
}

test_status_missing_arg() {
    local output exit_code=0
    output=$("$SCRIPT_UNDER_TEST" --status 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "--status without arg rejected"
    else
        test_fail "--status without arg should be rejected"
    fi
}

test_status_invalid_value() {
    local output exit_code=0
    output=$("$SCRIPT_UNDER_TEST" --status invalid 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "--status with invalid value rejected"
    else
        test_fail "--status with invalid value should be rejected"
    fi
    assert_contains "$output" "stale, merged, abandoned" "Error shows valid values"
}

test_days_missing_arg() {
    local output exit_code=0
    output=$("$SCRIPT_UNDER_TEST" --days 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "--days without arg rejected"
    else
        test_fail "--days without arg should be rejected"
    fi
}

test_days_non_numeric() {
    local output exit_code=0
    output=$("$SCRIPT_UNDER_TEST" --days abc 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "--days with non-numeric arg rejected"
    else
        test_fail "--days with non-numeric should be rejected"
    fi
    assert_contains "$output" "positive integer" "Error mentions positive integer"
}

test_path_missing_arg() {
    local output exit_code=0
    output=$("$SCRIPT_UNDER_TEST" --path 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "--path without arg rejected"
    else
        test_fail "--path without arg should be rejected"
    fi
}

test_unknown_option
test_status_missing_arg
test_status_invalid_value
test_days_missing_arg
test_days_non_numeric
test_path_missing_arg

# =============================================================================
# DRY RUN TESTS
# =============================================================================

test_section "Dry Run Mode"

test_dry_run_no_worktrees() {
    local output
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run 2>&1) || true

    assert_contains "$output" "DRY RUN" "Dry run mode indicated in output"
}

test_dry_run_with_days() {
    local output
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run --days 7 2>&1) || true

    assert_contains "$output" "7 days" "Custom days threshold shown"
}

test_dry_run_no_worktrees
test_dry_run_with_days

# =============================================================================
# PRUNE MODE TESTS
# =============================================================================

test_section "Prune Mode"

test_prune_runs() {
    local output exit_code=0
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --prune 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "Prune mode runs successfully"
    else
        test_fail "Prune mode should succeed (exit $exit_code)"
    fi
    assert_contains "$output" "Prune" "Prune header shown"
}

test_prune_dry_run() {
    local output exit_code=0
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --prune --dry-run 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "Prune dry-run mode runs successfully"
    else
        test_fail "Prune dry-run should succeed (exit $exit_code)"
    fi
    assert_contains "$output" "DRY RUN" "Dry run indicated in prune"
}

test_prune_runs
test_prune_dry_run

# =============================================================================
# PATHFLOW BLOCKING TESTS
# =============================================================================

test_section "PathFlow Awareness"

test_pathflow_blocks_when_active() {
    # Create a fake pathflow-active flag
    local fake_session_dir="$TEST_TMPDIR/fake-repo/.state/session/test-session"
    mkdir -p "$fake_session_dir"
    touch "$fake_session_dir/is-pathflow-active"

    # Create a minimal git repo
    local fake_repo="$TEST_TMPDIR/fake-repo"
    mkdir -p "$fake_repo/.codeflow/scripts/security/lib"
    mkdir -p "$fake_repo/.codeflow/config/enforcement"
    mkdir -p "$fake_repo/.state"

    # Copy context-lib to fake repo
    cp "$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh" \
       "$fake_repo/.codeflow/scripts/security/lib/" 2>/dev/null || true

    git -C "$fake_repo" init -q 2>/dev/null || true
    git -C "$fake_repo" commit --allow-empty -m "init" -q 2>/dev/null || true

    local output exit_code=0
    REPO_ROOT="$fake_repo" CODEFLOW_SESSION_ID="test-session" \
        "$SCRIPT_UNDER_TEST" --dry-run 2>&1 || exit_code=$?
    output=$(REPO_ROOT="$fake_repo" CODEFLOW_SESSION_ID="test-session" \
        "$SCRIPT_UNDER_TEST" --dry-run 2>&1) || exit_code=$?

    if [[ $exit_code -eq 2 ]]; then
        test_pass "PathFlow active blocks cleanup (exit 2)"
    else
        test_fail "PathFlow active should block cleanup (exit $exit_code)"
    fi
    assert_contains "$output" "BLOCKED" "Block message shown"
    assert_contains "$output" "PF-7" "PF-7 teardown mentioned"
}

test_pathflow_force_overrides() {
    local fake_session_dir="$TEST_TMPDIR/fake-repo2/.state/session/test-session2"
    mkdir -p "$fake_session_dir"
    touch "$fake_session_dir/is-pathflow-active"

    local fake_repo="$TEST_TMPDIR/fake-repo2"
    mkdir -p "$fake_repo/.codeflow/scripts/security/lib"
    mkdir -p "$fake_repo/.state"

    cp "$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh" \
       "$fake_repo/.codeflow/scripts/security/lib/" 2>/dev/null || true

    git -C "$fake_repo" init -q 2>/dev/null || true
    git -C "$fake_repo" commit --allow-empty -m "init" -q 2>/dev/null || true

    local output exit_code=0
    output=$(REPO_ROOT="$fake_repo" CODEFLOW_SESSION_ID="test-session2" \
        "$SCRIPT_UNDER_TEST" --force --dry-run 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "PathFlow active with --force proceeds"
    else
        test_fail "PathFlow active with --force should proceed (exit $exit_code)"
    fi
}

test_no_pathflow_allows() {
    # No pathflow flag = should proceed
    local fake_repo="$TEST_TMPDIR/fake-repo3"
    mkdir -p "$fake_repo/.codeflow/scripts/security/lib"
    mkdir -p "$fake_repo/.state"

    cp "$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh" \
       "$fake_repo/.codeflow/scripts/security/lib/" 2>/dev/null || true

    git -C "$fake_repo" init -q 2>/dev/null || true
    git -C "$fake_repo" commit --allow-empty -m "init" -q 2>/dev/null || true

    local output exit_code=0
    output=$(REPO_ROOT="$fake_repo" CODEFLOW_SESSION_ID="none" \
        "$SCRIPT_UNDER_TEST" --dry-run 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "No PathFlow flag allows cleanup"
    else
        test_fail "No PathFlow flag should allow cleanup (exit $exit_code)"
    fi
}

test_pathflow_blocks_when_active
test_pathflow_force_overrides
test_no_pathflow_allows

# =============================================================================
# FUNCTIONAL WORKTREE TESTS
# =============================================================================

test_section "Functional Worktree Tests"

test_create_and_cleanup_worktree() {
    # Create a test git repo with a worktree
    local test_repo="$TEST_TMPDIR/func-repo"
    mkdir -p "$test_repo"
    git -C "$test_repo" init -q 2>/dev/null
    git -C "$test_repo" commit --allow-empty -m "init" -q 2>/dev/null

    # Create a worktree
    local wt_dir="$TEST_TMPDIR/func-wt"
    git -C "$test_repo" worktree add "$wt_dir" -b test-cleanup-branch 2>/dev/null

    # Verify worktree exists
    if [[ -d "$wt_dir" ]]; then
        test_pass "Test worktree created"
    else
        test_fail "Could not create test worktree"
        return
    fi

    # Run cleanup with --force --path to remove it
    local output exit_code=0
    output=$(REPO_ROOT="$test_repo" \
        "$SCRIPT_UNDER_TEST" --path "$wt_dir" --force 2>&1) || exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        test_pass "Cleanup with --path succeeds"
    else
        test_fail "Cleanup with --path should succeed (exit $exit_code)"
    fi

    # Verify worktree was removed
    if [[ ! -d "$wt_dir" ]]; then
        test_pass "Worktree directory removed"
    else
        test_fail "Worktree directory should be removed"
        # Clean up manually
        git -C "$test_repo" worktree remove --force "$wt_dir" 2>/dev/null || true
        rm -rf "$wt_dir" 2>/dev/null || true
    fi
}

test_path_nonexistent() {
    local output exit_code=0
    output=$(REPO_ROOT="$REPO_ROOT" \
        "$SCRIPT_UNDER_TEST" --path "/tmp/nonexistent-worktree-path" 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "--path with nonexistent path fails"
    else
        test_fail "--path with nonexistent path should fail"
    fi
    assert_contains "$output" "does not exist" "Error mentions path doesn't exist"
}

test_dry_run_preserves_worktree() {
    # Create a test git repo with a worktree
    local test_repo="$TEST_TMPDIR/preserve-repo"
    mkdir -p "$test_repo"
    git -C "$test_repo" init -q 2>/dev/null
    git -C "$test_repo" commit --allow-empty -m "init" -q 2>/dev/null

    local wt_dir="$TEST_TMPDIR/preserve-wt"
    git -C "$test_repo" worktree add "$wt_dir" -b test-preserve-branch 2>/dev/null

    # Run cleanup with --dry-run --path
    local output exit_code=0
    output=$(REPO_ROOT="$test_repo" \
        "$SCRIPT_UNDER_TEST" --path "$wt_dir" --dry-run 2>&1) || exit_code=$?

    # Verify worktree still exists
    if [[ -d "$wt_dir" ]]; then
        test_pass "Dry run preserves worktree"
    else
        test_fail "Dry run should preserve worktree"
    fi

    assert_contains "$output" "DRY RUN" "Dry run indicated in output"

    # Clean up
    git -C "$test_repo" worktree remove --force "$wt_dir" 2>/dev/null || true
    rm -rf "$wt_dir" 2>/dev/null || true
}

test_create_and_cleanup_worktree
test_path_nonexistent
test_dry_run_preserves_worktree

# =============================================================================
# SOURCE GUARD TEST
# =============================================================================

test_section "Source Guard"

test_source_guard() {
    local output exit_code=0
    # Try to source the script (should fail with error message)
    output=$(bash -c "source '$SCRIPT_UNDER_TEST'" 2>&1) || exit_code=$?

    if [[ $exit_code -ne 0 ]]; then
        test_pass "Source guard prevents sourcing"
    else
        test_fail "Source guard should prevent sourcing"
    fi
}

test_source_guard

# =============================================================================
# CONFIG-DRIVEN DEFAULTS TEST
# =============================================================================

test_section "Config-Driven Behavior"

test_default_stale_days() {
    local output
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run 2>&1) || true

    # Default threshold should appear (14 days unless config overrides)
    assert_contains "$output" "days" "Stale threshold displayed"
}

test_custom_days_override() {
    local output
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run --days 30 2>&1) || true

    assert_contains "$output" "30 days" "Custom days override displayed"
}

test_default_stale_days
test_custom_days_override

# =============================================================================
# HEADER AND FORMAT TESTS
# =============================================================================

test_section "Output Format"

test_header_shown() {
    local output
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run 2>&1) || true

    assert_contains "$output" "Worktree Cleanup" "Cleanup header shown"
}

test_status_filter_shown() {
    local output
    cd "$REPO_ROOT"
    output=$("$SCRIPT_UNDER_TEST" --dry-run --status merged 2>&1) || true

    assert_contains "$output" "Filter: merged" "Status filter displayed"
}

test_header_shown
test_status_filter_shown

# =============================================================================
# SUMMARY
# =============================================================================

print_test_summary

# Exit with appropriate code
[[ "$TEST_FAIL_COUNT" -eq 0 ]] && exit 0 || exit 1
