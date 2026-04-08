---
id: "epic-01KNNDF0R0XGX4ERCKVBHJJ9FP"
format_id: "INF-EPC-044"
title: "Autorun Reliability Fixes and TUI Terminal Experience"
summary: "Fix autorun worker reliability issues (post-timeout PR detection, sentinel-aware completion, panic guard, cleanup retry) and build TUI terminal experience (autorun dashboard, interactive session monitor, onboarding wizard) using ratatui"
status: planning
area_type: "INF"
work_type: "PLAN"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-08T02:00:00Z"
updated_at: "2026-04-08T02:00:00Z"
---

# INF-EPC-044: Autorun Reliability Fixes and TUI Terminal Experience

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic covers two related infrastructure improvement areas:

1. **Autorun worker reliability fixes** -- six targeted fixes to the autorun worker subsystem: post-timeout PR detection and merge, PathFlow-aware session completion via sentinel polling, pre-PR rebase in agent definitions, configurable per-task timeout override, worker panic guard with resource cleanup, and worktree cleanup retry with fallback deregister.

2. **TUI terminal experience** -- build a shared ratatui-based TUI module and three consumer features: interactive autorun status dashboard (`codeflow autorun status --watch`), interactive session monitor (`codeflow interactive status --watch`), and full onboarding wizard (`codeflow init`). Design follows an elegant, minimal aesthetic using Unicode symbols (no emoji), Rounded borders, and a green/red/yellow/dim color palette.

Additionally: queue lifecycle improvements (pending task abort, graceful wrap-up signal, DB update ordering) and interactive session DB field population (branch, work_type, team_name).

## Scope

### In Scope

- Autorun worker reliability: post-timeout PR detection, sentinel-aware completion, panic guard, cleanup retry, configurable timeout
- Queue lifecycle: pending task cancel, graceful wrap-up signal, DB ordering fixes
- Interactive session management: DB field population (branch, work_type, team_name)
- TUI foundation: shared tui/ module (data.rs, theme.rs, widgets/) in codeflow-core
- TUI consumers: autorun status dashboard, interactive session dashboard, onboarding wizard, welcome screen
- Pre-PR rebase instruction in cf-git-operations agent definition
- Analysis document covering TUI architecture and design decisions

### Out of Scope

- Session ID consolidation (covered by INF-EPC-024)
- PID detection three-layer fix / worktree session latch bug (separate epic -- see MEMORY.md)
- Worktree isolation changes (covered by separate epics)
- Embedded terminal via PTY + vte (future Epic D -- documented in analysis only)
- Graphical App layer (INF-EPC-027)
- CI/CD pipeline changes
- Phase 2 resilience items (deferred): merge queue stuck detection and auto-dequeue, orphan tmux session cleanup on batch start, resume robustness (missing batch file recovery, partial cleanup handling, stale integration branch force-update)

## Acceptance Criteria

- [ ] Analysis document at `.codeflow/docs/analysis/tui-terminal-experience.md` covering ratatui capabilities, three TUI use cases, shared module architecture, and design guidelines
- [ ] INF-TSK-044-002: Post-timeout PR detection, sentinel polling, pre-PR rebase, configurable timeout, panic guard, cleanup retry -- all functional with tests
- [x] INF-TSK-044-003: Pending task cancel, wrap-up signal, DB ordering fix, interactive session DB fields -- all functional with tests
- [ ] INF-TSK-044-004: ratatui dependency added, shared tui/ module (data, theme, widgets) created, `codeflow autorun status --watch` renders interactive TUI dashboard with keyboard navigation
- [ ] INF-TSK-044-005: `codeflow interactive status --watch` TUI dashboard, `codeflow init` 7-step wizard, `codeflow welcome` formatted display -- all functional
- [ ] All modified Rust files pass `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] All modified Rust files pass `cargo fmt --check`
- [ ] Code coverage >= 85% per-file for all new and modified Rust files
- [ ] Unicode symbols only in TUI (no emoji). Rounded borders. Green/red/yellow/dim palette

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority | Estimate | Depends On |
|----|-------|--------|----------|----------|------------|
| INF-TSK-044-001 | Epic planning and analysis | complete | high | M | -- |
| INF-TSK-044-002 | Autorun Worker Reliability Fixes | complete | high | L | 001 |
| INF-TSK-044-003 | Queue Lifecycle and Session Management | complete | high | L | 002 |
| INF-TSK-044-004 | TUI Foundation and Autorun Status Dashboard | todo | high | L | 003 |
| INF-TSK-044-005 | Interactive Session TUI and Onboarding Wizard | todo | normal | L | 004 |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

**Dependency chain:** Tasks are strictly sequential: 001 (planning) -> 002 (worker fixes) -> 003 (queue/session) -> 004 (TUI foundation) -> 005 (interactive TUI/wizard). This ordering is required because:
- Task 3 depends on Task 2's sentinel polling infrastructure for the wrap-up signal
- Task 4 depends on Task 3's interactive session DB fields and pending abort support
- Task 5 depends on Task 4's shared tui/ module

**Key architecture decisions:**
- ratatui v0.29 with bundled crossterm (no separate crossterm dep)
- Shared tui/ module in codeflow-core behind optional `tui` feature flag
- TUI widgets render via ratatui Buffer for testability
- Unicode symbols only (no emoji) -- checkmark, cross, bullet, circle, arrow, triangle
- Rounded borders, green/red/yellow/dim color palette

**File path correction from analysis:** The team lead's original assignment referenced `autorun/autorun.rs` but the actual invoke loop, serialized merge, and cleanup code lives in `autorun/worker.rs` (3034 lines). The `autorun/mod.rs` file only contains re-exports. Task tickets use the corrected paths.

### Autorun Batching

All tasks are NOT autorun-eligible. They require interactive development due to:
- Task 2: Agent definition edits (.claude/agents/cf-git-operations.md) require careful manual review
- Task 3: Hook modifications (pre_tool_use.rs, post_tool_use.rs) are critical system components
- Task 4: TUI development requires visual verification in a real terminal
- Task 5: Wizard development requires interactive testing of all 7 steps

**Execution Order:** 001 -> 002 -> 003 -> 004 -> 005 (fully sequential)

**Batch Groups:**

| Batch | Tasks | max_workers | Notes |
|-------|-------|-------------|-------|
| 1 | INF-TSK-044-001 | 1 | Planning task (this session) |
| 2 | INF-TSK-044-002 | 1 | Interactive: worker reliability fixes |
| 3 | INF-TSK-044-003 | 1 | Interactive: queue lifecycle + session mgmt |
| 4 | INF-TSK-044-004 | 1 | Interactive: TUI foundation + autorun dashboard |
| 5 | INF-TSK-044-005 | 1 | Interactive: session TUI + wizard |

**Estimated Duration:** 5 interactive sessions (1 M + 4 L tasks)

## Related

- `.codeflow/docs/analysis/tui-terminal-experience.md` -- TUI architecture analysis (produced by INF-TSK-044-001)
- MEMORY.md: Worktree session latch bug, PID detection audit (separate fix -- not in this epic)
- INF-EPC-024: Data Layer Standardization (Session ID consolidation)
- INF-EPC-027: Graphical App layer (future -- TUI is permanent terminal experience)
- INF-EPC-015 / INF-TSK-015-009: Original Go wizard spec (reference for onboarding wizard)
