#!/usr/bin/env bash
# Test: cf-session-end-cleanup.sh
# Location: .codeflow/testing/claude-hooks/session-end/test-cf-session-end-cleanup.sh
#
# Tests SessionEnd cleanup hook
# Verifies proper cleanup of sentinels, state files, and temp files
#
# Compatibility: bash 3.2+ (macOS compatible)

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

# Allow override for testing the fixed version
if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh"
fi

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1${2:+ ($2)}"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

# Setup test directories
setup_test_dirs() {
    mkdir -p "$REPO_ROOT/.state/sentinels/skill" 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/session" 2>/dev/null || true
    mkdir -p /tmp/claude/codeflow 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/sentinels" 2>/dev/null || true
}

# Cleanup test artifacts
cleanup_test_artifacts() {
    rm -f "$REPO_ROOT/.state/sentinels/skill"/test-*.json 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/session"/*-test-session* 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/session"/memory-progress-* 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/runtime/active-task.json" 2>/dev/null || true
    rm -rf "$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-test-session}/pathflow" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/sentinels"/pathflow-* 2>/dev/null || true
    rm -rf /tmp/claude/codeflow 2>/dev/null || true
}

# Helper: create a sentinel JSON with specific expiry
create_sentinel() {
    local name="$1"
    local expires="$2"
    local dir="${3:-$REPO_ROOT/.state/sentinels/skill}"
    cat > "$dir/$name" <<EOF
{"skill":"test","operation":"test","expires":$expires,"created":$(date +%s)}
EOF
}

# Helper: run hook with optional stdin JSON
run_hook() {
    local session_id="${1:-test-session}"
    local stdin_json="${2:-}"
    if [[ -n "$stdin_json" ]]; then
        echo "$stdin_json" | CODEFLOW_SESSION_ID="$session_id" bash "$HOOK" 2>&1
    else
        CODEFLOW_SESSION_ID="$session_id" bash "$HOOK" </dev/null 2>&1
    fi
}

echo "=== Testing cf-session-end-cleanup.sh ==="
echo ""

# ═══════════════════════════════════════════════════════════════════════════════
# SECTION: Hook Basics
# ═══════════════════════════════════════════════════════════════════════════════

echo "--- Hook Basics ---"

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

# Test 4: Has proper header comments
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 5: Uses set -euo pipefail
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 6: Has VERSION constant
if grep -q "VERSION=" "$HOOK" || grep -q "readonly VERSION" "$HOOK"; then
    pass "Has VERSION constant"
else
    fail "Should have VERSION constant"
fi

# Test 7: Has Hook Type header
if grep -q "Hook Type:" "$HOOK"; then
    pass "Has Hook Type header"
else
    fail "Should have Hook Type header"
fi

# Test 8: Has Location header
if grep -q "Location:" "$HOOK"; then
    pass "Has Location header"
else
    fail "Should have Location header"
fi

echo ""
echo "--- Execution Tests ---"

