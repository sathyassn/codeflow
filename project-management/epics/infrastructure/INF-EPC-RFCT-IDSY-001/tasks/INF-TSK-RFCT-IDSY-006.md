---
id: task-01KH9V2G3GX72E72GK0X045KYN
format_id: INF-TSK-RFCT-IDSY-006
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "Migration: Scripts and project management updates"
description: Create migration script, update project management README and epic/task frontmatter for dual-ID
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: [".codeflow/scripts/db/", "project-management/"]
scope_policy: strict
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["migrate-to-dual-id.sh created and tested", "project-management README clarifies format_id in filenames", "all existing epic .md frontmatter has id (ULID) and format_id", "all existing task .md frontmatter has id (ULID) and format_id"]
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

# INF-TSK-RFCT-IDSY-006: Migration - Scripts and project management

## Description

Create the migration script and update existing project management files to use the dual-ID system.

### Changes (6 items from findings)

| Finding ID | Location | Change |
|------------|----------|--------|
| CF-NEW-01 | NEW: db/generate-format-id.sh | Sequence counter per AREA-TYPE-DOMAIN |
| CF-NEW-02 | NEW: db/migrate-to-dual-id.sh | Migration script |
| CF-PM-01 | project-management/README.md L26-29 | Clarify format_id in filenames |
| CF-PM-02 | project-management/epics/README.md | Same |
| CF-PM-03 | All existing epic .md frontmatter | Add id (ULID), rename current to format_id |
| CF-PM-04 | All existing task .md frontmatter | Add id (ULID), add format_id |

## Approach

1. Create generate-format-id.sh (sequence counter)
2. Create migrate-to-dual-id.sh (adds ULID ids and format_id to existing records)
3. Update project management READMEs
4. Update existing epic/task frontmatter with dual IDs

## Files

### To Create

- `.codeflow/scripts/db/generate-format-id.sh` - Format ID sequence generator
- `.codeflow/scripts/db/migrate-to-dual-id.sh` - Migration script

### To Modify

- `project-management/README.md`
- `project-management/epics/README.md`
- All existing epic .md files (frontmatter)
- All existing task .md files (frontmatter)

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-001 (schema must be updated first)
- INF-TSK-RFCT-IDSY-002 (ID generation must be working)
- INF-TSK-RFCT-IDSY-003 (business logic must handle dual IDs)

### Blocks

- None

## Verification

### Automated

- [ ] migrate-to-dual-id.sh runs without errors
- [ ] All epic .md files have id and format_id in frontmatter
- [ ] All task .md files have id and format_id in frontmatter

### Manual

- [ ] Existing format IDs preserved as format_id
- [ ] New ULID PKs generated correctly

## Notes

Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Sections 4.7-4.8
