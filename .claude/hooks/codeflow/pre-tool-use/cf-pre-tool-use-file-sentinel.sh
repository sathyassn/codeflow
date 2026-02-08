#!/usr/bin/env bash
# Purpose:   PreToolUse hook for file operation sentinel enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-file-sentinel.sh
# Hook Type: PreToolUse
# Matcher:   Edit|Write
#
# This hook (V3 spec compliant):
#   - Uses sentinel library for pattern matching (preferred)
#   - Falls back to config-driven pattern matching
#   - Supports new_file_only flag (only enforce on new files)
#   - Validates sentinel for protected file operations
#   - Supports prerequisite operations (operation A requires B first)
#   - Works with cf-security-management, cf-documentation-standards, cf-script-standards
#
# Configuration: Reads from enforcement-policy.json (skills section)
#   - skills.<skill>.operations.<op>.pattern: regex to match files
#   - skills.<skill>.operations.<op>.tool: "Write" or "Edit"
#   - skills.<skill>.operations.<op>.new_file_only: only enforce on new files
#   - skills.<skill>.operations.<op>.prerequisite: operation that must run first
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Operation allowed (sentinel valid or not required)
#   - 2: Operation blocked (sentinel required but missing/expired/prerequisite missing)

set -euo pipefail

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
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
SENTINEL_LIB="$REPO_ROOT/.codeflow/scripts/security/sentinel/cf-sentinel.sh"
SENTINEL_DIR="/tmp/claude/managed/sentinels"

# Source security library for logging
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export LIB_DIR
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# Source sentinel library
SENTINEL_LIB_LOADED="false"
if [[ -f "$SENTINEL_LIB" ]]; then
    # shellcheck source=/dev/null
    source "$SENTINEL_LIB"
    SENTINEL_LIB_LOADED="true"
fi

# Read sentinel config
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    SENTINEL_DIR=$(jq -r '.sentinel.directory // "/tmp/claude/managed/sentinels"' "$CONFIG" 2>/dev/null) || true
fi

# =============================================================================
# V4: PATHFLOW MODE CHECK
# =============================================================================

if declare -f is_agent_teams_active &>/dev/null && is_agent_teams_active; then
    # In agent-teams mode, check PathFlow sentinel for write authorization
    PATHFLOW_SENTINEL_DIR="$REPO_ROOT/.state/sentinels"
    if ls "$PATHFLOW_SENTINEL_DIR"/pathflow:pf-3-* &>/dev/null 2>&1; then
        # PF-3 complete: file operations authorized by PathFlow
        exit 0
    fi
    # PF-3 not complete: fall through to existing sentinel checks
    # (which will likely block since no skill sentinels in PathFlow mode)
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
    # Fallback: basic extraction
    FILE_PATH=$(echo "$TOOL_INPUT" | grep -o '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":.*"\([^"]*\)"/\1/')
fi

if [[ -z "$FILE_PATH" ]]; then
    exit 0
fi

