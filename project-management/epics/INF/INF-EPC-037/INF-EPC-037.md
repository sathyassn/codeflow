---
id: "epic-01KN593Z780W7ET0XTJAAYBNG9"
format_id: "INF-EPC-037"
title: "Fix worktree cleanup subsystem — logic bug, typed status enum, locked writes"
summary: "Fix three bugs in the worktree cleanup subsystem: logic bug in cleanup path, typed status enum alignment, and missing locked writes"
status: complete
area_type: INF
work_type: FIX
domain: WORKTREE
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-01T19:43:02Z"
updated_at: "2026-04-01T23:53:52Z"
---

# INF-EPC-037: Fix worktree cleanup subsystem — logic bug, typed status enum, locked writes

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Fix three related bugs in the worktree cleanup subsystem: (1) a logic bug in the cleanup code path, (2) typed status enum issues in the worktree registry/status handling, and (3) missing locked writes for registry/state file mutations.

## Scope

### In Scope

- Worktree cleanup logic fixes in `codeflow-cli/core/src/worktree/cleanup.rs`
- Typed status enum fixes in `codeflow-cli/core/src/worktree/mod.rs` and `registry.rs`
- Locked write safety for registry mutations in `codeflow-cli/core/src/worktree/registry.rs`
- Related fixes in `setup.rs`, `cmd/worktree.rs`, `session_start.rs`, `post_tool_use.rs`
- Tests for all fixed code paths

### Out of Scope

- Changes to the coordination/CRDT subsystem
- Changes to the autorun worker orchestration

## Acceptance Criteria

- [ ] Logic bug in cleanup path is fixed with no regression in existing cleanup tests
- [ ] Typed status enum is properly used throughout the worktree subsystem
- [ ] Registry mutations use locked writes (file-level locking) to prevent concurrent corruption
- [ ] All modified files pass clippy --all-targets --all-features -- -D warnings
- [ ] Existing worktree tests pass without regression
- [ ] New tests cover all fixed code paths with 85%+ per-file coverage

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The three bugs are independent and can be fixed in a single task:
1. Logic bug — trace the cleanup code path for force/dry-run/prune scenarios
2. Typed enum — audit for string literals vs typed enum variants in registry status fields
3. Locked writes — audit `write_registry` and mutation helpers for file-lock usage; add locking where missing using the `file_lock.rs` pattern (same as `locked_binary_rmw` for state.loro)

Reference: `codeflow-cli/core/src/file_lock.rs` for the locking pattern.

## Related

- PR #238: fix/worktree-cleanup-consolidation (merged) — previous worktree cleanup work this builds on
- INF-EPC-035: Fix worktree stale eviction and config-driven limit enforcement (complete)
- INF-EPC-036: Add dirty worktree guard to push/PR gate (in progress, separate session)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-037-001 | Fix worktree cleanup subsystem — logic bug, typed status enum, locked writes | complete | normal |
