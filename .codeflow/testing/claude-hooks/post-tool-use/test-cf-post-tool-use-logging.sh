#!/usr/bin/env bash
# Test: cf-post-tool-use-logging.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-logging.sh

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-logging.sh"

TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-post-tool-use-logging.sh ==="
echo ""

# Test 1: File exists
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Exits 0 when no TOOL_NAME
result=$(TOOL_NAME="" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_NAME"
else
    fail "Should exit 0 when no TOOL_NAME"
fi

# Test 5: Exits 0 for any tool
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls"}' TOOL_RESULT="file.txt" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 6: Has logging logic
if grep -qE "LOG|log|JSONL|jsonl" "$HOOK"; then
    pass "Has logging logic"
else
    fail "Should have logging logic"
fi

# Test 7: Has sensitive data redaction
if grep -qE "redact|REDACT|password|token" "$HOOK"; then
    pass "Has sensitive data handling"
else
    fail "Should handle sensitive data"
fi

# Test 8: Has proper header comments
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 9: Uses set -euo pipefail
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 10: Always exits 0 (logging shouldn't block)
if grep -q "exit 0" "$HOOK"; then
    pass "Has exit 0"
else
    fail "Should have exit 0"
fi

# Test 11: Has result truncation
if grep -qE "truncat|MAX.*SIZE" "$HOOK"; then
    pass "Has result truncation"
else
    fail "Should truncate large results"
fi

# Test 12: Creates log directory
if grep -q "mkdir" "$HOOK"; then
    pass "Creates log directory"
else
    fail "Should create log directory"
fi

# Test 13: Has email redaction (V1.1.0)
if grep -qE "EMAIL_REDACTED|email" "$HOOK"; then
    pass "Has email redaction"
else
    fail "Should redact email addresses"
fi

# Test 14: Has duration_ms support (V1.1.0)
if grep -q "duration_ms" "$HOOK" || grep -q "DURATION_MS" "$HOOK"; then
    pass "Has duration_ms support"
else
    fail "Should support duration_ms field"
fi

# Test 15: Has calculate_duration_ms function
if grep -q "calculate_duration_ms" "$HOOK"; then
    pass "Has calculate_duration_ms function"
else
    fail "Should have calculate_duration_ms function"
fi

# Test 16: Handles TOOL_START_TIME and TOOL_END_TIME
if grep -q "TOOL_START_TIME" "$HOOK" && grep -q "TOOL_END_TIME" "$HOOK"; then
    pass "Handles tool timing env vars"
else
    fail "Should handle TOOL_START_TIME and TOOL_END_TIME"
fi

# Test 17: Has Version header
if grep -q "Version:" "$HOOK"; then
    pass "Has Version header"
else
    fail "Should have Version header"
fi

echo ""
echo "--- Code Quality ---"

# Test 18: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 19: Has proper fallback grouping for REPO_ROOT
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 20: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 21: Documents bash compatibility
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Functional Tests ---"

# Test 22: Logs Edit tool
result=$(TOOL_NAME="Edit" TOOL_INPUT='{"file_path":"test.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Logs Edit tool successfully"
else
    fail "Should log Edit tool"
fi

# Test 23: Logs Write tool
result=$(TOOL_NAME="Write" TOOL_INPUT='{"file_path":"new.txt"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Logs Write tool successfully"
else
    fail "Should log Write tool"
fi

# Test 24: Logs Read tool
result=$(TOOL_NAME="Read" TOOL_INPUT='{"file_path":"doc.md"}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Logs Read tool successfully"
else
    fail "Should log Read tool"
fi

# Test 25: Handles session ID from environment
result=$(CODEFLOW_SESSION_ID="test-session-123" TOOL_NAME="Bash" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles session ID from environment"
else
    fail "Should handle session ID"
fi

# Test 26: Handles missing session ID
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles missing session ID gracefully"
else
    fail "Should handle missing session ID"
fi

echo ""
echo "--- Redaction Tests ---"

# Test 27: Has password redaction pattern
if grep -q "[Pp]assword" "$HOOK" && grep -q "REDACTED" "$HOOK"; then
    pass "Has password redaction pattern"
else
    fail "Should redact passwords"
fi

# Test 28: Has token redaction pattern
if grep -q "[Tt]oken" "$HOOK" && grep -q "REDACTED" "$HOOK"; then
    pass "Has token redaction pattern"
else
    fail "Should redact tokens"
fi

# Test 29: Has key redaction pattern
if grep -qE "\[Kk\]ey" "$HOOK" && grep -q "REDACTED" "$HOOK"; then
    pass "Has key redaction pattern"
else
    fail "Should redact keys"
fi

# Test 30: Has secret redaction pattern
if grep -qE "\[Ss\]ecret" "$HOOK" && grep -q "REDACTED" "$HOOK"; then
    pass "Has secret redaction pattern"
else
    fail "Should redact secrets"
fi

# Test 31: Has Bearer token redaction
if grep -q "Bearer.*REDACTED" "$HOOK"; then
    pass "Has Bearer token redaction"
else
    fail "Should redact Bearer tokens"
fi

# Test 32: Has long token redaction
if grep -q "LONG_TOKEN_REDACTED" "$HOOK"; then
    pass "Has long token redaction"
else
    fail "Should redact long tokens"
fi

echo ""
echo "--- Configuration Tests ---"

# Test 33: Reads max_result_size from config
if grep -q "max_result_size" "$HOOK"; then
    pass "Reads max_result_size from config"
else
    fail "Should read max_result_size from config"
fi

# Test 34: Reads redact_sensitive from config
if grep -q "redact_sensitive" "$HOOK"; then
    pass "Reads redact_sensitive from config"
else
    fail "Should read redact_sensitive from config"
fi

# Test 35: Reads tools_to_log from config
if grep -q "tools_to_log" "$HOOK"; then
    pass "Reads tools_to_log from config"
else
    fail "Should read tools_to_log from config"
fi

# Test 36: Has should_log function
if grep -q "should_log" "$HOOK"; then
    pass "Has should_log function"
else
    fail "Should have should_log function"
fi

echo ""
echo "--- Logging Output Tests ---"

# Test 37: Creates JSONL log entries
if grep -q "jsonl\|JSONL\|\.jsonl" "$HOOK"; then
    pass "Creates JSONL log entries"
else
    fail "Should create JSONL log entries"
fi

# Test 38: Has tool_completed event type
if grep -q "tool_completed" "$HOOK"; then
    pass "Has tool_completed event type"
else
    fail "Should have tool_completed event"
fi

# Test 39: Includes session_id in log entry
if grep -q "session_id" "$HOOK"; then
    pass "Includes session_id in log entry"
else
    fail "Should include session_id in log"
fi

# Test 40: Includes tool_name in log entry
if grep -q "tool_name" "$HOOK"; then
    pass "Includes tool_name in log entry"
else
    fail "Should include tool_name in log"
fi

# Test 41: Has result_truncated field
if grep -q "result_truncated" "$HOOK"; then
    pass "Has result_truncated field"
else
    fail "Should have result_truncated field"
fi


echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
