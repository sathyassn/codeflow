#!/usr/bin/env bash
# Purpose:   Status checking and verification for cf-protect-resources.sh
# Location:  .codeflow/scripts/security/protection/lib/cf-protection-verify.sh
# Usage:     source lib/cf-protection-verify.sh (from cf-protect-resources.sh)
# Version:   1.0.0
#
# This library provides status and verification functions:
#   - check_path_status() - Check protection status of a single path
#   - show_status() - Show status of all protected paths
#   - verify_protection() - Test if protection actually works
#
# Status output:
#   - PROTECTED: Root-owned with immutable flag
#   - PARTIAL: Root-owned but not immutable
#   - UNPROTECTED: Not root-owned
#   - MISSING: Path does not exist
#
# Requires: cf-protection-common.sh must be sourced first
#
# Provides:
#   - check_path_status()
#   - show_status()
#   - verify_protection()

# Prevent direct execution
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "ERROR: This script must be sourced, not executed directly." >&2
    exit 1
fi

# Verify dependencies
if [[ -z "${PROJECT_ROOT:-}" ]]; then
    echo "ERROR: cf-protection-common.sh must be sourced before cf-protection-verify.sh" >&2
    return 1
fi

# =============================================================================
# STATUS AND VERIFICATION
# =============================================================================

# Check protection status of a single path
check_path_status() {
    local path="$1"
    local full_path="$PROJECT_ROOT/$path"

    if [[ ! -e "$full_path" ]]; then
        echo -e "  ${RED}MISSING${NC}    $path"
        return 1
    fi

    # Get owner, group, and permissions
    local owner
    local group
    local perms
    if [[ "$OS" == "macos" ]]; then
        owner=$(stat -f "%Su" "$full_path")
        group=$(stat -f "%Sg" "$full_path")
        perms=$(stat -f "%Lp" "$full_path")
    else
        owner=$(stat -c "%U" "$full_path")
        group=$(stat -c "%G" "$full_path")
        perms=$(stat -c "%a" "$full_path")
    fi

    # Check immutable flag
    local immutable="no"
    local first_file
    local flags
    local attrs
    if [[ "$OS" == "macos" ]]; then
        if [[ -d "$full_path" ]]; then
            first_file=$(find "$full_path" -type f -print -quit 2>/dev/null)
            if [[ -n "$first_file" ]]; then
                # shellcheck disable=SC2012 # ls -lO required for macOS immutable flags
                flags=$(ls -lO "$first_file" 2>/dev/null | awk '{print $5}')
                [[ "$flags" == *"uchg"* ]] && immutable="yes"
            fi
        else
            # shellcheck disable=SC2012 # ls -lO required for macOS immutable flags
            flags=$(ls -lO "$full_path" 2>/dev/null | awk '{print $5}')
            [[ "$flags" == *"uchg"* ]] && immutable="yes"
        fi
    else
        if [[ -d "$full_path" ]]; then
            first_file=$(find "$full_path" -type f -print -quit 2>/dev/null)
            if [[ -n "$first_file" ]]; then
                attrs=$(lsattr "$first_file" 2>/dev/null | cut -d' ' -f1)
                [[ "$attrs" == *"i"* ]] && immutable="yes"
            fi
        else
            attrs=$(lsattr "$full_path" 2>/dev/null | cut -d' ' -f1)
            [[ "$attrs" == *"i"* ]] && immutable="yes"
        fi
    fi

    # Determine status
    local info="($owner:$group $perms)"
    if [[ "$owner" == "root" ]] && [[ "$immutable" == "yes" ]]; then
        echo -e "  ${GREEN}PROTECTED${NC}   $path $info"
        return 0
    elif [[ "$owner" == "root" ]]; then
        echo -e "  ${YELLOW}PARTIAL${NC}     $path $info (not immutable)"
        return 1
    else
        echo -e "  ${RED}UNPROTECTED${NC} $path $info"
        return 1
    fi
}

