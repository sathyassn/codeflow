#!/usr/bin/env bash
# Test: security-lib.sh function-level tests
# Location: .codeflow/testing/scripts/security/lib/test-security-lib-functions.sh
#
# Tests individual functions from security-lib.sh by sourcing the library
# and calling functions directly. Focuses on path-targeting and glob logic.

set -euo pipefail

# shellcheck disable=SC1090  # Non-constant source is intentional (dynamic LIB_FILE)

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../../lib/test-isolation.sh"

# Use the fixed file if available, otherwise fall back to repo copy
if [[ -f "$TEST_TMPDIR/fixed-security-lib.sh" ]]; then
    LIB_FILE="$TEST_TMPDIR/fixed-security-lib.sh"
else
    LIB_FILE="$REAL_REPO_ROOT/.codeflow/scripts/security/lib/security-lib.sh"
fi

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

pass() {
    echo "  PASS: $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

fail() {
    echo "  FAIL: $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

echo "=== Testing security-lib.sh functions ==="
echo "Using: $LIB_FILE"
echo ""

# ============================================================================
# is_path_targeted tests
# ============================================================================
echo "--- is_path_targeted ---"

# Test 1: Exact path in rm command -> targeted
if (
    source "$LIB_FILE"
    is_path_targeted "rm .claude/settings.json" ".claude/settings.json"
); then
    pass "T1: .claude/settings.json in 'rm .claude/settings.json' -> targeted"
else
    fail "T1: .claude/settings.json in 'rm .claude/settings.json' should be targeted"
fi

# Test 2: Path only in /tmp/claude context -> NOT targeted
if (
    source "$LIB_FILE"
    ! is_path_targeted "cp /tmp/claude/.claude-settings.json /tmp/" ".claude"
); then
    pass "T2: .claude in 'cp /tmp/claude/.claude-settings.json /tmp/' -> not targeted (tmp/claude)"
else
    fail "T2: .claude in /tmp/claude context should not be targeted"
fi

# Test 3: Path with space boundary
if (
    source "$LIB_FILE"
    is_path_targeted "rm -rf .claude " ".claude"
); then
    pass "T3: .claude with trailing space -> targeted"
else
    fail "T3: .claude with trailing space should be targeted"
fi

# Test 4: Path with slash boundary
if (
    source "$LIB_FILE"
    is_path_targeted "rm -rf .claude/hooks" ".claude"
); then
    pass "T4: .claude/ with slash boundary -> targeted"
else
    fail "T4: .claude/ with slash boundary should be targeted"
fi

# Test 5: Path at end of command
if (
    source "$LIB_FILE"
    is_path_targeted "rm -rf .claude" ".claude"
); then
    pass "T5: .claude at end of command -> targeted"
else
    fail "T5: .claude at end of command should be targeted"
fi

# Test 6: Path in double quotes
if (
    source "$LIB_FILE"
    is_path_targeted 'rm ".claude"' ".claude"
); then
    pass "T6: .claude in double quotes -> targeted"
else
    fail "T6: .claude in double quotes should be targeted"
fi

# Test 7: Path in single quotes
if (
    source "$LIB_FILE"
    is_path_targeted "rm '.claude'" ".claude"
); then
    pass "T7: .claude in single quotes -> targeted"
else
    fail "T7: .claude in single quotes should be targeted"
fi

# Test 8: Substring should NOT match (boundary check)
if (
    source "$LIB_FILE"
    ! is_path_targeted "rm -rf .claude-notes.md" ".claude"
); then
    pass "T8: .claude-notes.md is NOT a match for .claude (substring rejection)"
else
    fail "T8: .claude-notes.md should not match .claude"
fi

# Test 9: Path not in command at all
if (
    source "$LIB_FILE"
    ! is_path_targeted "rm -rf /tmp/other" ".claude"
); then
    pass "T9: path not in command -> not targeted"
else
    fail "T9: unrelated command should not be targeted"
fi

# Test 10: Multiple /tmp/claude paths all safe
if (
    source "$LIB_FILE"
    ! is_path_targeted "cp /tmp/claude/a /tmp/claude/.claude/b" ".claude"
); then
    pass "T10: multiple /tmp/claude paths -> not targeted"
else
    fail "T10: multiple /tmp/claude paths should all be stripped"
fi

echo ""

# ============================================================================
# is_glob_path_targeted tests
# ============================================================================
echo "--- is_glob_path_targeted ---"

# Test 11: Glob pattern matches real path in command
if (
    source "$LIB_FILE"
    is_glob_path_targeted "bash .claude/hooks/codeflow/stop/hook.sh" ".claude/hooks/codeflow/**"
); then
    pass "T11: .claude/hooks/codeflow/** matches 'bash .claude/hooks/codeflow/stop/hook.sh'"
else
    fail "T11: glob .claude/hooks/codeflow/** should match the command"
fi

# Test 12: Glob pattern with /tmp/claude exclusion (THE BUG FIX)
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "cp /tmp/claude/fixed-.claude-hooks-codeflow-hook.sh /tmp/" ".claude/hooks/codeflow/**"
); then
    pass "T12: .claude/hooks/codeflow/** NOT matched when path is in /tmp/claude (BUG FIX)"
else
    fail "T12: glob should NOT match when path only appears in /tmp/claude context"
fi

# Test 13: Non-matching glob returns false
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "rm other/file.txt" ".claude/*.md"
); then
    pass "T13: .claude/*.md does not match 'rm other/file.txt'"
