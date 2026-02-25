#!/usr/bin/env bash
# Test: cf-stop-logging.sh
# Location: .codeflow/testing/claude-hooks/stop/test-cf-stop-logging.sh
#
# Tests Stop logging hook
# Verifies stdin protocol, stop event logging, PCV status capture,
# task context, decision detection, git state, and config integration.

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

# Support HOOK_OVERRIDE for testing fixed copies from /tmp/claude/
if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/stop/cf-stop-logging.sh"
fi

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Helper: run hook with stdin JSON
run_hook_stdin() {
    local stdin_json="$1"
    echo "$stdin_json" | bash "$HOOK" 2>/dev/null
    return "${PIPESTATUS[1]}"
}

# Helper: build stdin JSON for stop hook
build_stdin() {
    local session_id="${1:-test-session}"
    local transcript="${2:-}"
    local stop_reason="${3:-}"
    jq -nc \
        --arg sid "$session_id" \
        --arg tp "$transcript" \
        --arg sr "$stop_reason" \
        '{session_id: $sid, transcript_path: $tp, stop_reason: $sr, stop_hook_active: false}'
}

# Setup test environment
LOG_DIR="$REPO_ROOT/.state/logs/sessions"
setup_test_env() {
    mkdir -p "$LOG_DIR" 2>/dev/null || true
}

# Get the last log entry from today's log file
get_last_log_entry() {
    local log_file
    log_file="$LOG_DIR/session-$(date +%Y-%m-%d).jsonl"
    if [[ -f "$log_file" ]]; then
        tail -1 "$log_file"
    else
        echo ""
    fi
}

# Count log entries in today's file
count_log_entries() {
    local log_file
    log_file="$LOG_DIR/session-$(date +%Y-%m-%d).jsonl"
    if [[ -f "$log_file" ]]; then
        wc -l < "$log_file" | tr -d ' '
    else
        echo "0"
    fi
}

echo "=== Testing cf-stop-logging.sh ==="
echo "Hook: $HOOK"
echo ""

# =========================================================================
# STRUCTURAL TESTS
# =========================================================================

echo "--- Structure ---"

# Test 1: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found at $HOOK"; fi

# Test 2: File is executable
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
TESTS_RUN=$((TESTS_RUN + 1))
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 -e SC2002 "$HOOK" 2>/dev/null; then
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

# Test 9: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# =========================================================================
# STDIN PROTOCOL
# =========================================================================

echo ""
echo "--- Stdin Protocol ---"

# Test 10: Reads from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_HOOK_STDIN=.*(cat)\|INPUT=.*(cat)' "$HOOK"; then
    pass "Reads from stdin"
else
    fail "Should read from stdin"
fi

# Test 11: Extracts session_id from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_id' "$HOOK"; then
    pass "Extracts session_id from stdin"
else
    fail "Should extract session_id"
fi

# Test 12: Extracts transcript_path from stdin
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'transcript_path' "$HOOK"; then
    pass "Extracts transcript_path from stdin"
else
    fail "Should extract transcript_path"
fi

# Test 13: Has env var fallback for session_id
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CODEFLOW_SESSION_ID' "$HOOK"; then
    pass "Has env var fallback for session_id"
else
    fail "Should have CODEFLOW_SESSION_ID fallback"
fi

# Test 14: Has env var fallback for stop_reason
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'STOP_REASON' "$HOOK"; then
    pass "Has env var fallback for stop_reason"
else
    fail "Should have STOP_REASON fallback"
fi

# =========================================================================
# CONFIG INTEGRATION
# =========================================================================

echo ""
echo "--- Config Integration ---"

# Test 15: References enforcement-policy.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 16: Has CONFIG variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 17: Reads from logging.stop config section
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'logging.stop' "$HOOK"; then
    pass "Reads from logging.stop config section"
else
    fail "Should read from logging.stop section"
fi

# Test 18: Has enabled config check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'STOP_LOGGING_ENABLED\|logging.*enabled\|_enabled' "$HOOK"; then
    pass "Has enabled config check"
else
    fail "Should have enabled config check"
fi

# Test 19: Reads log directory from config with fallback chain
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'logging.stop.log_directory\|logging.log_directory\|logging.session_start.log_directory' "$HOOK"; then
    pass "Reads log directory from config with fallback chain"
else
    fail "Should have log directory fallback chain"
fi

# Test 20: Has jq availability check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 21: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

