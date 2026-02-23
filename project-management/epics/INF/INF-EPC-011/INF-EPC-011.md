---
id: "epic-01KJ45Y34AQSFDJADG4AWWN67T"
format_id: "INF-EPC-011"
title: "Infrastructure Chores (ongoing)"
summary: "Ongoing infrastructure maintenance tasks: config clarity fixes, tooling improvements, and operational housekeeping that do not warrant standalone epics."
status: in_progress
area_type: "INF"
work_type: "CHOR"
domain: "ops"
is_ongoing: true
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-23T03:00:00Z"
updated_at: "2026-02-23T03:00:00Z"
---

# INF-EPC-011: Infrastructure Chores (ongoing)

## Summary

Ongoing infrastructure maintenance tasks: config clarity fixes, tooling improvements, and operational housekeeping that do not warrant standalone epics. Tasks are added here as they arise; the epic remains open until no active tasks exist.

## Scope

### In Scope

- pathflow-config.json and CLAUDE.md consistency fixes
- Agent definition updates (non-feature)
- Validation script fixes
- Operational housekeeping

### Out of Scope

- New features (use INF-EPC-FEAT-GENL-001)
- Refactors (use INF-EPC-RFCT-GENL-001)

## Acceptance Criteria

- [ ] All tasks in this epic reach `complete` status

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-011-001 | Fix pathflow-config task clarity and PF6-TSK-06 breakdown | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Replaces the legacy `INF-EPC-CHOR-GENL-001` epic which used a non-standard id/format_id pattern predating the `{AREA}-EPC-{NNN}` convention.

## Related

- Legacy epic: INF-EPC-CHOR-GENL-001 (non-standard ids, retained in DB for history)
