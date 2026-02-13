---
id: task-01KH9V2G1PAFW5CYV9H3TX8CC8
format_id: INF-TSK-RFCT-IDSY-001
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "Schema: Add format_id to epics/tasks tables"
description: Add format_id TEXT UNIQUE NOT NULL column to epics and tasks tables in schema.sql, update FK comments, add UNIQUE indexes
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: [".codeflow/scripts/db/schema.sql"]
scope_policy: strict
scope_root: null
estimate: S
priority: high
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["epics table has format_id TEXT UNIQUE NOT NULL column", "tasks table has format_id TEXT UNIQUE NOT NULL column", "id column comments clarify ULID PK format", "FK references documented as ULID-based", "UNIQUE indexes on format_id columns"]
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

# INF-TSK-RFCT-IDSY-001: Schema - Add format_id to epics/tasks tables

## Description

Add `format_id` column to the `epics` and `tasks` tables in `schema.sql`. This is the foundational change for the dual-ID system.

### Changes (3 items from findings)

| Finding ID | Location | Change |
|------------|----------|--------|
| CF-SCH-01 | schema.sql L110-142 | epics: add `format_id TEXT UNIQUE NOT NULL`, update id comment |
| CF-SCH-02 | schema.sql L144-202 | tasks: add `format_id TEXT UNIQUE NOT NULL`, update id comment |
| CF-SCH-03 | schema.sql L204-227 | Update FK comment documentation |

### Schema Changes

```sql
-- epics table: add after id column
format_id TEXT UNIQUE NOT NULL,  -- Human-readable: {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}

-- tasks table: add after id column
format_id TEXT UNIQUE NOT NULL,  -- Human-readable: {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}

-- Update id column comments
id TEXT PRIMARY KEY NOT NULL,    -- ULID format: epic-{ulid} or task-{ulid}
```

## Approach

1. Read current schema.sql
2. Add format_id column after id in both tables
3. Update id column comments to clarify ULID format
4. Update FK reference comments
5. Verify schema is valid SQL

## Files

### To Modify

- `.codeflow/scripts/db/schema.sql` - Add format_id columns and indexes

## Dependencies

### Blocked By

- None

### Blocks

- INF-TSK-RFCT-IDSY-002 (ID generation needs schema to reference)
- INF-TSK-RFCT-IDSY-003 (business logic needs schema)

## Verification

### Automated

- [ ] schema.sql is valid SQL (sqlite3 syntax check)
- [ ] format_id columns exist on epics and tasks tables
- [ ] UNIQUE indexes on format_id columns

## Notes

Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Section 4.1
