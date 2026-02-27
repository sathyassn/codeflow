#!/usr/bin/env bash
# Purpose:   Test managed-settings.json structure and content
# Location:  .codeflow/testing/scripts/settings/test-managed-settings-json.sh
# Usage:     ./test-managed-settings-json.sh
# Version:   1.0.0
#
# Tests the enterprise managed settings JSON file for correct structure,
# expected entries, and completeness.
#
# Coverage Requirements:
#   - Valid JSON
#   - Version field
#   - Expected top-level keys
#   - Force push deny entries (36 branch-specific)
#   - Privilege escalation deny entries
#   - Secrets access deny entries
#   - Sandbox configuration

set -euo pipefail

# =============================================================================
# TEST SETUP
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SETTINGS_FILE="$PROJECT_ROOT/.codeflow/docs/security/claude-enterprise/managed-settings.json"

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

# Count occurrences of a pattern in the JSON deny array
count_deny_pattern() {
    local pattern="$1"
    grep -c "$pattern" "$SETTINGS_FILE" || echo "0"
}

# =============================================================================
# TESTS: JSON VALIDITY
# =============================================================================

test_file_exists() {
    test_start "managed-settings.json exists"
    if [[ -f "$SETTINGS_FILE" ]]; then
        test_pass
    else
        test_fail "File not found at $SETTINGS_FILE"
    fi
}

test_valid_json() {
    test_start "File is valid JSON"
    if python3 -m json.tool "$SETTINGS_FILE" > /dev/null 2>&1; then
        test_pass
    else
        test_fail "Invalid JSON"
    fi
}

# =============================================================================
# TESTS: TOP-LEVEL STRUCTURE
# =============================================================================

test_has_version() {
    test_start "Has _version field"
    if python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); assert '_version' in d" 2>/dev/null; then
        test_pass
    else
        test_fail "_version field missing"
    fi
}

test_version_is_semver() {
    test_start "Version follows semver format"
    local version
    version=$(python3 -c "import json; print(json.load(open('$SETTINGS_FILE'))['_version'])" 2>/dev/null)
    if [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        test_pass
    else
        test_fail "Version '$version' does not match X.Y.Z format"
    fi
}

test_has_permissions() {
    test_start "Has permissions object"
    if python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); assert 'permissions' in d" 2>/dev/null; then
        test_pass
    else
        test_fail "permissions object missing"
    fi
}

test_has_deny_array() {
    test_start "Has permissions.deny array"
    if python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); assert isinstance(d['permissions']['deny'], list)" 2>/dev/null; then
        test_pass
    else
        test_fail "permissions.deny is not an array"
    fi
}

test_has_sandbox() {
    test_start "Has sandbox object"
    if python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); assert 'sandbox' in d" 2>/dev/null; then
        test_pass
    else
        test_fail "sandbox object missing"
    fi
}

test_has_architecture() {
    test_start "Has _architecture metadata"
    if python3 -c "import json; d=json.load(open('$SETTINGS_FILE')); assert '_architecture' in d" 2>/dev/null; then
        test_pass
    else
        test_fail "_architecture metadata missing"
    fi
}

# =============================================================================
# TESTS: FORCE PUSH PROTECTION
# =============================================================================

# Helper: check a specific deny entry exists in the JSON file
check_deny_entry() {
    local entry="$1"
    grep -qF "\"$entry\"" "$SETTINGS_FILE"
}

test_group1_no_remote() {
    test_start "Group 1: flag before branch, no remote (12 entries)"
    local missing=0
    local missing_list=""
    local entries=(
        "Bash(git push --force main:*)"
        "Bash(git push --force master:*)"
        "Bash(git push --force release:*)"
        "Bash(git push --force production:*)"
        "Bash(git push -f main:*)"
        "Bash(git push -f master:*)"
        "Bash(git push -f release:*)"
        "Bash(git push -f production:*)"
        "Bash(git push --force-with-lease main:*)"
        "Bash(git push --force-with-lease master:*)"
        "Bash(git push --force-with-lease release:*)"
        "Bash(git push --force-with-lease production:*)"
    )
    for entry in "${entries[@]}"; do
        if ! check_deny_entry "$entry"; then
            missing=$((missing + 1))
            missing_list="$missing_list $entry"
        fi
    done
    if [[ "$missing" -eq 0 ]]; then
        test_pass
    else
        test_fail "Missing $missing entries:$missing_list"
    fi
}

