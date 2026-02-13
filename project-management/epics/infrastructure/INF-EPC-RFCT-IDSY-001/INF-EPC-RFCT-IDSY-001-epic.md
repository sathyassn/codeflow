---
id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
format_id: INF-EPC-RFCT-IDSY-001
title: "Dual-ID System: ULID Primary Keys with Human-Readable Format IDs"
summary: Implement dual-ID system across schema, scripts, skills, and tests - every work graph table uses {prefix}-{ulid} as PK with format_id (TEXT UNIQUE NOT NULL) for human-readable identification
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
priority: high
is_ongoing: false
file_scope: [".codeflow/scripts/db/schema.sql", ".codeflow/scripts/shell-lib/", ".codeflow/scripts/codeflow_py_lib/", ".codeflow/scripts/state/", ".claude/skills/"]
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-12T00:00:00Z
updated_at: 2026-02-12T00:00:00Z
---

# INF-EPC-RFCT-IDSY-001: Dual-ID System

## Summary

Implement the confirmed dual-ID architecture (Option B) across the entire CodeFlow system. Every work graph table (epics, tasks) uses `{prefix}-{ulid}` as PRIMARY KEY with a `format_id` column (TEXT UNIQUE NOT NULL) for human-readable identification.

### Design

```text
Epic PK:     epic-{ulid}                         (e.g., epic-01ARZ3NDEKTSV4RRFFQ69G5FAV)
Epic FID:    {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}    (e.g., FRT-EPC-FEAT-AUTH-001)
Task PK:     task-{ulid}                         (e.g., task-01BRZ4PDFLUTW5SSGG70H6GBW)
Task FID:    {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}    (e.g., FRT-TSK-FEAT-AUTH-001)
```

### Usage Rules

| Context | Use id (ULID PK) | Use format_id |
|---------|-------------------|---------------|
| Database FK references | Always | Never |
| Internal lookups/joins | Always | Never |
| User-facing display | Never | Always |
| File/folder names | Never | Always |
| Commit messages | Never | Always |
| Branch names | Never | Always |

## Scope

### In Scope

- Database schema: Add format_id columns to epics/tasks tables
- Shell scripts: Update ID generation and validation in ulid.sh, validation.sh
- Python scripts: Update validation.py patterns
- State scripts: Update work-state.sh, ledger.sh to handle dual IDs
- Skill documentation: Update 8 skill files for dual-ID usage
- Tests: Update all test files to use correct ID formats
- V4 specification: Update schema, events, validation specs (separate repo)

### Out of Scope

- Non-work-graph tables (memory, sessions, logs) - these already use correct ULID PKs
- Migration of existing data (handled by migration script task)

## Acceptance Criteria

- [ ] schema.sql has format_id columns on epics and tasks tables with UNIQUE indexes
- [ ] Shell generate_epic_id/generate_task_id produce `{prefix}-{ulid}` format
- [ ] Shell/Python validators distinguish ULID PKs from format IDs
- [ ] State scripts (work-state.sh, ledger.sh) accept both ID types
- [ ] All skill documentation reflects dual-ID usage
- [ ] All tests updated and passing
- [ ] V4 spec updated (10 schema definitions, 5 event schemas, 3 validation specs)

## Tasks

| ID | Title | Status | Phase | Depends On |
|----|-------|--------|-------|------------|
| INF-TSK-RFCT-IDSY-001 | Schema: Add format_id to epics/tasks tables | todo | 1 | - |
| INF-TSK-RFCT-IDSY-002 | ID Generation: Shell + Python validators | todo | 2 | Phase 1 |
| INF-TSK-RFCT-IDSY-003 | Business Logic: State and coordination scripts | todo | 3 | Phase 2 |
| INF-TSK-RFCT-IDSY-004 | Skill Documentation: Update 8 skill files | todo | 5 | Phase 2 |
| INF-TSK-RFCT-IDSY-005 | Tests: Update all test files for dual-ID | todo | 6 | Phase 3 |
| INF-TSK-RFCT-IDSY-006 | Migration: Scripts and project management | todo | 7 | Phase 3 |
| INF-TSK-RFCT-IDSY-007 | V4 Spec: Schema definitions (10 items) | todo | 1 | - |
| INF-TSK-RFCT-IDSY-008 | V4 Spec: Events and validation (8 items) | todo | 4 | Phase 1 |
| INF-TSK-RFCT-IDSY-009 | V4 Spec: Documentation and queries (12+ items) | todo | 7 | Phase 1-3 |

## Dependencies

### Blocked By

- None (design confirmed)

### Blocks

- Phase 4 V4 implementation (agent definitions use task IDs)

## Technical Notes

- Research findings: `.codeflow/docs/research/dual-id-system-findings.md`
- Decision: Tier 3 project-wide architectural change
- Design confirmed: Option B (ULID PK + format_id UNIQUE)
- Total scope: 65 items across 46+ files in 2 repos

## Related

- INF-EPC-FEAT-GENL-001 (ongoing infrastructure - where research was initiated)
- INF-TSK-FIX-GENL-005 (pre-phase4-gaps branch - where audit was performed)
