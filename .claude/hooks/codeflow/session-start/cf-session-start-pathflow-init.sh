#!/usr/bin/env bash
# Purpose:   SessionStart hook for PathFlow initialization (flag creation)
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-pathflow-init.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start (runs AFTER cleanup hook)
# Platform:  macOS/Linux
#
# This hook:
#   - Reads session_id from stdin (Claude Code provides JSON on SessionStart)
#   - Sources cf-pathflow-state.sh library
#   - Creates the pathflow-active flag with JSON metadata
#   - Flag enables PathFlow enforcement for this session
#
# The cleanup hook (cf-session-start-cleanup.sh) creates the directories.
# This hook creates the PathFlow-specific flag file within those directories.
#
# Flag file: .state/session/{SESSION_ID}/is-pathflow-active
# Format: JSON {session_id, team_name, created_at, tracking_level}
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - PathFlow initialized successfully (always exits 0)

set -euo pipefail

# =============================================================================
# STDIN READING (Claude Code provides JSON with session_id)
# =============================================================================

if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null) || true
        if [[ -n "${_sid:-}" ]]; then
            CODEFLOW_SESSION_ID="$_sid"
        fi
    fi
fi

# Generate session ID if not set
if [[ -z "${CODEFLOW_SESSION_ID:-}" ]]; then
    TIMESTAMP=$(date +%s%N 2>/dev/null | cut -c1-13 || date +%s)
    RANDOM_PART=$(head -c 6 /dev/urandom | od -An -tx1 | tr -d ' \n')
    CODEFLOW_SESSION_ID="ses-${TIMESTAMP}${RANDOM_PART}"
fi
export CODEFLOW_SESSION_ID

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# SOURCE PATHFLOW STATE LIBRARY
# =============================================================================

_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ ! -f "$_PFS_LIB" ]]; then
    # Library not found — graceful degradation, skip flag creation
    exit 0
fi

# shellcheck source=/dev/null
source "$_PFS_LIB"

# =============================================================================
# CREATE PATHFLOW FLAG
# =============================================================================

# Ensure session directory exists (cleanup hook should have created it,
# but be defensive)
mkdir -p "$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID" 2>/dev/null || true

# Create flag with initial metadata
# team_name is empty at init (updated when TeamCreate is called)
# tracking_level starts as "pending" (updated at PF3-CLASSIFY)
create_pathflow_flag "$CODEFLOW_SESSION_ID" ""

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
