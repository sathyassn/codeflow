---
id: "epic-01KMKXVCQSSJD45RC24M2AH2A3"
format_id: "INF-EPC-029"
title: "Worktree heartbeat safety + autorun directory structure"
summary: "Replace broken lead_pid liveness with heartbeat-based session detection. Add autorun batches/ and local/ directories."
status: complete
area_type: "INF"
work_type: "HTFX"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-26T01:58:54Z"
updated_at: "2026-03-26T04:16:09Z"
---

# INF-EPC-029: Worktree heartbeat safety + autorun directory structure

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Hotfix addressing two infrastructure gaps: (1) worktree session detection via heartbeat files instead of unreliable lead_pid liveness checks, and (2) adding missing autorun directory structure (batches/ and local/) required by the autorun subsystem.

Previously numbered INF-EPC-922 (renumbered to INF-EPC-029 for sequential consistency).

## Scope

### In Scope

- Replace lead_pid-based liveness detection with heartbeat file mechanism in worktree/session detection
- Add autorun batches/ and local/ directory creation to session setup
- Update CLAUDE.md documentation to reflect new heartbeat-based detection

### Out of Scope

- Changes to CRDT coordination, merge queue, or sync daemon
- New features beyond heartbeat safety and autorun directories

## Acceptance Criteria

- [ ] Worktree session detection uses heartbeat file instead of lead_pid kill()
- [ ] Autorun batches/ and local/ directories created on session start
- [ ] All existing tests pass after changes

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-029-001 | Worktree heartbeat safety + autorun directory structure | complete | high |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The current `lead_pid` liveness check using `kill(pid, 0)` is unreliable because PIDs can be reused by the OS. A heartbeat file approach is more robust: the lead writes a timestamp to a well-known path periodically, and session detection checks file recency rather than PID liveness.

### Autorun Batching

N/A — this is an HTFX hotfix, single task, not autorun-eligible.

## Related

- PR #216 (worktree hook path resolution and integration)
- `.state/session/{SID}/pathflow/pathflow-team.json` (teammate detection)
- `codeflow-cli/core/src/` (session/worktree modules)
