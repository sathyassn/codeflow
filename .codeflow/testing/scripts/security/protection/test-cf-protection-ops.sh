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
# TESTS: DEPENDENCY CHECKS
# =============================================================================

test_dependency_check_without_project_root() {
    test_start "Sourcing without PROJECT_ROOT fails"
    local output
    output=$(bash -c '
        unset PROJECT_ROOT
        source "'"$LIB_DIR/cf-protection-ops.sh"'" 2>&1
    ' 2>&1 || true)
    if echo "$output" | grep -q "cf-protection-common.sh must be sourced"; then
        test_pass
    else
        test_fail "Did not detect missing PROJECT_ROOT: $output"
    fi
}

# =============================================================================
# TESTS: ROBUSTNESS
# =============================================================================

test_find_uses_print0() {
    test_start "find uses -print0 for filename safety"
    if grep -q 'find.*-print0.*while IFS=.*read.*-d' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "find does not use null-delimited output for filename safety"
    fi
}

test_header_codeflow_adapted() {
    test_start "Header references CodeFlow paths"
    if grep -q "cf-protect-resources.sh" "$LIB_DIR/cf-protection-ops.sh" && \
       grep -q "cf-protection-common.sh" "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Header not properly adapted for CodeFlow"
    fi
}

test_executable_detection_sh_extension() {
    test_start "Executable detection checks .sh extension"
    if grep -q '\*.sh' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail ".sh extension check not found"
    fi
}

test_executable_detection_x_flag() {
    test_start "Executable detection checks -x flag"
    if grep -q '\-x "$file"' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "-x flag check not found"
    fi
}

test_protect_single_file_skips_nonexistent() {
    test_start "protect_single_file skips non-file"
    # The function checks [[ ! -f "$file" ]] and returns early
    if grep -q '! -f "$file"' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Non-file guard not found"
    fi
}

test_protect_path_handles_nonexistent() {
    test_start "protect_path handles nonexistent path"
    if grep -q '! -e "$full_path"' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Nonexistent path guard not found in protect_path"
    fi
}

test_unprotect_path_handles_nonexistent() {
    test_start "unprotect_path handles nonexistent path"
    # Check that unprotect_path has its own -e check
    local count
    count=$(grep -c '! -e "$full_path"' "$LIB_DIR/cf-protection-ops.sh")
    if [[ "$count" -ge 2 ]]; then
        test_pass
    else
        test_fail "Nonexistent path guard not found in unprotect_path (count: $count)"
    fi
}

test_unprotect_logname_fallback() {
    test_start "unprotect_path has logname fallback chain"
    if grep -q 'SUDO_USER:-$(logname' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "SUDO_USER/logname fallback chain not found"
    fi
}

test_immutable_flag_removal_before_protect() {
    test_start "protect_path removes immutable flags before changes"
    # Check that protect_path removes flags first (nouchg/chattr -i before chown)
    local nouchg_line chown_line
    nouchg_line=$(grep -n "nouchg" "$LIB_DIR/cf-protection-ops.sh" | head -1 | cut -d: -f1)
    chown_line=$(grep -n 'chown -R root' "$LIB_DIR/cf-protection-ops.sh" | head -1 | cut -d: -f1)
    if [[ "$nouchg_line" -lt "$chown_line" ]]; then
        test_pass
    else
        test_fail "Immutable flag removal should precede chown"
    fi
}

test_directory_permissions_755() {
    test_start "Directories get 755 permissions"
    if grep -q "chmod 755.*full_path" "$LIB_DIR/cf-protection-ops.sh" && \
       grep -q 'find.*-type d.*chmod 755' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Directory 755 permissions not properly set"
    fi
}

test_file_permissions_644_or_755() {
    test_start "Files get 644 (non-exec) or 755 (exec) permissions"
    if grep -q 'chmod 755.*executable' "$LIB_DIR/cf-protection-ops.sh" && \
       grep -q 'chmod 644.*not executable' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "File permission logic not found"
    fi
}

test_audit_logging_protect() {
    test_start "protect_path calls log_audit"
    if grep -q 'log_audit "PROTECT"' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Audit logging not found in protect_path"
    fi
}

test_audit_logging_unprotect() {
    test_start "unprotect_path calls log_audit"
    if grep -q 'log_audit "UNPROTECT"' "$LIB_DIR/cf-protection-ops.sh"; then
        test_pass
    else
        test_fail "Audit logging not found in unprotect_path"
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

    # Dependency checks
    echo ""
    echo "Dependency Checks:"
    test_dependency_check_without_project_root

    # Robustness and quality
    echo ""
    echo "Robustness and Quality:"
    test_find_uses_print0
    test_header_codeflow_adapted
    test_executable_detection_sh_extension
    test_executable_detection_x_flag
    test_protect_single_file_skips_nonexistent
    test_protect_path_handles_nonexistent
    test_unprotect_path_handles_nonexistent
    test_unprotect_logname_fallback
    test_immutable_flag_removal_before_protect
    test_directory_permissions_755
    test_file_permissions_644_or_755
    test_audit_logging_protect
    test_audit_logging_unprotect

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