# Test 9: Exits 0 on execution
result=$(bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 on execution"
else
    fail "Should exit 0"
fi

# Test 10: Exits 0 with session ID
result=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 with session ID"
else
    fail "Should exit 0 with session ID"
fi

# Test 11: Exits 0 without session ID
result=$(bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 without session ID"
else
    fail "Should exit 0 without session ID"
fi

# Test 12: Has exit 0 at end
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Has exit 0 at end"
else
    fail "Should have exit 0 at end"
fi

echo ""
echo "--- Sentinel Cleanup ---"

# Test 13: Has sentinel cleanup logic
if grep -qE "sentinel|SENTINEL" "$HOOK"; then
    pass "Has sentinel cleanup logic"
else
    fail "Should cleanup sentinels"
fi

# Test 14: References SENTINEL_DIR
if grep -q "SENTINEL_DIR" "$HOOK"; then
    pass "References SENTINEL_DIR"
else
    fail "Should reference SENTINEL_DIR"
fi

# Test 15: Uses expired-only sentinel cleanup (not rm -f *.json)
if grep -q "sentinel.*clean\|expired\|_sentinels_cleaned" "$HOOK"; then
    pass "Uses expired-only sentinel cleanup"
else
    fail "Should use expired-only sentinel cleanup"
fi

# Test 16: Reads sentinel directory from config
if grep -q "sentinel.directory" "$HOOK"; then
    pass "Reads sentinel directory from config"
else
    fail "Should read sentinel directory from config"
fi

# Test 17: Has fallback sentinel directory
if grep -q ".state/sentinels/skill" "$HOOK"; then
    pass "Has fallback sentinel directory"
else
    fail "Should have fallback sentinel directory"
fi

echo ""
echo "--- State Cleanup ---"

# Test 18: Has state cleanup logic
if grep -q "STATE_DIR" "$HOOK"; then
    pass "Has state cleanup logic"
else
    fail "Should cleanup state files"
fi

# Test 19: Cleans session-specific state files
if grep -q 'SESSION_ID' "$HOOK" && grep -q "STATE_DIR" "$HOOK"; then
    pass "Cleans session-specific state files"
else
    fail "Should clean session-specific state files"
fi

# Test 20: Reads state directory from config
if grep -q "STATE_DIR\|SHARED_STATE_DIR\|SESSION_STATE_DIR" "$HOOK"; then
    pass "Reads state directory from config"
else
    fail "Should read state directory from config"
fi

# Test 21: Has fallback state directory
if grep -q '.state/session' "$HOOK"; then
    pass "Has fallback state directory"
else
    fail "Should have fallback state directory"
fi

# Test 22: Only cleans state for known session
if grep -q 'SESSION_ID.*!=.*unknown' "$HOOK"; then
    pass "Only cleans state for known session"
else
    fail "Should only clean state for known session"
fi

echo ""
echo "--- Temp File Cleanup ---"

# Test 23: Has temp cleanup logic
if grep -q "TEMP_DIR" "$HOOK"; then
    pass "Has temp cleanup logic"
else
    fail "Should cleanup temp files"
fi

# Test 24: References project temp directory
if grep -q "/tmp/claude/" "$HOOK"; then
    pass "References project temp directory"
else
    fail "Should reference project temp directory"
fi

# Test 25: Uses rm -rf for temp cleanup
if grep -q "rm -rf.*TEMP_DIR" "$HOOK"; then
    pass "Uses rm -rf for temp cleanup"
else
    fail "Should use rm -rf for temp cleanup"
fi

echo ""
echo "--- Config Integration ---"

# Test 26: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 27: Uses jq for JSON parsing
if grep -q "jq" "$HOOK"; then
    pass "Uses jq for JSON parsing"
else
    fail "Should use jq for JSON parsing"
fi

# Test 28: Has jq availability check
if grep -q "command -v jq" "$HOOK"; then
    pass "Has jq availability check"
else
    fail "Should check jq availability"
fi

# Test 29: Has jq error handling
if grep -q 'jq.*2>/dev/null' "$HOOK" || grep -q 'jq.*|| echo' "$HOOK"; then
    pass "Has jq error handling"
else
    fail "Should have jq error handling"
fi

echo ""
echo "--- Code Quality ---"

# Test 30: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 31: Has proper fallback grouping for REPO_ROOT
if grep -q '|| { cd' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have brace-grouped fallback for REPO_ROOT"
fi

# Test 32: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 33: Documents bash compatibility
if grep -qi "bash 3.2\|Compatibility:" "$HOOK"; then
    pass "Documents bash compatibility"
else
    fail "Should document bash 3.2+ compatibility"
fi

# Test 34: Has CONFIG variable
if grep -q "CONFIG=" "$HOOK"; then
    pass "Has CONFIG variable"
else
    fail "Should have CONFIG variable"
fi

# Test 35: All exits are 0
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then
    fail "SessionEnd should only have exit 0"
else
    pass "All exits are 0"
fi

echo ""
echo "--- Error Handling ---"

# Test 36: Has error suppression for rm commands
if grep -q 'rm.*|| true' "$HOOK"; then
    pass "Has error suppression for rm commands"
else
    fail "Should have error suppression for rm"
fi

# Test 37: Has 2>/dev/null for rm commands
if grep -q 'rm.*2>/dev/null' "$HOOK"; then
    pass "Has stderr suppression for rm commands"
else
    fail "Should suppress stderr for rm"
fi

# Test 38: Checks directory existence before cleanup
if grep -q '\[\[ -d.*SENTINEL_DIR' "$HOOK" && grep -q '\[\[ -d.*TEMP_DIR' "$HOOK"; then
    pass "Checks directory existence before cleanup"
else
    fail "Should check directory existence"
fi

echo ""
echo "--- Functional Tests ---"

# Test 39: Session sentinels are cleaned (session-end removes all for session)
setup_test_dirs
cleanup_test_artifacts
_now=$(date +%s)
_expired=$((_now - 100))
_valid=$((_now + 600))
mkdir -p "$REPO_ROOT/.state/sentinels/skill/test-session" 2>/dev/null || true
create_sentinel "test-expired.json" "$_expired" "$REPO_ROOT/.state/sentinels/skill/test-session"
create_sentinel "test-valid-other.json" "$_valid"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -f "$REPO_ROOT/.state/sentinels/skill/test-session/test-expired.json" ]]; then
    pass "Session sentinel is cleaned on session end"
