#!/usr/bin/env bash
# Purpose:   Test cf-change-approval-mode.sh script
# Location:  .codeflow/testing/scripts/settings/test-cf-change-approval-mode.sh
# Usage:     ./test-cf-change-approval-mode.sh
# Version:   2.0.0
#
# Tests the approval mode switching script.
#
# Coverage Requirements:
#   - Script existence
#   - All modes validated
#   - Template verification
#   - Error handling
#   - Status command
#   - Protection check
#   - Behavior summary
#   - No V3 references

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_PATH="$PROJECT_ROOT/.codeflow/scripts/settings/cf-change-approval-mode.sh"
TEMPLATES_DIR="$PROJECT_ROOT/.claude/settings-templates"

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
# TESTS: TEMPLATES EXISTENCE
# =============================================================================

test_strict_template() {
    test_start "Strict template exists"
    if [[ -f "$TEMPLATES_DIR/strict.json" ]]; then
        test_pass
    else
        test_fail "strict.json not found"
    fi
}

test_standard_template() {
    test_start "Standard template exists"
    if [[ -f "$TEMPLATES_DIR/standard.json" ]]; then
        test_pass
    else
        test_fail "standard.json not found"
    fi
}

test_autonomous_template() {
    test_start "Autonomous template exists"
    if [[ -f "$TEMPLATES_DIR/autonomous.json" ]]; then
        test_pass
    else
        test_fail "autonomous.json not found"
    fi
}

test_permissive_template() {
    test_start "Permissive template exists"
    if [[ -f "$TEMPLATES_DIR/permissive.json" ]]; then
        test_pass
    else
        test_fail "permissive.json not found"
    fi
}

# =============================================================================
# TESTS: HELP COMMAND
# =============================================================================

test_help_command() {
    test_start "Help command works"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -qi "usage"; then
        test_pass
    else
        test_fail "Help missing usage info"
    fi
}

test_help_shows_modes() {
    test_start "Help shows available modes"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -qi "strict" && \
       echo "$output" | grep -qi "autonomous"; then
        test_pass
    else
        test_fail "Modes not documented"
    fi
}

test_help_shows_status_option() {
    test_start "Help shows --status option"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -q "\-\-status"; then
        test_pass
    else
        test_fail "--status not documented"
    fi
}

test_help_shows_workflow() {
    test_start "Help shows workflow steps"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -qi "workflow"; then
        test_pass
    else
        test_fail "Workflow steps missing"
    fi
}

test_help_shows_current_mode() {
    test_start "Help shows current mode"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -qi "current mode"; then
        test_pass
    else
        test_fail "Current mode not shown in help"
    fi
}

test_help_shows_security_note() {
    test_start "Help shows security note"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -qi "security.*enforced\|L0.*blocks"; then
        test_pass
    else
        test_fail "Security note missing"
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
    output=$(bash "$SCRIPT_PATH" --list 2>&1)
    if echo "$output" | grep -qi "Available.*Mode"; then
        test_pass
    else
        test_fail "List output missing header"
    fi
}

test_list_shows_all_modes() {
    test_start "List shows all four modes"
    local output
    output=$(bash "$SCRIPT_PATH" --list 2>&1)
    if echo "$output" | grep -q "strict" && \
       echo "$output" | grep -q "standard" && \
       echo "$output" | grep -q "autonomous" && \
       echo "$output" | grep -q "permissive"; then
        test_pass
    else
        test_fail "Not all modes listed"
    fi
}

test_l_flag() {
    test_start "-l flag works"
    local output
    output=$(bash "$SCRIPT_PATH" -l 2>&1)
    if echo "$output" | grep -qi "Available.*Mode"; then
        test_pass
    else
        test_fail "-l not working"
    fi
}

# =============================================================================
# TESTS: STATUS COMMAND
# =============================================================================

test_status_command() {
    test_start "Status command works"
    local output
    output=$(bash "$SCRIPT_PATH" --status 2>&1)
    if echo "$output" | grep -qi "approval modes\|project default\|effective"; then
        test_pass
    else
        test_fail "Status output missing expected content"
    fi
}

test_status_shows_project() {
    test_start "Status shows project default"
    local output
    output=$(bash "$SCRIPT_PATH" --status 2>&1)
    if echo "$output" | grep -qi "project default"; then
        test_pass
    else
        test_fail "Project default not shown"
    fi
}

test_status_shows_local() {
    test_start "Status shows local override"
    local output
    output=$(bash "$SCRIPT_PATH" --status 2>&1)
    if echo "$output" | grep -qi "local override"; then
        test_pass
    else
        test_fail "Local override not shown"
    fi
}

test_status_shows_effective() {
    test_start "Status shows effective mode"
    local output
    output=$(bash "$SCRIPT_PATH" --status 2>&1)
    if echo "$output" | grep -qi "effective mode"; then
        test_pass
    else
        test_fail "Effective mode not shown"
    fi
}

test_s_flag() {
    test_start "-s flag works"
    local output
    output=$(bash "$SCRIPT_PATH" -s 2>&1)
    if echo "$output" | grep -qi "approval modes\|project default\|effective"; then
        test_pass
    else
        test_fail "-s not working"
    fi
}

# =============================================================================
# TESTS: INPUT VALIDATION
# =============================================================================

test_no_mode_error() {
    test_start "No mode shows error"
    local output
    output=$(bash "$SCRIPT_PATH" 2>&1 || true)
    if echo "$output" | grep -qi "error\|specify"; then
        test_pass
    else
        test_fail "Missing mode not caught"
    fi
}

