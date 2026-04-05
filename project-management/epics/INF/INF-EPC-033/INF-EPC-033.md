---
id: "epic-01KMVASRB0H931T1MZPXGJZ7V8"
format_id: "INF-EPC-033"
title: "Fix autorun subsystem: invocation, naming, cleanup, observability"
summary: "Fix the autorun subsystem across invocation, worker naming, stale session cleanup, pruning, claim TTL eviction, status transitions, and session-level Timeout handling."
status: complete
area_type: "INF"
work_type: "FIX"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-28T23:00:17Z"
updated_at: "2026-04-05T14:25:14Z"
---

# INF-EPC-033: Fix autorun subsystem: invocation, naming, cleanup, observability

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

The autorun subsystem accumulated a set of interrelated bugs and gaps: incorrect invocation patterns, worker naming inconsistencies, missing stale session cleanup, no pruning of old records, claim TTL eviction not wired, non-standard status transitions, and session-level Timeout status absent. This epic fixes all of these in a sequenced set of tasks to restore correctness and operability of the autorun pipeline.

## Scope

### In Scope

- Autorun invocation and worker naming fixes
- WorktreeManager path resolution and CF_PROJECT_ROOT wiring
- Stale session detection, cleanup, and abort logic
- DB pruning for old autorun_session/worker/task_run records with indexes
- Claim TTL eviction from Loro CRDT document
- Conditional status transitions for autorun sessions
- Session-level Timeout status and removal of Paused/Aborting states

### Out of Scope

- Autorun scheduling or external trigger changes
- UI or dashboard changes
- Non-autorun worktree functionality

## Acceptance Criteria

- [ ] All autorun subsystem bugs identified in tasks 001-007 are fixed
- [ ] No regressions in existing autorun tests
- [ ] 85%+ per-file test coverage on all modified Rust files
- [ ] Clippy clean across all modified crates

### PII Handling Review

- [x] Does this epic involve code that handles PII? N
- N/A: autorun operates on session metadata (timestamps, status, IDs) — no user PII

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-033-001 | Fix autorun subsystem: invocation, naming, cleanup, observability | complete | normal |
| INF-TSK-033-002 | Fix WorktreeManager path resolution and CF_PROJECT_ROOT | complete | high |
| INF-TSK-033-003 | Autorun stale session detection, cleanup, and abort fixes | complete | normal |
| INF-TSK-033-004 | Add autorun session/worker/task_run pruning and indexes | complete | normal |
| INF-TSK-033-005 | Add proactive claim TTL eviction from Loro CRDT document | complete | normal |
| INF-TSK-033-006 | Add conditional status transitions for autorun sessions | complete | normal |
| INF-TSK-033-007 | Wire session-level Timeout status and remove/document Paused/Aborting | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Tasks in this epic are sequenced: 001-003 are complete (fixes to invocation, path resolution, stale cleanup). Tasks 004-007 address remaining gaps in pruning, claim eviction, status transitions, and Timeout handling. Each task modifies independent areas of the autorun subsystem except where noted in individual task dependencies.

### Autorun Batching

Tasks 004-007 are NOT autorun-eligible (autorun_eligible: false). Execute sequentially per task dependency chain.

## Related

- INF-EPC-038: Worktree cleanup (related worktree fix work)
- PR #229: Autorun subsystem overhaul (earlier PR covering tasks 001-003)
