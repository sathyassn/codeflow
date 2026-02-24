---
id: "epic-01KJ60EJNVRG64W6MEVCJ0P1DR"
format_id: "INF-EPC-014"
title: "Stale Session PID-Based Cleanup and Teammate Detection"
summary: "Replace 24h time-based stale session cleanup with PID-based teammate detection, add pathflow-team.json bridge, and update worktree symlinks for selective hook access"
status: complete
area_type: "INF"
work_type: "FIX"
domain: "ENFC"
is_ongoing: false
file_scope:
  - ".claude/hooks/codeflow/session-start/"
  - ".claude/hooks/codeflow/session-end/"
  - ".codeflow/scripts/worktree/"
  - ".codeflow/docs/analysis/"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-23"
updated_at: "2026-02-23"
---

# INF-EPC-014: Stale Session PID-Based Cleanup and Teammate Detection

## Summary

The previous stale session cleanup (PR #63) replaced 24h auto-cleanup with warning-only detection. This epic implements the next evolution: PID-based teammate detection using a `pathflow-team.json` bridge file written at session start that maps teammate agent IDs to PIDs. The SessionStart hook can then detect truly stale sessions (teammates dead) vs live ones (teammates running). Additionally, worktree setup is updated to use selective `.state/` symlinks (shared directories symlinked to main repo; local directories created fresh per-worktree), and a `.codeflow/docs/analysis/` doc captures the design rationale.

## Scope

### In Scope

- `pathflow-team.json` bridge file schema and write location (`.state/session/{SID}/pathflow/`)
- SessionStart hook update: write PID at session start, detect stale via PID liveness check
- SessionEnd hook: clean up `pathflow-team.json` on normal exit
- Worktree setup script: selective `.state/` symlinks (shared: db, ledger, registry, backups, coordination, logs; local per-worktree: runtime, session, sentinels)
- Design documentation in `.codeflow/docs/analysis/`

### Out of Scope

- `.state/` directory reorganization (INF-EPC-015, future)
- Go CLI integration
- Multi-worktree coordination beyond symlink fix

## Acceptance Criteria

- [ ] pathflow-team.json written to `.state/session/{SID}/pathflow/pathflow-team.json` at session start with teammate PID entries
- [ ] SessionStart hook reads existing pathflow-team.json to detect live vs stale teammates (PID kill -0 check)
- [ ] SessionEnd hook removes pathflow-team.json on clean exit
- [ ] Worktree setup uses selective `.state/` symlinks: shared dirs (db, ledger, registry, backups, coordination, logs) symlinked to main repo; local dirs (runtime, session, sentinels) created fresh per-worktree
- [ ] Design analysis doc written to `.codeflow/docs/analysis/`
- [ ] All existing hook tests pass; new tests added for PID detection logic

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-014-001 | Implement PID-based stale session cleanup and teammate detection | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- INF-EPC-015 (.state/ reorg — future, depends on this establishing stable session paths)

## Technical Notes

- PID detection approach: write `{"agent_id": "{id}", "pid": $$}` at session start; on next session, `kill -0 $pid` to test liveness
- pathflow-team.json is ephemeral (not git-tracked); cleaned up at SessionEnd
- Worktree selective `.state/` symlinks: shared dirs (db, ledger, registry, backups, coordination, logs) symlinked to main repo; local dirs (runtime, session, sentinels) created fresh per-worktree (not symlinked)

## Related

- PR #63: fix/stale-session-cleanup-and-checkpoint-docs (predecessor — replaced auto-cleanup with warning)
- INF-EPC-013: Infrastructure Bug Fixes (completed, same domain)
- INF-EPC-015: .state/ directory reorganization (future)
