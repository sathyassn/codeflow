#!/usr/bin/env bash
# CodeFlow Test Framework: Test Runner
# Location: .codeflow/testing/lib/test-runner.sh

# Requires: test-common.sh, test-helpers.sh, test-config.sh, test-discovery.sh
RUNNER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$RUNNER_DIR/test-common.sh"
source "$RUNNER_DIR/test-helpers.sh"
source "$RUNNER_DIR/test-config.sh"
source "$RUNNER_DIR/test-discovery.sh"

# Detect Python with pytest - prefer venv if available
TESTING_ROOT="$(cd "${RUNNER_DIR}/.." && pwd)"
if [[ -x "${TESTING_ROOT}/.venv/bin/python" ]]; then
    PYTHON="${TESTING_ROOT}/.venv/bin/python"
elif [[ -x "${TESTING_ROOT}/venv/bin/python" ]]; then
    PYTHON="${TESTING_ROOT}/venv/bin/python"
else
    PYTHON="python3"
fi

# ============================================================================
# RUNNER STATE
# ============================================================================

declare -a FAILED_TESTS=()
declare -a PASSED_TESTS=()
declare -a SKIPPED_TESTS=()

RUNNER_START_TIME=""
RUNNER_STOP_ON_FAIL="${STOP_ON_FAIL:-false}"
RUNNER_VERBOSE="${VERBOSE:-false}"
RUNNER_DRY_RUN="${DRY_RUN:-false}"
RUNNER_WITH_COVERAGE="${WITH_COVERAGE:-false}"
RUNNER_COVERAGE_FAILED="false"

# ============================================================================
# SINGLE TEST EXECUTION
# ============================================================================

run_single_test() {
    local test_file="$1"
    local test_name
    test_name=$(basename "$test_file")

    if [[ "$RUNNER_DRY_RUN" == "true" ]]; then
        log_info "[DRY RUN] Would run: $test_name"
        return 0
    fi

    if [[ "$RUNNER_VERBOSE" == "true" ]]; then
        log_info "Running: $test_name"
    fi

    local exit_code=0
    local output=""

    if is_python_test "$test_file"; then
        output=$($PYTHON -m pytest "$test_file" -v --tb=short 2>&1) || exit_code=$?
    else
        output=$("$test_file" </dev/null 2>&1) || exit_code=$?
    fi

    if [[ $exit_code -eq 0 ]]; then
        PASSED_TESTS+=("$test_file")
        if [[ "$RUNNER_VERBOSE" == "true" ]]; then
            echo "$output"
        fi
        echo -e "${GREEN}✓${NC} $test_name"
        return 0
    else
        FAILED_TESTS+=("$test_file")
        echo -e "${RED}✗${NC} $test_name"
        if [[ "$RUNNER_VERBOSE" == "true" || "$RUNNER_STOP_ON_FAIL" == "true" ]]; then
            echo "$output"
        fi

        if [[ "$RUNNER_STOP_ON_FAIL" == "true" ]]; then
            log_error "Stopping on first failure"
            return 1
        fi
        return 1
    fi
}

# ============================================================================
# CATEGORY EXECUTION
# ============================================================================

