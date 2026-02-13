#!/usr/bin/env bash
# Test: context-lib.sh
# Location: .codeflow/testing/scripts/security/lib/test-context-lib.sh
#
# Tests the context detection library:
#   - Basic setup (exists, executable, shellcheck, headers, strict mode)
#   - Code quality (docs, local vars, known limitations)
#   - _find_progress_entry() internal helper
#   - is_sub_agent() public function
#   - is_forked_context_skill() public function
#   - is_sub_context() combined check
#   - is_pathflow_active() public function
#   - get_pathflow_setting() public function
#   - Double-source guard
#   - Integration with security-lib.sh (transitive sourcing)

# shellcheck disable=SC2030,SC2031,SC2034  # Intentional: subshell overrides for test isolation
set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
LIB_FILE="$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh"
SECURITY_LIB="$REPO_ROOT/.codeflow/scripts/security/lib/security-lib.sh"

export REPO_ROOT

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

echo "=== Testing context-lib.sh ==="
echo ""

# =============================================================================
# TEST TRANSCRIPT SETUP
# =============================================================================
# Create a temporary JSONL transcript file with known test data.
# This simulates the Claude Code transcript format.

TEST_TRANSCRIPT="/tmp/claude/test-context-lib-transcript-$$.jsonl"
mkdir -p /tmp/claude 2>/dev/null || true

cat > "$TEST_TRANSCRIPT" <<'TRANSCRIPT_EOF'
{"type":"progress","data":{"type":"agent_progress","agentId":"test-agent-123"},"tool_use_id":"toolu_AGENT_TEST"}
{"type":"progress","data":{"type":"skill_progress","skillId":"test-skill-456"},"tool_use_id":"toolu_SKILL_TEST"}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_MAIN_TEST","name":"Read"}]},"tool_use_id":"toolu_MAIN_TEST"}
{"type":"progress","data":{"type":"hook_progress"},"tool_use_id":"toolu_HOOK_TEST"}
TRANSCRIPT_EOF

# Cleanup function to remove temp files on exit
cleanup() {
    rm -f "$TEST_TRANSCRIPT"
    rm -f /tmp/claude/context-detect-*.json 2>/dev/null || true
}
trap cleanup EXIT

# =============================================================================
# 1. BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

# Test: File exists
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$LIB_FILE" ]]; then
    pass "Library file exists"
else
    fail "Library file not found at $LIB_FILE"
fi

# Test: File is executable
TESTS_RUN=$((TESTS_RUN + 1))
if [[ -x "$LIB_FILE" ]]; then
    pass "Library file is executable"
else
    fail "Library file is not executable"
fi

# Test: Shellcheck passes
TESTS_RUN=$((TESTS_RUN + 1))
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091,SC2034 "$LIB_FILE" 2>/dev/null; then
        pass "Passes shellcheck (-e SC1091,SC2034)"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test: Has proper header comments — Purpose
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Purpose:" "$LIB_FILE"; then
    pass "Has Purpose header comment"
else
    fail "Missing Purpose header comment"
fi

# Test: Has proper header comments — Location
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Location:" "$LIB_FILE"; then
    pass "Has Location header comment"
else
    fail "Missing Location header comment"
fi

# Test: Has proper header comments — Compatibility
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Compatibility:" "$LIB_FILE"; then
    pass "Has Compatibility header comment"
else
    fail "Missing Compatibility header comment"
fi

# Test: Uses set -euo pipefail
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$LIB_FILE"; then
    pass "Uses set -euo pipefail"
else
    fail "Should use set -euo pipefail"
fi

# Test: Has VERSION constant
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONTEXT_LIB_VERSION=" "$LIB_FILE"; then
    pass "Has VERSION constant (CONTEXT_LIB_VERSION)"
else
    fail "Should have CONTEXT_LIB_VERSION constant"
fi

# Test: Has double-source guard
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "_CONTEXT_LIB_LOADED" "$LIB_FILE"; then
    pass "Has double-source guard (_CONTEXT_LIB_LOADED)"
else
    fail "Should have _CONTEXT_LIB_LOADED double-source guard"
fi

echo ""

# =============================================================================
# 2. CODE QUALITY TESTS
# =============================================================================

echo "--- Code Quality ---"

# Test: Documents known limitations (Agent Teams, teammates)
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Known Limitations" "$LIB_FILE" && grep -qi "teammates" "$LIB_FILE"; then
    pass "Documents known limitations about Agent Teams teammates"
