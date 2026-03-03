#!/usr/bin/env bash
set -euo pipefail

# run-shadow-tests.sh -- Runs all shadow tests comparing Go and shell parity.
# Invokes the `codeflow shadow-test` command and reports results.
#
# Shadow testing validates behavioral equivalence between Go subcommands and
# their shell script equivalents before Phase E cutover.

readonly SCRIPT_NAME="run-shadow-tests"
readonly SCRIPT_VERSION="1.0.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
TESTING_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT
readonly CLI_DIR="$REPO_ROOT/codeflow-cli"

# Source test framework
# shellcheck source=../lib/test-common.sh
source "$TESTING_DIR/lib/test-common.sh"
# shellcheck source=../lib/test-helpers.sh
source "$TESTING_DIR/lib/test-helpers.sh"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: ${SCRIPT_NAME}.sh [OPTIONS]

Runs all shadow tests comparing Go CLI subcommands with shell script equivalents.
Uses the 'codeflow shadow-test' Go command.

Options:
  --category CATEGORY  Run only tests in this category (hook, pathflow, validate)
  --verbose            Print results for passing tests as well as failing ones
  -h, --help           Show this help message
  -v, --version        Show script version

EOF
}

build_shadow_test_binary() {
    test_section "Build shadow-test binary"

    if (cd "$CLI_DIR" && go build -o "$CLI_DIR/bin/codeflow-shadow-test" ./cmd/codeflow/) 2>/dev/null; then
        test_pass "go build succeeds"
    else
        test_fail "go build failed -- cannot run shadow tests"
        return 1
    fi
    return 0
}

run_shadow_tests() {
    local category="${1:-}"
    local verbose="${2:-false}"
    local binary="$CLI_DIR/bin/codeflow-shadow-test"

    if [[ ! -x "$binary" ]]; then
        test_fail "shadow-test binary not found or not executable at $binary"
        return 1
    fi

    test_section "Shadow tests"

    local cmd_args=("$binary" "shadow-test" "--project-dir" "$REPO_ROOT")
    if [[ -n "$category" ]]; then
        cmd_args+=("--category" "$category")
    fi
    if [[ "$verbose" == "true" ]]; then
        cmd_args+=("--verbose")
    fi

    local exit_code=0
    local output
    output=$("${cmd_args[@]}" 2>&1) || exit_code=$?

    echo "$output"

    if [[ $exit_code -eq 0 ]]; then
        test_pass "shadow-test: zero unexpected divergences"
    else
        test_fail "shadow-test: unexpected divergences detected (exit code: $exit_code)"
        return 1
    fi
    return 0
}

cleanup() {
    rm -f "$CLI_DIR/bin/codeflow-shadow-test"
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    local category=""
    local verbose="false"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --category)
                shift
                category="${1:-}"
                ;;
            --verbose)
                verbose="true"
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            -v|--version)
                echo "${SCRIPT_NAME} ${SCRIPT_VERSION}"
                exit 0
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage >&2
                exit 1
                ;;
        esac
        shift
    done

    # Skip if Go is not available.
    if ! command -v go &>/dev/null; then
        test_section "Shadow Tests"
        echo "  SKIP: 'go' not found on PATH. Skipping shadow tests."
        exit 0
    fi

    # Skip in CI — shadow tests verify Go/shell behavioral parity locally.
    # In CI, Go and shell are tested independently by their own jobs.
    if [[ "${CI:-}" == "true" ]]; then
        test_section "Shadow Tests"
        echo "  SKIP: CI environment detected. Shadow tests run locally only."
        exit 0
    fi

    # Ensure cleanup runs on exit.
    trap cleanup EXIT

    # Build then run shadow tests.
    if ! build_shadow_test_binary; then
        print_test_summary
        return 1
    fi

    run_shadow_tests "$category" "$verbose"

    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
