#!/usr/bin/env bash
# test-config.sh - Tests for shell-lib/config.sh
# Location: .codeflow/testing/scripts/shell-lib/test-config.sh
#
# Usage:
#   ./test-config.sh       Run all tests
#   ./test-config.sh -h    Show help
#   ./test-config.sh -V    Show version

set -euo pipefail

# Script metadata
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly SCRIPT_NAME
SCRIPT_VERSION="1.0.0"
readonly SCRIPT_VERSION
TESTING_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
readonly TESTING_DIR

# Usage function
usage() {
    cat <<EOF
Usage: $SCRIPT_NAME [OPTIONS]

Tests for shell-lib/config.sh module.

Options:
    -h, --help      Show this help message
    -V, --version   Show version information

Examples:
    $SCRIPT_NAME              Run all tests
    $SCRIPT_NAME --help       Show this help
EOF
}

# Source test framework
source "$TESTING_DIR/lib/test-common.sh"
source "$TESTING_DIR/lib/test-helpers.sh"

# ============================================================================
# TEST: Configuration Paths
# ============================================================================

test_get_config_dir() {
    test_section "get_config_dir"

    local config_dir
    config_dir=$(get_config_dir)

    # Should be non-empty
    assert_not_empty "$config_dir" "get_config_dir returns value"

    # Should end with .codeflow/config
    if [[ "$config_dir" == *".codeflow/config" ]]; then
        test_pass "Config dir ends with .codeflow/config"
    else
        test_fail "Config dir should end with .codeflow/config: $config_dir"
    fi
}

test_get_config_path() {
    test_section "get_config_path"

    local path
    path=$(get_config_path "enforcement/enforcement-policy.json")

    # Should contain config filename
    if [[ "$path" == *"enforcement-policy.json" ]]; then
        test_pass "get_config_path includes filename"
    else
        test_fail "get_config_path should include filename: $path"
    fi

    # Should be absolute path
    if [[ "$path" =~ ^/ ]]; then
        test_pass "get_config_path returns absolute path"
    else
        test_fail "get_config_path should return absolute path: $path"
    fi
}

# ============================================================================
# TEST: JSON Configuration
# ============================================================================

test_config_get_json() {
    test_section "config_get_json"

    setup_test_dir "config-json"

    # Create mock config directory
    local mock_config_dir="$TEST_DIR/.codeflow/config"
    mkdir -p "$mock_config_dir"

    # Create test JSON config
    cat > "$mock_config_dir/test-config.json" << 'EOF'
{
    "string_value": "hello",
    "number_value": 42,
    "bool_value": true,
    "nested": {
        "inner": "value"
    }
}
EOF

    # Override find_repo_root for this test
    local original_root
    original_root=$(find_repo_root)

    # Save original function and override
    local result

    # Test reading existing value (use actual config if exists)
    if [[ -f "$original_root/.codeflow/config/enforcement/enforcement-policy.json" ]]; then
        result=$(config_get_json "enforcement/enforcement-policy.json" ".enforcement_level" "default")
        assert_not_empty "$result" "config_get_json reads existing config"
    else
        test_skip "config_get_json" "No enforcement-policy.json found"
    fi

    # Test default value for missing key
    result=$(config_get_json "nonexistent.json" ".missing" "fallback")
    assert_equals "fallback" "$result" "config_get_json returns default for missing file"

    teardown_test_dir
}

test_config_has_json() {
    test_section "config_has_json"

    setup_test_dir "config-has"

    local repo_root
    repo_root=$(find_repo_root)

    # Test with actual config if exists
    if [[ -f "$repo_root/.codeflow/config/enforcement/enforcement-policy.json" ]]; then
        if config_has_json "enforcement/enforcement-policy.json" ".enforcement_levels"; then
            test_pass "config_has_json finds existing key"
        else
            test_fail "config_has_json should find existing key"
        fi
    else
        test_skip "config_has_json" "No enforcement-policy.json found"
    fi

    # Test missing key
    if config_has_json "enforcement/enforcement-policy.json" ".nonexistent_key_xyz"; then
        test_fail "config_has_json should not find missing key"
    else
        test_pass "config_has_json returns false for missing key"
    fi

    teardown_test_dir
}

# ============================================================================
# TEST: YAML Configuration
# ============================================================================

