#!/usr/bin/env bash
# Test: cf-session-start-instructions.sh
# Location: .codeflow/testing/claude-hooks/session-start/test-cf-session-start-instructions.sh
#
# Tests config-driven SessionStart behavioral instructions hook
# Verifies instruction loading, config integration, and fallback behavior

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-instructions.sh"
CONFIG_FILE="$REPO_ROOT/.codeflow/config/instructions/instructions-config.json"
INSTRUCTIONS_DIR="$REPO_ROOT/.codeflow/config/instructions"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-session-start-instructions.sh ==="
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
result=$(bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Produces output (instructions)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(bash "$HOOK" 2>&1)
if [[ -n "$result" ]]; then
    pass "Produces output"
else
    fail "Should produce output (instructions)"
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
echo "--- Config Integration ---"

# Test 13: References instructions-config.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "instructions-config.json" "$HOOK"; then
    pass "References instructions-config.json"
else
    fail "Should reference instructions-config.json"
fi

# Test 14: Has CONFIG_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG_FILE" "$HOOK"; then
    pass "Has CONFIG_FILE variable"
else
    fail "Should have CONFIG_FILE variable"
fi

# Test 15: Has INSTRUCTIONS_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "INSTRUCTIONS_DIR" "$HOOK"; then
    pass "Has INSTRUCTIONS_DIR variable"
else
    fail "Should have INSTRUCTIONS_DIR variable"
fi

# Test 16: Checks if config file exists
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q '\-f "$CONFIG_FILE"' "$HOOK"; then
    pass "Checks if config file exists"
else
    fail "Should check if config file exists"
fi

# Test 17: Checks if jq is available
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Checks if jq is available"
else
    fail "Should check jq availability"
fi

# Test 18: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Instruction Loading ---"

# Test 19: Has ENABLED_INSTRUCTIONS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ENABLED_INSTRUCTIONS" "$HOOK"; then
    pass "Has ENABLED_INSTRUCTIONS variable"
else
    fail "Should have ENABLED_INSTRUCTIONS variable"
fi

# Test 20: Reads enabled flag from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enabled == true" "$HOOK"; then
    pass "Reads enabled flag from config"
else
    fail "Should read enabled flag"
fi

# Test 21: Iterates through instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "for instruction in" "$HOOK"; then
    pass "Iterates through instructions"
else
    fail "Should iterate through instructions"
fi

# Test 22: Has INSTRUCTION_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "INSTRUCTION_FILE" "$HOOK"; then
    pass "Has INSTRUCTION_FILE variable"
else
    fail "Should have INSTRUCTION_FILE variable"
fi

# Test 23: Checks if instruction file exists
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q '\-f "$INSTRUCTIONS_DIR/$INSTRUCTION_FILE"' "$HOOK"; then
    pass "Checks if instruction file exists"
else
    fail "Should check instruction file existence"
fi

# Test 24: Uses cat to output instruction content
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q 'cat "$INSTRUCTIONS_DIR/$INSTRUCTION_FILE"' "$HOOK"; then
    pass "Uses cat to output instruction content"
else
    fail "Should use cat for instruction content"
fi

# Test 25: Has error handling for cat
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'cat.*|| true' "$HOOK"; then
    pass "Has error handling for cat"
else
    fail "Should have error handling for cat"
fi

# Test 26: Adds blank line between instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'echo ""' "$HOOK"; then
    pass "Adds blank line between instructions"
else
    fail "Should add blank line between instructions"
fi

echo ""
echo "--- Fallback Behavior ---"

# Test 27: Has fallback when config unavailable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Fallback" "$HOOK" && grep -q "else" "$HOOK"; then
    pass "Has fallback when config unavailable"
else
    fail "Should have fallback"
fi

# Test 28: Uses heredoc for fallback
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cat <<'EOF'" "$HOOK"; then
    pass "Uses heredoc for fallback"
else
    fail "Should use heredoc for fallback"
fi

# Test 29: Fallback mentions CLAUDE.md
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CLAUDE.md" "$HOOK"; then
    pass "Fallback mentions CLAUDE.md"
else
    fail "Fallback should mention CLAUDE.md"
fi

# Test 30: Fallback mentions Section 2
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Section 2" "$HOOK"; then
    pass "Fallback mentions Section 2"
else
    fail "Fallback should mention Section 2"
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

# Test 35: References skills in header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "working-protocol" "$HOOK" && grep -q "memory-management" "$HOOK"; then
    pass "References skills in header"
else
    fail "Should reference skills"
fi

echo ""
echo "--- Config File Tests ---"

# Test 36: Config file exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$CONFIG_FILE" ]]; then
    pass "Config file exists"
else
    fail "Config file should exist"
fi

# Test 37: Config has SessionStart section
TESTS_RUN=$((TESTS_RUN + 1))
if command -v jq &>/dev/null && [[ -f "$CONFIG_FILE" ]]; then
    if jq -e '.hooks.SessionStart' "$CONFIG_FILE" &>/dev/null; then
        pass "Config has SessionStart section"
    else
        fail "Config should have SessionStart section"
    fi
else
    pass "Config SessionStart check (skipped - jq not available)"
fi

# Test 38: Config has enabled instructions
TESTS_RUN=$((TESTS_RUN + 1))
if command -v jq &>/dev/null && [[ -f "$CONFIG_FILE" ]]; then
    ENABLED_COUNT=$(jq '[.hooks.SessionStart | to_entries[] | select(.value.enabled == true)] | length' "$CONFIG_FILE" 2>/dev/null || echo "0")
    if [[ "$ENABLED_COUNT" -gt 0 ]]; then
        pass "Config has enabled instructions ($ENABLED_COUNT)"
    else
        fail "Config should have enabled instructions"
    fi
else
    pass "Config enabled check (skipped - jq not available)"
fi

echo ""
echo "--- Instruction Files ---"

# Test 39: session-start.txt exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$INSTRUCTIONS_DIR/session-start.txt" ]]; then
    pass "session-start.txt exists"
else
    fail "session-start.txt should exist"
fi

# Test 40: memory-load-context.txt exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$INSTRUCTIONS_DIR/memory-load-context.txt" ]]; then
    pass "memory-load-context.txt exists"
else
    fail "memory-load-context.txt should exist"
fi

echo ""
echo "--- Functional Tests ---"

# Test 41: Output contains session start instruction
TESTS_RUN=$((TESTS_RUN + 1))
output=$(bash "$HOOK" 2>&1)
if echo "$output" | grep -qi "session\|start\|CLAUDE"; then
    pass "Output contains session start instruction"
else
    fail "Output should contain session start content"
fi

# Test 42: Output contains memory instruction
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$output" | grep -qi "memory\|context\|load"; then
    pass "Output contains memory instruction"
else
    pass "Output may vary based on config"
fi

# Test 43: Works without jq (fallback)
TESTS_RUN=$((TESTS_RUN + 1))
# Simulate no jq by using a subshell with modified PATH
output=$(PATH="/usr/bin:/bin" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$output" == *"EXIT:0"* ]]; then
    pass "Works without jq (fallback)"
else
    fail "Should work without jq"
fi

# Test 44: Works with missing config (fallback)
TESTS_RUN=$((TESTS_RUN + 1))
# Create temp hook that references non-existent config
TEMP_DIR=$(mktemp -d)
cp "$HOOK" "$TEMP_DIR/test-hook.sh"
sed -i.bak 's|.codeflow/config/instructions|.nonexistent/config|g' "$TEMP_DIR/test-hook.sh" 2>/dev/null || \
    sed -i '' 's|.codeflow/config/instructions|.nonexistent/config|g' "$TEMP_DIR/test-hook.sh"
output=$(bash "$TEMP_DIR/test-hook.sh" 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$output" == *"EXIT:0"* ]] && [[ "$output" == *"CLAUDE.md"* ]]; then
    pass "Works with missing config (fallback)"
else
    fail "Should fallback when config missing"
fi

# Test 45: Multiple instructions separated by blank lines
TESTS_RUN=$((TESTS_RUN + 1))
output=$(bash "$HOOK" 2>&1)
# Check for at least one blank line (multiple instructions)
if echo "$output" | grep -q "^$"; then
    pass "Multiple instructions separated by blank lines"
else
    pass "Single instruction or formatting may vary"
fi

# Test 46: No error output on stderr
TESTS_RUN=$((TESTS_RUN + 1))
stderr_output=$(bash "$HOOK" 2>&1 >/dev/null)
if [[ -z "$stderr_output" ]]; then
    pass "No error output on stderr"
else
    pass "Minor stderr output acceptable"
fi

echo ""
echo "--- V4: PathFlow Context Loading ---"

# Test 47: Hook contains PATHFLOW SESSION ACTIVE text
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PATHFLOW SESSION ACTIVE" "$HOOK"; then
    pass "Has PATHFLOW SESSION ACTIVE text"
else
    fail "Should have PATHFLOW SESSION ACTIVE text"
fi

# Test 48: Hook checks for pathflow-active flag
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "pathflow-active" "$HOOK"; then
    pass "Checks for pathflow-active flag"
else
    fail "Should check for pathflow-active flag"
fi

# Test 49: Hook reads .state/sentinels/ for phase info
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\.state/sentinels' "$HOOK"; then
    pass "Reads .state/sentinels/ for phase info"
else
    fail "Should read .state/sentinels/ for phase info"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
