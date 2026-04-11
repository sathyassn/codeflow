---
id: "epic-01kk3a7mw0p9xj5f2vn8qr6d4b"
format_id: "INF-EPC-025"
title: "Global Intelligence Layer (Epic C)"
summary: "Daemon-based SurrealDB server, cross-project data aggregation, Unix domain socket IPC, project registry, sync engine with ledger replay, and global SurrealDB schema for multi-project intelligence"
status: planning
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-06T19:00:00Z"
updated_at: "2026-03-06T19:00:00Z"
---

# INF-EPC-025: Global Intelligence Layer (Epic C)

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation script before committing:
> `bash .codeflow/scripts/validation/validate-epic.sh <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic implements the Global Intelligence Layer (Epic C) for CodeFlow. It adds a second tier of data persistence above the project-local SurrealDB embedded database, enabling cross-project querying, semantic search across repositories, and team-wide context aggregation.

The core architecture is a daemon process running SurrealDB in server mode over a Unix domain socket (`~/.codeflow/db.sock`). The `codeflow` binary IS the SurrealDB server in daemon mode -- no separate SurrealDB installation, no Docker, no external dependency. Projects register with the daemon and sync their JSONL ledger events via a background sync engine (ledger replay at 30-second intervals). All global records are project-scoped via `project_id` fields and `can_access` graph relations enforce cross-project visibility.

Epic C is organized into 5 phases with 26 tasks total:

- **Phase C1 (Infrastructure):** Core domain types, daemon process manager, IPC server/client, global SurrealDB instance, configuration, CLI subcommands
- **Phase C2 (Project Management):** Project registration/discovery, cross-project visibility, sync engine, project-scoped schema, rebuild command
- **Phase C3 (Intelligence):** KG ontology schema, extraction queue/INGEST, local ONNX embeddings, COGNIFY extraction, entity deduplication, cross-project entity linking
- **Phase C4 (Knowledge Graph):** Hybrid search (GRAPH_COMPLETION), MEMIFY refinement, Loro CRDT KG sync, KG configuration, context search and bootstrap CLI commands
- **Phase C5 (Integration):** Platform service installer (launchd/systemd), daemon logging/observability + doctor extension, integration test suite

## Scope

### In Scope

- Daemon process manager with PID file, health check, start/stop/restart lifecycle (Phase C1)
- Unix domain socket IPC server with length-prefixed JSON protocol (Phase C1)
- IPC client library with typed convenience methods (Phase C1)
- Global SurrealDB instance at `~/.codeflow/data/` with SCHEMAFULL tables (Phase C1)
- Global configuration at `~/.codeflow/config.toml` and per-project `.codeflow/config/project.toml` (Phase C1)
- Daemon CLI subcommands: start, stop, restart, status, install-service, uninstall-service, rebuild (Phase C1)
- Project registration and auto-discovery with `proj_{SHA-256}` identity (Phase C2)
- Cross-project visibility via SurrealDB `can_access` graph relations (Phase C2)
- Sync engine with per-project ledger replay, sync cursors, and rate limiting (Phase C2)
- Project-scoped SurrealDB schema for task, session, epic, memory tables (Phase C2)
- Rebuild command: drop data tables, re-init schema, replay all JSONL, regenerate embeddings (Phase C2)
- Local ONNX embeddings via `ort` crate with all-MiniLM-L6-v2 model (384 dimensions) (Phase C3)
- HNSW vector indexes on entity/task/session/memory tables (Phase C3)
- Context search command: `codeflow context search` with vector + graph hybrid queries (Phase C3)
- KG extraction pipeline: INGEST (DEFINE EVENT triggers), COGNIFY (LLM entity/relationship extraction) (Phase C3)
- MEMIFY refinement: edge reweighting, stale pruning, transitive inference (Phase C3)
- Loro CRDT knowledge graph entity/relationship sync for teams < 15 developers (Phase C4)
- Ontology configuration via `codeflow-knowledge.toml` (Phase C4)
- Bootstrap CLI: `codeflow knowledge bootstrap` with full/incremental/since modes (Phase C4)
- Trigger points: SessionEnd hook cognify, PF6-COMPLETE, daemon idle processing (Phase C4)
- Extended `codeflow doctor` with global diagnostics (Phase C5)
- Service installer: launchd plist (macOS) + systemd unit (Linux) (Phase C5)

### Out of Scope

- Epic D: Model Orchestrator (config-driven model-to-stage routing, T1/T3 tmux execution)
- Epic E: Dashboard (SvelteKit + Tauri)
- Loro CRDT coordination for parallel execution (Epic A: INF-EPC-023)
- Schema standardization and enforcement (Epic B: INF-EPC-024)
- TiKV cluster deployment for large teams (15+ developers) -- deferred to future epic
- SurrealDB Cloud integration -- deferred to future epic
- Dual-write sync (writing to both local and daemon simultaneously) -- future enhancement after ledger replay proves stable

## Acceptance Criteria

- [ ] Daemon starts/stops/restarts reliably with PID file management and stale PID detection
- [ ] Unix domain socket IPC server accepts concurrent connections with 0o600 permissions
- [ ] IPC client connects, pings, and sends typed requests with configurable timeout/retry
- [ ] Global SurrealDB instance creates all 9 SCHEMAFULL tables and 6 graph relation tables
- [ ] Global and per-project configuration parsed and validated with serde defaults
- [ ] All 7 daemon CLI subcommands and 4 project CLI subcommands operational with --json flag
- [ ] Projects register with deterministic `proj_{SHA-256}` identity and auto-discovery
- [ ] Cross-project visibility enforced via `can_access` graph relations in all queries
- [ ] Sync engine replays JSONL ledgers with per-project cursors and rate limiting (max 1 sync/project/5s)
- [ ] Project-scoped schema defines complete field-level tables with indexes
- [ ] Rebuild command drops data, re-inits schema, replays all JSONL, regenerates embeddings
- [ ] All existing tests pass; new Rust tests added (proptest, insta, cargo-llvm-cov >= 85%)

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (Y -- project registry stores repo paths which may contain usernames; developer entity type stores contributor names)
- [ ] Direct PII check -- no hardcoded PII in source/tests/comments
- [ ] Code logic review -- PII fields are local-only metadata; no encryption required at rest for local daemon files

## Tasks

| ID | Phase | Title | Work Type | Estimate | Dependencies |
|----|-------|-------|-----------|----------|--------------|
| INF-TSK-025-001 | C1 | Core Domain Types and Error Enums | FEAT | M | Epic 0 complete |
| INF-TSK-025-002 | C1 | Daemon Process Manager | FEAT | M | 001 |
| INF-TSK-025-003 | C1 | Unix Domain Socket IPC Server + Protocol | FEAT | L | 001 |
| INF-TSK-025-004 | C1 | IPC Client Library | FEAT | S | 003 |
| INF-TSK-025-005 | C1 | Global SurrealDB Instance + Schema | FEAT | L | 001 |
| INF-TSK-025-006 | C1 | Global Configuration and Project Registry | FEAT | S | 001 |
| INF-TSK-025-007 | C1 | Daemon CLI Subcommands | FEAT | M | 002, 004, 006 |
| INF-TSK-025-008 | C2 | Project Registration and Discovery | FEAT | M | 005, 006 |
| INF-TSK-025-009 | C2 | Cross-Project Visibility and Graph Relations | FEAT | M | 008 |
| INF-TSK-025-010 | C2 | Sync Engine -- Ledger Replay | FEAT | L | 005, 008 |
| INF-TSK-025-011 | C2 | Global SurrealDB Schema -- Project-Scoped Tables | FEAT | M | 005, 001 |
| INF-TSK-025-012 | C2 | Rebuild Command | FEAT | M | 010, 011 |
| INF-TSK-025-013 | C3 | KG Ontology Schema & Entity Management | FEAT | L | 005, 001 |
| INF-TSK-025-014 | C3 | Extraction Queue & INGEST Pipeline | FEAT | M | 005, 001 |
| INF-TSK-025-015 | C3 | Local ONNX Embedding Engine | FEAT | L | 005, 006 |
| INF-TSK-025-016 | C3 | LLM Entity & Relationship Extraction (COGNIFY) | FEAT | L | 013, 015, 014 |
| INF-TSK-025-017 | C3 | Entity Deduplication | FEAT | M | 015, 013 |
| INF-TSK-025-018 | C3 | Cross-Project Entity Linking | FEAT | M | 017, 009 |
| INF-TSK-025-019 | C4 | Hybrid Search -- GRAPH_COMPLETION | FEAT | L | 015, 013, 009 |
| INF-TSK-025-020 | C4 | MEMIFY Refinement Pipeline | FEAT | L | 013, 015 |
| INF-TSK-025-021 | C4 | Loro CRDT KG Sync Integration | FEAT | M | 013, Epic A (soft), 015 |
| INF-TSK-025-022 | C4 | KG Configuration System | FEAT | S | 001 |
| INF-TSK-025-023 | C5 | Context Search & KG Bootstrap CLI Commands | FEAT | M | 019, 016, 020 |
| INF-TSK-025-024 | C5 | Platform Service Integration | FEAT | M | 002 |
| INF-TSK-025-025 | C5 | Daemon Logging & Observability + Doctor Extension | FEAT | M | 003, 008 |
| INF-TSK-025-026 | C5 | Integration Test Suite | FEAT | XL | All C1-C4 tasks |

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) -- provides Rust crate structure (`codeflow-core`, `codeflow-cli`), SurrealDB embedded mode (`surrealkv://`), clap CLI framework, trait-based abstractions (`DataStore`, `Coordinator`), and structural graph foundation (DEFINE TABLE TYPE RELATION for belongs_to, depends_on, produced_in)

