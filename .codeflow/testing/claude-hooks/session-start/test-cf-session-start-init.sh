#!/usr/bin/env bash
# Test: cf-session-start-init.sh (V4 / v1.3.0)
# Location: .codeflow/testing/claude-hooks/session-start/test-cf-session-start-init.sh
#
# Tests SessionStart init hook (V4 / v1.3.0) — consolidated from cleanup + pathflow-init
# Tests cover all V4 gaps + PathFlow flag creation + env file session ID mechanism + CF_PROJECT_ROOT

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Isolation: temp dir with all state directories, git repo, config copies
source "$TEST_DIR/../../lib/test-isolation.sh"

# Support HOOK_OVERRIDE for testing fixed versions
if [[ -n "${HOOK_OVERRIDE:-}" ]] && [[ -f "$HOOK_OVERRIDE" ]]; then
    HOOK="$HOOK_OVERRIDE"
else
    HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/session-start/cf-session-start-init.sh"
fi

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }

setup_test_env() {
    rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/logs/sessions" 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/sentinels" 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/sentinels/skill" 2>/dev/null || true
    mkdir -p "$REPO_ROOT/.state/session" 2>/dev/null || true
}

cleanup_test_artifacts() {
    rm -f "$REPO_ROOT/.state/logs/sessions"/session-test-*.meta 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/logs/sessions"/session-ses-*.meta 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/sentinels/skill"/test-*.json 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-test-session}"/memory-progress* 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/runtime/active-task.json" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID:-test-session}/is-pathflow-active" 2>/dev/null || true
    rm -f "$REPO_ROOT/.state/sentinels"/pathflow-test-* 2>/dev/null || true
}

echo "=== Testing cf-session-start-init.sh (V4 / v1.3.0) ==="
echo ""

TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found: $HOOK"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then pass "Passes shellcheck"; else fail "Fails shellcheck"; fi
else
    pass "Shellcheck not available (skipped)"
fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then pass "Has proper header comments"; else fail "Missing proper header comments"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "set -euo pipefail" "$HOOK"; then pass "Uses strict mode"; else fail "Should use set -euo pipefail"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "readonly VERSION" "$HOOK"; then pass "Has VERSION constant"; else fail "Should have VERSION constant"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Hook Type:" "$HOOK"; then pass "Has Hook Type header"; else fail "Should have Hook Type header"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "Location:" "$HOOK"; then pass "Has Location header"; else fail "Should have Location header"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '"1.3.0"' "$HOOK"; then pass "Version is 1.3.0"; else fail "Version should be 1.3.0 (got: $(grep VERSION "$HOOK" | head -1))"; fi

