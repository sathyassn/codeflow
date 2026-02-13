#!/usr/bin/env bash
# Purpose:   SessionStart hook for session initialization and cleanup
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-cleanup.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   2.0.0
#
# This hook:
#   - Reads session_id from stdin (Claude Code provides JSON on SessionStart)
#   - Generates unique session ID (if not provided via stdin or env)
#   - Creates session directories
#   - Initializes sentinel directory
#   - Cleans up expired sentinels using sentinel library (checks expires field)
#   - Removes stale PathFlow sentinels from previous sessions
#   - Removes stale is-pathflow-active flag (crash recovery)
#   - Cleans memory-progress state files
#   - Checks active-task.json expiry
#   - Writes session metadata
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Session initialized successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="2.0.0"

# =============================================================================
# STDIN READING (Claude Code provides JSON with session_id, cwd, permission_mode)
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

# =============================================================================
# SESSION ID GENERATION
# =============================================================================

# Generate session ID if not already set (via stdin or environment)
if [[ -z "${CODEFLOW_SESSION_ID:-}" ]]; then
    # Generate ULID-like session ID
    TIMESTAMP=$(date +%s%N | cut -c1-13)
    RANDOM_PART=$(head -c 6 /dev/urandom | od -An -tx1 | tr -d ' \n')
    CODEFLOW_SESSION_ID="ses-${TIMESTAMP}${RANDOM_PART}"
fi
export CODEFLOW_SESSION_ID

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# Source sentinel library for proper expires-based cleanup
SENTINEL_LIB="$REPO_ROOT/.codeflow/scripts/security/sentinel/cf-sentinel.sh"
SENTINEL_LIB_AVAILABLE=""
if [[ -f "$SENTINEL_LIB" ]]; then
    # shellcheck source=/dev/null
    source "$SENTINEL_LIB"
    SENTINEL_LIB_AVAILABLE="true"
fi

# Source work-state library for active task functions
# shellcheck source=/dev/null
source "$REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh" 2>/dev/null || true

# Read sentinel directory from config (fallback to default)
SENTINEL_DIR="${SENTINEL_DIR:-$REPO_ROOT/.state/sentinels/skill/$CODEFLOW_SESSION_ID}"
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    _sd=$(jq -r '.sentinel.directory // ".state/sentinels/skill"' "$CONFIG" 2>/dev/null || echo ".state/sentinels/skill")
    if [[ "$_sd" != /* ]]; then _sd="$REPO_ROOT/$_sd"; fi
    SENTINEL_DIR="${_sd}/$CODEFLOW_SESSION_ID"
fi

SESSION_STATE_DIR="$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID"
SHARED_STATE_DIR="$REPO_ROOT/.state/session"


# =============================================================================
# DIRECTORY CREATION
# =============================================================================

# Create session directories
mkdir -p "$REPO_ROOT/.state/logs/sessions" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/logs/security" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/db" 2>/dev/null || true

# Create sentinel directories (session-scoped)
mkdir -p "$SENTINEL_DIR" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$CODEFLOW_SESSION_ID" 2>/dev/null || true

# Create session state directory
mkdir -p "$SESSION_STATE_DIR" 2>/dev/null || true
mkdir -p "$SHARED_STATE_DIR" 2>/dev/null || true

# Stale session cleanup (remove session dirs older than 24 hours)
if [[ -d "$REPO_ROOT/.state/sentinels/skill" ]]; then
    find "$REPO_ROOT/.state/sentinels/skill" -mindepth 1 -maxdepth 1 -type d -mtime +1 -exec rm -rf {} \; 2>/dev/null || true
fi
if [[ -d "$REPO_ROOT/.state/sentinels/pathflow" ]]; then
    find "$REPO_ROOT/.state/sentinels/pathflow" -mindepth 1 -maxdepth 1 -type d -mtime +1 -exec rm -rf {} \; 2>/dev/null || true
fi
if [[ -d "$REPO_ROOT/.state/session" ]]; then
    find "$REPO_ROOT/.state/session" -mindepth 1 -maxdepth 1 -type d -mtime +1 -exec rm -rf {} \; 2>/dev/null || true
fi

# =============================================================================
# SENTINEL CLEANUP (using sentinel library expires field, NOT mtime)
# =============================================================================

# Clean up expired sentinels using sentinel library (checks JSON expires field)
if [[ -n "$SENTINEL_LIB_AVAILABLE" ]]; then
    # Use sentinel library's proper expires-based cleanup
    sentinel_cleanup_expired > /dev/null 2>&1 || true
else
    # Fallback: manual expires-based cleanup if sentinel library unavailable
    if [[ -d "$SENTINEL_DIR" ]] && command -v jq &>/dev/null; then
        _now=$(date +%s)
        for _sf in "$SENTINEL_DIR"/*.json; do
            [[ -f "$_sf" ]] || continue
            _expires=$(jq -r '.expires // 0' "$_sf" 2>/dev/null) || continue
            # Validate expires is a number (bash 3.2 compatible)
            case "$_expires" in
                ''|*[!0-9]*) rm -f "$_sf" 2>/dev/null || true; continue ;;
            esac
            if [[ "$_now" -ge "$_expires" ]]; then
                rm -f "$_sf" 2>/dev/null || true
            fi
        done
    fi
fi

# =============================================================================
# PATHFLOW STALE STATE CLEANUP
# =============================================================================

# Stale PathFlow sentinels cleaned by stale session cleanup above
# (Each session has its own pathflow dir that gets removed after 24h)

# Stale PathFlow flags cleaned by stale session cleanup above
# (Each session's flag is in its own session dir)

# =============================================================================
# MEMORY PROGRESS CLEANUP
# =============================================================================

# Stale memory progress files cleaned by stale session cleanup above
# (Each session's memory-progress is in its own session dir)

# =============================================================================
# ACTIVE TASK CONTEXT EXPIRY CHECK
# =============================================================================

# V4: "Remove task context if expired"
# Preserve in_progress tasks, remove completed/expired/stale ones
if [[ -f "$(get_active_task_file)" ]]; then
    if ! is_task_active; then
        # Task is completed, expired, or has unknown status - remove
        clear_active_task
    elif is_active_task_stale; then
        # Task is stale (>24h old) - remove
        clear_active_task
    fi
fi

# =============================================================================
# SESSION METADATA
# =============================================================================

SESSION_META_FILE="$REPO_ROOT/.state/logs/sessions/session-${CODEFLOW_SESSION_ID}.meta"

# Get git info
GIT_BRANCH=$(git -C "$REPO_ROOT" branch --show-current 2>/dev/null || echo "unknown")
GIT_COMMIT=$(git -C "$REPO_ROOT" rev-parse --short HEAD 2>/dev/null || echo "unknown")

# Write session metadata (include started_epoch for reliable duration calculation)
if command -v jq &>/dev/null; then
    jq -nc \
        --arg session_id "$CODEFLOW_SESSION_ID" \
        --arg started_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --argjson started_epoch "$(date +%s)" \
        --arg repo_root "$REPO_ROOT" \
        --arg git_branch "$GIT_BRANCH" \
        --arg git_commit "$GIT_COMMIT" \
        --arg user "${USER:-unknown}" \
        '{session_id: $session_id, started_at: $started_at, started_epoch: $started_epoch, repo_root: $repo_root, git_branch: $git_branch, git_commit: $git_commit, user: $user}' \
        > "$SESSION_META_FILE" 2>/dev/null || true
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
