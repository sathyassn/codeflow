#!/usr/bin/env bash
# test-validation.sh - Tests for shell-lib/validation.sh
# Location: .codeflow/testing/scripts/shell-lib/test-validation.sh
#
# Usage:
#   ./test-validation.sh       Run all tests
#   ./test-validation.sh -h    Show help
#   ./test-validation.sh -V    Show version

set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
SCRIPT_VERSION="1.0.0"
readonly SCRIPT_VERSION
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for shell-lib/validation.sh module.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information

Examples:
    $SCRIPT_NAME              Run all tests
    $SCRIPT_NAME --help       Show this help
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# ============================================================================
# TEST: String Validation
# ============================================================================

test_is_empty() {
    test_section "is_empty"

    if is_empty ""; then
        test_pass "Empty string is empty"
    else
        test_fail "Empty string should be empty"
    fi

    if is_empty "not empty"; then
        test_fail "Non-empty string should not be empty"
    else
        test_pass "Non-empty string is not empty"
    fi
}

test_is_not_empty() {
    test_section "is_not_empty"

    if is_not_empty "content"; then
        test_pass "Non-empty string is not empty"
    else
        test_fail "Non-empty string should not be empty"
    fi

    if is_not_empty ""; then
        test_fail "Empty string should be empty"
    else
        test_pass "Empty string is empty"
    fi
}

test_matches_regex() {
    test_section "matches_regex"

    if matches_regex "hello123" "^[a-z]+[0-9]+$"; then
        test_pass "String matches regex"
    else
        test_fail "String should match regex"
    fi

    if matches_regex "123hello" "^[a-z]+[0-9]+$"; then
        test_fail "String should not match regex"
    else
        test_pass "String does not match incorrect regex"
    fi
}

test_is_alphanumeric() {
    test_section "is_alphanumeric"

    if is_alphanumeric "abc123"; then
        test_pass "Alphanumeric string is valid"
    else
        test_fail "Alphanumeric string should be valid"
    fi

    if is_alphanumeric "abc-123"; then
        test_fail "String with hyphen should not be alphanumeric"
    else
        test_pass "String with hyphen is not alphanumeric"
    fi

    if is_alphanumeric "abc 123"; then
        test_fail "String with space should not be alphanumeric"
    else
        test_pass "String with space is not alphanumeric"
    fi
}

test_is_numeric() {
    test_section "is_numeric"

    if is_numeric "12345"; then
        test_pass "Numeric string is valid"
    else
        test_fail "Numeric string should be valid"
    fi

    if is_numeric "123abc"; then
        test_fail "Alphanumeric should not be numeric"
    else
        test_pass "Alphanumeric string is not numeric"
    fi

    if is_numeric "-123"; then
        test_fail "Negative number should not match (unsigned only)"
    else
        test_pass "Negative number not matched as numeric"
    fi
}

# ============================================================================
# TEST: Path Validation
# ============================================================================

test_is_safe_path() {
    test_section "is_safe_path"

    # Valid paths - use subshell to avoid set -e issues
    local result

    result=$(is_safe_path "src/main.js" && echo "safe" || echo "unsafe")
    if [[ "$result" == "safe" ]]; then
        test_pass "Normal relative path is safe"
    else
        test_fail "Normal relative path should be safe"
    fi

    result=$(is_safe_path "file.txt" && echo "safe" || echo "unsafe")
    if [[ "$result" == "safe" ]]; then
        test_pass "Simple filename is safe"
    else
        test_fail "Simple filename should be safe"
    fi

    # Invalid paths - path traversal
    result=$(is_safe_path "../etc/passwd" && echo "safe" || echo "unsafe")
    if [[ "$result" == "unsafe" ]]; then
        test_pass "Path traversal detected as unsafe"
    else
        test_fail "Path traversal should be unsafe"
    fi

    result=$(is_safe_path "src/../../../etc/passwd" && echo "safe" || echo "unsafe")
    if [[ "$result" == "unsafe" ]]; then
        test_pass "Embedded traversal detected as unsafe"
    else
        test_fail "Embedded traversal should be unsafe"
    fi

    # Invalid paths - absolute
    result=$(is_safe_path "/etc/passwd" && echo "safe" || echo "unsafe")
    if [[ "$result" == "unsafe" ]]; then
        test_pass "Absolute path detected as unsafe"
    else
        test_fail "Absolute path should be unsafe"
    fi
}

