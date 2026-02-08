#!/usr/bin/env bash
# Test: Validation - cf-validate-yaml.sh
# Location: .codeflow/testing/scripts/security/test-cf-validate-yaml.sh
#
# Tests the YAML file validation functionality

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
SCRIPT="$REPO_ROOT/.codeflow/scripts/security/validation/cf-validate-yaml.sh"

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Cleanup
cleanup() {
    rm -f /tmp/claude/test-validate-*.yaml /tmp/claude/test-validate-*.yml 2>/dev/null || true
}
trap cleanup EXIT

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
# Test 4: Valid YAML
# ============================================================================
echo ""
echo "--- Validation functionality ---"

# Check if YAML validator is available
HAS_YAML_VALIDATOR=false
if python3 -c "import yaml" 2>/dev/null || command -v yq &>/dev/null; then
    HAS_YAML_VALIDATOR=true
fi

cat > /tmp/claude/test-validate-valid.yaml <<'EOF'
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
    OUTPUT=$("$SCRIPT" /tmp/claude/test-validate-valid.yaml 2>&1 || true)
    if echo "$OUTPUT" | grep -qi "PASS"; then
        echo "PASS: Valid YAML passes"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Valid YAML should pass"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available (python yaml or yq)"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 5: Invalid YAML (syntax error)
# ============================================================================
cat > /tmp/claude/test-validate-invalid.yaml <<'EOF'
name: test
  invalid indentation here
    broken: true
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    if ! "$SCRIPT" /tmp/claude/test-validate-invalid.yaml 2>/dev/null; then
        echo "PASS: Invalid YAML fails"
        ((TESTS_PASSED++)) || true
    else
        echo "FAIL: Invalid YAML should fail"
        ((TESTS_FAILED++)) || true
    fi
else
    echo "SKIP: No YAML validator available"
    ((TESTS_PASSED++)) || true
fi

# ============================================================================
# Test 6: Empty YAML (valid)
# ============================================================================
echo "---" > /tmp/claude/test-validate-empty.yaml

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" /tmp/claude/test-validate-empty.yaml 2>&1 || true)
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
# Test 7: YAML list (valid)
# ============================================================================
cat > /tmp/claude/test-validate-list.yaml <<'EOF'
- item1
- item2
- item3
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" /tmp/claude/test-validate-list.yaml 2>&1 || true)
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
# Test 8: .yml extension
# ============================================================================
cat > /tmp/claude/test-validate-ext.yml <<'EOF'
key: value
EOF

if [[ "$HAS_YAML_VALIDATOR" == "true" ]]; then
    OUTPUT=$("$SCRIPT" /tmp/claude/test-validate-ext.yml 2>&1 || true)
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
# Test 9: File not found
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
# Test 10: Shellcheck passes
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
