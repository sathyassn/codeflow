#!/usr/bin/env bash
# Purpose:   PreToolUse hook for Bash sentinel enforcement
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-bash-sentinel.sh
# Hook Type: PreToolUse
# Matcher:   Bash
#
# Purpose: Block protected Bash commands unless the appropriate skill
# was invoked first (proven by sentinel existence).
#
# Configuration: Reads protected patterns from enforcement-policy.json config.
#                No hardcoded patterns - all config-driven.
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Command allowed (sentinel valid or not required)
#   - 2: Command blocked (sentinel required but missing/expired/prerequisite missing)

set -euo pipefail

# =============================================================================
# EARLY EXIT FOR NON-BASH TOOLS
# =============================================================================

TOOL_NAME="${TOOL_NAME:-}"
if [[ "$TOOL_NAME" != "Bash" ]]; then
    exit 0
fi

# =============================================================================
# SETUP
# =============================================================================

# Get repo root using git (most robust) or fallback to relative path
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || { cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd; })"
export REPO_ROOT

CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
SENTINEL_LIB="$REPO_ROOT/.codeflow/scripts/security/sentinel/cf-sentinel.sh"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library for logging (optional)
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

# =============================================================================
# V4: PATHFLOW MODE CHECK
# =============================================================================
# In agent-teams mode, use PathFlow sentinels (session-scoped, no TTL)
# instead of skill sentinels (TTL 600s)

PATHFLOW_MODE="false"
if declare -f is_agent_teams_active &>/dev/null && is_agent_teams_active; then
    PATHFLOW_MODE="true"
    PATHFLOW_SENTINEL_DIR="$REPO_ROOT/.state/sentinels"
fi

# =============================================================================
# INPUT PARSING
# =============================================================================

TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    exit 0
fi

COMMAND=""
SANDBOX_BYPASS="false"

if command -v jq &>/dev/null; then
    COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty')
    SANDBOX_BYPASS=$(echo "$TOOL_INPUT" | jq -r '.dangerouslyDisableSandbox // false')
else
    # Fallback: basic extraction (less reliable)
    COMMAND=$(echo "$TOOL_INPUT" | grep -oE '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"command"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/' | head -1)
fi

if [[ -z "$COMMAND" ]]; then
    exit 0
fi

# =============================================================================
# SANDBOX BYPASS CHECK
# =============================================================================
# Note: Network operation blocking is handled by cf-pre-tool-use-security.sh
# This script validates sentinel for sandbox bypass skill invocation

if [[ "$SANDBOX_BYPASS" == "true" ]]; then
    if [[ "$SENTINEL_LIB_LOADED" == "true" ]] && declare -f sentinel_validate &>/dev/null; then
        if ! sentinel_validate "security-management" "dangerouslyDisableSandbox"; then
            # Log the block event
            if declare -f log_security_event &>/dev/null; then
                log_security_event "blocked" "sandbox_bypass_no_sentinel" "Bash" "$COMMAND" "Sandbox bypass requires skill"
            fi

            cat >&2 <<EOF
BLOCKED: Sandbox bypass requires skill invocation
Reason: Network ops fail without bypass classification

MUST: Skill('cf-security-management', args='sandbox-check')
EOF
            exit 2
        fi
    else
        # Fallback: check sentinel file directly
        sentinel_dir="/tmp/claude/managed/sentinels"
        if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
            sentinel_dir=$(jq -r '.sentinel.directory // "/tmp/claude/managed/sentinels"' "$CONFIG" 2>/dev/null)
        fi

        found=false
        now=$(date +%s)

        for file in "$sentinel_dir"/security-management:*.json; do
            [[ -f "$file" ]] || continue
            expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
            if [[ "$expires" =~ ^[0-9]+$ ]] && [[ "$now" -lt "$expires" ]]; then
                found=true
                break
            fi
        done

        if [[ "$found" != "true" ]]; then
            if declare -f log_security_event &>/dev/null; then
                log_security_event "blocked" "sandbox_bypass_no_sentinel" "Bash" "$COMMAND" "Sandbox bypass requires skill"
            fi

            cat >&2 <<EOF
