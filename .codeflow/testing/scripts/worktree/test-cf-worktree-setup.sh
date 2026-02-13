#!/usr/bin/env bash
# Purpose:   Test cf-worktree-setup.sh functionality
# Usage:     bash test-cf-worktree-setup.sh
# Platform:  macOS/Linux

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_UNDER_TEST="$REPO_ROOT/.codeflow/scripts/worktree/cf-worktree-setup.sh"

# Source test helpers
source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

# Create an isolated git repo for worktree testing
setup_worktree_test() {
    TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp/claude}/cf-wt-test-XXXXXX")
    TEST_REPO="$TEST_TMPDIR/repo"
    mkdir -p "$TEST_REPO"
    cd "$TEST_REPO"
    git init --quiet
    git config user.email "test@test.com"
    git config user.name "Test"
    echo "test" > file.txt
    git add file.txt
    git commit -m "Initial commit" --quiet
}

# Create a worktree in the test repo
create_test_worktree() {
    local wt_name="${1:-test-wt}"
    local branch="${2:-feat/test}"
    local wt_path="$TEST_TMPDIR/$wt_name"
    cd "$TEST_REPO"
    git worktree add "$wt_path" -b "$branch" --quiet 2>/dev/null
    echo "$wt_path"
}

cleanup_worktree_test() {
    cd "$REPO_ROOT"
    [[ -n "${TEST_TMPDIR:-}" ]] && rm -rf "$TEST_TMPDIR" 2>/dev/null || true
}

# =============================================================================
# TESTS: BASIC INTERFACE
# =============================================================================

test_section "Basic Interface"

test_subsection "Script existence and permissions"

