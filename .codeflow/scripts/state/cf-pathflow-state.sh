#!/usr/bin/env bash
# Purpose:   Shared library for PathFlow state management (flag + sentinels)
# Usage:     source "$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
# Platform:  macOS/Linux
#
# This is a LIBRARY file - meant to be sourced, not executed directly.
#
# Functions:
#   - create_pathflow_flag: Creates the pathflow-active flag with JSON metadata
#   - remove_pathflow_flag: Removes the pathflow-active flag
#   - create_sentinel: Creates a PathFlow sentinel file
#   - has_sentinel: Checks if a sentinel exists
#   - list_sentinels: Lists all sentinels for the current session
#
# Flag file:
#   .state/session/{SESSION_ID}/is-pathflow-active
#   Contains JSON: {session_id, team_name, created_at, tracking_level}
#
# Sentinel files:
#   .state/sentinels/pathflow/{SESSION_ID}/pathflow-{name}
#   Empty files (existence = truth). Created by touch, checked by [[ -f ]].
#
# Environment:
#   CODEFLOW_SESSION_ID  - Session identifier (required)
#   REPO_ROOT            - Repository root (auto-detected if not set)
#
# Compatibility: bash 3.2+ (macOS compatible)

set -euo pipefail

# =============================================================================
# SOURCE GUARD - Prevent double-sourcing
# =============================================================================

if [[ -n "${_CF_PATHFLOW_STATE_LIB_SOURCED:-}" ]]; then
    # shellcheck disable=SC2317
    return 0 2>/dev/null || exit 0
fi
_CF_PATHFLOW_STATE_LIB_SOURCED=1

# =============================================================================
# CONSTANTS
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

readonly _PFS_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
readonly _PFS_SESSION_DIR="$REPO_ROOT/.state/session/$_PFS_SESSION_ID"
readonly _PFS_FLAG_FILE="$_PFS_SESSION_DIR/is-pathflow-active"
readonly _PFS_SENTINEL_DIR="$REPO_ROOT/.state/sentinels/pathflow/$_PFS_SESSION_ID"

# =============================================================================
# FLAG OPERATIONS
# =============================================================================

# Create the pathflow-active flag file with JSON metadata.
# Args: [session_id] [team_name]
# Defaults: session_id from env, team_name empty
create_pathflow_flag() {
    local session_id="${1:-$_PFS_SESSION_ID}"
    local team_name="${2:-}"

    mkdir -p "$_PFS_SESSION_DIR" 2>/dev/null || true

    local created_at
    created_at=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)

    if command -v jq &>/dev/null; then
        jq -nc \
            --arg session_id "$session_id" \
            --arg team_name "$team_name" \
            --arg created_at "$created_at" \
            --arg tracking_level "pending" \
            '{session_id: $session_id, team_name: $team_name, created_at: $created_at, tracking_level: $tracking_level}' \
            > "$_PFS_FLAG_FILE"
    else
        # Fallback without jq - write JSON manually
        printf '{"session_id":"%s","team_name":"%s","created_at":"%s","tracking_level":"pending"}\n' \
            "$session_id" "$team_name" "$created_at" \
            > "$_PFS_FLAG_FILE"
    fi
}

# Remove the pathflow-active flag file.
remove_pathflow_flag() {
    rm -f "$_PFS_FLAG_FILE" 2>/dev/null || true
}

# =============================================================================
# SENTINEL OPERATIONS
# =============================================================================

# Create a PathFlow sentinel file.
# Args: name (e.g., "pf-1", "pf-3", "ws-dev")
# Creates: .state/sentinels/pathflow/{SID}/pathflow-{name}
# Idempotent: touch on existing file is a no-op.
create_sentinel() {
    local name="$1"

    if [[ -z "$name" ]]; then
        return 1
    fi

    mkdir -p "$_PFS_SENTINEL_DIR" 2>/dev/null || true
    touch "$_PFS_SENTINEL_DIR/pathflow-$name"
}

# Check if a PathFlow sentinel exists.
# Args: name (e.g., "pf-3", "ws-rev")
# Returns: 0 if exists, 1 if not
has_sentinel() {
    local name="$1"

    if [[ -z "$name" ]]; then
        return 1
    fi

    [[ -f "$_PFS_SENTINEL_DIR/pathflow-$name" ]]
}

# List all PathFlow sentinels for the current session.
# Output: one sentinel name per line (without pathflow- prefix)
# Returns: 0 always (empty list is valid)
list_sentinels() {
    if [[ ! -d "$_PFS_SENTINEL_DIR" ]]; then
        return 0
    fi

    local _pfs_sentinel _pfs_sname
    for _pfs_sentinel in "$_PFS_SENTINEL_DIR"/pathflow-*; do
        [[ -f "$_pfs_sentinel" ]] || continue
        _pfs_sname="${_pfs_sentinel##*/}"
        echo "${_pfs_sname#pathflow-}"
    done
}

# =============================================================================
# LIBRARY GUARD
# =============================================================================

if [[ "${BASH_SOURCE[0]:-}" == "${0:-}" ]]; then
    echo "Error: This is a library file. Source it instead of executing." >&2
    echo "Usage: source \"$(basename "$0")\"" >&2
    exit 1
fi
