#!/usr/bin/env bash
# Purpose:   State-managed PostToolUse hook for memory progress reminders
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-memory-progress.sh
# Hook Type: PostToolUse
# Matcher:   Edit|Write
#
# Description: Triggers memory progress reminder after N substantive tool operations.
# Uses state file to track tool count across invocations within a session.
# State files are cleaned up by session-start-cleanup.sh at next session start.
#
# V1.2.0 Features:
#   - Claim heartbeat: Renews active work claims on Edit/Write to prevent expiration
#   - Memory progress reminders after threshold reached
#   - Quick Check detection for early session prompts
#
# Configuration: Reads thresholds from .codeflow/config/enforcement/enforcement-policy.json
#
# Compatibility: bash 3.2+ (macOS compatible)

set -euo pipefail

# ==============================================================================
# Constants
# ==============================================================================

readonly VERSION="1.2.0"
readonly STATE_DIR="/tmp/claude/managed/state"
readonly SENTINEL_DIR="/tmp/claude/managed/sentinels"
readonly EARLY_SESSION_THRESHOLD=3
readonly CLAIM_RENEW_INTERVAL=180  # Renew claim every 3 minutes of activity

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT
readonly REPO_ROOT

readonly CONFIG_FILE="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
readonly INSTRUCTION_FILE="$REPO_ROOT/.codeflow/config/instructions/memory-progress.txt"

# Load config with fallback defaults
if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
    THRESHOLD=$(jq -r '.thresholds.post_tool_use.memory_progress.threshold // 5' "$CONFIG_FILE" 2>/dev/null || echo "5")
    TRACKED_TOOLS=$(jq -r '.thresholds.post_tool_use.memory_progress.tracked_tools // ["Edit", "Write"] | join("|")' "$CONFIG_FILE" 2>/dev/null || echo "Edit|Write")
else
    THRESHOLD=5
    TRACKED_TOOLS="Edit|Write"
fi
readonly THRESHOLD
readonly TRACKED_TOOLS

# ==============================================================================
# Functions
# ==============================================================================

show_help() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

State-managed PostToolUse hook for memory progress reminders.
Triggers reminder after $THRESHOLD substantive tool operations.

OPTIONS:
    -h, --help      Show this help message
    -V, --version   Show version information

DESCRIPTION:
    This hook is called automatically by Claude Code after tool execution.
    It tracks Edit/Write operations and shows a memory progress reminder
    after the threshold is reached, encouraging the agent to record
    decisions and milestones.
EOF
}

show_version() {
    echo "cf-post-tool-use-memory-progress.sh version $VERSION"
}

# Output reminder in Claude Code JSON format
output_reminder() {
    local message="$1"
    # Escape for JSON
    message=$(echo "$message" | sed 's/"/\\"/g' | tr '\n' ' ')
    cat <<EOF
{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "$message"}}
EOF
}

# Check if Quick Check (detect-active-work) was performed
check_quick_check_performed() {
    # shellcheck disable=SC2086
    if ls ${SENTINEL_DIR}/memory-management:detect-active-work-* 1>/dev/null 2>&1; then
        return 0
    fi
    return 1
}

# Renew active work claim if one exists (claim heartbeat)
# Prevents claim expiration during active work
renew_active_claim() {
    local session_id="$1"
    local claim_state_file="$STATE_DIR/active-claim-$session_id"
    local claim_heartbeat_file="$STATE_DIR/claim-heartbeat-$session_id"

    # Check if we have an active claim
    if [[ ! -f "$claim_state_file" ]]; then
        return 0
    fi

    local claim_id
    claim_id=$(jq -r '.claim_id // empty' "$claim_state_file" 2>/dev/null)
    if [[ -z "$claim_id" ]]; then
        return 0
    fi

    # Check heartbeat interval to avoid excessive renewals
    local last_heartbeat=0
    if [[ -f "$claim_heartbeat_file" ]]; then
        last_heartbeat=$(cat "$claim_heartbeat_file" 2>/dev/null || echo "0")
    fi

    local now
    now=$(date +%s)
    local elapsed=$((now - last_heartbeat))

    # Only renew if interval has passed
    if [[ $elapsed -lt $CLAIM_RENEW_INTERVAL ]]; then
        return 0
    fi

    # Renew the claim
    local renew_script="$REPO_ROOT/.codeflow/scripts/coordination/cf-claim-renew.py"
    if [[ -x "$renew_script" ]]; then
        if python3 "$renew_script" --claim-id "$claim_id" --json >/dev/null 2>&1; then
            # Update heartbeat timestamp
            echo "$now" > "$claim_heartbeat_file"
        fi
    fi

    return 0
}

# ==============================================================================
# Main
# ==============================================================================

main() {
    # Handle flags
    case "${1:-}" in
        -h|--help)
            show_help
            exit 0
            ;;
        -V|--version)
            show_version
            exit 0
            ;;
    esac

    local tool_input
    local tool_name
    local file_path
    local session_id
    local state_file
    local tool_count
    local reminder_content

    # Read tool input from stdin
    tool_input=$(cat)

    # Extract tool name
    tool_name=$(echo "$tool_input" | jq -r '.tool_name // empty' 2>/dev/null)
    [ -z "$tool_name" ] && exit 0

    # Only track substantive tools
    if ! echo "$tool_name" | grep -qE "^($TRACKED_TOOLS)$"; then
        exit 0
    fi

    # Get file path
    file_path=$(echo "$tool_input" | jq -r '.tool_input.file_path // ""' 2>/dev/null)

    # Exclude memory files (recording progress shouldn't trigger more recording)
    if echo "$file_path" | grep -qE "\.claude/memory/"; then
        exit 0
    fi

    # Get session ID for state file naming
    session_id=$(echo "$tool_input" | jq -r '.session_id // "unknown"' 2>/dev/null)
    state_file="$STATE_DIR/memory-progress-$session_id"

    # Ensure state directory exists
    mkdir -p "$STATE_DIR"

    # Read current count
    tool_count=0
    if [ -f "$state_file" ]; then
        tool_count=$(jq -r '.tool_count // 0' "$state_file" 2>/dev/null || echo "0")
    fi

    # Increment count
    tool_count=$((tool_count + 1))

    # Claim heartbeat - renew active work claim on edit activity
    renew_active_claim "$session_id"

    # Quick Check detection warning
    if [ "$tool_count" -le "$EARLY_SESSION_THRESHOLD" ]; then
        if ! check_quick_check_performed; then
            output_reminder "WARN [L3]: Session Quick Check not detected. Run: Skill('cf-memory-management', args='detect-active-work')"
        fi
    fi

    # Check threshold
    if [ "$tool_count" -ge "$THRESHOLD" ]; then
        # Reset counter
        echo '{"tool_count": 0}' > "$state_file"

        # Read reminder content
        if [ -f "$INSTRUCTION_FILE" ]; then
            reminder_content=$(cat "$INSTRUCTION_FILE")
        else
            reminder_content="MEMORY PROGRESS CHECK: Consider Skill('cf-memory-management', args='record-work-progress type=decision|progress|milestone content=\"...\"') if decisions or milestones occurred."
        fi

        output_reminder "$reminder_content"
    else
        # Update counter only
        echo "{\"tool_count\": $tool_count}" > "$state_file"
    fi

    exit 0
}

# ==============================================================================
# Entry Point
# ==============================================================================

main "$@"
