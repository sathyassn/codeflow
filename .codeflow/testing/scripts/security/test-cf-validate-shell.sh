#!/usr/bin/env bash
# Test: Validation - cf-validate-shell.sh
# Location: .codeflow/testing/scripts/security/test-cf-validate-shell.sh
#
# Tests the shell script validation functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/cf-validate-shell.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup
cleanup() {
    rm -f /tmp/test-validate-*.sh 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Testing cf-validate-shell.sh ==="
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
# Test 4: Valid shell script
# ============================================================================
echo ""
echo "--- Validation functionality ---"

cat > /tmp/test-validate-valid.sh <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "Hello, World!"
EOF

# Test valid script
OUTPUT=$("$SCRIPT" /tmp/test-validate-valid.sh 2>&1 || true)
if echo "$OUTPUT" | grep -qi "PASS"; then
    echo "PASS: Valid script passes"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Valid script should pass (output: $OUTPUT)"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: Invalid shell script (syntax error)
# ============================================================================
cat > /tmp/test-validate-invalid.sh <<'EOF'
#!/usr/bin/env bash
if [[ true ]  # Missing closing bracket
echo "broken"
fi
EOF

if ! "$SCRIPT" /tmp/test-validate-invalid.sh 2>/dev/null; then
    echo "PASS: Invalid script fails"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Invalid script should fail"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Quiet mode
# ============================================================================
if "$SCRIPT" --quiet /tmp/test-validate-valid.sh 2>&1 | grep -qv "."; then
    echo "PASS: Quiet mode suppresses output"
    ((TESTS_PASSED++)) || true
else
    # If output is empty, it passed
    if [[ -z "$("$SCRIPT" --quiet /tmp/test-validate-valid.sh 2>&1)" ]]; then
        echo "PASS: Quiet mode suppresses output"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Quiet mode should suppress output"
        ((TESTS_FAILED++)) || true
    fi
fi

# ============================================================================
# Test 7: File not found
# ============================================================================
OUTPUT=$("$SCRIPT" /nonexistent/file.sh 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "PASS: Errors on nonexistent file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Shellcheck passes on script itself
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