else
    fail "Should clean expired sentinel"
    rm -f "$REPO_ROOT/.state/sentinels/skill/test-session/test-expired.json"
fi

# Test 40: Non-session sentinel is preserved
if [[ -f "$REPO_ROOT/.state/sentinels/skill/test-valid-other.json" ]]; then
    pass "Valid sentinel is preserved"
    rm -f "$REPO_ROOT/.state/sentinels/skill/test-valid-other.json"
else
    fail "Should preserve valid (non-expired) sentinel"
fi

# Test 41: Actually cleans project temp directory
setup_test_dirs
mkdir -p /tmp/claude/codeflow/test-artifact 2>/dev/null || true
touch /tmp/claude/codeflow/test-artifact/test-file.txt 2>/dev/null || true
CF_PROJECT_ROOT="codeflow" CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -d /tmp/claude/codeflow ]]; then
    pass "Actually cleans project temp directory"
else
    fail "Should actually clean project temp directory"
    rm -rf /tmp/claude/codeflow
fi

# Test 42: Actually cleans session state directory
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session" 2>/dev/null || true
echo '{"count": 1}' > "$REPO_ROOT/.state/session/test-session/claim-heartbeat"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -d "$REPO_ROOT/.state/session/test-session" ]]; then
    pass "Actually cleans session state files"
else
    fail "Should actually clean session state files"
    rm -rf "$REPO_ROOT/.state/session/test-session"
fi

# Test 43: Does not clean state for unknown session
setup_test_dirs
echo '{"count": 1}' > "$REPO_ROOT/.state/session/claim-heartbeat-other-session"
bash "$HOOK" </dev/null 2>/dev/null || true  # No CODEFLOW_SESSION_ID set
if [[ -f "$REPO_ROOT/.state/session/claim-heartbeat-other-session" ]]; then
    pass "Does not clean state for unknown session"
    rm -f "$REPO_ROOT/.state/session/claim-heartbeat-other-session"
else
    fail "Should not clean state for unknown session"
fi

# Test 44: Handles missing directories gracefully
result=$(CODEFLOW_SESSION_ID="nonexistent-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles missing directories gracefully"
else
    fail "Should handle missing directories"
fi

# Cleanup
cleanup_test_artifacts

echo ""
echo "--- V4: PathFlow Cleanup ---"

# Test 45: Hook has pathflow-active guard (skip cleanup when PathFlow active)
if grep -q "_PATHFLOW_ACTIVE" "$HOOK"; then
    pass "Has pathflow-active guard (_PATHFLOW_ACTIVE check)"
else
    fail "Should have pathflow-active guard (_PATHFLOW_ACTIVE check)"
fi

# Test 46: Hook contains PathFlow sentinel cleanup (pathflow-*)
if grep -q 'pathflow/' "$HOOK" || grep -q 'PATHFLOW_SENTINEL_DIR' "$HOOK"; then
    pass "Has PathFlow sentinel cleanup"
else
    fail "Should have PathFlow sentinel cleanup (pathflow-*)"
fi

# Test 47: Hook has PATHFLOW section
if grep -qi "PATHFLOW" "$HOOK"; then
    pass "Has PATHFLOW section"
else
    fail "Should have PATHFLOW section"
fi

# Test 48: Skips cleanup when pathflow-active flag exists AND lead PID alive (guard behavior)
# Fix 4 behavior: hook checks pathflow-team.json lead_pid liveness before skipping.
# With a live lead PID, cleanup is skipped (teammate shutdown path).
# The flag is NOT removed here — team-guard removes it during PF7-END.
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "active" > "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active"
# Create pathflow-team.json with live lead PID (current shell's PID)
echo "{\"lead_pid\": $$, \"team_name\": \"test-team-48\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ -f "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active" ]]; then
    pass "Skips cleanup when pathflow-active + live lead PID (flag preserved for team-guard)"
else
    fail "Should skip cleanup when pathflow-active + live lead PID (flag should be preserved)"
fi
rm -rf "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true

# Test 49: Actually removes PathFlow sentinels (session-scoped)
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
echo '{"type":"pathflow"}' > "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-gate-test.json"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-gate-test.json" ]]; then
    pass "Actually removes PathFlow sentinels"
else
    fail "Should remove PathFlow sentinels"
    rm -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-gate-test.json"
fi

cleanup_test_artifacts

echo ""
echo "--- V4: Memory Progress Cleanup ---"

# Test 50: Has memory-progress cleanup logic
if grep -q "memory-progress" "$HOOK"; then
    pass "Has memory-progress cleanup logic"
else
    fail "Should have memory-progress cleanup"
fi

# Test 51: Actually cleans memory-progress files (via session dir cleanup)
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session" 2>/dev/null || true
echo '{"count":3}' > "$REPO_ROOT/.state/session/test-session/memory-progress"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -f "$REPO_ROOT/.state/session/test-session/memory-progress" ]]; then
    pass "Actually cleans all memory-progress files"
