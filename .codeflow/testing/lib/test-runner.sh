#!/usr/bin/env bash
# CodeFlow Test Framework: Test Runner
# Location: .codeflow/testing/lib/test-runner.sh

# Requires: test-common.sh, test-helpers.sh, test-config.sh, test-discovery.sh
RUNNER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$RUNNER_DIR/test-common.sh"
source "$RUNNER_DIR/test-helpers.sh"
source "$RUNNER_DIR/test-config.sh"
source "$RUNNER_DIR/test-discovery.sh"

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
        output=$(python3 -m pytest "$test_file" -v --tb=short 2>&1) || exit_code=$?
    else
        output=$("$test_file" 2>&1) || exit_code=$?
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
    local testing_root="${RUNNER_DIR}/.."
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

    # Check if directory has Python tests
    if [[ -f "$full_path/conftest.py" ]] || ls "$full_path"/test_*.py &>/dev/null 2>&1; then
        # Run pytest for Python tests
        if [[ "$RUNNER_DRY_RUN" == "true" ]]; then
            log_info "[DRY RUN] Would run pytest for $category"
        else
            log_info "Running pytest for $category..."
            python3 -m pytest "$full_path" -v --tb=short 2>&1 || {
                FAILED_TESTS+=("$category (pytest)")
            }
        fi
    else
        # Run bash tests
        local tests=()
        while IFS= read -r test_file; do
            [[ -n "$test_file" ]] && tests+=("$test_file")
        done < <(discover_tests_in_dir "$full_path")

        if [[ ${#tests[@]} -eq 0 ]]; then
            log_info "No tests found in $category"
            return 0
        fi

        for test_file in "${tests[@]}"; do
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
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    if [[ ${#FAILED_TESTS[@]} -gt 0 ]]; then
        echo ""
        echo -e "${RED}Failed Tests:${NC}"
        for test in "${FAILED_TESTS[@]}"; do
            echo "  - $(basename "$test")"
        done
        echo ""
        echo -e "${RED}FAILED${NC}"
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
            *)
                shift
                ;;
        esac
    done
}
