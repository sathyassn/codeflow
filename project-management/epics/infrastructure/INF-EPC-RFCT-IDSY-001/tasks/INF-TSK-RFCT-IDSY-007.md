---
id: task-01KH9V2G3XTE0Q82HFH0MRT41Z
format_id: INF-TSK-RFCT-IDSY-007
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "V4 Spec: Schema definitions (10 items across 3 files)"
description: Update 10 schema definitions in V4 specification to add format_id columns and clarify ULID PKs
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: []
scope_policy: soft
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["All 10 V4 schema definitions updated with format_id column", "ULID PK format documented in id column comments", "UNIQUE indexes specified for format_id"]
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-12T00:00:00Z
updated_at: 2026-02-12T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-RFCT-IDSY-007: V4 Spec - Schema definitions

## Description

Update all schema definitions in the V4 specification repository to include format_id columns and clarify ULID PK format. This task is in a **separate repository** (codeflow-specification-v4).

### Changes (10 items from findings)

| Finding ID | File | Lines | Change |
|------------|------|-------|--------|
| V4-SCH-01 | 11-reference/database/db-schema.md | 150-176 | epics table: add format_id |
| V4-SCH-02 | 11-reference/database/db-schema.md | 192-253 | tasks table: add format_id |
| V4-SCH-03 | 11-reference/database/db-schema.md | 1513-1517 | ID Formats reference table |
| V4-SCH-04 | 11-reference/database/db-schema.md | new | UNIQUE indexes on format_id |
| V4-SCH-05 | 11-reference/database/lifecycle/04-work-graph-tables.md | 28-47 | epics schema |
| V4-SCH-06 | 11-reference/database/lifecycle/04-work-graph-tables.md | 145-193 | tasks schema |
| V4-SCH-07 | 04-knowledge-layer/01-work-graph.md | 216-243 | epics schema |
| V4-SCH-08 | 04-knowledge-layer/01-work-graph.md | 283-334 | tasks schema |
| V4-SCH-09 | 10-implementation/phase-2-knowledge-layer.md | 68-89 | epics schema |
| V4-SCH-10 | 10-implementation/phase-2-knowledge-layer.md | 91-122 | tasks schema |

## Dependencies

### Blocked By

- None (spec can be updated independently)

### Blocks

- INF-TSK-RFCT-IDSY-008 (events reference schema)
- INF-TSK-RFCT-IDSY-009 (docs reference schema)

## Notes

- **Repository**: codeflow-specification-v4 (NOT this repo)
- Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Section 3.1
