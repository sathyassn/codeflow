#!/usr/bin/env bash
# Purpose:   Git hook bypass prevention
# Usage:     source "enforcement/cf-hook-bypass.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles:
#   - Git hook bypass flags (--no-verify, -n)
#   - Force push prevention (--force, -f)
#   - Hook path manipulation (core.hooksPath, env vars)
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security lib directory
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_command, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# GIT HOOK BYPASS PREVENTION
# =============================================================================

# --no-verify flag (anywhere in git command)
if [[ "$COMMAND" =~ git[[:space:]]+(commit|push|rebase|cherry-pick|merge|am)[[:space:]]+.*--no-verify ]]; then
    block_command \
        "Git Hook Bypass" \
        "Hook bypass flag detected" \
        "--no-verify"
fi

# --no-verify at git level (before subcommand)
if [[ "$COMMAND" =~ git[[:space:]]+--no-verify ]]; then
    block_command \
        "Git Hook Bypass" \
        "Hook bypass flag at git level" \
        "git --no-verify"
fi

# -n flag for git commit (NOT git push where it means --dry-run)
# CRITICAL: -n means DIFFERENT things for different git commands:
#   - git commit -n = --no-verify (skips hooks) -> MUST BLOCK
#   - git push -n   = --dry-run (simulates push) -> MUST ALLOW
if [[ "$COMMAND" =~ ^git[[:space:]]+commit[[:space:]] ]]; then
    flags_portion=$(get_flags_portion "$COMMAND")

    # Check for standalone -n flag
    if [[ "$flags_portion" =~ [[:space:]]-n($|[[:space:]]) ]]; then
        block_command \
            "Git Hook Bypass" \
            "Hook bypass flag (short form)" \
            "-n"
    fi

    # Check for combined flags containing n (e.g., -nm, -anm, -nam)
    if [[ "$flags_portion" =~ [[:space:]]-[a-mo-z]*n[a-mo-z]*($|[[:space:]]) ]]; then
        block_command \
            "Git Hook Bypass" \
            "Hook bypass in combined flags" \
            "-n in combined flags"
    fi
fi

# =============================================================================
# FORCE PUSH PREVENTION
# =============================================================================

# --force flag (anywhere after push)
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+.*--force($|[[:space:]]) ]]; then
    block_command \
        "Force Push" \
        "Force push attempt blocked" \
        "--force"
fi

# --force-with-lease flag
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+.*--force-with-lease($|[[:space:]]) ]]; then
    block_command \
        "Force Push" \
        "Force push attempt (with lease) blocked" \
        "--force-with-lease"
fi

# -f short flag for push
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+(.*[[:space:]])?-f($|[[:space:]]) ]]; then
    block_command \
        "Force Push" \
        "Force push attempt (short form) blocked" \
        "-f"
fi

# =============================================================================
# HOOK PATH MANIPULATION
# =============================================================================

# core.hooksPath modification via git config
if [[ "$COMMAND" =~ git[[:space:]]+config.*core\.hooksPath ]]; then
    block_command \
        "Hook Manipulation" \
        "Hook path modification attempt" \
        "core.hooksPath"
fi

# core.hooksPath via -c flag (git -c core.hooksPath=...)
if [[ "$COMMAND" =~ git[[:space:]]+-c[[:space:]]+core\.hooksPath ]]; then
    block_command \
        "Hook Manipulation" \
        "Hook path override via -c flag" \
        "-c core.hooksPath"
fi

# Unset core.hooksPath
if [[ "$COMMAND" =~ git[[:space:]]+config.*--unset.*core\.hooksPath ]]; then
    block_command \
        "Hook Manipulation" \
        "Hook path unset attempt" \
        "--unset core.hooksPath"
fi

# =============================================================================
# ENVIRONMENT VARIABLE BYPASS ATTEMPTS
# =============================================================================

# GIT_HOOKS_PATH env var
if [[ "$COMMAND" =~ GIT_HOOKS_PATH ]]; then
    block_command \
        "Hook Manipulation" \
        "Hook path env var" \
        "GIT_HOOKS_PATH"
fi

# SKIP_HOOKS env var
if [[ "$COMMAND" =~ SKIP_HOOKS ]]; then
    block_command \
        "Hook Manipulation" \
        "Hook skip env var" \
        "SKIP_HOOKS"
fi

# GIT_SKIP_HOOKS env var
if [[ "$COMMAND" =~ GIT_SKIP_HOOKS ]]; then
    block_command \
        "Hook Manipulation" \
        "Hook skip env var" \
        "GIT_SKIP_HOOKS"
fi

# HUSKY=0 (common hook framework bypass)
if [[ "$COMMAND" =~ HUSKY[[:space:]]*=[[:space:]]*0 ]]; then
    block_command \
        "Hook Manipulation" \
        "Husky bypass attempt" \
        "HUSKY=0"
fi

# PRE_COMMIT_ALLOW_NO_CONFIG (pre-commit framework bypass)
if [[ "$COMMAND" =~ PRE_COMMIT_ALLOW_NO_CONFIG ]]; then
    block_command \
        "Hook Manipulation" \
        "pre-commit bypass attempt" \
        "PRE_COMMIT_ALLOW_NO_CONFIG"
fi

# =============================================================================
# GIT HOOKS DIRECTORY MANIPULATION
# =============================================================================

# Modifying .git/hooks directly
if [[ "$COMMAND" =~ (rm|mv|chmod|chown)[[:space:]].*\.git/hooks ]]; then
    block_command \
        "Hook Manipulation" \
        "Direct .git/hooks modification" \
        ".git/hooks"
fi

# Writing to .git/hooks
if [[ "$COMMAND" =~ \>[[:space:]]*\.git/hooks ]]; then
    block_command \
        "Hook Manipulation" \
        "Redirect to .git/hooks" \
        "> .git/hooks"
fi

# All hook bypass checks passed
return 0
