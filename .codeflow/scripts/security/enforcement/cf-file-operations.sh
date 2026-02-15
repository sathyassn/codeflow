#!/usr/bin/env bash
# Purpose:   File operation security checks (Sections 9-11)
# Location:  .codeflow/scripts/security/enforcement/cf-file-operations.sh
# Usage:     source "cf-file-operations.sh" (from main hook)
#
# This module handles:
#   - Section 9: Indirect File Operations (cp, dd, tee, rsync, etc.)
#   - Section 10: Glob Pattern Bypass Prevention
#   - Section 11: Interpreter-Based File Write Detection
#
# Required variables (set by caller):
#   - COMMAND: The bash command being checked
#   - LIB_DIR: Path to security-lib.sh
#   - PROTECTED_PATHS: Array of protected paths
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
# SECTION 9: Indirect File Operations (cp, dd, tee, rsync, scp, install, ln)
# =============================================================================
# Problem: L0/L3 deny patterns only do PREFIX matching, not path-specific
# Solution: Regex-based detection of copy/write operations targeting protected paths

# Skip Section 9 if the final destination is /tmp/claude (safe scratch space).
# This prevents false positives when READING from protected paths to /tmp/claude
# (e.g., "cp .claude/hooks/hook.sh /tmp/claude/backup.sh").
# For compound commands, each segment after && or ; is a separate command, so
# "cp file /tmp/claude/x && cp /tmp/claude/x .claude/settings.json" still checks
# the second cp because /tmp/claude appears as source, not final destination.
# Note: is_path_or_glob_targeted strips /tmp/claude from path matching internally,
# but the protected source path still appears in the command and triggers the check.
_skip_section9=false
if [[ "$COMMAND" =~ [[:space:]]/tmp/claude($|/)[^[:space:]]*[[:space:]]*$ ]] && ! [[ "$COMMAND" =~ (&&|\|\||;) ]]; then
  _skip_section9=true
elif [[ "$COMMAND" =~ \>/tmp/claude($|/) ]]; then
  _skip_section9=true
fi

if [[ "$_skip_section9" == "false" ]]; then
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

# =============================================================================
# SECTION 11: Interpreter-Based File Write Detection
# =============================================================================
# Problem: Inline code passed to interpreters (python3 -c, perl -e, etc.)
# can write to protected paths, bypassing file operation checks.

# Protected path patterns to check for in inline interpreter code
INTERPRETER_PROTECTED_PATTERNS=('.claude/' '.state/' '.codeflow/config/' 'settings.json' 'enforcement-policy')

# Check for interpreter-based file writes to protected paths
# Detects: python3? -c, perl -e, ruby -e, node -e
check_interpreter_write() {
  local cmd="$1"

  # Match interpreter with inline code flag
  if [[ "$cmd" =~ (^|[[:space:]])(python3?|perl|ruby|node)[[:space:]]+-[ce][[:space:]] ]]; then
    for pattern in "${INTERPRETER_PROTECTED_PATTERNS[@]}"; do
      if [[ "$cmd" == *"$pattern"* ]]; then
        block_command "Interpreter File Write" \
          "Interpreter-based file write to protected path detected. Use Edit/Write tools instead." \
          "$pattern"
      fi
    done
  fi
}

check_interpreter_write "$COMMAND"

# All file operation checks passed
return 0
