---
id: "epic-01kk09r9yqn9hb7v8pa1zsa4gc"
format_id: "INF-EPC-022"
title: "Rust CLI -- Idiomatic Redesign (Epic 0)"
summary: "Replace the Go CLI with a pure Rust implementation using SurrealDB as the sole database, modular crate structure, and trait-based abstractions"
status: planning
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
is_ongoing: false
file_scope: ["codeflow-rs/", ".claude/skills/cf-rust-standards/", ".claude/skills/cf-surrealdb-standards/", ".codeflow/testing/cli/test-rust-cli.sh", ".github/workflows/test-suite.yml"]
priority: critical
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-05T00:00:00Z"
updated_at: "2026-03-05T00:00:00Z"
---

# INF-EPC-022: Rust CLI -- Idiomatic Redesign (Epic 0)

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation script before committing:
> `bash .codeflow/scripts/validation/validate-epic.sh <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory. Script is at `.codeflow/scripts/validation/validate-epic.sh`.

## Summary

Replace the existing Go CLI (`codeflow-cli/`) with a pure Rust implementation (`codeflow-rs/`) using SurrealDB embedded (`surrealkv://`) as the sole Tier 1 database. This is an idiomatic redesign -- not a line-for-line port. The Rust CLI introduces trait-based abstractions (`DataStore`, `LedgerWriter`, `HookHandler`), domain-specific error types (`thiserror`), compile-time state validation via enums, and a modular crate structure (`codeflow-core` library + `codeflow-cli` binary). The external interface is frozen: same binary name (`codeflow`), same subcommands, same hook JSON contracts, same exit codes, same file I/O paths. Go and Rust binaries coexist during migration (Phases 0A-0F). Cutover is atomic (Phase 0G). Epic 0 is the universal prerequisite for Epics A-E.

**Key architectural decisions (from `.codeflow/docs/analysis/parallel-work/decisions.md`):**

- Decision #1-#6: Rust CLI rationale, language choice
- Decision #14: Pure Rust data layer with trait hierarchy
- Decision #16: Idiomatic redesign with incremental coexistence
- Decision #17: SurrealDB as single DB platform (NO SqliteStore)
- Decision #22: Epic 0 as prerequisite for all subsequent epics

## Scope

### In Scope

- Rust workspace initialization (`codeflow-rs/` with `codeflow-core` lib + `codeflow-cli` bin crates)
- MCP server configuration (Context7, rust-analyzer) for AI-assisted development
- New skills: `cf-rust-standards` and `cf-surrealdb-standards`
- Core domain types: enums for `SessionState`, `PathFlowPhase`, `WorkStage`, `Sentinel`, `LedgerEvent`
- Domain error types: `SessionError`, `DbError`, `HookError`, `LedgerError` via `thiserror`
- Trait definitions: `DataStore`, `LedgerWriter`, `HookHandler`
- `SurrealStore` implementation (sole `DataStore` impl, embedded `surrealkv://`)
- `JsonlWriter` implementation (flock-based append, serde serialization)
- Session management, workstate, config ported with builder patterns
- Worktree manager via `git2` crate
- Workgraph operations (task/epic CRUD, status transitions as enum state machines)
- `HookHandler` trait and all hook implementations (session, tool-use, logging)
- Clap CLI dispatch layer mirroring all Go subcommands
- Rust unit testing infrastructure (`cargo-llvm-cov`, `proptest`, `insta`, `cargo-nextest`)
- Shell integration test bridge script for Rust binary
- Contract conformance test suite (Rust binary matches Go binary output)
- CI pipeline: `test-rust` job alongside existing Go jobs
- Atomic cutover: Go removal, Rust binary installed globally
- 85% per-file coverage threshold matching Go conventions

### Out of Scope

- `LoroCoordinator` implementation (Epic A -- Parallel Execution Core)
- `Coordinator` and `Transport` trait implementations (Epic A)
- Schema standardization in SurrealQL beyond current Go schema (Epic B)
- Global daemon mode, project registry, sync engine (Epic C)
- Local embeddings and vector search (Epic C)
- Model orchestrator (Epic D)
- Dashboard (Epic E)
- Any new features -- this is 1:1 behavioral replacement only
- Claims system CRDT migration (Epic A -- Loro replaces state.json)
- Sync daemon subcommand (`codeflow sync daemon` deferred to Epic A)

## Acceptance Criteria

### Phase 0A (Analysis + Tooling) -- Tasks 001-004

