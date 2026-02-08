#!/usr/bin/env bash
# Purpose:   Config-driven PostToolUse instruction hook
# Location:  .claude/hooks/codeflow/post-tool-use/cf-post-tool-use-instructions.sh
# Hook Type: PostToolUse
# Matcher:   Edit|Write|Read|Grep|Bash
#
# Description: Consolidated instruction hook that reads from instructions-config.json
# to provide contextual just-in-time skill invocation instructions after tool use.
#
# Config:    .codeflow/config/instructions/instructions-config.json
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Version: 2.0.0

set -euo pipefail

# ==============================================================================
# Constants
# ==============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT
CONFIG_FILE="$REPO_ROOT/.codeflow/config/instructions/instructions-config.json"

# ==============================================================================
# Functions
# ==============================================================================

# Output reminder in Claude Code JSON format
output_instruction() {
    local message="$1"
    cat <<EOF
{"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "$message"}}
EOF
}

# Check if tool is in the tools array for a rule
# Args: $1=rule_name, $2=tool_name
check_tool_match() {
    local rule_name="$1"
    local tool_name="$2"
    local tools
    # Query directly from config file
    tools=$(jq -r ".hooks.PostToolUse.\"$rule_name\".tools // [] | .[]" "$CONFIG_FILE" 2>/dev/null)
    for t in $tools; do
        if [ "$t" = "$tool_name" ]; then
            return 0
        fi
    done
    return 1
}

# Check if file matches any exclude pattern for a rule
# Args: $1=rule_name, $2=file_path
# Returns: 0 if excluded (should skip), 1 if not excluded (should process)
matches_exclude_pattern() {
    local rule_name="$1"
    local file_path="$2"
    local exclude_patterns
    local pattern

    # Get exclude_patterns array from config
    exclude_patterns=$(jq -r ".hooks.PostToolUse.\"$rule_name\".exclude_patterns // [] | .[]" "$CONFIG_FILE" 2>/dev/null)
    [ -z "$exclude_patterns" ] && return 1  # No patterns = not excluded

    while IFS= read -r pattern; do
        [ -z "$pattern" ] && continue
        if echo "$file_path" | grep -qE "$pattern"; then
            return 0  # File matches exclude pattern - should skip
        fi
    done <<< "$exclude_patterns"

    return 1  # No exclude pattern matched
}

# Process file-based instructions (Edit, Write, Read, Grep)
# Args: $1=rule_name, $2=tool_name, $3=file_path
process_file_instruction() {
    local rule_name="$1"
    local tool_name="$2"
    local file_path="$3"
    local file_pattern
    local tool_message
    local message
    local has_file_patterns
    local patterns
    local pattern

    # Check exclude_patterns first - skip rule if file is excluded
    if matches_exclude_pattern "$rule_name" "$file_path"; then
        return 1  # File is excluded from this rule
    fi

    # Check single file_pattern (query directly from config)
    file_pattern=$(jq -r ".hooks.PostToolUse.\"$rule_name\".file_pattern // empty" "$CONFIG_FILE" 2>/dev/null)
    if [ -n "$file_pattern" ]; then
        if echo "$file_path" | grep -qE "$file_pattern"; then
            # Check for tool-specific messages
            tool_message=$(jq -r ".hooks.PostToolUse.\"$rule_name\".messages.\"$tool_name\" // empty" "$CONFIG_FILE" 2>/dev/null)
            if [ -n "$tool_message" ]; then
                output_instruction "$tool_message"
                return 0
            fi
            # Fall back to generic message
            message=$(jq -r ".hooks.PostToolUse.\"$rule_name\".message // empty" "$CONFIG_FILE" 2>/dev/null)
            if [ -n "$message" ]; then
                output_instruction "$message"
                return 0
            fi
        fi
    fi

    # Check file_patterns (multiple patterns with different messages)
    # Query directly from config file to preserve escape sequences in keys
    has_file_patterns=$(jq -r ".hooks.PostToolUse.\"$rule_name\".file_patterns // empty" "$CONFIG_FILE" 2>/dev/null)
    if [ -n "$has_file_patterns" ] && [ "$has_file_patterns" != "null" ]; then
        # Get patterns directly from config file
        patterns=$(jq -r ".hooks.PostToolUse.\"$rule_name\".file_patterns | keys[]" "$CONFIG_FILE" 2>/dev/null)
        while IFS= read -r pattern; do
            [ -z "$pattern" ] && continue
            if echo "$file_path" | grep -qE "$pattern"; then
                # Try tool-specific message first (.messages.{tool})
                tool_message=$(jq -r --arg p "$pattern" ".hooks.PostToolUse.\"$rule_name\".file_patterns[\$p].messages.\"$tool_name\" // empty" "$CONFIG_FILE" 2>/dev/null)
                if [ -n "$tool_message" ]; then
                    output_instruction "$tool_message"
                    return 0
                fi
                # Fall back to generic message (.message)
                message=$(jq -r --arg p "$pattern" ".hooks.PostToolUse.\"$rule_name\".file_patterns[\$p].message // empty" "$CONFIG_FILE" 2>/dev/null)
                if [ -n "$message" ]; then
                    output_instruction "$message"
                    return 0
                fi
            fi
        done <<< "$patterns"
    fi

    return 1
}

# Process command-based instructions (Bash)
# Args: $1=rule_name, $2=command
process_command_instruction() {
    local rule_name="$1"
    local command="$2"
    local command_pattern
    local message

    # Query directly from config file
    command_pattern=$(jq -r ".hooks.PostToolUse.\"$rule_name\".command_pattern // empty" "$CONFIG_FILE" 2>/dev/null)
    if [ -n "$command_pattern" ]; then
        if echo "$command" | grep -qE "$command_pattern"; then
            message=$(jq -r ".hooks.PostToolUse.\"$rule_name\".message // empty" "$CONFIG_FILE" 2>/dev/null)
            if [ -n "$message" ]; then
                output_instruction "$message"
                return 0
            fi
        fi
    fi

    return 1
}

# Main processing function
main() {
    local tool_input
    local tool_name
    local file_path=""
    local command=""
    local rule_names
    local rule_name

    # Read tool input from stdin
    tool_input=$(cat)

    # Extract tool name
    tool_name=$(echo "$tool_input" | jq -r '.tool_name // empty' 2>/dev/null)
    [ -z "$tool_name" ] && exit 0

    # Check if config exists
    [ ! -f "$CONFIG_FILE" ] && exit 0

    # Get file path or command based on tool type
    case "$tool_name" in
        Edit|Write|Read)
            file_path=$(echo "$tool_input" | jq -r '.tool_input.file_path // empty' 2>/dev/null)
            ;;
        Grep)
            file_path=$(echo "$tool_input" | jq -r '.tool_input.path // empty' 2>/dev/null)
            ;;
        Bash)
            command=$(echo "$tool_input" | jq -r '.tool_input.command // empty' 2>/dev/null)
            ;;
    esac

    # Get PostToolUse instruction rule names from config (exclude _description)
    rule_names=$(jq -r '.hooks.PostToolUse // {} | keys[] | select(startswith("_") | not)' "$CONFIG_FILE" 2>/dev/null)
    [ -z "$rule_names" ] && exit 0

    # Iterate through instruction rules
    while IFS= read -r rule_name; do
        [ -z "$rule_name" ] && continue

        # Check if rule is enabled
        local enabled
        enabled=$(jq -r ".hooks.PostToolUse.\"$rule_name\".enabled // true" "$CONFIG_FILE" 2>/dev/null)
        [ "$enabled" != "true" ] && continue

        # Check if tool matches
        if ! check_tool_match "$rule_name" "$tool_name"; then
            continue
        fi

        # Handle file-based tools
        if [ -n "$file_path" ]; then
            if process_file_instruction "$rule_name" "$tool_name" "$file_path"; then
                exit 0
            fi
        fi

        # Handle command-based tools
        if [ -n "$command" ]; then
            if process_command_instruction "$rule_name" "$command"; then
                exit 0
            fi
        fi
    done <<< "$rule_names"

    exit 0
}

# ==============================================================================
# Entry Point
# ==============================================================================

main "$@"
