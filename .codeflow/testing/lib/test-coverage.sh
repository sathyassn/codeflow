#!/usr/bin/env bash
# CodeFlow Test Framework: Coverage Validation
# Location: .codeflow/testing/lib/test-coverage.sh
#
# This library provides test coverage validation functions that automatically
# discover scripts and validate they have corresponding tests using naming
# conventions. Configuration only stores exceptions.
#
# Provides:
#   - discover_scripts() - Find all scripts in .codeflow/scripts/ and .claude/hooks/
#   - derive_test_path() - Derive expected test path from script path
#   - is_excepted() - Check if script is in exceptions list
#   - is_test_registered() - Check if test is in test-config.json priorities
#   - validate_coverage() - Main validation function (all scripts)
#   - validate_staged_coverage() - Validate only staged/new scripts
#   - find_orphaned_tests() - Find test files without matching scripts
#   - find_misplaced_test() - Find test files in wrong locations

# Requires: test-common.sh
COVERAGE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$COVERAGE_DIR/test-common.sh"

# Provide fallback log functions if test-common.sh not sourced
if ! declare -f log_info > /dev/null 2>&1; then
    log_info()    { echo "[INFO] $*"; }
    log_warn()    { echo "[WARN] $*" >&2; }
    log_error()   { echo "[ERROR] $*" >&2; }
    log_success() { echo "[OK] $*"; }
    log_section() { echo ""; echo "=== $* ==="; }
    # Fallback colors
    BOLD="${BOLD:-}"
    RED="${RED:-}"
    YELLOW="${YELLOW:-}"
    NC="${NC:-}"
fi
# log_verbose not in test-common.sh — always provide fallback
if ! declare -f log_verbose > /dev/null 2>&1; then
    log_verbose() { [[ "${VERBOSE:-false}" == "true" ]] && echo "[VERBOSE] $*" || true; }
fi

# Config file location (may already be set as readonly by test-config.sh)
if [[ -z "${TEST_CONFIG_FILE:-}" ]]; then
    TEST_CONFIG_FILE="$COVERAGE_DIR/../test-config.json"
fi

# ============================================================================
# SCRIPT DISCOVERY
# ============================================================================

discover_scripts() {
    local repo_root
    repo_root=$(get_repo_root)

    # Find Python scripts in .codeflow/scripts/
    find "$repo_root/.codeflow/scripts/" -type f -name "*.py" \
        ! -path "*/__pycache__/*" \
        ! -path "*/lib/__init__.py" \
        ! -name "__init__.py" \
        -print 2>/dev/null | while read -r script; do
        echo "${script#"$repo_root"/}"
    done

    # Find Bash scripts in .codeflow/scripts/ (exclude lib/)
    find "$repo_root/.codeflow/scripts/" -type f -name "*.sh" \
        ! -path "*/shell-lib/*" \
        -print 2>/dev/null | while read -r script; do
        echo "${script#"$repo_root"/}"
    done

    # Find hooks in .claude/hooks/codeflow/
    find "$repo_root/.claude/hooks/codeflow/" -type f -name "*.sh" -print 2>/dev/null | while read -r script; do
        echo "${script#"$repo_root"/}"
    done
}

discover_lib_modules() {
    local repo_root
    repo_root=$(get_repo_root)

    # Python library modules
    find "$repo_root/.codeflow/scripts/codeflow_py_lib/" -type f -name "*.py" \
        ! -name "__init__.py" \
        -print 2>/dev/null | while read -r module; do
        echo "${module#"$repo_root"/}"
    done

    # Shell library modules
    find "$repo_root/.codeflow/scripts/shell-lib/" -type f -name "*.sh" \
        ! -name "index.sh" \
        -print 2>/dev/null | while read -r module; do
        echo "${module#"$repo_root"/}"
    done
}

# ============================================================================
# TEST PATH DERIVATION
# ============================================================================

