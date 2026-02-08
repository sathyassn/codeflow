#!/usr/bin/env bash
# Test: cf-pre-tool-use-claim-validation.sh
# Location: .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-claim-validation.sh
#
# Tests claim validation hook for Edit/Write operations:
#   - Tool filtering (Edit|Write only)
#   - L2 enforcement (warn but allow)
#   - Conflict detection with claims from other users
#   - Exclusive vs shared mode handling
#   - Graceful handling when claims file missing

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$TEST_DIR/../../../.." && pwd)"
HOOK="$REPO_ROOT/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-claim-validation.sh"
STATE_DIR="$REPO_ROOT/.state"
CLAIMS_FILE="$STATE_DIR/active-work-claims.yaml"

export REPO_ROOT

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

pass() { echo "PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); }
fail() { echo "FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); }
skip() { echo "SKIP: $1"; TESTS_SKIPPED=$((TESTS_SKIPPED + 1)); }

# Helper: Run hook with tool name and file path
run_claim_validation() {
    local tool_name="$1"
    local file_path="$2"

    local json_input="{\"file_path\": \"$file_path\"}"

    local output exit_code
    output=$(TOOL_NAME="$tool_name" TOOL_INPUT="$json_input" bash "$HOOK" 2>&1) && exit_code=0 || exit_code=$?

    HOOK_OUTPUT="$output"
    HOOK_EXIT_CODE=$exit_code
}

# Helper: Create test claims file
create_test_claims() {
    mkdir -p "$STATE_DIR"
    cat > "$CLAIMS_FILE" << 'EOF'
claims:
  - id: claim-001
    user: other-user
    machine: other-machine
    status: active
    work_description: "Working on feature X"
    file_scope:
      mode: exclusive
      patterns:
        - "src/feature-x/**"
        - "tests/feature-x/**"
  - id: claim-002
    user: another-user
    machine: another-machine
    status: active
    work_description: "Shared docs work"
    file_scope:
      mode: shared
      patterns:
        - "docs/**"
  - id: claim-003
    user: inactive-user
    machine: inactive-machine
    status: completed
    work_description: "Completed work"
    file_scope:
      mode: exclusive
      patterns:
        - "old/**"
EOF
}

# Helper: Create claims file with current user claim
create_own_claims() {
    local current_user
    local current_machine
    current_user="$(git config user.name 2>/dev/null || echo "${USER:-unknown}")"
    current_machine="$(hostname -s 2>/dev/null || echo 'unknown')"

    mkdir -p "$STATE_DIR"
    cat > "$CLAIMS_FILE" << EOF
claims:
  - id: claim-own
    user: $current_user
    machine: $current_machine
    status: active
    work_description: "My own work"
    file_scope:
      mode: exclusive
      patterns:
        - "my-files/**"
EOF
}

# Helper: Cleanup test claims
cleanup_test_claims() {
    rm -f "$CLAIMS_FILE" 2>/dev/null || true
}

# Backup existing claims file if present
backup_claims() {
    if [[ -f "$CLAIMS_FILE" ]]; then
        mv "$CLAIMS_FILE" "${CLAIMS_FILE}.backup"
    fi
}

# Restore claims file backup
restore_claims() {
    if [[ -f "${CLAIMS_FILE}.backup" ]]; then
        mv "${CLAIMS_FILE}.backup" "$CLAIMS_FILE"
    fi
}

echo "=== Testing cf-pre-tool-use-claim-validation.sh ==="
echo ""

# =============================================================================
# BASIC SETUP TESTS
# =============================================================================

echo "--- Basic Setup ---"

# Test 1: File exists
if [[ -f "$HOOK" ]]; then pass "Hook file exists"; else fail "Hook file not found"; fi

# Test 2: File is executable
if [[ -x "$HOOK" ]]; then pass "Hook is executable"; else fail "Hook not executable"; fi

# Test 3: Shellcheck passes
if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$HOOK" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    skip "Shellcheck not available"
fi

# Test 4: Uses strict mode
if grep -q "set -euo pipefail" "$HOOK"; then
    pass "Uses strict mode"
else
    fail "Should use set -euo pipefail"
fi

# Test 5: Has proper header comments
if grep -q "Purpose:" "$HOOK" && grep -q "Exit codes:" "$HOOK"; then
    pass "Has proper header comments"
else
    fail "Missing proper header comments"
fi

echo ""
echo "--- Tool Filtering ---"

# Test 6: Exits 0 for non-Edit/Write tools
result=$(TOOL_NAME="Read" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Read tool"
else
    fail "Should exit 0 for Read tool"
fi

# Test 7: Exits 0 for Bash tool
result=$(TOOL_NAME="Bash" TOOL_INPUT='{"command":"ls"}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Bash tool"
else
    fail "Should exit 0 for Bash tool"
fi

# Test 8: Exits 0 for Grep tool
result=$(TOOL_NAME="Grep" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 for Grep tool"
else
    fail "Should exit 0 for Grep tool"
fi

# Test 9: Exits 0 when no TOOL_INPUT
result=$(TOOL_NAME="Edit" TOOL_INPUT="" bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when no TOOL_INPUT"
else
    fail "Should exit 0 when no TOOL_INPUT"
fi

# Test 10: Exits 0 when file_path is empty
result=$(TOOL_NAME="Edit" TOOL_INPUT='{}' bash "$HOOK" 2>&1; echo "EXIT:$?")
if [[ "$result" == *"EXIT:0"* ]]; then
    pass "Exits 0 when file_path is empty"
else
    fail "Should exit 0 when file_path is empty"
fi

echo ""
echo "--- L2 Enforcement (Always Exit 0) ---"

# Test 11: Edit tool always exits 0
run_claim_validation "Edit" "/some/file.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Edit tool exits 0 (L2 enforcement)"
else
    fail "Edit tool should exit 0"
fi

# Test 12: Write tool always exits 0
run_claim_validation "Write" "/some/file.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Write tool exits 0 (L2 enforcement)"
else
    fail "Write tool should exit 0"
fi

# Test 13: Has L2 enforcement comment
if grep -q "L2" "$HOOK" && grep -q "warn" "$HOOK"; then
    pass "Documents L2 enforcement level"
else
    fail "Should document L2 enforcement"
fi

# Test 14: Final exit is always 0
last_exit=$(grep "^exit" "$HOOK" | tail -1)
if [[ "$last_exit" == "exit 0" ]]; then
    pass "Final exit is always 0"
else
    fail "Final exit should be 0"
fi

echo ""
echo "--- Claims File Handling ---"

backup_claims

# Test 15: Exits 0 when claims file doesn't exist
cleanup_test_claims
run_claim_validation "Edit" "src/file.txt"
if [[ $HOOK_EXIT_CODE -eq 0 ]]; then
    pass "Exits 0 when claims file missing"
else
    fail "Should exit 0 when claims file missing"
fi

# Test 16: References claims file path
if grep -q "CLAIMS_FILE" "$HOOK" && grep -q "active-work-claims" "$HOOK"; then
    pass "References claims file"
else
    fail "Should reference claims file"
fi

echo ""
echo "--- Conflict Detection (Functional) ---"

# Test 17: Detects exclusive claim conflict
if command -v python3 &>/dev/null && python3 -c "import yaml" 2>/dev/null; then
    create_test_claims
    run_claim_validation "Edit" "src/feature-x/component.ts"
    if [[ "$HOOK_OUTPUT" == *"CONFLICT"* ]] || [[ "$HOOK_OUTPUT" == *"WARNING"* ]] || [[ "$HOOK_OUTPUT" == *"exclusive"* ]] || [[ "$HOOK_OUTPUT" == *"EXCLUSIVE"* ]]; then
        pass "Detects exclusive claim conflict"
    else
        fail "Should detect exclusive claim conflict"
    fi
else
    skip "Python/PyYAML not available for conflict test"
fi

# Test 18: Detects shared claim notice
if command -v python3 &>/dev/null && python3 -c "import yaml" 2>/dev/null; then
    run_claim_validation "Edit" "docs/readme.md"
    if [[ "$HOOK_OUTPUT" == *"NOTICE"* ]] || [[ "$HOOK_OUTPUT" == *"shared"* ]] || [[ "$HOOK_OUTPUT" == *"Shared"* ]]; then
        pass "Detects shared claim notice"
    else
        fail "Should detect shared claim notice"
    fi
else
    skip "Python/PyYAML not available for shared test"
fi

# Test 19: No warning for unclaimed files
if command -v python3 &>/dev/null && python3 -c "import yaml" 2>/dev/null; then
    run_claim_validation "Edit" "unclaimed/file.txt"
    if [[ -z "$HOOK_OUTPUT" ]] || [[ "$HOOK_OUTPUT" != *"CONFLICT"* ]]; then
        pass "No warning for unclaimed files"
    else
        fail "Should not warn for unclaimed files"
    fi
else
    skip "Python/PyYAML not available"
fi

# Test 20: Ignores inactive claims
if command -v python3 &>/dev/null && python3 -c "import yaml" 2>/dev/null; then
    run_claim_validation "Edit" "old/legacy-file.txt"
    if [[ -z "$HOOK_OUTPUT" ]] || [[ "$HOOK_OUTPUT" != *"CONFLICT"* ]]; then
        pass "Ignores inactive claims"
    else
        fail "Should ignore inactive claims"
    fi
else
    skip "Python/PyYAML not available"
fi

# Test 21: Ignores own claims
if command -v python3 &>/dev/null && python3 -c "import yaml" 2>/dev/null; then
    create_own_claims
    run_claim_validation "Edit" "my-files/component.ts"
    if [[ -z "$HOOK_OUTPUT" ]] || [[ "$HOOK_OUTPUT" != *"CONFLICT"* ]]; then
        pass "Ignores own claims"
    else
        fail "Should ignore own claims"
    fi
else
    skip "Python/PyYAML not available"
fi

cleanup_test_claims
restore_claims

echo ""
echo "--- Code Structure ---"

# Test 22: Has Matcher for Edit|Write in header
if grep -q "Matcher:" "$HOOK" && grep -q "Edit" "$HOOK" && grep -q "Write" "$HOOK"; then
    pass "Has Matcher for Edit|Write in header"
else
    fail "Should have Matcher for Edit|Write"
fi

# Test 23: Uses Python for YAML/fnmatch processing
if grep -q "python3" "$HOOK" && grep -q "fnmatch" "$HOOK"; then
    pass "Uses Python for YAML/fnmatch processing"
else
    fail "Should use Python for YAML/fnmatch"
fi

# Test 24: Uses PyYAML directly (no external yaml_utils dependency)
if grep -q "import yaml" "$HOOK" && grep -q "yaml.safe_load" "$HOOK"; then
    pass "Uses PyYAML directly"
else
    fail "Should use PyYAML directly"
fi

# Test 25: Has Draft PR scope checking
if grep -q "DRAFT_PR" "$HOOK" && grep -q "draft-pullrequest-scope-check" "$HOOK"; then
    pass "Has Draft PR scope checking"
else
    fail "Should check Draft PR scope conflicts"
fi

# Test 26: Has jq fallback using grep
if grep -q "command -v jq" "$HOOK" && grep -q "grep -o" "$HOOK"; then
    pass "Has jq fallback using grep"
else
    fail "Should have jq fallback"
fi

# Test 27: Has warning/notice output for conflicts
if grep -q "WARNING:" "$HOOK" && grep -q "NOTICE:" "$HOOK"; then
    pass "Has warning/notice output"
else
    fail "Should have warning/notice output"
fi

# Test 28: Checks current user identity
if grep -q "CURRENT_USER" "$HOOK" && grep -q "git config user.name" "$HOOK"; then
    pass "Checks current user identity"
else
    fail "Should check current user identity"
fi

# Test 29: Handles graceful degradation for missing PyYAML
if grep -q "except ImportError" "$HOOK" && grep -q "sys.exit(0)" "$HOOK"; then
    pass "Graceful degradation for missing PyYAML"
else
    fail "Should handle missing PyYAML gracefully"
fi

# Test 30: Has security logging
if grep -q "log_security_event" "$HOOK"; then
    pass "Has security event logging"
else
    fail "Should log security events"
fi

echo ""
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Skipped: $TESTS_SKIPPED"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
