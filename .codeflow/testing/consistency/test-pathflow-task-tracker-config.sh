#!/usr/bin/env bash
# Test: PathFlow task_tracker configuration in pathflow-config.json
# Location: .codeflow/testing/consistency/test-pathflow-task-tracker-config.sh
#
# Validates that the task_tracker section in pathflow-config.json has:
#   1. Required mirroring configuration fields
#   2. Templates for all 7 PathFlow phases (PF1-PF7)
#   3. Templates for all 6 work stages (WS-DEV, WS-PLAN, WS-DOCS, WS-TEST, WS-REV, WS-QA)
#   4. Required fields (subject, description, activeForm) in each template
#
# Exit codes:
#   0 - All validation checks passed
#   1 - One or more checks failed

set -euo pipefail

# ============================================================================
# SETUP
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

# Source test helpers
if [[ -f "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh" ]]; then
    source "$REPO_ROOT/.codeflow/testing/lib/test-helpers.sh"
else
    echo "ERROR: test-helpers.sh not found" >&2
    exit 1
fi

# Path to config
readonly CONFIG_FILE="$REPO_ROOT/.codeflow/config/pathflow/pathflow-config.json"

echo ""
echo "=== PathFlow Task Tracker Config Test ==="
echo ""

# ============================================================================
# PREREQUISITE CHECKS
# ============================================================================

test_section "Prerequisites"

if [[ ! -f "$CONFIG_FILE" ]]; then
    test_fail "pathflow-config.json exists"
    print_test_summary
    exit 1
fi
test_pass "pathflow-config.json exists"

if ! command -v jq &>/dev/null; then
    test_fail "jq is available"
    print_test_summary
    exit 1
fi
test_pass "jq is available"

# Validate JSON syntax
if ! jq empty "$CONFIG_FILE" 2>/dev/null; then
    test_fail "pathflow-config.json is valid JSON"
    print_test_summary
    exit 1
fi
test_pass "pathflow-config.json is valid JSON"

# ============================================================================
# TEST 1: task_tracker section exists
# ============================================================================

test_section "Task Tracker Section"

if jq -e '.task_tracker' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_pass "task_tracker section exists"
else
    test_fail "task_tracker section exists"
    print_test_summary
    exit 1
fi

# ============================================================================
# TEST 2: Mirroring configuration
# ============================================================================

test_section "Mirroring Configuration"

assert_success "jq -e '.task_tracker.mirroring' '$CONFIG_FILE'" \
    "mirroring config object exists"

assert_success "jq -e '.task_tracker.mirroring.enabled == true' '$CONFIG_FILE'" \
    "mirroring.enabled is true"

assert_success "jq -e '.task_tracker.mirroring.source_of_truth == \"jsonl\"' '$CONFIG_FILE'" \
    "mirroring.source_of_truth is 'jsonl'"

assert_success "jq -e '.task_tracker.mirroring.mirror_target == \"task_tracker\"' '$CONFIG_FILE'" \
    "mirroring.mirror_target is 'task_tracker'"

# ============================================================================
# TEST 3: Phase templates - all 7 phases present
# ============================================================================

test_section "Phase Templates"

assert_success "jq -e '.task_tracker.phase_templates' '$CONFIG_FILE'" \
    "phase_templates object exists"

readonly EXPECTED_PHASES="PF1-INIT PF2-CONTEXT PF3-CLASSIFY PF4-EXECUTE PF5-VERIFY PF6-COMPLETE PF7-END"

for phase in $EXPECTED_PHASES; do
    if jq -e ".task_tracker.phase_templates.\"$phase\"" "$CONFIG_FILE" >/dev/null 2>&1; then
        test_pass "Phase template exists: $phase"
    else
        test_fail "Phase template exists: $phase"
    fi
done

# Count phases to ensure no extras or missing
PHASE_COUNT=$(jq '.task_tracker.phase_templates | keys | length' "$CONFIG_FILE" 2>/dev/null)
assert_equals "7" "$PHASE_COUNT" "Exactly 7 phase templates present"

# ============================================================================
# TEST 4: Phase template required fields
# ============================================================================

test_section "Phase Template Fields"

