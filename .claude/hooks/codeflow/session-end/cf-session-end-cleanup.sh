#!/usr/bin/env bash
# Purpose:   SessionEnd hook for session cleanup
# Location:  .claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh
# Hook Type: SessionEnd
# Usage:     Called by Claude Code at session end
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook:
#   - Cleans up session sentinels (*.json files)
#   - Removes session-specific temp files
#   - Cleans up session state files
#   - Finalizes session state
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Cleanup completed successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.1.0"

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

# Get session ID from environment
SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

# Read sentinel directory from config
SENTINEL_DIR="/tmp/claude/managed/sentinels"
STATE_DIR="/tmp/claude/managed/state"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    SENTINEL_DIR=$(jq -r '.sentinel.directory // "/tmp/claude/managed/sentinels"' "$CONFIG" 2>/dev/null || echo "/tmp/claude/managed/sentinels")
    STATE_DIR=$(jq -r '.protected_paths.managed_tmp.state_folder // "/tmp/claude/managed/state"' "$CONFIG" 2>/dev/null || echo "/tmp/claude/managed/state")
fi

# =============================================================================
# SENTINEL CLEANUP
# =============================================================================

# Remove all sentinels (they're session-scoped)
# Sentinel library creates *.json files in format: ${skill}:${operation}-${id}.json
if [[ -d "$SENTINEL_DIR" ]]; then
    rm -f "$SENTINEL_DIR"/*.json 2>/dev/null || true
fi

# =============================================================================
# V4: PATHFLOW CLEANUP
# =============================================================================

# Remove pathflow-active flag file
PATHFLOW_ACTIVE="/tmp/claude/managed/state/pathflow-active"
if [[ -f "$PATHFLOW_ACTIVE" ]]; then
    rm -f "$PATHFLOW_ACTIVE" 2>/dev/null || true
fi

# Remove PathFlow sentinels (session-scoped, stored in .state/sentinels/)
PATHFLOW_SENTINEL_DIR="$REPO_ROOT/.state/sentinels"
if [[ -d "$PATHFLOW_SENTINEL_DIR" ]]; then
    rm -f "$PATHFLOW_SENTINEL_DIR"/pathflow:* 2>/dev/null || true
fi

# =============================================================================
# STATE FILE CLEANUP
# =============================================================================

# Clean up session-specific state files (memory-progress, claim-heartbeat, etc.)
if [[ -d "$STATE_DIR" ]] && [[ "$SESSION_ID" != "unknown" ]]; then
    rm -f "$STATE_DIR"/*-"$SESSION_ID" 2>/dev/null || true
    rm -f "$STATE_DIR"/*-"$SESSION_ID".json 2>/dev/null || true
fi

# =============================================================================
# TEMPORARY FILE CLEANUP
# =============================================================================

# Clean up session-specific temp files
TEMP_DIR="/tmp/claude/sessions/$SESSION_ID"
if [[ -d "$TEMP_DIR" ]]; then
    rm -rf "$TEMP_DIR" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