else
    fail "Should clean all memory-progress files"
    rm -rf "$REPO_ROOT/.state/session/test-session" 2>/dev/null || true
fi

cleanup_test_artifacts

echo ""
echo "--- V4: Task Preservation ---"

# Test 52: Has task preservation logic
if grep -q "active-task\|ACTIVE_TASK\|task_preserved\|_task_preserved" "$HOOK"; then
    pass "Has task preservation logic"
else
    fail "Should have task preservation logic"
fi

# Test 53: Preserves in_progress task
setup_test_dirs
cat > "$REPO_ROOT/.state/runtime/active-task.json" <<'EOF'
{"task_id":"FRT-TSK-001","status":"in_progress","description":"Fix auth bug"}
EOF
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ -f "$REPO_ROOT/.state/runtime/active-task.json" ]]; then
    pass "Preserves in_progress task"
else
    fail "Should preserve in_progress task"
fi

# Test 54: Removes completed task
setup_test_dirs
cat > "$REPO_ROOT/.state/runtime/active-task.json" <<'EOF'
{"task_id":"FRT-TSK-002","status":"completed","description":"Done"}
EOF
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -f "$REPO_ROOT/.state/runtime/active-task.json" ]]; then
    pass "Removes completed task"
else
    fail "Should remove completed task"
    rm -f "$REPO_ROOT/.state/runtime/active-task.json"
fi

cleanup_test_artifacts

echo ""
echo "--- V4: Output Messages ---"

# Test 55: Produces output messages
setup_test_dirs
_now=$(date +%s)
_expired=$((_now - 100))
create_sentinel "test-output-expired.json" "$_expired"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"SessionEnd:"* ]]; then
    pass "Produces SessionEnd output messages"
else
    fail "Should produce SessionEnd: output messages"
fi
rm -f "$REPO_ROOT/.state/sentinels/skill/test-output-expired.json" 2>/dev/null || true

# Test 56: Reports sentinel count in output
setup_test_dirs
_now=$(date +%s)
_expired=$((_now - 100))
create_sentinel "test-count-1.json" "$_expired"
create_sentinel "test-count-2.json" "$_expired"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"sentinel(s)"* ]] || [[ "$output" == *"sentinels"* ]]; then
    pass "Reports sentinel count in output"
else
    fail "Should report sentinel count" "$output"
fi
rm -f "$REPO_ROOT/.state/sentinels/skill"/test-count-*.json 2>/dev/null || true

# Test 57: Reports task preservation in output
setup_test_dirs
cat > "$REPO_ROOT/.state/runtime/active-task.json" <<'EOF'
{"task_id":"FRT-TSK-003","status":"in_progress","description":"Test"}
EOF
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"preserved"* ]]; then
    pass "Reports task preservation in output"
else
    fail "Should report task preservation in output" "$output"
fi

cleanup_test_artifacts

echo ""
echo "--- V4: Stdin Session ID ---"

# Test 58: Has stdin reading block
if grep -q '_HOOK_STDIN\|HOOK_STDIN' "$HOOK"; then
    pass "Has stdin reading block"
else
    fail "Should read session_id from stdin"
fi

# Test 59: Reads session_id from stdin JSON as fallback
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/stdin-test-id" 2>/dev/null || true
echo '{"count":1}' > "$REPO_ROOT/.state/session/stdin-test-id/claim-heartbeat"
stdin_json='{"session_id":"stdin-test-id","transcript_path":"/tmp/test.jsonl"}'
# No CODEFLOW_SESSION_ID set, no env file — stdin should be used as fallback
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
echo "$stdin_json" | bash "$HOOK" 2>/dev/null || true
# If stdin session_id was used, it should have cleaned the stdin-test-id session dir
if [[ ! -d "$REPO_ROOT/.state/session/stdin-test-id" ]]; then
    pass "Uses session_id from stdin JSON as fallback"
else
    fail "Should use session_id from stdin JSON as fallback"
    rm -rf "$REPO_ROOT/.state/session/stdin-test-id"
fi

cleanup_test_artifacts

echo ""
echo "--- V4: Config Enabled Check ---"

# Test 60: Has cleanup enabled check
if grep -q "session_end\|CLEANUP_ENABLED\|cleanup.enabled" "$HOOK"; then
    pass "Has cleanup enabled check"
else
    fail "Should check session_end.cleanup.enabled config"
fi

echo ""
echo "--- V4: Security Lib Integration ---"

