#!/usr/bin/env bash
# Purpose:   Test cf-protection-ops.sh library through main script
# Location:  .codeflow/testing/scripts/security/protection/test-cf-protection-ops.sh
# Usage:     ./test-cf-protection-ops.sh
# Version:   2.0.0
#
# Tests the ops library indirectly. Full protection tests require root.
#
# Coverage Requirements:
#   - Library file existence
#   - Function definitions
#   - Non-root operations

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

test_ops_lib_exists() {
    test_start "cf-protection-ops.sh exists"
    if [[ -f "$LIB_DIR/cf-protection-ops.sh" ]]; then
        test_pass
    else
        test_fail "File not found"
    fi
}

test_direct_execution_blocked() {
    test_start "Direct execution is blocked"
    local output
    output=$(bash "$LIB_DIR/cf-protection-ops.sh" 2>&1 || true)
    if echo "$output" | grep -qi "must be sourced"; then
        test_pass
    else
        test_fail "Library allows direct execution"
    fi
}

# =============================================================================
# TESTS: LIBRARY CONTENT
# =============================================================================

test_defines_protect_single_file() {
    test_start "Library defines protect_single_file"
    if grep -q "protect_single_file()" "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "protect_single_file not defined"
    fi
}

test_defines_protect_path() {
    test_start "Library defines protect_path"
    if grep -q "protect_path()" "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "protect_path not defined"
    fi
}

test_defines_unprotect_path() {
    test_start "Library defines unprotect_path"
    if grep -q "unprotect_path()" "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "unprotect_path not defined"
    fi
}

test_has_macos_handling() {
    test_start "Library has macOS handling"
    if grep -q "chflags" "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "macOS chflags not found"
    fi
}

test_has_linux_handling() {
    test_start "Library has Linux handling"
    if grep -q "chattr" "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Linux chattr not found"
    fi
}

# =============================================================================
# TESTS: FUNCTIONAL (via main script)
# =============================================================================

test_protect_requires_root() {
    test_start "Protect requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$MAIN_SCRIPT" protect all 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo\|privilege"; then
        test_pass
    else
        test_fail "Did not require root"
    fi
}

test_unprotect_requires_root() {
    test_start "Unprotect requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$MAIN_SCRIPT" unprotect all 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo\|privilege"; then
        test_pass
    else
        test_fail "Did not require root"
    fi
}

test_extend_requires_root() {
    test_start "Extend requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$MAIN_SCRIPT" extend add test.txt 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo\|privilege"; then
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
    echo "Testing: cf-protection-ops.sh"
    echo "=============================="
    echo ""

    # Library file tests
    echo "Library File:"
    test_ops_lib_exists
    test_direct_execution_blocked

    # Library content tests
    echo ""
    echo "Library Content:"
    test_defines_protect_single_file
    test_defines_protect_path
    test_defines_unprotect_path
    test_has_macos_handling
    test_has_linux_handling

    # Functional tests
    echo ""
    echo "Functional (root required):"
    test_protect_requires_root
    test_unprotect_requires_root
    test_extend_requires_root

    # Summary
    echo ""
    echo "=============================="
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
