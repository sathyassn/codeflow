#!/usr/bin/env bash
# Purpose:   Normalize JSONL ledger files to canonical format
# Location:  .codeflow/scripts/db/normalize-jsonl.sh
# Usage:     ./normalize-jsonl.sh [OPTIONS]
# Version:   1.0.0
#
# Reads all 4 JSONL ledger files and normalizes them to canonical format:
#   - memory-events.jsonl: "type" -> "event", "ts" -> "timestamp",
#     handles raw "op":"INSERT" DB format entries
#   - work-graph.jsonl, sessions.jsonl: already canonical (pass through)
#   - config.jsonl: typically empty (handled gracefully)
#
# Canonical event format (per ledger.sh):
#   {"ts":"<ISO8601>","e":"<event_type>",<data>}
#
# Backups are created in .state/ledger/backups/ before any modifications.
# Normalization is idempotent -- running twice produces identical output.

set -euo pipefail

# =============================================================================
# CONFIGURATION
# =============================================================================

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
readonly REPO_ROOT
LEDGER_DIR="${CODEFLOW_LEDGER_PATH:-$REPO_ROOT/.state/ledger}"
readonly LEDGER_DIR
BACKUP_DIR="$LEDGER_DIR/backups"
readonly BACKUP_DIR

# Ledger file names (matching constants in ledger.sh)
readonly LEDGER_FILES=(
    "work-graph.jsonl"
    "memory-events.jsonl"
    "sessions.jsonl"
    "config.jsonl"
)

SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
readonly SCRIPT_VERSION="1.0.0"

# =============================================================================
# USAGE
# =============================================================================

usage() {
    cat <<'EOF'
Usage: normalize-jsonl.sh [OPTIONS]

Normalize JSONL ledger files to canonical format.

Options:
    -h, --help       Show this help message
    -V, --version    Show version information
    -d, --dry-run    Show what would change without modifying files
    -v, --verbose    Show detailed normalization output

Normalizations applied to memory-events.jsonl:
    - "type":"memory_stored" -> "event":"memory_store"
    - "ts":"<value>" -> "timestamp":"<value>" (when not canonical "ts"+"e" pair)
    - Raw "op":"INSERT" DB format entries -> canonical format

Other files (work-graph.jsonl, sessions.jsonl, config.jsonl) are already
canonical and are passed through unchanged.

Backups are saved to .state/ledger/backups/ before modification.
EOF
}

# =============================================================================
# NORMALIZATION LOGIC
# =============================================================================

# Normalize a single JSONL file using Python for reliable JSON manipulation.
# Args: $1=input_file, $2=output_file, $3=file_basename
# Returns: 0 on success, non-zero on error
# Stdout: count of lines changed
normalize_file() {
    local input_file="$1"
    local output_file="$2"
    local file_basename="$3"

    python3 << 'PYEOF' - "$input_file" "$output_file" "$file_basename"
import json
import sys
import os

input_file = sys.argv[1]
output_file = sys.argv[2]
file_basename = sys.argv[3]

changed = 0
total = 0

with open(input_file, 'r') as fin, open(output_file, 'w') as fout:
    for line_num, raw_line in enumerate(fin, 1):
        line = raw_line.strip()
        if not line:
            # Preserve empty lines
            fout.write(raw_line)
            continue

        total += 1
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            # Preserve malformed lines as-is
            fout.write(raw_line)
            sys.stderr.write(f"WARNING: Skipping malformed JSON at {file_basename}:{line_num}\n")
            continue

        if file_basename != "memory-events.jsonl":
            # Other files are already canonical -- pass through
            fout.write(raw_line)
            continue

        modified = False

        # Rule 1: "type":"memory_stored" -> "event":"memory_store"
        if "type" in obj and obj["type"] == "memory_stored":
            obj["event"] = "memory_store"
            del obj["type"]
            modified = True

        # Rule 2: Handle raw "op":"INSERT" DB format entries
        # These have "op":"INSERT","table":"memory_events","data":{...}
        if "op" in obj and obj["op"] == "INSERT" and "table" in obj:
            # Extract the inner data and reconstruct as a canonical event
            inner_data = obj.get("data", {})
            if isinstance(inner_data, str):
                try:
                    inner_data = json.loads(inner_data)
                except (json.JSONDecodeError, TypeError):
                    inner_data = {"raw_data": inner_data}

            # Build canonical event from the inner data
            new_obj = {}

            # Preserve timestamp from outer or inner
            ts_val = obj.get("ts") or obj.get("timestamp") or inner_data.get("created_at") or inner_data.get("timestamp")
            if ts_val:
                new_obj["timestamp"] = ts_val

            # Use event from inner data or derive from table
            event_val = inner_data.get("event") or inner_data.get("type")
            if event_val:
                if event_val == "memory_stored":
                    event_val = "memory_store"
                new_obj["event"] = event_val
            else:
                new_obj["event"] = "memory_event"

            # Copy remaining fields from inner data
            skip_keys = {"created_at", "timestamp", "event", "type", "ts"}
            for k, v in inner_data.items():
                if k not in skip_keys:
                    new_obj[k] = v

            # Copy ledger_op if present at outer level
            if "ledger_op" in obj:
                new_obj["ledger_op"] = obj["ledger_op"]

            obj = new_obj
            modified = True

        # Rule 3: Normalize "ts" -> "timestamp" for non-canonical entries
        # Canonical format uses "ts"+"e" (from ledger.sh append_ledger).
        # Non-canonical uses "ts" with other key patterns (e.g., "type", "event").
        # Only rename "ts" -> "timestamp" when the entry does NOT use the
        # canonical "e" field (i.e., it's a pre-canonical entry).
        if "ts" in obj and "e" not in obj:
            obj["timestamp"] = obj.pop("ts")
            modified = True

        if modified:
            changed += 1

        fout.write(json.dumps(obj, ensure_ascii=False, separators=(',', ':')) + "\n")

print(f"{changed}")
sys.exit(0)
PYEOF
}