# Show status of all protected paths
show_status() {
    local specific_path="${1:-}"

    echo ""
    echo "Protection Status"
    echo "================="
    echo "OS: $OS | Project: $(basename "$PROJECT_ROOT")"
    echo ""

    if [[ -n "$specific_path" ]]; then
        if ! validate_path "$specific_path"; then
            return 1
        fi
        check_path_status "$specific_path"
        return
    fi

    local all_protected=true
    local extended_paths
    local adhoc_paths

    echo -e "${CYAN}Core:${NC}"
    for path in "${CORE_PATHS[@]}"; do
        check_path_status "$path" || all_protected=false
    done

    echo ""
    echo -e "${CYAN}Extended:${NC}"
    extended_paths=$(read_list_file "$EXTENDED_LIST")
    if [[ -z "$extended_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            check_path_status "$path" || all_protected=false
        done <<< "$extended_paths"
    fi

    echo ""
    echo -e "${CYAN}Ad-hoc:${NC}"
    adhoc_paths=$(read_list_file "$ADHOC_LIST")
    if [[ -z "$adhoc_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            check_path_status "$path" || all_protected=false
        done <<< "$adhoc_paths"
    fi

    echo ""
    if $all_protected; then
        log_success "All paths protected"
    else
        log_warn "Some paths not fully protected"
    fi
}

# Verify protection by attempting writes
# For directories: tries to create a file inside
# For files: tries to write to the file itself (non-destructively)
verify_protection() {
    echo ""
    echo "Verification Test"
    echo "================="
    echo ""

    local original_user="${SUDO_USER:-$(logname 2>/dev/null || echo "$USER")}"
    local all_protected=true
    local extended_paths
    local adhoc_paths

    # Helper function to test a single path
    test_path_protection() {
        local path="$1"
        local full_path="$PROJECT_ROOT/$path"

        if [[ ! -e "$full_path" ]]; then
            return 0  # Skip missing paths
        fi

        if [[ -d "$full_path" ]]; then
            # Directory: test creating a file inside
            local test_file="$full_path/.protection-test-$$"
            if sudo -u "$original_user" touch "$test_file" 2>/dev/null; then
                echo -e "  ${RED}FAIL${NC} $path (directory write succeeded)"
                rm -f "$test_file" 2>/dev/null || true
                return 1
            else
                echo -e "  ${GREEN}PASS${NC} $path"
                return 0
            fi
        else
            # File: test writing to the file itself (non-destructive append of nothing)
            # This tests if the file can be opened for writing, without modifying content
            if sudo -u "$original_user" bash -c "echo -n >> '$full_path'" 2>/dev/null; then
                echo -e "  ${RED}FAIL${NC} $path (file write succeeded)"
                return 1
            else
                echo -e "  ${GREEN}PASS${NC} $path"
                return 0
            fi
        fi
    }

    # Test Core paths
    echo -e "${CYAN}Core:${NC}"
    for path in "${CORE_PATHS[@]}"; do
        test_path_protection "$path" || all_protected=false
    done

    # Test Extended paths
    echo ""
    echo -e "${CYAN}Extended:${NC}"
    extended_paths=$(read_list_file "$EXTENDED_LIST")
    if [[ -z "$extended_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            test_path_protection "$path" || all_protected=false
        done <<< "$extended_paths"
    fi

    # Test Ad-hoc paths
    echo ""
    echo -e "${CYAN}Ad-hoc:${NC}"
    adhoc_paths=$(read_list_file "$ADHOC_LIST")
    if [[ -z "$adhoc_paths" ]]; then
        echo "  (none)"
    else
        while IFS= read -r path; do
            test_path_protection "$path" || all_protected=false
        done <<< "$adhoc_paths"
    fi

    echo ""
    if $all_protected; then
        log_success "All paths verified"
        return 0
    else
        log_error "Some paths failed verification"
        return 1
    fi
}
