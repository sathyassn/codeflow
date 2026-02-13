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
SCRIPT_VERSION="1.1.0"
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
# HELPERS: Mock repo root override
# ============================================================================

# Override find_repo_root to point to a test directory for isolated tests.
# Call restore_repo_root after the test to restore original behavior.
_REAL_REPO_ROOT=""
_MOCK_REPO_ROOT=""

override_repo_root() {
    _MOCK_REPO_ROOT="$1"
    # shellcheck disable=SC2218  # find_repo_root defined in sourced common.sh
    _REAL_REPO_ROOT=$(find_repo_root)
    # shellcheck disable=SC2317  # Function called indirectly by config functions
    find_repo_root() { echo "$_MOCK_REPO_ROOT"; }
    export -f find_repo_root
}

restore_repo_root() {
    local saved="$_REAL_REPO_ROOT"
    # shellcheck disable=SC2317  # Function called indirectly by config functions
    find_repo_root() { git rev-parse --show-toplevel 2>/dev/null || pwd; }
    export -f find_repo_root
    _REAL_REPO_ROOT=""
    _MOCK_REPO_ROOT=""
    # Also reset CODEFLOW_ROOT to match
    export CODEFLOW_ROOT="${saved}"
}

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

    # Should contain .codeflow/config in the middle
    if [[ "$path" == *"/.codeflow/config/"* ]]; then
        test_pass "get_config_path includes config directory"
    else
        test_fail "get_config_path should include .codeflow/config/: $path"
    fi
}

# ============================================================================
# TEST: JSON Configuration
# ============================================================================

test_config_get_json() {
    test_section "config_get_json"

    setup_test_dir "config-json"

    # Create mock config structure
    mkdir -p "$TEST_DIR/.codeflow/config"

    cat > "$TEST_DIR/.codeflow/config/test-config.json" << 'EOF'
{
    "string_value": "hello",
    "number_value": 42,
    "bool_value": true,
    "nested": {
        "inner": "deep_value"
    }
}
EOF

    override_repo_root "$TEST_DIR"

    local result

    # Test reading string value
    result=$(config_get_json "test-config.json" ".string_value" "")
    assert_equals "hello" "$result" "config_get_json reads string value"

    # Test reading number value
    result=$(config_get_json "test-config.json" ".number_value" "")
    assert_equals "42" "$result" "config_get_json reads number value"

    # Test reading boolean value
    result=$(config_get_json "test-config.json" ".bool_value" "")
    assert_equals "true" "$result" "config_get_json reads boolean value"

    # Test reading nested value
    result=$(config_get_json "test-config.json" ".nested.inner" "")
    assert_equals "deep_value" "$result" "config_get_json reads nested value"

    # Test default value for missing key
    result=$(config_get_json "test-config.json" ".missing_key" "fallback")
    assert_equals "fallback" "$result" "config_get_json returns default for missing key"

    # Test default value for missing file
    result=$(config_get_json "nonexistent.json" ".key" "file_fallback")
    assert_equals "file_fallback" "$result" "config_get_json returns default for missing file"

    restore_repo_root
    teardown_test_dir
}

test_config_get_json_subdirectory() {
    test_section "config_get_json: subdirectory path"

    setup_test_dir "config-json-subdir"

    # Create mock config structure with subdirectory (like enforcement/)
    mkdir -p "$TEST_DIR/.codeflow/config/enforcement"

    cat > "$TEST_DIR/.codeflow/config/enforcement/policy.json" << 'EOF'
{
    "level": "strict",
    "enabled": true
}
EOF

    override_repo_root "$TEST_DIR"

    local result
    result=$(config_get_json "enforcement/policy.json" ".level" "default")
    assert_equals "strict" "$result" "config_get_json reads from subdirectory"

    restore_repo_root
    teardown_test_dir
}

