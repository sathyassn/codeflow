#!/usr/bin/env bash
set -euo pipefail

# test-go-cli.sh -- Bridge test script for the Go CLI.
# Runs build verification, binary smoke tests, Go unit tests, and coverage enforcement.

readonly SCRIPT_NAME="test-go-cli"
readonly SCRIPT_VERSION="2.1.0"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR
REPO_ROOT="$(cd "$TESTING_DIR/../.." && pwd)"
readonly REPO_ROOT
readonly CLI_DIR="$REPO_ROOT/codeflow-cli"
readonly CONFIG_FILE="$CLI_DIR/config/testing/test-config.json"

# Read coverage settings from config (fallback to defaults if jq unavailable)
if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
    COVERAGE_THRESHOLD=$(jq -r '.coverage.threshold' "$CONFIG_FILE")
    BUSINESS_PKGS=$(jq -r '.coverage.business_packages | join(" ")' "$CONFIG_FILE")
else
    COVERAGE_THRESHOLD=85
    BUSINESS_PKGS="./internal/db/... ./internal/session/... ./cmd/codeflow/... ./cmd/autorun/..."
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

Bridge test script for the Go CLI. Runs build, smoke, unit tests, and coverage.

Options:
  -h, --help     Show this help message
  -v, --version  Show script version

EOF
}

run_build_test() {
    test_section "Build test"

    if (cd "$CLI_DIR" && go build -o "$CLI_DIR/bin/codeflow-test" ./cmd/codeflow/) 2>/dev/null; then
        test_pass "go build succeeds"
    else
        test_fail "go build failed"
    fi
}

run_smoke_tests() {
    local test_binary="$CLI_DIR/bin/codeflow-test"

    if [[ ! -x "$test_binary" ]]; then
        test_fail "test binary not found or not executable at $test_binary"
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
    if echo "$help_output" | grep -q "Available Commands:"; then
        test_pass "codeflow --help shows Available Commands"
    else
        test_fail "codeflow --help missing 'Available Commands'"
    fi

    if echo "$help_output" | grep -q "version"; then
        test_pass "codeflow --help lists version subcommand"
    else
        test_fail "codeflow --help missing version subcommand"
    fi

    if echo "$help_output" | grep -q "uninstall"; then
        test_pass "codeflow --help lists uninstall subcommand"
    else
        test_fail "codeflow --help missing uninstall subcommand"
    fi

    # version subcommand
    local ver_output
    ver_output=$("$test_binary" version 2>&1) || true
    if [[ "$ver_output" == codeflow* ]]; then
        test_pass "codeflow version outputs version string"
    else
        test_fail "codeflow version unexpected output: $ver_output"
    fi
}

run_unit_tests() {
    test_section "Go unit tests"

    # Test explicit subdirectory patterns to avoid root module (no Go sources at root).
    if (cd "$CLI_DIR" && go test -coverprofile=coverage.out ./cmd/... ./internal/... -v -count=1) 2>&1; then
        test_pass "go test ./... passes"
    else
        test_fail "go test ./... failed"
        return 1
    fi
    return 0
}

run_coverage_enforcement() {
    test_section "Coverage enforcement"

    local coverage_file="$CLI_DIR/coverage.out"
    if [[ ! -f "$coverage_file" ]]; then
        test_fail "coverage.out not found -- unit tests may have failed"
        return
    fi

    # Overall coverage
    local overall
    overall=$(cd "$CLI_DIR" && go tool cover -func=coverage.out | grep '^total:' | awk '{print $NF}' | tr -d '%')
    echo "  Overall coverage (all packages): ${overall}%"

    # Business package coverage (packages from test-config.json)
    local business_cov_file="$CLI_DIR/business.out"
    IFS=' ' read -ra _biz_pkgs <<< "$BUSINESS_PKGS"
    if (cd "$CLI_DIR" && go test -coverprofile=business.out \
        "${_biz_pkgs[@]}" \
        > /dev/null 2>&1); then

        local impl
        impl=$(cd "$CLI_DIR" && go tool cover -func=business.out | grep '^total:' | awk '{print $NF}' | tr -d '%')

        echo "  Business package coverage: ${impl}%"
        echo "  Threshold: ${COVERAGE_THRESHOLD}%"

        if [ "$(echo "${impl} < ${COVERAGE_THRESHOLD}" | bc -l)" -eq 1 ]; then
            test_fail "Business package coverage ${impl}% is below ${COVERAGE_THRESHOLD}% threshold"
        else
            test_pass "Business package coverage ${impl}% meets ${COVERAGE_THRESHOLD}% threshold"
        fi

        # Per-file enforcement (uses business.out for accurate per-file data)
        run_per_file_coverage "$business_cov_file"

        rm -f "$business_cov_file"
    else
        rm -f "$business_cov_file"
        test_fail "Business package test run failed during coverage check"
    fi
}

