#!/usr/bin/env bash
# Purpose:   Rollback a protected file edit from backup
# Usage:     cf-rollback-edit.sh <file-path>
# Platform:  macOS/Linux
#
# Restores a protected file from its most recent backup.
# Logs the rollback for audit purposes.
#
# Arguments:
#   $1 - Path to the protected file
#
# Exit codes:
#   - 0: Rollback successful
#   - 1: Error (no backup found, etc.)

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-rollback-edit.sh - Rollback protected file edits from backup

USAGE:
    cf-rollback-edit.sh [OPTIONS] <file-path>

ARGUMENTS:
    file-path    Path to the protected file to rollback

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -l, --list       List available backups without rolling back
    -n, --number N   Rollback to the Nth most recent backup (default: 1)

DESCRIPTION:
    Restores a protected file from its most recent backup.
    Backups are created automatically when applying staged edits.

    Backup location: .state/backups/protected/

EXAMPLES:
    cf-rollback-edit.sh .claude/CLAUDE.md
    cf-rollback-edit.sh --list config.json
    cf-rollback-edit.sh -n 2 settings.json  # Second most recent

SEE ALSO:
    cf-apply-staged-edit.sh
EOF
}

show_version() {
    echo "cf-rollback-edit.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

LIST_ONLY=false
BACKUP_NUMBER=1

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
        -l|--list)
            LIST_ONLY=true
            shift
            ;;
        -n|--number)
            BACKUP_NUMBER="$2"
            shift 2
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-rollback-edit.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: file-path" >&2
    echo "Usage: cf-rollback-edit.sh <file-path>" >&2
    exit 1
fi

FILE_PATH="$1"

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
BACKUP_DIR="$REPO_ROOT/.state/backups/protected"
LIB_DIR="$REPO_ROOT/.codeflow/scripts/security/lib"

# Source security library if available
if [[ -f "$LIB_DIR/security-lib.sh" ]]; then
    export REPO_ROOT
    # shellcheck source=/dev/null
    source "$LIB_DIR/security-lib.sh"
fi

# =============================================================================
# RESOLVE PATHS
# =============================================================================

# Generate safe filename from path
SAFE_NAME=$(echo "$FILE_PATH" | sed 's/[\/]/_/g')

# Resolve full path
if [[ "$FILE_PATH" == /* ]]; then
    FULL_PATH="$FILE_PATH"
else
    FULL_PATH="$REPO_ROOT/$FILE_PATH"
fi

# =============================================================================
# FIND BACKUPS
# =============================================================================

# Find all backups for this file, sorted by date (newest first)
BACKUPS=()
while IFS= read -r line; do
    [[ -n "$line" ]] && BACKUPS+=("$line")
done < <(find "$BACKUP_DIR" -name "${SAFE_NAME}.*.backup" 2>/dev/null | sort -r)

if [[ ${#BACKUPS[@]} -eq 0 ]]; then
    echo "Error: No backups found for: $FILE_PATH" >&2
    exit 1
fi

# =============================================================================
# LIST MODE
# =============================================================================

if [[ "$LIST_ONLY" == "true" ]]; then
    echo "Available backups for: $FILE_PATH"
    echo ""
    for i in "${!BACKUPS[@]}"; do
        BACKUP="${BACKUPS[$i]}"
        BACKUP_NAME=$(basename "$BACKUP")
        # Extract timestamp from filename
        TIMESTAMP=$(echo "$BACKUP_NAME" | grep -oE '[0-9]{8}-[0-9]{6}' || echo "unknown")
        echo "  $((i + 1)). $TIMESTAMP ($BACKUP)"
    done
    exit 0
fi

# =============================================================================
# SELECT BACKUP
# =============================================================================

BACKUP_INDEX=$((BACKUP_NUMBER - 1))

if [[ $BACKUP_INDEX -ge ${#BACKUPS[@]} ]]; then
    echo "Error: Backup number $BACKUP_NUMBER not found" >&2
    echo "Only ${#BACKUPS[@]} backup(s) available." >&2
    exit 1
fi

SELECTED_BACKUP="${BACKUPS[$BACKUP_INDEX]}"

# =============================================================================
# VERIFY FILE EXISTS
# =============================================================================

if [[ ! -f "$FULL_PATH" ]]; then
    echo "Warning: Original file not found, will create from backup" >&2
fi

# =============================================================================
# PERFORM ROLLBACK
# =============================================================================

# Create a pre-rollback backup (in case rollback was a mistake)
if [[ -f "$FULL_PATH" ]]; then
    TIMESTAMP=$(date +%Y%m%d-%H%M%S)
    PRE_ROLLBACK="$BACKUP_DIR/${SAFE_NAME}.${TIMESTAMP}.pre-rollback"
    cp "$FULL_PATH" "$PRE_ROLLBACK"
fi

# Restore from backup
cp "$SELECTED_BACKUP" "$FULL_PATH"

# =============================================================================
# LOGGING
# =============================================================================

if declare -f log_security_event &>/dev/null; then
    log_security_event "protection" "protected_edit_rollback" "Edit" "$FILE_PATH" "restored from backup"
fi

# =============================================================================
# OUTPUT
# =============================================================================

echo "Rollback successful"
echo ""
echo "File: $FILE_PATH"
echo "Restored from: $SELECTED_BACKUP"
if [[ -n "${PRE_ROLLBACK:-}" ]]; then
    echo "Pre-rollback backup: $PRE_ROLLBACK"
fi

exit 0
