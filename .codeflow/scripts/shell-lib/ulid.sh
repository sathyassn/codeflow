#!/usr/bin/env bash
# CodeFlow Shell Library: ULID Generation
# Location: .codeflow/scripts/shell-lib/ulid.sh

# Requires: common.sh
[[ -z "${CODEFLOW_LIB_VERSION:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# ============================================================================
# ULID CONSTANTS
# ============================================================================

# Crockford's Base32 alphabet (excludes I, L, O, U)
readonly ULID_ALPHABET="0123456789ABCDEFGHJKMNPQRSTVWXYZ"

# ============================================================================
# ULID GENERATION
# ============================================================================

# Generate ULID
# Format: 10 chars timestamp + 16 chars random = 26 chars total
generate_ulid() {
    local timestamp
    local random_part

    # Get timestamp in milliseconds
    if [[ "$(uname)" == "Darwin" ]]; then
        # macOS: use python for milliseconds
        timestamp=$(python3 -c "import time; print(int(time.time() * 1000))")
    else
        # Linux: use date with nanoseconds
        timestamp=$(($(date +%s%N) / 1000000))
    fi

    # Encode timestamp (10 chars)
    local ts_encoded=""
    local ts=$timestamp
    for ((i=0; i<10; i++)); do
        ts_encoded="${ULID_ALPHABET:$((ts % 32)):1}${ts_encoded}"
        ts=$((ts / 32))
    done

    # Generate random part (16 chars)
    if [[ -r /dev/urandom ]]; then
        random_part=$(head -c 10 /dev/urandom | xxd -p | tr -d '\n')
        # Convert hex to base32 (simplified)
        random_part=""
        for ((i=0; i<16; i++)); do
            local rand=$((RANDOM % 32))
            random_part+="${ULID_ALPHABET:$rand:1}"
        done
    else
        # Fallback: use RANDOM
        random_part=""
        for ((i=0; i<16; i++)); do
            local rand=$((RANDOM % 32))
            random_part+="${ULID_ALPHABET:$rand:1}"
        done
    fi

    echo "${ts_encoded}${random_part}"
}

# ============================================================================
# ID GENERATION HELPERS
# ============================================================================

# Generate epic ID
generate_epic_id() {
    echo "EPC-$(generate_ulid)"
}

# Generate task ID
generate_task_id() {
    echo "TSK-$(generate_ulid)"
}

# Generate session ID
generate_session_id() {
    echo "SES-$(generate_ulid)"
}

# Generate claim ID
generate_claim_id() {
    echo "CLM-$(generate_ulid)"
}

# Generate event ID
generate_event_id() {
    echo "EVT-$(generate_ulid)"
}

# ============================================================================
# ULID PARSING
# ============================================================================

# Get character index in ULID alphabet (portable version)
_ulid_char_index() {
    local char="$1"
    local i
    for ((i=0; i<32; i++)); do
        if [[ "${ULID_ALPHABET:$i:1}" == "$char" ]]; then
            echo "$i"
            return 0
        fi
    done
    echo "-1"
    return 1
}

# Extract timestamp from ULID (returns milliseconds)
ulid_timestamp() {
    local ulid="$1"
    local ts_part="${ulid:0:10}"

    local result=0
    local i
    for ((i=0; i<10; i++)); do
        local char="${ts_part:$i:1}"
        local idx
        idx=$(_ulid_char_index "$char")
        result=$((result * 32 + idx))
    done

    echo "$result"
}

# Get ULID creation time as ISO timestamp
ulid_created_at() {
    local ulid="$1"
    local ms
    ms=$(ulid_timestamp "$ulid")
    local seconds=$((ms / 1000))

    if [[ "$(uname)" == "Darwin" ]]; then
        date -r "$seconds" -u +%Y-%m-%dT%H:%M:%SZ
    else
        date -d "@$seconds" -u +%Y-%m-%dT%H:%M:%SZ
    fi
}