test_config_has_json() {
    test_section "config_has_json"

    setup_test_dir "config-has"

    # Create mock config
    mkdir -p "$TEST_DIR/.codeflow/config"

    cat > "$TEST_DIR/.codeflow/config/has-test.json" << 'EOF'
{
    "existing_key": "value",
    "nested": {
        "deep_key": true
    }
}
EOF

    override_repo_root "$TEST_DIR"

    # Test existing top-level key
    if config_has_json "has-test.json" ".existing_key"; then
        test_pass "config_has_json finds existing key"
    else
        test_fail "config_has_json should find existing key"
    fi

    # Test existing nested key
    if config_has_json "has-test.json" ".nested.deep_key"; then
        test_pass "config_has_json finds nested key"
    else
        test_fail "config_has_json should find nested key"
    fi

    # Test missing key
    if config_has_json "has-test.json" ".nonexistent_key"; then
        test_fail "config_has_json should not find missing key"
    else
        test_pass "config_has_json returns false for missing key"
    fi

    # Test missing file
    if config_has_json "nonexistent.json" ".any_key"; then
        test_fail "config_has_json should not find key in missing file"
    else
        test_pass "config_has_json returns false for missing file"
    fi

    restore_repo_root
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
}

test_config_get_yaml_with_file() {
    test_section "config_get_yaml: with file"

    setup_test_dir "config-yaml"

    mkdir -p "$TEST_DIR/.codeflow/config"

    cat > "$TEST_DIR/.codeflow/config/test-config.yaml" << 'EOF'
version: "1.0"
settings:
  name: test-project
  enabled: true
EOF

    override_repo_root "$TEST_DIR"

    local result

    # Test reading top-level value (yq or python3 fallback)
    result=$(config_get_yaml "test-config.yaml" ".version" "unknown")
    if [[ "$result" == "1.0" ]]; then
        test_pass "config_get_yaml reads top-level YAML value"
    elif [[ "$result" == "unknown" ]]; then
        test_skip "config_get_yaml" "Neither yq nor python3+yaml available"
    else
        test_fail "config_get_yaml unexpected value: $result (expected 1.0)"
    fi

    # Test reading nested value
    result=$(config_get_yaml "test-config.yaml" ".settings.name" "unknown")
    if [[ "$result" == "test-project" ]]; then
        test_pass "config_get_yaml reads nested YAML value"
    elif [[ "$result" == "unknown" ]]; then
        test_skip "config_get_yaml nested" "Neither yq nor python3+yaml available"
    else
        test_fail "config_get_yaml unexpected nested value: $result (expected test-project)"
    fi

    # Test default for missing key
    result=$(config_get_yaml "test-config.yaml" ".nonexistent" "yaml_fallback")
    assert_equals "yaml_fallback" "$result" "config_get_yaml returns default for missing key"

    restore_repo_root
    teardown_test_dir
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

test_load_env_file_with_equals_in_value() {
    test_section "load_env_file: values with equals sign"

    setup_test_dir "env-equals"

    cat > "$TEST_DIR/.env" << 'EOF'
CONNECTION_STRING=host=localhost;port=5432
EOF

    load_env_file "$TEST_DIR/.env"

    assert_equals "host=localhost;port=5432" "${CONNECTION_STRING:-}" "Value with equals sign preserved"

    unset CONNECTION_STRING
    teardown_test_dir
}

# ============================================================================
# TEST: Common Configurations
# ============================================================================

test_get_enforcement_policy() {
    test_section "get_enforcement_policy"

    setup_test_dir "enforcement"

    mkdir -p "$TEST_DIR/.codeflow/config/enforcement"

    cat > "$TEST_DIR/.codeflow/config/enforcement/enforcement-policy.json" << 'EOF'
{
    "enforcement_level": "strict",
    "sentinel": {
        "default_ttl": 600
    }
}
EOF

    override_repo_root "$TEST_DIR"

    local level
    level=$(get_enforcement_policy "enforcement_level" "unknown")
    assert_equals "strict" "$level" "get_enforcement_policy reads enforcement level"

    # Test nested key
    local ttl
    ttl=$(get_enforcement_policy "sentinel.default_ttl" "0")
    assert_equals "600" "$ttl" "get_enforcement_policy reads nested key"

    # Test default for missing key
    local missing
    missing=$(get_enforcement_policy "nonexistent" "default_policy")
    assert_equals "default_policy" "$missing" "get_enforcement_policy returns default for missing key"

    restore_repo_root
    teardown_test_dir
}

test_get_work_graph_config() {
    test_section "get_work_graph_config"

    local result
    result=$(get_work_graph_config "nonexistent_key" "default_value")
    assert_equals "default_value" "$result" "get_work_graph_config returns default for missing key"
}

test_is_feature_enabled() {
    test_section "is_feature_enabled"

    setup_test_dir "feature-flags"

    mkdir -p "$TEST_DIR/.codeflow/config/enforcement"

    cat > "$TEST_DIR/.codeflow/config/enforcement/enforcement-policy.json" << 'EOF'
{
    "feature_alpha": "true",
    "feature_beta": "false",
    "feature_gamma": "enabled"
}
EOF

    override_repo_root "$TEST_DIR"

    # Test enabled feature
    if is_feature_enabled "feature_alpha"; then
        test_pass "is_feature_enabled returns true for 'true' value"
    else
        test_fail "is_feature_enabled should return true for feature_alpha"
    fi

    # Test disabled feature
    if is_feature_enabled "feature_beta"; then
        test_fail "is_feature_enabled should return false for 'false' value"
    else
        test_pass "is_feature_enabled returns false for 'false' value"
    fi

    # Test non-boolean feature (should be false)
    if is_feature_enabled "feature_gamma"; then
        test_fail "is_feature_enabled should return false for non-'true' value"
    else
        test_pass "is_feature_enabled returns false for non-'true' value"
    fi

    # Test nonexistent feature (should default to false)
    if is_feature_enabled "nonexistent_feature"; then
        test_fail "Nonexistent feature should not be enabled"
    else
        test_pass "Nonexistent feature is not enabled"
    fi

    restore_repo_root
    teardown_test_dir
}

# ============================================================================
# TEST: jq dependency handling
# ============================================================================

test_jq_missing_fallback() {
    test_section "jq missing fallback"

    setup_test_dir "no-jq"

    mkdir -p "$TEST_DIR/.codeflow/config"
    echo '{"key": "value"}' > "$TEST_DIR/.codeflow/config/test.json"

    override_repo_root "$TEST_DIR"

    # Temporarily override command_exists to simulate missing jq
    local _orig_command_exists
    _orig_command_exists=$(declare -f command_exists)

    # shellcheck disable=SC2317  # Function called indirectly
    command_exists() {
        [[ "$1" != "jq" ]] && command -v "$1" &>/dev/null
    }

    local result
    result=$(config_get_json "test.json" ".key" "jq_missing_default")
    assert_equals "jq_missing_default" "$result" "config_get_json returns default when jq missing"

    if config_has_json "test.json" ".key"; then
        test_fail "config_has_json should return false when jq missing"
    else
        test_pass "config_has_json returns false when jq missing"
    fi

    # Restore command_exists
    eval "$_orig_command_exists"

    restore_repo_root
    teardown_test_dir
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
    test_config_get_json_subdirectory
    test_config_has_json

    # YAML configuration
    test_config_get_yaml
    test_config_get_yaml_with_file

    # Environment configuration
    test_load_env_file
    test_load_env_file_nonexistent
    test_load_env_file_with_equals_in_value

    # Common configurations
    test_get_enforcement_policy
    test_get_work_graph_config
    test_is_feature_enabled

    # Dependency handling
    test_jq_missing_fallback

    print_test_summary

    [[ $TEST_FAIL_COUNT -eq 0 ]]
}

main "$@"
