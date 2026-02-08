#!/usr/bin/env bash
# Test: Git post-commit hook
# Location: .codeflow/testing/scripts/git-hooks/test-post-commit.sh
#
# Tests the post-commit hook functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/post-commit"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

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

echo "=== Testing Git Post-Commit Hook ==="
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
# Test 3: Commit info extraction
# ============================================================================
echo ""
echo "--- Commit info extraction ---"

check_pattern "git rev-parse HEAD" "Should extract commit hash"
check_pattern "git rev-parse --short HEAD" "Should extract short hash"
check_pattern "git log -1 --pretty=%s" "Should extract commit message"
check_pattern "git log -1 --pretty=%an" "Should extract commit author"
check_pattern "git branch --show-current" "Should extract current branch"

# ============================================================================
# Test 4: Logging functionality
# ============================================================================
echo ""
echo "--- Logging functionality ---"

check_pattern "LOG_DIR" "Should define log directory"
check_pattern "\.state/logs/git" "Should use correct log path"
check_pattern "mkdir -p" "Should create log directory"
check_pattern "\.jsonl" "Should use JSONL format"

# ============================================================================
# Test 5: JSON logging with jq
# ============================================================================
echo ""
echo "--- JSON logging ---"

check_pattern "jq -nc" "Should use jq for JSON creation"
check_pattern 'event: "commit"' "Should log commit event type"

# ============================================================================
# Test 6: User feedback
# ============================================================================
echo ""
echo "--- User feedback ---"

check_pattern "Commit created" "Should show commit confirmation"
check_pattern "COMMIT_SHORT" "Should display short hash"

# ============================================================================
# Test 7: Always succeeds (exit 0 only)
# ============================================================================
echo ""
echo "--- Exit behavior ---"

if grep -q "exit 0" "$HOOK" && ! grep -q "exit 1" "$HOOK"; then
    echo "PASS: Post-commit always succeeds (logging non-blocking)"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Post-commit should never block"
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
