#!/usr/bin/env bash
# test-common.sh - Tests for shell-lib/common.sh
# Location: .codeflow/testing/scripts/shell-lib/test-common.sh
#
# Usage:
#   ./test-common.sh           Run all tests
#   ./test-common.sh -h        Show help
#   ./test-common.sh -V        Show version

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

Tests for shell-lib/common.sh module.

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
# TEST: Path Utilities
# ============================================================================

test_find_repo_root() {
    test_section "find_repo_root"

    local result
    result=$(find_repo_root)

    assert_not_empty "$result" "find_repo_root returns non-empty"
    assert_dir_exists "$result" "find_repo_root returns valid directory"
    assert_dir_exists "$result/.codeflow" "Repo root contains .codeflow"
}

test_get_absolute_path() {
    test_section "get_absolute_path"

    setup_test_dir "abs-path"

    # Test with directory
    local abs_dir
    abs_dir=$(get_absolute_path "$TEST_DIR")
    assert_matches "$abs_dir" "^/" "Absolute path starts with /"
    assert_not_contains "$abs_dir" ".." "Absolute path has no .."

    # Test with file
    create_test_file "test.txt" "content"
    local abs_file
    abs_file=$(get_absolute_path "$TEST_DIR/test.txt")
    assert_file_exists "$abs_file" "Absolute file path is valid"

    # Test with non-existent path (should return as-is)
    local non_existent
    non_existent=$(get_absolute_path "/nonexistent/path")
    assert_equals "/nonexistent/path" "$non_existent" "Non-existent path returned as-is"

    teardown_test_dir
}

test_resolve_repo_path() {
    test_section "resolve_repo_path"

    local result
    result=$(resolve_repo_path ".codeflow/scripts")

    assert_contains "$result" ".codeflow/scripts" "resolve_repo_path includes relative path"
    assert_matches "$result" "^/" "Resolved path is absolute"
}

test_is_within_repo() {
    test_section "is_within_repo"

    setup_test_dir "within-repo"

    # Create file within repo
    local repo_root
    repo_root=$(find_repo_root)

    # Test repo path (should be within)
    if is_within_repo "$repo_root/.codeflow"; then
        test_pass "Repo path is within repo"
    else
        test_fail "Repo path is within repo"
    fi

    # Test temp dir (should not be within)
    if is_within_repo "$TEST_DIR"; then
        test_fail "Temp path should not be within repo"
    else
        test_pass "Temp path is not within repo"
    fi

    teardown_test_dir
}

# ============================================================================
# TEST: String Utilities
# ============================================================================

test_trim() {
    test_section "trim"

    assert_equals "hello" "$(trim "  hello  ")" "Trim spaces"
    assert_equals "hello world" "$(trim "  hello world  ")" "Trim preserves middle spaces"
    assert_equals "hello" "$(trim $'\t\nhello\t\n')" "Trim tabs and newlines"
    assert_equals "" "$(trim "   ")" "Trim all whitespace returns empty"
}

test_contains() {
    test_section "contains"

    if contains "hello world" "world"; then
        test_pass "String contains substring"
    else
        test_fail "String contains substring"
    fi

    if contains "hello" "xyz"; then
        test_fail "String should not contain substring"
    else
        test_pass "String does not contain absent substring"
    fi

    if contains "" "test"; then
        test_fail "Empty string should not contain anything"
    else
        test_pass "Empty string contains nothing"
    fi
}

test_starts_with() {
    test_section "starts_with"

    if starts_with "feat/my-feature" "feat/"; then
        test_pass "String starts with prefix"
    else
        test_fail "String starts with prefix"
    fi

    if starts_with "fix/bug" "feat/"; then
        test_fail "String should not start with wrong prefix"
    else
        test_pass "String does not start with wrong prefix"
    fi
}

test_ends_with() {
    test_section "ends_with"

    if ends_with "file.txt" ".txt"; then
        test_pass "String ends with suffix"
    else
        test_fail "String ends with suffix"
    fi

    if ends_with "file.txt" ".json"; then
        test_fail "String should not end with wrong suffix"
    else
        test_pass "String does not end with wrong suffix"
    fi
}

test_to_lower() {
    test_section "to_lower"

    assert_equals "hello" "$(to_lower "HELLO")" "Convert uppercase to lowercase"
    assert_equals "hello world" "$(to_lower "Hello World")" "Convert mixed case"
    assert_equals "hello123" "$(to_lower "HELLO123")" "Numbers preserved"
}

