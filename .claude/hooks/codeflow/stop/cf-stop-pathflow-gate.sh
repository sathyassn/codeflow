#!/usr/bin/env bash
# Purpose:   Stop hook for PathFlow phase completion logging
# Location:  .claude/hooks/codeflow/stop/cf-stop-pathflow-gate.sh
# Hook Type: Stop
# Usage:     Called by Claude Code at stop event
# Platform:  macOS/Linux
# Version:   3.0.0
#
# This hook:
#   - Reads JSON from stdin (session_id, transcript_path, stop_hook_active)
#   - Logs PathFlow phase completion status to JSONL for audit trail
#   - Non-blocking: never returns block JSON, always exits 0
#   - Replaces cf-stop-verify-work.sh (PCV enforcement removed)
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Always (stop hooks must never return non-zero)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="3.0.0"

# =============================================================================
# STDIN READING
# =============================================================================

# Read JSON from stdin (Claude Code provides session context)
INPUT=$(cat)

# Extract fields from stdin JSON
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty' 2>/dev/null || echo "")
STOP_ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // false' 2>/dev/null || echo "false")

# =============================================================================
# INFINITE LOOP GUARD
# =============================================================================

# If stop_hook_active is true, we are in an active retry from a previous block.
# Exit immediately to prevent infinite loop.
if [ "$STOP_ACTIVE" = "true" ]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"

# Source env file for canonical CODEFLOW_SESSION_ID
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [ -f "$_env_file" ]; then
    # shellcheck source=/dev/null
    source "$_env_file"
fi

# Set CODEFLOW_SESSION_ID: prefer env file, fall back to stdin
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-${SESSION_ID:-unknown}}"
export CODEFLOW_SESSION_ID

# Source security library for logging and pathflow detection
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [ -f "$LIB_DIR/security-lib.sh" ]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# PATHFLOW MODE CHECK
# =============================================================================

# Only log if PathFlow mode is active
if ! is_pathflow_active 2>/dev/null; then
    exit 0
fi

# =============================================================================
# LOG PHASE COMPLETION STATUS
# =============================================================================

# Read current phase from JSONL for audit logging
JSONL_FILE="${PATHFLOW_JSONL_FILE:-$REPO_ROOT/.state/logs/pathflow-events.jsonl}"

if [ -f "$JSONL_FILE" ] && command -v jq >/dev/null 2>&1; then
    CURRENT_SESSION_ID="$CODEFLOW_SESSION_ID"

    if [ -n "$CURRENT_SESSION_ID" ] && [ "$CURRENT_SESSION_ID" != "unknown" ]; then
        CURRENT_PHASE=$(tail -100 "$JSONL_FILE" 2>/dev/null | \
            jq -r "select(.type==\"phase_transition\" and .session_id==\"$CURRENT_SESSION_ID\") | .phase" 2>/dev/null | \
            tail -1 || echo "")

        if [ -n "$CURRENT_PHASE" ]; then
            # Log the stop event with current phase for audit trail
            echo "pathflow-stop: session=$CURRENT_SESSION_ID phase=$CURRENT_PHASE" >&2
        fi
    fi
fi

# =============================================================================
# ALWAYS ALLOW STOP
# =============================================================================

exit 0
