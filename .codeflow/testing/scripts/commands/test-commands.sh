#!/usr/bin/env bash
# test-commands.sh - Tests for Phase 5 slash command files
# Location: .codeflow/testing/scripts/commands/test-commands.sh
#
# Usage:
#   ./test-commands.sh           Run all tests
#   ./test-commands.sh -h        Show help
#   ./test-commands.sh -V        Show version

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

Tests for Phase 5 slash command files (.claude/commands/cf-*.md).

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
# CONSTANTS
# ============================================================================

COMMANDS_DIR="$REPO_ROOT/.claude/commands"

# All 14 expected command files
EXPECTED_COMMANDS=(
    cf-resume
    cf-help
    cf-approval-mode
    cf-stack
    cf-doctor
    cf-plan
    cf-develop
    cf-review
    cf-test
    cf-ship
    cf-deploy
    cf-document
    cf-cleanup
    cf-autorun
)

# Required sections (Working Protocol + sections 1-10)
REQUIRED_SECTIONS=(
    "## Working Protocol"
    "## 1. Purpose & Usage"
    "## 2. Arguments & Flags"
    "## 3. Prerequisites"
    "## 4. Workflow Definition"
    "## 5. Skills Integration"
    "## 6. Hooks Integration"
    "## 7. Memory Integration"
    "## 8. Error Handling"
    "## 9. Examples"
    "## 10. References"
)

# Forbidden terms (case-insensitive)
FORBIDDEN_TERMS=(
    "standalone"
    "sub-agent"
    "fork-first"
    "dual-mode"
    "cf-general-purpose"
    "cf-support"
    "cf-ops"
)

# Forbidden YAML frontmatter fields
FORBIDDEN_YAML_FIELDS=(
    "version:"
    "category:"
    "agent:"
    "created:"
)

# ============================================================================
# TEST: File Existence
# ============================================================================

test_file_existence() {
    test_section "File Existence (14 commands)"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        assert_file_exists "$COMMANDS_DIR/${cmd}.md" "${cmd}.md exists"
    done

    # Verify exact count (no extra files)
    local actual_count
    actual_count=$(find "$COMMANDS_DIR" -name "cf-*.md" -type f | wc -l | tr -d ' ')
    assert_equals "14" "$actual_count" "Exactly 14 cf-*.md files exist"
}

# ============================================================================
# TEST: YAML Frontmatter
# ============================================================================

test_yaml_frontmatter() {
    test_section "YAML Frontmatter"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        local file="$COMMANDS_DIR/${cmd}.md"
        [[ ! -f "$file" ]] && continue

        local content
        content=$(cat "$file")

        # Starts with ---
        local first_line
        first_line=$(head -1 "$file")
        assert_equals "---" "$first_line" "${cmd}: starts with ---"

        # Extract frontmatter (between first and second ---)
        local frontmatter
        frontmatter=$(sed -n '2,/^---$/p' "$file" | sed '$d')

        # Has description field
        assert_contains "$frontmatter" "description:" "${cmd}: has description field"

        # Has argument-hint field
        assert_contains "$frontmatter" "argument-hint:" "${cmd}: has argument-hint field"

        # No forbidden YAML fields
        for field in "${FORBIDDEN_YAML_FIELDS[@]}"; do
            assert_not_contains "$frontmatter" "$field" "${cmd}: no forbidden field ${field}"
        done
    done
}

# ============================================================================
# TEST: Section Structure
# ============================================================================

test_section_structure() {
    test_section "Section Structure (11 sections per file)"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        local file="$COMMANDS_DIR/${cmd}.md"
        [[ ! -f "$file" ]] && continue

        local content
        content=$(cat "$file")

        for section in "${REQUIRED_SECTIONS[@]}"; do
            if echo "$content" | grep -qF "$section"; then
                ((TEST_TOTAL_COUNT++)) || true
                ((TEST_PASS_COUNT++)) || true
                echo -e "  ${GREEN}✓${NC} ${cmd}: has '${section}'"
            else
                ((TEST_TOTAL_COUNT++)) || true
                ((TEST_FAIL_COUNT++)) || true
                echo -e "  ${RED}✗${NC} ${cmd}: missing '${section}'"
            fi
        done
    done
}

# ============================================================================
# TEST: Working Protocol Reference
# ============================================================================

test_working_protocol() {
    test_section "Working Protocol Reference"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        local file="$COMMANDS_DIR/${cmd}.md"
        [[ ! -f "$file" ]] && continue

        local content
        content=$(cat "$file")
        assert_contains "$content" "cf-working-protocol" "${cmd}: references cf-working-protocol"
    done
}