test_validate_file_path() {
    test_section "validate_file_path"

    # Valid path should not die (run in subshell to isolate exit)
    local output
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_file_path "src/main.js" 2>&1 && echo "OK"' 2>&1 || true)
    if [[ "$output" == *"OK"* ]]; then
        test_pass "Valid path passes validate_file_path"
    else
        test_fail "Valid path should pass validate_file_path"
    fi

    # Path traversal should die (run in subshell to isolate exit)
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_file_path "../etc/passwd" 2>&1; echo "OK"' 2>&1 || true)
    if [[ "$output" != *"OK"* ]]; then
        test_pass "Path traversal causes validate_file_path to die"
    else
        test_fail "Path traversal should cause validate_file_path to die"
    fi

    # Absolute path should die (run in subshell to isolate exit)
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_file_path "/etc/passwd" 2>&1; echo "OK"' 2>&1 || true)
    if [[ "$output" != *"OK"* ]]; then
        test_pass "Absolute path causes validate_file_path to die"
    else
        test_fail "Absolute path should cause validate_file_path to die"
    fi

    # Custom error message (run in subshell to isolate exit)
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_file_path "../bad" "Custom error" 2>&1' 2>&1 || true)
    if [[ "$output" == *"Custom error"* ]]; then
        test_pass "Custom error message included in output"
    else
        test_fail "Custom error message should be included"
    fi
}

# ============================================================================
# TEST: ID Validation
# ============================================================================

test_is_valid_ulid() {
    test_section "is_valid_ulid"

    # Valid ULID (26 chars, Crockford base32)
    if is_valid_ulid "01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_pass "Valid ULID accepted"
    else
        test_fail "Valid ULID should be accepted"
    fi

    # Invalid - too short
    if is_valid_ulid "01ARZ3NDEKTSV4RRFFQ69G5FA"; then
        test_fail "Short ULID should be rejected"
    else
        test_pass "Short ULID rejected"
    fi

    # Invalid - too long
    if is_valid_ulid "01ARZ3NDEKTSV4RRFFQ69G5FAVX"; then
        test_fail "Long ULID should be rejected"
    else
        test_pass "Long ULID rejected"
    fi

    # Invalid - contains I (not in Crockford)
    if is_valid_ulid "01ARZ3NDIKTSV4RRFFQ69G5FAV"; then
        test_fail "ULID with I should be rejected"
    else
        test_pass "ULID with I rejected"
    fi

    # Invalid - contains L (not in Crockford)
    if is_valid_ulid "01ARZ3NDLKTSV4RRFFQ69G5FAV"; then
        test_fail "ULID with L should be rejected"
    else
        test_pass "ULID with L rejected"
    fi

    # Invalid - lowercase
    if is_valid_ulid "01arz3ndektsv4rrffq69g5fav"; then
        test_fail "Lowercase ULID should be rejected"
    else
        test_pass "Lowercase ULID rejected"
    fi
}

test_is_valid_epic_id() {
    test_section "is_valid_epic_id"

    # Valid epic ID (ULID primary key format)
    if is_valid_epic_id "epic-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_pass "Valid epic ID accepted"
    else
        test_fail "Valid epic ID should be accepted"
    fi

    # Invalid - old EPC- prefix
    if is_valid_epic_id "EPC-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Old EPC- format should not be valid epic ID"
    else
        test_pass "Old EPC- format rejected as epic ID"
    fi

    # Invalid - task prefix
    if is_valid_epic_id "task-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Task ID should not be valid epic ID"
    else
        test_pass "Task ID rejected as epic ID"
    fi

    # Missing prefix
    if is_valid_epic_id "01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Bare ULID should not be valid epic ID"
    else
        test_pass "Bare ULID rejected as epic ID"
    fi
}

