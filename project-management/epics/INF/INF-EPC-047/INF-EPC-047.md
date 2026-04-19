---
id: "epic-01KPHCP81KY13S9FRQBRZR20ST"
format_id: "INF-EPC-047"
title: "Status TUI Fixes — Layout, Refresh, Task Column, and Details Panel"
summary: "Fix codeflow interactive status and autorun status TUI dashboards: TASK column correctness, stale-row phase semantics, details panel live updates, column layout tuning, and auto-refresh reliability."
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
created_at: "2026-04-18T22:47:20Z"
updated_at: "2026-04-18T22:47:20Z"
---

# INF-EPC-047: Status TUI Fixes — Layout, Refresh, Task Column, and Details Panel

## Summary

Fix the `codeflow interactive status` and autorun status TUI dashboards. The current
implementation has multiple defects: the TASK column renders raw ULIDs instead of formatted IDs,
stale rows show incorrect phase information, the details panel does not update on selection
change, column widths are misaligned, and the auto-refresh loop can stall.

## Scope

### In Scope

- TASK column rendering (formatted ID, ULID fallback, `--` when no task)
- Adhoc task format_id assignment in cf-knowledge-layer registration path
- Stale-row STATUS and PHASE display semantics
- Details panel live update on selection change and refresh tick
- Column layout tuning for 120-col terminal
- Auto-refresh loop reliability and non-blocking data fetch
- Unit tests for all above

### Out of Scope

- Autorun batch orchestration changes
- Backend session tracking infrastructure changes
- New TUI features beyond the defects listed

## Acceptance Criteria

- [ ] TASK column renders formatted ID when present, truncated ULID when not, `--` when no task
- [ ] Adhoc registration assigns `{AREA}-TSK-adhoc-{seq}` format_id when area is inferable
- [ ] Stale rows show last-completed phase sentinel or `pre-pf1` (not `Starti...`)
- [ ] Details panel updates immediately on selection change and on refresh tick
- [ ] Column widths fit 120-col terminal without truncation
- [ ] Auto-refresh loop non-blocking; changes reflected within 2 ticks
- [ ] All modified files at ≥85% test coverage; clippy clean

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-047-001 | Fix codeflow status TUI — TASK column, layout, stale semantics, details panel, auto-refresh | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

The status TUI is implemented in the Rust CLI at `codeflow-cli/`. The interactive status
command is at `codeflow-cli/cli/src/commands/interactive/` (or similar). The autorun status
command shares some display logic. Review the existing session/worktree data structures before
changing display code.

### Autorun Batching

Not applicable — single task, not autorun-eligible.

## Related

- INF-EPC-046: Generic Testing Subsystem (test infrastructure used to verify this fix)
- PR #255: Autorun integration branch (touched autorun session state)