else
    fail "T13: non-matching glob should return false"
fi

# Test 14: Complex glob pattern matches
if (
    source "$LIB_FILE"
    is_glob_path_targeted "rm src/test/foo.spec.js" "src/test/*.spec.js"
); then
    pass "T14: src/test/*.spec.js matches src/test/foo.spec.js"
else
    fail "T14: complex glob should match"
fi

# Test 15: Glob with path in /tmp/claude mixed with real path
if (
    source "$LIB_FILE"
    is_glob_path_targeted "cp /tmp/claude/backup.sh .claude/hooks/codeflow/stop/hook.sh" ".claude/hooks/codeflow/**"
); then
    pass "T15: real path still detected even with /tmp/claude path in same command"
else
    fail "T15: real path should still be detected alongside /tmp/claude path"
fi

# Test 16: Glob with only /tmp/claude paths (multiple)
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "cp /tmp/claude/.claude/hooks/a.sh /tmp/claude/.claude/hooks/b.sh" ".claude/hooks/*"
); then
    pass "T16: multiple /tmp/claude paths all stripped -> not targeted"
else
    fail "T16: all /tmp/claude paths should be stripped"
fi

echo ""

# ============================================================================
# is_path_or_glob_targeted tests
# ============================================================================
echo "--- is_path_or_glob_targeted ---"

# Test 17: Exact path dispatches to is_path_targeted
if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm .claude/settings.json" ".claude/settings.json"
); then
    pass "T17: exact path .claude/settings.json -> targeted"
else
    fail "T17: exact path should be targeted"
fi

# Test 18: Glob pattern dispatches to is_glob_path_targeted
if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm .claude/memory/work.md" ".claude/*/*.md"
); then
    pass "T18: glob .claude/*/*.md matches .claude/memory/work.md"
else
    fail "T18: glob via unified function should match"
fi

# Test 19: ? pattern dispatches to glob path
if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm file1.txt" "file?.txt"
); then
    pass "T19: ? pattern file?.txt matches file1.txt"
else
    fail "T19: ? pattern via unified function should match"
fi

# Test 20: Unified function inherits /tmp/claude exclusion for globs
if (
    source "$LIB_FILE"
    ! is_path_or_glob_targeted "cp /tmp/claude/fixed-.claude-hooks-hook.sh /tmp/" ".claude/hooks/*"
); then
    pass "T20: unified glob check inherits /tmp/claude exclusion"
else
    fail "T20: unified function should inherit /tmp/claude exclusion for globs"
fi

# Test 21: Unified function inherits /tmp/claude exclusion for exact paths
if (
    source "$LIB_FILE"
    ! is_path_or_glob_targeted "cp /tmp/claude/.claude/settings.json /tmp/" ".claude/settings.json"
); then
    pass "T21: unified exact check inherits /tmp/claude exclusion"
else
    fail "T21: unified function should inherit /tmp/claude exclusion for exact paths"
fi

# Test 22a: /** pattern matches directory itself (not just files inside)
if (
    source "$LIB_FILE"
    is_glob_path_targeted "rm -rf .claude/hooks/codeflow" ".claude/hooks/codeflow/**"
); then
    pass "T22a: /** matches directory itself (rm -rf .claude/hooks/codeflow)"
else
    fail "T22a: /** should match the directory itself, not just files inside"
fi

# Test 22b: /** directory match works for multiple config paths
if (
    source "$LIB_FILE"
    is_glob_path_targeted "rm -rf .codeflow/config" ".codeflow/config/**"
); then
    pass "T22b: /** matches .codeflow/config directory"
else
    fail "T22b: /** should match .codeflow/config directory"
fi

# Test 22c: /** directory match with mv
if (
    source "$LIB_FILE"
    is_glob_path_targeted "mv .codeflow/scripts/security /tmp/bak" ".codeflow/scripts/security/**"
); then
    pass "T22c: /** matches directory in mv command"
