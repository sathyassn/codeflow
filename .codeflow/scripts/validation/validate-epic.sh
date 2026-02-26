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

readonly REQUIRED_FIELDS="id format_id title status area_type work_type"
readonly VALID_STATUSES="draft planning in_progress blocked complete archived"
readonly VALID_WORK_TYPES="FEAT FIX HTFX RFCT DOCS TEST CHOR CICD SPKE PLAN"
readonly VALID_AREA_TYPES="FRT BKD INF SHR DOC PLN"
readonly VALID_PRIORITIES="low normal high critical"
readonly PII_FILE_PATTERNS="auth login user session password credential token account profile identity"
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

warn() {
    echo "[WARN] $*" >&2
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
    local WARNINGS=0

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

    # Validate title field does not contain template sentinel
    local title_value
    title_value=$(get_field "title" "$frontmatter")
    if [[ -n "$title_value" ]]; then
        if [[ "$title_value" == *"{"* || "$title_value" == *"}"* ]]; then
            error "title field contains unfilled template sentinel '$title_value' — assign a real title"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate id field does not contain placeholder value
    local id_value
    id_value=$(get_field "id" "$frontmatter")
    if [[ -n "$id_value" ]]; then
        local id_upper
        id_upper=$(echo "$id_value" | tr '[:lower:]' '[:upper:]')
        if [[ "$id_upper" == *"PLACEHOLDER"* ]]; then
            error "id field contains placeholder value — assign a real epic ID"
            ERRORS=$((ERRORS + 1))
        fi
        if [[ "$id_value" == *"{"* || "$id_value" == *"}"* ]]; then
            error "id field contains unfilled template sentinel '$id_value' — assign a real epic ID"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate format_id pattern
    local format_id
    format_id=$(get_field "format_id" "$frontmatter")
    if [[ -n "$format_id" ]]; then
        if ! echo "$format_id" | grep -Eq "$FORMAT_ID_PATTERN"; then
            error "Invalid format_id '$format_id': must match $FORMAT_ID_PATTERN"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate area_type enum
    local area_type
    area_type=$(get_field "area_type" "$frontmatter")
    if [[ -n "$area_type" ]]; then
        local valid_at="false"
        local at
        for at in $VALID_AREA_TYPES; do
            if [[ "$area_type" == "$at" ]]; then
                valid_at="true"
                break
            fi
        done
        if [[ "$valid_at" == "false" ]]; then
            error "Invalid area_type '$area_type': must be one of: $VALID_AREA_TYPES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate format_id prefix matches area_type
    if [[ -n "$format_id" && -n "$area_type" ]]; then
        local format_prefix
        format_prefix=$(echo "$format_id" | sed 's/-EPC-.*//')
        if [[ "$format_prefix" != "$area_type" ]]; then
            error "format_id prefix '$format_prefix' does not match area_type '$area_type'"
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

    # Validate work_type enum
    local work_type
    work_type=$(get_field "work_type" "$frontmatter")
    if [[ -n "$work_type" ]]; then
        local valid_wt="false"
        local wt
        for wt in $VALID_WORK_TYPES; do
            if [[ "$work_type" == "$wt" ]]; then
                valid_wt="true"
                break
            fi
        done
        if [[ "$valid_wt" == "false" ]]; then
            error "Invalid work_type '$work_type': must be one of: $VALID_WORK_TYPES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate priority enum (if present)
    local priority
    priority=$(get_field "priority" "$frontmatter")
    if [[ -n "$priority" ]]; then
        local valid_pr="false"
        local pr
        for pr in $VALID_PRIORITIES; do
            if [[ "$priority" == "$pr" ]]; then
                valid_pr="true"
                break
            fi
        done
        if [[ "$valid_pr" == "false" ]]; then
            error "Invalid priority '$priority': must be one of: $VALID_PRIORITIES"
            ERRORS=$((ERRORS + 1))
        fi
    fi

    # Validate filename matches format_id (warning only)
    if [[ -n "$format_id" ]]; then
        local basename
        basename=$(basename "$file_path" .md)
        if [[ "$basename" != "$format_id" ]]; then
            warn "Filename '$basename.md' does not match format_id '$format_id'"
            WARNINGS=$((WARNINGS + 1))
        fi
    fi

    # Check file_scope for PII-handling file patterns (warning)
    local file_scope_raw
    file_scope_raw=$(echo "$frontmatter" | grep -E "^file_scope:" | head -1 | sed 's/^[^:]*:[[:space:]]*//' || true)
    if [[ -n "$file_scope_raw" && "$file_scope_raw" != "[]" && "$file_scope_raw" != "null" && "$file_scope_raw" != "~" ]]; then
        local file_scope_lower
        file_scope_lower=$(echo "$file_scope_raw" | tr '[:upper:]' '[:lower:]')
        local pii_pattern
        for pii_pattern in $PII_FILE_PATTERNS; do
            if echo "$file_scope_lower" | grep -q "$pii_pattern"; then
                warn "file_scope contains PII-sensitive path pattern '$pii_pattern' — verify PII handling compliance (encryption, hashing, sanitization, logging redaction) per OWASP/industry standards"
                WARNINGS=$((WARNINGS + 1))
                break
            fi
        done
    fi

    # ==========================================================================
    # RESULT
    # ==========================================================================

    if [[ $ERRORS -gt 0 ]]; then
        error "Validation FAILED with $ERRORS error(s)"
        exit 1
    fi

    if [[ $WARNINGS -gt 0 ]]; then
        info "Validation PASSED with $WARNINGS warning(s)"
    else
        info "Validation PASSED"
    fi
    exit 0
}

main "$@"
