#!/usr/bin/env bash
# Purpose:   SessionStart hook for session initialization, cleanup, and PathFlow flag
# Location:  .claude/hooks/codeflow/session-start/cf-session-start-init.sh
# Hook Type: SessionStart
# Usage:     Called by Claude Code at session start
# Platform:  macOS/Linux
# Version:   1.4.0
#
# This hook consolidates session setup into a single script:
#   Section 1: Read stdin and generate session ID
#   Section 2: Setup (REPO_ROOT, libraries)
#   Section 3: Directory creation
#   Section 4: Stale session detection (warning-only)
#   Section 5: Sentinel cleanup (expires-based)
#   Section 6: Active task context expiry
#   Section 7: PathFlow flag creation (with guard)
#   Section 7b: Sentinel recovery (context overflow)
#   Section 7c: Checkpoint pre-initialization
#   Section 8: Session metadata
#   Section 9: Stale team detection
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Session initialized successfully (always exits 0)

set -euo pipefail

# shellcheck disable=SC2034  # VERSION used for identification
readonly VERSION="1.4.0"

# =============================================================================
# SECTION 1: STDIN READING AND SESSION ID
# =============================================================================

# Read hook stdin for Claude Code metadata
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
fi

# Store Claude Code's per-agent UUID as metadata only (NOT used as session ID).
# Each agent context (lead + each teammate) receives a unique UUID from Claude Code.
# Using it as session ID causes sentinel lookup failures across teammates.
_CLAUDE_UUID=""
# Session source: startup (fresh claude command), resume (/resume), clear (/clear), compact (auto-compaction)
_SESSION_SOURCE="unknown"
if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
    _CLAUDE_UUID=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null) || true
    _SESSION_SOURCE=$(echo "$_HOOK_STDIN" | jq -r '.source // "unknown"' 2>/dev/null) || _SESSION_SOURCE="unknown"
    [[ -z "$_SESSION_SOURCE" ]] && _SESSION_SOURCE="unknown"
fi

# =============================================================================
# SECTION 2: SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# TODO(go-cli): Replace ses-{ts}{hex} with session-{ulid} via Go CLI
# The Go CLI will generate the ID and write to the same env file
# This shell fallback becomes dead code once CLI handles session init

# CodeFlow-native session ID: shared across all teammates via env file.
# The env file (.state/runtime/codeflow-env.sh) is the stable interface.
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
    # Teammate joining existing session — ID already set
fi

# Generate session ID if not already set (via env file or environment)
if [[ -z "${CODEFLOW_SESSION_ID:-}" ]]; then
    # Lead starting new session — generate CodeFlow-native ID
    TIMESTAMP=$(date +%s%N | cut -c1-13)
    RANDOM_PART=$(head -c 6 /dev/urandom | od -An -tx1 | tr -d ' \n')
    CODEFLOW_SESSION_ID="ses-${TIMESTAMP}${RANDOM_PART}"

    # Write env file atomically (tmp + mv) for cross-teammate sharing
    mkdir -p "$(dirname "$_env_file")"
    _tmp_env=$(mktemp "${_env_file}.XXXXXX")
    cat > "$_tmp_env" <<ENVEOF
export CODEFLOW_SESSION_ID='${CODEFLOW_SESSION_ID}'
export CF_PROJECT_ROOT='$(basename "$REPO_ROOT")'
ENVEOF
    mv "$_tmp_env" "$_env_file"
fi
export CODEFLOW_SESSION_ID
CF_PROJECT_ROOT="${CF_PROJECT_ROOT:-$(basename "$REPO_ROOT")}"
export CF_PROJECT_ROOT

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
mkdir -p "$REPO_ROOT/.state/runtime" 2>/dev/null || true

# Create sentinel directories (session-scoped)
mkdir -p "$SENTINEL_DIR" 2>/dev/null || true
mkdir -p "$REPO_ROOT/.state/sentinels/pathflow/$CODEFLOW_SESSION_ID" 2>/dev/null || true

# Create session state directory
mkdir -p "$SESSION_STATE_DIR" 2>/dev/null || true
mkdir -p "$SHARED_STATE_DIR" 2>/dev/null || true

# =============================================================================
# SECTION 4: STALE SESSION DETECTION (warning-only)
# =============================================================================
# Design decision: Auto-cleanup at startup was REJECTED because /resume requires
# going through startup first — auto-cleanup would destroy state before the user
# can type /resume. Instead: warn at startup + manual /cf-cleanup --sessions.
# See: .codeflow/docs/analysis/stale-session-cleanup-design.md

_stale_session_warnings=()

