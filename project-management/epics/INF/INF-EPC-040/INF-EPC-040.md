---
id: "epic-1775184713837RWQ998DQ3ES9"
format_id: "INF-EPC-040"
title: "Worktree Lifecycle Fix: PendingCleanup Timing, TeamDelete Guard, Orphan Scanning, Cleanup Confirmation UX"
summary: "Fix worktree lifecycle issues: PendingCleanup state timing, TeamDelete guard enforcement, orphan scanning, and cleanup confirmation UX improvements"
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
created_at: "2026-04-02T21:00:00Z"
updated_at: "2026-04-02T21:00:00Z"
---

# INF-EPC-040: Worktree Lifecycle Fix: PendingCleanup Timing, TeamDelete Guard, Orphan Scanning, Cleanup Confirmation UX

## Summary

Fix a set of worktree lifecycle issues identified after recent worktree cleanup consolidation work. Addresses PendingCleanup state timing problems, TeamDelete guard gaps, orphan worktree scanning, and cleanup confirmation UX improvements.

## Scope

### In Scope

- PendingCleanup state timing and transition correctness
- TeamDelete guard enforcement in worktree lifecycle
- Orphan worktree scanning logic
- Cleanup confirmation UX improvements

### Out of Scope

- Autorun subsystem changes (tracked in INF-EPC-033)
- JSONL schema standardization (tracked in INF-EPC-024)

## Acceptance Criteria

- [ ] PendingCleanup state transitions correctly under all lifecycle scenarios
- [ ] TeamDelete guard prevents premature worktree cleanup
- [ ] Orphan worktree scanning detects and reports orphans accurately
- [ ] Cleanup confirmation UX is clear and consistent

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (Y/N)
- [x] N — no PII involvement

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-040-001 | Worktree Lifecycle Fix: PendingCleanup Timing, TeamDelete Guard, Orphan Scanning, Cleanup Confirmation UX | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Rust implementation. All changes in codeflow-cli workspace. Clippy clean required. 85%+ per-file coverage via cargo llvm-cov.

### Autorun Batching

Not applicable — single task, not autorun eligible.

## Related

- INF-EPC-039: Worktree PID Detection and Cleanup UX (predecessor)
- INF-EPC-038: Fix worktree cleanup directory removal (predecessor)
- PR #242: Fix PID detection, centralized liveness, interactive cleanup UX
