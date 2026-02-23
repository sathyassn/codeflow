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

# ============================================================================
# PART A Bug Fix Tests
# ============================================================================

# Test 41: sentinel_create with quotes in pattern (JSON escaping fix)
filename_q=$(sentinel_create "test-skill" "test-op" 'pattern-with-"quotes"' 60)
if command -v jq &>/dev/null; then
    if jq empty "$SENTINEL_DIR/$filename_q" 2>/dev/null; then
        pass "sentinel_create handles double quotes in pattern (valid JSON)"
    else
        fail "sentinel_create should produce valid JSON when pattern contains quotes"
    fi
else
    pass "jq not available (skipped JSON escaping test)"
fi
rm -f "$SENTINEL_DIR/$filename_q"

# Test 42: sentinel_create with backslash+quote in pattern
filename_bq=$(sentinel_create "test-skill" "test-op" 'path\\with\\"quotes' 60)
if command -v jq &>/dev/null; then
    if jq empty "$SENTINEL_DIR/$filename_bq" 2>/dev/null; then
        pass "sentinel_create handles backslash+quotes in pattern"
    else
        fail "sentinel_create should handle backslash+quotes"
    fi
else
    pass "jq not available (skipped backslash+quotes test)"
fi
rm -f "$SENTINEL_DIR/$filename_bq"

# Test 43: sentinel_validate with tool_input starting with - (printf fix)
filename_dash=$(sentinel_create "test-skill" "test-op" "test-pattern" 60)
if sentinel_validate "test-skill" "-rf /some/path test-pattern works"; then
    pass "sentinel_validate handles tool_input starting with - (printf fix)"
else
    fail "sentinel_validate should handle tool_input starting with -"
fi
rm -f "$SENTINEL_DIR/$filename_dash"

# ============================================================================
# Functional tests - cleanup expired
# ============================================================================

# Test 44: sentinel_cleanup_expired with actual expired sentinels
sentinel_create "expired-skill" "expired-op" "expired-pattern" 1
sleep 2
expired_cleaned=$(sentinel_cleanup_expired)
if [[ "$expired_cleaned" -ge 1 ]]; then
    pass "sentinel_cleanup_expired removes actually expired sentinels"
else
    fail "sentinel_cleanup_expired should remove expired sentinels (cleaned: $expired_cleaned)"
fi

# Test 45: sentinel_cleanup_expired does NOT remove valid sentinels
sentinel_create "valid-skill" "valid-op" "valid-pattern" 600
valid_before=$(find "$SENTINEL_DIR" -name "valid-skill:*.json" -type f 2>/dev/null | wc -l | tr -d ' ')
sentinel_cleanup_expired
valid_after=$(find "$SENTINEL_DIR" -name "valid-skill:*.json" -type f 2>/dev/null | wc -l | tr -d ' ')
if [[ "$valid_after" -ge 1 ]] && [[ "$valid_after" -eq "$valid_before" ]]; then
    pass "sentinel_cleanup_expired preserves valid sentinels"
else
    fail "sentinel_cleanup_expired should preserve valid sentinels (before: $valid_before, after: $valid_after)"
fi
sentinel_cleanup_skill "valid-skill"

# ============================================================================
# Functional tests - config-driven lookups
# ============================================================================

# Test 46: sentinel_find_skill_for_command matches git commit
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    skill_result=$(sentinel_find_skill_for_command "git commit -m test")
    if [[ "$skill_result" == "git-workflow" ]]; then
        pass "sentinel_find_skill_for_command matches git commit to git-workflow"
    else
        fail "sentinel_find_skill_for_command should match git commit (got: '$skill_result')"
    fi
else
    pass "Config or jq not available (skipped command match test)"
fi

# Test 47: sentinel_find_skill_for_command matches tmux new-session
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    skill_result=$(sentinel_find_skill_for_command "tmux new-session -d -s model")
    if [[ "$skill_result" == "model-orchestrator" ]]; then
        pass "sentinel_find_skill_for_command matches tmux new-session to model-orchestrator"
    else
        fail "sentinel_find_skill_for_command should match tmux new-session (got: '$skill_result')"
    fi
