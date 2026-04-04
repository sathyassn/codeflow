---
id: "epic-01kk242qx0myzsevaw2m47vj0a"
format_id: "INF-EPC-024"
title: "Data Layer Standardization"
summary: "Standardize JSONL ledger schemas, enforce field contracts, define retention policies, and establish three-tier consistency tooling across the data layer"
status: planning
area_type: "INF"
work_type: "RFCT"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-06T17:45:44Z"
updated_at: "2026-03-23T00:00:00Z"
---

# INF-EPC-024: Data Layer Standardization

> **MANDATORY VALIDATION:** Files created from this template MUST be validated before committing:
> `codeflow validate epic <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic standardizes the CodeFlow data layer: JSONL ledger schemas, schema enforcement in the Rust CLI writer, retention and rotation policies, DB-to-JSONL rebuild mapping, and three-tier consistency tooling. It addresses accumulated schema drift across four ledger files and multiple log files from shell-era vs Rust-era writer differences and ad-hoc field additions by multiple agent eras.

Epic B runs after Epic 0 (INF-EPC-022: Rust CLI -- COMPLETE) and Epic A (INF-EPC-023: Parallel Execution -- COMPLETE). All implementation tasks target the Rust codebase at `codeflow-cli/core/src/`, using Rust idioms: thiserror error enums, newtype wrappers (EventType, SessionId, WorktreeId), serde derive macros, proptest/insta for testing, and cargo-llvm-cov for coverage. SurrealDB is the Tier 1 database (Decision D17).

## Scope

### In Scope

- JSONL ledger file schema audit and standardization (sessions.jsonl, work-graph.jsonl, memory-events.jsonl, config.jsonl)
- Log file schema audit (pathflow-events.jsonl, pr-events, security, network, conversation logs)
- Schema enforcement: Rust CLI required-field validation with typed EventType enum, event-type routing with exhaustive match
- Field naming standardization (dual-ID coexistence fix: `id` always ULID via TaskId/EpicId newtypes, `format_id` always human-readable)
- Ad-hoc field migration: move flat ad-hoc fields into `details` object, add `worktree` field
- Retention policies: JSONL rotation thresholds, Rust CLI cleanup subcommand (clap + flate2)
- DB-to-JSONL rebuild mapping: per-table, per-event-type field mapping documentation and Rust tooling
- Three-tier consistency validation tooling (detect Tier 0/1/2 drift) in Rust CLI
- SurrealDB target schema design (surrealkv:// embedded mode)
- Session ID consolidation: audit and fix UUID session ID usage in hook handlers
- Worktree ledger isolation: make ledger LOCAL per-worktree instead of symlinked
- DB-authoritative autorun data flow: switch orchestrator to read task metadata from DB

### Out of Scope

- Rust CLI foundation setup (Epic 0: INF-EPC-022 -- COMPLETE)
- SurrealDB migration execution (Epic 0 -- COMPLETE, provides surrealkv:// infrastructure)
- Loro CRDT integration (Epic A: INF-EPC-023 -- COMPLETE)
- Worktree lifecycle management (Epic A: INF-EPC-023 -- COMPLETE)
- Global intelligence layer (Epic C)

## Acceptance Criteria

- [ ] All 4 ledger files have documented canonical schemas with required/optional field tables
- [ ] Rust CLI enforces required fields for all event types via typed EventType enum
- [ ] Event-type routing uses EventType enum with compile-time exhaustive match
- [ ] Dual-ID coexistence bug resolved: `id` always ULID, `format_id` always human-readable
- [ ] Ad-hoc fields migrated to `details` object in new entries (backward-compatible readers)
- [ ] `session_id` present on all new JSONL entries
- [ ] `worktree` field added to all new JSONL entries (nullable for non-worktree sessions)
- [ ] Retention policies defined and implemented for all ledger and log files
- [ ] `codeflow cleanup --retention` Rust CLI subcommand operational
- [ ] DB-to-JSONL rebuild mapping documented per table
- [ ] Three-tier consistency validator (`codeflow doctor --data-layer`) reports drift
- [ ] SurrealDB target schemas documented for all event types
- [ ] Session ID consolidation complete (single CODEFLOW_SESSION_ID, no UUID leakage)
- [ ] Worktree ledger isolation fixed (ledger LOCAL per-worktree, not symlinked)
- [ ] Autorun orchestrator reads task metadata from DB (not markdown frontmatter)
- [ ] All existing tests pass; new Rust tests added for schema enforcement (proptest, insta, cargo-llvm-cov >= 85%)

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (Y -- session metadata includes user_host, user_id, claude_id)
- [ ] Direct PII check -- no hardcoded PII in source/tests/comments
- [ ] Code logic review -- PII fields in JSONL are metadata only; no encryption required at rest for local ledger files

## Tasks

| # | ID | Title | Status | Est | Priority |
|---|-----|-------|--------|-----|----------|
| 1 | INF-TSK-024-001 | Ledger audit: sessions.jsonl schema documentation and gap analysis | complete | S | normal |
| 2 | INF-TSK-024-002 | Ledger audit: work-graph.jsonl schema documentation and gap analysis | complete | M | normal |
| 3 | INF-TSK-024-003 | Ledger audit: memory-events.jsonl schema documentation and gap analysis | todo | M | normal |
| 4 | INF-TSK-024-004 | Ledger audit: config.jsonl schema documentation and gap analysis | complete | XS | normal |
| 5 | INF-TSK-024-005 | Log audit: pathflow-events.jsonl and pr-events schema documentation | complete | M | normal |
| 6 | INF-TSK-024-006 | Log audit: security, network, conversation log schema documentation | todo | M | normal |
| 7 | INF-TSK-024-007 | Define canonical event schema: required fields, field naming conventions | todo | M | high |
| 8 | INF-TSK-024-008 | Fix dual-ID coexistence in work-graph.jsonl writer | todo | M | high |
| 9 | INF-TSK-024-009 | Standardize memory-events.jsonl: consolidate 3 schema patterns to 1 | todo | L | high |
| 10 | INF-TSK-024-010 | Add session_id to all JSONL writers (schema enforcement) | todo | M | normal |
| 11 | INF-TSK-024-011 | Add worktree field to all JSONL writers | todo | M | normal |
| 12 | INF-TSK-024-012 | Migrate ad-hoc flat fields to details object in sessions.jsonl writer | todo | S | normal |
| 13 | INF-TSK-024-013 | Migrate ad-hoc flat fields to details object in work-graph.jsonl writer | cancelled | -- | -- |
| 14 | INF-TSK-024-014 | Update event-type routing module to match canonical schema | todo | S | normal |
| 15 | INF-TSK-024-015 | Update schema validation for all event types with Rust type-level enforcement | todo | M | normal |
| 16 | INF-TSK-024-016 | Define retention policies for all ledger files (size/age thresholds) | todo | S | normal |
| 17 | INF-TSK-024-017 | Define retention policies for all log files (rotation, daily files) | todo | S | normal |
| 18 | INF-TSK-024-018 | Implement codeflow ledger cleanup subcommand in Rust CLI | todo | L | normal |
| 19 | INF-TSK-024-019 | Document DB-to-JSONL rebuild mapping per table | todo | M | normal |
| 20 | INF-TSK-024-020 | Implement JSONL-to-DB rebuild validation tooling in Rust CLI | todo | L | normal |
| 21 | INF-TSK-024-021 | Implement three-tier consistency validator (codeflow doctor --data-layer) | todo | L | normal |
| 22 | INF-TSK-024-022 | Design SurrealDB target schemas for all event types | todo | L | normal |
| 23 | INF-TSK-024-023 | Write schema standardization tests (Rust ledger module tests) | todo | M | normal |
| 24 | INF-TSK-024-024 | Update cf-knowledge-layer agent definition with canonical schema reference | todo | S | normal |
| 25 | INF-TSK-024-025 | INF-EPC-024 Template Improvements and Epic Refinement (planning task) | complete | M | normal |
| 26 | INF-TSK-024-026 | Switch autorun orchestrator to read task metadata from DB | todo | M | normal |
| 27 | INF-TSK-024-027 | Resolve batch dependencies from DB tasks table | todo | S | normal |
| 28 | INF-TSK-024-028 | Session ID consolidation: audit and fix UUID session ID usage | todo | M | high |
| 29 | INF-TSK-024-029 | Update Claude artifacts for DB-authoritative autorun data flow | todo | S | normal |
| 30 | INF-TSK-024-030 | Fix worktree ledger isolation -- make ledger LOCAL per-worktree | complete | M | normal |
| 31 | INF-TSK-024-031 | INF-EPC-024 post-ledger refinement + gitignore fix | complete | M | normal |
| 32 | INF-TSK-024-032 | INF-EPC-024 gap remediation: add 4 new tasks, expand 3 existing, update phase structure | complete | M | normal |
| 33 | INF-TSK-024-033 | Ledger audit: coordination-events.jsonl schema documentation | todo | M | normal |
| 34 | INF-TSK-024-034 | Ledger audit: autorun-events.jsonl schema documentation | todo | S | normal |
| 35 | INF-TSK-024-035 | Migrate pathflow-events.jsonl from .state/logs/ to .state/ledger/ | todo | S | normal |
| 36 | INF-TSK-024-036 | Execute SurrealDB schema updates from standardized JSONL | todo | L | normal |

**Task 013 cancelled:** Superseded by Epic 0 restructuring. The work-graph.jsonl details migration is now handled differently -- the workgraph module at `codeflow-cli/core/src/workgraph/` already defines typed events with structured fields, making the flat-to-details migration unnecessary for work-graph events.

**Tasks 001-012, 014-024, 026-029 reviewed (PR #221 refinement):** Stale file path references (flat `.state/ledger/{type}.jsonl` -> subdirectory `.state/ledger/{type}/{type}.jsonl`) and worktree assumptions (ledger was SHARED/symlinked, now LOCAL per-worktree) corrected in tasks 001-004, 008-010, 012. Remaining tasks confirmed no changes needed. See INF-TSK-024-031 PLAN Report for full details.

**Gap remediation (INF-TSK-024-032):** 4 new tasks added (033-036), 3 existing tasks expanded (006, 017, 022). Task 033 (coordination-events audit) and 034 (autorun-events audit) fill Phase 1 gaps. Task 035 (pathflow-events migration) adds a Phase 3 fix. Task 036 (SurrealDB schema execution) adds Phase 6 implementation step. Task 006 expanded with 4 criteria for operational logs (git, db, cleanup, .meta files). Task 017 expanded with 3 criteria for session .meta files, stale locks, and prompt counters. Task 022 expanded with 3 criteria for CANONICAL/ALL rationale and non-canonical DB table relationships.

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI) -- **COMPLETE**. Provides Rust crate structure, SurrealDB embedded mode, clap CLI framework. All tasks target the post-Epic 0 Rust codebase at `codeflow-cli/core/src/`.
- INF-EPC-023 (Epic A: Parallel Execution) -- **COMPLETE**. Provides worktree infrastructure, CRDT coordination, claims system. Tasks 011 (worktree field), 028 (session ID), and 030 (worktree ledger isolation) build on Epic A infrastructure.

### Blocks

- INF-EPC-023 tasks that depend on finalized worktree field schema (INF-TSK-024-011 output) -- Epic A is COMPLETE but may need schema updates
- Epic C (Global Intelligence Layer) -- depends on standardized data layer

## Technical Notes

**Architectural constraints:**
- D17 (SurrealDB-only): Schema design targets SurrealDB embedded (`surrealkv://`). INF-TSK-024-022 produces the SurrealDB target schemas.
- D18 (Global DB architecture): Three operating modes reflected in schema design.
- Backward compatibility: Old JSONL entries are immutable (append-only invariant). New Rust reader code handles both flat and `details`-nested formats via serde untagged enum. Migration is writer-side only.
- `session_id` fix: aligns with Session ID Consolidation (INF-TSK-024-028). Enforced via `SessionId` newtype in Rust at `codeflow-cli/core/src/types/ids.rs`.
- Rust idioms: All enforcement tasks use thiserror error enums, newtype wrappers, serde derive macros. Testing uses proptest (100+ property-based cases), insta (snapshot tests), cargo-llvm-cov (>= 85% per-file coverage), cargo clippy (-- -D warnings), cargo fmt (--check).
- Crate structure: Implementation lives in `codeflow-cli/core/src/` (core library) with CLI commands in `codeflow-cli/cli/src/` (clap subcommands).

