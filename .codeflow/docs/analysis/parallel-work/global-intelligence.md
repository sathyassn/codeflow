---
title: "Global Intelligence Layer"
type: analysis
status: draft
author: cf-documentation
created_at: "2026-03-05"
updated_at: "2026-03-05"
parent: "parallel-work/README.md"
---

# Global Intelligence Layer

[← Back to Overview](README.md)

## Table of Contents

- [1. Overview](#1-overview)
- [2. Architecture](#2-architecture)
- [3. SurrealDB Deployment Modes](#3-surrealdb-deployment-modes)
- [4. Daemon Architecture](#4-daemon-architecture)
- [5. Data Model](#5-data-model)
- [6. Configuration](#6-configuration)
- [7. Embeddings](#7-embeddings)
- [8. Sync Engine](#8-sync-engine)
- [9. Reliability](#9-reliability)
- [10. Performance](#10-performance)
- [11. CLI Commands](#11-cli-commands)
- [12. Migration Path](#12-migration-path)
- [13. Epic C Task Outline](#13-epic-c-task-outline)

---

## 1. Overview

The global intelligence layer adds a second tier of data persistence above the project-local SurrealDB embedded database. It enables cross-project querying, semantic search across repositories, and team-wide context aggregation — capabilities that are impossible when every project stores its data in isolation.

**Why it exists:**

The project-local SurrealDB embedded database (surrealkv:// at `.state/db/codeflow/`) handles all single-project operations: session tracking, task management, pathflow events, and workgraph queries. This is sufficient for solo developers and teams working in a single repository.

Global mode addresses three use cases that project-local cannot support:

1. **Cross-project context:** A developer working on a frontend repo needs to understand decisions made in the backend repo. Without global mode, this context is siloed in `.state/db/codeflow/` under the backend project root.
2. **Team-wide analytics:** A team lead wants to see all open tasks across five repositories in a single query.
3. **Semantic search across codebases:** An agent needs to find similar past decisions or resolved issues across all projects the team has worked on.

**Key constraints:**

- Global mode is **optional**. The default is project-only operation with SurrealDB embedded. No daemon is needed for solo or single-project use.
- The global database is **always reconstructible** from Tier 0 JSONL ledger files. No data is uniquely resident in the global database. If it is lost or corrupted, `codeflow db rebuild` replays all registered projects' JSONL ledgers and reconstructs the complete state.
- The `codeflow` binary IS the SurrealDB server in daemon mode. No separate SurrealDB installation, no Docker, no external dependency.

---

## 2. Architecture

### Two-Level Data Architecture

```text
Project Level (always present)
  .state/db/codeflow/        SurrealDB embedded (surrealkv://)
  .state/ledger/*.jsonl      Tier 0 JSONL (rebuild authority)
       |
       | ledger replay (sync)
       v
Global Level (optional)
  ~/.codeflow/db.sock        SurrealDB daemon (Unix socket)
  ~/.codeflow/data/          SurrealDB daemon storage
```

Each project maintains its own SurrealDB embedded instance. When global mode is enabled, the daemon aggregates data from all registered projects into a single queryable store.

### Access Pattern Split

| Operation | Uses | Reason |
|-----------|------|--------|
| Hook handlers (session start, tool use) | Local embedded | Sub-millisecond latency required; daemon not guaranteed available |
| PathFlow phase tracking | Local embedded | Isolation; no cross-project contamination |
| Task CRUD, workgraph writes | Local embedded | Authoritative writes stay local |
| Semantic search across history | Global daemon | Vector index spans all projects |
| Cross-project task queries | Global daemon | Data from multiple projects in one query |
| Team dashboard / analytics | Global daemon | Aggregated views |
| Context retrieval for agents | Global daemon (fallback: local) | Best quality with global; works locally without daemon |

### Data Flow: Ledger Replay (Recommended)

The recommended sync strategy is ledger replay: the daemon reads JSONL files from each registered project's `.state/ledger/` directory and imports them into the global SurrealDB store.

```text
Project A ledger  -->  sync engine  -->  SurrealDB daemon
Project B ledger  -->  sync engine  -->  (all records tagged with project_id)
Project C ledger  -->  sync engine  -->
```

**Why ledger replay over dual-write:**

- Tier 0 JSONL is already the rebuild authority. Replaying it is consistent by definition.
- No write-path latency added to hook handlers.
- The daemon can be rebuilt from scratch at any time without data loss.
- Dual-write requires every write path to be updated; ledger replay requires no changes to write paths.

Dual-write (writing to both local and daemon simultaneously) is a future enhancement for lower sync latency, not the initial implementation.

---

## 3. SurrealDB Deployment Modes

### Embedded Mode (surrealkv://)

Used for project-level storage. The SurrealDB embedded engine runs in-process within the `codeflow` binary. No separate process, no socket, no port.

```rust
let db = Surreal::new::<SurrealKv>(".state/db/codeflow/").await?;
```

**Characteristics:**

- File-based storage at `.state/db/codeflow/`
- Single-process access only (exclusive file lock)
- Sub-millisecond query latency
- Zero network overhead
- Works offline

### Server Mode (Unix Socket)

Used for global daemon. The SurrealDB server runs as a subprocess of the `codeflow` binary, accepting connections via a Unix domain socket at `~/.codeflow/db.sock`.

```rust
let db = Surreal::new::<Ws>("ws+unix://~/.codeflow/db.sock").await?;
```

**Characteristics:**

- Multiple processes can connect simultaneously
- ~1-2ms connection latency (local socket, no TCP)
- No port conflicts, no firewall rules
- Local-only (Unix socket is not network-accessible)

### Why Embedded Does Not Work for Global

SurrealDB embedded mode (`surrealkv://`) uses an exclusive file lock on the database directory. Only one process can open it at a time. With multiple projects registering and syncing data, the global database must accept concurrent connections — embedded mode cannot serve this.

| Mode | Concurrency | Latency | Use Case |
|------|-------------|---------|----------|
| Embedded (surrealkv://) | Single process only | <1ms | Project-local operations |
| Server (Unix socket) | Multiple processes | ~1-2ms | Global aggregation |
| Server (TCP) | Multiple processes | ~2-5ms | Future remote/team use |

### Hybrid Approach

The architecture uses embedded for single-project and daemon for multi-project. This is not a trade-off — it is the correct tool for each job:

- **Single project:** embedded gives maximum performance with zero overhead
- **Global:** daemon gives concurrent access with minimal latency overhead

Projects that do not enable global mode never start the daemon and incur no overhead.

---

## 4. Daemon Architecture

### The codeflow Binary IS the Server

The `codeflow db daemon start` subcommand starts an embedded SurrealDB server within the `codeflow` process. There is no separate SurrealDB binary to install, no Docker container to manage, and no external service to configure.

```text
codeflow db daemon start
    |
    v
Starts SurrealDB server inside the codeflow process
Listens on Unix socket: ~/.codeflow/db.sock
Writes PID to: ~/.codeflow/run/daemon.pid
Logs to: ~/.codeflow/logs/daemon.log
```

### Subcommands

| Command | Action |
|---------|--------|
| `codeflow db daemon start` | Start daemon in background; write PID file |
| `codeflow db daemon stop` | Graceful shutdown; remove PID file |
| `codeflow db daemon status` | Show running/stopped; connected clients; data size |

### Unix Socket vs TCP

The daemon exclusively uses a Unix domain socket (`~/.codeflow/db.sock`). TCP is not exposed by default.

**Reasons for Unix socket preference:**

- **Security:** Unix socket is only accessible to processes running as the same user. No port scanning, no accidental exposure.
- **Latency:** Unix socket avoids TCP handshake overhead. ~1-2ms vs ~2-5ms for loopback TCP.
- **No port conflicts:** No `EADDRINUSE` errors from other services occupying port 8000 or similar.
- **Convention:** launchd/systemd service standards favor socket activation for local services.

### Service Installer

The daemon can be registered as a system service for automatic start on login:

```text
codeflow db install-service    # Installs launchd plist (macOS) or systemd unit (Linux)
codeflow db uninstall-service  # Removes service registration
```

**macOS (launchd):**

- Plist written to `~/Library/LaunchAgents/dev.codeflow.daemon.plist`
- `RunAtLoad: true` — starts on login
- `KeepAlive: true` — auto-restarts on crash

**Linux (systemd):**

- Unit file written to `~/.config/systemd/user/codeflow-daemon.service`
- `WantedBy=default.target` — starts on user session login
- `Restart=on-failure` — auto-restarts on crash

### PID File

The daemon writes its PID to `~/.codeflow/run/daemon.pid`. The `codeflow db daemon stop` command reads this file to send SIGTERM, then waits up to 5 seconds for graceful shutdown before sending SIGKILL.

### Why Not Docker

Docker would add a container runtime dependency that is heavier than the problem it solves. The daemon is a local developer tool, not a server deployment:

| Concern | Docker | codeflow daemon |
|---------|--------|-----------------|
| Installation | Docker Desktop required | Zero — same `codeflow` binary |
| Startup overhead | ~2-5s container startup | <100ms process start |
| Resource usage | ~200MB+ baseline | <50MB |
| Local-only guarantee | Requires configuration | Unix socket is inherently local |
| Developer complexity | Dockerfile, compose, volumes | One CLI command |

---

## 5. Data Model

### Project Registration Schema

Every project must be registered before it can sync data to the global daemon:

```surql
DEFINE TABLE project SCHEMAFULL;
DEFINE FIELD project_id ON project TYPE string;      -- e.g., "proj_a3f8c2d1"
DEFINE FIELD name ON project TYPE string;            -- human-readable name
DEFINE FIELD repo_path ON project TYPE string;       -- absolute local filesystem path
DEFINE FIELD repo_url ON project TYPE option<string>; -- remote git URL (optional)
DEFINE FIELD area_prefixes ON project TYPE array<string>; -- ["INF", "PLN", "DEV"]
DEFINE FIELD registered_at ON project TYPE datetime;
DEFINE FIELD last_synced ON project TYPE option<datetime>;
DEFINE FIELD sync_cursor ON project TYPE option<string>; -- last synced ledger position
```

### Project-Scoped Records

Every record in the global database carries a `project` field linking it to its source project:

```surql
DEFINE TABLE task SCHEMAFULL;
DEFINE FIELD id ON task TYPE string;
DEFINE FIELD project ON task TYPE record<project>; -- project scoping
DEFINE FIELD format_id ON task TYPE string;        -- e.g., "INF-TSK-021-001"
DEFINE FIELD title ON task TYPE string;
DEFINE FIELD status ON task TYPE string;
DEFINE FIELD work_type ON task TYPE string;
DEFINE FIELD created_at ON task TYPE datetime;
-- ... additional task fields
```

The same pattern applies to `session`, `epic`, `event`, and `memory` tables.

### Graph Relations

SurrealDB's graph notation enables relationship traversal across records:

| Relation | From | To | Meaning |
|----------|------|-----|---------|
| `can_access` | project | project | Project A can query Project B's data |
| `depends_on` | task | task | Cross-project task dependency |
| `belongs_to` | task | epic | Task-to-epic containment |
| `produced_in` | artifact | session | Code/doc produced during session |

**Creating a relation:**

```surql
RELATE project:frontend->can_access->project:backend
  SET granted_at = time::now(),
      scopes = ["tasks", "sessions", "memory"];
```

### Cross-Project Query Example

A frontend project agent querying backend API decisions:

```surql
SELECT title, description, created_at
FROM task
WHERE project IN (
  SELECT ->can_access->project FROM project:frontend
)
AND work_type = "PLAN"
AND title CONTAINS "API"
ORDER BY created_at DESC
LIMIT 10;
```

The same query with semantic search (vector + graph in one statement):

```surql
SELECT title, description, vector::similarity::cosine(embedding, $query_vec) AS score
FROM task
WHERE project IN (
  SELECT ->can_access->project FROM project:frontend
)
AND vector::similarity::cosine(embedding, $query_vec) > 0.75
ORDER BY score DESC
LIMIT 5;
```

### Visibility Enforcement in Queries

The `can_access` graph relation is checked in every cross-project query. A project cannot see another project's data unless a `can_access` relation exists:

```surql
-- Safe cross-project query pattern
LET $visible = (SELECT ->can_access->project FROM project:$current_project);
SELECT * FROM task WHERE project IN $visible OR project = $current_project;
```

---

## 6. Configuration

### Per-Project Config

Located at `.codeflow/config/project.toml` in each repository:

```toml
[project]
name = "codeflow"
area_prefixes = ["INF", "PLN", "DEV"]
# project_id auto-generated on first registration

[sharing]
# Other projects this project can see data from
visible_projects = ["codeflow-frontend", "codeflow-docs"]

# Tags of projects to include (alternative to explicit names)
visible_tags = ["team:platform"]

# Specific data types to expose/consume
visible_scopes = ["tasks", "sessions"]

# Whether this project makes its data visible to others
allow_sharing = true

# Specific scopes to share (subset of all data)
shared_scopes = ["tasks", "epics"]
```

### Global Config

Located at `~/.codeflow/config.toml`:

```toml
[database]
# Project-local: always uses embedded SurrealDB (surrealkv://)
# Global: daemon socket path
daemon_socket = "~/.codeflow/db.sock"

# How long to wait for daemon connection before falling back to local-only
daemon_timeout_ms = 500

[daemon]
# Storage directory for global database files
data_dir = "~/.codeflow/data"

# Log file
log_file = "~/.codeflow/logs/daemon.log"

# PID file
pid_file = "~/.codeflow/run/daemon.pid"

# Sync interval for ledger replay
sync_interval_seconds = 30
```

---

## 7. Embeddings

### Local ONNX Model

Embeddings are generated locally using the `ort` crate (ONNX Runtime for Rust). No external API call, no internet dependency, no per-query cost.

**Model:** all-MiniLM-L6-v2

| Property | Value |
|----------|-------|
| Dimensions | 384 |
| Model size | ~50MB |
| Inference time | ~5-10ms per text chunk |
| Language | English (primary) |
| License | Apache 2.0 |
| Offline capable | Yes — model bundled with binary or downloaded once |

### What Gets Embedded

Embeddings are generated on sync (when records are written to the global daemon):

- Task titles and descriptions
- Epic summaries
- Session summaries from `.claude/memory/`
- Key decision text from planning documents

### Why Local ONNX

| Option | Cost | Latency | Offline | Setup |
|--------|------|---------|---------|-------|
| OpenAI API (text-embedding-3-small) | $0.02/1M tokens | ~100-200ms | No | API key required |
| Local ONNX (all-MiniLM-L6-v2) | Zero | ~5-10ms | Yes | Zero — crate dep |
| candle (Rust ML, HuggingFace) | Zero | ~5-10ms | Yes | Rust-native alternative |
| SurrealDB ML (built-in) | Zero | ~10-20ms | Yes | Needs SurrealDB ML feature |

**Recommendation: Local ONNX via `ort` crate.** The `ort` crate is production-ready, all-MiniLM-L6-v2 is widely used and well-validated, and offline operation is a hard requirement for a developer tool. candle is a valid alternative if the team prefers pure Rust without the ONNX Runtime dependency.

### Vector Index in SurrealDB

```surql
DEFINE INDEX task_embedding ON task FIELDS embedding HNSW DIMENSION 384;
```

HNSW (Hierarchical Navigable Small World) provides approximate nearest-neighbor search with sub-millisecond query latency at typical developer corpus sizes (tens of thousands of records).

---

## 8. Sync Engine

### Ledger Replay Strategy

The sync engine reads JSONL events from each registered project's ledger and imports them into the global daemon:

```text
for each registered project:
  read sync_cursor from project record
  open project.root_path + "/.state/ledger/"
  for each .jsonl file with events after cursor:
    parse LedgerEvent (serde)
    upsert into SurrealDB (project-scoped record)
    generate embedding if text field present
    update sync_cursor
```

### Sync Cursors

Each project record in the global database stores a `sync_cursor` — the position in the JSONL ledger up to which events have been imported. On each sync cycle, only new events since the cursor are processed.

Cursors are file-offset based (byte position in the last-read JSONL file), providing exact resumption after daemon restart.

### Real-Time Dual-Write (Future Enhancement)

For lower sync latency, write paths can be extended to write to both local embedded and the global daemon simultaneously. This reduces sync lag from the polling interval (~30s) to near-zero.

This is deferred to a future iteration because:
- Dual-write requires modifying all write paths in the Rust CLI
- Ledger replay provides sufficient freshness for context queries (30s lag is acceptable)
- Dual-write adds write-path latency and failure modes

### Rebuild Capability

The global database can be rebuilt from scratch at any time:

```bash
codeflow db rebuild
```

This command:
1. Drops all tables in the global daemon database
2. Re-registers all projects from `~/.codeflow/registry.toml`
3. Replays all JSONL ledger events from each project from the beginning
4. Regenerates all embeddings

Full rebuild is the recovery path for corruption, schema migrations, or adding new record types to the global schema.

---

## 9. Reliability

### Failure Modes

| Failure | Detection | Behavior | Recovery |
|---------|-----------|----------|---------|
| Daemon crash | `codeflow db daemon status` shows stopped; launchd/systemd auto-restarts | All operations fall back to local embedded; no data loss | Auto-restart via service installer; manual: `codeflow db daemon start` |
| Daemon not started | Connection attempt times out (500ms) | Graceful fallback to local-only mode; warning logged | `codeflow db daemon start` or `codeflow db install-service` |
| Corrupt daemon data | Queries return errors or unexpected results | `codeflow doctor` detects schema violations | `codeflow db rebuild` replays from JSONL |
| Socket inaccessible | File permission error or missing socket file | Fallback to local-only; error in `codeflow doctor` | Check permissions; restart daemon |
| Storage full | Write errors from SurrealDB | Daemon stops accepting writes; local embedded continues | Free disk space; `codeflow db daemon start` |

### Key Resilience Property

The global database is always reconstructible from Tier 0 JSONL. This is the most important reliability property:

- JSONL ledger files are the source of truth
- The global database is a derived, queryable view of the ledger
- Losing the global database is never catastrophic — it is an availability event, not a data loss event
- `codeflow db rebuild` restores full state from JSONL

### Graceful Degradation

The `codeflow` binary checks daemon availability at startup and on each global query. If the daemon is unavailable:

1. Local project operations continue without interruption
2. Global queries (`codeflow context search`, cross-project views) return local-only results
3. A warning is displayed: `Global daemon unavailable — showing local results only`
4. `codeflow doctor` reports daemon status and suggests `codeflow db daemon start`

---

## 10. Performance

### Expected Latencies

| Operation | Expected Latency | Notes |
|-----------|-----------------|-------|
| Hook handler (local embedded) | <1ms | In-process SurrealDB, no socket |
| Daemon connection | ~1-2ms | Unix socket, local only |
| Simple task query (daemon) | ~2-5ms | Single table lookup with index |
| Vector similarity search (daemon) | ~5-15ms | HNSW index, 384 dimensions |
| Cross-project graph + vector query | ~10-30ms | Graph traversal + vector filter |
| Embedding generation | ~5-10ms | Local ONNX, all-MiniLM-L6-v2 |
| Full sync cycle (100 events) | ~500ms | Ledger parse + upsert + embed |

### Scale Targets

The global daemon is designed for developer team scale, not enterprise scale:

| Metric | Target |
|--------|--------|
| Registered projects | Up to 20 |
| Total tasks across all projects | Up to 100,000 |
| Total sessions | Up to 50,000 |
| Concurrent connected clients | Up to 10 |
| Vector index size | Up to 500,000 embeddings |

Beyond these targets, performance degrades gracefully (slower queries, not failures). Enterprise scale is deferred to a future hosted tier.

### Multi-User Sync Architecture

The daemon-based architecture above (Unix socket SurrealDB) handles cross-project queries for a single developer. For multi-developer teams, a synchronization mechanism is needed to share knowledge graph entities and database state across machines.

**Team Size Decision Matrix:**

| Team Size | Sync Mechanism | Database | Infrastructure Cost |
|-----------|-------------------|----------|---------------------|
| Solo | N/A -- single embedded DB | surrealkv:// | Zero |
| Small team (2-5) | Loro CRDT via git refs | Each dev: local surrealkv://, KG entities synced via Loro | Zero -- uses existing git |
| Medium team (5-15) | Loro CRDT + optional daemon | Local surrealkv:// + optional daemon for shared queries | Zero to minimal |
| Large team (15+) | SurrealDB TiKV cluster (self-hosted or cloud) | Shared TiKV-backed SurrealDB | Substantial -- 7+ servers |
| Enterprise (50+) | SurrealDB Cloud Dedicated | Managed multi-node cluster | Cloud pricing |

**Loro CRDT as primary sync (teams < 15):**

For small and medium teams, the Loro CRDT infrastructure already designed for parallel execution coordination (claims, file ownership -- see [Decision #8](decisions.md#8-loro-crdt-as-foundation)) is extended to handle knowledge graph entity/relationship sync. Entities and relationships are small records (< 1KB each) mapped to LoroMap containers. Sync uses the same git ref transport (refs/coordination/loro/{peer-id}, 30s interval). Deterministic merge means no conflict resolution needed.

**Embeddings are local-only.** Each developer's machine regenerates embeddings locally via ONNX (all-MiniLM-L6-v2). Same model + same text = identical embeddings. Embeddings are NOT synced -- they are too large for CRDT (1.5KB per 384-dim embedding per entity) and can be computed deterministically.

**TiKV is only needed at 15+ developers.** TiKV itself is 100% free (Apache 2.0, CNCF graduated), but the infrastructure cost is substantial: minimum 7 servers (3 TiKV + 3 PD + 1 monitoring), each requiring 16+ cores, 32+ GB RAM, 200+ GB NVMe. This is impractical for CodeFlow's target audience (small dev teams, indie developers).

See [Knowledge Graph Engine Analysis, Section 8](knowledge-graph-engine.md#8-multi-user-synchronization-architecture) and [Decision #23](decisions.md#23-knowledge-graph-synchronization) for the full analysis.

---

## 11. CLI Commands

### New Commands Added by Epic C

**Daemon management:**

```bash
codeflow db daemon start           # Start global daemon in background
codeflow db daemon stop            # Graceful shutdown
codeflow db daemon status          # Show status, connected clients, data size
codeflow db install-service        # Register launchd/systemd service
codeflow db uninstall-service      # Remove service registration
codeflow db rebuild                # Drop and replay all JSONL to rebuild global DB
```

**Project management:**

```bash
codeflow project register          # Register current project with global daemon
codeflow project link <name>       # Link another project (creates can_access relation)
codeflow project unlink <name>     # Remove cross-project link
codeflow project list              # Show all registered projects and sync status
```

**Context search (enhanced):**

```bash
codeflow context search "API authentication decisions"
# Returns: top 5 semantically similar tasks/decisions across all visible projects
# Options: --limit N, --scope tasks|sessions|memory, --project <name>
```

**Doctor (extended):**

```bash
codeflow doctor
# Existing checks + new global checks:
#   - Daemon running? (yes/no, PID)
#   - Socket accessible? (~/.codeflow/db.sock)
#   - Registered projects? (count, last sync time per project)
#   - Sync cursor fresh? (lag in minutes)
#   - Embedding model present? (~50MB, path)
```

---

## 12. Migration Path

### Enabling Global Mode

1. **Register project:** `codeflow project register` — creates a project record in `~/.codeflow/registry.toml` with auto-generated `project_id`.

2. **Start daemon:** `codeflow db daemon start` — starts SurrealDB server on `~/.codeflow/db.sock`. First run creates `~/.codeflow/data/` and performs initial schema migration.

3. **Initial sync:** `codeflow db rebuild` (first time) or sync runs automatically on daemon start. Reads all JSONL ledger events and populates the global database.

4. **Ongoing sync:** The daemon's background sync loop runs every 30 seconds (configurable). New ledger events are imported automatically.

### Linking Projects

After registering multiple projects:

```bash
cd ~/projects/codeflow-frontend
codeflow project link codeflow-backend
# Creates: project:frontend ->can_access-> project:backend
```

### Fallback to Local

If the daemon is stopped or unavailable:

- All project-local operations continue without change
- Global queries fall back to local project data with a warning
- No configuration change required — fallback is automatic

---

## 13. Epic C Task Outline

Epic C: Global Intelligence Layer (14-18 tasks across 3 phases)

**Dependency:** Epic 0 (Rust CLI redesign) must be complete. Epic C adds new subcommands and SurrealDB server-mode integration to the Rust binary.

### Phase G1: Infrastructure

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 1 | Daemon subcommand | `codeflow db daemon start/stop/status`. SurrealDB server mode within the Rust binary. PID file management. | L |
| 2 | Unix socket configuration | Socket at `~/.codeflow/db.sock`. Connection pooling for daemon clients. Timeout and fallback logic. | M |
| 3 | Service installer | `codeflow db install-service` / `uninstall-service`. launchd plist (macOS) + systemd unit (Linux). Auto-start on login. | M |
| 4 | Global config and registry | `~/.codeflow/config.toml` schema. `~/.codeflow/registry.toml` for project list. Config parsing in Rust. | S |

### Phase G2: Project Management

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 5 | Project registration | `codeflow project register`. Auto-generate project_id. Write to registry. Create project record in daemon. | M |
| 6 | Cross-project linking | `codeflow project link/unlink`. `can_access` graph relations. Visibility enforcement in queries. | M |
| 7 | Sync engine — ledger replay | Background sync loop. Per-project sync cursors. JSONL parse + upsert to daemon. 30s interval. | L |
| 8 | Sync engine — schema | SurrealDB SCHEMAFULL tables for task, epic, session, event, memory. Project-scoped records. | M |
| 9 | Rebuild command | `codeflow db rebuild`. Drop + replay all JSONL for all registered projects. Progress reporting. | M |

### Phase G3: Intelligence

| # | Task | Description | Effort |
|---|------|-------------|--------|
| 10 | Local embeddings | `ort` crate integration. all-MiniLM-L6-v2 model (384d). Generate embeddings on sync. Model download on first run. | L |
| 11 | Vector index | HNSW index on task/session/memory tables. DEFINE INDEX in SurrealQL. | S |
| 12 | Context search command | `codeflow context search`. Vector + graph query in single SurrealQL statement. Cross-project visibility enforcement. Local fallback if daemon unavailable. | L |
| 13 | Doctor global diagnostics | Extend `codeflow doctor` with global checks: daemon status, socket, registered projects, sync lag, embedding model. | M |

**Note:** Tasks C-11 through C-14 (Loro KG sync, configuration system, bootstrap CLI, trigger points) are defined in [Knowledge Graph Engine Analysis, Section 5](knowledge-graph-engine.md#5-implementation-recommendations) and will be added to this outline when Epic C planning is finalized.
