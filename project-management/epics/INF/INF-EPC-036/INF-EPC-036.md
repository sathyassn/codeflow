---
id: "epic-01KN52K771SW11DNVY8FMMWCW8"
format_id: "INF-EPC-036"
title: "Add dirty worktree guard to push/PR gate"
summary: "Block git push when worktree has uncommitted changes by adding dirty-file check to check_cumulative_push_pr_gate()"
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
created_at: "2026-04-01T17:50:02Z"
updated_at: "2026-04-01T17:50:02Z"
---

# INF-EPC-036: Add dirty worktree guard to push/PR gate

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Add a dirty worktree check to `check_cumulative_push_pr_gate()` in `pre_tool_use.rs` so that `git push` and `gh pr` commands are blocked when the worktree has uncommitted changes. This prevents pushing a partial/dirty state, which is a correctness and safety issue.

## Scope

### In Scope

- Adding `has_dirty_files()` check inside `check_cumulative_push_pr_gate()` in `pre_tool_use.rs`
- Returning a clear blocking error message when dirty files are detected
- Unit tests for the new guard behavior

### Out of Scope

- Changes to `rescue_uncommitted_work()` or other cleanup logic
- Changes to the `has_dirty_files()` helper itself

## Acceptance Criteria

- [ ] `check_cumulative_push_pr_gate()` blocks `git push` when worktree has uncommitted changes
- [ ] Blocking error message identifies the cause as uncommitted changes
- [ ] All existing push/PR gate tests continue to pass
- [ ] New tests cover the dirty-guard path (blocked) and clean path (allowed)

### PII Handling Review

- [ ] Does this epic involve code that handles PII? N

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-036-001 | Add dirty worktree guard to push/PR gate | complete | normal |

## Related

- None

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The `has_dirty_files()` function already exists in `cleanup.rs` (line 381). The fix is to expose or re-implement the check in `pre_tool_use.rs` and call it from `check_cumulative_push_pr_gate()`. Since `cleanup.rs` is in the `worktree` module and `pre_tool_use.rs` is in the `hooks` module, the function may need to be made `pub` or a thin wrapper added to the worktree module's public API.

### Autorun Batching

N/A — single task, not autorun-eligible.
