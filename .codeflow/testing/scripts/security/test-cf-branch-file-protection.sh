#!/usr/bin/env bash
# Test: cf-branch-file-protection.sh
# Location: .codeflow/testing/scripts/security/test-cf-branch-file-protection.sh
#
# Tests the branch-aware file protection module
# Note: These tests simulate being on main branch

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-branch-file-protection.sh"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

export REPO_ROOT LIB_DIR CONFIG

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Get current branch for test awareness
CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "main")

# Helper function to test command blocking
test_blocks_command() {
    local command="$1"
    local description="$2"
    local output

    output=$(COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" CONFIG="$CONFIG" \
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

    if COMMAND="$command" REPO_ROOT="$REPO_ROOT" LIB_DIR="$LIB_DIR" CONFIG="$CONFIG" \
       bash -c "source '$MODULE'" 2>/dev/null; then
        echo "PASS: $description"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo "FAIL: $description - Expected allow"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

echo "=== Testing cf-branch-file-protection.sh ==="
echo "Current branch: $CURRENT_BRANCH"
echo ""

# Note: These tests are designed for main branch
if [[ "$CURRENT_BRANCH" != "main" ]] && [[ "$CURRENT_BRANCH" != "master" ]]; then
    echo "Note: Not on protected branch, some tests may pass differently"
fi

# Test 1: Block redirect on main
echo "Test 1: Block redirect on main"
if [[ "$CURRENT_BRANCH" == "main" ]] || [[ "$CURRENT_BRANCH" == "master" ]]; then
    test_blocks_command "echo test > src/file.txt" "Should block redirect on main"
else
    test_allows_command "echo test > src/file.txt" "Should allow redirect on feature branch"
fi

# Test 2: Block append on main
echo "Test 2: Block append on main"
if [[ "$CURRENT_BRANCH" == "main" ]] || [[ "$CURRENT_BRANCH" == "master" ]]; then
    test_blocks_command "echo test >> src/file.txt" "Should block append on main"
else
    test_allows_command "echo test >> src/file.txt" "Should allow append on feature branch"
fi

# Test 3: Block sed -i on main
echo "Test 3: Block sed -i on main"
if [[ "$CURRENT_BRANCH" == "main" ]] || [[ "$CURRENT_BRANCH" == "master" ]]; then
    test_blocks_command "sed -i '' 's/a/b/' file.txt" "Should block sed -i on main"
else
    test_allows_command "sed -i '' 's/a/b/' file.txt" "Should allow sed -i on feature"
fi

# Test 4: Allow /tmp/claude writes
echo "Test 4: Allow /tmp/claude writes"
test_allows_command "echo test > /tmp/claude/file.txt" "Should allow /tmp/claude writes"

# Test 5: Block touch on main
echo "Test 5: Block touch on main"
if [[ "$CURRENT_BRANCH" == "main" ]] || [[ "$CURRENT_BRANCH" == "master" ]]; then
    test_blocks_command "touch src/newfile.txt" "Should block touch on main"
else
    test_allows_command "touch src/newfile.txt" "Should allow touch on feature"
fi

# Test 6: Allow touch in /tmp
echo "Test 6: Allow touch in /tmp"
test_allows_command "touch /tmp/testfile" "Should allow touch in /tmp"

# Test 7: Block tee on main
echo "Test 7: Block tee on main"
if [[ "$CURRENT_BRANCH" == "main" ]] || [[ "$CURRENT_BRANCH" == "master" ]]; then
    test_blocks_command "echo test | tee src/file.txt" "Should block tee on main"
else
    test_allows_command "echo test | tee src/file.txt" "Should allow tee on feature"
fi

# Test 8: Block cp on main
echo "Test 8: Block cp on main"
if [[ "$CURRENT_BRANCH" == "main" ]] || [[ "$CURRENT_BRANCH" == "master" ]]; then
    test_blocks_command "cp /tmp/a src/b" "Should block cp on main"
else
    test_allows_command "cp /tmp/a src/b" "Should allow cp on feature"
fi

# Test 9: Allow read commands
echo "Test 9: Allow read commands"
test_allows_command "cat src/file.txt" "Should allow cat"

# Test 10: Allow ls commands
echo "Test 10: Allow ls commands"
test_allows_command "ls -la src/" "Should allow ls"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