# Derive expected test path from script path using naming conventions
# Usage: derive_test_path "script_path"
# Returns: Expected test file path (relative to repo root) or empty if no convention matches
derive_test_path() {
    local script="$1"

    # Pattern: .codeflow/scripts/codeflow_py_lib/{name}.py → .codeflow/testing/scripts/codeflow_py_lib/test_{name}.py
    if [[ "$script" =~ ^\.codeflow/scripts/codeflow_py_lib/([^/]+)\.py$ ]]; then
        local name="${BASH_REMATCH[1]}"
        echo ".codeflow/testing/scripts/codeflow_py_lib/test_${name}.py"
        return
    fi

    # Pattern: .codeflow/scripts/shell-lib/{name}.sh → .codeflow/testing/scripts/shell-lib/test-{name}.sh
    if [[ "$script" =~ ^\.codeflow/scripts/shell-lib/([^/]+)\.sh$ ]]; then
        local name="${BASH_REMATCH[1]}"
        echo ".codeflow/testing/scripts/shell-lib/test-${name}.sh"
        return
    fi

    # Pattern: .codeflow/scripts/{domain}/{sub}/**/{name}.py → .codeflow/testing/scripts/{domain}/{sub}/test_{name}.py
    if [[ "$script" =~ ^\.codeflow/scripts/([^/]+)/([^/]+)/.*/([^/]+)\.py$ ]]; then
        local domain="${BASH_REMATCH[1]}"
        local subdomain="${BASH_REMATCH[2]}"
        local name="${BASH_REMATCH[3]}"
        name=$(echo "$name" | sed 's/-/_/g')
        echo ".codeflow/testing/scripts/${domain}/${subdomain}/test_${name}.py"
        return
    fi

    # Pattern: .codeflow/scripts/{domain}/{sub}/{name}.py → .codeflow/testing/scripts/{domain}/{sub}/test_{name}.py
    if [[ "$script" =~ ^\.codeflow/scripts/([^/]+)/([^/]+)/([^/]+)\.py$ ]]; then
        local domain="${BASH_REMATCH[1]}"
        local subdomain="${BASH_REMATCH[2]}"
        local name="${BASH_REMATCH[3]}"
        name=$(echo "$name" | sed 's/-/_/g')
        echo ".codeflow/testing/scripts/${domain}/${subdomain}/test_${name}.py"
        return
    fi

    # Pattern: .codeflow/scripts/{domain}/{name}.py → .codeflow/testing/scripts/{domain}/test_{name}.py
    if [[ "$script" =~ ^\.codeflow/scripts/([^/]+)/([^/]+)\.py$ ]]; then
        local domain="${BASH_REMATCH[1]}"
        local name="${BASH_REMATCH[2]}"
        name=$(echo "$name" | sed 's/-/_/g')
        echo ".codeflow/testing/scripts/${domain}/test_${name}.py"
        return
    fi

    # Pattern: .codeflow/scripts/{domain}/{sub}/**/{name}.sh → .codeflow/testing/scripts/{domain}/{sub}/test-{name}.sh
    if [[ "$script" =~ ^\.codeflow/scripts/([^/]+)/([^/]+)/.*/([^/]+)\.sh$ ]]; then
        local domain="${BASH_REMATCH[1]}"
        local subdomain="${BASH_REMATCH[2]}"
        local name="${BASH_REMATCH[3]}"
        echo ".codeflow/testing/scripts/${domain}/${subdomain}/test-${name}.sh"
        return
    fi

    # Pattern: .codeflow/scripts/{domain}/{sub}/{name}.sh → .codeflow/testing/scripts/{domain}/{sub}/test-{name}.sh
    if [[ "$script" =~ ^\.codeflow/scripts/([^/]+)/([^/]+)/([^/]+)\.sh$ ]]; then
        local domain="${BASH_REMATCH[1]}"
        local subdomain="${BASH_REMATCH[2]}"
        local name="${BASH_REMATCH[3]}"
        echo ".codeflow/testing/scripts/${domain}/${subdomain}/test-${name}.sh"
        return
    fi

    # Pattern: .codeflow/scripts/{domain}/{name}.sh → .codeflow/testing/scripts/{domain}/test-{name}.sh
    if [[ "$script" =~ ^\.codeflow/scripts/([^/]+)/([^/]+)\.sh$ ]]; then
        local domain="${BASH_REMATCH[1]}"
        local name="${BASH_REMATCH[2]}"
        echo ".codeflow/testing/scripts/${domain}/test-${name}.sh"
        return
    fi

    # Pattern: .claude/hooks/codeflow/{category}/{name}.sh → .codeflow/testing/claude-hooks/{category}/test-{name}.sh
    if [[ "$script" =~ ^\.claude/hooks/codeflow/([^/]+)/([^/]+)\.sh$ ]]; then
        local category="${BASH_REMATCH[1]}"
        local name="${BASH_REMATCH[2]}"
        # Convert category to lowercase kebab-case
        category=$(echo "$category" | tr '[:upper:]' '[:lower:]' | sed 's/\([A-Z]\)/-\L\1/g' | sed 's/^-//')
        echo ".codeflow/testing/claude-hooks/${category}/test-${name}.sh"
        return
    fi

    # No convention matches
    echo ""
}

# ============================================================================
# MISPLACED TEST DETECTION
# ============================================================================

# Global cache for test file paths (populated once, reused as a file)
_TEST_CACHE_FILE=""

# Populate the test file cache once
_populate_test_cache() {
    if [[ -n "$_TEST_CACHE_FILE" ]] && [[ -f "$_TEST_CACHE_FILE" ]] && [[ -s "$_TEST_CACHE_FILE" ]]; then
        return
    fi

    local repo_root
    repo_root=$(get_repo_root)

    _TEST_CACHE_FILE=$(mktemp)
    # Note: Using process substitution to avoid subshell variable scope issues
    local test_file
    while IFS= read -r test_file; do
        [[ -z "$test_file" ]] && continue
        local relative="${test_file#"$repo_root"/}"
        local test_name
        test_name=$(basename "$test_file")
        echo "$test_name|$relative"
    done < <(find "$repo_root/.codeflow/testing" -type f \( -name "test-*.sh" -o -name "test_*.py" \) ! -path "*/.venv/*" ! -path "*/__pycache__/*" 2>/dev/null) > "$_TEST_CACHE_FILE"
}

# Find if a test file exists in the wrong location
# Usage: find_misplaced_test "expected_test_path"
# Returns: Actual path if found elsewhere, empty if not found
find_misplaced_test() {
    local expected="$1"
    local test_name
    test_name=$(basename "$expected")

    # Ensure cache is populated
    _populate_test_cache

    # Look up in cache file (grep instead of find)
    local actual
    actual=$(grep "^${test_name}|" "$_TEST_CACHE_FILE" 2>/dev/null | head -1 | cut -d'|' -f2) || true

    if [[ -n "$actual" ]] && [[ "$actual" != "$expected" ]]; then
        echo "$actual"
    fi
}

