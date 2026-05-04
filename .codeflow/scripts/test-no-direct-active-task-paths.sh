#!/usr/bin/env bash
#
# Lint guard: prevent direct construction of active-task.json paths outside
# the canonical resolver `WorktreePaths::active_task()` and the
# session::active_task module.
#
# Background: INF-TSK-050-010 — multiple call sites historically built
# active-task.json paths from string literals (e.g., `.join(".state/runtime/active-task.json")`
# or `Path::new(wt).join(...)`), causing path divergence across worktree
# vs main-repo modes. This script fails CI when such patterns reappear.
#
# Whitelist:
#   - codeflow-cli/core/src/session/active_task.rs (the resolver implementation)
#   - codeflow-cli/core/src/worktree/paths.rs (the WorktreePaths helper)
#   - Any path under `/tests/` (integration tests)
#   - Lines containing the comment marker `// LINT-EXEMPT-ACTIVE-TASK:` with a reason
#   - Lines that are doc comments (`//!`, `///`, or trailing `//`-comment-only)
#   - Lines inside a `#[cfg(test)]` module (brace-counted)
#
# Whitelisted file: codeflow-cli/core/src/autorun/worker.rs
#   The autorun worker writes a JSON literal that includes additional
#   per-worker fields (worktree_path, target_branch, auto_merge, epic_update)
#   that are not part of the `ActiveTask` struct. Refactoring would either
#   (a) require extending `ActiveTask` with autorun-only fields, or
#   (b) lose information. The literal already uses `WorktreePaths::active_task()`
#   for the *path*; only the JSON shape is hand-built.

set -euo pipefail

readonly SCRIPT_NAME="test-no-direct-active-task-paths"

# Patterns that indicate a direct active-task.json path literal bypassing
# the resolver. Each pattern is a fragment that, when matched, signals a
# violation candidate.
readonly -a VIOLATION_PATTERNS=(
    '\.state/runtime/active-task\.json'
)

# Files exempt from the lint entirely (canonical resolver + helper sources).
readonly -a EXEMPT_FILES=(
    "codeflow-cli/core/src/session/active_task.rs"
    "codeflow-cli/core/src/worktree/paths.rs"
    # Autorun worker uses literal JSON construction with autorun-specific
    # fields not in `ActiveTask`. The PATH is via WorktreePaths::active_task();
    # only the JSON shape is hand-built. See worker.rs:1432.
    "codeflow-cli/core/src/autorun/worker.rs"
)

usage() {
    cat <<USAGE
Usage: ${SCRIPT_NAME} [--root <repo-root>]

Scans Rust source files (codeflow-cli/{core,cli}/src/**/*.rs) for direct
active-task.json path literals that bypass WorktreePaths::active_task().

Exits 0 on clean tree, exits 1 (with a list of offenders) on violations.

Options:
    --root <path>   Repository root (default: current working directory).
    -h, --help      Show this help message.
USAGE
}

is_exempt_file() {
    local file="$1"
    local exempt
    for exempt in "${EXEMPT_FILES[@]}"; do
        if [[ "${file}" == *"${exempt}" ]]; then
            return 0
        fi
    done
    return 1
}

is_test_path() {
    # Files under any `tests/` directory are integration tests.
    [[ "$1" == *"/tests/"* ]]
}

is_doc_comment_line() {
    local line="$1"
    # Strip leading whitespace.
    local trimmed
    trimmed="${line#"${line%%[![:space:]]*}"}"
    # Outer (`//!`) or item (`///`) doc comments.
    [[ "${trimmed}" == "///"* ]] || [[ "${trimmed}" == "//!"* ]]
}

is_lint_exempt_marker() {
    [[ "$1" == *"// LINT-EXEMPT-ACTIVE-TASK:"* ]]
}

