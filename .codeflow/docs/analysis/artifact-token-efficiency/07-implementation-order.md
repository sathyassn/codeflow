# Implementation Order

## Recommended Timing

**Post Phase 6 (Commands Implementation), before Phase 9 (E2E Testing).**

```text
Current V4 Implementation Phases:
    Phase 1: Foundation (Hooks, Scripts)     -- DONE
    Phase 2: Data Layer (JSONL, SurrealDB)   -- DONE
    Phase 3: Agent Definitions               -- DONE
    Phase 4: PathFlow Integration            -- DONE
    Phase 5: Commands                        -- IN PROGRESS
    Phase 6: Autorun                         -- PLANNED
    |
    +-- RESTRUCTURING WINDOW (here) ---+
    |                                   |
    Phase 7: Go CLI                     |
    Phase 8: Welcome Screen             |
    Phase 9: E2E Testing             <--+
    Phase 10: Polish
```

**Why this timing:**

1. All artifacts exist and are functional -- restructuring modifies existing files, not creating from scratch
2. Phase 5 (Commands) completes all 14 command files -- restructuring them before they're all done wastes effort
3. Before Phase 9 (E2E Testing) -- so E2E tests validate the restructured artifacts
4. Go CLI (Phase 7) benefits from leaner artifacts -- CLI needs to understand artifact structure for validation

**Prerequisites:**

- All 14 commands implemented and passing review
- All 8 agent definitions stable
- CLAUDE.md at current functional state
- Test suite covering current artifact validation

## 5-Phase Bottom-Up Execution Plan

The execution order is bottom-up: create skills first, then reference them from agent definitions, commands, and CLAUDE.md. This avoids broken references at any point during the restructuring.

```text
Phase A: Foundation Skills (no dependencies)
    |
    v
Phase B: Agent Definition Slimming (depends on Phase A skills)
    |
    v
Phase C: Command Slimming (depends on Phase A skills)
    |
    v
Phase D: CLAUDE.md Restructuring (depends on Phase A cf-pathflow-protocol)
    |
    v
Phase E: Validation (depends on all above)
```

### Phase A: Foundation Skills (INF-TSK-019-001 through 007)

Create or restore all 9 new/restored skills. No dependencies between skills -- all can execute in parallel within batch sizing constraints.

| Task | Skill | Source | Estimate | Priority |
|------|-------|--------|:--------:|:--------:|
| INF-TSK-019-001 | cf-pathflow-protocol | NEW: extract from CLAUDE.md S4/S5/S6/S7/S11 + agent defs | L | High |
| INF-TSK-019-002 | cf-git-workflow | RESTORE: archive + cf-git-operations agent def | M | High |
| INF-TSK-019-003 | cf-memory-management | RESTORE: archive + cf-knowledge-layer agent def | M | High |
| INF-TSK-019-015 | cf-task-management | RESTORE: archive + cf-knowledge-layer agent def | M | High |
| INF-TSK-019-016 | cf-db-operations | RESTORE: archive + cf-knowledge-layer agent def | M | High |
| INF-TSK-019-004 | cf-security-operations | RESTORE: archive + cf-security agent def | M | Normal |
| INF-TSK-019-005 | cf-team-communication | NEW: extract from all 8 agent defs | M | Normal |
| INF-TSK-019-006 | cf-code-exploration | RESTORE: archive (minimal changes) | S | Normal |
| INF-TSK-019-007 | cf-documentation-standards | RESTORE + MERGE cf-markdown-standards: archive + current skill + agent def | M | Normal |

**Parallelization:** All 9 skill tasks are independent. Execute in batches per WS-DEV max_parallel (3 concurrent).

### Phase B: Agent Definition Slimming (INF-TSK-019-009, 010)

Slim all 8 agent definitions to reference Phase A skills. Two batches:

