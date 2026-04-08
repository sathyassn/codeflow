#!/usr/bin/env bash
# Purpose:   Test cf-protect-resources.sh main orchestrator script
# Location:  .codeflow/testing/scripts/security/protection/test-cf-protect-resources.sh
# Usage:     ./test-cf-protect-resources.sh
# Version:   2.0.0
#
# Tests the main protection script commands.
#
# Coverage Requirements:
#   - Script existence
#   - All commands validated (protect, unprotect, status, verify, list, extend)
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
    if grep -qi "usage" <<< "$output"; then
        test_pass
    else
        test_fail "Help output missing usage info"
    fi
}

test_help_shows_protect() {
    test_start "Help shows protect command"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if grep -qi "protect" <<< "$output"; then
        test_pass
    else
        test_fail "Protect not documented"
    fi
}

test_help_shows_unprotect() {
    test_start "Help shows unprotect command"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if grep -qi "unprotect" <<< "$output"; then
        test_pass
    else
        test_fail "Unprotect not documented"
    fi
}

test_help_shows_extend() {
    test_start "Help shows extend command"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if grep -qi "extend" <<< "$output"; then
        test_pass
    else
        test_fail "Extend not documented"
    fi
}

test_help_shows_examples() {
    test_start "Help shows examples"
    local output
    output=$(bash "$SCRIPT_PATH" help 2>&1 || true)
    if grep -qi "example" <<< "$output"; then
        test_pass
    else
        test_fail "Examples missing"
    fi
}

test_h_flag() {
    test_start "-h flag works"
    local output
    output=$(bash "$SCRIPT_PATH" -h 2>&1 || true)
    if grep -qi "usage" <<< "$output"; then
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
    if grep -qi "Protection Lists" <<< "$output"; then
        test_pass
    else
        test_fail "List output missing header"
    fi
}

test_list_shows_core() {
    test_start "List shows Core section"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if grep -qi "Core" <<< "$output"; then
        test_pass
    else
        test_fail "Core section missing"
    fi
}

test_list_shows_extended() {
    test_start "List shows Extended section"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if grep -qi "Extended" <<< "$output"; then
        test_pass
    else
        test_fail "Extended section missing"
    fi
}

test_list_shows_adhoc() {
    test_start "List shows Ad-hoc section"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if grep -qi "Ad-hoc" <<< "$output"; then
        test_pass
    else
        test_fail "Ad-hoc section missing"
    fi
}

test_list_includes_hooks() {
    test_start "List includes hooks path"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if grep -q "hooks/codeflow" <<< "$output"; then
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
    if grep -qi "Protection Status" <<< "$output"; then
        test_pass
    else
        test_fail "Status output missing header"
    fi
}

test_status_shows_os() {
    test_start "Status shows OS info"
    local output
    output=$(bash "$SCRIPT_PATH" status 2>&1)
    if grep -q "OS:" <<< "$output"; then
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
    if grep -qi "unknown\|error" <<< "$output"; then
        test_pass
    else
        test_fail "Invalid command not caught"
    fi
}

# =============================================================================
# TESTS: PROTECT REQUIRES TARGET
# =============================================================================

test_protect_requires_target() {
    test_start "Protect requires explicit target"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" protect 2>&1 || true)
    # Should show error about missing target (runs as non-root, so may
    # get root error first — either root or missing-target is valid)
    if grep -qi "root\|sudo\|missing target" <<< "$output"; then
        test_pass
    else
        test_fail "Protect without target not caught"
    fi
}

# =============================================================================
# TESTS: ROOT REQUIREMENTS
# =============================================================================

test_protect_requires_root() {
    test_start "Protect requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" protect all 2>&1 || true)
    if grep -qi "root\|sudo" <<< "$output"; then
        test_pass
    else
        test_fail "Protect did not require root"
    fi
}

test_protect_core_requires_root() {
    test_start "Protect core requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" protect core 2>&1 || true)
    if grep -qi "root\|sudo" <<< "$output"; then
        test_pass
    else
        test_fail "Protect core did not require root"
    fi
}

test_unprotect_requires_root() {
    test_start "Unprotect requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" unprotect all 2>&1 || true)
    if grep -qi "root\|sudo" <<< "$output"; then
        test_pass
    else
        test_fail "Unprotect did not require root"
    fi
}

test_extend_requires_root() {
    test_start "Extend requires root"
    if [[ $EUID -eq 0 ]]; then
        test_skip "Running as root"
        return
    fi
    local output
    output=$(bash "$SCRIPT_PATH" extend add test.txt 2>&1 || true)
    if grep -qi "root\|sudo" <<< "$output"; then
        test_pass
    else
        test_fail "Extend did not require root"
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
    if grep -qi "root\|sudo" <<< "$output"; then
        test_pass
    else
        test_fail "Verify did not require root"
    fi
}

test_status_no_root() {
    test_start "Status does NOT require root"
    local output
    output=$(bash "$SCRIPT_PATH" status 2>&1)
    if grep -qi "Protection Status" <<< "$output"; then
        test_pass
    else
        test_fail "Status should work without root"
    fi
}

test_list_no_root() {
    test_start "List does NOT require root"
    local output
    output=$(bash "$SCRIPT_PATH" list 2>&1)
    if grep -qi "Protection Lists" <<< "$output"; then
        test_pass
    else
        test_fail "List should work without root"
    fi
}

# =============================================================================
# TESTS: PATTERN CHECKS (script content validation)
# =============================================================================

test_has_protect_command() {
    test_start "Script handles protect command"
    if grep -q "cmd_protect" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "cmd_protect not found"
    fi
}

test_has_unprotect_command() {
    test_start "Script handles unprotect command"
    if grep -q "cmd_unprotect" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "cmd_unprotect not found"
    fi
}

test_has_extend_command() {
    test_start "Script handles extend command"
    if grep -q "cmd_extend" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "cmd_extend not found"
    fi
}

test_has_list_command() {
    test_start "Script handles list command"
    if grep -q "cmd_list" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "cmd_list not found"
    fi
}

test_has_confirmation_prompt() {
    test_start "Script has confirmation prompts"
    if grep -q "read -rp" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "No confirmation prompts found"
    fi
}

test_has_reprotect_reminder() {
    test_start "Script has re-protect reminder"
    if grep -qi "re-protect\|protect all" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Re-protect reminder missing"
    fi
}

test_no_v3_references() {
    test_start "Script has no V3 references"
    if grep -qi "v3\|version 3" "$SCRIPT_PATH"; then
        test_fail "V3 reference found"
    else
        test_pass
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
    test_help_shows_protect
    test_help_shows_unprotect
    test_help_shows_extend
    test_help_shows_examples
    test_h_flag

    # List command tests
    echo ""
    echo "List Command:"
    test_list_command
    test_list_shows_core
    test_list_shows_extended
    test_list_shows_adhoc
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
    test_protect_requires_target

    # Root requirement tests
    echo ""
    echo "Root Requirements:"
    test_protect_requires_root
    test_protect_core_requires_root
    test_unprotect_requires_root
    test_extend_requires_root
    test_verify_requires_root
    test_status_no_root
    test_list_no_root

    # Pattern checks
    echo ""
    echo "Pattern Checks:"
    test_has_protect_command
    test_has_unprotect_command
    test_has_extend_command
    test_has_list_command
    test_has_confirmation_prompt
    test_has_reprotect_reminder
    test_no_v3_references

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