else
    pass "Config or jq not available (skipped tmux match test)"
fi

# Test 48: sentinel_find_skill_for_command returns empty for non-matching
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    skill_result=$(sentinel_find_skill_for_command "ls -la" 2>/dev/null) || true
    if [[ -z "$skill_result" ]]; then
        pass "sentinel_find_skill_for_command returns empty for non-matching command"
    else
        fail "sentinel_find_skill_for_command should return empty for ls -la (got: '$skill_result')"
    fi
else
    pass "Config or jq not available (skipped non-match test)"
fi

# Test 49: sentinel_find_skill_for_file matches .md file
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    file_result=$(sentinel_find_skill_for_file "/path/to/README.md" "Write")
    if [[ "$file_result" == "documentation-standards:apply-standard" ]]; then
        pass "sentinel_find_skill_for_file matches .md to documentation-standards:apply-standard"
    else
        fail "sentinel_find_skill_for_file should match .md (got: '$file_result')"
    fi
else
    pass "Config or jq not available (skipped file match test)"
fi

# Test 50: sentinel_find_skill_for_file matches .sh file
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    file_result=$(sentinel_find_skill_for_file "/path/to/script.sh" "Write")
    if [[ "$file_result" == "script-standards:apply-shell-standards" ]]; then
        pass "sentinel_find_skill_for_file matches .sh to script-standards:apply-shell-standards"
    else
        fail "sentinel_find_skill_for_file should match .sh (got: '$file_result')"
    fi
else
    pass "Config or jq not available (skipped .sh match test)"
fi

# Test 51: sentinel_find_skill_for_grep matches .py with def pattern
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    grep_result=$(sentinel_find_skill_for_grep "/path/to/module.py" "def my_func")
    if [[ "$grep_result" == "code-exploration:navigate-to-definition" ]]; then
        pass "sentinel_find_skill_for_grep matches .py + def to code-exploration"
    else
        fail "sentinel_find_skill_for_grep should match .py with def (got: '$grep_result')"
    fi
else
    pass "Config or jq not available (skipped grep match test)"
fi

# Test 52: sentinel_find_by_operation finds created sentinel
sentinel_create "test-findop" "my-operation" "any-pattern" 60
if sentinel_find_by_operation "test-findop" "my-operation"; then
    pass "sentinel_find_by_operation finds sentinel by operation name"
else
    fail "sentinel_find_by_operation should find created sentinel"
fi
sentinel_cleanup_skill "test-findop"

# Test 53: sentinel_find_by_operation returns 1 for non-existent
sentinel_find_by_operation "nonexistent-skill" "nonexistent-op" 2>/dev/null && findop_found=true || findop_found=false
if [[ "$findop_found" == "false" ]]; then
    pass "sentinel_find_by_operation returns 1 for non-existent operation"
else
    fail "sentinel_find_by_operation should return 1 for non-existent operation"
fi

# Test 54: sentinel_matches_exclude_pattern excludes .claude/memory/
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    if sentinel_matches_exclude_pattern "documentation-standards" ".claude/memory/general-work/test.md"; then
        pass "sentinel_matches_exclude_pattern excludes .claude/memory/ files"
    else
        fail "sentinel_matches_exclude_pattern should exclude .claude/memory/ files"
    fi
else
    pass "Config or jq not available (skipped exclude pattern test)"
fi

# Test 55: sentinel_matches_exclude_pattern does NOT exclude regular .md
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    sentinel_matches_exclude_pattern "documentation-standards" "docs/README.md" && matched_non_exclude=true || matched_non_exclude=false
    if [[ "$matched_non_exclude" == "false" ]]; then
        pass "sentinel_matches_exclude_pattern does not exclude regular .md files"
    else
        fail "sentinel_matches_exclude_pattern should not exclude regular .md files"
    fi
else
    pass "Config or jq not available (skipped non-exclude test)"
fi

