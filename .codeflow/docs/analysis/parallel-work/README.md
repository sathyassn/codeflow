---
title: "Parallel Work Support Analysis"
type: analysis
status: active
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-05"
epic: "PLN-EPC-001"
related: "V4 Specification, PathFlow Architecture, Worktree Manager"
---

# Parallel Work Support Analysis

## Overview

This analysis package examines what is required to enable true parallel PathFlow sessions -- multiple independent work streams executing simultaneously in the same repository. CodeFlow currently operates as a strictly single-session system, enforced by singleton state files and destructive initialization routines.

## Documents

| Document | Description |
|----------|-------------|
| [Worktree Architecture](worktree-architecture.md) | Session-scoped state inventory, worktree isolation model, SessionStart/SessionEnd lifecycle, detached HEAD pattern |
| [Data Layer Protection](data-layer-protection.md) | JSONL append safety, claims TOCTOU race and Loro resolution, CLI-only enforcement, SurrealDB evaluation (decided: SurrealDB-only), pure Rust CLI architecture, global DB architecture summary |
| [CRDT Coordination](crdt-coordination.md) | Loro CRDT integration plan, claims migration to Loro Map, sync daemon subcommand design, multi-machine delta sync via git ref, coordination examples |
| [Schema Standardization](schema-standardization.md) | Ledger and log audit, per-file target schemas, field name standardization, retention policies, cleanup CLI |
| [Autorun Integration](autorun-integration.md) | Worktree-integrated autorun workers, pre-created worktree detection, worker lifecycle |
| [Global Intelligence Layer](global-intelligence.md) | Global daemon architecture, project registration, cross-project visibility, local ONNX embeddings, SurrealDB graph queries |
| [Product Strategy](product-strategy.md) | Pricing model, open-core analysis, competitive positioning, go-to-market approach |
| [Decisions](decisions.md) | All 22 analyzed recommendations with options, trade-offs, and justified decisions |

## Key Findings

1. **Session-scoped state is already isolated.** PathFlow flags, sentinels, and checkpoints live under `.state/session/{SID}/` and do not collide between sessions (`start.go:640-671`, `start.go:674-684`).

2. **Three critical singletons block parallelism.** The `codeflow-env.sh` file (`session.go:301-321`), `/tmp/claude/{project}/` directory (`start.go:830-850`), and `active-task.json` (`activetask.go:39-63`) are all written per-session without session scoping. Concurrent sessions overwrite each other.

3. **The worktree manager already exists and splits state correctly.** `worktree.go:110-191` creates git worktrees with shared dirs (`db`, `ledger`, `coordination`, `logs`) symlinked and local dirs (`runtime`, `session`, `sentinels`) created fresh per worktree. Worktree creation must happen at SessionStart (hook-level), not PF1-INIT, so all PathFlow phases execute within the worktree.

4. **The data layer is partially ready.** Ledger writes use `flock` for file I/O append safety (`writer.go:74-119`). SQLite uses WAL mode with `busy_timeout=5000` and retry logic (`connection.go:85-115`). However, state.json for claims uses non-atomic read-modify-write (`claim.go:89-133`) and must be replaced with Loro Map CRDT (the sole coordination mechanism).

5. **The claims system exists but is not wired into PathFlow.** `claim.go:148-221` implements pattern-based conflict detection with fencing tokens, but no PathFlow hook calls `Acquire()` before Edit/Write operations. The claims state.json has a TOCTOU race that is resolved by migrating to Loro Map CRDT.

6. **Autorun workers have zero worktree integration.** `worker.go:109` (`TmuxWorker.Run()`) passes `WorkDir: "."` -- all workers share the project root. Each autorun worker needs its own worktree.

7. **The ledger and log subsystem has significant schema inconsistencies.** Five distinct naming conventions coexist (`ts` vs `timestamp`, `event` vs `event_type` vs `e`), security logs use pretty-printed multi-line JSON instead of compact JSONL, and no retention policies exist for any log category.

8. **Data layer CLI access lacks enforcement.** `codeflow db exec` accepts arbitrary SQL with zero validation. Agents can bypass the CLI entirely via `sqlite3` shell. Neither `.state/ledger/` nor `.state/db/` is in the edit-write-guard's `blocked_directories` list.

9. **SurrealDB offers compelling multi-model capabilities for agentic use cases** (vector, graph, document, time-series in a single query language), but the Go embedded mode does not exist -- only WebSocket/HTTP connections work from Go. True embedded mode exists only in Rust, JS, Python, and WASM.

10. **Loro CRDT integration is feasible and should be included in this epic.** Loro is Rust-native. With the pure Rust CLI (Epic 0), Loro is a direct crate dependency -- `use loro::LoroDoc;`. The V4 spec explicitly designed for `state.loro` at `.state/coordination/`. The current `crdt.go` is JSONL replay (not true CRDT). Loro provides Map, List, Text, Tree CRDTs with delta-based op-log sync and deterministic merge.

11. **A pure Rust CLI redesign solves the SurrealDB, Loro, and CGo complexity gaps simultaneously.** By redesigning the Go CLI in idiomatic Rust as a prerequisite Epic 0, CodeFlow eliminates CGo FFI overhead, shared library cross-compilation, and two-language build complexity. The redesign is not a line-for-line port -- it introduces trait-based abstractions (`DataStore`, `Coordinator`, `Transport`), type system leverage (enums for states, tagged unions for events), and domain-specific error types that provide compile-time correctness guarantees. Loro and SurrealDB become direct crate dependencies (`use loro::LoroDoc;`, `use surrealdb::Surreal;`) -- native Rust, zero binding indirection. The sync daemon becomes a `codeflow sync daemon` subcommand within the single `codeflow` binary. The external interface is FROZEN: same binary name, subcommands, hook JSON contracts, and exit codes.

