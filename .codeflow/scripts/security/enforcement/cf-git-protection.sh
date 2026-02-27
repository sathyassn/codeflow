#!/usr/bin/env bash
# Purpose:   Git-related security checks (Sections 1-5)
# Location:  .codeflow/scripts/security/enforcement/cf-git-protection.sh
# Usage:     source "cf-git-protection.sh" (from main hook)
#
# This module handles:
#   - Section 1: Git Hook Bypass Prevention (--no-verify, -n)
#   - Section 2: Force Push Prevention (--force, -f) — protected branches only
#   - Section 3: Hook Path Manipulation (core.hooksPath, env vars)
#   - Section 4: Git Hooks Directory Protection (.git/hooks)
#   - Section 5: Protected Branch Operations (merge, cherry-pick, rebase, reset)
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
# SHARED: Protected Branch Detection
# =============================================================================
# Used by Section 2 (force push) and Section 5 (merge/rebase/reset).
# Defined early so all sections can reference it.

# Get current branch
CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "")

# Load protected branches from config
PROT_BRANCHES=()
if [[ -n "${CONFIG:-}" && -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    while IFS= read -r pb; do
        [[ -n "$pb" ]] && PROT_BRANCHES+=("$pb")
    done < <(jq -r '.protected_branches[]? // empty' "$CONFIG" 2>/dev/null)
fi
[[ ${#PROT_BRANCHES[@]} -eq 0 ]] && PROT_BRANCHES=("main" "master" "release/*" "production")

# Check if current branch is protected
_on_protected_branch() {
    local branch="$1"
    [[ -z "$branch" ]] && return 1
    for pb in "${PROT_BRANCHES[@]}"; do
        if [[ "$branch" == "$pb" ]]; then
            return 0
        fi
        # Wildcard support (e.g., release/*)
        if [[ "$pb" == *"*"* ]]; then
            local pattern="${pb//\*/.*}"
            if [[ "$branch" =~ ^${pattern}$ ]]; then
                return 0
            fi
        fi
    done
    return 1
}

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
# SECTION 2: Force Push Prevention (protected branches only)
# =============================================================================
# Force push is blocked on protected branches (main, master, release/*, production).
# On feature branches, force push (including --force-with-lease) is allowed
# for legitimate workflows like squash-before-PR.

# --force flag (anywhere after push)
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+.*--force($|[[:space:]]) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Force Push" "Force push to protected branch '$CURRENT_BRANCH'" "--force"
    fi
fi

# --force-with-lease flag
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+.*--force-with-lease($|[[:space:]]) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Force Push" "Force push (with lease) to protected branch '$CURRENT_BRANCH'" "--force-with-lease"
    fi
fi

# -f short flag
if [[ "$COMMAND" =~ git[[:space:]]+push[[:space:]]+(.*[[:space:]])?-f($|[[:space:]]) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Force Push" "Force push (short form) to protected branch '$CURRENT_BRANCH'" "-f"
    fi
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


# =============================================================================
# SECTION 5: Protected Branch Operations (merge, cherry-pick, rebase, reset)
# =============================================================================
# PR-only workflow: no direct merges, cherry-picks, rebases, or resets on
# protected branches (main, master, release/*, production).
# These bypass pre-commit hooks (especially fast-forward merges).
#
# Note: CURRENT_BRANCH, PROT_BRANCHES, and _on_protected_branch() are defined
# in the SHARED section above (before Section 1).

# 5a. Block git merge on protected branches
if [[ "$COMMAND" =~ ^git[[:space:]]+merge([[:space:]]|$) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Protected Branch" "Merge to protected branch '$CURRENT_BRANCH' blocked" "git merge on $CURRENT_BRANCH"
    fi
fi

# 5b. Block git cherry-pick on protected branches
if [[ "$COMMAND" =~ ^git[[:space:]]+cherry-pick([[:space:]]|$) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Protected Branch" "Cherry-pick to protected branch '$CURRENT_BRANCH' blocked" "git cherry-pick on $CURRENT_BRANCH"
    fi
fi

# 5c. Block git rebase on protected branches (history rewrite)
if [[ "$COMMAND" =~ ^git[[:space:]]+rebase([[:space:]]|$) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Protected Branch" "Rebase on protected branch '$CURRENT_BRANCH' blocked" "git rebase on $CURRENT_BRANCH"
    fi
fi

# 5d. Block git reset on protected branches
if [[ "$COMMAND" =~ ^git[[:space:]]+reset([[:space:]]|$) ]]; then
    if _on_protected_branch "$CURRENT_BRANCH"; then
        block_command "Protected Branch" "Reset on protected branch '$CURRENT_BRANCH' blocked" "git reset on $CURRENT_BRANCH"
    fi
fi

# 5e. Block git checkout to protected branch with merge intent
# Catches: git checkout main && git merge (piped/chained commands)
if [[ "$COMMAND" =~ git[[:space:]]+checkout[[:space:]]+(main|master|production)[[:space:]]*(\&\&|;|\|)[[:space:]]*git[[:space:]]+(merge|cherry-pick|rebase|reset) ]]; then
    block_command "Protected Branch" "Chained checkout+merge to protected branch blocked" "checkout && merge"
fi

# 5f. Block git switch to protected branch with merge intent
if [[ "$COMMAND" =~ git[[:space:]]+switch[[:space:]]+(main|master|production)[[:space:]]*(\&\&|;|\|)[[:space:]]*git[[:space:]]+(merge|cherry-pick|rebase|reset) ]]; then
    block_command "Protected Branch" "Chained switch+merge to protected branch blocked" "switch && merge"
fi

# All git protection checks passed
return 0