**Key codebase locations (verified against current codebase):**
- Ledger module: `codeflow-cli/core/src/ledger/` -- `jsonl.rs` (LedgerWriter), `routing.rs` (event routing), `mod.rs`. NO per-file modules (no sessions.rs, work_graph.rs, etc.).
- Types module: `codeflow-cli/core/src/types/` -- `ids.rs` (SessionId, etc.), `events.rs` (typed events), `enums.rs`, `work.rs`, `phase.rs`, `sentinel.rs`, `stage.rs`.
- WorkGraph module: `codeflow-cli/core/src/workgraph/` -- `task.rs`, `epic.rs`, `format_id.rs`, `transitions.rs`, `query.rs`.
- Worktree module: `codeflow-cli/core/src/worktree/` -- `setup.rs`, `cleanup.rs`, `paths.rs`, `registry.rs`, `mod.rs`.
- Session module: `codeflow-cli/core/src/session/` -- `mod.rs`, `env.rs`, `active_task.rs`, `builder.rs`, `state.rs`.
- Hooks: `codeflow-cli/core/src/hooks/` -- `session_start.rs`, `session_end.rs`, `pre_tool_use.rs`, `post_tool_use.rs`, `task_completed.rs`, `prompt_validate.rs`, `pipeline.rs`, `logging/`, `security/`.

