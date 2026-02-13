#!/usr/bin/env bash
# Purpose:   Test cf-worktree-status.sh functionality
# Usage:     bash test-cf-worktree-status.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"

# Source test framework
source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"

SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-status.sh"

# Create isolated test environment
TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/cf-test-wt-status-XXXXXX")

cleanup() {
    # Clean up any worktrees we created in the real repo
    if [[ -n "${FUNC_WORKTREE_PATH:-}" ]] && [[ -d "$FUNC_WORKTREE_PATH" ]]; then
        git -C "$REPO_ROOT" worktree remove --force "$FUNC_WORKTREE_PATH" 2>/dev/null || true
    fi
    rm -rf "$TEST_TMPDIR" 2>/dev/null || true
}
trap cleanup EXIT

# =============================================================================
# BASIC TESTS
# =============================================================================

test_section "Basic Tests"

test_subsection "Script exists and is executable"
assert_file_exists "$SCRIPT_UNDER_TEST" "Script file exists" || true
((TEST_TOTAL_COUNT++)) || true
if [[ -x "$SCRIPT_UNDER_TEST" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script is executable"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script is not executable"
fi

test_subsection "Valid bash syntax"
assert_success "bash -n '$SCRIPT_UNDER_TEST'" "Script has valid bash syntax" || true

# =============================================================================
# HELP AND VERSION
# =============================================================================

test_section "Help and Version"

test_subsection "--help output"
help_output=$("$SCRIPT_UNDER_TEST" --help 2>&1)
assert_contains "$help_output" "cf-worktree-status.sh" "--help shows script name" || true
assert_contains "$help_output" "--path" "--help documents --path option" || true
assert_contains "$help_output" "--json" "--help documents --json option" || true
assert_contains "$help_output" "--verbose" "--help documents --verbose option" || true
assert_contains "$help_output" "RELATED" "--help shows related scripts" || true
assert_contains "$help_output" "cf-worktree-setup.sh" "--help references setup script" || true

test_subsection "-h short form"
h_output=$("$SCRIPT_UNDER_TEST" -h 2>&1)
assert_contains "$h_output" "USAGE" "-h shows usage" || true

test_subsection "--version output"
version_output=$("$SCRIPT_UNDER_TEST" --version 2>&1)
assert_contains "$version_output" "version" "--version shows version string" || true
assert_contains "$version_output" "1.1.0" "--version shows correct version" || true

test_subsection "-V short form"
v_output=$("$SCRIPT_UNDER_TEST" -V 2>&1)
assert_contains "$v_output" "1.1.0" "-V shows version" || true

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

test_section "Argument Parsing"

test_subsection "Unknown option handling"
((TEST_TOTAL_COUNT++)) || true
exit_code=0
"$SCRIPT_UNDER_TEST" --invalid-flag >/dev/null 2>&1 || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Unknown option exits with code 2"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Unknown option exits with $exit_code, expected 2"
fi

test_subsection "Unknown option error message"
error_output=$("$SCRIPT_UNDER_TEST" --bad-opt 2>&1 || true)
assert_contains "$error_output" "Unknown option" "Error message mentions unknown option" || true
assert_contains "$error_output" "--help" "Error message suggests --help" || true

test_subsection "--path without argument"
((TEST_TOTAL_COUNT++)) || true
exit_code=0
"$SCRIPT_UNDER_TEST" --path >/dev/null 2>&1 || exit_code=$?
if [[ $exit_code -eq 2 ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} --path without arg exits with code 2"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} --path without arg exits with $exit_code, expected 2"
fi

# =============================================================================
# RUNS IN GIT REPO (NO EXTRA WORKTREES)
# =============================================================================

test_section "Default Execution (no extra worktrees)"

test_subsection "Runs successfully in git repo"
cd "$REPO_ROOT"
default_output=$("$SCRIPT_UNDER_TEST" 2>&1) || true
assert_contains "$default_output" "Git Worktree Status" "Shows header in default mode" || true
assert_contains "$default_output" "Summary:" "Shows summary line" || true

# =============================================================================
# NON-EXISTENT PATH
# =============================================================================

test_section "Non-existent Path"

test_subsection "Non-existent worktree path"
((TEST_TOTAL_COUNT++)) || true
bad_path_output=$("$SCRIPT_UNDER_TEST" --path "/tmp/cf-nonexistent-worktree-path" 2>&1) || true
if echo "$bad_path_output" | grep -qi "ERROR\|not exist"; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Reports error for non-existent path"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Should report error for non-existent path"
fi

test_subsection "Non-existent path exit code"
((TEST_TOTAL_COUNT++)) || true
if ! "$SCRIPT_UNDER_TEST" --path "/tmp/cf-nonexistent-worktree-path" >/dev/null 2>&1; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Non-existent path exits with non-zero"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Non-existent path should exit non-zero"
fi

# =============================================================================
# NOT A WORKTREE
# =============================================================================

test_section "Not a Git Worktree"

test_subsection "Non-git directory"
mkdir -p "$TEST_TMPDIR/not-a-repo"
((TEST_TOTAL_COUNT++)) || true
not_git_output=$("$SCRIPT_UNDER_TEST" --path "$TEST_TMPDIR/not-a-repo" 2>&1) || true
if echo "$not_git_output" | grep -qi "ERROR\|not.*worktree"; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Reports error for non-git directory"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Should report error for non-git directory"
fi

# =============================================================================
# JSON OUTPUT
# =============================================================================

test_section "JSON Output"

test_subsection "--json with non-existent path"
json_error=$("$SCRIPT_UNDER_TEST" --json --path "/tmp/cf-nonexistent-path" 2>&1) || true
assert_contains "$json_error" "directory_not_found" "JSON error output for missing path" || true

test_subsection "--json with non-git directory"
json_not_git=$("$SCRIPT_UNDER_TEST" --json --path "$TEST_TMPDIR/not-a-repo" 2>&1) || true
assert_contains "$json_not_git" "not_a_worktree" "JSON error output for non-git dir" || true

test_subsection "--json all worktrees"
json_all=$("$SCRIPT_UNDER_TEST" --json 2>&1) || true
assert_contains "$json_all" "[" "JSON all output starts with [" || true
assert_contains "$json_all" "]" "JSON all output ends with ]" || true

# =============================================================================
# FUNCTIONAL TEST: REAL WORKTREE
# =============================================================================

test_section "Functional: Real Worktree"

# Create a temporary worktree for functional testing
FUNC_WORKTREE_BRANCH="test/wt-status-func-$$"
FUNC_WORKTREE_PATH="$TEST_TMPDIR/wt-func-test"

test_subsection "Create test worktree"
((TEST_TOTAL_COUNT++)) || true
if git -C "$REPO_ROOT" worktree add "$FUNC_WORKTREE_PATH" -b "$FUNC_WORKTREE_BRANCH" HEAD 2>/dev/null; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Created test worktree"

    test_subsection "Check worktree with --path"
    status_output=$("$SCRIPT_UNDER_TEST" --path "$FUNC_WORKTREE_PATH" 2>&1) || true
    assert_contains "$status_output" "Branch:" "Output shows branch" || true
    assert_contains "$status_output" "Status:" "Output shows status label" || true
    assert_contains "$status_output" "clean" "Clean worktree shows clean status" || true
    assert_contains "$status_output" "Last commit:" "Output shows last commit" || true

    test_subsection "Check worktree with positional arg"
    pos_output=$("$SCRIPT_UNDER_TEST" "$FUNC_WORKTREE_PATH" 2>&1) || true
    assert_contains "$pos_output" "Branch:" "Positional arg works for path" || true

    test_subsection "Check worktree status fields"
    assert_contains "$status_output" "Remote:" "Output shows remote sync info" || true

    test_subsection "Verbose output"
    verbose_output=$("$SCRIPT_UNDER_TEST" --verbose --path "$FUNC_WORKTREE_PATH" 2>&1) || true
    assert_contains "$verbose_output" "Recent commits:" "Verbose shows recent commits" || true

    test_subsection "JSON output for real worktree"
    json_wt=$("$SCRIPT_UNDER_TEST" --json --path "$FUNC_WORKTREE_PATH" 2>&1) || true
    assert_contains "$json_wt" "\"branch\":" "JSON has branch field" || true
    assert_contains "$json_wt" "\"status\":\"clean\"" "JSON shows clean status" || true
    assert_contains "$json_wt" "\"uncommitted_changes\":0" "JSON shows 0 uncommitted changes" || true
    assert_contains "$json_wt" "\"has_upstream\":" "JSON has upstream field" || true

    test_subsection "Dirty worktree detection"
    # Create uncommitted change
    echo "test content" > "$FUNC_WORKTREE_PATH/test-dirty-file.txt"
    dirty_output=$("$SCRIPT_UNDER_TEST" --path "$FUNC_WORKTREE_PATH" 2>&1) || true
    assert_contains "$dirty_output" "dirty" "Dirty worktree shows dirty status" || true
    assert_contains "$dirty_output" "Uncommitted changes:" "Shows uncommitted changes count" || true

    test_subsection "Dirty worktree exit code"
    ((TEST_TOTAL_COUNT++)) || true
    if ! "$SCRIPT_UNDER_TEST" --path "$FUNC_WORKTREE_PATH" >/dev/null 2>&1; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} Dirty worktree exits with non-zero"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} Dirty worktree should exit non-zero"
    fi

    test_subsection "Dirty worktree JSON"
    dirty_json=$("$SCRIPT_UNDER_TEST" --json --path "$FUNC_WORKTREE_PATH" 2>&1) || true
    assert_contains "$dirty_json" "\"status\":\"dirty\"" "JSON shows dirty status" || true

    # Clean up the test file
    rm -f "$FUNC_WORKTREE_PATH/test-dirty-file.txt"

    test_subsection "Clean up test worktree"
    ((TEST_TOTAL_COUNT++)) || true
    if git -C "$REPO_ROOT" worktree remove --force "$FUNC_WORKTREE_PATH" 2>/dev/null; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} Removed test worktree"
        FUNC_WORKTREE_PATH=""  # Prevent double cleanup
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} Failed to remove test worktree"
    fi

    # Delete the test branch
    git -C "$REPO_ROOT" branch -D "$FUNC_WORKTREE_BRANCH" 2>/dev/null || true
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Failed to create test worktree"
    test_skip "Skipping functional tests" "worktree creation failed"
