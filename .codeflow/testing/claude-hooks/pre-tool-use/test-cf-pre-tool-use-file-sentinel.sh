#!/usr/bin/env bash
# Test: cf-pre-tool-use-file-sentinel.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-file-sentinel.sh
#
# Comprehensive tests for file-sentinel pre-tool-use hook:
#   - Tool filtering (Edit|Write only)
#   - Sentinel library integration
#   - Config-driven pattern matching (fallback)
#   - new_file_only flag handling
#   - Prerequisite operation checking
#   - File type detection

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-file-sentinel.sh"
SENTINEL_DIR="/tmp/claude/managed/sentinels"

export REPO_ROOT

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

# Helper: Run hook with tool name and file path
run_file_sentinel() {
    local tool_name="$1"
    local file_path="$2"

    local json_input="{\"file_path\": \"$file_path\"}"

    local output exit_code
    output=$(TOOL_NAME="$tool_name" TOOL_INPUT="$json_input" bash "$HOOK" 2>&1) && exit_code=0 || exit_code=$?

    # shellcheck disable=SC2034  # HOOK_OUTPUT used by test assertions externally
    HOOK_OUTPUT="$output"
    HOOK_EXIT_CODE=$exit_code
}

# Helper: Create a test sentinel file
create_test_sentinel() {
    local skill="$1"
    local operation="$2"
    local ttl="${3:-300}"

    mkdir -p "$SENTINEL_DIR"

    local id
    id="test-$$-$(date +%s)"
    local filename="${skill}:${operation}-${id}.json"
    local created expires
    created=$(date +%s)
    expires=$((created + ttl))

    cat > "$SENTINEL_DIR/$filename" << EOF
{
  "sentinel": "${skill}:${operation}",
  "skill": "$skill",
  "operation": "$operation",
  "created": $created,
  "expires": $expires
}
EOF
    echo "$SENTINEL_DIR/$filename"
}

