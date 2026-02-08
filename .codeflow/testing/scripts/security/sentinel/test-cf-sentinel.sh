#!/usr/bin/env bash
# Test: cf-sentinel.sh
# Location: .codeflow/testing/scripts/security/sentinel/test-cf-sentinel.sh
#
# Tests the sentinel library for skills enforcement

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
SENTINEL_DIR="$REPO_ROOT/.codeflow/scripts/security/sentinel"
MODULE="$SENTINEL_DIR/cf-sentinel.sh"

export REPO_ROOT

# Create test sentinel directory in /tmp/claude for isolation
TEST_SENTINEL_DIR="/tmp/claude/test-sentinels-$$"
mkdir -p "$TEST_SENTINEL_DIR"

# Source the module to test functions
# shellcheck source=/dev/null
source "$MODULE"

# Override sentinel directory for testing
SENTINEL_DIR="$TEST_SENTINEL_DIR"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

cleanup() {
    rm -rf "$TEST_SENTINEL_DIR"
}
trap cleanup EXIT

echo "=== Testing cf-sentinel.sh ==="
echo ""

# Test 1: File exists
if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found"; fi

# Test 2: File is readable (sentinel libs are sourced, not executed)
if [[ -r "$MODULE" ]]; then pass "Module is readable"; else fail "Module not readable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$MODULE" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

# Test 4: Has proper header comments
if grep -q "Purpose:" "$MODULE" && grep -q "Usage:" "$MODULE"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

# Test 5: Has compatibility note
if grep -q "bash 3.2" "$MODULE" || grep -q "macOS" "$MODULE"; then
    pass "Has compatibility note"
else
    fail "Should have compatibility note for bash 3.2/macOS"
fi

# ============================================================================
# Function existence tests
# ============================================================================

# Test 6: Has sentinel_load_config function
if declare -f sentinel_load_config &>/dev/null; then
    pass "Has sentinel_load_config function"
else
    fail "Missing sentinel_load_config function"
fi

# Test 7: Has sentinel_init function
if declare -f sentinel_init &>/dev/null; then
    pass "Has sentinel_init function"
else
    fail "Missing sentinel_init function"
fi

# Test 8: Has sentinel_create function
if declare -f sentinel_create &>/dev/null; then
    pass "Has sentinel_create function"
else
    fail "Missing sentinel_create function"
fi

# Test 9: Has sentinel_validate function
if declare -f sentinel_validate &>/dev/null; then
    pass "Has sentinel_validate function"
else
    fail "Missing sentinel_validate function"
fi

# Test 10: Has sentinel_exists function
if declare -f sentinel_exists &>/dev/null; then
    pass "Has sentinel_exists function"
else
    fail "Missing sentinel_exists function"
fi

# Test 11: Has sentinel_cleanup_expired function
if declare -f sentinel_cleanup_expired &>/dev/null; then
    pass "Has sentinel_cleanup_expired function"
else
    fail "Missing sentinel_cleanup_expired function"
fi

# Test 12: Has sentinel_cleanup_skill function
if declare -f sentinel_cleanup_skill &>/dev/null; then
    pass "Has sentinel_cleanup_skill function"
else
    fail "Missing sentinel_cleanup_skill function"
fi

# Test 13: Has sentinel_cleanup_all function
if declare -f sentinel_cleanup_all &>/dev/null; then
    pass "Has sentinel_cleanup_all function"
else
    fail "Missing sentinel_cleanup_all function"
fi

# Test 14: Has sentinel_list function
if declare -f sentinel_list &>/dev/null; then
    pass "Has sentinel_list function"
else
    fail "Missing sentinel_list function"
fi

# Test 15: Has sentinel_get_operation_ttl function
if declare -f sentinel_get_operation_ttl &>/dev/null; then
    pass "Has sentinel_get_operation_ttl function"
else
    fail "Missing sentinel_get_operation_ttl function"
fi

# Test 16: Has sentinel_get_operation_pattern function
if declare -f sentinel_get_operation_pattern &>/dev/null; then
    pass "Has sentinel_get_operation_pattern function"
else
    fail "Missing sentinel_get_operation_pattern function"
fi

# Test 17: Has sentinel_get_skill_operations function
if declare -f sentinel_get_skill_operations &>/dev/null; then
    pass "Has sentinel_get_skill_operations function"
else
    fail "Missing sentinel_get_skill_operations function"
fi

# Test 18: Has sentinel_get_all_skills function
if declare -f sentinel_get_all_skills &>/dev/null; then
    pass "Has sentinel_get_all_skills function"
else
    fail "Missing sentinel_get_all_skills function"
fi

# Test 19: Has sentinel_find_skill_for_command function
if declare -f sentinel_find_skill_for_command &>/dev/null; then
    pass "Has sentinel_find_skill_for_command function"
else
    fail "Missing sentinel_find_skill_for_command function"
fi

# Test 20: Has sentinel_find_skill_for_file function
if declare -f sentinel_find_skill_for_file &>/dev/null; then
    pass "Has sentinel_find_skill_for_file function"
else
    fail "Missing sentinel_find_skill_for_file function"
fi

# Test 21: Has sentinel_find_skill_for_grep function
if declare -f sentinel_find_skill_for_grep &>/dev/null; then
    pass "Has sentinel_find_skill_for_grep function"
else
    fail "Missing sentinel_find_skill_for_grep function"
fi

# Test 22: Has sentinel_find_by_operation function
if declare -f sentinel_find_by_operation &>/dev/null; then
    pass "Has sentinel_find_by_operation function"
