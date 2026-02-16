#!/usr/bin/env bash
# Purpose:   SessionStart hook for session initialization, cleanup, and PathFlow flag
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-init.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   1.1.0
#
# This hook consolidates session setup into a single script:
#   Section 1: Read stdin and generate session ID
#   Section 2: Setup (REPO_ROOT, libraries)
#   Section 3: Directory creation
#   Section 4: Stale session cleanup
#   Section 5: Sentinel cleanup (expires-based)
#   Section 6: Active task context expiry
#   Section 7: PathFlow flag creation
#   Section 8: Session metadata
#   Section 9: Stale team detection
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Session initialized successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.1.0"

# =============================================================================
# SECTION 1: STDIN READING AND SESSION ID
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

# Generate session ID if not already set (via stdin or environment)
if [[ -z "${CODEFLOW_SESSION_ID:-}" ]]; then
    TIMESTAMP=$(date +%s%N | cut -c1-13)
    RANDOM_PART=$(head -c 6 /dev/urandom | od -An -tx1 | tr -d ' \n')
    CODEFLOW_SESSION_ID="ses-${TIMESTAMP}${RANDOM_PART}"
fi
export CODEFLOW_SESSION_ID

# =============================================================================
# SECTION 2: SETUP
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
# SECTION 3: DIRECTORY CREATION
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

# =============================================================================
# SECTION 4: STALE SESSION CLEANUP
# =============================================================================

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
# SECTION 5: SENTINEL CLEANUP (expires-based)
# =============================================================================

# Clean up expired sentinels using sentinel library (checks JSON expires field)
if [[ -n "$SENTINEL_LIB_AVAILABLE" ]]; then
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
# SECTION 6: ACTIVE TASK CONTEXT EXPIRY
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
# SECTION 7: PATHFLOW FLAG CREATION
# =============================================================================

# Source PathFlow state library for flag creation
_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ -f "$_PFS_LIB" ]]; then
    # shellcheck source=/dev/null
    source "$_PFS_LIB"

    # Create flag with initial metadata
    # team_name is empty at init (updated when TeamCreate is called)
    # tracking_level starts as "pending" (updated at PF3-CLASSIFY)
    create_pathflow_flag "$CODEFLOW_SESSION_ID" ""
fi
# If library missing, skip flag creation (graceful degradation)

# =============================================================================
# SECTION 8: SESSION METADATA
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
# SECTION 9: STALE TEAM DETECTION
# =============================================================================

# Check for stale team configs (teams with dead tmux panes).
# This detects context overflow recovery situations where teammates died
# but team config still references them.
# Advisory only (exit 0) -- outputs warning to stderr for lead awareness.

_TEAMS_DIR="${HOME}/.claude/teams"
if [[ -d "$_TEAMS_DIR" ]] && command -v jq &>/dev/null; then
    for _team_config in "$_TEAMS_DIR"/*/config.json; do
        [[ -f "$_team_config" ]] || continue

        _team_dir=$(dirname "$_team_config")
        _team_name=$(basename "$_team_dir")
        _stale_count=0
        _total_count=0

        # Parse member pane IDs from team config
        _member_count=$(jq -r '.members | length // 0' "$_team_config" 2>/dev/null) || continue
        # Validate member_count is a number
        case "$_member_count" in
            ''|*[!0-9]*) continue ;;
        esac

        if [[ "$_member_count" -eq 0 ]]; then
            continue
        fi

        _idx=0
        while [[ "$_idx" -lt "$_member_count" ]]; do
            _total_count=$(( _total_count + 1 ))
            _pane_id=$(jq -r ".members[$_idx].tmuxPaneId // empty" "$_team_config" 2>/dev/null) || true

            if [[ -n "$_pane_id" ]]; then
                # Check if tmux pane is alive
                if ! tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -q "^${_pane_id}$"; then
                    _stale_count=$(( _stale_count + 1 ))
                fi
            else
                _stale_count=$(( _stale_count + 1 ))
            fi

            _idx=$(( _idx + 1 ))
        done

        if [[ "$_stale_count" -gt 0 ]]; then
            echo "WARNING: STALE TEAM DETECTED: Team '${_team_name}' has ${_stale_count}/${_total_count} members with dead panes." >&2
            echo "Recovery needed: See CLAUDE.md Section 11 (Context Overflow Recovery)" >&2
        fi
    done
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
