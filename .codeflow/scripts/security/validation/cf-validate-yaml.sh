#!/usr/bin/env bash
# Purpose:   Validate YAML file syntax
# Usage:     cf-validate-yaml.sh <file-path>
# Platform:  macOS/Linux
#
# Validates YAML files using Python yaml module or yq.
# Returns appropriate exit code for validation status.
#
# Arguments:
#   $1 - Path to YAML file to validate
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
cf-validate-yaml.sh - Validate YAML file syntax

USAGE:
    cf-validate-yaml.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the YAML file to validate

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -q, --quiet      Suppress output (exit code only)
    -s, --strict     Enable strict mode (no duplicate keys)

DESCRIPTION:
    Validates YAML files using:
      - python3 yaml module: Preferred (with safe_load)
      - yq: Alternative (if available)

EXAMPLES:
    cf-validate-yaml.sh config.yaml
    cf-validate-yaml.sh --strict workflow.yml

SEE ALSO:
    cf-validate-json.sh, cf-validate-shell.sh
EOF
}

show_version() {
    echo "cf-validate-yaml.sh version $VERSION"
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
            echo "Run 'cf-validate-yaml.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-validate-yaml.sh <file-path>" >&2
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

# Try python3 yaml module first
if command -v python3 &>/dev/null && python3 -c "import yaml" 2>/dev/null; then
    if [[ "$STRICT" == "true" ]]; then
        # Strict mode: check for duplicate keys
        PYTHON_SCRIPT='
import yaml, sys

class NoDuplicatesLoader(yaml.SafeLoader):
    pass

def check_duplicates(loader, node, deep=False):
    mapping = {}
    for key_node, value_node in node.value:
        key = loader.construct_object(key_node, deep=deep)
        if key in mapping:
            raise yaml.constructor.ConstructorError(
                "while constructing a mapping", node.start_mark,
                f"found duplicate key ({key})", key_node.start_mark
            )
        mapping[key] = loader.construct_object(value_node, deep=deep)
    return mapping

NoDuplicatesLoader.add_constructor(
    yaml.resolver.BaseResolver.DEFAULT_MAPPING_TAG,
    check_duplicates
)

try:
    with open(sys.argv[1], "r") as f:
        yaml.load(f, Loader=NoDuplicatesLoader)
    sys.exit(0)
except yaml.YAMLError as e:
    print(f"YAML Error: {e}", file=sys.stderr)
    sys.exit(1)
'
    else
        PYTHON_SCRIPT='
import yaml, sys

try:
    with open(sys.argv[1], "r") as f:
        yaml.safe_load(f)
    sys.exit(0)
except yaml.YAMLError as e:
    print(f"YAML Error: {e}", file=sys.stderr)
    sys.exit(1)
'
    fi

    if ! python3 -c "$PYTHON_SCRIPT" "$FILE_PATH" 2>/dev/null; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: Invalid YAML syntax" >&2
        if [[ "$QUIET" == "false" ]]; then
            python3 -c "$PYTHON_SCRIPT" "$FILE_PATH" 2>&1
        fi
        exit 1
    fi

    [[ "$QUIET" == "false" ]] && echo "PASS: Valid YAML"

# Fallback to yq
elif command -v yq &>/dev/null; then
    if ! yq eval '.' "$FILE_PATH" >/dev/null 2>&1; then
        [[ "$QUIET" == "false" ]] && echo "FAIL: Invalid YAML syntax" >&2
        if [[ "$QUIET" == "false" ]]; then
            yq eval '.' "$FILE_PATH" 2>&1
        fi
        exit 1
    fi

    [[ "$QUIET" == "false" ]] && echo "PASS: Valid YAML"

else
    [[ "$QUIET" == "false" ]] && echo "Error: No YAML validator available (python3 yaml or yq)" >&2
    exit 1
fi

exit 0