else
    fail "Missing sentinel_find_by_operation function"
fi

# Test 23: Has sentinel_get_operation_guidance function
if declare -f sentinel_get_operation_guidance &>/dev/null; then
    pass "Has sentinel_get_operation_guidance function"
else
    fail "Missing sentinel_get_operation_guidance function"
fi

# Test 24: Has sentinel_is_new_file_only function
if declare -f sentinel_is_new_file_only &>/dev/null; then
    pass "Has sentinel_is_new_file_only function"
else
    fail "Missing sentinel_is_new_file_only function"
fi

# Test 25: Has sentinel_get_skill_exclude_patterns function
if declare -f sentinel_get_skill_exclude_patterns &>/dev/null; then
    pass "Has sentinel_get_skill_exclude_patterns function"
else
    fail "Missing sentinel_get_skill_exclude_patterns function"
fi

# Test 26: Has sentinel_matches_exclude_pattern function
if declare -f sentinel_matches_exclude_pattern &>/dev/null; then
    pass "Has sentinel_matches_exclude_pattern function"
else
    fail "Missing sentinel_matches_exclude_pattern function"
fi

# Test 27: Has sentinel_get_operation_prerequisite function
if declare -f sentinel_get_operation_prerequisite &>/dev/null; then
    pass "Has sentinel_get_operation_prerequisite function"
else
    fail "Missing sentinel_get_operation_prerequisite function"
fi

# Test 28: Has sentinel_get_operation_tool function
if declare -f sentinel_get_operation_tool &>/dev/null; then
    pass "Has sentinel_get_operation_tool function"
else
    fail "Missing sentinel_get_operation_tool function"
fi

# ============================================================================
# Functional tests - sentinel creation and validation
# ============================================================================

# Test 29: sentinel_init creates directory
sentinel_init
if [[ -d "$SENTINEL_DIR" ]]; then
    pass "sentinel_init creates directory"
else
    fail "sentinel_init should create directory"
fi

# Test 30: sentinel_create creates sentinel file
filename=$(sentinel_create "test-skill" "test-op" "test-pattern" 60)
if [[ -n "$filename" ]] && [[ -f "$SENTINEL_DIR/$filename" ]]; then
    pass "sentinel_create creates sentinel file"
else
    fail "sentinel_create should create sentinel file"
fi

# Test 31: Created sentinel file is valid JSON
if command -v jq &>/dev/null; then
    if jq empty "$SENTINEL_DIR/$filename" 2>/dev/null; then
        pass "Created sentinel file is valid JSON"
    else
        fail "Created sentinel file should be valid JSON"
    fi
else
    pass "jq not available (skipped JSON validation)"
fi

# Test 32: Created sentinel has required fields
if command -v jq &>/dev/null; then
    has_fields=true
    for field in skill operation tool_pattern created expires id; do
        if ! jq -e ".$field" "$SENTINEL_DIR/$filename" &>/dev/null; then
            has_fields=false
            break
        fi
    done
    if $has_fields; then
        pass "Created sentinel has required fields"
    else
        fail "Created sentinel should have all required fields"
    fi
else
    pass "jq not available (skipped field check)"
fi

# Test 33: sentinel_validate returns 0 for valid sentinel
if sentinel_validate "test-skill" "test-pattern"; then
    pass "sentinel_validate returns 0 for valid sentinel"
else
    fail "sentinel_validate should return 0 for valid sentinel"
fi

# Test 34: sentinel_validate returns 1 for non-matching pattern
if ! sentinel_validate "test-skill" "non-matching-pattern"; then
    pass "sentinel_validate returns 1 for non-matching pattern"
else
    fail "sentinel_validate should return 1 for non-matching pattern"
fi

# Test 35: sentinel_validate returns 1 for non-existent skill
if ! sentinel_validate "non-existent-skill" "test-pattern"; then
    pass "sentinel_validate returns 1 for non-existent skill"
else
    fail "sentinel_validate should return 1 for non-existent skill"
fi

# Test 36: sentinel_exists is alias for sentinel_validate
if sentinel_exists "test-skill" "test-pattern"; then
    pass "sentinel_exists works as alias"
else
    fail "sentinel_exists should work as alias"
fi

# Test 37: sentinel_cleanup_skill removes skill sentinels
cleaned=$(sentinel_cleanup_skill "test-skill")
if [[ "$cleaned" -ge 1 ]]; then
    pass "sentinel_cleanup_skill removes skill sentinels"
else
    fail "sentinel_cleanup_skill should remove sentinels (cleaned: $cleaned)"
fi

# Test 38: sentinel_validate returns 1 after cleanup
if ! sentinel_validate "test-skill" "test-pattern"; then
    pass "sentinel_validate returns 1 after cleanup"
else
    fail "sentinel_validate should return 1 after cleanup"
fi

# Test 39: sentinel_cleanup_all removes all sentinels
# Create some sentinels first
sentinel_create "skill-a" "op-a" "pattern-a" 60
sentinel_create "skill-b" "op-b" "pattern-b" 60
cleaned=$(sentinel_cleanup_all)
if [[ "$cleaned" -ge 2 ]]; then
    pass "sentinel_cleanup_all removes all sentinels"
else
    fail "sentinel_cleanup_all should remove all sentinels (cleaned: $cleaned)"
fi

# Test 40: sentinel_list outputs format
list_output=$(sentinel_list)
if [[ "$list_output" == *"Active Sentinels"* ]]; then
    pass "sentinel_list outputs expected format"
else
    fail "sentinel_list should output 'Active Sentinels' header"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