12. **SurrealDB replaces SQLite entirely — no hybrid mode.** The decision to use a pure Rust CLI (Epic 0) makes true embedded SurrealDB (`surrealkv://`) available as a direct crate dependency. SQLite is dropped completely. `SurrealStore` is the sole `DataStore` implementation with two connection modes: `surrealkv://` for project-local embedded operations, and `ws+unix://` for optional global daemon access. The three-tier data model is updated: Tier 0 JSONL (unchanged), Tier 1 SurrealDB embedded (replaces SQLite), Tier 1+ SurrealDB daemon (optional global), Tier 2 Markdown (unchanged). JSONL remains the rebuild authority. See [Decision #17](decisions.md#17-surrealdb-as-single-database-platform).

13. **A global database layer enables cross-project intelligence as an optional, modular feature.** The default mode remains project-only with SurrealDB embedded — no daemon required. `codeflow global enable` starts the SurrealDB daemon inside the codeflow binary (Unix socket at `~/.codeflow/db.sock`, no external install, no Docker). Projects opt in to sharing via `.codeflow/config/project.toml` visibility config. Local ONNX embeddings (`all-MiniLM-L6-v2`, 384 dims, via the `ort` crate) enable offline semantic search across linked project memories. Graceful degradation: if the daemon is unavailable, all project-local work continues unaffected. See [Decision #18](decisions.md#18-global-database-architecture) and [Global Intelligence Layer](global-intelligence.md).

**Assessment:** Parallel work support is architecturally feasible with substantial but achievable effort. The work now organizes into six epics. Epic 0 (Rust CLI idiomatic redesign with SurrealDB-only data layer) is the universal prerequisite foundation. Epic A (parallel execution core with native Loro CRDT) and Epic B (data layer standardization) can run in parallel after Epic 0. Epic C (Global Intelligence Layer — daemon, project registry, cross-project queries, local embeddings) also runs after Epic 0. Epic D (Model Orchestrator — config-driven routing to external models) depends on Epic 0 and Epic C. Epic E (Dashboard) is deferred until the data and orchestration layers stabilize. Loro CRDT is the sole coordination mechanism. The SurrealDB-only decision simplifies the architecture by eliminating the hybrid SQLite+SurrealDB scenario that would have required data sync logic between two stores.

## V4 Specification Gap Analysis

### Spec Vision vs Implementation

| V4 Spec Concept | Implementation Status | Gap |
|----------------|----------------------|-----|
| Git worktrees for isolation | Manager exists (`worktree.go`) with shared/local split | Not integrated into SessionStart or autorun; needs `SetupDetached()` method |
| Loro CRDT for coordination | `state.loro` file exists on disk; CRDT rebuild from JSONL exists (`crdt.go`) | state.json (plain JSON) is the actual state file; Loro binary is an unused artifact |
| Claims for conflict detection | Full lifecycle implemented (`claim.go:148-221`) | Not wired into PathFlow hooks (no Acquire before Edit/Write); state.json has TOCTOU race |
| Fencing tokens | Implemented in claims (`claim.go:178`, `claim.go:35`) | Not validated by any hook or guard |
| Parallel autorun workers | Worker infrastructure exists (`worker.go`) | `WorkDir: "."` -- no worktree isolation |
| Session-scoped state | Partial: flags/checkpoints/sentinels are scoped; env/active-task are not | Three CRITICAL singletons need scoping |

### Critical Missing Integrations

1. **Claim acquisition in PreToolUse hooks:** The gate-check hook enforces PathFlow phase ordering but does not acquire resource claims. For parallel safety, every Edit/Write tool call should acquire an exclusive claim on the target file pattern before proceeding.

2. **Worktree setup in session initialization:** `StartInit()` at `start.go:189-305` creates directories and PathFlow state but never calls `worktree.Manager.Setup()`. The parallel session flow must create a worktree before writing any state.

3. **Fencing token validation:** Claims include fencing tokens (`claim.go:35`) but no hook validates that the token presented with a write operation matches the latest token for the claimed resource.

4. **Merge conflict resolution:** The V4 spec envisions branches per parallel session merged back to main. No automated merge conflict resolution exists.

## Proposed Epic Structure

Given the full scope (Rust CLI redesign, worktree integration, Loro CRDT, SurrealDB-only data layer, schema standardization, autorun, data access enforcement, global intelligence, model orchestration), the work spans 80+ tasks and is organized into 6 epics.

**Structure: 6 epics.** Epic 0 (Rust CLI idiomatic redesign — universal prerequisite), Epic A (parallel execution core), Epic B (data layer standardization), Epic C (Global Intelligence Layer), Epic D (Model Orchestrator), Epic E (Dashboard — deferred).

### Epic 0: Rust CLI — Idiomatic Redesign

**Scope:** Replace the Go CLI (`codeflow-cli/`) with a pure Rust CLI. This is NOT a line-for-line port — it is a redesign opportunity. The Go codebase is the functional specification, but the Rust implementation applies idiomatic patterns, consolidates duplicate logic, and leverages Rust's type system for correctness guarantees that Go cannot express.

**External interface contract (FROZEN):**

| Surface | Contract | Constraint |
|---------|----------|-----------|
| Binary name | `codeflow` | Unchanged — all hooks, scripts, and CI reference this name |
| Subcommands | `codeflow hooks session-start init`, `codeflow test`, etc. | Same command tree. Clap replaces Cobra but external interface identical. |
| Hook stdin/stdout | JSON contract per hook event type | Byte-for-byte compatible. Hooks receive same JSON, produce same JSON. |
| Exit codes | 0 (success), 1 (error), 2 (soft block) | Same semantics for all hook handlers and CLI commands. |
| File I/O | `.state/`, `project-management/`, JSONL ledger paths | Same paths, same formats, same atomicity guarantees. |

Everything behind this interface is fair game for redesign.

**Redesign principles:**

1. **Trait-based abstraction for swappable implementations.** Core behaviors are defined as traits, not concrete types. This enables testing with mocks, future backend swaps, and clean separation of concerns.

   | Trait | Purpose | Initial Impl | Future Impl |
   |-------|---------|-------------|-------------|
   | `DataStore` | Database read/write operations | `SurrealStore` (embedded `surrealkv://`) | (extensible — e.g., `ws+unix://` for global daemon) |
   | `Coordinator` | CRDT coordination for parallel work | `LoroCoordinator` (Epic A) | (extensible) |
   | `Transport` | Sync delta exchange mechanism | `FileTransport` (local file) | `GitRefTransport` (Epic A) |
   | `LedgerWriter` | Append-only event logging | `JsonlWriter` (flock + append) | (extensible) |
   | `HookHandler` | Hook event processing | Per-event structs | (extensible) |

2. **DRY consolidation.** The Go CLI has duplicate logic across packages (e.g., session ID validation in multiple places, JSON read/write patterns repeated, error formatting inconsistent). The Rust redesign consolidates these into shared modules.

3. **Type system leverage.** Rust's enums and type system encode states that Go represents as strings or booleans:

   | Go Pattern | Rust Redesign |
   |-----------|---------------|
   | `status string` ("pending", "active", "complete") | `enum SessionState { Pending, Active, Complete }` |
   | `phase int` + manual bounds checking | `enum PathFlowPhase { PF1Init, PF2Context, ..., PF7End }` with `impl` for valid transitions |
   | `eventType string` in JSONL | `enum LedgerEvent { SessionStart { .. }, PhaseTransition { .. }, ... }` (tagged union via serde) |
   | `if err != nil` repeated per call | `Result<T, E>` + `?` operator — errors propagate automatically |
   | Sentinel names as strings | `enum Sentinel { Phase(PathFlowPhase), Stage(WorkStage) }` with `Display` impl |

4. **Domain-specific error types.** Replace Go's `fmt.Errorf` with `thiserror`-derived enums per domain:

   | Domain | Error Enum | Example Variants |
   |--------|-----------|------------------|
   | Session | `SessionError` | `InvalidId`, `AlreadyActive`, `NotFound` |
   | Database | `DbError` | `ConnectionFailed`, `MigrationError`, `QueryFailed(String)` |
   | Hooks | `HookError` | `InvalidInput`, `GateBlocked(String)`, `SentinelMissing` |
   | Ledger | `LedgerError` | `LockFailed`, `SchemaViolation`, `AppendFailed` |

5. **Builder pattern for complex configuration.** Session creation, database connections, and hook context use builders instead of multi-parameter constructors.

6. **Serde for all serialization.** Every struct that touches JSON (hook input/output, JSONL events, config files) derives `Serialize`/`Deserialize`. No manual JSON construction.

7. **Modular crate structure.** Core library crate (`codeflow-core`) separated from CLI binary crate (`codeflow-cli`). The core crate is independently testable and reusable; the CLI crate is a thin command-dispatch layer.

**What this eliminates:**
- `codeflow-data/` separate Rust workspace (no longer needed — the CLI IS Rust)
- `libcodeflow_data` shared library (no separate library — Loro and SurrealDB are direct deps)
- `internal/datalib/` Go CGo wrapper package (no CGo — native Rust)
- CGo FFI boundary (~100-200ns/call overhead, cross-compilation complexity, two-language build)
- `codeflow-sync/` separate binary (sync daemon is a subcommand)
- Two build pipelines (Go + Rust → just Rust)

#### Phase 0A: Codebase Analysis + Tooling

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 1 | Audit Go CLI for redesign opportunities | Analyze `codeflow-cli/` (27 packages under `internal/`, `cmd/codeflow/`) to identify: DRY violations (duplicate logic across packages), tightly coupled components (packages that import each other circularly or share globals), patterns that simplify under Rust idioms (error handling, state machines, tagged unions), and Go-specific workarounds that Rust eliminates (interface{} casts, string-typed enums, manual JSON marshaling). Produce a findings document with specific file:line references. | M |
| 2 | Define trait hierarchy and module map | Design the `DataStore`, `Coordinator`, `Transport`, `LedgerWriter`, and `HookHandler` traits. Map each Go package to its Rust module. Identify which Go packages merge (consolidation) and which split (separation of concerns). Produce a module dependency graph. | M |
| 3 | Create cf-rust-standards skill | Create `.claude/skills/cf-rust-standards/SKILL.md` (parallels existing `cf-go-standards`, `cf-shell-standards`, `cf-python-standards`). Covers: workspace and crate structure conventions, error handling (`thiserror` for library errors in `codeflow-core`, `anyhow` for CLI binary in `codeflow-cli`), trait design patterns (trait objects vs generics — when to use each), `serde` serialization conventions, async patterns (`tokio` runtime), testing conventions (`#[test]`, `#[tokio::test]`, property-based with `proptest`, snapshot with `insta`), naming conventions (snake_case, module structure), Clippy lint configuration and enforcement (`#![deny(clippy::all)]`), `unsafe` policy (when permitted, review requirements), crate dependency governance (approved crates list: `clap`, `serde`, `thiserror`, `anyhow`, `rusqlite`, `loro`, `surrealdb`, `git2`, `tokio`, `proptest`, `insta`, `cargo-nextest`), and builder pattern for complex configuration. | M |
| 4 | Configure Rust MCP servers | Add two MCP servers to `.claude/settings.json` or `.claude/settings.local.json`: **(1) Context7 MCP** — fetches up-to-date documentation for any crate (loro, surrealdb, clap, tokio, serde, etc.), prevents hallucinated APIs. Install: `claude mcp add context7 -- npx -y @upstash/context7-mcp@latest`. **(2) rust-analyzer MCP** — bridges AI with rust-analyzer for code intelligence, type checking, trait resolution. Install: `claude mcp add-json "rust-analyzer" '{"command":"rustmcp","args":[]}'`. Verify both MCPs respond correctly. Document MCP usage in cf-rust-standards skill. | S |

#### Phase 0B: Foundation + Testing Infrastructure

**Key principle:** Testing infrastructure is a Phase 0B deliverable. The test harness MUST be in place BEFORE migrating the first package in Phase 0C. Every migrated package must pass both its Rust unit tests AND the relevant shell integration tests before moving to the next package.

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 5 | Initialize Rust workspace | Create `codeflow-rs/` workspace with two crates: `codeflow-core` (library) and `codeflow-cli` (binary). `Cargo.toml` workspace with shared dependencies. Clap CLI skeleton mirroring existing subcommand tree. Wire `cargo build` alongside Go build — both binaries coexist during migration. | M |
| 6 | Core types and error domains | Define domain types: `SessionState` enum, `PathFlowPhase` enum with transition validation, `WorkStage` enum, `Sentinel` enum, `LedgerEvent` tagged union. Define error enums with `thiserror`: `SessionError`, `DbError`, `HookError`, `LedgerError`. These types are the foundation all other modules build on. | M |
| 7 | Set up Rust unit testing infrastructure | Configure `cargo test` for all Rust unit tests (`#[test]`, `#[tokio::test]`). Set up coverage via `cargo-llvm-cov` or `cargo-tarpaulin` with 85% minimum threshold (matching current Go business package threshold). Add `proptest` for property-based testing (serialization roundtrips, CRDT operations). Add `insta` for snapshot testing (JSON/YAML output verification). Configure `cargo-nextest` for faster parallel test execution. Add `Makefile` targets: `test-cover` (coverage with threshold), `test-race` (thread sanitizer equivalent via `cargo test` with sanitizer flags or `#[cfg(loom)]`). | M |
| 8 | Set up shell integration test bridge | Adapt or create Rust equivalent of `.codeflow/testing/cli/test-go-cli.sh` bridge script. The existing ~1,555 shell tests MUST pass UNCHANGED against the Rust binary — they call the binary externally via the frozen interface. Ensure `codeflow test` command works from the Rust binary (it orchestrates the shell test suite). The bridge script must: build the Rust binary, place it on PATH, and run the existing shell test suite. Go unit tests are ported to Rust `#[test]` as part of each package migration (Phases 0C-0E); shell integration tests require NO changes. | M |

#### Phase 0C: Core Library Crate

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 9 | Implement `DataStore` trait and `SurrealStore` | `DataStore` trait with CRUD operations for sessions, tasks, epics, workgraph. `SurrealStore` implementation using the `surrealdb` crate in embedded `surrealkv://` mode. Port `internal/db/` query logic, rewriting SQL as SurrealQL. No `SqliteStore` — SurrealDB replaces SQLite from the start. | L |
| 10 | Implement `LedgerWriter` trait and `JsonlWriter` | `LedgerWriter` trait for append-only event logging. `JsonlWriter` implementation with flock-based locking, serde serialization of `LedgerEvent` enum, schema validation at compile time via typed events. Port `internal/ledger/` logic. | M |
| 11 | Implement session and state management | Port `internal/session/`, `internal/workstate/`, `internal/config/`. Session builder pattern. `SessionState` enum-based state machine with compile-time valid transitions. Active-task management, codeflow-env.sh read/write. | L |
| 12 | Implement worktree manager | Port `internal/worktree/`. Git operations via `git2` crate. Registry (worktrees.yaml), shared/local symlinks, setup/cleanup. Typed worktree states. | M |
| 13 | Implement claims system | Port `internal/claim/`. Replace state.json with direct Loro Map integration (`use loro::LoroDoc;`). Claim acquisition/release as typed operations. | L |
| 14 | Implement workgraph operations | Port `internal/workgraph/`. Task/epic CRUD, status transitions (as enum state machines), queries. Uses `DataStore` trait — not `SurrealStore` directly. | M |

#### Phase 0D: Hook Handlers

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 15 | Implement `HookHandler` trait and session hooks | `HookHandler` trait with `fn handle(&self, input: HookInput) -> Result<HookOutput, HookError>`. Session hooks (start, end) as `HookHandler` impls. Serde for stdin JSON parsing, stdout JSON output. Exit code semantics preserved. | L |
| 16 | Implement tool-use hooks | Pre-tool-use gates (gate-check, team-guard, edit-write-guard, etc.) and post-tool-use handlers (sentinel-write, checkpoint-register, etc.) as `HookHandler` impls. Sentinel and checkpoint logic ported from `internal/hooks/tooluse/`. | L |
| 17 | Implement logging and autorun | Port `internal/hooks/logging/` (activity writer, prompt logging, tool-use logging) and `internal/autorun/` (worker, orchestrator, batch parsing). Logging uses `LedgerWriter` trait. Autorun uses typed batch configuration. | L |

#### Phase 0E: CLI Commands

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 18 | Implement CLI command dispatch | Port `cmd/codeflow/` subcommands (hooks, db, workgraph, state, ledger, test, doctor, cleanup, validate) to Clap commands in the `codeflow-cli` crate. Each command is a thin dispatch to `codeflow-core` functions. | L |
| 19 | Add sync daemon subcommand | `codeflow sync daemon` — Loro delta sync as a subcommand. Uses `loro` + `git2` crates natively. Same binary, no separate process. Peer ID at `.state/runtime/peer-id`. | L |

#### Phase 0F: Integration Testing + Contract Conformance

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 20 | Port Go unit tests to Rust | Port Go integration tests to Rust `#[test]` and `#[tokio::test]`. Feature parity with `make test-cover` and `make test-race` equivalents. Test against the frozen external interface (subcommands, JSON contracts, exit codes). Trait-based design enables unit testing core logic with mock implementations. Property-based tests with `proptest` for serialization roundtrips. Snapshot tests with `insta` for JSON output verification. | L |
| 21 | Contract conformance test suite | Automated tests that verify the Rust binary produces identical output to the Go binary for all hook events and CLI commands. Run both binaries against the same inputs, diff outputs. This is the migration safety net. Must cover: all hook stdin/stdout JSON contracts, all CLI subcommand outputs, all exit code paths, all file I/O (JSONL, SQLite, state files). | M |
| 22 | Validate shell test suite passes | Run the full ~1,555 shell integration test suite against the Rust binary via the bridge script from task 8. All tests must pass UNCHANGED — these tests exercise the frozen external interface. Any failure indicates a contract violation that must be fixed before proceeding to Phase 0G. | M |

#### Phase 0G: CI/CD + Cutover

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 23 | CI pipeline for Rust binary | GitHub Actions workflow update: replace Go build/test jobs with Rust equivalents. Jobs: `cargo build --release` for darwin-arm64, darwin-amd64, linux-amd64 (cross-compilation). `cargo test` (unit + integration). `cargo clippy -- -D warnings` (lint enforcement, deny warnings). `cargo fmt --check` (format enforcement). Coverage job with 85% threshold gate via `cargo-llvm-cov`/`cargo-tarpaulin`. Run contract conformance suite. Run shell integration test suite. | L |
| 24 | Cutover and Go removal | Remove `codeflow-cli/` Go source. Rename `codeflow-rs/` to `codeflow-cli/`. Update all references. Single `codeflow` Rust binary. Final validation of all 1,555+ shell tests against the Rust binary. Remove Go-specific CI jobs, `Makefile` targets, and `.golangci.yml`. | M |
| 25 | Update test infrastructure references | Update `.codeflow/testing/cli/test-go-cli.sh` bridge script (or its Rust replacement) to point to the final `codeflow-cli/` location. Update `codeflow test` orchestration. Ensure `codeflow doctor` validates the Rust binary. Remove Go test bridge artifacts. | S |

**Test migration strategy summary:**
- **Go unit tests** → Rust `#[test]` (ported as part of each package migration in Phases 0C-0E)
- **Shell integration tests** → NO CHANGES (they call the binary externally via frozen interface)
- **Coverage** → `cargo-llvm-cov` or `cargo-tarpaulin` with 85% threshold (same as Go)
- **Race detection** → Thread sanitizer via `cargo test` sanitizer flags or `#[cfg(loom)]` for concurrency-sensitive code
- **Parallel execution** → `cargo-nextest` for faster test runs
- **Lint enforcement** → `cargo clippy -- -D warnings` (zero warnings policy)
- **Format enforcement** → `cargo fmt --check` (consistent formatting)

### Epic A: Parallel Execution Core (19 tasks)

**Scope:** Loro CRDT coordination, worktree integration, singleton elimination, claims wiring, autorun integration. All implemented in the pure Rust CLI from Epic 0.

**Design principle:** Loro CRDT is the sole coordination mechanism. With the Rust CLI, Loro is a direct crate dependency (`use loro::LoroDoc;`) -- no FFI, no shared library, no CGo wrappers.

**Prerequisite:** Epic 0 (Rust CLI redesign) must be complete. Epic A extends Epic 0's trait hierarchy: `LoroCoordinator` implements the `Coordinator` trait, `GitRefTransport` implements the `Transport` trait.

#### Phase A: Loro CRDT Foundation

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 1 | Fix worktree base directory default | Change code default from `.claude/worktrees` to `.git-worktrees`. Update tests. Remove empty `.claude/worktrees/` dir if present. `.gitignore:67` is already correct. | S |
| 2 | Replace state.json with state.loro | Migrate claims from JSON to Loro Map CRDT. Direct `loro` crate API -- no FFI. Loro handles concurrency natively for both same-machine and multi-machine. | L |
| 3 | Wire claims into PreToolUse hooks | Acquire exclusive claim before Edit/Write. Release on stage completion. | L |
| 4 | Fencing token validation via Loro | Store and validate fencing tokens in Loro Map. | M |
| 5 | Implement sync daemon logic | `codeflow sync daemon` subcommand (from Epic 0 Phase 0E). Same-machine: compaction and health for shared `state.loro`. Multi-machine: exports Loro deltas via `doc.export(updates(&last_sync_vv))`, pushes to per-peer git ref `refs/coordination/loro/{peer-id}`, fetches all peer refs, imports deltas. Configurable 10-30s interval. Retry with backoff on network partition. | L |
| 6 | Clean up unused state.loro artifact | Remove unused 483-byte file. Replace with real Loro-managed state. | S |

#### Phase B: Worktree Integration + Singleton Scoping

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 7 | Add SetupDetached method | Add `Manager.setup_detached(name)` using `git worktree add --detach`. Same shared/local symlink logic as `setup()`. | M |
| 8 | Integrate worktree into SessionStart | Add worktree creation to session start after session ID generation. Write codeflow-env.sh inside worktree. Set CODEFLOW_WORKTREE_PATH. Claims use Loro from Phase A. | L |
| 9 | Scope codeflow-env.sh per worktree | `session::write_env_file()` writes to worktree's `.state/runtime/`. `session::current()` reads from worktree-local path. | M |
| 10 | Scope active-task.json per worktree | `set_active_task()` writes to worktree's `.state/runtime/`. `get_active_task()` reads from worktree-local path. | M |
| 11 | Scope project temp dir per worktree | Change temp dir to `/tmp/claude/{project}/{worktree-name}/managed/protected-edits/`. Remove shared directory cleanup. | S |
| 12 | Scope team config by session | Use `{project}-{task-format-id}` or `{project}-{SID-short}` format for team names. | S |
| 13 | Worktree cleanup in SessionEnd | Add worktree removal to session end cleanup. Handle abnormal termination. | M |
| 14 | Pre-created worktree detection | SessionStart hook detects existing worktree (for autorun integration) and skips creation. | S |

#### Phase C: Autorun + Coordination

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 15 | Autorun worktree integration | Update autorun worker to create/cleanup worktree per worker. Pass worktree path as work directory. | L |
| 16 | Merge conflict detection | Before PR creation, check for merge conflicts with target branch. | M |
| 17 | Parallel session coordination | Sessions discover each other via worktree registry. FIFO merge queue. | L |
| 18 | Increase SQLite busy_timeout | Bump to 15000 for parallel mode. Add jitter to retry delay. | S |
| 19 | pathflow-events.jsonl worktree field | Add `worktree` field to all events. Consumers filter by session_id. | S |

### Epic B: Data Layer Standardization (23 tasks)

**Scope:** Schema standardization, CLI-only enforcement, SurrealDB embedded, retention, log cleanup. All implemented in the pure Rust CLI from Epic 0.

**Prerequisite:** Epic 0 (Rust CLI redesign). `SurrealStore` is the sole `DataStore` implementation from Epic 0 — SQLite is not present in the Rust CLI. Epic B Phase D (schema standardization) uses SurrealQL throughout. Phase F has been removed from Epic B — SurrealDB is integral to Epic 0.

#### Phase D: Schema Standardization + Enforcement

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 1 | Standardize event field names | Migrate `ts` to `timestamp`, `event_type`/`e` to `event` across all writers. | M |
| 2 | Add worktree field to all schemas | Universal `worktree` field in every event schema. | M |
| 3 | Fix security log format | Convert pretty-printed JSON to compact JSONL. | M |
| 4 | Convert protection-audit.log | Plain text to JSONL format. Add session_id. Fix root ownership. | S |
| 5 | Unify pr-events writer | Consolidate all pr-events writes into the Rust activity writer. Remove bash echo writes. | M |
| 6 | Enforce memory-events schema | Define structured schema. Variable content in `details` object. Validate via schema validation. | S |
| 7 | Add session_id to commits log | Add `session_id` to commit log struct and all commit log writes. | S |
| 8 | Enhance prompts log | Add `agent_role`, `task_id`, `word_count`, `summary` fields to prompt logging. | M |
| 9 | Revive file-changes log | Re-enable with `agent_role`, `agent_name`, `worktree`, `task_id`. | M |
| 10 | Revive tasks log | Re-enable with agent_type, description, result_summary, duration. | M |
| 11 | Add `.state/ledger/` and `.state/db/` to blocked_directories | Update enforcement-policy.json. Closes Edit/Write bypass. | S |
| 12 | Add PreToolUse hook blocking sqlite3 and db exec | Reject Bash tool calls containing `sqlite3 .state/db/` or `codeflow db exec`. | M |
| 13 | Add PreToolUse hook blocking direct JSONL writes | Reject Bash tool calls writing directly to `.state/ledger/*.jsonl`. | S |
| 14 | Replace db exec with named ops | Remove raw SQL `codeflow db exec`. Named commands only. | L |
| 15 | Strengthen db query SQL parsing | Parse SQL to reject write operations including CTEs with side effects. | M |
| 16 | Update cf-knowledge-layer SOPs | Remove all raw SQL. CLI-only commands. | M |

#### Phase E: Retention + Cleanup

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 17 | Implement retention CLI | `codeflow cleanup --retention` with per-category policies. | M |
| 18 | Clean up discontinued logs | Remove stop-events, git-operations, sentinel, test-category. | S |
| 19 | Archive Tier 0 ledger files | Implement 90-day archival for ledger JSONL files. | M |

#### Phase F: SurrealDB Embedded (Native Rust)

> **Note:** SurrealDB integration is now part of Epic 0 (not Epic B). Epic 0 uses `SurrealStore` as the only `DataStore` implementation from the start — SQLite is never present in the Rust CLI. Epic B Phase F is removed. Schema standardization work (Phases D and E) is still in Epic B and uses SurrealQL rather than SQL.

### Epic C: Global Intelligence Layer (~12-15 tasks, 3 phases)

**Scope:** Global daemon, project registration, cross-project visibility, local ONNX embeddings, cross-project SurrealQL queries. See [Global Intelligence Layer](global-intelligence.md) for the full design.

**Prerequisite:** Epic 0 (Rust CLI with SurrealDB embedded).

| Phase | Description |
|-------|-------------|
| Phase G: Daemon + Registry | `codeflow db daemon start` subcommand (SurrealDB daemon inside the binary), Unix socket at `~/.codeflow/db.sock`, `codeflow global enable/disable`, project registration schema, launchd/systemd service install |
| Phase H: Sync Engine | Project-to-daemon sync via SurrealDB live queries, `codeflow project register/link`, visibility graph relations, graceful degradation when daemon unavailable |
| Phase I: Local Embeddings + Search | `all-MiniLM-L6-v2` via `ort` crate (384 dims, ~50MB, offline), HNSW index on memory/task tables, `codeflow context search` combining vector + graph in single SurrealQL query |

### Epic D: Model Orchestrator (~8-10 tasks, 2 phases)

**Scope:** Port archived `cf-model-orchestrator` skill to Rust CLI. Config-driven model-to-stage routing. T1 (one-shot tmux) and T3 (persistent tmux) execution modes.

**Prerequisites:** Epic 0 + Epic C (model routing uses global context for task selection).

| Phase | Description |
|-------|-------------|
| Phase J: Core Orchestrator | Config schema for model-to-stage routing, T1/T3 tmux execution engine, model invocation API |
| Phase K: Integration + Routing | Wire model routing into PathFlow work stages (WS-DEV, WS-REV, WS-QA), stage completion detection, result ingestion back into task graph |

### Epic E: Dashboard (deferred, scope TBD)

**Scope:** SvelteKit + Tauri desktop application. Deferred until Epics 0-D stabilize. Depends on a stable data layer (Epic 0 + B), global intelligence (Epic C), and defined API surface from the Rust CLI.

### Epic Dependencies

```text
Epic 0: Rust CLI — Idiomatic Redesign (PREREQUISITE — must complete first, 25 tasks)
    SurrealStore is the ONLY DataStore impl (surrealkv:// embedded, no SqliteStore)
    Phase 0A (Codebase Analysis + Tooling)
    Phase 0B (Foundation + Testing Infrastructure — BEFORE Phase 0C code migration)
    Phase 0C (Core Library Crate — DataStore/SurrealStore, LedgerWriter, session, worktree, claims)
    Phase 0D (Hook Handlers)
    Phase 0E (CLI Commands)
    Phase 0F (Integration Testing + Contract Conformance)
    Phase 0G (CI/CD + Cutover — Go CLI removed, single Rust binary)
         |
         +---> Epic A: Parallel Execution Core (19 tasks)
         |         Phase A (Loro CRDT — LoroCoordinator via Coordinator trait)
         |         Phase B (Worktree + Singleton Scoping)
         |         Phase C (Autorun Integration)
         |
         +---> Epic B: Data Layer Standardization (~16 tasks, 2 phases)
         |         Phase D (Schema Standardization in SurrealQL + Enforcement)
         |         Phase E (Retention + Cleanup)
         |         [Phase F removed — SurrealDB is in Epic 0]
         |
         +---> Epic C: Global Intelligence Layer (~12-15 tasks, 3 phases)
                   Phase G (Daemon + Registry)
                   Phase H (Sync Engine)
                   Phase I (Local Embeddings + Cross-Project Search)
                        |
                        +---> Epic D: Model Orchestrator (~8-10 tasks, 2 phases)
                                  Phase J (Core Orchestrator)
                                  Phase K (Integration + Stage Routing)
                                       |
                                       +---> Epic E: Dashboard (deferred, TBD)
```

**Key dependency notes:**

- Epic 0 is the universal prerequisite. It introduces `SurrealStore` as the only `DataStore` impl (no SQLite). All downstream epics build on Epic 0's trait hierarchy.
- Epics A, B, and C can run in parallel after Epic 0 completes.
- Epic B Phase D can start independently of Epic A — schema standardization in SurrealQL does not depend on parallel execution.
- Epic D depends on Epic 0 and Epic C — model routing uses global project context for cross-project task selection.
- Epic E is deferred until data (Epic 0 + B) and orchestration (Epic C + D) layers are stable.

## Scenarios and Use Cases

### Two Interactive Sessions

**Setup:** Developer opens two terminal windows, starts Claude Code in each for independent tasks.

**Current behavior:** Session B's `StartInit()` calls `createProjectTempDir()` which runs `os.RemoveAll()` on Session A's temp files. Session B's `WriteEnvFile()` overwrites Session A's `codeflow-env.sh`. Session A's next hook invocation reads Session B's session ID.

**With Epic A Phase A+B (Loro + worktrees):** Each session's SessionStart hook creates a worktree (`worktree-{SID}`). codeflow-env.sh, active-task.json, and temp dirs are all per-worktree. Claims are managed via native Loro Map CRDT. If both sessions need the same file, the second session gets a conflict notification.

### Autorun Batch Execution

**Setup:** `codeflow autorun` launches 3 workers for 3 independent tasks.

**Current behavior:** All workers use `WorkDir: "."` (`worker.go:109` (`TmuxWorker.Run()`)). All workers share the same `codeflow-env.sh`, `active-task.json`, and `/tmp/claude/{project}/`. Chaos ensues.

**With Epic A Phase B+C (worktrees + autorun integration):** Each worker gets its own worktree via `Manager.setup_detached()`. Workers operate in isolation. Each creates its own branch. Claims via native Loro prevent file conflicts. FIFO merge queue handles PR ordering.

### Interactive + Autorun Concurrent

**Setup:** Developer works interactively while autorun processes background tasks.

**With full parallel support:** Interactive session runs in its own worktree. Autorun workers run in separate worktrees. Claims prevent file conflicts. The developer's interactive experience is unaffected by background workers.

### Two Sessions Modifying Same File

**Without claims:** Both sessions succeed independently. When Session B creates its PR after Session A's PR is merged, git reports merge conflicts.

**With claims (advisory mode, initial rollout):** Session A acquires an exclusive claim via Loro Map. Session B's attempt returns `ErrConflict` (`claim.go:172-174`). Advisory mode: Session B logs a warning and proceeds. The PR merge conflict is detected at PF6.

**With claims (enforcing mode, later):** Session B is blocked and the orchestrator either queues the task for after Session A completes, or suggests an alternative task with no file overlap.

## Risk Assessment

### Technical Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| SurrealDB write contention under heavy parallel writes | Low | Medium | SurrealDB multi-writer architecture handles concurrent writes better than SQLite WAL. Monitor under load in Epic A Phase C. |
| state.json TOCTOU race in claims | High (on any parallel use) | High | Replace with Loro Map CRDT (Phase A) |
| Git worktree branch conflicts | High (if sessions modify same files) | Medium | Claims prevent overlap; FIFO merge queue |
| Rust CLI redesign scope | Medium | High (blocks Epic A and B -- this IS the foundation) | Idiomatic redesign with codebase analysis phase. Contract conformance tests validate identical external behavior. Go and Rust binaries coexist during migration. |
| SurrealDB embedded in Rust CLI | Low (native Rust) | Medium | Graceful degradation: JSONL Tier 0 is always the rebuild authority. If embedded DB is corrupted, `codeflow db rebuild` restores from JSONL. Embedded mode eliminates sidecar failure mode. |
| Schema migration breaks consumers | Medium | Medium | Dual-write period: write both old and new formats, consumers migrate |
| Stale cleanup destroys peer session | Low (narrow race) | High | PID check mitigates; add advisory lock |
| Hook framework confusion with multiple sessions | Medium | Medium | Ensure all hooks resolve from worktree-local codeflow-env.sh |

### Complexity Risks

| Risk | Description | Mitigation |
|------|-------------|-----------|
| Worktree state split adds maintenance burden | Every new state file must be classified as shared or local | Document classification criteria; add CI check |
| SurrealDB-only data layer (no SQLite) | Full 37-table schema migration, all SQL rewritten in SurrealQL | SurrealDB embedded is native in Rust CLI (Epic 0). JSONL remains rebuild authority — SurrealDB state is always recoverable from Tier 0. |
| Rust CLI redesign scope | Full Go-to-Rust idiomatic redesign, external interface frozen | Incremental migration with coexisting binaries. Contract conformance test suite validates identical external behavior. Codebase analysis phase (0A) identifies consolidation opportunities before implementation. |
| Testing parallel scenarios | Inherently harder to test | Dedicated parallel integration tests; Loro concurrency tests |

### Incremental Rollout Strategy

1. **Epic 0** (Rust CLI redesign) is the prerequisite foundation (25 tasks, 7 phases). It is an idiomatic redesign, not a line-for-line port. `SurrealStore` is the sole `DataStore` implementation — SurrealDB embedded (`surrealkv://`) replaces SQLite from the start. Phase 0A audits the Go CLI and establishes tooling (cf-rust-standards skill, MCP servers). Phase 0B sets up the workspace AND the complete testing infrastructure — testing is ready BEFORE any code migration. Phases 0C-0E implement trait-based abstractions (`DataStore`, `Coordinator`, `Transport`, `LedgerWriter`, `HookHandler`). Phase 0F validates external interface conformance. Go and Rust binaries coexist during migration. The external interface is FROZEN: same binary name, subcommands, hook JSON contracts, and exit codes.

2. **Epic A Phase A** (Loro CRDT foundation) is the first post-migration work. It fixes the TOCTOU race in claims and establishes the coordination layer using native Loro.

3. **Epic A Phase B** (Worktree integration + singleton scoping) builds on Phase A. Worktrees use Loro-backed claims from day one. Opt-in via configuration flag for parallel mode.

4. **Epic A Phase C** depends on Phase B. Autorun integration is the key parallel use case.

5. **Epics B (Phases D-E)** and **C** can begin after Epic 0, running in parallel with Epic A. Epic B covers schema standardization in SurrealQL and enforcement. Epic C builds the global daemon, project registry, and local embedding search.

6. **Epic D** (Model Orchestrator) begins after Epic 0 and Epic C are complete. Config-driven model-to-stage routing uses global project context for intelligent task selection.

7. **Epic E** (Dashboard) is deferred until Epics 0-D are stable. No timeline set.

## File Reference Index

| Document | File | Lines | Purpose |
|----------|------|-------|---------|
| worktree-architecture | `codeflow-cli/internal/session/session.go` | 225-241, 301-321 | Current(), WriteEnvFile() |
| worktree-architecture | `codeflow-cli/internal/hooks/session/start.go` | 830-850 | createProjectTempDir() |
| worktree-architecture | `codeflow-cli/internal/workstate/activetask.go` | 39-63, 67-84 | SetActiveTask(), GetActiveTask() |
| worktree-architecture | `codeflow-cli/internal/hooks/session/start.go` | 334-387 | handleStaleCleanup() |
| worktree-architecture | `codeflow-cli/internal/worktree/worktree.go` | 62-79, 110-191 | Manager, Setup() |
| worktree-architecture | `.gitignore` | 67 | `.git-worktrees/` (correct; code default at `worktree.go:102` must change from `.claude/worktrees` to match) |
| worktree-architecture | `codeflow-cli/internal/hooks/session/start.go` | 189-305 | StartInit() full flow |
| worktree-architecture | `codeflow-cli/internal/worktree/worktree.go` | 114-116, 126 | Branch requirement, git worktree add |
| worktree-architecture | `codeflow-cli/internal/hooks/session/end.go` | 75-132 | EndCleanup() |
| data-layer-protection | `codeflow-cli/internal/db/connection.go` | 85-115, 118-163 | newDB(), Query(), Execute() |
| data-layer-protection | `codeflow-cli/internal/ledger/writer.go` | 74-119 | appendToFile() with flock |
| data-layer-protection | `codeflow-cli/internal/claim/claim.go` | 85-133, 148-221 | loadDoc(), saveDoc(), Acquire() |
| data-layer-protection | `codeflow-cli/cmd/codeflow/db.go` | 220-243, 266-287 | runDBQuery(), runDBExec() |
| data-layer-protection | `.codeflow/config/enforcement/enforcement-policy.json` | 6-17 | blocked_directories (missing .state/) |
| data-layer-protection | `codeflow-rs/` (proposed, Epic 0) | New | Pure Rust CLI workspace (replaces Go CLI after cutover) |
| schema-standardization | `codeflow-cli/internal/hooks/logging/prompt.go` | 18-53 | LogPrompt() with prompt_hash/length/type |
| schema-standardization | `codeflow-cli/internal/ledger/schema.go` | 10-50, 55+ | requiredFields (10-50), ValidateEvent() (55+) |
| autorun-integration | `codeflow-cli/internal/autorun/worker.go` | 108-116 | TmuxWorker.Run() with WorkDir: "." |
| crdt-coordination | `codeflow-cli/internal/db/crdt.go` | (full file) | RebuildCRDT() |
| crdt-coordination | `codeflow-cli/internal/claim/claim.go` | 148-221 | Acquire() with conflict detection |
| worktree-architecture | `codeflow-cli/internal/hooks/session/start.go` | 640-671, 674-684 | createPathFlowFlag(), initCheckpoint() |

## Status

- **Analysis:** Complete (9 documents, 3800+ lines across package, 80+ tasks across 6 epics)
- **Review:** Pending WS-REV re-review (global DB architecture + model orchestration + revised epic structure added post-initial review)
- **Epic creation:** Pending (route through cf-knowledge-layer after review approval)
