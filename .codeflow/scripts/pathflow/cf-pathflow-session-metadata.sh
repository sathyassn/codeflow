#!/usr/bin/env bash
# cf-pathflow-session-metadata.sh - Record session metadata in PathFlow JSONL
#
# Location: .codeflow/scripts/pathflow/cf-pathflow-session-metadata.sh
#
# Writes a session_metadata event to pathflow-events.jsonl.
# Used for recording work_type, area_type, tracking_level changes,
# branch name, and other session properties.
#
# Usage:
#   cf-pathflow-session-metadata.sh -s <session_id> -k <key> -v <value>
#   cf-pathflow-session-metadata.sh -s SES-01HX... -k work_type -v FEAT
#   cf-pathflow-session-metadata.sh -s SES-01HX... -k tracking_level -v tracked
#   cf-pathflow-session-metadata.sh -s SES-01HX... -k branch -v feat/my-feature

set -euo pipefail

# Load dependencies
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
source "$REPO_ROOT/.codeflow/scripts/state/ledger.sh"

# Constants
readonly PATHFLOW_LEDGER="pathflow-events.jsonl"
readonly KNOWN_KEYS="work_type area_type tracking_level branch task_id interaction_mode"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: $(basename "$0") -s <session_id> -k <key> -v <value>

Record session metadata in the PathFlow JSONL ledger.

Required:
  -s  Session ID (e.g., SES-01HXYZ...)
  -k  Metadata key (e.g., work_type, area_type, tracking_level, branch)
  -v  Metadata value

Known keys: $KNOWN_KEYS
Custom keys are allowed but should follow snake_case convention.

Optional:
  -h  Show this help message
EOF
}

record_metadata() {
    local session_id="$1"
    local key="$2"
    local value="$3"

    # Ensure ledger exists
    # shellcheck disable=SC2153  # LEDGER_PATH exported from sourced ledger.sh
    local ledger_path="$LEDGER_PATH/$PATHFLOW_LEDGER"
    mkdir -p "$(dirname "$ledger_path")"
    [[ -f "$ledger_path" ]] || touch "$ledger_path"

    # Generate event
    local event_id
    event_id=$(generate_event_id)

    local event
    event=$(jq -c -n \
        --arg id "$event_id" \
        --arg type "session_metadata" \
        --arg session_id "$session_id" \
        --arg key "$key" \
        --arg value "$value" \
        '{id: $id, type: $type, session_id: $session_id, key: $key, value: $value}')

    append_event "$PATHFLOW_LEDGER" "$event"

    # Output success
    jq -c -n \
        --arg event_id "$event_id" \
        --arg key "$key" \
        --arg value "$value" \
        '{status: "recorded", event_id: $event_id, key: $key, value: $value}'
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local session_id=""
    local key=""
    local value=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -s) session_id="$2"; shift 2 ;;
            -k) key="$2"; shift 2 ;;
            -v) value="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
    done

    if [[ -z "$session_id" ]]; then
        echo '{"error":"session_id is required (-s)"}' >&2
        exit 2
    fi

    if [[ -z "$key" ]]; then
        echo '{"error":"key is required (-k)"}' >&2
        exit 2
    fi

    if [[ -z "$value" ]]; then
        echo '{"error":"value is required (-v)"}' >&2
        exit 2
    fi

    record_metadata "$session_id" "$key" "$value"
}

main "$@"
