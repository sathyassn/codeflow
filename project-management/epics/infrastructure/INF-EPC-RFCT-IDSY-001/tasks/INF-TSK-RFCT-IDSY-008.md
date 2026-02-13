---
id: task-01KH9V2G49CTQ6HK514511C5NE
format_id: INF-TSK-RFCT-IDSY-008
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "V4 Spec: Events and validation specs (8 items)"
description: Update JSONL event schemas and shell/Python validation specs in V4 specification for dual-ID
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: []
scope_policy: soft
scope_root: null
estimate: M
priority: normal
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["All 5 event schema groups updated with format_id fields", "ULID PK format used for id/epic_id/task_id in events", "Validation specs updated for dual-ID patterns"]
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

# INF-TSK-RFCT-IDSY-008: V4 Spec - Events and validation specs

## Description

Update JSONL event schemas and validation/shell specs in the V4 specification for dual-ID system. **Separate repository** (codeflow-specification-v4).

### Changes (8 items from findings)

| Finding ID | File | Change |
|------------|------|--------|
| V4-EVT-01 | 12-jsonl-event-reference.md L179-198 | epic_created: id -> ULID, add format_id |
| V4-EVT-02 | 12-jsonl-event-reference.md L200-213 | epic_status_changed: id -> ULID |
| V4-EVT-03 | 12-jsonl-event-reference.md L215-234 | task_created: id/epic_id -> ULID, add format_id |
| V4-EVT-04 | 12-jsonl-event-reference.md L236-280 | task_completed, dependency_added, criterion_met |
| V4-EVT-05 | 12-jsonl-event-reference.md L307-616 | progress, stage events, work_started |
| V4-VAL-01 | shell-modularization-architecture.md L1030-1040 | Update shell validators |
| V4-VAL-02 | shell-modularization-architecture.md L1222-1228 | Update shell generators |
| V4-VAL-03 | python-modularization-architecture.md L529-536 | Split PATTERNS dict |

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-007 (schema specs should be updated first)

### Blocks

- INF-TSK-RFCT-IDSY-009 (documentation references events)

## Notes

- **Repository**: codeflow-specification-v4 (NOT this repo)
- Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Sections 3.3-3.4
