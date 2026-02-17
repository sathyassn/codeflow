#!/usr/bin/env bash
# cf-pathflow-session-register.sh - Register a new PathFlow session in JSONL
#
# Location: .codeflow/scripts/pathflow/cf-pathflow-session-register.sh
#
# Writes a session_metadata event to pathflow-events.jsonl with tracking_level='pending'.
# Called by cf-knowledge-layer during PF1-INIT.
#
# Usage:
#   cf-pathflow-session-register.sh -s <session_id>
#   cf-pathflow-session-register.sh -s <session_id> -m <interaction_mode>

set -euo pipefail

# Load dependencies
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
source "$REPO_ROOT/.codeflow/scripts/state/ledger.sh"

# Constants
readonly PATHFLOW_LEDGER="pathflow-events.jsonl"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: $(basename "$0") -s <session_id> [-m <mode>]

Register a new PathFlow session in the JSONL ledger.

Required:
  -s  Session ID (e.g., SES-01HXYZ...)

Optional:
  -m  Interaction mode: interactive (default) or autorun
  -h  Show this help message
EOF
}

register_session() {
    local session_id="$1"
    local mode="${2:-interactive}"

    # Validate session ID
    if [[ -z "$session_id" ]]; then
        echo '{"error":"session_id is required"}' >&2
        return 1
    fi

    # Ensure logs directory exists
    # shellcheck disable=SC2153  # PATHFLOW_LOGS_PATH exported from sourced ledger.sh
    local ledger_path="$PATHFLOW_LOGS_PATH/$PATHFLOW_LEDGER"
    mkdir -p "$(dirname "$ledger_path")"
    [[ -f "$ledger_path" ]] || touch "$ledger_path"

    # Generate event ID
    local event_id
    event_id=$(generate_event_id)

    # Write tracking_level=pending event
    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"

    local event
    event=$(jq -c -n \
        --arg id "$event_id" \
        --arg type "session_metadata" \
        --arg session_id "$session_id" \
        --arg key "tracking_level" \
        --arg value "pending" \
        --arg ts "$ts" \
        '{id: $id, type: $type, session_id: $session_id, key: $key, value: $value, ts: $ts}')

    LEDGER_PATH="$PATHFLOW_LOGS_PATH" append_event "$PATHFLOW_LEDGER" "$event"

    # Write interaction_mode event
    local mode_event_id
    mode_event_id=$(generate_event_id)
    local mode_event
    mode_event=$(jq -c -n \
        --arg id "$mode_event_id" \
        --arg type "session_metadata" \
        --arg session_id "$session_id" \
        --arg key "interaction_mode" \
        --arg value "$mode" \
        --arg ts "$ts" \
        '{id: $id, type: $type, session_id: $session_id, key: $key, value: $value, ts: $ts}')

    LEDGER_PATH="$PATHFLOW_LOGS_PATH" append_event "$PATHFLOW_LEDGER" "$mode_event"

    # Output success
    jq -c -n \
        --arg session_id "$session_id" \
        --arg tracking_level "pending" \
        --arg interaction_mode "$mode" \
        --arg event_id "$event_id" \
        '{status: "registered", session_id: $session_id, tracking_level: $tracking_level, interaction_mode: $interaction_mode, event_id: $event_id}'
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local session_id=""
    local mode="interactive"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -s) session_id="$2"; shift 2 ;;
            -m) mode="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
    done

    if [[ -z "$session_id" ]]; then
        echo '{"error":"session_id is required (-s)"}' >&2
        usage >&2
        exit 2
    fi

    if [[ "$mode" != "interactive" && "$mode" != "autorun" ]]; then
        echo '{"error":"mode must be interactive or autorun"}' >&2
        exit 2
    fi

    register_session "$session_id" "$mode"
}

main "$@"