# ============================================================================
# TEST: Forbidden Terms
# ============================================================================

test_forbidden_terms() {
    test_section "Forbidden Terms"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        local file="$COMMANDS_DIR/${cmd}.md"
        [[ ! -f "$file" ]] && continue

        local content
        content=$(cat "$file")
        local content_lower
        content_lower=$(echo "$content" | tr '[:upper:]' '[:lower:]')

        for term in "${FORBIDDEN_TERMS[@]}"; do
            local term_lower
            term_lower=$(echo "$term" | tr '[:upper:]' '[:lower:]')
            assert_not_contains "$content_lower" "$term_lower" "${cmd}: no forbidden term '${term}'"
        done
    done
}

# ============================================================================
# TEST: Cross-References
# ============================================================================

test_cross_references() {
    test_section "Cross-References"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        local file="$COMMANDS_DIR/${cmd}.md"
        [[ ! -f "$file" ]] && continue

        # Extract referenced command files (./cf-*.md pattern)
        local refs
        refs=$(grep -oE '\./cf-[a-z-]+\.md' "$file" 2>/dev/null || true)

        for ref in $refs; do
            local ref_file
            ref_file="$COMMANDS_DIR/$(basename "$ref")"
            if [[ -f "$ref_file" ]]; then
                ((TEST_TOTAL_COUNT++)) || true
                ((TEST_PASS_COUNT++)) || true
                echo -e "  ${GREEN}✓${NC} ${cmd}: ref ${ref} resolves"
            else
                ((TEST_TOTAL_COUNT++)) || true
                ((TEST_FAIL_COUNT++)) || true
                echo -e "  ${RED}✗${NC} ${cmd}: ref ${ref} not found"
            fi
        done

        # Extract referenced agent files (../agents/cf-*.md pattern)
        local agent_refs
        agent_refs=$(grep -oE '\.\./agents/cf-[a-z-]+\.md' "$file" 2>/dev/null || true)

        for ref in $agent_refs; do
            local agent_file
            agent_file="$REPO_ROOT/.claude/agents/$(basename "$ref")"
            if [[ -f "$agent_file" ]]; then
                ((TEST_TOTAL_COUNT++)) || true
                ((TEST_PASS_COUNT++)) || true
                echo -e "  ${GREEN}✓${NC} ${cmd}: agent ref $(basename "$ref") resolves"
            else
                ((TEST_TOTAL_COUNT++)) || true
                ((TEST_FAIL_COUNT++)) || true
                echo -e "  ${RED}✗${NC} ${cmd}: agent ref $(basename "$ref") not found"
            fi
        done
    done
}

# ============================================================================
# TEST: CLAUDE.md Updates
# ============================================================================

test_claude_md_updates() {
    test_section "CLAUDE.md Updates"

    local claude_md="$REPO_ROOT/.claude/CLAUDE.md"
    assert_file_exists "$claude_md" "CLAUDE.md exists"

    local content
    content=$(cat "$claude_md")

    assert_contains "$content" "Stage-Gated Availability" "CLAUDE.md has Stage-Gated Availability table"
    assert_contains "$content" "14 slash command definitions" "CLAUDE.md has updated commands directory description"
}

# ============================================================================
# TEST: Section Separator Count
# ============================================================================

test_separator_count() {
    test_section "Section Separator Count"

    for cmd in "${EXPECTED_COMMANDS[@]}"; do
        local file="$COMMANDS_DIR/${cmd}.md"
        [[ ! -f "$file" ]] && continue

        local count
        count=$(grep -c '^---$' "$file")
        assert_equals "12" "$count" "${cmd}: has 12 --- separators (2 frontmatter + 10 sections)"
    done
}

# ============================================================================
# MAIN
# ============================================================================

main() {
    # Parse arguments
    case "${1:-}" in
        -h|--help) usage; exit 0 ;;
        -V|--version) echo "$SCRIPT_NAME v$SCRIPT_VERSION"; exit 0 ;;
    esac

    echo "Phase 5 Command File Tests"
    echo "========================="

    test_file_existence
    test_yaml_frontmatter
    test_section_structure
    test_working_protocol
    test_forbidden_terms
    test_cross_references
    test_claude_md_updates
    test_separator_count

    print_test_summary
    exit "$TEST_FAIL_COUNT"
}

main "$@"