((TEST_TOTAL_COUNT++)) || true
if [[ -x "$SCRIPT_UNDER_TEST" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script exists and is executable"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script not found or not executable"
fi

assert_success "bash -n '$SCRIPT_UNDER_TEST'" "Script has valid bash syntax"

test_subsection "Help and version"

HELP_OUTPUT=$("$SCRIPT_UNDER_TEST" --help 2>&1)
assert_contains "$HELP_OUTPUT" "cf-worktree-setup.sh" "--help shows script name"
assert_contains "$HELP_OUTPUT" "USAGE" "--help shows usage section"
assert_contains "$HELP_OUTPUT" "worktree-path" "--help documents worktree-path argument"
assert_contains "$HELP_OUTPUT" "branch-name" "--help documents branch-name argument"
assert_contains "$HELP_OUTPUT" "--version" "--help documents --version option"

VERSION_OUTPUT=$("$SCRIPT_UNDER_TEST" --version 2>&1)
assert_contains "$VERSION_OUTPUT" "version" "--version shows version"
assert_contains "$VERSION_OUTPUT" "cf-worktree-setup.sh" "--version shows script name"

test_subsection "Argument validation"

assert_fails "'$SCRIPT_UNDER_TEST' 2>/dev/null" "Fails with no arguments (exit 1)"

assert_fails "'$SCRIPT_UNDER_TEST' /tmp/claude/only-path 2>/dev/null" "Fails with only one argument"

# Missing both args should show error on stderr
MISSING_ARGS_ERR=$("$SCRIPT_UNDER_TEST" 2>&1 || true)
assert_contains "$MISSING_ARGS_ERR" "Missing required arguments" "Error message mentions missing arguments"
assert_contains "$MISSING_ARGS_ERR" "--help" "Error message references --help"

# =============================================================================
# TESTS: PATH VALIDATION
# =============================================================================

test_section "Path Validation"

INVALID_PATH_ERR=$(REPO_ROOT="/tmp/claude" "$SCRIPT_UNDER_TEST" /tmp/claude/nonexistent-path feat/test 2>&1 || true)
assert_contains "$INVALID_PATH_ERR" "does not exist" "Rejects nonexistent worktree path"
assert_contains "$INVALID_PATH_ERR" "git worktree add" "Error guides user to create worktree first"

# =============================================================================
# TESTS: FUNCTIONAL - WORKTREE SETUP
# =============================================================================

test_section "Functional: Worktree Setup"

test_subsection "Basic worktree setup"

setup_worktree_test
trap cleanup_worktree_test EXIT

WT_PATH=$(create_test_worktree "wt-basic" "feat/basic-test")

OUTPUT=$(REPO_ROOT="$TEST_REPO" "$SCRIPT_UNDER_TEST" "$WT_PATH" "feat/basic-test" developer "Basic test" manual 2>&1)
SETUP_EXIT=$?

assert_equals "0" "$SETUP_EXIT" "Setup exits with 0 on success"
assert_contains "$OUTPUT" "Setting up worktree" "Output shows setup start"
assert_contains "$OUTPUT" "feat/basic-test" "Output shows branch name"
assert_contains "$OUTPUT" "developer" "Output shows agent type"
assert_contains "$OUTPUT" "Basic test" "Output shows purpose"
assert_contains "$OUTPUT" "Worktree setup complete" "Output shows completion message"

test_subsection "State symlink creation"

((TEST_TOTAL_COUNT++)) || true
if [[ -L "$WT_PATH/.state" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} .state symlink created in worktree"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} .state symlink NOT created in worktree"
fi

LINK_TARGET=$(readlink "$WT_PATH/.state" 2>/dev/null || echo "")
assert_contains "$LINK_TARGET" ".state" "Symlink points to main repo .state"

test_subsection "Worktrees.yaml registration"

WORKTREES_FILE="$TEST_REPO/.state/worktrees.yaml"
assert_file_exists "$WORKTREES_FILE" "worktrees.yaml created"
assert_file_contains "$WORKTREES_FILE" "path:" "YAML contains path field"
assert_file_contains "$WORKTREES_FILE" "feat/basic-test" "YAML contains branch name"
assert_file_contains "$WORKTREES_FILE" "developer" "YAML contains agent type"
assert_file_contains "$WORKTREES_FILE" "manual" "YAML contains trigger"
assert_file_contains "$WORKTREES_FILE" "Basic test" "YAML contains purpose"
assert_file_contains "$WORKTREES_FILE" "status: active" "YAML shows active status"
assert_file_contains "$WORKTREES_FILE" "last_updated:" "YAML has last_updated metadata"

# Verify YAML structure: worktrees: should NOT have [] when entries exist
((TEST_TOTAL_COUNT++)) || true
if ! grep -q "^worktrees: \[\]" "$WORKTREES_FILE" 2>/dev/null; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} YAML uses 'worktrees:' without [] (valid YAML list)"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} YAML still has 'worktrees: []' with entries below (invalid)"
fi

# =============================================================================
# TESTS: RE-REGISTRATION
# =============================================================================

test_section "Re-registration"

test_subsection "Running setup on already-registered worktree"

cd "$TEST_REPO"
RERUN_OUTPUT=$(REPO_ROOT="$TEST_REPO" "$SCRIPT_UNDER_TEST" "$WT_PATH" "feat/basic-test" developer "Basic test" manual 2>&1)
assert_contains "$RERUN_OUTPUT" "already registered" "Detects already-registered worktree"
assert_contains "$RERUN_OUTPUT" "Worktree setup complete" "Completes successfully on re-run"

# Verify only one entry exists (no duplicates)
cd "$TEST_REPO"
ENTRY_COUNT=$(grep -c "path:.*wt-basic" "$WORKTREES_FILE" 2>/dev/null || echo "0")
assert_equals "1" "$ENTRY_COUNT" "No duplicate entries after re-registration"

# =============================================================================
# TESTS: MULTIPLE WORKTREES
# =============================================================================

test_section "Multiple Worktrees"

WT_PATH2=$(create_test_worktree "wt-second" "feat/second-test")

cd "$TEST_REPO"
REPO_ROOT="$TEST_REPO" "$SCRIPT_UNDER_TEST" "$WT_PATH2" "feat/second-test" planner "Second worktree" manual >/dev/null 2>&1

SECOND_COUNT=$(grep -c "path:" "$WORKTREES_FILE" 2>/dev/null || echo "0")
assert_equals "2" "$SECOND_COUNT" "Two worktree entries after second setup"
assert_file_contains "$WORKTREES_FILE" "feat/second-test" "Second worktree branch registered"
assert_file_contains "$WORKTREES_FILE" "planner" "Second worktree agent type registered"

# =============================================================================
# TESTS: DEFAULT VALUES
# =============================================================================

test_section "Default Values"

WT_PATH3=$(create_test_worktree "wt-defaults" "feat/defaults-test")

cd "$TEST_REPO"
REPO_ROOT="$TEST_REPO" "$SCRIPT_UNDER_TEST" "$WT_PATH3" "feat/defaults-test" >/dev/null 2>&1

assert_file_contains "$WORKTREES_FILE" "unknown" "Default agent type is 'unknown'"
assert_file_contains "$WORKTREES_FILE" "manual" "Default trigger is 'manual'"

# =============================================================================
# TESTS: SYMLINK ALREADY EXISTS
# =============================================================================

test_section "Symlink Idempotency"

WT_PATH4=$(create_test_worktree "wt-symlink" "feat/symlink-test")

# Pre-create .state symlink
ln -s "$TEST_REPO/.state" "$WT_PATH4/.state" 2>/dev/null || true

cd "$TEST_REPO"
SYMLINK_OUTPUT=$(REPO_ROOT="$TEST_REPO" "$SCRIPT_UNDER_TEST" "$WT_PATH4" "feat/symlink-test" 2>&1)
assert_contains "$SYMLINK_OUTPUT" "symlink already exists" "Detects existing .state symlink"

# =============================================================================
# SUMMARY
# =============================================================================

print_test_summary

[[ "$TEST_FAIL_COUNT" -eq 0 ]] && exit 0 || exit 1
