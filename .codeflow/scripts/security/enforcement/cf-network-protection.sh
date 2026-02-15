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
#   - 2: Block command (via block_with_skill, exits script)

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

# Check if command matches network operation patterns from config
# Returns 0 if match found, 1 if no match
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

# =============================================================================
# GIT NETWORK OPERATIONS
# =============================================================================

if check_network_pattern ".network_operations.git_network.patterns"; then
    if declare -f is_pathflow_active &>/dev/null && is_pathflow_active; then
        # PathFlow mode: route to cf-git-operations teammate for git network ops
        block_with_skill "Network Operation" "Git network operation requires sandbox bypass (dangerouslyDisableSandbox: true). In PathFlow mode, delegate to cf-git-operations teammate." "git network" "security" "sandbox-check"
    else
        # Standalone mode: delegate to cf-security teammate
        block_with_skill "Network Operation" "Git network operation requires sandbox bypass (dangerouslyDisableSandbox: true). Delegate to cf-security teammate for sandbox-check, then cf-git-operations for sync-remote." "git network" "security" "sandbox-check"
    fi
fi

# =============================================================================
# GITHUB CLI OPERATIONS
# =============================================================================

if check_network_pattern ".network_operations.github_cli.patterns"; then
    block_with_skill "Network Operation" "GitHub CLI requires sandbox bypass (dangerouslyDisableSandbox: true)" "gh cli" "security" "sandbox-check"
fi

# All network protection checks passed
return 0