### Autorun Batching

Tasks are organized into 7 execution phases for autorun batch planning:

**Phase 1 -- Audit (tasks 001-006, 033, 034):** Batchable. 3 workers, each task reads different JSONL/log files. Non-overlapping file_scope. Tasks 033 and 034 run in parallel with 003-006.

| Batch | Worker | Tasks | file_scope |
|-------|--------|-------|------------|
| 1a | W1 | 001, 004 | sessions-jsonl-audit.md, config-jsonl-audit.md |
| 1b | W2 | 002, 005, 034 | work-graph-jsonl-audit.md, pathflow-events-audit.md, pr-events-audit.md, autorun-events-audit.md |
| 1c | W3 | 003, 006, 033 | memory-events-jsonl-audit.md, security/network/conversation-logs-audit.md, operational-logs-audit.md, coordination-events-audit.md |

**Phase 2 -- Schema Definition (task 007):** Sequential. Depends on all Phase 1 outputs (including 033, 034).

| Batch | Worker | Tasks | Blocked By |
|-------|--------|-------|------------|
| 2 | W1 | 007 | 001-006, 033, 034 |

**Phase 3 -- Fixes (tasks 008, 009, 035):** Batchable. 3 workers, non-overlapping scope. Task 035 parallel with 008/009.

| Batch | Worker | Tasks | Blocked By |
|-------|--------|-------|------------|
| 3a | W1 | 008 | 002, 007 |
| 3b | W2 | 009 | 003, 007 |
| 3c | W3 | 035 | 005 |