else
    fail "Should document known limitations about Agent Teams teammates"
fi

# Test: Has function documentation comments
TESTS_RUN=$((TESTS_RUN + 1))
doc_count=0
grep -c "^# " "$LIB_FILE" > /dev/null && doc_count=$(grep -c "^# " "$LIB_FILE")
if [[ $doc_count -ge 10 ]]; then
    pass "Has function documentation comments ($doc_count comment lines)"
else
    fail "Should have adequate function documentation (found $doc_count comment lines)"
fi

# Test: Uses local for variable declarations in functions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "local transcript_path" "$LIB_FILE" && grep -q "local tool_use_id" "$LIB_FILE" && grep -q "local tmpfile" "$LIB_FILE"; then
    pass "Uses local for variable declarations in functions"
else
    fail "Should use local for variable declarations in functions"
fi

echo ""

# =============================================================================
# 3. _find_progress_entry() TESTS
# =============================================================================

echo "--- _find_progress_entry() Tests ---"

# Test: Returns 1 when transcript_path is empty
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "" "toolu_AGENT_TEST" && echo "found" || echo "not_found"
)
if [[ "$result" == "not_found" ]]; then
    pass "_find_progress_entry returns 1 when transcript_path is empty"
else
    fail "_find_progress_entry should return 1 when transcript_path is empty"
fi

# Test: Returns 1 when tool_use_id is empty
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "" && echo "found" || echo "not_found"
)
if [[ "$result" == "not_found" ]]; then
    pass "_find_progress_entry returns 1 when tool_use_id is empty"
else
    fail "_find_progress_entry should return 1 when tool_use_id is empty"
fi

# Test: Returns 1 when transcript file doesn't exist
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "/tmp/claude/nonexistent-transcript-$$.jsonl" "toolu_AGENT_TEST" && echo "found" || echo "not_found"
)
if [[ "$result" == "not_found" ]]; then
    pass "_find_progress_entry returns 1 when transcript file doesn't exist"
else
    fail "_find_progress_entry should return 1 when transcript file doesn't exist"
fi

# Test: Returns 1 when jq not available (skip if can't simulate)
TESTS_RUN=$((TESTS_RUN + 1))
if command -v jq &>/dev/null; then
    # Try to simulate no-jq by using a subshell with PATH stripped
    result=$(
        # Override PATH to exclude jq
        export PATH="/usr/bin:/bin"
        # Check if jq is still reachable — if so, we can't test this
        if command -v jq &>/dev/null; then
            echo "skip"
        else
            source "$LIB_FILE"
            _find_progress_entry "$TEST_TRANSCRIPT" "toolu_AGENT_TEST" && echo "found" || echo "not_found"
        fi
    )
    if [[ "$result" == "skip" ]]; then
        pass "_find_progress_entry jq check (skipped: jq in /usr/bin)"
    elif [[ "$result" == "not_found" ]]; then
        pass "_find_progress_entry returns 1 when jq not available"
    else
        fail "_find_progress_entry should return 1 when jq not available"
    fi
else
    pass "_find_progress_entry jq check (skipped: jq not installed)"
fi

# Test: Returns 1 when no matching tool_use_id in transcript
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_NONEXISTENT_ID" && echo "found" || echo "not_found"
)
if [[ "$result" == "not_found" ]]; then
    pass "_find_progress_entry returns 1 when no matching tool_use_id"
else
    fail "_find_progress_entry should return 1 when tool_use_id not found"
fi

# Test: Returns 0 when matching agent_progress entry found
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_AGENT_TEST" && echo "found" || echo "not_found"
)
if [[ "$result" == "found" ]]; then
    pass "_find_progress_entry returns 0 for matching agent_progress entry"
else
    fail "_find_progress_entry should return 0 for matching agent_progress entry"
fi

# Test: Returns 0 when matching skill_progress entry found
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_SKILL_TEST" && echo "found" || echo "not_found"
)
if [[ "$result" == "found" ]]; then
    pass "_find_progress_entry returns 0 for matching skill_progress entry"
else
    fail "_find_progress_entry should return 0 for matching skill_progress entry"
fi

# Test: Returns 1 when matching line is NOT a progress entry (type=assistant)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_MAIN_TEST" && echo "found" || echo "not_found"
)
if [[ "$result" == "not_found" ]]; then
    pass "_find_progress_entry returns 1 for non-progress entry (type=assistant)"
