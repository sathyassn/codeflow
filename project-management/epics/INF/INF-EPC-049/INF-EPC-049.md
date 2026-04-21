---
id: "epic-01KPPHGD2QPFH5NESG26XN0PH8"
format_id: "INF-EPC-049"
title: "Status TUI Time/Duration, Layout, and Source Filtering Fixes"
summary: "Follow-up to INF-EPC-048 (PR #300). Propagates the freeze-on-terminal pattern to all TUI time/duration displays; fixes autorun batches layout and empty TASK column; adds detail-view refresh; prevents cross-source contamination between autorun and interactive status TUIs."
status: complete
area_type: "INF"
work_type: "FIX"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-20T23:00:00Z"
updated_at: "2026-04-20T23:15:00Z"
---

# INF-EPC-049: Status TUI Time/Duration, Layout, and Source Filtering Fixes

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Follow-up to INF-EPC-048 (PR #300). PR #300 introduced the freeze-on-terminal elapsed pattern for the `codeflow autorun status` batches LIST view only. This epic propagates the same pattern to every remaining time/duration display across both the `codeflow autorun status` and `codeflow interactive status` TUIs, and resolves collateral bugs found during the audit: autorun batches list column layout imbalance, empty TASK column on terminal rows, missing force-refresh keybinding + last-updated indicator in both detail views, cross-source contamination where autorun worker sessions appear in the interactive status TUI, and a session-local filter so the interactive status default view no longer shows historical stale noise.

## Scope

### In Scope

- Propagate PR #300 freeze-on-terminal pattern to: autorun batch detail header elapsed, autorun text-mode status elapsed.
- Interactive list DURATION + detail Duration: already frozen via `duration_secs` — verify, keep.
- Refactor common helper so autorun and interactive share a single freeze-on-terminal implementation where architecturally sound.
- Add `session_kind` field to `interactive_session` (new migration). Set from `AUTORUN_SESSION_ID` at write time. Filter interactive status to `session_kind = 'interactive'`.
- Interactive status default list: session-scoped (active + terminations during this TUI's lifetime). Add `[s]` keybinding to toggle "show stale backlog".
- Fix async stale-promotion race: make the completed_at write synchronous before returning from fetch, eliminating the "00:01 blip".
- `codeflow interactive cleanup` UPDATE: add `completed_at = $now` alongside `updated_at`.
- Autorun batches list layout: cap BATCH (proposed `Max(36)`), tune other column widths.
- TASK column in autorun batches list: preserve `current_task_id` on task finish, render `task_format_id` preferred with `task_id` fallback so terminal rows show which task the batch was on.
- Both detail views: add `[r]` force-refresh keybinding and `[last updated Ns ago]` footer span.

### Out of Scope

- Changing the text-mode output shape for non-elapsed columns.
- Detail-view background watch channel (known limitation, tracked separately).

## Acceptance Criteria

- [ ] All individual task ACs pass (see INF-TSK-049-001).
- [ ] `codeflow test --mode full` with 2+ consecutive clean runs, per-file coverage ≥85% for modified files.
- [ ] PR body contains the full `## Test Results` 5-subsection section.

### Motivation

Users running autorun batches cannot rely on the TUI to report elapsed time: the batch-detail header and the text-mode `autorun status` surface both tick indefinitely even on terminal batches. Interactive status is polluted by autorun workers (Claude sessions inside autorun worktrees register as interactive sessions) and dominated by historical stale rows, making it unusable as an at-a-glance dashboard. Detail views give no visual indication of data freshness and no way to force a refresh.

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-049-001 | Propagate freeze-on-terminal TUI pattern, fix layout/TASK column/detail refresh/source filtering | complete | high |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Predecessor: INF-EPC-048 (closed, PR #300 @ aa8a049e). The freeze-on-terminal helper introduced in PR #300 for the autorun batches LIST view is the canonical implementation to propagate. All new freeze-on-terminal logic should call the same helper where architecturally sound rather than duplicating logic.

### Autorun Batching

**Execution Order:** INF-TSK-049-001 (single task, no parallelism needed)

**Batch Groups:**

| Batch | Tasks | max_workers | Notes |
|-------|-------|-------------|-------|
| 1 | INF-TSK-049-001 | 1 | Single Rust crate, all changes coupled |

**Estimated Duration:** L (full day)

## Related

- INF-EPC-048 (PR #300) — predecessor, freeze-on-terminal pattern origin
- INF-EPC-047 (PR #297) — status TUI fixes round 1
