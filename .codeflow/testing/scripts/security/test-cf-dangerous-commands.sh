#!/usr/bin/env bash
# Test: cf-dangerous-commands.sh
# Location: .codeflow/testing/scripts/security/test_dangerous_commands.sh
#
# Tests the dangerous command detection module

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-dangerous-commands.sh"

# Set security-specific LIB_DIR
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
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

echo "=== Testing cf-dangerous-commands.sh ==="
echo ""

# Test 1: Block rm -rf /
echo "Test 1: Block rm -rf /"
test_blocks_command "rm -rf /" "Should block rm -rf /"

# Test 2: Block rm -rf /*
echo "Test 2: Block rm -rf /*"
test_blocks_command "rm -rf /*" "Should block rm -rf /*"

# Test 3: Block dd if=/dev/zero
echo "Test 3: Block dd if=/dev/zero"
test_blocks_command "dd if=/dev/zero of=/dev/sda" "Should block dd to disk"

# Test 4: Block chmod 777 /
echo "Test 4: Block chmod 777 /"
test_blocks_command "chmod 777 /" "Should block chmod 777 /"

# Test 5: Block fork bomb
echo "Test 5: Block fork bomb"
test_blocks_command ":(){:|:&};:" "Should block fork bomb"

# Test 6: Allow safe rm commands
echo "Test 6: Allow safe rm commands"
test_allows_command "rm -f /tmp/testfile" "Should allow safe rm"

# Test 7: Allow normal commands
echo "Test 7: Allow normal commands"
test_allows_command "ls -la" "Should allow ls"

# Test 8: Allow chmod on project files
echo "Test 8: Allow chmod on project files"
test_allows_command "chmod +x script.sh" "Should allow chmod +x on files"

# Test 9: Block mkfs commands
echo "Test 9: Block mkfs commands"
test_blocks_command "mkfs.ext4 /dev/sda1" "Should block mkfs"

# Test 10: Block recursive delete of home
echo "Test 10: Block recursive delete of home"
test_blocks_command "rm -rf ~" "Should block rm -rf ~"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
