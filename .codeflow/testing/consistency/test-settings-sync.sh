#!/usr/bin/env bash
# Purpose:   Verify hooks consistency across settings templates and settings files
# Usage:     test-settings-sync.sh [--verbose]
# Author:    CodeFlow Project
# Created:   2026-02-04
# Version:   1.0.0
#
# CRITICAL TEST - Runs on every commit via pre-commit hook
#
# This test enforces MANDATORY consistency requirements:
#   1. All 4 settings templates MUST have IDENTICAL hooks sections
#   2. settings.json hooks MUST match its source template (strict.json)
#   3. settings.local.json hooks MUST match its source template (autonomous.json)
#   4. All .claude/hooks/codeflow/**/*.sh files MUST be registered in settings
#   5. _version MUST be identical across ALL templates
#
# FAILURE = BLOCKED COMMIT. These are non-negotiable requirements.
#
# Exit codes:
#   0 - All consistency checks passed
#   1 - One or more consistency checks failed (COMMIT BLOCKED)
#   2 - Invalid arguments or missing dependencies

set -uo pipefail

# ═══════════════════════════════════════════════════════════════════════════════
# CONSTANTS
# ═══════════════════════════════════════════════════════════════════════════════

readonly VERSION="1.0.0"

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR

# Get repository root
REPO_ROOT="$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel 2>/dev/null || dirname "$(dirname "$(dirname "$SCRIPT_DIR")")")"
readonly REPO_ROOT

# Template paths
readonly TEMPLATES_DIR="$REPO_ROOT/.claude/settings-templates"
readonly STRICT_TEMPLATE="$TEMPLATES_DIR/strict.json"
readonly STANDARD_TEMPLATE="$TEMPLATES_DIR/standard.json"
readonly AUTONOMOUS_TEMPLATE="$TEMPLATES_DIR/autonomous.json"
readonly PERMISSIVE_TEMPLATE="$TEMPLATES_DIR/permissive.json"

# Settings files
readonly SETTINGS_FILE="$REPO_ROOT/.claude/settings.json"
readonly SETTINGS_LOCAL="$REPO_ROOT/.claude/settings.local.json"

# Hooks directory (codeflow uses subdirectories per event type)
readonly HOOKS_DIR="$REPO_ROOT/.claude/hooks/codeflow"

# Minimal self-contained test harness. The legacy shell test-framework was
# retired in INF-TSK-046-008; this file runs standalone under bash.
if true; then
    TEST_PASS_COUNT=0
    TEST_FAIL_COUNT=0
    TEST_SKIP_COUNT=0
    TEST_TOTAL_COUNT=0
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    NC='\033[0m'
    BOLD='\033[1m'

    test_pass() { ((TEST_PASS_COUNT++)) || true; ((TEST_TOTAL_COUNT++)) || true; echo -e "  ${GREEN}✓${NC} $1"; }
    test_fail() { ((TEST_FAIL_COUNT++)) || true; ((TEST_TOTAL_COUNT++)) || true; echo -e "  ${RED}✗${NC} $1${2:+ - }${2:-}"; }
    test_skip() { ((TEST_SKIP_COUNT++)) || true; ((TEST_TOTAL_COUNT++)) || true; echo -e "  ${YELLOW}○${NC} $1${2:+ (}${2}${2:+)}"; }
    test_section() { echo ""; echo -e "${BOLD}=== $1 ===${NC}"; }
    print_test_summary() {
        echo ""
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo -e "${BOLD}Test Summary${NC}"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        echo -e "  ${GREEN}Passed:${NC}  $TEST_PASS_COUNT"
        echo -e "  ${RED}Failed:${NC}  $TEST_FAIL_COUNT"
        echo -e "  ${YELLOW}Skipped:${NC} $TEST_SKIP_COUNT"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    }
fi

# ═══════════════════════════════════════════════════════════════════════════════
# CONFIGURATION
# ═══════════════════════════════════════════════════════════════════════════════

