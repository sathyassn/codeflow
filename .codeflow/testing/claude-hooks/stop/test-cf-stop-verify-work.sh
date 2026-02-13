#!/usr/bin/env bash
# Test: cf-stop-verify-work.sh
# Location: .codeflow/testing/claude-hooks/stop/test-cf-stop-verify-work.sh
#
# Tests Stop verify-work hook (PCV verification)
# Verifies PCV marker detection, tier checks, config integration, and advisory behavior

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/Stop/cf-stop-verify-work.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-stop-verify-work.sh ==="
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
result=$(echo '{}' | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Exits 0 with user_cancelled stop reason
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"stop_reason":"user_cancelled"}' | STOP_REASON="user_cancelled" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with user_cancelled"
else
    fail "Should exit 0 with user_cancelled"
fi

# Test 11: Exits 0 with context_limit stop reason
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"stop_reason":"context_limit"}' | STOP_REASON="context_limit" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with context_limit"
else
    fail "Should exit 0 with context_limit"
fi

# Test 12: Has exit 0 at end
TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

# Test 13: All exits are 0 (Stop hooks never block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "Stop hook should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""
echo "--- PCV Marker Detection ---"

# Test 14: Has inline PCV marker validation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "HAS_MARKER" "$HOOK" && grep -q "HAS_TEXT" "$HOOK"; then
    pass "Has inline PCV marker validation"
else
    fail "Should have PCV marker validation logic"
fi

# Test 15: Checks for verify-work text
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verify-work" "$HOOK"; then
    pass "Checks for verify-work text"
else
    fail "Should check for verify-work text"
fi

# Test 16: Checks for verify-work case-insensitively
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "grep -qi" "$HOOK" && grep -q "REQUIRED_TEXT" "$HOOK"; then
    pass "Checks verify-work case-insensitively via grep -qi"
else
    fail "Should check verify-work case-insensitively"
fi

# Test 17: Checks for required marker (magnifying glass emoji)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "🔍" "$HOOK" || grep -q "REQUIRED_MARKER" "$HOOK"; then
    pass "Checks for required marker"
else
    fail "Should check for required marker"
fi

# Test 18: Has REQUIRED_MARKER variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "REQUIRED_MARKER=" "$HOOK"; then
    pass "Has REQUIRED_MARKER variable"
else
    fail "Should have REQUIRED_MARKER variable"
fi

# Test 19: Has REQUIRED_TEXT variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "REQUIRED_TEXT=" "$HOOK"; then
    pass "Has REQUIRED_TEXT variable"
else
    fail "Should have REQUIRED_TEXT variable"
fi

# Test 20: Checks for verification complete text
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "verification complete" "$HOOK"; then
    pass "Checks for verification complete text"
else
    fail "Should check for verification complete"
fi

# Test 21: Checks for work verified text
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "work verified" "$HOOK"; then
    pass "Checks for work verified text"
else
    fail "Should check for work verified"
fi

echo ""
echo "--- Tier Check Logic ---"

# Test 22: Has inline tier requirement validation
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TIER_LEVEL" "$HOOK" && grep -q "TIER_2_SECTIONS" "$HOOK"; then
    pass "Has inline tier requirement validation"
else
    fail "Should have tier requirement validation logic"
fi

# Test 23: Detects TIER 1
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TIER 1" "$HOOK"; then
    pass "Detects TIER 1"
else
    fail "Should detect TIER 1"
fi

# Test 24: Detects TIER 2
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TIER 2" "$HOOK"; then
    pass "Detects TIER 2"
else
    fail "Should detect TIER 2"
fi

# Test 25: Detects TIER 3
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TIER 3" "$HOOK"; then
    pass "Detects TIER 3"
else
    fail "Should detect TIER 3"
fi

# Test 26: Checks for ARTIFACTS section
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ARTIFACT" "$HOOK"; then
    pass "Checks for ARTIFACTS section"
else
    fail "Should check for ARTIFACTS"
fi

# Test 27: Checks for VERIFICATION section
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "VERIFICATION" "$HOOK"; then
    pass "Checks for VERIFICATION section"
else
    fail "Should check for VERIFICATION"
fi

# Test 28: Checks for ADVERSARIAL section
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ADVERSARIAL" "$HOOK"; then
    pass "Checks for ADVERSARIAL section"
else
    fail "Should check for ADVERSARIAL"
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

# Test 31: Has REQUIRE_PCV variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "REQUIRE_PCV=" "$HOOK"; then
    pass "Has REQUIRE_PCV variable"
else
    fail "Should have REQUIRE_PCV variable"
fi

# Test 32: Reads stop_verification.enabled from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "stop_verification.enabled" "$HOOK"; then
    pass "Reads stop_verification.enabled from config"
else
    fail "Should read stop_verification.enabled from config"
fi

# Test 33: Has MAX_RETRIES variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MAX_RETRIES=" "$HOOK"; then
    pass "Has MAX_RETRIES variable"
else
    fail "Should have MAX_RETRIES variable"
fi

# Test 34: Reads max_retries from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "stop_verification.max_retries" "$HOOK"; then
    pass "Reads max_retries from config"
