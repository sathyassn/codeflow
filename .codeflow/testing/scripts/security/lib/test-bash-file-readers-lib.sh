#!/usr/bin/env bash
# Test: bash-file-readers-lib.sh
# Location: .codeflow/testing/scripts/security/lib/test-bash-file-readers-lib.sh
#
# Tests the bash file readers library for detecting file-reading commands

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../../.." && pwd)"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$LIB_DIR/bash-file-readers-lib.sh"

export REPO_ROOT LIB_DIR

# Source the module to test functions
# shellcheck source=/dev/null
source "$MODULE"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

echo "=== Testing bash-file-readers-lib.sh ==="
echo ""

# Test 1: File exists
if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found"; fi

# Test 2: File is executable
if [[ -x "$MODULE" ]]; then pass "Module is executable"; else fail "Module not executable"; fi

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

# Test 5: Has guard against multiple sourcing
if grep -q '_BASH_FILE_READERS_LIB_LOADED' "$MODULE"; then
    pass "Has multiple-source guard"
else
    fail "Should have multiple-source guard"
fi

# Test 6: Has version constant
if grep -q 'BASH_FILE_READERS_LIB_VERSION' "$MODULE"; then
    pass "Has version constant"
else
    fail "Should have version constant"
fi

# ============================================================================
# Function existence tests
# ============================================================================

# Test 7: Has is_file_reading_command function
if declare -f is_file_reading_command &>/dev/null; then
    pass "Has is_file_reading_command function"
else
    fail "Missing is_file_reading_command function"
fi

# Test 8: Has detect_indirect_read function
if declare -f detect_indirect_read &>/dev/null; then
    pass "Has detect_indirect_read function"
else
    fail "Missing detect_indirect_read function"
fi

# Test 9: Has extract_target_file function
if declare -f extract_target_file &>/dev/null; then
    pass "Has extract_target_file function"
else
    fail "Missing extract_target_file function"
fi

# Test 10: Has get_detection_category function
if declare -f get_detection_category &>/dev/null; then
    pass "Has get_detection_category function"
else
    fail "Missing get_detection_category function"
fi

# Test 11: Has is_cat_family function
if declare -f is_cat_family &>/dev/null; then
    pass "Has is_cat_family function"
else
    fail "Missing is_cat_family function"
fi

# Test 12: Has is_text_processor function
if declare -f is_text_processor &>/dev/null; then
    pass "Has is_text_processor function"
else
    fail "Missing is_text_processor function"
fi

# Test 13: Has is_search_tool function
if declare -f is_search_tool &>/dev/null; then
    pass "Has is_search_tool function"
else
    fail "Missing is_search_tool function"
fi

# Test 14: Has is_shell_builtin function
if declare -f is_shell_builtin &>/dev/null; then
    pass "Has is_shell_builtin function"
else
    fail "Missing is_shell_builtin function"
fi

# Test 15: Has is_diff_tool function
if declare -f is_diff_tool &>/dev/null; then
    pass "Has is_diff_tool function"
else
    fail "Missing is_diff_tool function"
fi

# Test 16: Has is_encoding_tool function
if declare -f is_encoding_tool &>/dev/null; then
    pass "Has is_encoding_tool function"
else
    fail "Missing is_encoding_tool function"
fi

# Test 17: Has is_archive_reader function
if declare -f is_archive_reader &>/dev/null; then
    pass "Has is_archive_reader function"
else
    fail "Missing is_archive_reader function"
fi

# Test 18: Has is_network_fetcher function
if declare -f is_network_fetcher &>/dev/null; then
    pass "Has is_network_fetcher function"
else
    fail "Missing is_network_fetcher function"
fi

# Test 19: Has is_utility_tool function
if declare -f is_utility_tool &>/dev/null; then
    pass "Has is_utility_tool function"
else
    fail "Missing is_utility_tool function"
fi

# Test 20: Has is_path_evasion function
if declare -f is_path_evasion &>/dev/null; then
    pass "Has is_path_evasion function"
else
    fail "Missing is_path_evasion function"
fi

# Test 21: Has normalize_command function
if declare -f normalize_command &>/dev/null; then
    pass "Has normalize_command function"
else
    fail "Missing normalize_command function"
fi

