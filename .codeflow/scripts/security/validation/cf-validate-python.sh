#!/usr/bin/env bash
# Purpose:   Validate Python script syntax and style
# Usage:     cf-validate-python.sh <file-path>
# Platform:  macOS/Linux
#
# Validates Python scripts using py_compile and ruff/flake8.
# Returns appropriate exit code for validation status.
#
# Arguments:
#   $1 - Path to Python script to validate
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
cf-validate-python.sh - Validate Python script syntax and style

USAGE:
    cf-validate-python.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the Python script to validate

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -q, --quiet      Suppress output (exit code only)
    -s, --strict     Enable strict mode (all warnings are errors)

DESCRIPTION:
    Validates Python scripts using:
      - python3 -m py_compile: Syntax check
      - ruff: Fast linter (preferred, if available)
      - flake8: Traditional linter (fallback)

    By default, only errors cause failure. Use --strict for warnings.

EXAMPLES:
    cf-validate-python.sh script.py
    cf-validate-python.sh --strict /path/to/script.py

SEE ALSO:
    cf-validate-shell.sh, cf-validate-json.sh
EOF
}

show_version() {
    echo "cf-validate-python.sh version $VERSION"
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
            echo "Run 'cf-validate-python.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-validate-python.sh <file-path>" >&2
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

# Check 1: Python syntax
if command -v python3 &>/dev/null; then
    if ! python3 -m py_compile "$FILE_PATH" 2>/dev/null; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: Python syntax check failed" >&2
        if [[ "$QUIET" == "false" ]]; then
            python3 -m py_compile "$FILE_PATH" 2>&1 | head -20
        fi
        ERRORS=$((ERRORS + 1))
    else
        [[ "$QUIET" == "false" ]] && echo "PASS: Python syntax check"
    fi
else
    [[ "$QUIET" == "false" ]] && echo "SKIP: python3 not available"
fi

# Check 2: Ruff or Flake8
LINTER=""
LINTER_OPTS=()

if command -v ruff &>/dev/null; then
    LINTER="ruff"
    LINTER_OPTS=(check)
    if [[ "$STRICT" != "true" ]]; then
        LINTER_OPTS+=(--ignore E501)  # Ignore line length in non-strict
    fi
elif command -v flake8 &>/dev/null; then
    LINTER="flake8"
    if [[ "$STRICT" != "true" ]]; then
        LINTER_OPTS+=(--ignore=E501)  # Ignore line length in non-strict
    fi
fi

if [[ -n "$LINTER" ]]; then
    if ! "$LINTER" "${LINTER_OPTS[@]}" "$FILE_PATH" >/dev/null 2>&1; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: $LINTER analysis failed" >&2
        if [[ "$QUIET" == "false" ]]; then
            "$LINTER" "${LINTER_OPTS[@]}" "$FILE_PATH" 2>&1 | head -30
        fi
        ERRORS=$((ERRORS + 1))
    else
        [[ "$QUIET" == "false" ]] && echo "PASS: $LINTER analysis"
    fi
else
    [[ "$QUIET" == "false" ]] && echo "SKIP: No Python linter available (ruff or flake8)"
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
