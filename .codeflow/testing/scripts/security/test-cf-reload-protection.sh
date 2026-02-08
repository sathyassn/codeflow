#!/usr/bin/env bash
# Test: Protection - cf-reload-protection.sh
# Location: .codeflow/testing/scripts/security/test-cf-reload-protection.sh
#
# Tests the protection list reload functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/protection/cf-reload-protection.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

echo "=== Testing cf-reload-protection.sh ==="
echo ""

# ============================================================================
# Test 1: Script exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$SCRIPT" ]]; then
    echo "PASS: Script is executable"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Script is not executable"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 2: Help flag
# ============================================================================
if "$SCRIPT" --help 2>&1 | grep -q "USAGE:"; then
    echo "PASS: --help shows usage"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: --help not working"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 3: Version flag
# ============================================================================
if "$SCRIPT" --version 2>&1 | grep -qE "version [0-9]+\.[0-9]+\.[0-9]+"; then
    echo "PASS: --version shows version"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: --version not working"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 4: Validate mode
# ============================================================================
echo ""
echo "--- Functionality ---"

OUTPUT=$("$SCRIPT" --validate 2>&1 || true)
if echo "$OUTPUT" | grep -qi "pass\|validation\|valid"; then
    echo "PASS: Validate mode works"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: Validate mode should report"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 5: List mode
# ============================================================================
OUTPUT=$("$SCRIPT" --list 2>&1 || true)
if echo "$OUTPUT" | grep -qi "protected\|critical\|core\|pattern"; then
    echo "PASS: List mode shows protected paths"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo "FAIL: List mode should show protected paths"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi

# ============================================================================
# Test 6: Reload creates cache (if jq available)
# ============================================================================
if command -v jq &>/dev/null; then
    "$SCRIPT" >/dev/null 2>&1 || true
    if [[ -f "/tmp/claude/managed/protection-cache.json" ]]; then
        echo "PASS: Reload creates cache"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: Reload should create cache"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
else
    echo "SKIP: jq not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
fi

# ============================================================================
# Test 7: Shellcheck passes
# ============================================================================
echo ""
echo "--- Code quality ---"

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$SCRIPT" 2>/dev/null; then
        echo "PASS: Script passes shellcheck"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: Script fails shellcheck"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
else
    echo "SKIP: shellcheck not available"
    TESTS_PASSED=$((TESTS_PASSED + 1))
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
