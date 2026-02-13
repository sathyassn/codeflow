#!/usr/bin/env bash
# Purpose:   Shared library for active task state management
# Usage:     source "$REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh"
# Platform:  macOS/Linux
#
# This is a LIBRARY file - meant to be sourced, not executed directly.
#
# Functions:
#   - get_active_task_file: Returns canonical path to active-task.json
#   - set_active_task: Writes active-task.json with task metadata
#   - get_active_task: Reads and returns active-task.json content
#   - get_active_task_id: Returns just the task_id
#   - update_active_task_status: Updates status field in active-task.json
#   - clear_active_task: Removes the active-task.json file
#   - is_task_active: Checks if file exists and status is in_progress
#   - is_current_session_task: Checks if task belongs to current session
#   - is_active_task_stale: Checks if task is older than 24h
#
# Parallel work safety:
#   - active-task.json includes session_id field for ownership checking
#   - Uses flock for atomic reads/writes to prevent race conditions
#   - .state/ is gitignored so no cross-machine conflicts
#
# V4 PathFlow support:
#   - Optional current_stage and team_name fields in active-task.json
#   - When PathFlow is active, set_active_task accepts these additional parameters

set -euo pipefail

# =============================================================================
# SOURCE GUARD - Prevent double-sourcing
# =============================================================================

if [[ -n "${_CF_WORK_STATE_LIB_SOURCED:-}" ]]; then
    # shellcheck disable=SC2317
    return 0 2>/dev/null || exit 0
fi
_CF_WORK_STATE_LIB_SOURCED=1

# =============================================================================
# CONSTANTS
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
readonly _WS_ACTIVE_TASK_DIR="$REPO_ROOT/.state/runtime"
readonly _WS_ACTIVE_TASK_FILE="$_WS_ACTIVE_TASK_DIR/active-task.json"

# Exported convenience aliases (for hooks that previously used their own paths)
# shellcheck disable=SC2034
ACTIVE_TASK_DIR="$_WS_ACTIVE_TASK_DIR"
# shellcheck disable=SC2034
ACTIVE_TASK_FILE="$_WS_ACTIVE_TASK_FILE"

# =============================================================================
# FUNCTIONS
# =============================================================================

# Get the canonical path to active-task.json
# Returns: path string on stdout
get_active_task_file() {
    echo "$_WS_ACTIVE_TASK_FILE"
}

# Write active-task.json with task metadata
# Args: task_id epic_id title status branch [current_stage] [team_name]
# Uses flock for atomic write
set_active_task() {
    local task_id="$1"
    local epic_id="${2:-}"
    local title="${3:-}"
    local status="${4:-in_progress}"
    local branch="${5:-}"
    local current_stage="${6:-}"
    local team_name="${7:-}"
    local session_id="${CODEFLOW_SESSION_ID:-unknown}"

    mkdir -p "$_WS_ACTIVE_TASK_DIR" 2>/dev/null || true

    if command -v jq &>/dev/null; then
        local json
        json=$(jq -nc \
            --arg task_id "$task_id" \
            --arg epic_id "$epic_id" \
            --arg title "$title" \
            --arg status "$status" \
            --arg branch "$branch" \
            --arg session_id "$session_id" \
            --arg created_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            --arg updated_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            '{task_id: $task_id, epic_id: $epic_id, title: $title, status: $status, branch: $branch, session_id: $session_id, created_at: $created_at, updated_at: $updated_at}')

        # Add PathFlow fields if provided
        if [[ -n "$current_stage" ]]; then
            json=$(echo "$json" | jq --arg current_stage "$current_stage" '. + {current_stage: $current_stage}')
        fi
        if [[ -n "$team_name" ]]; then
            json=$(echo "$json" | jq --arg team_name "$team_name" '. + {team_name: $team_name}')
        fi

        # Atomic write with flock if available
        if command -v flock &>/dev/null; then
            (
                flock -x 200
                echo "$json" > "$_WS_ACTIVE_TASK_FILE"
            ) 200>"$_WS_ACTIVE_TASK_FILE.lock"
        else
            echo "$json" > "$_WS_ACTIVE_TASK_FILE"
        fi
    fi
}

