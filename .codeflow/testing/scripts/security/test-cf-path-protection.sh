#!/usr/bin/env bash
# Test: cf-path-protection.sh (fixed version)
# Location: /tmp/claude/fixed-test-cf-path-protection.sh
#
# Functional tests for the path protection enforcement module.
# Tests command segmentation (cross-contamination fix) and cp direction awareness.
#
# Uses a wrapper script that reads the COMMAND from a file to avoid
# the live security hooks blocking test invocations.

set -euo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$TEST_DIR/../../lib/test-isolation.sh"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
MODULE="$REAL_REPO_ROOT/.codeflow/scripts/security/enforcement/cf-path-protection.sh"
WRAPPER="$REAL_REPO_ROOT/.codeflow/testing/scripts/security/enforcement/helper-runner-wrapper.sh"
CMDFILE="$TEST_TMPDIR/test-cmd.txt"

export REPO_ROOT LIB_DIR CF_PATH_PROTECTION_MODULE="$MODULE"

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0

pass() { echo "  PASS: $1"; TESTS_PASSED=$((TESTS_PASSED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }
fail() { echo "  FAIL: $1"; TESTS_FAILED=$((TESTS_FAILED + 1)); TESTS_RUN=$((TESTS_RUN + 1)); }

# expect_blocked: run module with command, expect exit 2 (blocked)
expect_blocked() {
    local desc="$1"
    local cmd="$2"
    printf '%s' "$cmd" > "$CMDFILE"
    local rc=0
    bash "$WRAPPER" "$CMDFILE" 2>/dev/null || rc=$?
    if [[ $rc -eq 2 ]]; then
        pass "$desc"
    else
        fail "$desc (expected exit 2, got $rc)"
    fi
}

# expect_allowed: run module with command, expect exit 0 (allowed)
expect_allowed() {
    local desc="$1"
    local cmd="$2"
    printf '%s' "$cmd" > "$CMDFILE"
    local rc=0
    bash "$WRAPPER" "$CMDFILE" 2>/dev/null || rc=$?
    if [[ $rc -eq 0 ]]; then
        pass "$desc"
    else
        fail "$desc (expected exit 0, got $rc)"
    fi
}

echo "=== Testing cf-path-protection.sh (Fixed Version) ==="
echo ""

# =========================================================================
# SECTION 1: Basic structural checks
# =========================================================================
echo "--- Structural Checks ---"

if [[ -f "$MODULE" ]]; then pass "Module file exists"; else fail "Module file not found at $MODULE"; fi

if bash -n "$MODULE" 2>/dev/null; then
    pass "Module has valid bash syntax"
else
    fail "Module has invalid bash syntax"
fi

if command -v shellcheck &>/dev/null; then
    if shellcheck -e SC1091 "$MODULE" 2>/dev/null; then
        pass "Passes shellcheck"
    else
        fail "Fails shellcheck"
    fi
else
    pass "Shellcheck not available (skipped)"
fi

echo ""

# =========================================================================
# SECTION 2: Direct dangerous operations on protected paths (MUST block)
# =========================================================================
echo "--- Direct Dangerous Operations (should block) ---"

expect_blocked "T01: rm protected file" \
    "rm .claude/settings.json"

expect_blocked "T02: rm -f protected file" \
    "rm -f .claude/settings.json"

expect_blocked "T03: mv protected path" \
    "mv .codeflow/config/file.json /tmp/claude/backup"

expect_blocked "T04: chmod protected hook" \
    "chmod 755 .claude/hooks/codeflow/stop/hook.sh"

expect_blocked "T05: git rm protected file" \
    "git rm .claude/settings.json"

expect_blocked "T06: shred protected file" \
    "shred .claude/settings.json"

expect_blocked "T07: truncate protected path" \
    "truncate -s 0 .codeflow/config/enforcement-policy.json"

echo ""

# =========================================================================
# SECTION 3: Cross-contamination fix (compound commands)
# =========================================================================
echo "--- Cross-contamination Fix (compound commands) ---"

expect_allowed "T08: hook-path && rm-other-target (different segments)" \
    "bash .claude/hooks/codeflow/stop/hook.sh && rm -f /tmp/claude/file"

expect_blocked "T09: echo && rm-protected (rm in same segment as path)" \
    "echo test && rm .codeflow/config/file.json"

expect_allowed "T10: mv-unrelated && hook-path (different segments)" \
    "mv old-file.txt new-file.txt && bash .claude/hooks/codeflow/stop/hook.sh"

expect_allowed "T11: multi-safe-segments with hook ref" \
    "rm /tmp/file && cp /tmp/a /tmp/b && bash .claude/hooks/codeflow/stop/hook.sh"

expect_allowed "T12: cat-protected ; rm-tmp (semicolon separated)" \
    "cat .claude/settings.json; rm /tmp/claude/garbage"

expect_allowed "T13: cat-protected | grep (pipe separated)" \
    "cat .claude/settings.json | grep something"

expect_allowed "T14: test-protected || echo (or-separated)" \
    "test -f .claude/settings.json || echo missing"

expect_blocked "T15: echo && rm-protected-deep" \
    "echo hello && rm -rf .codeflow/scripts/security/lib"

echo ""

# =========================================================================
# SECTION 4: cp direction awareness
# =========================================================================
echo "--- cp Direction Awareness ---"

expect_blocked "T16: cp TO protected (destination)" \
    "cp /tmp/claude/file .claude/settings.json"

expect_allowed "T17: cp FROM protected (source)" \
    "cp .claude/settings.json /tmp/claude/backup"

expect_allowed "T18: cp -r FROM protected dir" \
    "cp -r .codeflow/config /tmp/claude/config-backup"

expect_blocked "T19: cp -r TO protected dir" \
    "cp -r /tmp/claude/malicious .codeflow/config"

expect_blocked "T20: cp -f TO protected" \
    "cp -f /tmp/claude/evil .claude/settings.json"

echo ""

# =========================================================================
# SECTION 5: Safe operations (reading, execution)
# =========================================================================
echo "--- Safe Operations (should allow) ---"

expect_allowed "T21: bash execution of hook" \
    "bash .claude/hooks/codeflow/stop/hook.sh"

expect_allowed "T22: cat (read) protected file" \
    "cat .claude/settings.json"

expect_allowed "T23: ls protected directory" \
    "ls .codeflow/config/"

expect_allowed "T24: grep in protected path" \
    "grep -r pattern .codeflow/scripts/security"

expect_allowed "T25: head protected file" \
    "head -5 .claude/settings.json"

echo ""

# =========================================================================
# SECTION 6: Directory protection (.claude and .codeflow)
# =========================================================================
echo "--- Directory Protection ---"

expect_blocked "T26: rm -rf .claude" \
    "rm -rf .claude"

expect_blocked "T27: rm -rf .claude/" \
    "rm -rf .claude/"

expect_blocked "T28: rm -rf .codeflow" \
    "rm -rf .codeflow"

expect_blocked "T29: rm -rf .codeflow/" \
    "rm -rf .codeflow/"

expect_allowed "T30: echo .claude && rm tmp (dir cross-contamination)" \
    "echo .claude && rm /tmp/claude/file"

echo ""

# =========================================================================
# SECTION 7: Redirect protection
# =========================================================================
echo "--- Redirect Protection ---"

expect_blocked "T31: redirect overwrite protected" \
    "echo test > .codeflow/config/enforcement/enforcement-policy.json"

expect_blocked "T32: redirect append protected" \
    "echo data >> .claude/settings.json"

expect_allowed "T33: fd redirect 2>&1 with hook path" \
    "bash .claude/hooks/codeflow/stop/hook.sh 2>&1"

echo ""

# =========================================================================
# SECTION 8: Quote-aware segmentation
# =========================================================================
echo "--- Quote-Aware Segmentation ---"

expect_allowed "T34: && inside single quotes (not real split)" \
    "echo 'safe && text .claude/settings.json'"

expect_allowed "T35: && inside double quotes (not real split)" \
    'echo "safe && text .claude/settings.json"'

echo ""

# =========================================================================
# SECTION 9: Edge cases
# =========================================================================
echo "--- Edge Cases ---"

expect_blocked "T36: unlink protected" \
    "unlink .claude/settings.json"

expect_blocked "T37: chown protected" \
    "chown root .claude/settings.json"

expect_allowed "T38: empty command" \
    ""

expect_allowed "T39: whitespace-only command" \
    "   "

expect_allowed "T40: protected path as substring (no boundary match)" \
    "rm .claude/settings.json.bak"

echo ""

# =========================================================================
# Summary
# =========================================================================
echo "=== Test Summary ==="
echo "Passed: $TESTS_PASSED"
echo "Failed: $TESTS_FAILED"
echo "Total:  $TESTS_RUN"

# Cleanup
rm -f "$CMDFILE"

[[ $TESTS_FAILED -gt 0 ]] && exit 1
exit 0