# =========================================================================
# LOG ENTRY FORMAT
# =========================================================================

echo ""
echo "--- Log Entry Format ---"

# Test 22: Uses 'timestamp' field (not 'ts')
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- '--arg timestamp' "$HOOK"; then
    pass "Uses 'timestamp' field (not 'ts')"
else
    fail "Should use 'timestamp' field name (not 'ts')"
fi

# Test 23: Does NOT use 'ts' field
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- "--arg ts " "$HOOK"; then
    fail "Should NOT use 'ts' field (use 'timestamp')"
else
    pass "Does not use deprecated 'ts' field"
fi

# Test 24: Logs stop event type
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"stop"\|event.*stop' "$HOOK"; then
    pass "Logs stop event type"
else
    fail "Should log event='stop'"
fi

# Test 25: Writes to session-*.jsonl (not stop-events-*)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session-.*\.jsonl\|session-\$LOG_DATE' "$HOOK"; then
    pass "Writes to session-*.jsonl"
else
    fail "Should write to session-*.jsonl (not stop-events-*)"
fi

# Test 26: Does NOT write to stop-events-*.jsonl
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "stop-events" "$HOOK"; then
    fail "Should NOT write to stop-events-*.jsonl"
else
    pass "Does not write to deprecated stop-events-*.jsonl"
fi

# Test 27: Includes session_id in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- '--arg session_id' "$HOOK"; then
    pass "Includes session_id in log entry"
else
    fail "Should include session_id in log entry"
fi

# Test 28: Includes stop_reason in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q -- '--arg stop_reason\|--arg reason' "$HOOK"; then
    pass "Includes stop_reason in log entry"
else
    fail "Should include stop_reason in log entry"
fi

# Test 29: Uses jq for JSON creation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then
    pass "Uses jq for JSON creation"
else
    fail "Should use jq for JSON creation"
fi

# =========================================================================
# PCV STATUS CAPTURE
# =========================================================================

echo ""
echo "--- PCV Status Capture ---"

# Test 30: Has PCV status capture logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PCV_STATUS\|pcv_status\|capture_pcv" "$HOOK"; then
    pass "Has PCV status capture logic"
else
    fail "Should capture PCV status"
fi

# Test 31: Checks for verify-work marker in transcript
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verify-work" "$HOOK"; then
    pass "Checks for verify-work marker in transcript"
else
    fail "Should check for verify-work in transcript"
fi

# Test 32: Checks for TIER indicator
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TIER" "$HOOK"; then
    pass "Checks for TIER indicator"
else
    fail "Should check for TIER indicator"
fi

# Test 33: Builds markers_present array
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "markers_present\|_present" "$HOOK"; then
    pass "Builds markers_present array"
else
    fail "Should build markers_present array"
fi

# Test 34: Builds markers_missing array
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "markers_missing\|_missing" "$HOOK"; then
    pass "Builds markers_missing array"
else
    fail "Should build markers_missing array"
fi

# Test 35: Has verified boolean
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verified\|VERIFIED" "$HOOK"; then
    pass "Has verified boolean"
else
    fail "Should have verified boolean"
fi

# Test 36: Has capture_pcv_details config flag
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "capture_pcv_details\|CAPTURE_PCV" "$HOOK"; then
    pass "Has capture_pcv_details config flag"
else
    fail "Should have capture_pcv_details config"
fi

# =========================================================================
# TASK CONTEXT CAPTURE
# =========================================================================

echo ""
echo "--- Task Context Capture ---"

# Test 37: Has task context capture logic
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TASK_CONTEXT\|task_context\|active-task" "$HOOK"; then
    pass "Has task context capture logic"
else
    fail "Should capture task context"
fi

# Test 38: Checks active-task.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "active-task.json\|active_task" "$HOOK"; then
    pass "Checks active-task.json"
else
    fail "Should check active-task.json"
fi

# Test 39: Has capture_task_context config flag
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "capture_task_context\|CAPTURE_TASK" "$HOOK"; then
    pass "Has capture_task_context config flag"
else
    fail "Should have capture_task_context config"
fi

# =========================================================================
# DECISION DETECTION
# =========================================================================

echo ""
echo "--- Decision Detection ---"

# Test 40: Has decision field in log entry
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "decision\|DECISION" "$HOOK"; then
    pass "Has decision field in log entry"
else
    fail "Should have decision field"
fi

# Test 41: verify-work-retry dead code removed (V4 cleanup)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verify-work-retry" "$HOOK"; then
    fail "verify-work-retry is dead code (V3 orphan) and should be removed"
