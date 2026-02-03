#!/usr/bin/env bash
# CodeFlow Test Framework: Coverage Validation
# Location: .codeflow/testing/lib/test-coverage.sh

# Requires: test-common.sh
COVERAGE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ -z "${TEST_FRAMEWORK_VERSION:-}" ]] && source "$COVERAGE_DIR/test-common.sh"

# ============================================================================
# SCRIPT DISCOVERY
# ============================================================================

discover_scripts() {
    local repo_root
    repo_root=$(get_repo_root)

    # Find Python scripts
    find "$repo_root/.codeflow/scripts/" -type f -name "*.py" \
        ! -path "*/__pycache__/*" \
        ! -path "*/lib/__init__.py" \
        ! -name "__init__.py" \
        -print 2>/dev/null

    # Find Bash scripts
    find "$repo_root/.codeflow/scripts/" -type f -name "*.sh" \
        ! -path "*/shell-lib/*" \
        -print 2>/dev/null

    # Find hooks
    find "$repo_root/.claude/hooks/codeflow/" -type f -name "*.sh" -print 2>/dev/null
}

discover_lib_modules() {
    local repo_root
    repo_root=$(get_repo_root)

    # Python library modules
    find "$repo_root/.codeflow/scripts/codeflow_py_lib/" -type f -name "*.py" \
        ! -name "__init__.py" \
        -print 2>/dev/null

    # Shell library modules
    find "$repo_root/.codeflow/scripts/shell-lib/" -type f -name "*.sh" \
        ! -name "index.sh" \
        -print 2>/dev/null
}

# ============================================================================
# TEST PATH DERIVATION
# ============================================================================

derive_test_path() {
    local script="$1"
    local repo_root
    repo_root=$(get_repo_root)
    local rel_path="${script#$repo_root/}"

    case "$rel_path" in
        .codeflow/scripts/codeflow_py_lib/*.py)
            # Library modules: validation.py → test_validation.py
            local name
            name=$(basename "$script" .py)
            echo "$repo_root/.codeflow/testing/scripts/codeflow_py_lib/test_${name}.py"
            ;;
        .codeflow/scripts/shell-lib/*.sh)
            # Shell library: common.sh → test-common.sh
            local name
            name=$(basename "$script" .sh)
            echo "$repo_root/.codeflow/testing/scripts/shell-lib/test-${name}.sh"
            ;;
        .codeflow/scripts/*/*.py)
            # Domain scripts: cf-seed.py → test_cf_seed.py
            local dir
            dir=$(dirname "$rel_path" | sed 's|^.codeflow/scripts/|.codeflow/testing/scripts/|')
            local name
            name=$(basename "$script" .py | sed 's/-/_/g')
            echo "$repo_root/$dir/test_${name}.py"
            ;;
        .codeflow/scripts/*/*.sh)
            # Domain scripts: cf-sentinel.sh → test-cf-sentinel.sh
            local dir
            dir=$(dirname "$rel_path" | sed 's|^.codeflow/scripts/|.codeflow/testing/scripts/|')
            local name
            name=$(basename "$script" .sh)
            echo "$repo_root/$dir/test-${name}.sh"
            ;;
        .claude/hooks/codeflow/*/*.sh)
            # Hooks: cf-pre-tool-use-security.sh → test-cf-pre-tool-use-security.sh
            local category
            category=$(dirname "$rel_path" | sed 's|^.claude/hooks/codeflow/||' | tr '[:upper:]' '[:lower:]')
            # Convert PascalCase to kebab-case
            category=$(echo "$category" | sed 's/\([A-Z]\)/-\L\1/g' | sed 's/^-//')
            local name
            name=$(basename "$script" .sh)
            echo "$repo_root/.codeflow/testing/hooks/$category/test-${name}.sh"
            ;;
        *)
            echo ""
            ;;
    esac
}

# ============================================================================
# EXCEPTION HANDLING (Bash 3.2 compatible - no associative arrays)
# ============================================================================

