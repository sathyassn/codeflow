#!/usr/bin/env bash
# Purpose:   PreToolUse hook for team guard enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh
# Hook Type: PreToolUse
# Matcher:   Teammate
#
# This hook:
#   - Blocks Teammate tool cleanup operation while PathFlow is active
#   - Only applies to Teammate tool with operation="cleanup"
#   - If not Teammate tool or not cleanup operation, allows through
#   - If cleanup but no pathflow-active flag, allows cleanup
#   - If cleanup AND pathflow-active exists, blocks with exit 2
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed (not Teammate, not cleanup, or PathFlow not active)
#   - 2: Operation blocked (cleanup while PathFlow active)

set -euo pipefail

# shellcheck disable=SC2034
VERSION="1.0.0"

# =============================================================================
# EARLY EXIT: CHECK TOOL NAME
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Teammate" ]]; then
    exit 0
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

# Extract operation from tool input
OPERATION=""
if command -v jq &>/dev/null; then
    OPERATION=$(echo "$TOOL_INPUT" | jq -r '.operation // empty')
else
    OPERATION=$(echo "$TOOL_INPUT" | grep -oE '"operation"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"operation"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' | head -1)
fi

# Only gate cleanup operations
if [[ "$OPERATION" != "cleanup" ]]; then
    exit 0
fi

# =============================================================================
# PATHFLOW FLAG CHECK
# =============================================================================

# Allow overriding the flag path for testing
PATHFLOW_FLAG="${PATHFLOW_FLAG_FILE:-/tmp/claude/managed/state/pathflow-active}"

if [[ ! -f "$PATHFLOW_FLAG" ]]; then
    # Not in PathFlow mode - allow cleanup
    exit 0
fi

# =============================================================================
# SETUP (only needed when blocking)
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

# Security library for logging
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# BLOCK: CLEANUP WHILE PATHFLOW ACTIVE
# =============================================================================

# Log the block event
if declare -f log_security_event &>/dev/null; then
    log_security_event "blocked" "team_guard_cleanup" "Teammate" "cleanup" "PathFlow active - team cleanup blocked"
fi

cat >&2 <<EOF
BLOCKED: Team cleanup not allowed during active PathFlow
Reason: PathFlow is active - team resources are still in use
Operation: cleanup

Complete the PathFlow workflow before cleaning up team resources.
EOF
exit 2