test_is_valid_task_id() {
    test_section "is_valid_task_id"

    # Valid task ID (ULID primary key format)
    if is_valid_task_id "task-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_pass "Valid task ID accepted"
    else
        test_fail "Valid task ID should be accepted"
    fi

    # Invalid - old TSK- prefix
    if is_valid_task_id "TSK-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Old TSK- format should not be valid task ID"
    else
        test_pass "Old TSK- format rejected as task ID"
    fi

    # Invalid - epic prefix
    if is_valid_task_id "epic-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Epic ID should not be valid task ID"
    else
        test_pass "Epic ID rejected as task ID"
    fi
}

test_is_valid_epic_format_id() {
    test_section "is_valid_epic_format_id"

    # Valid format IDs
    if is_valid_epic_format_id "FRT-EPC-FEAT-AUTH-001"; then
        test_pass "Valid epic format ID accepted"
    else
        test_fail "Valid epic format ID should be accepted"
    fi

    if is_valid_epic_format_id "XCUT-EPC-HTFX-AUTH-001"; then
        test_pass "4-letter area epic format ID accepted"
    else
        test_fail "4-letter area epic format ID should be accepted"
    fi

    # Invalid - ULID primary key format
    if is_valid_epic_format_id "epic-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "ULID epic ID should not be valid format ID"
    else
        test_pass "ULID epic ID rejected as format ID"
    fi

    # Invalid - old EPC- prefix
    if is_valid_epic_format_id "EPC-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Old EPC-ULID should not be valid format ID"
    else
        test_pass "Old EPC-ULID rejected as format ID"
    fi

    # Invalid - TSK entity
    if is_valid_epic_format_id "FRT-TSK-FEAT-AUTH-001"; then
        test_fail "TSK entity should not be valid epic format ID"
    else
        test_pass "TSK entity rejected as epic format ID"
    fi
}

test_is_valid_task_format_id() {
    test_section "is_valid_task_format_id"

    # Valid format IDs
    if is_valid_task_format_id "FRT-TSK-FEAT-AUTH-001"; then
        test_pass "Valid task format ID accepted"
    else
        test_fail "Valid task format ID should be accepted"
    fi

    if is_valid_task_format_id "INF-TSK-FIX-GENL-005"; then
        test_pass "INF area task format ID accepted"
    else
        test_fail "INF area task format ID should be accepted"
    fi

    # Invalid - ULID primary key format
    if is_valid_task_format_id "task-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "ULID task ID should not be valid format ID"
    else
        test_pass "ULID task ID rejected as format ID"
    fi

    # Invalid - old TSK- prefix
    if is_valid_task_format_id "TSK-01ARZ3NDEKTSV4RRFFQ69G5FAV"; then
        test_fail "Old TSK-ULID should not be valid format ID"
    else
        test_pass "Old TSK-ULID rejected as format ID"
    fi

    # Invalid - EPC entity
    if is_valid_task_format_id "FRT-EPC-FEAT-AUTH-001"; then
        test_fail "EPC entity should not be valid task format ID"
    else
        test_pass "EPC entity rejected as task format ID"
    fi
}

# ============================================================================
# TEST: Branch Validation
# ============================================================================

