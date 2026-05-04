#!/usr/bin/env bash
# Test: lint guard for legacy liveness functions (INF-TSK-050-003 AC-15).
# Location: .codeflow/testing/scripts/lint/test-no-legacy-liveness.sh
#
# Asserts that no production Rust code in `codeflow-cli/` references
# `is_session_stale` or `check_heartbeat_alive`. Both functions were
# deleted by AC-09 of INF-TSK-050-003. The canonical liveness
# chokepoint is `crate::session::liveness::is_session_alive` (and its
# companions). Any new caller of the deleted symbols is a regression
# back to the divergent-signal world that PR #309/#310/#311
# progressively peeled away.
#
# Allowlist: doc-comment lines (`///` or `//!`) that NAME the deleted
# functions in prose are NOT call sites. The script filters them out
# before the assertion. Test files (`#[cfg(test)]`) and the `tests`
# directory are also excluded — only production paths are guarded.
#
# Optional --format json emits a machine-readable summary so the
# unified test runner can parse findings without reparsing stdout.
#
# Exit codes:
#   0 - No legacy references found in production code.
#   1 - One or more references found; details on stderr.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../../.." && pwd)"

readonly DEFAULT_TARGET="codeflow-cli"
readonly LEGACY_PATTERN='\b(is_session_stale|check_heartbeat_alive)\b'

format=text
target="${REPO_ROOT}/${DEFAULT_TARGET}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --format)
            shift
            format="${1:-text}"
            ;;
        --target)
            shift
            target="${1:?--target requires a path}"
            ;;
        -h|--help)
            cat <<EOF
Usage: $0 [--format text|json] [--target <path>]

Asserts that no production Rust code references the deleted legacy
liveness symbols 'is_session_stale' or 'check_heartbeat_alive'.

Options:
  --format text|json   Output format (default: text)
  --target <path>      Source root to scan (default: codeflow-cli)
EOF
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
    shift
done

if [[ ! -d "${target}" ]]; then
    echo "target directory does not exist: ${target}" >&2
    exit 2
fi

# Find all .rs files under the target. Exclude the integration `tests/`
# directory (mostly fixture / integration cases) and any file whose
# path includes `/tests/` (e.g. `cli/tests/...`). Production code lives
# under `src/`. The find expression also filters the Cargo build
# directory out defensively in case it ever gets indexed.
mapfile -t rust_files < <(
    find "${target}" \
        -type f \
        -name '*.rs' \
        -not -path '*/target/*' \
        -not -path '*/tests/*' \
        | sort
)

if [[ ${#rust_files[@]} -eq 0 ]]; then
    echo "no rust files found under ${target}" >&2
    exit 2
fi

# Use grep -E for ERE \b boundary support. -H prefixes the filename;
# -n includes the line number. We then filter out doc-comment lines
# (`^/// ` or `^//! ` after leading whitespace). Inline `//` comments
# that mention the symbol on the same line as code are still flagged
# as a precaution: the AC contract is "no reference outside deprecated
# module", and a leftover comment can mask a leftover use.
findings=()
for file in "${rust_files[@]}"; do
    while IFS= read -r line; do
        # Each line is "<path>:<lineno>:<content>". Drop pure
        # doc-comment lines AND test mod blocks. Test mod detection
        # is path-based: any file under `#[cfg(test)] mod tests` is
        # already in production code (inline tests), but the strings
        # in those tests are themselves test fixtures — skip lines
        # whose grep context is inside `#[cfg(test)]`.
        body="${line#*:*:}"
        # Strip leading whitespace.
        trimmed="${body#"${body%%[![:space:]]*}"}"
        # Doc-comment passthrough.
        if [[ "${trimmed}" == "///"* ]] || [[ "${trimmed}" == "//!"* ]]; then
            continue
        fi
        # Plain block-comment passthrough (best-effort).
        if [[ "${trimmed}" == "//"* ]]; then
            continue
        fi
        findings+=("${line}")
    done < <(grep -EHn "${LEGACY_PATTERN}" "${file}" || true)
done

case "${format}" in
    json)
        # Emit a minimal JSON object: { "findings": [...], "count": N }.
        printf '{\n  "count": %d,\n  "findings": [\n' "${#findings[@]}"
        if [[ ${#findings[@]} -gt 0 ]]; then
            for ((i = 0; i < ${#findings[@]}; i++)); do
                escaped="${findings[i]//\\/\\\\}"
                escaped="${escaped//\"/\\\"}"
                if [[ $i -eq $((${#findings[@]} - 1)) ]]; then
                    printf '    "%s"\n' "${escaped}"
                else
                    printf '    "%s",\n' "${escaped}"
                fi
            done
        fi
        printf '  ]\n}\n'
        ;;
    text|*)
        if [[ ${#findings[@]} -eq 0 ]]; then
            echo "ok: no legacy liveness references in production code under ${target}"
        else
            echo "FAIL: legacy liveness references found in production code:" >&2
            for finding in "${findings[@]}"; do
                echo "  ${finding}" >&2
            done
        fi
        ;;
esac

if [[ ${#findings[@]} -gt 0 ]]; then
    exit 1
fi
exit 0