# Parse arguments
for arg in "$@"; do
    case $arg in
        -v|--verbose)
            # shellcheck disable=SC2034  # TEST_VERBOSE read by test framework helpers
            TEST_VERBOSE=true
            ;;
        -h|--help)
            echo "Usage: $(basename "$0") [--verbose]"
            echo ""
            echo "CRITICAL test: Verifies hooks consistency across settings templates."
            echo "FAILURE = BLOCKED COMMIT."
            exit 0
            ;;
        -V|--version)
            echo "test-settings-sync.sh version $VERSION"
            exit 0
            ;;
    esac
done

# ═══════════════════════════════════════════════════════════════════════════════
# HELPER FUNCTIONS
# ═══════════════════════════════════════════════════════════════════════════════

# Check if jq is available (REQUIRED for this test)
check_dependencies() {
    if ! command -v jq &> /dev/null; then
        echo "❌ CRITICAL: jq is REQUIRED for hooks consistency test" >&2
        echo "Install with: brew install jq" >&2
        exit 2
    fi
}

# Extract hooks section from a JSON file
get_hooks_section() {
    local file="$1"
    if [[ -f "$file" ]]; then
        jq -S '.hooks // empty' "$file" 2>/dev/null
    else
        echo ""
    fi
}

# Extract _version from a JSON file
get_version() {
    local file="$1"
    if [[ -f "$file" ]]; then
        jq -r '._version // empty' "$file" 2>/dev/null
    else
        echo ""
    fi
}

# Get all hook commands from a settings template
# Structure: .hooks.EventType[].hooks[].command
get_hook_commands() {
    local file="$1"
    if [[ -f "$file" ]]; then
        jq -r '.hooks | to_entries[] | .value[]? | .hooks[]? | .command // empty' "$file" 2>/dev/null | sort -u
    else
        echo ""
    fi
}

# ═══════════════════════════════════════════════════════════════════════════════
# TEST FUNCTIONS
# ═══════════════════════════════════════════════════════════════════════════════

# TEST 1: All 4 templates MUST have IDENTICAL hooks sections
test_template_hooks_consistency() {
    test_section "Template Hooks Consistency (MANDATORY)"

    # Check all templates exist
    local templates=("$STRICT_TEMPLATE" "$STANDARD_TEMPLATE" "$AUTONOMOUS_TEMPLATE" "$PERMISSIVE_TEMPLATE")
    local template_names=("strict.json" "standard.json" "autonomous.json" "permissive.json")

    for i in "${!templates[@]}"; do
        if [[ ! -f "${templates[$i]}" ]]; then
            test_fail "Template MUST exist: ${template_names[$i]}" "File not found: ${templates[$i]}"
            return 1
        fi
    done
    test_pass "All 4 templates exist"

    # Get hooks from strict (reference)
    local strict_hooks
    strict_hooks=$(get_hooks_section "$STRICT_TEMPLATE")

    if [[ -z "$strict_hooks" ]]; then
        test_fail "strict.json MUST have hooks section" "hooks section is empty or missing"
        return 1
    fi
    test_pass "strict.json has hooks section (reference)"

    # Compare all templates against strict
    local failed=false
    for i in "${!templates[@]}"; do
        local template_hooks
        template_hooks=$(get_hooks_section "${templates[$i]}")

        if [[ "$template_hooks" != "$strict_hooks" ]]; then
            test_fail "${template_names[$i]} hooks MUST match strict.json" "Hooks sections differ - this is FORBIDDEN"
            failed=true
        else
            test_pass "${template_names[$i]} hooks match strict.json"
        fi
    done

    if [[ "$failed" == "true" ]]; then
        echo "" >&2
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
        echo "🔒 REQUIRED: Delegate to cf-security teammate" >&2
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
        echo "" >&2
        echo "→ Delegate to cf-security teammate" >&2
        echo "→ Operation: 🔧 sync-settings-templates" >&2
        echo "" >&2
        echo "All 4 templates MUST have IDENTICAL hooks sections." >&2
        echo "This is a NON-NEGOTIABLE requirement." >&2
        echo "" >&2
        return 1
    fi

    return 0
}