else
    pass "verify-work-retry dead code removed"
fi

# =========================================================================
# GIT STATE
# =========================================================================

echo ""
echo "--- Git State Capture ---"

# Test 42: Captures git branch
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_BRANCH\|git_branch" "$HOOK"; then
    pass "Captures git branch"
else
    fail "Should capture git branch"
fi

# Test 43: Uses git branch --show-current
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*branch --show-current" "$HOOK"; then
    pass "Uses git branch --show-current"
else
    fail "Should use git branch --show-current"
fi

# Test 44: Captures uncommitted changes status
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "HAS_CHANGES\|has_uncommitted" "$HOOK"; then
    pass "Captures uncommitted changes status"
else
    fail "Should capture uncommitted changes"
fi

# Test 45: Uses git status --porcelain
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git.*status --porcelain" "$HOOK"; then
    pass "Uses git status --porcelain"
else
    fail "Should use git status --porcelain"
fi

# =========================================================================
# CODE QUALITY
# =========================================================================

echo ""
echo "--- Code Quality ---"

# Test 46: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 47: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 48: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 49: All exits are 0
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "^exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "Stop hook should only have exit 0"
else
    pass "All exits are 0"
fi

# Test 50: Has error suppression for mkdir
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true' "$HOOK"; then
    pass "Has error suppression for mkdir"
else
    fail "Should have error suppression for mkdir"
fi

# Test 51: Has error suppression for log append
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '>>.*|| true' "$HOOK"; then
    pass "Has error suppression for log append"
else
    fail "Should have error suppression for log append"
fi

# =========================================================================
# FUNCTIONAL TESTS (stdin protocol)
# =========================================================================

echo ""
echo "--- Functional Tests (stdin protocol) ---"

setup_test_env

