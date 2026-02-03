#!/usr/bin/env bash
# CodeFlow Test Runner
# Location: .codeflow/testing/run-all-tests.sh
#
# Usage:
#   ./run-all-tests.sh                    # Run with default mode (standard)
#   ./run-all-tests.sh --mode essential   # Run essential mode (fastest)
#   ./run-all-tests.sh --mode full        # Run full mode (all tests)
#   ./run-all-tests.sh --category hooks-pre-tool-use  # Run specific category
#   ./run-all-tests.sh --verbose          # Verbose output
#   ./run-all-tests.sh --dry-run          # Show what would run
#   ./run-all-tests.sh --stop-on-fail     # Stop on first failure
#   ./run-all-tests.sh --validate-coverage  # Validate test coverage
#   ./run-all-tests.sh --report           # Generate report

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Load framework
source "$SCRIPT_DIR/lib/test-runner.sh"
source "$SCRIPT_DIR/lib/test-reporting.sh"
source "$SCRIPT_DIR/lib/test-coverage.sh"

# ============================================================================
# DEFAULTS
# ============================================================================

RUNNER_CATEGORY=""
GENERATE_REPORT="false"
VALIDATE_COVERAGE_FIRST="false"

# ============================================================================
# ARGUMENT PARSING
# ============================================================================

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)
            TEST_MODE="$2"
            shift 2
            ;;
        --category)
            RUNNER_CATEGORY="$2"
            shift 2
            ;;
        --verbose|-v)
            RUNNER_VERBOSE="true"
            shift
            ;;
        --stop-on-fail)
            RUNNER_STOP_ON_FAIL="true"
            shift
            ;;
        --dry-run)
            RUNNER_DRY_RUN="true"
            shift
            ;;
        --report)
            GENERATE_REPORT="true"
            shift
            ;;
        --validate-coverage)
            VALIDATE_COVERAGE_FIRST="true"
            shift
            ;;
        --help|-h)
            echo "CodeFlow Test Runner"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --mode <mode>         Test mode: essential, standard, full (default: standard)"
            echo "  --category <cat>      Run specific category only"
            echo "  --verbose, -v         Verbose output"
            echo "  --stop-on-fail        Stop on first failure"
            echo "  --dry-run             Show what would run"
            echo "  --report              Generate JSON/text reports"
            echo "  --validate-coverage   Validate test coverage before running"
            echo "  --help, -h            Show this help"
            echo ""
            echo "Modes:"
            echo "  essential   CRITICAL priority only (~30s)"
            echo "  standard    CRITICAL + HIGH priorities (~60s)"
            echo "  full        All priorities (~120s)"
            echo ""
            echo "Categories:"
            for cat in $(list_categories); do
                echo "  $cat"
            done
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# ============================================================================
# MAIN
# ============================================================================

main() {
    local mode
    mode=$(get_current_mode)

    # Print mode info
    print_mode_info "$mode"

    # Validate coverage first if requested
    if [[ "$VALIDATE_COVERAGE_FIRST" == "true" ]]; then
        validate_coverage "warn" || true
        echo ""
    fi

    # Run tests
    local start_time
    start_time=$(date +%s)
    local exit_code=0

    if [[ -n "$RUNNER_CATEGORY" ]]; then
        run_category_tests "$RUNNER_CATEGORY" "$mode" || exit_code=$?
    else
        run_all_tests "$mode" || exit_code=$?
    fi

    local end_time
    end_time=$(date +%s)
    local duration=$((end_time - start_time))

    # Generate reports if requested
    if [[ "$GENERATE_REPORT" == "true" ]]; then
        log_info "Generating reports..."
        local json_report
        json_report=$(generate_json_report \
            "${#PASSED_TESTS[@]}" \
            "${#FAILED_TESTS[@]}" \
            "${#SKIPPED_TESTS[@]}" \
            "$duration" \
            "$mode" \
            "${FAILED_TESTS[@]}")
        log_info "JSON report: $json_report"

        local text_report
        text_report=$(generate_text_report \
            "${#PASSED_TESTS[@]}" \
            "${#FAILED_TESTS[@]}" \
            "${#SKIPPED_TESTS[@]}" \
            "$duration" \
            "$mode" \
            "${FAILED_TESTS[@]}")
        log_info "Text report: $text_report"
    fi

    exit $exit_code
}

main