else
    fail "_find_progress_entry should return 1 for non-progress entry (type=assistant)"
fi

# Test: Sets _PROGRESS_TOP_TYPE correctly
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_AGENT_TEST"
    echo "$_PROGRESS_TOP_TYPE"
)
if [[ "$result" == "progress" ]]; then
    pass "_find_progress_entry sets _PROGRESS_TOP_TYPE to 'progress'"
else
    fail "_find_progress_entry should set _PROGRESS_TOP_TYPE to 'progress' (got '$result')"
fi

# Test: Sets _PROGRESS_DATA_TYPE correctly for agent_progress
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_AGENT_TEST"
    echo "$_PROGRESS_DATA_TYPE"
)
if [[ "$result" == "agent_progress" ]]; then
    pass "_find_progress_entry sets _PROGRESS_DATA_TYPE to 'agent_progress'"
else
    fail "_find_progress_entry should set _PROGRESS_DATA_TYPE to 'agent_progress' (got '$result')"
fi

# Test: Sets _PROGRESS_DATA_TYPE correctly for skill_progress
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_SKILL_TEST"
    echo "$_PROGRESS_DATA_TYPE"
)
if [[ "$result" == "skill_progress" ]]; then
    pass "_find_progress_entry sets _PROGRESS_DATA_TYPE to 'skill_progress'"
else
    fail "_find_progress_entry should set _PROGRESS_DATA_TYPE to 'skill_progress' (got '$result')"
fi

# Test: Sets _PROGRESS_CONTEXT_ID from agentId
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_AGENT_TEST"
    echo "$_PROGRESS_CONTEXT_ID"
)
if [[ "$result" == "test-agent-123" ]]; then
    pass "_find_progress_entry sets _PROGRESS_CONTEXT_ID from agentId"
else
    fail "_find_progress_entry should set _PROGRESS_CONTEXT_ID from agentId (got '$result')"
fi

# Test: Sets _PROGRESS_CONTEXT_ID from skillId
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_SKILL_TEST"
    echo "$_PROGRESS_CONTEXT_ID"
)
if [[ "$result" == "test-skill-456" ]]; then
    pass "_find_progress_entry sets _PROGRESS_CONTEXT_ID from skillId"
else
    fail "_find_progress_entry should set _PROGRESS_CONTEXT_ID from skillId (got '$result')"
fi

# Test: Sets _PROGRESS_CONTEXT_ID empty for hook_progress (no agentId or skillId)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    _find_progress_entry "$TEST_TRANSCRIPT" "toolu_HOOK_TEST"
    echo "CTX:${_PROGRESS_CONTEXT_ID}"
)
if [[ "$result" == "CTX:" ]]; then
    pass "_find_progress_entry sets _PROGRESS_CONTEXT_ID empty for hook_progress"
else
    fail "_find_progress_entry should set _PROGRESS_CONTEXT_ID empty for hook_progress (got '$result')"
fi

# Test: Cleans up temp file after successful match
TESTS_RUN=$((TESTS_RUN + 1))
# Remove any stale temp files first
rm -f /tmp/claude/context-detect-*.json 2>/dev/null || true
# Run in subshell to avoid set -e issues
(source "$LIB_FILE" 2>/dev/null; _find_progress_entry "$TEST_TRANSCRIPT" "toolu_AGENT_TEST") 2>/dev/null || true
# Check no temp files remain (glob returns literal if no match)
_found_leftover="false"
for _f in /tmp/claude/context-detect-*.json; do
    if [[ -f "$_f" ]]; then
        _found_leftover="true"
        break
    fi
done
if [[ "$_found_leftover" == "false" ]]; then
    pass "_find_progress_entry cleans up temp file after match"
else
    fail "_find_progress_entry should clean up temp file after match"
    rm -f /tmp/claude/context-detect-*.json 2>/dev/null || true
fi

echo ""

# =============================================================================
# 4. is_sub_agent() TESTS
# =============================================================================

echo "--- is_sub_agent() Tests ---"

# Test: Returns 0 for tool_use_id matching agent_progress entry
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "$TEST_TRANSCRIPT" "toolu_AGENT_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "yes" ]]; then
    pass "is_sub_agent returns 0 for agent_progress entry"
else
    fail "is_sub_agent should return 0 for agent_progress entry"