# ============================================================================
# CONFIG REGISTRATION CHECK
# ============================================================================

# Check if test is registered in test-config.json priorities
# Usage: is_test_registered "test_relative_path"
# Returns: 0 if registered, 1 if not
# Note: test_relative_path should be relative to .codeflow/testing/ (e.g., "claude-hooks/pre-tool-use/test-foo.sh")
is_test_registered() {
    local test_path="$1"

    if [[ ! -f "$TEST_CONFIG_FILE" ]] || ! command -v jq &>/dev/null; then
        return 1  # Assume not registered if can't check
    fi

    # Check all priority arrays for this test path
    jq -e --arg t "$test_path" '
        .priorities | to_entries | map(.value.files) | flatten | index($t) != null
    ' "$TEST_CONFIG_FILE" &>/dev/null
}

# ============================================================================
# EXCEPTION CHECKING
# ============================================================================

# Check if script is in exceptions list (no test required or integration tested)
# Usage: is_excepted "script_path"
# Returns: 0 if excepted, 1 if not
is_excepted() {
    local script="$1"

    if [[ ! -f "$TEST_CONFIG_FILE" ]] || ! command -v jq &>/dev/null; then
        return 1  # Not excepted if can't read config
    fi

    # Check no_test_required exceptions
    local no_test_patterns
    no_test_patterns=$(jq -r '.coverage_enforcement.exceptions.no_test_required[]?.pattern // empty' "$TEST_CONFIG_FILE" 2>/dev/null)
    if [[ -n "$no_test_patterns" ]]; then
        while IFS= read -r pattern; do
            [[ -z "$pattern" ]] && continue
            if _pattern_matches "$script" "$pattern"; then
                return 0
            fi
        done <<< "$no_test_patterns"
    fi

    # Check integration_tested exceptions
    local integration_patterns
    integration_patterns=$(jq -r '.coverage_enforcement.exceptions.integration_tested[]?.pattern // empty' "$TEST_CONFIG_FILE" 2>/dev/null)
    if [[ -n "$integration_patterns" ]]; then
        while IFS= read -r pattern; do
            [[ -z "$pattern" ]] && continue
            if _pattern_matches "$script" "$pattern"; then
                return 0
            fi
        done <<< "$integration_patterns"
    fi

    return 1  # Not excepted
}

# Match a script path against an exception pattern
# Handles both flat globs (dir/*) and recursive matching (dir/* also matches dir/sub/file)
# Usage: _pattern_matches "script_path" "pattern"
_pattern_matches() {
    local script="$1"
    local pattern="$2"

    # Try direct glob match first (handles exact paths and single-level globs)
    # shellcheck disable=SC2053
    if [[ "$script" == $pattern ]]; then
        return 0
    fi

    # For patterns ending in /*, also match recursively (any depth under that directory)
    if [[ "$pattern" == *'/*' ]]; then
        local dir_prefix="${pattern%/\*}"
        if [[ "$script" == "$dir_prefix/"* ]]; then
            return 0
        fi
    fi

    return 1
}

# Check if script is in no_test_required exceptions only
# (Does NOT check integration_tested — those scripts have tests)
# Usage: _is_no_test_required "script_path"
# Returns: 0 if no test required, 1 otherwise
_is_no_test_required() {
    local script="$1"

    if [[ ! -f "$TEST_CONFIG_FILE" ]] || ! command -v jq &>/dev/null; then
        return 1
    fi

    local no_test_patterns
    no_test_patterns=$(jq -r '.coverage_enforcement.exceptions.no_test_required[]?.pattern // empty' "$TEST_CONFIG_FILE" 2>/dev/null)
    if [[ -n "$no_test_patterns" ]]; then
        while IFS= read -r pattern; do
            [[ -z "$pattern" ]] && continue
            if _pattern_matches "$script" "$pattern"; then
                return 0
            fi
        done <<< "$no_test_patterns"
    fi

    return 1
}

# Get exception reason for a script
# Usage: get_exception_reason "script_path"
get_exception_reason() {
    local script="$1"

    if [[ ! -f "$TEST_CONFIG_FILE" ]] || ! command -v jq &>/dev/null; then
        echo "Config unavailable"
        return
    fi

    # Check no_test_required (iterate patterns and use glob matching)
    local pattern reason
    while IFS='|' read -r pattern reason; do
        [[ -z "$pattern" ]] && continue
        if _pattern_matches "$script" "$pattern"; then
            echo "No test required: $reason"
            return
        fi
    done < <(jq -r '.coverage_enforcement.exceptions.no_test_required[]? | "\(.pattern // "")|\(.reason // "")"' "$TEST_CONFIG_FILE" 2>/dev/null)

    # Check integration_tested (iterate patterns and use glob matching)
    local tested_by
    while IFS='|' read -r pattern tested_by; do
        [[ -z "$pattern" ]] && continue
        if _pattern_matches "$script" "$pattern"; then
            echo "Integration tested by: $tested_by"
            return
        fi
    done < <(jq -r '.coverage_enforcement.exceptions.integration_tested[]? | "\(.pattern // "")|\(.tested_by // "")"' "$TEST_CONFIG_FILE" 2>/dev/null)

    echo "Exception not found in config"
}