for phase in $EXPECTED_PHASES; do
    for field in subject description activeForm; do
        VALUE=$(jq -r ".task_tracker.phase_templates.\"$phase\".\"$field\" // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -n "$VALUE" ]]; then
            test_pass "$phase has $field"
        else
            test_fail "$phase has $field"
        fi
    done
done

# ============================================================================
# TEST 5: Stage templates - all 6 stages present
# ============================================================================

test_section "Stage Templates"

assert_success "jq -e '.task_tracker.stage_templates' '$CONFIG_FILE'" \
    "stage_templates object exists"

readonly EXPECTED_STAGES="WS-DEV WS-PLAN WS-DOCS WS-TEST WS-REV WS-QA"

for stage in $EXPECTED_STAGES; do
    if jq -e ".task_tracker.stage_templates.\"$stage\"" "$CONFIG_FILE" >/dev/null 2>&1; then
        test_pass "Stage template exists: $stage"
    else
        test_fail "Stage template exists: $stage"
    fi
done

# Count stages
STAGE_COUNT=$(jq '.task_tracker.stage_templates | keys | length' "$CONFIG_FILE" 2>/dev/null)
assert_equals "6" "$STAGE_COUNT" "Exactly 6 stage templates present"

# ============================================================================
# TEST 6: Stage template required fields
# ============================================================================

test_section "Stage Template Fields"

for stage in $EXPECTED_STAGES; do
    for field in subject description activeForm; do
        VALUE=$(jq -r ".task_tracker.stage_templates.\"$stage\".\"$field\" // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -n "$VALUE" ]]; then
            test_pass "$stage has $field"
        else
            test_fail "$stage has $field"
        fi
    done
done

# ============================================================================
# TEST 7: Phase templates reference phase_id placeholder
# ============================================================================

test_section "Template Placeholders"

PHASES_WITH_PHASE_ID=0
for phase in $EXPECTED_PHASES; do
    DESC=$(jq -r ".task_tracker.phase_templates.\"$phase\".description // empty" "$CONFIG_FILE" 2>/dev/null)
    if [[ "$DESC" == *"{phase_id}"* ]]; then
        ((PHASES_WITH_PHASE_ID++)) || true
    fi
done

if [[ "$PHASES_WITH_PHASE_ID" -eq 7 ]]; then
    test_pass "All phase templates reference {phase_id} placeholder"
else
    test_fail "All phase templates reference {phase_id} placeholder (found $PHASES_WITH_PHASE_ID/7)"
fi

STAGES_WITH_STAGE_ID=0
for stage in $EXPECTED_STAGES; do
    DESC=$(jq -r ".task_tracker.stage_templates.\"$stage\".description // empty" "$CONFIG_FILE" 2>/dev/null)
    if [[ "$DESC" == *"{stage_id}"* ]]; then
        ((STAGES_WITH_STAGE_ID++)) || true
    fi
done

if [[ "$STAGES_WITH_STAGE_ID" -eq 6 ]]; then
    test_pass "All stage templates reference {stage_id} placeholder"
else
    test_fail "All stage templates reference {stage_id} placeholder (found $STAGES_WITH_STAGE_ID/6)"
fi

# ============================================================================
# TEST 8: Consistency with phases and stages sections
# ============================================================================

test_section "Cross-Section Consistency"

# Every phase in phases section should have a task_tracker template
PHASES_IN_PHASES=$(jq -r '.phases | keys[]' "$CONFIG_FILE" 2>/dev/null | sort)
PHASES_IN_TEMPLATES=$(jq -r '.task_tracker.phase_templates | keys[]' "$CONFIG_FILE" 2>/dev/null | sort)

if [[ "$PHASES_IN_PHASES" == "$PHASES_IN_TEMPLATES" ]]; then
    test_pass "Phase templates match phases section keys"
else
    test_fail "Phase templates match phases section keys"
fi

# Every stage in stages section should have a task_tracker template
STAGES_IN_STAGES=$(jq -r '.stages | keys[]' "$CONFIG_FILE" 2>/dev/null | sort)
STAGES_IN_TEMPLATES=$(jq -r '.task_tracker.stage_templates | keys[]' "$CONFIG_FILE" 2>/dev/null | sort)

if [[ "$STAGES_IN_STAGES" == "$STAGES_IN_TEMPLATES" ]]; then
    test_pass "Stage templates match stages section keys"
else
    test_fail "Stage templates match stages section keys"
fi

# ============================================================================
# TEST 9: Existing config sections remain intact
# ============================================================================

test_section "Existing Config Integrity"

assert_success "jq -e '.version' '$CONFIG_FILE'" \
    "version field still present"

assert_success "jq -e '.phases' '$CONFIG_FILE'" \
    "phases section still present"

assert_success "jq -e '.stages' '$CONFIG_FILE'" \
    "stages section still present"

assert_success "jq -e '.pipelines' '$CONFIG_FILE'" \
    "pipelines section still present"

assert_success "jq -e '.teammates' '$CONFIG_FILE'" \
    "teammates section still present"

assert_success "jq -e '.rework' '$CONFIG_FILE'" \
    "rework section still present"

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