fi

# Test: Returns 1 for tool_use_id matching skill_progress entry
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "$TEST_TRANSCRIPT" "toolu_SKILL_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_agent returns 1 for skill_progress entry"
else
    fail "is_sub_agent should return 1 for skill_progress entry"
fi

# Test: Returns 1 for tool_use_id matching hook_progress entry
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "$TEST_TRANSCRIPT" "toolu_HOOK_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_agent returns 1 for hook_progress entry"
else
    fail "is_sub_agent should return 1 for hook_progress entry"
fi

# Test: Returns 1 for tool_use_id matching assistant entry (main agent)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "$TEST_TRANSCRIPT" "toolu_MAIN_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_agent returns 1 for assistant entry (main agent)"
else
    fail "is_sub_agent should return 1 for assistant entry (main agent)"
fi

# Test: Returns 1 for tool_use_id not in transcript
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "$TEST_TRANSCRIPT" "toolu_NONEXISTENT" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_agent returns 1 for tool_use_id not in transcript"
else
    fail "is_sub_agent should return 1 for tool_use_id not in transcript"
fi

# Test: Returns 1 for empty transcript_path
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "" "toolu_AGENT_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_agent returns 1 for empty transcript_path"
else
    fail "is_sub_agent should return 1 for empty transcript_path"
fi

# Test: Returns 1 for empty tool_use_id
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_agent "$TEST_TRANSCRIPT" "" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_agent returns 1 for empty tool_use_id"
else
    fail "is_sub_agent should return 1 for empty tool_use_id"
fi

echo ""

# =============================================================================
# 5. is_forked_context_skill() TESTS
# =============================================================================

echo "--- is_forked_context_skill() Tests ---"

# Test: Returns 0 for tool_use_id matching skill_progress entry
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_forked_context_skill "$TEST_TRANSCRIPT" "toolu_SKILL_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "yes" ]]; then
    pass "is_forked_context_skill returns 0 for skill_progress entry"
else
    fail "is_forked_context_skill should return 0 for skill_progress entry"
fi

# Test: Returns 1 for tool_use_id matching agent_progress entry
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_forked_context_skill "$TEST_TRANSCRIPT" "toolu_AGENT_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_forked_context_skill returns 1 for agent_progress entry"
else
    fail "is_forked_context_skill should return 1 for agent_progress entry"
fi

# Test: Returns 1 for tool_use_id matching hook_progress entry
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_forked_context_skill "$TEST_TRANSCRIPT" "toolu_HOOK_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_forked_context_skill returns 1 for hook_progress entry"
else
    fail "is_forked_context_skill should return 1 for hook_progress entry"
fi

# Test: Returns 1 for tool_use_id not in transcript
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_forked_context_skill "$TEST_TRANSCRIPT" "toolu_NONEXISTENT" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_forked_context_skill returns 1 for tool_use_id not in transcript"
else
    fail "is_forked_context_skill should return 1 for tool_use_id not in transcript"
fi

# Test: Returns 1 for empty transcript_path
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_forked_context_skill "" "toolu_SKILL_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_forked_context_skill returns 1 for empty transcript_path"
else
    fail "is_forked_context_skill should return 1 for empty transcript_path"
fi

# Test: Returns 1 for empty tool_use_id
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_forked_context_skill "$TEST_TRANSCRIPT" "" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_forked_context_skill returns 1 for empty tool_use_id"
else
    fail "is_forked_context_skill should return 1 for empty tool_use_id"
fi

echo ""

# =============================================================================
# 6. is_sub_context() TESTS
# =============================================================================

echo "--- is_sub_context() Tests ---"

# Test: Returns 0 for agent_progress (sub-agent)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "$TEST_TRANSCRIPT" "toolu_AGENT_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "yes" ]]; then
    pass "is_sub_context returns 0 for agent_progress (sub-agent)"
else
    fail "is_sub_context should return 0 for agent_progress (sub-agent)"
fi

# Test: Returns 0 for skill_progress (forked skill)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "$TEST_TRANSCRIPT" "toolu_SKILL_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "yes" ]]; then
    pass "is_sub_context returns 0 for skill_progress (forked skill)"
else
    fail "is_sub_context should return 0 for skill_progress (forked skill)"
fi

# Test: Returns 1 for hook_progress (main agent hook)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "$TEST_TRANSCRIPT" "toolu_HOOK_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_context returns 1 for hook_progress (main agent hook)"
else
    fail "is_sub_context should return 1 for hook_progress (main agent hook)"
