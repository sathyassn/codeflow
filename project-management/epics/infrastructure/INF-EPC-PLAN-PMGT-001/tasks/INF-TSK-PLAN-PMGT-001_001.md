---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_001
epic_id: INF-EPC-PLAN-PMGT-001
title: "DB Schema Migration"
description: "Migrate live DB schema to include all V4 columns (acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch, stage, stage_status, stage_history), add QUAL and PMGT domain codes, and verify schema alignment with schema.sql definition."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: [".codeflow/scripts/db/schema.sql", ".codeflow/scripts/db/migrations/", ".state/db/codeflow.db"]
scope_policy: soft
scope_root: null
estimate: M
priority: high
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "tasks table has columns: acceptance (TEXT), tests (TEXT), auto_commit (BOOLEAN DEFAULT TRUE), raise_pr (BOOLEAN DEFAULT TRUE), auto_merge (BOOLEAN DEFAULT FALSE), target_branch (TEXT), stage (TEXT), stage_status (TEXT), stage_history (TEXT DEFAULT '[]')"
  - "domains table contains QUAL code with description 'Quality assurance and testing'"
  - "domains table contains PMGT code with description 'Project management'"
  - "All CHECK constraints from schema.sql are present on stage and stage_status columns"
  - "Indexes idx_tasks_stage and idx_tasks_autorun exist on tasks table"
  - "work_types table contains PLAN code with branch_prefix 'plan/' and commit_type 'plan'"
  - "sqlite3 .state/db/codeflow.db '.schema tasks' output matches schema.sql definition for all columns"
  - "Existing data preserved — zero row count change in epics and tasks tables after migration"
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-18T19:00:00Z
updated_at: 2026-02-18T19:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-PLAN-PMGT-001_001: DB Schema Migration

## Description

The live SQLite database (`.state/db/codeflow.db`) uses an older schema that is missing V4 columns added to `schema.sql`. This task creates and applies a migration to add the missing columns to the `tasks` table and registers two new domain codes (`QUAL`, `PMGT`) in the `domains` reference table.

### Missing Columns (tasks table)

| Column | Type | Default | Purpose |
|--------|------|---------|---------|
| acceptance | TEXT | NULL | JSON array of acceptance criteria |
| tests | TEXT | NULL | JSON array of test references |
| auto_commit | BOOLEAN | TRUE | Auto-commit after WS-DEV |
| raise_pr | BOOLEAN | TRUE | Create PR automatically |
| auto_merge | BOOLEAN | FALSE | Auto-merge after QA pass |
| target_branch | TEXT | NULL | Override default target branch |
| stage | TEXT | NULL | Current execution stage (dev/work/review/qa/done) |
| stage_status | TEXT | NULL | Stage execution status (pending/in_progress/complete/failed) |
| stage_history | TEXT | '[]' | JSON array of stage transition records |

### Missing Domain Codes

| Code | Description |
|------|-------------|
| QUAL | Quality assurance and testing |
| PMGT | Project management |

## Approach

1. Create migration script at `.codeflow/scripts/db/migrations/` (directory to be established by this task; no existing migrations directory)
2. Use `ALTER TABLE tasks ADD COLUMN` for each missing column (SQLite does not support adding CHECK constraints via ALTER — apply them in the schema.sql definition for new databases; existing DB relies on application-level validation)
3. Insert QUAL and PMGT rows into `domains` table using `INSERT OR IGNORE`
3a. Insert PLAN row into `work_types` table (not currently seeded in schema.sql)
4. Verify migration by comparing `.schema tasks` output against schema.sql
5. Record row counts before and after to confirm zero data loss

## Files

### To Modify

- `.codeflow/scripts/db/schema.sql` -- Verify V4 columns are present (should already be; confirm)
- `.state/db/codeflow.db` -- Apply migration (ALTER TABLE + INSERT)

### To Create

- `.codeflow/scripts/db/migrations/001-add-v4-task-columns.sql` -- Migration script (directory and naming convention established by this task)

## Dependencies

### Blocked By

- None (foundational task)

### Blocks

- INF-TSK-PLAN-PMGT-001_002 (Epic Standardization -- needs correct schema for DB reconciliation)
- INF-TSK-PLAN-PMGT-001_003 (Task Standardization -- needs correct schema for DB reconciliation)
- INF-TSK-PLAN-PMGT-001_004 (Epic-Scoped Task Numbering -- needs schema for bulk renames)
- INF-TSK-PLAN-PMGT-001_005 (work-graph.jsonl Cleanup -- needs schema context for valid event fields)

## Verification

### Automated

- [ ] Migration script executes without errors: `sqlite3 .state/db/codeflow.db < migration.sql`
- [ ] `.schema tasks` includes all 9 new columns
- [ ] `SELECT code FROM domains WHERE code IN ('QUAL','PMGT')` returns 2 rows

### Manual

- [ ] Row count in tasks and epics tables unchanged after migration
- [ ] Existing task queries still return correct results

## Notes

- SQLite `ALTER TABLE ADD COLUMN` does not support CHECK constraints. The schema.sql definition includes them for new databases, but the live DB migration adds columns without constraints. Application-level validation (in cf-knowledge-layer) must enforce allowed values.
- The `stage_history` column defaults to `'[]'` (empty JSON array string), not NULL, matching schema.sql.
- PLAN must be added to the `work_types` seed data in schema.sql — it is not currently seeded. Insert: `('PLAN', 'Plan', 'plan/', 'plan', 'normal')`.
- The `.codeflow/scripts/db/migrations/` directory does not exist yet. This task establishes the migrations directory and naming convention (NNN-description.sql).
