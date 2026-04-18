#!/usr/bin/env bash
# Test: Git commit-msg hook (thin wrapper -> Go binary)
# Location: .codeflow/testing/scripts/git-hooks/test-commit-msg.sh
#
# Tests the commit-msg thin wrapper and Go binary behavior.

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../helpers.sh
source "$TEST_DIR/../../helpers.sh"

HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/commit-msg"
GO_BIN="codeflow"
TEMP_MSG="/tmp/claude/test-commit-msg-$$"

# Ensure temp directory exists
mkdir -p /tmp/claude

# Cleanup
# shellcheck disable=SC2329
cleanup() {
    rm -f "$TEMP_MSG"
}
trap cleanup EXIT

# Helper: test a commit message through the Go binary
test_message() {
    local msg="$1"
    local description="$2"
    local expect_pass="$3"

    echo "$msg" > "$TEMP_MSG"
    local exit_code=0
    "$GO_BIN" git-hooks commit-msg "$TEMP_MSG" 2>/dev/null || exit_code=$?

    if [[ "$expect_pass" == "yes" ]]; then
        assert_equals "0" "$exit_code" "$description"
    else
        assert_not_equals "0" "$exit_code" "$description"
    fi
}

echo ""
echo "=== Testing Git Commit-Msg Hook (Thin Wrapper) ==="
echo ""

# ============================================================================
# Test 1: Wrapper structure
# ============================================================================
echo "--- Wrapper structure ---"

assert_file_exists "$HOOK" "Hook file exists"

if [[ -x "$HOOK" ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Hook is executable"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Hook is executable"
fi

assert_file_contains "$HOOK" "set -euo pipefail" "Has strict mode"
assert_file_contains "$HOOK" "exec codeflow git-hooks" "Delegates to Go binary via exec"
assert_file_contains "$HOOK" "git-hooks commit-msg" "Uses correct subcommand"
assert_file_contains "$HOOK" 'exec ' "Uses exec for delegation"

# ============================================================================
# Test 2: Shellcheck
# ============================================================================
echo ""
echo "--- Shellcheck ---"

if command -v shellcheck &>/dev/null; then
    if shellcheck -x -s bash "$HOOK" 2>/dev/null; then
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_PASS_COUNT++)) || true
        echo -e "  ${GREEN}✓${NC} Passes shellcheck"
    else
        ((TEST_TOTAL_COUNT++)) || true
        ((TEST_FAIL_COUNT++)) || true
        echo -e "  ${RED}✗${NC} Passes shellcheck"
    fi
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_SKIP_COUNT++)) || true
    echo -e "  ${YELLOW}-${NC} shellcheck not available (skipped)"
fi

# ============================================================================
# Test 3: Go binary exists
# ============================================================================
echo ""
echo "--- Go binary ---"

if command -v codeflow &>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Go binary on PATH"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Go binary on PATH"
fi

if "$GO_BIN" git-hooks commit-msg --help &>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Go subcommand responds to --help"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Go subcommand responds to --help"
fi

# ============================================================================
# Test 4: Valid commit messages (functional)
# ============================================================================
echo ""
echo "--- Valid messages ---"

test_message "feat: add new feature" "Accepts feat type" "yes"
test_message "fix: resolve bug" "Accepts fix type" "yes"
test_message "docs: update readme" "Accepts docs type" "yes"
test_message "refactor: clean up code" "Accepts refactor type" "yes"
test_message "test: add unit tests" "Accepts test type" "yes"
test_message "chore: update deps" "Accepts chore type" "yes"
test_message "perf: optimize query" "Accepts perf type" "yes"
test_message "ci: update workflow" "Accepts ci type" "yes"
test_message "build: update makefile" "Accepts build type" "yes"

# ============================================================================
# Test 5: Invalid commit messages (functional)
# ============================================================================
echo ""
echo "--- Invalid messages ---"

test_message "random message without type" "Rejects missing type" "no"
test_message "FEAT: uppercase type" "Rejects uppercase type" "no"
test_message "feat:" "Rejects empty description" "no"
test_message "feat:missing space" "Rejects missing space after colon" "no"
test_message "" "Rejects empty message" "no"

# ============================================================================
# Test 6: Scoped types rejected
# ============================================================================
echo ""
echo "--- Scoped types ---"

test_message "feat(api): add endpoint" "Rejects scoped type" "no"
test_message "fix(core): resolve issue" "Rejects scoped fix" "no"

# ============================================================================
# Test 7: Subject length
# ============================================================================
echo ""
echo "--- Subject length ---"

LONG_DESC=$(printf 'x%.0s' {1..100})
test_message "feat: $LONG_DESC" "Rejects overly long subject" "no"

# ============================================================================
# Test 8: Edge cases
# ============================================================================
echo ""
echo "--- Edge cases ---"

test_message "feat: a" "Accepts minimal valid message" "yes"
test_message "feat: add feature with numbers 123" "Accepts alphanumeric description" "yes"

# ============================================================================
# Summary
# ============================================================================

print_test_summary
exit "$TEST_FAIL_COUNT"
