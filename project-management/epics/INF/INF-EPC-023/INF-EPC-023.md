---
id: "epic-01kk0s6b6k9qmhkq79hpz22rka"
format_id: "INF-EPC-023"
title: "Parallel Execution Core (Epic A)"
summary: "Implement parallel PathFlow sessions: Loro CRDT coordination, git worktree isolation, singleton scoping, autorun integration, and sync daemon"
status: planning
area_type: "INF"
work_type: "PLAN"
domain: "GENL"
is_ongoing: false
file_scope: ["codeflow-rs/codeflow-cli/src/coordination/", "codeflow-rs/codeflow-cli/src/worktree/", "codeflow-rs/codeflow-cli/src/hooks/", "codeflow-rs/codeflow-cli/src/session/", "codeflow-rs/codeflow-cli/src/autorun/", "codeflow-rs/codeflow-cli/src/commands/", "codeflow-rs/codeflow-cli/src/store/", "codeflow-rs/codeflow-cli/src/transport/", "codeflow-rs/codeflow-cli/src/traits/", ".state/"]
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
| INF-TSK-023-042 | Fix worktree base directory default | todo | S | high | A: Loro CRDT Foundation |
| INF-TSK-023-043 | Replace state.json with state.loro using Loro Map CRDT | todo | L | critical | A: Loro CRDT Foundation |
| INF-TSK-023-044 | Wire claims into PreToolUse hooks | todo | L | critical | A: Loro CRDT Foundation |
| INF-TSK-023-045 | Fencing token validation via Loro Map | todo | M | high | A: Loro CRDT Foundation |
| INF-TSK-023-046 | Implement sync daemon logic (codeflow sync daemon) | todo | L | high | A: Loro CRDT Foundation |
| INF-TSK-023-047 | Clean up unused state.loro artifact | todo | S | normal | A: Loro CRDT Foundation |
| INF-TSK-023-048 | Add SetupDetached method to worktree manager | todo | M | critical | B: Worktree + Singleton |
| INF-TSK-023-049 | Integrate worktree creation into SessionStart hook | todo | L | critical | B: Worktree + Singleton |
| INF-TSK-023-050 | Scope codeflow-env.sh per worktree | todo | M | critical | B: Worktree + Singleton |
| INF-TSK-023-051 | Scope active-task.json per worktree | todo | M | high | B: Worktree + Singleton |
| INF-TSK-023-052 | Scope project temp dir per worktree | todo | S | high | B: Worktree + Singleton |
| INF-TSK-023-053 | Scope team config by session | todo | S | normal | B: Worktree + Singleton |
| INF-TSK-023-054 | Add worktree cleanup to SessionEnd hook | todo | M | high | B: Worktree + Singleton |
| INF-TSK-023-055 | Pre-created worktree detection in SessionStart | todo | S | normal | B: Worktree + Singleton |
| INF-TSK-023-056 | Autorun worker worktree integration | todo | L | high | C: Autorun + Coordination |
| INF-TSK-023-057 | Merge conflict detection before PR creation | todo | M | normal | C: Autorun + Coordination |
| INF-TSK-023-058 | Parallel session coordination (worktree registry, FIFO merge queue) | todo | L | high | C: Autorun + Coordination |
| INF-TSK-023-059 | SurrealDB parallel access tuning | todo | S | normal | C: Autorun + Coordination |
| INF-TSK-023-060 | Add worktree field to pathflow-events.jsonl events | todo | S | normal | B: Worktree + Singleton |
| INF-TSK-023-061 | Cross-Epic Review Handoff: Complete Fix List | complete | M | normal | Cross-Epic |

### Dependency DAG

