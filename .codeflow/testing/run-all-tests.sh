#!/usr/bin/env bash
# CodeFlow Test Runner
# Location: .codeflow/testing/run-all-tests.sh
#
# Usage:
#   ./run-all-tests.sh                    # Run with default mode (parallel, 6 jobs)
#   ./run-all-tests.sh --mode essential   # Run essential mode (fastest)
#   ./run-all-tests.sh --mode full        # Run full mode (all tests)
#   ./run-all-tests.sh --jobs 4           # Override parallel job count
#   ./run-all-tests.sh --sequential       # Force sequential execution
#   ./run-all-tests.sh --category scripts-db  # Run specific category
#   ./run-all-tests.sh --verbose          # Verbose output
#   ./run-all-tests.sh --dry-run          # Show what would run
#   ./run-all-tests.sh --stop-on-fail     # Stop on first failure
#   ./run-all-tests.sh --validate-coverage  # Validate test coverage
#   ./run-all-tests.sh --report           # Generate report

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Load framework
source "$SCRIPT_DIR/lib/test-runner.sh"
source "$SCRIPT_DIR/lib/test-parallel.sh"
source "$SCRIPT_DIR/lib/test-reporting.sh"
source "$SCRIPT_DIR/lib/test-coverage.sh"

# ============================================================================
# DEFAULTS
# ============================================================================

RUNNER_CATEGORY=""
GENERATE_REPORT="false"
VALIDATE_COVERAGE_FIRST="false"
MAX_JOBS=""
SEQUENTIAL="false"

# ============================================================================
# ARGUMENT PARSING
# ============================================================================

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mode)
            validate_mode "$2" || exit 1
            export TEST_MODE="$2"
            shift 2
            ;;
        --category)
            export RUNNER_CATEGORY="$2"
            shift 2
            ;;
        --verbose|-v)
            export RUNNER_VERBOSE="true"
            shift
            ;;
        --stop-on-fail)
            export RUNNER_STOP_ON_FAIL="true"
            shift
            ;;
        --dry-run)
            export RUNNER_DRY_RUN="true"
            shift
            ;;
        --coverage|--with-coverage)
            export RUNNER_WITH_COVERAGE="true"
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
        --jobs)
            if [[ -z "${2:-}" ]] || ! [[ "$2" =~ ^[1-9][0-9]*$ ]]; then
                log_error "--jobs requires a positive integer (got: '${2:-}')"
                exit 1
            fi
            MAX_JOBS="$2"
            shift 2
            ;;
        --sequential)
            SEQUENTIAL="true"
            shift
            ;;
        --help|-h)
            echo "CodeFlow Test Runner"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --mode <mode>         Test mode: essential, standard, full (default: standard)"
            echo "  --category <cat>      Run specific category only (sequential)"
            echo "  --jobs N              Max parallel category jobs (default: 6)"
            echo "  --sequential          Force sequential execution (no parallelism)"
            echo "  --verbose, -v         Verbose output"
            echo "  --stop-on-fail        Stop on first failure"
            echo "  --dry-run             Show what would run"
            echo "  --coverage            Run with coverage enforcement (fail if below $(get_coverage_threshold fail_under)%)"
            echo "  --report              Generate JSON/text reports"
            echo "  --validate-coverage   Validate test coverage before running"
            echo "  --help, -h            Show this help"
            echo ""
            echo "Modes:"
            echo "  essential   CRITICAL priority only (~20s)"
            echo "  standard    CRITICAL + HIGH priorities (~30s parallel)"
            echo "  full        All priorities (~40s parallel)"
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

    # Determine parallel job count
    local jobs="${MAX_JOBS:-}"
    if [[ -z "$jobs" ]]; then
        if [[ -f "$SCRIPT_DIR/test-config.json" ]] && command -v jq &>/dev/null; then
            jobs=$(jq -r '.parallel.max_jobs // 6' "$SCRIPT_DIR/test-config.json" 2>/dev/null)
        fi
        jobs="${jobs:-6}"
    fi
    if [[ "$SEQUENTIAL" == "true" ]]; then
        jobs=1
    fi

    # Validate coverage first if requested
    if [[ "$VALIDATE_COVERAGE_FIRST" == "true" ]]; then
        validate_coverage "warn" || true
        echo ""
    fi

    # Run tests
    local exit_code=0

    if [[ -n "$RUNNER_CATEGORY" ]]; then
        # Single category — always sequential
        print_mode_info "$mode"
        run_category_tests "$RUNNER_CATEGORY" "$mode" || exit_code=$?
    elif [[ "$jobs" -gt 1 ]] && [[ "${RUNNER_DRY_RUN:-false}" != "true" ]]; then
        # Parallel execution (default)
        run_all_tests_parallel "$mode" "$jobs" || exit_code=$?
    else
        # Sequential: --sequential, --jobs 1, or --dry-run
        run_all_tests "$mode" || exit_code=$?
    fi

    # Generate reports if requested
    if [[ "$GENERATE_REPORT" == "true" ]]; then
        log_info "Generating reports..."
        local json_report
        json_report=$(generate_json_report \
            "${#PASSED_TESTS[@]}" \
            "${#FAILED_TESTS[@]}" \
            "${#SKIPPED_TESTS[@]}" \
            "0" \
            "$mode" \
            "${FAILED_TESTS[@]}")
        log_info "JSON report: $json_report"

        local text_report
        text_report=$(generate_text_report \
            "${#PASSED_TESTS[@]}" \
            "${#FAILED_TESTS[@]}" \
            "${#SKIPPED_TESTS[@]}" \
            "0" \
            "$mode" \
            "${FAILED_TESTS[@]}")
        log_info "Text report: $text_report"
    fi

    exit $exit_code
}

main
