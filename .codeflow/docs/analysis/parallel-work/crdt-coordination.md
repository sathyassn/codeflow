---
title: "CRDT Coordination"
type: analysis
status: draft
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-05"
parent: "parallel-work/README.md"
---

# CRDT Coordination

[← Back to Overview](README.md)

## Table of Contents

- [1. Current State in Codebase](#1-current-state-in-codebase)
- [2. V4 Specification Vision](#2-v4-specification-vision)
- [3. Loro CRDT Capabilities](#3-loro-crdt-capabilities)
- [4. Integration Path: Pure Rust CLI](#4-integration-path-pure-rust-cli)
- [5. Integration Plan (Phase A)](#5-integration-plan-phase-a)
- [6. How Loro CRDT Coordination Works](#6-how-loro-crdt-coordination-works)
- [7. Multi-Machine Sync Mechanism](#7-multi-machine-sync-mechanism)
- [8. Multi-Machine Merge Flow](#8-multi-machine-merge-flow)

---

## 1. Current State in Codebase

| Component | Location | Status | Purpose |
|-----------|----------|--------|---------|
| `claim.go` | `internal/claim/claim.go` (619 lines) | Active, TOCTOU race | File claims with pattern-based conflict detection, fencing tokens, TTL expiry |
| `crdt.go` | `internal/db/crdt.go` | Active | `RebuildCRDT()` replays `claim_created`/`claim_released`/`claim_renewed` events from JSONL |
| `state.json` | `.state/coordination/state.json` (653 bytes) | Active | Plain JSON with claims map and token counter -- the actual coordination state |
| `state.loro` | `.state/coordination/state.loro` (483 bytes) | **UNUSED ARTIFACT** | On disk but never read or written by any Go code |

---

## 2. V4 Specification Vision

The V4 specification envisions:
- Loro binary format (`state.loro`) as the coordination state
- Python scripts for CRDT rebuild (pre-Go CLI era design)
- Git-synced JSONL events enabling multi-machine merge
- Fencing tokens validated by hooks

---

## 3. Loro CRDT Capabilities

Loro provides conflict-free replicated data types with these features:

| CRDT Type | Use Case in CodeFlow | Merge Semantics |
|-----------|---------------------|----------------|
| Map | Claims state (`state.loro` replacing `state.json`) | Last-writer-wins per key, concurrent key additions merge |
| List | Ordered task queues, merge queues | Position-based insertion, concurrent inserts at different positions merge |
| Text (Fugue) | Future use: structured config or spec files requiring concurrent edits | Character-level merge, concurrent edits in different regions merge cleanly |
| Tree | Epic-task hierarchy, dependency graphs | Move operations with cycle detection |

**Key properties:**
- Delta-based op-log sync (not full-state transfer)
- Deterministic merge (same ops in any order produce same result)
- Offline-capable (edits accumulate locally, merge on sync)
- Compressed binary format (smaller than JSON equivalent)

---

## 4. Integration Path: Pure Rust CLI

**Approach:** Loro as a direct crate dependency in the pure Rust CLI. No FFI, no shared library, no CGo wrappers.

With the Rust CLI redesign (Epic 0), the Go CLI is replaced by a single `codeflow` Rust binary. Loro is a direct `use loro::LoroDoc;` import -- native Rust, zero binding indirection. The redesign introduces a `Coordinator` trait that `LoroCoordinator` implements, enabling mock-based testing and future extensibility. This supersedes the earlier Rust shared library + CGo FFI approach entirely.

**Architecture (modular crate structure with trait-based abstractions):**

```text
codeflow-rs/                          (Rust workspace — replaces Go CLI)
├── codeflow-core/                    (library crate — core logic, independently testable)
│   ├── src/
│   │   ├── lib.rs                    (public API surface)
│   │   ├── types/                    (domain types — enums, tagged unions)
│   │   │   ├── session.rs            (SessionState enum, phase/stage enums)
│   │   │   ├── events.rs             (LedgerEvent tagged union via serde)
│   │   │   └── sentinel.rs           (Sentinel enum with Display impl)
│   │   ├── traits/                   (trait definitions — swappable implementations)
│   │   │   ├── datastore.rs          (DataStore trait: CRUD for sessions, tasks, epics)
│   │   │   ├── coordinator.rs        (Coordinator trait: CRDT claim/merge/sync)
│   │   │   ├── transport.rs          (Transport trait: delta exchange mechanism)
│   │   │   └── ledger.rs             (LedgerWriter trait: append-only event logging)
│   │   ├── store/                    (DataStore implementations)
│   │   │   └── surreal.rs            (SurrealStore — sole DataStore impl, surrealkv:// embedded)
│   │   ├── coordination/             (Coordinator implementations)
│   │   │   ├── loro.rs               (LoroCoordinator — native loro crate)
│   │   │   │   ├── LoroDoc           (document container)
│   │   │   │   ├── LoroMap           (claims state, key-value coordination)
│   │   │   │   ├── LoroText          (future: concurrent config edits via Fugue)
│   │   │   │   └── LoroTree          (work hierarchy, dependency graphs)
│   │   │   └── claims.rs             (state.loro management, fencing tokens)
│   │   ├── transport/                (Transport implementations)
│   │   │   ├── file.rs               (FileTransport — local file exchange)
│   │   │   └── gitref.rs             (GitRefTransport — git2 ref-based sync)
│   │   ├── hooks/                    (HookHandler trait + implementations)
│   │   │   ├── mod.rs                (HookHandler trait, HookInput/Output types)
│   │   │   ├── session.rs            (session-start, session-end handlers)
│   │   │   ├── tooluse.rs            (pre/post tool-use gates, sentinels, checkpoints)
│   │   │   └── logging.rs            (activity writer, prompt/tool-use logging)
│   │   ├── session/                  (session management, state machine)
│   │   ├── workgraph/                (task/epic CRUD, status transitions)
│   │   ├── worktree/                 (git2-based worktree manager)
│   │   ├── autorun/                  (worker, orchestrator, typed batch config)
│   │   └── errors/                   (thiserror-derived domain error enums)
│   │       ├── session.rs            (SessionError)
│   │       ├── db.rs                 (DbError)
│   │       ├── hooks.rs              (HookError)
│   │       └── ledger.rs             (LedgerError)
│   └── Cargo.toml                    (loro, surrealdb, git2, rusqlite, serde, thiserror, ...)
│
├── codeflow-cli/                     (binary crate — thin CLI dispatch layer)
│   ├── src/
│   │   ├── main.rs                   (Clap CLI entry point)
│   │   └── commands/                 (subcommand dispatch to codeflow-core)
│   │       ├── hooks.rs              (codeflow hooks {event} {subcommand})
│   │       ├── db.rs                 (codeflow db {operation})
│   │       ├── workgraph.rs          (codeflow workgraph {operation})
│   │       ├── sync.rs               (codeflow sync daemon)
│   │       └── ...                   (test, doctor, cleanup, validate, etc.)
│   └── Cargo.toml                    (depends on codeflow-core, clap)
│
└── Cargo.toml                        (workspace: members = ["codeflow-core", "codeflow-cli"])

Single `codeflow` binary (external interface FROZEN):
  ├── codeflow hooks session-start init     (session management)
  ├── codeflow hooks pre-tool-use gate-check (enforcement)
  ├── codeflow sync daemon                  (Loro delta sync — subcommand)
  └── ... all other subcommands (identical command tree to Go CLI)
```

**Native Rust API for Loro operations:**

```rust
use loro::LoroDoc;

// Direct Rust API — no FFI, no C bindings, no CGo
let doc = LoroDoc::new();
let claims = doc.get_map("claims");
claims.insert("cmd/root.go", claim_value)?;     // Map CRDT set
let delta = doc.export(ExportMode::updates(&last_vv));  // Delta export
doc.import(&peer_delta)?;                        // Merge import
let snapshot = doc.export(ExportMode::Snapshot); // Full snapshot
```

**Complexity assessment:**

| Factor | Assessment |
|--------|-----------|
| Rust toolchain required | Yes -- for building the CLI binary (CI and developer machines) |
| FFI complexity | **None.** No CGo, no shared library, no FFI boundary. Loro and SurrealDB are direct crate dependencies. |
| Binding quality | **Maximum -- native Rust API.** `use loro::LoroDoc;` is the canonical, first-party API. |
| Cross-platform | macOS (arm64, x64) + Linux (x64) -- 3 builds via `cargo build --release --target`. |
| Testing | Rust unit tests and integration tests. Single language, single test framework. Trait-based design enables testing `LoroCoordinator` against mock `Transport` implementations. |
| Performance | Sub-millisecond for individual operations. Zero FFI overhead. Direct function calls within the same binary. |
| Alternative (pure Go) | No mature pure-Go CRDT library with Loro's feature set. Building one is significantly more effort. |
| Build pipeline | Single `cargo build` produces the `codeflow` binary. No two-language build. No shared library cross-compilation. |

---

## 5. Integration Plan (Phase A)

Phase A uses Loro CRDT as a direct crate dependency in the pure Rust CLI (from Epic 0). Loro CRDT is the sole coordination mechanism -- it is established in Phase A as the foundation for all parallel work. SurrealDB tasks are in Phase F (see [README](README.md)).

**Prerequisite:** Epic 0 (Rust CLI redesign) must be complete. After Epic 0, `use loro::LoroDoc;` is available natively -- no FFI, no shared library, no CGo. `LoroCoordinator` implements Epic 0's `Coordinator` trait; `GitRefTransport` implements Epic 0's `Transport` trait.

| Task | Description | Effort |
|------|-------------|--------|
| Replace state.json with state.loro | Migrate claims system from JSON read-modify-write to Loro Map CRDT. Direct `loro` crate API -- no FFI. Loro handles all coordination natively. | L |
| Wire claims into PreToolUse hooks | Add claim acquisition to gate-check hook. Before Edit/Write, acquire exclusive claim on target file pattern via Loro Map. Release on stage completion. | L |
| Fencing token validation via Loro | Fencing tokens stored in Loro Map. PreToolUse validates token matches latest for claimed resource. | M |
| Implement sync daemon logic | `codeflow sync daemon` subcommand (from Epic 0 Phase 0E). Uses native Loro API directly (same binary). Started by SessionStart hook, stops when no active sessions. Same-machine: compaction and health for shared `state.loro`. Multi-machine: exports Loro deltas via `doc.export(updates(&last_sync_vv))`, pushes to per-peer git ref `refs/coordination/loro/{peer-id}`, fetches all peer refs, imports deltas. Configurable sync interval (10-30s). Retry with backoff on network partition. Peer ID stored at `.state/runtime/peer-id`. | L |
| Clean up unused state.loro artifact | Remove current unused 483-byte `state.loro` file. Replace with real Loro-managed state. | S |

---

## 6. How Loro CRDT Coordination Works

This section demonstrates Loro CRDT coordination through concrete examples. Every example uses the same mechanism -- Loro operations on a shared `state.loro` document. The only difference between same-machine and multi-machine scenarios is the transport layer (shared file vs git delta sync).

### Example A: Claims -- Same Machine, Two Parallel Sessions

Two Claude Code sessions run in separate worktrees on the same machine. Both attempt to claim files for editing.

```text
Session A (worktree-ses-A)              Session B (worktree-ses-B)
    |                                       |
    v                                       v
PreToolUse fires:                       PreToolUse fires:
  Edit cmd/root.go                        Edit cmd/root.go
    |                                       |
    v                                       v
LoroMap.insert(                         LoroMap.insert(
  key="cmd/root.go",                      key="cmd/root.go",
  value={owner:"ses-A",                   value={owner:"ses-B",
         token:42,                               token:43,
         ttl:"5m"})                              ttl:"5m"})
    |                                       |
    v                                       v
Read state.loro                         Read state.loro
(shared via symlink to                  (shared via symlink to
 .state/coordination/state.loro)         .state/coordination/state.loro)
    |                                       |
    v                                       v
Loro merge: both inserts               Loro merge: both inserts
applied -- last-writer-wins             applied -- last-writer-wins
on same key. Token 43 wins.            on same key. Token 43 wins.
    |                                       |
    v                                       v
Check: my token (42) !=                 Check: my token (43) ==
  latest (43) --> BLOCKED                 latest (43) --> PROCEED
    |                                       |
    v                                       v
PreToolUse returns exit 2               PreToolUse returns exit 0
  "File claimed by ses-B"                Edit proceeds
```

**Key mechanism:** Both sessions write to the same `state.loro` file (symlinked from each worktree into the shared `.state/coordination/` directory). Loro's Map CRDT resolves concurrent inserts on the same key via last-writer-wins using the fencing token as the ordering criterion. The loser detects the conflict by comparing its own token against the latest value in the merged state.

### Example B: Claims -- Multi-Machine via Git Delta Sync

A developer machine and a CI worker operate on the same repository. Claims are synchronized through Loro delta export embedded in git commits.

```text
Developer Machine                       CI Worker
    |                                       |
    v                                       v
Acquire claim on                        Acquire claim on
  internal/db/*.go                        internal/api/*.go
    |                                       |
    v                                       v
LoroMap.insert(                         LoroMap.insert(
  key="internal/db/*",                    key="internal/api/*",
  value={owner:"dev-1",                   value={owner:"ci-1",
         token:100})                             token:200})
    |                                       |
    v                                       v
Sync daemon (codeflow sync)             Sync daemon (codeflow sync)
exports delta:                          exports delta:
  doc.export(updates(&last_vv))           doc.export(updates(&last_vv))
  --> bytes (only new ops)                --> bytes (only new ops)
    |                                       |
    v                                       v
Push to OWN peer ref:                  Push to OWN peer ref:
  git update-ref                          git update-ref
    refs/coordination/loro/dev-1            refs/coordination/loro/ci-1
  git push origin                         git push origin
    refs/coordination/loro/dev-1            refs/coordination/loro/ci-1
  (no conflicts -- each peer              (no conflicts -- each peer
   writes only to its own ref)             writes only to its own ref)
    |                                       |
    v                                       v
Fetch ALL peer refs:                   Fetch ALL peer refs:
  git fetch origin                        git fetch origin
    'refs/coordination/loro/*'              'refs/coordination/loro/*'
    |                                       |
    v                                       v
Import deltas from each peer:          Import deltas from each peer:
  doc.import(ci-1_delta)                 doc.import(dev-1_delta)
    |                                       |
    v                                       v
Loro merge: deterministic.             Loro merge: deterministic.
Both claims visible.                   Both claims visible.
No conflict (different keys).          No conflict (different keys).
    |                                       |
    v                                       v
Update last_sync_vv =                  Update last_sync_vv =
  doc.oplog_vv()                         doc.oplog_vv()
```

**Key mechanism:** Each machine's sync daemon (`codeflow sync daemon`, a subcommand of the Rust CLI binary) pushes only to its own per-peer ref (`refs/coordination/loro/{peer-id}`). No push conflicts are possible because each peer writes exclusively to its own ref. Daemons fetch all peer refs via glob (`refs/coordination/loro/*`) and import every peer's deltas. Loro's deterministic merge ensures all machines converge to the same state. Different keys merge without conflict. Same-key conflicts resolve via last-writer-wins, identical to the same-machine case.

### Example C: Merge Queue -- Autorun Workers with LoroList

Three autorun workers coordinate a merge queue. LoroList maintains FIFO ordering across concurrent inserts.

```text
Worker 1           Worker 2           Worker 3
    |                  |                  |
    v                  v                  v
Task: fix/login    Task: feat/api     Task: fix/typo
    |                  |                  |
    v                  v                  v
LoroList.insert(   LoroList.insert(   LoroList.insert(
  pos=end,           pos=end,           pos=end,
  value={            value={            value={
    task:"fix/login",  task:"feat/api",   task:"fix/typo",
    worker:"w1",       worker:"w2",       worker:"w3",
    priority:1})       priority:2})       priority:1})
    |                  |                  |
    +--------+---------+---------+--------+
             |                   |
             v                   v
      Loro merge: all three inserts
      applied. Position-based ordering
      preserves each worker's insert point.
             |
             v
      Merged queue (deterministic):
        [fix/login, fix/typo, feat/api]
        (priority-sorted within Loro order)
             |
             v
      Worker 1 pops fix/login (head)
      Worker 2 pops fix/typo (next)
      Worker 3 waits (feat/api is third)
```

**Key mechanism:** LoroList handles concurrent inserts at the same position (end of list) by assigning deterministic ordering based on operation IDs. All workers see the same merged queue regardless of the order they receive deltas. Priority sorting is applied at the application layer after Loro merge.

### Example D: Work Hierarchy -- LoroTree for Epic-Task Structure

Two sessions concurrently create tasks under the same epic. LoroTree handles concurrent tree mutations with cycle detection.

```text
Session A                               Session B
    |                                       |
    v                                       v
Create task under                       Create task under
  INF-EPC-022                             INF-EPC-022
    |                                       |
    v                                       v
LoroTree.create_node(                   LoroTree.create_node(
  parent="INF-EPC-022",                  parent="INF-EPC-022",
  id="INF-TSK-022-001",                  id="INF-TSK-022-002",
  data={title:"Setup",                   data={title:"Tests",
        status:"pending"})                     status:"pending"})
    |                                       |
    v                                       v
Session A also moves                    Session B also moves
  existing task:                          existing task:
LoroTree.move(                          (no move)
  id="INF-TSK-021-040",
  new_parent="INF-EPC-022")
    |                                       |
    +---------------+-----------------------+
                    |
                    v
             Loro merge:
               - Both new nodes created under INF-EPC-022
               - Move operation applied (no cycle created)
               - Result: INF-EPC-022 has 3 children
                    |
                    v
             INF-EPC-022
             ├── INF-TSK-022-001 (Setup)
             ├── INF-TSK-022-002 (Tests)
             └── INF-TSK-021-040 (moved from INF-EPC-021)
```

**Key mechanism:** LoroTree prevents cycles automatically. Concurrent node creations under the same parent merge cleanly. Concurrent move operations that would create a cycle are detected and one is rejected deterministically. The tree structure is always valid after merge.

### Example E: Architecture -- Single Mechanism, Two Transports

```text
                    ┌─────────────────────────────┐
                    │       Loro CRDT Engine       │
                    │  (native crate in Rust CLI)  │
                    │                              │
                    │  LoroDoc                     │
                    │  ├── LoroMap  (claims)       │
                    │  ├── LoroList (queues)       │
                    │  └── LoroTree (hierarchy)    │
                    └──────────┬──────────────────┘
                               │
                    ┌──────────┴──────────┐
                    │                     │
            ┌───────▼───────┐    ┌────────▼────────┐
            │ Same-Machine  │    │  Multi-Machine  │
            │  Transport    │    │   Transport     │
            │               │    │                 │
            │ Shared file:  │    │ codeflow sync   │
            │ state.loro    │    │ (Rust binary)   │
            │ (symlinked    │    │                 │
            │  into each    │    │ Export: doc.    │
            │  worktree)    │    │   export(       │
            │               │    │   updates(&vv)) │
            │ Daemon role:  │    │ Push own ref:   │
            │ Compaction,   │    │   refs/coord/   │
            │ health, TTL   │    │   loro/{peer}   │
            │               │    │ Fetch all refs: │
            │ Latency:      │    │   loro/*        │
            │ Immediate     │    │                 │
            │ (shared fs)   │    │ Latency:        │
            │               │    │ 10-30s interval │
            └───────────────┘    └─────────────────┘

One CRDT engine. Two transports. Zero coordination logic differences.
Sync daemon subcommand manages both: compaction for local, per-peer delta sync for remote.
```

**Key insight:** The Loro CRDT engine is identical in both scenarios. Same-machine sessions share the `state.loro` file directly (symlinked from each worktree). Multi-machine setups use `codeflow sync daemon` (a subcommand of the Rust CLI binary) to export Loro deltas and push them to per-peer git refs (`refs/coordination/loro/{peer-id}`). Each peer writes only to its own ref -- no push conflicts. The merge semantics are identical -- only the transport latency differs. The daemon subcommand manages both transports: compaction and health for same-machine, per-peer delta export/import for multi-machine.

---

## 7. Multi-Machine Sync Mechanism

This section addresses how Loro delta sync works across machines. A background sync daemon is essential for multi-machine coordination -- this is the industry standard pattern used by all production CRDT systems (Figma, Automerge, Riak). Loro is explicitly transport-agnostic: it provides `export()` and `import()` APIs and leaves the sync transport to the application layer.

### Sync Architecture

| Transport | Scope | Mechanism | Latency |
|-----------|-------|-----------|---------|
| Same-machine | Parallel sessions on one machine | Shared `state.loro` file (symlinked into each worktree) | Immediate (filesystem) |
| Multi-machine | Sessions across machines | `codeflow sync daemon` (Rust CLI subcommand) exports Loro deltas to per-peer git ref (`refs/coordination/loro/{peer-id}`), fetches all peer refs | 10-30 seconds (configurable) |

Both transports use the same Loro CRDT engine. The `codeflow sync daemon` subcommand manages the multi-machine transport and also handles compaction and health monitoring for same-machine shared state.

### Sync Daemon Design

**Subcommand:** `codeflow sync daemon`

A long-running process (spawned as a subcommand of the Rust CLI binary) that manages Loro state synchronization using the native Loro API directly. Runs within the same binary as the CLI -- no separate binary, no FFI overhead. Started by the SessionStart hook, stops automatically when no active sessions remain.

```text
SessionStart hook
    |
    v
Is daemon already running? (PID file at .state/coordination/sync-daemon.pid)
    |
    +--- YES --> Register session with daemon (increment ref count)
    |
    +--- NO ---> Start daemon:
                   codeflow sync daemon --interval 10s
                 Daemon reads peer ID from .state/runtime/peer-id
                   (format: {username}-{hostname}, created on first run)
                 Register session (ref count = 1)
    |
    v
Session runs normally...
    |
    v
SessionEnd hook
    |
    v
Deregister session (decrement ref count)
    |
    v
Ref count == 0?
    |
    +--- YES --> Daemon shuts down gracefully
    +--- NO ---> Daemon continues for remaining sessions
```

**Daemon sync loop (multi-machine):**

```text
Every sync interval (10-30s, configurable):
    |
    v
1. Export new deltas since last sync:
     delta = doc.export(updates(&last_sync_vv))
     (Loro's VersionVector tracks exactly which ops are new)
    |
    v
2. Push to OWN peer ref (no conflicts possible):
     git update-ref refs/coordination/loro/{peer-id} <tree-with-delta>
     git push origin refs/coordination/loro/{peer-id}
     (NOT a branch -- like git notes refs, lightweight,
      doesn't pollute branch list or trigger branch protections.
      Each peer writes ONLY to its own ref -- zero push conflicts.)
    |
    v
3. Fetch ALL peer refs:
     git fetch origin 'refs/coordination/loro/*'
     (Glob fetch retrieves every peer's ref in one call)
    |
    v
4. Import deltas from each peer:
     for each remote peer ref:
       doc.import(peer_delta_bytes)
     (Loro handles merge deterministically, regardless of import order)
    |
    v
5. Update local version vector:
     last_sync_vv = doc.oplog_vv()
     (Stored locally at .state/coordination/sync-state.json)
    |
    v
6. Write merged state:
     doc.export(snapshot) --> state.loro
     (Atomic write: write to temp file, rename)
```

**Loro export modes used by the daemon:**

| Export Mode | When Used | What It Produces |
|-------------|-----------|-----------------|
| `updates(&VersionVector)` | Every sync interval | Only ops newer than the given version vector -- minimal delta |
| `snapshot` | After import (writing merged state) | Complete document state for local `state.loro` |
| `all_updates` | Initial sync (first push) or state recovery | All ops from the beginning of time |

**Version tracking:**

The daemon stores `last_sync_vv` (the version vector at last successful sync) in `.state/coordination/sync-state.json`. On each cycle, it calls `doc.export(updates(&last_sync_vv))` to produce only the ops generated since the last push. After a successful fetch and import, it updates `last_sync_vv = doc.oplog_vv()`. This ensures:
- No duplicate ops are pushed (version vector is precise)
- No ops are missed (version vector captures all causal history)
- Recovery after crash: re-export from the stored version vector (may re-send some ops; Loro's idempotent import handles duplicates)

**Git ref namespace (`refs/coordination/loro/{peer-id}`):**

Each daemon writes to a per-peer ref under a shared namespace:
- **Per-peer isolation:** Each machine pushes ONLY to `refs/coordination/loro/{peer-id}` -- zero push conflicts, no compare-and-swap retry loops
- **Peer discovery:** `git ls-remote origin 'refs/coordination/loro/*'` lists all active peers
- **Glob fetch:** `git fetch origin 'refs/coordination/loro/*'` retrieves all peer deltas in one call
- **Peer ID:** `{username}-{hostname}` or stable machine fingerprint, stored at `.state/runtime/peer-id` (created on first daemon start, reused across sessions)
- Does not appear in `git branch` output -- no pollution of the branch list
- Does not trigger branch protection rules or CI pipelines
- Lightweight: same mechanism as `git notes` or GitHub PR refs (`refs/pull/*/head`)
- Standard pattern used by git notes, GitHub PR refs, Gerrit change refs

**Same-machine daemon responsibilities:**

For same-machine parallel sessions (shared `state.loro` via symlink), the daemon subcommand does not perform sync (the file is already shared). Instead, it handles:
- **Compaction:** Periodically calls `doc.export(snapshot)` to write a fresh `state.loro`, preventing unbounded op-log growth
- **Health monitoring:** Detects stale claims (TTL expired), logs warnings, optionally auto-releases
- **Crash recovery:** On startup, validates `state.loro` integrity. If corrupt, rebuilds from peer ref history (`refs/coordination/loro/*`) or JSONL event ledger

### Network Partition Behavior

| Partition Scenario | Daemon Behavior | Session Behavior | Recovery |
|-------------------|-----------------|------------------|----------|
| Machine loses network | Daemon retries push/fetch with exponential backoff (1s, 2s, 4s, ... up to 60s). Logs warnings. | Sessions continue normally with local state. All Loro operations succeed locally. | On reconnect: daemon pushes accumulated deltas. Remote machines import and converge. |
| Two machines diverge for extended period | Each daemon maintains local state independently. Deltas accumulate locally. | Claims may overlap (both machines think they own a file). | On merge: Loro resolves deterministically. Stale claims expire via TTL. Fencing token comparison resolves overlapping claims. |
| Git push rejected (stale peer ref) | With per-peer refs, push rejection is rare (only the local peer writes to its own ref). If it occurs (e.g., daemon restarted mid-push), daemon simply retries. | No impact on sessions -- daemon handles retry transparently. | Automatic. Single retry suffices since no other peer writes to this ref. |
| Daemon crashes | PID file becomes stale. | Sessions continue with local state (same-machine: shared file still works). | Next SessionStart detects stale PID, restarts daemon. Resumes from stored `last_sync_vv`. |

**Key property:** Sessions are never blocked by daemon state. The daemon is a best-effort sync process -- if it fails, sessions degrade to local-only operation and converge when the daemon recovers. This is the fundamental advantage of CRDTs over lock-based coordination: the sync layer can fail without blocking work.

### Latency Characteristics

| Scenario | Transport | Typical Latency | Conflict Window |
|----------|-----------|-----------------|-----------------|
| Same machine, parallel sessions | Shared file (symlink) | Immediate (filesystem) | Microseconds |
| Same machine, separate worktrees | Shared file (symlink) | Immediate (filesystem) | Microseconds |
| Multi-machine, daemon sync | Git ref push/fetch (periodic) | 10-30 seconds (configurable) | Sync interval |
| Multi-machine, daemon (network partition) | Local accumulation, sync on reconnect | Minutes to hours (partition duration) | Partition duration (TTL mitigates) |

**Critical observation:** Same-machine parallel sessions (the primary use case) have effectively zero sync latency because all worktrees share the same `state.loro` file via symlink. The daemon's multi-machine sync adds 10-30 seconds of latency, which is acceptable for claim coordination where TTLs are measured in minutes.

### Conflict-Free Guarantee

Loro provides a mathematically proven conflict-free merge guarantee:

1. **Commutativity:** Applying operations A then B produces the same state as B then A
2. **Idempotency:** Applying the same operation twice produces the same state as applying it once
3. **Associativity:** Grouping operations differently produces the same result

This means:
- **No merge conflicts.** Ever. Two machines can diverge for hours, then merge cleanly.
- **No coordination protocol.** No leader election, no distributed locks, no consensus algorithm.
- **Order independence.** Deltas can arrive out of order, be duplicated, or be delayed -- the final state is always the same.

The one caveat: "conflict-free" means the data structure always merges cleanly, not that the application-level semantics are always what you want. Two sessions claiming the same file will both succeed in writing to Loro -- but the application layer (fencing token comparison) detects the semantic conflict and blocks the loser. Loro handles the data merge; the application handles the policy.

### Industry Precedent

This daemon-based sync pattern is the industry standard for production CRDT systems:

| System | Sync Mechanism | Similarity to CodeFlow |
|--------|---------------|----------------------|
| Figma | Background WebSocket sync to central server | Same pattern: background process, delta export/import, local-first operation |
| Automerge | Pluggable sync protocol, typically WebSocket or peer-to-peer | Same: transport-agnostic CRDT engine, application provides sync transport |
| Riak | Background anti-entropy protocol between nodes | Same: periodic sync, version vectors for delta tracking, eventual consistency |
| Yjs | Provider-based sync (WebSocket, WebRTC, or custom) | Same: CRDT engine separated from transport, background sync process |

Loro explicitly follows this pattern: the library provides `export()` and `import()` APIs and delegates transport entirely to the application. `codeflow sync daemon` is CodeFlow's transport implementation -- a subcommand of the Rust CLI binary, using Loro's native API directly within the same process.

### Open Design Questions

These questions require implementation-phase decisions (Phase A tasks):

| # | Question | Options | Impact |
|---|----------|---------|--------|
| 1 | Delta format for per-peer ref storage | (a) Loro binary snapshot as blob object (b) Base64-encoded delta in a JSON metadata file | Affects git storage efficiency. Binary is smaller; JSON is human-inspectable. |
| 2 | TTL enforcement location | (a) Daemon expires stale claims periodically (b) Hook-based check at PreToolUse only (c) Both | Affects claim freshness. Daemon-based (a) provides proactive cleanup; hook-based (b) is reactive. |
| 3 | State recovery after corrupt `state.loro` | (a) Rebuild from peer ref history (`refs/coordination/loro/*`) (b) Rebuild from JSONL event ledger (`claim_created`/`claim_released` events) (c) Both, with cross-validation | Affects reliability. Option (c) is most robust but most complex. |
| 4 | Same-machine lock for `state.loro` writes | (a) File-level flock for write serialization (I/O safety, preventing torn writes) (b) Rely on Loro's CRDT merge (no lock needed -- atomic rename after write) (c) Advisory lock with fallback to merge | I/O safety question, not coordination logic. Atomic rename (b) may be sufficient. |
| 5 | Stale peer ref cleanup | (a) Daemon prunes refs for peers not seen in N days (b) Manual `codeflow sync prune` command (c) Both | Prevents unbounded ref accumulation from decommissioned machines. |

---

## 8. Multi-Machine Merge Flow

With Loro integration and the sync daemon, multi-machine coordination works automatically:

```text
Machine A (Developer)                Machine B (CI Worker)
    |                                     |
    v                                     v
Edit file foo.go                     Edit file bar.go
Acquire claim (Loro Map)             Acquire claim (Loro Map)
    |                                     |
    v                                     v
codeflow sync daemon (10-30s):        codeflow sync daemon (10-30s):
  delta = doc.export(                  delta = doc.export(
    updates(&last_sync_vv))              updates(&last_sync_vv))
  push refs/coord/loro/dev-1         push refs/coord/loro/ci-1
  fetch refs/coord/loro/*            fetch refs/coord/loro/*
    |                                     |
    v                                     v
Import each peer's delta:            Import each peer's delta:
  doc.import(ci-1_delta)               doc.import(dev-1_delta)
    |                                     |
    v                                     v
Loro merge: deterministic.           Loro merge: deterministic.
Both claims visible.                 Both claims visible.
Both machines converge to            Both machines converge to
  same state automatically.            same state automatically.
```

The `codeflow sync daemon` subcommand handles all multi-machine coordination transparently. Sessions operate on local Loro state; the daemon syncs deltas in the background via per-peer git refs (`refs/coordination/loro/{peer-id}`). Each peer pushes only to its own ref -- no conflicts. Same-machine parallel sessions use the shared `state.loro` file directly (no daemon sync needed -- immediate via filesystem). One CRDT engine, one sync subcommand, one solution.

---

[← Back to Overview](README.md)
