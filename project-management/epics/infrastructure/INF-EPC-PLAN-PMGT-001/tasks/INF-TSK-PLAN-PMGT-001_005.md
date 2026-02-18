---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_005
epic_id: INF-EPC-PLAN-PMGT-001
title: "work-graph.jsonl Cleanup"
description: "Remove misrouted events from work-graph.jsonl, normalize inconsistent keys, remove duplicates, and write a clean file containing only valid work_graph event types."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: [".state/ledger/work-graph.jsonl", ".codeflow/scripts/state/ledger.sh"]
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
  - "work-graph.jsonl contains only valid event types: epic_created, task_created, task_status_changed, epic_status_changed"
  - "Zero events with type work_progress, decision, milestone, finding, stage_transition, begin_work, or complete_work remain in work-graph.jsonl"
  - "All events use consistent key names: 'event' (not 'e'), 'timestamp' (not 'ts')"
  - "ledger.sh append_ledger() updated to write 'event' and 'timestamp' keys instead of 'e' and 'ts', preventing future inconsistency"
  - "No duplicate events (same event type + entity_id + timestamp)"
  - "Backup of original file saved at .state/ledger/work-graph.jsonl.bak before cleanup"
  - "Line count of clean file is less than original (misrouted events removed)"
  - "Every remaining event is valid JSON parseable by jq"
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

# INF-TSK-PLAN-PMGT-001_005: work-graph.jsonl Cleanup

## Description

The work-graph.jsonl ledger file has accumulated 14+ misrouted events that belong in other JSONL files (or nowhere). Additionally, key naming is inconsistent (`e` vs `event`, `ts` vs `timestamp`) and some events are duplicates or corrections of earlier malformed entries.

### Misrouted Event Types to Remove

| Event Type | Belongs In | Count (approx) |
|-----------|-----------|----------------|
| work_progress | sessions.jsonl | 3+ |
| decision | memory-events.jsonl | 2+ |
| milestone | sessions.jsonl | 1+ |
| finding | sessions.jsonl | 2+ |
| stage_transition | sessions.jsonl | 2+ |
| begin_work | sessions.jsonl | 2+ |
| complete_work | sessions.jsonl | 2+ |

### Valid Event Types for work-graph.jsonl

Per ledger.sh helper functions (`record_epic_created`, `record_task_created`, `record_task_status_changed`, `record_epic_status_changed`), work-graph.jsonl should only contain:

- `epic_created`
- `task_created`
- `task_status_changed`
- `epic_status_changed`

> **Note:** Historical events with types `epic_updated` or `task_updated` may exist from direct LLM writes. These are not supported by ledger.sh. During cleanup, review any such events — if they contain valid field-level update data, capture it as a status change or discard if redundant.

## Approach

1. Back up original: `cp work-graph.jsonl work-graph.jsonl.bak`
2. Read all lines, parse each as JSON
3. Filter: keep only events with valid event types (4 types above)
4. Normalize keys: rename `e` to `event`, `ts` to `timestamp` where found
5. Deduplicate: if same event type + entity_id + timestamp appears multiple times, keep only the last occurrence
6. If Task 004 updated task IDs in JSONL, verify those updates are preserved
7. Write clean file, verify each line is valid JSON
8. Update ledger.sh `append_ledger()` to write `event` and `timestamp` keys instead of `e` and `ts`
9. Update ledger.sh `append_event()` timestamp check to match new key name
10. Update ledger.sh `filter_ledger()` grep pattern from `"e":` to `"event":`

## Files

### To Modify

- `.state/ledger/work-graph.jsonl` -- Clean and normalize in-place (after backup)
- `.codeflow/scripts/state/ledger.sh` -- Update `append_ledger()`, `append_event()`, and `filter_ledger()` to use `event`/`timestamp` keys instead of `e`/`ts`

### To Create

- `.state/ledger/work-graph.jsonl.bak` -- Backup of original (temporary, not committed)

## Dependencies

### Blocked By

- INF-TSK-PLAN-PMGT-001_001 (DB Schema Migration -- needs schema context to understand valid event fields)

### Blocks

- None

## Verification

### Automated

- [ ] `jq -e '.event' .state/ledger/work-graph.jsonl` succeeds for every line (all events have `event` key)
- [ ] `jq -r '.event' .state/ledger/work-graph.jsonl | sort -u` outputs only the 4 valid event types
- [ ] `wc -l work-graph.jsonl` < `wc -l work-graph.jsonl.bak`
- [ ] No line fails JSON parsing: `jq empty .state/ledger/work-graph.jsonl` exits 0

### Manual

- [ ] Backup file exists before cleanup begins
- [ ] Spot-check 3 remaining events: all have consistent key names and valid structure

## Notes

- The backup file (.bak) should NOT be committed to git. Add to .gitignore or delete after verification.
- Misrouted events are NOT relocated to their correct files — that is out of scope. They are simply removed from work-graph.jsonl. If needed, the correct files can be populated from the backup in a future task.
- Per MEMORY.md: "LLMs write events to wrong files" — this cleanup addresses the historical damage. Future prevention is handled by cf-knowledge-layer agent definition updates (Task 009).
