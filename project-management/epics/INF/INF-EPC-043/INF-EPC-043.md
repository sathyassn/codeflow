---
id: "epic-01KNMAQKK5V2EG1TJG1XZV4B6S"
format_id: "INF-EPC-043"
title: "Fix Interactive Session Implementation Gaps"
summary: "Fix 13 gaps in the codeflow interactive command: gitignore, heartbeat lifecycle, cleanup comprehensiveness, and stale artifacts."
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
created_at: "2026-04-07T15:59:55Z"
updated_at: "2026-04-07T15:59:55Z"
---

# INF-EPC-043: Fix Interactive Session Implementation Gaps

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Fixes 13 gaps in the `codeflow interactive` command implementation introduced in PR #258. Covers gitignore entries, heartbeat lifecycle management, comprehensive cleanup of all filesystem artifacts, stale PID env files, session-worktree-map stale entries, main repo session directory sweeping, DB-only cleanup gaps, session.lock migration artifact, worktree registry stale entries, and session log accumulation.

## Scope

### In Scope

- `.gitignore` entry for `.state/interactive`
- `LOCAL_STATE_DIRS` in `worktree/mod.rs` missing `interactive`
- Interactive heartbeat dead code (no readers, no cleanup on session end)
- Interactive heartbeat cleanup by stale sweep
- `codeflow interactive status/cleanup` wired to filesystem fallback via heartbeat
- `codeflow interactive cleanup` sweeping all filesystem artifacts (heartbeats, PID env files, session dirs, sentinel dirs, worktree map entries)
- Dead-PID env file sweep
- `session.lock` migration artifact fix
- Tests for new cleanup logic

### Out of Scope

- Changes to the interactive command feature set
- Autorun subsystem changes
- Worktree CRDT coordination changes

## Acceptance Criteria

- [ ] `.state/interactive` is gitignored
- [ ] `interactive` added to `LOCAL_STATE_DIRS` in `worktree/mod.rs`
- [ ] Interactive heartbeat cleaned on session end
- [ ] Interactive heartbeat cleaned by stale sweep
- [ ] Interactive heartbeat wired up as filesystem fallback in `codeflow interactive status/cleanup`
- [ ] `codeflow interactive cleanup` sweeps filesystem artifacts (heartbeats, PID env files, session dirs, sentinel dirs, worktree map)
- [ ] Dead-PID env file sweep added
- [ ] `session.lock` migration artifact fixed
- [ ] All existing stale artifacts cleaned by new code paths
- [ ] Tests for new cleanup logic

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-043-001 | Fix interactive session implementation gaps | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

This epic addresses post-merge gap analysis from the interactive command PR (#258). The fixes are focused on correctness and completeness of the cleanup subsystem — no new features.

### Autorun Batching

Not applicable — single task, interactive session.

## Related

- PR #258 (feat/interactive-command — merged)
- INF-EPC-042 (worktree cleanup improvements)
