#!/usr/bin/env bash
# Purpose:   PreToolUse hook for PathFlow gate enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write|Bash
#
# This hook:
#   - Blocks Edit/Write/Bash(git commit) when PathFlow is active but PF-3 not complete
#   - Only checks when pathflow-active flag file exists (PathFlow mode)
#   - If flag doesn't exist, allows everything (standalone mode)
#   - Checks for PF-3 sentinel at REPO_ROOT/.state/sentinels/pathflow:pf-3-*
#   - For Bash: only gates "git commit" commands
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed (not in PathFlow, PF-3 complete, or non-gated tool)
#   - 2: Operation blocked (PathFlow active, PF-3 not complete)

set -euo pipefail

# shellcheck disable=SC2034
VERSION="1.0.0"

# =============================================================================
# EARLY EXIT: CHECK TOOL NAME
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Edit" ]] && [[ "$TOOL_NAME" != "Write" ]] && [[ "$TOOL_NAME" != "Bash" ]]; then
    exit 0
fi

# =============================================================================
# PATHFLOW FLAG CHECK
# =============================================================================

# Allow overriding the flag path for testing
PATHFLOW_FLAG="${PATHFLOW_FLAG_FILE:-/tmp/claude/managed/state/pathflow-active}"

if [[ ! -f "$PATHFLOW_FLAG" ]]; then
    # Not in PathFlow mode - allow everything
    exit 0
fi

# =============================================================================
# SETUP
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
# BASH TOOL: ONLY GATE GIT COMMIT
# =============================================================================

if [[ "$TOOL_NAME" == "Bash" ]]; then
    TOOL_INPUT="${TOOL_INPUT:-}"
    if [[ -z "$TOOL_INPUT" ]]; then
        exit 0
    fi

    COMMAND=""
    if command -v jq &>/dev/null; then
        COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty')
    else
        COMMAND=$(echo "$TOOL_INPUT" | grep -oE '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"command"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' | head -1)
    fi

    if [[ -z "$COMMAND" ]]; then
        exit 0
    fi

    # Only gate git commit commands
    if ! echo "$COMMAND" | grep -qE '(^|\s|&&|\|)git\s+commit(\s|$)'; then
        exit 0
    fi
fi

# =============================================================================
# PF-3 SENTINEL CHECK
# =============================================================================

# Allow overriding sentinel dir for testing
SENTINEL_DIR="${PATHFLOW_SENTINEL_DIR:-$REPO_ROOT/.state/sentinels}"

# Check for PF-3 sentinel (work classification complete)
pf3_found=false
for file in "$SENTINEL_DIR"/pathflow:pf-3-*; do
    if [[ -f "$file" ]]; then
        pf3_found=true
        break
    fi
done

if [[ "$pf3_found" == "true" ]]; then
    # PF-3 complete - allow operation
    exit 0
fi

# =============================================================================
# BLOCK: PATHFLOW ACTIVE BUT PF-3 NOT COMPLETE
# =============================================================================

# Log the block event
if declare -f log_security_event &>/dev/null; then
    log_security_event "blocked" "pathflow_gate_pf3" "$TOOL_NAME" "${COMMAND:-$TOOL_NAME}" "PathFlow active but PF-3 not complete"
fi

cat >&2 <<EOF
BLOCKED: PathFlow gate - work classification required
Reason: PathFlow is active but PF-3 (work classification) has not been completed
Tool: $TOOL_NAME

Complete PF-3 before making changes or committing.
EOF
exit 2
