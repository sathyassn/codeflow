#!/usr/bin/env bash
# Test: PathFlow task tracker configuration in pathflow-config.json
# Location: .codeflow/testing/consistency/test-pathflow-task-tracker-config.sh
#
# Validates that pathflow-config.json has:
#   1. Required mirroring configuration fields
#   2. Inline template properties (subject, description, activeForm) on all 7 phases
#   3. Inline template properties (subject, description, activeForm) on all 6 stages
#   4. No separate phase_templates or stage_templates sections (consolidated)
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

assert_success "jq -e '.task_tracker.mirroring.mirror_target == \"Claude Code internal task tracker (TaskCreate/TaskUpdate API tools)\"' '$CONFIG_FILE'" \
    "mirroring.mirror_target is 'Claude Code internal task tracker (TaskCreate/TaskUpdate API tools)'"

# ============================================================================
# TEST 3: No separate template sections (consolidated)
# ============================================================================

test_section "Template Consolidation"

if jq -e '.task_tracker.phase_templates' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_fail "phase_templates removed from task_tracker (consolidated into phases)"
else
    test_pass "phase_templates removed from task_tracker (consolidated into phases)"
fi

if jq -e '.task_tracker.stage_templates' "$CONFIG_FILE" >/dev/null 2>&1; then
    test_fail "stage_templates removed from task_tracker (consolidated into stages)"
else
    test_pass "stage_templates removed from task_tracker (consolidated into stages)"
fi

# ============================================================================
# TEST 4: Phase inline template properties - all 7 phases present
# ============================================================================

test_section "Phase Inline Properties"

readonly EXPECTED_PHASES="PF1-INIT PF2-CONTEXT PF3-CLASSIFY PF4-EXECUTE PF5-VERIFY PF6-COMPLETE PF7-END"

for phase in $EXPECTED_PHASES; do
    if jq -e ".phases.\"$phase\"" "$CONFIG_FILE" >/dev/null 2>&1; then
        test_pass "Phase exists: $phase"
    else
        test_fail "Phase exists: $phase"
    fi
done

# Count phases to ensure no extras or missing
PHASE_COUNT=$(jq '.phases | keys | length' "$CONFIG_FILE" 2>/dev/null)
assert_equals "7" "$PHASE_COUNT" "Exactly 7 phases present"

# ============================================================================
# TEST 5: Phase inline template required fields
# ============================================================================

test_section "Phase Template Fields (inline)"

