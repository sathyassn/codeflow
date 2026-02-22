#!/usr/bin/env bash
# Purpose:   Validate epic YAML frontmatter fields against schema rules
# Usage:     validate-epic.sh [OPTIONS] <file-path>
# Arguments: file-path - Path to epic markdown file with YAML frontmatter
# Exit codes:
#   0 - Valid file
#   1 - Invalid file (errors reported to stderr)

set -euo pipefail

# =============================================================================
# CONSTANTS
# =============================================================================

readonly SCRIPT_NAME="${0##*/}"
readonly VERSION="1.0.0"

readonly REQUIRED_FIELDS="id format_id title status area_type"
readonly VALID_STATUSES="draft planning in_progress blocked complete archived"
readonly FORMAT_ID_PATTERN='^[A-Z]{2,4}-EPC-[0-9]{3}$'

# =============================================================================
# HELP AND VERSION
# =============================================================================

usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS] <file-path>

Validate epic YAML frontmatter fields against schema rules.

Arguments:
    file-path    Path to epic markdown file with YAML frontmatter

Options:
    -h, --help       Show this help message
    -V, --version    Show version information
    -q, --quiet      Suppress info messages, only show errors

Exit Codes:
    0    Valid file
    1    Invalid file (errors reported to stderr)

Examples:
    $SCRIPT_NAME project-management/epics/INF/INF-EPC-008/INF-EPC-008.md
    $SCRIPT_NAME --quiet epic.md
EOF
}

show_version() {
    echo "$SCRIPT_NAME version $VERSION"
}

# =============================================================================
# UTILITY FUNCTIONS
# =============================================================================

info() {
    if [[ "$QUIET" == "false" ]]; then
        echo "[INFO] $*"
    fi
}

error() {
    echo "[ERROR] $*" >&2
}

# Extract a field value from YAML frontmatter content
get_field() {
    local field="$1"
    local content="$2"
    local value

    value=$(echo "$content" | grep -E "^${field}:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' | sed 's/^"//' | sed 's/"$//' | sed "s/^'//" | sed "s/'$//" | sed 's/[[:space:]]*$//')

    if [[ -z "$value" || "$value" == "null" || "$value" == "~" ]]; then
        echo ""
    else
        echo "$value"
    fi
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

QUIET="false"

main() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            -V|--version)
                show_version
                exit 0
                ;;
            -q|--quiet)
                QUIET="true"
                shift
                ;;
            -*)
                error "Unknown option: $1"
                echo "Run '$SCRIPT_NAME --help' for usage." >&2
                exit 1
                ;;
            *)
                break
                ;;
        esac
    done

    if [[ $# -lt 1 ]]; then
        error "Missing required argument: file-path"
        echo "Usage: $SCRIPT_NAME <file-path>" >&2
        exit 1
    fi

    local file_path="$1"

    # ==========================================================================
    # FILE VALIDATION
    # ==========================================================================

    if [[ ! -f "$file_path" ]]; then
        error "File not found: $file_path"
        exit 1
    fi

    # ==========================================================================
    # EXTRACT FRONTMATTER
    # ==========================================================================

    local frontmatter=""
    local in_frontmatter="false"
    local found_start="false"
    local line_num=0

    while IFS= read -r line; do
        line_num=$((line_num + 1))
        if [[ "$line" == "---" ]]; then
            if [[ "$found_start" == "false" ]]; then
                found_start="true"
                in_frontmatter="true"
                continue
            else
                in_frontmatter="false"
                break
            fi
        fi
        if [[ "$in_frontmatter" == "true" ]]; then
            frontmatter="$frontmatter
$line"
        fi
    done < "$file_path"

    # Trim leading newline
    frontmatter="${frontmatter#
}"

    if [[ -z "$frontmatter" ]]; then
        error "No frontmatter fields found in $file_path"
        exit 1
    fi

    info "Validating: $file_path"

    # ==========================================================================
    # VALIDATION
    # ==========================================================================

    local ERRORS=0

    # Check required fields
    local field
    for field in $REQUIRED_FIELDS; do
        local value
        value=$(get_field "$field" "$frontmatter")
        if [[ -z "$value" ]]; then
            error "Missing required field: $field"
            ERRORS=$((ERRORS + 1))
        fi
    done

    # Validate format_id pattern
    local format_id
    format_id=$(get_field "format_id" "$frontmatter")
    if [[ -n "$format_id" ]]; then
        if ! echo "$format_id" | grep -Eq "$FORMAT_ID_PATTERN"; then
            error "Invalid format_id '$format_id': must match $FORMAT_ID_PATTERN"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate status value
    local status
    status=$(get_field "status" "$frontmatter")
    if [[ -n "$status" ]]; then
        local valid="false"
        local s
        for s in $VALID_STATUSES; do
            if [[ "$status" == "$s" ]]; then
                valid="true"
                break
            fi
        done
        if [[ "$valid" == "false" ]]; then
            error "Invalid status '$status': must be one of: $VALID_STATUSES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # ==========================================================================
    # RESULT
    # ==========================================================================

    if [[ $ERRORS -gt 0 ]]; then
        error "Validation FAILED with $ERRORS error(s)"
        exit 1
    fi

    info "Validation PASSED"
    exit 0
}

main "$@"
