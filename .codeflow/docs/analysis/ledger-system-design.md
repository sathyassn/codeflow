---
title: "JSONL Ledger System Design"
type: analysis
status: active
author: cf-documentation
created_at: "2026-03-24"
updated_at: "2026-03-24"
area: infrastructure
scope:
  - codeflow-cli/core/src/ledger/mod.rs
  - codeflow-cli/core/src/ledger/jsonl.rs
  - codeflow-cli/core/src/ledger/routing.rs
  - codeflow-cli/core/src/ledger/compact.rs
  - codeflow-cli/core/src/ledger/rebuild.rs
  - codeflow-cli/core/src/ledger/migrate.rs
  - codeflow-cli/core/src/worktree/mod.rs
---

# JSONL Ledger System Design

The JSONL ledger is Tier 0 in CodeFlow's three-tier data model. It is the
immutable, append-only event log that serves as the authoritative rebuild
source for the SurrealDB database. This document describes the ledger
architecture after the worktree ledger isolation fix (PR #221, INF-TSK-024-030).

## Table of Contents

- [1. Three-Tier Data Model](#1-three-tier-data-model)
- [2. Directory Layout](#2-directory-layout)
- [3. Event Types and Routing](#3-event-types-and-routing)
- [4. Write Path](#4-write-path)
- [5. Session-Scoped Isolation](#5-session-scoped-isolation)
- [6. Worktree Integration](#6-worktree-integration)
- [7. Compaction Algorithm](#7-compaction-algorithm)
- [8. Rebuild Algorithm](#8-rebuild-algorithm)
- [9. Migration: Flat to Subdirectory](#9-migration-flat-to-subdirectory)
- [10. CLI Commands](#10-cli-commands)
- [11. Git Tracking](#11-git-tracking)
- [12. Env File Propagation](#12-env-file-propagation)
- [13. Key Files Reference](#13-key-files-reference)
- [14. Cross-References](#14-cross-references)

---

## 1. Three-Tier Data Model

CodeFlow uses a three-tier model for persistent state:

| Tier | Location | Purpose | Git Tracked |
|------|----------|---------|-------------|
| **0 (JSONL)** | `.state/ledger/` | Rebuild authority -- immutable, append-only event log | Yes |
| **1 (SurrealDB)** | `.state/db/codeflow.db` | Query interface -- fast indexed lookups | No |
| **2 (Markdown)** | `project-management/`, `.claude/memory/` | Human-readable derived views | Yes |

**Key principle:** If Tier 1 (database) is lost or corrupted, rebuild it from
Tier 0 (JSONL). Tier 2 (markdown) is always derived from Tier 1. JSONL is the
ultimate source of truth and is committed to git.

---

## 2. Directory Layout

The ledger uses a **subdirectory layout** introduced in INF-TSK-024-030. Each
canonical type gets its own subdirectory. Within that subdirectory, two file
kinds exist: a base file and session fragment files.

```text
.state/ledger/
├── work-graph/
│   ├── work-graph.jsonl                 ← base file (compacted history)
│   └── work-graph-ses-{session_id}.jsonl ← session fragment (active writes)
├── sessions/
│   ├── sessions.jsonl
│   └── sessions-ses-{session_id}.jsonl
├── memory-events/
│   ├── memory-events.jsonl
│   └── memory-events-ses-{session_id}.jsonl
├── config/
│   ├── config.jsonl
│   └── config-ses-{session_id}.jsonl
├── pathflow-events/
│   ├── pathflow-events.jsonl
│   └── pathflow-events-ses-{session_id}.jsonl
├── coordination-events/
│   ├── coordination-events.jsonl
│   └── coordination-events-ses-{session_id}.jsonl
└── autorun-events/
    ├── autorun-events.jsonl
    └── autorun-events-ses-{session_id}.jsonl
```

**Base file naming:** `{type}/{type}.jsonl`

**Session fragment naming:** `{type}/{type}-{session_id}.jsonl`

The session ID portion of the fragment filename matches the full session ID
(e.g., `ses-177137202131769e89b2d5688`), not just a numeric suffix. This is
because the `JsonlWriter` formats the fragment name as
`{type_name}-{sid}.jsonl` where `sid` is the complete session ID string.

Source: `codeflow-cli/core/src/ledger/jsonl.rs:81`.

---

## 3. Event Types and Routing

The ledger defines 7 canonical types. Each type maps to a subdirectory. Events
are routed to the correct type based on their `event_type` field.

### 3.1 Canonical Type Constants

Defined in `codeflow-cli/core/src/ledger/mod.rs:22-44`:

| Constant | Value | Directory |
|----------|-------|-----------|
| `files::WORK_GRAPH` | `"work-graph"` | `.state/ledger/work-graph/` |
| `files::MEMORY_EVENTS` | `"memory-events"` | `.state/ledger/memory-events/` |
| `files::SESSIONS` | `"sessions"` | `.state/ledger/sessions/` |
| `files::CONFIG` | `"config"` | `.state/ledger/config/` |
| `files::PATHFLOW_EVENTS` | `"pathflow-events"` | `.state/ledger/pathflow-events/` |
| `files::COORDINATION_EVENTS` | `"coordination-events"` | `.state/ledger/coordination-events/` |
| `files::AUTORUN_EVENTS` | `"autorun-events"` | `.state/ledger/autorun-events/` |

**`files::CANONICAL`** (4 types) — synced to SurrealDB on rebuild:
`work-graph`, `memory-events`, `sessions`, `config`

**`files::ALL`** (7 types) — all types including non-DB types:
adds `pathflow-events`, `coordination-events`, `autorun-events`

### 3.2 Event Routing Table

Routing is implemented in `codeflow-cli/core/src/ledger/routing.rs` by the
`route_event_type()` function:

| Target | Event Types |
|--------|-------------|
| `sessions` | `session_start`, `session_end`, `session_progress`, `work_claimed`, `claim_created`, `claim_released`, `claim_renewed` |
| `work-graph` | `epic_created`, `epic_status_changed`, `task_created`, `task_status_changed`, `task_updated`, `task_id_corrected`, `task_cancelled`, `begin_work`, `complete_work`, `work_complete`, `commit`, `pr_created`, `pr_merged`, `work_finding`, `void`, `work_cancelled`, `stale_work_cleanup` |
| `memory-events` | `memory_store`, `memory_stored`, `milestone`, `progress`, `finding`, `decision`, `session_summary`, `memory_event`, `memory_milestone`, `blocker` |
| `config` | `config_set`, `config_updated` |
| `pathflow-events` | `phase_transition`, `stage_transition`, `session_register`, `session_metadata`, `pathflow_task_update` |
| `coordination-events` | `claim_acquired`, `claim_conflict`, `coord_claim_released`, `scope_expansion`, `merge_conflict_detected`, `merge_rebase_attempted` |
| `autorun-events` | `batch_started`, `batch_completed`, `batch_aborted`, `worker_started`, `worker_completed`, `worker_failed`, `worker_timeout`, `worker_blocked`, `worker_cancelled` |

Unknown event types return `LedgerError::UnknownEventType`. Misrouted events
(correct type but wrong target file) return `LedgerError::MisroutedEvent`.

### 3.3 Event Structure

Every ledger event serializes to a single JSON line (`codeflow-cli/core/src/ledger/mod.rs:86-107`):

```json
{
  "event": "task_created",
  "timestamp": "2026-03-24T12:00:00Z",
  "session_id": "ses-177137202131769e89b2d5688",
  "worktree": "/path/to/.git-worktrees/worktree-ses-...",
  "format_id": "INF-TSK-024-030",
  ...additional fields flattened...
}
```

- `event` — serialized from the `event_type` field (renamed by serde)
- `session_id` — omitted when `None` (skip_serializing_if)
- `worktree` — omitted when `None` (skip_serializing_if); present for worktree-originated events
- `data` fields — flattened into the top-level object

---

## 4. Write Path

The `JsonlWriter` struct (`codeflow-cli/core/src/ledger/jsonl.rs`) implements
the `LedgerWriter` trait and handles all writes to the JSONL ledger.

### 4.1 Write Path Flow

```text
Event (event_type + timestamp + session_id + data)
        |
        v
LedgerWriter::append_event(event)
        |
        v
route_event_type(event.event_type)
  → Returns canonical type name (e.g., "work-graph")
        |
        v
session_scoped_path(type_name)
  → If session_id = Some(sid):
      .state/ledger/{type}/{type}-{sid}.jsonl   ← fragment
  → If session_id = None:
      .state/ledger/{type}/{type}.jsonl          ← base
        |
        v
create_dir_all(subdirectory)     ← ensures subdir exists
        |
        v
open lock_path ({file}.jsonl.lock)
flock(LOCK_EX)                   ← exclusive lock via fs2 crate
        |
        v
serde_json::to_vec(event) + '\n' ← single JSON line
        |
        v
OpenOptions { append, create }.open(file_path)
write_all(line)                  ← O_APPEND atomic semantics
        |
        v
drop(lock_file)                  ← lock released on drop
```

**Concurrency model:** Multiple writers (hook processes) can safely append to
the same fragment file. The `flock(LOCK_EX)` on the `.jsonl.lock` sidecar file
serializes writes. The data file is opened with `O_APPEND`, which provides
kernel-level atomic append semantics on POSIX systems.

Lock files are named `{file}.jsonl.lock` and stored alongside the data file in
the same subdirectory.

Source: `codeflow-cli/core/src/ledger/jsonl.rs:86-128`.

---

## 5. Session-Scoped Isolation

### 5.1 Why Fragments Prevent Merge Conflicts

Before this fix, all sessions wrote to the same base files under `.state/ledger/`.
In worktree mode, the ledger directory was symlinked (shared), meaning writes
from worktree A went to the main repo's working tree. This caused two problems:

1. Events from parallel sessions interleaved in the same files.
2. Ledger files modified in the worktree appeared in the main repo's `git status`.

After INF-TSK-024-030, each session writes to its own fragment file:

```text
work-graph/work-graph-ses-17713720213...abc.jsonl  ← session A
work-graph/work-graph-ses-17713720213...def.jsonl  ← session B
```

Because fragment filenames embed the session ID, no two sessions write to the
same file. No merge conflict is possible between parallel sessions for active
writes. When both sessions' changes are merged into `main`, git sees two new
files (both append-only), and the merge is clean.

### 5.2 Writer Construction

```rust
// Base writes (no session scope):
JsonlWriter::new(ledger_dir)
// Session-scoped writes (go to fragment):
JsonlWriter::new_with_session(ledger_dir, Some(session_id.to_string()))
```

Source: `codeflow-cli/core/src/ledger/jsonl.rs:37-71`.

During an active session, hooks construct a `JsonlWriter` with the current
session ID, so all writes land on the session's fragment. Base file writes
only occur during compaction.

---

## 6. Worktree Integration

### 6.1 Ledger is LOCAL, Not SHARED

After INF-TSK-024-030, the ledger directory is **LOCAL** to each worktree
(a real directory, not a symlink). This is in contrast to the database,
coordination state, and logs, which remain shared (symlinked).

Defined in `codeflow-cli/core/src/worktree/mod.rs:43-58`:

| Category | Directories | Storage in Worktree |
|----------|------------|---------------------|
| **SHARED** (symlinked) | `db`, `coordination`, `logs`, `registry`, `backups`, `worktrees` | Symlink → main repo `.state/{dir}/` |
| **LOCAL** (real dir) | `runtime`, `session`, `sentinels`, **`ledger`** | Real directory, per-worktree copy |

**Why ledger is LOCAL:** Each session writes session-scoped fragment files to
the ledger. Because fragment filenames are unique per session, there is no
write collision between parallel sessions. Keeping ledger local means ledger
writes appear in the worktree's `git status`, not the main repo's. At PR merge
time, fragment files from the feature branch are merged normally (they are
new files, no conflicts).

**Why ledger was previously SHARED (incorrect):** The original design shared
the ledger via symlink alongside the database. This worked for single-session
mode but broke parallel worktree mode because writes from the worktree would
appear in the main repo's working tree, polluting `git status`.

### 6.2 Worktree `.state/` Layout with LOCAL Ledger

```text
.git-worktrees/worktree-{SID}/
└── .state/
    ├── db/             → symlink → ../../.state/db/           (SHARED)
    ├── coordination/   → symlink → ../../.state/coordination/ (SHARED)
    ├── logs/           → symlink → ../../.state/logs/         (SHARED)
    ├── registry/       → symlink → ../../.state/registry/     (SHARED)
    ├── backups/        → symlink → ../../.state/backups/      (SHARED)
    ├── worktrees/      → symlink → ../../.state/worktrees/    (SHARED)
    ├── runtime/                                                (LOCAL)
    ├── session/                                                (LOCAL)
    ├── sentinels/                                              (LOCAL)
    └── ledger/                                                 (LOCAL)
        ├── work-graph/
        │   └── work-graph-ses-{SID}.jsonl
        ├── sessions/
        │   └── sessions-ses-{SID}.jsonl
        └── ...
```

---

## 7. Compaction Algorithm

Compaction merges completed session fragment files into the base file for each
ledger type. Only sessions that have a `session_end` event are eligible for
compaction. Active sessions and the current session are always skipped.

### 7.1 Compaction Flow

```text
compact_ledger_type(ledger_dir, type_name, current_session_id)
        |
        v
Check subdir exists
  → If missing: return empty CompactionResult
        |
        v
open .compaction.lock
try_lock_exclusive()          ← non-blocking; error if already locked
        |
        v
find_completed_sessions(ledger_dir)
  → Scan sessions/sessions.jsonl + all sessions-ses-*.jsonl
  → Collect session IDs with a session_end event
        |
        v
compactable_fragments(subdir, type_name, completed_sessions, current_session_id)
  → Find {type}-ses-*.jsonl fragment files
  → Skip current session fragment
  → Skip fragments whose session has no session_end (active)
  → Return list of compactable fragment paths
        |
        v
If no fragments: return early (zero merged)
        |
        v
read_events_from_file(base_path)      ← read existing base events
for each fragment: read_events_from_file(path)
        |
        v
sort by timestamp (stable sort)       ← preserves intra-file order
        |
        v
write to {type}.jsonl.tmp
  → serialize each event to single JSON line
  → sync_all() to flush to disk
        |
        v
fs::rename(tmp_path, base_path)       ← atomic replace
        |
        v
for each compacted fragment:
  fs::remove_file(path)               ← delete fragment
  fs::remove_file(path.jsonl.lock)    ← delete lock file
        |
        v
drop(.compaction.lock)                ← lock released on drop
        |
        v
return CompactionResult { merged_count, deleted_files, skipped_active }
```

**Atomic write guarantee:** The base file is written to a `.tmp` file first,
then atomically renamed. If the process crashes during the write, the original
base file is unaffected.

**`compact_all()`** iterates all 7 types in `files::ALL`.

Source: `codeflow-cli/core/src/ledger/compact.rs`.

---

## 8. Rebuild Algorithm

Rebuild reconstructs the full event stream for a ledger type from base and
all fragment files. This is used to replay Tier 0 data into Tier 1 (SurrealDB).

```text
rebuild_ledger_type(ledger_dir, type_name)
        |
        v
Check {type_name}/ subdir exists
  → If missing: return empty Vec
        |
        v
Read base file: {type}/{type}.jsonl (if exists)
  → Parse each non-empty line as Event
  → Skip corrupt lines (log to stderr)
        |
        v
Glob fragment files: {type}/{type}-ses-*.jsonl
  → Excludes .lock files
  → Reads each file the same way
        |
        v
Sort all events by timestamp (stable sort)
  → Preserves intra-file insertion order for equal timestamps
        |
        v
Return Vec<Event>
```

**`rebuild_all()`** iterates all 7 types in `files::ALL` and returns
`HashMap<String, Vec<Event>>` (empty types omitted).

Source: `codeflow-cli/core/src/ledger/rebuild.rs`.

---

## 9. Migration: Flat to Subdirectory

The `migrate_flat_to_subdirs()` function converts a flat layout (all `.jsonl`
files directly in `.state/ledger/`) to the subdirectory layout.

### 9.1 Flat Layout (Pre-Migration)

```text
.state/ledger/
├── work-graph.jsonl
├── sessions.jsonl
├── memory-events.jsonl
├── config.jsonl
├── pathflow-events.jsonl
├── coordination-events.jsonl
└── autorun-events.jsonl
```

### 9.2 Migration Behavior

For each type in `files::ALL`:

1. Check if `{type}/{type}.jsonl` already exists — if so, skip (idempotent).
2. Create subdirectory `{type}/`.
3. Move `{type}.jsonl` → `{type}/{type}.jsonl` (preserves content).
4. Move `{type}.jsonl.lock` → `{type}/{type}.jsonl.lock` (if exists).

If neither flat files nor subdirectories exist, creates empty subdirectories.
Returns `MigrationResult { migrated_count, already_migrated }`.

**Idempotency:** Safe to run multiple times. Already-migrated types are skipped
without modification. Partial migrations resume from where they left off.

### 9.3 Auto-Migration at SessionStart

Migration runs automatically at SessionStart for the `source=startup` path.
The hook calls `migrate_flat_to_subdirs()` before writing any ledger events.
This ensures new sessions always write to the subdirectory layout.

Source: `codeflow-cli/core/src/hooks/session_start.rs:885`.

Source: `codeflow-cli/core/src/ledger/migrate.rs`.

---

## 10. CLI Commands

The `codeflow ledger` subcommand provides operational visibility and control
over the JSONL ledger. Implemented in `codeflow-cli/cli/src/cmd/ledger.rs`.

| Command | Description |
|---------|-------------|
| `codeflow ledger` | Alias for `status` (default subcommand) |
| `codeflow ledger status` | Show fragment count and event count per type |
| `codeflow ledger compact` | Compact all completed fragments into base files |
| `codeflow ledger compact --type work-graph` | Compact a single type only |
| `codeflow ledger rebuild` | Read and print all events (base + fragments) sorted by timestamp |
| `codeflow ledger rebuild --type sessions` | Rebuild a single type only |
| `codeflow ledger migrate` | Migrate flat layout to subdirectory layout (idempotent) |

### Status Output Format

```text
TYPE                     FRAGS       BASE      TOTAL
------------------------------------------------------
work-graph                   2        147        153
sessions                     1         42         44
memory-events                0         12         12
...
```

Columns: type name, active fragment count, base file event count, total events.

---

## 11. Git Tracking

### What Is Tracked

JSONL ledger files are committed to git. This is intentional: the ledger is
the authoritative event history. Committing it enables:

- Auditable history of all work events
- Database reconstruction from any git checkout
- Cross-session event merging at PR merge time

In worktree mode, session fragment files appear in the worktree's `git status`
and are committed as part of the feature branch. At PR merge, they become part
of the main branch's event history.

### What Is Not Tracked

- Lock files (`*.jsonl.lock`) — ephemeral per-write artifacts
- The SurrealDB file (`.state/db/codeflow.db`) — rebuilt from JSONL, not tracked
- Worktree directories (`.git-worktrees/`) — gitignored

---

## 12. Env File Propagation

The `CODEFLOW_WORKTREE_PATH` variable enables hooks and the CLI to locate the
correct worktree and its LOCAL state (including the ledger). Three layers
ensure the variable propagates correctly.

### 12.1 Three-Layer Defense

```text
Layer 1: SessionStart (source=startup)
  write_env_file_with_worktree(worktree_path)
  → writes to {worktree}/.state/runtime/codeflow-env.sh
  → contains CODEFLOW_WORKTREE_PATH={worktree_path}
        |
        v
Layer 2: SessionStart (source=compact / source=resume)
  Re-reads codeflow-env.sh from existing session
  Re-exports CODEFLOW_WORKTREE_PATH via result.EnvVars
  → hooks executed after compact/resume have the correct path
        |
        v
Layer 3: Main repo env redirect
  After worktree creation, also writes CODEFLOW_WORKTREE_PATH
  into the main repo's .state/runtime/codeflow-env.sh
  → hooks running from main repo CWD (not the worktree) can
    find the worktree via the main repo's env file fallback
```

Source: `codeflow-cli/core/src/hooks/session_start.rs:435`, `:451`, `:1335`, `:1460`.

### 12.2 env File Location

| Context | env File Path |
|---------|--------------|
| Main repo (no worktree) | `.state/runtime/codeflow-env.sh` |
| Worktree session | `.git-worktrees/worktree-{SID}/.state/runtime/codeflow-env.sh` |
| Main repo (worktree active) | `.state/runtime/codeflow-env.sh` (contains `CODEFLOW_WORKTREE_PATH`) |

### 12.3 env File Contents

```bash
export CODEFLOW_SESSION_ID='ses-177137202131769e89b2d5688'
export CF_PROJECT_ROOT='codeflow'
export CODEFLOW_WORKTREE_PATH='/path/to/.git-worktrees/worktree-ses-...'
```

`CODEFLOW_WORKTREE_PATH` is omitted when not in worktree mode.

---

## 13. Key Files Reference

| File | Location | Purpose |
|------|----------|---------|
| `mod.rs` | `codeflow-cli/core/src/ledger/mod.rs` | Constants (`files::*`), `Event` struct, `LedgerWriter` trait, `CompactionResult`, `MigrationResult` |
| `jsonl.rs` | `codeflow-cli/core/src/ledger/jsonl.rs` | `JsonlWriter`: session-scoped path resolution, flock-based writes |
| `routing.rs` | `codeflow-cli/core/src/ledger/routing.rs` | `route_event_type()`: maps event type strings to canonical type names |
| `compact.rs` | `codeflow-cli/core/src/ledger/compact.rs` | `compact_ledger_type()`, `compact_all()`: merge completed fragments |
| `rebuild.rs` | `codeflow-cli/core/src/ledger/rebuild.rs` | `rebuild_ledger_type()`, `rebuild_all()`: reconstruct sorted event stream |
| `migrate.rs` | `codeflow-cli/core/src/ledger/migrate.rs` | `migrate_flat_to_subdirs()`: one-time idempotent migration |
| `worktree/mod.rs` | `codeflow-cli/core/src/worktree/mod.rs:43-58` | `SHARED_STATE_DIRS`, `LOCAL_STATE_DIRS` constants |
| `ledger.rs` | `codeflow-cli/cli/src/cmd/ledger.rs` | CLI: `status`, `compact`, `rebuild`, `migrate` subcommands |
| `session_start.rs` | `codeflow-cli/core/src/hooks/session_start.rs:885` | Auto-migration call at startup |

---

## 14. Cross-References

| Document | Relationship |
|----------|-------------|
| [CLAUDE.md Section 10](../../../.claude/CLAUDE.md) | Three-tier data model overview; ledger as Tier 0 |
| [worktree-architecture.md](parallel-work/worktree-architecture.md) | SHARED vs LOCAL state split; now updated: ledger is LOCAL |
| [parallel-work-config-spec.md](parallel-work/parallel-work-config-spec.md) | Parallel work configuration schema |
| [session-lifecycle-unified.md](session-lifecycle-unified.md) | Full session lifecycle including SessionStart/SessionEnd |
| [data-layer-protection.md](parallel-work/data-layer-protection.md) | Database and coordination layer parallel safety |
