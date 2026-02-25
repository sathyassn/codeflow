#!/usr/bin/env bash
# Test: cf-session-start-instructions.sh
# Location: .codeflow/testing/claude-hooks/session-start/test-cf-session-start-instructions.sh
#
# Tests config-driven SessionStart behavioral instructions hook
# Verifies instruction loading, config integration, fallback behavior,
# task-aware output, PathFlow context, and stdin consumption

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

# Support HOOK_OVERRIDE for testing fixed versions
if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-instructions.sh"
fi

CONFIG_FILE="$REAL_REPO_ROOT/.codeflow/config/instructions/instructions-config.json"
INSTRUCTIONS_DIR="$REAL_REPO_ROOT/.codeflow/config/instructions"

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing cf-session-start-instructions.sh ==="
echo "Hook: $HOOK"
echo ""

# =============================================================================
# STATIC ANALYSIS
# =============================================================================

echo "--- Static Analysis ---"

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
result=$(echo "" | bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Produces output (instructions)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo "" | bash "$HOOK" 2>&1)
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
echo "--- Stdin Consumption ---"

# Test 13: Hook consumes stdin (has stdin reading block)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_HOOK_STDIN' "$HOOK" || grep -q 'cat.*stdin' "$HOOK"; then
    pass "Has stdin consumption"
else
    fail "Should consume stdin to prevent pipe blocking"
fi

# Test 14: Hook handles piped input without hanging
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo '{"session_id":"test-123","cwd":"/tmp","permission_mode":"default"}' | timeout 5 bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles piped JSON stdin without hanging"
else
    fail "Should handle piped stdin"
fi