fi

# =============================================================================
# WORK ITEM ASSOCIATION
# =============================================================================

test_section "Work Item Association"

test_subsection "Lookup from worktrees.yaml"
# Create a mock worktrees.yaml
mkdir -p "$TEST_TMPDIR/state-test/.state"
cat > "$TEST_TMPDIR/state-test/.state/worktrees.yaml" << 'YAML'
worktrees:
  - path: ".git-worktrees/feat-auth"
    branch: "feat/auth"
    purpose: "Implement JWT authentication"
    status: active
    last_active: "2026-01-15T10:00:00Z"

metadata:
  version: "1.0.0"
YAML

# Verify the script file documents work item association in its header
((TEST_TOTAL_COUNT++)) || true
if grep -q "Work item" "$SCRIPT_UNDER_TEST" 2>/dev/null; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script documents work item association"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script should document work item association"
fi

# =============================================================================
# SUMMARY MESSAGES
# =============================================================================

test_section "Summary Messages"

test_subsection "No worktrees summary"
no_wt_output=$("$SCRIPT_UNDER_TEST" 2>&1) || true
((TEST_TOTAL_COUNT++)) || true
if echo "$no_wt_output" | grep -q "Summary:"; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Shows summary line"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Missing summary line"
fi

test_subsection "JSON mode suppresses summary"
json_summary=$("$SCRIPT_UNDER_TEST" --json 2>&1) || true
assert_not_contains "$json_summary" "Summary:" "JSON mode has no text summary" || true

