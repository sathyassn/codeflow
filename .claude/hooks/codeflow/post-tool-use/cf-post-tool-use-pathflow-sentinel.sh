#!/usr/bin/env bash
# Purpose:   PostToolUse hook for automatic PathFlow stage sentinel creation
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh
# Hook Type: PostToolUse
# Matcher:   SendMessage (configured in settings.json)
# Usage:     Called by Claude Code PostToolUse hook system
# Platform:  macOS/Linux
#
# This hook:
#   - Detects stage-completion events from SendMessage tool calls
#   - Creates stage sentinel files to track PathFlow work stage progression
#   - Validates stage ordering and blocks out-of-order completions (exit 2)
#   - Enables pathflow-gate enforcement without requiring JSONL writes
#   - Creates pathflow-team.json on TeamCreate (records lead PID for teammate detection)
#   - Updates pathflow-team.json on Task (records teammate spawns)
#
# Note: Phase sentinels (pf-1, pf-2, pf-3, pf-6) are created by the
#       phase checkpoint hooks (session-start, post-tool-use checkpoint hooks).
#
# Sentinel detection:
#   tool_name=SendMessage, STAGE-COMPLETE: WS-* -> pathflow-ws-{stage}
#
# Stage ordering validation (hard gates — blocks out-of-order, exit 2):
#   ws-rev requires prior primary stage (ws-dev/ws-plan/ws-docs/ws-test)
#   ws-qa requires prior ws-dev or ws-test
#
# Idempotent: touch on existing file is a no-op.
# Early exit: If is_pathflow_active() returns false, exit 0.
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   0 - Operation allowed (sentinel created or non-sentinel tool call)
#   2 - Stage ordering violation (sentinel NOT created, out-of-order blocked)

set -euo pipefail

# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
TOOL_INPUT="${TOOL_INPUT:-}"

if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _tn=$(echo "$_HOOK_STDIN" | jq -r '.tool_name // empty' 2>/dev/null)
        [[ -n "$_tn" ]] && TOOL_NAME="$_tn"
        _ti=$(echo "$_HOOK_STDIN" | jq -c '.tool_input // empty' 2>/dev/null)
        [[ -n "$_ti" ]] && [[ "$_ti" != "null" ]] && TOOL_INPUT="$_ti"
    fi
fi

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT

# =============================================================================
# SESSION ID: Source from env file (shared across all teammates)
# =============================================================================
# The env file contains the real session ID written at session start.
# Stdin .session_id is a per-agent UUID from Claude Code, NOT the shared session.
# Using stdin would create sentinels under the wrong directory.
# TODO(go-cli): Session ID sourcing unchanged when CLI arrives

_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
else
    # Fallback: read from hook input (degraded mode — per-agent UUID)
    if [[ -n "${_HOOK_STDIN:-}" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

echo "PostToolUse[sentinel]: hook start, tool=$TOOL_NAME, session=$CODEFLOW_SESSION_ID" >&2

# =============================================================================
# PATHFLOW CHECK — early exit if not active
# =============================================================================

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
export LIB_DIR

if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
else
    echo "PostToolUse[sentinel]: security-lib.sh not found, skipping" >&2
    exit 0
fi

if ! is_pathflow_active 2>/dev/null; then
    echo "PostToolUse[sentinel]: pathflow not active, skipping" >&2
    exit 0
fi

# =============================================================================
# SOURCE PATHFLOW STATE LIBRARY
# =============================================================================

_PFS_LIB="$REPO_ROOT/.codeflow/scripts/state/cf-pathflow-state.sh"
if [[ ! -f "$_PFS_LIB" ]]; then
    echo "PostToolUse[sentinel]: pathflow-state lib not found at $_PFS_LIB, skipping" >&2
    exit 0
fi
# shellcheck source=/dev/null
if ! source "$_PFS_LIB"; then
    echo "PostToolUse[sentinel]: failed to source pathflow-state lib" >&2
    exit 0
fi

# =============================================================================
# SENTINEL DETECTION — Stage completion via SendMessage
# =============================================================================

if [[ "$TOOL_NAME" == "SendMessage" ]]; then
    if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
        _content=$(echo "$TOOL_INPUT" | jq -r '.content // empty' 2>/dev/null) || true
        if [[ -n "${_content:-}" ]]; then
            # Normalize for case-insensitive matching
            _content_normalized=$(echo "$_content" | tr '[:lower:]' '[:upper:]' | tr -s '[:space:]' ' ')
            # Match STAGE-COMPLETE: WS-{STAGE} pattern
            if [[ "$_content_normalized" =~ STAGE-COMPLETE:\ WS-(DEV|REV|QA|TEST|PLAN|DOCS) ]]; then
                _stage="${BASH_REMATCH[1]}"
                # Convert to lowercase for sentinel name
                _stage_lower=$(echo "$_stage" | tr '[:upper:]' '[:lower:]')

                # Stage ordering validation (hard blocks — checked BEFORE sentinel creation)
                # Prevents out-of-order sentinels that would allow downstream gates to pass prematurely
                case "$_stage_lower" in
                    rev)
                        # WS-REV requires a prior primary stage (ws-dev, ws-plan, ws-docs, ws-test)
                        _has_primary=""
                        for _ps in dev plan docs test; do
                            if has_sentinel "ws-${_ps}"; then
                                _has_primary="true"
                                break
                            fi
                        done
                        if [[ -z "$_has_primary" ]]; then
                            echo "PostToolUse[sentinel]: BLOCKED: ws-rev requires prior primary stage (ws-dev/ws-plan/ws-docs/ws-test)" >&2
                            exit 2
                        fi
                        ;;
                    qa)
                        # WS-QA requires prior WS-DEV or WS-TEST
                        if ! has_sentinel "ws-dev" && ! has_sentinel "ws-test"; then
                            echo "PostToolUse[sentinel]: BLOCKED: ws-qa requires prior ws-dev or ws-test sentinel" >&2
                            exit 2
                        fi
                        ;;
                esac

                if create_sentinel "ws-${_stage_lower}"; then
                    echo "PostToolUse[sentinel]: created sentinel ws-${_stage_lower}" >&2
                else
                    echo "PostToolUse[sentinel]: FAILED to create sentinel ws-${_stage_lower}" >&2
                fi
            fi
        fi
    fi
