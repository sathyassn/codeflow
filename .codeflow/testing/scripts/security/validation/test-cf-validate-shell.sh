#!/usr/bin/env bash
# Test: Validation - cf-validate-shell.sh
# Location: .codeflow/testing/scripts/security/validation/test-cf-validate-shell.sh
#
# Tests the shell script validation functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# REAL_REPO_ROOT: the real repo root (git toplevel), distinct from any
# isolated sandbox a test may create. Legacy lib/test-isolation.sh retired
# in INF-TSK-046-008; derive both REAL_REPO_ROOT and TEST_TMPDIR directly.
REAL_REPO_ROOT="$(git -C "$TEST_DIR" rev-parse --show-toplevel 2>/dev/null || echo "")"
# Honour TMPDIR if set (sandboxed environments restrict mktemp(1) paths).
TEST_TMPDIR="$(mktemp -d "${TMPDIR:-/tmp}/cf-validate-shell-test.XXXXXX")"
trap 'rm -rf "$TEST_TMPDIR"' EXIT
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/validation/cf-validate-shell.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

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

cat > "$TEST_TMPDIR/test-validate-valid.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "Hello, World!"
EOF

# Test valid script
OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-valid.sh" 2>&1 || true)
if grep -qi "PASS" <<< "$OUTPUT"; then
    echo "PASS: Valid script passes"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Valid script should pass (output: $OUTPUT)"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: Invalid shell script (syntax error)
# ============================================================================
cat > "$TEST_TMPDIR/test-validate-invalid.sh" <<'EOF'
#!/usr/bin/env bash
if [[ true ]  # Missing closing bracket
echo "broken"
fi
EOF

if ! "$SCRIPT" "$TEST_TMPDIR/test-validate-invalid.sh" 2>/dev/null; then
    echo "PASS: Invalid script fails"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Invalid script should fail"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Quiet mode
# ============================================================================
OUTPUT=$("$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.sh" 2>&1 || true)
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
OUTPUT=$("$SCRIPT" /nonexistent/file.sh 2>&1 || true)
if grep -qi "not found\|error" <<< "$OUTPUT"; then
    echo "PASS: Errors on nonexistent file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 8: Exit code 0 on valid script
# ============================================================================
if "$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.sh"; then
    echo "PASS: Exit code 0 on valid script"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Expected exit code 0 on valid script"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 9: Exit code 1 on invalid script
# ============================================================================
if "$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-invalid.sh" 2>/dev/null; then
    echo "FAIL: Expected exit code 1 on invalid script"
    ((TESTS_FAILED++)) || true
else
    echo "PASS: Exit code 1 on invalid script"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 10: Unknown option handling
# ============================================================================
OUTPUT=$("$SCRIPT" --invalid-flag 2>&1 || true)
if grep -qi "unknown option\|error" <<< "$OUTPUT"; then
    echo "PASS: Unknown option shows error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Unknown option should show error"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 11: Missing arguments
# ============================================================================
OUTPUT=$("$SCRIPT" 2>&1 || true)
if grep -qi "missing\|usage\|error" <<< "$OUTPUT"; then
    echo "PASS: Missing arguments shows error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Missing arguments should show error"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 12: Strict mode with shellcheck
# ============================================================================
if command -v shellcheck &>/dev/null; then
    if "$SCRIPT" --strict "$TEST_TMPDIR/test-validate-valid.sh" 2>/dev/null; then
        echo "PASS: Strict mode works on valid script"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Strict mode should pass on valid script"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: Strict mode test (shellcheck not available)"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 13: Shellcheck passes on script itself
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