test_is_valid_branch_name() {
    test_section "is_valid_branch_name"

    # Valid branch names
    if is_valid_branch_name "feat/add-login"; then
        test_pass "feat/ branch is valid"
    else
        test_fail "feat/ branch should be valid"
    fi

    if is_valid_branch_name "fix/bug-123"; then
        test_pass "fix/ branch is valid"
    else
        test_fail "fix/ branch should be valid"
    fi

    if is_valid_branch_name "refactor/cleanup"; then
        test_pass "refactor/ branch is valid"
    else
        test_fail "refactor/ branch should be valid"
    fi

    if is_valid_branch_name "docs/readme"; then
        test_pass "docs/ branch is valid"
    else
        test_fail "docs/ branch should be valid"
    fi

    if is_valid_branch_name "test/unit-tests"; then
        test_pass "test/ branch is valid"
    else
        test_fail "test/ branch should be valid"
    fi

    if is_valid_branch_name "chore/deps"; then
        test_pass "chore/ branch is valid"
    else
        test_fail "chore/ branch should be valid"
    fi

    # V4 spec prefixes that were previously untested
    if is_valid_branch_name "plan/roadmap-q1"; then
        test_pass "plan/ branch is valid"
    else
        test_fail "plan/ branch should be valid"
    fi

    if is_valid_branch_name "ops/monitoring"; then
        test_pass "ops/ branch is valid"
    else
        test_fail "ops/ branch should be valid"
    fi

    if is_valid_branch_name "deploy/prod-v2"; then
        test_pass "deploy/ branch is valid"
    else
        test_fail "deploy/ branch should be valid"
    fi

    # Git-workflow SKILL.md additional prefixes
    if is_valid_branch_name "feature/auth-jwt"; then
        test_pass "feature/ branch is valid (alias for feat/)"
    else
        test_fail "feature/ branch should be valid"
    fi

    if is_valid_branch_name "bugfix/null-ptr"; then
        test_pass "bugfix/ branch is valid (alias for fix/)"
    else
        test_fail "bugfix/ branch should be valid"
    fi

    if is_valid_branch_name "hotfix/critical-fix"; then
        test_pass "hotfix/ branch is valid"
    else
        test_fail "hotfix/ branch should be valid"
    fi

    if is_valid_branch_name "ci/pipeline-update"; then
        test_pass "ci/ branch is valid"
    else
        test_fail "ci/ branch should be valid"
    fi

    if is_valid_branch_name "perf/query-optimize"; then
        test_pass "perf/ branch is valid"
    else
        test_fail "perf/ branch should be valid"
    fi

    if is_valid_branch_name "revert/bad-commit"; then
        test_pass "revert/ branch is valid"
    else
        test_fail "revert/ branch should be valid"
    fi

    if is_valid_branch_name "release/v2.0"; then
        test_pass "release/ branch is valid"
    else
        test_fail "release/ branch should be valid"
    fi

    if is_valid_branch_name "merge/main-to-dev"; then
        test_pass "merge/ branch is valid"
    else
        test_fail "merge/ branch should be valid"
    fi

    # Invalid branch names
    if is_valid_branch_name "main"; then
        test_fail "main should not be valid branch name"
    else
        test_pass "main is not valid branch name"
    fi

    if is_valid_branch_name "random-name"; then
        test_fail "Unprefixed name should not be valid"
    else
        test_pass "Unprefixed name is invalid"
    fi
}

test_get_branch_prefix() {
    test_section "get_branch_prefix"

    local prefix

    prefix=$(get_branch_prefix "feat/my-feature")
    assert_equals "feat" "$prefix" "get_branch_prefix extracts feat"

    prefix=$(get_branch_prefix "fix/bug-fix")
    assert_equals "fix" "$prefix" "get_branch_prefix extracts fix"

    prefix=$(get_branch_prefix "docs/update-readme")
    assert_equals "docs" "$prefix" "get_branch_prefix extracts docs"

    # New git-workflow prefixes
    prefix=$(get_branch_prefix "hotfix/critical")
    assert_equals "hotfix" "$prefix" "get_branch_prefix extracts hotfix"

    prefix=$(get_branch_prefix "perf/optimize")
    assert_equals "perf" "$prefix" "get_branch_prefix extracts perf"

    prefix=$(get_branch_prefix "release/v2.0")
    assert_equals "release" "$prefix" "get_branch_prefix extracts release"

    # Invalid branch returns empty
    prefix=$(get_branch_prefix "invalid-branch" || true)
    assert_equals "" "$prefix" "get_branch_prefix returns empty for invalid"
}

# ============================================================================
# TEST: JSON Validation
# ============================================================================