```text
Phase A: Loro CRDT Foundation
  Epic 0 (INF-EPC-022)
    └── 042 (worktree base dir fix)
    └── 043 (state.loro migration)
          ├── 044 (wire claims) ── 045 (fencing tokens)
          ├── 046 (sync daemon)
          └── 047 (cleanup artifact)

Phase B: Worktree Integration + Singleton Scoping
  042 ── 048 (SetupDetached)
           └── 049 (SessionStart integration)
                 ├── 050 (scope codeflow-env.sh)
                 ├── 051 (scope active-task.json)
                 ├── 052 (scope temp dir)
                 ├── 054 (SessionEnd cleanup)
                 ├── 055 (pre-created detection)
                 └── 060 (worktree field in events)
  Epic 0 ── 053 (scope team config)

Phase C: Autorun + Coordination
  048 + 055 + 044 ── 056 (autorun worker)
  Epic 0 ── 057 (merge conflict detection)
  043 + 048 ── 058 (parallel coordination)
  Epic 0 ── 059 (SurrealDB tuning)
```

### Effort Summary

| Phase | Tasks | Total Effort |
|-------|-------|-------------|
| A: Loro CRDT Foundation | 6 | 2S + 1M + 3L |
| B: Worktree + Singleton | 9 | 4S + 4M + 1L |
| C: Autorun + Coordination | 4 | 1S + 1M + 2L |
| **Total** | **19** | **7S + 6M + 6L** |

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) — must be complete before Epic A implementation begins

### Blocks

- None

## Technical Notes

Key architectural decisions from `.codeflow/docs/analysis/parallel-work/decisions.md`:

| # | Decision | Conclusion | Epic A Task |
|---|----------|-----------|-------------|
| 1 | Worktree lifecycle | Ephemeral -- create at SessionStart, destroy at SessionEnd | INF-TSK-023-049, 054 |
| 2 | Worktree creation timing | SessionStart hook, NOT PF1-INIT | INF-TSK-023-049 |
| 3 | Shared vs per-worktree DB | Shared SurrealDB embedded | INF-TSK-023-059 |
| 4 | Team name convention | `{project}-{task-format-id}-{SID-short}` planned / `{project}-{SID-short}` adhoc | INF-TSK-023-053 |
| 5 | Claim conflict UX | Two-phase: advisory then enforcing | INF-TSK-023-044 |
| 6 | Merge queue ordering | FIFO -- first-completed-first-merged | INF-TSK-023-058 |
| 7 | Interactive visibility | Minimal: worker count + claim conflicts only | INF-TSK-023-058 criterion #5 |
| 8 | Loro CRDT role | Sole coordination mechanism from day one -- `use loro::LoroDoc;` | INF-TSK-023-043 |
| 9 | Memory file coordination | Git merge only -- no CRDT needed | N/A (no task needed) |
| 10 | Maximum concurrent workers | 3 | INF-TSK-023-058 criterion #6 |
| 11 | SurrealDB approach | Embedded surrealkv:// engine | INF-TSK-023-059 |
| 12 | Documentation structure | Out of scope -- covered by Epic B |
| 13 | Epic organization | Out of scope -- structure decision (Decision #22 supersedes) |
| 14 | Rust data layer architecture | Out of scope -- covered by Epic 0 (INF-EPC-022) |
| 15 | Worktree base directory | `.git-worktrees/` -- update Rust worktree manager default | INF-TSK-023-042 |
| 16 | Rust CLI migration strategy | Out of scope -- covered by Epic 0 (INF-EPC-022) |
| 17 | SurrealDB as single DB platform | Out of scope -- covered by Epic 0 (INF-EPC-022) |
| 18 | Global database architecture | Out of scope -- covered by Epic B (INF-EPC-024) |
| 19 | Project identity and scoping | Out of scope -- covered by Epic B (INF-EPC-024) |
| 20 | Cross-project visibility model | Out of scope -- covered by Epic E |
| 21 | Multi-model orchestration strategy | Out of scope -- covered by Epic D |
| 22 | Revised epic structure | Applied -- 6-epic structure (A-E + Epic 0) |
| 23 | Knowledge graph synchronization | LoroDoc namespace reserves KG containers for Epic C | INF-TSK-023-043 criterion #9 |

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
