#!/usr/bin/env bash
# Purpose:   Main PreToolUse security hook for Bash commands (Modular Orchestrator)
# Location:  .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-security.sh
# Hook Type: PreToolUse
# Matcher:   Bash
#
# This hook:
#   - Orchestrates 7 security enforcement modules (per V3 spec)
#   - Validates Bash commands for dangerous patterns
#   - Blocks privilege escalation, hook bypass, etc.
#   - Logs security events to audit trail
#
# Modules (in execution order):
#   1. cf-dangerous-commands.sh - rm -rf /, fork bombs
#   2. cf-privilege-protection.sh - sudo, su, doas, pkexec
#   3. cf-git-protection.sh - Hook bypass, force push, hook manipulation
#   4. cf-hook-bypass.sh - --no-verify, SKIP_HOOKS, etc.
#   5. cf-path-protection.sh - Path boundary validation
#   6. cf-file-operations.sh - Protected paths, glob bypass
#   7. cf-branch-file-protection.sh - Branch-specific file protection
#   8. cf-tmp-protection.sh - Temp directory isolation
#   9. cf-network-protection.sh - Git push/pull, gh commands
#
# Environment variables from Claude Code:
#   - TOOL_NAME: Should be "Bash"
#   - TOOL_INPUT: JSON with command field
#
# Compatibility: bash 3.2+ (macOS compatible)
#
# Exit codes:
#   - 0: Command allowed (all modules passed)
#   - 2: Command blocked (one or more modules blocked)

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

# Security library and enforcement paths
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
ENFORCEMENT_DIR="$REPO_ROOT/.codeflow/scripts/security/enforcement"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"

export LIB_DIR CONFIG

# =============================================================================
# INPUT PARSING
# =============================================================================

# Parse command from tool input
TOOL_INPUT="${TOOL_INPUT:-}"
if [[ -z "$TOOL_INPUT" ]]; then
    echo "Warning: No tool input provided" >&2
    exit 0
fi

# Extract command using jq if available, otherwise basic parsing
if command -v jq &>/dev/null; then
    COMMAND=$(echo "$TOOL_INPUT" | jq -r '.command // empty')
else
    # Fallback: basic extraction
    COMMAND=$(echo "$TOOL_INPUT" | grep -o '"command"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*":.*"\([^"]*\)"/\1/')
fi

if [[ -z "$COMMAND" ]]; then
    exit 0
fi

export COMMAND

# =============================================================================
# ENFORCEMENT MODULE LOADING
# =============================================================================

# Load protected paths from config for skip logic
PROTECTED_PATHS=()
EXECUTION_BLOCKED_PATHS=()
INDIRECT_WRITE_CMDS="(cp|dd|tee|rsync|scp|install|ln)"

# Managed tmp folder protection
MANAGED_TMP_FOLDERS=(
    "/tmp/claude/managed"
    "/tmp/claude/managed/protected-edits"
    "/tmp/claude/managed/state"
)
STATE_FOLDER="/tmp/claude/managed/state"

if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    while IFS= read -r path; do
        [[ -n "$path" ]] && PROTECTED_PATHS+=("$path")
    done < <(jq -r '.protected_resources | .critical[]?, .high[]?, .moderate[]? // empty' "$CONFIG" 2>/dev/null)

    # Load execution-blocked paths (hooks, scripts)
    while IFS= read -r path; do
        [[ -n "$path" ]] && EXECUTION_BLOCKED_PATHS+=("$path")
    done < <(jq -r '.protected_paths.core[]? | select(.tools[] == "Bash") | .pattern // empty' "$CONFIG" 2>/dev/null)

    # Load managed tmp folders from config if available
    MANAGED_TMP_FOLDERS=()
    while IFS= read -r folder; do
        [[ -n "$folder" ]] && MANAGED_TMP_FOLDERS+=("$folder")
    done < <(jq -r '.protected_paths.managed_tmp.protected_folders[]? // empty' "$CONFIG" 2>/dev/null)
    [[ ${#MANAGED_TMP_FOLDERS[@]} -eq 0 ]] && MANAGED_TMP_FOLDERS=("/tmp/claude/managed" "/tmp/claude/managed/protected-edits" "/tmp/claude/managed/state")

    STATE_FOLDER=$(jq -r '.protected_paths.managed_tmp.state_folder // "/tmp/claude/managed/state"' "$CONFIG" 2>/dev/null)
fi

export PROTECTED_PATHS EXECUTION_BLOCKED_PATHS INDIRECT_WRITE_CMDS MANAGED_TMP_FOLDERS STATE_FOLDER

# =============================================================================
# SECURITY LIBRARY (for logging)
# =============================================================================

LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# ENFORCEMENT MODULE ORCHESTRATION (V3 Spec: 7+ modules)
# =============================================================================
# Each module checks and blocks if needed (exit 2), or returns 0 to continue.
# Modules are sourced in priority order - critical checks first.

# 1. Dangerous commands (rm -rf /, fork bombs, etc.) - CRITICAL
if [[ -f "$ENFORCEMENT_DIR/cf-dangerous-commands.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-dangerous-commands.sh"
fi

# 2. Privilege escalation (sudo, su, doas, pkexec) - CRITICAL
if [[ -f "$ENFORCEMENT_DIR/cf-privilege-protection.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-privilege-protection.sh"
fi

# 3. Git protection (hook bypass via config, force push to protected) - HIGH
if [[ -f "$ENFORCEMENT_DIR/cf-git-protection.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-git-protection.sh"
fi

# 4. Hook bypass (--no-verify, SKIP_HOOKS, HUSKY=0) - HIGH
if [[ -f "$ENFORCEMENT_DIR/cf-hook-bypass.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-hook-bypass.sh"
fi

# 5. Path protection (settings.json, hooks, security scripts) - HIGH
if [[ -f "$ENFORCEMENT_DIR/cf-path-protection.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-path-protection.sh"
fi

# 6. File operations (protected paths, glob bypass) - HIGH
if [[ -f "$ENFORCEMENT_DIR/cf-file-operations.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-file-operations.sh"
fi

# 7. Branch file protection (blocks writes on main/master) - MODERATE
if [[ -f "$ENFORCEMENT_DIR/cf-branch-file-protection.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-branch-file-protection.sh"
fi

# 8. Temp directory isolation (managed tmp protection) - MODERATE
if [[ -f "$ENFORCEMENT_DIR/cf-tmp-protection.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-tmp-protection.sh"
fi

# 9. Network protection (git push/pull, gh commands) - MODERATE
if [[ -f "$ENFORCEMENT_DIR/cf-network-protection.sh" ]]; then
    # shellcheck source=/dev/null
    source "$ENFORCEMENT_DIR/cf-network-protection.sh"
fi

# =============================================================================
# ALL CHECKS PASSED
# =============================================================================

exit 0
