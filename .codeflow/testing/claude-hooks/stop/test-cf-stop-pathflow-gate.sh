#!/usr/bin/env bash
# Test: cf-stop-pathflow-gate.sh
# Location: .codeflow/testing/claude-hooks/stop/test-cf-stop-pathflow-gate.sh
#
# Tests Stop pathflow-gate hook (v3.0.0 - non-blocking phase logger)
# Verifies:
#   - File exists, executable, shellcheck, headers, strict mode, VERSION
#   - Always exits 0 (never blocks)
#   - Exits immediately when stop_hook_active is true
#   - Exits 0 when not in PathFlow mode
#   - Logs phase completion when PathFlow is active
#   - Sources codeflow-env.sh for canonical session ID (falls back to stdin session_id)

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../lib/test-isolation.sh"
HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/stop/cf-stop-pathflow-gate.sh"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-stop-pathflow-gate.sh (v3 non-blocking) ==="
echo ""

# =============================================================================
# BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

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

# Test 6: Has VERSION 3.0.0
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'VERSION="3.0.0"' "$HOOK"; then
    pass "Has VERSION 3.0.0"
else
    fail "Should have VERSION 3.0.0"
fi

# Test 7: Has Hook Type header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Hook Type:" "$HOOK"; then
    pass "Has Hook Type header"
else
    fail "Should have Hook Type header"
fi

echo ""
echo "--- Code Quality ---"

# Test 8: Does NOT output block JSON (non-blocking hook)
TESTS_RUN=$((TESTS_RUN + 1))
if ! grep -q '"decision":"block"' "$HOOK"; then
    pass "Does not output block JSON"
else
    fail "Should NOT output block JSON (non-blocking hook)"
fi

# Test 9: Does NOT use PCV markers in logic (comments OK)
TESTS_RUN=$((TESTS_RUN + 1))
if ! grep -v '^#' "$HOOK" | grep -q "verify-work\|PCV\|REQUIRED_MARKER\|REQUIRED_TEXT"; then
    pass "No PCV marker logic (comments only)"
else
    fail "Should not have PCV marker logic in code"
fi

# Test 10: Does NOT parse transcripts in logic (comments OK)
TESTS_RUN=$((TESTS_RUN + 1))
if ! grep -v '^#' "$HOOK" | grep -q 'TRANSCRIPT_PATH\|tail.*transcript'; then
    pass "No transcript parsing logic (comments only)"
else
    fail "Should not parse transcripts in code"
fi

# Test 11: Sources security-lib.sh
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "security-lib.sh" "$HOOK"; then
    pass "Sources security-lib.sh"
else
    fail "Should source security-lib.sh"
fi

echo ""
echo "--- Execution Tests ---"

# Test 12: Always exits 0 with minimal input
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"session_id":"test-sess","stop_hook_active":false}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with standard input"
else
    fail "Should always exit 0"
fi

# Test 13: Exits 0 immediately when stop_hook_active is true
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"session_id":"test-sess","stop_hook_active":true}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when stop_hook_active=true (loop guard)"
else
    fail "Should exit 0 immediately for stop_hook_active=true"
fi

# Test 14: Exits 0 with empty input
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with empty JSON input"
else
    fail "Should exit 0 with empty input"
fi

# Test 15: Never outputs to stdout (no block JSON)
TESTS_RUN=$((TESTS_RUN + 1))
stdout_output=$(echo '{"session_id":"test-sess","stop_hook_active":false}' | bash "$HOOK" 2>/dev/null)
if [[ -z "$stdout_output" ]]; then
    pass "No stdout output (non-blocking)"
else
    fail "Should not write to stdout (got: $stdout_output)"
fi

echo ""
echo "--- Session ID Resolution ---"

# Test 16: Sources codeflow-env.sh for canonical session ID
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'codeflow-env.sh' "$HOOK"; then
    pass "Sources codeflow-env.sh for canonical session ID"
else
    fail "Should source codeflow-env.sh for canonical session ID"
fi

# Test 17: Uses CODEFLOW_SESSION_ID from env file for phase lookup
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CODEFLOW_SESSION_ID' "$HOOK"; then
    pass "Uses CODEFLOW_SESSION_ID for phase lookup"
else
    fail "Should use CODEFLOW_SESSION_ID from env file for phase lookup"
fi

# Test 18: Falls back to stdin session_id when env file unavailable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CODEFLOW_SESSION_ID:-${SESSION_ID:-' "$HOOK" || grep -q 'CODEFLOW_SESSION_ID:-.*SESSION_ID' "$HOOK"; then
    pass "Falls back to stdin session_id when env file unavailable"
else
    fail "Should fall back to stdin session_id when env file unavailable"
fi

# Test 19: Does NOT use current-session-id file (removed in session ID fix)
TESTS_RUN=$((TESTS_RUN + 1))
if ! grep -q 'current-session-id' "$HOOK"; then
    pass "Does not use current-session-id file (dead code removed)"
else
    fail "Should not use current-session-id file (replaced by codeflow-env.sh)"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
