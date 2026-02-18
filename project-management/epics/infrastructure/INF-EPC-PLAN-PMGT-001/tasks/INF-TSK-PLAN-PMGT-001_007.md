---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_007
epic_id: INF-EPC-PLAN-PMGT-001
title: "Tracking Regeneration"
description: "Regenerate epic-tracker.md from normalized epic/task data using cf-tracking-generate.sh, and add instructions to cf-knowledge-layer to regenerate after every status change."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: ["project-management/tracking/", ".claude/agents/cf-knowledge-layer.md"]
scope_policy: soft
scope_root: null
estimate: XS
priority: low
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "project-management/tracking/epic-tracker.md regenerated and reflects current epic/task statuses"
  - "epic-tracker.md lists all epics with correct status, task counts, and completion percentages"
  - "epic-tracker.md uses new epic-scoped task IDs (if Task 004 has completed)"
  - "cf-knowledge-layer.md contains instruction to regenerate tracking after status changes, referencing cf-tracking-generate.sh"
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

# INF-TSK-PLAN-PMGT-001_007: Tracking Regeneration

## Description

The `project-management/tracking/epic-tracker.md` file is a derived view (Tier 2) generated from epic/task data. After Tasks 002 (Epic Standardization) and 003 (Task Standardization) normalize the data, the tracker must be regenerated to reflect the current state.

Additionally, the cf-knowledge-layer agent definition needs an explicit instruction to regenerate tracking after every status change, ensuring the tracker stays in sync going forward.

## Approach

1. Locate cf-tracking-generate.sh at `.codeflow/scripts/tracking/cf-tracking-generate.sh` — if it does not exist, create it as part of this task
2. Run the script (or equivalent logic) to regenerate epic-tracker.md from current DB/frontmatter data
3. Review output for correctness: all epics listed, statuses match DB, task counts accurate
4. Add instruction to cf-knowledge-layer.md:
   - After any `task_status_changed` or `epic_updated` event, run `cf-tracking-generate.sh`
   - Location: in the relevant SOP section of the agent definition

## Files

### To Modify

- `project-management/tracking/epic-tracker.md` -- Regenerated output (overwritten by script)
- `.claude/agents/cf-knowledge-layer.md` -- Add tracking regeneration instruction

## Dependencies

### Blocked By

- INF-TSK-PLAN-PMGT-001_002 (Epic Standardization -- needs clean epic data)
- INF-TSK-PLAN-PMGT-001_003 (Task Standardization -- needs clean task data)

### Blocks

- None

## Verification

### Automated

- [ ] `project-management/tracking/epic-tracker.md` exists and is non-empty
- [ ] All epics from `SELECT id FROM epics` appear in the tracker
- [ ] cf-knowledge-layer.md contains string "cf-tracking-generate" or "tracking regenerat"

### Manual

- [ ] Epic statuses in tracker match DB statuses
- [ ] Task counts per epic are accurate

## Notes

- cf-tracking-generate.sh does not currently exist. This task is responsible for creating it at `.codeflow/scripts/tracking/cf-tracking-generate.sh` if needed, or implementing equivalent regeneration logic inline.
- This is an XS task because it's primarily running an existing script and adding one instruction. Autorun eligible.
