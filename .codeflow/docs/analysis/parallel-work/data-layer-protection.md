---
title: "Data Layer Protection"
type: analysis
status: active
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-21"
parent: "parallel-work/README.md"
---

# Data Layer Protection

[← Back to Overview](README.md)

## Table of Contents

- [1. SQLite (Tier 1)](#1-sqlite-tier-1)
- [2. JSONL Ledger (Tier 0)](#2-jsonl-ledger-tier-0)
- [3. Claims System (Coordination Layer)](#3-claims-system-coordination-layer)
- [4. Data Layer CLI-Only Enforcement](#4-data-layer-cli-only-enforcement)
- [5. SurrealDB Evaluation](#5-surrealdb-evaluation)
- [6. Pure Rust CLI Architecture](#6-pure-rust-cli-architecture)
- [7. Global Database Architecture](#7-global-database-architecture)
- [8. Coordination Events Ledger](#8-coordination-events-ledger)
- [9. Scope Policy as Data Protection](#9-scope-policy-as-data-protection)

---

## 1. SQLite (Tier 1)

**Configuration** (from `connection.go:85-115`):
- WAL mode (`PRAGMA journal_mode=WAL`, line 96)
- Foreign keys enabled (`PRAGMA foreign_keys=ON`, line 97)
- Busy timeout 5000ms (`PRAGMA busy_timeout=5000`, line 98)
- Synchronous NORMAL (`PRAGMA synchronous=NORMAL`, line 99)
- Max open connections: 1 (`SetMaxOpenConns(1)`, line 92)

**Retry logic** (from `connection.go:118-163`):
- `Query()` retries 3 times on SQLITE_BUSY with 100ms delay (lines 118-136)
- `Execute()` retries 3 times on SQLITE_BUSY with 100ms delay (lines 145-163)
- `Transaction()` wraps operations in BEGIN/COMMIT with rollback on error (lines 167-189)

**Parallel safety assessment:**
- WAL mode enables concurrent readers with a single writer -- this is SQLite's strongest concurrency mode
- `busy_timeout=5000` means writes will wait up to 5 seconds for the write lock
- The singleton pattern (`Get()` at `connection.go:56-68`) means all operations within a single process share one connection
- Across worktrees (separate processes), each process opens its own connection to the same database file (symlinked)
- Write serialization is handled by SQLite's WAL write lock -- not by Go code

**Risk:** Under heavy parallel write load, `busy_timeout=5000` may be insufficient. Two sessions writing to different tables (e.g., Session A writing sessions, Session B writing tasks) are still serialized at the WAL write lock level.

**Recommendation:** Increase `busy_timeout` to 15000 for parallel mode. Add jitter to retry delay (randomized 50-200ms instead of fixed 100ms) to reduce thundering herd.

---

## 2. JSONL Ledger (Tier 0)

**Locking mechanism** (from `writer.go:74-119`):
- Sidecar `.lock` file created per JSONL file (line 89)
- Exclusive `flock` acquired before write (line 99: `syscall.LOCK_EX`)
- Data file opened `O_APPEND` for atomic append (line 107)
- Single `Write()` call for the entire JSON line (line 114)
- Lock released on function return via defer (line 102-104)

**Parallel safety assessment:** Strong. `flock` is process-safe on macOS/Linux. `O_APPEND` ensures atomic seek-to-EOF. The combination provides robust concurrent append safety. No changes needed.

---

## 3. Claims System (Coordination Layer)

**State file:** `.state/coordination/state.loro` (Loro CRDT document, managed by `LoroCoordinator` in the Rust CLI)

**Status: RESOLVED by Loro CRDT (INF-EPC-023 Phase A).**

The previous Go implementation used `state.json` with a TOCTOU race: two sessions calling `Acquire()` concurrently could both read the same state, both add their claim, and one write would overwrite the other. This is eliminated by the Rust CLI's `LoroCoordinator`.

**Current implementation:**
- `LoroCoordinator` implements the `Coordinator` trait from Epic 0
- `Coordinator::acquire(path, session_id)` — atomic claim via Loro Map CRDT, no TOCTOU race
- `claims::acquire_batch(coordinator, paths, session_id)` — pre-claim all `file_scope` entries at startup
- `claims::release_all(session_id)` — release on session end (also triggered by SessionEnd hook)
- Fencing tokens are monotonically increasing `u64` values stored in the Loro Map
- Same-machine parallel sessions share `state.loro` via symlink — immediate conflict detection
- Multi-machine sessions sync via per-peer git refs (5s default interval)

**Coordination events** are now recorded in `coordination-events.jsonl` and the `coordination_event` SurrealDB table. See [Section 8: Coordination Events Ledger](#8-coordination-events-ledger) for the schema.

---

## 4. Data Layer CLI-Only Enforcement

The data layer (SQLite + JSONL) currently has insufficient access control. This is a general issue that becomes critical under parallel execution, where one session's destructive operation can affect all concurrent sessions.

### 4.1 Current Enforcement Mechanisms

**For JSONL:**

| Mechanism | Protection Level | Gap |
|-----------|-----------------|-----|
| `codeflow ledger append` | Schema-validated via `ValidateEvent()` in `schema.go` -- checks event type exists, required fields present | Properly gated |
| edit-write-guard | `blocked_directories` in `enforcement-policy.json` does NOT include `.state/ledger/` | Agents CAN bypass validation via direct Edit/Write to JSONL files |
| PreToolUse hooks | No hook blocks `echo >> .state/ledger/*.jsonl` | Agents CAN bypass validation via Bash echo |

**For SQLite:**

| Mechanism | Protection Level | Gap |
|-----------|-----------------|-----|
| `codeflow db exec` (`db.go:266-287`) | Zero validation -- accepts ANY SQL | Agent can `DROP TABLE tasks` |
| `codeflow db query` (`db.go:220-243`) | Weak prefix check -- blocks `INSERT`, `UPDATE`, `DELETE`, `DROP`, `ALTER`, `CREATE` | Bypassable via CTE: `WITH x AS (DELETE FROM tasks) SELECT 1` |
| Named operations | `codeflow workgraph create-epic/create-task/update-epic/update-task/query`, `codeflow state active-task get/set/clear`, `codeflow state memory record` | Properly gated, but agents not required to use them |
| PreToolUse hooks | No hook blocks `sqlite3 .state/db/codeflow.db` | Full unrestricted shell access |
| edit-write-guard | `blocked_directories` does NOT include `.state/db/` | Agents CAN Edit/Write to the .db file directly |

### 4.2 Required Enforcement Changes

1. **Add `.state/ledger/` and `.state/db/` to `blocked_directories`** in `enforcement-policy.json`. This closes the Edit/Write bypass path for both JSONL and SQLite.

2. **Add PreToolUse hook blocking `sqlite3` shell access.** Pattern: reject `Bash` tool calls containing `sqlite3` targeting `.state/db/`. This closes the direct shell bypass path.

3. **Add PreToolUse hook blocking `codeflow db exec`** in Bash calls. This forces agents to use named operations.

4. **Add PreToolUse hook blocking direct JSONL writes.** Pattern: reject `Bash` tool calls containing `>> .state/ledger/` or `echo` + `.jsonl`. This forces agents to use `codeflow ledger append`.

5. **Remove `codeflow db exec` raw SQL interface.** Replace with named operations only. The CLI binary is the trusted gatekeeper for SQLite.

6. **Strengthen `codeflow db query` SQL parsing.** Not just prefix matching -- parse the SQL statement to reject any write operations including CTEs with side effects.

7. **Update cf-knowledge-layer agent definition.** Remove all raw SQL from SOPs. Replace with CLI-only commands.

8. **Move `git/pr-events` to CLI writer.** Currently written by LLM agent via bash echo AND CLI hooks, creating schema inconsistency. Move all writes to the Rust activity writer (after Epic 0) or Go `ActivityWriter` (before cutover).

---

## 5. SurrealDB Evaluation

### 5.1 Overview

SurrealDB is a multi-model database offering document, graph, vector (HNSW), full-text search, relational, and time-series capabilities in a single system with a unified query language (SurrealQL).

- **Version:** SurrealDB 3.0 GA (February 2026)
- **Funding:** $44M total (Series A $35M in 2023)
- **Named users:** Tencent, Later.com
- **Maturity:** 3 years vs SQLite's 24+ years. v2.x had stability issues. v3 claims "most stable release."

### 5.2 Capabilities Assessment

| Capability | SurrealDB | SQLite | Relevance to CodeFlow |
|-----------|-----------|--------|----------------------|
| Document store | Native | JSON via json_extract() | Medium -- task/epic records are document-like |
| Graph queries | Native (RELATE, graph traversal) | Not supported | HIGH -- task dependency graphs, epic-task-session relationships |
| Vector search (HNSW) | Native (DEFINE INDEX ... HNSW) | Not supported | HIGH -- semantic search over memory, code summaries, agent context |
| Full-text search | Native (DEFINE INDEX ... SEARCH) | FTS5 extension | Medium -- already possible with SQLite FTS5 |
| Time-series | Native (GROUP BY time intervals) | Manual with date functions | Medium -- session metrics, performance trending |
| Relational | SurrealQL JOIN equivalent | Full SQL | High -- current schema is relational (37 tables) |
| ACID transactions | Yes, lockless reads | Yes, WAL mode | Parity |
| Concurrency | Multi-writer, lockless reads | Single writer via WAL | SurrealDB better for parallel writes |

### 5.3 Unified Query Advantage for Agentic Use Cases

> **Note:** SurrealDB embedded replaces SQLite entirely in the Rust CLI (see [Decision #17](decisions.md#17-surrealdb-as-single-database-platform)). References to SQLite `busy_timeout` and WAL mode in this document reflect the legacy/transitional Go implementation. The Rust CLI uses SurrealDB's `RetryConfig` with exponential backoff for parallel access, not SQLite busy_timeout.

SurrealDB's key advantage is combining multiple query types in a single statement:

```surql
-- Find tasks similar to a description, traverse their epic graph,
-- and return related sessions — all in one query
SELECT *,
  ->belongs_to->epic AS epic,
  ->belongs_to->epic->has_session->session AS sessions
FROM task
WHERE description <|10|> $embedding
AND status = 'in_progress'
ORDER BY score DESC
LIMIT 5;
```

With SQLite, this requires:
1. A separate vector database (or embedding library)
2. Multiple queries to traverse relationships
3. Application-level join logic

For agentic use cases where agents need to find contextually relevant work, semantic search combined with graph traversal in a single query is a significant capability advantage.

### 5.4 Go Embedded Mode Blocker (Resolved by Epic 0)

**Finding: Go embedded mode does NOT exist.**

The SurrealDB Go SDK (`surrealdb.go`) returns `"embedded database not enabled"` for `mem://` and `surrealkv://` connection strings. The SDK only supports WebSocket (`ws://`) and HTTP (`http://`) connections.

True embedded mode exists in:
- **Rust** (native, full embedded support)
- **JavaScript/TypeScript** (via WASM, in-browser)
- **Python** (via PyO3 FFI)
- **WASM** (web assembly)

Using SurrealDB from Go would require a **sidecar server process** (~50MB binary, process lifecycle management, network socket communication, additional failure modes).

**This blocker is resolved by Epic 0 (Rust CLI redesign).** By replacing the Go CLI with a pure Rust implementation, SurrealDB embedded (`surrealkv://`) becomes a direct crate dependency — `use surrealdb::Surreal;`. No sidecar, no server process, no network socket. The blocker is architectural, not a SurrealDB limitation. See [Decision #17](decisions.md#17-surrealdb-as-single-database-platform) and Section 6.

### 5.5 Performance Characteristics

| Metric | SurrealDB v3 | SQLite | Notes |
|--------|-------------|--------|-------|
| Simple queries | ~150ms (community benchmark) | ~10ms | 15x slower for basic CRUD |
| Graph traversal (3 hops) | 8-22x faster than v2 | N/A (not supported) | No head-to-head SQLite comparison |
| Write throughput | Multi-writer, lockless | Single writer via WAL | SurrealDB better for parallel writes |
| Cold start | ~2-3s (sidecar startup) | ~1ms (file open) | Significant startup penalty |
| Memory footprint | ~200MB baseline | ~10MB | 20x higher |

### 5.6 Migration Effort

| Factor | Assessment |
|--------|-----------|
| Tables to convert | 37 tables (current schema) |
| SQL to SurrealQL | Every query must be rewritten |
| Schema migration tool | None exists (SQLite to SurrealDB) |
| ORM/driver maturity | Go SDK v1.x, less battle-tested than database/sql |
| Testing infrastructure | All test helpers assume SQLite |
| Rollback risk | High -- SurrealDB is the only copy of data |

### 5.7 Options Analysis

> **Decision reached:** Option E is implemented as Epic 0. SQLite is being **replaced entirely** by SurrealDB — not supplemented. There is no hybrid mode. See [Decision #17](decisions.md#17-surrealdb-as-single-database-platform).

| Option | Description | Pros | Cons | Status |
|--------|-------------|------|------|--------|
| A. Full replacement (SurrealDB sidecar) | Replace SQLite with SurrealDB as sidecar server (~50MB process) | Unified multi-model queries. Better parallel writes. | BLOCKED by Go embedded gap. 37 tables to migrate. Sidecar process management. Cold start ~2-3s. | Superseded by E |
| B. Hybrid (SQLite + SurrealDB sidecar) | Keep SQLite for operational data. Add SurrealDB sidecar for agent memory/search. | Best of both worlds incrementally. | Two databases + sidecar process. Data sync. Double infrastructure. | Rejected — SurrealDB-only is the decision |
| C. Track and wait | Monitor Go embedded progress. Plan migration when available. | Zero risk now. | Misses capabilities now. Unknown timeline. | Rejected |
| D. Rust shared library | Implement data layer in Rust. Expose to Go via C-compatible FFI. | True embedded SurrealDB. Native Loro. | CGo FFI overhead. Two-language codebase. | Superseded by E |
| **E. Pure Rust CLI (DECIDED)** | **Replace Go CLI with Rust. SurrealDB embedded (`surrealkv://`) and Loro are direct crate dependencies. `SurrealStore` is the ONLY `DataStore` impl — no `SqliteStore`.** | **Single language. No FFI. No sidecar. One build pipeline. SQLite fully retired.** | **Large redesign scope (Epic 0, 25 tasks).** | **DECIDED — Epic 0** |

---

## 6. Pure Rust CLI Architecture

The Rust CLI is an idiomatic redesign, not a line-for-line port of the Go CLI. Data access is abstracted behind traits, enabling swappable backends and comprehensive testing.

> **Updated:** `SurrealStore` is the ONLY `DataStore` implementation. `SqliteStore` is not present in the Rust redesign — SurrealDB embedded (`surrealkv://`) replaces SQLite from the start of Epic 0. The `DataStore` trait is retained for testability (mock implementations) and future extensibility (global daemon mode via `ws+unix://`).

```text
Rust CLI binary (codeflow) — modular crate structure
  codeflow-core (library crate):
  ├── DataStore trait ─────────── SurrealStore (embedded surrealkv:// for project-local;
  │                               ws+unix:// for global daemon — Epic C)
  │                               [SqliteStore REMOVED — SurrealDB is the sole Tier 1 store]
  ├── Coordinator trait ───────┬── LoroCoordinator (native loro crate — Epic A)
  │                            └── (extensible)
  ├── Transport trait ─────────┬── FileTransport (local file exchange)
  │                            └── GitRefTransport (git2 crate — Epic A)
  ├── LedgerWriter trait ──────┬── JsonlWriter (flock + serde append)
  │                            └── (extensible)
  ├── Domain error enums ──────── SessionError, DbError, HookError, LedgerError (thiserror)
  └── Domain type enums ───────── SessionState, PathFlowPhase, WorkStage, LedgerEvent (serde)

  codeflow-cli (binary crate):
  └── Clap CLI dispatch ──────── Thin layer routing subcommands to codeflow-core
```

### 6.1 Why Option E (Pure Rust CLI) Solves All Gaps

1. **SurrealDB embedded works natively in Rust, and replaces SQLite entirely.** The `surrealdb` crate supports `mem://` (in-memory) and `surrealkv://` (key-value store, persistent) embedded modes — no server process, no network sockets, no sidecar. This eliminates the Go SDK gap entirely. `SurrealStore` is the sole `DataStore` implementation: `surrealkv://` at `.state/db/codeflow/` for project-local operations; `ws+unix://~/.codeflow/db.sock` for optional global daemon access (Epic C). SQLite and `SqliteStore` are not part of the Rust CLI at all.

2. **Loro CRDT is Rust-native.** The `loro` crate is the primary implementation (not a binding). Direct API calls (`use loro::LoroDoc;`), no community-maintained wrappers, no UniFFI indirection. Map, List, Text (Fugue), Tree CRDTs all available natively. `LoroCoordinator` implements the `Coordinator` trait.

3. **Single binary, modular crates.** One `codeflow` binary contains everything: CLI commands, hook handlers, database layer, CRDT coordination, and sync daemon. Internally, the `codeflow-core` library crate separates testable logic from the `codeflow-cli` binary crate's command dispatch. No FFI boundary, no shared library, no CGo complexity.

4. **Data layer protection enforced through traits and types.** All data access flows through the `DataStore` trait's typed API. Domain error enums (`DbError`, `SessionError`, etc.) with `thiserror` provide exhaustive error handling at compile time. The type system encodes states that Go represents as strings -- session states, phase transitions, and event types are all enums with compile-time validity checks.

5. **Eliminates two-language complexity.** No CGo FFI overhead (~100-200ns/call), no cross-compilation for shared libraries, no two-language debugging, no FFI error marshaling. One language, one build pipeline, one test framework. DRY violations identified in Phase 0A (codebase analysis) are consolidated in the redesign.

### 6.2 Build Pipeline

| Factor | Assessment |
|--------|-----------|
| Platforms | darwin-arm64, darwin-amd64, linux-amd64 (3 targets) |
| CI | `cargo build --release --target {triple}`. GitHub Actions has Rust toolchain support. |
| Developer experience | Standard Rust toolchain (`rustup`). No shared library management. Single `cargo build`. |
| FFI overhead | **None.** All components are native Rust crate dependencies within the same binary. |
| Binary size | SurrealDB embedded + Loro + Rust runtime approximately 30-50MB. Comparable to the current Go CLI (~25MB) plus what was the shared library (~20-40MB). |
| Testing | Rust unit tests and integration tests. Single language, single test framework, single CI pipeline. |

### 6.3 Recommendation

**Option E (pure Rust CLI) is the recommended approach.**

This option solves the SurrealDB Go embedded gap (Section 5.4), the Loro integration concern, AND the CGo FFI complexity in one architectural decision. Epic 0 is an idiomatic redesign of the Go CLI in Rust -- not a line-for-line port. The redesign introduces trait-based abstractions (`DataStore`, `Coordinator`, `Transport`, `LedgerWriter`), domain-specific error types, and type system leverage (enums for states, tagged unions for events) that provide compile-time correctness guarantees Go cannot express. All subsequent work (Loro CRDT, SurrealDB embedded, sync daemon) implements these traits as native Rust crate dependencies with zero FFI overhead.

**Why not Option A (Line-for-line port)?** A mechanical translation carries over Go-specific workarounds, DRY violations, and string-typed states. Rust's type system enables correctness guarantees (exhaustive pattern matching, compile-time state validation, typed errors) that a transliteration would miss. The external interface is frozen; the internal design should be optimal for the target language.

**Why not Option D (Rust shared library)?** Option E eliminates the FFI boundary entirely. No CGo, no shared library cross-compilation, no two-language debugging, no FFI error marshaling. Option D was the previous recommendation but is superseded by the decision to fully migrate to Rust.

**Why not Option B (Hybrid sidecar)?** Option E eliminates the sidecar entirely. No process management, no health checks, no port conflicts, no cold start penalty. SurrealDB operates in-process as a direct crate dependency.

**Why not Option C (Track and wait)?** The Go embedded SDK may never be prioritized. Migrating to Rust makes the question moot -- SurrealDB embedded is a native Rust feature.

**Migration strategy:** Epic 0 implements the Rust CLI redesign (see [Decision #16](decisions.md#16-rust-cli-migration-strategy)). Go and Rust binaries coexist during migration. A contract conformance test suite verifies identical external behavior before cutover. The external interface is FROZEN: same binary name, subcommands, hook JSON contracts, and exit codes.

---

## 7. Global Database Architecture

> This section is a summary. The full design is in [global-intelligence.md](global-intelligence.md) (created as part of PLN-TSK-001-006).

The global database layer is an optional extension of the project-local SurrealDB embedded instance. It enables cross-project queries, team-wide memory search, and aggregated task visibility across multiple repositories.

### 7.1 Architecture Overview

```text
Project A (.state/db/codeflow/ — surrealkv://)
    |
    +--[codeflow global enable]--> Unix socket daemon (~/.codeflow/db.sock)
    |                                  |
Project B (.state/db/codeflow/)        +-- Aggregates: tasks, epics, memory
    |                                  |   sessions, embeddings
    +--[registered]-------------------> |
                                        |
Project C (.state/db/codeflow/)         |
    |                                   |
    +--[registered]-------------------> |
```

### 7.2 Operating Modes

| Mode | Description | When to Use |
|------|-------------|-------------|
| Project-only (default) | SurrealDB embedded at `.state/db/codeflow/`. No daemon. All features work. | Single developer, single repo. |
| Global-enabled | Embedded for local ops + daemon at `~/.codeflow/db.sock` for cross-project queries. | Multi-repo team or personal cross-project visibility. |
| Global-only (future) | All state via daemon, no per-project embedded DB. | Large org with centralized coordination. |

### 7.3 Daemon Lifecycle

- **Binary:** Daemon runs inside the `codeflow` Rust binary. `codeflow db daemon start` launches it.
- **Socket path:** `~/.codeflow/db.sock` — Unix domain socket, local only, no TCP exposure.
- **Service integration:** `codeflow db install-service` registers a launchd plist (macOS) or systemd unit (Linux). `auto_start = true` in global config triggers auto-start on first use.
- **Graceful degradation:** If the daemon is unavailable, project-local operations continue unaffected via embedded mode. Cross-project features return a `DaemonUnavailable` error with a clear message.

### 7.4 Project Registration and Sync

- `codeflow project register --name "{name}" --tags "{tags}"` — registers the current repo with the global daemon, assigns `project_id`.
- `codeflow project link {project_id} --scopes memory,tasks` — establishes a visibility relation in SurrealDB.
- Sync between project-local embedded DB and global daemon uses SurrealDB's live query and change feed capabilities, not a custom sync protocol.

### 7.5 Local Embeddings

Cross-project semantic search requires vector embeddings. These are generated locally:

- **Model:** `all-MiniLM-L6-v2` (384 dimensions, ~50MB)
- **Runtime:** `ort` crate (ONNX Runtime for Rust) — works offline, no API key required
- **When:** Embeddings are generated on write when memory events sync to SurrealDB
- **Storage:** SurrealDB HNSW vector index on memory and task tables
- **Query:** `description <|10|> $embedding` syntax in SurrealQL for approximate nearest-neighbor search

### 7.6 Three-Tier Model (Updated)

| Tier | Location | Purpose | Git Tracked | Changed? |
|------|----------|---------|-------------|----------|
| 0 (JSONL) | `.state/ledger/*.jsonl` | Rebuild authority — immutable, append-only | Yes | No |
| 1 (SurrealDB embedded) | `.state/db/codeflow/` | Query interface for project-local ops | No | **SQLite replaced** |
| 1+ (SurrealDB daemon) | `~/.codeflow/db.sock` | Optional global cross-project layer | No | **New** |
| 2 (Markdown) | `project-management/`, `.claude/memory/` | Human-readable derived views | Yes | No |

JSONL (Tier 0) remains the rebuild authority — unchanged. If the SurrealDB embedded store is lost or corrupted, it is rebuilt from JSONL. The rebuild path (`codeflow db rebuild`) uses the same event-sourcing logic as the current SQLite rebuild path.

---

## 8. Coordination Events Ledger

All coordination events are written to two destinations:

1. **`coordination-events.jsonl`** — append-only ledger at `.state/ledger/coordination-events.jsonl`. Written by `LoroCoordinator` on every claim operation. Protected by `flock` for I/O append safety.
2. **`coordination_event` SurrealDB table** — queryable view for operational analysis, reporting, and debugging.

### 8.1 SurrealDB Schema

```surql
DEFINE TABLE coordination_event SCHEMAFULL;

DEFINE FIELD event_type    ON coordination_event TYPE string;
DEFINE FIELD session_id    ON coordination_event TYPE string;
DEFINE FIELD task_id       ON coordination_event TYPE option<string>;
DEFINE FIELD worktree_id   ON coordination_event TYPE option<string>;
DEFINE FIELD file_path     ON coordination_event TYPE option<string>;
DEFINE FIELD token         ON coordination_event TYPE option<int>;
DEFINE FIELD scope_policy  ON coordination_event TYPE option<string>;
DEFINE FIELD was_expansion ON coordination_event TYPE option<bool>;
DEFINE FIELD holding_session ON coordination_event TYPE option<string>;
DEFINE FIELD holding_task  ON coordination_event TYPE option<string>;
DEFINE FIELD file_count    ON coordination_event TYPE option<int>;
DEFINE FIELD release_type  ON coordination_event TYPE option<string>;
DEFINE FIELD original_scope ON coordination_event TYPE option<array>;
DEFINE FIELD attempt_number ON coordination_event TYPE option<int>;
DEFINE FIELD max_attempts  ON coordination_event TYPE option<int>;
DEFINE FIELD occurred_at   ON coordination_event TYPE datetime;
```

### 8.2 Event Types

| Event | Written When | Key Fields |
|-------|-------------|------------|
| `ClaimAcquired` | Successful claim acquisition | file_path, token, was_expansion |
| `ClaimConflict` | Blocked by another holder | file_path, holding_session, holding_task, scope_policy |
| `ClaimReleased` | Claims released on session end | file_count, release_type (normal/crash/timeout) |
| `ScopeExpansion` | Soft mode allowed out-of-scope claim | file_path, original_scope, token |
| `MergeConflictDetected` | Git merge conflict detected pre-PR | file_path |
| `MergeRebaseAttempted` | Rebase retry initiated | attempt_number, max_attempts |

### 8.3 Example Query

Find all claim conflicts for a specific session:

```surql
SELECT file_path, holding_session, holding_task, occurred_at
FROM coordination_event
WHERE event_type = 'ClaimConflict'
  AND session_id = 'ses-01kjxabc123'
ORDER BY occurred_at DESC;
```

---

## 9. Scope Policy as Data Protection

`scope_policy` enforces file-level access control in parallel worker scenarios. Where Sections 1-4 address protection of the data layer (SQLite, JSONL, CRDT state) from agent misuse, `scope_policy` protects source files and project artifacts from concurrent write conflicts between parallel workers.

### 9.1 The Concurrent Edit Problem

In parallel autorun mode, multiple workers execute independently in separate git worktrees. Without coordination, two workers can both attempt to modify the same file:

```text
Worker A (task: add caching)          Worker B (task: fix latency)
    |                                     |
    v                                     v
Edit: coordination/claims.rs         Edit: coordination/claims.rs
    |                                     |
    v                                     v
Both edits succeed locally.
    |
    v
PR A merged first.
PR B's edit is based on stale version — overwrites or conflicts.
```

This is a data integrity problem: the merged state may not represent either worker's intended change, or one worker's changes may be silently lost.

### 9.2 How scope_policy Prevents This

`scope_policy` drives CRDT claim enforcement in the PreToolUse hook (`pre_tool_use.rs`). Claims are stored in the Loro CRDT (`state.loro`) and shared across all workers in the same repository. Before a worker can edit a file, the claim system checks whether another worker already holds a claim on it.

**Claim enforcement modes:**

| Policy | Mechanism | Data Protection Guarantee |
|--------|-----------|--------------------------|
| `soft` (default) | `Coordinator::acquire()` for out-of-scope files; pre-claim for in-scope via `acquire_batch()` | Concurrent edits to the same file are blocked with conflict details. Scope expansion to unclaimed files is logged for audit. |
| `hard` | Out-of-scope edits blocked without claim attempt | Strict territory enforcement — no worker can access files outside its declared scope. Prevents unintended cross-cutting changes. |
| `permissive` | No enforcement | Unrestricted. Only valid for interactive sessions where a human is present to resolve conflicts manually. Forbidden for autorun. |

### 9.3 Protection Layers for Source Files

| Layer | What It Protects | Mechanism |
|-------|-----------------|-----------|
| CRDT claims (`scope_policy`) | Source files and project artifacts from concurrent edits | PreToolUse hook blocks conflicting edits; Loro CRDT detects conflicts atomically |
| edit-write-guard hook | Protected directories (`.state/ledger/`, `.state/db/`) | Blocks Edit/Write tool calls to protected paths |
| protection-guard hook | CRITICAL/HIGH/MODERATE tier files (CLAUDE.md, settings.json, hooks) | Enforces staged-edit workflow for critical resources |
| Worktree isolation | File system-level separation | Each worker operates in its own git worktree; no shared working directory |

`scope_policy` is the first layer — it prevents conflicts before they occur by ensuring only one worker can claim a file at a time. The other layers protect infrastructure files from agent misuse regardless of parallelism.

### 9.4 Claim Struct Extension (INF-TSK-023-027)

The `Claim` struct in `codeflow-cli/core/src/coordination/mod.rs` was extended with `task_id: String` and `worktree_id: Option<WorktreeId>` fields. These fields serve two data protection purposes:

1. **Auditability:** `task_id` links every claim to the work item that holds it. When a conflict occurs, the error message includes which task holds the file, enabling the blocked worker to coordinate scope changes.
2. **Crash recovery:** `worktree_id` enables the sync daemon to identify which worktree holds a claim during dead worker detection. If the worktree's process is no longer alive, the claim can be force-released, preventing permanent lockout.

### 9.5 Coordination Events as Audit Log

All claim operations are recorded in `coordination-events.jsonl` at `.state/ledger/coordination-events.jsonl`. This file is git-tracked, providing an immutable audit log of all file access coordination events:

| Event | Data Protection Value |
|-------|----------------------|
| `ClaimAcquired` | Records which session claimed which file, when, and whether it was an in-scope or expansion claim |
| `ClaimConflict` | Records every blocked access attempt — enables post-incident analysis of coordination failures |
| `ScopeExpansion` | Records every out-of-scope access that succeeded — visible deviations from declared task scope |
| `ClaimReleased` | Records scope release at session end — confirms no claims are held beyond their intended lifetime |
| `MergeConflictDetected` | Records when a git merge conflict is found pre-PR — provides traceability from git conflict to original claim coordination |

The `coordination_event` SurrealDB table provides a queryable view of the same data for operational analysis (see [Section 8](#8-coordination-events-ledger) for schema).

---

[← Back to Overview](README.md)
