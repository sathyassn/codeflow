---
id: "epic-01KHSQPQRG63YAYS81SFSAEFPG"
format_id: "INF-EPC-002"
title: "Infrastructure Chores"
summary: "Routine infrastructure maintenance including file relocations, config restructuring, and workflow fixes"
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: "INF-EPC-CHOR-GENL-001"
external_url: null
created_at: "2026-02-13T00:00:00Z"
updated_at: "2026-02-17T00:00:00Z"
completed_at: "2026-02-17T00:00:00Z"
---

# INF-EPC-002: Infrastructure Chores

## Summary

Routine infrastructure maintenance including file relocations, config restructuring, and workflow fixes. This epic covered moving pathflow-events.jsonl to the correct location, fixing PF6-COMPLETE memory update flow, and restructuring pathflow-config with task_order objects.

## Scope

### In Scope

- File relocation (pathflow-events.jsonl from ledger to logs)
- PathFlow config restructuring
- PF6-COMPLETE workflow fixes

### Out of Scope

- New features or capabilities
- Agent definition changes

## Acceptance Criteria

- [x] pathflow-events.jsonl moved from ledger to logs
- [x] PF6-COMPLETE memory update flow fixed
- [x] pathflow-config restructured with task_order objects

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-002-001 | Move pathflow-events.jsonl from ledger to logs | complete | normal |
| INF-TSK-002-002 | Fix pre-PR memory update flow in PF6-COMPLETE | complete | normal |
| INF-TSK-002-003 | Restructure pathflow-config with task_order objects | complete | normal |
| INF-TSK-002-004 | Fix PF6-COMPLETE gaps: epic status rollup and PR event recording | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Old format ID: INF-EPC-CHOR-GENL-001. Migrated to rationalized format in PLN-TSK-001-001.

## Related

- Old task IDs: INF-TSK-CHOR-GENL-001, INF-TSK-CHOR-GENL-002, INF-TSK-CHOR-GENL-003
