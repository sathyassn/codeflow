#!/usr/bin/env bash
# Purpose:   Git-related security checks (Sections 1-4)
# Location:  .codeflow/scripts/security/enforcement/cf-git-protection.sh
# Usage:     source "cf-git-protection.sh" (from main hook)
#
# This module handles:
#   - Section 1: Git Hook Bypass Prevention (--no-verify, -n)
#   - Section 2: Force Push Prevention (--force, -f)
#   - Section 3: Hook Path Manipulation (core.hooksPath, env vars)
#   - Section 4: Git Hooks Directory Protection (.git/hooks)
#
# SECTION OWNERSHIP: This script is the AUTHORITATIVE owner of Sections 1-4.
# cf-hook-bypass.sh duplicates these checks (git -c core.hooksPath,
# PRE_COMMIT_ALLOW_NO_CONFIG, .git/hooks manipulation, .git/hooks redirect).
# Since cf-git-protection.sh runs first in the orchestrator chain, the
# overlapping checks in cf-hook-bypass.sh are dead code.
# cf-hook-bypass.sh should only retain checks unique to it (currently none
# after --unset core.hooksPath was absorbed here).
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security-lib.sh
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_command, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null  # LIB_DIR set by caller
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# SECTION 1: Git Hook Bypass Prevention
# =============================================================================

# --no-verify flag (anywhere in command)
if [[ "$COMMAND" =~ git[[:space:]]+(commit|push|rebase|cherry-pick|merge|am)[[:space:]]+.*--no-verify ]]; then
  block_command "Git Hook Bypass" "Hook bypass flag detected" "--no-verify"
fi

# --no-verify at git level (before subcommand)
if [[ "$COMMAND" =~ git[[:space:]]+--no-verify ]]; then
  block_command "Git Hook Bypass" "Hook bypass flag at git level" "git --no-verify"
fi

# -n flag checks - ONLY for git commit, NOT git push
# CRITICAL: -n means DIFFERENT things for different git commands:
#   - git commit -n = --no-verify (skips hooks) -> MUST BLOCK
#   - git push -n   = --dry-run (simulates push) -> MUST ALLOW
if [[ "$COMMAND" =~ ^git[[:space:]]+commit[[:space:]] ]]; then
  flags_portion=$(get_flags_portion "$COMMAND")

  # Check for standalone -n flag
  if [[ "$flags_portion" =~ [[:space:]]-n($|[[:space:]]) ]]; then
    block_command "Git Hook Bypass" "Hook bypass flag (short form)" "-n"
  fi

  # Check for combined flags containing n (e.g., -nm, -anm, -nam)
  if [[ "$flags_portion" =~ [[:space:]]-[a-mo-z]*n[a-mo-z]*($|[[:space:]]) ]]; then
    block_command "Git Hook Bypass" "Hook bypass in combined flags" "-n in combined flags"
  fi
fi

# =============================================================================
# SECTION 2: Force Push Prevention
# =============================================================================

# --force flag (anywhere after push)
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+.*--force($|[[:space:]]) ]]; then
  block_command "Force Push" "Force push attempt" "--force"
fi

# --force-with-lease flag
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+.*--force-with-lease($|[[:space:]]) ]]; then
  block_command "Force Push" "Force push attempt (with lease)" "--force-with-lease"
fi

# -f short flag
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+(.*[[:space:]])?-f($|[[:space:]]) ]]; then
  block_command "Force Push" "Force push attempt (short form)" "-f"
fi

# =============================================================================
# SECTION 3: Hook Path Manipulation
# =============================================================================

# core.hooksPath modification via git config
if [[ "$COMMAND" =~ git[[:space:]]+config.*core\.hooksPath ]]; then
  block_command "Hook Manipulation" "Hook path modification attempt" "core.hooksPath"
fi

# core.hooksPath via -c flag (git -c core.hooksPath=...)
if [[ "$COMMAND" =~ git[[:space:]]+-c[[:space:]]+core\.hooksPath ]]; then
  block_command "Hook Manipulation" "Hook path override via -c flag" "-c core.hooksPath"
fi

# core.hooksPath unset attempt (git config --unset core.hooksPath)
if [[ "$COMMAND" =~ git[[:space:]]+config.*--unset.*core\.hooksPath ]]; then
  block_command "Hook Manipulation" "Hook path unset attempt" "--unset core.hooksPath"
fi

# Environment variable bypass attempts
if [[ "$COMMAND" =~ GIT_HOOKS_PATH ]]; then
  block_command "Hook Manipulation" "Hook path env var" "GIT_HOOKS_PATH"
fi

if [[ "$COMMAND" =~ SKIP_HOOKS ]]; then
  block_command "Hook Manipulation" "Hook skip env var" "SKIP_HOOKS"
fi

if [[ "$COMMAND" =~ GIT_SKIP_HOOKS ]]; then
  block_command "Hook Manipulation" "Hook skip env var" "GIT_SKIP_HOOKS"
fi

if [[ "$COMMAND" =~ HUSKY[[:space:]]*=[[:space:]]*0 ]]; then
  block_command "Hook Manipulation" "Husky bypass attempt" "HUSKY=0"
fi

# PRE_COMMIT_ALLOW_NO_CONFIG (pre-commit framework bypass)
if [[ "$COMMAND" =~ PRE_COMMIT_ALLOW_NO_CONFIG ]]; then
  block_command "Hook Manipulation" "pre-commit bypass attempt" "PRE_COMMIT_ALLOW_NO_CONFIG"
fi

# =============================================================================
# SECTION 4: Git Hooks Directory Protection
# =============================================================================

# Modifying .git/hooks directly (rm, mv, chmod, chown)
if [[ "$COMMAND" =~ (rm|mv|chmod|chown)[[:space:]].*\.git/hooks ]]; then
  block_command "Hook Manipulation" "Direct .git/hooks modification" ".git/hooks"
fi

# Writing to .git/hooks via redirect
if [[ "$COMMAND" =~ \>[[:space:]]*\.git/hooks ]]; then
  block_command "Hook Manipulation" "Redirect to .git/hooks" "> .git/hooks"
fi

# All git protection checks passed
return 0