test_invalid_mode_error() {
    test_start "Invalid mode shows error"
    local output
    output=$(bash "$SCRIPT_PATH" invalidmode 2>&1 || true)
    if echo "$output" | grep -qi "invalid\|error"; then
        test_pass
    else
        test_fail "Invalid mode not caught"
    fi
}

# =============================================================================
# TESTS: MODE VALIDATION
# =============================================================================

test_strict_valid() {
    test_start "Strict mode is valid"
    local output
    output=$(bash "$SCRIPT_PATH" strict 2>&1 || true)
    if echo "$output" | grep -qi "invalid mode: strict"; then
        test_fail "Strict rejected as invalid"
    else
        test_pass
    fi
}

test_standard_valid() {
    test_start "Standard mode is valid"
    local output
    output=$(bash "$SCRIPT_PATH" standard 2>&1 || true)
    if echo "$output" | grep -qi "invalid mode: standard"; then
        test_fail "Standard rejected as invalid"
    else
        test_pass
    fi
}

test_autonomous_valid() {
    test_start "Autonomous mode is valid"
    local output
    output=$(bash "$SCRIPT_PATH" autonomous 2>&1 || true)
    if echo "$output" | grep -qi "invalid mode: autonomous"; then
        test_fail "Autonomous rejected as invalid"
    else
        test_pass
    fi
}

test_permissive_valid() {
    test_start "Permissive mode is valid"
    local output
    output=$(bash "$SCRIPT_PATH" permissive 2>&1 || true)
    if echo "$output" | grep -qi "invalid mode: permissive"; then
        test_fail "Permissive rejected as invalid"
    else
        test_pass
    fi
}

# =============================================================================
# TESTS: TEMPLATE VALIDATION
# =============================================================================

test_templates_valid_json() {
    test_start "All templates are valid JSON"
    if ! command -v jq &>/dev/null; then
        test_skip "jq not installed"
        return
    fi

    for mode in strict standard autonomous permissive; do
        if ! jq empty "$TEMPLATES_DIR/${mode}.json" 2>/dev/null; then
            test_fail "$mode.json is invalid JSON"
            return
        fi
    done
    test_pass
}

test_templates_have_template_id() {
    test_start "All templates have _template identifier"
    if ! command -v jq &>/dev/null; then
        test_skip "jq not installed"
        return
    fi

    for mode in strict standard autonomous permissive; do
        local template_val
        template_val=$(jq -r '._template // empty' "$TEMPLATES_DIR/${mode}.json" 2>/dev/null)
        if [[ "$template_val" != "$mode" ]]; then
            test_fail "$mode.json has wrong _template: $template_val"
            return
        fi
    done
    test_pass
}

# =============================================================================
# TESTS: PATTERN CHECKS (script content validation)
# =============================================================================

test_has_protection_check() {
    test_start "Script has protection check"
    if grep -q "check_protection" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Protection check not found"
    fi
}

test_has_behavior_summary() {
    test_start "Script has behavior summary"
    if grep -q "show_behavior_summary" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Behavior summary not found"
    fi
}

test_has_reprotect_reminder() {
    test_start "Script has re-protect reminder"
    if grep -qi "re-protect\|protect.*settings.local" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Re-protect reminder not found"
    fi
}

test_has_mode_detection_fallback() {
    test_start "Script has mode detection fallback"
    if grep -q "get_mode_from_file" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Mode detection fallback not found"
    fi
}

test_has_backup() {
    test_start "Script has backup mechanism"
    if grep -q "backup_settings" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Backup mechanism not found"
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

test_references_protect_script() {
    test_start "Script references cf-protect-resources.sh"
    if grep -q "cf-protect-resources" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "No reference to cf-protect-resources.sh"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo ""
    echo "Testing: cf-change-approval-mode.sh"
    echo "===================================="
    echo ""

    # Script existence tests
    echo "Script Existence:"
    test_script_exists
    test_script_has_shebang

    # Template existence tests
    echo ""
    echo "Templates Existence:"
    test_strict_template
    test_standard_template
    test_autonomous_template
    test_permissive_template

    # Help command tests
    echo ""
    echo "Help Command:"
    test_help_command
    test_help_shows_modes
    test_help_shows_status_option
    test_help_shows_workflow
    test_help_shows_current_mode
    test_help_shows_security_note
    test_h_flag

    # List command tests
    echo ""
    echo "List Command:"
    test_list_command
    test_list_shows_all_modes
    test_l_flag

    # Status command tests
    echo ""
    echo "Status Command:"
    test_status_command
    test_status_shows_project
    test_status_shows_local
    test_status_shows_effective
    test_s_flag

    # Input validation tests
    echo ""
    echo "Input Validation:"
    test_no_mode_error
    test_invalid_mode_error

    # Mode validation tests
    echo ""
    echo "Mode Validation:"
    test_strict_valid
    test_standard_valid
    test_autonomous_valid
    test_permissive_valid

    # Template validation tests
    echo ""
    echo "Template Validation:"
    test_templates_valid_json
    test_templates_have_template_id

    # Pattern checks
    echo ""
    echo "Pattern Checks:"
    test_has_protection_check
    test_has_behavior_summary
    test_has_reprotect_reminder
    test_has_mode_detection_fallback
    test_has_backup
    test_no_v3_references
    test_references_protect_script

    # Summary
    echo ""
    echo "===================================="
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