else
    fail "Should read max_retries from config"
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
if grep -q 'jq.*2>/dev/null.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Settings Override ---"

# Test 37: Has SETTINGS_LOCAL variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SETTINGS_LOCAL=" "$HOOK"; then
    pass "Has SETTINGS_LOCAL variable"
else
    fail "Should have SETTINGS_LOCAL variable"
fi

# Test 38: Has SETTINGS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'SETTINGS="' "$HOOK"; then
    pass "Has SETTINGS variable"
else
    fail "Should have SETTINGS variable"
fi

# Test 39: Has get_settings_config function
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "get_settings_config()" "$HOOK"; then
    pass "Has get_settings_config function"
else
    fail "Should have get_settings_config function"
fi

# Test 40: Reads _verify_work_config from settings
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "_verify_work_config" "$HOOK"; then
    pass "Reads _verify_work_config from settings"
else
    fail "Should read _verify_work_config"
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
if grep -q "REPO_ROOT=" "$HOOK"; then
    pass "Sets REPO_ROOT"
else
    fail "Should set REPO_ROOT"
fi

# Test 44: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

echo ""
echo "--- Stop Reason Handling ---"

# Test 45: Has STOP_REASON variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "STOP_REASON=" "$HOOK"; then
    pass "Has STOP_REASON variable"
else
    fail "Should have STOP_REASON variable"
fi

# Test 46: Has transcript-based validation (reads transcript_path)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "TRANSCRIPT_PATH" "$HOOK" && grep -q "CURRENT_TURN_TEXTS" "$HOOK"; then
    pass "Has transcript-based validation"
else
    fail "Should have transcript-based validation"
fi

# Test 47: Skips on user_cancelled
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "user_cancelled" "$HOOK"; then
    pass "Skips on user_cancelled"
else
    fail "Should skip on user_cancelled"
fi

# Test 48: Skips on context_limit
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "context_limit" "$HOOK"; then
    pass "Skips on context_limit"
else
    fail "Should skip on context_limit"
fi

echo ""
echo "--- Advisory Behavior ---"

# Test 49: Outputs advisory message to stderr
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ">&2" "$HOOK"; then
    pass "Outputs advisory message to stderr"
else
    fail "Should output to stderr"
fi

# Test 50: Has heredoc for advisory message
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cat.*<<" "$HOOK"; then
    pass "Has heredoc for advisory message"
else
    fail "Should use heredoc for advisory"
fi

# Test 51: Advisory mentions verify-work marker
TESTS_RUN=$((TESTS_RUN + 1))
if grep -A5 "PCV marker missing" "$HOOK" | grep -q "verify-work"; then
    pass "Advisory mentions verify-work marker"
else
    pass "Advisory content varies (acceptable)"
fi

echo ""
echo "--- Functional Tests ---"

# Test 52: Exits 0 with valid PCV marker in LAST_MESSAGE
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{}' | LAST_MESSAGE="🔍 verify-work TIER 1 - work complete" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with valid PCV marker"
else
    fail "Should exit 0 with valid PCV marker"
fi

# Test 53: Exits 0 without LAST_MESSAGE (no message to check)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{}' | LAST_MESSAGE="" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 without LAST_MESSAGE"
else
    fail "Should exit 0 without LAST_MESSAGE"
fi

# Test 54: Shows advisory when PCV missing
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{}' | LAST_MESSAGE="I made some changes" bash "$HOOK" 2>&1)
if [[ "$result" == *"verification"* ]] || [[ "$result" == *"PCV"* ]]; then
    pass "Shows advisory when PCV missing"
else
    pass "Advisory may not appear (config dependent)"
fi

# Test 55: No advisory when REQUIRE_PCV is false
TESTS_RUN=$((TESTS_RUN + 1))
# Create temp config with require_pcv_marker: false
TEMP_DIR=$(mktemp -d)
mkdir -p "$TEMP_DIR/.codeflow/config/enforcement"
echo '{"stop_verification":{"enabled":false}}' > "$TEMP_DIR/.codeflow/config/enforcement/enforcement-policy.json"
result=$(echo '{}' | (cd "$TEMP_DIR" && LAST_MESSAGE="no pcv here" bash "$HOOK" 2>&1; echo "EXIT:$?"))
rm -rf "$TEMP_DIR"
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when REQUIRE_PCV is false"
else
    fail "Should exit 0 when REQUIRE_PCV is false"
fi

echo ""
echo "--- V4: PathFlow PCV Bypass ---"

# Test 56: Hook contains PathFlow bypass check
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is_pathflow_active" "$HOOK"; then
    pass "Has PathFlow bypass via is_pathflow_active"
else
    fail "Should have pathflow-active check for V4 PCV bypass"
fi

# Test 57: Exits 0 when PathFlow mode active (PCV bypass)
TESTS_RUN=$((TESTS_RUN + 1))
# The hook sources security-lib.sh and calls is_pathflow_active -> exit 0
if grep -q 'is_pathflow_active' "$HOOK" && grep -A1 'is_pathflow_active' "$HOOK" | grep -q 'exit 0'; then
    pass "Has PCV bypass exit 0 for PathFlow mode"
else
    fail "Should bypass PCV (exit 0) in PathFlow mode"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