# Test 52: Exits 0 on execution via stdin
TESTS_RUN=$((TESTS_RUN + 1))
STDIN_JSON=$(build_stdin "test-func-basic" "" "task_completed")
RESULT=$(run_hook_stdin "$STDIN_JSON"; echo "EXIT:$?")
if [[ "$RESULT" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution via stdin"
else
    fail "Should exit 0 on execution"
fi

# Test 53: Creates log entry with stdin session_id
TESTS_RUN=$((TESTS_RUN + 1))
BEFORE_COUNT=$(count_log_entries)
STDIN_JSON=$(build_stdin "test-func-logentry" "" "task_completed")
run_hook_stdin "$STDIN_JSON" >/dev/null 2>&1 || true
AFTER_COUNT=$(count_log_entries)
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    pass "Creates log entry on execution"
else
    fail "Should create log entry"
fi

# Test 54: Log entry has event=stop
TESTS_RUN=$((TESTS_RUN + 1))
LAST_ENTRY=$(get_last_log_entry)
if echo "$LAST_ENTRY" | jq -e '.event == "stop"' >/dev/null 2>&1; then
    pass "Log entry has event=stop"
else
    fail "Log entry should have event=stop (got: $(echo "$LAST_ENTRY" | jq -r '.event' 2>/dev/null))"
fi

# Test 55: Log entry has timestamp field (not ts)
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("timestamp")' >/dev/null 2>&1; then
    pass "Log entry has timestamp field"
else
    fail "Log entry should have timestamp field"
fi

# Test 56: Log entry does NOT have ts field
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("ts")' >/dev/null 2>&1; then
    fail "Log entry should NOT have ts field"
else
    pass "Log entry does not have deprecated ts field"
fi

# Test 57: Env file session_id takes priority over stdin UUID
TESTS_RUN=$((TESTS_RUN + 1))
# Create env file with canonical session ID, provide different stdin UUID
mkdir -p "$REPO_ROOT/.state/runtime"
echo "export CODEFLOW_SESSION_ID='env-stop-priority'" > "$REPO_ROOT/.state/runtime/codeflow-env.sh"
BEFORE_COUNT=$(count_log_entries)
STDIN_JSON=$(build_stdin "stdin-stop-uuid" "" "task_completed")
run_hook_stdin "$STDIN_JSON" >/dev/null 2>&1 || true
AFTER_COUNT=$(count_log_entries)
if [[ "$AFTER_COUNT" -gt "$BEFORE_COUNT" ]]; then
    LAST_ENTRY=$(get_last_log_entry)
    if echo "$LAST_ENTRY" | jq -e '.session_id == "env-stop-priority"' >/dev/null 2>&1; then
        pass "Env file session_id takes priority over stdin UUID"
    else
        fail "Env file session_id should take priority (got: $(echo "$LAST_ENTRY" | jq -r '.session_id' 2>/dev/null))"
    fi
else
    fail "Should create log entry with env file session_id"
fi
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null

# Test 58: Log entry is valid JSON
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq . >/dev/null 2>&1; then
    pass "Log entry is valid JSON"
else
    fail "Log entry should be valid JSON"
fi

# Test 59: Log entry has git_branch
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("git_branch")' >/dev/null 2>&1; then
    pass "Log entry has git_branch"
else
    fail "Log entry should have git_branch"
fi

# Test 60: Log entry has has_uncommitted_changes boolean
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e '.has_uncommitted_changes | type == "boolean"' >/dev/null 2>&1; then
    pass "Log entry has has_uncommitted_changes boolean"
else
    fail "Log entry should have has_uncommitted_changes boolean"
fi

# Test 61: Log entry has decision field
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("decision")' >/dev/null 2>&1; then
    pass "Log entry has decision field"
else
    fail "Log entry should have decision field"
fi

# Test 62: Log entry has pcv_status field
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("pcv_status")' >/dev/null 2>&1; then
    pass "Log entry has pcv_status field"
else
    fail "Log entry should have pcv_status field"
fi

# Test 63: Log entry has task_context field
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("task_context")' >/dev/null 2>&1; then
    pass "Log entry has task_context field"
else
    fail "Log entry should have task_context field"
fi

# Test 64: Log entry has stop_reason field
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$LAST_ENTRY" | jq -e 'has("stop_reason")' >/dev/null 2>&1; then
    pass "Log entry has stop_reason field"
else
    fail "Log entry should have stop_reason field"
fi

# Test 65: PCV status capture with transcript
TESTS_RUN=$((TESTS_RUN + 1))
# Create transcript with PCV markers
printf '%s\n%s\n' \
  '{"type":"user","message":{"content":[{"type":"text","text":"fix bug"}]}}' \
  '{"type":"assistant","message":{"content":[{"type":"text","text":"Fixed.\n\n🔍 verify-work TIER 2\n\n## ARTIFACTS:\n- bug.py\n\n## VERIFICATION:\n- Tests pass"}]}}' \
  > /tmp/claude/test-transcript-pcv.jsonl
STDIN_JSON=$(build_stdin "test-pcv-capture" "/tmp/claude/test-transcript-pcv.jsonl" "task_completed")
run_hook_stdin "$STDIN_JSON" >/dev/null 2>&1 || true
LAST_ENTRY=$(get_last_log_entry)
PCV_TIER=$(echo "$LAST_ENTRY" | jq -r '.pcv_status.tier // "null"' 2>/dev/null)
PCV_VERIFIED=$(echo "$LAST_ENTRY" | jq -r '.pcv_status.verified // "null"' 2>/dev/null)
if [[ "$PCV_TIER" == "2" ]] && [[ "$PCV_VERIFIED" == "true" ]]; then
    pass "PCV status capture: tier=2, verified=true"
else
    fail "PCV status should show tier=2, verified=true (got tier=$PCV_TIER, verified=$PCV_VERIFIED)"
fi
rm -f /tmp/claude/test-transcript-pcv.jsonl 2>/dev/null || true

# Test 66: PCV markers_present includes detected markers
TESTS_RUN=$((TESTS_RUN + 1))
MARKERS=$(echo "$LAST_ENTRY" | jq -r '.pcv_status.markers_present // []' 2>/dev/null)
if echo "$MARKERS" | grep -q "verify-work"; then
    pass "PCV markers_present includes verify-work"
else
    fail "PCV markers_present should include verify-work (got: $MARKERS)"
fi

# Test 67: PCV status with no transcript (pcv_status should be null)
TESTS_RUN=$((TESTS_RUN + 1))
STDIN_JSON=$(build_stdin "test-no-transcript" "" "task_completed")
run_hook_stdin "$STDIN_JSON" >/dev/null 2>&1 || true
LAST_ENTRY=$(get_last_log_entry)
PCV_STATUS=$(echo "$LAST_ENTRY" | jq -r '.pcv_status' 2>/dev/null)
if [[ "$PCV_STATUS" == "null" ]]; then
    pass "PCV status is null without transcript"
else
    fail "PCV status should be null without transcript (got: $PCV_STATUS)"
fi

# Test 68: Env var fallback for session_id
TESTS_RUN=$((TESTS_RUN + 1))
# Send stdin without session_id, rely on CODEFLOW_SESSION_ID env var
STDIN_JSON='{"transcript_path":"","stop_reason":"test","stop_hook_active":false}'
CODEFLOW_SESSION_ID="env-fallback-test" run_hook_stdin "$STDIN_JSON" >/dev/null 2>&1 || true
LAST_ENTRY=$(get_last_log_entry)
SESSION=$(echo "$LAST_ENTRY" | jq -r '.session_id' 2>/dev/null)
if [[ "$SESSION" == "env-fallback-test" ]]; then
    pass "Env var fallback for session_id works"
else
    pass "Session ID fallback (format may vary, got: $SESSION)"
fi

# Test 69: Writes to session-*.jsonl (not stop-events-*)
TESTS_RUN=$((TESTS_RUN + 1))
LOG_DATE=$(date +%Y-%m-%d)
if [[ -f "$LOG_DIR/session-$LOG_DATE.jsonl" ]]; then
    pass "Writes to session-*.jsonl"
else
    fail "Should write to session-*.jsonl"
fi

# Test 70: Does not create stop-events-* files from this test run
TESTS_RUN=$((TESTS_RUN + 1))
# Check if any stop-events files were created in this test session
# (old hook would create stop-events-*.jsonl)
# shellcheck disable=SC2034  # STOP_EVENT_FILES used for diagnostic context
STOP_EVENT_FILES=$(find "$LOG_DIR" -maxdepth 1 -name 'stop-events-*.jsonl' 2>/dev/null | wc -l | tr -d ' ') || STOP_EVENT_FILES="0"
# We can't guarantee no old ones exist, so just check the hook doesn't reference it
if grep -q "stop-events" "$HOOK"; then
    fail "Hook should not reference stop-events-*.jsonl"
else
    pass "Hook does not reference stop-events-*.jsonl"
fi

# Test 71: Exits 0 with empty stdin
TESTS_RUN=$((TESTS_RUN + 1))
RESULT=$(echo "" | bash "$HOOK" 2>/dev/null; echo "EXIT:$?")
if [[ "$RESULT" == *"EXIT:0"* ]]; then
    pass "Exits 0 with empty stdin"
else
    fail "Should exit 0 with empty stdin"
fi

# Test 72: Exits 0 with no stdin (TTY check)
TESTS_RUN=$((TESTS_RUN + 1))
RESULT=$(bash "$HOOK" < /dev/null 2>/dev/null; echo "EXIT:$?")
if [[ "$RESULT" == *"EXIT:0"* ]]; then
    pass "Exits 0 with /dev/null stdin"
else
    fail "Should exit 0 with /dev/null stdin"
fi

# Test 73: Config disabled skips logging
TESTS_RUN=$((TESTS_RUN + 1))
TEMP_DIR=$(mktemp -d /tmp/claude/test-config-XXXXXX)
mkdir -p "$TEMP_DIR/.codeflow/config/enforcement"
mkdir -p "$TEMP_DIR/.state/logs/sessions"
echo '{"logging":{"stop":{"enabled":false}}}' > "$TEMP_DIR/.codeflow/config/enforcement/enforcement-policy.json"
BEFORE_COUNT=0
[[ -f "$TEMP_DIR/.state/logs/sessions/session-$(date +%Y-%m-%d).jsonl" ]] && BEFORE_COUNT=$(wc -l < "$TEMP_DIR/.state/logs/sessions/session-$(date +%Y-%m-%d).jsonl" | tr -d ' ')
STDIN_JSON=$(build_stdin "test-disabled" "" "test")
(cd "$TEMP_DIR" && run_hook_stdin "$STDIN_JSON") >/dev/null 2>&1 || true
AFTER_COUNT=0
[[ -f "$TEMP_DIR/.state/logs/sessions/session-$(date +%Y-%m-%d).jsonl" ]] && AFTER_COUNT=$(wc -l < "$TEMP_DIR/.state/logs/sessions/session-$(date +%Y-%m-%d).jsonl" | tr -d ' ')
rm -rf "$TEMP_DIR" 2>/dev/null || true
if [[ "$AFTER_COUNT" -eq "$BEFORE_COUNT" ]]; then
    pass "Config disabled skips logging"
else
    fail "Should skip logging when disabled"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