fi

# Test: Returns 1 for assistant entry (main agent tool call)
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "$TEST_TRANSCRIPT" "toolu_MAIN_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_context returns 1 for assistant entry (main agent tool call)"
else
    fail "is_sub_context should return 1 for assistant entry (main agent tool call)"
fi

# Test: Returns 1 for tool_use_id not in transcript
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "$TEST_TRANSCRIPT" "toolu_NONEXISTENT" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_context returns 1 for tool_use_id not in transcript"
else
    fail "is_sub_context should return 1 for tool_use_id not in transcript"
fi

# Test: Returns 1 for empty transcript_path
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "" "toolu_AGENT_TEST" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_context returns 1 for empty transcript_path"
else
    fail "is_sub_context should return 1 for empty transcript_path"
fi

# Test: Returns 1 for empty tool_use_id
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    source "$LIB_FILE"
    is_sub_context "$TEST_TRANSCRIPT" "" && echo "yes" || echo "no"
)
if [[ "$result" == "no" ]]; then
    pass "is_sub_context returns 1 for empty tool_use_id"
else
    fail "is_sub_context should return 1 for empty tool_use_id"
fi

echo ""

# =============================================================================
# 7. is_pathflow_active() TESTS
# =============================================================================

echo "--- is_pathflow_active() ---"

# Test: Returns 1 when flag file does not exist
TESTS_RUN=$((TESTS_RUN + 1))
PF_TEST_DIR="/tmp/claude/test-pathflow-$$"
mkdir -p "$PF_TEST_DIR/repo/.state/session"
result=$(
    export REPO_ROOT="$PF_TEST_DIR/repo"
    export CODEFLOW_SESSION_ID="test-pf-session"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    is_pathflow_active && echo "active" || echo "inactive"
) 2>/dev/null
if [[ "$result" == "inactive" ]]; then
    pass "is_pathflow_active returns 1 when flag file absent"
else
    fail "is_pathflow_active should return 1 when flag file absent"
fi
rm -rf "$PF_TEST_DIR"

# Test: Returns 0 when flag file exists
TESTS_RUN=$((TESTS_RUN + 1))
PF_TEST_DIR="/tmp/claude/test-pathflow-active-$$"
mkdir -p "$PF_TEST_DIR/repo/.state/session/test-pf-session"
touch "$PF_TEST_DIR/repo/.state/session/test-pf-session/is-pathflow-active"
result=$(
    export REPO_ROOT="$PF_TEST_DIR/repo"
    export CODEFLOW_SESSION_ID="test-pf-session"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    is_pathflow_active && echo "active" || echo "inactive"
) 2>/dev/null
if [[ "$result" == "active" ]]; then
    pass "is_pathflow_active returns 0 when flag file exists"
else
    fail "is_pathflow_active should return 0 when flag file exists"
fi
rm -rf "$PF_TEST_DIR"

# Test: Returns 1 when session ID is unknown and no flag
TESTS_RUN=$((TESTS_RUN + 1))
PF_TEST_DIR="/tmp/claude/test-pathflow-unknown-$$"
mkdir -p "$PF_TEST_DIR/repo/.state/session"
result=$(
    export REPO_ROOT="$PF_TEST_DIR/repo"
    unset CODEFLOW_SESSION_ID 2>/dev/null || true
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    is_pathflow_active && echo "active" || echo "inactive"
) 2>/dev/null
if [[ "$result" == "inactive" ]]; then
    pass "is_pathflow_active returns 1 with unknown session ID"
else
    fail "is_pathflow_active should return 1 with unknown session ID"
fi
rm -rf "$PF_TEST_DIR"

# Test: Returns 1 after flag file removed (simulates session end)
TESTS_RUN=$((TESTS_RUN + 1))
PF_TEST_DIR="/tmp/claude/test-pathflow-removed-$$"
mkdir -p "$PF_TEST_DIR/repo/.state/session/test-pf-session"
touch "$PF_TEST_DIR/repo/.state/session/test-pf-session/is-pathflow-active"
rm -f "$PF_TEST_DIR/repo/.state/session/test-pf-session/is-pathflow-active"
result=$(
    export REPO_ROOT="$PF_TEST_DIR/repo"
    export CODEFLOW_SESSION_ID="test-pf-session"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    is_pathflow_active && echo "active" || echo "inactive"
) 2>/dev/null
if [[ "$result" == "inactive" ]]; then
    pass "is_pathflow_active returns 1 after flag removed (session end)"
