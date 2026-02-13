#!/usr/bin/env bash
# Purpose:   Common configuration, logging, and path validation for cf-protect-resources.sh
# Location:  .codeflow/scripts/security/protection/lib/cf-protection-common.sh
# Usage:     source lib/cf-protection-common.sh (from cf-protect-resources.sh)
# Version:   1.0.0
#
# This library provides shared configuration and utility functions for the
# cf-protect-resources.sh modular architecture. It must be sourced, not executed.
#
# Provides:
#   - Configuration variables (PROJECT_ROOT, OS detection, colors)
#   - Path constants (EXTENDED_LIST, ADHOC_LIST, AUDIT_LOG)
#   - CORE_PATHS array (hardcoded security-critical paths)
#   - Logging functions (log_info, log_success, log_warn, log_error, log_audit)
#   - Path validation (validate_path)

# Prevent direct execution
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "ERROR: This script must be sourced, not executed directly." >&2
    exit 1
fi

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

# Script location (set by sourcing script, provide fallback)
SCRIPT_DIR="${SCRIPT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

# Project root (assumes script is in .codeflow/scripts/security/protection/)
PROJECT_ROOT="${PROJECT_ROOT:-$(cd "$SCRIPT_DIR/../../../.." && pwd)}"

# Detect OS
# shellcheck disable=SC2034
if [[ "$OSTYPE" == "darwin"* ]]; then
    OS="macos"
    ROOT_GROUP="wheel"
else
    OS="linux"
    ROOT_GROUP="root"
fi

# List files (codeflow paths - under config/enforcement/protection/)
# shellcheck disable=SC2034
EXTENDED_LIST=".codeflow/config/enforcement/protection/protected-extended.list"
# shellcheck disable=SC2034
ADHOC_LIST=".codeflow/config/enforcement/protection/protected-adhoc.list"

# Audit log location - in .state/logs/security/
# shellcheck disable=SC2034
AUDIT_LOG=".state/logs/security/protection-audit.log"

# Core protected paths for CodeFlow V4 (hardcoded, cannot be removed)
# Note: .claude/CLAUDE.md is NOT protected - Claude should be able to edit it
# shellcheck disable=SC2034
CORE_PATHS=(
    ".claude/hooks/codeflow"
    ".claude/settings.json"
    ".claude/settings.local.json"
    ".claude/settings-templates"
    ".codeflow/scripts/security"
    ".codeflow/scripts/git-hooks"
    ".codeflow/scripts/shell-lib"
    ".codeflow/config/enforcement"
    ".github/workflows"
)

# Colors for output
# shellcheck disable=SC2034
RED='\033[0;31m'
# shellcheck disable=SC2034
GREEN='\033[0;32m'
# shellcheck disable=SC2034
YELLOW='\033[1;33m'
# shellcheck disable=SC2034
BLUE='\033[0;34m'
# shellcheck disable=SC2034
CYAN='\033[0;36m'
# shellcheck disable=SC2034
NC='\033[0m' # No Color

# =============================================================================
# LOGGING FUNCTIONS
# =============================================================================

log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
}

# Audit logging - records protection operations for accountability
log_audit() {
    local action="$1"
    local path="$2"
    local user="${SUDO_USER:-$(whoami)}"
    local timestamp
    timestamp=$(date '+%Y-%m-%d %H:%M:%S')

    local audit_dir
    audit_dir="$(dirname "$PROJECT_ROOT/$AUDIT_LOG")"

    # Ensure audit log directory exists
    if ! mkdir -p "$audit_dir" 2>/dev/null; then
        return 0
    fi

    # Write audit entry - fail silently if unable
    echo "[$timestamp] USER=$user ACTION=$action PATH=$path" >> "$PROJECT_ROOT/$AUDIT_LOG" 2>/dev/null || true
}

# =============================================================================
# PATH VALIDATION
# =============================================================================

validate_path() {
    local path="$1"

    # 1. Must not be empty
    if [[ -z "$path" ]]; then
        log_error "Path cannot be empty"
        return 1
    fi

    # 2. Must be relative (no absolute paths)
    if [[ "$path" == /* ]]; then
        log_error "Absolute paths not allowed. Use relative path from project root."
        return 1
    fi

    # 3. No parent directory escape
    if [[ "$path" == *".."* ]]; then
        log_error "Parent directory references (..) not allowed."
        return 1
    fi

    # 4. Path must exist
    local full_path="$PROJECT_ROOT/$path"
    if [[ ! -e "$full_path" ]]; then
        log_error "Path does not exist: $path"
        return 1
    fi

    # 5. Resolve and verify within project
    local resolved
    if [[ -d "$full_path" ]]; then
        resolved=$(cd "$full_path" && pwd)
    else
        resolved=$(cd "$(dirname "$full_path")" && pwd)/$(basename "$full_path")
    fi

    if [[ "$resolved" != "$PROJECT_ROOT"* ]]; then
        log_error "Path must be within project directory."
        return 1
    fi

    return 0
}