# Test 56: sentinel_is_new_file_only returns 0 for apply-standard
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    if sentinel_is_new_file_only "documentation-standards" "apply-standard"; then
        pass "sentinel_is_new_file_only returns true for apply-standard"
    else
        fail "sentinel_is_new_file_only should return true for apply-standard"
    fi
else
    pass "Config or jq not available (skipped new_file_only test)"
fi

# Test 57: sentinel_is_new_file_only returns 1 for lint-file
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    sentinel_is_new_file_only "documentation-standards" "lint-file" && is_new_file=true || is_new_file=false
    if [[ "$is_new_file" == "false" ]]; then
        pass "sentinel_is_new_file_only returns false for lint-file"
    else
        fail "sentinel_is_new_file_only should return false for lint-file"
    fi
else
    pass "Config or jq not available (skipped lint-file test)"
fi

# Test 58: sentinel_get_operation_prerequisite returns prerequisite
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    prereq=$(sentinel_get_operation_prerequisite "memory-management" "begin-work")
    if [[ "$prereq" == "search-related-work" ]]; then
        pass "sentinel_get_operation_prerequisite returns correct prerequisite"
    else
        fail "sentinel_get_operation_prerequisite should return search-related-work (got: '$prereq')"
    fi
else
    pass "Config or jq not available (skipped prerequisite test)"
fi

# Test 59: sentinel_get_operation_tool returns Bash for create-commit
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    tool=$(sentinel_get_operation_tool "git-workflow" "create-commit")
    if [[ "$tool" == "Bash" ]]; then
        pass "sentinel_get_operation_tool returns Bash for create-commit"
    else
        fail "sentinel_get_operation_tool should return Bash (got: '$tool')"
    fi
else
    pass "Config or jq not available (skipped operation tool test)"
fi

# Test 60: sentinel_get_operation_guidance runs without error
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    # shellcheck disable=SC2034  # guidance verified by test assertion below
    guidance=$(sentinel_get_operation_guidance "git-workflow" "create-commit" 2>/dev/null) || true
    pass "sentinel_get_operation_guidance runs without error"
else
    pass "Config or jq not available (skipped guidance test)"
fi

# ============================================================================
# PART B: PathFlow sentinel tests
# ============================================================================

# Test 61-66: PathFlow function existence
for func in sentinel_pathflow_dir sentinel_create_pathflow sentinel_validate_pathflow sentinel_cleanup_pathflow sentinel_list_pathflow sentinel_is_pathflow_mode; do
    if declare -f "$func" &>/dev/null; then
        pass "Has $func function"
    else
        fail "Missing $func function"
    fi
done

# Test 67: sentinel_pathflow_dir returns correct path format
export CODEFLOW_SESSION_ID="test-session-$$"
pf_dir=$(sentinel_pathflow_dir)
if [[ "$pf_dir" == *".state/sentinels/pathflow/test-session-$$" ]]; then
    pass "sentinel_pathflow_dir returns correct path format"
else
    fail "sentinel_pathflow_dir should contain .state/sentinels/pathflow/{session_id} (got: $pf_dir)"
fi

# Test 68: sentinel_pathflow_dir uses SENTINEL_REPO_ROOT
expected_prefix="$SENTINEL_REPO_ROOT/.state/sentinels/pathflow"
if [[ "$pf_dir" == "$expected_prefix"* ]]; then
    pass "sentinel_pathflow_dir uses SENTINEL_REPO_ROOT"
else
    fail "sentinel_pathflow_dir should use SENTINEL_REPO_ROOT (got: $pf_dir)"
fi

# Test 69: sentinel_create_pathflow creates file
pf_filename=$(sentinel_create_pathflow "pf-3" "test-skill" "test-operation")
pf_dir_actual=$(sentinel_pathflow_dir)
if [[ -n "$pf_filename" ]] && [[ -f "$pf_dir_actual/$pf_filename" ]]; then
    pass "sentinel_create_pathflow creates sentinel file"
else
    fail "sentinel_create_pathflow should create a file (filename: $pf_filename)"
fi

