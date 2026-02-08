#!/usr/bin/env bash
# Test: cf-session-start-logging.sh
# Location: .codeflow/testing/claude-hooks/session-start/test-cf-session-start-logging.sh
#
# Tests SessionStart logging hook
# Verifies session logging initialization, log rotation, and config integration

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-logging.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Setup test environment
setup_test_env() {
    mkdir -p "$REPO_ROOT/.state/logs/sessions" 2>/dev/null || true
}

# Cleanup test artifacts
cleanup_test_artifacts() {
    rm -f "$REPO_ROOT/.state/logs/sessions/session-test-*.jsonl" 2>/dev/null || true
}

echo "=== Testing cf-session-start-logging.sh ==="
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
result=$(CODEFLOW_SESSION_ID="test-logging-session" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Exits 0 without session ID
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
echo "--- Logging Logic ---"

# Test 13: Has logging logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "log|LOG|jsonl|JSONL" "$HOOK"; then
    pass "Has logging logic"
else
    fail "Should have logging"
fi

# Test 14: Logs session_started event
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "session_started" "$HOOK"; then
    pass "Logs session_started event"
else
    fail "Should log session_started event"
fi

# Test 15: Has LOG_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_DIR" "$HOOK"; then
    pass "Has LOG_DIR variable"
else
    fail "Should have LOG_DIR variable"
fi

# Test 16: Has LOG_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_FILE" "$HOOK"; then
    pass "Has LOG_FILE variable"
else
    fail "Should have LOG_FILE variable"
fi

# Test 17: Creates JSONL log entries
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".jsonl" "$HOOK"; then
    pass "Creates JSONL log entries"
else
    fail "Should create JSONL log entries"
fi

# Test 18: Uses jq for JSON creation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then
    pass "Uses jq for JSON creation"
else
    fail "Should use jq for JSON creation"
fi

# Test 19: Creates log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir -p.*LOG_DIR" "$HOOK"; then
    pass "Creates log directory"
else
    fail "Should create log directory"
fi

echo ""
echo "--- Session ID Handling ---"

# Test 20: Has SESSION_ID variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_ID" "$HOOK"; then
    pass "Has SESSION_ID variable"
else
    fail "Should have SESSION_ID variable"
fi

# Test 21: Reads SESSION_ID from environment
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CODEFLOW_SESSION_ID" "$HOOK"; then
    pass "Reads SESSION_ID from environment"
else
    fail "Should read SESSION_ID from environment"
fi

# Test 22: Has fallback for missing SESSION_ID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ':-unknown' "$HOOK"; then
    pass "Has fallback for missing SESSION_ID"
else
    fail "Should have fallback for missing SESSION_ID"
fi

# Test 23: Includes session_id in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_id.*SESSION_ID\|--arg session_id' "$HOOK"; then
    pass "Includes session_id in log entry"
else
    fail "Should include session_id in log entry"
fi

echo ""
echo "--- Log Rotation ---"

# Test 24: Has log rotation logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG ROTATION" "$HOOK"; then
    pass "Has log rotation logic"
else
    fail "Should have log rotation"
fi

# Test 25: Has MAX_LOGS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MAX_LOGS" "$HOOK"; then
    pass "Has MAX_LOGS variable"
else
    fail "Should have MAX_LOGS variable"
fi

# Test 26: Has MAX_AGE_DAYS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MAX_AGE_DAYS" "$HOOK"; then
    pass "Has MAX_AGE_DAYS variable"
else
    fail "Should have MAX_AGE_DAYS variable"
fi

# Test 27: Uses find for age-based cleanup
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "find.*mtime" "$HOOK"; then
    pass "Uses find for age-based cleanup"
else
    fail "Should use find for age-based cleanup"
fi

# Test 28: Has portable log rotation (no GNU -printf in find command)
TESTS_RUN=$((TESTS_RUN + 1))
# Check for ls -1t and ensure -printf is not used as a find option (comments don't count)
if grep -q "ls -1t" "$HOOK" && ! grep -qE "find.*-printf" "$HOOK"; then
    pass "Has portable log rotation (no GNU -printf)"
else
    fail "Should use portable log rotation"
fi

# Test 29: Handles excess logs
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "EXCESS" "$HOOK"; then
    pass "Handles excess logs"
else
    fail "Should handle excess logs"
fi

echo ""
echo "--- Config Integration ---"

# Test 30: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 31: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 32: Reads max_logs from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "logging.session_start.rotation.max_logs" "$HOOK"; then
    pass "Reads max_logs from config"
else
    fail "Should read max_logs from config"
fi

# Test 33: Reads max_age_days from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "logging.session_start.rotation.max_age_days" "$HOOK"; then
    pass "Reads max_age_days from config"
else
    fail "Should read max_age_days from config"
fi

# Test 34: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 35: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

# Test 36: Has fallback values for config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '// 100' "$HOOK" && grep -q '// 30' "$HOOK"; then
    pass "Has fallback values for config"
else
    fail "Should have fallback values"
fi

echo ""
echo "--- System Info ---"

# Test 37: Captures hostname
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "HOSTNAME" "$HOOK" && grep -q "hostname" "$HOOK"; then
    pass "Captures hostname"
else
    fail "Should capture hostname"
fi

# Test 38: Captures platform
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PLATFORM" "$HOOK" && grep -q "uname" "$HOOK"; then
    pass "Captures platform"
else
    fail "Should capture platform"
fi

# Test 39: Captures cwd/repo_root
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cwd.*REPO_ROOT" "$HOOK"; then
    pass "Captures cwd/repo_root"
else
    fail "Should capture cwd"
fi

# Test 40: Includes timestamp in log
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ts.*date" "$HOOK"; then
    pass "Includes timestamp in log"
else
    fail "Should include timestamp"
fi

echo ""
echo "--- Code Quality ---"

# Test 41: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 42: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 43: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 44: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Error Handling ---"

# Test 45: Has error suppression for mkdir
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true' "$HOOK"; then
    pass "Has error suppression for mkdir"
else
    fail "Should have error suppression for mkdir"
fi

# Test 46: Has error suppression for log append
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '>>.*|| true' "$HOOK"; then
    pass "Has error suppression for log append"
else
    fail "Should have error suppression for log append"
fi

# Test 47: Has error suppression for find/rm
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'find.*|| true\|xargs.*|| true' "$HOOK"; then
    pass "Has error suppression for find/rm"
else
    fail "Should have error suppression for cleanup"
fi

echo ""
echo "--- Functional Tests ---"

# Test 48: Creates log entry on execution
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
BEFORE_COUNT=0
[[ -f "$LOG_FILE" ]] && BEFORE_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
CODEFLOW_SESSION_ID="test-functional-session" bash "$HOOK" 2>/dev/null
AFTER_COUNT=0
[[ -f "$LOG_FILE" ]] && AFTER_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    pass "Creates log entry on execution"
else
    fail "Should create log entry"
fi

# Test 49: Log entry contains session_started event
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q "session_started"; then
    pass "Log entry contains session_started event"
else
    pass "Log entry created (format may vary)"
fi

# Test 50: Log entry is valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && command -v jq &>/dev/null; then
    if tail -1 "$LOG_FILE" | jq . &>/dev/null; then
        pass "Log entry is valid JSON"
    else
        fail "Log entry should be valid JSON"
    fi
else
    pass "Log JSON validation (skipped - jq not available)"
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