# TEST 2: Settings files MUST match their source templates
test_settings_match_templates() {
    test_section "Settings Files Match Templates (MANDATORY)"

    # settings.json MUST match strict.json hooks
    if [[ -f "$SETTINGS_FILE" ]]; then
        local settings_hooks
        local strict_hooks
        settings_hooks=$(get_hooks_section "$SETTINGS_FILE")
        strict_hooks=$(get_hooks_section "$STRICT_TEMPLATE")

        if [[ "$settings_hooks" != "$strict_hooks" ]]; then
            test_fail "settings.json hooks MUST match strict.json" "Hooks differ - sync REQUIRED"
            echo "" >&2
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
            echo "🔒 REQUIRED: Sync settings from template" >&2
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
            echo "" >&2
            echo "Run: cp .claude/settings-templates/strict.json .claude/settings.json" >&2
            echo "" >&2
            return 1
        fi
        test_pass "settings.json hooks match strict.json"
    else
        test_skip "settings.json" "File not found (may be gitignored)"
    fi

    # settings.local.json MUST match autonomous.json hooks
    if [[ -f "$SETTINGS_LOCAL" ]]; then
        local local_hooks
        local autonomous_hooks
        local_hooks=$(get_hooks_section "$SETTINGS_LOCAL")
        autonomous_hooks=$(get_hooks_section "$AUTONOMOUS_TEMPLATE")

        if [[ "$local_hooks" != "$autonomous_hooks" ]]; then
            test_fail "settings.local.json hooks MUST match autonomous.json" "Hooks differ - sync REQUIRED"
            echo "" >&2
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
            echo "🔒 REQUIRED: Sync local settings from template" >&2
            echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
            echo "" >&2
            echo "Run: cp .claude/settings-templates/autonomous.json .claude/settings.local.json" >&2
            echo "" >&2
            return 1
        fi
        test_pass "settings.local.json hooks match autonomous.json"
    else
        test_skip "settings.local.json" "File not found (expected - gitignored)"
    fi

    return 0
}

# TEST 3: All hook scripts MUST be registered
test_hook_scripts_registered() {
    test_section "Hook Scripts Registration (MANDATORY)"

    if [[ ! -d "$HOOKS_DIR" ]]; then
        test_skip "Hook scripts registration" "Hooks directory not found"
        return 0
    fi

    # Get all .sh files in hooks directory (codeflow uses subdirectories)
    local hook_files
    hook_files=$(find "$HOOKS_DIR" -name "*.sh" -type f 2>/dev/null | sort)

    if [[ -z "$hook_files" ]]; then
        test_skip "Hook scripts registration" "No .sh files in hooks directory"
        return 0
    fi

    # Get hooks configuration from strict.json (reference)
    local hooks_config
    hooks_config=$(get_hook_commands "$STRICT_TEMPLATE")

    local failed=false
    local registered_count=0
    local unregistered_count=0

    while IFS= read -r hook_file; do
        local hook_name
        hook_name=$(basename "$hook_file")

        # Skip .gitkeep files
        [[ "$hook_name" == ".gitkeep" ]] && continue

        # Check if hook is referenced in configuration
        if echo "$hooks_config" | grep -q "$hook_name"; then
            test_pass "Hook registered: $hook_name"
            ((registered_count++)) || true
        else
            test_fail "Hook MUST be registered: $hook_name" "Not found in settings templates"
            ((unregistered_count++)) || true
            failed=true
        fi
    done <<< "$hook_files"

    echo ""
    echo "  Registered: $registered_count, Unregistered: $unregistered_count"

    if [[ "$failed" == "true" ]]; then
        echo "" >&2
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
        echo "🔒 REQUIRED: Register all hook scripts" >&2
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
        echo "" >&2
        echo "All .claude/hooks/codeflow/**/*.sh files MUST be registered in settings templates." >&2
        echo "Unregistered hooks will NOT execute - this defeats their purpose." >&2
        echo "" >&2
        echo "→ Delegate to cf-security teammate" >&2
        echo "→ Operation: 🔧 sync-settings-templates" >&2
        echo "" >&2
        return 1
    fi

    return 0
}

