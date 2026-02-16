#!/usr/bin/env bash
# Purpose:   Shared library functions for security enforcement modules
# Usage:     source "$CODEFLOW_ROOT/.codeflow/scripts/security/lib/security-lib.sh"
# Platform:  macOS/Linux
#
# This is a LIBRARY file - meant to be sourced, not executed directly.
# Sources context-lib.sh for mode detection (is_pathflow_active, get_pathflow_setting).
#
# Functions:
#   - block_command: Block with standard security message
#   - block_with_skill: Block with skill guidance
#   - log_security_event: Audit logging (JSONL)
#   - log_blocked: Log blocked command event
#   - log_protection: Log path protection event
#   - log_sentinel: Log sentinel operation event
#   - log_network: Log network activity event
#   - get_flags_portion: Extract git commit flags before -m
#   - is_path_targeted: Check if path appears with proper boundaries
#   - glob_to_regex: Convert glob pattern to regex
#   - is_glob_path_targeted: Check if command targets a glob pattern
#   - is_path_or_glob_targeted: Unified check for both exact and glob patterns
#
# Re-exported from context-lib.sh (available after sourcing this file):
#   - is_pathflow_active: Check if PathFlow mode is active (flag file)
#   - get_pathflow_setting: Read agent_teams setting from settings.json

set -euo pipefail

# =============================================================================
# SOURCE GUARD - Prevent double-sourcing
# =============================================================================

if [[ -n "${_SECURITY_LIB_SOURCED:-}" ]]; then
    # shellcheck disable=SC2317  # exit is fallback when return fails (executed vs sourced)
    return 0 2>/dev/null || exit 0
fi
_SECURITY_LIB_SOURCED=1

# =============================================================================
# CONSTANTS
# =============================================================================

# Get repo root (set by caller or detect)
REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# Log paths

readonly CF_LOG_BASE="${REPO_ROOT}/.state/logs/security"
CF_DATE=$(date +%Y-%m-%d)
readonly CF_DATE

# Session ID (set by environment or generate)
readonly CF_SESSION_ID="${CODEFLOW_SESSION_ID:-unknown}"

# =============================================================================
# CONTEXT LIBRARY (mode detection — is_pathflow_active, get_pathflow_setting)
# =============================================================================

LIB_DIR="${LIB_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
if [[ -f "${LIB_DIR}/context-lib.sh" ]]; then
    # shellcheck source=context-lib.sh
    source "${LIB_DIR}/context-lib.sh"
fi

# =============================================================================
# AUDIT LOGGING FUNCTIONS
# =============================================================================

# Ensure log directories exist
_ensure_log_dirs() {
    mkdir -p "$CF_LOG_BASE"/{audit,blocked,protection,sentinel,network} 2>/dev/null || true
}
# Log security event to JSONL file
# Parameters:
#   $1 - log_type: blocked, protection, sentinel, network
#   $2 - event: event type
#   $3 - tool: tool name
#   $4 - target: target path/command/url
#   $5 - reason: reason for event
#   $6 - extra_json: optional extra fields as JSON
log_security_event() {
    local log_type="$1"
    local event="$2"
    local tool="${3:-}"
    local target="${4:-}"
    local reason="${5:-}"
    local extra_json="${6:-}"

    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%S.000Z)

    _ensure_log_dirs

    # Build JSON object
    local json
    if command -v jq &>/dev/null; then
        json=$(jq -nc \
            --arg ts "$ts" \
            --arg level "WARN" \
            --arg session_id "$CF_SESSION_ID" \
            --arg event "$event" \
            --arg log_type "$log_type" \
            --arg tool "$tool" \
            --arg target "$target" \
            --arg reason "$reason" \
            '{ts: $ts, level: $level, session_id: $session_id, event: $event, log_type: $log_type, tool: $tool, target: $target, reason: $reason}')

        # Merge extra fields if provided
        if [[ -n "$extra_json" ]]; then
            json=$(echo "$json" | jq ". + $extra_json")
        fi
    else
        # Fallback without jq
        json="{\"ts\":\"$ts\",\"level\":\"WARN\",\"session_id\":\"$CF_SESSION_ID\",\"event\":\"$event\",\"log_type\":\"$log_type\",\"tool\":\"$tool\",\"target\":\"$target\",\"reason\":\"$reason\"}"
    fi

    # 1. Write to JSONL file (primary - always succeeds)
    local log_file="$CF_LOG_BASE/$log_type/$log_type-$CF_DATE.jsonl"
    echo "$json" >> "$log_file" 2>/dev/null || true

    # Also write to combined audit log
    echo "$json" >> "$CF_LOG_BASE/audit/audit-$CF_DATE.jsonl" 2>/dev/null || true

}

# Convenience: Log blocked command
log_blocked() {
    local tool="$1"
    local target="$2"
    local reason="$3"
    local module="${4:-}"

    local extra=""
    if [[ -n "$module" ]] && command -v jq &>/dev/null; then
        extra=$(jq -nc --arg m "$module" '{module: $m}')
    fi
    log_security_event "blocked" "command_blocked" "$tool" "$target" "$reason" "$extra"
}

