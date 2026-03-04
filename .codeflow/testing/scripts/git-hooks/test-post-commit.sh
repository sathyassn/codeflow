#!/usr/bin/env bash
# Test: Git post-commit hook (thin wrapper -> Go binary)
# Location: .codeflow/testing/scripts/git-hooks/test-post-commit.sh
#
# Tests the post-commit thin wrapper and Go binary behavior.

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../lib/test-helpers.sh
source "$TEST_DIR/../../lib/test-helpers.sh"

HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/post-commit"
GO_BIN="codeflow"

echo ""
echo "=== Testing Git Post-Commit Hook (Thin Wrapper) ==="
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
assert_file_contains "$HOOK" "git-hooks post-commit" "Uses correct subcommand"
assert_file_contains "$HOOK" 'exec ' "Uses exec for delegation"

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

if "$GO_BIN" git-hooks post-commit --help &>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Go subcommand responds to --help"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Go subcommand responds to --help"
fi

# ============================================================================
# Test 4: Non-blocking behavior (post-commit should never fail the commit)
# ============================================================================
echo ""
echo "--- Non-blocking behavior ---"

# Post-commit runs AFTER the commit is recorded; it should always exit 0
# even outside a git repo context. The Go binary should handle gracefully.
EXIT_CODE=0
"$GO_BIN" git-hooks post-commit 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "Post-commit succeeds (non-blocking)"

# ============================================================================
# Test 5: Wrapper does not contain old shell logic
# ============================================================================
echo ""
echo "--- No old shell logic ---"

# Old shell hook had JSONL logging, commit stats, etc.
if ! grep -q "JSONL" "$HOOK" 2>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} No JSONL logic in wrapper"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} No JSONL logic in wrapper"
fi

if ! grep -q "git log" "$HOOK" 2>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} No git log commands in wrapper"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} No git log commands in wrapper"
fi

# ============================================================================
# Summary
# ============================================================================

print_test_summary
exit "$TEST_FAIL_COUNT"
