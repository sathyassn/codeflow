---
id: "epic-01kk0s6b6k9qmhkq79hpz22rka"
format_id: "INF-EPC-023"
title: "Parallel Execution Core (Epic A)"
summary: "Implement parallel PathFlow sessions: Loro CRDT coordination, git worktree isolation, singleton scoping, autorun integration, and sync daemon"
status: in_progress
area_type: "INF"
work_type: "PLAN"
domain: "GENL"
is_ongoing: false
file_scope: ["codeflow-cli/core/src/coordination/", "codeflow-cli/core/src/worktree/", "codeflow-cli/core/src/hooks/", "codeflow-cli/core/src/session/", "codeflow-cli/core/src/autorun/", "codeflow-cli/cli/src/cmd/", "codeflow-cli/core/src/store/", "codeflow-cli/core/src/transport/", ".state/"]
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-06T05:16:31Z"
updated_at: "2026-03-06T05:16:31Z"
---

# INF-EPC-023: Parallel Execution Core (Epic A)

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation script before committing:
> `bash .codeflow/scripts/validation/validate-epic.sh project-management/epics/INF/INF-EPC-023/INF-EPC-023.md`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Epic A implements true parallel PathFlow sessions in CodeFlow. Building on the Rust CLI foundation from Epic 0 (INF-EPC-022), Epic A delivers: Loro CRDT as the sole coordination mechanism (claims, file ownership, knowledge graph sync), git worktree isolation for concurrent agent sessions, elimination of the three blocking singleton files, autorun worker integration, and a `codeflow sync` daemon subcommand for multi-machine delta sync.

This epic depends on Epic 0 (INF-EPC-022) being complete. Epics B (Data Layer Standardization) and C (Global Intelligence Layer) can run in parallel with Epic A after Epic 0.

## Scope

### In Scope

- Loro CRDT foundation: `LoroCoordinator` implementing `Coordinator` trait from Epic 0
- Git worktree lifecycle: create at SessionStart hook, destroy at SessionEnd hook, base at `.git-worktrees/`
- Singleton elimination: per-session scoping of `codeflow-env.sh`, `/tmp/claude/{project}/`, `active-task.json`
- Claims system migration: replace `claim.go` TOCTOU race with Loro Map containers
- Autorun worker integration: per-worker worktrees, pre-created worktree detection in SessionStart
- Sync daemon: `codeflow sync daemon` subcommand for multi-machine delta sync via git refs
- Team name convention: `{project}-{task-format-id}-{SID-short}` (planned) / `{project}-{SID-short}` (adhoc)
- Merge queue: FIFO ordering, max 3 concurrent workers

### Out of Scope

- SurrealDB integration (Epic 0 prerequisite, already delivered)
- Schema standardization (Epic B)
- Global intelligence layer / embeddings (Epic C)
- Model orchestration (Epic D)
- Dashboard (Epic E)
- Knowledge graph engine (Epic C)

## Acceptance Criteria