# Test 61: Sources or references security-lib.sh
if grep -q "security-lib.sh\|SECURITY_LIB\|is_pathflow_active" "$HOOK"; then
    pass "References security-lib.sh for PathFlow detection"
else
    fail "Should use security-lib.sh for PathFlow mode detection"
fi

# Test 62: Uses is_pathflow_active or is_pathflow_active
if grep -q "is_pathflow_active\|is_pathflow_active\|_PATHFLOW_ACTIVE" "$HOOK"; then
    pass "Uses PathFlow mode detection function"
else
    fail "Should use is_pathflow_active() or is_pathflow_active()"
fi

echo ""
echo "--- V4: Cleanup Order ---"

# Test 63: PathFlow sentinels cleaned before skill sentinels
pf_line=$(grep -n "PATHFLOW.*SENTINEL\|pathflow/\|PATHFLOW_SENTINEL_DIR" "$HOOK" | head -1 | cut -d: -f1)
sk_line=$(grep -n "EXPIRED.*SENTINEL\|sentinel_cleanup_expired\|SKILL.*SENTINEL" "$HOOK" | head -1 | cut -d: -f1)
if [[ -n "$pf_line" ]] && [[ -n "$sk_line" ]] && [[ "$pf_line" -lt "$sk_line" ]]; then
    pass "PathFlow sentinels cleaned before skill sentinels"
else
    fail "V4 spec requires PathFlow sentinels cleaned first"
fi

# Test 64: Session state cleanup (which removes pathflow flag) after sentinel cleanup
state_line=$(grep -n "SESSION_STATE_DIR.*rm\|rm.*SESSION_STATE_DIR" "$HOOK" | head -1 | cut -d: -f1 || echo "")
if [[ -z "$state_line" ]]; then
    # Fallback: check for is-pathflow-active reference after sentinel cleanup
    state_line=$(grep -n "is-pathflow-active\|SESSION_STATE_DIR" "$HOOK" | tail -1 | cut -d: -f1 || echo "")
fi
if [[ -n "$state_line" ]] && [[ -n "$sk_line" ]] && [[ "$state_line" -gt "$sk_line" ]]; then
    pass "PathFlow flag removed after sentinel cleanup (via session dir)"
else
    fail "V4 spec requires flag removal after sentinel cleanup"
fi

echo ""
echo "--- V4: Env File Session ID ---"

# Test 65: Session-end sources env file for session ID
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/ses-envtest-end" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/runtime" 2>/dev/null || true
echo '{"count":1}' > "$REPO_ROOT/.state/session/ses-envtest-end/claim-heartbeat"
echo "export CODEFLOW_SESSION_ID='ses-envtest-end'" > "$REPO_ROOT/.state/runtime/codeflow-env.sh"
# Run hook without setting CODEFLOW_SESSION_ID in process env — env file should provide it
bash "$HOOK" </dev/null 2>/dev/null || true
# If env file session ID was used, it should have cleaned the ses-envtest-end session dir
if [[ ! -d "$REPO_ROOT/.state/session/ses-envtest-end" ]]; then
    pass "Sources env file for session ID"
else
    fail "Should source env file to get session ID"
    rm -rf "$REPO_ROOT/.state/session/ses-envtest-end"
fi
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true

# Test 66: Session-end removes env file during cleanup
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/runtime" 2>/dev/null || true
echo "export CODEFLOW_SESSION_ID='ses-cleanup-env'" > "$REPO_ROOT/.state/runtime/codeflow-env.sh"
CODEFLOW_SESSION_ID="ses-cleanup-env" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    pass "Removes env file during cleanup"
else
    fail "Should remove env file during cleanup"
    rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh"
fi

# Test 67: Session-end has TODO(go-cli) comment
if grep -q "TODO(go-cli)" "$HOOK"; then
    pass "Has TODO(go-cli) comment"
else
    fail "Should have TODO(go-cli) comment near session ID sourcing"
fi

# Test 68: Session-end falls back to stdin when env file missing
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/stdin-fallback-id" 2>/dev/null || true
echo '{"count":1}' > "$REPO_ROOT/.state/session/stdin-fallback-id/claim-heartbeat"
# Ensure no env file exists
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
stdin_json='{"session_id":"stdin-fallback-id"}'
echo "$stdin_json" | bash "$HOOK" 2>/dev/null || true
if [[ ! -d "$REPO_ROOT/.state/session/stdin-fallback-id" ]]; then
    pass "Falls back to stdin when env file missing"
else
    fail "Should fall back to stdin session_id when env file missing"
    rm -rf "$REPO_ROOT/.state/session/stdin-fallback-id"
fi

cleanup_test_artifacts

