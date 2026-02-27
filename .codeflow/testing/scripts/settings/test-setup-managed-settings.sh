#!/usr/bin/env bash
# Purpose:   Test setup-managed-settings.sh script
# Location:  .codeflow/testing/scripts/settings/test-setup-managed-settings.sh
# Usage:     ./test-setup-managed-settings.sh
# Version:   1.0.0
#
# Tests the enterprise managed settings installer script.
#
# Coverage Requirements:
#   - Script existence and structure
#   - Source file validation
#   - OS detection
#   - Dry run mode
#   - Help command
#   - Error handling

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SCRIPT_PATH="$PROJECT_ROOT/.codeflow/scripts/settings/setup-managed-settings.sh"
SOURCE_JSON="$PROJECT_ROOT/.codeflow/docs/security/claude-enterprise/managed-settings.json"

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
        test_fail "Script not found at $SCRIPT_PATH"
    fi
}

test_script_has_shebang() {
    test_start "Script has bash shebang"
    if head -1 "$SCRIPT_PATH" | grep -q "#!/usr/bin/env bash"; then
        test_pass
    else
        test_fail "Shebang missing or incorrect"
    fi
}

test_script_is_executable() {
    test_start "Script is executable"
    if [[ -x "$SCRIPT_PATH" ]]; then
        test_pass
    else
        test_fail "Script is not executable"
    fi
}

test_script_has_set_euo_pipefail() {
    test_start "Script has set -euo pipefail"
    if grep -q "set -euo pipefail" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Missing set -euo pipefail"
    fi
}

# =============================================================================
# TESTS: SOURCE FILE
# =============================================================================

test_source_json_exists() {
    test_start "Source managed-settings.json exists"
    if [[ -f "$SOURCE_JSON" ]]; then
        test_pass
    else
        test_fail "Source JSON not found at $SOURCE_JSON"
    fi
}

test_source_json_valid() {
    test_start "Source managed-settings.json is valid JSON"
    if python3 -m json.tool "$SOURCE_JSON" > /dev/null 2>&1; then
        test_pass
    else
        test_fail "Invalid JSON in $SOURCE_JSON"
    fi
}

test_script_references_correct_source() {
    test_start "Script references correct source directory"
    if grep -q '.codeflow/docs/security/claude-enterprise' "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Script does not reference .codeflow/docs/security/claude-enterprise"
    fi
}

# =============================================================================
# TESTS: HELP COMMAND
# =============================================================================

test_help_command() {
    test_start "Help command works"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -qi "usage\|dry-run\|help"; then
        test_pass
    else
        test_fail "Help output missing expected content"
    fi
}

test_help_shows_dry_run_option() {
    test_start "Help shows --dry-run option"
    local output
    output=$(bash "$SCRIPT_PATH" --help 2>&1 || true)
    if echo "$output" | grep -q "\-\-dry-run"; then
        test_pass
    else
        test_fail "Help does not mention --dry-run"
    fi
}

# =============================================================================
# TESTS: SCRIPT STRUCTURE
# =============================================================================

test_script_has_detect_os() {
    test_start "Script has detect_os function"
    if grep -q "detect_os()" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "detect_os function not found"
    fi
}

test_script_has_get_settings_dir() {
    test_start "Script has get_settings_dir function"
    if grep -q "get_settings_dir()" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "get_settings_dir function not found"
    fi
}

test_script_has_main() {
    test_start "Script has main function"
    if grep -q "^main()" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "main function not found"
    fi
}

test_script_has_install_settings() {
    test_start "Script has install_settings function"
    if grep -q "install_settings()" "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "install_settings function not found"
    fi
}

test_script_supports_macos() {
    test_start "Script supports macOS install path"
    if grep -q '/Library/Application Support/ClaudeCode' "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "macOS install path not found"
    fi
}

test_script_supports_linux() {
    test_start "Script supports Linux install path"
    if grep -q '/etc/claude-code' "$SCRIPT_PATH"; then
        test_pass
    else
        test_fail "Linux install path not found"
    fi
}

# =============================================================================
# TESTS: DRY RUN MODE
# =============================================================================

test_dry_run_does_not_install() {
    test_start "Dry run does not install files"
    local output
    # Run with dry-run; should NOT require sudo or create files
    output=$(bash "$SCRIPT_PATH" --dry-run 2>&1 || true)
    if echo "$output" | grep -qi "dry run\|would"; then
        test_pass
    else
        test_fail "Dry run output does not indicate dry run mode"
    fi
}

test_dry_run_shows_config() {
    test_start "Dry run shows configuration"
    local output
    output=$(bash "$SCRIPT_PATH" --dry-run 2>&1 || true)
    if echo "$output" | grep -qi "operating system\|source file\|target file"; then
        test_pass
    else
        test_fail "Dry run does not show configuration"
    fi
}

# =============================================================================
# TESTS: ERROR HANDLING
# =============================================================================

test_unknown_option_fails() {
    test_start "Unknown option returns error"
    local exit_code=0
    bash "$SCRIPT_PATH" --bogus-option > /dev/null 2>&1 || exit_code=$?
    if [[ $exit_code -ne 0 ]]; then
        test_pass
    else
        test_fail "Unknown option did not cause non-zero exit"
    fi
}

test_unknown_option_shows_message() {
    test_start "Unknown option shows error message"
    local output
    output=$(bash "$SCRIPT_PATH" --bogus-option 2>&1 || true)
    if echo "$output" | grep -qi "unknown option"; then
        test_pass
    else
        test_fail "No error message for unknown option"
    fi
}

# =============================================================================
# TESTS: SHELLCHECK
# =============================================================================

test_shellcheck_clean() {
    test_start "Script passes shellcheck"
    if command -v shellcheck &>/dev/null; then
        local output
        output=$(shellcheck -x -s bash "$SCRIPT_PATH" 2>&1 || true)
        local sc_errors
        sc_errors=$(echo "$output" | grep -c "SC1" || true)
        if [[ "$sc_errors" -eq 0 ]]; then
            test_pass
        else
            test_fail "ShellCheck found SC1xxx errors: $output"
        fi
    else
        test_skip "shellcheck not installed"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo "===================================="
    echo "Test: setup-managed-settings.sh"
    echo "===================================="
    echo ""

    # Script existence tests
    echo "Script Existence:"
    test_script_exists
    test_script_has_shebang
    test_script_is_executable
    test_script_has_set_euo_pipefail

    # Source file tests
    echo ""
    echo "Source File:"
    test_source_json_exists
    test_source_json_valid
    test_script_references_correct_source

    # Help command tests
    echo ""
    echo "Help Command:"
    test_help_command
    test_help_shows_dry_run_option

    # Script structure tests
    echo ""
    echo "Script Structure:"
    test_script_has_detect_os
    test_script_has_get_settings_dir
    test_script_has_main
    test_script_has_install_settings
    test_script_supports_macos
    test_script_supports_linux

    # Dry run tests
    echo ""
    echo "Dry Run Mode:"
    test_dry_run_does_not_install
    test_dry_run_shows_config

    # Error handling tests
    echo ""
    echo "Error Handling:"
    test_unknown_option_fails
    test_unknown_option_shows_message

    # Shellcheck tests
    echo ""
    echo "Code Quality:"
    test_shellcheck_clean

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
