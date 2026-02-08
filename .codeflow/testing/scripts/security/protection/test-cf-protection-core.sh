#!/usr/bin/env bash
# Purpose:   Test cf-protection-core.sh library through main script
# Location:  .codeflow/testing/scripts/security/protection/test-cf-protection-core.sh
# Usage:     ./test-cf-protection-core.sh
# Version:   1.1.0
#
# Tests the core library indirectly through the main protection script.
#
# Coverage Requirements:
#   - Library file existence
#   - Core path detection
#   - List file operations

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
MAIN_SCRIPT="$PROJECT_ROOT/.codeflow/scripts/security/protection/cf-protect-resources.sh"
LIB_DIR="$PROJECT_ROOT/.codeflow/scripts/security/protection/lib"

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

# =============================================================================
# TEST HELPERS
# =============================================================================

test_start() {
    echo -n "  Testing: $1 ... "
    TESTS_RUN=$((TESTS_RUN + 1))
}

test_pass() {
    echo -e "${GREEN}PASS${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

test_fail() {
    echo -e "${RED}FAIL${NC}"
    echo "    Reason: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

# =============================================================================
# TESTS: LIBRARY FILE
# =============================================================================

test_core_lib_exists() {
    test_start "cf-protection-core.sh exists"
    if [[ -f "$LIB_DIR/cf-protection-core.sh" ]]; then
        test_pass
    else
        test_fail "File not found"
    fi
}

test_direct_execution_blocked() {
    test_start "Direct execution is blocked"
    local output
    output=$(bash "$LIB_DIR/cf-protection-core.sh" 2>&1 || true)
    if echo "$output" | grep -qi "must be sourced"; then
        test_pass
    else
        test_fail "Library allows direct execution"
    fi
}

# =============================================================================
# TESTS: LIBRARY CONTENT
# =============================================================================

test_defines_read_list_file() {
    test_start "Library defines read_list_file"
    if grep -q "read_list_file()" "$LIB_DIR/cf-protection-core.sh"; then
        test_pass
    else
        test_fail "read_list_file not defined"
    fi
}

test_defines_is_core_path() {
    test_start "Library defines is_core_path"
    if grep -q "is_core_path()" "$LIB_DIR/cf-protection-core.sh"; then
        test_pass
    else
        test_fail "is_core_path not defined"
    fi
}

test_defines_get_all_paths() {
    test_start "Library defines get_all_paths"
    if grep -q "get_all_paths()" "$LIB_DIR/cf-protection-core.sh"; then
        test_pass
    else
        test_fail "get_all_paths not defined"
    fi
}

test_defines_add_to_list() {
    test_start "Library defines add_to_list"
    if grep -q "add_to_list()" "$LIB_DIR/cf-protection-core.sh"; then
        test_pass
    else
        test_fail "add_to_list not defined"
    fi
}

test_defines_remove_from_list() {
    test_start "Library defines remove_from_list"
    if grep -q "remove_from_list()" "$LIB_DIR/cf-protection-core.sh"; then
        test_pass
    else
        test_fail "remove_from_list not defined"
    fi
}

# =============================================================================
# TESTS: FUNCTIONAL (via main script)
# =============================================================================

test_list_shows_core_section() {
    test_start "List shows Core section"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    if echo "$output" | grep -qi "Core"; then
        test_pass
    else
        test_fail "Core section not shown"
    fi
}

test_list_shows_extended_section() {
    test_start "List shows Extended section"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    if echo "$output" | grep -qi "Extended"; then
        test_pass
    else
        test_fail "Extended section not shown"
    fi
}

test_list_shows_adhoc_section() {
    test_start "List shows Ad-hoc section"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    if echo "$output" | grep -qi "Ad-hoc"; then
        test_pass
    else
        test_fail "Ad-hoc section not shown"
    fi
}

test_core_hooks_listed() {
    test_start "Core hooks path is listed"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    if echo "$output" | grep -q ".claude/hooks/codeflow"; then
        test_pass
    else
        test_fail "Hooks path not in list"
    fi
}

test_core_security_listed() {
    test_start "Core security path is listed"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    if echo "$output" | grep -q ".codeflow/scripts/security"; then
        test_pass
    else
        test_fail "Security path not in list"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo ""
    echo "Testing: cf-protection-core.sh"
    echo "==============================="
    echo ""

    # Library file tests
    echo "Library File:"
    test_core_lib_exists
    test_direct_execution_blocked

    # Library content tests
    echo ""
    echo "Library Content:"
    test_defines_read_list_file
    test_defines_is_core_path
    test_defines_get_all_paths
    test_defines_add_to_list
    test_defines_remove_from_list

    # Functional tests
    echo ""
    echo "Functional (via main script):"
    test_list_shows_core_section
    test_list_shows_extended_section
    test_list_shows_adhoc_section
    test_core_hooks_listed
    test_core_security_listed

    # Summary
    echo ""
    echo "==============================="
    echo "Results: $TESTS_PASSED/$TESTS_RUN passed"

    if [[ $TESTS_FAILED -gt 0 ]]; then
        echo -e "${RED}$TESTS_FAILED tests failed${NC}"
        exit 1
    else
        echo -e "${GREEN}All tests passed${NC}"
        exit 0
    fi
}

main "$@"
