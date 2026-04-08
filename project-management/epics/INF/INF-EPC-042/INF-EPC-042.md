---
id: "epic-01KNBE4SXQJDG3QQA4JPDY8YS0"
format_id: "INF-EPC-042"
title: "Fix worktree cleanup output, classification, and force-id scoping"
description: "Fix 8 issues in worktree cleanup: single classify_entry function, force_names scoping, configurable Unknown threshold, status-aware classification, unified output, dry-run orphan reporting, force-id orphan message"
status: in_progress
area_type: INF
work_type: FIX
origin: adhoc
created_at: "2026-04-04T05:07:00.000Z"
updated_at: "2026-04-04T06:30:00.000Z"
---

# INF-EPC-042: Fix worktree cleanup output, classification, and force-id scoping

## Summary

Fix 8 findings in the worktree cleanup command covering output formatting, entry classification, force-id scoping, configurable thresholds, and dry-run orphan reporting.

## Tasks

| ID | Title | Status |
|----|-------|--------|
| INF-TSK-042-001 | Fix worktree cleanup output, classification, and force-id scoping | complete |
| INF-TSK-042-002 | Fix worktree cleanup log residue and missing source field in registry | complete |

## Scope

Fix worktree cleanup log residue and missing source field in the worktrees.yaml registry:

- `write_cleanup_log()` in `session_end.rs` must write to the main repo `.state/logs/sessions/` path, not the worktree-local path (which is destroyed during cleanup)
- `interactive.rs` must populate the `source` field when creating `WorktreeEntry` records so the registry can identify how each worktree was created

## Acceptance Criteria

1. No orphaned directories left after worktree cleanup
2. `source` field populated for all worktree creation paths in `interactive.rs`
3. Existing tests pass (`cargo test -p codeflow-core`, `cargo test -p codeflow-cli`)
4. New tests cover both fixes (cleanup log path and source field)

## Dependencies

None

## Technical Notes

- `session_end.rs` `write_cleanup_log()`: path must resolve via main repo root (`CF_PROJECT_ROOT` / `detect_project_dir()`), not the worktree path which is about to be removed
- `interactive.rs`: check `WorktreeEntry` struct in `registry.rs` for the exact `source` field type before implementing
- Related to INF-EPC-038 worktree cleanup work; does not overlap with INF-TSK-042-001 (different files and issues)

## Related

- INF-EPC-038: Worktree cleanup directory removal and tombstone elimination (merged PR #241)