- [ ] Go CLI audit document produced with DRY violations, coupling analysis, and Rust redesign opportunities
- [ ] Trait hierarchy and module map documented, matching `data-layer-protection.md` Section 6
- [ ] `cf-rust-standards` skill created with all required sections (workspace, errors, traits, serde, async, testing, naming, clippy, unsafe, crates, builders, modules)
- [ ] `cf-surrealdb-standards` skill created with all required sections (embedded, SurrealQL, graph, vector, schema, connection, testing, data model)
- [ ] MCP servers (Context7, rust-analyzer) configured and verified

### Phase 0B (Foundation + Testing) -- Tasks 005-008

- [ ] Rust workspace initializes and builds (`cargo build` succeeds)
- [ ] Core domain types compile with serde derive and Display impls
- [ ] Error enums compile with `thiserror` derives
- [ ] `cargo test` runs with 85% coverage threshold enforced
- [ ] `proptest`, `insta`, `cargo-nextest` configured and functional
- [ ] Shell integration test bridge script runs existing tests against Rust binary

### Phase 0C (Core Library) -- Tasks 009-014

- [ ] `DataStore` trait defined with CRUD operations for sessions, tasks, epics, workgraph
- [ ] `SurrealStore` implements `DataStore` using embedded `surrealkv://`
- [ ] `LedgerWriter` trait defined; `JsonlWriter` implements it with flock + serde
- [ ] Session management ported with builder pattern and enum state machine
- [ ] Worktree manager ported via `git2` crate
- [ ] Workgraph operations ported with `DataStore` trait (not `SurrealStore` directly)
- [ ] All Phase 0C modules have unit tests achieving 85% coverage

### Phase 0D (Hook Handlers) -- Tasks 015-017

- [ ] `HookHandler` trait defined with `fn handle(&self, input: HookInput) -> Result<HookOutput, HookError>`
- [ ] All session hooks (start, end) ported as `HookHandler` impls
- [ ] All pre-tool-use gates (gate-check, team-guard, edit-write-guard, etc.) ported
- [ ] All post-tool-use handlers (sentinel-write, checkpoint-register, etc.) ported
- [ ] Logging and autorun modules ported
- [ ] All hook handlers maintain exit code contract (0/1/2)

### Phase 0E (CLI Commands) -- Task 018

- [ ] All Go subcommands ported to Clap commands in `codeflow-cli` crate
- [ ] Each command is a thin dispatch to `codeflow-core` functions
- [ ] `codeflow test` orchestrates the shell test suite from the Rust binary
- [ ] `codeflow doctor` validates infrastructure from the Rust binary

### Phase 0F (Integration Testing) -- Tasks 019-021

- [ ] Go unit tests ported to Rust `#[test]` and `#[tokio::test]`
- [ ] Contract conformance suite verifies identical output between Go and Rust binaries
- [ ] All ~1,555 shell integration tests pass UNCHANGED against the Rust binary
- [ ] Property-based tests with `proptest` for serialization roundtrips
- [ ] Snapshot tests with `insta` for JSON output verification

### Phase 0G (CI/CD + Cutover) -- Tasks 022-027

