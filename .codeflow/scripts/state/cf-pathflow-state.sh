#!/usr/bin/env bash
# Purpose:   Shared library for PathFlow state management (flag + sentinels + checkpoints)
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
#   - checkpoint_read: Read checkpoint JSON for the current session
#   - checkpoint_write: Write checkpoint JSON atomically
#   - checkpoint_init_phase: Initialize a phase entry with expected tasks from config
#   - checkpoint_register_task: Register a task in the checkpoint
#   - checkpoint_complete_task: Mark a task complete, return phase completion status
#   - checkpoint_skip_task: Mark a task as skipped (conditional tasks not applicable)
#   - checkpoint_is_phase_complete: Check if all expected tasks are done/skipped
#   - checkpoint_init_all_phases: Pre-initialize all phases from pathflow-config.json
#
# Flag file:
#   .state/session/{SESSION_ID}/pathflow/is-pathflow-active
#   Contains JSON: {session_id, team_name, created_at, tracking_level}
#
# Sentinel files:
#   .state/sentinels/pathflow/{SESSION_ID}/pathflow-{name}
#   Empty files (existence = truth). Created by touch, checked by [[ -f ]].
#
# Checkpoint file:
#   .state/session/{SESSION_ID}/pathflow/pathflow-phase-tasks.json
#   Contains JSON: {PF1: {expected, registered, completed, skipped, sentinel_created}, ...}
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

# TODO(go-cli): Session ID source unchanged. Go CLI writes same env file
# Consider adding DB lookup as secondary source for resilience
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
fi
readonly _PFS_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
readonly _PFS_SESSION_DIR="$REPO_ROOT/.state/session/$_PFS_SESSION_ID"
readonly _PFS_PATHFLOW_DIR="$_PFS_SESSION_DIR/pathflow"
readonly _PFS_FLAG_FILE="$_PFS_PATHFLOW_DIR/is-pathflow-active"
readonly _PFS_SENTINEL_DIR="$REPO_ROOT/.state/sentinels/pathflow/$_PFS_SESSION_ID"
readonly _PFS_CHECKPOINT_DIR="$_PFS_PATHFLOW_DIR"
readonly _PFS_CHECKPOINT_FILE="$_PFS_CHECKPOINT_DIR/pathflow-phase-tasks.json"
readonly _PFS_PATHFLOW_CONFIG="$REPO_ROOT/.codeflow/config/pathflow/pathflow-config.json"

# =============================================================================
# FLAG OPERATIONS
# =============================================================================

