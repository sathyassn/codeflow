#!/usr/bin/env bash
# Test: Validation - cf-validate-json.sh
# Location: .codeflow/testing/scripts/security/test-cf-validate-json.sh
#
# Tests the JSON file validation functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../lib/test-isolation.sh"
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/validation/cf-validate-json.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

echo "=== Testing cf-validate-json.sh ==="
echo ""

# ============================================================================
# Test 1: Script exists and is executable
# ============================================================================
echo "--- Basic checks ---"

if [[ -x "$SCRIPT" ]]; then
    echo "PASS: Script is executable"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Script is not executable"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 2: Help flag
# ============================================================================
if "$SCRIPT" --help 2>&1 | grep -q "USAGE:"; then
    echo "PASS: --help shows usage"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --help not working"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 3: Version flag
# ============================================================================
if "$SCRIPT" --version 2>&1 | grep -qE "version [0-9]+\.[0-9]+\.[0-9]+"; then
    echo "PASS: --version shows version"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: --version not working"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 4: Valid JSON
# ============================================================================
echo ""
echo "--- Validation functionality ---"

echo '{"name": "test", "value": 42}' > "$TEST_TMPDIR/test-validate-valid.json"

if "$SCRIPT" "$TEST_TMPDIR/test-validate-valid.json" 2>&1 | grep -q "PASS"; then
    echo "PASS: Valid JSON passes"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Valid JSON should pass"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: Invalid JSON (syntax error)
# ============================================================================
echo '{"name": "test", "value": }' > "$TEST_TMPDIR/test-validate-invalid.json"

if ! "$SCRIPT" "$TEST_TMPDIR/test-validate-invalid.json" 2>/dev/null; then
    echo "PASS: Invalid JSON fails"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Invalid JSON should fail"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Empty JSON object (valid)
# ============================================================================
echo '{}' > "$TEST_TMPDIR/test-validate-empty.json"

if "$SCRIPT" "$TEST_TMPDIR/test-validate-empty.json" 2>&1 | grep -q "PASS"; then
    echo "PASS: Empty object is valid"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Empty object should be valid"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: JSON array (valid)
# ============================================================================
echo '[1, 2, 3]' > "$TEST_TMPDIR/test-validate-array.json"

if "$SCRIPT" "$TEST_TMPDIR/test-validate-array.json" 2>&1 | grep -q "PASS"; then
    echo "PASS: JSON array is valid"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: JSON array should be valid"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Pretty print option
# ============================================================================
if "$SCRIPT" --pretty "$TEST_TMPDIR/test-validate-valid.json" 2>&1 | grep -q "name"; then
    echo "PASS: Pretty print works"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Pretty print should show content"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: File not found
# ============================================================================
OUTPUT=$("$SCRIPT" /nonexistent/file.json 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "PASS: Errors on nonexistent file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 10: Unknown option handling
# ============================================================================
echo ""
echo "--- Edge cases ---"

OUTPUT=$("$SCRIPT" --invalid-flag 2>&1 || true)
if echo "$OUTPUT" | grep -qi "unknown option\|error"; then
    echo "PASS: Unknown option shows error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Unknown option should show error"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 11: Missing arguments (no args)
# ============================================================================
OUTPUT=$("$SCRIPT" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "missing\|usage\|error"; then
    echo "PASS: Missing arguments shows error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Missing arguments should show error"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 12: Quiet mode suppresses output on valid file
# ============================================================================
OUTPUT=$("$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.json" 2>&1)
if [[ -z "$OUTPUT" ]]; then
    echo "PASS: Quiet mode suppresses output on valid file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Quiet mode should suppress output (got: $OUTPUT)"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 13: Exit code 0 on valid JSON
# ============================================================================
if "$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.json"; then
    echo "PASS: Exit code 0 on valid JSON"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Exit code should be 0 on valid JSON"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 14: Exit code 1 on invalid JSON
# ============================================================================
if "$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-invalid.json" 2>/dev/null; then
    echo "FAIL: Exit code should be 1 on invalid JSON"
    ((TESTS_FAILED++)) || true
else
    echo "PASS: Exit code 1 on invalid JSON"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 15: Shellcheck passes
# ============================================================================
echo ""
echo "--- Code quality ---"

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$SCRIPT" 2>/dev/null; then
        echo "PASS: Script passes shellcheck"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Script fails shellcheck"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: shellcheck not available"
    ((TESTS_PASSED++)) || true
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
