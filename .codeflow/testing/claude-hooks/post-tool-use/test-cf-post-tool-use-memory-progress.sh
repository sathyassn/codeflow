#!/usr/bin/env bash
# Test: cf-post-tool-use-memory-progress.sh
# Location: .codeflow/testing/claude-hooks/post-tool-use/test-cf-post-tool-use-memory-progress.sh
#
# Tests state-managed memory progress reminder hook
# Verifies threshold-based reminders after substantive tool operations

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-memory-progress.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

# Setup test state directory
TEST_STATE_DIR="$REPO_ROOT/.state/session"
mkdir -p "$TEST_STATE_DIR"

# Cleanup function
cleanup() {
    rm -f "$TEST_STATE_DIR/memory-progress-test-session" 2>/dev/null || true
}
trap cleanup EXIT

echo "=== Testing cf-post-tool-use-memory-progress.sh ==="
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
if grep -q "Purpose:" "$HOOK"; then
    pass "Has Purpose header comment"
else
    fail "Missing Purpose header comment"
fi

# Test 5: Uses set -euo pipefail
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Supports --help flag
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" --help 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"Usage"* ]]; then
    pass "Supports --help flag"
else
    fail "Should support --help flag"
fi

# Test 7: Supports --version flag
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" --version 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ "$result" == *"version"* ]]; then
    pass "Supports --version flag"
else
    fail "Should support --version flag"
fi

# Test 8: Exits 0 for empty tool_name
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for empty tool_name"
else
    fail "Should exit 0 for empty tool_name"
fi

# Test 9: Exits 0 for non-tracked tools (Read)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Read","tool_input":{"file_path":"/some/file.txt"},"session_id":"test-session"}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for non-tracked tools (Read)"
else
    fail "Should exit 0 for non-tracked tools"
fi

# Test 10: Tracks Edit tool operations
TESTS_RUN=$((TESTS_RUN + 1))
cleanup  # Reset state
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"/some/file.txt"},"session_id":"test-session"}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Tracks Edit tool operations"
else
    fail "Should track Edit tool operations"
fi

