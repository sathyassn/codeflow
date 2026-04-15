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
STRUCTURAL_CHECK="auto"
MAX_JOBS=""
SEQUENTIAL="false"
CTRF_FORMAT=""
CTRF_OUTPUT=""

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
        --mode=*)
            validate_mode "${1#--mode=}" || exit 1
            export TEST_MODE="${1#--mode=}"
            shift
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
        --structural)
            STRUCTURAL_CHECK="always"
            shift
            ;;
        --skip-structural)
            STRUCTURAL_CHECK="never"
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
        --format|--format=*)
            if [[ "$1" == --format=* ]]; then
                CTRF_FORMAT="${1#--format=}"
                shift
            else
                CTRF_FORMAT="$2"
                shift 2
            fi
            ;;
        --output|--output=*)
            if [[ "$1" == --output=* ]]; then
                CTRF_OUTPUT="${1#--output=}"
                shift
            else
                CTRF_OUTPUT="$2"
                shift 2
            fi
            ;;
        --help|-h)
            echo "CodeFlow Test Runner"
            echo ""
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --mode <mode>         Test mode: essential, standard, full (default: full)"
            echo "  --category <cat>      Run specific category only (sequential)"
            echo "  --jobs N              Max parallel category jobs (default: 6)"
            echo "  --sequential          Force sequential execution (no parallelism)"
            echo "  --verbose, -v         Verbose output"
            echo "  --stop-on-fail        Stop on first failure"
            echo "  --dry-run             Show what would run"
            echo "  --coverage            Run with coverage enforcement (fail if below $(get_coverage_threshold fail_under)%)"
            echo "  --report              Generate JSON/text reports"
            echo "  --validate-coverage   Validate test coverage before running"
            echo "  --structural          Force bidirectional structural integrity check"
            echo "  --skip-structural     Skip structural integrity check"
            echo "  --help, -h            Show this help"
            echo ""
            echo "Modes:"
            echo "  essential   CRITICAL priority only (~20s)"
            echo "  standard    CRITICAL + HIGH priorities (~30s parallel)"
            echo "  full        All priorities + coverage validation (~40s parallel)"
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

    # Structural integrity check (bidirectional test-to-source mapping)
    # Runs in standard and full modes by default, or when explicitly requested
    local run_structural=false
    if [[ "$STRUCTURAL_CHECK" == "always" ]]; then
        run_structural=true
    elif [[ "$STRUCTURAL_CHECK" == "auto" ]] && [[ "$mode" == "standard" || "$mode" == "full" ]]; then
        run_structural=true
    fi

    if [[ "$run_structural" == "true" ]]; then
        if ! validate_structural_integrity; then
            log_error "Structural integrity check failed — fix issues before running tests"
            exit 1
        fi
        echo ""
    fi

    # Validate coverage first if requested (legacy flag, source→test only)
    if [[ "$VALIDATE_COVERAGE_FIRST" == "true" ]] && [[ "$run_structural" != "true" ]]; then
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

    # Auto-validate coverage in full mode (skip if already validated via structural or --validate-coverage)
    if [[ "$mode" == "full" ]] && [[ "$VALIDATE_COVERAGE_FIRST" != "true" ]] && [[ "$run_structural" != "true" ]]; then
        echo ""
        if ! validate_coverage; then
            # Coverage validation failed (only when validation_mode=fail in config)
            [[ $exit_code -eq 0 ]] && exit_code=1
        fi
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

    # Emit CTRF JSON if --format=ctrf --output=<path> were given
    if [[ "$CTRF_FORMAT" == "ctrf" ]]; then
        if [[ -z "$CTRF_OUTPUT" ]]; then
            log_error "--format=ctrf requires --output=<path>"
            exit 1
        fi
        if [[ "$CTRF_OUTPUT" == *".."* ]]; then
            log_error "--output path must not contain '..': $CTRF_OUTPUT"
            exit 1
        fi
        command -v jq >/dev/null 2>&1 || { log_error "jq is required for --format=ctrf output but is not installed"; exit 1; }
    fi
    if [[ "$CTRF_FORMAT" == "ctrf" ]] && [[ -n "$CTRF_OUTPUT" ]]; then
        local passed_count="${#PASSED_TESTS[@]}"
        local failed_count="${#FAILED_TESTS[@]}"
        local skipped_count="${#SKIPPED_TESTS[@]}"
        local total=$((passed_count + failed_count + skipped_count))
        local epoch_ms
        epoch_ms=$(date +%s)000

        local tests_json="["
        local first=true
        for t in "${PASSED_TESTS[@]}"; do
            if [[ "$first" != "true" ]]; then tests_json+=","; fi
            first=false
            tests_json+="{\"name\":$(printf '%s' "$t" | jq -Rs .),\"status\":\"passed\",\"duration\":0}"
        done
        for t in "${FAILED_TESTS[@]}"; do
            if [[ "$first" != "true" ]]; then tests_json+=","; fi
            first=false
            tests_json+="{\"name\":$(printf '%s' "$t" | jq -Rs .),\"status\":\"failed\",\"duration\":0}"
        done
        for t in "${SKIPPED_TESTS[@]}"; do
            if [[ "$first" != "true" ]]; then tests_json+=","; fi
            first=false
            tests_json+="{\"name\":$(printf '%s' "$t" | jq -Rs .),\"status\":\"skipped\",\"duration\":0}"
        done
        tests_json+="]"

        cat > "$CTRF_OUTPUT" <<CTRF_EOF
{
  "results": {
    "tool": {"name": "codeflow-shell-tests"},
    "summary": {
      "tests": $total,
      "passed": $passed_count,
      "failed": $failed_count,
      "pending": 0,
      "skipped": $skipped_count,
      "other": 0,
      "start": $epoch_ms,
      "stop": $epoch_ms
    },
    "tests": $tests_json
  }
}
CTRF_EOF
    fi

    exit $exit_code
}

main