| Task | Agents | Dependencies | Estimate |
|------|--------|-------------|:--------:|
| INF-TSK-019-009 | Batch 1: cf-git-operations, cf-knowledge-layer, cf-security (function agents) | Phase A skills for these agents must exist | L |
| INF-TSK-019-010 | Batch 2: cf-development, cf-planning, cf-documentation, cf-review, cf-quality-assurance (role agents) | Phase A cf-team-communication must exist | M |

**Why function agents first:** They see the largest reduction (57-68%) and their skills are agent-specific. Role agents have a simpler change (remove Working Protocol + Communication common patterns only).

### Phase C: Command Slimming (INF-TSK-019-011, 012)

Slim all 14 commands to use SPINE pattern and reference skills. Two batches:

| Task | Commands | Dependencies | Estimate |
|------|----------|-------------|:--------:|
| INF-TSK-019-011 | Batch 1: Pipeline commands (cf-develop, cf-plan, cf-document, cf-deploy, cf-test, cf-review, cf-ship) | Phase A cf-pathflow-protocol must exist | L |
| INF-TSK-019-012 | Batch 2: Lifecycle + utility commands (cf-ship, cf-resume, cf-cleanup, cf-autorun, cf-doctor, cf-help, cf-stack, cf-approval-mode) | Phase A cf-pathflow-protocol must exist | M |

### Phase D: CLAUDE.md Restructuring (INF-TSK-019-008)

Restructure CLAUDE.md to awareness-only content, with references to cf-pathflow-protocol for procedures.

| Task | Scope | Dependencies | Estimate |
|------|-------|-------------|:--------:|
| INF-TSK-019-008 | Extract procedural HOW content, restructure to ~400 lines with SPINE diagram | Phase A cf-pathflow-protocol must exist (destination for extracted content) | L |

### Phase E: Finalize (INF-TSK-019-013, 017, 014)

Capabilities inventory update, pathflow-config skill references, and full validation pass.

| Task | Scope | Dependencies | Estimate |
|------|-------|-------------|:--------:|
| INF-TSK-019-013 | Update CLAUDE.md Section 8 (Capabilities) to reflect 14 active skills | Phase A all skills created, Phase B agent definitions updated | S |
| INF-TSK-019-017 | Add `skill_ref` fields to every PF{N}-TSK-{NN} entry in pathflow-config.json | All Phase A-D tasks complete (needs final skill operation names) | S |
| INF-TSK-019-014 | Run full test suite. Verify PathFlow E2E still works. Verify all skill references resolve. Verify no broken cross-references. | Tasks 013 and 017 complete | M |

## Dependency Graph

```text
INF-TSK-019-001 (cf-pathflow-protocol)  ----+
INF-TSK-019-002 (cf-git-workflow)       -+  |
INF-TSK-019-003 (cf-memory-management)  -+  |
INF-TSK-019-015 (cf-task-management)    -+  |
INF-TSK-019-016 (cf-db-operations)      -+  |
INF-TSK-019-004 (cf-security-operations)-+  |
INF-TSK-019-005 (cf-team-communication) -+--+--> INF-TSK-019-009 (agents batch 1)
INF-TSK-019-006 (cf-code-exploration)   -+  |   INF-TSK-019-010 (agents batch 2)
INF-TSK-019-007 (cf-doc-standards)      -+  |   INF-TSK-019-011 (commands batch 1)
                                             |   INF-TSK-019-012 (commands batch 2)
                                             +--> INF-TSK-019-008 (CLAUDE.md restructure)
                                                  INF-TSK-019-013 (capabilities update)
                                                       |
                                                       v
                                                  INF-TSK-019-017 (pathflow-config skill refs)
                                                       |
                                                       v
                                                  INF-TSK-019-014 (validation)
```

## Task-to-Phase Mapping

| Phase | Tasks | Can Run In Parallel? |
|-------|-------|:-------------------:|
| A | 001, 002, 003, 015, 016, 004, 005, 006, 007 | Yes (batched by max_parallel=3) |
| B | 009, 010 | Yes (independent batches) |
| C | 011, 012 | Yes (independent batches) |
| D | 008 | Single task (CLAUDE.md restructure) |
| E | 013, 017, 014 | Sequential (013 -> 017 -> 014) |

