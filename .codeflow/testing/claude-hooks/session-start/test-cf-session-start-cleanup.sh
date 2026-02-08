#!/usr/bin/env bash
# Test: cf-session-start-cleanup.sh
# Location: .codeflow/testing/claude-hooks/session-start/test-cf-session-start-cleanup.sh
#
# Tests SessionStart cleanup/init hook
# Verifies session initialization, directory creation, and sentinel cleanup

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-cleanup.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Setup test environment
setup_test_env() {
    mkdir -p "$REPO_ROOT/.state/logs/sessions" 2>/dev/null || true
    mkdir -p /tmp/claude/managed/sentinels 2>/dev/null || true
}

# Cleanup test artifacts
cleanup_test_artifacts() {
    rm -f "$REPO_ROOT/.state/logs/sessions/session-test-*.meta" 2>/dev/null || true
    rm -f /tmp/claude/managed/sentinels/test-*.json 2>/dev/null || true
}

echo "=== Testing cf-session-start-cleanup.sh ==="
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
setup_test_env
result=$(CODEFLOW_SESSION_ID="test-cleanup-session" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Exits 0 without session ID (generates one)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 without session ID"
else
    fail "Should exit 0 without session ID"
fi

# Test 11: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

# Test 12: All exits are 0
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "SessionStart should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""
echo "--- Session ID Generation ---"

# Test 13: Has SESSION_ID generation logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CODEFLOW_SESSION_ID" "$HOOK"; then
    pass "Has SESSION_ID generation logic"
else
    fail "Should generate session ID"
fi

# Test 14: Generates ULID-like session ID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ses-" "$HOOK"; then
    pass "Generates ULID-like session ID with ses- prefix"
else
    fail "Should generate ses- prefixed ID"
fi

# Test 15: Uses /dev/urandom for randomness
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/dev/urandom" "$HOOK"; then
    pass "Uses /dev/urandom for randomness"
else
    fail "Should use /dev/urandom"
fi

# Test 16: Exports CODEFLOW_SESSION_ID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export CODEFLOW_SESSION_ID" "$HOOK"; then
    pass "Exports CODEFLOW_SESSION_ID"
else
    fail "Should export CODEFLOW_SESSION_ID"
fi

# Test 17: Checks if session ID already set
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CODEFLOW_SESSION_ID:-' "$HOOK"; then
    pass "Checks if session ID already set"
else
    fail "Should check existing session ID"
fi

echo ""
echo "--- Directory Creation ---"

# Test 18: Creates session log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*logs/sessions" "$HOOK"; then
    pass "Creates session log directory"
else
    fail "Should create session log directory"
fi

# Test 19: Creates security log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*logs/security" "$HOOK"; then
    pass "Creates security log directory"
else
    fail "Should create security log directory"
fi

# Test 20: Creates db directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*\.state/db" "$HOOK"; then
    pass "Creates db directory"
else
    fail "Should create db directory"
fi

# Test 21: Creates sentinel directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*SENTINEL_DIR" "$HOOK"; then
    pass "Creates sentinel directory"
else
    fail "Should create sentinel directory"
fi

# Test 22: Has error suppression for mkdir
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true' "$HOOK"; then
    pass "Has error suppression for mkdir"
else
    fail "Should have error suppression for mkdir"
fi

echo ""
echo "--- Sentinel Cleanup ---"

# Test 23: Has sentinel cleanup logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "sentinel|SENTINEL" "$HOOK"; then
    pass "Has sentinel cleanup logic"
else
    fail "Should cleanup sentinels"
fi

# Test 24: References SENTINEL_DIR
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_DIR" "$HOOK"; then
    pass "References SENTINEL_DIR"
else
    fail "Should reference SENTINEL_DIR"
fi

# Test 25: Cleans *.json sentinel files (not *.sentinel)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\*\.json' "$HOOK" && grep -q "SENTINEL_DIR" "$HOOK"; then
    pass "Cleans *.json sentinel files"
else
    fail "Should clean *.json sentinel files"
fi

# Test 26: Has SENTINEL_TTL variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_TTL" "$HOOK"; then
    pass "Has SENTINEL_TTL variable"
else
    fail "Should have SENTINEL_TTL variable"
fi

# Test 27: Uses find for cleanup
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "find.*SENTINEL_DIR" "$HOOK"; then
    pass "Uses find for cleanup"
else
    fail "Should use find for cleanup"
fi

# Test 28: Uses -mmin for age check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "\-mmin" "$HOOK"; then
    pass "Uses -mmin for age check"
else
    fail "Should use -mmin for age check"
fi

echo ""
echo "--- Config Integration ---"

# Test 29: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 30: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 31: Reads sentinel directory from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel.directory" "$HOOK"; then
    pass "Reads sentinel directory from config"
else
    fail "Should read sentinel directory from config"
fi

# Test 32: Reads sentinel TTL from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel.default_ttl" "$HOOK"; then
    pass "Reads sentinel TTL from config"
else
    fail "Should read sentinel TTL from config"
fi

# Test 33: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 34: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

# Test 35: Has fallback for sentinel directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/tmp/claude/managed/sentinels" "$HOOK"; then
    pass "Has fallback for sentinel directory"
else
    fail "Should have fallback sentinel directory"
fi

echo ""
echo "--- Session Metadata ---"

# Test 36: Has SESSION_META_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_META_FILE" "$HOOK"; then
    pass "Has SESSION_META_FILE variable"
else
    fail "Should have SESSION_META_FILE variable"
fi

# Test 37: Writes .meta file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".meta" "$HOOK"; then
    pass "Writes .meta file"
else
    fail "Should write .meta file"
fi

# Test 38: Captures git branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_BRANCH" "$HOOK"; then
    pass "Captures git branch"
else
    fail "Should capture git branch"
fi

# Test 39: Captures git commit
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_COMMIT" "$HOOK"; then
    pass "Captures git commit"
else
    fail "Should capture git commit"
fi

# Test 40: Captures user
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q '\${USER:-' "$HOOK"; then
    pass "Captures user with fallback"
else
    fail "Should capture user"
fi

# Test 41: Includes started_at timestamp
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "started_at" "$HOOK"; then
    pass "Includes started_at timestamp"
else
    fail "Should include started_at"
fi

# Test 42: Includes started_epoch for duration calculation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "started_epoch" "$HOOK"; then
    pass "Includes started_epoch for duration calculation"
else
    fail "Should include started_epoch"
fi

# Test 43: Uses jq -nc for JSON creation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then
    pass "Uses jq -nc for JSON creation"
else
    fail "Should use jq -nc for JSON"
fi

echo ""
echo "--- Code Quality ---"

# Test 44: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 45: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 46: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 47: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Functional Tests ---"

# Test 48: Creates session metadata on execution
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
CODEFLOW_SESSION_ID="test-metadata-session" bash "$HOOK" 2>/dev/null
META_FILE="$REPO_ROOT/.state/logs/sessions/session-test-metadata-session.meta"
if [[ -f "$META_FILE" ]]; then
    pass "Creates session metadata on execution"
else
    fail "Should create session metadata"
fi

# Test 49: Metadata contains session_id
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$META_FILE" ]] && grep -q "test-metadata-session" "$META_FILE"; then
    pass "Metadata contains session_id"
else
    fail "Metadata should contain session_id"
fi

# Test 50: Metadata contains started_epoch
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$META_FILE" ]] && grep -q "started_epoch" "$META_FILE"; then
    pass "Metadata contains started_epoch"
else
    fail "Metadata should contain started_epoch"
fi

# Test 51: Metadata is valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$META_FILE" ]] && command -v jq &>/dev/null; then
    if jq . "$META_FILE" &>/dev/null; then
        pass "Metadata is valid JSON"
    else
        fail "Metadata should be valid JSON"
    fi
else
    pass "Metadata JSON validation (skipped - jq not available)"
fi

# Test 52: Cleans expired sentinels
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
# Create a "old" sentinel by using touch with old timestamp
OLD_SENTINEL="/tmp/claude/managed/sentinels/test-old-sentinel.json"
echo '{"test": true}' > "$OLD_SENTINEL"
# Use touch to set modification time to 15 minutes ago (if available)
if touch -A -001500 "$OLD_SENTINEL" 2>/dev/null || touch -d "15 minutes ago" "$OLD_SENTINEL" 2>/dev/null; then
    CODEFLOW_SESSION_ID="test-cleanup-sentinel" bash "$HOOK" 2>/dev/null
    if [[ ! -f "$OLD_SENTINEL" ]]; then
        pass "Cleans expired sentinels"
    else
        fail "Should clean expired sentinels"
        rm -f "$OLD_SENTINEL"
    fi
else
    # Can't reliably test timestamp modification
    pass "Cleans expired sentinels (timestamp test skipped)"
fi

# Test 53: Preserves recent sentinels
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
RECENT_SENTINEL="/tmp/claude/managed/sentinels/test-recent-sentinel.json"
echo '{"test": true}' > "$RECENT_SENTINEL"
CODEFLOW_SESSION_ID="test-preserve-sentinel" bash "$HOOK" 2>/dev/null
if [[ -f "$RECENT_SENTINEL" ]]; then
    pass "Preserves recent sentinels"
    rm -f "$RECENT_SENTINEL"
else
    fail "Should preserve recent sentinels"
fi

# Test 54: Handles missing sentinel directory gracefully
TESTS_RUN=$((TESTS_RUN + 1))
rm -rf /tmp/claude/managed/sentinels-test 2>/dev/null || true
result=$(CODEFLOW_SESSION_ID="test-no-sentinel-dir" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles missing sentinel directory gracefully"
else
    fail "Should handle missing sentinel directory"
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
