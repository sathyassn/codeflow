#!/usr/bin/env bash
# Purpose:   PreToolUse hook for task registration enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-task-sentinel.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write
#
# This hook:
#   - Enforces task-centric workflow for file modifications
#   - Blocks Edit/Write without active task context
#   - Validates file is within task scope (soft warning if outside)
#   - Exempts memory, state, and tmp paths from task requirement
#
# Configuration: Reads from enforcement-policy.json and active-task.json
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Task exists (even if out of scope warning)
#   - 2: No active task - BLOCK

set -euo pipefail


# =============================================================================
# HOOK INPUT PARSING (Claude Code sends JSON on stdin)
# =============================================================================

# Read hook data from stdin (Claude Code protocol) or env vars (test fallback)
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _tn=$(echo "$_HOOK_STDIN" | jq -r '.tool_name // empty' 2>/dev/null)
        [[ -n "$_tn" ]] && TOOL_NAME="$_tn"
        _ti=$(echo "$_HOOK_STDIN" | jq -c '.tool_input // empty' 2>/dev/null)
        [[ -n "$_ti" ]] && [[ "$_ti" != "null" ]] && TOOL_INPUT="$_ti"
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && CODEFLOW_SESSION_ID="$_sid"
    fi
fi
CODEFLOW_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"
export CODEFLOW_SESSION_ID

# =============================================================================
# EARLY EXIT FOR NON-EDIT/WRITE TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Edit" ]] && [[ "$TOOL_NAME" != "Write" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })}"
export REPO_ROOT
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# Source work-state library for active task functions
# shellcheck source=/dev/null
source "$REPO_ROOT/.codeflow/scripts/state/cf-work-state.sh" 2>/dev/null || true

# =============================================================================
# CONFIGURATION
# =============================================================================

# Check if task sentinel is enabled (can be disabled in config)
TASK_SENTINEL_ENABLED="true"
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    TASK_SENTINEL_ENABLED=$(jq -r '.enforcement_levels.L1_SENTINEL.enabled // true' "$CONFIG" 2>/dev/null)
fi

if [[ "$TASK_SENTINEL_ENABLED" != "true" ]]; then
    exit 0
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

# Extract file path from tool input
FILE_PATH=""
if command -v jq &>/dev/null; then
    FILE_PATH=$(echo "$TOOL_INPUT" | jq -r '.file_path // empty')
else
    FILE_PATH=$(echo "$TOOL_INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":.*"\([^"]*\)"/\1/')
fi

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

# Normalize to relative path
if [[ "$FILE_PATH" == "$REPO_ROOT"/* ]]; then
    FILE_PATH="${FILE_PATH#"$REPO_ROOT"/}"
fi

# =============================================================================
# EXEMPT PATHS (Do not require task registration)
# =============================================================================

# Check if path is exempt from task requirement
is_exempt_path() {
    local path="$1"

    # Tmp paths - managed workspace
    if [[ "$path" == /tmp/claude/* ]] || [[ "$path" == /tmp/* ]]; then
        return 0
    fi

    # Memory files - meta-work
    if [[ "$path" == .claude/memory/* ]]; then
        return 0
    fi

    # State files - meta-work
    if [[ "$path" == .state/* ]]; then
        return 0
    fi

    # Codeflow state - workflow state
    if [[ "$path" == .codeflow/state/* ]]; then
        return 0
    fi

    return 1
}

# Skip check for exempt paths
if is_exempt_path "$FILE_PATH"; then
    exit 0
fi

# =============================================================================
# ACTIVE TASK CHECK
# =============================================================================

# Check if active task context exists (using work-state.sh)
if ! is_task_active; then
    # Log the block event
    if declare -f log_security_event &>/dev/null; then
        log_security_event "blocked" "task_required" "$TOOL_NAME" "$FILE_PATH" "No active task context"
    fi

    cat >&2 <<EOF
BLOCKED: Task registration required for file modifications

No active task context found.
Path: $FILE_PATH

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
REQUIRED: Register work first
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

MUST: Skill('cf-task-management', args='ensure-work-registered')

This ensures all work is tracked in the task-centric workflow.
EOF
    exit 2
fi

# =============================================================================
# TASK SCOPE VALIDATION
# =============================================================================

# Read active task details
TASK_ID=""
FILE_SCOPE=()
SCOPE_POLICY="soft"

if command -v jq &>/dev/null; then
    TASK_ID=$(get_active_task_id)
    SCOPE_POLICY=$(jq -r '.scope_policy // "soft"' "$(get_active_task_file)" 2>/dev/null)

    # Read file scope patterns
    while IFS= read -r pattern; do
        [[ -n "$pattern" ]] && FILE_SCOPE+=("$pattern")
    done < <(jq -r '.file_scope[]? // empty' "$(get_active_task_file)" 2>/dev/null)
fi

# If no scope defined, allow all (scope is optional)
if [[ ${#FILE_SCOPE[@]} -eq 0 ]]; then
    exit 0
fi

# Check if file matches scope
matches_scope() {
    local path="$1"

    for pattern in "${FILE_SCOPE[@]}"; do
        # Convert glob to regex
        local regex="${pattern//\*\*/.*}"
        regex="${regex//\*/[^/]*}"

        if [[ "$path" =~ ^$regex$ ]]; then
            return 0
        fi
    done
    return 1
}

# Validate file is within scope
if ! matches_scope "$FILE_PATH"; then
    # Out of scope - warn but don't block (soft policy)
    if [[ "$SCOPE_POLICY" == "soft" ]]; then
        cat >&2 <<EOF
WARNING: File outside task scope

Task: $TASK_ID
Scope: ${FILE_SCOPE[*]}
File: $FILE_PATH
Policy: $SCOPE_POLICY

Consider updating task scope:
SUGGEST: Skill('cf-task-management', args='update-task')
EOF
        # Log warning but allow
        if declare -f log_security_event &>/dev/null; then
            log_security_event "warn" "task_scope_warning" "$TOOL_NAME" "$FILE_PATH" "File outside task scope"
        fi
    elif [[ "$SCOPE_POLICY" == "strict" ]]; then
        # Strict policy - block out of scope
        if declare -f log_security_event &>/dev/null; then
            log_security_event "blocked" "task_scope_blocked" "$TOOL_NAME" "$FILE_PATH" "Strict scope policy"
        fi

        cat >&2 <<EOF
BLOCKED: File outside task scope (strict policy)

Task: $TASK_ID
Scope: ${FILE_SCOPE[*]}
File: $FILE_PATH

MUST: Skill('cf-task-management', args='update-task')
EOF
        exit 2
    fi
fi

# =============================================================================
# TASK CONTEXT VALID
# =============================================================================

exit 0
