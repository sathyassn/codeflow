---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_010
epic_id: INF-EPC-PLAN-PMGT-001
title: "Script Updates"
description: "Update generate-format-id.sh to support epic-scoped task numbering pattern ({AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_{TASK_NNN}) in addition to the existing global sequential pattern."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: [".codeflow/scripts/db/generate-format-id.sh"]
scope_policy: hard
scope_root: null
estimate: S
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "generate-format-id.sh accepts an --epic-id flag (or equivalent) to generate epic-scoped task IDs"
  - "When --epic-id=INF-EPC-PLAN-PMGT-001 is passed with entity_type=task, output follows pattern INF-TSK-{TYPE}-{DOMAIN}-001_{NNN} where 001 is extracted from the epic ID"
  - "Task sequence number ({NNN}) is calculated by scanning existing tasks under the given epic, not globally"
  - "Epic ID generation (entity_type=epic) remains unchanged"
  - "Backward compatibility: script still works without --epic-id flag (falls back to global sequential for epics)"
  - "Script passes shellcheck with zero errors"
  - "Usage/help text documents the new --epic-id flag"
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

# INF-TSK-PLAN-PMGT-001_010: Script Updates

## Description

The `generate-format-id.sh` script currently generates task IDs with global sequential numbering. It must be updated to support the epic-scoped numbering pattern introduced by Task 004.

### Current Behavior

```bash
# Generates next global ID by scanning all tasks matching prefix
./generate-format-id.sh --area INF --type FEAT --domain GENL --entity task
# Output: INF-TSK-FEAT-GENL-004  (next global sequence number)
```

### Target Behavior

```bash
# With --epic-id: generates epic-scoped task ID
./generate-format-id.sh --area INF --type FEAT --domain GENL --entity task --epic-id INF-EPC-FEAT-GENL-001
# Output: INF-TSK-FEAT-GENL-001_003  (epic 001, next task in that epic)

# Without --epic-id: generates global ID (backward compat for epics)
./generate-format-id.sh --area INF --type FEAT --domain GENL --entity epic
# Output: INF-EPC-FEAT-GENL-002  (unchanged behavior)
```

## Approach

1. Read current `generate-format-id.sh` to understand argument parsing and sequence logic
2. Add `--epic-id` flag to the argument parser
3. When `--epic-id` is provided and entity_type is "task":
   - Extract the epic sequence number (NNN) from the epic ID (e.g., `001` from `INF-EPC-FEAT-GENL-001`)
   - Scan `project-management/epics/**/tasks/` for existing tasks matching `{AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_*`
   - Find the maximum task sequence number and increment
   - Output: `{AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_{next_task_NNN}`
4. When `--epic-id` is not provided: existing behavior unchanged
5. Update usage/help text
6. Run shellcheck

## Files

### To Modify

- `.codeflow/scripts/db/generate-format-id.sh` -- Add --epic-id flag and scoped numbering logic

## Dependencies

### Blocked By

- INF-TSK-PLAN-PMGT-001_004 (Epic-Scoped Task Numbering -- the numbering scheme must be finalized before scripting it)

### Blocks

- None

## Verification

### Automated

- [ ] `shellcheck .codeflow/scripts/db/generate-format-id.sh` exits 0
- [ ] Script with `--epic-id INF-EPC-PLAN-PMGT-001 --entity task --area INF --type PLAN --domain PMGT` outputs `INF-TSK-PLAN-PMGT-001_011` (or next available after the 10 tasks)
- [ ] Script without `--epic-id --entity epic --area INF --type PLAN --domain PMGT` outputs `INF-EPC-PLAN-PMGT-002` (next global epic)
- [ ] `./generate-format-id.sh --help` mentions `--epic-id`

### Manual

- [ ] Backward compatibility: existing callers of the script without --epic-id still work

## Notes

- The script scans `project-management/` for existing format_id values. After Task 004 renames all files, the scan should find the new epic-scoped IDs correctly.
- Consider adding unit tests for the script in a future task (out of scope for this task).
