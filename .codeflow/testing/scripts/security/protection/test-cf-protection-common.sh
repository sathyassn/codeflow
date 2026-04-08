#!/usr/bin/env bash
# Purpose:   Test cf-protection-common.sh library through main script
# Location:  .codeflow/testing/scripts/security/protection/test-cf-protection-common.sh
# Usage:     ./test-cf-protection-common.sh
# Version:   1.1.0
#
# Tests the common library indirectly through the main protection script,
# which is the canonical way the library is used.
#
# Coverage Requirements:
#   - Library file existence
#   - Configuration variables work (via script output)
#   - OS detection works
#   - Path validation works
#   - Logging functions produce output

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
# TESTS: LIBRARY FILES EXIST
# =============================================================================

test_common_lib_exists() {
    test_start "cf-protection-common.sh exists"
    if [[ -f "$LIB_DIR/cf-protection-common.sh" ]]; then
        test_pass
    else
        test_fail "File not found"
    fi
}

test_common_lib_readable() {
    test_start "cf-protection-common.sh is readable"
    if [[ -r "$LIB_DIR/cf-protection-common.sh" ]]; then
        test_pass
    else
        test_fail "File not readable"
    fi
}

test_common_lib_has_shebang() {
    test_start "Library has bash shebang"
    if head -1 "$LIB_DIR/cf-protection-common.sh" | grep -q "#!/usr/bin/env bash"; then
        test_pass
    else
        test_fail "Shebang missing or incorrect"
    fi
}

test_direct_execution_blocked() {
    test_start "Direct execution is blocked"
    local output
    output=$(bash "$LIB_DIR/cf-protection-common.sh" 2>&1 || true)
    if grep -qi "must be sourced" <<< "$output"; then
        test_pass
    else
        test_fail "Library allows direct execution (output: $output)"
    fi
}

# =============================================================================
# TESTS: LIBRARY CONTENT
# =============================================================================

test_defines_core_paths() {
    test_start "Library defines CORE_PATHS"
    if grep -q "CORE_PATHS=" "$LIB_DIR/cf-protection-common.sh"; then
        test_pass
    else
        test_fail "CORE_PATHS not defined"
    fi
}

test_defines_log_functions() {
    test_start "Library defines logging functions"
    if grep -q "log_info()" "$LIB_DIR/cf-protection-common.sh" && \
       grep -q "log_error()" "$LIB_DIR/cf-protection-common.sh"; then
        test_pass
    else
        test_fail "Logging functions not defined"
    fi
}

test_defines_validate_path() {
    test_start "Library defines validate_path"
    if grep -q "validate_path()" "$LIB_DIR/cf-protection-common.sh"; then
        test_pass
    else
        test_fail "validate_path not defined"
    fi
}

test_defines_os_detection() {
    test_start "Library has OS detection"
    if grep -q "darwin\|linux" "$LIB_DIR/cf-protection-common.sh" && \
       grep -q 'OS=' "$LIB_DIR/cf-protection-common.sh"; then
        test_pass
    else
        test_fail "OS detection not found"
    fi
}

# =============================================================================
# TESTS: FUNCTIONAL (via main script)
# =============================================================================

test_script_lists_paths() {
    test_start "Script can list protected paths"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    if grep -q ".claude/hooks/codeflow" <<< "$output"; then
        test_pass
    else
        test_fail "list command failed"
    fi
}

test_script_shows_status() {
    test_start "Script can show status"
    local output
    output=$(bash "$MAIN_SCRIPT" status 2>&1)
    if grep -qi "Protection Status" <<< "$output"; then
        test_pass
    else
        test_fail "status command failed"
    fi
}

test_os_info_in_output() {
    test_start "OS info appears in output"
    local output
    output=$(bash "$MAIN_SCRIPT" status 2>&1)
    if grep -q "OS:" <<< "$output"; then
        test_pass
    else
        test_fail "OS info not in output"
    fi
}

test_core_paths_in_list() {
    test_start "Core paths appear in list"
    local output
    output=$(bash "$MAIN_SCRIPT" list 2>&1)
    # Check for expected core paths
    if grep -q ".claude/settings.json" <<< "$output" || \
       grep -q ".codeflow/scripts/security" <<< "$output"; then
        test_pass
    else
        test_fail "Expected core paths not in list"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo ""
    echo "Testing: cf-protection-common.sh"
    echo "================================="
    echo ""

    # Library file tests
    echo "Library Files:"
    test_common_lib_exists
    test_common_lib_readable
    test_common_lib_has_shebang
    test_direct_execution_blocked

    # Library content tests
    echo ""
    echo "Library Content:"
    test_defines_core_paths
    test_defines_log_functions
    test_defines_validate_path
    test_defines_os_detection

    # Functional tests via main script
    echo ""
    echo "Functional (via main script):"
    test_script_lists_paths
    test_script_shows_status
    test_os_info_in_output
    test_core_paths_in_list

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