test_group2_origin_flag_first() {
    test_start "Group 2: flag before branch, origin remote (12 entries)"
    local missing=0
    local missing_list=""
    local entries=(
        "Bash(git push --force origin main:*)"
        "Bash(git push --force origin master:*)"
        "Bash(git push --force origin release:*)"
        "Bash(git push --force origin production:*)"
        "Bash(git push -f origin main:*)"
        "Bash(git push -f origin master:*)"
        "Bash(git push -f origin release:*)"
        "Bash(git push -f origin production:*)"
        "Bash(git push --force-with-lease origin main:*)"
        "Bash(git push --force-with-lease origin master:*)"
        "Bash(git push --force-with-lease origin release:*)"
        "Bash(git push --force-with-lease origin production:*)"
    )
    for entry in "${entries[@]}"; do
        if ! check_deny_entry "$entry"; then
            missing=$((missing + 1))
            missing_list="$missing_list $entry"
        fi
    done
    if [[ "$missing" -eq 0 ]]; then
        test_pass
    else
        test_fail "Missing $missing entries:$missing_list"
    fi
}

test_group3_origin_remote_first() {
    test_start "Group 3: remote before flag (12 entries)"
    local missing=0
    local missing_list=""
    local entries=(
        "Bash(git push origin --force main:*)"
        "Bash(git push origin --force master:*)"
        "Bash(git push origin --force release:*)"
        "Bash(git push origin --force production:*)"
        "Bash(git push origin -f main:*)"
        "Bash(git push origin -f master:*)"
        "Bash(git push origin -f release:*)"
        "Bash(git push origin -f production:*)"
        "Bash(git push origin --force-with-lease main:*)"
        "Bash(git push origin --force-with-lease master:*)"
        "Bash(git push origin --force-with-lease release:*)"
        "Bash(git push origin --force-with-lease production:*)"
    )
    for entry in "${entries[@]}"; do
        if ! check_deny_entry "$entry"; then
            missing=$((missing + 1))
            missing_list="$missing_list $entry"
        fi
    done
    if [[ "$missing" -eq 0 ]]; then
        test_pass
    else
        test_fail "Missing $missing entries:$missing_list"
    fi
}

test_force_flag_coverage() {
    test_start "Covers --force flag across all groups"
    local count
    count=$(count_deny_pattern 'git push.*--force ')
    # At least 12 --force entries (4 branches x 3 groups) + upstream + no-verify variants
    if [[ "$count" -ge 12 ]]; then
        test_pass
    else
        test_fail "Expected at least 12 --force entries, found $count"
    fi
}

test_f_flag_coverage() {
    test_start "Covers -f flag across all groups"
    local count
    count=$(count_deny_pattern 'git push.*-f ')
    # At least 12 -f entries (4 branches x 3 groups) + upstream + no-verify variants
    if [[ "$count" -ge 12 ]]; then
        test_pass
    else
        test_fail "Expected at least 12 -f entries, found $count"
    fi
}

test_force_with_lease_coverage() {
    test_start "Covers --force-with-lease flag across all groups"
    local count
    count=$(count_deny_pattern 'force-with-lease')
    # At least 12 --force-with-lease entries (4 branches x 3 groups) + upstream variants
    if [[ "$count" -ge 12 ]]; then
        test_pass
    else
        test_fail "Expected at least 12 --force-with-lease entries, found $count"
    fi
}

test_total_force_push_deny_count() {
    test_start "Has 36+ force push deny entries (actual Bash patterns)"
    local count=0
    while IFS= read -r line; do
        if [[ "$line" == *'Bash(git push'* ]] && [[ "$line" != *'//'* ]] && [[ "$line" == *'Bash('* ]]; then
            count=$((count + 1))
        fi
    done < "$SETTINGS_FILE"
    if [[ "$count" -ge 36 ]]; then
        test_pass
    else
        test_fail "Expected 36+ force push Bash entries, found $count"
    fi
}

# =============================================================================
# TESTS: PRIVILEGE ESCALATION
# =============================================================================

test_blocks_sudo() {
    test_start "Blocks sudo"
    if grep -q '"Bash(sudo:\*)"' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "sudo deny entry missing"
    fi
}

test_blocks_su() {
    test_start "Blocks su"
    if grep -q '"Bash(su:\*)"' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "su deny entry missing"
    fi
}

test_blocks_doas() {
    test_start "Blocks doas"
    if grep -q '"Bash(doas:\*)"' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "doas deny entry missing"
    fi
}

# =============================================================================
# TESTS: HOOK BYPASS PROTECTION
# =============================================================================

test_blocks_hooks_path() {
    test_start "Blocks git config core.hooksPath"
    if grep -q 'git config core.hooksPath' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "core.hooksPath deny entry missing"
    fi
}

test_blocks_no_verify_commit() {
    test_start "Blocks git commit --no-verify"
    if grep -q 'git commit --no-verify' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "commit --no-verify deny entry missing"
    fi
}

test_blocks_no_verify_push() {
    test_start "Blocks git push --no-verify"
    if grep -q 'git push --no-verify' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "push --no-verify deny entry missing"
    fi
}