# TEST 4: _version MUST be identical across ALL templates
test_version_consistency() {
    test_section "Version Consistency (MANDATORY)"

    local templates=("$STRICT_TEMPLATE" "$STANDARD_TEMPLATE" "$AUTONOMOUS_TEMPLATE" "$PERMISSIVE_TEMPLATE")
    local template_names=("strict.json" "standard.json" "autonomous.json" "permissive.json")

    # Get version from strict (reference)
    local strict_version
    strict_version=$(get_version "$STRICT_TEMPLATE")

    if [[ -z "$strict_version" ]]; then
        test_fail "strict.json MUST have _version field" "_version is missing"
        return 1
    fi
    test_pass "strict.json has _version: $strict_version"

    local failed=false
    for i in "${!templates[@]}"; do
        local template_version
        template_version=$(get_version "${templates[$i]}")

        if [[ "$template_version" != "$strict_version" ]]; then
            test_fail "${template_names[$i]} _version MUST match ($template_version != $strict_version)" "Version mismatch is FORBIDDEN"
            failed=true
        else
            test_pass "${template_names[$i]} _version matches: $template_version"
        fi
    done

    if [[ "$failed" == "true" ]]; then
        echo "" >&2
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
        echo "🔒 REQUIRED: Sync _version across templates" >&2
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2
        echo "" >&2
        echo "→ Delegate to cf-security teammate" >&2
        echo "→ Operation: 🔧 sync-settings-templates" >&2
        echo "" >&2
        echo "All 4 templates MUST have the SAME _version value." >&2
        echo "This is a NON-NEGOTIABLE requirement." >&2
        echo "" >&2
        return 1
    fi

    return 0
}

# TEST 5: env section consistency across templates
test_env_consistency() {
    test_section "Env Section Consistency (MANDATORY)"

    local templates=("$STRICT_TEMPLATE" "$STANDARD_TEMPLATE" "$AUTONOMOUS_TEMPLATE" "$PERMISSIVE_TEMPLATE")
    local template_names=("strict.json" "standard.json" "autonomous.json" "permissive.json")

    # Get env from strict (reference)
    local strict_env
    strict_env=$(jq -S '.env // empty' "$STRICT_TEMPLATE" 2>/dev/null)

    if [[ -z "$strict_env" ]]; then
        test_fail "strict.json MUST have env section" "env section is missing"
        return 1
    fi
    test_pass "strict.json has env section (reference)"

    # Compare all templates against strict
    local failed=false
    for i in "${!templates[@]}"; do
        local template_env
        template_env=$(jq -S '.env // empty' "${templates[$i]}" 2>/dev/null)

        if [[ "$template_env" != "$strict_env" ]]; then
            test_fail "${template_names[$i]} env MUST match strict.json" "env sections differ"
            failed=true
        else
            test_pass "${template_names[$i]} env matches strict.json"
        fi
    done

    if [[ "$failed" == "true" ]]; then
        return 1
    fi

    return 0
}

# TEST 6: _codeflow section removed across templates (dead code cleanup)
test_codeflow_section() {
    test_section "_codeflow Section Absence"

    local templates=("$STRICT_TEMPLATE" "$STANDARD_TEMPLATE" "$AUTONOMOUS_TEMPLATE" "$PERMISSIVE_TEMPLATE")
    local template_names=("strict.json" "standard.json" "autonomous.json" "permissive.json")

    local failed=false
    for i in "${!templates[@]}"; do
        local codeflow_section
        codeflow_section=$(jq '._codeflow // empty' "${templates[$i]}" 2>/dev/null)

        if [[ -n "$codeflow_section" ]]; then
            test_fail "${template_names[$i]} should NOT have _codeflow section (dead code)" "_codeflow section still present"
            failed=true
        else
            test_pass "${template_names[$i]} has no _codeflow section (cleanup confirmed)"
        fi
    done

    if [[ "$failed" == "true" ]]; then
        return 1
    fi

    return 0
}