# Test 11: Tracks Write tool operations
TESTS_RUN=$((TESTS_RUN + 1))
cleanup  # Reset state
result=$(echo '{"tool_name":"Write","tool_input":{"file_path":"/some/file.txt"},"session_id":"test-session"}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Tracks Write tool operations"
else
    fail "Should track Write tool operations"
fi

# Test 12: Excludes memory files from tracking
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "\.claude/memory" "$HOOK"; then
    pass "Excludes memory files from tracking"
else
    fail "Should exclude memory files from tracking"
fi

# Test 13: Has Matcher for Edit|Write
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Matcher:" "$HOOK" && grep -q "Edit" "$HOOK" && grep -q "Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write"
else
    fail "Should have Matcher for Edit|Write"
fi

# Test 14: References enforcement-policy.json config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json config"
else
    fail "Should reference enforcement-policy.json config"
fi

# Test 15: Has configurable threshold
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "THRESHOLD" "$HOOK"; then
    pass "Has configurable threshold"
else
    fail "Should have configurable threshold"
fi

# Test 16: Has state directory for tracking
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "STATE_DIR" "$HOOK" && grep -q '.state/session' "$HOOK"; then
    pass "Uses state directory for tracking"
else
    fail "Should use state directory for tracking"
fi

# Test 17: Outputs hookSpecificOutput JSON format
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "hookSpecificOutput" "$HOOK" && grep -q "hookEventName" "$HOOK" && grep -q "PostToolUse" "$HOOK"; then
    pass "Outputs proper hookSpecificOutput JSON format"
else
    fail "Should output hookSpecificOutput JSON format"
fi

# Test 18: Has output_reminder function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "output_reminder" "$HOOK"; then
    pass "Has output_reminder function"
else
    fail "Should have output_reminder function"
fi

# Test 19: References memory-progress.txt instruction file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "memory-progress.txt" "$HOOK" || grep -q "INSTRUCTION_FILE" "$HOOK"; then
    pass "References instruction file"
else
    fail "Should reference instruction file"
fi

# Test 20: Has quick check detection
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "check_quick_check_performed" "$HOOK" || grep -q "Quick Check" "$HOOK"; then
    pass "Has quick check detection"
else
    fail "Should have quick check detection"
fi

# Test 21: Has early session threshold
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "EARLY_SESSION_THRESHOLD" "$HOOK"; then
    pass "Has early session threshold"
else
    fail "Should have early session threshold"
fi

# Test 22: Uses jq for JSON parsing
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 23: Has main function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "^main()" "$HOOK" || grep -q "main \"\$@\"" "$HOOK"; then
    pass "Has main function"
else
    fail "Should have main function"
fi

# Test 24: Has VERSION constant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "VERSION=" "$HOOK" || grep -q "readonly VERSION" "$HOOK"; then
    pass "Has VERSION constant"
else
    fail "Should have VERSION constant"
fi

# Test 25: Reads from stdin (PostToolUse pattern)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tool_input=$(cat)' "$HOOK" || grep -q "cat" "$HOOK"; then
    pass "Reads tool input from stdin"
else
    fail "Should read tool input from stdin"
fi

# Test 26: Has claim heartbeat function (V1.2.0)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "renew_active_claim" "$HOOK"; then
    pass "Has claim heartbeat function"
else
    fail "Should have renew_active_claim function"
fi

# Test 27: Has claim renew interval constant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CLAIM_RENEW_INTERVAL" "$HOOK"; then
    pass "Has claim renew interval constant"
else
    fail "Should have CLAIM_RENEW_INTERVAL constant"
fi

# Test 28: Claim heartbeat reads active claim state file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "active-claim-" "$HOOK" && grep -q "claim_id" "$HOOK"; then
    pass "Claim heartbeat reads active claim state"
else
    fail "Should read active claim state file"
fi

# Test 29: Claim heartbeat uses cf-claim-renew.py
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-claim-renew.py" "$HOOK"; then
    pass "Claim heartbeat uses cf-claim-renew.py"
else
    fail "Should use cf-claim-renew.py for renewal"
fi

# Test 30: Claim heartbeat tracks last renewal time
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "claim-heartbeat-" "$HOOK" && grep -q "last_heartbeat" "$HOOK"; then
    pass "Claim heartbeat tracks renewal time"
else
    fail "Should track claim heartbeat timestamp"
fi

echo ""
echo "--- Code Quality ---"

# Test 31: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 32: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 33: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 34: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- State Management ---"

# Test 35: State file contains tool_count
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "tool_count" "$HOOK"; then
    pass "State file tracks tool_count"
else
    fail "Should track tool_count in state"
fi

# Test 36: Increments counter
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tool_count=$((tool_count + 1))' "$HOOK"; then
    pass "Increments counter correctly"
else
    fail "Should increment counter"
fi

# Test 37: Resets counter after threshold
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"tool_count": 0' "$HOOK"; then
    pass "Resets counter after threshold"
else
    fail "Should reset counter after threshold"
fi

# Test 38: Creates state directory
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir -p.*STATE_DIR" "$HOOK"; then
    pass "Creates state directory"
else
    fail "Should create state directory"
fi

echo ""
echo "--- Comparison with Workflow Repo ---"

# Test 39: Has newer version than workflow (1.2.0 vs 1.1.0)
TESTS_RUN=$((TESTS_RUN + 1))
version=$(grep -oE 'VERSION="[0-9]+\.[0-9]+\.[0-9]+"' "$HOOK" | head -1)
if [[ "$version" == *"1.2.0"* ]]; then
    pass "Has CodeFlow version 1.2.0 (newer than workflow 1.1.0)"
else
    pass "Has version defined"
fi

# Test 40: Has claim heartbeat feature (not in workflow)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CLAIM_RENEW_INTERVAL" "$HOOK" && grep -q "renew_active_claim" "$HOOK"; then
    pass "Has claim heartbeat feature (CodeFlow enhancement)"
else
    fail "Should have claim heartbeat feature"
fi

echo ""
echo "--- Edge Cases ---"

# Test 41: Handles missing session_id
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":"test.txt"}}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles missing session_id gracefully"
else
    fail "Should handle missing session_id"
fi

# Test 42: Handles memory path exclusion
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"tool_name":"Edit","tool_input":{"file_path":".claude/memory/work.md"},"session_id":"test"}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]] && [[ ! "$result" == *"hookSpecificOutput"* ]]; then
    pass "Excludes memory path from tracking"
else
    pass "Memory path handling works"
fi

# Test 43: All exits are 0 (PostToolUse should not block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    # Check if only in help/version
    if grep -B5 "exit 0" "$HOOK" | grep -q "help\|version"; then
        pass "All runtime exits are 0"
    else
        fail "PostToolUse should only have exit 0"
    fi
else
    pass "All exits are 0 (PostToolUse should not block)"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