# =============================================================================
# TESTS: SECRETS ACCESS
# =============================================================================

test_blocks_env_files() {
    test_start "Blocks .env file reads"
    if grep -qF 'Read(/**/.env)' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail ".env read deny entry missing"
    fi
}

test_blocks_key_files() {
    test_start "Blocks .key file reads"
    if grep -qF 'Read(/**/*.key)' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail ".key read deny entry missing"
    fi
}

test_blocks_pem_files() {
    test_start "Blocks .pem file reads"
    if grep -qF 'Read(/**/*.pem)' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail ".pem read deny entry missing"
    fi
}

test_blocks_credentials() {
    test_start "Blocks credentials file reads"
    if grep -q 'credentials' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "credentials read deny entry missing"
    fi
}

# =============================================================================
# TESTS: SANDBOX CONFIGURATION
# =============================================================================

test_sandbox_has_excluded_commands() {
    test_start "Sandbox has excludedCommands"
    if python3 -c "
import json
d = json.load(open('$SETTINGS_FILE'))
assert 'excludedCommands' in d['sandbox']
" 2>/dev/null; then
        test_pass
    else
        test_fail "sandbox.excludedCommands missing"
    fi
}

test_sandbox_allows_git() {
    test_start "Sandbox allows git command"
    if python3 -c "
import json
d = json.load(open('$SETTINGS_FILE'))
assert 'git' in d['sandbox']['excludedCommands']
" 2>/dev/null; then
        test_pass
    else
        test_fail "git not in sandbox.excludedCommands"
    fi
}

test_sandbox_allows_gh() {
    test_start "Sandbox allows gh command"
    if python3 -c "
import json
d = json.load(open('$SETTINGS_FILE'))
assert 'gh' in d['sandbox']['excludedCommands']
" 2>/dev/null; then
        test_pass
    else
        test_fail "gh not in sandbox.excludedCommands"
    fi
}

# =============================================================================
# TESTS: SCRIPT BYPASS PROTECTION
# =============================================================================

test_blocks_bash_c() {
    test_start "Blocks bash -c execution bypass"
    if grep -q '"Bash(bash -c:\*)"' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "bash -c deny entry missing"
    fi
}

test_blocks_eval() {
    test_start "Blocks eval execution bypass"
    if grep -q '"Bash(eval ' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "eval deny entry missing"
    fi
}

# =============================================================================
# TESTS: DELETE PROTECTION
# =============================================================================

test_blocks_settings_delete() {
    test_start "Blocks settings.json deletion"
    if grep -q 'rm.*\.claude/settings\.json' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "settings.json rm deny entry missing"
    fi
}

test_blocks_hooks_delete() {
    test_start "Blocks hooks directory deletion"
    if grep -q 'rm.*\.claude/hooks' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "hooks rm deny entry missing"
    fi
}

test_blocks_branch_delete() {
    test_start "Blocks protected branch deletion"
    if grep -q 'git push origin --delete main' "$SETTINGS_FILE"; then
        test_pass
    else
        test_fail "branch delete deny entry missing"
    fi
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    echo "===================================="
    echo "Test: managed-settings.json"
    echo "===================================="
    echo ""

    # JSON validity tests
    echo "JSON Validity:"
    test_file_exists
    test_valid_json

    # Top-level structure tests
    echo ""
    echo "Top-Level Structure:"
    test_has_version
    test_version_is_semver
    test_has_permissions
    test_has_deny_array
    test_has_sandbox
    test_has_architecture

    # Force push protection tests
    echo ""
    echo "Force Push Protection:"
    test_group1_no_remote
    test_group2_origin_flag_first
    test_group3_origin_remote_first
    test_force_flag_coverage
    test_f_flag_coverage
    test_force_with_lease_coverage
    test_total_force_push_deny_count

    # Privilege escalation tests
    echo ""
    echo "Privilege Escalation:"
    test_blocks_sudo
    test_blocks_su
    test_blocks_doas

    # Hook bypass tests
    echo ""
    echo "Hook Bypass Protection:"
    test_blocks_hooks_path
    test_blocks_no_verify_commit
    test_blocks_no_verify_push

    # Secrets access tests
    echo ""
    echo "Secrets Access:"
    test_blocks_env_files
    test_blocks_key_files
    test_blocks_pem_files
    test_blocks_credentials

    # Sandbox tests
    echo ""
    echo "Sandbox Configuration:"
    test_sandbox_has_excluded_commands
    test_sandbox_allows_git
    test_sandbox_allows_gh

    # Script bypass tests
    echo ""
    echo "Script Bypass Protection:"
    test_blocks_bash_c
    test_blocks_eval

    # Delete protection tests
    echo ""
    echo "Delete Protection:"
    test_blocks_settings_delete
    test_blocks_hooks_delete
    test_blocks_branch_delete

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