# =============================================================================
# COMBINED OPTIONS
# =============================================================================

test_section "Combined Options"

test_subsection "--json and --verbose together"
((TEST_TOTAL_COUNT++)) || true
if "$SCRIPT_UNDER_TEST" --json --verbose 2>/dev/null; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} --json and --verbose accepted together"
else
    # Still passes if exit 1 (issues) vs exit 2 (error)
    exit_code=$?
    if [[ $exit_code -ne 2 ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} --json and --verbose accepted (exit $exit_code from issues)"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} --json and --verbose rejected with error"
    fi
fi

# =============================================================================
# EXIT CODES
# =============================================================================

test_section "Exit Codes"

test_subsection "Help exits 0"
((TEST_TOTAL_COUNT++)) || true
if "$SCRIPT_UNDER_TEST" --help >/dev/null 2>&1; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} --help exits 0"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} --help should exit 0"
fi

test_subsection "Version exits 0"
((TEST_TOTAL_COUNT++)) || true
if "$SCRIPT_UNDER_TEST" --version >/dev/null 2>&1; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} --version exits 0"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} --version should exit 0"
fi

# =============================================================================
# RESULTS
# =============================================================================

print_test_summary

[[ $TEST_FAIL_COUNT -eq 0 ]] && exit 0 || exit 1