echo ""
echo "--- V4: PathFlow Team Cleanup (v2.2.0) ---"

# Test 69: Hook has pathflow-team.json reading logic
if grep -q "pathflow-team.json" "$HOOK"; then
    pass "Has pathflow-team.json reading logic"
else
    fail "Should read pathflow-team.json for team cleanup"
fi

# Test 70: Hook cleans team config directory
if grep -q '\.claude/teams' "$HOOK"; then
    pass "Has team config directory cleanup"
else
    fail "Should clean ~/.claude/teams/{team_name}"
fi

# Test 71: Hook cleans task list directory
if grep -q '\.claude/tasks' "$HOOK"; then
    pass "Has task list directory cleanup"
else
    fail "Should clean ~/.claude/tasks/{team_name}"
fi

# Test 72: Cleans up team config when pathflow-team.json has team_name
setup_test_dirs
_team_name="test-team-cleanup-72"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "{\"team_name\":\"$_team_name\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
mkdir -p "${HOME}/.claude/teams/${_team_name}" 2>/dev/null || true
echo '{"members":[]}' > "${HOME}/.claude/teams/${_team_name}/config.json"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -d "${HOME}/.claude/teams/${_team_name}" ]]; then
    pass "Cleans team config directory from pathflow-team.json"
else
    fail "Should clean team config directory"
    rm -rf "${HOME}/.claude/teams/${_team_name}" 2>/dev/null || true
fi

# Test 73: Cleans up task list when pathflow-team.json has team_name
setup_test_dirs
_team_name="test-team-cleanup-73"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "{\"team_name\":\"$_team_name\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
mkdir -p "${HOME}/.claude/tasks/${_team_name}" 2>/dev/null || true
echo '{"tasks":[]}' > "${HOME}/.claude/tasks/${_team_name}/tasks.json"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -d "${HOME}/.claude/tasks/${_team_name}" ]]; then
    pass "Cleans task list directory from pathflow-team.json"
else
    fail "Should clean task list directory"
    rm -rf "${HOME}/.claude/tasks/${_team_name}" 2>/dev/null || true
fi

# Test 74: Cleans both team config and task list together
setup_test_dirs
_team_name="test-team-cleanup-74"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "{\"team_name\":\"$_team_name\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
mkdir -p "${HOME}/.claude/teams/${_team_name}" 2>/dev/null || true
echo '{"members":[]}' > "${HOME}/.claude/teams/${_team_name}/config.json"
mkdir -p "${HOME}/.claude/tasks/${_team_name}" 2>/dev/null || true
echo '{"tasks":[]}' > "${HOME}/.claude/tasks/${_team_name}/tasks.json"
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
_both_cleaned="true"
[[ -d "${HOME}/.claude/teams/${_team_name}" ]] && _both_cleaned="false"
[[ -d "${HOME}/.claude/tasks/${_team_name}" ]] && _both_cleaned="false"
if [[ "$_both_cleaned" == "true" ]]; then
    pass "Cleans both team config and task list together"
else
    fail "Should clean both team config and task list"
    rm -rf "${HOME}/.claude/teams/${_team_name}" 2>/dev/null || true
    rm -rf "${HOME}/.claude/tasks/${_team_name}" 2>/dev/null || true
fi

# Test 75: Graceful when pathflow-team.json doesn't exist
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session" 2>/dev/null || true
# No pathflow directory or pathflow-team.json
result=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful when pathflow-team.json missing (exits 0)"
else
    fail "Should exit 0 when pathflow-team.json missing"
fi

# Test 76: Graceful when pathflow-team.json has no team_name field
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo '{"session_id":"test-session","lead_pid":12345}' > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
result=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Graceful when pathflow-team.json has no team_name (exits 0)"
else
    fail "Should exit 0 when team_name missing from JSON"
fi

# Test 77: Reports team config removal in output
setup_test_dirs
_team_name="test-team-cleanup-77"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "{\"team_name\":\"$_team_name\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
mkdir -p "${HOME}/.claude/teams/${_team_name}" 2>/dev/null || true
echo '{"members":[]}' > "${HOME}/.claude/teams/${_team_name}/config.json"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"Removed team config"* ]]; then
    pass "Reports team config removal in output"
else
    fail "Should report team config removal" "$output"
fi
rm -rf "${HOME}/.claude/teams/${_team_name}" 2>/dev/null || true

# Test 78: No team cleanup messages when pathflow-team.json missing
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session" 2>/dev/null || true
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" != *"Removed team config"* ]] && [[ "$output" != *"Removed task list"* ]]; then
    pass "No team cleanup messages when pathflow-team.json missing"
else
    fail "Should not report team cleanup when no pathflow-team.json"
fi

