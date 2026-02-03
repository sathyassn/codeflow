#!/usr/bin/env bash
# CodeFlow Test Framework: Reporting
# Location: .codeflow/testing/lib/test-reporting.sh

# Requires: test-common.sh
REPORTING_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$REPORTING_DIR/test-common.sh"

# ============================================================================
# REPORT PATHS
# ============================================================================

get_report_dir() {
    local repo_root
    repo_root=$(get_repo_root)
    echo "$repo_root/.state/test-reports"
}

ensure_report_dir() {
    local report_dir
    report_dir=$(get_report_dir)
    mkdir -p "$report_dir"
    echo "$report_dir"
}

# ============================================================================
# JSON REPORT
# ============================================================================

generate_json_report() {
    local passed_count="$1"
    local failed_count="$2"
    local skipped_count="$3"
    local duration="$4"
    local mode="$5"
    shift 5
    local failed_tests=("$@")

    local report_dir
    report_dir=$(ensure_report_dir)
    local timestamp
    timestamp=$(date +%Y%m%d_%H%M%S)
    local report_file="$report_dir/test-report-${timestamp}.json"

    local failed_json="[]"
    if [[ ${#failed_tests[@]} -gt 0 ]]; then
        failed_json=$(printf '%s\n' "${failed_tests[@]}" | jq -R . | jq -s .)
    fi

    cat > "$report_file" <<EOF
{
    "timestamp": "$(get_timestamp)",
    "mode": "$mode",
    "duration_seconds": $duration,
    "summary": {
        "passed": $passed_count,
        "failed": $failed_count,
        "skipped": $skipped_count,
        "total": $((passed_count + failed_count + skipped_count))
    },
    "failed_tests": $failed_json,
    "success": $([ "$failed_count" -eq 0 ] && echo "true" || echo "false")
}
EOF

    echo "$report_file"
}

# ============================================================================
# TEXT REPORT
# ============================================================================

generate_text_report() {
    local passed_count="$1"
    local failed_count="$2"
    local skipped_count="$3"
    local duration="$4"
    local mode="$5"
    shift 5
    local failed_tests=("$@")

    local report_dir
    report_dir=$(ensure_report_dir)
    local timestamp
    timestamp=$(date +%Y%m%d_%H%M%S)
    local report_file="$report_dir/test-report-${timestamp}.txt"

    {
        echo "CodeFlow Test Report"
        echo "===================="
        echo ""
        echo "Timestamp: $(get_timestamp)"
        echo "Mode: $mode"
        echo "Duration: ${duration}s"
        echo ""
        echo "Summary"
        echo "-------"
        echo "Passed:  $passed_count"
        echo "Failed:  $failed_count"
        echo "Skipped: $skipped_count"
        echo "Total:   $((passed_count + failed_count + skipped_count))"
        echo ""

        if [[ ${#failed_tests[@]} -gt 0 ]]; then
            echo "Failed Tests"
            echo "------------"
            for test in "${failed_tests[@]}"; do
                echo "  - $test"
            done
            echo ""
        fi

        if [[ $failed_count -eq 0 ]]; then
            echo "Result: PASSED"
        else
            echo "Result: FAILED"
        fi
    } > "$report_file"

    echo "$report_file"
}

# ============================================================================
# CONSOLE REPORT
# ============================================================================

print_category_summary() {
    local category="$1"
    local passed="$2"
    local failed="$3"
    local skipped="$4"

    local total=$((passed + failed + skipped))
    local status="${GREEN}✓${NC}"
    [[ $failed -gt 0 ]] && status="${RED}✗${NC}"

    printf "  %-25s %s %3d passed, %3d failed, %3d skipped\n" \
        "$category" "$status" "$passed" "$failed" "$skipped"
}

print_mode_info() {
    local mode="$1"

    echo ""
    echo -e "${BOLD}Test Mode: $mode${NC}"
    echo ""

    case "$mode" in
        essential)
            echo "  Running: CRITICAL priority tests only"
            echo "  Purpose: Security enforcement validation"
            ;;
        standard)
            echo "  Running: CRITICAL + HIGH priority tests"
            echo "  Purpose: Security + workflow enforcement"
            ;;
        full)
            echo "  Running: All priority levels"
            echo "  Purpose: Comprehensive validation"
            ;;
    esac

    echo ""
}

# ============================================================================
# CLEANUP
# ============================================================================

cleanup_old_reports() {
    local max_age_days="${1:-30}"
    local report_dir
    report_dir=$(get_report_dir)

    if [[ -d "$report_dir" ]]; then
        find "$report_dir" -name "test-report-*" -mtime "+$max_age_days" -delete 2>/dev/null
        log_info "Cleaned up test reports older than $max_age_days days"
    fi
}