# Get structural coverage threshold from config
# Returns: integer percentage (default: 85)
get_structural_threshold() {
    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        local threshold
        threshold=$(jq -r '.coverage_enforcement.thresholds.fail_under // 85' "$TEST_CONFIG_FILE" 2>/dev/null)
        echo "${threshold:-85}"
    else
        echo "85"
    fi
}

# ============================================================================
# COVERAGE VALIDATION
# ============================================================================

# Validate test coverage for all discovered scripts
# Usage: validate_coverage [mode]
# Modes: fail, warn, audit (default: from config or warn)
# Returns: 0 if all covered, 1 if gaps found (when mode=fail and below threshold)
validate_coverage() {
    local mode="${1:-}"
    local repo_root
    repo_root=$(get_repo_root)

    # Read config values
    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        local enabled validation_mode
        enabled=$(jq -r '.coverage_enforcement.enabled // false' "$TEST_CONFIG_FILE" 2>/dev/null)
        validation_mode=$(jq -r '.coverage_enforcement.validation_mode // "warn"' "$TEST_CONFIG_FILE" 2>/dev/null)

        # Use config mode if not overridden
        [[ -z "$mode" ]] && mode="$validation_mode"

        # Skip if disabled
        if [[ "$enabled" != "true" ]] || [[ "$mode" == "disabled" ]]; then
            log_info "Coverage enforcement disabled in config (skipping)"
            return 0
        fi
    fi

    # Default to warn if no mode specified
    [[ -z "$mode" ]] && mode="warn"

    local covered=0 excepted=0 missing=0 misplaced=0 unregistered=0
    local missing_list=()
    local misplaced_list=()
    local unregistered_list=()
    local excepted_list=()
    local total=0

    log_section "Coverage Validation"

    # Check scripts
    while IFS= read -r script; do
        [[ -z "$script" ]] && continue
        ((total++)) || true

        local test_path
        test_path=$(derive_test_path "$script")

        if [[ -z "$test_path" ]]; then
            # No naming convention matches - skip
            log_verbose "Skipping $script (no naming convention)"
            continue
        fi

        # Check if test file exists at expected location
        if [[ -f "$repo_root/$test_path" ]]; then
            # Test exists - check if registered in config
            local config_path="${test_path#.codeflow/testing/}"
            if ! is_test_registered "$config_path"; then
                ((unregistered++)) || true
                unregistered_list+=("$script|$test_path|$config_path")
            else
                ((covered++)) || true
            fi
            continue
        fi

        # Check if script is excepted
        if is_excepted "$script"; then
            ((excepted++)) || true
            excepted_list+=("$script")
            continue
        fi

        # Test doesn't exist at expected path - check if misplaced
        local actual_path
        actual_path=$(find_misplaced_test "$test_path") || true
        if [[ -n "$actual_path" ]]; then
            ((misplaced++)) || true
            misplaced_list+=("$script|$test_path|$actual_path")
            continue
        fi

        # Coverage gap found - no test anywhere
        ((missing++)) || true
        missing_list+=("$script|$test_path")
    done < <(discover_scripts)

    # Check library modules
    while IFS= read -r module; do
        [[ -z "$module" ]] && continue
        ((total++)) || true

        local test_path
        test_path=$(derive_test_path "$module")

        if [[ -z "$test_path" ]]; then
            continue
        fi

        if [[ -f "$repo_root/$test_path" ]]; then
            local config_path="${test_path#.codeflow/testing/}"
            if ! is_test_registered "$config_path"; then
                ((unregistered++)) || true
                unregistered_list+=("$module|$test_path|$config_path")
            else
                ((covered++)) || true
            fi
            continue
        fi

        if is_excepted "$module"; then
            ((excepted++)) || true
            excepted_list+=("$module")
            continue
        fi

        local actual_path
        actual_path=$(find_misplaced_test "$test_path") || true
        if [[ -n "$actual_path" ]]; then
            ((misplaced++)) || true
            misplaced_list+=("$module|$test_path|$actual_path")
            continue
        fi

        ((missing++)) || true
        missing_list+=("$module|$test_path")
    done < <(discover_lib_modules)

    # Generate report
    # Calculate coverage percentage (covered + excepted count as "covered" for threshold)
    local effective_covered=$((covered + excepted))
    local coverage_pct=0
    if [[ $total -gt 0 ]]; then
        coverage_pct=$(( (effective_covered * 100) / total ))
    fi
    local threshold
    threshold=$(get_structural_threshold)

    echo ""
    echo -e "${BOLD}TEST COVERAGE REPORT${NC}"
    echo ""
    echo "  Scripts discovered: $total"
    echo "  Scripts with tests: $covered"
    echo "  Scripts excepted:   $excepted"
    echo "  Tests misplaced:    $misplaced"
    echo "  Tests unregistered: $unregistered"
    echo "  Coverage gaps:      $missing"
    echo "  Coverage:           ${coverage_pct}% (threshold: ${threshold}%)"
    echo ""

    # Show misplaced tests
    if [[ ${#misplaced_list[@]} -gt 0 ]]; then
        echo -e "${RED}MISPLACED TESTS (wrong location):${NC}"
        for entry in "${misplaced_list[@]}"; do
            IFS='|' read -r script_name expected_path actual_path <<< "$entry"
            echo "  $script_name"
            echo "     Expected: $expected_path"
            echo "     Found at: $actual_path ${RED}(WRONG LOCATION)${NC}"
            echo "     Action: mv $repo_root/$actual_path $repo_root/$expected_path"
        done
        echo ""
    fi

    # Show unregistered tests
    if [[ ${#unregistered_list[@]} -gt 0 ]]; then
        echo -e "${RED}UNREGISTERED TESTS (not in test-config.json):${NC}"
        for entry in "${unregistered_list[@]}"; do
            IFS='|' read -r script_name test_path config_path <<< "$entry"
            echo "  $script_name"
            echo "     Test file: $test_path"
            echo "     ${RED}NOT REGISTERED in test-config.json${NC}"
            echo "     Action: Add \"$config_path\" to .codeflow/testing/test-config.json priorities"
        done
        echo ""
    fi

    # Show missing tests
    if [[ ${#missing_list[@]} -gt 0 ]]; then
        echo -e "${RED}COVERAGE GAPS (missing tests):${NC}"
        for entry in "${missing_list[@]}"; do
            local script_name="${entry%%|*}"
            local test_path="${entry##*|}"
            echo "  $script_name"
            echo "     Expected: $test_path"
            echo "     Action: Create test file or add to exceptions"
        done
        echo ""
    fi

    # Show excepted scripts (verbose only)
    if [[ "${VERBOSE:-false}" == "true" ]] && [[ ${#excepted_list[@]} -gt 0 ]]; then
        echo -e "${YELLOW}EXCEPTIONS:${NC}"
        for script in "${excepted_list[@]}"; do
            local reason
            reason=$(get_exception_reason "$script")
            echo "  $script"
            echo "     $reason"
        done
        echo ""
    fi

    # Calculate total issues
    local total_issues=$((missing + misplaced + unregistered))

    # Return result based on mode
    if [[ $total_issues -gt 0 ]]; then
        local issue_details=""
        [[ $missing -gt 0 ]] && issue_details+="$missing missing"
        [[ $misplaced -gt 0 ]] && {
            [[ -n "$issue_details" ]] && issue_details+=", "
            issue_details+="$misplaced misplaced"
        }
        [[ $unregistered -gt 0 ]] && {
            [[ -n "$issue_details" ]] && issue_details+=", "
            issue_details+="$unregistered unregistered"
        }

        case "$mode" in
            audit)
                log_info "Coverage: ${coverage_pct}% ($issue_details) [audit mode]"
                return 0
                ;;
            warn)
                log_warn "Coverage: ${coverage_pct}% ($issue_details)"
                return 0
                ;;
            fail)
                # Misplaced and unregistered are always failures (structural issues)
                if [[ $misplaced -gt 0 ]] || [[ $unregistered -gt 0 ]]; then
                    echo "" >&2
                    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}" >&2
                    echo -e "${RED}❌ COVERAGE VALIDATION FAILED${NC}" >&2
                    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}" >&2
                    echo "" >&2
                    [[ $misplaced -gt 0 ]] && echo "MISPLACED: Test file(s) in wrong location. Move to expected path." >&2
                    [[ $unregistered -gt 0 ]] && echo "UNREGISTERED: Test file(s) not in test-config.json. Add to priorities." >&2
                    echo "" >&2
                    log_error "Coverage validation failed: $issue_details"
                    return 1
                fi
                # For missing tests only, check against threshold
                if [[ $coverage_pct -lt $threshold ]]; then
                    echo "" >&2
                    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}" >&2
                    echo -e "${RED}❌ COVERAGE BELOW THRESHOLD${NC}" >&2
                    echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}" >&2
                    echo "" >&2
                    echo "Coverage: ${coverage_pct}% (minimum: ${threshold}%)" >&2
                    echo "MISSING: $missing script(s) without tests. Create test or add exception." >&2
                    echo "" >&2
                    log_error "Coverage ${coverage_pct}% below ${threshold}% threshold ($issue_details)"
                    return 1
                fi
                # Above threshold — warn about gaps but pass
                log_warn "Coverage: ${coverage_pct}% >= ${threshold}% threshold ($issue_details — non-blocking)"
                return 0
                ;;
            *)
                log_warn "Coverage: ${coverage_pct}% ($issue_details)"
                return 0
                ;;
        esac
    fi

    log_success "All scripts have tests, correctly placed and registered (${coverage_pct}%)"
    return 0
}

# ============================================================================
# STAGED FILES VALIDATION
# ============================================================================

# Validate test coverage for staged (new/modified) scripts only
# Usage: validate_staged_coverage
# Returns: 0 if all covered, 1 if gaps found (when validation_mode=fail)
validate_staged_coverage() {
    local repo_root
    repo_root=$(get_repo_root)

    # Read config values
    local enabled validation_mode pre_commit_mode
    if [[ -f "$TEST_CONFIG_FILE" ]] && command -v jq &>/dev/null; then
        enabled=$(jq -r '.coverage_enforcement.enabled // false' "$TEST_CONFIG_FILE" 2>/dev/null)
        validation_mode=$(jq -r '.coverage_enforcement.validation_mode // "warn"' "$TEST_CONFIG_FILE" 2>/dev/null)
        pre_commit_mode=$(jq -r '.coverage_enforcement.pre_commit_mode // "none"' "$TEST_CONFIG_FILE" 2>/dev/null)

        # Skip if disabled
        if [[ "$enabled" != "true" ]] || [[ "$validation_mode" == "disabled" ]] || [[ "$pre_commit_mode" == "none" ]]; then
            return 0
        fi
    else
        log_info "Coverage config unavailable (skipping staged check)"
        return 0
    fi

    log_section "Staged Files Coverage"

    # Get staged scripts based on pre_commit_mode
    local staged_scripts
    case "$pre_commit_mode" in
        new_only)
            # Only newly ADDED scripts
            staged_scripts=$(git diff --cached --name-only --diff-filter=A 2>/dev/null | grep -E '\.(py|sh)$' || true)
            ;;
        all)
            # All staged scripts (added, copied, modified)
            staged_scripts=$(git diff --cached --name-only --diff-filter=ACM 2>/dev/null | grep -E '\.(py|sh)$' || true)
            ;;
        *)
            log_info "Pre-commit coverage check disabled (mode: $pre_commit_mode)"
            return 0
            ;;
    esac

    # Filter to only .codeflow/scripts/ and .claude/hooks/
    staged_scripts=$(echo "$staged_scripts" | grep -E '^(\.codeflow/scripts/|\.claude/hooks/)' || true)

    if [[ -z "$staged_scripts" ]]; then
        log_info "No staged scripts to check (mode: $pre_commit_mode)"
        return 0
    fi

    local missing=0 misplaced=0 unregistered=0

    while IFS= read -r script; do
        [[ -z "$script" ]] && continue

        local test_path
        test_path=$(derive_test_path "$script")

        # Skip if no naming convention matches
        [[ -z "$test_path" ]] && continue

        # Check if test file exists at expected location
        if [[ -f "$repo_root/$test_path" ]]; then
            # Test exists - check if registered in config
            local config_path="${test_path#.codeflow/testing/}"
            if ! is_test_registered "$config_path"; then
                ((unregistered++)) || true
                log_warn "Test not registered: $script"
                echo "    Test file: $test_path" >&2
                echo "    ${RED}NOT REGISTERED in test-config.json${NC}" >&2
                echo "    Action: Add \"$config_path\" to .codeflow/testing/test-config.json priorities" >&2
            fi
            continue
        fi

        # Skip if excepted
        is_excepted "$script" && continue

        # Test doesn't exist at expected path - check if misplaced
        local actual_path
        actual_path=$(find_misplaced_test "$test_path") || true
        if [[ -n "$actual_path" ]]; then
            ((misplaced++)) || true
            log_warn "Test misplaced: $script"
            echo "    Expected: $test_path" >&2
            echo "    Found at: $actual_path ${RED}(WRONG LOCATION)${NC}" >&2
            echo "    Action: mv $repo_root/$actual_path $repo_root/$test_path" >&2
            continue
        fi

        # Coverage gap found - no test anywhere
        ((missing++)) || true
        log_warn "Script without test: $script"
        echo "    Expected: $test_path" >&2
    done <<< "$staged_scripts"

    local total_issues=$((missing + misplaced + unregistered))

    if [[ $total_issues -gt 0 ]]; then
        echo "" >&2
        echo "Fix issues above or document exceptions in .codeflow/testing/test-config.json" >&2
        echo "" >&2

        local issue_details=""
        [[ $missing -gt 0 ]] && issue_details+="$missing missing"
        [[ $misplaced -gt 0 ]] && {
            [[ -n "$issue_details" ]] && issue_details+=", "
            issue_details+="$misplaced misplaced"
        }
        [[ $unregistered -gt 0 ]] && {
            [[ -n "$issue_details" ]] && issue_details+=", "
            issue_details+="$unregistered unregistered"
        }

        case "$validation_mode" in
            audit)
                log_info "Coverage issues detected ($issue_details) [audit mode]"
                return 0
                ;;
            warn)
                log_warn "Coverage issues detected ($issue_details)"
                return 0
                ;;
            fail)
                log_error "Coverage validation failed: $issue_details"
                return 1
                ;;
            *)
                log_warn "Coverage issues detected ($issue_details)"
                return 0
                ;;
        esac
    fi

    log_success "All staged scripts have tests, correctly placed and registered"
    return 0
}

