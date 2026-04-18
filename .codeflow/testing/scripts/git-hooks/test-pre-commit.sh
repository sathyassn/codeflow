#!/usr/bin/env bash
# Test: Git pre-commit hook (hybrid wrapper: Go + shell linting)
# Location: .codeflow/testing/scripts/git-hooks/test-pre-commit.sh
#
# Tests the pre-commit hybrid wrapper structure and Go binary behavior.
# The hybrid wrapper delegates validation logic to Go, with shell linting
# for staged files (shellcheck, ruff, markdownlint).

set -euo pipefail

# Setup
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../helpers.sh
source "$TEST_DIR/../../helpers.sh"

HOOK="$REPO_ROOT/.codeflow/scripts/git-hooks/pre-commit"
GO_BIN="codeflow"

echo ""
echo "=== Testing Git Pre-Commit Hook (Hybrid Wrapper) ==="
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
assert_file_contains "$HOOK" "codeflow git-hooks" "Delegates to Go binary"
assert_file_contains "$HOOK" "git-hooks pre-commit-validate" "Uses correct Go subcommand"

# ============================================================================
# Test 2: Hybrid structure (Go + shell linting)
# ============================================================================
echo ""
echo "--- Hybrid structure ---"

assert_file_contains "$HOOK" "shellcheck" "Has shellcheck linting section"
assert_file_contains "$HOOK" "git diff --cached" "Gets staged files"

# Pre-commit uses the Go binary but does NOT exec (continues to shell linting)
if ! grep -q '^exec ' "$HOOK" 2>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Does not use exec (hybrid continues to shell linting)"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Does not use exec (hybrid continues to shell linting)"
fi

# ============================================================================
# Test 3: Shellcheck
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
# Test 4: Go binary exists and subcommand responds
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

if "$GO_BIN" git-hooks pre-commit-validate --help &>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Go subcommand responds to --help"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Go subcommand responds to --help"
fi

# ============================================================================
# Test 5: Go pre-commit-validate runs successfully
# ============================================================================
echo ""
echo "--- Go validation ---"

# pre-commit-validate should succeed on a clean repo (no staged sensitive files,
# not on protected branch -- we're on a feature branch)
EXIT_CODE=0
"$GO_BIN" git-hooks pre-commit-validate 2>/dev/null || EXIT_CODE=$?
assert_equals "0" "$EXIT_CODE" "Go validation passes on feature branch"

# ============================================================================
# Test 6: Wrapper does not contain old shell validation logic
# ============================================================================
echo ""
echo "--- No old shell validation logic ---"

# Old shell hook had branch protection, sensitive file detection, etc.
# Those are now in Go. Wrapper should only have linting.
if ! grep -q "PROTECTED_BRANCHES" "$HOOK" 2>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} No PROTECTED_BRANCHES variable in wrapper"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} No PROTECTED_BRANCHES variable in wrapper"
fi

if ! grep -q "SENSITIVE_PATTERNS" "$HOOK" 2>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} No SENSITIVE_PATTERNS in wrapper"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} No SENSITIVE_PATTERNS in wrapper"
fi

if ! grep -q "enforcement-policy" "$HOOK" 2>/dev/null; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} No enforcement-policy parsing in wrapper"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} No enforcement-policy parsing in wrapper"
fi

# ============================================================================
# Test 7: Line count check (hybrid should be under 120 lines)
# ============================================================================
echo ""
echo "--- Size check ---"

LINE_COUNT=$(wc -l < "$HOOK" | tr -d ' ')
if [[ $LINE_COUNT -le 120 ]]; then
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_PASS_COUNT++)) || true
    echo -e "  ${GREEN}✓${NC} Hybrid wrapper is under 120 lines ($LINE_COUNT lines)"
else
    ((TEST_TOTAL_COUNT++)) || true
    ((TEST_FAIL_COUNT++)) || true
    echo -e "  ${RED}✗${NC} Hybrid wrapper is under 120 lines (got $LINE_COUNT)"
fi

# ============================================================================
# Summary
# ============================================================================

print_test_summary
exit "$TEST_FAIL_COUNT"
