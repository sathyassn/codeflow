#!/usr/bin/env bash
# Test: cf-session-end-cleanup.sh
# Location: .codeflow/testing/claude-hooks/session-end/test-cf-session-end-cleanup.sh
#
# Tests SessionEnd cleanup hook
# Verifies proper cleanup of sentinels, state files, and temp files

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Setup test directories
setup_test_dirs() {
    mkdir -p /tmp/claude/managed/sentinels 2>/dev/null || true
    mkdir -p /tmp/claude/managed/state 2>/dev/null || true
    mkdir -p /tmp/claude/sessions/test-session 2>/dev/null || true
}

# Cleanup test artifacts
cleanup_test_artifacts() {
    rm -f /tmp/claude/managed/sentinels/test-*.json 2>/dev/null || true
    rm -f /tmp/claude/managed/state/*-test-session* 2>/dev/null || true
    rm -rf /tmp/claude/sessions/test-session 2>/dev/null || true
}

echo "=== Testing cf-session-end-cleanup.sh ==="
echo ""

# Test 1: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
TESTS_RUN=$((TESTS_RUN + 1))
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Has proper header comments
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 5: Uses set -euo pipefail
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Has VERSION constant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "VERSION=" "$HOOK" || grep -q "readonly VERSION" "$HOOK"; then
    pass "Has VERSION constant"
else
    fail "Should have VERSION constant"
fi

# Test 7: Has Hook Type header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Hook Type:" "$HOOK"; then
    pass "Has Hook Type header"
else
    fail "Should have Hook Type header"
fi

# Test 8: Has Location header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Location:" "$HOOK"; then
    pass "Has Location header"
else
    fail "Should have Location header"
fi

echo ""
echo "--- Execution Tests ---"

# Test 9: Exits 0 on execution
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Exits 0 with session ID
TESTS_RUN=$((TESTS_RUN + 1))
result=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with session ID"
else
    fail "Should exit 0 with session ID"
fi

# Test 11: Exits 0 without session ID
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 without session ID"
else
    fail "Should exit 0 without session ID"
fi

# Test 12: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

echo ""
echo "--- Sentinel Cleanup ---"

# Test 13: Has sentinel cleanup logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "sentinel|SENTINEL" "$HOOK"; then
    pass "Has sentinel cleanup logic"
else
    fail "Should cleanup sentinels"
fi

# Test 14: References SENTINEL_DIR
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_DIR" "$HOOK"; then
    pass "References SENTINEL_DIR"
else
    fail "Should reference SENTINEL_DIR"
fi

# Test 15: Cleans *.json sentinel files
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qF '*.json' "$HOOK" && grep -q "SENTINEL_DIR" "$HOOK"; then
    pass "Cleans *.json sentinel files"
else
    fail "Should clean *.json sentinel files"
fi

# Test 16: Reads sentinel directory from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel.directory" "$HOOK"; then
    pass "Reads sentinel directory from config"
else
    fail "Should read sentinel directory from config"
fi

# Test 17: Has fallback sentinel directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/tmp/claude/managed/sentinels" "$HOOK"; then
    pass "Has fallback sentinel directory"
else
    fail "Should have fallback sentinel directory"
fi

echo ""
echo "--- State Cleanup ---"

# Test 18: Has state cleanup logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "STATE_DIR" "$HOOK"; then
    pass "Has state cleanup logic"
else
    fail "Should cleanup state files"
fi

# Test 19: Cleans session-specific state files
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'SESSION_ID' "$HOOK" && grep -q "STATE_DIR" "$HOOK"; then
    pass "Cleans session-specific state files"
else
    fail "Should clean session-specific state files"
fi

# Test 20: Reads state directory from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "state_folder\|managed_tmp" "$HOOK"; then
    pass "Reads state directory from config"
else
    fail "Should read state directory from config"
fi

# Test 21: Has fallback state directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/tmp/claude/managed/state" "$HOOK"; then
    pass "Has fallback state directory"
else
    fail "Should have fallback state directory"
fi

# Test 22: Only cleans state for known session
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'SESSION_ID.*!=.*unknown' "$HOOK"; then
    pass "Only cleans state for known session"
else
    fail "Should only clean state for known session"
fi

echo ""
echo "--- Temp File Cleanup ---"

# Test 23: Has temp cleanup logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TEMP_DIR" "$HOOK"; then
    pass "Has temp cleanup logic"
else
    fail "Should cleanup temp files"
fi

# Test 24: References session temp directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/tmp/claude/sessions" "$HOOK"; then
    pass "References session temp directory"
else
    fail "Should reference session temp directory"
fi

# Test 25: Uses rm -rf for temp cleanup
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "rm -rf.*TEMP_DIR" "$HOOK"; then
    pass "Uses rm -rf for temp cleanup"
else
    fail "Should use rm -rf for temp cleanup"
fi

echo ""
echo "--- Config Integration ---"

# Test 26: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 27: Uses jq for JSON parsing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 28: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 29: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null' "$HOOK" || grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Code Quality ---"

# Test 30: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 31: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 32: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 33: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 34: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 35: All exits are 0
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "SessionEnd should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""
echo "--- Error Handling ---"

# Test 36: Has error suppression for rm commands
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'rm.*|| true' "$HOOK"; then
    pass "Has error suppression for rm commands"
else
    fail "Should have error suppression for rm"
fi

# Test 37: Has 2>/dev/null for rm commands
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'rm.*2>/dev/null' "$HOOK"; then
    pass "Has stderr suppression for rm commands"
else
    fail "Should suppress stderr for rm"
fi

# Test 38: Checks directory existence before cleanup
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\[\[ -d.*SENTINEL_DIR' "$HOOK" && grep -q '\[\[ -d.*TEMP_DIR' "$HOOK"; then
    pass "Checks directory existence before cleanup"
else
    fail "Should check directory existence"
fi

echo ""
echo "--- Functional Tests ---"

# Test 39: Actually cleans sentinel files
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
echo '{"test": true}' > /tmp/claude/managed/sentinels/test-sentinel.json
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" 2>/dev/null
if [[ ! -f /tmp/claude/managed/sentinels/test-sentinel.json ]]; then
    pass "Actually cleans sentinel files"
else
    fail "Should actually clean sentinel files"
    rm -f /tmp/claude/managed/sentinels/test-sentinel.json
fi

# Test 40: Actually cleans session temp directory
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
touch /tmp/claude/sessions/test-session/test-file.txt 2>/dev/null || true
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" 2>/dev/null
if [[ ! -d /tmp/claude/sessions/test-session ]]; then
    pass "Actually cleans session temp directory"
else
    fail "Should actually clean session temp directory"
    rm -rf /tmp/claude/sessions/test-session
fi

# Test 41: Actually cleans session state files
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
echo '{"count": 1}' > /tmp/claude/managed/state/memory-progress-test-session
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" 2>/dev/null
if [[ ! -f /tmp/claude/managed/state/memory-progress-test-session ]]; then
    pass "Actually cleans session state files"
else
    fail "Should actually clean session state files"
    rm -f /tmp/claude/managed/state/memory-progress-test-session
fi

# Test 42: Does not clean state for unknown session
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
echo '{"count": 1}' > /tmp/claude/managed/state/memory-progress-other-session
bash "$HOOK" 2>/dev/null  # No CODEFLOW_SESSION_ID set
if [[ -f /tmp/claude/managed/state/memory-progress-other-session ]]; then
    pass "Does not clean state for unknown session"
    rm -f /tmp/claude/managed/state/memory-progress-other-session
else
    fail "Should not clean state for unknown session"
fi

# Test 43: Handles missing directories gracefully
TESTS_RUN=$((TESTS_RUN + 1))
result=$(CODEFLOW_SESSION_ID="nonexistent-session" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles missing directories gracefully"
else
    fail "Should handle missing directories"
fi

# Cleanup
cleanup_test_artifacts

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
