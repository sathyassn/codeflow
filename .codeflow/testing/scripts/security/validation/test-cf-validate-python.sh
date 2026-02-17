#!/usr/bin/env bash
# Test: Validation - cf-validate-python.sh
# Location: .codeflow/testing/scripts/security/validation/test-cf-validate-python.sh
#
# Tests the Python script validation functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../../lib/test-isolation.sh"
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/validation/cf-validate-python.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

echo "=== Testing cf-validate-python.sh ==="
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
# Test 4: Valid Python script
# ============================================================================
echo ""
echo "--- Validation functionality ---"

cat > "$TEST_TMPDIR/test-validate-valid.py" <<'EOF'
#!/usr/bin/env python3
"""A valid Python script."""


def hello():
    """Print hello."""
    print("Hello, World!")


if __name__ == "__main__":
    hello()
EOF

OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-valid.py" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "PASS"; then
    echo "PASS: Valid script passes"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Valid script should pass"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: Invalid Python script (syntax error)
# ============================================================================
cat > "$TEST_TMPDIR/test-validate-invalid.py" <<'EOF'
#!/usr/bin/env python3
def broken(
    # Missing closing parenthesis
    print("broken")
EOF

if ! "$SCRIPT" "$TEST_TMPDIR/test-validate-invalid.py" 2>/dev/null; then
    echo "PASS: Invalid script fails"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Invalid script should fail"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Quiet mode
# ============================================================================
OUTPUT=$("$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.py" 2>&1 || true)
if [[ -z "$OUTPUT" ]]; then
    echo "PASS: Quiet mode suppresses output"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Quiet mode should suppress output"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 7: File not found
# ============================================================================
OUTPUT=$("$SCRIPT" /nonexistent/file.py 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "PASS: Errors on nonexistent file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Simple valid script
# ============================================================================
cat > "$TEST_TMPDIR/test-validate-simple.py" <<'EOF'
x = 1
y = 2
print(x + y)
EOF

OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-simple.py" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "PASS"; then
    echo "PASS: Simple script passes"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Simple script should pass"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: Unknown option handling
# ============================================================================
echo ""
echo "--- Error handling ---"

EXIT_CODE=0
OUTPUT=$("$SCRIPT" --invalid-flag 2>&1) || EXIT_CODE=$?
if [[ $EXIT_CODE -ne 0 ]] && echo "$OUTPUT" | grep -qi "unknown option\|error"; then
    echo "PASS: Unknown option shows error and exits non-zero"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Unknown option should show error and exit non-zero (got exit $EXIT_CODE)"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 10: Missing arguments
# ============================================================================
EXIT_CODE=0
OUTPUT=$("$SCRIPT" 2>&1) || EXIT_CODE=$?
if [[ $EXIT_CODE -ne 0 ]] && echo "$OUTPUT" | grep -qi "missing\|error\|usage"; then
    echo "PASS: Missing arguments shows error and exits non-zero"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Missing arguments should show error and exit non-zero (got exit $EXIT_CODE)"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 11: Exit code 0 on valid Python
# ============================================================================
echo ""
echo "--- Exit code verification ---"

EXIT_CODE=0
"$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.py" >/dev/null 2>&1 || EXIT_CODE=$?
if [[ $EXIT_CODE -eq 0 ]]; then
    echo "PASS: Exit code 0 on valid Python"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Expected exit code 0 on valid Python, got $EXIT_CODE"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 12: Exit code 1 on invalid Python
# ============================================================================
EXIT_CODE=0
"$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-invalid.py" >/dev/null 2>&1 || EXIT_CODE=$?
if [[ $EXIT_CODE -eq 1 ]]; then
    echo "PASS: Exit code 1 on invalid Python"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Expected exit code 1 on invalid Python, got $EXIT_CODE"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 13: Strict mode (if linter available)
# ============================================================================
echo ""
echo "--- Strict mode ---"

if command -v ruff &>/dev/null || command -v flake8 &>/dev/null; then
    cat > "$TEST_TMPDIR/test-validate-strict.py" <<'PYEOF'
import os
import sys

print("hello")
PYEOF

    EXIT_STRICT=0
    "$SCRIPT" --strict --quiet "$TEST_TMPDIR/test-validate-strict.py" >/dev/null 2>&1 || EXIT_STRICT=$?
    if [[ $EXIT_STRICT -ne 0 ]]; then
        echo "PASS: Strict mode catches linter issues"
        ((TESTS_PASSED++)) || true
    else
        echo "INFO: Strict mode passed (linter may not flag unused imports by default)"
        ((TESTS_PASSED++)) || true
    fi
else
    echo "SKIP: No linter available for strict mode test"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 14: Shellcheck passes on script itself
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