# ============================================================================
# ORPHAN EXCEPTION CHECKING
# ============================================================================

# Check if a test file is excepted from orphan detection
# Usage: is_orphan_excepted "test_relative_path"
# test_relative_path is relative to .codeflow/testing/ (e.g., "lib/test-common.sh")
# Returns: 0 if excepted, 1 if not
is_orphan_excepted() {
    local test_path="$1"

    if [[ ! -f "$TEST_CONFIG_FILE" ]] || ! command -v jq &>/dev/null; then
        return 1
    fi

    local pattern
    while IFS= read -r pattern; do
        [[ -z "$pattern" ]] && continue
        if _pattern_matches "$test_path" "$pattern"; then
            return 0
        fi
    done < <(jq -r '.coverage_enforcement.orphan_exceptions[]?.pattern // empty' "$TEST_CONFIG_FILE" 2>/dev/null)

    return 1
}

# ============================================================================
# ORPHANED TEST DETECTION
# ============================================================================

# Find test files that don't have corresponding scripts
# Usage: find_orphaned_tests
find_orphaned_tests() {
    local repo_root
    repo_root=$(get_repo_root)
    local orphaned=()
    local excepted_count=0

    log_section "Orphaned Test Detection"

    # Find all shell test files
    while IFS= read -r test_file; do
        [[ -z "$test_file" ]] && continue

        local relative="${test_file#"$repo_root"/}"
        local config_relative="${relative#.codeflow/testing/}"
        local test_name script_found=false

        # Check orphan exceptions first
        if is_orphan_excepted "$config_relative"; then
            ((excepted_count++)) || true
            continue
        fi

        # Extract test name (test-{name}.sh → {name})
        test_name=$(basename "$test_file" .sh)
        test_name="${test_name#test-}"

        # Determine expected script location based on test location
        if [[ "$relative" == .codeflow/testing/claude-hooks/* ]]; then
            # Extract category from path
            local category
            category=$(dirname "$relative" | sed 's|.codeflow/testing/claude-hooks/||')
            if [[ -f "$repo_root/.claude/hooks/codeflow/$category/${test_name}.sh" ]]; then
                script_found=true
            fi
        elif [[ "$relative" == .codeflow/testing/scripts/* ]]; then
            # Extract domain from path
            local domain
            domain=$(echo "$relative" | sed -E 's|.codeflow/testing/scripts/([^/]+)/.*|\1|')

            # Try multiple name variants for source scripts
            local found_file=""
            found_file=$(find "$repo_root/.codeflow/scripts/$domain" \( \
                -name "${test_name}.sh" -o \
                -name "${test_name//_/-}.sh" -o \
                -name "cf-${test_name}.sh" -o \
                -name "cf-${test_name//_/-}.sh" -o \
                -name "${test_name}.py" -o \
                -name "${test_name//_/-}.py" -o \
                -name "cf-${test_name}.py" -o \
                -name "cf-${test_name//_/-}.py" -o \
                -name "${test_name}" -o \
                -name "${test_name//_/-}" \
                \) -type f 2>/dev/null | head -1) || true

            if [[ -n "$found_file" ]]; then
                script_found=true
            fi
        fi

        if [[ "$script_found" == "false" ]]; then
            orphaned+=("$relative")
        fi
    done < <(find "$repo_root/.codeflow/testing" -name "test-*.sh" -type f ! -path "*/.venv/*" 2>/dev/null)

    # Find all Python test files
    while IFS= read -r test_file; do
        [[ -z "$test_file" ]] && continue

        local relative="${test_file#"$repo_root"/}"
        local config_relative="${relative#.codeflow/testing/}"
        local test_name script_found=false

        # Check orphan exceptions first
        if is_orphan_excepted "$config_relative"; then
            ((excepted_count++)) || true
            continue
        fi

        # Extract test name (test_{name}.py → {name})
        test_name=$(basename "$test_file" .py)
        test_name="${test_name#test_}"

        if [[ "$relative" == .codeflow/testing/scripts/* ]]; then
            local domain
            domain=$(echo "$relative" | sed -E 's|.codeflow/testing/scripts/([^/]+)/.*|\1|')

            local found_file=""
            found_file=$(find "$repo_root/.codeflow/scripts/$domain" \( \
                -name "${test_name}.py" -o \
                -name "${test_name//_/-}.py" -o \
                -name "cf-${test_name}.py" -o \
                -name "cf-${test_name//_/-}.py" -o \
                -name "${test_name}.sh" -o \
                -name "${test_name//_/-}.sh" -o \
                -name "cf-${test_name}.sh" -o \
                -name "cf-${test_name//_/-}.sh" \
                \) -type f 2>/dev/null | head -1) || true

            if [[ -n "$found_file" ]]; then
                script_found=true
            fi
        fi

        if [[ "$script_found" == "false" ]]; then
            orphaned+=("$relative")
        fi
    done < <(find "$repo_root/.codeflow/testing" -name "test_*.py" -type f ! -path "*/.venv/*" ! -path "*/__pycache__/*" 2>/dev/null)

    # Report results
    if [[ $excepted_count -gt 0 ]]; then
        log_verbose "$excepted_count test file(s) skipped (orphan exceptions)"
    fi

    if [[ ${#orphaned[@]} -gt 0 ]]; then
        echo ""
        echo -e "${RED}ORPHANED TESTS (no matching script):${NC}"
        for test in "${orphaned[@]}"; do
            echo "  $test"
            echo "     Action: Remove test file, fix name to match source, or add orphan exception"
        done
        echo ""
        log_error "Found ${#orphaned[@]} orphaned test file(s)"
        return 1
    else
        log_success "No orphaned tests found"
        return 0
    fi
}

# ============================================================================
# CHANGE COVERAGE (GIT-AWARE)
# ============================================================================

# Validate that modified source scripts have corresponding test file updates
# Usage: validate_change_coverage
# Returns: 0 if all modified source scripts have test updates, 1 if gaps found
validate_change_coverage() {
    local repo_root
    repo_root=$(get_repo_root)

    # Get files changed on this branch compared to main
    local changed_files
    changed_files=$(git -C "$repo_root" diff --name-only main...HEAD 2>/dev/null) || {
        log_info "Cannot diff against main branch. Skipping change coverage check."
        return 0
    }

    if [[ -z "$changed_files" ]]; then
        log_info "No files changed vs main. Skipping change coverage check."
        return 0
    fi

    # Filter for source scripts (hooks + scripts, exclude shell-lib and __init__.py)
    local source_scripts
    source_scripts=$(echo "$changed_files" | grep -E '^(\.claude/hooks/codeflow/.*\.sh|\.codeflow/scripts/.*\.(sh|py))$' | grep -vE '/shell-lib/|/__pycache__/|/__init__\.py$' || true)

    if [[ -z "$source_scripts" ]]; then
        log_info "No source scripts in diff"
        return 0
    fi

    local total=0 covered=0 uncovered=0 excepted=0
    local uncovered_list=()

    while IFS= read -r script; do
        [[ -z "$script" ]] && continue

        # Only skip scripts that truly have no test file (no_test_required).
        # Do NOT skip integration_tested scripts — they have tests, and those
        # tests must be updated when the source changes.
        if _is_no_test_required "$script"; then
            ((excepted++)) || true
            continue
        fi

        # Derive expected test path
        local test_path
        test_path=$(derive_test_path "$script")

        if [[ -z "$test_path" ]]; then
            continue
        fi

        # Only check if the test file actually exists on disk
        # (missing test files are caught by Direction 1)
        if [[ ! -f "$repo_root/$test_path" ]]; then
            continue
        fi

        ((total++)) || true

        # Check if test file is also in the diff (exact line match)
        if echo "$changed_files" | grep -qxF "$test_path"; then
            ((covered++)) || true
        else
            ((uncovered++)) || true
            uncovered_list+=("$script|$test_path")
        fi
    done <<< "$source_scripts"

    # Report
    echo ""
    echo "  Source scripts modified: $total"
    echo "  With test updates:      $covered"
    echo "  Without test updates:   $uncovered"
    [[ $excepted -gt 0 ]] && echo "  Excepted (no test req): $excepted"
    echo ""

    if [[ $uncovered -gt 0 ]]; then
        echo -e "${RED}CHANGE COVERAGE GAPS (source changed, test not updated):${NC}"
        for entry in "${uncovered_list[@]}"; do
            IFS='|' read -r script_name test_path_val <<< "$entry"
            echo "  $script_name"
            echo "     Expected test update: $test_path_val"
            echo "     Action: Update the test file to cover the source change"
        done
        echo ""
        log_error "Change coverage: $uncovered source script(s) modified without corresponding test update"
        return 1
    fi

    log_success "All modified source scripts have corresponding test updates"
    return 0
}

# ============================================================================
# STRUCTURAL INTEGRITY (BIDIRECTIONAL)
# ============================================================================

# Validate bidirectional test-to-source mapping
# Direction 1: Every source script must have a test (validate_coverage)
# Direction 2: Every test file must map to a source (find_orphaned_tests)
# Usage: validate_structural_integrity
# Returns: 0 if both pass, 1 if either fails
validate_structural_integrity() {
    local rc=0

    log_section "Structural Integrity Check (Bidirectional)"
    echo "  Direction 1: Every source script must have a test file"
    echo "  Direction 2: Every test file must map to a source script"
    echo "  Direction 3: Every modified source script must have its test also modified"
    echo ""

    # Direction 1: Source → Test (always fail mode)
    if ! validate_coverage "fail"; then
        rc=1
    fi

    # Direction 2: Test → Source
    if ! find_orphaned_tests; then
        rc=1
    fi

    # Direction 3: Changed source → Changed test (git-aware)
    log_section "Change Coverage Check"
    if ! validate_change_coverage; then
        rc=1
    fi

    echo ""
    if [[ $rc -eq 0 ]]; then
        echo -e "${BOLD}STRUCTURAL INTEGRITY: PASS${NC}"
    else
        echo "" >&2
        echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}" >&2
        echo -e "${RED}❌ STRUCTURAL INTEGRITY CHECK FAILED${NC}" >&2
        echo -e "${RED}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}" >&2
        echo "" >&2
        echo "Both directions must pass before running functional tests." >&2
        echo "Fix all issues above, then re-run." >&2
    fi

    return $rc
}

# ============================================================================
# CLI
# ============================================================================

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    case "${1:-}" in
        --staged)
            validate_staged_coverage
            ;;
        --audit)
            validate_coverage "audit"
            ;;
        --warn)
            validate_coverage "warn"
            ;;
        --fail)
            validate_coverage "fail"
            ;;
        --orphaned)
            find_orphaned_tests
            ;;
        --change-coverage)
            validate_change_coverage
            ;;
        --structural)
            validate_structural_integrity
            ;;
        --help|-h)
            echo "Usage: $(basename "$0") [option]"
            echo ""
            echo "Options:"
            echo "  --staged           Validate only staged files (for pre-commit)"
            echo "  --audit            Report issues without failing"
            echo "  --warn             Report issues as warnings (default)"
            echo "  --fail             Fail if any issues found"
            echo "  --orphaned         Find test files without matching scripts"
            echo "  --change-coverage  Check modified source scripts have test updates"
            echo "  --structural       Run full bidirectional structural integrity check"
            echo "  --help             Show this help"
            ;;
        *)
            validate_coverage
            ;;
    esac
fi
