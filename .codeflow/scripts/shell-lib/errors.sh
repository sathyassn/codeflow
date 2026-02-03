#!/usr/bin/env bash
# CodeFlow Shell Library: Error Handling
# Location: .codeflow/scripts/shell-lib/errors.sh

# Requires: common.sh, logging.sh
[[ -z "${CODEFLOW_LIB_VERSION:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
[[ -z "${LOG_LEVEL_ERROR:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/logging.sh"

# ============================================================================
# EXIT CODES
# ============================================================================

# Standard exit codes (exported for use by other scripts)
EXIT_SUCCESS=0
EXIT_GENERAL_ERROR=1
EXIT_BLOCKED=2          # Hook blocked operation
EXIT_INVALID_INPUT=3
EXIT_CONFIG_ERROR=4
EXIT_DB_ERROR=5
EXIT_PERMISSION_DENIED=6
EXIT_NOT_FOUND=7
EXIT_TIMEOUT=8
EXIT_DEPENDENCY_ERROR=9
readonly EXIT_SUCCESS EXIT_GENERAL_ERROR EXIT_BLOCKED EXIT_INVALID_INPUT
readonly EXIT_CONFIG_ERROR EXIT_DB_ERROR EXIT_PERMISSION_DENIED EXIT_NOT_FOUND
readonly EXIT_TIMEOUT EXIT_DEPENDENCY_ERROR
export EXIT_SUCCESS EXIT_GENERAL_ERROR EXIT_BLOCKED EXIT_INVALID_INPUT
export EXIT_CONFIG_ERROR EXIT_DB_ERROR EXIT_PERMISSION_DENIED EXIT_NOT_FOUND
export EXIT_TIMEOUT EXIT_DEPENDENCY_ERROR

# ============================================================================
# ERROR HANDLING
# ============================================================================

# Set up error trap
setup_error_trap() {
    trap 'handle_error $? $LINENO "$BASH_COMMAND"' ERR
}

# Error handler function
handle_error() {
    local exit_code="$1"
    local line_number="$2"
    local command="$3"

    log_error "Command failed at line ${line_number}: ${command}"
    log_error "Exit code: ${exit_code}"

    # Don't exit if we're being sourced
    [[ "${BASH_SOURCE[0]}" != "${0}" ]] || exit "$exit_code"
}

# ============================================================================
# ERROR FUNCTIONS
# ============================================================================

# Die with error message
die() {
    local message="$1"
    local exit_code="${2:-$EXIT_GENERAL_ERROR}"

    log_error "$message"
    exit "$exit_code"
}

# Die if condition fails
die_if() {
    local condition="$1"
    local message="$2"
    local exit_code="${3:-$EXIT_GENERAL_ERROR}"

    if eval "$condition"; then
        die "$message" "$exit_code"
    fi
}

# Die unless condition passes
die_unless() {
    local condition="$1"
    local message="$2"
    local exit_code="${3:-$EXIT_GENERAL_ERROR}"

    if ! eval "$condition"; then
        die "$message" "$exit_code"
    fi
}

# ============================================================================
# ASSERTIONS
# ============================================================================

# Assert file exists
assert_file_exists() {
    local file="$1"
    local message="${2:-File not found: $file}"

    [[ -f "$file" ]] || die "$message" $EXIT_NOT_FOUND
}

# Assert directory exists
assert_dir_exists() {
    local dir="$1"
    local message="${2:-Directory not found: $dir}"

    [[ -d "$dir" ]] || die "$message" $EXIT_NOT_FOUND
}

# Assert command exists
assert_command_exists() {
    local cmd="$1"
    local message="${2:-Required command not found: $cmd}"

    command_exists "$cmd" || die "$message" $EXIT_DEPENDENCY_ERROR
}

# Assert variable is set
assert_var_set() {
    local var_name="$1"
    local message="${2:-Required variable not set: $var_name}"

    [[ -n "${!var_name:-}" ]] || die "$message" $EXIT_INVALID_INPUT
}

# Assert not empty
assert_not_empty() {
    local value="$1"
    local message="${2:-Value cannot be empty}"

    [[ -n "$value" ]] || die "$message" $EXIT_INVALID_INPUT
}

# ============================================================================
# GRACEFUL DEGRADATION
# ============================================================================

# Try command with fallback
try_or_default() {
    local default="$1"
    shift
    "$@" 2>/dev/null || echo "$default"
}

# Try command, warn on failure
try_or_warn() {
    local message="$1"
    shift
    if ! "$@" 2>/dev/null; then
        log_warn "$message"
        return 1
    fi
    return 0
}

# Try command multiple times
retry() {
    local max_attempts="${1:-3}"
    local delay="${2:-1}"
    shift 2

    local attempt=1
    while [[ $attempt -le $max_attempts ]]; do
        if "$@"; then
            return 0
        fi

        if [[ $attempt -lt $max_attempts ]]; then
            log_debug "Attempt $attempt failed, retrying in ${delay}s..."
            sleep "$delay"
        fi

        ((attempt++))
    done

    log_error "All $max_attempts attempts failed"
    return 1
}
