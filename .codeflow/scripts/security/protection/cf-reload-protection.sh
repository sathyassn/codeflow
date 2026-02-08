#!/usr/bin/env bash
# Purpose:   Reload protection lists from all sources
# Usage:     cf-reload-protection.sh
# Platform:  macOS/Linux
#
# Reloads and validates protection configuration from:
#   - enforcement-policy.json (primary)
#   - protected-extended.list (extended patterns)
#   - protected-adhoc.list (session-specific)
#
# Exit codes:
#   - 0: Reload successful
#   - 1: Error (invalid config, etc.)

set -euo pipefail

# =============================================================================
# VERSION AND HELP
# =============================================================================

readonly VERSION="1.0.0"

show_help() {
    cat <<EOF
cf-reload-protection.sh - Reload protection lists from all sources

USAGE:
    cf-reload-protection.sh [OPTIONS]

OPTIONS:
    -h, --help       Show this help message
    -V, --version    Show version information
    -v, --validate   Validate only, don't reload
    -l, --list       List all protected paths

DESCRIPTION:
    Reloads protection configuration from:
      - .codeflow/config/enforcement/enforcement-policy.json (primary)
      - .codeflow/config/enforcement/protection/protected-extended.list
      - .codeflow/config/enforcement/protection/protected-adhoc.list

    Use after modifying protection configuration.

EXAMPLES:
    cf-reload-protection.sh
    cf-reload-protection.sh --validate
    cf-reload-protection.sh --list

SEE ALSO:
    cf-promote-protection.sh
EOF
}

show_version() {
    echo "cf-reload-protection.sh version $VERSION"
}

# =============================================================================
# ARGUMENT PARSING
# =============================================================================

VALIDATE_ONLY=false
LIST_PATHS=false

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
        -v|--validate)
            VALIDATE_ONLY=true
            shift
            ;;
        -l|--list)
            LIST_PATHS=true
            shift
            ;;
        -*)
            echo "Error: Unknown option: $1" >&2
            echo "Run 'cf-reload-protection.sh --help' for usage." >&2
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

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
CONFIG="$REPO_ROOT/.codeflow/config/enforcement/enforcement-policy.json"
EXTENDED_LIST="$REPO_ROOT/.codeflow/config/enforcement/protection/protected-extended.list"
ADHOC_LIST="$REPO_ROOT/.codeflow/config/enforcement/protection/protected-adhoc.list"
CACHE_FILE="/tmp/claude/managed/protection-cache.json"

# =============================================================================
# VALIDATION
# =============================================================================

ERRORS=0

# Validate primary config
if [[ -f "$CONFIG" ]]; then
    if command -v jq &>/dev/null; then
        if ! jq empty "$CONFIG" 2>/dev/null; then
            echo "Error: Invalid JSON in enforcement-policy.json" >&2
            ERRORS=$((ERRORS + 1))
        else
            echo "PASS: enforcement-policy.json valid"
        fi
    fi
else
    echo "Warning: enforcement-policy.json not found" >&2
fi

# Validate extended list (if exists)
if [[ -f "$EXTENDED_LIST" ]]; then
    LINE_COUNT=$(wc -l < "$EXTENDED_LIST" | tr -d ' ')
    echo "PASS: protected-extended.list ($LINE_COUNT patterns)"
fi

# Validate adhoc list (if exists)
if [[ -f "$ADHOC_LIST" ]]; then
    LINE_COUNT=$(wc -l < "$ADHOC_LIST" | tr -d ' ')
    echo "PASS: protected-adhoc.list ($LINE_COUNT patterns)"
fi

if [[ $ERRORS -gt 0 ]]; then
    echo ""
    echo "Validation failed with $ERRORS error(s)"
    exit 1
fi

if [[ "$VALIDATE_ONLY" == "true" ]]; then
    echo ""
    echo "Validation passed"
    exit 0
fi

# =============================================================================
# LIST MODE
# =============================================================================

if [[ "$LIST_PATHS" == "true" ]]; then
    echo "Protected paths:"
    echo ""

    echo "=== Critical (from enforcement-policy.json) ==="
    if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
        jq -r '.protected_resources.critical[]? // empty' "$CONFIG" 2>/dev/null | sed 's/^/  /'
    fi

    echo ""
    echo "=== High (from enforcement-policy.json) ==="
    if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
        jq -r '.protected_resources.high[]? // empty' "$CONFIG" 2>/dev/null | sed 's/^/  /'
    fi

    echo ""
    echo "=== Moderate (from enforcement-policy.json) ==="
    if [[ -f "$CONFIG" ]] && command -v jq &>/dev/null; then
        jq -r '.protected_resources.moderate[]? // empty' "$CONFIG" 2>/dev/null | sed 's/^/  /'
    fi

    if [[ -f "$EXTENDED_LIST" ]]; then
        echo ""
        echo "=== Extended (from protected-extended.list) ==="
        grep -v '^#' "$EXTENDED_LIST" 2>/dev/null | grep -v '^$' | sed 's/^/  /'
    fi

    if [[ -f "$ADHOC_LIST" ]]; then
        echo ""
        echo "=== Adhoc (from protected-adhoc.list) ==="
        grep -v '^#' "$ADHOC_LIST" 2>/dev/null | grep -v '^$' | sed 's/^/  /'
    fi

    exit 0
fi

# =============================================================================
# RELOAD (Build Cache)
# =============================================================================

mkdir -p "$(dirname "$CACHE_FILE")"

# Build combined protection cache
if command -v jq &>/dev/null; then
    # Collect all paths
    CRITICAL_PATHS="[]"
    HIGH_PATHS="[]"
    MODERATE_PATHS="[]"
    EXTENDED_PATHS="[]"
    ADHOC_PATHS="[]"

    if [[ -f "$CONFIG" ]]; then
        CRITICAL_PATHS=$(jq -c '.protected_resources.critical // []' "$CONFIG" 2>/dev/null || echo "[]")
        HIGH_PATHS=$(jq -c '.protected_resources.high // []' "$CONFIG" 2>/dev/null || echo "[]")
        MODERATE_PATHS=$(jq -c '.protected_resources.moderate // []' "$CONFIG" 2>/dev/null || echo "[]")
    fi

    if [[ -f "$EXTENDED_LIST" ]]; then
        EXTENDED_PATHS=$(grep -v '^#' "$EXTENDED_LIST" 2>/dev/null | grep -v '^$' | jq -R -s -c 'split("\n") | map(select(length > 0))' || echo "[]")
    fi

    if [[ -f "$ADHOC_LIST" ]]; then
        ADHOC_PATHS=$(grep -v '^#' "$ADHOC_LIST" 2>/dev/null | grep -v '^$' | jq -R -s -c 'split("\n") | map(select(length > 0))' || echo "[]")
    fi

    # Write cache
    jq -n \
        --argjson critical "$CRITICAL_PATHS" \
        --argjson high "$HIGH_PATHS" \
        --argjson moderate "$MODERATE_PATHS" \
        --argjson extended "$EXTENDED_PATHS" \
        --argjson adhoc "$ADHOC_PATHS" \
        --arg updated_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        '{
            critical: $critical,
            high: $high,
            moderate: $moderate,
            extended: $extended,
            adhoc: $adhoc,
            updated_at: $updated_at
        }' > "$CACHE_FILE"

    echo "Protection cache updated: $CACHE_FILE"
else
    echo "Warning: jq not available, cache not updated"
fi

# =============================================================================
# OUTPUT
# =============================================================================

echo ""
echo "Protection lists reloaded successfully"

exit 0