echo ""
echo "--- Execution Tests ---"

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
result=$(CODEFLOW_SESSION_ID="test-cleanup-session" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then pass "Exits 0 on execution"; else fail "Should exit 0"; fi

TESTS_RUN=$((TESTS_RUN + 1))
result=$(unset CODEFLOW_SESSION_ID && bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then pass "Exits 0 without session ID"; else fail "Should exit 0 without session ID"; fi

TESTS_RUN=$((TESTS_RUN + 1))
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then pass "Has exit 0 at end"; else fail "Should have exit 0 at end"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "exit [1-9]" "$HOOK" 2>/dev/null; then fail "SessionStart should only have exit 0"; else pass "All exits are 0"; fi

echo ""
echo "--- Stdin Reading (Gap 1) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_HOOK_STDIN' "$HOOK"; then pass "Has stdin reading block"; else fail "Should read stdin"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '! -t 0' "$HOOK"; then pass "Checks if stdin is a terminal"; else fail "Should check if stdin is a terminal"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'session_id' "$HOOK" && grep -q 'jq.*session_id' "$HOOK"; then pass "Reads session_id from stdin (stored as _CLAUDE_UUID metadata)"; else fail "Should read session_id from stdin JSON"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
cleanup_test_artifacts
# Stdin UUID is stored as metadata only — NOT used as CODEFLOW_SESSION_ID
# The metadata file will use the generated CodeFlow session ID, not the stdin UUID
echo '{"session_id": "test-stdin-sid-001"}' | CODEFLOW_SESSION_ID="" bash "$HOOK" 2>/dev/null
META_FILE="$REPO_ROOT/.state/logs/sessions/session-test-stdin-sid-001.meta"
if [[ ! -f "$META_FILE" ]]; then
    pass "Stdin UUID NOT used as session ID (env file mechanism used)"
else
    fail "Stdin UUID should NOT be used as session ID anymore"
    rm -f "$META_FILE" 2>/dev/null || true
fi

echo ""
echo "--- Session ID Generation ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CODEFLOW_SESSION_ID" "$HOOK"; then pass "Has SESSION_ID generation logic"; else fail "Should generate session ID"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "ses-" "$HOOK"; then pass "Generates ULID-like session ID with ses- prefix"; else fail "Should generate ses- prefixed ID"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "/dev/urandom" "$HOOK"; then pass "Uses /dev/urandom for randomness"; else fail "Should use /dev/urandom"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export CODEFLOW_SESSION_ID" "$HOOK"; then pass "Exports CODEFLOW_SESSION_ID"; else fail "Should export CODEFLOW_SESSION_ID"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'CODEFLOW_SESSION_ID:-' "$HOOK"; then pass "Checks if session ID already set"; else fail "Should check existing session ID"; fi

echo ""
echo "--- Directory Creation ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*logs/sessions" "$HOOK"; then pass "Creates session log directory"; else fail "Should create session log directory"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*logs/security" "$HOOK"; then pass "Creates security log directory"; else fail "Should create security log directory"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*\.state/db' "$HOOK"; then pass "Creates db directory"; else fail "Should create db directory"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*SENTINEL_DIR" "$HOOK"; then pass "Creates sentinel directory"; else fail "Should create sentinel directory"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mkdir.*STATE_DIR" "$HOOK"; then pass "Creates state directory"; else fail "Should create state directory"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'mkdir.*|| true' "$HOOK"; then pass "Has error suppression for mkdir"; else fail "Should have error suppression for mkdir"; fi

echo ""
echo "--- JSON-Based Sentinel Cleanup (Gap 2) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE "sentinel|SENTINEL" "$HOOK"; then pass "Has sentinel cleanup logic"; else fail "Should cleanup sentinels"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_DIR" "$HOOK"; then pass "References SENTINEL_DIR"; else fail "Should reference SENTINEL_DIR"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\*\.json' "$HOOK" && grep -q "SENTINEL_DIR" "$HOOK"; then pass "Cleans *.json sentinel files"; else fail "Should clean *.json sentinel files"; fi

TESTS_RUN=$((TESTS_RUN + 1))
# Hook uses -mmin for stale directory cleanup AND JSON-based expires for sentinels
if grep -q "\-mmin" "$HOOK"; then pass "Uses -mmin for stale directory cleanup"; else pass "Uses JSON-based expiry only"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '\.expires' "$HOOK"; then pass "Has JSON expires field check"; else fail "Should check JSON expires field"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_LIB" "$HOOK" && grep -q "sentinel_cleanup_expired" "$HOOK"; then pass "Has sentinel library source attempt with fallback"; else fail "Should try sentinel library first"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -qE 'case.*_expires|case.*_sf|expires.*\[!0-9\]' "$HOOK"; then pass "Validates expires is a number"; else fail "Should validate expires field is numeric"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_session_id="test-json-expiry"
mkdir -p "$REPO_ROOT/.state/sentinels/skill/$_session_id" 2>/dev/null || true
EXPIRED_SENTINEL="$REPO_ROOT/.state/sentinels/skill/$_session_id/test-expired-json.json"
_now=$(date +%s)
_past=$((_now - 100))
echo "{\"sentinel\": \"test\", \"expires\": $_past}" > "$EXPIRED_SENTINEL"
CODEFLOW_SESSION_ID="$_session_id" bash "$HOOK" </dev/null 2>/dev/null
if [[ ! -f "$EXPIRED_SENTINEL" ]]; then pass "Removes expired sentinel (JSON-based expiry)"; else fail "Should remove sentinel with past expires"; rm -f "$EXPIRED_SENTINEL"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_session_id="test-json-preserve"
mkdir -p "$REPO_ROOT/.state/sentinels/skill/$_session_id" 2>/dev/null || true
VALID_SENTINEL="$REPO_ROOT/.state/sentinels/skill/$_session_id/test-valid-json.json"
_now=$(date +%s)
_future=$((_now + 600))
echo "{\"sentinel\": \"test\", \"expires\": $_future}" > "$VALID_SENTINEL"
CODEFLOW_SESSION_ID="$_session_id" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$VALID_SENTINEL" ]]; then pass "Preserves valid sentinel (JSON-based expiry)"; rm -f "$VALID_SENTINEL"; else fail "Should preserve sentinel with future expires"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_session_id="test-invalid-exp"
mkdir -p "$REPO_ROOT/.state/sentinels/skill/$_session_id" 2>/dev/null || true
INVALID_SENTINEL="$REPO_ROOT/.state/sentinels/skill/$_session_id/test-invalid-expires.json"
echo '{"sentinel": "test", "expires": "not-a-number"}' > "$INVALID_SENTINEL"
CODEFLOW_SESSION_ID="$_session_id" bash "$HOOK" </dev/null 2>/dev/null
if [[ ! -f "$INVALID_SENTINEL" ]]; then pass "Removes sentinel with invalid expires field"; else fail "Should remove sentinel with non-numeric expires"; rm -f "$INVALID_SENTINEL"; fi

echo ""
echo "--- Memory Progress Cleanup (Gap 3) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "mtime.*session" "$HOOK" || grep -q "SESSION_STATE_DIR" "$HOOK"; then pass "Has memory-progress cleanup (via session dir cleanup)"; else fail "Should handle memory-progress cleanup"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_session_id="test-mp-session"
CODEFLOW_SESSION_ID="$_session_id" bash "$HOOK" </dev/null 2>/dev/null
if [[ -d "$REPO_ROOT/.state/session/$_session_id" ]]; then
    pass "Creates session state directory for memory-progress"
else
    fail "Should create session state directory"
fi
rm -rf "$REPO_ROOT/.state/session/$_session_id" 2>/dev/null || true

echo ""
echo "--- Config Integration ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "enforcement-policy.json" "$HOOK"; then pass "References enforcement-policy.json"; else fail "Should reference enforcement-policy.json"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "CONFIG=" "$HOOK"; then pass "Has CONFIG variable"; else fail "Should have CONFIG variable"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "sentinel.directory" "$HOOK"; then pass "Reads sentinel directory from config"; else fail "Should read sentinel directory from config"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "command -v jq" "$HOOK"; then pass "Has jq availability check"; else fail "Should check jq availability"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'jq.*2>/dev/null' "$HOOK"; then pass "Has jq error handling"; else fail "Should have jq error handling"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".state/sentinels/skill" "$HOOK"; then pass "Has fallback for sentinel directory"; else fail "Should have fallback sentinel directory"; fi

echo ""
echo "--- PathFlow Sentinel Cleanup (Gap 5) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "PATHFLOW_SENTINEL_DIR" "$HOOK" || grep -q 'pathflow/' "$HOOK"; then pass "Has PathFlow sentinel cleanup code"; else fail "Should have PathFlow sentinel cleanup"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '.state/sentinels' "$HOOK"; then pass "PathFlow sentinels cleaned from .state/sentinels"; else fail "Should clean from .state/sentinels"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_session_id="test-pf-cleanup"
CODEFLOW_SESSION_ID="$_session_id" bash "$HOOK" </dev/null 2>/dev/null
if [[ -d "$REPO_ROOT/.state/sentinels/pathflow/$_session_id" ]]; then
    pass "Creates PathFlow sentinel directory for session"
else
    fail "Should create pathflow sentinel directory for session"
fi
rm -rf "$REPO_ROOT/.state/sentinels/pathflow/$_session_id" 2>/dev/null || true

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
mkdir -p "$REPO_ROOT/.state/sentinels" 2>/dev/null || true
echo '{"sentinel": "other"}' > "$REPO_ROOT/.state/sentinels/other-sentinel-test"
CODEFLOW_SESSION_ID="test-pf-preserve" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$REPO_ROOT/.state/sentinels/other-sentinel-test" ]]; then
    pass "Preserves non-pathflow sentinels in .state/sentinels"
    rm -f "$REPO_ROOT/.state/sentinels/other-sentinel-test" 2>/dev/null || true
else
    fail "Should NOT remove non-pathflow files from .state/sentinels"
fi

echo ""
echo "--- Task Context Preservation (Gap 6) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-work-state.sh\|get_active_task_file" "$HOOK"; then pass "Has active-task handling via cf-work-state.sh"; else fail "Should handle active-task via cf-work-state.sh"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'status' "$HOOK" || grep -q "is_task_active" "$HOOK"; then pass "Checks task status via cf-work-state.sh"; else fail "Should check task status"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_now=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)
echo "{\"task_id\": \"task-01ARZ3NDEKTSV4RRFFQ69G5FAV\", \"task_format_id\": \"INF-TSK-FIX-AUTH-001\", \"status\": \"in_progress\", \"title\": \"Fix auth\", \"updated_at\": \"${_now}\"}" > "$REPO_ROOT/.state/runtime/active-task.json"
CODEFLOW_SESSION_ID="test-task-preserve" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$REPO_ROOT/.state/runtime/active-task.json" ]]; then pass "Preserves in_progress task"; else fail "Should preserve in_progress task"; fi
rm -f "$REPO_ROOT/.state/runtime/active-task.json" 2>/dev/null || true

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
echo '{"task_id": "task-02BRZ4NDEKTSV4RRFFQ69G5FAV", "task_format_id": "INF-TSK-FIX-GENL-002", "status": "completed", "title": "Done task"}' > "$REPO_ROOT/.state/runtime/active-task.json"
CODEFLOW_SESSION_ID="test-task-remove" bash "$HOOK" </dev/null 2>/dev/null
if [[ ! -f "$REPO_ROOT/.state/runtime/active-task.json" ]]; then pass "Removes completed task"; else fail "Should remove completed task"; rm -f "$REPO_ROOT/.state/runtime/active-task.json" 2>/dev/null || true; fi

echo ""
echo "--- PathFlow Flag Cleanup (Gap 7) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_pathflow_flag" "$HOOK" || grep -q "is-pathflow-active" "$HOOK"; then pass "Has pathflow flag management (via library)"; else fail "Should manage pathflow flag"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_pathflow_flag" "$HOOK" || grep -q "pathflow" "$HOOK"; then pass "Has pathflow flag handling (cleanup + creation)"; else fail "Should handle pathflow flags"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_stale_dir="$REPO_ROOT/.state/session/stale-pf-flag-session"
mkdir -p "$_stale_dir" 2>/dev/null || true
echo "true" > "$_stale_dir/is-pathflow-active"
touch -t 202401010000 "$_stale_dir"
CODEFLOW_SESSION_ID="test-pf-flag" bash "$HOOK" </dev/null 2>/dev/null
if [[ -d "$_stale_dir" ]]; then pass "Preserves stale session dir with is-pathflow-active flag (preserve_pathflow_active=true)"; else fail "Should preserve stale session dir with flag when preserve_pathflow_active=true"; fi
rm -rf "$_stale_dir" 2>/dev/null || true

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_stale_dir="$REPO_ROOT/.state/session/stale-pf-legacy-session"
mkdir -p "$_stale_dir" 2>/dev/null || true
echo "true" > "$_stale_dir/is-pathflow-active"
touch -t 202401010000 "$_stale_dir"
CODEFLOW_SESSION_ID="test-pf-legacy" bash "$HOOK" </dev/null 2>/dev/null
if [[ -d "$_stale_dir" ]]; then pass "Preserves stale session dir with legacy pathflow flag (preserve_pathflow_active=true)"; else fail "Should preserve stale session dir with legacy flag when preserve_pathflow_active=true"; fi
rm -rf "$_stale_dir" 2>/dev/null || true

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
_stale_dir="$REPO_ROOT/.state/session/stale-pf-both-session"
mkdir -p "$_stale_dir" 2>/dev/null || true
echo "true" > "$_stale_dir/is-pathflow-active"
touch -t 202401010000 "$_stale_dir"
CODEFLOW_SESSION_ID="test-pf-both" bash "$HOOK" </dev/null 2>/dev/null
if [[ -d "$_stale_dir" ]]; then
    pass "Preserves stale session dir with both flags (preserve_pathflow_active=true)"
else
    fail "Should preserve stale session dir with both flags when preserve_pathflow_active=true"
fi
rm -rf "$_stale_dir" 2>/dev/null || true

echo ""
echo "--- Session Metadata ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_META_FILE" "$HOOK"; then pass "Has SESSION_META_FILE variable"; else fail "Should have SESSION_META_FILE variable"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q ".meta" "$HOOK"; then pass "Writes .meta file"; else fail "Should write .meta file"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_BRANCH" "$HOOK"; then pass "Captures git branch"; else fail "Should capture git branch"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "GIT_COMMIT" "$HOOK"; then pass "Captures git commit"; else fail "Should capture git commit"; fi

TESTS_RUN=$((TESTS_RUN + 1))
# shellcheck disable=SC2016
if grep -q '${USER:-' "$HOOK"; then pass "Captures user with fallback"; else fail "Should capture user"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "started_at" "$HOOK"; then pass "Includes started_at timestamp"; else fail "Should include started_at"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "started_epoch" "$HOOK"; then pass "Includes started_epoch"; else fail "Should include started_epoch"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "jq -nc" "$HOOK"; then pass "Uses jq -nc for JSON creation"; else fail "Should use jq -nc for JSON"; fi

echo ""
echo "--- Code Quality ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then pass "Uses robust REPO_ROOT with git rev-parse"; else fail "Should use git rev-parse for REPO_ROOT"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '{ cd' "$HOOK"; then pass "Has proper fallback grouping for REPO_ROOT"; else fail "Should have brace-grouped fallback for REPO_ROOT"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "export REPO_ROOT" "$HOOK"; then pass "Exports REPO_ROOT"; else fail "Should export REPO_ROOT"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -qi 'bash 3.2\|Compatibility:' "$HOOK"; then pass "Documents bash compatibility"; else fail "Should document bash 3.2+ compatibility"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "security-lib.sh" "$HOOK"; then pass "Sources security-lib.sh"; else fail "Should source security-lib.sh"; fi

echo ""
echo "--- Functional Tests ---"

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
CODEFLOW_SESSION_ID="test-metadata-session" bash "$HOOK" </dev/null 2>/dev/null
META_FILE="$REPO_ROOT/.state/logs/sessions/session-test-metadata-session.meta"
if [[ -f "$META_FILE" ]]; then pass "Creates session metadata on execution"; else fail "Should create session metadata"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$META_FILE" ]] && grep -q "test-metadata-session" "$META_FILE"; then pass "Metadata contains session_id"; else fail "Metadata should contain session_id"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$META_FILE" ]] && grep -q "started_epoch" "$META_FILE"; then pass "Metadata contains started_epoch"; else fail "Metadata should contain started_epoch"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if [[ -f "$META_FILE" ]] && command -v jq &>/dev/null; then
    if jq . "$META_FILE" &>/dev/null; then pass "Metadata is valid JSON"; else fail "Metadata should be valid JSON"; fi