- [ ] Three critical singletons scoped per-session: `codeflow-env.sh`, `/tmp/claude/{project}/`, `active-task.json`
- [ ] Worktrees created at SessionStart hook in `.git-worktrees/{session-id}/`, destroyed at SessionEnd
- [ ] `LoroCoordinator` implements `Coordinator` trait; `LoroDoc` is direct crate dependency (no FFI)
- [ ] Claims system uses Loro Map containers (replaces `claim.go` TOCTOU race)
- [ ] Max 3 concurrent autorun workers; each gets own worktree
- [ ] `codeflow sync daemon` subcommand implements delta sync via git refs (30s interval)
- [ ] All existing shell tests pass (1,555+) after worktree changes
- [ ] Worktree base directory is `.git-worktrees/` (`.gitignore:67` already correct)

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Estimate | Priority | Phase |
|----|-------|--------|----------|----------|-------|
| INF-TSK-023-001 | Plan: Epic A -- Parallel Execution Core | complete | M | normal | Planning |
| INF-TSK-023-002 | Fix worktree base directory default | complete | S | high | A: Loro CRDT Foundation |
| INF-TSK-023-003 | Replace state.json with state.loro using Loro Map CRDT | complete | L | critical | A: Loro CRDT Foundation |
| INF-TSK-023-004 | Wire claims into PreToolUse hooks | complete | L | critical | A: Loro CRDT Foundation |
| INF-TSK-023-005 | Fencing token validation via Loro Map | complete | M | high | A: Loro CRDT Foundation |
| INF-TSK-023-006 | Implement sync daemon logic (codeflow sync daemon) | complete | L | high | A: Loro CRDT Foundation |
| INF-TSK-023-007 | Clean up unused state.loro artifact | complete | S | normal | A: Loro CRDT Foundation |
| INF-TSK-023-008 | Add SetupDetached method to worktree manager | complete | M | critical | B: Worktree + Singleton |
| INF-TSK-023-009 | Integrate worktree creation into SessionStart hook | complete | L | critical | B: Worktree + Singleton |
| INF-TSK-023-010 | Scope codeflow-env.sh per worktree | complete | M | critical | B: Worktree + Singleton |
| INF-TSK-023-011 | Scope active-task.json per worktree | complete | M | high | B: Worktree + Singleton |
| INF-TSK-023-012 | Scope project temp dir per worktree | complete | S | high | B: Worktree + Singleton |
| INF-TSK-023-013 | Scope team config by session | complete | S | normal | B: Worktree + Singleton |
| INF-TSK-023-014 | Add worktree cleanup to SessionEnd hook | complete | M | high | B: Worktree + Singleton |
| INF-TSK-023-015 | Pre-created worktree detection in SessionStart | complete | S | normal | B: Worktree + Singleton |
| INF-TSK-023-016 | Autorun worker worktree integration | complete | L | high | C: Autorun + Coordination |
| INF-TSK-023-017 | Merge conflict detection before PR creation | complete | M | normal | C: Autorun + Coordination |
| INF-TSK-023-018 | Parallel session coordination (worktree registry, FIFO merge queue) | complete | L | high | C: Autorun + Coordination |
| INF-TSK-023-019 | SurrealDB parallel access tuning | complete | S | normal | C: Autorun + Coordination |
| INF-TSK-023-020 | Add worktree field to pathflow-events.jsonl events | complete | S | normal | B: Worktree + Singleton |
| INF-TSK-023-021 | Cross-Epic Review Handoff: Complete Fix List | complete | M | normal | Cross-Epic |
| INF-TSK-023-023 | Document parallel execution capabilities | complete | L | normal | D: Documentation |
| INF-TSK-023-024 | Wire autorun parallel worker orchestration | complete | XL | high | E: Integration |
| INF-TSK-023-025 | Implement worktree CLI subcommands (cleanup/prune) | complete | L | normal | E: Integration |
| INF-TSK-023-026 | Implement sync daemon lifecycle management | complete | M | normal | E: Integration |
| INF-TSK-023-027 | Implement scope policy enforcement + validation | complete | L | normal | E: Integration |
| INF-TSK-023-028 | Implement Claude invoker (hybrid tmux + file-marker) | complete | L | high | F: Autorun Readiness |
| INF-TSK-023-029 | Fix worker coordination bugs | complete | L | high | F: Autorun Readiness |
| INF-TSK-023-030 | Wire merge conflict remediation | complete | M | normal | F: Autorun Readiness |
| INF-TSK-023-031 | Comprehensive batch validation | todo | L | high | F: Autorun Readiness |
| INF-TSK-023-032 | Wire autorun DB + event recording | todo | L | high | F: Autorun Readiness |
| INF-TSK-023-033 | Autorun config section | todo | S | normal | F: Autorun Readiness |
| INF-TSK-023-034 | Pre-flight checks + CLI args | todo | M | normal | F: Autorun Readiness |
| INF-TSK-023-035 | Orchestrator graceful shutdown | todo | M | normal | F: Autorun Readiness |
| INF-TSK-023-036 | Agent autorun SOPs | todo | M | normal | F: Autorun Readiness |
| INF-TSK-023-037 | Autorun session management commands | todo | L | normal | F: Autorun Readiness |
| INF-TSK-023-038 | Batch report generation | todo | S | normal | F: Autorun Readiness |

### Dependency DAG

```text
Phase A: Loro CRDT Foundation
  Epic 0 (INF-EPC-022)
    └── 002 (worktree base dir fix)
    └── 003 (state.loro migration)
          ├── 004 (wire claims) ── 005 (fencing tokens)
          ├── 006 (sync daemon)
          └── 007 (cleanup artifact)

Phase B: Worktree Integration + Singleton Scoping
  002 ── 008 (SetupDetached)
           └── 009 (SessionStart integration)
                 ├── 010 (scope codeflow-env.sh)
                 ├── 011 (scope active-task.json)
                 ├── 012 (scope temp dir)
                 ├── 014 (SessionEnd cleanup)
                 ├── 015 (pre-created detection)
                 └── 020 (worktree field in events)
  Epic 0 ── 013 (scope team config)

Phase C: Autorun + Coordination
  008 + 015 + 004 ── 016 (autorun worker)
  Epic 0 ── 017 (merge conflict detection)
  003 + 008 ── 018 (parallel coordination)
  Epic 0 ── 019 (SurrealDB tuning)

Phase F: Autorun Readiness
  033 (config section)
    └── 029 (worker coord bugs, needs blocked_behavior)
  028 (Claude invoker)
  030 (merge conflict remediation)
  031 (batch validation)
  032 (DB + events)
    ├── 037 (session mgmt cmds, needs DB)
    └── 038 (batch reports, needs DB)
  034 (pre-flight + CLI args)
    └── 037 (session mgmt cmds, needs subcommand enum)
  035 (graceful shutdown)
  036 (agent SOPs, DOCS pipeline)
```

