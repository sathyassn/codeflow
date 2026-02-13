#!/usr/bin/env bash
# Test: cf-branch-file-protection.sh
# Location: .codeflow/testing/scripts/security/test-cf-branch-file-protection.sh
#
# Tests the branch-aware file protection module.
# Uses test-isolation.sh to create an isolated git repo on 'main' branch,
# ensuring blocking tests always exercise the protected-branch path.

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../lib/test-isolation.sh"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$REAL_REPO_ROOT/.codeflow/scripts/security/enforcement/cf-branch-file-protection.sh"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

export REPO_ROOT LIB_DIR CONFIG

# Ensure isolated repo is on 'main' branch
git -C "$REPO_ROOT" checkout -b main 2>/dev/null || git -C "$REPO_ROOT" checkout main 2>/dev/null || true

# Verify branch
ISO_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
if [[ "$ISO_BRANCH" != "main" ]]; then
    echo "WARN: Isolated repo is on '$ISO_BRANCH', expected 'main'"
fi

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Helper: test that a command is BLOCKED on the isolated repo (main branch)
test_blocks_command() {
    local command="$1"
    local description="$2"
    local output

    output=$(cd "$REPO_ROOT" && COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" CONFIG="$CONFIG" \
       bash -c "source '$MODULE'" 2>&1 || true)

    if echo "$output" | grep -q "BLOCKED"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected block, got: $output"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Helper: test that a command is ALLOWED on the isolated repo (main branch)
test_allows_command() {
    local command="$1"
    local description="$2"

    if cd "$REPO_ROOT" && COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" CONFIG="$CONFIG" \
       bash -c "source '$MODULE'" 2>/dev/null; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected allow"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Helper: test block message contains expected text
test_blocks_with_message() {
    local command="$1"
    local expected="$2"
    local description="$3"
    local output

    output=$(cd "$REPO_ROOT" && COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" CONFIG="$CONFIG" \
       bash -c "source '$MODULE'" 2>&1 || true)

    if echo "$output" | grep -q "BLOCKED" && echo "$output" | grep -q "$expected"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected block with '$expected', got: $output"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing cf-branch-file-protection.sh ==="
echo "Isolated repo branch: $ISO_BRANCH"
echo ""

# =========================================================================
# SECTION: Redirect blocking (>, >>)
# =========================================================================
echo "--- Redirect blocking ---"

test_blocks_command "echo test > src/file.txt" \
    "Block redirect on main"

test_blocks_command "echo test >> src/file.txt" \
    "Block append redirect on main"

test_blocks_command "printf '%s' data > output.txt" \
    "Block printf redirect on main"

# FD redirects should NOT be blocked
test_allows_command "ls 2>&1" \
    "Allow fd redirect 2>&1"

test_allows_command "some_cmd 2>/dev/null" \
    "Allow fd redirect 2>/dev/null"

# =========================================================================
# SECTION: sed -i blocking
# =========================================================================
echo "--- sed -i blocking ---"

test_blocks_command "sed -i '' 's/a/b/' file.txt" \
    "Block sed -i (macOS format)"

test_blocks_command "sed -i.bak 's/foo/bar/' config.json" \
    "Block sed -i.bak"

# Regular sed (not in-place) should be allowed
test_allows_command "sed 's/a/b/' file.txt" \
    "Allow sed without -i"

# =========================================================================
# SECTION: touch blocking
# =========================================================================
echo "--- touch blocking ---"

test_blocks_command "touch src/newfile.txt" \
    "Block touch on relative path"

test_blocks_command "touch myfile.txt" \
    "Block touch on local path"

test_allows_command "touch /tmp/testfile" \
    "Allow touch in /tmp"

test_allows_command "touch /tmp/claude/something" \
    "Allow touch in /tmp/claude"

# =========================================================================
# SECTION: tee blocking
# =========================================================================
echo "--- tee blocking ---"

test_blocks_command "echo test | tee src/file.txt" \
    "Block piped tee to relative path"

test_allows_command "echo test | tee /tmp/output.txt" \
    "Allow piped tee to /tmp"

# =========================================================================
# SECTION: cp blocking
# =========================================================================
echo "--- cp blocking ---"

# Note: "cp /tmp/a src/b" is allowed because the /tmp safe-path early-exit
# triggers on any command containing /tmp/ (matching workflow behavior).
# This is a known trade-off to avoid false positives on /tmp operations.
test_allows_command "cp /tmp/a src/b" \
    "Allow cp from /tmp (safe-path early exit)"

test_blocks_command "cp file1.txt file2.txt" \
    "Block cp between local files"

test_blocks_command "cp -r src/dir dest/dir" \
    "Block cp -r to relative destination"

test_allows_command "cp src/a /tmp/b" \
    "Allow cp to absolute /tmp destination"

# =========================================================================
# SECTION: Safe path exceptions (/tmp/claude, /tmp)
# =========================================================================
echo "--- Safe path exceptions ---"

test_allows_command "echo test > /tmp/claude/file.txt" \
    "Allow redirect to /tmp/claude/"

test_allows_command "cat data >> /tmp/claude/log.txt" \
    "Allow append to /tmp/claude/"

test_allows_command "echo data > /tmp/testfile" \
    "Allow redirect to /tmp/"

# =========================================================================
# SECTION: Read-only commands (should always be allowed)
# =========================================================================
echo "--- Read-only commands ---"

test_allows_command "cat src/file.txt" \
    "Allow cat"

test_allows_command "ls -la src/" \
    "Allow ls"

test_allows_command "grep -r 'pattern' src/" \
    "Allow grep"

test_allows_command "git status" \
    "Allow git status"

test_allows_command "git diff" \
    "Allow git diff"

# =========================================================================
# SECTION: Protected path skip logic
# =========================================================================
echo "--- Protected path skip logic ---"

# When PROTECTED_PATHS is set and command targets one of those paths,
# branch-file-protection should defer to path-protection.
# Note: arrays can't be exported across bash -c subshells, so we declare
# PROTECTED_PATHS inside the subshell.

_test_protected_path_skip() {
    local command="$1"
    local description="$2"

    if cd "$REPO_ROOT" && COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" CONFIG="$CONFIG" \
       bash -c 'PROTECTED_PATHS=(".claude/CLAUDE.md" ".codeflow/config"); source "'"$MODULE"'"' 2>/dev/null; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected allow"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

_test_protected_path_skip "echo x > .claude/CLAUDE.md" \
    "Defer to path-protection for PROTECTED_PATHS target"

_test_protected_path_skip "sed -i '' 's/a/b/' .codeflow/config/test.json" \
    "Defer to path-protection for .codeflow/config"

# =========================================================================
# SECTION: Feature branch bypass
# =========================================================================
echo "--- Feature branch bypass ---"

# Switch isolated repo to a feature branch
git -C "$REPO_ROOT" checkout -b feat/test-branch 2>/dev/null

test_allows_command "echo test > src/file.txt" \
    "Allow redirect on feature branch"

test_allows_command "sed -i '' 's/a/b/' file.txt" \
    "Allow sed -i on feature branch"

test_allows_command "touch newfile.txt" \
    "Allow touch on feature branch"

test_allows_command "cp file1.txt file2.txt" \
    "Allow cp on feature branch"

# Switch back to main for remaining tests
git -C "$REPO_ROOT" checkout main 2>/dev/null

# =========================================================================
# SECTION: Block message content
# =========================================================================
echo "--- Block message content ---"

test_blocks_with_message "echo test > src/file.txt" \
    "create-branch" \
    "Block message references create-branch operation"

test_blocks_with_message "echo test > src/file.txt" \
    "git-workflow" \
    "Block message references git-workflow skill"

test_blocks_with_message "echo test > src/file.txt" \
    "Branch Protection" \
    "Block message has Branch Protection category"

# =========================================================================
# SECTION: PathFlow-conditional messages
# =========================================================================
echo "--- PathFlow-conditional messages ---"

# In standalone mode (no pathflow flag), message says "Create a feature branch first"
test_blocks_with_message "echo test > src/file.txt" \
    "Create a feature branch first" \
    "Standalone mode shows 'Create a feature branch first'"

# Create pathflow-active flag to simulate agent-teams mode
mkdir -p "$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-unknown}"
echo "test" > "$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-unknown}/is-pathflow-active"

test_blocks_with_message "echo test > src/file.txt" \
    "cf-gitops teammate" \
    "PathFlow mode shows cf-gitops teammate instruction"

# Clean up flag
rm -f "$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-unknown}/is-pathflow-active"

# =========================================================================
# SECTION: Compound / edge-case commands
# =========================================================================
echo "--- Compound and edge-case commands ---"

test_blocks_command "mkdir -p dir && echo test > dir/file.txt" \
    "Block redirect in compound command"

test_blocks_command "cat input.txt | sed -i '' 's/a/b/' target.txt" \
    "Block piped sed -i"

test_allows_command "echo 'hello world'" \
    "Allow echo without redirect"

test_allows_command "python3 -c 'print(1)'" \
    "Allow python execution"

# =========================================================================
# SUMMARY
# =========================================================================
echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
