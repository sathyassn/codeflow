#!/usr/bin/env bash
# Purpose:   Test cf-protection-verify.sh library through main script
# Location:  .codeflow/testing/scripts/security/protection/test-cf-protection-verify.sh
# Usage:     ./test-cf-protection-verify.sh
# Version:   1.1.0
#
# Tests the verify library indirectly. Full verification requires root.
#
# Coverage Requirements:
#   - Library file existence
#   - Function definitions
#   - Status output format

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
YELLOW='\033[1;33m'
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

test_skip() {
    echo -e "${YELLOW}SKIP${NC}"
    echo "    Reason: $1"
}

# =============================================================================
# TESTS: LIBRARY FILE
# =============================================================================

test_verify_lib_exists() {
    test_start "cf-protection-verify.sh exists"
    if [[ -f "$LIB_DIR/cf-protection-verify.sh" ]]; then
        test_pass
    else
        test_fail "File not found"
    fi
}

test_direct_execution_blocked() {
    test_start "Direct execution is blocked"
    local output
    output=$(bash "$LIB_DIR/cf-protection-verify.sh" 2>&1 || true)
    if grep -qi "must be sourced" <<< "$output"; then
        test_pass
    else
        test_fail "Library allows direct execution"
    fi
}

# =============================================================================
# TESTS: LIBRARY CONTENT
# =============================================================================

test_defines_check_path_status() {
    test_start "Library defines check_path_status"
    if grep -q "check_path_status()" "$LIB_DIR/cf-protection-verify.sh"; then
        test_pass
    else
        test_fail "check_path_status not defined"
    fi
}

test_defines_show_status() {
    test_start "Library defines show_status"
    if grep -q "show_status()" "$LIB_DIR/cf-protection-verify.sh"; then
        test_pass
    else
        test_fail "show_status not defined"
    fi
}

test_defines_verify_protection() {
    test_start "Library defines verify_protection"
    if grep -q "verify_protection()" "$LIB_DIR/cf-protection-verify.sh"; then
        test_pass
    else
        test_fail "verify_protection not defined"
    fi
}

test_has_status_constants() {
    test_start "Library has status constants"
    if grep -q "PROTECTED\|UNPROTECTED\|PARTIAL\|MISSING" "$LIB_DIR/cf-protection-verify.sh"; then
        test_pass
    else
        test_fail "Status constants not found"
    fi
}

# =============================================================================
# TESTS: FUNCTIONAL (via main script)
# =============================================================================

test_status_command_works() {
    test_start "Status command works"
    local output
    output=$(bash "$MAIN_SCRIPT" status 2>&1)
    if grep -qi "Protection Status" <<< "$output"; then
        test_pass
    else
        test_fail "Status command failed"
    fi
}

test_status_shows_os() {
    test_start "Status shows OS info"
    local output
    output=$(bash "$MAIN_SCRIPT" status 2>&1)
    if grep -q "OS:" <<< "$output"; then
        test_pass
    else
        test_fail "OS info not shown"
    fi
}

test_status_shows_core() {
    test_start "Status shows Core section"
    local output
    output=$(bash "$MAIN_SCRIPT" status 2>&1)
    if grep -qi "Core" <<< "$output"; then
        test_pass
    else
        test_fail "Core section not shown"
    fi
}

test_status_shows_extended() {
    test_start "Status shows Extended section"
    local output
    output=$(bash "$MAIN_SCRIPT" status 2>&1)
    if grep -qi "Extended" <<< "$output"; then
        test_pass
    else
        test_fail "Extended section not shown"
    fi
}

test_verify_requires_root() {
    test_start "Verify requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$MAIN_SCRIPT" verify 2>&1 || true)
    if grep -qi "root\|sudo\|privilege" <<< "$output"; then
        test_pass
    else
        test_fail "Did not require root"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo ""
    echo "Testing: cf-protection-verify.sh"
    echo "================================="
    echo ""

    # Library file tests
    echo "Library File:"
    test_verify_lib_exists
    test_direct_execution_blocked

    # Library content tests
    echo ""
    echo "Library Content:"
    test_defines_check_path_status
    test_defines_show_status
    test_defines_verify_protection
    test_has_status_constants

    # Functional tests
    echo ""
    echo "Functional (via main script):"
    test_status_command_works
    test_status_shows_os
    test_status_shows_core
    test_status_shows_extended
    test_verify_requires_root

    # Summary
    echo ""
    echo "================================="
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
