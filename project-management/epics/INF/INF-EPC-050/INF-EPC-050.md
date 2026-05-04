---
id: "epic-01KQ5ZTFVPW1AHX0G72XR0F8RT"
format_id: "INF-EPC-050"
title: "Comprehensive Autorun + Interactive Status Correctness Sweep"
summary: "Fix 23 (now 23 + 28) confirmed correctness issues across autorun session lifecycle, interactive status display, TUI controls, liveness detection, and hook permission suppression. TSK-050-001 (PR #309) shipped initial 23 AC; follow-up audit (TSK-050-002) surfaced 28 additional issues split across Tier-A (TSK-050-003), Tier-B (TSK-050-004), Tier-C (TSK-050-005)."
status: in_progress
area_type: INF
work_type: FIX
domain: ops
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-25T00:00:00Z"
updated_at: "2026-05-03T00:00:00Z"
---

# INF-EPC-050: Comprehensive Autorun + Interactive Status Correctness Sweep

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic consolidates 23 confirmed correctness issues across the autorun and interactive status subsystems that were surfaced during PRs #300, #302, #307 and post-merge audits of INF-EPC-047, INF-EPC-048, and INF-EPC-049. Rather than applying piecemeal patches, this epic performs a comprehensive sweep across six issue layers: schema/lifecycle enforcement, reaper/liveness detection, display semantics (TUI format_id), autorun TUI controls, interactive CLI correctness, and hook permission suppression for autorun workers.

## Scope

### In Scope

- Autorun session status enum canonicalization and state-machine enforcement
- Stale/reaper detection rewritten to use PID liveness instead of age thresholds
- TUI TASK column resolution to format_id (not raw ULID) for all autorun and batch views
- Autorun TUI: freeze-on-terminal propagation, [r]/[s] keybindings, IDLE/running display semantics
- Interactive session: heartbeat-based liveness, `codeflow interactive status/list/cleanup` correctness
- `codeflow autorun status` exit code semantics and `codeflow autorun resume` skip logic
- Configurable reaper timeout via parallel-work-config.json
- Hook permission suppression: network.rs AUTORUN_SESSION_ID check, PermissionRequest matcher expansion
- pathflow-events.jsonl write path correctness in autorun workers (per-worktree ledger)
- Full test suite verification (AC-23)

### Out of Scope

- New autorun features or workflow changes beyond correctness fixes
- Changes to CRDT coordination layer or merge queue
- TUI visual redesign (layout changes only where required for AC-14)
- Changes to autorun batch file format or scheduling

## Acceptance Criteria

> **Chain-coverage requirement:** Epic acceptance criteria MUST cover the full delivery chain, not just top-level outcomes.

- [ ] All 23 task-level acceptance criteria verified PASS by WS-QA
- [ ] `codeflow test --mode full` reports zero failures and zero coverage regressions
- [ ] No existing autorun integration tests regress
- [ ] TUI TASK column shows format_id (not ULID) verified by manual inspection
- [ ] Autorun worker lifecycle: pending -> running -> complete/failed verified by integration test
- [ ] Reaper does not kill live workers: verified by process liveness check in test

### PII Handling Review

- [ ] Does this epic involve code that handles PII? N
- N/A: no PII handling in autorun/TUI/hooks subsystems

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-050-001 | Comprehensive Autorun + Interactive Status Correctness Sweep | complete | high |
| INF-TSK-050-002 | EPC-050 follow-up planning (analysis + Tier A/B/C task docs + template + agent defs) | complete | high |
| INF-TSK-050-003 | Tier-A autorun + interactive correctness fixes (chokepoint discipline + DB recovery + final-PR gate) | complete | high |
| INF-TSK-050-004 | Tier-B autorun worker robustness fixes (tmux/claim retries, protected-branch failures, DB CREATE failures, pre-PR conflict check) | todo | high |
| INF-TSK-050-005 | Tier-C deferred design notes (4 ADRs + 3 design notes) | todo | normal |
| INF-TSK-050-006 | (placeholder) Implement ADR-001 CI timeout retry policy | planned | normal |
| INF-TSK-050-007 | (placeholder) Implement ADR-002 integration branch reset semantics | planned | normal |
| INF-TSK-050-008 | (placeholder) Implement ADR-003 sync daemon supervision | planned | normal |
| INF-TSK-050-009 | (placeholder) Implement ADR-004 reaper daemonization | planned | normal |
| INF-TSK-050-010 | (placeholder) Implement DESIGN-005 env file write hardening | planned | normal |
| INF-TSK-050-011 | (placeholder) Implement DESIGN-006 stage timeout polling cadence | planned | normal |
| INF-TSK-050-012 | (placeholder) Implement DESIGN-007 periodic-task stop-signal pattern | planned | normal |

## Analysis

- [Comprehensive Autorun + Interactive Audit (2026-05-03)](analysis/2026-05-03-autorun-interactive-comprehensive-audit.md) -- captures 28 follow-up findings driving TSK-050-003/004/005

## Dependencies

### Blocked By

- None

### Blocks

- INF-EPC-024 autorun batch execution (blocked by permission prompts until AC-21/AC-22 fixed)

## Technical Notes

This epic addresses root causes identified during INF-EPC-024 autorun reliability work. The reaper race condition (worktrees killed mid-execution) and permission prompt blocks were the primary blockers preventing any INF-EPC-024 autorun batch from completing successfully.

Key architectural constraints:
- Heartbeat file writes must be atomic (avoid partial reads by liveness checker)
- Status enum changes must be backward-compatible with existing DB entries
- Reaper liveness check must handle startup race: worker starts, PID not yet written to disk
- PermissionRequest matcher changes in settings.json must be tested against hook test suite

### Autorun Batching

Not applicable — this epic contains a single interactive task (not autorun-eligible, requires human review of TUI changes).

## Related

- INF-EPC-024: Autorun reliability parent epic (blocked by this)
- INF-EPC-047: Status TUI Fixes (predecessor — partial fixes)
- INF-EPC-048: Autorun Reliability P0 (predecessor — partial fixes)
- INF-EPC-049: TUI freeze + rescue XDG (predecessor — partial fixes)
- PR #300: Autorun Reliability P0 (merged 2026-04-20)
- PR #302: TUI freeze propagation + rescue XDG (merged 2026-04-21)
- PR #307: Autorun reliability P1 (merged)