# Convenience: Log protection event
log_protection() {
    local event="$1"
    local tool="$2"
    local path="$3"
    local tier="${4:-}"
    local outcome="${5:-}"

    local extra=""
    if command -v jq &>/dev/null; then
        extra=$(jq -nc --arg tier "$tier" --arg outcome "$outcome" '{tier: $tier, outcome: $outcome}')
    fi
    log_security_event "protection" "$event" "$tool" "$path" "" "$extra"
}

# Convenience: Log sentinel event
log_sentinel() {
    local event="$1"
    local skill="$2"
    local operation="$3"
    local sentinel_id="${4:-}"
    local ttl="${5:-0}"

    local extra=""
    if command -v jq &>/dev/null; then
        extra=$(jq -nc \
            --arg skill "$skill" \
            --arg op "$operation" \
            --arg sid "$sentinel_id" \
            --argjson ttl "$ttl" \
            '{skill: $skill, operation: $op, sentinel_id: $sid, ttl_sec: $ttl}')
    fi
    log_security_event "sentinel" "$event" "" "" "" "$extra"
}

# Convenience: Log network event
log_network() {
    local event="$1"
    local tool="$2"
    local url="$3"
    local domain="$4"
    local allowlist="${5:-}"

    local extra=""
    if command -v jq &>/dev/null; then
        extra=$(jq -nc --arg domain "$domain" --arg allowlist "$allowlist" '{domain: $domain, allowlist: $allowlist}')
    fi
    log_security_event "network" "$event" "$tool" "$url" "" "$extra"
}

# =============================================================================
# BLOCKING FUNCTIONS
# =============================================================================

# Block command with standard message
# Output to stderr (Claude Code expects stderr for exit 2)
# Parameters:
#   $1 - category: Category of security violation
#   $2 - reason: Human-readable reason for block
#   $3 - pattern: The pattern that triggered the block
# Environment:
#   COMMAND - Must be set by caller (the command being checked)
# Returns: Exits with code 2 (NEVER returns)
block_command() {
    local category="$1"
    local reason="$2"
    local pattern="$3"

    # Log the blocked event
    log_blocked "Bash" "${COMMAND:-<unknown>}" "$reason" "${BASH_SOURCE[1]:-unknown}"

    cat >&2 <<EOF
BLOCKED: Security violation detected

Category: $category
Reason: $reason
Pattern: $pattern
Command: ${COMMAND:-<unknown>}

This is a security restriction enforced by CodeFlow.
See: .codeflow/docs/security/README.md

⛔ Do NOT circumvent this restriction by using dangerouslyDisableSandbox, interpreter workarounds, or other bypass techniques.
Work THROUGH PathFlow: delegate to the appropriate teammate instead.

EOF
    exit 2
}

# Block with skill guidance
# Parameters:
#   $1 - category: Category of security violation
#   $2 - reason: Human-readable reason for block
#   $3 - pattern: The pattern that triggered the block
#   $4 - skill: Skill name to invoke (without cf- prefix)
#   $5 - operation: Operation within the skill
# Environment:
#   COMMAND - Must be set by caller
# Returns: Exits with code 2 (NEVER returns)
block_with_skill() {
    local category="$1"
    local reason="$2"
    local pattern="$3"
    local skill="$4"
    local operation="$5"

    # Log the blocked event
    log_blocked "Bash" "${COMMAND:-<unknown>}" "$reason" "${BASH_SOURCE[1]:-unknown}"

    cat >&2 <<EOF
BLOCKED: $category
Reason: $reason | Pattern: $pattern
Command: ${COMMAND:-<unknown>}

MUST: Delegate to cf-$skill teammate: $operation
⛔ Do NOT attempt to bypass this restriction. The delegation above is the correct path.
EOF
    exit 2
}

# =============================================================================
# UTILITY FUNCTIONS
# =============================================================================

# Extract the FLAGS portion of a git commit command (before -m/--message)
# This prevents false positives from words in commit messages
# Parameters:
#   $1 - cmd: The full command string
# Returns: Echoes the flags portion (before any -m flag)
get_flags_portion() {
    local cmd="$1"

    # Check for various message flag formats and extract everything before them
    if [[ "$cmd" == *" --message="* ]]; then
        echo "${cmd%% --message=*}"
        return
    fi
    if [[ "$cmd" == *" --message "* ]]; then
        echo "${cmd%% --message *}"
        return
    fi
    if [[ "$cmd" == *" -m\""* ]]; then
        echo "${cmd%% -m\"*}"
        return
    fi
    if [[ "$cmd" == *" -m'"* ]]; then
        echo "${cmd%% -m\'*}"
        return
    fi
    if [[ "$cmd" == *" -m "* ]]; then
        echo "${cmd%% -m *}"
        return
    fi

    # No message flag found - return entire command
    echo "$cmd"
}

