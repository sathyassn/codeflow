#!/usr/bin/env bash
set -euo pipefail

# hook-latency.sh — Measure real-world latency of codeflow hook subcommands.
#
# Sends realistic stdin JSON payloads to each hook subcommand and measures
# wall-clock time. Outputs a results table comparing against the ~300ms
# historical shell baseline documented in the migration guide.
#
# Usage:
#   bash .codeflow/testing/benchmarks/hook-latency.sh [iterations]
#
# Default: 5 iterations per hook.

readonly ITERATIONS="${1:-5}"
readonly BASELINE_MS=300

# Verify codeflow binary is available.
if ! command -v codeflow &>/dev/null; then
    echo "ERROR: codeflow binary not found in PATH" >&2
    exit 1
fi

# Create temporary session state for hooks that need it.
readonly TMPDIR_BASE="${TMPDIR:-/tmp}/hook-latency-$$"
mkdir -p "$TMPDIR_BASE"
trap 'rm -rf "$TMPDIR_BASE"' EXIT

# Prepare a minimal session env for hooks that read CODEFLOW_SESSION_ID.
readonly FAKE_SID="ses-hooklatencybench000000"
export CODEFLOW_SESSION_ID="$FAKE_SID"

# time_hook runs a hook subcommand with stdin, measures ms, returns average.
# Args: label command stdin_payload
time_hook() {
    local label="$1"
    local cmd="$2"
    local stdin_payload="$3"
    local total_ms=0
    local i

    for ((i = 1; i <= ITERATIONS; i++)); do
        local start_ns end_ns elapsed_ms
        start_ns=$(date +%s%N 2>/dev/null || perl -e 'use Time::HiRes; print int(Time::HiRes::time()*1e9)')
        echo "$stdin_payload" | bash -c "$cmd" >/dev/null 2>&1 || true
        end_ns=$(date +%s%N 2>/dev/null || perl -e 'use Time::HiRes; print int(Time::HiRes::time()*1e9)')
        elapsed_ms=$(( (end_ns - start_ns) / 1000000 ))
        total_ms=$((total_ms + elapsed_ms))
    done

    local avg_ms=$((total_ms / ITERATIONS))
    local speedup=""
    if [ "$avg_ms" -gt 0 ]; then
        speedup=$(echo "scale=1; $BASELINE_MS / $avg_ms" | bc 2>/dev/null || echo "n/a")
    fi

    printf "| %-25s | %6d ms | %6d ms | %5sx |\n" "$label" "$avg_ms" "$BASELINE_MS" "$speedup"
}

# Print header.
echo ""
echo "Hook Latency Benchmark ($ITERATIONS iterations each)"
echo "======================================================="
echo ""
printf "| %-25s | %9s | %9s | %7s |\n" "Hook" "Avg (Go)" "Baseline" "Speedup"
printf "|%-27s|%-11s|%-11s|%-9s|\n" "---------------------------" "-----------" "-----------" "---------"

# 1. gate-check (Edit tool, should be fast — no sentinel dir needed)
time_hook "gate-check (Edit)" \
    "codeflow hooks pre-tool-use gate-check" \
    '{"tool_name":"Edit","tool_input":{"file_path":"/tmp/test.go","old_string":"a","new_string":"b"}}'

# 2. sentinel-write (SendMessage with STAGE-COMPLETE)
time_hook "sentinel-write (stage)" \
    "codeflow hooks post-tool-use sentinel-write" \
    '{"tool_name":"SendMessage","tool_input":{"content":"STAGE-COMPLETE: WS-DEV"}}'

# 3. sentinel-write (TeamCreate)
time_hook "sentinel-write (team)" \
    "codeflow hooks post-tool-use sentinel-write" \
    '{"tool_name":"TeamCreate","tool_input":{"team_name":"bench-team"}}'

# 4. checkpoint-register (TaskCreate with PF task)
time_hook "checkpoint-register" \
    "codeflow hooks post-tool-use checkpoint-register" \
    '{"tool_name":"TaskCreate","tool_input":{"subject":"PF1-TSK-01 Init session"}}'

# 5. checkpoint-complete (TaskCompleted)
time_hook "checkpoint-complete" \
    "codeflow hooks task-completed checkpoint-complete" \
    '{"task_subject":"PF1-TSK-01 Init session"}'

# 6. security check (Bash)
time_hook "security (Bash ls)" \
    "codeflow hooks pre-tool-use security" \
    '{"tool_name":"Bash","tool_input":{"command":"ls -la"}}'

# 7. team-guard (TeamDelete — expect block in active session)
time_hook "team-guard (TeamDelete)" \
    "codeflow hooks pre-tool-use team-guard" \
    '{"tool_name":"TeamDelete","tool_input":{}}'

echo ""
echo "Baseline: ~${BASELINE_MS}ms (historical shell hook average)"
echo "Note: First run may be slower due to binary cold-start."