# Test 79: Handles empty team_name in pathflow-team.json
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo '{"team_name":"","lead_pid":12345}' > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
result=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Handles empty team_name gracefully (exits 0)"
else
    fail "Should exit 0 with empty team_name"
fi

# Test 80: Reports task list removal in output
setup_test_dirs
_team_name="test-team-cleanup-80"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "{\"team_name\":\"$_team_name\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
mkdir -p "${HOME}/.claude/tasks/${_team_name}" 2>/dev/null || true
echo '{"tasks":[]}' > "${HOME}/.claude/tasks/${_team_name}/tasks.json"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"Removed task list"* ]]; then
    pass "Reports task list removal in output"
else
    fail "Should report task list removal" "$output"
fi
rm -rf "${HOME}/.claude/tasks/${_team_name}" 2>/dev/null || true

cleanup_test_artifacts

echo ""
echo "--- V4: PF7 Diagnostic (Three-Layer Cleanup) ---"

# Test 81: Logs 'Clean PF7 shutdown' when pf-7 sentinel exists
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
touch "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-7"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"Clean PF7 shutdown"* ]]; then
    pass "Logs 'Clean PF7 shutdown' when pf-7 exists"
else
    fail "Should log 'Clean PF7 shutdown' when pf-7 sentinel exists"
fi

# Test 82: Logs 'Incomplete PF7 shutdown' when pf-7 sentinel absent
setup_test_dirs
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"Incomplete PF7 shutdown"* ]]; then
    pass "Logs 'Incomplete PF7 shutdown' when pf-7 absent"
else
    fail "Should log 'Incomplete PF7 shutdown' when pf-7 sentinel absent"
fi

# Test 83: Cleanup proceeds when pathflow-active flag is absent
# (simulates PostToolUse already removed it before SessionEnd)
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
touch "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3"
# No pathflow-active flag
CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>/dev/null || true
if [[ ! -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3" ]]; then
    pass "Cleanup proceeds when flag absent (sentinels cleaned)"
else
    fail "Cleanup should proceed when flag absent"
    rm -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3" 2>/dev/null || true
fi

# Test 84: Cleanup skipped when pathflow-active flag + live lead PID (teammate guard)
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "active" > "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active"
# Fix 4: Create pathflow-team.json with live lead PID for skip behavior
echo "{\"lead_pid\": $$, \"team_name\": \"test-team-84\"}" > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json"
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
touch "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3" ]]; then
    pass "Cleanup skipped when flag present + live lead PID (sentinels preserved)"
else
    fail "Cleanup should be skipped when flag present + live lead PID"
fi
if [[ "$output" == *"PathFlow active"* ]]; then
    pass "Logs 'PathFlow active' skip message"
else
    fail "Should log PathFlow active skip message"
fi
rm -rf "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
rm -rf "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true

# Test 85: Hook references PostToolUse for flag removal (not team-guard)
if grep -q "PostToolUse on TeamDelete\|PostToolUse.*TeamDelete" "$HOOK"; then
    pass "Hook comments reference PostToolUse for flag removal"
else
    fail "Hook should reference PostToolUse on TeamDelete for flag removal"
fi

cleanup_test_artifacts

echo ""
echo "--- V4: SessionEnd Teammate Guard Enhancement (Fix 4) ---"

