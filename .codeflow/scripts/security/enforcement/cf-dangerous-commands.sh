#!/usr/bin/env bash
# Purpose:   Dangerous command detection and blocking
# Usage:     source "enforcement/cf-dangerous-commands.sh" (from main hook)
# Platform:  macOS/Linux
#
# This module handles detection and blocking of destructive commands:
#   - System destruction: rm -rf /, dd if=/dev/zero
#   - Dangerous permissions: chmod 777 /
#   - Disk operations: mkfs, > /dev/sd*
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
# DANGEROUS COMMAND PATTERNS
# =============================================================================

# Destructive patterns that should always be blocked
readonly DANGEROUS_PATTERNS=(
    "rm -rf /"
    "rm -rf /*"
    "rm -rf ~"
    "rm -rf ~/*"
    "dd if=/dev/zero"
    "dd if=/dev/random"
    "mkfs."
    "> /dev/sd"
    "chmod 777 /"
    "chmod -R 777 /"
    "chown -R /"
    ":(){:|:&};:"
)

# =============================================================================
# CHECK FUNCTIONS
# =============================================================================

# Check for destructive patterns
check_dangerous_patterns() {
    local cmd="$1"

    for pattern in "${DANGEROUS_PATTERNS[@]}"; do
        if [[ "$cmd" == *"$pattern"* ]]; then
            block_command \
                "Dangerous Command" \
                "Destructive operation detected" \
                "$pattern"
        fi
    done
}

# Check for recursive deletion of root or home
check_recursive_delete() {
    local cmd="$1"

    # rm with -r or -R flag targeting root
    if [[ "$cmd" =~ rm[[:space:]].*-[a-zA-Z]*[rR][a-zA-Z]*[[:space:]]+/($|[[:space:]]) ]]; then
        block_command \
            "Dangerous Command" \
            "Recursive deletion of root directory" \
            "rm -r /"
    fi

    # rm -rf with space before /
    if [[ "$cmd" =~ rm[[:space:]]+-rf[[:space:]]+/[[:space:]]* ]]; then
        block_command \
            "Dangerous Command" \
            "Recursive forced deletion of root" \
            "rm -rf /"
    fi
}

# Check for dangerous disk operations
check_disk_operations() {
    local cmd="$1"

    # dd to disk devices
    if [[ "$cmd" =~ dd[[:space:]].*of=/dev/(sd|hd|nvme|vd)[a-z] ]]; then
        block_command \
            "Dangerous Command" \
            "Direct disk write operation" \
            "dd of=/dev/*"
    fi

    # Format commands
    if [[ "$cmd" =~ (mkfs|mke2fs|mkswap)[[:space:]] ]]; then
        block_command \
            "Dangerous Command" \
            "Disk format operation" \
            "mkfs/mke2fs/mkswap"
    fi
}

# Check for dangerous permission changes
check_permission_changes() {
    local cmd="$1"

    # chmod 777 on system directories
    if [[ "$cmd" =~ chmod[[:space:]].*777[[:space:]].*(/|/etc|/var|/usr|/bin|/sbin) ]]; then
        block_command \
            "Dangerous Command" \
            "Dangerous permission change on system path" \
            "chmod 777 /"
    fi

    # Recursive permission change on root
    if [[ "$cmd" =~ chmod[[:space:]]+-R[[:space:]].*[[:space:]]/($|[[:space:]]) ]]; then
        block_command \
            "Dangerous Command" \
            "Recursive permission change on root" \
            "chmod -R /"
    fi
}

# Check for fork bomb patterns
check_fork_bomb() {
    local cmd="$1"

    # Classic fork bomb
    if [[ "$cmd" =~ :\(\)\{.*\|.*:\&\}\;: ]]; then
        block_command \
            "Dangerous Command" \
            "Fork bomb detected" \
            ":(){:|:&};:"
    fi

    # Variations
    if [[ "$cmd" =~ \.\(\)\{.*\|.*\.\&\}\;\. ]]; then
        block_command \
            "Dangerous Command" \
            "Fork bomb variation detected" \
            ".(){.|.&};."
    fi
}

# =============================================================================
# MAIN CHECK EXECUTION
# =============================================================================

# Run all dangerous command checks
check_dangerous_patterns "$COMMAND"
check_recursive_delete "$COMMAND"
check_disk_operations "$COMMAND"
check_permission_changes "$COMMAND"
check_fork_bomb "$COMMAND"

# All checks passed
return 0