# Normalize to relative path
REL_PATH="$FILE_PATH"
if [[ "$FILE_PATH" == "$REPO_ROOT"/* ]]; then
    REL_PATH="${FILE_PATH#"$REPO_ROOT"/}"
fi
# Remove leading ./
REL_PATH="${REL_PATH#./}"

# Check if file exists (for new_file_only check)
FILE_EXISTS="false"
if [[ -f "$FILE_PATH" ]]; then
    FILE_EXISTS="true"
elif [[ -f "$REPO_ROOT/$REL_PATH" ]]; then
    FILE_EXISTS="true"
fi

# =============================================================================
# HELPER FUNCTIONS
# =============================================================================

# Block with missing sentinel message
block_missing_sentinel() {
    local skill="$1"
    local operation="$2"
    local file_type="$3"

    # Log the block event
    if declare -f log_security_event &>/dev/null; then
        log_security_event "blocked" "file_sentinel_missing" "$TOOL_NAME" "$FILE_PATH" "No sentinel for $file_type"
    fi

    cat >&2 <<EOF
BLOCKED: File operation requires skill invocation

File: $FILE_PATH
Type: $file_type
Required: cf-$skill:$operation

MUST: Skill('cf-$skill', args='$operation')
EOF
    exit 2
}

# Block with missing prerequisite message
block_missing_prerequisite() {
    local skill="$1"
    local operation="$2"
    local prerequisite="$3"

    # Log the block event
    if declare -f log_security_event &>/dev/null; then
        log_security_event "blocked" "file_sentinel_prereq" "$TOOL_NAME" "$FILE_PATH" "Missing prereq: $prerequisite"
    fi

    cat >&2 <<EOF
BLOCKED: Missing prerequisite operation

File: $FILE_PATH
Operation: $operation
Prerequisite: $prerequisite

SEQUENCE: 1. Skill('cf-$skill', args='$prerequisite') 2. Skill('cf-$skill', args='$operation') 3. Retry $TOOL_NAME
EOF
    exit 2
}

# Check if sentinel exists and is not expired (fallback if library not loaded)
check_sentinel_fallback() {
    local skill="$1"
    local operation="${2:-}"
    local now file expires

    now=$(date +%s)

    # Look for sentinel files matching this skill (and optionally operation)
    if [[ -n "$operation" ]]; then
        for file in "$SENTINEL_DIR/${skill}:${operation}"*.json; do
            [[ -f "$file" ]] || continue
            expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
            if [[ "$expires" =~ ^[0-9]+$ ]] && [[ "$now" -lt "$expires" ]]; then
                return 0
            fi
        done
    else
        for file in "$SENTINEL_DIR/${skill}:"*.json; do
            [[ -f "$file" ]] || continue
            expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
            if [[ "$expires" =~ ^[0-9]+$ ]] && [[ "$now" -lt "$expires" ]]; then
                return 0
            fi
        done
    fi

    return 1
}

# Get file type description for messages
get_file_type() {
    local path="$1"
    case "$path" in
        *-adr.md) echo "ADR document" ;;
        *-brief.md) echo "Brief document" ;;
        *-runbook.md) echo "Runbook document" ;;
        *.sh) echo "Shell script" ;;
        *.py) echo "Python script" ;;
        epics/*) echo "Epic/Task document" ;;
        *.md) echo "Markdown document" ;;
        *) echo "File" ;;
    esac
}

# =============================================================================
# SENTINEL LIBRARY VALIDATION (preferred path)
# =============================================================================

if [[ "$SENTINEL_LIB_LOADED" == "true" ]] && declare -f sentinel_find_skill_for_file &>/dev/null; then
    # Use sentinel library to find required skill for this file
    RESULT=$(sentinel_find_skill_for_file "$REL_PATH" "$TOOL_NAME" 2>/dev/null || echo "")

    if [[ -n "$RESULT" ]]; then
        REQUIRED_SKILL="${RESULT%%:*}"
        OPERATION="${RESULT#*:}"

        # Check new_file_only flag
        if declare -f sentinel_is_new_file_only &>/dev/null; then
            if sentinel_is_new_file_only "$REQUIRED_SKILL" "$OPERATION" 2>/dev/null; then
                if [[ "$FILE_EXISTS" == "true" ]]; then
                    # File exists and this operation is new_file_only - skip check
                    exit 0
                fi
            fi
        fi

        # Validate sentinel exists
        if declare -f sentinel_validate &>/dev/null; then
            if ! sentinel_validate "$REQUIRED_SKILL" "$REL_PATH" 2>/dev/null; then
                FILE_TYPE=$(get_file_type "$REL_PATH")
                block_missing_sentinel "$REQUIRED_SKILL" "$OPERATION" "$FILE_TYPE"
            fi
        fi

        # Check for prerequisite operation
        if declare -f sentinel_get_operation_prerequisite &>/dev/null; then
            PREREQUISITE=$(sentinel_get_operation_prerequisite "$REQUIRED_SKILL" "$OPERATION" 2>/dev/null || echo "")

            if [[ -n "$PREREQUISITE" ]]; then
                # Verify prerequisite was invoked
                if declare -f sentinel_find_by_operation &>/dev/null; then
                    if ! sentinel_find_by_operation "$REQUIRED_SKILL" "$PREREQUISITE" 2>/dev/null; then
                        block_missing_prerequisite "$REQUIRED_SKILL" "$OPERATION" "$PREREQUISITE"
                    fi
                fi
            fi
        fi
    fi

    # Library handled everything - exit
    exit 0
fi

# =============================================================================
# FALLBACK: CONFIG-DRIVEN PATTERN MATCHING
# =============================================================================

# If sentinel library not available, use config-driven approach
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Iterate through skills and their operations
    while IFS= read -r skill; do
        [[ -z "$skill" ]] && continue

        while IFS= read -r op; do
            [[ -z "$op" ]] && continue

            # Get operation config
            OP_TOOL=$(jq -r ".skills[\"$skill\"].operations[\"$op\"].tool // empty" "$CONFIG" 2>/dev/null)
            OP_PATTERN=$(jq -r ".skills[\"$skill\"].operations[\"$op\"].pattern // empty" "$CONFIG" 2>/dev/null)
            OP_NEW_FILE_ONLY=$(jq -r ".skills[\"$skill\"].operations[\"$op\"].new_file_only // false" "$CONFIG" 2>/dev/null)
            OP_PREREQUISITE=$(jq -r ".skills[\"$skill\"].operations[\"$op\"].prerequisite // empty" "$CONFIG" 2>/dev/null)

            # Skip if tool doesn't match
            [[ "$OP_TOOL" != "$TOOL_NAME" ]] && continue

            # Skip if no pattern
            [[ -z "$OP_PATTERN" ]] && continue

            # Check if file matches pattern
            if echo "$REL_PATH" | grep -qE "$OP_PATTERN"; then
                # Pattern matched

                # Check new_file_only flag
                if [[ "$OP_NEW_FILE_ONLY" == "true" ]] && [[ "$FILE_EXISTS" == "true" ]]; then
                    # File exists and this is new_file_only - skip
                    continue
                fi

                # Check sentinel
                if ! check_sentinel_fallback "$skill" "$op"; then
                    FILE_TYPE=$(get_file_type "$REL_PATH")
                    block_missing_sentinel "$skill" "$op" "$FILE_TYPE"
                fi

                # Check prerequisite
                if [[ -n "$OP_PREREQUISITE" ]]; then
                    if ! check_sentinel_fallback "$skill" "$OP_PREREQUISITE"; then
                        block_missing_prerequisite "$skill" "$op" "$OP_PREREQUISITE"
                    fi
                fi

                # Sentinel valid - operation allowed for this pattern
                break 2
            fi
        done < <(jq -r ".skills[\"$skill\"].operations | keys[]" "$CONFIG" 2>/dev/null)
    done < <(jq -r '.skills | keys[]' "$CONFIG" 2>/dev/null)
fi

# =============================================================================
# OPERATION ALLOWED
# =============================================================================

# Log allowed operation (audit trail)
if declare -f log_security_event &>/dev/null; then
    log_security_event "audit" "file_sentinel_allowed" "$TOOL_NAME" "$FILE_PATH" ""
fi

exit 0