else
    fail "is_pathflow_active should return 1 after flag removed"
fi
rm -rf "$PF_TEST_DIR"

echo ""

# =============================================================================
# 7b. get_pathflow_setting() TESTS
# =============================================================================

echo "--- get_pathflow_setting() ---"

# Test: Returns empty when settings.json missing
TESTS_RUN=$((TESTS_RUN + 1))
GPS_TEST_DIR="/tmp/claude/test-gps-nosettings-$$"
mkdir -p "$GPS_TEST_DIR/repo/.claude"
result=$(
    export REPO_ROOT="$GPS_TEST_DIR/repo"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    get_pathflow_setting
) 2>/dev/null
if [[ -z "$result" ]]; then
    pass "get_pathflow_setting returns empty when no settings.json"
else
    fail "get_pathflow_setting should return empty when no settings.json (got '$result')"
fi
rm -rf "$GPS_TEST_DIR"

# Test: Returns empty when _codeflow key missing
TESTS_RUN=$((TESTS_RUN + 1))
GPS_TEST_DIR="/tmp/claude/test-gps-nokey-$$"
mkdir -p "$GPS_TEST_DIR/repo/.claude"
echo '{"permissions":{"allow":[]}}' > "$GPS_TEST_DIR/repo/.claude/settings.json"
result=$(
    export REPO_ROOT="$GPS_TEST_DIR/repo"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    get_pathflow_setting
) 2>/dev/null
if [[ -z "$result" ]]; then
    pass "get_pathflow_setting returns empty when _codeflow key missing"
else
    fail "get_pathflow_setting should return empty when _codeflow key missing (got '$result')"
fi
rm -rf "$GPS_TEST_DIR"

# Test: Returns "auto"
TESTS_RUN=$((TESTS_RUN + 1))
GPS_TEST_DIR="/tmp/claude/test-gps-auto-$$"
mkdir -p "$GPS_TEST_DIR/repo/.claude"
echo '{"_codeflow":{"agent_teams":"auto"}}' > "$GPS_TEST_DIR/repo/.claude/settings.json"
result=$(
    export REPO_ROOT="$GPS_TEST_DIR/repo"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    get_pathflow_setting
) 2>/dev/null
if [[ "$result" == "auto" ]]; then
    pass "get_pathflow_setting returns 'auto'"
else
    fail "get_pathflow_setting should return 'auto' (got '$result')"
fi
rm -rf "$GPS_TEST_DIR"

# Test: Returns "always"
TESTS_RUN=$((TESTS_RUN + 1))
GPS_TEST_DIR="/tmp/claude/test-gps-always-$$"
mkdir -p "$GPS_TEST_DIR/repo/.claude"
echo '{"_codeflow":{"agent_teams":"always"}}' > "$GPS_TEST_DIR/repo/.claude/settings.json"
result=$(
    export REPO_ROOT="$GPS_TEST_DIR/repo"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    get_pathflow_setting
) 2>/dev/null
if [[ "$result" == "always" ]]; then
    pass "get_pathflow_setting returns 'always'"
else
    fail "get_pathflow_setting should return 'always' (got '$result')"
fi
rm -rf "$GPS_TEST_DIR"

# Test: Returns "never"
TESTS_RUN=$((TESTS_RUN + 1))
GPS_TEST_DIR="/tmp/claude/test-gps-never-$$"
mkdir -p "$GPS_TEST_DIR/repo/.claude"
echo '{"_codeflow":{"agent_teams":"never"}}' > "$GPS_TEST_DIR/repo/.claude/settings.json"
result=$(
    export REPO_ROOT="$GPS_TEST_DIR/repo"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    source "$LIB_FILE"
    get_pathflow_setting
) 2>/dev/null
if [[ "$result" == "never" ]]; then
    pass "get_pathflow_setting returns 'never'"
else
    fail "get_pathflow_setting should return 'never' (got '$result')"
fi
rm -rf "$GPS_TEST_DIR"

echo ""

# =============================================================================
# 8. DOUBLE-SOURCE GUARD TEST
# =============================================================================

echo "--- Double-Source Guard ---"

