#!/usr/bin/env bash
# Purpose:   Validate shell script syntax and style
# Usage:     cf-validate-shell.sh <file-path>
# Platform:  macOS/Linux
#
# Validates shell scripts using shellcheck and bash -n.
# Returns appropriate exit code for validation status.
#
# Arguments:
#   $1 - Path to shell script to validate
#
# Exit codes:
#   - 0: Validation passed
#   - 1: Validation failed or error

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-validate-shell.sh - Validate shell script syntax and style

USAGE:
    cf-validate-shell.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the shell script to validate

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -q, --quiet      Suppress output (exit code only)
    -s, --strict     Enable strict mode (all warnings are errors)

DESCRIPTION:
    Validates shell scripts using:
      - bash -n: Basic syntax check
      - shellcheck: Static analysis (if available)

    By default, only errors cause failure. Use --strict for warnings.

EXAMPLES:
    cf-validate-shell.sh script.sh
    cf-validate-shell.sh --strict /path/to/script.sh

SEE ALSO:
    cf-validate-python.sh, cf-validate-json.sh
EOF
}

show_version() {
    echo "cf-validate-shell.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

QUIET=false
STRICT=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help)
            show_help
            exit 0
            ;;
        -V|--version)
            show_version
            exit 0
            ;;
        -q|--quiet)
            QUIET=true
            shift
            ;;
        -s|--strict)
            STRICT=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-validate-shell.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-validate-shell.sh <file-path>" >&2
    exit 1
fi

FILE_PATH="$1"

# =============================================================================
# VALIDATION
# =============================================================================

if [[ ! -f "$FILE_PATH" ]]; then
    [[ "$QUIET" == "false" ]] && echo "Error: File not found: $FILE_PATH" >&2
    exit 1
fi

ERRORS=0

# Check 1: Basic bash syntax
if ! bash -n "$FILE_PATH" 2>/dev/null; then
    [[ "$QUIET" == "false" ]] && echo "FAIL: Bash syntax check failed" >&2
    if [[ "$QUIET" == "false" ]]; then
        bash -n "$FILE_PATH" 2>&1 | head -20
    fi
    ERRORS=$((ERRORS + 1))
else
    [[ "$QUIET" == "false" ]] && echo "PASS: Bash syntax check"
fi

# Check 2: ShellCheck (if available)
if command -v shellcheck &>/dev/null; then
    SHELLCHECK_OPTS=(-e SC1091)  # Ignore source file not found

    if [[ "$STRICT" == "true" ]]; then
        # In strict mode, treat warnings as errors
        SHELLCHECK_OPTS+=(-S warning)
    fi

    if ! shellcheck "${SHELLCHECK_OPTS[@]}" "$FILE_PATH" 2>/dev/null; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: ShellCheck analysis failed" >&2
        if [[ "$QUIET" == "false" ]]; then
            shellcheck "${SHELLCHECK_OPTS[@]}" "$FILE_PATH" 2>&1 | head -30
        fi
        ERRORS=$((ERRORS + 1))
    else
        [[ "$QUIET" == "false" ]] && echo "PASS: ShellCheck analysis"
    fi
else
    [[ "$QUIET" == "false" ]] && echo "SKIP: ShellCheck not available"
fi

# =============================================================================
# RESULT
# =============================================================================

if [[ $ERRORS -gt 0 ]]; then
    [[ "$QUIET" == "false" ]] && echo "" && echo "Validation FAILED with $ERRORS error(s)"
    exit 1
fi

[[ "$QUIET" == "false" ]] && echo "" && echo "Validation PASSED"
exit 0
