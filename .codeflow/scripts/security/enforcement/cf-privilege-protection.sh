#!/usr/bin/env bash
# Purpose:   Privilege escalation security checks
# Usage:     source "enforcement/cf-privilege-protection.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles:
#   - Shell chaining with privilege escalation (&&, ||, ;)
#   - Direct privilege escalation (sudo, su, doas, pkexec)
#   - Script execution bypass (bash -c, sh -c, eval, source)
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
# CONSTANTS
# =============================================================================

# Privilege escalation commands
readonly PRIV_ESC_CMDS=("sudo" "su" "doas" "pkexec" "runuser")

# =============================================================================
# SHELL CHAINING CHECKS
# =============================================================================
# Detect attempts to chain privilege escalation commands

for cmd in "${PRIV_ESC_CMDS[@]}"; do
    # && or || chaining: cmd1 && sudo cmd2
    if [[ "$COMMAND" =~ ("&&"|"||")[[:space:]]*"$cmd"[[:space:]] ]]; then
        block_command \
            "Privilege Escalation" \
            "Chained $cmd command" \
            "&& $cmd / || $cmd"
    fi

    # Semicolon chaining: cmd1; sudo cmd2
    if [[ "$COMMAND" == *"; $cmd"* ]]; then
        block_command \
            "Privilege Escalation" \
            "Semicolon chained $cmd" \
            "; $cmd"
    fi

    # Pipe to privilege command: echo password | sudo -S cmd
    if [[ "$COMMAND" =~ \|[[:space:]]*"$cmd"[[:space:]] ]]; then
        block_command \
            "Privilege Escalation" \
            "Piped to $cmd command" \
            "| $cmd"
    fi
done

# =============================================================================
# DIRECT PRIVILEGE ESCALATION
# =============================================================================
# Block direct use of privilege escalation commands

for cmd in "${PRIV_ESC_CMDS[@]}"; do
    # Command at start of line
    if [[ "$COMMAND" =~ ^"$cmd"[[:space:]] ]] || [[ "$COMMAND" == "$cmd" ]]; then
        block_command \
            "Privilege Escalation" \
            "$cmd command not permitted" \
            "$cmd"
    fi
done

# =============================================================================
# SCRIPT EXECUTION BYPASS
# =============================================================================
# Block attempts to bypass protections via script execution

# bash -c with quotes
if [[ "$COMMAND" =~ bash[[:space:]]+-c[[:space:]]+[\"\'] ]]; then
    block_command \
        "Script Bypass" \
        "bash -c execution" \
        "bash -c"
fi

# sh -c with quotes
if [[ "$COMMAND" =~ sh[[:space:]]+-c[[:space:]]+[\"\'] ]]; then
    block_command \
        "Script Bypass" \
        "sh -c execution" \
        "sh -c"
fi

# zsh -c with quotes
if [[ "$COMMAND" =~ zsh[[:space:]]+-c[[:space:]]+[\"\'] ]]; then
    block_command \
        "Script Bypass" \
        "zsh -c execution" \
        "zsh -c"
fi

# eval command - blocks all eval usage (potential bypass vector)
if [[ "$COMMAND" =~ ^eval[[:space:]] ]] || [[ "$COMMAND" =~ [[:space:]]eval[[:space:]] ]]; then
    block_command \
        "Script Bypass" \
        "eval command not permitted" \
        "eval"
fi

# source command - blocks external script sourcing
if [[ "$COMMAND" =~ ^source[[:space:]] ]]; then
    block_command \
        "Script Bypass" \
        "source command not permitted" \
        "source"
fi

# . (dot) command - blocks external script sourcing
# Pattern: starts with . followed by space and then something
if [[ "$COMMAND" =~ ^\.\ +[^\.] ]]; then
    block_command \
        "Script Bypass" \
        "dot source command not permitted" \
        ". (dot source)"
fi

# =============================================================================
# ENV VAR MANIPULATION
# =============================================================================
# Block attempts to manipulate environment for privilege bypass

# Setting PATH to include dangerous directories
if [[ "$COMMAND" =~ PATH=.*:/tmp ]]; then
    block_command \
        "Environment Manipulation" \
        "PATH modification with /tmp" \
        "PATH=/tmp"
fi

# LD_PRELOAD attempts
if [[ "$COMMAND" =~ LD_PRELOAD= ]]; then
    block_command \
        "Environment Manipulation" \
        "LD_PRELOAD injection attempt" \
        "LD_PRELOAD"
fi

# LD_LIBRARY_PATH attempts
if [[ "$COMMAND" =~ LD_LIBRARY_PATH= ]]; then
    block_command \
        "Environment Manipulation" \
        "LD_LIBRARY_PATH injection attempt" \
        "LD_LIBRARY_PATH"
fi

# All privilege protection checks passed
return 0