# Create the pathflow-active flag file with JSON metadata.
# Args: [session_id] [team_name]
# Defaults: session_id from env, team_name empty
create_pathflow_flag() {
    local session_id="${1:-$_PFS_SESSION_ID}"
    local team_name="${2:-}"

    mkdir -p "$_PFS_PATHFLOW_DIR" 2>/dev/null || true

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
# CHECKPOINT OPERATIONS
# =============================================================================

# Read checkpoint JSON for the current session.
# Output: JSON string to stdout (empty object {} if file missing or corrupted)
# Returns: 0 on success, 1 on read error (still outputs {} on error)
checkpoint_read() {
    if [[ ! -f "$_PFS_CHECKPOINT_FILE" ]]; then
        echo "{}"
        return 0
    fi

    local content
    content=$(cat "$_PFS_CHECKPOINT_FILE" 2>/dev/null) || {
        echo "{}"
        return 1
    }

    # Validate JSON
    if command -v jq &>/dev/null; then
        if echo "$content" | jq empty 2>/dev/null; then
            echo "$content"
            return 0
        else
            echo "checkpoint_read: corrupted JSON in checkpoint file" >&2
            echo "{}"
            return 1
        fi
    else
        echo "checkpoint_read: jq not available, cannot parse checkpoint" >&2
        echo "{}"
        return 1
    fi
}

# Write checkpoint JSON atomically (tmp file + mv).
# Args: json_content (JSON string)
# Returns: 0 on success, 1 on write error
checkpoint_write() {
    local json_content="$1"

    if [[ -z "$json_content" ]]; then
        echo "checkpoint_write: empty content" >&2
        return 1
    fi

    # Validate JSON before writing
    if command -v jq &>/dev/null; then
        if ! echo "$json_content" | jq empty 2>/dev/null; then
            echo "checkpoint_write: invalid JSON, refusing to write" >&2
            return 1
        fi
    fi

    mkdir -p "$_PFS_CHECKPOINT_DIR" 2>/dev/null || {
        echo "checkpoint_write: failed to create checkpoint dir" >&2
        return 1
    }

    local tmp_file
    tmp_file="${_PFS_CHECKPOINT_FILE}.tmp.$$"

    # Note: Not using trap EXIT for tmp cleanup because this is a library
    # function sourced by callers with their own EXIT traps (test-isolation,
    # hook scripts). Overwriting global traps is unsafe. Instead, explicit
    # cleanup on all exit paths. If the process is killed between jq write
    # and mv, the PID-suffixed tmp file remains but won't conflict.

    if echo "$json_content" | jq '.' > "$tmp_file" 2>/dev/null; then
        mv "$tmp_file" "$_PFS_CHECKPOINT_FILE" || {
            rm -f "$tmp_file" 2>/dev/null
            echo "checkpoint_write: mv failed" >&2
            return 1
        }
        return 0
    else
        rm -f "$tmp_file" 2>/dev/null
        echo "checkpoint_write: jq formatting failed" >&2
        return 1
    fi
}

# Initialize a phase entry with expected tasks from pathflow-config.json.
# Args: phase_id (e.g., "PF1")
# Reads tasks from config, populates expected array in checkpoint.
# Returns: 0 on success, 1 on error
checkpoint_init_phase() {
    local phase_id="$1"

    if [[ -z "$phase_id" ]]; then
        echo "checkpoint_init_phase: phase_id required" >&2
        return 1
    fi

    if ! command -v jq &>/dev/null; then
        echo "checkpoint_init_phase: jq required" >&2
        return 1
    fi

    if [[ ! -f "$_PFS_PATHFLOW_CONFIG" ]]; then
        echo "checkpoint_init_phase: pathflow-config.json not found" >&2
        return 1
    fi

    # Map phase_id (PF1) to config key (PF1-INIT, PF2-CONTEXT, etc.)
    local config_key
    config_key=$(jq -r --arg pfx "$phase_id" \
        '.phases | keys[] | select(startswith($pfx + "-"))' \
        "$_PFS_PATHFLOW_CONFIG" 2>/dev/null) || {
        echo "checkpoint_init_phase: failed to find phase $phase_id in config" >&2
        return 1
    }

    if [[ -z "$config_key" ]]; then
        echo "checkpoint_init_phase: phase $phase_id not found in config" >&2
        return 1
    fi

    # Extract required task IDs for this phase
    local expected_tasks
    expected_tasks=$(jq -c --arg key "$config_key" \
        '.phases[$key].required_tasks' \
        "$_PFS_PATHFLOW_CONFIG" 2>/dev/null) || {
        echo "checkpoint_init_phase: failed to read required_tasks for $config_key" >&2
        return 1
    }

    # Read current checkpoint
    local checkpoint
    checkpoint=$(checkpoint_read)

    # Check if phase already initialized
    local existing
    existing=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" '.[$pf] // empty' 2>/dev/null)
    if [[ -n "$existing" ]]; then
        # Phase already initialized, skip
        return 0
    fi

    # Initialize phase entry
    local updated
    updated=$(echo "$checkpoint" | jq -c --arg pf "$phase_id" --argjson exp "$expected_tasks" \
        '.[$pf] = {expected: $exp, registered: {}, completed: {}, skipped: {}, sentinel_created: false}' \
        2>/dev/null) || {
        echo "checkpoint_init_phase: failed to build phase entry" >&2
        return 1
    }

    checkpoint_write "$updated"
}

# Register a task in the checkpoint file.
# Args: task_id (e.g., "PF1-TSK-01")
# Extracts phase from task_id, initializes phase if needed, adds registration.
# Returns: 0 on success, 1 on error
checkpoint_register_task() {
    local task_id="$1"

    if [[ -z "$task_id" ]]; then
        echo "checkpoint_register_task: task_id required" >&2
        return 1
    fi

    if ! command -v jq &>/dev/null; then
        echo "checkpoint_register_task: jq required" >&2
        return 1
    fi

    # Extract phase from task_id (PF1-TSK-01 -> PF1)
    local phase_id
    if [[ "$task_id" =~ ^(PF[0-9]+)-TSK-[0-9]+$ ]]; then
        phase_id="${BASH_REMATCH[1]}"
    else
        echo "checkpoint_register_task: invalid task_id format: $task_id" >&2
        return 1
    fi

    # Initialize phase if not already done
    checkpoint_init_phase "$phase_id" || return 1

    # Read current checkpoint
    local checkpoint
    checkpoint=$(checkpoint_read)

    # Check if already registered (idempotent)
    local already
    already=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" --arg tid "$task_id" \
        '.[$pf].registered[$tid] // empty' 2>/dev/null)
    if [[ -n "$already" ]]; then
        return 0
    fi

    # Add registration with timestamp
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    local updated
    updated=$(echo "$checkpoint" | jq -c --arg pf "$phase_id" --arg tid "$task_id" --arg ts "$ts" \
        '.[$pf].registered[$tid] = $ts' 2>/dev/null) || {
        echo "checkpoint_register_task: failed to update checkpoint" >&2
        return 1
    }

    checkpoint_write "$updated"
}

# Mark a task as completed in the checkpoint file.
# Args: task_id (e.g., "PF1-TSK-01")
# Returns: 0 on success, 1 on error
# Stdout: "phase_complete" if all phase tasks are now done, empty otherwise
checkpoint_complete_task() {
    local task_id="$1"

    if [[ -z "$task_id" ]]; then
        echo "checkpoint_complete_task: task_id required" >&2
        return 1
    fi

    if ! command -v jq &>/dev/null; then
        echo "checkpoint_complete_task: jq required" >&2
        return 1
    fi

    # Extract phase from task_id
    local phase_id
    if [[ "$task_id" =~ ^(PF[0-9]+)-TSK-[0-9]+$ ]]; then
        phase_id="${BASH_REMATCH[1]}"
    else
        echo "checkpoint_complete_task: invalid task_id format: $task_id" >&2
        return 1
    fi

    # Read current checkpoint
    local checkpoint
    checkpoint=$(checkpoint_read)

    # Verify phase exists in checkpoint
    local phase_entry
    phase_entry=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" '.[$pf] // empty' 2>/dev/null)
    if [[ -z "$phase_entry" ]]; then
        echo "checkpoint_complete_task: phase $phase_id not initialized in checkpoint" >&2
        return 1
    fi

    # Check if sentinel already created (phase already done)
    local sentinel_done
    sentinel_done=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" \
        '.[$pf].sentinel_created' 2>/dev/null)
    if [[ "$sentinel_done" == "true" ]]; then
        return 0
    fi

    # Check if already completed (idempotent)
    local already
    already=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" --arg tid "$task_id" \
        '.[$pf].completed[$tid] // empty' 2>/dev/null)
    if [[ -n "$already" ]]; then
        # Already completed, still check phase status
        if checkpoint_is_phase_complete "$phase_id"; then
            echo "phase_complete"
        fi
        return 0
    fi

    # Add completion with timestamp
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    local updated
    updated=$(echo "$checkpoint" | jq -c --arg pf "$phase_id" --arg tid "$task_id" --arg ts "$ts" \
        '.[$pf].completed[$tid] = $ts' 2>/dev/null) || {
        echo "checkpoint_complete_task: failed to update checkpoint" >&2
        return 1
    }

    checkpoint_write "$updated" || return 1

    # Check if phase is now complete
    if checkpoint_is_phase_complete "$phase_id"; then
        echo "phase_complete"
    fi

    return 0
}

# Mark a task as skipped in the checkpoint file (conditional tasks not applicable).
# Args: task_id (e.g., "PF3-TSK-04")
# Returns: 0 on success, 1 on error
checkpoint_skip_task() {
    local task_id="$1"

    if [[ -z "$task_id" ]]; then
        echo "checkpoint_skip_task: task_id required" >&2
        return 1
    fi

    if ! command -v jq &>/dev/null; then
        echo "checkpoint_skip_task: jq required" >&2
        return 1
    fi

    # Extract phase from task_id
    local phase_id
    if [[ "$task_id" =~ ^(PF[0-9]+)-TSK-[0-9]+$ ]]; then
        phase_id="${BASH_REMATCH[1]}"
    else
        echo "checkpoint_skip_task: invalid task_id format: $task_id" >&2
        return 1
    fi

    # Read current checkpoint
    local checkpoint
    checkpoint=$(checkpoint_read)

    # Verify phase exists
    local phase_entry
    phase_entry=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" '.[$pf] // empty' 2>/dev/null)
    if [[ -z "$phase_entry" ]]; then
        echo "checkpoint_skip_task: phase $phase_id not initialized" >&2
        return 1
    fi

    # Check if already skipped (idempotent)
    local already
    already=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" --arg tid "$task_id" \
        '.[$pf].skipped[$tid] // empty' 2>/dev/null)
    if [[ -n "$already" ]]; then
        return 0
    fi

    # Add skip with timestamp
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    local updated
    updated=$(echo "$checkpoint" | jq -c --arg pf "$phase_id" --arg tid "$task_id" --arg ts "$ts" \
        '.[$pf].skipped[$tid] = $ts' 2>/dev/null) || {
        echo "checkpoint_skip_task: failed to update checkpoint" >&2
        return 1
    }

    checkpoint_write "$updated"
}

# Check if all expected tasks in a phase are completed or skipped.
# Args: phase_id (e.g., "PF1")
# Returns: 0 if all done, 1 if not
checkpoint_is_phase_complete() {
    local phase_id="$1"

    if [[ -z "$phase_id" ]]; then
        return 1
    fi

    if ! command -v jq &>/dev/null; then
        return 1
    fi

    # Read current checkpoint
    local checkpoint
    checkpoint=$(checkpoint_read)

    # Check phase exists
    local phase_entry
    phase_entry=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" '.[$pf] // empty' 2>/dev/null)
    if [[ -z "$phase_entry" ]]; then
        return 1
    fi

    # Check if sentinel already created
    local sentinel_done
    sentinel_done=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" \
        '.[$pf].sentinel_created' 2>/dev/null)
    if [[ "$sentinel_done" == "true" ]]; then
        return 0
    fi

    # Check each expected task is either completed or skipped
    local result
    result=$(echo "$checkpoint" | jq -r --arg pf "$phase_id" '
        .[$pf] as $phase |
        $phase.expected | length as $total |
        if $total == 0 then "incomplete"
        else
            [.[] | select(
                . as $tid |
                ($phase.completed[$tid] != null) or ($phase.skipped[$tid] != null)
            )] | length as $done |
            if $done >= $total then "complete"
            else "incomplete"
            end
        end
    ' 2>/dev/null)

    [[ "$result" == "complete" ]]
}

# Pre-initialize ALL phases from pathflow-config.json in the checkpoint file.
# This ensures PF1 (and all other phases) exist before any tasks register,
# preventing the lazy-init gap where early tasks are lost.
# Idempotent: only initializes phases that don't already exist.
# Returns: 0 on success, 1 on error
checkpoint_init_all_phases() {
    if ! command -v jq &>/dev/null; then
        echo "checkpoint_init_all_phases: jq required" >&2
        return 1
    fi

    if [[ ! -f "$_PFS_PATHFLOW_CONFIG" ]]; then
        echo "checkpoint_init_all_phases: pathflow-config.json not found" >&2
        return 1
    fi

    # Extract all phase IDs (PF1, PF2, ...) from config keys (PF1-INIT, PF2-CONTEXT, ...)
    local phase_keys
    phase_keys=$(jq -r '.phases | keys[]' "$_PFS_PATHFLOW_CONFIG" 2>/dev/null) || {
        echo "checkpoint_init_all_phases: failed to read phases from config" >&2
        return 1
    }

    if [[ -z "$phase_keys" ]]; then
        echo "checkpoint_init_all_phases: no phases found in config" >&2
        return 1
    fi

    local phase_key phase_id
    for phase_key in $phase_keys; do
        # Extract phase prefix: PF1-INIT -> PF1, PF2-CONTEXT -> PF2
        phase_id="${phase_key%%-*}"
        checkpoint_init_phase "$phase_id" || {
            echo "checkpoint_init_all_phases: failed to init phase $phase_id" >&2
            return 1
        }
    done

    return 0
}

# =============================================================================
# LIBRARY GUARD
# =============================================================================

if [[ "${BASH_SOURCE[0]:-}" == "${0:-}" ]]; then
    echo "Error: This is a library file. Source it instead of executing." >&2
    echo "Usage: source \"$(basename "$0")\"" >&2
    exit 1
fi
