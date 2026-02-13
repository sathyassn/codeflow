#!/usr/bin/env bash
# Test: Validation - cf-validate-yaml.sh
# Location: .codeflow/testing/scripts/security/test-cf-validate-yaml.sh
#
# Tests the YAML file validation functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../lib/test-isolation.sh"
SCRIPT="$REAL_REPO_ROOT/.codeflow/scripts/security/validation/cf-validate-yaml.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

echo "=== Testing cf-validate-yaml.sh ==="
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
# Test 4: Unknown option handling
# ============================================================================
OUTPUT=$("$SCRIPT" --invalid-flag 2>&1 || true)
if echo "$OUTPUT" | grep -qi "unknown option"; then
    echo "PASS: Unknown option shows error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Unknown option should show error"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 5: Missing arguments
# ============================================================================
OUTPUT=$("$SCRIPT" 2>&1 || true)
if echo "$OUTPUT" | grep -qi "missing"; then
    echo "PASS: Missing arguments shows error"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Missing arguments should show error"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 6: Valid YAML
# ============================================================================
echo ""
echo "--- Validation functionality ---"

# Check if YAML validator is available
HAS_YAML_VALIDATOR=false
if python3 -c "import yaml" 2>/dev/null || command -v yq &>/dev/null; then
    HAS_YAML_VALIDATOR=true
fi

cat > "$TEST_TMPDIR/test-validate-valid.yaml" <<'EOF'
name: test
version: 1.0.0
features:
  - feature1
  - feature2
config:
  key: value
  nested:
    item: data
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    if "$SCRIPT" "$TEST_TMPDIR/test-validate-valid.yaml" >/dev/null 2>&1; then
        echo "PASS: Valid YAML exits 0"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Valid YAML should exit 0"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available (python yaml or yq)"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 7: Valid YAML output contains PASS
# ============================================================================
if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-valid.yaml" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "PASS"; then
        echo "PASS: Valid YAML shows PASS message"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Valid YAML should show PASS message"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 8: Invalid YAML (syntax error) exits non-zero
# ============================================================================
cat > "$TEST_TMPDIR/test-validate-invalid.yaml" <<'EOF'
name: test
  invalid indentation here
    broken: true
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    if ! "$SCRIPT" "$TEST_TMPDIR/test-validate-invalid.yaml" 2>/dev/null; then
        echo "PASS: Invalid YAML exits non-zero"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Invalid YAML should exit non-zero"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 9: Quiet mode suppresses output
# ============================================================================
if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" --quiet "$TEST_TMPDIR/test-validate-valid.yaml" 2>&1 || true)
    if [[ -z "$OUTPUT" ]]; then
        echo "PASS: Quiet mode suppresses output"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Quiet mode should suppress output (got: $OUTPUT)"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 10: Empty YAML (valid)
# ============================================================================
echo "---" > "$TEST_TMPDIR/test-validate-empty.yaml"

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-empty.yaml" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "PASS"; then
        echo "PASS: Empty YAML is valid"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Empty YAML should be valid"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 11: YAML list (valid)
# ============================================================================
cat > "$TEST_TMPDIR/test-validate-list.yaml" <<'EOF'
- item1
- item2
- item3
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-list.yaml" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "PASS"; then
        echo "PASS: YAML list is valid"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: YAML list should be valid"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 12: .yml extension
# ============================================================================
cat > "$TEST_TMPDIR/test-validate-ext.yml" <<'EOF'
key: value
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" "$TEST_TMPDIR/test-validate-ext.yml" 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "PASS"; then
        echo "PASS: .yml extension works"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: .yml extension should work"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 13: File not found
# ============================================================================
OUTPUT=$("$SCRIPT" /nonexistent/file.yaml 2>&1 || true)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "PASS: Errors on nonexistent file"
    ((TESTS_PASSED++)) || true
else
    echo "FAIL: Should error on nonexistent file"
    ((TESTS_FAILED++)) || true
fi

# ============================================================================
# Test 14: Strict mode - duplicate key detection
# ============================================================================
echo ""
echo "--- Strict mode ---"

if [[ "$HAS_YAML_VALIDATOR" == "true" ]] && python3 -c "import yaml" 2>/dev/null; then
    cat > "$TEST_TMPDIR/test-validate-dupkeys.yaml" <<'EOF'
name: first
name: second
key: value
EOF

    if ! "$SCRIPT" --strict "$TEST_TMPDIR/test-validate-dupkeys.yaml" 2>/dev/null; then
        echo "PASS: Strict mode catches duplicate keys"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Strict mode should catch duplicate keys"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: python3 yaml not available for strict mode test"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 15: Strict mode - valid YAML passes
# ============================================================================
if [[ "$HAS_YAML_VALIDATOR" == "true" ]] && python3 -c "import yaml" 2>/dev/null; then
    if "$SCRIPT" --strict "$TEST_TMPDIR/test-validate-valid.yaml" >/dev/null 2>&1; then
        echo "PASS: Strict mode passes valid YAML"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Strict mode should pass valid YAML"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: python3 yaml not available for strict mode test"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 16: Shellcheck passes
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
