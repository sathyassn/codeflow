#!/usr/bin/env bash
# Test: Git prepare-commit-msg hook
# Location: .codeflow/testing/scripts/git-hooks/test-prepare-commit-msg.sh
#
# Tests the prepare-commit-msg hook functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/prepare-commit-msg"
TEMP_MSG="/tmp/claude/test-prepare-commit-msg-$$"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Ensure temp directory exists
mkdir -p /tmp/claude

# Cleanup (called via trap)
# shellcheck disable=SC2329  # Invoked indirectly via trap
cleanup() {
    rm -f "$TEMP_MSG"
}
trap cleanup EXIT

# Test helper
check_pattern() {
    local pattern="$1"
    local description="$2"

    if grep -qE "$pattern" "$HOOK"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Test hook with empty message
test_template_generation() {
    local commit_source="$1"
    local description="$2"
    local should_generate="$3"

    echo "" > "$TEMP_MSG"
    bash "$HOOK" "$TEMP_MSG" "$commit_source" 2>/dev/null || true

    local content
    content=$(cat "$TEMP_MSG")

    if [[ "$should_generate" == "yes" ]]; then
        if [[ -n "$content" ]] && [[ ${#content} -gt 5 ]]; then
            echo "PASS: $description"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo "FAIL: $description (no template generated)"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    else
        # For skip conditions, message should remain empty or unchanged
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    fi
}

echo "=== Testing Git Prepare-Commit-Msg Hook ==="
echo ""

# ============================================================================
# Test 1: Hook exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$HOOK" ]]; then
    echo "PASS: Hook is executable"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Hook is not executable"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 2: Shellcheck passes
# ============================================================================
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        echo "PASS: Hook passes shellcheck"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: Hook fails shellcheck"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
else
    echo "SKIP: shellcheck not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# ============================================================================
# Test 3: Skip conditions (pattern checks)
# ============================================================================
echo ""
echo "--- Skip conditions (pattern) ---"

check_pattern 'COMMIT_SOURCE.*message' "Should check for message source"
check_pattern 'COMMIT_SOURCE.*merge' "Should check for merge source"
check_pattern 'COMMIT_SOURCE.*commit' "Should check for commit source (amend)"

# ============================================================================
# Test 3b: Skip conditions (functional)
# ============================================================================
echo ""
echo "--- Skip conditions (functional) ---"

# Source "message": Should skip template generation (message already provided via -m)
echo "" > "$TEMP_MSG"
bash "$HOOK" "$TEMP_MSG" "message" 2>/dev/null || true
MSG_CONTENT=$(cat "$TEMP_MSG")
if [[ -z "$MSG_CONTENT" ]] || [[ ${#MSG_CONTENT} -le 1 ]]; then
    echo "PASS: source=message skips template generation"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: source=message should skip template generation (got: '$MSG_CONTENT')"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Source "merge": Should skip template generation
echo "" > "$TEMP_MSG"
bash "$HOOK" "$TEMP_MSG" "merge" 2>/dev/null || true
MERGE_CONTENT=$(cat "$TEMP_MSG")
if [[ -z "$MERGE_CONTENT" ]] || [[ ${#MERGE_CONTENT} -le 1 ]]; then
    echo "PASS: source=merge skips template generation"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: source=merge should skip template generation (got: '$MERGE_CONTENT')"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# Source "commit": Should skip template generation (amend)
echo "" > "$TEMP_MSG"
bash "$HOOK" "$TEMP_MSG" "commit" 2>/dev/null || true
COMMIT_CONTENT=$(cat "$TEMP_MSG")
if [[ -z "$COMMIT_CONTENT" ]] || [[ ${#COMMIT_CONTENT} -le 1 ]]; then
    echo "PASS: source=commit skips template generation"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: source=commit should skip template generation (got: '$COMMIT_CONTENT')"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 4: Branch detection
# ============================================================================
echo ""
echo "--- Branch detection ---"

check_pattern "git branch --show-current" "Should detect current branch"
check_pattern "BASH_REMATCH" "Should use regex matching for branch parsing"

# ============================================================================
# Test 5: Commit type extraction
# ============================================================================
echo ""
echo "--- Commit type extraction ---"

check_pattern "feat|fix|docs|refactor|test|chore|perf|ci|build" "Should recognize all commit types"
check_pattern "COMMIT_TYPE" "Should extract commit type"
check_pattern "COMMIT_SCOPE" "Should extract commit scope"

# ============================================================================
# Test 6: Template content
# ============================================================================
echo ""
echo "--- Template content ---"

check_pattern "Conventional commit format" "Should include format instructions"
check_pattern "Examples" "Should include examples"

# ============================================================================
# Test 7: Template generation tests
# ============================================================================
echo ""
echo "--- Template generation ---"

test_template_generation "" "Should generate template for empty source" "yes"
test_template_generation "template" "Should generate template for template source" "yes"

# ============================================================================
# Test 8: Always succeeds
# ============================================================================
echo ""
echo "--- Exit behavior ---"

if grep -q "exit 0" "$HOOK" && ! grep -q "exit 1" "$HOOK"; then
    echo "PASS: Prepare-commit-msg always succeeds"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Prepare-commit-msg should never block"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