run_category_tests() {
    local category="$1"
    local mode="${2:-$(get_current_mode)}"
    local testing_root
    testing_root="$(cd "${RUNNER_DIR}/.." && pwd)"
    local category_dir
    category_dir=$(get_category_dir "$category")

    if [[ -z "$category_dir" ]]; then
        log_error "Unknown category: $category"
        return 1
    fi

    local full_path="$testing_root/$category_dir"

    if [[ ! -d "$full_path" ]]; then
        log_warn "Category directory not found: $full_path"
        return 0
    fi

    log_section "Category: $category"

    local has_python=false
    local has_bash=false

    # Check for Python tests
    if [[ -f "$full_path/conftest.py" ]] || ls "$full_path"/test_*.py &>/dev/null 2>&1; then
        has_python=true
    fi

    # Check for bash tests
    local bash_tests=()
    while IFS= read -r test_file; do
        [[ -n "$test_file" ]] && bash_tests+=("$test_file")
    done < <(discover_tests_in_dir "$full_path")
    [[ ${#bash_tests[@]} -gt 0 ]] && has_bash=true

    # Run Python tests if present
    if [[ "$has_python" == "true" ]]; then
        local python_tests=()
        for test_file in "$full_path"/test_*.py; do
            [[ -f "$test_file" ]] || continue
            # shellcheck disable=SC2295
            local relative_path="${test_file#"$testing_root"/}"
            if should_run_test "$relative_path" "$mode"; then
                python_tests+=("$test_file")
            else
                SKIPPED_TESTS+=("$test_file")
                if [[ "$RUNNER_VERBOSE" == "true" ]]; then
                    local priority
                    priority=$(get_test_priority "$relative_path")
                    log_info "Skipping (priority $priority): $(basename "$test_file")"
                fi
            fi
        done

        if [[ ${#python_tests[@]} -eq 0 ]]; then
            log_info "No Python tests to run in $category for mode $mode"
        elif [[ "$RUNNER_DRY_RUN" == "true" ]]; then
            log_info "[DRY RUN] Would run pytest for ${#python_tests[@]} files in $category"
            for t in "${python_tests[@]}"; do
                echo "  - $(basename "$t")"
            done
        else
            log_info "Running pytest for ${#python_tests[@]} files in $category..."
            local pytest_output
            local pytest_args=("-v" "--tb=short")

            # Add coverage flags if coverage is enabled
            if [[ "$RUNNER_WITH_COVERAGE" == "true" ]] && is_coverage_enabled; then
                local fail_under
                fail_under=$(get_coverage_threshold fail_under)
                local cov_source="${TESTING_ROOT}/../scripts"
                pytest_args+=("--cov=$cov_source" "--cov-report=term-missing" "--cov-fail-under=$fail_under")
                log_info "Coverage enabled: fail_under=${fail_under}%"
            fi

            if pytest_output=$($PYTHON -m pytest "${python_tests[@]}" "${pytest_args[@]}" 2>&1); then
                local passed
                passed=$(echo "$pytest_output" | grep -oE '[0-9]+ passed' | grep -oE '[0-9]+' || echo 0)
                for ((i=0; i<passed; i++)); do
                    PASSED_TESTS+=("pytest-$category-$i")
                done
                echo "$pytest_output" | tail -10
            else
                if echo "$pytest_output" | grep -q "FAIL Required test coverage"; then
                    RUNNER_COVERAGE_FAILED="true"
                    log_error "Coverage below threshold in $category"
                    echo "$pytest_output" | grep -A2 "TOTAL" | head -5
                    echo "$pytest_output" | grep "FAIL Required" | head -1
                fi
                FAILED_TESTS+=("$category (pytest)")
                echo "$pytest_output" | tail -20
            fi
        fi
    fi

    # Run bash tests if present
    if [[ "$has_bash" == "true" ]]; then
        for test_file in "${bash_tests[@]}"; do
            local priority
            priority=$(get_test_priority "$test_file")

            if should_run_priority "$priority" "$mode"; then
                run_single_test "$test_file" || {
                    [[ "$RUNNER_STOP_ON_FAIL" == "true" ]] && return 1
                }
            else
                SKIPPED_TESTS+=("$test_file")
                if [[ "$RUNNER_VERBOSE" == "true" ]]; then
                    log_info "Skipping (priority $priority): $(basename "$test_file")"
                fi
            fi
        done
    fi

    if [[ "$has_python" == "false" ]] && [[ "$has_bash" == "false" ]]; then
        log_info "No tests found in $category"
    fi

    return 0
}

# ============================================================================
# ALL TESTS EXECUTION
# ============================================================================

run_all_tests() {
    local mode="${1:-$(get_current_mode)}"

    RUNNER_START_TIME=$(date +%s)
    FAILED_TESTS=()
    PASSED_TESTS=()
    SKIPPED_TESTS=()

    log_section "CodeFlow Test Runner"
    log_info "Mode: $mode"
    log_info "Priorities: $(get_mode_priorities "$mode")"
    echo ""

    for category in $(list_categories); do
        run_category_tests "$category" "$mode" || {
            [[ "$RUNNER_STOP_ON_FAIL" == "true" ]] && break
        }
    done

    local end_time
    end_time=$(date +%s)
    local duration=$((end_time - RUNNER_START_TIME))

    print_runner_summary "$duration"

    [[ ${#FAILED_TESTS[@]} -eq 0 ]]
}

# ============================================================================
# SUMMARY
# ============================================================================

print_runner_summary() {
    local duration="${1:-0}"

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${BOLD}Test Run Summary${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "  ${GREEN}Passed:${NC}  ${#PASSED_TESTS[@]}"
    echo -e "  ${RED}Failed:${NC}  ${#FAILED_TESTS[@]}"
    echo -e "  ${YELLOW}Skipped:${NC} ${#SKIPPED_TESTS[@]}"
    echo -e "  Duration: ${duration}s"

    # Show coverage status if coverage was enabled
    if [[ "$RUNNER_WITH_COVERAGE" == "true" ]]; then
        local threshold
        threshold=$(get_coverage_threshold fail_under)
        if [[ "$RUNNER_COVERAGE_FAILED" == "true" ]]; then
            echo -e "  ${RED}Coverage:${NC} BELOW ${threshold}% threshold"
        else
            echo -e "  ${GREEN}Coverage:${NC} ≥${threshold}% (passed)"
        fi
    fi
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    if [[ ${#FAILED_TESTS[@]} -gt 0 ]]; then
        echo ""
        echo -e "${RED}Failed Tests:${NC}"
        for test in "${FAILED_TESTS[@]}"; do
            echo "  - $(basename "$test")"
        done
        echo ""
        if [[ "$RUNNER_COVERAGE_FAILED" == "true" ]]; then
            echo -e "${RED}FAILED (coverage below $(get_coverage_threshold fail_under)%)${NC}"
        else
            echo -e "${RED}FAILED${NC}"
        fi
    else
        echo ""
        echo -e "${GREEN}ALL TESTS PASSED${NC}"
    fi
}

# ============================================================================
# OPTION PARSING
# ============================================================================

parse_runner_options() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --mode)
                export TEST_MODE="$2"
                shift 2
                ;;
            --category)
                export RUNNER_CATEGORY="$2"
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
            --coverage|--with-coverage)
                RUNNER_WITH_COVERAGE="true"
                shift
                ;;
            *)
                shift
                ;;
        esac
    done
}

# ============================================================================
# PARALLEL WORKER
# ============================================================================

# Run a single category and write results to a file (for parallel execution)
# Args: $1=category $2=mode $3=result_file
run_category_worker() {
    local category="$1" mode="$2" result_file="$3"
    local log_file="${result_file%.result}.log"

    # Reset counters for this isolated worker
    PASSED_TESTS=()
    FAILED_TESTS=()
    SKIPPED_TESTS=()

    local start_time end_time exit_code=0
    start_time=$(date +%s)

    run_category_tests "$category" "$mode" > "$log_file" 2>&1 || exit_code=$?

    end_time=$(date +%s)

    # Write structured results (machine-parseable)
    {
        echo "exit_code=$exit_code"
        echo "passed=${#PASSED_TESTS[@]}"
        echo "failed=${#FAILED_TESTS[@]}"
        echo "skipped=${#SKIPPED_TESTS[@]}"
        echo "duration=$((end_time - start_time))"
        echo "failed_list=$(IFS=,; echo "${FAILED_TESTS[*]}")"
    } > "$result_file"
}