BLOCKED: Sandbox bypass requires skill invocation
Reason: Network ops fail without bypass classification

MUST: Skill('cf-security-management', args='sandbox-check')
EOF
            exit 2
        fi
    fi
fi

# =============================================================================
# SENTINEL VALIDATION (config-driven)
# =============================================================================

# V4: In PathFlow mode, check PathFlow sentinels instead of skill sentinels
if [[ "$PATHFLOW_MODE" == "true" ]]; then
    # PathFlow sentinels authorize all operations after PF-3
    if ls "$PATHFLOW_SENTINEL_DIR"/pathflow:pf-3-* &>/dev/null 2>&1; then
        exit 0  # PF-3 complete, all bash operations authorized
    fi
    # If no PF-3 sentinel but pathflow active, fall through to existing checks
    # This allows pre-PF-3 operations that don't require sentinels
fi

# Check if command matches a protected pattern from config
# Uses sentinel_find_skill_for_command() which reads from enforcement-policy.json
required_skill=""

if [[ "$SENTINEL_LIB_LOADED" == "true" ]] && declare -f sentinel_find_skill_for_command &>/dev/null; then
    required_skill=$(sentinel_find_skill_for_command "$COMMAND" "Bash" || true)
elif [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Fallback: directly check config for matching patterns
    # Iterate through skills and their operations to find a match
    while IFS= read -r skill; do
        [[ -z "$skill" ]] && continue

        while IFS= read -r op; do
            [[ -z "$op" ]] && continue

            # Check if this operation applies to Bash tool
            tool=$(jq -r ".skills[\"$skill\"].operations[\"$op\"].tool // empty" "$CONFIG" 2>/dev/null)
            [[ "$tool" != "Bash" ]] && continue

            # Get pattern
            pattern=$(jq -r ".skills[\"$skill\"].operations[\"$op\"].pattern // empty" "$CONFIG" 2>/dev/null)
            [[ -z "$pattern" ]] && continue

            # Check if command matches pattern
            if echo "$COMMAND" | grep -qE "$pattern"; then
                required_skill="$skill"
                break 2
            fi
        done < <(jq -r ".skills[\"$skill\"].operations | keys[]" "$CONFIG" 2>/dev/null)
    done < <(jq -r '.skills | keys[]' "$CONFIG" 2>/dev/null)
fi

# If no skill required, allow command
if [[ -z "$required_skill" ]]; then
    exit 0
fi

# =============================================================================
# VALIDATE SENTINEL FOR REQUIRED SKILL
# =============================================================================

sentinel_valid=false

if [[ "$SENTINEL_LIB_LOADED" == "true" ]] && declare -f sentinel_validate &>/dev/null; then
    if sentinel_validate "$required_skill" "$COMMAND"; then
        sentinel_valid=true
    fi
else
    # Fallback: check sentinel file directly
    sentinel_dir="/tmp/claude/managed/sentinels"
    if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
        sentinel_dir=$(jq -r '.sentinel.directory // "/tmp/claude/managed/sentinels"' "$CONFIG" 2>/dev/null)
    fi

    now=$(date +%s)

    for file in "$sentinel_dir/${required_skill}:"*.json; do
        [[ -f "$file" ]] || continue
        expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
        if [[ "$expires" =~ ^[0-9]+$ ]] && [[ "$now" -lt "$expires" ]]; then
            sentinel_valid=true
            break
        fi
    done
fi

if [[ "$sentinel_valid" != "true" ]]; then
    # Log the block event
    if declare -f log_security_event &>/dev/null; then
        log_security_event "blocked" "bash_sentinel_missing" "Bash" "$COMMAND" "Required skill: $required_skill"
    fi

    cat >&2 <<EOF
BLOCKED: Protected operation requires skill invocation
Reason: $COMMAND | Skill: $required_skill

MUST: Skill('cf-$required_skill', args='...')
EOF
    exit 2
fi

# =============================================================================
# CROSS-SKILL PREREQUISITE CHECK
# =============================================================================
# Some operations require a DIFFERENT skill's operation to be invoked first.
# Example: git commit requires memory-management:complete-work
# Example: git checkout -b requires memory-management:begin-work

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    # Find which operation matched by iterating and checking patterns
    operations=$(jq -r ".skills[\"$required_skill\"].operations | keys[]" "$CONFIG" 2>/dev/null || true)

    matched_operation=""
    while IFS= read -r op; do
        [[ -z "$op" ]] && continue

        pattern=$(jq -r ".skills[\"$required_skill\"].operations[\"$op\"].pattern // empty" "$CONFIG" 2>/dev/null)
        [[ -z "$pattern" ]] && continue

        if echo "$COMMAND" | grep -qE "$pattern"; then
            matched_operation="$op"
            break
        fi
    done <<< "$operations"

    if [[ -n "$matched_operation" ]]; then
        # Check for cross_skill_prerequisite
        cross_prereq_skill=$(jq -r ".skills[\"$required_skill\"].operations[\"$matched_operation\"].cross_skill_prerequisite.skill // empty" "$CONFIG" 2>/dev/null)
        cross_prereq_op=$(jq -r ".skills[\"$required_skill\"].operations[\"$matched_operation\"].cross_skill_prerequisite.operation // empty" "$CONFIG" 2>/dev/null)
        cross_prereq_reason=$(jq -r ".skills[\"$required_skill\"].operations[\"$matched_operation\"].cross_skill_prerequisite.reason // empty" "$CONFIG" 2>/dev/null)

        if [[ -n "$cross_prereq_skill" ]] && [[ -n "$cross_prereq_op" ]]; then
            # Validate the cross-skill prerequisite sentinel exists
            prereq_valid=false

            if [[ "$SENTINEL_LIB_LOADED" == "true" ]] && declare -f sentinel_find_by_operation &>/dev/null; then
                if sentinel_find_by_operation "$cross_prereq_skill" "$cross_prereq_op"; then
                    prereq_valid=true
                fi
            else
                # Fallback: check sentinel file directly
                sentinel_dir="/tmp/claude/managed/sentinels"
                if command -v jq &>/dev/null; then
                    sentinel_dir=$(jq -r '.sentinel.directory // "/tmp/claude/managed/sentinels"' "$CONFIG" 2>/dev/null)
                fi

                now=$(date +%s)

                for file in "$sentinel_dir/${cross_prereq_skill}:${cross_prereq_op}"*.json; do
                    [[ -f "$file" ]] || continue
                    expires=$(jq -r '.expires // 0' "$file" 2>/dev/null)
                    if [[ "$expires" =~ ^[0-9]+$ ]] && [[ "$now" -lt "$expires" ]]; then
                        prereq_valid=true
                        break
                    fi
                done
            fi

            if [[ "$prereq_valid" != "true" ]]; then
                # Log the block event
                if declare -f log_security_event &>/dev/null; then
                    log_security_event "blocked" "bash_sentinel_prereq" "Bash" "$COMMAND" "Missing: $cross_prereq_skill:$cross_prereq_op"
                fi

                cat >&2 <<EOF
BLOCKED: Cross-skill prerequisite not satisfied
Reason: ${cross_prereq_reason:-Prerequisite required} | Command: $COMMAND

MUST: Skill('cf-$cross_prereq_skill', args='$cross_prereq_op') THEN Skill('cf-$required_skill', args='$matched_operation')
EOF
                exit 2
            fi
        fi

        # Check if this is a blocked operation (requires user confirmation)
        is_blocked=$(jq -r ".skills[\"$required_skill\"].operations[\"$matched_operation\"].block // false" "$CONFIG" 2>/dev/null)

        if [[ "$is_blocked" == "true" ]]; then
            # Log the block event
            if declare -f log_security_event &>/dev/null; then
                log_security_event "blocked" "bash_blocked_operation" "Bash" "$COMMAND" "Blocked operation"
            fi

            cat >&2 <<EOF
BLOCKED: This operation requires user confirmation
Command: $COMMAND
Reason: Destructive or protected operation

Ask the user before proceeding with this operation.
EOF
            exit 2
        fi
    fi
fi

# =============================================================================
# ALL CHECKS PASSED
# =============================================================================

exit 0
