#!/usr/bin/env bash
# CodeFlow Test Framework: Test Discovery
# Location: .codeflow/testing/lib/test-discovery.sh

# Requires: test-common.sh
DISCOVERY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$DISCOVERY_DIR/test-common.sh"

# ============================================================================
# TEST DISCOVERY
# ============================================================================

discover_tests() {
    local category="${1:-}"
    local testing_root="${DISCOVERY_DIR}/.."

    if [[ -n "$category" ]]; then
        local category_dir
        category_dir=$(get_category_dir "$category")
        if [[ -n "$category_dir" ]]; then
            discover_tests_in_dir "$testing_root/$category_dir"
        fi
    else
        # Discover all tests
        for cat in "${!CATEGORIES[@]}"; do
            discover_tests_in_dir "$testing_root/${CATEGORIES[$cat]}"
        done
    fi
}

discover_tests_in_dir() {
    local dir="$1"

    [[ -d "$dir" ]] || return 0

    # Find bash tests
    find "$dir" -maxdepth 1 -name "test-*.sh" -type f 2>/dev/null | sort

    # Find Python tests
    find "$dir" -maxdepth 1 -name "test_*.py" -type f 2>/dev/null | sort
}

# ============================================================================
# PRIORITY EXTRACTION
# ============================================================================

get_test_priority() {
    local test_file="$1"
    local priority="MEDIUM"  # Default

    if [[ -f "$test_file" ]]; then
        # Look for priority comment
        local found
        found=$(grep -m1 "^# Priority:" "$test_file" 2>/dev/null | sed 's/^# Priority:[[:space:]]*//')
        if [[ -n "$found" ]]; then
            priority="$found"
        fi
    fi

    echo "$priority"
}

# ============================================================================
# TEST FILTERING
# ============================================================================

filter_tests_by_priority() {
    local mode="$1"
    shift
    local tests=("$@")
    local priorities
    priorities=$(get_mode_priorities "$mode")

    for test_file in "${tests[@]}"; do
        local test_priority
        test_priority=$(get_test_priority "$test_file")
        if [[ " $priorities " == *" $test_priority "* ]]; then
            echo "$test_file"
        fi
    done
}

# ============================================================================
# CATEGORY DETECTION
# ============================================================================

get_test_category() {
    local test_file="$1"
    local testing_root="${DISCOVERY_DIR}/.."
    local rel_path="${test_file#$testing_root/}"

    for cat in "${!CATEGORIES[@]}"; do
        if [[ "$rel_path" == "${CATEGORIES[$cat]}/"* ]]; then
            echo "$cat"
            return 0
        fi
    done

    echo "unknown"
}

is_python_test() {
    local test_file="$1"
    [[ "$test_file" == *.py ]]
}

is_bash_test() {
    local test_file="$1"
    [[ "$test_file" == *.sh ]]
}

# ============================================================================
# TEST COUNTING
# ============================================================================

count_tests() {
    local category="${1:-}"
    local count=0

    while IFS= read -r test_file; do
        ((count++))
    done < <(discover_tests "$category")

    echo "$count"
}

list_categories() {
    for cat in "${!CATEGORIES[@]}"; do
        echo "$cat"
    done | sort
}
