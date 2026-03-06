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
updated_at: "2026-03-06T17:45:44Z"
---

# INF-EPC-024: Data Layer Standardization

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation script before committing:
> `bash .codeflow/scripts/validation/validate-epic.sh <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic standardizes the CodeFlow data layer: JSONL ledger schemas, schema enforcement in the Rust CLI writer, retention and rotation policies, DB-to-JSONL rebuild mapping, and three-tier consistency tooling. It addresses accumulated schema drift across four ledger files and multiple log files from shell-era vs Go-era writer differences and ad-hoc field additions by multiple agent eras.

Epic B runs in parallel with Epic A (INF-EPC-023: Parallel Execution Core) after Epic 0 (INF-EPC-022: Rust CLI Idiomatic Redesign) completes. All implementation tasks target the post-Epic 0 Rust codebase (`codeflow-cli/codeflow-core/src/`), using Rust idioms: thiserror error enums, newtype wrappers (EventType, SessionId, WorktreeId), serde derive macros, proptest/insta for testing, and cargo-llvm-cov for coverage. SurrealDB replaces SQLite as Tier 1 (Decision D17).

## Scope

### In Scope

- JSONL ledger file schema audit and standardization (sessions.jsonl, work-graph.jsonl, memory-events.jsonl, config.jsonl)
- Log file schema audit (pathflow-events.jsonl, pr-events, security, network, conversation logs)
- Schema enforcement: Rust CLI `schema.rs` required-field validation with typed EventType enum, `routing.rs` event-type routing with exhaustive match
- Field naming standardization (dual-ID coexistence fix: `id` always ULID via TaskId/EpicId newtypes, `format_id` always human-readable)
- Ad-hoc field migration: move flat ad-hoc fields into `details` object, add `worktree` field
- Retention policies: JSONL rotation thresholds, Rust CLI cleanup subcommand (clap + flate2)
- DB-to-JSONL rebuild mapping: per-table, per-event-type field mapping documentation and Rust tooling
- Three-tier consistency validation tooling (detect Tier 0/1/2 drift) in Rust CLI
- SurrealDB target schema design (surrealkv:// embedded mode, for use after Epic 0)

### Out of Scope

- Rust CLI foundation setup (Epic 0: INF-EPC-022 provides the crate structure, SurrealDB embedded, clap CLI)
- SurrealDB migration execution (Epic 0 provides surrealkv:// infrastructure)
- Loro CRDT integration (Epic A: INF-EPC-023)
- Worktree lifecycle management (Epic A: INF-EPC-023)
- Global intelligence layer (Epic C)

## Acceptance Criteria

- [ ] All 4 ledger files have documented canonical schemas with required/optional field tables
- [ ] Rust CLI `schema.rs` enforces required fields for all event types via typed EventType enum and exhaustive match
- [ ] `routing.rs` event-type routing uses EventType enum with compile-time exhaustive match
- [ ] Dual-ID coexistence bug resolved: `id` always ULID via TaskId/EpicId newtypes, `format_id` always human-readable across all writers
- [ ] Ad-hoc fields migrated to `details` object in new entries (backward-compatible readers)
- [ ] `session_id` present on all new JSONL entries
- [ ] `worktree` field added to all new JSONL entries (nullable for non-worktree sessions)
- [ ] Retention policies defined and implemented for all ledger and log files
- [ ] `codeflow cleanup --retention` Rust CLI subcommand operational (clap + flate2)
- [ ] DB-to-JSONL rebuild mapping documented per table
- [ ] Three-tier consistency validator (`codeflow doctor --data-layer`) reports drift
- [ ] SurrealDB target schemas documented for all event types
- [ ] All existing tests pass; new Rust tests added for schema enforcement (proptest, insta, cargo-llvm-cov >= 85%)

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (Y — session metadata includes user_host, user_id, claude_id)
- [ ] Direct PII check — no hardcoded PII in source/tests/comments
- [ ] Code logic review — PII fields in JSONL are metadata only; no encryption required at rest for local ledger files

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-024-001 | Ledger audit: sessions.jsonl schema documentation and gap analysis | todo | normal |
| INF-TSK-024-002 | Ledger audit: work-graph.jsonl schema documentation and gap analysis | todo | normal |
| INF-TSK-024-003 | Ledger audit: memory-events.jsonl schema documentation and gap analysis | todo | normal |
| INF-TSK-024-004 | Ledger audit: config.jsonl schema documentation and gap analysis | todo | normal |
| INF-TSK-024-005 | Log audit: pathflow-events.jsonl and pr-events schema documentation | todo | normal |
| INF-TSK-024-006 | Log audit: security, network, conversation log schema documentation | todo | normal |
| INF-TSK-024-007 | Define canonical event schema: required fields, field naming conventions | todo | high |
| INF-TSK-024-008 | Fix dual-ID coexistence in work-graph.jsonl writer | todo | high |
| INF-TSK-024-009 | Standardize memory-events.jsonl: consolidate 3 schema patterns to 1 | todo | high |
| INF-TSK-024-010 | Add session_id to all JSONL writers (schema enforcement) | todo | normal |
| INF-TSK-024-011 | Add worktree field to all JSONL writers | todo | normal |
| INF-TSK-024-012 | Migrate ad-hoc flat fields to details object in sessions.jsonl writer | todo | normal |
| INF-TSK-024-013 | Migrate ad-hoc flat fields to details object in work-graph.jsonl writer | todo | normal |
| INF-TSK-024-014 | Update event-type routing module to match canonical schema | todo | normal |
| INF-TSK-024-015 | Update schema validation for all event types with Rust type-level enforcement | todo | normal |
| INF-TSK-024-016 | Define retention policies for all ledger files (size/age thresholds) | todo | normal |
| INF-TSK-024-017 | Define retention policies for all log files (rotation, daily files) | todo | normal |
| INF-TSK-024-018 | Implement codeflow ledger cleanup subcommand in Rust CLI | todo | normal |
| INF-TSK-024-019 | Document DB-to-JSONL rebuild mapping per table | todo | normal |
| INF-TSK-024-020 | Implement JSONL-to-DB rebuild validation tooling in Rust CLI | todo | normal |
| INF-TSK-024-021 | Implement three-tier consistency validator (codeflow doctor --data-layer) in Rust CLI | todo | normal |
| INF-TSK-024-022 | Design SurrealDB target schemas for all event types | todo | normal |
| INF-TSK-024-023 | Write schema standardization tests (Rust ledger module tests) | todo | normal |
| INF-TSK-024-024 | Update cf-knowledge-layer agent definition with canonical schema reference | todo | normal |

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) — provides Rust crate structure, SurrealDB embedded mode, and clap CLI framework. All 24 tasks target the post-Epic 0 Rust codebase; INF-TSK-024-022 additionally depends on SurrealDB infrastructure

### Blocks

- INF-EPC-022 tasks that depend on finalized SurrealDB schemas (INF-TSK-024-022 output)
- INF-EPC-023 tasks that depend on finalized worktree field schema (INF-TSK-024-011 output)

## Technical Notes

**Architectural constraints:**
- D17 (SurrealDB-only): Schema design must target SurrealDB embedded (`surrealkv://`), not extend SQLite. INF-TSK-024-022 produces the SurrealDB target schemas.
- D18 (Global DB architecture): Three operating modes must be reflected in schema design.
- Backward compatibility: Old JSONL entries are immutable (append-only invariant). New Rust reader code must handle both flat and `details`-nested formats via serde untagged enum. Migration is writer-side only.
- `session_id` fix: aligns with Session ID Consolidation fix (MEMORY.md). Enforced via `SessionId(Ulid)` newtype in Rust.
- Rust idioms: All enforcement tasks use thiserror error enums, newtype wrappers, serde derive macros. Testing uses proptest (100+ property-based cases), insta (snapshot tests), cargo-llvm-cov (>= 85% coverage), cargo clippy (-- -D warnings), cargo fmt (--check).
- Crate structure: Implementation lives in `codeflow-cli/codeflow-core/src/` (core library) with CLI commands in `codeflow-cli/src/commands/` (clap subcommands).

**Source analysis:** `.codeflow/docs/analysis/parallel-work/schema-standardization.md`

## Related

- INF-EPC-022: Rust CLI Idiomatic Redesign (Epic 0 — prerequisite for SurrealDB tasks)
- INF-EPC-023: Parallel Execution Core (Epic A — parallel, depends on worktree field from INF-TSK-024-011)
- `.codeflow/docs/analysis/parallel-work/schema-standardization.md` — source analysis
- `.codeflow/docs/analysis/parallel-work/decisions.md` — D13, D17, D18, D22
