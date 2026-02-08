#!/usr/bin/env bash
# Purpose:   File operation security checks (Sections 8-10)
# Location:  .codeflow/scripts/security/enforcement/cf-file-operations.sh
# Usage:     source "cf-file-operations.sh" (from main hook)
#
# This module handles:
#   - Section 8: Script Execution from Hook Directories
#   - Section 9: Indirect File Operations (cp, dd, tee, rsync, etc.)
#   - Section 10: Glob Pattern Bypass Prevention
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security-lib.sh
#   - PROTECTED_PATHS: Array of protected paths
#   - EXECUTION_BLOCKED_PATHS: Array of execution-blocked paths
#   - INDIRECT_WRITE_CMDS: Regex pattern of indirect write commands
#
# Exit codes:
#   - 0: All checks passed (via return, not exit)
#   - 2: Block command (via block_command, exits script)

set -euo pipefail

# Source shared library
# shellcheck source=/dev/null  # LIB_DIR set by caller
source "${LIB_DIR}/security-lib.sh"

# =============================================================================
# SECTION 8: Script Execution from Hook Directories
# =============================================================================
# Block direct execution of hook scripts (these should only be run by frameworks)
# Uses EXECUTION_BLOCKED_PATHS (subset of PROTECTED_PATHS)

# Guard: Only iterate if array is non-empty (set -u crashes on empty array iteration)
if [[ ${#EXECUTION_BLOCKED_PATHS[@]} -gt 0 ]]; then
  for path in "${EXECUTION_BLOCKED_PATHS[@]}"; do
    # ./blocked_path/script or bash blocked_path/script
    if [[ "$COMMAND" =~ (^|\./|bash[[:space:]]+|sh[[:space:]]+|python[[:space:]]+|python3[[:space:]]+)"$path" ]]; then
      block_command "Script Execution" "Direct execution of hook scripts not allowed" "$path"
    fi
  done
fi

# =============================================================================
# SECTION 9: Indirect File Operations (cp, dd, tee, rsync, scp, install, ln)
# =============================================================================
# Problem: L0/L3 deny patterns only do PREFIX matching, not path-specific
# Solution: Regex-based detection of copy/write operations targeting protected paths

# Skip Section 9 entirely if destination is /tmp/claude (safe scratch space)
# Pattern matches /tmp/claude with or without trailing slash
if [[ "$COMMAND" =~ [[:space:]]/tmp/claude($|/) ]] || [[ "$COMMAND" =~ \>/tmp/claude($|/) ]]; then
  # Destination is tmp/claude, skip protected path checks for indirect writes
  :  # No-op, fall through to Section 10
else
  # Single loop for all indirect write commands (DRY)
  # Uses is_path_or_glob_targeted to support glob patterns (e.g., .claude/memory/*/work-agreement*.md)
  for path in "${PROTECTED_PATHS[@]}"; do
    if is_path_or_glob_targeted "$COMMAND" "$path"; then
      # Check all indirect write commands in one pattern
      if [[ "$COMMAND" =~ (^|[[:space:]]|/)($INDIRECT_WRITE_CMDS)[[:space:]] ]]; then
        block_command "Protected Path Write" "Indirect write operation targeting protected path" "$path"
      fi
    fi
  done
fi

# dd command - special handling for of= parameter
if [[ "$COMMAND" =~ (^|[[:space:]])(dd)[[:space:]] ]]; then
  for path in "${PROTECTED_PATHS[@]}"; do
    if [[ "$COMMAND" =~ of=[^[:space:]]*"$path" ]]; then
      block_command "Protected Path Copy" "dd operation targeting protected path" "$path"
    fi
  done
fi

# Piped tee to protected paths: cmd | tee protected_path
# Uses is_path_or_glob_targeted to support glob patterns
if [[ "$COMMAND" =~ \|[[:space:]]*(tee)[[:space:]] ]]; then
  for path in "${PROTECTED_PATHS[@]}"; do
    if is_path_or_glob_targeted "$COMMAND" "$path"; then
      block_command "Protected Path Pipe" "Piped tee to protected path" "$path"
    fi
  done
fi

# cat with append redirection to protected paths
if [[ "$COMMAND" =~ cat[[:space:]].*\>\>[[:space:]]* ]]; then
  for path in "${PROTECTED_PATHS[@]}"; do
    # Check if path comes AFTER the >> (is the target, not source)
    if [[ "$COMMAND" =~ \>\>[[:space:]]*"$path"($|[[:space:]\"\'/]) ]]; then
      block_command "Protected Path Append" "cat append redirection to protected path" "$path"
    fi
  done
fi

# =============================================================================
# SECTION 10: Glob Pattern Bypass Prevention
# =============================================================================
# Problem: Attackers can use glob patterns to bypass exact path matching

# Only check commands that could write to files
if [[ "$COMMAND" =~ (^|[[:space:]]|/)($INDIRECT_WRITE_CMDS)[[:space:]] ]]; then
  # Check if command contains glob characters
  if [[ "$COMMAND" == *"*"* ]] || [[ "$COMMAND" == *"?"* ]]; then
    for path in "${PROTECTED_PATHS[@]}"; do
      # Extract path without last character for ? pattern matching
      path_minus_one="${path%?}"

      # Check for ? glob that could match protected path
      if [[ "$COMMAND" == *"${path_minus_one}?"* ]]; then
        block_command "Glob Bypass Attempt" "Glob pattern '?' could match protected path" "$path"
      fi

      # Check for * glob that could match protected path
      prefix="$path"
      while [[ ${#prefix} -ge 5 ]]; do
        if [[ "$COMMAND" == *"${prefix}"* ]] && [[ "$COMMAND" == *"${prefix}"*"*"* ]]; then
          # Found prefix followed eventually by * - check if * comes right after prefix
          if [[ "$COMMAND" =~ "${prefix}"[^\"/[:space:]]*\* ]]; then
            block_command "Glob Bypass Attempt" "Glob pattern '*' could match protected path" "$path"
          fi
        fi
        # Remove last character and try again
        prefix="${prefix%?}"
      done
    done
  fi
fi

# All file operation checks passed
return 0
