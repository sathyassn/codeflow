#!/usr/bin/env bash
# CodeFlow Shell Library: ULID Generation
# Location: .codeflow/scripts/shell-lib/ulid.sh

# Requires: common.sh
[[ -z "${CODEFLOW_LIB_VERSION:-}" ]] && source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# Source guard to prevent multiple loads (readonly ULID_ALPHABET would fail)
[[ -n "${_CODEFLOW_ULID_LOADED:-}" ]] && return 0
_CODEFLOW_ULID_LOADED=1

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
    local i
    for ((i=0; i<10; i++)); do
        ts_encoded="${ULID_ALPHABET:$((ts % 32)):1}${ts_encoded}"
        ts=$((ts / 32))
    done

    # Generate random part (16 chars from 80 bits)
    # Use /dev/urandom for cryptographic randomness (matches Python ulid.py)
    if [[ -r /dev/urandom ]]; then
        # Read 10 bytes (80 bits), convert to hex, then to base32
        local hex_bytes
        hex_bytes=$(head -c 10 /dev/urandom | xxd -p | tr -d '\n')
        # Convert each 5-bit group to base32 character
        # 80 bits / 5 bits = 16 characters
        local bit_accumulator=0
        local bits_in_acc=0
        local hex_pos=0
        random_part=""
        while [[ ${#random_part} -lt 16 ]]; do
            if [[ $bits_in_acc -lt 5 ]]; then
                local hex_char="${hex_bytes:$hex_pos:2}"
                bit_accumulator=$(( (bit_accumulator << 8) | 16#$hex_char ))
                bits_in_acc=$((bits_in_acc + 8))
                hex_pos=$((hex_pos + 2))
            fi
            bits_in_acc=$((bits_in_acc - 5))
            local index=$(( (bit_accumulator >> bits_in_acc) & 31 ))
            random_part+="${ULID_ALPHABET:$index:1}"
        done
    else
        # Fallback: use RANDOM (lower quality, 15 bits per call)
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

# Generate epic ULID primary key
generate_epic_id() {
    echo "epic-$(generate_ulid)"
}

# Generate task ULID primary key
generate_task_id() {
    echo "task-$(generate_ulid)"
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
