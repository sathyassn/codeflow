---
id: "epic-01KMS2RKEYY9T0CR3JHGMHVAB7"
format_id: "INF-EPC-032"
title: "Multi-Session Worktree Isolation Fix"
summary: "Fix two bugs preventing independent Claude Code windows from getting their own worktrees; add design doc; renumber INF-EPC-922 to INF-EPC-029."
status: complete
area_type: "INF"
work_type: "FIX"
domain: "GENL"
is_ongoing: false
file_scope:
  - "codeflow-cli/core/src/hooks/session_start.rs"
  - "codeflow-cli/core/src/hooks/post_tool_use.rs"
  - "codeflow-cli/core/src/hooks/mod.rs"
  - ".codeflow/docs/analysis/parallel-work/multi-session-worktree-isolation.md"
  - "project-management/epics/INF/INF-EPC-922/"
  - "project-management/epics/INF/INF-EPC-029/"
priority: high
pr_number: 227
external_id: null
external_url: null
created_at: "2026-03-28T02:01:05Z"
updated_at: "2026-03-28T02:10:00Z"
---

# INF-EPC-032: Multi-Session Worktree Isolation Fix

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Two bugs in the worktree setup path prevent independent Claude Code windows from receiving their own isolated worktrees. The `handle_stale_cleanup()` function produces false-positive teammate detections, causing lead sessions to be treated as teammates and skip worktree creation. The `detect_precreated_worktree_inner()` function reuses stale worktrees from prior sessions instead of creating fresh ones. This epic fixes both bugs, adds a design document for multi-session isolation, and renumbers the missequenced INF-EPC-922 epic to INF-EPC-029.

## Scope

### In Scope

- Fix teammate false positive in `handle_stale_cleanup()` in `session_start.rs`
- Fix stale worktree reuse in `detect_precreated_worktree_inner()` in `post_tool_use.rs` or `mod.rs`
- Create design document at `.codeflow/docs/analysis/parallel-work/multi-session-worktree-isolation.md`
- Renumber INF-EPC-922 directory and markdown to INF-EPC-029

### Out of Scope

- Changes to claim/CRDT coordination logic
- Worktree registry schema changes
- CI/CD pipeline changes

## Acceptance Criteria

- [ ] `handle_stale_cleanup()` correctly distinguishes lead sessions from teammates
- [ ] `detect_precreated_worktree_inner()` does not reuse worktrees from prior sessions
- [ ] Design document created and committed
- [ ] INF-EPC-922 renumbered to INF-EPC-029 (on-disk directory + markdown + DB)
- [ ] All existing tests pass after changes
- [ ] New unit tests added for fixed functions

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-032-001 | Multi-session worktree isolation fix + design doc + epic renumber | complete | high |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The two bugs manifest together during a second Claude Code window startup:
1. `handle_stale_cleanup()` reads `lead_pid` from `pathflow-team.json` of the first session and detects the first lead as alive, incorrectly flagging the second session as a teammate.
2. `detect_precreated_worktree_inner()` finds the first session's worktree registered in `worktrees.yaml` and returns it as "pre-created", so the second session inherits the first session's worktree.

Fix requires scoping stale detection to the current session and validating worktree ownership before reuse.

## Related

- PR #226: hotfix/worktree-heartbeat-safety (immediate predecessor)
- INF-EPC-922 (to be renumbered INF-EPC-029)
