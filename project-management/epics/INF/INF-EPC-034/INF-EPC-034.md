---
id: "epic-01KMXM44BF6PYMMGVH951NV4F7"
format_id: "INF-EPC-034"
title: "Worktree Path Resolution Architecture Fix + Autorun Gap Fixes"
summary: "Fix per-PID env files for detect_project_dir(), CLAUDE_ENV_FILE propagation, session pointer for cross-session discovery, missing ledger dir, autorun session ID mismatch, merge queue docs, autorun resume command, and minor autorun fixes."
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
created_at: "2026-03-29T20:00:00Z"
updated_at: "2026-03-29T20:00:00Z"
---

# INF-EPC-034: Worktree Path Resolution Architecture Fix + Autorun Gap Fixes

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic addresses the remaining worktree path resolution architecture gaps and autorun subsystem issues identified after PR #230. Key fixes include per-PID env files for `detect_project_dir()`, `CLAUDE_ENV_FILE` propagation in SessionStart, session pointers for cross-session teammate/compact/stale-sweep discovery, missing ledger directory creation in worktrees, autorun session ID mismatch (worker_sid vs batch session_id), merge queue documentation, autorun resume command, and minor autorun operational fixes.

## Scope

### In Scope

- Per-PID env file written/read for worktree path resolution
- CLAUDE_ENV_FILE export in SessionStart hook
- Session pointer file in main repo for cross-session discovery
- Ledger directory creation in worktree setup
- AUTORUN_SESSION_ID fix (use worker_sid not batch session_id)
- Merge queue documented as advisory (not blocking)
- Autorun resume command implementation
- Minor autorun fixes: sync daemon auto-start, dead config field removal, logs follow, status watch, batch listing

### Out of Scope

- New CRDT coordination features
- Changes to session ID format
- Test suite infrastructure changes

## Acceptance Criteria

- [ ] Per-PID env file written/read for worktree path resolution (replaces shared fallback)
- [ ] CLAUDE_ENV_FILE exports set in SessionStart
- [ ] Session pointer written to main repo for teammate/compact/stale-sweep discovery
- [ ] Ledger directory created in worktree
- [ ] AUTORUN_SESSION_ID uses worker_sid not batch session_id
- [ ] Merge queue documented as advisory
- [ ] Autorun resume command implemented
- [ ] Minor autorun fixes (sync daemon auto-start, dead config field, logs follow, status watch, batch listing)
- [ ] Architecture documentation written
- [ ] All existing tests pass + new tests per fix
- [ ] Clippy clean, cargo fmt clean, 85% per-file coverage on modified files

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-034-001 | Worktree path resolution architecture fix + autorun gap fixes | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Fixes the architecture gap where `detect_project_dir()` uses a shared env file fallback, which breaks in parallel worktree sessions. The per-PID approach eliminates the race condition. CLAUDE_ENV_FILE propagation ensures all hook invocations resolve to the correct worktree context.

### Autorun Batching

**Execution Order:** Single task, sequential

**Batch Groups:** N/A

| Batch | Tasks | max_workers | Notes |
|-------|-------|-------------|-------|
| 1 | INF-TSK-034-001 | 1 | Sequential implementation |

## Related

- PR #228: fix/worktree-project-dir-resolution
- PR #229: fix/autorun-subsystem-overhaul
- PR #230: fix/worktree-protection-guard-enhancement
- INF-EPC-030: PostToolUse Worktree Path Resolution and CLI Fixes
- INF-EPC-033: Fix autorun subsystem
