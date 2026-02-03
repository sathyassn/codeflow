#!/usr/bin/env bash
# CodeFlow Test Framework: Common Utilities
# Location: .codeflow/testing/lib/test-common.sh

set -euo pipefail

# ============================================================================
# VERSION
# ============================================================================

# shellcheck disable=SC2034  # Used by sourcing scripts
readonly TEST_FRAMEWORK_VERSION="1.0.0"

# ============================================================================
# COLORS (exported for use by sourcing scripts)
# ============================================================================

# shellcheck disable=SC2034  # Colors used by sourcing scripts
if [[ -t 1 ]]; then
    readonly RED='\033[0;31m'
    readonly GREEN='\033[0;32m'
    readonly YELLOW='\033[0;33m'
    readonly BLUE='\033[0;34m'
    readonly CYAN='\033[0;36m'
    readonly GRAY='\033[0;90m'
    readonly BOLD='\033[1m'
    readonly NC='\033[0m'  # No Color
else
    readonly RED=''
    readonly GREEN=''
    readonly YELLOW=''
    readonly BLUE=''
    readonly CYAN=''
    readonly GRAY=''
    readonly BOLD=''
    readonly NC=''
fi

# ============================================================================
# TEST COUNTERS
# ============================================================================

export TEST_PASS_COUNT=0
export TEST_FAIL_COUNT=0
export TEST_SKIP_COUNT=0
export TEST_TOTAL_COUNT=0

# ============================================================================
# CATEGORY DEFINITIONS
# ============================================================================

# Note: Using function-based lookup for bash 3.2 compatibility (macOS default)
# Associative arrays require bash 4.0+
get_category_path() {
    local category="$1"
    case "$category" in
        hooks-pre-tool-use)         echo "hooks/pre-tool-use" ;;
        hooks-post-tool-use)        echo "hooks/post-tool-use" ;;
        hooks-session-start)        echo "hooks/session-start" ;;
        hooks-session-end)          echo "hooks/session-end" ;;
        hooks-stop)                 echo "hooks/stop" ;;
        hooks-user-prompt)          echo "hooks/user-prompt-submit" ;;
        scripts-db)                 echo "scripts/db" ;;
        scripts-memory)             echo "scripts/memory" ;;
        scripts-coordination)       echo "scripts/coordination" ;;
        scripts-lib)                echo "scripts/codeflow_py_lib" ;;
        scripts-codeflow-py-lib)    echo "scripts/codeflow_py_lib" ;;
        scripts-shell-lib)          echo "scripts/shell-lib" ;;
        scripts-security)           echo "scripts/security" ;;
        scripts-state)              echo "scripts/state" ;;
        scripts-worktree)           echo "scripts/worktree" ;;
        scripts-health)             echo "scripts/health" ;;
        scripts-commands)           echo "scripts/commands" ;;
        consistency)                echo "consistency" ;;
        autorun)                    echo "autorun" ;;
        *)                          echo "" ;;
    esac
}

# ============================================================================
# PRIORITY LEVELS
# ============================================================================

readonly PRIORITY_CRITICAL="CRITICAL"
readonly PRIORITY_HIGH="HIGH"
readonly PRIORITY_MEDIUM="MEDIUM"
readonly PRIORITY_LOW="LOW"

export PRIORITY_CRITICAL PRIORITY_HIGH PRIORITY_MEDIUM PRIORITY_LOW

# ============================================================================
# LOGGING
# ============================================================================

log_info() {
    echo -e "${BLUE}[INFO]${NC} $*"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

log_debug() {
    if [[ "${DEBUG:-0}" == "1" ]]; then
        echo -e "${GRAY}[DEBUG]${NC} $*" >&2
    fi
}

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

get_category_dir() {
    local category="$1"
    get_category_path "$category"
}

get_timestamp() {
    date -u +%Y-%m-%dT%H:%M:%SZ
}

get_repo_root() {
    git rev-parse --show-toplevel 2>/dev/null || pwd
}