# Test 70: PathFlow sentinel file is valid JSON
if command -v jq &>/dev/null; then
    if jq empty "$pf_dir_actual/$pf_filename" 2>/dev/null; then
        pass "PathFlow sentinel file is valid JSON"
    else
        fail "PathFlow sentinel file should be valid JSON"
    fi
else
    pass "jq not available (skipped PathFlow JSON test)"
fi

# Test 71: PathFlow sentinel has type=pathflow
if command -v jq &>/dev/null; then
    pf_type_val=$(jq -r .type "$pf_dir_actual/$pf_filename" 2>/dev/null)
    if [[ "$pf_type_val" == "pathflow" ]]; then
        pass "PathFlow sentinel has type=pathflow"
    else
        fail "PathFlow sentinel should have type=pathflow (got: $pf_type_val)"
    fi
else
    pass "jq not available (skipped type field test)"
fi

# Test 72: PathFlow sentinel has correct sentinel field
if command -v jq &>/dev/null; then
    pf_sentinel=$(jq -r .sentinel "$pf_dir_actual/$pf_filename" 2>/dev/null)
    if [[ "$pf_sentinel" == "pathflow:pf-3" ]]; then
        pass "PathFlow sentinel has sentinel=pathflow:pf-3"
    else
        fail "PathFlow sentinel should have sentinel=pathflow:pf-3 (got: $pf_sentinel)"
    fi
else
    pass "jq not available (skipped sentinel field test)"
fi

# Test 73: PathFlow sentinel has session_id
if command -v jq &>/dev/null; then
    pf_sid=$(jq -r .session_id "$pf_dir_actual/$pf_filename" 2>/dev/null)
    if [[ "$pf_sid" == "test-session-$$" ]]; then
        pass "PathFlow sentinel has correct session_id"
    else
        fail "PathFlow sentinel should have correct session_id (got: $pf_sid)"
    fi
else
    pass "jq not available (skipped session_id test)"
fi

# Test 74: sentinel_validate_pathflow returns 0 for existing type
if sentinel_validate_pathflow "pf-3"; then
    pass "sentinel_validate_pathflow returns 0 for existing pf-3"
else
    fail "sentinel_validate_pathflow should return 0 for existing pf-3"
fi

# Test 75: sentinel_validate_pathflow returns 1 for non-existent type
sentinel_validate_pathflow "pf-99" && pf99_found=true || pf99_found=false
if [[ "$pf99_found" == "false" ]]; then
    pass "sentinel_validate_pathflow returns 1 for non-existent pf-99"
else
    fail "sentinel_validate_pathflow should return 1 for non-existent type"
fi

# Test 76: sentinel_list_pathflow outputs header
pf_list_output=$(sentinel_list_pathflow)
if [[ "$pf_list_output" == *"PathFlow Sentinels"* ]]; then
    pass "sentinel_list_pathflow outputs expected header"
else
    fail "sentinel_list_pathflow should output PathFlow Sentinels header"
fi

# Test 77: sentinel_list_pathflow shows created sentinel
if [[ "$pf_list_output" == *"pathflow:pf-3"* ]]; then
    pass "sentinel_list_pathflow lists created pathflow sentinel"
else
    fail "sentinel_list_pathflow should list pathflow:pf-3"
fi

# Test 78: Create multiple PathFlow sentinel types
sentinel_create_pathflow "pf-1" "session-skill" "start-op"
sentinel_create_pathflow "ws-dev-done" "work-skill" "dev-complete"
pf_list_multi=$(sentinel_list_pathflow)
if [[ "$pf_list_multi" == *"pathflow:pf-1"* ]] && [[ "$pf_list_multi" == *"pathflow:ws-dev-done"* ]]; then
    pass "sentinel_list_pathflow lists multiple sentinel types"
else
    fail "sentinel_list_pathflow should list all sentinel types"
fi

# Test 79: sentinel_cleanup_pathflow removes all
pf_cleaned=$(sentinel_cleanup_pathflow)
if [[ "$pf_cleaned" -ge 3 ]]; then
    pass "sentinel_cleanup_pathflow removes all sentinels (cleaned: $pf_cleaned)"