# run_per_file_coverage parses go tool cover -func output for per-file percentages
# and enforces the threshold on each file in business packages.
run_per_file_coverage() {
    local cov_file="$1"

    test_section "Per-file coverage enforcement"

    if [[ ! -f "$cov_file" ]]; then
        echo "  WARN: Coverage file not found -- skipping per-file enforcement"
        return
    fi

    # Load file-level exceptions from config
    local exception_files=""
    if [[ -f "$CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        exception_files=$(jq -r '.conventions.exceptions[]?.file // empty' "$CONFIG_FILE" 2>/dev/null) || true
    fi

    local per_file_fail=false
    local file_count=0
    local fail_count=0
    local pass_count=0

    # Parse go tool cover -func output: each line is "file:line:\tfunction\tpercent%"
    # Aggregate by file: use the last line per file (total for that file).
    # go tool cover -func groups by file, with a "total:" line at the very end.
    # We extract per-file totals by looking at each unique file's functions.
    #
    # Strategy: parse all lines, collect coverage per function, compute per-file average.
    # Simpler: use go tool cover -func and group the file-level total.
    # Actually, go tool cover -func doesn't give per-file totals -- only per-function + grand total.
    # So we compute per-file coverage by counting covered/total statements from the raw profile.

    # Use go tool cover -func to get per-function coverage, then aggregate per file.
    local func_output
    func_output=$(cd "$CLI_DIR" && go tool cover -func="$cov_file" 2>/dev/null) || return

    # Build per-file coverage: sum up covered statements per file.
    # Format of each line: "github.com/.../file.go:line:\tfunction\t\tpercent%"
    # We'll track unique files and their average coverage.
    declare -A file_coverages
    declare -A file_counts

    while IFS= read -r line; do
        # Skip the grand total line
        [[ "$line" == total:* ]] && continue

        # Extract file path and percentage
        local file_path pct_str
        file_path=$(echo "$line" | awk '{print $1}' | cut -d: -f1)
        pct_str=$(echo "$line" | awk '{print $NF}' | tr -d '%')

        [[ -z "$file_path" || -z "$pct_str" ]] && continue

        # Extract short path (relative to module)
        local short_path="${file_path#*/codeflow-cli/}"

        # Accumulate
        if [[ -n "${file_coverages[$short_path]+x}" ]]; then
            file_coverages[$short_path]=$(echo "${file_coverages[$short_path]} + $pct_str" | bc -l)
            file_counts[$short_path]=$((file_counts[$short_path] + 1))
        else
            file_coverages[$short_path]="$pct_str"
            file_counts[$short_path]=1
        fi
    done <<< "$func_output"

    # Check each file against threshold
    for short_path in "${!file_coverages[@]}"; do
        # Only check files in business packages
        local in_business=false
        IFS=' ' read -ra biz_array <<< "$BUSINESS_PKGS"
        for pkg in "${biz_array[@]}"; do
            # Convert "./internal/hooks/..." to "internal/hooks/"
            local pkg_prefix="${pkg#./}"
            pkg_prefix="${pkg_prefix%/...}"
            if [[ "$short_path" == "$pkg_prefix/"* ]]; then
                in_business=true
                break
            fi
        done
        [[ "$in_business" == "false" ]] && continue

        # Check exception list
        local is_excepted=false
        for exc in $exception_files; do
            if [[ "$short_path" == "$exc" ]]; then
                is_excepted=true
                break
            fi
        done
        [[ "$is_excepted" == "true" ]] && continue

        # Skip test files
        [[ "$short_path" == *_test.go ]] && continue

        file_count=$((file_count + 1))

        # Compute average coverage for this file
        local avg
        avg=$(echo "${file_coverages[$short_path]} / ${file_counts[$short_path]}" | bc -l)
        local avg_int="${avg%.*}"
        avg_int="${avg_int:-0}"

        if [[ "$avg_int" -lt "$COVERAGE_THRESHOLD" ]]; then
            printf "  FAIL: %s -- %.1f%% (below %s%%)\n" "$short_path" "$avg" "$COVERAGE_THRESHOLD"
            per_file_fail=true
            fail_count=$((fail_count + 1))
        else
            pass_count=$((pass_count + 1))
        fi
    done

    if [[ "$per_file_fail" == "true" ]]; then
        test_fail "Per-file coverage: ${fail_count}/${file_count} files below ${COVERAGE_THRESHOLD}% threshold"
    else
        test_pass "Per-file coverage: all ${file_count} files meet ${COVERAGE_THRESHOLD}% threshold"
    fi
}

cleanup() {
    rm -f "$CLI_DIR/bin/codeflow-test"
    rm -f "$CLI_DIR/coverage.out"
    rm -f "$CLI_DIR/coverage.log"
    rm -f "$CLI_DIR/business.out"
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

    # Skip if Go is not available
    if ! command -v go &>/dev/null; then
        test_section "Go CLI Bridge Tests"
        echo "  SKIP: 'go' not found on PATH. Skipping Go CLI tests."
        exit 0
    fi

    # Check Go version meets go.mod requirement
    local required_go
    required_go=$(grep '^go ' "$CLI_DIR/go.mod" 2>/dev/null | awk '{print $2}')
    if [[ -n "$required_go" ]]; then
        local current_go
        current_go=$(go version | grep -oE 'go[0-9]+\.[0-9]+(\.[0-9]+)?' | sed 's/go//')
        if [[ "$(printf '%s\n' "$required_go" "$current_go" | sort -V | head -1)" != "$required_go" ]]; then
            test_section "Go CLI Bridge Tests"
            echo "  SKIP: Go $current_go < required $required_go. Skipping Go CLI tests."
            exit 0
        fi
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
