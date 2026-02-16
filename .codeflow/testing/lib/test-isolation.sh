#!/usr/bin/env bash
# CodeFlow Test Framework: Test Isolation
# Location: .codeflow/testing/lib/test-isolation.sh
#
# Source this file at the top of any test that needs a fully isolated repo root.
# Provides complete state isolation for parallel-safe test execution.
#
# What it does:
#   - Auto-detects the real project root (walks up to find .codeflow + .claude)
#   - Creates an isolated temp directory with ALL state subdirectories
#   - Creates a separate TEST_TMPDIR for scratch files
#   - Initializes a git repo (hooks call git commands)
#   - Copies real config files (hooks read enforcement-policy.json etc.)
#   - Exports REPO_ROOT (temp dir), REAL_REPO_ROOT (real project),
#     TEST_REPO_ROOT (alias for REPO_ROOT), and TEST_TMPDIR (scratch dir)
#   - Sets up cleanup trap for both temp directories
#
# Usage (add near top of test file, after set -euo pipefail and TEST_DIR):
#   source "$TEST_DIR/../../lib/test-isolation.sh"
#
# After sourcing, use:
#   REAL_REPO_ROOT  -- path to real project (for HOOK= and reading source files)
#   REPO_ROOT       -- path to isolated temp dir (hooks write state here)
#   TEST_REPO_ROOT  -- same as REPO_ROOT (explicit alias for clarity)
#   TEST_TMPDIR     -- isolated temp dir for scratch files
#
# Example:
#   HOOK="$REAL_REPO_ROOT/.claude/hooks/codeflow/stop/cf-stop-logging.sh"
#   echo '{"session_id":"test"}' | bash "$HOOK"  # writes to $REPO_ROOT/.state/
#
# Compatibility: bash 3.2+ (macOS compatible)
# Version: 2.0.0

# shellcheck disable=SC2034  # REAL_REPO_ROOT, TEST_REPO_ROOT, TEST_TMPDIR exported for test use

# Direct execution prevention (must be sourced, not run)
[[ "${BASH_SOURCE[0]}" == "${0}" ]] && {
    echo "Error: This script must be sourced, not executed directly" >&2
    exit 1
}

# Auto-detect project root by walking up from the sourcing file's directory.
# Looks for a directory containing both .codeflow and .claude.
# BASH_SOURCE[1] is the file that sourced us; [0] is this file.
_find_repo_root() {
    local dir
    dir="$(cd "$(dirname "${BASH_SOURCE[1]}")" && pwd)"
    while [[ "$dir" != "/" ]]; do
        [[ -d "$dir/.codeflow" && -d "$dir/.claude" ]] && { echo "$dir"; return 0; }
        dir="$(dirname "$dir")"
    done
    return 1
}
REAL_REPO_ROOT="$(_find_repo_root)" || {
    echo "Error: Could not find project root (no .codeflow + .claude directory found)" >&2
    return 1
}

# Create isolated temp directory for the fake repo root
TEST_REPO_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/cf-test-isolated-XXXXXX")

# Create isolated temp directory for scratch files
TEST_TMPDIR=$(mktemp -d "${TMPDIR:-/tmp}/cf-test-tmp-XXXXXX")
export TEST_TMPDIR

# Create ALL state directories that any hook might need
mkdir -p "$TEST_REPO_ROOT/.state/logs/sessions"
mkdir -p "$TEST_REPO_ROOT/.state/logs/security/sentinel"
mkdir -p "$TEST_REPO_ROOT/.state/session"
mkdir -p "$TEST_REPO_ROOT/.state/runtime"
mkdir -p "$TEST_REPO_ROOT/.state/sentinels/skill"
mkdir -p "$TEST_REPO_ROOT/.state/sentinels/pathflow"
mkdir -p "$TEST_REPO_ROOT/.state/db"

# Initialize as git repo (hooks call git branch, git status, etc.)
git -C "$TEST_REPO_ROOT" init -q 2>/dev/null || true
git -C "$TEST_REPO_ROOT" commit --allow-empty -m "init" -q 2>/dev/null || true
# Create non-protected branch so tests aren't affected by real repo's branch
git -C "$TEST_REPO_ROOT" checkout -b test-branch -q 2>/dev/null || true

# Copy real config and scripts so hooks can read enforcement policy, modules, etc.
mkdir -p "$TEST_REPO_ROOT/.codeflow"
if [[ -d "$REAL_REPO_ROOT/.codeflow/config" ]]; then
    cp -R "$REAL_REPO_ROOT/.codeflow/config" "$TEST_REPO_ROOT/.codeflow/config" 2>/dev/null || true
fi
# Copy security lib and enforcement modules (needed by security hook).
# Do NOT copy sentinel/ -- its SENTINEL_REPO_ROOT hardcodes git rev-parse,
# which would override the isolated REPO_ROOT. Hooks fall back to manual
# sentinel cleanup when the library is absent, which works correctly.
if [[ -d "$REAL_REPO_ROOT/.codeflow/scripts/security" ]]; then
    mkdir -p "$TEST_REPO_ROOT/.codeflow/scripts/security"
    if [[ -d "$REAL_REPO_ROOT/.codeflow/scripts/security/lib" ]]; then
        cp -R "$REAL_REPO_ROOT/.codeflow/scripts/security/lib" "$TEST_REPO_ROOT/.codeflow/scripts/security/lib" 2>/dev/null || true
    fi
    if [[ -d "$REAL_REPO_ROOT/.codeflow/scripts/security/enforcement" ]]; then
        cp -R "$REAL_REPO_ROOT/.codeflow/scripts/security/enforcement" "$TEST_REPO_ROOT/.codeflow/scripts/security/enforcement" 2>/dev/null || true
    fi
fi
# Copy state scripts (cf-work-state.sh, cf-tracking-generate.sh) needed by hooks
if [[ -d "$REAL_REPO_ROOT/.codeflow/scripts/state" ]]; then
    mkdir -p "$TEST_REPO_ROOT/.codeflow/scripts/state"
    cp -R "$REAL_REPO_ROOT/.codeflow/scripts/state/"*.sh "$TEST_REPO_ROOT/.codeflow/scripts/state/" 2>/dev/null || true
fi

# Export for hooks
REPO_ROOT="$TEST_REPO_ROOT"
export REPO_ROOT
export REAL_REPO_ROOT
export TEST_REPO_ROOT

# Cleanup on exit -- remove both temp directories
_test_isolation_cleanup() {
    rm -rf "$TEST_REPO_ROOT" 2>/dev/null || true
    rm -rf "$TEST_TMPDIR" 2>/dev/null || true
}
trap _test_isolation_cleanup EXIT