### Effort Summary

| Phase | Tasks | Total Effort |
|-------|-------|-------------|
| A: Loro CRDT Foundation | 6 | 2S + 1M + 3L |
| B: Worktree + Singleton | 9 | 4S + 4M + 1L |
| C: Autorun + Coordination | 4 | 1S + 1M + 2L |
| D: Documentation | 1 | 1L |
| E: Integration | 4 | 1M + 1L + 1XL + 1L |
| F: Autorun Readiness | 11 | 2S + 3M + 5L + 1M(DOCS) |
| **Total** | **35** | **9S + 10M + 11L + 1XL** |

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) — must be complete before Epic A implementation begins

### Blocks

- None

## Technical Notes

Key architectural decisions from `.codeflow/docs/analysis/parallel-work/decisions.md`:

| # | Decision | Conclusion | Epic A Task |
|---|----------|-----------|-------------|
| 1 | Worktree lifecycle | Ephemeral -- create at SessionStart, destroy at SessionEnd | INF-TSK-023-009, 014 |
| 2 | Worktree creation timing | SessionStart hook, NOT PF1-INIT | INF-TSK-023-009 |
| 3 | Shared vs per-worktree DB | Shared SurrealDB embedded | INF-TSK-023-019 |
| 4 | Team name convention | `{project}-{task-format-id}-{SID-short}` planned / `{project}-{SID-short}` adhoc | INF-TSK-023-013 |
| 5 | Claim conflict UX | Two-phase: advisory then enforcing | INF-TSK-023-004 |
| 6 | Merge queue ordering | FIFO -- first-completed-first-merged | INF-TSK-023-018 |
| 7 | Interactive visibility | Minimal: worker count + claim conflicts only | INF-TSK-023-018 criterion #5 |
| 8 | Loro CRDT role | Sole coordination mechanism from day one -- `use loro::LoroDoc;` | INF-TSK-023-003 |
| 9 | Memory file coordination | Git merge only -- no CRDT needed | N/A (no task needed) |
| 10 | Maximum concurrent workers | 3 | INF-TSK-023-018 criterion #6 |
| 11 | SurrealDB approach | Embedded surrealkv:// engine | INF-TSK-023-019 |
| 12 | Documentation structure | Out of scope -- covered by Epic B |
| 13 | Epic organization | Out of scope -- structure decision (Decision #22 supersedes) |
| 14 | Rust data layer architecture | Out of scope -- covered by Epic 0 (INF-EPC-022) |
| 15 | Worktree base directory | `.git-worktrees/` -- update Rust worktree manager default | INF-TSK-023-002 |
| 16 | Rust CLI migration strategy | Out of scope -- covered by Epic 0 (INF-EPC-022) |
| 17 | SurrealDB as single DB platform | Out of scope -- covered by Epic 0 (INF-EPC-022) |
| 18 | Global database architecture | Out of scope -- covered by Epic B (INF-EPC-024) |
| 19 | Project identity and scoping | Out of scope -- covered by Epic B (INF-EPC-024) |
| 20 | Cross-project visibility model | Out of scope -- covered by Epic E |
| 21 | Multi-model orchestration strategy | Out of scope -- covered by Epic D |
| 22 | Revised epic structure | Applied -- 6-epic structure (A-E + Epic 0) |
| 23 | Knowledge graph synchronization | LoroDoc namespace reserves KG containers for Epic C | INF-TSK-023-003 criterion #13 |

Three critical singletons blocking parallelism (from worktree-architecture.md):
1. `codeflow-env.sh` — session ID singleton (written at SessionStart, destructive)
2. `/tmp/claude/{project}/` — temp path singleton (shared across sessions)
3. `active-task.json` — active task singleton (written at PF4-TSK-02 begin-work)

## Related

- `.codeflow/docs/analysis/parallel-work/` — full analysis package (7 documents)
- `.codeflow/docs/analysis/parallel-work/decisions.md` — 23 analyzed decisions
- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) — prerequisite
- INF-EPC-024 (Epic B: Data Layer Standardization) — parallel sibling
- INF-EPC-025 (Epic C: Global Intelligence Layer) — parallel sibling
