#!/usr/bin/env bash
# Purpose:   Network operation security checks - blocks network ops without sandbox bypass
# Location:  .codeflow/scripts/security/enforcement/cf-network-protection.sh
# Usage:     source "cf-network-protection.sh" (from main hook)
#
# This module handles:
#   - Git network operations (push, pull, fetch, clone, remote update, ls-remote)
#   - GitHub CLI network operations (pr, issue, release, api, workflow, run, repo, gist)
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - TOOL_INPUT: Raw JSON tool input (for sandbox bypass check)
#   - CONFIG: Path to enforcement-policy.json
#   - LIB_DIR: Path to security-lib.sh
#   - REPO_ROOT: Repository root path
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_command, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null  # LIB_DIR set by caller
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# CHECK SANDBOX BYPASS STATUS
# =============================================================================

# Read sandbox bypass flag from tool input
# TOOL_INPUT is set by the calling hook (pre-tool-use-security.sh)
TOOL_INPUT="${TOOL_INPUT:-}"
SANDBOX_BYPASS=$(echo "$TOOL_INPUT" | jq -r '.dangerouslyDisableSandbox // false' 2>/dev/null || echo "false")

# If sandbox bypass is enabled, network operations are allowed
if [[ "$SANDBOX_BYPASS" == "true" ]]; then
    return 0
fi

# =============================================================================
# NETWORK OPERATION DETECTION (config-driven)
# =============================================================================

INSTRUCTIONS_DIR="${REPO_ROOT}/.codeflow/config/instructions"

# Check if command matches network operation patterns from config
check_network_pattern() {
    local patterns_path="$1"

    if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
        local patterns
        patterns=$(jq -r "$patterns_path | .[]?" "$CONFIG" 2>/dev/null)

        while IFS= read -r pattern; do
            [[ -z "$pattern" ]] && continue
            if echo "$COMMAND" | grep -qE "$pattern"; then
                return 0  # Match found
            fi
        done <<< "$patterns"
    fi
    return 1  # No match
}

# Output block message from instruction file
output_network_block() {
    local instruction_file="${INSTRUCTIONS_DIR}/network-operations.txt"

    if [[ -f "$instruction_file" ]]; then
        cat "$instruction_file" >&2
    else
        # Instruction file missing - minimal error
        echo "BLOCKED: Network operation requires sandbox bypass." >&2
        echo "Missing instruction file: $instruction_file" >&2
    fi
}

# =============================================================================
# GIT NETWORK OPERATIONS
# =============================================================================

if check_network_pattern ".network_operations.git_network.patterns"; then
    output_network_block
    exit 2
fi

# =============================================================================
# GITHUB CLI OPERATIONS
# =============================================================================

if check_network_pattern ".network_operations.github_cli.patterns"; then
    output_network_block
    exit 2
fi

# All network protection checks passed
return 0