**Phases B, C, and D can partially overlap:** Agent slimming (B) and command slimming (C) are independent of each other and can run in parallel once Phase A skills exist. CLAUDE.md restructuring (D) requires only cf-pathflow-protocol from Phase A.

## Open Questions

| # | Question | Impact | Resolution |
|---|----------|--------|------------|
| 1 | Should cf-pathflow-protocol be split into separate phase and enforcement skills? | Affects skill granularity | RESOLVED: Single skill. Phases and enforcement are tightly coupled. One skill per domain. |
| 2 | Should command slimming happen in the same epic or a separate DOC epic? | Affects epic scope | RESOLVED: Same epic (INF) -- commands are infrastructure, not documentation. |
| 3 | Should cf-team-communication include commit message formats? | Affects skill scope | RESOLVED: Yes -- commit request is a shared communication pattern used by all stage agents. |
| 4 | What is the minimum viable content for agent Working Protocol sections? | Affects per-agent reduction | RESOLVED: Remove the Working Protocol section entirely. Each agent's YAML description includes the skill references. |
| 5 | Should the restructuring validate against the full test suite between each task? | Affects task dependencies | OPEN: Recommended yes, but adds time. Final validation task (014) provides safety net. |
| 6 | Will hook enforcement scripts need updates for restructured artifact paths? | Affects scope | RESOLVED: Out of scope. Hooks check sentinels and git commands, not artifact content. |

## Assumptions

| # | Assumption | Verified? | Evidence |
|---|-----------|-----------|----------|
| 1 | CLAUDE.md is auto-loaded every session | YES | V4 spec `01-claude-md-spec.md:38`: "Loaded at session start" |
| 2 | cf-working-protocol loaded by SessionStart hook | YES | CLAUDE.md Section 1: "Loaded automatically by SessionStart hook" |
| 3 | Agent definitions loaded only when teammate spawned | YES | CLAUDE.md Section 5: "Loading at spawn: Agent definitions are NOT auto-injected" |
| 4 | V4 CLAUDE.md token target is ~2,650 | YES | V4 spec `01-claude-md-spec.md:49`: Total ~2,650 |
| 5 | Archived skills directory exists at `.codeflow/docs/archived/skills/` | YES | Glob confirmed 10 archived skill directories |
| 6 | cf-code-exploration archived skill has 265 lines | YES | Line count verified |
| 7 | cf-documentation-standards archived skill has 267 lines | YES | Line count verified |
| 8 | INF-EPC-018 is the highest existing INF epic | YES | `ls project-management/epics/INF/` shows INF-EPC-018 as highest |
| 9 | Next INF epic should be INF-EPC-019 | YES | Convention: highest existing + 1 |
| 10 | YAML description field enforces skill scope | YES | Design decision by team lead -- "Used by:" and "Not used by:" in description field |
| 11 | Commands are loaded on-demand (not always-loaded) | YES | Claude Code loads commands only when invoked |
| 12 | PLN-EPC-001 is the ongoing planning epic | YES | Glob confirmed at `project-management/epics/PLN/PLN-EPC-001/` |
| 13 | Archived cf-git-workflow has 468 lines | YES | `wc -l` verified |
| 14 | Archived cf-memory-management has 423 lines | YES | `wc -l` verified |
| 15 | Archived cf-task-management has 409 lines | YES | `wc -l` verified |
| 16 | Archived cf-db-operations has 468 lines | YES | `wc -l` verified |
| 17 | Archived cf-security-management has 344 lines | YES | `wc -l` verified |
| 18 | Existing on-demand standards skills remain unchanged (except cf-markdown-standards, merged into cf-documentation-standards) | YES | Design decision: cf-shell-standards, cf-python-standards, cf-go-standards, cf-sandbox-standards stay as-is. cf-markdown-standards merged into cf-documentation-standards (see 02-skill-taxonomy.md). |
