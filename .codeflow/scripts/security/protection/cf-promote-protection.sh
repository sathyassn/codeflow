#!/usr/bin/env bash
# Purpose:   Promote adhoc protection patterns to extended list
# Usage:     cf-promote-protection.sh <pattern>
# Platform:  macOS/Linux
#
# Promotes a path pattern from adhoc (temporary) to extended (permanent)
# protection list. Removes from adhoc and adds to extended.
#
# Arguments:
#   $1 - Path pattern to promote
#
# Exit codes:
#   - 0: Promotion successful
#   - 1: Error (pattern not found, etc.)

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-promote-protection.sh - Promote adhoc protection to extended list

USAGE:
    cf-promote-protection.sh [OPTIONS] <pattern>

ARGUMENTS:
    pattern    Path pattern to promote

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -f, --force      Add to extended even if not in adhoc
    -k, --keep       Keep in adhoc after promoting

DESCRIPTION:
    Promotes a protection pattern from the adhoc list (session-specific)
    to the extended list (permanent).

    Files:
      - .codeflow/config/enforcement/protection/protected-adhoc.list (source)
      - .codeflow/config/enforcement/protection/protected-extended.list (destination)

EXAMPLES:
    cf-promote-protection.sh ".secrets/*"
    cf-promote-protection.sh --force "config/credentials.json"

SEE ALSO:
    cf-reload-protection.sh
EOF
}

show_version() {
    echo "cf-promote-protection.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

FORCE=false
KEEP=false

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
        -k|--keep)
            KEEP=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-promote-protection.sh --help' for usage." >&2
            exit 1
            ;;
        *)
            break
            ;;
    esac
done

if [[ $# -lt 1 ]]; then
    echo "Error: Missing required argument: pattern" >&2
    echo "Usage: cf-promote-protection.sh <pattern>" >&2
    exit 1
fi

PATTERN="$1"

# =============================================================================
# SETUP
# =============================================================================

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
EXTENDED_LIST="$REPO_ROOT/.codeflow/config/enforcement/protection/protected-extended.list"
ADHOC_LIST="$REPO_ROOT/.codeflow/config/enforcement/protection/protected-adhoc.list"

# =============================================================================
# VALIDATION
# =============================================================================

# Check if pattern exists in adhoc
IN_ADHOC=false
if [[ -f "$ADHOC_LIST" ]] && grep -qxF "$PATTERN" "$ADHOC_LIST"; then
    IN_ADHOC=true
fi

if [[ "$IN_ADHOC" == "false" ]] && [[ "$FORCE" == "false" ]]; then
    echo "Error: Pattern not found in adhoc list: $PATTERN" >&2
    echo "Use --force to add directly to extended list." >&2
    exit 1
fi

# Check if already in extended
if [[ -f "$EXTENDED_LIST" ]] && grep -qxF "$PATTERN" "$EXTENDED_LIST"; then
    echo "Pattern already in extended list: $PATTERN"

    if [[ "$IN_ADHOC" == "true" ]] && [[ "$KEEP" == "false" ]]; then
        # Remove from adhoc since it's already in extended
        temp_file=$(mktemp "${TMPDIR:-/tmp/claude}/cf-promote-XXXXXX")
        grep -vxF "$PATTERN" "$ADHOC_LIST" > "$temp_file" 2>/dev/null || true
        mv "$temp_file" "$ADHOC_LIST"
        echo "Removed duplicate from adhoc list"
    fi

    exit 0
fi

# =============================================================================
# PROMOTION
# =============================================================================

# Ensure extended list exists with header
if [[ ! -f "$EXTENDED_LIST" ]]; then
    cat > "$EXTENDED_LIST" <<EOF
# Extended Protection List
# Patterns added here provide permanent protection
# One pattern per line, supports glob syntax
# Lines starting with # are comments

EOF
fi

# Add to extended list
echo "$PATTERN" >> "$EXTENDED_LIST"
echo "Added to extended list: $PATTERN"

# Remove from adhoc (unless --keep)
if [[ "$IN_ADHOC" == "true" ]] && [[ "$KEEP" == "false" ]]; then
    temp_file=$(mktemp "${TMPDIR:-/tmp/claude}/cf-promote-XXXXXX")
    grep -vxF "$PATTERN" "$ADHOC_LIST" > "$temp_file" 2>/dev/null || true
    mv "$temp_file" "$ADHOC_LIST"
    echo "Removed from adhoc list"
fi

# =============================================================================
# RELOAD
# =============================================================================

# Suggest reloading protection
echo ""
echo "Run 'cf-reload-protection.sh' to apply changes"

exit 0
