#!/usr/bin/env bash
# Purpose:   Generate human-readable format IDs for epics and tasks
# Location:  .codeflow/scripts/db/generate-format-id.sh
# Usage:     ./generate-format-id.sh <epic|task> <AREA> <TYPE> <DOMAIN>
# Version:   1.0.0
#
# Format:
#   Epic:  {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}  (e.g., FRT-EPC-FEAT-AUTH-001)
#   Task:  {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}  (e.g., FRT-TSK-FEAT-AUTH-001)
#
# Sequence:
#   NNN is zero-padded to 3 digits, determined by scanning existing
#   format_id: fields in project-management/ markdown frontmatter.
#
# Examples:
#   ./generate-format-id.sh epic INF FEAT GENL    # → INF-EPC-FEAT-GENL-001
#   ./generate-format-id.sh task FRT FEAT AUTH     # → FRT-TSK-FEAT-AUTH-001
#   ./generate-format-id.sh epic INF RFCT IDSY     # → INF-EPC-RFCT-IDSY-002 (if 001 exists)

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PM_DIR="${REPO_ROOT}/project-management"

# =============================================================================
# USAGE
# =============================================================================

usage() {
    cat <<'EOF'
Usage: generate-format-id.sh <kind> <AREA> <TYPE> <DOMAIN>

Arguments:
  kind     "epic" or "task"
  AREA     Area code: 2-4 uppercase letters (e.g., FRT, BKD, INF)
  TYPE     Work type code: 2-4 uppercase letters (e.g., FEAT, FIX, RFCT)
  DOMAIN   Domain code: 2-4 uppercase letters (e.g., GENL, AUTH, IDSY)

Output:
  Prints the next available format ID to stdout.

Examples:
  generate-format-id.sh epic INF FEAT GENL
  generate-format-id.sh task FRT FEAT AUTH
EOF
    exit "${1:-0}"
}

# =============================================================================
# VALIDATION
# =============================================================================

validate_uppercase_code() {
    local label="$1"
    local value="$2"
    if [[ ! "$value" =~ ^[A-Z]{2,4}$ ]]; then
        echo "Error: ${label} must be 2-4 uppercase letters, got '${value}'" >&2
        exit 1
    fi
}

# =============================================================================
# MAIN
# =============================================================================

# Handle --help
if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    usage 0
fi

# Require 4 arguments
if [[ $# -ne 4 ]]; then
    echo "Error: Expected 4 arguments, got $#" >&2
    usage 1
fi

KIND="$1"
AREA="$2"
TYPE="$3"
DOMAIN="$4"

# Validate kind
case "$KIND" in
    epic)  KIND_CODE="EPC" ;;
    task)  KIND_CODE="TSK" ;;
    *)
        echo "Error: kind must be 'epic' or 'task', got '${KIND}'" >&2
        exit 1
        ;;
esac

# Validate codes
validate_uppercase_code "AREA" "$AREA"
validate_uppercase_code "TYPE" "$TYPE"
validate_uppercase_code "DOMAIN" "$DOMAIN"

# Build the prefix pattern: e.g., INF-EPC-FEAT-GENL-
PREFIX="${AREA}-${KIND_CODE}-${TYPE}-${DOMAIN}-"

# Scan existing format_ids in project-management markdown files
MAX_SEQ=0

if [[ -d "$PM_DIR" ]]; then
    # Search for format_id or id fields matching our prefix in frontmatter
    while IFS= read -r line; do
        # Extract the sequence number from the end of the format ID
        if [[ "$line" =~ ${PREFIX}([0-9]{3}) ]]; then
            seq_num="${BASH_REMATCH[1]}"
            # Remove leading zeros for arithmetic
            seq_num=$((10#$seq_num))
            if [[ $seq_num -gt $MAX_SEQ ]]; then
                MAX_SEQ=$seq_num
            fi
        fi
    done < <(grep -rh "^\(id\|format_id\):.*${PREFIX}" "$PM_DIR" 2>/dev/null || true)
fi

# Next sequence number
NEXT_SEQ=$((MAX_SEQ + 1))
NEXT_SEQ_PADDED=$(printf "%03d" "$NEXT_SEQ")

# Output the format ID
echo "${PREFIX}${NEXT_SEQ_PADDED}"
