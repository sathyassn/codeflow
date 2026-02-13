#!/usr/bin/env bash
# Purpose:   Reject a staged protected file edit
# Usage:     cf-reject-staged-edit.sh <file-path>
# Platform:  macOS/Linux
#
# Rejects and cleans up a staged edit without applying it.
# Logs the rejection for audit purposes.
#
# Arguments:
#   $1 - Path to the protected file
#
# Exit codes:
#   - 0: Edit successfully rejected
#   - 1: Error (no staged edit found)

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-reject-staged-edit.sh - Reject staged protected file edits

USAGE:
    cf-reject-staged-edit.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the protected file

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -r, --reason     Reason for rejection (logged)

DESCRIPTION:
    Rejects a staged edit and removes all staging files.
    The rejection is logged for security audit purposes.

EXAMPLES:
    cf-reject-staged-edit.sh .claude/CLAUDE.md
    cf-reject-staged-edit.sh -r "Changes not approved" config.json

SEE ALSO:
    cf-stage-edit.sh, cf-apply-staged-edit.sh
EOF
}

show_version() {
    echo "cf-reject-staged-edit.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

REASON=""

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
        -r|--reason)
            if [[ $# -lt 2 ]]; then
                echo "Error: --reason requires a value" >&2
                exit 1
            fi
            REASON="$2"
            shift 2
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-reject-staged-edit.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-reject-staged-edit.sh <file-path>" >&2
    exit 1
fi

FILE_PATH="$1"

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
STAGING_DIR="/tmp/claude/managed/codeflow/protected-edits"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library if available
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# LOCATE STAGED FILES
# =============================================================================

# Generate safe filename from path
SAFE_NAME=$(echo "$FILE_PATH" | sed 's/[\/]/_/g')
STAGED_FILE="$STAGING_DIR/${SAFE_NAME}.staged"
ORIGINAL_FILE="$STAGING_DIR/${SAFE_NAME}.original"
METADATA_FILE="$STAGING_DIR/${SAFE_NAME}.metadata.json"

# Check if staged edit exists
if [[ ! -f "$STAGED_FILE" ]]; then
    echo "Error: No staged edit found for: $FILE_PATH" >&2
    exit 1
fi

# =============================================================================
# LOGGING
# =============================================================================

if declare -f log_security_event &>/dev/null; then
    log_security_event "protection" "protected_edit_rejected" "Edit" "$FILE_PATH" "${REASON:-user rejected}"
fi

# =============================================================================
# CLEANUP
# =============================================================================

rm -f "$STAGED_FILE" "$ORIGINAL_FILE" "$METADATA_FILE"

# =============================================================================
# OUTPUT
# =============================================================================

echo "Staged edit rejected and cleaned up"
echo ""
echo "File: $FILE_PATH"
if [[ -n "$REASON" ]]; then
    echo "Reason: $REASON"
fi

exit 0