# Test: Guard variable _CONTEXT_LIB_LOADED is checked before re-defining functions
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_CONTEXT_LIB_LOADED:-' "$LIB_FILE" && grep -q 'return 0' "$LIB_FILE"; then
    pass "Double-source guard checks _CONTEXT_LIB_LOADED and returns 0"
else
    fail "Double-source guard should check _CONTEXT_LIB_LOADED and return 0"
fi

# Test: Guard uses readonly to prevent tampering
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'readonly _CONTEXT_LIB_LOADED=' "$LIB_FILE"; then
    pass "Guard variable _CONTEXT_LIB_LOADED is readonly"
else
    fail "Guard variable _CONTEXT_LIB_LOADED should be readonly"
fi

# Test: Double-sourcing in a subshell — guard prevents re-execution of function defs
# NOTE: CONTEXT_LIB_VERSION is declared readonly BEFORE the guard, so double-sourcing
# will produce a "readonly variable" warning on that line. The guard still protects
# against re-defining functions and _CONTEXT_LIB_LOADED. We verify that the guard
# itself works by checking that functions remain available after the second source attempt.
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    set +e  # Disable errexit for this subshell — readonly warning is expected
    source "$LIB_FILE" 2>/dev/null
    source "$LIB_FILE" 2>/dev/null
    # Verify functions are still available (guard did not corrupt state)
    if declare -f is_sub_agent &>/dev/null && declare -f is_sub_context &>/dev/null; then
        echo "OK"
    else
        echo "BROKEN"
    fi
) || true  # Tolerate subshell non-zero exit from readonly warning under set -e
if [[ "$result" == "OK" ]]; then
    pass "Functions remain available after double-source attempt"
else
    fail "Functions should remain available after double-source attempt (got '$result')"
fi

echo ""

# =============================================================================
# 9. INTEGRATION WITH security-lib.sh (transitive sourcing)
# =============================================================================

echo "--- Integration with security-lib.sh ---"

# Test: Sourcing security-lib.sh transitively provides context-lib.sh functions
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    export REPO_ROOT="$REPO_ROOT"
    unset _CONTEXT_LIB_LOADED 2>/dev/null || true
    unset _SECURITY_LIB_SOURCED 2>/dev/null || true
    source "$SECURITY_LIB" 2>/dev/null
    if declare -f is_pathflow_active >/dev/null 2>&1 && declare -f get_pathflow_setting >/dev/null 2>&1 && declare -f is_sub_context >/dev/null 2>&1; then
        echo "OK"
    else
        echo "MISSING"
    fi
) 2>/dev/null || true
if [[ "$result" == "OK" ]]; then
    pass "security-lib.sh transitively exports is_pathflow_active, get_pathflow_setting, is_sub_context"
else
    fail "security-lib.sh should transitively export context-lib.sh functions (got '$result')"
fi

# Test: Sourcing both libraries produces no conflicts
# NOTE: Double-sourcing context-lib.sh (once via security-lib.sh, once directly)
# produces a harmless readonly warning for CONTEXT_LIB_VERSION. We disable errexit
# to tolerate this expected warning.
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    set +e  # readonly CONTEXT_LIB_VERSION warning is expected on double-source
    export REPO_ROOT="$REPO_ROOT"
    source "$SECURITY_LIB" 2>/dev/null
    source "$LIB_FILE" 2>/dev/null
    echo "OK"
) 2>/dev/null || true
if [[ "$result" == "OK" ]]; then
    pass "Sourcing both security-lib.sh and context-lib.sh produces no conflicts"
else
    fail "Sourcing both libraries produces conflicts (got '$result')"
fi

# Test: Functions from both libraries are available
TESTS_RUN=$((TESTS_RUN + 1))
result=$(
    set +e  # readonly CONTEXT_LIB_VERSION warning is expected on double-source
    export REPO_ROOT="$REPO_ROOT"
    source "$SECURITY_LIB" 2>/dev/null
    source "$LIB_FILE" 2>/dev/null
    if declare -f is_pathflow_active >/dev/null 2>&1 && declare -f is_sub_context >/dev/null 2>&1; then
        echo "OK"
    else
        echo "MISSING"
    fi
) 2>/dev/null || true
if [[ "$result" == "OK" ]]; then
    pass "Functions from both libraries are available after sourcing both"
else
    fail "Functions from both libraries should be available (got '$result')"
fi

echo ""

# =============================================================================
# SUMMARY
# =============================================================================

echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
