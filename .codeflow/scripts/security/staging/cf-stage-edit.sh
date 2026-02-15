#!/usr/bin/env bash
# Purpose:   Stage protected file edits to temporary location for review
# Usage:     cf-stage-edit.sh <file-path> <new-content-file>
# Platform:  macOS/Linux
#
# Creates staged version of protected file for user review before applying.
# Generates metadata including checksums and timestamps.
#
# Arguments:
#   $1 - Path to the protected file to edit
#   $2 - Path to file containing new content
#
# Exit codes:
#   - 0: Edit successfully staged
#   - 1: Error (missing arguments, invalid paths, etc.)

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-stage-edit.sh - Stage protected file edits for review

USAGE:
    cf-stage-edit.sh [OPTIONS] <file-path> <new-content-file>

ARGUMENTS:
    file-path           Path to the protected file to edit
    new-content-file    Path to file containing new content

OPTIONS:
    -h, --help          Show this help message
    -V, --version       Show version information

DESCRIPTION:
    Stages edits to protected files in a temporary location for user review.
    Protected files cannot be edited directly - changes must go through the
    staging workflow for security review.

    Staged files are stored in:
      /tmp/claude/managed/codeflow/protected-edits/

EXAMPLES:
    cf-stage-edit.sh .claude/CLAUDE.md /tmp/new-content.md
    cf-stage-edit.sh project/mission.md changes.md

SEE ALSO:
    cf-apply-staged-edit.sh, cf-reject-staged-edit.sh
EOF
}

show_version() {
    echo "cf-stage-edit.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

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
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-stage-edit.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 2 ]]; then
    echo "Error: Missing required arguments" >&2
    echo "Usage: cf-stage-edit.sh <file-path> <new-content-file>" >&2
    exit 1
fi

FILE_PATH="$1"
NEW_CONTENT_FILE="$2"

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="${REPO_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}"
STAGING_DIR="/tmp/claude/managed/codeflow/protected-edits"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library if available
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# VALIDATION
# =============================================================================

# Resolve file path (support both absolute and relative)
if [[ "$FILE_PATH" == /* ]]; then
    FULL_PATH="$FILE_PATH"
else
    FULL_PATH="$REPO_ROOT/$FILE_PATH"
fi

# Check if new content file exists
if [[ ! -f "$NEW_CONTENT_FILE" ]]; then
    echo "Error: New content file not found: $NEW_CONTENT_FILE" >&2
    exit 1
fi

# Check if original file exists
if [[ ! -f "$FULL_PATH" ]]; then
    echo "Error: Original file not found: $FULL_PATH" >&2
    exit 1
fi

# =============================================================================
# CHECK FOR EXISTING STAGED EDIT
# =============================================================================

# Generate safe filename from path
SAFE_NAME=$(echo "$FILE_PATH" | sed 's/[\/]/_/g')
STAGED_FILE="$STAGING_DIR/${SAFE_NAME}.staged"
ORIGINAL_FILE="$STAGING_DIR/${SAFE_NAME}.original"
METADATA_FILE="$STAGING_DIR/${SAFE_NAME}.metadata.json"

if [[ -f "$STAGED_FILE" ]]; then
    echo "Error: A staged edit already exists for this file" >&2
    echo "Resolve the existing staged edit first:" >&2
    echo "  MUST: Delegate to cf-security teammate: handle-protected-resource $FILE_PATH" >&2
    echo "  The teammate will guide you through applying or rejecting the staged edit." >&2
    exit 1
fi

# =============================================================================
# STAGE THE EDIT
# =============================================================================

# Create staging directory
mkdir -p "$STAGING_DIR"

# Copy original for backup/comparison
cp "$FULL_PATH" "$ORIGINAL_FILE"

# Copy new content to staged location
cp "$NEW_CONTENT_FILE" "$STAGED_FILE"

# Generate checksums
# Cross-platform checksum (macOS: shasum, Linux: sha256sum)
_checksum() { shasum -a 256 "$1" 2>/dev/null || sha256sum "$1" 2>/dev/null; }
ORIGINAL_CHECKSUM=$(_checksum "$ORIGINAL_FILE" | cut -d' ' -f1)
STAGED_CHECKSUM=$(_checksum "$STAGED_FILE" | cut -d' ' -f1)

# Get TTL from config or use default (1 hour)
TTL_SECONDS=3600
if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
    CONFIG_TTL=$(jq -r '.managed_tmp.staging_ttl // 3600' "$CONFIG" 2>/dev/null || echo "3600")
    TTL_SECONDS="$CONFIG_TTL"
fi

# Calculate expiry
CURRENT_TIME=$(date +%s)
EXPIRY_TIME=$((CURRENT_TIME + TTL_SECONDS))
EXPIRY_ISO=$(date -r "$EXPIRY_TIME" -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -d "@$EXPIRY_TIME" -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null)

# Create metadata
if command -v jq &>/dev/null; then
    jq -n \
        --arg file_path "$FILE_PATH" \
        --arg full_path "$FULL_PATH" \
        --arg original_checksum "$ORIGINAL_CHECKSUM" \
        --arg staged_checksum "$STAGED_CHECKSUM" \
        --arg staged_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        --arg expires_at "$EXPIRY_ISO" \
        --argjson ttl_seconds "$TTL_SECONDS" \
        --arg session_id "${CODEFLOW_SESSION_ID:-unknown}" \
        '{
            file_path: $file_path,
            full_path: $full_path,
            original_checksum: $original_checksum,
            staged_checksum: $staged_checksum,
            staged_at: $staged_at,
            expires_at: $expires_at,
            ttl_seconds: $ttl_seconds,
            session_id: $session_id
        }' > "$METADATA_FILE"
else
    cat > "$METADATA_FILE" <<EOF
{
    "file_path": "$FILE_PATH",
    "full_path": "$FULL_PATH",
    "original_checksum": "$ORIGINAL_CHECKSUM",
    "staged_checksum": "$STAGED_CHECKSUM",
    "staged_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
    "expires_at": "$EXPIRY_ISO",
    "ttl_seconds": $TTL_SECONDS,
    "session_id": "${CODEFLOW_SESSION_ID:-unknown}"
}
EOF
fi

# Log staging event
if declare -f log_security_event &>/dev/null; then
    log_security_event "protection" "protected_edit_staged" "Edit" "$FILE_PATH" "staged for review"
fi

# =============================================================================
# OUTPUT
# =============================================================================

echo "Edit staged successfully"
echo ""
echo "File: $FILE_PATH"
echo "Staged: $STAGED_FILE"
echo "Expires: $EXPIRY_ISO"
echo ""
echo "To review diff:"
echo "  diff \"$ORIGINAL_FILE\" \"$STAGED_FILE\""
echo ""
echo "Next steps - use the security management skill:"
echo "  MUST: Delegate to cf-security teammate: handle-protected-resource $FILE_PATH"
echo "  The teammate will guide you through reviewing, applying, or rejecting the edit."

exit 0
