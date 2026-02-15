#!/usr/bin/env bash
# cf-pathflow-phase-transition.sh - Record phase transitions in PathFlow JSONL
#
# Location: .codeflow/scripts/pathflow/cf-pathflow-phase-transition.sh
#
# Writes a phase_transition event to pathflow-events.jsonl.
# Called by team lead / cf-knowledge-layer at each phase boundary.
#
# Usage:
#   cf-pathflow-phase-transition.sh -s <session_id> -p <phase> -t <status>
#   cf-pathflow-phase-transition.sh -s SES-01HX... -p PF1-INIT -t entered
#   cf-pathflow-phase-transition.sh -s SES-01HX... -p PF3-CLASSIFY -t completed

set -euo pipefail

# Load dependencies
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
source "$REPO_ROOT/.codeflow/scripts/state/ledger.sh"

# Constants
readonly PATHFLOW_LEDGER="pathflow-events.jsonl"
readonly VALID_PHASES="PF1-INIT PF2-CONTEXT PF3-CLASSIFY PF4-EXECUTE PF5-VERIFY PF6-COMPLETE PF7-END"
readonly VALID_STATUSES="entered completed"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: $(basename "$0") -s <session_id> -p <phase> -t <status>

Record a PathFlow phase transition event.

Required:
  -s  Session ID (e.g., SES-01HXYZ...)
  -p  Phase: PF1-INIT, PF2-CONTEXT, PF3-CLASSIFY, PF4-EXECUTE, PF5-VERIFY, PF6-COMPLETE, PF7-END
  -t  Status: entered or completed

Optional:
  -h  Show this help message
EOF
}

validate_phase() {
    local phase="$1"
    for valid in $VALID_PHASES; do
        [[ "$phase" == "$valid" ]] && return 0
    done
    return 1
}

validate_status() {
    local status="$1"
    for valid in $VALID_STATUSES; do
        [[ "$status" == "$valid" ]] && return 0
    done
    return 1
}

record_phase_transition() {
    local session_id="$1"
    local phase="$2"
    local status="$3"

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
        --arg type "phase_transition" \
        --arg session_id "$session_id" \
        --arg phase "$phase" \
        --arg status "$status" \
        '{id: $id, type: $type, session_id: $session_id, phase: $phase, status: $status}')

    append_event "$PATHFLOW_LEDGER" "$event"

    # Output success
    jq -c -n \
        --arg event_id "$event_id" \
        --arg phase "$phase" \
        --arg status "$status" \
        '{status: "recorded", event_id: $event_id, phase: $phase, transition: $status}'
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local session_id=""
    local phase=""
    local status=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -s) session_id="$2"; shift 2 ;;
            -p) phase="$2"; shift 2 ;;
            -t) status="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
    done

    if [[ -z "$session_id" ]]; then
        echo '{"error":"session_id is required (-s)"}' >&2
        exit 2
    fi

    if [[ -z "$phase" ]]; then
        echo '{"error":"phase is required (-p)"}' >&2
        exit 2
    fi

    if ! validate_phase "$phase"; then
        echo "{\"error\":\"invalid phase: $phase. Valid: $VALID_PHASES\"}" >&2
        exit 2
    fi

    if [[ -z "$status" ]]; then
        echo '{"error":"status is required (-t)"}' >&2
        exit 2
    fi

    if ! validate_status "$status"; then
        echo "{\"error\":\"invalid status: $status. Valid: $VALID_STATUSES\"}" >&2
        exit 2
    fi

    record_phase_transition "$session_id" "$phase" "$status"
}

main "$@"