fi

# =============================================================================
# PATHFLOW-TEAM.JSON — TeamCreate handler (records lead PID)
# =============================================================================

if [[ "$TOOL_NAME" == "TeamCreate" ]]; then
    if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
        _team_name=$(echo "$TOOL_INPUT" | jq -r '.team_name // empty' 2>/dev/null) || true
        if [[ -n "${_team_name:-}" ]]; then
            # Read lead's Claude UUID from team config
            _team_cfg="${HOME}/.claude/teams/${_team_name}/config.json"
            _lead_uuid=""
            if [[ -f "$_team_cfg" ]]; then
                _lead_uuid=$(jq -r '.leadSessionId // empty' "$_team_cfg" 2>/dev/null) || true
            fi

            # Get lead PID ($PPID = Claude Code process that spawned this hook)
            _lead_pid="${PPID:-0}"

            # Write pathflow-team.json atomically (tmp + mv)
            _pf_dir="$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID/pathflow"
            if [[ -d "$_pf_dir" ]]; then
                _team_file="$_pf_dir/pathflow-team.json"
                _tmp_team=$(mktemp "${_team_file}.XXXXXX" 2>/dev/null) || true
                if [[ -n "${_tmp_team:-}" ]]; then
                    jq -nc \
                        --arg team_name "$_team_name" \
                        --arg lead_claude_uuid "${_lead_uuid:-unknown}" \
                        --argjson lead_pid "$_lead_pid" \
                        --arg codeflow_session_id "$CODEFLOW_SESSION_ID" \
                        --argjson teammate_spawned false \
                        --arg created_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
                        '{team_name: $team_name, lead_claude_uuid: $lead_claude_uuid, lead_pid: $lead_pid, codeflow_session_id: $codeflow_session_id, teammate_spawned: $teammate_spawned, created_at: $created_at, last_spawn_name: null}' \
                        > "$_tmp_team" 2>/dev/null || true
                    if ! mv "$_tmp_team" "$_team_file" 2>/dev/null; then
                        rm -f "$_tmp_team" 2>/dev/null || true
                    fi
                    echo "PostToolUse[sentinel]: created pathflow-team.json for team=$_team_name, pid=$_lead_pid" >&2
                fi
            fi
        fi
    fi
fi

# =============================================================================
# TEAMDELETE HANDLER — Remove pathflow-active flag after successful TeamDelete
# =============================================================================
# PostToolUse fires AFTER TeamDelete succeeds. Safe to remove the flag now.
# SessionEnd hook will then proceed with full cleanup (flag absent → cleanup runs).

if [[ "$TOOL_NAME" == "TeamDelete" ]]; then
    _flag_file="$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID/pathflow/is-pathflow-active"
    if [[ -f "$_flag_file" ]]; then
        rm -f "$_flag_file" 2>/dev/null || true
        echo "PostToolUse[sentinel]: TeamDelete succeeded — pathflow-active flag removed" >&2
    else
        echo "PostToolUse[sentinel]: TeamDelete — flag already absent (no-op)" >&2
    fi
fi

# =============================================================================
# PATHFLOW-TEAM.JSON — Task handler (records teammate spawn)
# =============================================================================

if [[ "$TOOL_NAME" == "Task" ]]; then
    if [[ -n "$TOOL_INPUT" ]] && command -v jq &>/dev/null; then
        _task_team=$(echo "$TOOL_INPUT" | jq -r '.team_name // empty' 2>/dev/null) || true
        if [[ -n "${_task_team:-}" ]]; then
            # This is a teammate spawn (has team_name in tool_input)
            _spawn_name=$(echo "$TOOL_INPUT" | jq -r '.name // empty' 2>/dev/null) || true

            _pf_dir="$REPO_ROOT/.state/session/$CODEFLOW_SESSION_ID/pathflow"
            _team_file="$_pf_dir/pathflow-team.json"
            if [[ -f "$_team_file" ]]; then
                _tmp_team=$(mktemp "${_team_file}.XXXXXX" 2>/dev/null) || true
                if [[ -n "${_tmp_team:-}" ]]; then
                    jq \
                        --argjson teammate_spawned true \
                        --arg last_spawn_name "${_spawn_name:-unknown}" \
                        '.teammate_spawned = $teammate_spawned | .last_spawn_name = $last_spawn_name' \
                        "$_team_file" > "$_tmp_team" 2>/dev/null || true
                    if ! mv "$_tmp_team" "$_team_file" 2>/dev/null; then
                        rm -f "$_tmp_team" 2>/dev/null || true
                    fi
                    echo "PostToolUse[sentinel]: updated pathflow-team.json teammate_spawned=true, name=${_spawn_name:-unknown}" >&2
                fi
            fi
        fi
    fi
fi

# =============================================================================
# SUCCESS
# =============================================================================

exit 0
