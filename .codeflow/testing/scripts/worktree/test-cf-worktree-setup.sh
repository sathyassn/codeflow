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

test_subsection "State directory setup (selective symlink)"

# With selective symlinks, .state is a directory (not a full symlink)
# Shared subdirs (db, ledger, etc.) are symlinked; local subdirs (runtime, session, sentinels) are real dirs
((TEST_TOTAL_COUNT++)) || true
if [[ -d "$WT_PATH/.state" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} .state directory exists in worktree"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} .state directory NOT created in worktree"
fi

# .state should be a real directory (not a symlink) under selective symlink mode
((TEST_TOTAL_COUNT++)) || true
if [[ -d "$WT_PATH/.state" ]] && [[ ! -L "$WT_PATH/.state" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} .state is a real directory (selective symlink mode)"
else
    # Legacy behavior: .state is a full symlink — acceptable for older worktree setups
    if [[ -L "$WT_PATH/.state" ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} .state is a full symlink (legacy mode)"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} .state should be a directory or symlink"
    fi
fi

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

# Pre-create .state symlink (old full symlink behavior)
ln -s "$TEST_REPO/.state" "$WT_PATH4/.state" 2>/dev/null || true

cd "$TEST_REPO"
SYMLINK_OUTPUT=$(REPO_ROOT="$TEST_REPO" "$SCRIPT_UNDER_TEST" "$WT_PATH4" "feat/symlink-test" 2>&1)
# Script should either detect existing symlink or migrate from old full symlink
((TEST_TOTAL_COUNT++)) || true
if echo "$SYMLINK_OUTPUT" | grep -qi 'symlink already exists\|Migrating.*old.*symlink\|removing.*full.*symlink\|Symlinked:'; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Handles pre-existing .state symlink (detects or migrates)"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Should detect or migrate pre-existing .state symlink"
fi

# =============================================================================
# TESTS: SELECTIVE SYMLINK SETUP
# =============================================================================

# Helper for selective symlink tests - extends basic setup with .state directories
setup_symlink_worktree_test() {
    TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp/claude}/cf-wt-symlink-test-XXXXXX")
    TEST_REPO="$TEST_TMPDIR/repo"
    mkdir -p "$TEST_REPO"
    cd "$TEST_REPO"
    git init --quiet
    git config user.email "test@test.com"
    git config user.name "Test"
    echo "test" > file.txt
    git add file.txt
    git commit -m "Initial commit" --quiet

    # Create .state directories in main repo
    mkdir -p "$TEST_REPO/.state/db"
    mkdir -p "$TEST_REPO/.state/ledger"
    mkdir -p "$TEST_REPO/.state/registry"
    mkdir -p "$TEST_REPO/.state/backups"
    mkdir -p "$TEST_REPO/.state/coordination"
    mkdir -p "$TEST_REPO/.state/logs"
    mkdir -p "$TEST_REPO/.state/runtime"
    mkdir -p "$TEST_REPO/.state/session"
    mkdir -p "$TEST_REPO/.state/sentinels"
}

test_section "Selective Symlink Setup"

test_subsection "Shared directories are symlinked"

# Test: Run worktree setup and check shared dirs
setup_symlink_worktree_test
WT_PATH=$(create_test_worktree "wt-symlink-test" "feat/symlink-test")
REPO_ROOT="$TEST_REPO" bash "$SCRIPT_UNDER_TEST" "$WT_PATH" "feat/symlink-test" "developer" "Test symlinks" >/dev/null 2>&1 || true

# Shared directories should be symlinks
for shared_dir in db ledger registry backups coordination logs; do
    ((TEST_TOTAL_COUNT++)) || true
    if [[ -L "$WT_PATH/.state/$shared_dir" ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} .state/$shared_dir is a symlink"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} .state/$shared_dir should be a symlink"
    fi
done

test_subsection "Shared directories point to main repo"

for shared_dir in db ledger registry backups coordination logs; do
    ((TEST_TOTAL_COUNT++)) || true
    _target=$(readlink "$WT_PATH/.state/$shared_dir" 2>/dev/null) || true
    if [[ "$_target" == "$TEST_REPO/.state/$shared_dir" ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} .state/$shared_dir -> $TEST_REPO/.state/$shared_dir"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} .state/$shared_dir target should be $TEST_REPO/.state/$shared_dir, got $_target"
    fi
done

test_subsection "Local directories are real directories (not symlinks)"

for local_dir in runtime session sentinels; do
    ((TEST_TOTAL_COUNT++)) || true
    if [[ -d "$WT_PATH/.state/$local_dir" ]] && [[ ! -L "$WT_PATH/.state/$local_dir" ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} .state/$local_dir is a real directory"
    else
        ((TEST_FAIL_COUNT++)) || true
        if [[ -L "$WT_PATH/.state/$local_dir" ]]; then
            echo -e "  ${RED}✗${NC} .state/$local_dir should NOT be a symlink"
        else
            echo -e "  ${RED}✗${NC} .state/$local_dir should exist as a directory"
        fi
    fi
done

test_subsection ".state itself is a directory, not a symlink"

((TEST_TOTAL_COUNT++)) || true
if [[ -d "$WT_PATH/.state" ]] && [[ ! -L "$WT_PATH/.state" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} .state is a real directory (not a full symlink)"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} .state should be a real directory, not a symlink"
fi

cleanup_worktree_test

# =============================================================================
# TESTS: MIGRATION FROM FULL SYMLINK
# =============================================================================

test_section "Migration from Full Symlink"

test_subsection "Old full .state symlink is replaced"

setup_symlink_worktree_test
WT_PATH=$(create_test_worktree "wt-migrate-test" "feat/migrate-test")

# Pre-create a full .state symlink (old behavior)
ln -s "$TEST_REPO/.state" "$WT_PATH/.state"

# Verify it's a symlink before migration
((TEST_TOTAL_COUNT++)) || true
if [[ -L "$WT_PATH/.state" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Pre-condition: .state is a full symlink"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Pre-condition failed: .state should be a symlink"
fi

# Run setup — should migrate
REPO_ROOT="$TEST_REPO" bash "$SCRIPT_UNDER_TEST" "$WT_PATH" "feat/migrate-test" "developer" "Test migration" >/dev/null 2>&1 || true

# After migration, .state should be a directory
((TEST_TOTAL_COUNT++)) || true
if [[ -d "$WT_PATH/.state" ]] && [[ ! -L "$WT_PATH/.state" ]]; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} After migration: .state is a real directory"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} After migration: .state should be a real directory"
fi

# After migration, shared dirs should be symlinks
for shared_dir in db ledger registry backups coordination logs; do
    ((TEST_TOTAL_COUNT++)) || true
    if [[ -L "$WT_PATH/.state/$shared_dir" ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} After migration: .state/$shared_dir is a symlink"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} After migration: .state/$shared_dir should be a symlink"
    fi
done

# After migration, local dirs should be real
for local_dir in runtime session sentinels; do
    ((TEST_TOTAL_COUNT++)) || true
    if [[ -d "$WT_PATH/.state/$local_dir" ]] && [[ ! -L "$WT_PATH/.state/$local_dir" ]]; then
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} After migration: .state/$local_dir is a real directory"
    else
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} After migration: .state/$local_dir should be a real directory"
    fi
done

cleanup_worktree_test

# =============================================================================
# TESTS: SELECTIVE SYMLINK STATIC ANALYSIS
# =============================================================================

test_section "Selective Symlink Static Analysis"

test_subsection "Script has selective symlink logic"

((TEST_TOTAL_COUNT++)) || true
if grep -q 'Symlinked:.*\.state/' "$SCRIPT_UNDER_TEST"; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script has selective symlink output messages"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script should have selective symlink output"
fi

((TEST_TOTAL_COUNT++)) || true
if grep -q 'Created local:.*\.state/' "$SCRIPT_UNDER_TEST"; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script has local directory creation messages"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script should have local directory creation messages"
fi

((TEST_TOTAL_COUNT++)) || true
if grep -q 'runtime.*session.*sentinels' "$SCRIPT_UNDER_TEST" || \
   (grep -q 'runtime' "$SCRIPT_UNDER_TEST" && grep -q 'session' "$SCRIPT_UNDER_TEST" && grep -q 'sentinels' "$SCRIPT_UNDER_TEST"); then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script creates runtime, session, sentinels as local"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script should create runtime, session, sentinels locally"
fi

((TEST_TOTAL_COUNT++)) || true
if grep -q 'Migrating.*old.*symlink\|removing.*full.*symlink' "$SCRIPT_UNDER_TEST"; then
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Script handles migration from old full symlink"
else
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Script should handle migration from old full .state symlink"
fi

# =============================================================================
# SUMMARY
# =============================================================================

print_test_summary

[[ "$TEST_FAIL_COUNT" -eq 0 ]] && exit 0 || exit 1