# Helper: Cleanup test sentinels
cleanup_test_sentinels() {
    rm -f "$SENTINEL_DIR"/*test-*.json 2>/dev/null || true
}

echo "=== Testing cf-pre-tool-use-file-sentinel.sh ==="
echo ""

# =============================================================================
# BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

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
    skip "Shellcheck not available"
fi

# Test 4: Uses strict mode
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 5: Has proper header comments
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 6: Has Matcher for Edit|Write in header
if grep -q "Matcher:" "$HOOK" && grep -q "Edit|Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write in header"
else
    fail "Should have Matcher for Edit|Write"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 7: Exits 0 for non-Edit/Write tools (Bash)
result=$(TOOL_NAME="Bash" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 8: Exits 0 for non-Edit/Write tools (Read)
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 9: Exits 0 for non-Edit/Write tools (Grep)
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 10: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Edit" TOOL_INPUT="" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 11: Exits 0 when empty file_path
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when empty file_path"
else
    fail "Should exit 0 when empty file_path"
fi

echo ""
echo "--- Allowed Operations (non-protected files) ---"

# Test 12: Allows editing regular files
run_file_sentinel "Edit" "/tmp/test.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows editing regular files"
else
    fail "Should allow editing regular files"
fi

# Test 13: Allows Write to regular files
run_file_sentinel "Write" "/tmp/test.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows Write to regular files"
else
    fail "Should allow Write to regular files"
fi

# Test 14: Allows editing .ts files
run_file_sentinel "Edit" "src/component.ts"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows editing .ts files"
else
    fail "Should allow editing .ts files"
fi

# Test 15: Allows editing .json files
run_file_sentinel "Edit" "package.json"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Allows editing .json files"
else
    fail "Should allow editing .json files"
fi

echo ""
echo "--- Config-Driven Features ---"

# Test 16: References enforcement-policy.json
if grep -q "enforcement-policy.json" "$HOOK"; then
    pass "References enforcement-policy.json"
else
    fail "Should reference enforcement-policy.json"
fi

# Test 17: Reads skills from config
if grep -q '\.skills\[' "$HOOK" || grep -q "sentinel_find_skill_for_file" "$HOOK"; then
    pass "Reads skills configuration"
else
    fail "Should read skills from config"
fi

# Test 18: Has sentinel library loading
if grep -q "SENTINEL_LIB" "$HOOK" && grep -q "cf-sentinel.sh" "$HOOK"; then
    pass "Has sentinel library loading"
else
    fail "Should load sentinel library"
fi

# Test 19: Has fallback when sentinel library not available
if grep -q "SENTINEL_LIB_LOADED" "$HOOK" && grep -q "check_sentinel_fallback" "$HOOK"; then
    pass "Has fallback for missing sentinel library"
else
    fail "Should have fallback logic"
fi

echo ""
echo "--- new_file_only Flag ---"

# Test 20: References new_file_only in code
if grep -q "new_file_only" "$HOOK"; then
    pass "References new_file_only flag"
else
    fail "Should reference new_file_only flag"
fi

# Test 21: Checks FILE_EXISTS for new_file_only logic
if grep -q "FILE_EXISTS" "$HOOK"; then
    pass "Checks FILE_EXISTS for new_file_only"
else
    fail "Should check FILE_EXISTS"
fi

# Test 22: Has sentinel_is_new_file_only function check
if grep -q "sentinel_is_new_file_only" "$HOOK"; then
    pass "Has sentinel_is_new_file_only function check"
else
    fail "Should check sentinel_is_new_file_only"
fi

echo ""
echo "--- Prerequisite Operations ---"

# Test 23: Has prerequisite check
if grep -q "prerequisite" "$HOOK"; then
    pass "Has prerequisite check"
else
    fail "Should check prerequisites"
fi

# Test 24: Has sentinel_get_operation_prerequisite check
if grep -q "sentinel_get_operation_prerequisite" "$HOOK"; then
    pass "Has sentinel_get_operation_prerequisite check"
else
    fail "Should use sentinel_get_operation_prerequisite"
fi

# Test 25: Has block_missing_prerequisite function
if grep -q "block_missing_prerequisite" "$HOOK"; then
    pass "Has block_missing_prerequisite function"
else
    fail "Should have block_missing_prerequisite function"
fi

# Test 26: Prerequisite block message includes SEQUENCE
if grep -q "SEQUENCE:" "$HOOK"; then
    pass "Prerequisite block shows SEQUENCE"
else
    fail "Should show SEQUENCE in prerequisite block"
fi

echo ""
echo "--- File Type Detection ---"

# Test 27: Has get_file_type function
if grep -q "get_file_type" "$HOOK"; then
    pass "Has get_file_type function"
else
    fail "Should have get_file_type function"
fi

# Test 28: Detects ADR documents
if grep -q "\*-adr.md" "$HOOK" && grep -q "ADR document" "$HOOK"; then
    pass "Detects ADR documents"
else
    fail "Should detect ADR documents"
fi

# Test 29: Detects Brief documents
if grep -q "\*-brief.md" "$HOOK" && grep -q "Brief document" "$HOOK"; then
    pass "Detects Brief documents"
else
    fail "Should detect Brief documents"
fi

# Test 30: Detects Shell scripts
if grep -q "\*.sh" "$HOOK" && grep -q "Shell script" "$HOOK"; then
    pass "Detects Shell scripts"
else
    fail "Should detect Shell scripts"
fi

# Test 31: Detects Python scripts
if grep -q "\*.py" "$HOOK" && grep -q "Python script" "$HOOK"; then
    pass "Detects Python scripts"
else
    fail "Should detect Python scripts"
fi

echo ""
echo "--- Blocked Operation Messages ---"

# Test 32: Has block_missing_sentinel function
if grep -q "block_missing_sentinel" "$HOOK"; then
    pass "Has block_missing_sentinel function"
else
    fail "Should have block_missing_sentinel function"
fi

# Test 33: Block message includes MUST: Skill
if grep -q "MUST: Skill" "$HOOK"; then
    pass "Block message includes MUST: Skill"
else
    fail "Block message should include MUST: Skill"
fi

# Test 34: Block message includes file path
if grep -q 'File: \$FILE_PATH' "$HOOK" || grep -q 'File:.*FILE_PATH' "$HOOK"; then
    pass "Block message includes file path"
else
    fail "Block message should include file path"
fi

echo ""
echo "--- Security Logging ---"

# Test 35: Has security event logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should have security event logging"
fi

# Test 36: Logs blocked operations
if grep -q 'log_security_event.*blocked' "$HOOK"; then
    pass "Logs blocked operations"
else
    fail "Should log blocked operations"
fi

# Test 37: Logs allowed operations (audit)
if grep -q 'log_security_event.*audit' "$HOOK"; then
    pass "Logs allowed operations (audit)"
else
    fail "Should log allowed operations"
fi

echo ""
echo "--- Code Quality ---"

# Test 38: Uses robust REPO_ROOT with git rev-parse
if grep -q "git rev-parse --show-toplevel" "$HOOK"; then
    pass "Uses robust REPO_ROOT with git rev-parse"
else
    fail "Should use git rev-parse for REPO_ROOT"
fi

# Test 39: Has proper fallback grouping for REPO_ROOT
if grep -q '|| {.*cd.*&&.*pwd.*}' "$HOOK"; then
    pass "Has proper fallback grouping for REPO_ROOT"
else
    fail "Should have proper fallback grouping"
fi

# Test 40: Has bash 3.2+ compatibility note
if grep -q "bash 3.2" "$HOOK" || grep -q "macOS compatible" "$HOOK"; then
    pass "Has bash 3.2+ compatibility note"
else
    fail "Should note bash 3.2+ compatibility"
fi

# Test 41: Exports REPO_ROOT
if grep -q "export REPO_ROOT" "$HOOK"; then
    pass "Exports REPO_ROOT"
else
    fail "Should export REPO_ROOT"
fi

# Test 42: Has jq fallback for file_path extraction
if grep -q "grep -o" "$HOOK" && grep -q "file_path" "$HOOK"; then
    pass "Has jq fallback for file_path extraction"
else
    fail "Should have jq fallback"
fi

echo ""
echo "--- Path Normalization ---"

# Test 43: Normalizes file path to relative
if grep -q "REL_PATH" "$HOOK"; then
    pass "Normalizes file path to relative"
else
    fail "Should normalize path to relative"
fi

# Test 44: Removes leading ./
if grep -q '#./' "$HOOK" || grep -q 'REL_PATH.*#' "$HOOK"; then
    pass "Removes leading ./"
else
    fail "Should remove leading ./"
fi

echo ""
echo "--- Sentinel Validation ---"

# Test 45: Has check_sentinel_fallback function
if grep -q "check_sentinel_fallback()" "$HOOK"; then
    pass "Has check_sentinel_fallback function"
else
    fail "Should have check_sentinel_fallback function"
fi

# Test 46: Checks sentinel expiry
if grep -q "expires" "$HOOK" && grep -q 'date +%s' "$HOOK"; then
    pass "Checks sentinel expiry"
else
    fail "Should check sentinel expiry"
fi

# Test 47: Uses sentinel_validate when available
if grep -q "sentinel_validate" "$HOOK"; then
    pass "Uses sentinel_validate when available"
else
    fail "Should use sentinel_validate"
fi

echo ""
echo "--- V3 Spec Compliance ---"

# Test 48: Mentions V3 spec in header
if grep -q "V3" "$HOOK"; then
    pass "Mentions V3 spec compliance"
else
    fail "Should mention V3 spec"
fi

# Test 49: References sentinel library path
if grep -q "sentinel/cf-sentinel.sh" "$HOOK"; then
    pass "References sentinel library path"
else
    fail "Should reference sentinel library"
fi

# Test 50: Has exit code 2 for blocked operations
if grep -q "exit 2" "$HOOK"; then
    pass "Has exit code 2 for blocked operations"
else
    fail "Should exit 2 for blocked operations"
fi

cleanup_test_sentinels

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