# ============================================================================
# Functional tests - cat family detection
# ============================================================================

# Test 22: Detects cat command
if is_cat_family "cat /etc/passwd"; then
    pass "Detects cat command"
else
    fail "Should detect cat command"
fi

# Test 23: Detects head command
if is_cat_family "head -n 10 file.txt"; then
    pass "Detects head command"
else
    fail "Should detect head command"
fi

# Test 24: Detects tail command
if is_cat_family "tail -f /var/log/syslog"; then
    pass "Detects tail command"
else
    fail "Should detect tail command"
fi

# Test 25: Does not detect echo as cat family
if ! is_cat_family "echo hello"; then
    pass "Does not detect echo as cat family"
else
    fail "Should not detect echo as cat family"
fi

# ============================================================================
# Functional tests - text processor detection
# ============================================================================

# Test 26: Detects awk command
if is_text_processor "awk '{print \$1}' file.txt"; then
    pass "Detects awk command"
else
    fail "Should detect awk command"
fi

# Test 27: Detects sed command
if is_text_processor "sed 's/foo/bar/g' file.txt"; then
    pass "Detects sed command"
else
    fail "Should detect sed command"
fi

# Test 28: Detects perl command
if is_text_processor "perl -p -e 's/foo/bar/g' file.txt"; then
    pass "Detects perl command"
else
    fail "Should detect perl command"
fi

# ============================================================================
# Functional tests - search tool detection
# ============================================================================

# Test 29: Detects grep command
if is_search_tool "grep 'pattern' file.txt"; then
    pass "Detects grep command"
else
    fail "Should detect grep command"
fi

# Test 30: Detects rg command
if is_search_tool "rg 'pattern' /path"; then
    pass "Detects rg (ripgrep) command"
else
    fail "Should detect rg command"
fi

# ============================================================================
# Functional tests - diff tool detection
# ============================================================================

# Test 31: Detects diff command
if is_diff_tool "diff file1.txt file2.txt"; then
    pass "Detects diff command"
else
    fail "Should detect diff command"
fi

# Test 32: Detects cmp command
if is_diff_tool "cmp file1 file2"; then
    pass "Detects cmp command"
else
    fail "Should detect cmp command"
fi

# ============================================================================
# Functional tests - encoding tool detection
# ============================================================================

# Test 33: Detects base64 command
if is_encoding_tool "base64 file.txt"; then
    pass "Detects base64 command"
else
    fail "Should detect base64 command"
fi

# Test 34: Detects xxd command
if is_encoding_tool "xxd file.bin"; then
    pass "Detects xxd command"
else
    fail "Should detect xxd command"
fi

# ============================================================================
# Functional tests - combined detection
# ============================================================================

# Test 35: is_file_reading_command detects cat
if is_file_reading_command "cat /etc/passwd"; then
    pass "is_file_reading_command detects cat"
else
    fail "is_file_reading_command should detect cat"
fi

# Test 36: is_file_reading_command detects grep
if is_file_reading_command "grep 'root' /etc/passwd"; then
    pass "is_file_reading_command detects grep"
else
    fail "is_file_reading_command should detect grep"
fi

# Test 37: is_file_reading_command does not detect ls
if ! is_file_reading_command "ls -la"; then
    pass "is_file_reading_command does not detect ls"
else
    fail "is_file_reading_command should not detect ls"
fi

# Test 38: detect_indirect_read is alias for is_file_reading_command
if detect_indirect_read "cat file.txt"; then
    pass "detect_indirect_read works as alias"
else
    fail "detect_indirect_read should work as alias"
fi

# ============================================================================
# Functional tests - get_detection_category
# ============================================================================

# Test 39: get_detection_category returns cat_family for cat
category=$(get_detection_category "cat file.txt" || echo "none")
if [[ "$category" == "cat_family" ]]; then
    pass "get_detection_category returns cat_family for cat"
else
    fail "get_detection_category should return cat_family (got: $category)"
fi

# Test 40: get_detection_category returns search_tool for grep
category=$(get_detection_category "grep pattern file.txt" || echo "none")
if [[ "$category" == "search_tool" ]]; then
    pass "get_detection_category returns search_tool for grep"
else
    fail "get_detection_category should return search_tool (got: $category)"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