- [ ] CI pipeline has `test-rust` job alongside existing Go jobs
- [ ] `cargo build --release` produces binaries for darwin-arm64, darwin-amd64, linux-amd64
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo fmt --check` passes
- [ ] Coverage gate: 85% threshold via `cargo-llvm-cov`
- [ ] Cutover: `codeflow-cli/` (Go) removed, `codeflow-rs/` renamed to `codeflow-cli/`
- [ ] All references updated (CI, test scripts, docs)
- [ ] Final validation: all 1,555+ shell tests pass against Rust binary at final location
- [ ] Go-specific CI jobs, Makefile targets, `.golangci.yml` removed
- [ ] Go CLI binary (`codeflow-cli/`) retained throughout Phases 0A-0F; removal only in Phase 0G after contract conformance verification

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)
- [ ] No hardcoded PII in source/tests/comments

## Tasks

| ID | Title | Status | Priority | Phase |
|----|-------|--------|----------|-------|
| INF-TSK-022-001 | Audit Go CLI for redesign opportunities | todo | high | 0A |
| INF-TSK-022-002 | Define trait hierarchy and module map | todo | high | 0A |
| INF-TSK-022-003 | Create cf-rust-standards skill | todo | high | 0A |
| INF-TSK-022-004 | Configure MCP servers and create cf-surrealdb-standards skill | todo | normal | 0A |
| INF-TSK-022-005 | Initialize Rust workspace | todo | critical | 0B |
| INF-TSK-022-006 | Define core types and error domains | todo | critical | 0B |
| INF-TSK-022-007 | Set up Rust unit testing infrastructure | todo | critical | 0B |
| INF-TSK-022-008 | Set up shell integration test bridge | todo | high | 0B |
| INF-TSK-022-009 | Implement DataStore trait and SurrealStore | todo | critical | 0C |
| INF-TSK-022-010 | Implement LedgerWriter trait and JsonlWriter | todo | critical | 0C |
| INF-TSK-022-011 | Implement session and state management | todo | high | 0C |
| INF-TSK-022-012 | Implement worktree manager | todo | normal | 0C |
| INF-TSK-022-013 | Implement workgraph operations | todo | high | 0C |
| INF-TSK-022-014 | Implement config, doctor, and utility modules | todo | normal | 0C |
| INF-TSK-022-015 | Implement HookHandler trait and session hooks | todo | critical | 0D |
| INF-TSK-022-016 | Implement tool-use hooks | todo | critical | 0D |
| INF-TSK-022-017 | Implement logging and autorun modules | todo | high | 0D |
| INF-TSK-022-018 | Implement CLI command dispatch | todo | high | 0E |
| INF-TSK-022-019 | Port Go unit tests to Rust | todo | high | 0F |
| INF-TSK-022-020 | Build contract conformance test suite | todo | critical | 0F |
| INF-TSK-022-021 | Validate shell test suite passes | todo | critical | 0F |
| INF-TSK-022-022 | Set up CI pipeline for Rust binary | todo | high | 0G |
| INF-TSK-022-023 | Coverage enforcement and lint pipeline | todo | high | 0G |
| INF-TSK-022-024 | Cross-compile release builds | todo | normal | 0G |
| INF-TSK-022-025 | Cutover: Go removal and Rust installation | todo | critical | 0G |
| INF-TSK-022-026 | Update test infrastructure references | todo | high | 0G |
| INF-TSK-022-027 | Final validation and cleanup | todo | high | 0G |

## Dependencies

### Blocked By

- None -- Epic 0 is the universal prerequisite with no external dependencies

### Blocks

- INF-EPC-023 (Epic A: Parallel Execution Core) -- requires Rust CLI with `Coordinator` trait
- INF-EPC-024 (Epic B: Data Layer Standardization) -- requires SurrealDB-based data layer
- INF-EPC-025 (Epic C: Global Intelligence Layer) -- requires Rust CLI for daemon mode, knowledge graph extraction pipeline (INGEST/COGNIFY/MEMIFY/SEARCH), Loro CRDT KG sync, extended ontology (16 entity types, 18 relationship types)
- INF-EPC-026 (Epic D: Model Orchestrator) -- requires Rust CLI + Epic C
- INF-EPC-027 (Epic E: CodeFlow App) -- requires Rust CLI foundation

## Technical Notes

### Architecture

```text
codeflow-rs/                          (Rust workspace)
  codeflow-core/                      (library crate)
    src/
      lib.rs                          (public API surface)
      types/                          (domain types -- enums, tagged unions)
        session.rs                    (SessionState, PathFlowPhase, WorkStage)
        events.rs                     (LedgerEvent tagged union via serde)
        sentinel.rs                   (Sentinel enum with Display impl)
      traits/                         (trait definitions)
        datastore.rs                  (DataStore: CRUD for sessions, tasks, epics)
        ledger.rs                     (LedgerWriter: append-only event logging)
        hook.rs                       (HookHandler: stdin JSON -> stdout JSON)
      store/                          (DataStore implementations)
        surreal.rs                    (SurrealStore -- sole impl, surrealkv://)
      ledger/                         (LedgerWriter implementations)
        jsonl.rs                      (JsonlWriter -- flock + serde)
      session/                        (session management, state machine)
      hooks/                          (HookHandler implementations)
        session_start.rs              (SessionStart hooks)
        session_end.rs                (SessionEnd hooks)
        pre_tool_use.rs               (gate-check, team-guard, etc.)
        post_tool_use.rs              (sentinel-write, checkpoint, etc.)
        logging.rs                    (activity writer, prompt logging)
      workgraph/                      (task/epic CRUD, status transitions)
      worktree/                       (git2-based worktree management)
      config/                         (configuration loading)
      doctor/                         (infrastructure diagnostics)
      autorun/                        (batch worker, orchestrator)
  codeflow-cli/                       (binary crate)
    src/
      main.rs                         (Clap CLI entry point)
      commands/                       (subcommand dispatch to codeflow-core)
```

### Go Package to Rust Module Mapping

| Go Package | Rust Module | Notes |
|-----------|------------|-------|
| `internal/db/` | `store/surreal.rs` | SQLite -> SurrealDB embedded |
| `internal/ledger/` | `ledger/jsonl.rs` | Same flock semantics, serde serialization |
| `internal/session/` | `session/` | Builder pattern, enum state machine |
| `internal/workstate/` | `session/` (merged) | Consolidated with session |
| `internal/config/` | `config/` | Direct port |
| `internal/worktree/` | `worktree/` | git2 crate replaces exec.Command("git") |
| `internal/claim/` | Deferred to Epic A | LoroCoordinator replaces state.json |
| `internal/workgraph/` | `workgraph/` | DataStore trait, not direct DB |
| `internal/hooks/` | `hooks/` | HookHandler trait |
| `internal/sentinel/` | `pathflow/` (merged) | Sentinel file read/check operations, consumed by hooks |
| `internal/pathflow/` | `pathflow/` (merged) | Checkpoint state management, consumed by hooks |
| `internal/autorun/` | `autorun/` | Typed batch config |
| `internal/doctor/` | `doctor/` | Direct port |
| `internal/validate/` | `validate/` | Direct port |
| `internal/idgen/` | `idgen.rs` | ULID generation, direct port |
| `internal/settings/` | `settings/` | settings.json read/validation |
| `internal/githooks/` | `codeflow-cli/commands/githooks.rs` | CLI-layer: commit-msg, pre-push, pre-commit validation |
| `internal/initialize/` | `codeflow-cli/commands/init.rs` | CLI-layer: project initialization wizard |
| `internal/verification/` | `codeflow-cli/commands/` or test module | CLI-layer: benchmark verification, incorporated into tests |
| `cmd/codeflow/` | `codeflow-cli/commands/` | Clap dispatch |

### Key Constraints

1. **Frozen external interface** -- identical CLI subcommands, hook JSON contracts, exit codes, file I/O
2. **SurrealDB-only** -- NO SqliteStore, NO SQLite dependency
3. **Same binary name** -- output binary is `codeflow`
4. **No new features** -- 1:1 behavioral replacement
5. **Go binary remains active** throughout Phases 0A-0F
6. **Rust binary builds in parallel** -- `codeflow-rs/` alongside `codeflow-cli/`
7. **Tests in same PR as source** -- every code task includes unit tests
8. **85% coverage threshold** -- per-file enforcement matching Go conventions
9. **Coexistence requirement** -- both `codeflow-cli/` (Go) and `codeflow-rs/` (Rust) exist simultaneously during migration. The Go binary remains the active `codeflow` command until Phase 0G explicitly retires it after all Phase 0F contract conformance tests pass.

### Rust-Idiomatic Design Principles

This is an **idiomatic redesign**, not a mechanical 1:1 port. All implementation tasks MUST follow these principles:

1. **Strong type system / no raw strings** -- Newtype wrappers for all IDs (`SessionId`, `TaskId`, `EpicId`, `FormatId`, `BranchName`). Enums for all categorical values (`WorkType`, `SessionState`, `HookEvent`, `TaskStatus`). Sum types for state machines. `From`/`TryFrom` for type conversions. Exhaustive `match` on enums (compiler-enforced completeness).
2. **Modular restructuring** -- Rust module tree is designed for Rust, not translated from Go's flat package layout. Nested modules, re-exports, `pub(crate)` visibility used idiomatically.
3. **DRY principle** -- Shared traits, generic implementations, and derive macros eliminate Go's duplicated patterns (error handling boilerplate, similar struct definitions, duplicated validation).
4. **Rust correctness patterns** -- `Result<T, E>` with domain-specific error enums via `thiserror` in `codeflow-core`; `anyhow` ONLY at CLI binary boundary. `Option<T>` instead of Go zero-value patterns. Builder pattern for complex configurations. Zero `unsafe` code. Minimize `.clone()` -- prefer borrows and lifetimes.

**Note:** Decision #22 originally estimated 25 tasks. Task 004 (MCP servers + cf-surrealdb-standards) was added per team lead requirements, and Phase 0G expanded from 3 to 6 tasks (dedicated coverage enforcement, cross-compilation, and test infrastructure update tasks), bringing the total to 27.

## Related

- Analysis: `.codeflow/docs/analysis/parallel-work/README.md` (Epic 0 section)
- Decisions: `.codeflow/docs/analysis/parallel-work/decisions.md` (#1-6, #8, #14, #16, #17, #22)
- Architecture: `.codeflow/docs/analysis/parallel-work/data-layer-protection.md` (Section 6)
- Schema: `.codeflow/docs/analysis/parallel-work/schema-standardization.md` (JSONL audit)
- CRDT: `.codeflow/docs/analysis/parallel-work/crdt-coordination.md` (module tree)
- Knowledge Graph: `.codeflow/docs/analysis/parallel-work/knowledge-graph-engine.md` (KG engine analysis, ontology, extraction pipeline)
- Predecessor: `INF-EPC-021` (Go CLI Integration -- completed, being replaced)