test_to_upper() {
    test_section "to_upper"

    assert_equals "HELLO" "$(to_upper "hello")" "Convert lowercase to uppercase"
    assert_equals "HELLO WORLD" "$(to_upper "Hello World")" "Convert mixed case"
    assert_equals "HELLO123" "$(to_upper "hello123")" "Numbers preserved"
}

# ============================================================================
# TEST: File Utilities
# ============================================================================

test_ensure_dir() {
    test_section "ensure_dir"

    setup_test_dir "ensure-dir"

    local new_dir="$TEST_DIR/new/nested/dir"
    ensure_dir "$new_dir"
    assert_dir_exists "$new_dir" "ensure_dir creates nested directories"

    # Should not error on existing dir
    ensure_dir "$new_dir"
    assert_dir_exists "$new_dir" "ensure_dir handles existing dir"

    teardown_test_dir
}

test_safe_read() {
    test_section "safe_read"

    setup_test_dir "safe-read"

    # Test reading existing file
    create_test_file "exists.txt" "test content"
    local content
    content=$(safe_read "$TEST_DIR/exists.txt")
    assert_contains "$content" "test content" "safe_read returns file content"

    # Test reading non-existent file
    local empty
    empty=$(safe_read "$TEST_DIR/nonexistent.txt")
    assert_empty "$empty" "safe_read returns empty for missing file"

    teardown_test_dir
}

test_atomic_write() {
    test_section "atomic_write"

    setup_test_dir "atomic-write"

    local file="$TEST_DIR/atomic.txt"
    atomic_write "$file" "atomic content"

    assert_file_exists "$file" "atomic_write creates file"
    assert_file_contains "$file" "atomic content" "atomic_write writes content"

    teardown_test_dir
}

test_backup_file() {
    test_section "backup_file"

    setup_test_dir "backup"

    create_test_file "original.txt" "original content"
    backup_file "$TEST_DIR/original.txt"

    # Check backup was created
    local backup_count
    backup_count=$(ls -1 "$TEST_DIR/original.txt.backup."* 2>/dev/null | wc -l | tr -d ' ')
    assert_equals "1" "$backup_count" "backup_file creates backup"

    teardown_test_dir
}

# ============================================================================
# TEST: Environment Utilities
# ============================================================================

test_get_env() {
    test_section "get_env"

    # Test existing variable
    export TEST_VAR="test_value"
    local result
    result=$(get_env "TEST_VAR" "default")
    assert_equals "test_value" "$result" "get_env returns existing value"
    unset TEST_VAR

    # Test missing variable with default
    result=$(get_env "NONEXISTENT_VAR" "default_value")
    assert_equals "default_value" "$result" "get_env returns default for missing"
}

test_command_exists() {
    test_section "command_exists"

    if command_exists "bash"; then
        test_pass "bash command exists"
    else
        test_fail "bash command should exist"
    fi

    if command_exists "nonexistent_cmd_xyz"; then
        test_fail "nonexistent command should not exist"
    else
        test_pass "Nonexistent command does not exist"
    fi
}

# ============================================================================
# TEST: Array Utilities
# ============================================================================

test_array_contains() {
    test_section "array_contains"

    local arr=("one" "two" "three")

    if array_contains "two" "${arr[@]}"; then
        test_pass "Array contains element"
    else
        test_fail "Array should contain element"
    fi

    if array_contains "four" "${arr[@]}"; then
        test_fail "Array should not contain missing element"
    else
        test_pass "Array does not contain missing element"
    fi
}

test_array_join() {
    test_section "array_join"

    local arr=("a" "b" "c")
    local result
    result=$(array_join "," "${arr[@]}")
    assert_equals "a,b,c" "$result" "array_join with comma"

    result=$(array_join " - " "${arr[@]}")
    assert_equals "a - b - c" "$result" "array_join with multi-char delimiter"

    local single=("only")
    result=$(array_join "," "${single[@]}")
    assert_equals "only" "$result" "array_join single element"
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
    echo -e "${BOLD}Testing: shell-lib/common.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Path utilities
    test_find_repo_root
    test_get_absolute_path
    test_resolve_repo_path
    test_is_within_repo

    # String utilities
    test_trim
    test_contains
    test_starts_with
    test_ends_with
    test_to_lower
    test_to_upper

    # File utilities
    test_ensure_dir
    test_safe_read
    test_atomic_write
    test_backup_file

    # Environment utilities
    test_get_env
    test_command_exists

    # Array utilities
    test_array_contains
    test_array_join

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