test_is_valid_json() {
    test_section "is_valid_json"

    # Valid JSON
    if is_valid_json '{"key": "value"}'; then
        test_pass "Valid JSON object accepted"
    else
        test_fail "Valid JSON object should be accepted"
    fi

    if is_valid_json '["a", "b", "c"]'; then
        test_pass "Valid JSON array accepted"
    else
        test_fail "Valid JSON array should be accepted"
    fi

    # Invalid JSON
    if is_valid_json '{invalid}'; then
        test_fail "Invalid JSON should be rejected"
    else
        test_pass "Invalid JSON rejected"
    fi

    if is_valid_json 'not json at all'; then
        test_fail "Plain text should not be valid JSON"
    else
        test_pass "Plain text rejected as JSON"
    fi
}

test_validate_json_file() {
    test_section "validate_json_file"

    local test_dir
    test_dir=$(setup_test_dir "json-validation")

    # Valid JSON file (subshell to isolate die/exit)
    echo '{"key": "value"}' > "$test_dir/valid.json"
    local output
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_json_file "'"$test_dir"'/valid.json" 2>&1 && echo "OK"' 2>&1 || true)
    if [[ "$output" == *"OK"* ]]; then
        test_pass "Valid JSON file passes validation"
    else
        test_fail "Valid JSON file should pass validation"
    fi

    # Invalid JSON file (subshell to isolate die/exit)
    echo '{invalid json' > "$test_dir/invalid.json"
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_json_file "'"$test_dir"'/invalid.json" 2>&1; echo "OK"' 2>&1 || true)
    if [[ "$output" != *"OK"* ]]; then
        test_pass "Invalid JSON file fails validation"
    else
        test_fail "Invalid JSON file should fail validation"
    fi

    # Non-existent file should die via assert_file_exists (subshell)
    output=$(bash -c 'source "'"$REPO_ROOT"'/.codeflow/scripts/shell-lib/index.sh"; validate_json_file "'"$test_dir"'/nonexistent.json" 2>&1; echo "OK"' 2>&1 || true)
    if [[ "$output" != *"OK"* ]]; then
        test_pass "Non-existent file fails validation"
    else
        test_fail "Non-existent file should fail validation"
    fi

    teardown_test_dir
}

# ============================================================================
# TEST: Command Validation
# ============================================================================

test_is_dangerous_command() {
    test_section "is_dangerous_command"

    # Dangerous commands
    if is_dangerous_command "rm -rf /"; then
        test_pass "rm -rf / detected as dangerous"
    else
        test_fail "rm -rf / should be dangerous"
    fi

    if is_dangerous_command "rm -rf /*"; then
        test_pass "rm -rf /* detected as dangerous"
    else
        test_fail "rm -rf /* should be dangerous"
    fi

    if is_dangerous_command "sudo rm -rf something"; then
        test_pass "sudo rm detected as dangerous"
    else
        test_fail "sudo rm should be dangerous"
    fi

    # Safe commands
    if is_dangerous_command "ls -la"; then
        test_fail "ls -la should not be dangerous"
    else
        test_pass "ls -la is safe"
    fi

    if is_dangerous_command "cat file.txt"; then
        test_fail "cat file.txt should not be dangerous"
    else
        test_pass "cat file.txt is safe"
    fi

    if is_dangerous_command "rm -rf ./temp"; then
        test_fail "rm -rf ./temp should not be dangerous (relative path)"
    else
        test_pass "rm -rf ./temp is safe (relative path)"
    fi
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            -V|--version)
                echo "$SCRIPT_NAME version $SCRIPT_VERSION"
                exit 0
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
        shift
    done

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: shell-lib/validation.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # String validation
    test_is_empty
    test_is_not_empty
    test_matches_regex
    test_is_alphanumeric
    test_is_numeric

    # Path validation
    test_is_safe_path
    test_validate_file_path

    # ID validation
    test_is_valid_ulid
    test_is_valid_epic_id
    test_is_valid_task_id
    test_is_valid_epic_format_id
    test_is_valid_task_format_id

    # Branch validation
    test_is_valid_branch_name
    test_get_branch_prefix

    # JSON validation
    test_is_valid_json
    test_validate_json_file

    # Command validation
    test_is_dangerous_command

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
