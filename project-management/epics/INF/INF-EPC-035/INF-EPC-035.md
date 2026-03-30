---
id: "epic-01KMZXNG1DC41FB4M1WKG3NT5R"
format_id: "INF-EPC-035"
title: "Fix worktree stale eviction and config-driven limit enforcement"
summary: "Fix bugs in worktree stale cleanup safety and enforce config-driven worktree limits"
status: in_progress
area_type: INF
work_type: FIX
domain: GENL
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-30T17:46:34Z"
updated_at: "2026-03-30T17:46:34Z"
---

# INF-EPC-035: Fix worktree stale eviction and config-driven limit enforcement

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Fix two related bugs in the worktree subsystem: (1) stale worktree eviction was unsafe when cleanup failed mid-way, and (2) the maximum concurrent worktrees limit was hardcoded instead of being read from `parallel-work-config.json`.

## Scope

### In Scope

- Worktree stale cleanup safety improvements in `codeflow-cli/core/src/worktree/`
- Config-driven worktree limit enforcement reading from `parallel-work-config.json`
- Related tests for cleanup safety and limit loading

### Out of Scope

- Changes to the coordination/CRDT subsystem
- Changes to the autorun worker orchestration

## Acceptance Criteria

- [ ] Stale worktree eviction is safe and idempotent when cleanup fails mid-way
- [ ] Max concurrent worktree limit is read from `parallel-work-config.json` (not hardcoded)
- [ ] Existing worktree tests pass without regression
- [ ] New tests cover the fixed code paths

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-035-001 | fix: worktree stale eviction and config-driven limit enforcement | in_progress | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Stale cleanup and worktree limit enforcement are in `codeflow-cli/core/src/worktree/`. The config-driven limit should use `parallel-work-config.json` worktree section.

## Related

- PR #232: fix/worktree-stale-cleanup-safety (prior related fix)