# TEST 7: Settings entries reference files that exist on disk (reverse orphan check)
test_settings_reference_existing_files() {
    test_section "Settings Reference Existing Files"

    # Get all .sh hook paths from strict.json
    local hook_paths
    hook_paths=$(jq -r '.. | objects | select(.command?) | .command // empty' "$STRICT_TEMPLATE" 2>/dev/null | \
        grep '\.sh' | sed 's|.*bash ||' | sed 's|"\$CLAUDE_PROJECT_DIR"/||' | sort -u)

    if [[ -z "$hook_paths" ]]; then
        test_skip "Settings reference check" "No hook paths found in template"
        return 0
    fi

    local failed=false
    local found_count=0
    local missing_count=0

    while IFS= read -r hook_path; do
        if [[ -f "$REPO_ROOT/$hook_path" ]]; then
            test_pass "File exists: $(basename "$hook_path")"
            ((found_count++)) || true
        else
            test_fail "Referenced file MUST exist: $hook_path" "File not found on disk"
            ((missing_count++)) || true
            failed=true
        fi
    done <<< "$hook_paths"

    echo ""
    echo "  Found: $found_count, Missing: $missing_count"

    if [[ "$failed" == "true" ]]; then
        echo "" >&2
        echo "Settings templates reference hook files that do not exist on disk." >&2
        echo "Either create the files or remove the references." >&2
        return 1
    fi

    return 0
}

# TEST 8: Structural validation of templates
test_structural_validation() {
    test_section "Structural Validation"

    local templates=("$STRICT_TEMPLATE" "$STANDARD_TEMPLATE" "$AUTONOMOUS_TEMPLATE" "$PERMISSIVE_TEMPLATE")
    local template_names=("strict.json" "standard.json" "autonomous.json" "permissive.json")

    # TC-SYNC-011: All templates have valid JSON syntax
    for i in "${!templates[@]}"; do
        if jq empty "${templates[$i]}" 2>/dev/null; then
            test_pass "TC-SYNC-011: ${template_names[$i]} has valid JSON syntax"
        else
            test_fail "TC-SYNC-011: ${template_names[$i]} has valid JSON syntax" "Invalid JSON"
        fi
    done

    # TC-SYNC-012: All required hook types present
    local required_hooks="SessionStart UserPromptSubmit PreToolUse PostToolUse Stop SubagentStop SessionEnd"
    local missing_types=""
    for hook_type in $required_hooks; do
        if jq -e ".hooks.$hook_type" "$STRICT_TEMPLATE" >/dev/null 2>&1; then
            : # present
        else
            missing_types="$missing_types $hook_type"
        fi
    done

    if [[ -z "$missing_types" ]]; then
        test_pass "TC-SYNC-012: All required hook types present"
    else
        test_fail "TC-SYNC-012: All required hook types present" "missing:$missing_types"
    fi

    # TC-SYNC-013: Hook timeouts within valid range (1-30 seconds)
    local invalid_timeouts
    invalid_timeouts=$(jq -r '.. | objects | select(.timeout?) | .timeout' "$STRICT_TEMPLATE" 2>/dev/null | \
        while read -r timeout; do
            if [[ -n "$timeout" ]] && { [[ "$timeout" -lt 1 ]] || [[ "$timeout" -gt 30 ]]; }; then
                echo "$timeout"
            fi
        done)

    if [[ -z "$invalid_timeouts" ]]; then
        test_pass "TC-SYNC-013: Hook timeouts within valid range (1-30 seconds)"
    else
        test_fail "TC-SYNC-013: Hook timeouts within valid range (1-30 seconds)" "invalid: $invalid_timeouts"
    fi

    # TC-SYNC-014: Hook paths use correct relative format (.claude/hooks/codeflow/)
    local bad_paths
    bad_paths=$(jq -r '.. | objects | select(.command?) | .command' "$STRICT_TEMPLATE" 2>/dev/null | \
        grep -E 'hooks/.*\.sh' | grep -v '\.claude/hooks/codeflow/' || true)

    if [[ -z "$bad_paths" ]]; then
        test_pass "TC-SYNC-014: Hook paths use correct relative format (.claude/hooks/codeflow/)"
    else
        test_fail "TC-SYNC-014: Hook paths use correct relative format (.claude/hooks/codeflow/)" "bad paths found"
    fi

    # TC-SYNC-015: No duplicate hook entries within same event type + matcher combo
    # Deduplication is per-matcher, not across all matchers for an event type.
    # The same command (e.g., protection-guard) can legitimately appear in
    # different matchers (Bash vs Edit|Write) since they trigger on different tools.
    local has_duplicates=false
    local event_types
    event_types=$(jq -r '.hooks | keys[]' "$STRICT_TEMPLATE" 2>/dev/null)

    for event_type in $event_types; do
        # Get the number of matchers for this event type
        local matcher_count
        matcher_count=$(jq -r --arg et "$event_type" '.hooks[$et] | length' "$STRICT_TEMPLATE" 2>/dev/null)

        local idx=0
        while [[ "$idx" -lt "$matcher_count" ]]; do
            # Get commands within this specific matcher
            local matcher_commands
            matcher_commands=$(jq -r --arg et "$event_type" --argjson i "$idx" \
                '.hooks[$et][$i].hooks[]?.command // empty' "$STRICT_TEMPLATE" 2>/dev/null | sort)
            local matcher_duplicates
            matcher_duplicates=$(echo "$matcher_commands" | uniq -d)
            if [[ -n "$matcher_duplicates" ]]; then
                has_duplicates=true
            fi
            idx=$((idx + 1))
        done
    done

    if [[ "$has_duplicates" == "false" ]]; then
        test_pass "TC-SYNC-015: No duplicate hook entries within same event type + matcher"
    else
        test_fail "TC-SYNC-015: No duplicate hook entries within same event type + matcher" "duplicates found"
    fi

    # TC-SYNC-016: _template field present and matches filename
    for i in "${!templates[@]}"; do
        local template_field
        template_field=$(jq -r '._template // empty' "${templates[$i]}" 2>/dev/null)
        local expected_name="${template_names[$i]%.json}"  # Remove .json suffix

        if [[ "$template_field" == "$expected_name" ]]; then
            test_pass "TC-SYNC-016: ${template_names[$i]} has correct _template field"
        else
            test_fail "TC-SYNC-016: ${template_names[$i]} _template mismatch" "expected: $expected_name, got: $template_field"
        fi
    done

    return 0
}

