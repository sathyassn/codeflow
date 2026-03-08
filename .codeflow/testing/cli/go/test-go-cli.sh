#!/usr/bin/env bash
set -euo pipefail

# test-go-cli.sh -- Bridge test script for the Go CLI.
# Runs build verification, binary smoke tests, Go unit tests, and coverage enforcement.

readonly SCRIPT_NAME="test-go-cli"
readonly SCRIPT_VERSION="2.0.0"

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
        rm -f "$business_cov_file"

        echo "  Business package coverage: ${impl}%"
        echo "  Threshold: ${COVERAGE_THRESHOLD}%"

        if [ "$(echo "${impl} < ${COVERAGE_THRESHOLD}" | bc -l)" -eq 1 ]; then
            test_fail "Business package coverage ${impl}% is below ${COVERAGE_THRESHOLD}% threshold"
        else
            test_pass "Business package coverage ${impl}% meets ${COVERAGE_THRESHOLD}% threshold"
        fi
    else
        rm -f "$business_cov_file"
        test_fail "Business package test run failed during coverage check"
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