**Phase 4 -- Enforcement (tasks 010-015, excluding cancelled 013):** Partially batchable.

| Batch | Worker | Tasks | Blocked By |
|-------|--------|-------|------------|
| 4a | W1 | 010, 011 | 007 (parallel, non-overlapping) |
| 4b | W2 | 012 | 001, 007, 010, 011 |
| 4c | W1 | 014, 015 | 007 (parallel after 4a) |

**Phase 5 -- Retention + Rebuild (tasks 016-021):** Partially batchable. Task 017 expanded with 3 additional criteria.

| Batch | Worker | Tasks | Blocked By |
|-------|--------|-------|------------|
| 5a | W1 | 016, 017 | None (parallel, non-overlapping) |
| 5b | W2 | 019 | 007 |
| 5c | W1 | 018 | 016, 017 |
| 5d | W2 | 020 | 019 |
| 5e | W1 | 021 | 020 |

**Phase 6 -- SurrealDB + DB Switch (tasks 022, 026, 027, 036):** Sequential chain. Task 022 expanded with 3 additional criteria. Task 036 after 022 + 020.

| Batch | Worker | Tasks | Blocked By |
|-------|--------|-------|------------|
| 6a | W1 | 022 | 007, 015 |
| 6b | W1 | 026 | 022 |
| 6c | W1 | 027 | 026 |
| 6d | W2 | 036 | 022, 020 |

**Phase 7 -- Docs + Session ID + Worktree Fix (tasks 023, 024, 028, 029, 030):** Batchable (non-overlapping scopes).

| Batch | Worker | Tasks | Blocked By |
|-------|--------|-------|------------|
| 7a | W1 | 023 | 008, 009, 010, 011, 012, 014, 015 |
| 7b | W2 | 024, 029 | 022, 026 (parallel, non-overlapping) |
| 7c | W3 | 028 | None (independent) |
| 7d | W3 | 030 | None (independent) |

Note: Tasks 028 and 030 are independent of all other tasks and can run in any phase. Placed in Phase 7 for organizational clarity but can be promoted earlier for parallel execution.

**Source analysis:** `.codeflow/docs/analysis/parallel-work/schema-standardization.md`

## Related

- INF-EPC-022: Rust CLI Idiomatic Redesign (Epic 0 -- COMPLETE)
- INF-EPC-023: Parallel Execution Core (Epic A -- COMPLETE)
- `.codeflow/docs/analysis/parallel-work/schema-standardization.md` -- source analysis
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- D13, D17, D18, D22