else
    fail "T22c: /** should match directory in mv command"
fi

# Test 22d: /** directory match does NOT match substring
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "rm -rf .claude/hooks/codeflow-notes" ".claude/hooks/codeflow/**"
); then
    pass "T22d: /** does NOT match substring (.claude/hooks/codeflow-notes)"
else
    fail "T22d: /** should not match substring of directory name"
fi

# Test 22e: /** directory match respects /tmp/claude exclusion
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "rm -rf /tmp/claude/.claude/hooks/codeflow" ".claude/hooks/codeflow/**"
); then
    pass "T22e: /** directory match respects /tmp/claude exclusion"
else
    fail "T22e: /** directory match should respect /tmp/claude exclusion"
fi

# Test 22f: is_path_or_glob_targeted propagates /** directory fix
if (
    source "$LIB_FILE"
    is_path_or_glob_targeted "rm -rf .claude/hooks/codeflow" ".claude/hooks/codeflow/**"
); then
    pass "T22f: unified function propagates /** directory protection"
else
    fail "T22f: unified function should propagate /** directory protection"
fi

# Test 22g: /** with git rm
if (
    source "$LIB_FILE"
    is_glob_path_targeted "git rm -r .claude/hooks/codeflow" ".claude/hooks/codeflow/**"
); then
    pass "T22g: /** matches directory in git rm command"
else
    fail "T22g: /** should match directory in git rm command"
fi

# Test 22h: single * pattern does NOT get directory match treatment
if (
    source "$LIB_FILE"
    ! is_glob_path_targeted "rm -rf .claude/hooks" ".claude/hooks/*.sh"
); then
    pass "T22h: single * pattern does NOT match parent directory"
else
    fail "T22h: only /** should match directory, not /*.sh"
fi

echo ""

# ============================================================================
# glob_to_regex tests
# ============================================================================
echo "--- glob_to_regex ---"

# Test 22: .claude/hooks/** conversion
result=$(
    source "$LIB_FILE"
    glob_to_regex ".claude/hooks/**"
)
expected="\\.claude/hooks/[^/]*[^/]*"
if [[ "$result" == "$expected" ]]; then
    pass "T22: .claude/hooks/** -> $expected"
else
    fail "T22: .claude/hooks/** got '$result', expected '$expected'"
fi

# Test 23: *.md conversion
result=$(
    source "$LIB_FILE"
    glob_to_regex "*.md"
)
expected="[^/]*\\.md"
if [[ "$result" == "$expected" ]]; then
    pass "T23: *.md -> $expected"
else
    fail "T23: *.md got '$result', expected '$expected'"
fi

# Test 24: Pattern with dots and stars
result=$(
    source "$LIB_FILE"
    glob_to_regex ".claude/*/file.md"
)
expected="\\.claude/[^/]*/file\\.md"
if [[ "$result" == "$expected" ]]; then
    pass "T24: .claude/*/file.md -> $expected"
else
    fail "T24: .claude/*/file.md got '$result', expected '$expected'"
fi

# Test 25: Pattern with ? wildcard
result=$(
    source "$LIB_FILE"
    glob_to_regex "file?.txt"
)
expected="file.\\.txt"
if [[ "$result" == "$expected" ]]; then
    pass "T25: file?.txt -> $expected"
else
    fail "T25: file?.txt got '$result', expected '$expected'"
fi

# Test 26: Pattern with parentheses
result=$(
    source "$LIB_FILE"
    glob_to_regex "test(1).sh"
)
expected="test\\(1\\)\\.sh"
if [[ "$result" == "$expected" ]]; then
    pass "T26: test(1).sh -> $expected"
else
    fail "T26: test(1).sh got '$result', expected '$expected'"
fi

# Test 27: Pattern with braces
result=$(
    source "$LIB_FILE"
    glob_to_regex "file{a,b}.txt"
)
expected="file\\{a,b\\}\\.txt"
if [[ "$result" == "$expected" ]]; then
    pass "T27: file{a,b}.txt -> $expected"
else
    fail "T27: file{a,b}.txt got '$result', expected '$expected'"
fi

# Test 28: Pattern with ^ and $
result=$(
    source "$LIB_FILE"
    glob_to_regex 'test^file$name'
)
expected='test\^file\$name'
if [[ "$result" == "$expected" ]]; then
    pass "T28: special chars ^ and \$ escaped"
else
    fail "T28: special chars got '$result', expected '$expected'"
fi

echo ""

# ============================================================================
# Summary
# ============================================================================
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
fi
exit 0
