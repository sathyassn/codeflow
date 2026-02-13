#!/usr/bin/env bash
# Test: cf-session-start-logging.sh (v2.0.0)
# Location: .codeflow/testing/claude-hooks/session-start/test-cf-session-start-logging.sh
#
# Tests SessionStart logging hook v2.0.0
# Verifies: stdin reading, config checks, V4 event format, metadata, rotation, current-session.txt

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-logging.sh"
fi

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
echo "Hook: $HOOK"
echo ""

# =========================================================================
# SECTION 1: File Structure Tests
# =========================================================================

# Test 1: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found at $HOOK"; fi

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
result=$(echo '{}' | CODEFLOW_SESSION_ID="test-logging-session" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Exits 0 without session ID
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
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
echo "--- Stdin Reading (NEW) ---"

# Test 13: Has stdin reading block
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_HOOK_STDIN\|_sid' "$HOOK" && grep -q '! -t 0' "$HOOK"; then
    pass "Has stdin reading block"
else
    fail "Should read session_id from stdin"
fi

# Test 14: Parses session_id from stdin JSON
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_id // empty' "$HOOK"; then
    pass "Parses session_id from stdin JSON"
else
    fail "Should parse session_id from stdin JSON"
fi

# Test 15: Stdin session_id takes priority over env var
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
echo '{"session_id":"stdin-priority-test"}' | CODEFLOW_SESSION_ID="env-session" bash "$HOOK" 2>/dev/null
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q "stdin-priority-test"; then
    pass "Stdin session_id takes priority over env var"
else
    fail "Stdin session_id should take priority"
fi

echo ""
echo "--- V4 Event Format (NEW) ---"

# Test 16: Event name is session_start (not session_started)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"session_start"' "$HOOK"; then
    pass "Event name is session_start"
else
    fail "Event name should be session_start (not session_started)"
fi

# Test 17: No session_started event name
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"session_started"' "$HOOK" 2>/dev/null; then
    fail "Should NOT use session_started event name"
else
    pass "Does not use legacy session_started name"
fi

# Test 18: Uses timestamp field name (not ts)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"timestamp"' "$HOOK" || grep -q '\-\-arg timestamp' "$HOOK"; then
    pass "Uses timestamp field name"
else
    fail "Should use timestamp field name (not ts)"
fi

# Test 19: Does not use ts field name
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE '\-\-arg ts ' "$HOOK" 2>/dev/null; then
    fail "Should NOT use legacy ts field name"
else
    pass "Does not use legacy ts field name"
fi

echo ""
echo "--- Logging Logic ---"

# Test 20: Has logging logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "log|LOG|jsonl|JSONL" "$HOOK"; then
    pass "Has logging logic"
else
    fail "Should have logging"
fi

# Test 21: Has LOG_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_DIR" "$HOOK"; then
    pass "Has LOG_DIR variable"
else
    fail "Should have LOG_DIR variable"
fi

# Test 22: Has LOG_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG_FILE" "$HOOK"; then
    pass "Has LOG_FILE variable"
else
    fail "Should have LOG_FILE variable"
fi

# Test 23: Creates JSONL log entries
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".jsonl" "$HOOK"; then
    pass "Creates JSONL log entries"
else
    fail "Should create JSONL log entries"
fi

# Test 24: Uses jq for JSON creation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then
    pass "Uses jq for JSON creation"
else
    fail "Should use jq for JSON creation"
fi

# Test 25: Creates log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir -p.*LOG_DIR" "$HOOK"; then
    pass "Creates log directory"
else
    fail "Should create log directory"
fi

echo ""
echo "--- Session ID Handling ---"

# Test 26: Has SESSION_ID variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_ID" "$HOOK"; then
    pass "Has SESSION_ID variable"
else
    fail "Should have SESSION_ID variable"
fi

# Test 27: Reads SESSION_ID from environment
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CODEFLOW_SESSION_ID" "$HOOK"; then
    pass "Reads SESSION_ID from environment"
else
    fail "Should read SESSION_ID from environment"
fi

# Test 28: Has fallback for missing SESSION_ID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ':-unknown' "$HOOK" || grep -q 'unknown' "$HOOK"; then
    pass "Has fallback for missing SESSION_ID"
else
    fail "Should have fallback for missing SESSION_ID"
fi

# Test 29: Includes session_id in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_id.*SESSION_ID\|--arg session_id' "$HOOK"; then
    pass "Includes session_id in log entry"
else
    fail "Should include session_id in log entry"
fi

echo ""
echo "--- Metadata Object (NEW) ---"

# Test 30: Has metadata object in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'metadata' "$HOOK" && grep -q 'git_branch' "$HOOK"; then
    pass "Has metadata object in log entry"
else
    fail "Should have metadata object in log entry"
fi

# Test 31: Metadata includes git_branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'git_branch' "$HOOK"; then
    pass "Metadata includes git_branch"
else
    fail "Should include git_branch in metadata"
fi

# Test 32: Metadata includes git_commit
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'git_commit' "$HOOK"; then
    pass "Metadata includes git_commit"
else
    fail "Should include git_commit in metadata"
fi

# Test 33: Metadata includes approval_mode
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'approval_mode' "$HOOK"; then
    pass "Metadata includes approval_mode"
else
    fail "Should include approval_mode in metadata"
fi

# Test 34: Metadata includes active_task
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'active_task' "$HOOK"; then
    pass "Metadata includes active_task"
else
    fail "Should include active_task in metadata"
fi

# Test 35: Metadata includes cwd
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"cwd"' "$HOOK" || grep -q '\-\-arg cwd' "$HOOK"; then
    pass "Metadata includes cwd"
else
    fail "Should include cwd in metadata"
fi

# Test 36: Has CAPTURE_METADATA config check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CAPTURE_METADATA' "$HOOK" && grep -q 'capture_metadata' "$HOOK"; then
    pass "Has CAPTURE_METADATA config check"
else
    fail "Should check capture_metadata config"
fi

echo ""
echo "--- Config Enabled Check (NEW) ---"

# Test 37: Has enabled config check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'LOGGING_ENABLED' "$HOOK" && grep -q 'logging.session_start.enabled' "$HOOK"; then
    pass "Has enabled config check"
else
    fail "Should check logging.session_start.enabled config"
fi

# Test 38: Exits early when disabled
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'LOGGING_ENABLED.*false' "$HOOK" && grep -q 'exit 0' "$HOOK"; then
    pass "Exits early when disabled"
else
    fail "Should exit early when disabled"
fi

echo ""
echo "--- Config-Driven Log Directory (NEW) ---"

# Test 39: Reads log_directory from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'logging.session_start.log_directory' "$HOOK"; then
    pass "Reads log_directory from config"
else
    fail "Should read log_directory from config"
fi

# Test 40: Has log_directory fallback chain
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'logging.session_start.log_directory' "$HOOK" && grep -q 'LOG_DIR=.*state/logs/sessions' "$HOOK"; then
    pass "Has log_directory fallback chain"
else
    fail "Should have fallback chain for log_directory"
fi

echo ""
echo "--- Current Session State File (NEW) ---"

# Test 41: Writes current-session.txt
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'current-session.txt' "$HOOK"; then
    pass "Writes current-session.txt"
else
    fail "Should write current-session.txt"
fi

# Test 42: Creates state directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'managed/state\|STATE_DIR' "$HOOK"; then
    pass "Creates state directory"
else
    fail "Should create state directory"
fi

# Test 43: Functional: current-session.txt has session ID
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
STATE_FILE="$REPO_ROOT/.state/session/state-file-test/current-session.txt"
rm -f "$STATE_FILE" 2>/dev/null
echo '{"session_id":"state-file-test"}' | REPO_ROOT="$REPO_ROOT" bash "$HOOK" 2>/dev/null
if [[ -f "$STATE_FILE" ]]; then
    STATE_CONTENT=$(cat "$STATE_FILE")
    if [[ "$STATE_CONTENT" == "state-file-test" ]]; then
        pass "current-session.txt has correct session ID"
    else
        fail "current-session.txt has wrong content: $STATE_CONTENT"
    fi
else
    fail "current-session.txt not created"
fi

echo ""
echo "--- Log Rotation ---"

# Test 44: Has log rotation logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "LOG ROTATION" "$HOOK"; then
    pass "Has log rotation logic"
else
    fail "Should have log rotation"
fi

# Test 45: Has MAX_LOGS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MAX_LOGS" "$HOOK"; then
    pass "Has MAX_LOGS variable"
else
    fail "Should have MAX_LOGS variable"
fi

# Test 46: Has MAX_AGE_DAYS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MAX_AGE_DAYS" "$HOOK"; then
    pass "Has MAX_AGE_DAYS variable"
else
    fail "Should have MAX_AGE_DAYS variable"
fi

# Test 47: Uses find for age-based cleanup
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "find.*mtime" "$HOOK"; then
    pass "Uses find for age-based cleanup"
else
    fail "Should use find for age-based cleanup"
fi

# Test 48: Has portable log rotation (no GNU -printf in find command)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ls -1t" "$HOOK" && ! grep -qE "find.*-printf" "$HOOK"; then
    pass "Has portable log rotation (no GNU -printf)"
else
    fail "Should use portable log rotation"
fi

# Test 49: Handles excess logs
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "EXCESS" "$HOOK"; then
    pass "Handles excess logs"
else
    fail "Should handle excess logs"
fi

echo ""
echo "--- Config Integration ---"

# Test 50: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 51: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 52: Reads max_logs from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "logging.session_start.rotation.max_logs" "$HOOK"; then
    pass "Reads max_logs from config"
else
    fail "Should read max_logs from config"
fi

# Test 53: Reads max_age_days from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "logging.session_start.rotation.max_age_days" "$HOOK"; then
    pass "Reads max_age_days from config"
else
    fail "Should read max_age_days from config"
fi

# Test 54: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 55: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

# Test 56: Has fallback values for config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '// 50' "$HOOK" && grep -q '// 7' "$HOOK"; then
    pass "Has fallback values for config"
else
    fail "Should have fallback values"
fi

echo ""
echo "--- Code Quality ---"

# Test 57: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 58: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 59: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 60: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Error Handling ---"

# Test 61: Has error suppression for mkdir
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true' "$HOOK"; then
    pass "Has error suppression for mkdir"
else
    fail "Should have error suppression for mkdir"
fi

# Test 62: Has error suppression for log append
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '>>.*|| true' "$HOOK"; then
    pass "Has error suppression for log append"
else
    fail "Should have error suppression for log append"
fi

# Test 63: Has error suppression for find/rm
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'find.*|| true\|xargs.*|| true' "$HOOK"; then
    pass "Has error suppression for find/rm"
else
    fail "Should have error suppression for cleanup"
fi

echo ""
echo "--- V4 Consistency with Session-End ---"

# Test 64: Version is 2.0.0
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '2.0.0' "$HOOK"; then
    pass "Version is 2.0.0"
else
    fail "Should be version 2.0.0"
fi

# Test 65: Log entry field order matches V4 (event, session_id, timestamp, metadata)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'timestamp.*session_id.*event.*metadata' "$HOOK"; then
    pass "Log entry field order matches V4 spec"
else
    fail "Field order should be: timestamp, session_id, event, metadata"
fi

echo ""
echo "--- Functional Tests ---"

# Test 66: Creates log entry on execution
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
BEFORE_COUNT=0
[[ -f "$LOG_FILE" ]] && BEFORE_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
echo '{}' | CODEFLOW_SESSION_ID="test-functional-session" bash "$HOOK" 2>/dev/null
AFTER_COUNT=0
[[ -f "$LOG_FILE" ]] && AFTER_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    pass "Creates log entry on execution"
else
    fail "Should create log entry"
fi

# Test 67: Log entry contains session_start event
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q '"session_start"'; then
    pass "Log entry contains session_start event"
else
    fail "Log entry should contain session_start event"
fi

# Test 68: Log entry is valid JSON
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

# Test 69: Log entry has timestamp field
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | jq -e '.timestamp' &>/dev/null; then
    pass "Log entry has timestamp field"
else
    fail "Log entry should have timestamp field"
fi

# Test 70: Log entry has metadata object
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | jq -e '.metadata' &>/dev/null; then
    pass "Log entry has metadata object"
else
    fail "Log entry should have metadata object"
fi

# Test 71: Metadata has git_branch field
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | jq -e '.metadata.git_branch' &>/dev/null; then
    pass "Metadata has git_branch field"
else
    fail "Metadata should have git_branch field"
fi

# Test 72: Metadata has git_commit field
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | jq -e '.metadata.git_commit' &>/dev/null; then
    pass "Metadata has git_commit field"
else
    fail "Metadata should have git_commit field"
fi

# Test 73: Metadata has approval_mode field
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | jq -e '.metadata.approval_mode' &>/dev/null; then
    pass "Metadata has approval_mode field"
else
    fail "Metadata should have approval_mode field"
fi

# Test 74: Metadata has active_task field (null)
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | jq -e 'has("metadata") and (.metadata | has("active_task"))' &>/dev/null; then
    pass "Metadata has active_task field"
else
    fail "Metadata should have active_task field"
fi

# Test 75: Stdin-provided session ID appears in log entry
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
echo '{"session_id":"functional-stdin-test"}' | REPO_ROOT="$REPO_ROOT" bash "$HOOK" 2>/dev/null
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q "functional-stdin-test"; then
    pass "Stdin session ID appears in log entry"
else
    fail "Stdin session ID should appear in log entry"
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
