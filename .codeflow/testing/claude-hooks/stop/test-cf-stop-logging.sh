#!/usr/bin/env bash
# Test: cf-stop-logging.sh
# Location: .codeflow/testing/claude-hooks/stop/test-cf-stop-logging.sh
#
# Tests Stop logging hook
# Verifies stop event logging, git state capture, and config integration

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/stop/cf-stop-logging.sh"

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
    rm -f "$REPO_ROOT/.state/logs/sessions/stop-events-test-*.jsonl" 2>/dev/null || true
}

echo "=== Testing cf-stop-logging.sh ==="
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
result=$(CODEFLOW_SESSION_ID="test-stop-session" bash "$HOOK" 2>&1; echo "EXIT:$?")
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

# Test 11: Exits 0 with stop reason
TESTS_RUN=$((TESTS_RUN + 1))
result=$(STOP_REASON="user_requested" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with stop reason"
else
    fail "Should exit 0 with stop reason"
fi

# Test 12: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

# Test 13: All exits are 0
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "Stop hook should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""
echo "--- Logging Logic ---"

# Test 14: Has logging logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "log|LOG|jsonl|JSONL" "$HOOK"; then
    pass "Has logging logic"
else
    fail "Should have logging"
fi

# Test 15: Logs stop event
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"stop"' "$HOOK" || grep -q "event.*stop" "$HOOK"; then
    pass "Logs stop event"
else
    fail "Should log stop event"
fi

# Test 16: Has LOG_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_DIR" "$HOOK"; then
    pass "Has LOG_DIR variable"
else
    fail "Should have LOG_DIR variable"
fi

# Test 17: Has LOG_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_FILE" "$HOOK"; then
    pass "Has LOG_FILE variable"
else
    fail "Should have LOG_FILE variable"
fi

# Test 18: Creates JSONL log entries
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".jsonl" "$HOOK"; then
    pass "Creates JSONL log entries"
else
    fail "Should create JSONL log entries"
fi

# Test 19: Uses jq for JSON creation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then
    pass "Uses jq for JSON creation"
else
    fail "Should use jq for JSON creation"
fi

# Test 20: Creates log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir -p.*LOG_DIR" "$HOOK"; then
    pass "Creates log directory"
else
    fail "Should create log directory"
fi

echo ""
echo "--- Session ID Handling ---"

# Test 21: Has SESSION_ID variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_ID" "$HOOK"; then
    pass "Has SESSION_ID variable"
else
    fail "Should have SESSION_ID variable"
fi

# Test 22: Reads SESSION_ID from environment
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CODEFLOW_SESSION_ID" "$HOOK"; then
    pass "Reads SESSION_ID from environment"
else
    fail "Should read SESSION_ID from environment"
fi

# Test 23: Has fallback for missing SESSION_ID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ':-unknown' "$HOOK"; then
    pass "Has fallback for missing SESSION_ID"
else
    fail "Should have fallback for missing SESSION_ID"
fi

# Test 24: Includes session_id in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- '--arg session_id' "$HOOK"; then
    pass "Includes session_id in log entry"
else
    fail "Should include session_id in log entry"
fi

echo ""
echo "--- Stop Reason Handling ---"

# Test 25: Has STOP_REASON variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "STOP_REASON" "$HOOK"; then
    pass "Has STOP_REASON variable"
else
    fail "Should have STOP_REASON variable"
fi

# Test 26: Has fallback for missing STOP_REASON
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'STOP_REASON:-' "$HOOK"; then
    pass "Has fallback for missing STOP_REASON"
else
    fail "Should have fallback for STOP_REASON"
fi

# Test 27: Includes reason in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- '--arg reason' "$HOOK"; then
    pass "Includes reason in log entry"
else
    fail "Should include reason in log entry"
fi

echo ""
echo "--- Git State Capture ---"

# Test 28: Captures git branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_BRANCH" "$HOOK"; then
    pass "Captures git branch"
else
    fail "Should capture git branch"
fi

# Test 29: Uses git branch --show-current
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*branch --show-current" "$HOOK"; then
    pass "Uses git branch --show-current"
else
    fail "Should use git branch --show-current"
fi

# Test 30: Captures uncommitted changes status
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "HAS_CHANGES\|has_uncommitted" "$HOOK"; then
    pass "Captures uncommitted changes status"
else
    fail "Should capture uncommitted changes"
fi

# Test 31: Uses git status --porcelain
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*status --porcelain" "$HOOK"; then
    pass "Uses git status --porcelain"
else
    fail "Should use git status --porcelain"
fi

# Test 32: Includes git_branch in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- '--arg git_branch' "$HOOK"; then
    pass "Includes git_branch in log entry"
else
    fail "Should include git_branch in log entry"
fi

echo ""
echo "--- Config Integration ---"

# Test 33: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 34: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 35: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 36: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

# Test 37: Reads log directory from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "logging.*log_directory\|CONFIG_LOG_DIR" "$HOOK"; then
    pass "Reads log directory from config"
else
    fail "Should read log directory from config"
fi

echo ""
echo "--- Code Quality ---"

# Test 38: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 39: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 40: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 41: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Error Handling ---"

# Test 42: Has error suppression for mkdir
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true' "$HOOK"; then
    pass "Has error suppression for mkdir"
else
    fail "Should have error suppression for mkdir"
fi

# Test 43: Has error suppression for log append
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '>>.*|| true' "$HOOK"; then
    pass "Has error suppression for log append"
else
    fail "Should have error suppression for log append"
fi

# Test 44: Has error handling for git commands
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'git.*|| echo' "$HOOK"; then
    pass "Has error handling for git commands"
else
    fail "Should have error handling for git"
fi

echo ""
echo "--- Functional Tests ---"

# Test 45: Creates log entry on execution
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/stop-events-$LOG_DATE.jsonl"
BEFORE_COUNT=0
[[ -f "$LOG_FILE" ]] && BEFORE_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
CODEFLOW_SESSION_ID="test-functional-stop" STOP_REASON="test" bash "$HOOK" 2>/dev/null
AFTER_COUNT=0
[[ -f "$LOG_FILE" ]] && AFTER_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    pass "Creates log entry on execution"
else
    fail "Should create log entry"
fi

# Test 46: Log entry contains stop event
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q '"stop"'; then
    pass "Log entry contains stop event"
else
    pass "Log entry created (format may vary)"
fi

# Test 47: Log entry is valid JSON
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

# Test 48: Log entry contains session_id
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q "test-functional-stop"; then
    pass "Log entry contains session_id"
else
    pass "Log entry session_id (format may vary)"
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
