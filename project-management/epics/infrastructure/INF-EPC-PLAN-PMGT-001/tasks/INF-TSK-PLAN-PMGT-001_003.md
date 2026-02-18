---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_003
epic_id: INF-EPC-PLAN-PMGT-001
title: "Task Standardization"
description: "Convert all task markdown files to proper YAML frontmatter format with complete field set aligned to DB tasks table, reconcile DB vs filesystem task records."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: ["project-management/epics/**/tasks/*.md", ".state/db/codeflow.db"]
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
  - "5 markdown-table-format task files converted to YAML frontmatter format (files: INF-TSK-FEAT-GENL-001, INF-TSK-FEAT-GENL-002, DOC-TSK-DOCS-GENL-001, DOC-TSK-DOCS-GENL-002, INF-TSK-CHOR-GENL-001)"
  - "8 format-ID-as-ID task files have all 30 frontmatter fields present (acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch, and others)"
  - "Every task markdown file has all 30 required frontmatter fields: id, epic_id, title, description, status, area_type, work_type, domain, origin, file_scope, scope_policy, scope_root, estimate, priority, assignee_id, autorun_eligible, auto_commit, raise_pr, auto_merge, target_branch, acceptance, tests, branch, pr_number, external_id, external_url, created_at, updated_at, started_at, completed_at"
  - "DB tasks table record count matches filesystem task file count"
  - "DB task status values match corresponding markdown frontmatter for all tasks"
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

# INF-TSK-PLAN-PMGT-001_003: Task Standardization

## Description

Task files in the project-management directory use three different formats:

1. **Markdown table format** (5 files) -- Tasks defined as rows in a markdown table within the epic file, with minimal or no individual task files
2. **Format-ID-as-ID with partial frontmatter** (8 files) -- Have YAML frontmatter but missing V4 columns (acceptance, tests, auto_commit, etc.)
3. **Full frontmatter** (newer files) -- Complete 30-field YAML frontmatter aligned with DB schema

All task files must be normalized to format (3).

### Files Requiring Conversion

**Markdown-table format (need full rewrite to YAML frontmatter):**

| File | Epic |
|------|------|
| INF-TSK-FEAT-GENL-001.md | INF-EPC-FEAT-GENL-001 |
| INF-TSK-FEAT-GENL-002.md | INF-EPC-FEAT-GENL-001 |
| DOC-TSK-DOCS-GENL-001.md | DOC-EPC-DOCS-GENL-001 |
| DOC-TSK-DOCS-GENL-002.md | DOC-EPC-DOCS-GENL-001 |
| INF-TSK-CHOR-GENL-001.md | INF-EPC-CHOR-GENL-001 |

**Partial frontmatter (need missing fields added):**

| File | Missing Fields |
|------|---------------|
| INF-TSK-FIX-GENL-003.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-FIX-GENL-004.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-FIX-GENL-005.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-FIX-GENL-006.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-FIX-GENL-007.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-FIX-GENL-010.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-FEAT-GENL-003.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |
| INF-TSK-CHOR-GENL-004.md | acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch |

## Approach

1. Query DB for all task records to get authoritative field values
2. Read each task markdown file to determine its current format
3. For markdown-table files: create proper YAML frontmatter from DB record + template defaults
4. For partial-frontmatter files: add missing fields with DB values or defaults
5. Preserve existing body content (Description, Approach, Files, etc.) where present
6. For files with no body content, generate minimal body from DB description field
7. Reconcile any DB-vs-filesystem mismatches (DB is authoritative for field values)

## Files

### To Modify

- `project-management/epics/infrastructure/INF-EPC-FEAT-GENL-001/tasks/*.md` -- All task files in this epic
- `project-management/epics/infrastructure/INF-EPC-FIX-GENL-001/tasks/*.md` -- All task files
- `project-management/epics/infrastructure/INF-EPC-CHOR-GENL-001/tasks/*.md` -- All task files
- `project-management/epics/infrastructure/INF-EPC-QUAL-GENL-001/tasks/*.md` -- All task files (post-reclassification to CHOR-QUAL)
- `project-management/epics/documentation/DOC-EPC-DOCS-GENL-001/tasks/*.md` -- All task files
- `.state/db/codeflow.db` -- Add any filesystem-only tasks to DB

## Dependencies

### Blocked By

- INF-TSK-PLAN-PMGT-001_001 (DB Schema Migration -- needs V4 columns to exist in DB for reconciliation)

### Blocks

- INF-TSK-PLAN-PMGT-001_007 (Tracking Regeneration -- needs clean task data)

## Verification

### Automated

- [ ] All task `*.md` files have valid YAML frontmatter (parseable by any YAML parser)
- [ ] All task files contain exactly 30 frontmatter fields (count check)
- [ ] `SELECT COUNT(*) FROM tasks` matches count of task `*.md` files in project-management/
- [ ] No frontmatter field has a `{placeholder}` value

### Manual

- [ ] Spot-check 3 converted files: frontmatter values match DB record
- [ ] Body content preserved for files that had existing descriptions

## Notes

- Task 004 (Epic-Scoped Task Numbering) will rename these files after standardization. This task focuses on format/content only, not file naming.
- The 5 markdown-table files may have minimal information. Use DB records as the primary source for field values, supplemented by whatever body text exists.
