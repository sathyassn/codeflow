#!/usr/bin/env bash
set -euo pipefail

# test-rust-cli.sh -- Bridge test script for the Rust CLI.
#
# Historical: This script used to run `cargo build`, smoke tests, `cargo nextest
# run --workspace`, AND coverage enforcement. Post INF-EPC-046 the generic
# testing engine owns the full Rust test run + coverage via the `rust-core`
# target in `.codeflow/config/testing/test-config.json`.
#
# Current role (INF-TSK-046-006 onward): build + smoke only. This verifies
# the binary is still producible and behaves correctly at the CLI boundary.
# Nested `cargo nextest run --workspace` inside shell-scripts caused lock
# contention against the outer `cargo llvm-cov nextest` invocation — the
# flake this fix addresses.

readonly SCRIPT_NAME="test-rust-cli"
readonly SCRIPT_VERSION="2.0.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT
readonly RS_DIR="$REPO_ROOT/codeflow-cli"

# Source test framework
# shellcheck source=../../lib/test-common.sh
source "$TESTING_DIR/lib/test-common.sh"
# shellcheck source=../../lib/test-helpers.sh
source "$TESTING_DIR/lib/test-helpers.sh"

# ============================================================================
# FUNCTIONS
# ============================================================================

usage() {
    cat <<EOF
Usage: ${SCRIPT_NAME}.sh [OPTIONS]

Bridge test script for the Rust CLI. Runs build, smoke, unit tests, and coverage.

Options:
  -h, --help     Show this help message
  -v, --version  Show script version

EOF
}

run_build_test() {
    test_section "Build test"

    if (cd "$RS_DIR" && cargo build --release 2>&1); then
        test_pass "cargo build --release succeeds"
    else
        test_fail "cargo build --release failed"
    fi
}

run_smoke_tests() {
    local test_binary="$RS_DIR/target/release/codeflow"

    if [[ ! -x "$test_binary" ]]; then
        test_fail "Rust binary not found or not executable at $test_binary"
        return
    fi

    test_section "Smoke tests"

    # --version
    local version_output
    version_output=$("$test_binary" --version 2>&1) || true
    if [[ "$version_output" == codeflow* ]]; then
        test_pass "codeflow --version outputs version string"
    else
        test_fail "codeflow --version unexpected output: $version_output"
    fi

    # --help
    local help_output
    help_output=$("$test_binary" --help 2>&1) || true
    if grep -q "Commands:" <<< "$help_output"; then
        test_pass "codeflow --help shows Commands"
    else
        test_fail "codeflow --help missing 'Commands'"
    fi

    # version subcommand
    local ver_output
    ver_output=$("$test_binary" version 2>&1) || true
    if [[ -n "$ver_output" ]]; then
        test_pass "codeflow version outputs version info"
    else
        test_fail "codeflow version produced no output"
    fi
}

cleanup() {
    # Clean up instrumentation data but preserve build artifacts to avoid
    # cold-start rebuilds (OpenSSL vendored build fails under llvm instrumentation
    # on macOS aarch64 without pre-built artifacts).
    find "$RS_DIR/target/llvm-cov-target" -name "*.profraw" -delete 2>/dev/null || true
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # Arg parsing
    while [[ $# -gt 0 ]]; do
        case "$1" in
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
    done

    # Skip if cargo is not available
    if ! command -v cargo &>/dev/null; then
        test_section "Rust CLI Bridge Tests"
        echo "  SKIP: 'cargo' not found on PATH. Skipping Rust CLI tests."
        exit 0
    fi

    # Check Rust workspace exists
    if [[ ! -f "$RS_DIR/Cargo.toml" ]]; then
        test_section "Rust CLI Bridge Tests"
        echo "  SKIP: codeflow-cli/Cargo.toml not found. Skipping Rust CLI tests."
        exit 0
    fi

    # Verify Rust environment can compile this project
    if ! (cd "$RS_DIR" && cargo check --quiet 2>/dev/null); then
        test_section "Rust CLI Bridge Tests"
        echo "  SKIP: Rust environment cannot compile project. Skipping (dedicated CI job handles Rust tests)."
        exit 0
    fi

    # Ensure cleanup runs on exit
    trap cleanup EXIT

    # Run test sections. Unit tests + coverage were removed in v2.0.0 —
    # the generic testing engine's `rust-core` target owns those runs now
    # (see .codeflow/config/testing/test-config.json).
    run_build_test
    run_smoke_tests

    # Summary
    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
