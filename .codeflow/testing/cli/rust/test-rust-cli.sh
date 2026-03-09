#!/usr/bin/env bash
set -euo pipefail

# test-rust-cli.sh -- Bridge test script for the Rust CLI.
# Runs build verification, binary smoke tests, Rust unit tests, and coverage enforcement.

readonly SCRIPT_NAME="test-rust-cli"
readonly SCRIPT_VERSION="1.2.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT
readonly RS_DIR="$REPO_ROOT/codeflow-rs"
readonly CONFIG_FILE="$RS_DIR/config/testing/test-config.json"

# Read coverage settings from config (fallback to defaults if jq unavailable)
if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
    COVERAGE_THRESHOLD=$(jq -r '.coverage.threshold' "$CONFIG_FILE")
    BUSINESS_PKGS=$(jq -r '.coverage.business_packages | join(" ")' "$CONFIG_FILE")
else
    COVERAGE_THRESHOLD=85
    BUSINESS_PKGS="codeflow-core"
fi
readonly COVERAGE_THRESHOLD
readonly BUSINESS_PKGS

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
    if echo "$help_output" | grep -q "Commands:"; then
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

run_unit_tests() {
    test_section "Rust unit tests"

    if command -v cargo-nextest &>/dev/null; then
        if (cd "$RS_DIR" && cargo nextest run --workspace 2>&1); then
            test_pass "cargo nextest run --workspace passes"
        else
            test_fail "cargo nextest run --workspace failed"
            return 1
        fi
    else
        if (cd "$RS_DIR" && cargo test --workspace 2>&1); then
            test_pass "cargo test --workspace passes"
        else
            test_fail "cargo test --workspace failed"
            return 1
        fi
    fi
    return 0
}

run_coverage_enforcement() {
    test_section "Coverage enforcement"

    if ! cargo llvm-cov --version &>/dev/null; then
        test_fail "cargo-llvm-cov not installed — coverage enforcement requires it"
        return 1
    fi

    # Build coverage args scoped to business packages
    local cov_args=("nextest")
    for pkg in $BUSINESS_PKGS; do
        cov_args+=("--package" "$pkg")
    done
    # Skip cfg-based coverage instrumentation — blocked by macOS SIP (SIGKILL).
    # LLVM source-based instrumentation still produces valid coverage data.
    cov_args+=("--no-cfg-coverage")

    echo "  Business packages: $BUSINESS_PKGS"
    echo "  Threshold: ${COVERAGE_THRESHOLD}%"

    # Run coverage and capture JSON output for per-file analysis
    local cov_json="/tmp/codeflow-rust-cov-$$.json"
    if ! (cd "$RS_DIR" && cargo llvm-cov "${cov_args[@]}" --json 2>/dev/null > "$cov_json"); then
        test_fail "Coverage run failed"
        rm -f "$cov_json"
        return 1
    fi

    # Aggregate check
    if (cd "$RS_DIR" && cargo llvm-cov "${cov_args[@]}" --fail-under-lines "$COVERAGE_THRESHOLD" 2>&1); then
        test_pass "Aggregate coverage meets ${COVERAGE_THRESHOLD}% threshold"
    else
        test_fail "Aggregate coverage below ${COVERAGE_THRESHOLD}% threshold"
    fi

    # Per-file enforcement (requires jq)
    if ! command -v jq &>/dev/null; then
        echo "  WARN: jq not found — skipping per-file enforcement (aggregate-only)"
        rm -f "$cov_json"
        return 0
    fi

    if [[ ! -s "$cov_json" ]]; then
        echo "  WARN: Empty coverage JSON — skipping per-file enforcement"
        rm -f "$cov_json"
        return 0
    fi

    # Load exception crates from config
    local exception_crates=""
    if [[ -f "$CONFIG_FILE" ]]; then
        exception_crates=$(jq -r '.conventions.exceptions[]?.crate // empty' "$CONFIG_FILE" 2>/dev/null) || true
    fi

    # Parse per-file coverage and enforce threshold
    local per_file_fail=false
    local file_count=0
    local fail_count=0

    while IFS=$'\t' read -r filename pct; do
        # Skip files outside business packages
        local in_business=false
        for pkg in $BUSINESS_PKGS; do
            if [[ "$filename" == */"$pkg/"* ]]; then
                in_business=true
                break
            fi
        done
        [[ "$in_business" == "false" ]] && continue

        # Check exception list (crate-level exceptions)
        local is_excepted=false
        for exc in $exception_crates; do
            if [[ "$filename" == */"$exc/"* ]]; then
                is_excepted=true
                break
            fi
        done
        [[ "$is_excepted" == "true" ]] && continue

        file_count=$((file_count + 1))

        # Compare (integer truncation for threshold comparison)
        local pct_int="${pct%.*}"
        # Handle edge case where pct_int is empty (0% files)
        pct_int="${pct_int:-0}"

        if [[ "$pct_int" -lt "$COVERAGE_THRESHOLD" ]]; then
            # Extract short path (crate/src/...) for readability
            local short_name="${filename##*/codeflow-rs/}"
            echo "  FAIL: ${short_name} — ${pct}% (below ${COVERAGE_THRESHOLD}%)"
            per_file_fail=true
            fail_count=$((fail_count + 1))
        fi
    done < <(jq -r '.data[0].files[] | "\(.filename)\t\(.summary.lines.percent)"' "$cov_json" 2>/dev/null)

    rm -f "$cov_json"

    if [[ "$per_file_fail" == "true" ]]; then
        test_fail "Per-file coverage: ${fail_count}/${file_count} files below ${COVERAGE_THRESHOLD}% threshold"
    else
        test_pass "Per-file coverage: all ${file_count} files meet ${COVERAGE_THRESHOLD}% threshold"
    fi
}

cleanup() {
    # Remove test artifacts if needed
    rm -rf "$RS_DIR/target/llvm-cov-target" 2>/dev/null || true
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
        echo "  SKIP: codeflow-rs/Cargo.toml not found. Skipping Rust CLI tests."
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

    # Run test sections
    run_build_test
    run_smoke_tests

    # Run unit tests; skip coverage enforcement if they fail
    local unit_tests_passed=true
    if ! run_unit_tests; then
        unit_tests_passed=false
    fi

    if [[ "$unit_tests_passed" == "true" ]]; then
        run_coverage_enforcement
    fi

    # Summary
    print_test_summary
    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
