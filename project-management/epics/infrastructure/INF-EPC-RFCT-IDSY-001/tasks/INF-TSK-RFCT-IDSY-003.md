---
id: task-01KH9V2G2E454CDEDR3NEBCM7W
format_id: INF-TSK-RFCT-IDSY-003
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "Business Logic: State, ledger, and coordination scripts"
description: Update work-state.sh, ledger.sh, cf-memory-store.py, and 5 coordination scripts to handle dual IDs correctly
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: [".codeflow/scripts/state/work-state.sh", ".codeflow/scripts/state/ledger.sh", ".codeflow/scripts/memory/cf-memory-store.py", ".codeflow/scripts/coordination/"]
scope_policy: strict
scope_root: null
estimate: M
priority: high
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["work-state.sh accepts both ULID PK and format_id lookups", "ledger.sh record_* functions include format_id in events", "cf-memory-store.py documents work_id = ULID PK", "claim scripts document work_id = ULID PK"]
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

# INF-TSK-RFCT-IDSY-003: Business Logic - State and coordination scripts

## Description

Update state management, ledger, and coordination scripts to properly handle the dual-ID system. These scripts form the business logic layer that sits between the schema and the user-facing skills.

### Changes (8 items from findings)

| Finding ID | Location | Change |
|------------|----------|--------|
| CF-STATE-01 | work-state.sh L62-93 | Accept both ULID PK + format_id for lookups |
| CF-STATE-02 | ledger.sh L254-288 | Add format_id to record_* functions |
| CF-STATE-03 | cf-memory-store.py L67-68 | Document work_id = ULID PK |
| CF-COORD-01 | cf-claim-acquire.py | Document work_id = ULID PK |
| CF-COORD-02 | cf-claim-check.py | Document work_id = ULID PK |
| CF-COORD-03 | cf-claim-list.py | Document work_id = ULID PK |
| CF-COORD-04 | cf-claim-release.py | Document work_id = ULID PK |
| CF-COORD-05 | cf-claim-renew.py | Document work_id = ULID PK |

## Approach

1. Update work-state.sh to resolve format_id -> ULID PK when given a format ID
2. Update ledger.sh record_* functions to emit format_id alongside id
3. Add docstring/comments to coordination scripts clarifying ULID usage

## Files

### To Modify

- `.codeflow/scripts/state/work-state.sh` - Dual-ID lookup support
- `.codeflow/scripts/state/ledger.sh` - Add format_id to events
- `.codeflow/scripts/memory/cf-memory-store.py` - Document ULID usage
- `.codeflow/scripts/coordination/cf-claim-acquire.py` - Document ULID usage
- `.codeflow/scripts/coordination/cf-claim-check.py` - Document ULID usage
- `.codeflow/scripts/coordination/cf-claim-list.py` - Document ULID usage
- `.codeflow/scripts/coordination/cf-claim-release.py` - Document ULID usage
- `.codeflow/scripts/coordination/cf-claim-renew.py` - Document ULID usage

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-002 (needs updated ID validation functions)

### Blocks

- INF-TSK-RFCT-IDSY-005 (tests depend on business logic)
- INF-TSK-RFCT-IDSY-006 (migration uses business logic)

## Verification

### Automated

- [ ] work-state.sh resolves format_id to ULID PK
- [ ] ledger events include format_id field
- [ ] Coordination scripts have ULID documentation

## Notes

Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Sections 4.3-4.4