# Check if a script is excepted from coverage requirements
# Args: $1 = script path
# Returns: 0 if excepted, 1 otherwise
is_excepted() {
    local script="$1"
    local repo_root
    repo_root=$(get_repo_root)
    local rel_path="${script#$repo_root/}"

    # Integration-tested modules - these are tested via hook tests
    case "$rel_path" in
        .codeflow/scripts/security/enforcement/checks/*)
            # Tested by: hooks/pre-tool-use/test-cf-pre-tool-use-security.sh
            return 0
            ;;
    esac

    return 1
}

# ============================================================================
# COVERAGE VALIDATION
# ============================================================================

validate_coverage() {
    local mode="${1:-fail}"  # fail, warn, or audit
    local repo_root
    repo_root=$(get_repo_root)

    local missing=()
    local covered=0
    local total=0

    log_section "Coverage Validation"

    # Check scripts
    while IFS= read -r script; do
        [[ -z "$script" ]] && continue
        ((total++))

        if is_excepted "$script"; then
            ((covered++))
            continue
        fi

        local test_path
        test_path=$(derive_test_path "$script")

        if [[ -n "$test_path" && -f "$test_path" ]]; then
            ((covered++))
        else
            missing+=("$script")
        fi
    done < <(discover_scripts)

    # Check library modules
    while IFS= read -r module; do
        [[ -z "$module" ]] && continue
        ((total++))

        local test_path
        test_path=$(derive_test_path "$module")

        if [[ -n "$test_path" && -f "$test_path" ]]; then
            ((covered++))
        else
            missing+=("$module")
        fi
    done < <(discover_lib_modules)

    # Report results
    local percentage=0
    [[ $total -gt 0 ]] && percentage=$((covered * 100 / total))

    echo ""
    echo -e "  Total scripts:  $total"
    echo -e "  With tests:     $covered"
    echo -e "  Missing tests:  ${#missing[@]}"
    echo -e "  Coverage:       ${percentage}%"

    if [[ ${#missing[@]} -gt 0 ]]; then
        echo ""
        echo -e "${YELLOW}Missing tests for:${NC}"
        for script in "${missing[@]}"; do
            local rel_path="${script#$repo_root/}"
            local expected
            expected=$(derive_test_path "$script")
            expected="${expected#$repo_root/}"
            echo "  - $rel_path"
            [[ -n "$expected" ]] && echo "    Expected: $expected"
        done

        case "$mode" in
            fail)
                echo ""
                log_error "Coverage validation failed"
                return 1
                ;;
            warn)
                echo ""
                log_warn "Coverage validation has warnings"
                return 0
                ;;
            audit)
                echo ""
                log_info "Coverage audit complete (report only)"
                return 0
                ;;
        esac
    else
        echo ""
        log_success "All scripts have tests"
        return 0
    fi
}

# ============================================================================
# STAGED FILES VALIDATION
# ============================================================================

validate_staged_coverage() {
    local repo_root
    repo_root=$(get_repo_root)

    log_section "Staged Files Coverage"

    local staged_scripts=()
    while IFS= read -r file; do
        if [[ "$file" == *.py || "$file" == *.sh ]]; then
            if [[ "$file" == .codeflow/scripts/* || "$file" == .claude/hooks/* ]]; then
                staged_scripts+=("$repo_root/$file")
            fi
        fi
    done < <(git diff --cached --name-only 2>/dev/null)

    if [[ ${#staged_scripts[@]} -eq 0 ]]; then
        log_info "No staged scripts to check"
        return 0
    fi

    local missing=()
    for script in "${staged_scripts[@]}"; do
        if is_excepted "$script"; then
            continue
        fi

        local test_path
        test_path=$(derive_test_path "$script")

        if [[ -z "$test_path" || ! -f "$test_path" ]]; then
            missing+=("$script")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        echo ""
        log_warn "Staged scripts missing tests:"
        for script in "${missing[@]}"; do
            echo "  - ${script#$repo_root/}"
        done
        return 1
    fi

    log_success "All staged scripts have tests"
    return 0
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
        *)
            validate_coverage "fail"
            ;;
    esac
fi
