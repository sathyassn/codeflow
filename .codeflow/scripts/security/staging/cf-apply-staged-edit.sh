#!/usr/bin/env bash
# Purpose:   Apply a staged protected file edit after user approval
# Usage:     cf-apply-staged-edit.sh <file-path>
# Platform:  macOS/Linux
#
# Applies a previously staged edit to a protected file.
# Creates timestamped backup, validates syntax, and logs the change.
#
# Arguments:
#   $1 - Path to the protected file
#
# Exit codes:
#   - 0: Edit successfully applied
#   - 1: Error (no staged edit, validation failed, etc.)

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-apply-staged-edit.sh - Apply staged protected file edits

USAGE:
    cf-apply-staged-edit.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the protected file

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -f, --force      Skip syntax validation

DESCRIPTION:
    Applies a staged edit to a protected file after user review.
    Before applying, the script:
      - Creates a timestamped backup
      - Validates the staged file syntax (for known types)
      - Verifies the original hasn't changed
      - Preserves file permissions

EXAMPLES:
    cf-apply-staged-edit.sh .claude/CLAUDE.md
    cf-apply-staged-edit.sh --force config/settings.json

SEE ALSO:
    cf-stage-edit.sh, cf-reject-staged-edit.sh, cf-rollback-edit.sh
EOF
}

show_version() {
    echo "cf-apply-staged-edit.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

FORCE=false

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
        -f|--force)
            FORCE=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-apply-staged-edit.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-apply-staged-edit.sh <file-path>" >&2
    exit 1
fi

FILE_PATH="$1"

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
STAGING_DIR="/tmp/claude/managed/protected-edits"
BACKUP_DIR="$REPO_ROOT/.state/backups/protected"
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

# Resolve full path
if [[ "$FILE_PATH" == /* ]]; then
    FULL_PATH="$FILE_PATH"
else
    FULL_PATH="$REPO_ROOT/$FILE_PATH"
fi

# Check if staged edit exists
if [[ ! -f "$STAGED_FILE" ]]; then
    echo "Error: No staged edit found for: $FILE_PATH" >&2
    exit 1
fi

# =============================================================================
# VERIFY ORIGINAL HASN'T CHANGED
# =============================================================================

if [[ -f "$METADATA_FILE" ]] && command -v jq &>/dev/null; then
    STORED_CHECKSUM=$(jq -r '.original_checksum' "$METADATA_FILE")
    CURRENT_CHECKSUM=$(shasum -a 256 "$FULL_PATH" | cut -d' ' -f1)

    if [[ "$STORED_CHECKSUM" != "$CURRENT_CHECKSUM" ]]; then
        echo "Error: Original file has been modified since staging" >&2
        echo "The staged edit is based on an outdated version." >&2
        echo "Please re-stage the edit with the current version." >&2
        exit 1
    fi
fi

# =============================================================================
# SYNTAX VALIDATION
# =============================================================================

validate_syntax() {
    local file="$1"
    local extension="${file##*.}"

    case "$extension" in
        sh)
            if command -v shellcheck &>/dev/null; then
                if ! shellcheck -e SC1091 "$file" 2>/dev/null; then
                    return 1
                fi
            fi
            if ! bash -n "$file" 2>/dev/null; then
                return 1
            fi
            ;;
        py)
            if command -v python3 &>/dev/null; then
                if ! python3 -m py_compile "$file" 2>/dev/null; then
                    return 1
                fi
            fi
            ;;
        json)
            if command -v jq &>/dev/null; then
                if ! jq empty "$file" 2>/dev/null; then
                    return 1
                fi
            fi
            ;;
        yaml|yml)
            if command -v python3 &>/dev/null; then
                if ! python3 -c "import yaml; yaml.safe_load(open('$file'))" 2>/dev/null; then
                    return 1
                fi
            fi
            ;;
    esac

    return 0
}

if [[ "$FORCE" != "true" ]]; then
    if ! validate_syntax "$STAGED_FILE"; then
        echo "Error: Syntax validation failed for staged file" >&2
        echo "Use --force to skip validation (not recommended)" >&2
        exit 1
    fi
fi

# =============================================================================
# CREATE BACKUP
# =============================================================================

mkdir -p "$BACKUP_DIR"

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_NAME="${SAFE_NAME}.${TIMESTAMP}.backup"
BACKUP_PATH="$BACKUP_DIR/$BACKUP_NAME"

cp "$FULL_PATH" "$BACKUP_PATH"

# Store backup metadata
if command -v jq &>/dev/null; then
    jq -n \
        --arg file_path "$FILE_PATH" \
        --arg backup_path "$BACKUP_PATH" \
        --arg created_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        --arg session_id "${CODEFLOW_SESSION_ID:-unknown}" \
        '{
            file_path: $file_path,
            backup_path: $backup_path,
            created_at: $created_at,
            session_id: $session_id
        }' > "$BACKUP_PATH.meta.json"
fi

# =============================================================================
# APPLY THE EDIT
# =============================================================================

# Preserve permissions
ORIGINAL_PERMS=$(stat -f '%A' "$FULL_PATH" 2>/dev/null || stat -c '%a' "$FULL_PATH" 2>/dev/null || echo "644")

# Apply the staged content
cp "$STAGED_FILE" "$FULL_PATH"

# Restore permissions
chmod "$ORIGINAL_PERMS" "$FULL_PATH"

# =============================================================================
# CLEANUP STAGED FILES
# =============================================================================

rm -f "$STAGED_FILE" "$ORIGINAL_FILE" "$METADATA_FILE"

# =============================================================================
# LOGGING
# =============================================================================

if declare -f log_security_event &>/dev/null; then
    log_security_event "protection" "protected_edit_approved" "Edit" "$FILE_PATH" "applied with backup"
fi

# =============================================================================
# OUTPUT
# =============================================================================

echo "Edit applied successfully"
echo ""
echo "File: $FILE_PATH"
echo "Backup: $BACKUP_PATH"
echo ""
echo "To rollback if needed:"
echo "  cf-rollback-edit.sh \"$FILE_PATH\""

exit 0
