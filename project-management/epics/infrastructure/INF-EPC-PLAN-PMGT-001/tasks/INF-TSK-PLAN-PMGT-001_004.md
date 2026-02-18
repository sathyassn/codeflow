---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_004
epic_id: INF-EPC-PLAN-PMGT-001
title: "Epic-Scoped Task Numbering"
description: "Migrate all tasks from global sequential numbering (INF-TSK-FEAT-GENL-{NNN}) to epic-scoped numbering (INF-TSK-FEAT-GENL-{EPIC_NNN}_{TASK_NNN}). Move misplaced FIX tasks from FEAT epic to FIX epic. Rename all task files and update all references."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: ["project-management/epics/**/tasks/*.md", "project-management/epics/**/*-epic.md", ".state/ledger/work-graph.jsonl", ".state/db/codeflow.db"]
scope_policy: soft
scope_root: null
estimate: L
priority: high
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "All task IDs follow pattern {AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_{TASK_NNN} where EPIC_NNN matches the parent epic's sequence number"
  - "FIX tasks (INF-TSK-FIX-GENL-003, -004, -005) moved from INF-EPC-FEAT-GENL-001/ to INF-EPC-FIX-GENL-001/ with updated epic_id and renumbered IDs"
  - "All task filenames match their frontmatter id field"
  - "All epic task tables reference the new task IDs"
  - "work-graph.jsonl references updated to new task IDs (or noted for Task 005 cleanup)"
  - "DB tasks table id and epic_id fields updated to reflect new numbering"
  - "Zero orphaned task files (every file has a matching DB record)"
  - "No two tasks share the same id value"
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

# INF-TSK-PLAN-PMGT-001_004: Epic-Scoped Task Numbering

## Description

Currently, tasks are numbered with global sequential IDs (e.g., `INF-TSK-FEAT-GENL-001`, `INF-TSK-FEAT-GENL-002`, ...) which creates confusion when multiple epics share the same area/type/domain prefix. The new scheme uses epic-scoped numbering:

```text
Old: INF-TSK-FEAT-GENL-003   (global sequence, ambiguous epic)
New: INF-TSK-FEAT-GENL-001_003  (epic 001, task 003 — epic is explicit)
```

Additionally, 3 FIX tasks are incorrectly located under the FEAT epic and must be moved to the FIX epic.

### Renaming Map

This is the complete mapping of current to target IDs. The exact mapping will be finalized during execution based on the state after Tasks 002 and 003 complete.

**INF-EPC-FEAT-GENL-001 tasks:**

| Current ID | New ID | Notes |
|-----------|--------|-------|
| INF-TSK-FEAT-GENL-001 | INF-TSK-FEAT-GENL-001_001 | Stays in FEAT epic |
| INF-TSK-FEAT-GENL-002 | INF-TSK-FEAT-GENL-001_002 | Stays in FEAT epic |
| INF-TSK-FIX-GENL-003 | INF-TSK-FIX-GENL-001_001 | **Move to FIX epic** |
| INF-TSK-FIX-GENL-004 | INF-TSK-FIX-GENL-001_002 | **Move to FIX epic** |
| INF-TSK-FIX-GENL-005 | INF-TSK-FIX-GENL-001_003 | **Move to FIX epic** |

**INF-EPC-FIX-GENL-001 tasks:**

| Current ID | New ID | Notes |
|-----------|--------|-------|
| INF-TSK-FIX-GENL-006 | INF-TSK-FIX-GENL-001_004 | Stays (renumber after incoming tasks) |

**Other epics:** Similar renaming pattern applied to all task files across all epics.

## Approach

1. Generate complete old-to-new ID mapping for all tasks across all epics
2. Move FIX tasks from FEAT epic directory to FIX epic directory
3. Rename all task files to match new IDs
4. Update frontmatter `id` and `epic_id` fields in each task file
5. Update task tables in all `*-epic.md` files
6. Update DB records: `UPDATE tasks SET id=?, epic_id=? WHERE id=?`
7. Update work-graph.jsonl references (or flag for Task 005 cleanup)
8. Verify no orphans or duplicates

## Files

### To Modify

- `project-management/epics/**/tasks/*.md` -- Rename all task files, update frontmatter
- `project-management/epics/**/*-epic.md` -- Update task tables with new IDs
- `.state/db/codeflow.db` -- Update task id and epic_id values
- `.state/ledger/work-graph.jsonl` -- Update task_id references in events

## Dependencies

### Blocked By

- INF-TSK-PLAN-PMGT-001_001 (DB Schema Migration -- needs correct schema before bulk updates)

### Blocks

- INF-TSK-PLAN-PMGT-001_010 (Script Updates -- generate-format-id.sh must support the new pattern)

## Verification

### Automated

- [ ] `grep -r 'id:' project-management/epics/**/tasks/*.md` shows all IDs contain `_` separator
- [ ] No task file exists without a matching DB record
- [ ] No DB task record exists without a matching markdown file
- [ ] `SELECT id FROM tasks WHERE id NOT LIKE '%\_%' ESCAPE '\'` returns 0 rows (all IDs have underscore)

### Manual

- [ ] FIX tasks no longer present under INF-EPC-FEAT-GENL-001/tasks/
- [ ] FIX tasks present under INF-EPC-FIX-GENL-001/tasks/ with correct epic_id
- [ ] Epic task tables in all `*-epic.md` files reference new IDs

## Notes

- This is the highest-effort task in the epic due to the cascading rename across files, DB, and JSONL.
- The renaming map above is illustrative. The final mapping depends on the state after Tasks 002 and 003 normalize the epic and task data.
- Consider creating a shell script to automate the bulk rename and update operations rather than doing them manually.
- JSONL updates: if events reference old task IDs, they should be updated in-place. However, if the event is malformed (wrong event type in wrong file), defer to Task 005.