# ═══════════════════════════════════════════════════════════════════════════════
# MAIN
# ═══════════════════════════════════════════════════════════════════════════════

main() {
    # Check dependencies first
    check_dependencies

    # Change to repo root
    cd "$REPO_ROOT" || exit 2

    echo ""
    echo "═══════════════════════════════════════════════════════════════"
    echo "HOOKS CONSISTENCY TEST (CRITICAL)"
    echo "═══════════════════════════════════════════════════════════════"
    echo ""
    echo "⚠️  FAILURE = BLOCKED COMMIT"
    echo "These are NON-NEGOTIABLE consistency requirements."
    echo ""

    local failed=0

    # Run all tests
    test_template_hooks_consistency || ((failed++)) || true
    test_settings_match_templates || ((failed++)) || true
    test_hook_scripts_registered || ((failed++)) || true
    test_version_consistency || ((failed++)) || true
    test_env_consistency || ((failed++)) || true
    test_codeflow_section || ((failed++)) || true
    test_settings_reference_existing_files || ((failed++)) || true
    test_structural_validation || ((failed++)) || true

    # Print summary
    print_test_summary

    if [[ $failed -gt 0 ]] || [[ ${TEST_FAIL_COUNT:-0} -gt 0 ]]; then
        echo "" >&2
        echo "❌ HOOKS CONSISTENCY CHECK FAILED" >&2
        echo "" >&2
        echo "This commit is BLOCKED until all consistency requirements are met." >&2
        echo "" >&2
        return 1
    fi

    echo ""
    echo "✅ All hooks consistency checks passed"
    return 0
}

main "$@"
