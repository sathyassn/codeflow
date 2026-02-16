#!/usr/bin/env bash
# context-lib.sh — Context detection and mode library for CodeFlow hooks
#
# Purpose: Detect execution context (sub-agent, forked skill) and PathFlow mode.
#          Provides mode queries (is_pathflow_active, get_pathflow_setting) and
#          sub-context detection for hooks to exempt isolated contexts.
#
# Location: .codeflow/scripts/security/lib/context-lib.sh
# Compatibility: Bash 3.2+ (macOS default)
#
# Functions:
#   _find_progress_entry()       - Internal: search transcript for progress entries
#   is_sub_agent()               - Detect Task sub-agent context via agent_progress
#   is_forked_context_skill()    - Detect forked skill context via skill_progress
#   is_sub_context()             - Combined check (either of the above)
#   is_pathflow_active()         - Detect if PathFlow mode is active (flag file)
#   get_pathflow_setting()       - Read _codeflow.agent_teams from settings.json
#
# Usage:
#   source "$REPO_ROOT/.codeflow/scripts/security/lib/context-lib.sh"
#   if is_sub_context "$TRANSCRIPT_PATH" "$TOOL_USE_ID"; then
#       # Running in isolated context — exempt from restrictions
#       exit 0
#   fi
#
# Known Limitations:
#   - Agent Teams teammates CANNOT be detected. In-process teammates share
#     the same session_id and transcript_path as the team lead, and produce
#     no distinguishing transcript entries before PreToolUse hooks fire.
#     See: GitHub issue #6885 (requesting agentName/teamName in hook stdin)
#   - Workaround: Teammates can spawn their own Task sub-agents to read
#     large files (sub-agents ARE detectable via agent_progress entries).

set -euo pipefail

# Guard against double-sourcing (must come before any readonly declarations)
# shellcheck disable=SC2317  # return works when sourced; true is fallback when executed
if [[ "${_CONTEXT_LIB_LOADED:-}" = "true" ]]; then
    return 0 2>/dev/null || true
fi
readonly _CONTEXT_LIB_LOADED="true"

# shellcheck disable=SC2034  # CONTEXT_LIB_VERSION available for version queries
readonly CONTEXT_LIB_VERSION="1.1.0"

# --- Internal Helper ---

# Search transcript for progress entries matching a tool_use_id.
# Sets global variables: _PROGRESS_TOP_TYPE, _PROGRESS_DATA_TYPE, _PROGRESS_CONTEXT_ID
# Returns 0 if a progress entry is found, 1 otherwise.
_find_progress_entry() {
    local transcript_path="$1"
    local tool_use_id="$2"

    if [[ -z "$transcript_path" ]] || [[ -z "$tool_use_id" ]]; then
        return 1
    fi
    if [[ ! -f "$transcript_path" ]]; then
        return 1
    fi
    if ! command -v jq &>/dev/null; then
        return 1
    fi

    local tmpfile
    tmpfile=$(mktemp "${TMPDIR:-/tmp/claude}/context-detect-XXXXXX.json")

    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        echo "$line" > "$tmpfile"

        local top_type data_type agent_id skill_id
        top_type=$(jq -r '.type // empty' "$tmpfile" 2>/dev/null)
        data_type=$(jq -r '.data.type // empty' "$tmpfile" 2>/dev/null)
        agent_id=$(jq -r '.data.agentId // empty' "$tmpfile" 2>/dev/null)
        skill_id=$(jq -r '.data.skillId // empty' "$tmpfile" 2>/dev/null)

        if [[ "$top_type" = "progress" ]]; then
            _PROGRESS_TOP_TYPE="$top_type"
            _PROGRESS_DATA_TYPE="$data_type"
            if [[ -n "$agent_id" ]]; then
                _PROGRESS_CONTEXT_ID="$agent_id"
            elif [[ -n "$skill_id" ]]; then
                _PROGRESS_CONTEXT_ID="$skill_id"
            else
                _PROGRESS_CONTEXT_ID=""
            fi
            rm -f "$tmpfile"
            return 0
        fi
    done < <(grep "$tool_use_id" "$transcript_path" 2>/dev/null)

    rm -f "$tmpfile" 2>/dev/null
    return 1
}

# --- Public Functions ---

# Detect if running inside a Task sub-agent.
# Sub-agent tool calls produce agent_progress entries in the main transcript.
# Returns 0 if sub-agent detected, 1 otherwise.
is_sub_agent() {
    local transcript_path="$1"
    local tool_use_id="$2"

    _PROGRESS_TOP_TYPE=""
    _PROGRESS_DATA_TYPE=""
    _PROGRESS_CONTEXT_ID=""

    if ! _find_progress_entry "$transcript_path" "$tool_use_id"; then
        return 1
    fi

    if [[ "$_PROGRESS_DATA_TYPE" = "agent_progress" ]] && [[ -n "$_PROGRESS_CONTEXT_ID" ]]; then
        return 0
    fi
    return 1
}

# Detect if running inside a forked context skill.
# Forked skills produce skill_progress entries in the main transcript.
# Returns 0 if forked skill detected, 1 otherwise.
is_forked_context_skill() {
    local transcript_path="$1"
    local tool_use_id="$2"

    _PROGRESS_TOP_TYPE=""
    _PROGRESS_DATA_TYPE=""
    _PROGRESS_CONTEXT_ID=""

    if ! _find_progress_entry "$transcript_path" "$tool_use_id"; then
        return 1
    fi

    if [[ "$_PROGRESS_DATA_TYPE" = "skill_progress" ]] && [[ -n "$_PROGRESS_CONTEXT_ID" ]]; then
        return 0
    fi
    return 1
}

# Combined check: detect if running in any isolated sub-context.
# Returns 0 if in a sub-agent OR forked skill context, 1 otherwise.
#
# NOTE: Does NOT detect Agent Teams teammates (see Known Limitations above).
is_sub_context() {
    local transcript_path="$1"
    local tool_use_id="$2"

    if is_sub_agent "$transcript_path" "$tool_use_id"; then
        return 0
    fi
    if is_forked_context_skill "$transcript_path" "$tool_use_id"; then
        return 0
    fi
    return 1
}

# Detect if PathFlow mode is active.
# Checks for pathflow-active flag file (runtime indicator).
# The flag is created at PF-1 (session start) and removed at PF-7/session-end.
# Returns 0 if active, 1 otherwise.
is_pathflow_active() {
    local repo_root="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

    # TODO(go-cli): Session flag check unchanged. Go CLI writes same flag files
    # Future: query DB sessions table instead of flag file
    local _env_file="$repo_root/.state/runtime/codeflow-env.sh"
    if [[ -f "$_env_file" ]]; then
        # shellcheck source=/dev/null
        source "$_env_file"
    fi

    local flag_file="$repo_root/.state/session/${CODEFLOW_SESSION_ID:-unknown}/is-pathflow-active"
    [[ -f "$flag_file" ]]
}

# Read the agent_teams setting from settings.json.
# Returns: "auto", "always", "never", or "" if not set.
# This reads the _codeflow.agent_teams value from project settings.
get_pathflow_setting() {
    local repo_root="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
    local settings_file="${repo_root}/.claude/settings.json"

    if [[ ! -f "$settings_file" ]]; then
        echo ""
        return
    fi

    if command -v jq &>/dev/null; then
        jq -r '._codeflow.agent_teams // ""' "$settings_file" 2>/dev/null || echo ""
    else
        echo ""
    fi
}