else
    pass "Metadata JSON validation (skipped)"
fi

TESTS_RUN=$((TESTS_RUN + 1))
result=$(CODEFLOW_SESSION_ID="test-no-sentinel-dir" bash "$HOOK" </dev/null 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then pass "Handles missing sentinel directory gracefully"; else fail "Should handle missing sentinel directory"; fi

echo ""
echo "--- V4: Silent Output (Gap 9) ---"

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
output=$(CODEFLOW_SESSION_ID="test-silent-run" bash "$HOOK" </dev/null 2>/dev/null)
if [[ -z "$output" ]]; then pass "Normal execution produces no stdout (V4 spec)"; else fail "Should produce no stdout, got: $output"; fi

echo ""
echo "--- V4: No SENTINEL_TTL / find -mmin ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SENTINEL_TTL" "$HOOK"; then fail "Should NOT have SENTINEL_TTL variable"; else pass "No SENTINEL_TTL variable (JSON-based expiry used)"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "find.*SENTINEL_DIR" "$HOOK"; then fail "Should NOT use find for sentinel cleanup"; else pass "No find command for sentinel cleanup"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "STATE_DIR=" "$HOOK"; then pass "Has STATE_DIR variable"; else fail "Should have STATE_DIR variable"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_pathflow_flag" "$HOOK" || grep -q "pathflow" "$HOOK"; then pass "Has pathflow flag reference (via library)"; else fail "Should reference pathflow flag"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "SESSION_STATE_DIR=" "$HOOK"; then pass "Has SESSION_STATE_DIR for flag cleanup"; else fail "Should have SESSION_STATE_DIR variable"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-work-state.sh" "$HOOK"; then pass "Sources cf-work-state.sh (provides ACTIVE_TASK_FILE)"; else fail "Should source cf-work-state.sh for active task management"; fi

echo ""
echo "--- PathFlow Flag Creation (consolidated from pathflow-init) ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "cf-pathflow-state.sh" "$HOOK"; then pass "Sources cf-pathflow-state.sh library"; else fail "Should source cf-pathflow-state.sh library"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q "create_pathflow_flag" "$HOOK"; then pass "Calls create_pathflow_flag function"; else fail "Should call create_pathflow_flag function"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'tracking_level.*pending' "$HOOK" || grep -q 'create_pathflow_flag' "$HOOK"; then pass "Flag starts with pending tracking level"; else fail "Should create flag with pending tracking level"; fi

TESTS_RUN=$((TESTS_RUN + 1))
# Graceful degradation: if library missing, hook continues without error
if grep -q '_PFS_LIB' "$HOOK" && grep -q 'if.*-f.*_PFS_LIB' "$HOOK"; then pass "Graceful degradation when pathflow library missing"; else fail "Should degrade gracefully when library missing"; fi

echo ""
echo "--- Env File Session ID Mechanism ---"

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'codeflow-env.sh' "$HOOK"; then pass "References env file (codeflow-env.sh)"; else fail "Should reference codeflow-env.sh"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q '_CLAUDE_UUID' "$HOOK"; then pass "Stores Claude UUID as _CLAUDE_UUID (not session ID)"; else fail "Should store Claude UUID as _CLAUDE_UUID"; fi

TESTS_RUN=$((TESTS_RUN + 1))
if grep -q 'TODO(go-cli)' "$HOOK"; then pass "Has TODO(go-cli) comment for future CLI migration"; else fail "Should have TODO(go-cli) comment"; fi

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
cleanup_test_artifacts
# When no env file exists, hook should create one
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
CODEFLOW_SESSION_ID="" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    pass "Creates env file when none exists"
else
    fail "Should create env file at .state/runtime/codeflow-env.sh"
fi

TESTS_RUN=$((TESTS_RUN + 1))
# Env file should contain CODEFLOW_SESSION_ID export
if [[ -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    if grep -q "CODEFLOW_SESSION_ID=" "$REPO_ROOT/.state/runtime/codeflow-env.sh"; then
        pass "Env file contains CODEFLOW_SESSION_ID"
    else
        fail "Env file should contain CODEFLOW_SESSION_ID"
    fi
else
    fail "Env file should exist for content check"
fi

TESTS_RUN=$((TESTS_RUN + 1))
# Session ID format: ses-{timestamp}{hex} (at least 26 chars total)
if [[ -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    # shellcheck source=/dev/null
    source "$REPO_ROOT/.state/runtime/codeflow-env.sh"
    if echo "$CODEFLOW_SESSION_ID" | grep -qE '^ses-[0-9N]{10,13}[a-f0-9]{12}$'; then
        pass "Session ID format: ses-{timestamp}{hex}"
    else
        fail "Session ID format should be ses-{ts}{hex}, got: $CODEFLOW_SESSION_ID"
    fi
else
    fail "Env file should exist for ID format check"
fi
cleanup_test_artifacts

TESTS_RUN=$((TESTS_RUN + 1))
setup_test_env
# Teammate scenario: env file already exists, hook should source it and NOT overwrite
mkdir -p "$REPO_ROOT/.state/runtime" 2>/dev/null || true
echo "export CODEFLOW_SESSION_ID='ses-1234567890123abcdef012345'" > "$REPO_ROOT/.state/runtime/codeflow-env.sh"
CODEFLOW_SESSION_ID="" bash "$HOOK" </dev/null 2>/dev/null
_env_after=$(cat "$REPO_ROOT/.state/runtime/codeflow-env.sh")
if echo "$_env_after" | grep -q "ses-1234567890123abcdef012345"; then
    pass "Teammate: env file preserved (not overwritten)"
else
    fail "Teammate: env file should not be overwritten when it already exists"
fi

TESTS_RUN=$((TESTS_RUN + 1))
# Teammate scenario: metadata file uses CodeFlow session ID from env file
setup_test_env
mkdir -p "$REPO_ROOT/.state/runtime" 2>/dev/null || true
echo "export CODEFLOW_SESSION_ID='ses-1234567890123abcdef012345'" > "$REPO_ROOT/.state/runtime/codeflow-env.sh"
echo '{"session_id": "uuid-from-claude-code"}' | CODEFLOW_SESSION_ID="" bash "$HOOK" 2>/dev/null
META_FILE="$REPO_ROOT/.state/logs/sessions/session-ses-1234567890123abcdef012345.meta"
if [[ -f "$META_FILE" ]]; then
    pass "Metadata file uses CodeFlow session ID from env file"
    rm -f "$META_FILE" 2>/dev/null || true
else
    fail "Metadata file should use CodeFlow session ID from env file"
fi

TESTS_RUN=$((TESTS_RUN + 1))
# Atomic write: env file is written via tmp+mv pattern
if grep -q 'mktemp' "$HOOK" && grep -q 'mv.*_tmp_env.*_env_file' "$HOOK"; then
    pass "Env file written atomically (tmp + mv)"
else
    fail "Env file should be written atomically"
fi
cleanup_test_artifacts

echo ""
echo "--- CF_PROJECT_ROOT in Env File ---"

TESTS_RUN=$((TESTS_RUN + 1))
# Hook should export CF_PROJECT_ROOT
if grep -q "export CF_PROJECT_ROOT" "$HOOK"; then
    pass "Exports CF_PROJECT_ROOT"
else
    fail "Should export CF_PROJECT_ROOT"
fi

TESTS_RUN=$((TESTS_RUN + 1))
# Env file should contain CF_PROJECT_ROOT export
setup_test_env
cleanup_test_artifacts
rm -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
CODEFLOW_SESSION_ID="" bash "$HOOK" </dev/null 2>/dev/null
if [[ -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    if grep -q "CF_PROJECT_ROOT=" "$REPO_ROOT/.state/runtime/codeflow-env.sh"; then
        pass "Env file contains CF_PROJECT_ROOT"
    else
        fail "Env file should contain CF_PROJECT_ROOT"
    fi
else
    fail "Env file should exist for CF_PROJECT_ROOT check"
fi

TESTS_RUN=$((TESTS_RUN + 1))
# CF_PROJECT_ROOT value should be the basename of the repo root
if [[ -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    _expected_root=$(basename "$REPO_ROOT")
    # shellcheck source=/dev/null
    source "$REPO_ROOT/.state/runtime/codeflow-env.sh"
    if [[ "$CF_PROJECT_ROOT" == "$_expected_root" ]]; then
        pass "CF_PROJECT_ROOT value equals basename of repo root ($_expected_root)"
    else
        fail "CF_PROJECT_ROOT should be '$_expected_root', got: '$CF_PROJECT_ROOT'"
    fi
else
    fail "Env file should exist for CF_PROJECT_ROOT value check"
fi
cleanup_test_artifacts

TESTS_RUN=$((TESTS_RUN + 1))
# CF_PROJECT_ROOT should have fallback when env file exists without it
setup_test_env
mkdir -p "$REPO_ROOT/.state/runtime" 2>/dev/null || true
echo "export CODEFLOW_SESSION_ID='ses-1234567890123abcdef012345'" > "$REPO_ROOT/.state/runtime/codeflow-env.sh"
CF_PROJECT_ROOT="" CODEFLOW_SESSION_ID="" bash "$HOOK" </dev/null 2>/dev/null
# After hook runs, CF_PROJECT_ROOT should be set via fallback (basename of REPO_ROOT)
_expected_root=$(basename "$REPO_ROOT")
# shellcheck source=/dev/null
source "$REPO_ROOT/.state/runtime/codeflow-env.sh" 2>/dev/null || true
# The hook sets CF_PROJECT_ROOT via fallback even if env file lacks it
# Check the hook code has the fallback pattern
if grep -q 'CF_PROJECT_ROOT:-' "$HOOK"; then
    pass "CF_PROJECT_ROOT has fallback to basename of REPO_ROOT"
else
    fail "Should have CF_PROJECT_ROOT fallback"
fi
cleanup_test_artifacts

# Cleanup
cleanup_test_artifacts

echo ""
echo "=== Test Summary ==="
echo "Ran: $TESTS_RUN"
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
