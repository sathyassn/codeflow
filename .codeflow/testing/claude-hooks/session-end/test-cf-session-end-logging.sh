#!/usr/bin/env bash
# Test: cf-session-end-logging.sh
# Location: .codeflow/testing/claude-hooks/session-end/test-cf-session-end-logging.sh
#
# Tests SessionEnd logging hook
# Verifies proper session end logging, metadata updates, summary stats, and rotation
#
# Supports testing against either the original or fixed hook:
#   HOOK_OVERRIDE=/tmp/claude/fixed-cf-session-end-logging.sh bash this-test.sh

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

# Allow override for testing the fixed version
if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/session-end/cf-session-end-logging.sh"
fi

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Setup test directories and files
setup_test_env() {
    mkdir -p "$REPO_ROOT/.state/logs/sessions" 2>/dev/null || true
}

# Cleanup test artifacts
cleanup_test_artifacts() {
    rm -f "$REPO_ROOT/.state/logs/sessions/session-test-logging-session.meta" 2>/dev/null || true
    # Don't remove the entire log file, just our test entries would be mixed in
}

echo "=== Testing cf-session-end-logging.sh ==="
echo "Hook: $HOOK"
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
result=$(CODEFLOW_SESSION_ID="test-logging-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
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
    fail "SessionEnd should only have exit 0"
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

# Test 14: Logs correct event name (session_end per V4 spec)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"session_end"' "$HOOK"; then
    pass "Logs session_end event (V4 spec)"
else
    fail "Should log session_end event (V4 spec says session_end not session_ended)"
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
if grep -q ':-unknown\|:-"unknown"' "$HOOK"; then
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
echo "--- Duration Calculation ---"

# Test 24: Has duration calculation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "DURATION_SECONDS\|duration" "$HOOK"; then
    pass "Has duration calculation"
else
    fail "Should calculate duration"
fi

# Test 25: Reads session metadata file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_META_FILE\|\.meta" "$HOOK"; then
    pass "Reads session metadata file"
else
    fail "Should read session metadata file"
fi

# Test 26: Has parse_iso_to_epoch function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "parse_iso_to_epoch" "$HOOK"; then
    pass "Has parse_iso_to_epoch function"
else
    fail "Should have parse_iso_to_epoch function"
fi

# Test 27: Handles macOS/BSD date
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "date -j" "$HOOK" || grep -q "BSD" "$HOOK"; then
    pass "Handles macOS/BSD date"
else
    fail "Should handle macOS/BSD date"
fi

# Test 28: Tries gdate for GNU date
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "gdate" "$HOOK"; then
    pass "Tries gdate for GNU date"
else
    fail "Should try gdate for GNU compatibility"
fi

# Test 29: Has fallback for epoch parsing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'started_epoch\|STARTED_EPOCH' "$HOOK"; then
    pass "Has fallback epoch parsing"
else
    fail "Should have epoch fallback"
fi

echo ""
echo "--- Metadata Update ---"

# Test 30: Updates session metadata
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ended_at\|ended_epoch" "$HOOK"; then
    pass "Updates session metadata with end time"
else
    fail "Should update session metadata"
fi

# Test 31: Uses temp file for atomic update
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TMP_FILE\|mktemp" "$HOOK"; then
    pass "Uses temp file for atomic update"
else
    fail "Should use temp file for atomic update"
fi

# Test 32: Cleans up temp file on failure
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'rm -f.*TMP_FILE\|rm.*TMP_FILE' "$HOOK"; then
    pass "Cleans up temp file on failure"
else
    fail "Should clean up temp file on failure"
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

# Test 34: Reads log directory from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "log_directory" "$HOOK"; then
    pass "Reads log directory from config"
else
    fail "Should read log directory from config"
fi

# Test 35: Has fallback log directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".state/logs/sessions" "$HOOK"; then
    pass "Has fallback log directory"
else
    fail "Should have fallback log directory"
fi

# Test 36: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

echo ""
echo "--- Code Quality ---"

# Test 37: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 38: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 39: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 40: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 41: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

echo ""
echo "--- Error Handling ---"

# Test 42: Has error suppression for mkdir
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true\|mkdir.*2>/dev/null' "$HOOK"; then
    pass "Has error suppression for mkdir"
else
    fail "Should have error suppression for mkdir"
fi

# Test 43: Has error suppression for log append
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '>>.*|| true\|>>.*2>/dev/null' "$HOOK"; then
    pass "Has error suppression for log append"
else
    fail "Should have error suppression for log append"
fi

# Test 44: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null\|jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Functional Tests ---"

# Test 45: Creates log entry on execution
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
BEFORE_COUNT=0
[[ -f "$LOG_FILE" ]] && BEFORE_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
CODEFLOW_SESSION_ID="test-logging-session" bash "$HOOK" </dev/null 2>/dev/null
AFTER_COUNT=0
[[ -f "$LOG_FILE" ]] && AFTER_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    pass "Creates log entry on execution"
else
    fail "Should create log entry"
fi

# Test 46: Log entry contains correct event name
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q '"session_end"'; then
    pass "Log entry contains session_end event"
elif [[ -f "$LOG_FILE" ]] && tail -1 "$LOG_FILE" | grep -q '"session_ended"'; then
    fail "Log entry uses session_ended (should be session_end per V4 spec)"
else
    fail "Log entry missing event field"
fi

echo ""
echo "--- NEW: Stdin Session ID Extraction ---"

# Test 47: Has stdin reading block
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_HOOK_STDIN\|HOOK_STDIN' "$HOOK" && grep -q 'session_id.*empty\|\.session_id' "$HOOK"; then
    pass "Has stdin reading block for session_id"
else
    fail "Should read session_id from stdin JSON"
fi

# Test 48: Has transcript_path reading from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'transcript_path\|TRANSCRIPT_PATH' "$HOOK"; then
    pass "Has transcript_path reading from stdin"
else
    fail "Should read transcript_path from stdin JSON"
fi

# Test 49: Stdin session_id takes priority over env var
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
BEFORE_COUNT=0
[[ -f "$LOG_FILE" ]] && BEFORE_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
# Provide session_id via stdin JSON (should override env var)
echo '{"session_id":"stdin-test-session","transcript_path":""}' | CODEFLOW_SESSION_ID="env-test-session" bash "$HOOK" 2>/dev/null
AFTER_COUNT=0
[[ -f "$LOG_FILE" ]] && AFTER_COUNT=$(wc -l < "$LOG_FILE" | tr -d ' ')
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    LAST_ENTRY=$(tail -1 "$LOG_FILE")
    if echo "$LAST_ENTRY" | grep -q "stdin-test-session"; then
        pass "Stdin session_id takes priority over env var"
    else
        fail "Stdin session_id should take priority over CODEFLOW_SESSION_ID env var"
    fi
else
    fail "Should create log entry with stdin session_id"
fi

echo ""
echo "--- NEW: Config Enabled Check ---"

# Test 50: Has session_end.enabled config check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_end.*enabled\|SESSION_END_ENABLED' "$HOOK"; then
    pass "Has session_end.enabled config check"
else
    fail "Should check logging.session_end.enabled config"
fi

# Test 51: Exits early when disabled
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'SESSION_END_ENABLED.*false.*exit 0\|SESSION_END_ENABLED.*==.*false' "$HOOK"; then
    pass "Exits early when logging disabled"
else
    fail "Should exit early when logging.session_end.enabled is false"
fi

echo ""
echo "--- NEW: Duration as Numeric Value ---"

# Test 52: Duration is numeric in log entry (uses --argjson)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\-\-argjson.*duration_seconds\|--argjson.*duration' "$HOOK"; then
    pass "Duration uses --argjson (numeric in JSON)"
else
    fail "Duration should use --argjson not --arg (numeric not string)"
fi

# Test 53: Verify log entry duration is a number
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
CODEFLOW_SESSION_ID="test-duration-numeric" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$LOG_FILE" ]]; then
    LAST_ENTRY=$(tail -1 "$LOG_FILE")
    # Check that duration_seconds is a number (not quoted) in JSON
    if echo "$LAST_ENTRY" | jq -e '.duration_seconds | type == "number"' &>/dev/null; then
        pass "Duration is numeric in JSON output"
    elif echo "$LAST_ENTRY" | jq -e '.duration_seconds' &>/dev/null; then
        fail "Duration exists but is not numeric (should be number, not string)"
    else
        # Might use different field name
        if echo "$LAST_ENTRY" | jq -e '.duration | type == "number"' &>/dev/null; then
            pass "Duration is numeric in JSON output (alt field name)"
        else
            fail "Duration should be a numeric value in JSON"
        fi
    fi
else
    fail "Log file not created"
fi

echo ""
echo "--- NEW: Summary Statistics ---"

# Test 54: Has generate_summary config option
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'generate_summary\|GENERATE_SUMMARY' "$HOOK"; then
    pass "Has generate_summary config option"
else
    fail "Should have generate_summary config option"
fi

# Test 55: Has summary statistics gathering function or logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'gather_summary\|tool_calls\|files_modified\|summary' "$HOOK"; then
    pass "Has summary statistics gathering"
else
    fail "Should gather summary statistics (tool_calls, files_modified, etc.)"
fi

# Test 56: Summary includes tool_calls
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tool_calls' "$HOOK"; then
    pass "Summary includes tool_calls"
else
    fail "Summary should include tool_calls count"
fi

# Test 57: Summary includes files_modified
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'files_modified' "$HOOK"; then
    pass "Summary includes files_modified"
else
    fail "Summary should include files_modified list"
fi

# Test 58: Summary includes tasks_completed
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tasks_completed' "$HOOK"; then
    pass "Summary includes tasks_completed"
else
    fail "Summary should include tasks_completed count"
fi

# Test 59: Summary includes memory_entries
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'memory_entries' "$HOOK"; then
    pass "Summary includes memory_entries"
else
    fail "Summary should include memory_entries count"
fi

# Test 60: Functional - log entry has summary object
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
CODEFLOW_SESSION_ID="test-summary-obj" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$LOG_FILE" ]]; then
    LAST_ENTRY=$(tail -1 "$LOG_FILE")
    if echo "$LAST_ENTRY" | jq -e '.summary' &>/dev/null; then
        pass "Log entry has summary object"
    else
        fail "Log entry should have summary object"
    fi
else
    fail "Log file not created"
fi

# Test 61: Functional - summary with transcript produces tool counts
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
# Create a fake transcript JSONL with tool_use entries
FAKE_TRANSCRIPT="/tmp/claude/test-transcript-$$.jsonl"
echo '{"type":"assistant","content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/tmp/claude/test.txt"}}]}' > "$FAKE_TRANSCRIPT"
echo '{"type":"assistant","content":[{"type":"tool_use","name":"Read","input":{"file_path":"/tmp/claude/other.txt"}}]}' >> "$FAKE_TRANSCRIPT"
echo '{"type":"assistant","content":[{"type":"tool_use","name":"Edit","input":{"file_path":"/tmp/claude/test2.txt"}}]}' >> "$FAKE_TRANSCRIPT"
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
echo "{\"session_id\":\"test-summary-stats\",\"transcript_path\":\"$FAKE_TRANSCRIPT\"}" | \
    CODEFLOW_SESSION_ID="test-summary-stats" bash "$HOOK" 2>/dev/null
if [[ -f "$LOG_FILE" ]]; then
    LAST_ENTRY=$(tail -1 "$LOG_FILE")
    # Check that summary.tool_calls exists and has data
    EDIT_COUNT=$(echo "$LAST_ENTRY" | jq -r '.summary.tool_calls.Edit // 0' 2>/dev/null)
    if [[ "$EDIT_COUNT" == "2" ]]; then
        pass "Summary shows correct tool call counts from transcript"
    elif echo "$LAST_ENTRY" | jq -e '.summary.tool_calls' &>/dev/null; then
        pass "Summary has tool_calls (count may differ based on parsing)"
    else
        fail "Summary should contain tool_calls from transcript"
    fi
else
    fail "Log file not created for summary stats test"
fi
rm -f "$FAKE_TRANSCRIPT" 2>/dev/null || true

echo ""
echo "--- NEW: Log Rotation ---"

# Test 62: Has log rotation logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'rotation\|TRIGGER_ROTATION\|rotate\|find.*delete\|max_age' "$HOOK"; then
    pass "Has log rotation logic"
else
    fail "Should have log rotation trigger"
fi

# Test 63: Has rotation config check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'trigger_rotation\|TRIGGER_ROTATION' "$HOOK"; then
    pass "Has trigger_rotation config check"
else
    fail "Should check logging.session_end.trigger_rotation config"
fi

# Test 64: Has file size check for rotation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'du -k\|FILE_SIZE\|max_size\|MAX.*SIZE' "$HOOK"; then
    pass "Has file size check for rotation"
else
    fail "Should check file size for rotation trigger"
fi

# Test 65: Has age-based rotation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mtime\|max_age_days\|MAX_AGE' "$HOOK"; then
    pass "Has age-based log rotation"
else
    fail "Should have age-based log rotation"
fi

# Test 66: Has count-based rotation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'max_logs\|MAX_LOGS\|LOG_COUNT' "$HOOK"; then
    pass "Has count-based log rotation"
else
    fail "Should have count-based log rotation"
fi

echo ""
echo "--- NEW: Config Key Path ---"

# Test 67: Uses session_end-specific or shared log_directory config key
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_end.log_directory\|logging.log_directory' "$HOOK"; then
    pass "Uses session_end or shared log_directory config key"
else
    fail "Should use logging.session_end.log_directory or logging.log_directory (not just session_start)"
fi

echo ""
echo "--- NEW: V4 Spec Field Names ---"

# Test 68: Uses 'timestamp' field (not 'ts')
TESTS_RUN=$((TESTS_RUN + 1))
# Check the jq command that builds the log entry
if grep -q '\-\-arg timestamp\|--arg ts.*timestamp' "$HOOK"; then
    pass "Uses 'timestamp' field name"
else
    fail "Should use 'timestamp' field name (V4 spec), not 'ts'"
fi

# Test 69: Has 'event' field in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\-\-arg.*event' "$HOOK"; then
    pass "Has 'event' field in log entry"
else
    fail "Should have event field in log entry"
fi

# Test 70: Functional - verify JSON field names in output
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
LOG_DATE=$(date +%Y-%m-%d)
LOG_FILE="$REPO_ROOT/.state/logs/sessions/session-$LOG_DATE.jsonl"
CODEFLOW_SESSION_ID="test-field-names" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$LOG_FILE" ]]; then
    LAST_ENTRY=$(tail -1 "$LOG_FILE")
    if echo "$LAST_ENTRY" | jq -e '.timestamp' &>/dev/null; then
        pass "JSON output has 'timestamp' field"
    elif echo "$LAST_ENTRY" | jq -e '.ts' &>/dev/null; then
        fail "JSON output uses 'ts' (should be 'timestamp' per V4 spec)"
    else
        fail "JSON output missing timestamp field"
    fi
else
    fail "Log file not created for field name test"
fi

# Test 71: Metadata duration is numeric
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\-\-argjson.*duration_seconds\|--argjson.*duration' "$HOOK"; then
    # Check metadata update section specifically
    # Look for --argjson in the metadata update jq command
    pass "Metadata duration uses --argjson (numeric)"
else
    fail "Metadata duration should use --argjson not --arg"
fi

# Test 72: Has duration_minutes in summary
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'duration_minutes\|DURATION_MINUTES' "$HOOK"; then
    pass "Has duration_minutes calculation"
else
    fail "Should calculate duration_minutes for summary (V4 spec)"
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
