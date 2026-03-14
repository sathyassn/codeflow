#!/usr/bin/env bash
# CodeFlow Test Framework: Parallel Execution Engine
# Location: .codeflow/testing/lib/test-parallel.sh
#
# Provides parallel category execution using a rolling job pool.
# Categories run in parallel (up to max_jobs), but tests within each
# category run sequentially to avoid shared-state race conditions.
# Requires bash 5.0+ for `wait -n`.
#
# Usage: Sourced by run-all-tests.sh. Call run_all_tests_parallel(mode, max_jobs).

# Requires: test-runner.sh (already sources test-common.sh, test-helpers.sh, test-config.sh, test-discovery.sh)
PARALLEL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$PARALLEL_DIR/test-runner.sh"

# ============================================================================
# PARALLEL EXECUTION
# ============================================================================

run_all_tests_parallel() {
    local mode="${1:-$(get_current_mode)}"
    local max_jobs="${2:-6}"

    RUNNER_START_TIME=$(date +%s)
    FAILED_TESTS=()
    PASSED_TESTS=()
    SKIPPED_TESTS=()

    log_section "CodeFlow Test Runner (parallel, ${max_jobs} category jobs)"
    log_info "Mode: $mode"
    log_info "Priorities: $(get_mode_priorities "$mode")"
    echo ""

    # Create temp results directory
    local results_dir
    results_dir=$(mktemp -d "${TMPDIR:-/tmp}/codeflow-test-results-XXXXXX")
    # shellcheck disable=SC2064  # Intentional: expand $results_dir now, not at signal time
    trap "rm -rf '$results_dir'" EXIT

    # Initialize coverage collection directory for parallel workers
    if [[ "$RUNNER_WITH_COVERAGE" == "true" ]] && is_coverage_enabled; then
        export _COV_RESULTS_DIR="$results_dir"
    fi

    # Collect categories
    local -a categories=()
    while IFS= read -r cat; do
        [[ -n "$cat" ]] && categories+=("$cat")
    done < <(list_categories)

    local total=${#categories[@]}
    local -a active_pids=()
    local -a active_cats=()
    local completed=0

    # Stop-on-fail poison file
    local stop_file="$results_dir/.stop"

    for i in "${!categories[@]}"; do
        local category="${categories[$i]}"

        # Skip categories marked ci_skip when running in CI
        if [[ "${CI:-}" == "true" ]] && command -v jq &>/dev/null; then
            local config_file="${RUNNER_DIR}/../test-config.json"
            if [[ -f "$config_file" ]]; then
                local ci_skip
                ci_skip=$(jq -r --arg cat "$category" '.categories[$cat].ci_skip // false' "$config_file" 2>/dev/null)
                if [[ "$ci_skip" == "true" ]]; then
                    echo "  [SKIP] ${category}: skipped in CI (dedicated CI job)"
                    continue
                fi
            fi
        fi

        # Check poison file (stop-on-fail)
        if [[ -f "$stop_file" ]]; then
            SKIPPED_TESTS+=("$category (cancelled)")
            continue
        fi

        # Wait if at capacity
        while [[ ${#active_pids[@]} -ge $max_jobs ]]; do
            # Wait for any one job to finish
            local finished_pid=0
            wait -n -p finished_pid "${active_pids[@]}" 2>/dev/null || true

            # Remove finished PID and process its results
            local -a new_pids=()
            local -a new_cats=()
            for j in "${!active_pids[@]}"; do
                if [[ "${active_pids[$j]}" -eq "$finished_pid" ]]; then
                    # This one finished — process results
                    _process_category_result "${active_cats[$j]}" "$results_dir" "$stop_file"
                    ((completed++))
                    _print_progress "$completed" "$total" "${active_cats[$j]}" "$results_dir"
                else
                    new_pids+=("${active_pids[$j]}")
                    new_cats+=("${active_cats[$j]}")
                fi
            done
            active_pids=("${new_pids[@]}")
            active_cats=("${new_cats[@]}")
        done

        # Launch category worker in background (sequential within category)
        (
            # Each worker writes coverage to its own file (avoids race conditions)
            # shellcheck disable=SC2030  # Intentional: subshell isolation per worker
            if [[ -n "${_COV_RESULTS_DIR:-}" ]]; then
                export COVERAGE_FILE="${_COV_RESULTS_DIR}/.coverage.${category}"
            fi
            run_category_worker "$category" "$mode" "$results_dir/${category}.result"
        ) &
        active_pids+=($!)
        active_cats+=("$category")
    done

    # Drain remaining jobs
    while [[ ${#active_pids[@]} -gt 0 ]]; do
        local finished_pid=0
        wait -n -p finished_pid "${active_pids[@]}" 2>/dev/null || true

        local -a new_pids=()
        local -a new_cats=()
        for j in "${!active_pids[@]}"; do
            if [[ "${active_pids[$j]}" -eq "$finished_pid" ]]; then
                _process_category_result "${active_cats[$j]}" "$results_dir" "$stop_file"
                ((completed++))
                _print_progress "$completed" "$total" "${active_cats[$j]}" "$results_dir"
            else
                new_pids+=("${active_pids[$j]}")
                new_cats+=("${active_cats[$j]}")
            fi
        done
        active_pids=("${new_pids[@]}")
        active_cats=("${new_cats[@]}")
    done

    # Final aggregation — read all result files in category order
    echo ""
    _aggregate_all_results "$results_dir" "${categories[@]}"

    # Check aggregate Python coverage after all parallel workers complete
    if [[ "$RUNNER_WITH_COVERAGE" == "true" ]] && is_coverage_enabled; then
        # shellcheck disable=SC2086  # Intentional: glob expansion for coverage files
        local -a cov_files=()
        for f in "$results_dir"/.coverage.*; do
            [[ -f "$f" ]] && cov_files+=("$f")
        done
        if [[ ${#cov_files[@]} -gt 0 ]]; then
            local fail_under
            fail_under=$(get_python_coverage_threshold)
            # Combine per-worker coverage data into single report
            # shellcheck disable=SC2031  # Intentional: separate from subshell workers above
            export COVERAGE_FILE="${results_dir}/.coverage"
            $PYTHON -m coverage combine "${cov_files[@]}" 2>/dev/null || true
            echo ""
            log_section "Aggregate Python Coverage"
            local cov_output
            if cov_output=$($PYTHON -m coverage report --fail-under="$fail_under" 2>&1); then
                echo "$cov_output" | tail -20
            else
                echo "$cov_output" | tail -20
                RUNNER_COVERAGE_FAILED="true"
                log_error "Coverage below ${fail_under}% threshold"
            fi
        else
            log_warn "No coverage data collected (no Python tests ran or pytest-cov not installed)"
        fi
    fi

    local end_time
    end_time=$(date +%s)
    local duration=$((end_time - RUNNER_START_TIME))

    print_runner_summary "$duration"

    # Cleanup handled by trap
    [[ ${#FAILED_TESTS[@]} -eq 0 && "$RUNNER_COVERAGE_FAILED" != "true" ]]
}

# ============================================================================
# INTERNAL HELPERS
# ============================================================================

_process_category_result() {
    local category="$1" results_dir="$2" stop_file="$3"
    local result_file="$results_dir/${category}.result"

    if [[ ! -f "$result_file" ]]; then
        return
    fi

    local exit_code=0 failed=0
    exit_code=$(grep '^exit_code=' "$result_file" | cut -d= -f2)
    failed=$(grep '^failed=' "$result_file" | cut -d= -f2)

    if [[ "$exit_code" -ne 0 ]] || [[ "$failed" -gt 0 ]]; then
        if [[ "$RUNNER_STOP_ON_FAIL" == "true" ]]; then
            touch "$stop_file"
        fi
    fi
}

_print_progress() {
    local completed="$1" total="$2" category="$3" results_dir="$4"
    local result_file="$results_dir/${category}.result"

    if [[ -f "$result_file" ]]; then
        local passed=0 failed=0 duration=0
        passed=$(grep '^passed=' "$result_file" | cut -d= -f2)
        failed=$(grep '^failed=' "$result_file" | cut -d= -f2)
        duration=$(grep '^duration=' "$result_file" | cut -d= -f2)

        local status="${GREEN}passed${NC}"
        [[ "$failed" -gt 0 ]] && status="${RED}${failed} failed${NC}"
        echo -e "  [${completed}/${total}] ${category}: ${passed} passed, ${status} (${duration}s)"
    fi
}

_aggregate_all_results() {
    local results_dir="$1"
    shift
    local -a categories=("$@")

    for category in "${categories[@]}"; do
        local result_file="$results_dir/${category}.result"
        [[ -f "$result_file" ]] || continue

        local passed=0 failed=0 skipped=0 failed_list=""
        passed=$(grep '^passed=' "$result_file" | cut -d= -f2)
        failed=$(grep '^failed=' "$result_file" | cut -d= -f2)
        skipped=$(grep '^skipped=' "$result_file" | cut -d= -f2)
        failed_list=$(grep '^failed_list=' "$result_file" | cut -d= -f2-)

        # Accumulate passed/skipped into global arrays
        for ((i=0; i<passed; i++)); do
            PASSED_TESTS+=("${category}-pass-${i}")
        done
        for ((i=0; i<skipped; i++)); do
            SKIPPED_TESTS+=("${category}-skip-${i}")
        done

        # Accumulate failures: use actual names when available, synthetic otherwise
        if [[ -n "$failed_list" ]]; then
            IFS=',' read -ra failed_names <<< "$failed_list"
            for name in "${failed_names[@]}"; do
                [[ -n "$name" ]] && FAILED_TESTS+=("$name")
            done
        else
            for ((i=0; i<failed; i++)); do
                FAILED_TESTS+=("${category}-fail-${i}")
            done
        fi
    done
}