# Read and return active-task.json content
# Returns: JSON string on stdout, empty if no file
get_active_task() {
    if [[ -f "$_WS_ACTIVE_TASK_FILE" ]]; then
        cat "$_WS_ACTIVE_TASK_FILE" 2>/dev/null || echo ""
    fi
}

# Return just the task_id from active-task.json
# Returns: task_id string on stdout, empty if no file
get_active_task_id() {
    if [[ -f "$_WS_ACTIVE_TASK_FILE" ]] && command -v jq &>/dev/null; then
        jq -r '.task_id // empty' "$_WS_ACTIVE_TASK_FILE" 2>/dev/null || echo ""
    fi
}

# Update the status field in active-task.json
# Args: new_status
update_active_task_status() {
    local new_status="$1"

    if [[ ! -f "$_WS_ACTIVE_TASK_FILE" ]]; then
        return 1
    fi

    if command -v jq &>/dev/null; then
        local updated
        updated=$(jq \
            --arg status "$new_status" \
            --arg updated_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
            '.status = $status | .updated_at = $updated_at' \
            "$_WS_ACTIVE_TASK_FILE" 2>/dev/null)

        if [[ -n "$updated" ]]; then
            if command -v flock &>/dev/null; then
                (
                    flock -x 200
                    echo "$updated" > "$_WS_ACTIVE_TASK_FILE"
                ) 200>"$_WS_ACTIVE_TASK_FILE.lock"
            else
                echo "$updated" > "$_WS_ACTIVE_TASK_FILE"
            fi
        fi
    fi
}

# Remove the active-task.json file
clear_active_task() {
    rm -f "$_WS_ACTIVE_TASK_FILE" 2>/dev/null || true
    rm -f "$_WS_ACTIVE_TASK_FILE.lock" 2>/dev/null || true
}

# Check if an active task exists and is in_progress
# Returns: 0 if active, 1 if not
is_task_active() {
    if [[ ! -f "$_WS_ACTIVE_TASK_FILE" ]]; then
        return 1
    fi

    if command -v jq &>/dev/null; then
        local status
        status=$(jq -r '.status // empty' "$_WS_ACTIVE_TASK_FILE" 2>/dev/null)
        [[ "$status" == "in_progress" ]]
    else
        return 1
    fi
}

# Check if active task belongs to current session
# Args: [session_id] (defaults to CODEFLOW_SESSION_ID)
# Returns: 0 if current session, 1 if not
is_current_session_task() {
    local current_session="${1:-${CODEFLOW_SESSION_ID:-}}"
    [[ -z "$current_session" ]] && return 0  # Can't determine, assume yes
    [[ -f "$_WS_ACTIVE_TASK_FILE" ]] || return 1

    local file_session
    file_session=$(jq -r '.session_id // empty' "$_WS_ACTIVE_TASK_FILE" 2>/dev/null)
    [[ "$file_session" == "$current_session" || "$file_session" == "unknown" ]]
}

# Check if active task is stale (>24h old)
# Returns: 0 if stale, 1 if fresh or no file
is_active_task_stale() {
    [[ -f "$_WS_ACTIVE_TASK_FILE" ]] || return 1
    local updated_at
    updated_at=$(jq -r '.updated_at // empty' "$_WS_ACTIVE_TASK_FILE" 2>/dev/null)
    [[ -z "$updated_at" ]] && return 0  # No timestamp = stale

    # Compare timestamps (24h = 86400s)
    local file_epoch current_epoch
    if date -j -f "%Y-%m-%dT%H:%M:%SZ" "$updated_at" +%s &>/dev/null 2>&1; then
        # macOS
        file_epoch=$(date -j -f "%Y-%m-%dT%H:%M:%SZ" "$updated_at" +%s 2>/dev/null)
    elif date -j -f "%Y-%m-%dT%H:%M:%S.000Z" "$updated_at" +%s &>/dev/null 2>&1; then
        # macOS with milliseconds format
        file_epoch=$(date -j -f "%Y-%m-%dT%H:%M:%S.000Z" "$updated_at" +%s 2>/dev/null)
    else
        # Linux
        file_epoch=$(date -d "$updated_at" +%s 2>/dev/null)
    fi
    current_epoch=$(date +%s)

    [[ -n "${file_epoch:-}" ]] && (( current_epoch - file_epoch > 86400 ))
}