### Blocks

- INF-EPC-026 (Epic D: Model Orchestrator) -- depends on Epic 0 + Epic C global infrastructure for cross-project model routing
- INF-EPC-027 (Epic E: CodeFlow App) -- depends on stable Epic 0 + B + C data layer

## Technical Notes

**Architectural constraints:**

- D17 (SurrealDB-only): All schema design targets SurrealDB. `SurrealStore` is the sole `DataStore` implementation.
- D18 (Global DB architecture): Three operating modes -- project-only (default), global-enabled, global-only (future). Daemon is INSIDE the `codeflow` Rust binary.
- D19 (Project identity): `project_id = proj_{SHA-256(canonical_root_path)[0..8]}`. Deterministic, stable, not path-dependent.
- D20 (Cross-project visibility): Per-project opt-in via `.codeflow/config/project.toml`. `can_access` graph relation in SurrealDB.
- D22 (Revised epic structure): Epic C runs parallel with Epics A and B after Epic 0. Epic D depends on Epic 0 + C.
- D23 (KG synchronization): Loro CRDT for teams < 15. TiKV for 15+. Embeddings local-only (ONNX).
- Rust idioms: All tasks use thiserror error enums, newtype wrappers, serde derive macros. Testing uses proptest, insta, cargo-llvm-cov (>= 85%).
- Crate structure: Core library at `codeflow-rs/codeflow-core/src/daemon/`, CLI commands at `codeflow-rs/codeflow-cli/src/commands/`.

**Source analysis:**

- `.codeflow/docs/analysis/parallel-work/global-intelligence.md` -- daemon architecture, sync engine, configuration
- `.codeflow/docs/analysis/parallel-work/knowledge-graph-engine.md` -- KG pipeline, ontology, extraction
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- decisions #17-#23

## Related

- INF-EPC-022: Rust CLI Idiomatic Redesign (Epic 0 -- prerequisite)
- INF-EPC-023: Parallel Execution Core (Epic A -- parallel, Loro CRDT foundation shared with C4 tasks)
- INF-EPC-024: Data Layer Standardization (Epic B -- parallel, schema work informs C2 tasks)
- `.codeflow/docs/analysis/parallel-work/global-intelligence.md`
- `.codeflow/docs/analysis/parallel-work/knowledge-graph-engine.md`
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- D17, D18, D19, D20, D22, D23