# Fix 4 Test 1: PathFlow active + dead lead PID → proceed with cleanup
if [[ "${CI:-}" != "true" && "${GITHUB_ACTIONS:-}" != "true" ]]; then

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
_team_name="test-team-fix4-dead"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "active" > "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active"
# Create pathflow-team.json with a dead PID (99999)
cat > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json" <<FIX4EOF
{"team_name":"$_team_name","lead_pid":99999,"codeflow_session_id":"test-session"}
FIX4EOF
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
touch "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
# With dead lead PID, cleanup should PROCEED (not skip)
if [[ ! -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3" ]]; then
    pass "Fix 4: Dead lead PID — cleanup proceeds (sentinels cleaned)"
else
    fail "Fix 4: Dead lead PID — should proceed with cleanup (not skip)"
    rm -rf "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
fi
rm -rf "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true

# Fix 4 Test 2: PathFlow active + alive lead PID → skip cleanup (existing behavior)
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
_team_name_alive="test-team-fix4-alive"
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "active" > "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active"
# Create pathflow-team.json with ALIVE PID (current shell)
cat > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json" <<FIX4EOF2
{"team_name":"$_team_name_alive","lead_pid":$$,"codeflow_session_id":"test-session"}
FIX4EOF2
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
touch "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
# With alive lead PID, cleanup should be SKIPPED
if [[ -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-3" ]]; then
    pass "Fix 4: Alive lead PID — cleanup skipped (sentinels preserved)"
else
    fail "Fix 4: Alive lead PID — should skip cleanup (sentinels should be preserved)"
fi
if [[ "$output" == *"skipping cleanup"* ]] || [[ "$output" == *"PathFlow active"* ]]; then
    pass "Fix 4: Alive lead PID — logs skip message"
else
    fail "Fix 4: Alive lead PID — should log skip message"
fi
TESTS_RUN=$((TESTS_RUN + 1))
rm -rf "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
rm -rf "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true

# Fix 4 Test 3: PathFlow active + no team file → proceed with cleanup
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "active" > "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active"
# NO pathflow-team.json — simulates pre-TeamCreate crash
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
touch "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-1"
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ ! -f "$REPO_ROOT/.state/sentinels/pathflow/test-session/pathflow-pf-1" ]]; then
    pass "Fix 4: No team file — cleanup proceeds (sentinels cleaned)"
else
    fail "Fix 4: No team file — should proceed with cleanup"
    rm -rf "$REPO_ROOT/.state/sentinels/pathflow/test-session" 2>/dev/null || true
fi
rm -rf "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true

# Fix 4 Test 4: Dead lead PID logs appropriate message
TESTS_RUN=$((TESTS_RUN + 1))
setup_test_dirs
mkdir -p "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true
echo "active" > "$REPO_ROOT/.state/session/test-session/pathflow/is-pathflow-active"
cat > "$REPO_ROOT/.state/session/test-session/pathflow/pathflow-team.json" <<FIX4EOF3
{"team_name":"test-team-fix4-log","lead_pid":99999,"codeflow_session_id":"test-session"}
FIX4EOF3
output=$(CODEFLOW_SESSION_ID="test-session" bash "$HOOK" </dev/null 2>&1) || true
if [[ "$output" == *"dead"* ]] || [[ "$output" == *"proceeding with cleanup"* ]]; then
    pass "Fix 4: Dead lead PID logs diagnostic message"
else
    fail "Fix 4: Dead lead PID should log 'dead' or 'proceeding' message"
fi
rm -rf "$REPO_ROOT/.state/session/test-session/pathflow" 2>/dev/null || true

fi  # End CI guard

# =============================================================================
# PROJECT TEMP DIRECTORY CLEANUP
# =============================================================================

echo ""
echo "--- Project Temp Directory Cleanup ---"

# Test: Hook defines PROJECT_TEMP_DIR variable
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'PROJECT_TEMP_DIR=' "$HOOK"; then
    pass "Hook defines PROJECT_TEMP_DIR variable"
else
    fail "Missing PROJECT_TEMP_DIR variable definition"
fi

# Test: Project temp dir uses CF_PROJECT_ROOT with fallback
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CF_PROJECT_ROOT:-codeflow' "$HOOK"; then
    pass "Project temp dir uses CF_PROJECT_ROOT with codeflow fallback"
else
    fail "Missing CF_PROJECT_ROOT fallback in project temp dir path"
fi

# Test: Project temp cleanup uses /tmp/claude/ base path
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '/tmp/claude/' "$HOOK" | head -1 && grep 'PROJECT_TEMP_DIR=' "$HOOK" | grep -q '/tmp/claude/'; then
    pass "Project temp cleanup uses /tmp/claude/ base path"
else
    # Alternative check
    if grep -q 'PROJECT_TEMP_DIR="/tmp/claude/' "$HOOK"; then
        pass "Project temp cleanup uses /tmp/claude/ base path"
    else
        fail "Wrong or missing /tmp/claude/ base path for project temp"
    fi
fi

# Test: Project temp cleanup uses rm -rf with error suppression
TESTS_RUN=$((TESTS_RUN + 1))
if grep -A2 'PROJECT_TEMP_DIR' "$HOOK" | grep -q 'rm -rf.*2>/dev/null'; then
    pass "Project temp cleanup uses rm -rf with error suppression"
else
    fail "Missing rm -rf or error suppression for project temp"
fi

# Test: Project temp cleanup checks directory existence before removal
TESTS_RUN=$((TESTS_RUN + 1))
if grep -B1 'rm -rf.*PROJECT_TEMP_DIR' "$HOOK" | grep -q '\-d.*PROJECT_TEMP_DIR'; then
    pass "Project temp cleanup checks directory existence"
else
    fail "Missing directory existence check for project temp"
fi

# Test: Dead /tmp/claude/sessions/ code was removed
TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '/tmp/claude/sessions/' "$HOOK"; then
    fail "Dead /tmp/claude/sessions/ code should be removed"
else
    pass "Dead /tmp/claude/sessions/ code removed"
fi

cleanup_test_artifacts

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
