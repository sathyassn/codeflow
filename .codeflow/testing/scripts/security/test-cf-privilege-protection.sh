#!/usr/bin/env bash
# Test: cf-privilege-protection.sh
# Location: .codeflow/testing/scripts/security/test-cf-privilege-protection.sh
#
# Tests the privilege escalation protection module

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-privilege-protection.sh"

export REPO_ROOT LIB_DIR

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Helper function to test command blocking
test_blocks_command() {
    local command="$1"
    local description="$2"
    local output

    output=$(COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>&1 || true)

    if echo "$output" | grep -q "BLOCKED"; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected block"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Helper function to test command allowing
test_allows_command() {
    local command="$1"
    local description="$2"

    if COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" \
       bash -c "source '$MODULE'" 2>/dev/null; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected allow"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing cf-privilege-protection.sh ==="
echo ""

# Test 1: Block sudo
echo "Test 1: Block sudo"
test_blocks_command "sudo apt install vim" "Should block sudo"

# Test 2: Block su
echo "Test 2: Block su"
test_blocks_command "su -" "Should block su"

# Test 3: Block doas
echo "Test 3: Block doas"
test_blocks_command "doas command" "Should block doas"

# Test 4: Block pkexec
echo "Test 4: Block pkexec"
test_blocks_command "pkexec /usr/bin/command" "Should block pkexec"

# Test 5: Block bash -c
echo "Test 5: Block bash -c"
test_blocks_command "bash -c 'malicious command'" "Should block bash -c"

# Test 6: Block eval
echo "Test 6: Block eval"
test_blocks_command "eval 'command'" "Should block eval"

# Test 7: Block LD_PRELOAD
echo "Test 7: Block LD_PRELOAD"
test_blocks_command "LD_PRELOAD=/lib/evil.so command" "Should block LD_PRELOAD"

# Test 8: Allow normal commands
echo "Test 8: Allow normal commands"
test_allows_command "ls -la" "Should allow ls"

# Test 9: Allow npm commands
echo "Test 9: Allow npm commands"
test_allows_command "npm install package" "Should allow npm"

# Test 10: Allow git commands
echo "Test 10: Allow git commands"
test_allows_command "git status" "Should allow git status"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
