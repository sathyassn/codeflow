#!/usr/bin/env bash
# Test: Git prepare-commit-msg hook (thin wrapper -> Go binary)
# Location: .codeflow/testing/scripts/git-hooks/test-prepare-commit-msg.sh
#
# Tests the prepare-commit-msg thin wrapper and Go binary behavior.

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../helpers.sh
source "$TEST_DIR/../../helpers.sh"

HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/prepare-commit-msg"
GO_BIN="codeflow"
TEMP_MSG="/tmp/claude/test-prepare-commit-msg-$$"

# Ensure temp directory exists
mkdir -p /tmp/claude

# Cleanup
# shellcheck disable=SC2329
cleanup() {
    rm -f "$TEMP_MSG"
}
trap cleanup EXIT

echo ""
echo "=== Testing Git Prepare-Commit-Msg Hook (Thin Wrapper) ==="
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
assert_file_contains "$HOOK" "git-hooks prepare-commit-msg" "Uses correct subcommand"
assert_file_contains "$HOOK" 'exec ' "Uses exec for delegation"
assert_file_contains "$HOOK" '"$@"' "Passes all args"

# Verify it's a thin wrapper (under 15 lines)
LINE_COUNT=$(wc -l < "$HOOK" | tr -d ' ')
if [[ $LINE_COUNT -le 15 ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Is a thin wrapper ($LINE_COUNT lines)"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Is a thin wrapper (got $LINE_COUNT lines, expected <=15)"
fi

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
# Test 3: Go binary exists and subcommand responds
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

if "$GO_BIN" git-hooks prepare-commit-msg --help &>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Go subcommand responds to --help"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Go subcommand responds to --help"
fi

# ============================================================================
# Test 4: Skip conditions (functional)
# ============================================================================
echo ""
echo "--- Skip conditions ---"

# source=message: should skip template (message already via -m)
echo "" > "$TEMP_MSG"
EXIT_CODE=0
"$GO_BIN" git-hooks prepare-commit-msg "$TEMP_MSG" "message" 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "source=message exits successfully"
MSG_CONTENT=$(cat "$TEMP_MSG")
if [[ -z "$MSG_CONTENT" ]] || [[ ${#MSG_CONTENT} -le 1 ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} source=message skips template generation"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} source=message skips template generation (got: '$MSG_CONTENT')"
fi

# source=merge: should skip template
echo "" > "$TEMP_MSG"
EXIT_CODE=0
"$GO_BIN" git-hooks prepare-commit-msg "$TEMP_MSG" "merge" 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "source=merge exits successfully"
MERGE_CONTENT=$(cat "$TEMP_MSG")
if [[ -z "$MERGE_CONTENT" ]] || [[ ${#MERGE_CONTENT} -le 1 ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} source=merge skips template generation"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} source=merge skips template generation (got: '$MERGE_CONTENT')"
fi

# source=commit (amend): should skip template
echo "" > "$TEMP_MSG"
EXIT_CODE=0
"$GO_BIN" git-hooks prepare-commit-msg "$TEMP_MSG" "commit" 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "source=commit exits successfully"
COMMIT_CONTENT=$(cat "$TEMP_MSG")
if [[ -z "$COMMIT_CONTENT" ]] || [[ ${#COMMIT_CONTENT} -le 1 ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} source=commit skips template generation"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} source=commit skips template generation (got: '$COMMIT_CONTENT')"
fi

# ============================================================================
# Test 5: Template generation for empty source
# ============================================================================
echo ""
echo "--- Template generation ---"

echo "" > "$TEMP_MSG"
EXIT_CODE=0
"$GO_BIN" git-hooks prepare-commit-msg "$TEMP_MSG" "" 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "Empty source exits successfully"
TEMPLATE_CONTENT=$(cat "$TEMP_MSG")
if [[ -n "$TEMPLATE_CONTENT" ]] && [[ ${#TEMPLATE_CONTENT} -gt 5 ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Generates template for empty source"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Generates template for empty source (got ${#TEMPLATE_CONTENT} chars)"
fi

# ============================================================================
# Test 6: Always succeeds (non-blocking hook)
# ============================================================================
echo ""
echo "--- Non-blocking ---"

# prepare-commit-msg should never block a commit
echo "" > "$TEMP_MSG"
EXIT_CODE=0
"$GO_BIN" git-hooks prepare-commit-msg "$TEMP_MSG" "template" 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "Always succeeds (non-blocking)"

# ============================================================================
# Summary
# ============================================================================

print_test_summary
exit "$TEST_FAIL_COUNT"