# Check if a path appears with proper boundaries (not as substring)
# This prevents ".claude" from matching ".claude-notes.md"
# Parameters:
#   $1 - cmd: The command string to check
#   $2 - path: The path to look for
# Returns: 0 if path is targeted, 1 if not
is_path_targeted() {
    local cmd="$1"
    local path="$2"

    # Quick check: if path not in command at all, definitely not targeted
    if [[ "$cmd" != *"$path"* ]]; then
        return 1
    fi

    # Special case: if path only appears within /tmp/claude, it's safe
    local cmd_clean="$cmd"
    while [[ "$cmd_clean" =~ (.*)"/tmp/claude"[^[:space:]]*(.*) ]]; do
        cmd_clean="${BASH_REMATCH[1]}${BASH_REMATCH[2]}"
    done
    if [[ "$cmd_clean" != *"$path"* ]]; then
        return 1  # Path only appeared in /tmp/claude contexts
    fi

    # Check for valid path boundaries
    # Path followed by / (accessing contents)
    if [[ "$cmd" == *"$path/"* ]]; then
        return 0
    fi

    # Path followed by space (standalone argument)
    if [[ "$cmd" == *"$path "* ]]; then
        return 0
    fi

    # Path at absolute end of command
    if [[ "$cmd" == *"$path" ]]; then
        return 0
    fi

    # Path followed by quote (end of quoted path)
    if [[ "$cmd" == *"$path\""* ]] || [[ "$cmd" == *"$path'"* ]]; then
        return 0
    fi

    # Path not at a valid boundary - it's a substring of something else
    return 1
}

# =============================================================================
# GLOB PATTERN FUNCTIONS
# =============================================================================

# Convert glob pattern to regex for matching
# e.g., ".claude/memory/*/work-agreement*.md" -> "\.claude/memory/[^/]+/work-agreement[^/]*\.md"
# Parameters:
#   $1 - pattern: The glob pattern to convert
# Returns: Echoes the regex pattern
glob_to_regex() {
    local pattern="$1"
    # Escape regex special chars except * and ?
    local escaped="${pattern//./\\.}"
    escaped="${escaped//\[/\\[}"
    escaped="${escaped//\]/\\]}"
    escaped="${escaped//\(/\\(}"
    escaped="${escaped//\)/\\)}"
    escaped="${escaped//\{/\\{}"
    local rbrace=$'\\}'
    escaped="${escaped//\}/$rbrace}"
    escaped="${escaped//\^/\\^}"
    escaped="${escaped//\$/\\$}"
    # Convert glob * to regex [^/]* (match zero or more chars in path segment)
    escaped="${escaped//\*/[^/]*}"
    # Convert glob ? to regex . (match single char)
    escaped="${escaped//\?/.}"
    printf '%s' "$escaped"
}

# Check if command targets a glob pattern path
# Parameters:
#   $1 - cmd: The command string to check
#   $2 - pattern: The glob pattern to match
# Returns: 0 if path is targeted, 1 if not
is_glob_path_targeted() {
    local cmd="$1"
    local pattern="$2"
    local regex
    regex=$(glob_to_regex "$pattern")

    # Strip /tmp/claude paths before checking (same as is_path_targeted)
    local cmd_clean="$cmd"
    while [[ "$cmd_clean" =~ (.*)"/tmp/claude"[^[:space:]]*(.*) ]]; do
        cmd_clean="${BASH_REMATCH[1]}${BASH_REMATCH[2]}"
    done

    # Check if command contains a path matching the pattern
    if [[ "$cmd_clean" =~ $regex ]]; then
        return 0
    fi

    # For /** patterns, also match the directory itself
    # e.g., ".claude/hooks/codeflow/**" should match "rm -rf .claude/hooks/codeflow"
    if [[ "$pattern" == *"/**" ]]; then
        local dir_path="${pattern%/**}"
        if is_path_targeted "$cmd" "$dir_path"; then
            return 0
        fi
    fi

    return 1
}

# Unified check for both exact paths and glob patterns
# Parameters:
#   $1 - cmd: The command string to check
#   $2 - path: The path or glob pattern to match
# Returns: 0 if path is targeted, 1 if not
is_path_or_glob_targeted() {
    local cmd="$1"
    local path="$2"

    # Check if path contains glob characters
    if [[ "$path" == *"*"* ]] || [[ "$path" == *"?"* ]]; then
        # Use glob matching (includes /** directory protection)
        is_glob_path_targeted "$cmd" "$path"
    else
        # Use standard path matching
        is_path_targeted "$cmd" "$path"
    fi
}

# =============================================================================
# LIBRARY GUARD
# =============================================================================
# This file should be sourced, not executed directly
if [[ "${BASH_SOURCE[0]:-}" == "${0:-}" ]]; then
    echo "Error: This is a library file. Source it instead of executing." >&2
    echo "Usage: source \"$(basename "$0")\"" >&2
    exit 1
fi
