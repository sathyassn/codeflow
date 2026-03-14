---
id: "epic-01kknszgxkgz9czbwpx5kzqyjg"
format_id: "INF-EPC-028"
title: "PathFlow Enforcement Fixes"
summary: "Fix enforcement gaps in PathFlow checkpoint and sentinel validation system."
status: complete
area_type: INF
work_type: FIX
domain: GENL
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-14T00:00:00Z"
updated_at: "2026-03-14T00:00:00Z"
---

# INF-EPC-028: PathFlow Enforcement Fixes

## Summary

Fix enforcement gaps in the PathFlow checkpoint and sentinel validation system. The current gate-check hook checks individual sentinels but does not enforce cumulative phase ordering, config-driven pipeline lookups, or dynamic task-to-stage mapping — allowing the checkpoint system to be gamed by marking tasks complete without executing them.

## Scope

### In Scope

- Cumulative sentinel checking in gate-check hook
- Config-driven pipeline sentinel lookups from pathflow-config.json
- Dynamic task-to-stage mapping for PF4 task tracker entries

### Out of Scope

- Changes to session lifecycle flow
- New PathFlow phases or stages

## Acceptance Criteria

- [ ] Gate-check verifies all prior phase sentinels cumulatively before allowing phase-N operations
- [ ] Required stage sentinels are derived from pathflow-config.json per work type, not hardcoded
- [ ] PF4-TSK-05/06/07 are checked against the stage sentinels they represent
- [ ] All existing gate-check tests continue to pass

### PII Handling Review

- [x] Does this epic involve code that handles PII? N — enforcement/hook logic only

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-028-001 | Cumulative sentinel enforcement — config-driven phase and stage checks | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The PathFlow checkpoint system creates phase sentinels when all tasks for a phase are marked completed in the task tracker. The gate-check hook then checks for the presence of specific sentinels before allowing operations. The gap: the system does not verify that ALL prerequisite sentinels exist cumulatively, nor does it derive required stage sentinels from the config. This allows a team lead to skip stages by completing task tracker entries without spawning the required teammates.

## Related

- INF-EPC-912: Phase Checkpoint Enforcement (prior related work, now complete)