if [[ -d "$SHARED_STATE_DIR" ]]; then
    for _session_dir in "$SHARED_STATE_DIR"/ses-*; do
        [[ -d "$_session_dir" ]] || continue
        _sid=$(basename "$_session_dir")

        # Skip current session
        [[ "$_sid" == "$CODEFLOW_SESSION_ID" ]] && continue

        # Only check sessions with pathflow-active flag (those that didn't clean up)
        [[ -f "$_session_dir/pathflow/is-pathflow-active" ]] || continue

        # Check if any tmux panes from this session's team are alive
        # Read team_name from flag file if possible
        _is_stale="true"
        if command -v jq &>/dev/null && [[ -f "$_session_dir/pathflow/is-pathflow-active" ]]; then
            _flag_team=$(jq -r '.team_name // empty' "$_session_dir/pathflow/is-pathflow-active" 2>/dev/null) || true
            if [[ -n "$_flag_team" ]]; then
                _team_cfg="${HOME}/.claude/teams/${_flag_team}/config.json"
                if [[ -f "$_team_cfg" ]]; then
                    # Check if any member pane is alive
                    _mc=$(jq -r '.members | length // 0' "$_team_cfg" 2>/dev/null) || _mc=0
                    case "$_mc" in ''|*[!0-9]*) _mc=0 ;; esac
                    _mi=0
                    while [[ "$_mi" -lt "$_mc" ]]; do
                        _pid=$(jq -r ".members[$_mi].tmuxPaneId // empty" "$_team_cfg" 2>/dev/null) || true
                        if [[ -n "$_pid" ]] && tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -q "^${_pid}$"; then
                            _is_stale="false"
                            break
                        fi
                        _mi=$(( _mi + 1 ))
                    done
                fi
            fi
        fi

        if [[ "$_is_stale" == "true" ]]; then
            _stale_session_warnings+=("$_sid")
        fi
    done
fi

if [[ ${#_stale_session_warnings[@]} -gt 0 ]]; then
    echo "WARNING: STALE SESSIONS DETECTED:" >&2
    for _sw in "${_stale_session_warnings[@]}"; do
        echo "  - $_sw (pathflow-active flag set, no live tmux panes)" >&2
    done
    echo "Run '/cf-cleanup --sessions' to clean up stale session state." >&2
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
_IS_RECOVERY="false"
if [[ -f "$_PFS_LIB" ]]; then
    # shellcheck source=/dev/null
    source "$_PFS_LIB"

    # Guard: only create flag if it doesn't already exist
    # Prevents teammate spawns from resetting tracking_level to "pending"
    # Track whether flag pre-existed for Section 7b recovery decision
    if [[ -f "$SESSION_STATE_DIR/pathflow/is-pathflow-active" ]]; then
        echo "SessionStart: PathFlow flag already exists, preserving" >&2
        _IS_RECOVERY="true"
    else
        # Create flag with initial metadata
        # team_name is empty at init (updated when TeamCreate is called)
        # tracking_level starts as "pending" (updated at PF3-CLASSIFY)
        create_pathflow_flag "$CODEFLOW_SESSION_ID" ""
    fi
fi
# If library missing, skip flag creation (graceful degradation)

# =============================================================================
# SECTION 7b: SENTINEL RECOVERY
# =============================================================================
# If pathflow flag PRE-EXISTED this session start (genuine recovery from context
# overflow or teammate join) and sentinel dir is empty, recreate pf-1, pf-2, pf-3
# sentinels so the pathflow-gate hook doesn't block Edit/Write operations.
# Fresh sessions (_IS_RECOVERY=false) skip this to preserve pf-3 enforcement.

_PF_SENTINEL_DIR="$REPO_ROOT/.state/sentinels/pathflow/$CODEFLOW_SESSION_ID"
if [[ "$_IS_RECOVERY" == "true" ]] && [[ -d "$_PF_SENTINEL_DIR" ]]; then
    # Check if sentinel dir has any pathflow-* files
    _sentinel_count=0
    for _sf in "$_PF_SENTINEL_DIR"/pathflow-*; do
        [[ -f "$_sf" ]] && _sentinel_count=$(( _sentinel_count + 1 ))
    done

    if [[ "$_sentinel_count" -eq 0 ]] && type create_sentinel &>/dev/null; then
        create_sentinel "pf-1"
        create_sentinel "pf-2"
        create_sentinel "pf-3"
        echo "SessionStart: Recovered PathFlow sentinels (pf-1, pf-2, pf-3)" >&2
    fi
fi

# =============================================================================
# SECTION 7c: CHECKPOINT PRE-INITIALIZATION
# =============================================================================
# Pre-initialize the checkpoint file with ALL phases from pathflow-config.json.
# This fixes the PF1 gap: without this, PF1 tasks register before the checkpoint
# file exists (because checkpoint_init_phase is called lazily on first register).
# By calling checkpoint_init_all_phases here, all phases exist before any
# TaskCreate hook fires.

if type checkpoint_init_all_phases &>/dev/null; then
    checkpoint_init_all_phases 2>/dev/null || {
        echo "SessionStart: checkpoint_init_all_phases failed (non-fatal)" >&2
    }
fi

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
        --arg claude_uuid "${_CLAUDE_UUID:-unknown}" \
        --arg source "$_SESSION_SOURCE" \
        --arg started_at "$(date -u +%Y-%m-%dT%H:%M:%S.000Z)" \
        --argjson started_epoch "$(date +%s)" \
        --arg repo_root "$REPO_ROOT" \
        --arg git_branch "$GIT_BRANCH" \
        --arg git_commit "$GIT_COMMIT" \
        --arg user "${USER:-unknown}" \
        '{session_id: $session_id, claude_uuid: $claude_uuid, source: $source, started_at: $started_at, started_epoch: $started_epoch, repo_root: $repo_root, git_branch: $git_branch, git_commit: $git_commit, user: $user}' \
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
