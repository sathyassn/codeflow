#!/usr/bin/env bash
# Purpose:   Test cf-protect-resources.sh main orchestrator script
# Location:  .codeflow/testing/scripts/security/protection/test-cf-protect-resources.sh
# Usage:     ./test-cf-protect-resources.sh
# Version:   1.1.0
#
# Tests the main protection script commands.
#
# Coverage Requirements:
#   - Script existence
#   - All commands validated
#   - Help text complete
#   - Root requirement checks

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
SCRIPT_PATH="$PROJECT_ROOT/.codeflow/scripts/security/protection/cf-protect-resources.sh"

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
# TESTS: SCRIPT EXISTENCE
# =============================================================================

test_script_exists() {
    test_start "Script file exists"
    if [[ -f "$SCRIPT_PATH" ]]; then
        test_pass
    else
        test_fail "Script not found"
    fi
}

test_script_has_shebang() {
    test_start "Script has bash shebang"
    if head -1 "$SCRIPT_PATH" | grep -q "#!/usr/bin/env bash"; then
        test_pass
    else
        test_fail "Shebang missing"
    fi
}

# =============================================================================
# TESTS: HELP COMMAND
# =============================================================================

test_help_command() {
    test_start "Help command works"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if echo "$output" | grep -qi "usage"; then
        test_pass
    else
        test_fail "Help output missing usage info"
    fi
}

test_help_shows_enable() {
    test_start "Help shows enable command"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if echo "$output" | grep -qi "enable"; then
        test_pass
    else
        test_fail "Enable not documented"
    fi
}

test_help_shows_disable() {
    test_start "Help shows disable command"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if echo "$output" | grep -qi "disable"; then
        test_pass
    else
        test_fail "Disable not documented"
    fi
}

test_help_shows_examples() {
    test_start "Help shows examples"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if echo "$output" | grep -qi "example"; then
        test_pass
    else
        test_fail "Examples missing"
    fi
}

test_h_flag() {
    test_start "-h flag works"
    local output
    output=$(bash "$SCRIPT_PATH" -h 2>&1 || true)
    if echo "$output" | grep -qi "usage"; then
        test_pass
    else
        test_fail "-h not working"
    fi
}

# =============================================================================
# TESTS: LIST COMMAND
# =============================================================================

test_list_command() {
    test_start "List command works"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if echo "$output" | grep -qi "Protected Paths"; then
        test_pass
    else
        test_fail "List output missing header"
    fi
}

test_list_shows_core() {
    test_start "List shows Core section"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if echo "$output" | grep -qi "Core"; then
        test_pass
    else
        test_fail "Core section missing"
    fi
}

test_list_includes_hooks() {
    test_start "List includes hooks path"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if echo "$output" | grep -q "hooks/codeflow"; then
        test_pass
    else
        test_fail "Hooks path missing"
    fi
}

# =============================================================================
# TESTS: STATUS COMMAND
# =============================================================================

test_status_command() {
    test_start "Status command works"
    local output
    output=$(bash "$SCRIPT_PATH" status 2>&1)
    if echo "$output" | grep -qi "Protection Status"; then
        test_pass
    else
        test_fail "Status output missing header"
    fi
}

test_status_shows_os() {
    test_start "Status shows OS info"
    local output
    output=$(bash "$SCRIPT_PATH" status 2>&1)
    if echo "$output" | grep -q "OS:"; then
        test_pass
    else
        test_fail "OS info missing"
    fi
}

# =============================================================================
# TESTS: INVALID COMMANDS
# =============================================================================

test_invalid_command() {
    test_start "Invalid command shows error"
    local output
    output=$(bash "$SCRIPT_PATH" invalidcmd 2>&1 || true)
    if echo "$output" | grep -qi "unknown\|error"; then
        test_pass
    else
        test_fail "Invalid command not caught"
    fi
}

# =============================================================================
# TESTS: ROOT REQUIREMENTS
# =============================================================================

test_enable_requires_root() {
    test_start "Enable requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" enable 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo"; then
        test_pass
    else
        test_fail "Enable did not require root"
    fi
}

test_disable_requires_root() {
    test_start "Disable requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" disable 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo"; then
        test_pass
    else
        test_fail "Disable did not require root"
    fi
}

test_add_requires_root() {
    test_start "Add requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" add test.txt 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo"; then
        test_pass
    else
        test_fail "Add did not require root"
    fi
}

test_remove_requires_root() {
    test_start "Remove requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" remove test.txt 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo"; then
        test_pass
    else
        test_fail "Remove did not require root"
    fi
}

test_verify_requires_root() {
    test_start "Verify requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" verify 2>&1 || true)
    if echo "$output" | grep -qi "root\|sudo"; then
        test_pass
    else
        test_fail "Verify did not require root"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo ""
    echo "Testing: cf-protect-resources.sh"
    echo "================================="
    echo ""

    # Script existence tests
    echo "Script Existence:"
    test_script_exists
    test_script_has_shebang

    # Help command tests
    echo ""
    echo "Help Command:"
    test_help_command
    test_help_shows_enable
    test_help_shows_disable
    test_help_shows_examples
    test_h_flag

    # List command tests
    echo ""
    echo "List Command:"
    test_list_command
    test_list_shows_core
    test_list_includes_hooks

    # Status command tests
    echo ""
    echo "Status Command:"
    test_status_command
    test_status_shows_os

    # Invalid command tests
    echo ""
    echo "Invalid Commands:"
    test_invalid_command

    # Root requirement tests
    echo ""
    echo "Root Requirements:"
    test_enable_requires_root
    test_disable_requires_root
    test_add_requires_root
    test_remove_requires_root
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