# =============================================================================
# MAIN
# =============================================================================

main() {
    local dry_run=false
    local verbose=false

    # Track temp files for cleanup on early exit (e.g., Python crash)
    local -a _tmp_files=()
    trap 'rm -f "${_tmp_files[@]}"' EXIT

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)    usage; exit 0 ;;
            -V|--version) echo "$SCRIPT_NAME v$SCRIPT_VERSION"; exit 0 ;;
            -d|--dry-run) dry_run=true; shift ;;
            -v|--verbose) verbose=true; shift ;;
            *)
                echo "Error: Unknown option: $1" >&2
                usage >&2
                exit 1
                ;;
        esac
    done

    # Verify ledger directory exists
    if [[ ! -d "$LEDGER_DIR" ]]; then
        echo "Error: Ledger directory not found: $LEDGER_DIR" >&2
        exit 1
    fi

    # Create backup directory
    if [[ "$dry_run" == "false" ]]; then
        local timestamp
        timestamp=$(date +%Y%m%d_%H%M%S)
        local backup_subdir="$BACKUP_DIR/normalize-$timestamp"
        mkdir -p "$backup_subdir"
    fi

    local total_files=0
    local total_changed=0

    for file_name in "${LEDGER_FILES[@]}"; do
        local file_path="$LEDGER_DIR/$file_name"

        # Handle missing files gracefully
        if [[ ! -f "$file_path" ]]; then
            if [[ "$verbose" == "true" ]]; then
                echo "SKIP: $file_name (not found)"
            fi
            continue
        fi

        # Handle empty files gracefully
        # Use grep -c '.' consistently (counts non-empty lines) rather than
        # wc -l (counts newlines, misses files without trailing newline).
        local line_count
        line_count=$(grep -c '.' "$file_path" || true)
        if [[ "$line_count" -eq 0 ]]; then
            if [[ "$verbose" == "true" ]]; then
                echo "SKIP: $file_name (empty)"
            fi
            continue
        fi

        total_files=$((total_files + 1))

        # Count lines before normalization
        local before_count
        before_count=$(grep -c '.' "$file_path" || true)

        if [[ "$dry_run" == "true" ]]; then
            # Dry run: normalize to temp file and report
            local tmp_output
            tmp_output=$(mktemp "${TMPDIR:-/tmp}/normalize-XXXXXX.jsonl")
            _tmp_files+=("$tmp_output")
            local changed
            changed=$(normalize_file "$file_path" "$tmp_output" "$file_name")
            local after_count
            after_count=$(grep -c '.' "$tmp_output" || true)
            rm -f "$tmp_output"

            echo "DRY-RUN: $file_name -- $changed lines would change ($before_count lines total)"
            if [[ "$before_count" != "$after_count" ]]; then
                echo "  WARNING: Line count mismatch: before=$before_count after=$after_count"
            fi
        else
            # Backup original
            cp "$file_path" "$backup_subdir/$file_name"

            # Normalize to temp file
            local tmp_output
            tmp_output=$(mktemp "${TMPDIR:-/tmp}/normalize-XXXXXX.jsonl")
            _tmp_files+=("$tmp_output")

            local changed
            changed=$(normalize_file "$file_path" "$tmp_output" "$file_name")

            # Verify data preservation: line counts must match
            local after_count
            after_count=$(grep -c '.' "$tmp_output" || true)

            if [[ "$before_count" != "$after_count" ]]; then
                echo "ERROR: Data loss detected in $file_name (before=$before_count, after=$after_count). Restoring backup." >&2
                cp "$backup_subdir/$file_name" "$file_path"
                rm -f "$tmp_output"
                exit 1
            fi

            # Replace original with normalized version
            mv "$tmp_output" "$file_path"
            total_changed=$((total_changed + changed))

            if [[ "$verbose" == "true" ]]; then
                echo "NORMALIZED: $file_name -- $changed lines changed ($before_count total)"
            fi
        fi
    done

    if [[ "$dry_run" == "true" ]]; then
        echo "Dry run complete. No files modified."
    else
        echo "Normalization complete: $total_files files processed, $total_changed lines changed"
        echo "Backups saved to: $backup_subdir"
    fi
}

main "$@"