# Use awk to compute a per-line in-test-mod mask via brace counting.
# Marks lines inside any `#[cfg(test)] mod ...` scope.
emit_test_mask() {
    local file="$1"
    awk '
    BEGIN { depth = 0; in_test = 0 }
    {
        # Trim leading whitespace for the cfg-test detector.
        line = $0
        sub(/^[[:space:]]+/, "", line)
        if (!in_test && line ~ /^#\[cfg\(test\)\]/) {
            cfg_pending = 1
            print "0:" $0
            next
        }
        if (cfg_pending == 1) {
            n_open = gsub(/\{/, "{", $0)
            n_close = gsub(/\}/, "}", $0)
            if (n_open > 0) {
                in_test = 1
                depth = n_open - n_close
                cfg_pending = 0
                print "1:" $0
                next
            }
            print "0:" $0
            next
        }
        if (in_test) {
            n_open = gsub(/\{/, "{", $0)
            n_close = gsub(/\}/, "}", $0)
            depth += n_open - n_close
            print "1:" $0
            if (depth <= 0) {
                in_test = 0
                depth = 0
            }
            next
        }
        print "0:" $0
    }' "${file}"
}

scan_file() {
    local file="$1"
    local -a offenders=()
    local lineno=0
    local pattern

    if is_exempt_file "${file}"; then
        return 0
    fi
    if is_test_path "${file}"; then
        return 0
    fi

    # Stream the file with the test-mask prefix; classify each line.
    while IFS= read -r prefixed_line; do
        lineno=$((lineno + 1))
        local mask="${prefixed_line%%:*}"
        local line="${prefixed_line#*:}"

        # Skip lines inside #[cfg(test)] modules.
        if [[ "${mask}" == "1" ]]; then
            continue
        fi
        # Skip doc comments.
        if is_doc_comment_line "${line}"; then
            continue
        fi
        # Skip explicitly exempted lines.
        if is_lint_exempt_marker "${line}"; then
            continue
        fi
        for pattern in "${VIOLATION_PATTERNS[@]}"; do
            if echo "${line}" | grep -E -q "${pattern}"; then
                offenders+=("${file}:${lineno}: ${line}")
                break
            fi
        done
    done < <(emit_test_mask "${file}")

    if [[ ${#offenders[@]} -gt 0 ]]; then
        printf '%s\n' "${offenders[@]}"
        return 1
    fi
    return 0
}

main() {
    local root="${PWD}"
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --root)
                root="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Error: unknown argument: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
    done

    if [[ ! -d "${root}/codeflow-cli" ]]; then
        echo "Error: ${root}/codeflow-cli not found; not a CodeFlow repo root?" >&2
        exit 2
    fi

    local -a files=()
    while IFS= read -r f; do
        files+=("${f}")
    done < <(find "${root}/codeflow-cli/core/src" "${root}/codeflow-cli/cli/src" \
                -name '*.rs' -type f 2>/dev/null | sort)

    if [[ ${#files[@]} -eq 0 ]]; then
        echo "Error: no Rust sources found under ${root}/codeflow-cli/{core,cli}/src" >&2
        exit 2
    fi

    local violations=0
    local file
    local -a all_offenders=()
    for file in "${files[@]}"; do
        local rel="${file#"${root}/"}"
        local out
        if ! out="$(scan_file "${rel}")"; then
            while IFS= read -r line; do
                [[ -z "${line}" ]] && continue
                all_offenders+=("${line}")
            done <<< "${out}"
            violations=$((violations + 1))
        fi
    done

    if [[ ${#all_offenders[@]} -gt 0 ]]; then
        echo "ERROR: direct active-task.json path literal(s) detected outside the resolver." >&2
        echo "       Use WorktreePaths::active_task() or active_task_path_resolved() instead." >&2
        echo "" >&2
        printf '%s\n' "${all_offenders[@]}" >&2
        echo "" >&2
        echo "If this literal is intentional (e.g., test fixture or migration helper)," >&2
        echo "add a trailing comment marker on the same line: // LINT-EXEMPT-ACTIVE-TASK: <reason>" >&2
        exit 1
    fi
    echo "OK: no direct active-task.json path literals outside the resolver."
    exit 0
}

main "$@"
