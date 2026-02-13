#!/usr/bin/env bash
# Purpose:   Validate JSON file syntax
# Usage:     cf-validate-json.sh <file-path>
# Platform:  macOS/Linux
#
# Validates JSON files using jq or python json module.
# Returns appropriate exit code for validation status.
#
# Arguments:
#   $1 - Path to JSON file to validate
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
cf-validate-json.sh - Validate JSON file syntax

USAGE:
    cf-validate-json.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the JSON file to validate

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -q, --quiet      Suppress output (exit code only)
    -p, --pretty     Output pretty-printed JSON on success

DESCRIPTION:
    Validates JSON files using:
      - jq: Preferred (if available)
      - python3 json module: Fallback

EXAMPLES:
    cf-validate-json.sh config.json
    cf-validate-json.sh --pretty data.json

SEE ALSO:
    cf-validate-yaml.sh, cf-validate-shell.sh
EOF
}

show_version() {
    echo "cf-validate-json.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

QUIET=false
PRETTY=false

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
        -p|--pretty)
            PRETTY=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-validate-json.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-validate-json.sh <file-path>" >&2
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

# Try jq first, then python3
if command -v jq &>/dev/null; then
    if ! jq empty "$FILE_PATH" 2>/dev/null; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: Invalid JSON syntax" >&2
        if [[ "$QUIET" == "false" ]]; then
            jq empty "$FILE_PATH" 2>&1
        fi
        exit 1
    fi

    [[ "$QUIET" == "false" ]] && echo "PASS: Valid JSON"

    if [[ "$PRETTY" == "true" ]]; then
        echo ""
        jq '.' "$FILE_PATH"
    fi

elif command -v python3 &>/dev/null; then
    if ! python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$FILE_PATH" 2>/dev/null; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: Invalid JSON syntax" >&2
        if [[ "$QUIET" == "false" ]]; then
            python3 -c "import json,sys; json.load(open(sys.argv[1]))" "$FILE_PATH" 2>&1
        fi
        exit 1
    fi

    [[ "$QUIET" == "false" ]] && echo "PASS: Valid JSON"

    if [[ "$PRETTY" == "true" ]]; then
        echo ""
        python3 -c "import json,sys; print(json.dumps(json.load(open(sys.argv[1])), indent=2))" "$FILE_PATH"
    fi

else
    [[ "$QUIET" == "false" ]] && echo "Error: No JSON validator available (jq or python3)" >&2
    exit 1
fi

exit 0