for phase in $EXPECTED_PHASES; do
    for field in subject description activeForm; do
        VALUE=$(jq -r ".phases.\"$phase\".\"$field\" // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -n "$VALUE" ]]; then
            test_pass "$phase has inline $field"
        else
            test_fail "$phase has inline $field"
        fi
    done
done

# ============================================================================
# TEST 6: Stage inline template properties - all 6 stages present
# ============================================================================

test_section "Stage Inline Properties"

readonly EXPECTED_STAGES="WS-DEV WS-PLAN WS-DOCS WS-TEST WS-REV WS-QA"

for stage in $EXPECTED_STAGES; do
    if jq -e ".stages.\"$stage\"" "$CONFIG_FILE" >/dev/null 2>&1; then
        test_pass "Stage exists: $stage"
    else
        test_fail "Stage exists: $stage"
    fi
done

# Count stages
STAGE_COUNT=$(jq '.stages | keys | length' "$CONFIG_FILE" 2>/dev/null)
assert_equals "6" "$STAGE_COUNT" "Exactly 6 stages present"

# ============================================================================
# TEST 7: Stage inline template required fields
# ============================================================================

test_section "Stage Template Fields (inline)"

for stage in $EXPECTED_STAGES; do
    for field in subject description activeForm; do
        VALUE=$(jq -r ".stages.\"$stage\".\"$field\" // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -n "$VALUE" ]]; then
            test_pass "$stage has inline $field"
        else
            test_fail "$stage has inline $field"
        fi
    done
done

# ============================================================================
# TEST 8: Phase descriptions reference phase_id placeholder
# ============================================================================

test_section "Template Placeholders"

PHASES_WITH_PHASE_ID=0
for phase in $EXPECTED_PHASES; do
    DESC=$(jq -r ".phases.\"$phase\".description // empty" "$CONFIG_FILE" 2>/dev/null)
    if [[ "$DESC" == *"{phase_id}"* ]]; then
        ((PHASES_WITH_PHASE_ID++)) || true
    fi
done

if [[ "$PHASES_WITH_PHASE_ID" -eq 7 ]]; then
    test_pass "All phase descriptions reference {phase_id} placeholder"
else
    test_fail "All phase descriptions reference {phase_id} placeholder (found $PHASES_WITH_PHASE_ID/7)"
fi

STAGES_WITH_STAGE_ID=0
for stage in $EXPECTED_STAGES; do
    DESC=$(jq -r ".stages.\"$stage\".description // empty" "$CONFIG_FILE" 2>/dev/null)
    if [[ "$DESC" == *"{stage_id}"* ]]; then
        ((STAGES_WITH_STAGE_ID++)) || true
    fi
done

if [[ "$STAGES_WITH_STAGE_ID" -eq 6 ]]; then
    test_pass "All stage descriptions reference {stage_id} placeholder"
else
    test_fail "All stage descriptions reference {stage_id} placeholder (found $STAGES_WITH_STAGE_ID/6)"
fi

# ============================================================================
# TEST 9: Cross-section consistency
# ============================================================================

test_section "Cross-Section Consistency"

# Every phase in phases section should have inline template properties
PHASES_WITH_TEMPLATES=0
for phase in $EXPECTED_PHASES; do
    HAS_ALL="true"
    for field in subject description activeForm; do
        VALUE=$(jq -r ".phases.\"$phase\".\"$field\" // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -z "$VALUE" ]]; then
            HAS_ALL="false"
            break
        fi
    done
    if [[ "$HAS_ALL" == "true" ]]; then
        ((PHASES_WITH_TEMPLATES++)) || true
    fi
done

if [[ "$PHASES_WITH_TEMPLATES" -eq 7 ]]; then
    test_pass "All 7 phases have complete inline template properties"
else
    test_fail "All 7 phases have complete inline template properties (found $PHASES_WITH_TEMPLATES/7)"
fi

# Every stage in stages section should have inline template properties
STAGES_WITH_TEMPLATES=0
for stage in $EXPECTED_STAGES; do
    HAS_ALL="true"
    for field in subject description activeForm; do
        VALUE=$(jq -r ".stages.\"$stage\".\"$field\" // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -z "$VALUE" ]]; then
            HAS_ALL="false"
            break
        fi
    done
    if [[ "$HAS_ALL" == "true" ]]; then
        ((STAGES_WITH_TEMPLATES++)) || true
    fi
done

if [[ "$STAGES_WITH_TEMPLATES" -eq 6 ]]; then
    test_pass "All 6 stages have complete inline template properties"
else
    test_fail "All 6 stages have complete inline template properties (found $STAGES_WITH_TEMPLATES/6)"
fi

# ============================================================================
# TEST 10: Existing config sections remain intact
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
# TEST 11: PF6 squash task exists
# ============================================================================

test_section "PF6 Squash Task"

SQUASH_TASK=$(jq -r '.phases."PF6-COMPLETE".tasks[] | select(.operation == "squash-branch") | .id' "$CONFIG_FILE" 2>/dev/null)
if [[ -n "$SQUASH_TASK" ]]; then
    test_pass "PF6 has squash-branch task ($SQUASH_TASK)"
else
    test_fail "PF6 has squash-branch task"
fi

# Verify squash task is before create-pr task
SQUASH_ORDER=$(jq -r '.phases."PF6-COMPLETE".tasks[] | select(.operation == "squash-branch") | .task_order' "$CONFIG_FILE" 2>/dev/null)
PR_ORDER=$(jq -r '.phases."PF6-COMPLETE".tasks[] | select(.operation == "create-pr") | .task_order' "$CONFIG_FILE" 2>/dev/null)
if [[ -n "$SQUASH_ORDER" ]] && [[ -n "$PR_ORDER" ]] && [[ "$SQUASH_ORDER" -lt "$PR_ORDER" ]]; then
    test_pass "squash-branch (order $SQUASH_ORDER) is before create-pr (order $PR_ORDER)"
else
    test_fail "squash-branch is before create-pr (squash=$SQUASH_ORDER, pr=$PR_ORDER)"
fi

# Verify squash task is assigned to cf-git-operations
SQUASH_ASSIGNED=$(jq -r '.phases."PF6-COMPLETE".tasks[] | select(.operation == "squash-branch") | .assigned_to' "$CONFIG_FILE" 2>/dev/null)
assert_equals "cf-git-operations" "$SQUASH_ASSIGNED" "squash-branch assigned to cf-git-operations"

# ============================================================================
# TEST 12: enforcement_note on all tasks
# ============================================================================

test_section "Enforcement Note Validation"

# Count total tasks across all phases
TOTAL_TASKS=0
TASKS_WITH_NOTE=0
TASKS_UNCONDITIONAL=0
TASKS_CONDITIONAL=0
TASKS_MISSING_NOTE=0

for phase in $EXPECTED_PHASES; do
    TASK_COUNT=$(jq -r ".phases.\"$phase\".tasks | length" "$CONFIG_FILE" 2>/dev/null)
    for i in $(seq 0 $((TASK_COUNT - 1))); do
        ((TOTAL_TASKS++)) || true
        # shellcheck disable=SC2034  # TASK_ID extracted for loop context; used implicitly in counting
        TASK_ID=$(jq -r ".phases.\"$phase\".tasks[$i].id" "$CONFIG_FILE" 2>/dev/null)
        NOTE=$(jq -r ".phases.\"$phase\".tasks[$i].enforcement_note // empty" "$CONFIG_FILE" 2>/dev/null)
        CONDITION=$(jq -r ".phases.\"$phase\".tasks[$i].condition // empty" "$CONFIG_FILE" 2>/dev/null)

        if [[ -n "$NOTE" ]]; then
            ((TASKS_WITH_NOTE++)) || true
            if [[ -n "$CONDITION" ]]; then
                ((TASKS_CONDITIONAL++)) || true
            else
                ((TASKS_UNCONDITIONAL++)) || true
            fi
        else
            ((TASKS_MISSING_NOTE++)) || true
        fi
    done
done

# All 30 tasks must have enforcement_note
assert_equals "30" "$TOTAL_TASKS" "Exactly 30 tasks across all phases"
assert_equals "30" "$TASKS_WITH_NOTE" "All 30 tasks have enforcement_note"
assert_equals "0" "$TASKS_MISSING_NOTE" "No tasks missing enforcement_note"

# 28 unconditional + 2 conditional
assert_equals "28" "$TASKS_UNCONDITIONAL" "28 unconditional tasks"
assert_equals "2" "$TASKS_CONDITIONAL" "2 conditional tasks"

# Verify unconditional notes contain "MUST execute" or "MUST spawn"
test_section "Enforcement Note Content"

MUST_EXECUTE_COUNT=0
for phase in $EXPECTED_PHASES; do
    TASK_COUNT=$(jq -r ".phases.\"$phase\".tasks | length" "$CONFIG_FILE" 2>/dev/null)
    for i in $(seq 0 $((TASK_COUNT - 1))); do
        CONDITION=$(jq -r ".phases.\"$phase\".tasks[$i].condition // empty" "$CONFIG_FILE" 2>/dev/null)
        NOTE=$(jq -r ".phases.\"$phase\".tasks[$i].enforcement_note // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -z "$CONDITION" ]] && [[ "$NOTE" == *"MUST execute"* || "$NOTE" == *"MUST spawn"* ]]; then
            ((MUST_EXECUTE_COUNT++)) || true
        fi
    done
done
assert_equals "28" "$MUST_EXECUTE_COUNT" "All 28 unconditional notes contain MUST execute/spawn"

# Verify conditional notes contain "condition field"
CONDITION_FIELD_COUNT=0
for phase in $EXPECTED_PHASES; do
    TASK_COUNT=$(jq -r ".phases.\"$phase\".tasks | length" "$CONFIG_FILE" 2>/dev/null)
    for i in $(seq 0 $((TASK_COUNT - 1))); do
        CONDITION=$(jq -r ".phases.\"$phase\".tasks[$i].condition // empty" "$CONFIG_FILE" 2>/dev/null)
        NOTE=$(jq -r ".phases.\"$phase\".tasks[$i].enforcement_note // empty" "$CONFIG_FILE" 2>/dev/null)
        if [[ -n "$CONDITION" ]] && [[ "$NOTE" == *"condition field"* ]]; then
            ((CONDITION_FIELD_COUNT++)) || true
        fi
    done
done
assert_equals "2" "$CONDITION_FIELD_COUNT" "Both conditional notes contain 'condition field'"

# Verify specific conditional tasks are PF4-TSK-01 and PF4-TSK-07
COND_TASK_IDS=$(jq -r '.phases | to_entries[] | .value.tasks[] | select(.condition != null) | .id' "$CONFIG_FILE" 2>/dev/null | sort)
EXPECTED_COND_IDS=$(echo -e "PF4-TSK-01\nPF4-TSK-07" | sort)
if [[ "$COND_TASK_IDS" == "$EXPECTED_COND_IDS" ]]; then
    test_pass "Conditional tasks are PF4-TSK-01 and PF4-TSK-07"
else
    test_fail "Conditional tasks should be PF4-TSK-01 and PF4-TSK-07 (got: $COND_TASK_IDS)"
fi

# ============================================================================
# SUMMARY
# ============================================================================

print_test_summary

if [[ $TEST_FAIL_COUNT -gt 0 ]]; then
    exit 1
fi
exit 0
