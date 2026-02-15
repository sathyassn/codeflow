#!/usr/bin/env bash
# cf-pathflow-stage-transition.sh - Record work stage transitions in PathFlow JSONL
#
# Location: .codeflow/scripts/pathflow/cf-pathflow-stage-transition.sh
#
# Writes a stage_transition event to pathflow-events.jsonl.
# Called during PF4-EXECUTE when stages start, complete, or receive verdicts.
#
# Usage:
#   cf-pathflow-stage-transition.sh -s <session_id> -g <stage> -t <status> [-i <iteration>] [-v <verdict>]
#   cf-pathflow-stage-transition.sh -s SES-01HX... -g WS-DEV -t in_progress -i 1
#   cf-pathflow-stage-transition.sh -s SES-01HX... -g WS-REV -t complete -v approved
#   cf-pathflow-stage-transition.sh -s SES-01HX... -g WS-QA -t complete -v fail

set -euo pipefail

# Load dependencies
[[ -z "${REPO_ROOT:-}" ]] && REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
source "$REPO_ROOT/.codeflow/scripts/shell-lib/index.sh"
source "$REPO_ROOT/.codeflow/scripts/state/ledger.sh"

# Constants
readonly PATHFLOW_LEDGER="pathflow-events.jsonl"
readonly VALID_STAGES="WS-DEV WS-PLAN WS-DOCS WS-TEST WS-REV WS-QA"
readonly VALID_STATUSES="in_progress complete"
readonly VALID_VERDICTS="pass fail approved changes_requested"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: $(basename "$0") -s <session_id> -g <stage> -t <status> [-i <iteration>] [-v <verdict>]

Record a PathFlow work stage transition event.

Required:
  -s  Session ID (e.g., SES-01HXYZ...)
  -g  Stage: WS-DEV, WS-PLAN, WS-DOCS, WS-TEST, WS-REV, WS-QA
  -t  Status: in_progress or complete

Optional:
  -i  Iteration number (default: 1, required for in_progress)
  -v  Verdict: pass, fail, approved, changes_requested (for complete status)
  -h  Show this help message
EOF
}

validate_stage() {
    local stage="$1"
    for valid in $VALID_STAGES; do
        [[ "$stage" == "$valid" ]] && return 0
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

validate_verdict() {
    local verdict="$1"
    [[ -z "$verdict" ]] && return 0  # verdict is optional
    for valid in $VALID_VERDICTS; do
        [[ "$verdict" == "$valid" ]] && return 0
    done
    return 1
}

record_stage_transition() {
    local session_id="$1"
    local stage="$2"
    local status="$3"
    local iteration="${4:-1}"
    local verdict="${5:-}"

    # Ensure ledger exists
    # shellcheck disable=SC2153  # LEDGER_PATH exported from sourced ledger.sh
    local ledger_path="$LEDGER_PATH/$PATHFLOW_LEDGER"
    mkdir -p "$(dirname "$ledger_path")"
    [[ -f "$ledger_path" ]] || touch "$ledger_path"

    # Generate event
    local event_id
    event_id=$(generate_event_id)

    local event
    if [[ -n "$verdict" ]]; then
        event=$(jq -c -n \
            --arg id "$event_id" \
            --arg type "stage_transition" \
            --arg session_id "$session_id" \
            --arg stage "$stage" \
            --arg status "$status" \
            --argjson iteration "$iteration" \
            --arg verdict "$verdict" \
            '{id: $id, type: $type, session_id: $session_id, stage: $stage, status: $status, iteration: $iteration, verdict: $verdict}')
    else
        event=$(jq -c -n \
            --arg id "$event_id" \
            --arg type "stage_transition" \
            --arg session_id "$session_id" \
            --arg stage "$stage" \
            --arg status "$status" \
            --argjson iteration "$iteration" \
            '{id: $id, type: $type, session_id: $session_id, stage: $stage, status: $status, iteration: $iteration}')
    fi

    append_event "$PATHFLOW_LEDGER" "$event"

    # Output success
    local output
    output=$(jq -c -n \
        --arg event_id "$event_id" \
        --arg stage "$stage" \
        --arg status "$status" \
        --argjson iteration "$iteration" \
        '{status: "recorded", event_id: $event_id, stage: $stage, transition: $status, iteration: $iteration}')

    if [[ -n "$verdict" ]]; then
        output=$(echo "$output" | jq -c --arg verdict "$verdict" '. + {verdict: $verdict}')
    fi

    echo "$output"
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local session_id=""
    local stage=""
    local status=""
    local iteration="1"
    local verdict=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -s) session_id="$2"; shift 2 ;;
            -g) stage="$2"; shift 2 ;;
            -t) status="$2"; shift 2 ;;
            -i) iteration="$2"; shift 2 ;;
            -v) verdict="$2"; shift 2 ;;
            -h|--help) usage; exit 0 ;;
            *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
        esac
    done

    if [[ -z "$session_id" ]]; then
        echo '{"error":"session_id is required (-s)"}' >&2
        exit 2
    fi

    if [[ -z "$stage" ]]; then
        echo '{"error":"stage is required (-g)"}' >&2
        exit 2
    fi

    if ! validate_stage "$stage"; then
        echo "{\"error\":\"invalid stage: $stage. Valid: $VALID_STAGES\"}" >&2
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

    if ! validate_verdict "$verdict"; then
        echo "{\"error\":\"invalid verdict: $verdict. Valid: $VALID_VERDICTS\"}" >&2
        exit 2
    fi

    record_stage_transition "$session_id" "$stage" "$status" "$iteration" "$verdict"
}

main "$@"
