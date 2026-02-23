---
id: "epic-01KJ4622VJW18Y40SZHPN4QMDQ"
format_id: "INF-EPC-012"
title: "PathFlow task clarity and PF6 breakdown"
summary: "Audit and fix pathflow-config.json task definitions for one-operation/one-teammate principle violations, including PF6-TSK-06 decomposition."
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "ops"
is_ongoing: false
file_scope:
  - ".codeflow/config/pathflow/pathflow-config.json"
  - ".claude/CLAUDE.md"
  - ".claude/agents/cf-git-operations.md"
  - ".claude/agents/cf-knowledge-layer.md"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-23T03:10:00Z"
updated_at: "2026-02-23T03:10:00Z"
---

# INF-EPC-012: PathFlow task clarity and PF6 breakdown

## Summary

Audit and fix pathflow-config.json task definitions for one-operation/one-teammate principle violations. Specifically breaks PF6-TSK-06 (verify-pr-and-sync) into discrete atomic tasks and renumbers/clarifies other PF phase tasks.

## Scope

### In Scope

- pathflow-config.json PF task renumbering and description clarity
- PF6-TSK-06 decomposition into 4 discrete tasks
- CLAUDE.md Section 4.2 PF6 step alignment
- Agent definition updates for PF task ID references (cf-git-operations.md, cf-knowledge-layer.md)

### Out of Scope

- validate-task.sh FORMAT_ID_PATTERN fix (separate follow-up)
- Any new PathFlow features

## Acceptance Criteria

- [x] PF1 tasks renumbered: PF1-TSK-03 → PF1-TSK-02
- [x] PF6-TSK-06 broken into 4 discrete tasks
- [x] CLAUDE.md PF6 execution steps match new task count
- [x] Each PF task has exactly one assigned_to and one operation
- [x] All existing tests pass

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-012-001 | Fix pathflow-config task clarity and PF6-TSK-06 breakdown | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Branch: chore/pf-task-clarity. Pipeline: WS-DEV → WS-REV → WS-QA (all passed).

## Related

- PR: chore/pf-task-clarity (pending)
- Previous: INF-EPC-002 (PF6-COMPLETE gaps, PR #58), INF-EPC-002 task INF-TSK-002-005 (verify-pr-and-sync fix, PR #59)
