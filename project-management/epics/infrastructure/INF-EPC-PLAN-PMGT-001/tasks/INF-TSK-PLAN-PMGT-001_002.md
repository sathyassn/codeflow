---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_002
epic_id: INF-EPC-PLAN-PMGT-001
title: "Epic Standardization"
description: "Normalize all epic markdown files to have complete YAML frontmatter aligned with DB epics table, reclassify invalid work types, reconcile DB vs filesystem epics, and sync statuses."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: ["project-management/epics/**/*-epic.md", ".state/db/codeflow.db"]
scope_policy: soft
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "INF-EPC-QUAL-GENL-001 reclassified to INF-EPC-CHOR-QUAL-001: directory renamed, frontmatter updated, DB record updated"
  - "Markdown files created for DB-only epics: INF-EPC-SPKE-GENL-001 and INF-EPC-RFCT-GENL-001 with full YAML frontmatter"
  - "INF-EPC-RFCT-IDSY-001 exists in DB with status and fields matching its markdown frontmatter"
  - "Every *-epic.md file has all 15 frontmatter fields: id, title, summary, status, area_type, work_type, domain, is_ongoing, priority, file_scope, pr_number, external_id, external_url, created_at, updated_at"
  - "DB epics table status values match corresponding markdown frontmatter status values for all epics"
  - "No epic exists in DB without a corresponding markdown file, and vice versa"
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

# INF-TSK-PLAN-PMGT-001_002: Epic Standardization

## Description

Multiple inconsistencies exist between epic markdown files and DB records. Some epics exist only in the DB, others only on the filesystem. One epic (INF-EPC-QUAL-GENL-001) uses an invalid work_type (QUAL is a domain, not a work type). Frontmatter fields are incomplete on older epic files.

### Issues to Resolve

| Issue | Current State | Target State |
|-------|--------------|--------------|
| INF-EPC-QUAL-GENL-001 invalid work_type | work_type=QUAL (not a valid work_type code) | Reclassify as INF-EPC-CHOR-QUAL-001 (work_type=CHOR, domain=QUAL) |
| INF-EPC-SPKE-GENL-001 DB-only | Exists in DB, no markdown file | Create markdown file with full frontmatter |
| INF-EPC-RFCT-GENL-001 DB-only | Exists in DB, no markdown file | Create markdown file with full frontmatter |
| INF-EPC-RFCT-IDSY-001 filesystem-only | Exists on filesystem, not in DB | Add to DB |
| Incomplete frontmatter | Older epics missing fields | All epics get complete 15-field frontmatter |
| Status drift | DB and markdown statuses may differ | Sync: markdown is derived from DB (DB is authoritative) |

## Approach

1. Query DB for all epic records: `SELECT * FROM epics`
2. List all `*-epic.md` files on filesystem
3. Diff the two sets to identify mismatches
4. Reclassify INF-EPC-QUAL-GENL-001:
   - Rename directory from `INF-EPC-QUAL-GENL-001/` to `INF-EPC-CHOR-QUAL-001/`
   - Update frontmatter id, work_type, domain
   - Update DB record
   - Update task epic_id references
5. Create markdown for DB-only epics using template
6. Add INF-EPC-RFCT-IDSY-001 to DB
7. Update all epic frontmatter to include all 15 fields
8. Sync statuses (DB authoritative)

## Files

### To Modify

- `project-management/epics/infrastructure/INF-EPC-QUAL-GENL-001/` -- Rename directory and update frontmatter
- `project-management/epics/infrastructure/INF-EPC-QUAL-GENL-001/*-epic.md` -- Reclassify to CHOR-QUAL
- `project-management/epics/**/*-epic.md` -- Add missing frontmatter fields to all epic files
- `.state/db/codeflow.db` -- Update/add epic records

### To Create

- `project-management/epics/infrastructure/INF-EPC-SPKE-GENL-001/INF-EPC-SPKE-GENL-001-epic.md`
- `project-management/epics/infrastructure/INF-EPC-RFCT-GENL-001/INF-EPC-RFCT-GENL-001-epic.md`

## Dependencies

### Blocked By

- INF-TSK-PLAN-PMGT-001_001 (DB Schema Migration -- needs QUAL domain code in DB)

### Blocks

- INF-TSK-PLAN-PMGT-001_007 (Tracking Regeneration -- needs clean epic data)

## Verification

### Automated

- [ ] `SELECT COUNT(*) FROM epics` matches count of `*-epic.md` files
- [ ] `SELECT id FROM epics WHERE work_type='QUAL'` returns 0 rows
- [ ] `SELECT id FROM epics WHERE id='INF-EPC-CHOR-QUAL-001'` returns 1 row
- [ ] All `*-epic.md` files pass YAML frontmatter validation (15 required fields present)

### Manual

- [ ] INF-EPC-QUAL-GENL-001 directory no longer exists
- [ ] INF-EPC-CHOR-QUAL-001 directory exists with updated content
- [ ] INF-EPC-RFCT-IDSY-001 exists in both DB and filesystem

## Notes

- The reclassification of QUAL-GENL-001 to CHOR-QUAL-001 also requires updating any task files that reference the old epic_id.
- When syncing statuses, DB is authoritative. If markdown says "complete" but DB says "in_progress", update the markdown.