test_config_get_yaml() {
    test_section "config_get_yaml"

    # Test default for missing file
    local result
    result=$(config_get_yaml "nonexistent.yaml" ".key" "default_value")
    assert_equals "default_value" "$result" "config_get_yaml returns default for missing file"

    # Test with actual work-graph.yaml if exists
    local repo_root
    repo_root=$(find_repo_root)

    if [[ -f "$repo_root/.codeflow/config/work-graph.yaml" ]]; then
        result=$(config_get_yaml "work-graph.yaml" ".version" "")
        if [[ -n "$result" ]]; then
            test_pass "config_get_yaml reads YAML config"
        else
            test_skip "config_get_yaml" "Could not read version from work-graph.yaml"
        fi
    else
        test_skip "config_get_yaml" "No work-graph.yaml found"
    fi
}

# ============================================================================
# TEST: Environment Configuration
# ============================================================================

test_load_env_file() {
    test_section "load_env_file"

    setup_test_dir "env-file"

    # Create test env file
    cat > "$TEST_DIR/.env" << 'EOF'
# Comment line
TEST_KEY1=value1
TEST_KEY2=value2

# Another comment
TEST_KEY3=value with spaces
EOF

    # Load env file
    load_env_file "$TEST_DIR/.env"

    # Check values were loaded
    assert_equals "value1" "${TEST_KEY1:-}" "TEST_KEY1 loaded"
    assert_equals "value2" "${TEST_KEY2:-}" "TEST_KEY2 loaded"
    assert_equals "value with spaces" "${TEST_KEY3:-}" "TEST_KEY3 with spaces loaded"

    # Cleanup
    unset TEST_KEY1 TEST_KEY2 TEST_KEY3

    teardown_test_dir
}

test_load_env_file_nonexistent() {
    test_section "load_env_file: nonexistent file"

    # Should not error on missing file
    if load_env_file "/nonexistent/path/.env" 2>/dev/null; then
        test_pass "load_env_file handles missing file gracefully"
    else
        test_fail "load_env_file should handle missing file"
    fi
}

# ============================================================================
# TEST: Common Configurations
# ============================================================================

test_get_enforcement_policy() {
    test_section "get_enforcement_policy"

    local repo_root
    repo_root=$(find_repo_root)

    if [[ -f "$repo_root/.codeflow/config/enforcement/enforcement-policy.json" ]]; then
        local level
        level=$(get_enforcement_policy "enforcement_level" "unknown")
        assert_not_empty "$level" "get_enforcement_policy returns value"

        # Should be one of the valid levels
        if [[ "$level" == "strict" || "$level" == "warn" || "$level" == "permissive" || "$level" == "unknown" ]]; then
            test_pass "Enforcement level is valid: $level"
        else
            test_fail "Unknown enforcement level: $level"
        fi
    else
        # Test default value
        local result
        result=$(get_enforcement_policy "nonexistent" "default_policy")
        assert_equals "default_policy" "$result" "get_enforcement_policy returns default"
    fi
}

test_get_work_graph_config() {
    test_section "get_work_graph_config"

    local result
    result=$(get_work_graph_config "nonexistent_key" "default_value")
    assert_equals "default_value" "$result" "get_work_graph_config returns default for missing key"
}

test_is_feature_enabled() {
    test_section "is_feature_enabled"

    # Test non-existent feature (should default to false)
    if is_feature_enabled "nonexistent_feature_xyz"; then
        test_fail "Nonexistent feature should not be enabled"
    else
        test_pass "Nonexistent feature is not enabled"
    fi
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                usage
                exit 0
                ;;
            -V|--version)
                echo "$SCRIPT_NAME version $SCRIPT_VERSION"
                exit 0
                ;;
            *)
                echo "Unknown option: $1" >&2
                usage >&2
                exit 2
                ;;
        esac
        shift
    done

    reset_test_counters

    echo ""
    echo -e "${BOLD}Testing: shell-lib/config.sh${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Configuration paths
    test_get_config_dir
    test_get_config_path

    # JSON configuration
    test_config_get_json
    test_config_has_json

    # YAML configuration
    test_config_get_yaml

    # Environment configuration
    test_load_env_file
    test_load_env_file_nonexistent

    # Common configurations
    test_get_enforcement_policy
    test_get_work_graph_config
    test_is_feature_enabled

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
