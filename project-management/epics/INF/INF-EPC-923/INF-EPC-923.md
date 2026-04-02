---
id: "epic-01KN7CB1H9PQT1MWXA1EV7RPMB"
format_id: "INF-EPC-923"
title: "Fix worktree cleanup directory removal"
summary: "Fix worktree cleanup to actually delete .git-worktrees/ directories and remove WorktreeStatus::Removed tombstone state"
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
created_at: "2026-04-02T15:17:54Z"
updated_at: "2026-04-02T15:17:54Z"
---

# INF-EPC-923: Fix worktree cleanup directory removal

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Fix worktree cleanup to actually delete directories from `.git-worktrees/` and remove the `WorktreeStatus::Removed` tombstone state. Directories are not being deleted during cleanup, and the `Removed` status creates unrecoverable registry entries that accumulate over time.

## Scope

### In Scope

- Fix `cleanup.rs` to actually remove `.git-worktrees/worktree-{SID}/` directories
- Remove `WorktreeStatus::Removed` tombstone state from registry and replace with immediate deletion
- Ensure `session_start.rs` worktree setup handles the absence of the `Removed` state correctly
- Fix `mod.rs` and `registry.rs` to reflect the simplified status model

### Out of Scope

- Changes to worktree isolation logic or CRDT coordination
- New worktree features

## Acceptance Criteria

- [ ] `codeflow worktree cleanup` deletes directories from `.git-worktrees/`
- [ ] `WorktreeStatus::Removed` is eliminated; entries are deleted from registry on cleanup
- [ ] No unrecoverable worktree entries accumulate in `worktrees.yaml`
- [ ] Existing tests pass; new tests cover directory removal

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-923-001 | fix: worktree cleanup directory removal | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The `WorktreeStatus::Removed` state was introduced as a tombstone but has no recovery path — once an entry is `Removed`, cleanup does not delete the actual directory, leaving orphaned directories in `.git-worktrees/`. The fix should:
1. Remove the `Removed` variant from `WorktreeStatus`
2. Have `cleanup_worktree()` actually call `fs::remove_dir_all()` on the worktree path
3. Remove entries from `worktrees.yaml` after successful directory deletion (not mark as `Removed`)

## Related

- PR #240: worktree cleanup subsystem improvements (prior work)
- PR #238: worktree cleanup consolidation
