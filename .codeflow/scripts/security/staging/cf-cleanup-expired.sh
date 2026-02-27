#!/usr/bin/env bash
# Purpose:   Cleanup expired staged edits
# Usage:     cf-cleanup-expired.sh
# Platform:  macOS/Linux
#
# Removes staged edits that have exceeded their TTL.
# Should be run periodically or on session start.
#
# Exit codes:
#   - 0: Cleanup successful

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-cleanup-expired.sh - Cleanup expired staged edits

USAGE:
    cf-cleanup-expired.sh [OPTIONS]

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -n, --dry-run    Show what would be cleaned up without removing
    -v, --verbose    Show detailed output

DESCRIPTION:
    Removes staged edits that have exceeded their TTL (time-to-live).
    By default, staged edits expire after 1 hour.

    Staged edit location: /tmp/claude/\${CF_PROJECT_ROOT}/managed/protected-edits/

    This script should be run:
      - On session start (via cf-session-start-init hook)
      - Periodically (cron job or similar)

EXAMPLES:
    cf-cleanup-expired.sh
    cf-cleanup-expired.sh --dry-run
    cf-cleanup-expired.sh --verbose

SEE ALSO:
    cf-stage-edit.sh
EOF
}

show_version() {
    echo "cf-cleanup-expired.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

DRY_RUN=false
VERBOSE=false

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
        -n|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -v|--verbose)
            VERBOSE=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-cleanup-expired.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"

# Source codeflow-env.sh for CF_PROJECT_ROOT
if [[ -f "$REPO_ROOT/.state/runtime/codeflow-env.sh" ]]; then
    # shellcheck source=/dev/null
    source "$REPO_ROOT/.state/runtime/codeflow-env.sh"
fi

STAGING_DIR="/tmp/claude/${CF_PROJECT_ROOT:-codeflow}/managed/protected-edits"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library if available
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# CLEANUP
# =============================================================================

if [[ ! -d "$STAGING_DIR" ]]; then
    [[ "$VERBOSE" == "true" ]] && echo "No staging directory found"
    exit 0
fi

CURRENT_TIME=$(date +%s)
CLEANED=0
KEPT=0

# Find all metadata files
for metadata_file in "$STAGING_DIR"/*.metadata.json; do
    [[ -f "$metadata_file" ]] || continue

    # Extract expiry time
    EXPIRES_AT=""
    if command -v jq &>/dev/null; then
        EXPIRES_AT=$(jq -r '.expires_at // empty' "$metadata_file" 2>/dev/null)
    fi

    if [[ -z "$EXPIRES_AT" ]]; then
        [[ "$VERBOSE" == "true" ]] && echo "Warning: No expiry in $metadata_file, skipping"
        continue
    fi

    # Convert ISO time to epoch
    EXPIRY_EPOCH=""
    if [[ "$OSTYPE" == "darwin"* ]]; then
        EXPIRY_EPOCH=$(date -j -f "%Y-%m-%dT%H:%M:%SZ" "$EXPIRES_AT" +%s 2>/dev/null || echo "")
    else
        EXPIRY_EPOCH=$(date -d "$EXPIRES_AT" +%s 2>/dev/null || echo "")
    fi

    if [[ -z "$EXPIRY_EPOCH" ]]; then
        [[ "$VERBOSE" == "true" ]] && echo "Warning: Could not parse expiry for $metadata_file"
        continue
    fi

    # Extract base name for related files
    BASE_NAME="${metadata_file%.metadata.json}"
    FILE_PATH=$(jq -r '.file_path // "unknown"' "$metadata_file" 2>/dev/null || echo "unknown")

    if [[ $CURRENT_TIME -gt $EXPIRY_EPOCH ]]; then
        [[ "$VERBOSE" == "true" ]] && echo "Expired: $FILE_PATH"

        if [[ "$DRY_RUN" == "false" ]]; then
            rm -f "$metadata_file" "${BASE_NAME}.staged" "${BASE_NAME}.original"

            # Log cleanup
            if declare -f log_security_event &>/dev/null; then
                log_security_event "protection" "staged_edit_expired" "" "$FILE_PATH" "auto cleanup"
            fi
        fi

        CLEANED=$((CLEANED + 1))
    else
        KEPT=$((KEPT + 1))
        [[ "$VERBOSE" == "true" ]] && echo "Active: $FILE_PATH (expires: $EXPIRES_AT)"
    fi
done

# =============================================================================
# OUTPUT
# =============================================================================

if [[ "$DRY_RUN" == "true" ]]; then
    echo "Dry run complete"
    echo "Would clean: $CLEANED expired staged edit(s)"
    echo "Would keep: $KEPT active staged edit(s)"
else
    if [[ $CLEANED -gt 0 ]] || [[ "$VERBOSE" == "true" ]]; then
        echo "Cleanup complete"
        echo "Cleaned: $CLEANED expired staged edit(s)"
        echo "Kept: $KEPT active staged edit(s)"
    fi
fi

exit 0