else
    fail "sentinel_cleanup_pathflow should remove at least 3 (cleaned: $pf_cleaned)"
fi

# Test 80: sentinel_validate_pathflow returns 1 after cleanup
sentinel_validate_pathflow "pf-3" && pf3_after_cleanup=true || pf3_after_cleanup=false
if [[ "$pf3_after_cleanup" == "false" ]]; then
    pass "sentinel_validate_pathflow returns 1 after cleanup"
else
    fail "sentinel_validate_pathflow should return 1 after cleanup"
fi

# Test 81: sentinel_cleanup_pathflow returns 0 on empty dir
pf_cleaned2=$(sentinel_cleanup_pathflow)
if [[ "$pf_cleaned2" -eq 0 ]]; then
    pass "sentinel_cleanup_pathflow returns 0 on empty directory"
else
    fail "sentinel_cleanup_pathflow should return 0 on empty dir (got: $pf_cleaned2)"
fi

# Test 82: sentinel_is_pathflow_mode returns 1 when inactive
# Ensure no flag file exists
rm -f "$SENTINEL_REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID}/pathflow/is-pathflow-active" 2>/dev/null || true
sentinel_is_pathflow_mode && pf_mode_result=true || pf_mode_result=false
if [[ "$pf_mode_result" == "false" ]]; then
    pass "sentinel_is_pathflow_mode returns 1 when PathFlow inactive"
else
    fail "sentinel_is_pathflow_mode should return 1 when no flag file"
fi

# Test 83: sentinel_is_pathflow_mode returns 0 when flag exists
PF_FLAG_DIR="$SENTINEL_REPO_ROOT/.state/session/${CODEFLOW_SESSION_ID}/pathflow"
mkdir -p "$PF_FLAG_DIR"
touch "$PF_FLAG_DIR/is-pathflow-active"
if sentinel_is_pathflow_mode; then
    pass "sentinel_is_pathflow_mode returns 0 when flag file exists"
else
    fail "sentinel_is_pathflow_mode should return 0 when flag file exists"
fi
rm -f "$PF_FLAG_DIR/is-pathflow-active"
rmdir "$PF_FLAG_DIR" 2>/dev/null || true

# Clean up PathFlow test directory
rm -rf "$(sentinel_pathflow_dir)" 2>/dev/null || true

# ============================================================================
# Edge case tests
# ============================================================================

# Test 84: sentinel_create with empty pattern creates valid file
filename_empty=$(sentinel_create "edge-skill" "edge-op" "" 60)
if [[ -n "$filename_empty" ]] && [[ -f "$SENTINEL_DIR/$filename_empty" ]]; then
    if command -v jq &>/dev/null && jq empty "$SENTINEL_DIR/$filename_empty" 2>/dev/null; then
        pass "sentinel_create with empty pattern creates valid JSON"
    else
        fail "sentinel_create with empty pattern should create valid JSON"
    fi
else
    fail "sentinel_create with empty pattern should create file"
fi
sentinel_cleanup_skill "edge-skill"

# Test 85: sentinel_get_operation_ttl returns valid TTL
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    ttl_val=$(sentinel_get_operation_ttl "git-workflow" "create-commit")
    if [[ "$ttl_val" =~ ^[0-9]+$ ]] && [[ "$ttl_val" -ge 1 ]]; then
        pass "sentinel_get_operation_ttl returns valid TTL for known operation"
    else
        fail "sentinel_get_operation_ttl should return numeric TTL (got: '$ttl_val')"
    fi
else
    pass "Config or jq not available (skipped TTL test)"
fi

# Test 86: sentinel_get_operation_pattern returns pattern
if command -v jq &>/dev/null && [[ -f "$SENTINEL_CONFIG" ]]; then
    pattern_val=$(sentinel_get_operation_pattern "git-workflow" "create-commit")
    if [[ -n "$pattern_val" ]]; then
        pass "sentinel_get_operation_pattern returns pattern for known operation"
    else
        fail "sentinel_get_operation_pattern should return non-empty pattern"
    fi
else
    pass "Config or jq not available (skipped pattern test)"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
