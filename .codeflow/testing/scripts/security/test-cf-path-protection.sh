#!/usr/bin/env bash
# Test: cf-path-protection.sh
# Location: .codeflow/testing/scripts/security/test-cf-path-protection.sh
#
# Tests the path protection module

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
MODULE="$ENFORCEMENT_DIR/cf-path-protection.sh"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

export REPO_ROOT LIB_DIR CONFIG

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

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

echo "=== Testing cf-path-protection.sh ==="
echo ""

# Test 1: Block redirect TO settings.json
echo "Test 1: Block redirect to settings.json"
test_blocks_command "echo test > .claude/settings.json" "Should block redirect to settings.json"

# Test 2: Block modification of CLAUDE.md
echo "Test 2: Block modification of CLAUDE.md"
test_blocks_command "echo test >> .claude/CLAUDE.md" "Should block append to CLAUDE.md"

# Test 3: Block rm on protected hooks
echo "Test 3: Block rm on protected hooks"
test_blocks_command "rm .claude/hooks/codeflow/file.sh" "Should block rm on hooks"

# Test 4: Block redirect to config
echo "Test 4: Block redirect to config"
test_blocks_command "echo '{}' > .codeflow/config/file.json" "Should block write to config"

# Test 5: Allow read from normal files
echo "Test 5: Allow normal commands"
test_allows_command "cat src/main.py" "Should allow cat on normal files"

# Test 6: Allow /tmp/claude operations
echo "Test 6: Allow /tmp/claude operations"
test_allows_command "cat /tmp/claude/test.txt" "Should allow /tmp/claude access"

# Test 7: Allow normal file operations
echo "Test 7: Allow normal file operations"
test_allows_command "ls -la src/" "Should allow ls"

# Test 8: Block cp to protected path
echo "Test 8: Block cp to protected path"
test_blocks_command "cp /tmp/file .claude/settings.json" "Should block cp to protected"

# Test 9: Allow operations on non-protected paths
echo "Test 9: Allow operations on non-protected paths"
test_allows_command "rm -f src/temp.py" "Should allow rm on normal files"

# Test 10: Block mv to settings
echo "Test 10: Block mv to settings"
test_blocks_command "mv /tmp/file .claude/settings.json" "Should block mv to settings"

# Test 11: Block chmod on protected paths
echo "Test 11: Block chmod on protected paths"
test_blocks_command "chmod 777 .claude/settings.json" "Should block chmod on protected"

# Test 12: Block chown on protected paths
echo "Test 12: Block chown on protected paths"
test_blocks_command "chown root .claude/settings.json" "Should block chown on protected"

# Test 13: Block git rm on protected paths
echo "Test 13: Block git rm on protected paths"
test_blocks_command "git rm .claude/settings.json" "Should block git rm on protected"

# Test 14: Block rm on .claude directory
echo "Test 14: Block rm on .claude directory"
test_blocks_command "rm -rf .claude" "Should block rm on .claude dir"

# Test 15: Block rm on .claude/ with trailing slash
echo "Test 15: Block rm on .claude/ with trailing slash"
test_blocks_command "rm -rf .claude/" "Should block rm on .claude/"

# Test 16: Block rm on .codeflow directory
echo "Test 16: Block rm on .codeflow directory"
test_blocks_command "rm -rf .codeflow" "Should block rm on .codeflow dir"

# Test 17: Block rm on .codeflow/ with trailing slash
echo "Test 17: Block rm on .codeflow/ with trailing slash"
test_blocks_command "rm -rf .codeflow/" "Should block rm on .codeflow/"

# Test 18: Block unlink on protected paths
echo "Test 18: Block unlink on protected paths"
test_blocks_command "unlink .claude/settings.json" "Should block unlink on protected"

# Test 19: Block shred on protected paths
echo "Test 19: Block shred on protected paths"
test_blocks_command "shred .claude/settings.json" "Should block shred on protected"

# Test 20: Block truncate on protected paths
echo "Test 20: Block truncate on protected paths"
test_blocks_command "truncate -s 0 .claude/settings.json" "Should block truncate on protected"

# Test 21: Block operations on .github/workflows
echo "Test 21: Block operations on .github/workflows"
test_blocks_command "rm .github/workflows/ci.yml" "Should block rm on workflows"

# Test 22: Block operations on .git/hooks
echo "Test 22: Block operations on .git/hooks"
test_blocks_command "rm .git/hooks/pre-commit" "Should block rm on .git/hooks"

# Test 23: Block redirect to settings.local.json
echo "Test 23: Block redirect to settings.local.json"
test_blocks_command "echo '{}' > .claude/settings.local.json" "Should block redirect to local settings"

# Test 24: Block append to security scripts
echo "Test 24: Block append to security scripts"
test_blocks_command "echo 'exit 0' >> .codeflow/scripts/security/test.sh" "Should block append to security"

# Test 25: Allow fd redirect (2>&1)
echo "Test 25: Allow fd redirect"
test_allows_command "cat .claude/settings.json 2>&1" "Should allow fd redirect"

# Test 26: Allow read FROM protected paths
echo "Test 26: Allow read from protected paths"
test_allows_command "cat .claude/CLAUDE.md" "Should allow reading protected files"

# Test 27: Block cp overwrite to CLAUDE.md
echo "Test 27: Block cp to CLAUDE.md"
test_blocks_command "cp /tmp/evil.txt .claude/CLAUDE.md" "Should block cp to CLAUDE.md"

# Test 28: Block operations on quoted paths
echo "Test 28: Block operations on quoted paths"
test_blocks_command "rm \".claude/settings.json\"" "Should block rm on quoted path"

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