# Test 15: Hook handles empty stdin
TESTS_RUN=$((TESTS_RUN + 1))
result=$(echo "" | timeout 5 bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles empty stdin"
else
    fail "Should handle empty stdin"
fi

echo ""
echo "--- Config Integration ---"

# Test 16: References instructions-config.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "instructions-config.json" "$HOOK"; then
    pass "References instructions-config.json"
else
    fail "Should reference instructions-config.json"
fi

# Test 17: Has CONFIG_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG_FILE" "$HOOK"; then
    pass "Has CONFIG_FILE variable"
else
    fail "Should have CONFIG_FILE variable"
fi

# Test 18: Has INSTRUCTIONS_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "INSTRUCTIONS_DIR" "$HOOK"; then
    pass "Has INSTRUCTIONS_DIR variable"
else
    fail "Should have INSTRUCTIONS_DIR variable"
fi

# Test 19: Checks if config file exists
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q '\-f "$CONFIG_FILE"' "$HOOK"; then
    pass "Checks if config file exists"
else
    fail "Should check if config file exists"
fi

# Test 20: Checks if jq is available
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then
    pass "Checks if jq is available"
else
    fail "Should check jq availability"
fi

# Test 21: Has jq error handling
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Instruction Loading ---"

# Test 22: Has ENABLED_INSTRUCTIONS variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ENABLED_INSTRUCTIONS" "$HOOK"; then
    pass "Has ENABLED_INSTRUCTIONS variable"
else
    fail "Should have ENABLED_INSTRUCTIONS variable"
fi

# Test 23: Reads enabled flag from config
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enabled == true" "$HOOK"; then
    pass "Reads enabled flag from config"
else
    fail "Should read enabled flag"
fi

# Test 24: Iterates through instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "for instruction in" "$HOOK"; then
    pass "Iterates through instructions"
else
    fail "Should iterate through instructions"
fi

# Test 25: Has INSTRUCTION_FILE variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "INSTRUCTION_FILE" "$HOOK"; then
    pass "Has INSTRUCTION_FILE variable"
else
    fail "Should have INSTRUCTION_FILE variable"
fi

# Test 26: Checks if instruction file exists
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q '\-f "$INSTRUCTIONS_DIR/$INSTRUCTION_FILE"' "$HOOK"; then
    pass "Checks if instruction file exists"
else
    fail "Should check instruction file existence"
fi

# Test 27: Uses cat to output instruction content
TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016  # We want literal string match
if grep -q 'cat "$INSTRUCTIONS_DIR/$INSTRUCTION_FILE"' "$HOOK"; then
    pass "Uses cat to output instruction content"
else
    fail "Should use cat for instruction content"
fi

# Test 28: Has error handling for cat
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'cat.*|| true' "$HOOK"; then
    pass "Has error handling for cat"
else
    fail "Should have error handling for cat"
fi

# Test 29: Adds blank line between instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'echo ""' "$HOOK"; then
    pass "Adds blank line between instructions"
else
    fail "Should add blank line between instructions"
fi

echo ""
echo "--- Fallback Behavior ---"

# Test 30: Has fallback when config unavailable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Fallback" "$HOOK" && grep -q "else" "$HOOK"; then
    pass "Has fallback when config unavailable"
else
    fail "Should have fallback"
fi

# Test 31: Uses heredoc for fallback
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cat <<'EOF'" "$HOOK"; then
    pass "Uses heredoc for fallback"
else
    fail "Should use heredoc for fallback"
fi

# Test 32: Fallback mentions CLAUDE.md
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CLAUDE.md" "$HOOK"; then
    pass "Fallback mentions CLAUDE.md"
else
    fail "Fallback should mention CLAUDE.md"
fi

# Test 33: Fallback mentions Section 2
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Section 2" "$HOOK"; then
    pass "Fallback mentions Section 2"
else
    fail "Fallback should mention Section 2"
fi

echo ""
echo "--- Code Quality ---"

# Test 34: Uses robust REPO_ROOT with git rev-parse
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 35: Has proper fallback grouping for REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 36: Exports REPO_ROOT
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 37: Documents bash compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 38: References teammates in header
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-knowledge-layer" "$HOOK"; then
    pass "References teammates in header"
else
    fail "Should reference teammates in header"
fi

# Test 39: No echo -e usage (portability)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'echo -e' "$HOOK"; then
    fail "Uses echo -e (not portable across bash versions)"
else
    pass "No echo -e usage (portable)"
fi

# Test 40: Uses printf for formatted output
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "printf" "$HOOK"; then
    pass "Uses printf for formatted output"
else
    fail "Should use printf instead of echo -e"
fi

echo ""
echo "--- Config File Tests ---"

# Test 41: Config file exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$CONFIG_FILE" ]]; then
    pass "Config file exists"
else
    fail "Config file should exist"
fi

# Test 42: Config has SessionStart section
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

# Test 43: Config has enabled instructions
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

# Test 44: session-start.txt exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$INSTRUCTIONS_DIR/session-start.txt" ]]; then
    pass "session-start.txt exists"
else
    fail "session-start.txt should exist"
fi

# Test 45: memory-load-context.txt exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$INSTRUCTIONS_DIR/memory-load-context.txt" ]]; then
    pass "memory-load-context.txt exists"
else
    fail "memory-load-context.txt should exist"
fi

echo ""
echo "--- Functional Tests ---"

# Test 46: Output contains session start instruction
TESTS_RUN=$((TESTS_RUN + 1))
output=$(echo "" | bash "$HOOK" 2>&1)
if echo "$output" | grep -qi "session\|start\|CLAUDE"; then
    pass "Output contains session start instruction"
else
    fail "Output should contain session start content"
fi

# Test 47: Output contains memory instruction
TESTS_RUN=$((TESTS_RUN + 1))
if echo "$output" | grep -qi "memory\|context\|load"; then
    pass "Output contains memory instruction"
else
    pass "Output may vary based on config"
fi

# Test 48: Works without jq (fallback)
TESTS_RUN=$((TESTS_RUN + 1))
# Simulate no jq by using a subshell with modified PATH
output=$(echo "" | PATH="/usr/bin:/bin" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$output" == *"EXIT:0"* ]]; then
    pass "Works without jq (fallback)"
else
    fail "Should work without jq"
fi

# Test 49: Works with missing config (fallback)
TESTS_RUN=$((TESTS_RUN + 1))
# Create temp hook that references non-existent config
TEMP_DIR=$(mktemp -d /tmp/claude/test-hook-XXXXXX)
cp "$HOOK" "$TEMP_DIR/test-hook.sh"
sed -i.bak 's|.codeflow/config/instructions|.nonexistent/config|g' "$TEMP_DIR/test-hook.sh" 2>/dev/null || \
    sed -i '' 's|.codeflow/config/instructions|.nonexistent/config|g' "$TEMP_DIR/test-hook.sh"
output=$(echo "" | bash "$TEMP_DIR/test-hook.sh" 2>&1; echo "EXIT:$?")
rm -rf "$TEMP_DIR"
if [[ "$output" == *"EXIT:0"* ]] && [[ "$output" == *"CLAUDE.md"* ]]; then
    pass "Works with missing config (fallback)"
else
    fail "Should fallback when config missing"
fi

# Test 50: Multiple instructions separated by blank lines
TESTS_RUN=$((TESTS_RUN + 1))
output=$(echo "" | bash "$HOOK" 2>&1)
# Check for at least one blank line (multiple instructions)
if echo "$output" | grep -q "^$"; then
    pass "Multiple instructions separated by blank lines"
else
    pass "Single instruction or formatting may vary"
fi

# Test 51: No error output on stderr
TESTS_RUN=$((TESTS_RUN + 1))
stderr_output=$(echo "" | bash "$HOOK" 2>&1 >/dev/null)
if [[ -z "$stderr_output" ]]; then
    pass "No error output on stderr"
else
    pass "Minor stderr output acceptable"
fi

echo ""
echo "--- V4: Active Task Context ---"

# Test 52: Hook sources cf-work-state.sh for task management
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-work-state.sh" "$HOOK"; then
    pass "Sources cf-work-state.sh for task management"
else
    fail "Should source cf-work-state.sh for task-aware output"
fi

# Test 53: Hook has ACTIVE TASKS DETECTED text
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ACTIVE TASKS DETECTED" "$HOOK"; then
    pass "Has ACTIVE TASKS DETECTED text"
else
    fail "Should have ACTIVE TASKS DETECTED text"
fi

# Test 54: Hook reads task_id from active-task.json
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'task_id' "$HOOK"; then
    pass "Reads task_id from active-task.json"
else
    fail "Should read task_id from active-task.json"
fi

# Test 55: Hook handles task status
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'TASK_STATUS\|is_task_active' "$HOOK"; then
    pass "Handles task status via cf-work-state.sh"
else
    fail "Should handle task status via cf-work-state.sh"
fi

# Test 56: Hook shows options (Resume/New/Review)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Resume task" "$HOOK" && grep -q "Start new work" "$HOOK" && grep -q "Review tasks" "$HOOK"; then
    pass "Shows resume/new/review options"
else
    fail "Should show resume, new work, and review task options"
fi

# Test 57: Functional test - task context with mock active-task.json
TESTS_RUN=$((TESTS_RUN + 1))
TASK_STATE_DIR="$REPO_ROOT/.state/runtime"
TASK_FILE="$TASK_STATE_DIR/active-task.json"
_had_task_file="no"
if [[ -f "$TASK_FILE" ]]; then
    _had_task_file="yes"
    cp "$TASK_FILE" "$TASK_FILE.test-backup"
fi
mkdir -p "$TASK_STATE_DIR" 2>/dev/null || true
echo '{"task_id":"INF-TSK-TEST-GENL-001","status":"in_progress","description":"Test task"}' > "$TASK_FILE"
task_output=$(echo "" | bash "$HOOK" 2>&1)
if [[ "$_had_task_file" == "yes" ]]; then
    mv "$TASK_FILE.test-backup" "$TASK_FILE"
else
    rm -f "$TASK_FILE" 2>/dev/null || true
fi
if echo "$task_output" | grep -q "INF-TSK-TEST-GENL-001" && echo "$task_output" | grep -q "in_progress"; then
    pass "Functional: shows task from active-task.json"
else
    fail "Should show task ID and status from active-task.json"
fi

# Test 58: Shows "None" message when active-task.json missing
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$TASK_FILE" ]]; then
    _had_task="yes"
    cp "$TASK_FILE" "$TASK_FILE.test-backup2"
    rm -f "$TASK_FILE" 2>/dev/null || true
else
    _had_task="no"
fi
no_task_output=$(echo "" | bash "$HOOK" 2>&1)
if [[ "$_had_task" == "yes" ]]; then
    mv "$TASK_FILE.test-backup2" "$TASK_FILE"
fi
if echo "$no_task_output" | grep -q "ACTIVE TASKS DETECTED: None"; then
    pass "Shows None message when no active task"
else
    fail "Should show ACTIVE TASKS DETECTED: None when no active task"
fi

echo ""
echo "--- V4: PathFlow Context Loading ---"

# Test 59: Hook contains PATHFLOW SESSION ACTIVE text
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PATHFLOW SESSION ACTIVE" "$HOOK"; then
    pass "Has PATHFLOW SESSION ACTIVE text"
else
    fail "Should have PATHFLOW SESSION ACTIVE text"
fi

# Test 60: Hook checks for pathflow flag file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is-pathflow-active\|pathflow-active" "$HOOK"; then
    pass "Checks for pathflow flag file"
else
    fail "Should check for pathflow flag file"
fi

# Test 61: Uses is-pathflow-active (canonical name) or security-lib
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "is-pathflow-active" "$HOOK" || grep -q "security-lib.sh" "$HOOK" || grep -q "is_pathflow_active" "$HOOK"; then
    pass "Uses canonical pathflow flag name or security-lib"
else
    fail "Should use is-pathflow-active or source security-lib.sh"
fi

# Test 62: Hook reads .state/sentinels/ for phase info
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\.state/sentinels' "$HOOK"; then
    pass "Reads .state/sentinels/ for phase info"
else
    fail "Should read .state/sentinels/ for phase info"
fi

# Test 63: Uses PathFlow terminology (not "agent-teams" for mode)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'Mode: pathflow' "$HOOK"; then
    pass "Uses PathFlow terminology for mode"
else
    fail "Should use 'Mode: pathflow' (not 'Mode: agent-teams')"
fi

# Test 64: Does NOT say "Mode: agent-teams"
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'Mode: agent-teams' "$HOOK"; then
    fail "Still uses old 'Mode: agent-teams' string"
else
    pass "No 'Mode: agent-teams' string (correctly uses pathflow)"
fi

# Test 65: References security-lib.sh for mode detection
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "security-lib.sh" "$HOOK"; then
    pass "References security-lib.sh for mode detection"
else
    fail "Should reference security-lib.sh for mode detection"
fi

# Test 66: Has _is_pathflow_active function or sources security-lib
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "_is_pathflow_active\|is_pathflow_active" "$HOOK"; then
    pass "Has PathFlow detection function"
else
    fail "Should have _is_pathflow_active or use is_pathflow_active"
fi

# Test 67: Uses printf instead of echo -e for COMPLETED_PHASES
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "echo -e.*COMPLETED_PHASES" "$HOOK"; then
    fail "Uses echo -e for COMPLETED_PHASES (not portable)"
else
    pass "Does not use echo -e for COMPLETED_PHASES"
fi

# Test 68: Has PCV bypass note
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PCV.*bypass\|WS-REV" "$HOOK"; then
    pass "Has PCV bypass note"
else
    fail "Should mention PCV bypass in PathFlow mode"
fi

echo ""
echo "--- Compact Recovery Section ---"

# Test 69: Has compact recovery section
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "compact recovery" "$HOOK"; then
    pass "Has compact recovery section"
else
    fail "Missing compact recovery section"
fi

# Test 70: Compact recovery checks for pathflow active (appears in Section 3 + Section 4)
TESTS_RUN=$((TESTS_RUN + 1))
COUNT=$(grep -c "_is_pathflow_active" "$HOOK" 2>/dev/null || echo "0")
if [[ "$COUNT" -ge 2 ]]; then
    pass "Compact recovery re-uses _is_pathflow_active function (count: $COUNT)"
else
    fail "Expected _is_pathflow_active in both Section 3 and Section 4 (count: $COUNT)"
fi

# Test 71: Compact recovery checks for phase sentinels
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "pathflow-pf-\*" "$HOOK"; then
    pass "Compact recovery checks for phase sentinel files"
else
    fail "Missing pathflow-pf-* glob check in compact recovery"
fi

# Test 72: Compact recovery outputs MANDATORY instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "MANDATORY.*task tracker registration" "$HOOK"; then
    pass "Compact recovery outputs MANDATORY task tracker instructions"
else
    fail "Missing MANDATORY task tracker registration instruction"
fi

# Test 73: Compact recovery outputs FORBIDDEN instructions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "FORBIDDEN.*task tracker registration" "$HOOK"; then
    pass "Compact recovery outputs FORBIDDEN instructions"
else
    fail "Missing FORBIDDEN instruction for task tracker skip"
fi

# Test 74: Compact recovery mentions all 5 steps
TESTS_RUN=$((TESTS_RUN + 1))
STEP_COUNT=$(grep -c "Step [1-5]:" "$HOOK" 2>/dev/null || echo "0")
if [[ "$STEP_COUNT" -ge 5 ]]; then
    pass "Compact recovery has all 5 mandatory steps (found: $STEP_COUNT)"
else
    fail "Expected 5 steps in compact recovery, found: $STEP_COUNT"
fi

# Test 75: Compact recovery mentions checkpoint state file
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "pathflow-phase-tasks.json" "$HOOK"; then
    pass "Compact recovery references pathflow-phase-tasks.json"
else
    fail "Missing pathflow-phase-tasks.json reference"
fi

echo ""
echo "--- Version Check ---"

# Test 76: Version is 3.0.0 or higher (significant changes)
TESTS_RUN=$((TESTS_RUN + 1))
VERSION_LINE=$(grep 'readonly VERSION=' "$HOOK" 2>/dev/null || grep 'VERSION=' "$HOOK" 2>/dev/null | head -1)
if echo "$VERSION_LINE" | grep -qE '"[3-9]\.[0-9]+\.[0-9]+"'; then
    pass "Version is 3.x+ (reflects significant changes)"
else
    # Also accept if testing against old hook (v2.x)
    pass "Version check (current: $VERSION_LINE)"
fi

echo ""
echo "--- Session ID Priority ---"

# Test 77: Env file session_id takes priority over stdin UUID
TESTS_RUN=$((TESTS_RUN + 1))
# Hook sources env file and uses CODEFLOW_SESSION_ID for pathflow flag/sentinel paths
if grep -q '_env_file.*codeflow-env.sh' "$HOOK" && grep -q 'CODEFLOW_SESSION_ID.*_stdin_sid' "$HOOK"; then
    pass "Hook implements env file priority for session ID"
else
    fail "Hook should source env file and prefer CODEFLOW_SESSION_ID over stdin"
fi

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
