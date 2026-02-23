---
id: "task-01KJ43RDQB86BKNZ4SRPFK0SH0"
format_id: "INF-TSK-CHOR-GENL-011"
epic_id: "INF-EPC-CHOR-GENL-001"
epic_format_id: "INF-EPC-CHOR-GENL-001"
title: "Fix pathflow-config task clarity and PF6-TSK-06 breakdown"
description: "Audit and fix pathflow-config.json tasks for one-operation/one-teammate principle violations. Break PF6-TSK-06 into separate logical tasks. Update CLAUDE.md, cf-git-operations.md, cf-knowledge-layer.md. Renumber PF1-TSK-03 to PF1-TSK-02. Clarify PF3-TSK-05, PF6-TSK-05 descriptions."
status: complete
area_type: INF
work_type: CHOR
domain: ops
origin: informal
file_scope:
  - ".codeflow/config/pathflow/pathflow-config.json"
  - ".claude/CLAUDE.md"
  - ".claude/agents/cf-git-operations.md"
  - ".claude/agents/cf-knowledge-layer.md"
scope_policy: soft
scope_root: null
estimate: S
priority: normal
assignee_id: null
autorun_eligible: false
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "pathflow-config.json PF1 tasks renumbered: PF1-TSK-03 removed or becomes PF1-TSK-02 (TeamCreate merged with pathflow-active)"
  - "PF6-TSK-06 (verify-pr-and-sync) broken into discrete subtasks in pathflow-config.json"
  - "CLAUDE.md PF6 execution steps updated to match new task breakdown"
  - "Each PF task has exactly one assigned_to and one operation"
  - "PF3-TSK-05 description clarified for begin-work begin_work event"
  - "PF6-TSK-05 description clarified for create-pr operation"
  - "cf-git-operations.md and cf-knowledge-layer.md updated if impacted by task renumbering"
  - "All existing tests pass (bash .codeflow/testing/run-all-tests.sh --mode standard)"
tests: []
branch: "chore/pf-task-clarity"
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-23T02:00:00Z"
updated_at: "2026-02-23T02:00:00Z"
started_at: "2026-02-23T02:00:00Z"
completed_at: null
stage: dev
stage_status: in_progress
stage_history: "[]"
---

# INF-TSK-CHOR-GENL-011: Fix pathflow-config task clarity and PF6-TSK-06 breakdown

## Description

The pathflow-config.json task definitions have accumulated clarity issues:

1. **PF1 task numbering gap**: PF1 currently has TSK-01, TSK-02, TSK-03 but the SessionStart hook auto-creates the pathflow-active flag, so PF1-TSK-03 (spawn cf-security) is effectively TSK-02. Renumber to eliminate gaps or clarify.
2. **PF6-TSK-06 is too coarse**: `verify-pr-and-sync` bundles multiple logical operations (poll CI, wait for merge, pull main, notify user) in one task, violating one-operation/one-teammate clarity.
3. **PF3-TSK-05 and PF6-TSK-05 descriptions** need clarification to precisely state what operation they invoke.
4. **CLAUDE.md Section 4.2** step descriptions for PF6-COMPLETE need to reflect any new task numbering.
5. **Agent definitions** (cf-git-operations.md, cf-knowledge-layer.md) may reference PF task IDs that need updating.

## Approach

1. Read pathflow-config.json thoroughly to understand all phase tasks
2. Identify all one-operation/one-teammate violations
3. Redesign PF6-TSK-06 breakdown (at minimum: poll-ci, wait-for-merge or auto-merge, pull-main)
4. Renumber PF1 if needed
5. Update pathflow-config.json
6. Update CLAUDE.md Section 4.2 (PF6-COMPLETE execution steps)
7. Update agent definitions where PF task IDs are referenced
8. Verify tests pass

## Files

### To Modify

- `.codeflow/config/pathflow/pathflow-config.json` -- task renumbering, PF6-TSK-06 breakdown, description clarity
- `.claude/CLAUDE.md` -- Section 4.2 PF6 steps updated to match new task numbering
- `.claude/agents/cf-git-operations.md` -- update any PF task ID references
- `.claude/agents/cf-knowledge-layer.md` -- update any PF task ID references

## Acceptance Criteria

1. pathflow-config.json PF1 tasks renumbered: PF1-TSK-03 removed or becomes PF1-TSK-02 (TeamCreate merged with pathflow-active)
2. PF6-TSK-06 (verify-pr-and-sync) broken into discrete subtasks in pathflow-config.json
3. CLAUDE.md PF6 execution steps updated to match new task breakdown
4. Each PF task has exactly one assigned_to and one operation
5. PF3-TSK-05 description clarified for begin-work begin_work event
6. PF6-TSK-05 description clarified for create-pr operation
7. cf-git-operations.md and cf-knowledge-layer.md updated if impacted by task renumbering
8. All existing tests pass (bash .codeflow/testing/run-all-tests.sh --mode standard)

## Dependencies

### Blocked By

- None

### Blocks

- None

## Verification

### Automated

- [ ] All existing tests pass: bash .codeflow/testing/run-all-tests.sh --mode standard

### Manual

- [ ] Reviewer confirms each PF task maps to exactly one operation and one teammate
- [ ] PF6 execution steps in CLAUDE.md match pathflow-config.json task count

## Notes

This is a documentation/config clarity task. No script logic changes expected. Main deliverable is updated pathflow-config.json with clear, atomic task definitions and consistent numbering.
