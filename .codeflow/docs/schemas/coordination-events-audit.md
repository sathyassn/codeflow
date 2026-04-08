---
title: "coordination-events.jsonl Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-07"
updated_at: "2026-04-07"
scope: INF-TSK-024-033
feeds_into:
  - INF-TSK-024-007
---

# coordination-events.jsonl Schema Audit

This document audits the schema of `.state/ledger/coordination-events/` across all 6
`CoordinationEvent` variants emitted by the CRDT coordination subsystem. It documents
the tag format difference from `LedgerEvent`, maps JSONL fields against the existing
SurrealDB `coordination_event` table, and assesses Loro CRDT state rebuild viability.
It feeds directly into INF-TSK-024-007 (canonical event schema synthesis).

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Serialization Format and Tag Difference](#2-serialization-format-and-tag-difference)
- [3. Event Type Inventory](#3-event-type-inventory)
- [4. Field Inconsistencies Catalog](#4-field-inconsistencies-catalog)
- [5. Variable vs Universal Fields](#5-variable-vs-universal-fields)
- [6. CRDT Relationship and Rebuild Viability](#6-crdt-relationship-and-rebuild-viability)
- [7. files::CANONICAL Exclusion Rationale](#7-filescanonical-exclusion-rationale)
- [8. SurrealDB Gap Analysis](#8-surrealdb-gap-analysis)
- [9. BEFORE/AFTER JSON Examples](#9-beforeafter-json-examples)
- [10. Rust Type Reference](#10-rust-type-reference)

---

## 1. File Location and Layout

**Current (subdirectory layout):** `.state/ledger/coordination-events/`

```text
.state/ledger/
└── coordination-events/
    ├── coordination-events.jsonl             ← base (compacted history)
    └── coordination-events-ses-{id}.jsonl    ← per-session fragment
```

The subdirectory layout follows the same `JsonlWriter` pattern used by all other
ledger types. When `session_id` is `Some`, events write to the session fragment file.
When `None`, events write to the base file.

**Writer:** `JsonlWriter::append_event()` in `codeflow-cli/core/src/ledger/jsonl.rs`,
routing via `codeflow-cli/core/src/ledger/routing.rs`.

**Event types routed to `coordination-events`** (from `routing.rs:58-64`):

```text
claim_acquired | claim_conflict | coord_claim_released | scope_expansion
merge_conflict_detected | merge_rebase_attempted
```

**Empirical data availability:** The `.state/ledger/coordination-events/` directory
exists but is currently empty. No parallel sessions have triggered claim events in this
repository. All schema documentation is derived from the typed Rust structs in
`codeflow-cli/core/src/coordination/types/events.rs`.

**Related state files (`.state/coordination/`):**

| File | Purpose |
|------|---------|
| `state.loro` | Loro CRDT binary snapshot (claims, fencing tokens, merge queue) |
| `state.json` | JSON representation of current coordination state |
| `state.loro.lock` | Advisory lock file for concurrent access |
| `sync-state.json` | Sync daemon state (last sync time, peer list) |

---

## 2. Serialization Format and Tag Difference

### 2.1 CoordinationEvent tag format

`CoordinationEvent` uses `#[serde(tag = "type")]`, making the discriminant field `"type"`:

```rust
#[serde(tag = "type")]
pub enum CoordinationEvent {
    #[serde(rename = "claim_acquired")]
    ClaimAcquired { ... },
    // ...
}
```

Serialized output (the `"type"` key carries the variant name):

```json
{"type": "claim_acquired", "session_id": "ses-01k...", "path": "src/main.rs", "timestamp": "..."}
```

### 2.2 LedgerEvent tag format (contrast)

`LedgerEvent` (used by sessions, work-graph, memory-events, etc.) uses `#[serde(tag = "event")]`,
making the discriminant field `"event"`:

```rust
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LedgerEvent { ... }
```

Serialized output:

```json
{"event": "session_start", "session_id": "ses-01k...", "timestamp": "..."}
```

### 2.3 Implication for readers

Any reader that assumes the discriminant field is `"event"` will fail to deserialize
`CoordinationEvent` JSON. Generic JSONL readers and tools that look for `"event"` as the
event type field must handle `"type"` as an alternative for coordination event records.

**Gap for downstream tasks:** INF-TSK-024-007 (canonical event schema synthesis) must
decide whether to normalize coordination events to use `"event"` as the discriminant or
document `"type"` as an accepted alternative. The current schema is internally consistent
but diverges from the rest of the ledger.

---

## 3. Event Type Inventory

All 6 variants are defined in `codeflow-cli/core/src/coordination/types/events.rs:17-82`.
Common fields across all variants:

| Field | Type | Required | Serialization rule |
|-------|------|----------|--------------------|
| `type` | string (discriminant) | Yes | Always present (serde tag) |
| `session_id` | `SessionId` | Yes | Always present |
| `timestamp` | string (RFC 3339) | Yes | Always present |
| `task_id` | `Option<String>` | No | Omitted when `None` (`skip_serializing_if`) |

### 3.1 ClaimAcquired

**Discriminant value:** `"claim_acquired"`

Emitted when a file claim is successfully acquired by a session. Written by
`Coordinator::acquire()` (via `codeflow-cli/core/src/coordination/loro.rs`).

| Field | Type | Required | Example |
|-------|------|----------|---------|
| `type` | `"claim_acquired"` | Yes | `"claim_acquired"` |
| `session_id` | `SessionId` | Yes | `"ses-01kjw2gertbnn8xnpmr8mcrmc8"` |
| `path` | `String` | Yes | `"src/main.rs"` |
| `task_id` | `Option<String>` | No | `"INF-TSK-024-033"` or omitted |
| `timestamp` | `String` | Yes | `"2026-03-21T10:00:00Z"` |

**CRDT operation:** Corresponds to `LoroMap::insert()` in the `"claims"` container,
setting a claim entry for the given path with the session's identity and TTL.

### 3.2 ClaimConflict

**Discriminant value:** `"claim_conflict"`

Emitted when claim acquisition is blocked because another session already holds the file.
Written by `try_acquire_claim()` in `pre_tool_use.rs` when `Coordinator::acquire()` returns
a conflict result.

| Field | Type | Required | Example |
|-------|------|----------|---------|
| `type` | `"claim_conflict"` | Yes | `"claim_conflict"` |
| `session_id` | `SessionId` | Yes | `"ses-01k..."` (requesting session) |
| `path` | `String` | Yes | `"src/lib.rs"` |
| `held_by` | `SessionId` | Yes | `"ses-01j..."` (holding session) |
| `task_id` | `Option<String>` | No | Omitted if no active task |
| `timestamp` | `String` | Yes | `"2026-03-21T10:01:00Z"` |

**CRDT operation:** This event is a diagnostic record. The CRDT state is not modified on
conflict — the claim remains held by the original holder. The `held_by` field captures
the incumbent owner for post-hoc analysis.

### 3.3 ClaimReleased

**Discriminant value:** `"coord_claim_released"`

Emitted when a file claim is explicitly released (e.g., on session end or
`claims::release_all()`). The `coord_` prefix distinguishes this from the Go-era
`claim_released` event that routes to `sessions.jsonl`.

| Field | Type | Required | Example |
|-------|------|----------|---------|
| `type` | `"coord_claim_released"` | Yes | `"coord_claim_released"` |
| `session_id` | `SessionId` | Yes | `"ses-01k..."` |
| `path` | `String` | Yes | `"src/main.rs"` |
| `task_id` | `Option<String>` | No | Omitted if no active task |
| `timestamp` | `String` | Yes | `"2026-03-21T10:02:00Z"` |

**Naming note:** The `coord_` prefix was added to avoid routing collisions with the Go-era
`claim_released` event type (which routes to `sessions.jsonl`). The routing table in
`routing.rs` maps each string exactly; a collision would misroute events.

**CRDT operation:** Corresponds to `LoroMap::delete()` (or equivalent `set null`) on the
claim entry in the `"claims"` container.

### 3.4 ScopeExpansion

**Discriminant value:** `"scope_expansion"`

Emitted when a worker dynamically acquires a file outside its pre-declared `file_scope`.
This occurs under `scope_policy=soft` when the file is unclaimed. Blocked under
`scope_policy=hard`.

| Field | Type | Required | Example |
|-------|------|----------|---------|
| `type` | `"scope_expansion"` | Yes | `"scope_expansion"` |
| `session_id` | `SessionId` | Yes | `"ses-01k..."` |
| `path` | `String` | Yes | `"src/extra.rs"` (newly claimed) |
| `original_scope` | `Vec<String>` | Yes | `["src/main.rs", "src/lib.rs"]` |
| `task_id` | `Option<String>` | No | Omitted if no active task |
| `timestamp` | `String` | Yes | `"2026-03-21T10:03:00Z"` |

**CRDT operation:** Same as `ClaimAcquired` — inserts a claim for the expanded-scope file.
The `original_scope` field provides an audit trail of what was pre-declared vs what was
actually touched.

### 3.5 MergeConflictDetected

**Discriminant value:** `"merge_conflict_detected"`

Emitted when `check_merge_conflicts()` detects that a branch cannot merge cleanly into
the target. Written during PF6-COMPLETE before PR creation.

| Field | Type | Required | Example |
|-------|------|----------|---------|
| `type` | `"merge_conflict_detected"` | Yes | `"merge_conflict_detected"` |
| `session_id` | `SessionId` | Yes | `"ses-01k..."` |
| `branch` | `String` | Yes | `"feat/my-feature"` |
| `target_branch` | `String` | Yes | `"main"` |
| `task_id` | `Option<String>` | No | `"INF-TSK-024-033"` or omitted |
| `timestamp` | `String` | Yes | `"2026-03-21T12:00:00Z"` |

**CRDT operation:** No direct CRDT state mutation. This event is a diagnostic record.
The merge conflict is resolved via `conflict::attempt_rebase()` which generates a
`MergeRebaseAttempted` event.

### 3.6 MergeRebaseAttempted

**Discriminant value:** `"merge_rebase_attempted"`

Emitted after `conflict::attempt_rebase()` completes, recording whether the rebase
succeeded or was aborted due to conflicts.

| Field | Type | Required | Example |
|-------|------|----------|---------|
| `type` | `"merge_rebase_attempted"` | Yes | `"merge_rebase_attempted"` |
| `session_id` | `SessionId` | Yes | `"ses-01k..."` |
| `branch` | `String` | Yes | `"feat/my-feature"` |
| `target_branch` | `String` | Yes | `"main"` |
| `success` | `bool` | Yes | `true` or `false` |
| `task_id` | `Option<String>` | No | `"INF-TSK-024-033"` or omitted |
| `timestamp` | `String` | Yes | `"2026-03-21T12:01:00Z"` |

**CRDT operation:** No direct CRDT state mutation. The `success` field records the
`RebaseResult` outcome from `git/conflict.rs`.

---

## 4. Field Inconsistencies Catalog

### 4.1 Discriminant field name mismatch

| Schema | Discriminant field | Example value |
|--------|--------------------|---------------|
| `CoordinationEvent` | `"type"` | `"claim_acquired"` |
| `LedgerEvent` (all other files) | `"event"` | `"session_start"` |

**Finding:** `coordination-events.jsonl` uses `"type"` as the JSON discriminant while all
other ledger files use `"event"`. This is the single most significant schema inconsistency
in the coordination events schema and requires special handling by any generic JSONL reader.

### 4.2 session_id required vs optional

| Context | session_id status |
|---------|------------------|
| `CoordinationEvent` struct fields | `SessionId` (required, no `Option<>`) |
| `Event` struct (general ledger wrapper) | `Option<String>` (`skip_serializing_if = "Option::is_none"`) |

**Finding:** `CoordinationEvent` variants declare `session_id` as a required `SessionId`
(non-optional) on every variant — unlike the `Event` struct's `Option<String>` pattern.
This means coordination events always carry a session identifier, providing stronger
traceability than the general ledger format.

### 4.3 task_id optional with omission

All `CoordinationEvent` variants carry `#[serde(skip_serializing_if = "Option::is_none")]`
on `task_id`. When no task is active (e.g., during non-tracked sessions), `task_id` is
absent from the JSON entirely.

**Finding:** Readers must handle absent `task_id` fields. This matches the pattern used
by other ledger types but is enforced at the Rust type level (not just convention).

### 4.4 No worktree field

`CoordinationEvent` has no `worktree` field. The general `Event` struct has an optional
`worktree: Option<String>` field. Coordination events do not record which worktree emitted
them — the `session_id` provides the worktree identity indirectly (each session maps to
one worktree in the registry).

---

## 5. Variable vs Universal Fields

### 5.1 Universal fields (present on all 6 event types)

| Field | Type | Notes |
|-------|------|-------|
| `type` | string (discriminant) | `#[serde(tag = "type")]` — present on all variants |
| `session_id` | `SessionId` | Required, non-optional on all variants |
| `timestamp` | `String` | RFC 3339 format; required on all variants |

### 5.2 Common optional field

| Field | Type | Present on |
|-------|------|-----------|
| `task_id` | `Option<String>` | All 6 variants; omitted when `None` |

### 5.3 Variant-specific fields

| Field | Type | Present on |
|-------|------|-----------|
| `path` | `String` | `claim_acquired`, `claim_conflict`, `coord_claim_released`, `scope_expansion` |
| `held_by` | `SessionId` | `claim_conflict` only |
| `original_scope` | `Vec<String>` | `scope_expansion` only |
| `branch` | `String` | `merge_conflict_detected`, `merge_rebase_attempted` |
| `target_branch` | `String` | `merge_conflict_detected`, `merge_rebase_attempted` |
| `success` | `bool` | `merge_rebase_attempted` only |

---

## 6. CRDT Relationship and Rebuild Viability

### 6.1 Loro CRDT state structure

The `LoroCoordinator` (in `codeflow-cli/core/src/coordination/loro.rs:84-92`) maintains a
`LoroDoc` with named containers:

| Container | Type | Purpose |
|-----------|------|---------|
| `"claims"` | `LoroMap` | File-level claims keyed by path |
| `"merge_queue"` | `LoroList` | Ordered merge queue for PR serialization |
| `"kg_entities"` | `LoroMap` | Reserved (Epic C knowledge graph) |
| `"kg_relationships"` | `LoroMap` | Reserved (Epic C) |
| `"kg_metadata"` | `LoroMap` | Reserved (Epic C) |
| `"extraction_queue"` | `LoroList` | Reserved (Epic C) |

The `state.loro` file is a Loro snapshot binary, persisted after every mutation by
`LoroCoordinator::persist()`. The `state.json` file provides a human-readable
representation of the same state.

### 6.2 Event-to-CRDT operation mapping

| Coordination event | CRDT operation | Container affected |
|-------------------|----------------|-------------------|
| `claim_acquired` | Insert claim entry | `"claims"` map |
| `coord_claim_released` | Delete/null claim entry | `"claims"` map |
| `scope_expansion` | Insert claim entry (expanded path) | `"claims"` map |
| `claim_conflict` | None (diagnostic only) | None |
| `merge_conflict_detected` | None (diagnostic only) | None |
| `merge_rebase_attempted` | None (diagnostic only) | None |

### 6.3 CRDT rebuild viability assessment

**Can `coordination-events.jsonl` serve as a rebuild source for `state.loro`?**

**Assessment: Partial — claims only, with significant limitations.**

The claim lifecycle events (`claim_acquired`, `coord_claim_released`, `scope_expansion`)
represent a partial audit trail of CRDT mutations. However, rebuilding `state.loro` from
these events faces fundamental obstacles:

| Obstacle | Detail |
|----------|--------|
| **Incomplete CRDT state** | `state.loro` stores the full Loro document including fencing tokens, merge queue entries, and Knowledge Graph containers. Coordination events only capture claim lifecycle — the merge queue and KG containers have no corresponding events. |
| **Fencing token loss** | Each claim is assigned a monotonically increasing `FencingToken`. The event log does not record fencing token values, so a rebuilt claims map cannot restore the fencing token sequence. |
| **TTL not recorded** | Claim TTL (`DEFAULT_TTL_SECS = 4200`) is part of the CRDT claim entry but not recorded in coordination events. A rebuilt claim would have an incorrect TTL. |
| **Session-scoped logs** | Per-worktree JSONL fragments (`coordination-events-ses-{id}.jsonl`) are local to each worktree and not globally merged. A cross-session rebuild would require gathering all fragments. |
| **Active vs expired claims** | Without TTL and fencing token data, it is impossible to distinguish active from expired claims in a rebuilt state. |

**Conclusion:** `coordination-events.jsonl` is an operational audit log, not a state
machine replay source. The authoritative recovery path for a corrupt or missing
`state.loro` is:

1. Start with an empty Loro document (all claims considered released).
2. Allow active workers to re-acquire their claims on next operation.
3. Use `claims::release_all(session_id)` to clean up stale claims for known dead sessions.

If `state.loro` is lost during an active parallel session, the worst-case outcome is
claim conflicts on re-acquisition, which are handled gracefully by the `scope_policy`
enforcement (workers receive `ClaimConflict` events rather than silent data corruption).

---

## 7. files::CANONICAL Exclusion Rationale

### 7.1 The two constants

From `codeflow-cli/core/src/ledger/mod.rs:33-44`:

```rust
/// Types synced to the database (excludes pathflow-events, coordination-events,
/// and autorun-events).
pub const CANONICAL: &[&str] = &[WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG];

/// All 7 canonical ledger type names.
pub const ALL: &[&str] = &[
    WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG,
    PATHFLOW_EVENTS, COORDINATION_EVENTS, AUTORUN_EVENTS,
];
```

### 7.2 Why coordination-events is in ALL but not CANONICAL

`CANONICAL` defines the 4 ledger types that are synced to SurrealDB via the rebuild
pipeline. A type is CANONICAL when its data is:

1. **Queried by application code** — the WorkGraph, memory, sessions, and config tables
   are read by hooks and CLI commands via `SurrealDB`.
2. **Long-lived** — the data must persist across sessions for cross-session queries.
3. **Business-critical** — loss requires rebuild from JSONL (hence the rebuild mechanism).

Coordination events fail criterion 1 and 2:

| Criterion | Assessment |
|-----------|-----------|
| Queried by application code | No. No application code reads the `coordination_event` table for decisions. Coordination state is read directly from `state.loro`, not the DB. |
| Long-lived across sessions | No. Coordination state is ephemeral — claims are acquired and released within a session. Historical claim data has no cross-session query value. |
| Business-critical | No. A lost `state.loro` is recovered by starting fresh (see Section 6). The JSONL log is useful for forensics, not for runtime decisions. |

**SurrealDB table exists but is never populated:** The `coordination_event` table is
defined in `schema.surql:242-255` but is not populated by the rebuild pipeline. The
table schema was created as a schema placeholder during development. The rebuild
implementation (`codeflow-cli/core/src/ledger/rebuild.rs`) only processes `CANONICAL`
types — coordination events are excluded.

---

## 8. SurrealDB Gap Analysis

### 8.1 SurrealDB coordination_event table schema

From `codeflow-cli/core/src/store/schema.surql:242-255`:

```sql
DEFINE TABLE OVERWRITE coordination_event SCHEMAFULL;
DEFINE FIELD OVERWRITE event_type   ON TABLE coordination_event TYPE string;
DEFINE FIELD OVERWRITE session_id   ON TABLE coordination_event TYPE string;
DEFINE FIELD OVERWRITE path         ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE branch       ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE target_branch ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE held_by      ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE task_id      ON TABLE coordination_event TYPE option<string>;
DEFINE FIELD OVERWRITE success      ON TABLE coordination_event TYPE option<bool>;
DEFINE FIELD OVERWRITE original_scope ON TABLE coordination_event TYPE option<array<string>>;
DEFINE FIELD OVERWRITE timestamp    ON TABLE coordination_event TYPE string;

DEFINE INDEX OVERWRITE idx_coordination_event_type ON TABLE coordination_event FIELDS event_type;
DEFINE INDEX OVERWRITE idx_coordination_event_session_id ON TABLE coordination_event FIELDS session_id;
```

10 fields, 2 indexes.

### 8.2 JSONL field vs SurrealDB table mapping

| JSONL field (CoordinationEvent) | SurrealDB field | Present in DB table | Notes |
|---------------------------------|----------------|---------------------|-------|
| `type` (discriminant) | `event_type` | Yes | Field renamed: `type` → `event_type` |
| `session_id` | `session_id` | Yes | Both required strings |
| `path` | `path` | Yes | `option<string>` in DB; required on 4 variants |
| `held_by` | `held_by` | Yes | `option<string>` in DB; required on `claim_conflict` only |
| `original_scope` | `original_scope` | Yes | `option<array<string>>` in DB; required on `scope_expansion` |
| `branch` | `branch` | Yes | `option<string>` in DB; required on merge variants |
| `target_branch` | `target_branch` | Yes | `option<string>` in DB; required on merge variants |
| `success` | `success` | Yes | `option<bool>` in DB; required on `merge_rebase_attempted` |
| `task_id` | `task_id` | Yes | `option<string>` in both |
| `timestamp` | `timestamp` | Yes | Both `string` |

**All 10 JSONL fields have corresponding SurrealDB columns.** The SurrealDB schema was
designed to accommodate the full `CoordinationEvent` union — every variant-specific field
is represented as `option<*>` in the DB schema.

### 8.3 Gaps and discrepancies

| Gap | Description | Severity |
|-----|-------------|---------|
| **Table never populated** | The `coordination_event` DB table exists but no data is ever written to it. The rebuild pipeline excludes `coordination-events` from `files::CANONICAL`. | High — the schema exists but is a dead letter. |
| **Field name mismatch: `type` vs `event_type`** | JSONL uses `"type"` as the discriminant; SurrealDB uses `"event_type"`. A sync pipeline would need to rename this field on insertion. | Medium — straightforward rename but easy to miss. |
| **No `worktree` column** | Neither JSONL events nor the DB table have a `worktree` field. Cross-worktree correlation requires joining via `session_id → worktree registry`. | Low — current design accepts this gap. |
| **`session_id` type mismatch** | JSONL uses `SessionId` newtype (validated non-empty string). DB uses plain `string` (no enforcement). A rebuild would need to validate the format. | Low — validation gap only. |

---

## 9. BEFORE/AFTER JSON Examples

The "BEFORE" format is what `CoordinationEvent` currently serializes to. The "AFTER"
format shows the canonical target with `"event"` as the discriminant and a `details`
nesting for variant-specific fields — aligned with the canonical schema direction from
INF-TSK-024-007.

### 9.1 claim_acquired (BEFORE)

```json
{
  "type": "claim_acquired",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "path": "src/main.rs",
  "task_id": "INF-TSK-024-033",
  "timestamp": "2026-03-21T10:00:00Z"
}
```

### 9.2 claim_acquired (AFTER — canonical target)

```json
{
  "event": "claim_acquired",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-21T10:00:00Z",
  "details": {
    "path": "src/main.rs",
    "task_id": "INF-TSK-024-033"
  }
}
```

### 9.3 claim_conflict (BEFORE)

```json
{
  "type": "claim_conflict",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "path": "src/lib.rs",
  "held_by": "ses-01kjvsv5rat20cj16np25y45tg",
  "timestamp": "2026-03-21T10:01:00Z"
}
```

Note: `task_id` is omitted because the field is `None` and `skip_serializing_if =
"Option::is_none"` applies.

### 9.4 claim_conflict (AFTER — canonical target)

```json
{
  "event": "claim_conflict",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-21T10:01:00Z",
  "details": {
    "path": "src/lib.rs",
    "held_by": "ses-01kjvsv5rat20cj16np25y45tg"
  }
}
```

### 9.5 coord_claim_released (BEFORE)

```json
{
  "type": "coord_claim_released",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "path": "src/main.rs",
  "task_id": "INF-TSK-024-033",
  "timestamp": "2026-03-21T10:02:00Z"
}
```

### 9.6 coord_claim_released (AFTER — canonical target)

```json
{
  "event": "coord_claim_released",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-21T10:02:00Z",
  "details": {
    "path": "src/main.rs",
    "task_id": "INF-TSK-024-033"
  }
}
```

### 9.7 scope_expansion (BEFORE)

```json
{
  "type": "scope_expansion",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "path": "src/extra.rs",
  "original_scope": ["src/main.rs", "src/lib.rs"],
  "task_id": "INF-TSK-024-033",
  "timestamp": "2026-03-21T10:03:00Z"
}
```

### 9.8 scope_expansion (AFTER — canonical target)

```json
{
  "event": "scope_expansion",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-21T10:03:00Z",
  "details": {
    "path": "src/extra.rs",
    "original_scope": ["src/main.rs", "src/lib.rs"],
    "task_id": "INF-TSK-024-033"
  }
}
```

### 9.9 merge_conflict_detected (BEFORE)

```json
{
  "type": "merge_conflict_detected",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "branch": "feat/my-feature",
  "target_branch": "main",
  "task_id": "INF-TSK-024-033",
  "timestamp": "2026-03-21T12:00:00Z"
}
```

### 9.10 merge_conflict_detected (AFTER — canonical target)

```json
{
  "event": "merge_conflict_detected",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-21T12:00:00Z",
  "details": {
    "branch": "feat/my-feature",
    "target_branch": "main",
    "task_id": "INF-TSK-024-033"
  }
}
```

### 9.11 merge_rebase_attempted (BEFORE)

```json
{
  "type": "merge_rebase_attempted",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "branch": "feat/my-feature",
  "target_branch": "main",
  "success": false,
  "timestamp": "2026-03-21T12:01:00Z"
}
```

### 9.12 merge_rebase_attempted (AFTER — canonical target)

```json
{
  "event": "merge_rebase_attempted",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-21T12:01:00Z",
  "details": {
    "branch": "feat/my-feature",
    "target_branch": "main",
    "success": false
  }
}
```

---

## 10. Rust Type Reference

### 10.1 SessionId

**Location:** `codeflow-cli/core/src/types/ids.rs:87-91`

A `#[serde(transparent)]` newtype wrapper around `String`. Validates non-empty on
construction. Maps to the `session_id` field on all `CoordinationEvent` variants and
the `held_by` field on `ClaimConflict`.

**Format:** `ses-{ULID}` (e.g., `ses-01kjw2gertbnn8xnpmr8mcrmc8`).

**Distinction from `Event` struct usage:** In the general `Event` struct (`ledger/mod.rs`),
`session_id` is `Option<String>`. In `CoordinationEvent`, `session_id` is the `SessionId`
newtype (required, non-optional). This is a stronger type guarantee on coordination events.

### 10.2 CoordinationEvent enum

**Location:** `codeflow-cli/core/src/coordination/types/events.rs:17-82`

**Serde configuration:** `#[serde(tag = "type")]` — discriminant field is `"type"`.

| Variant | Discriminant | Unique fields |
|---------|-------------|---------------|
| `ClaimAcquired` | `"claim_acquired"` | `path: String` |
| `ClaimConflict` | `"claim_conflict"` | `path: String`, `held_by: SessionId` |
| `ClaimReleased` | `"coord_claim_released"` | `path: String` |
| `ScopeExpansion` | `"scope_expansion"` | `path: String`, `original_scope: Vec<String>` |
| `MergeConflictDetected` | `"merge_conflict_detected"` | `branch: String`, `target_branch: String` |
| `MergeRebaseAttempted` | `"merge_rebase_attempted"` | `branch: String`, `target_branch: String`, `success: bool` |

**Event type method:** `CoordinationEvent::event_type(&self) -> &'static str` returns the
discriminant string for use in ledger routing.

**Derives:** `Debug, Clone, Serialize, Deserialize, PartialEq, Eq`

### 10.3 LoroCoordinator

**Location:** `codeflow-cli/core/src/coordination/loro.rs:84-92`

The CRDT backend that generates claim events. Owns:

- `doc: LoroDoc` — the in-memory Loro document
- `next_token: u64` — monotonically increasing fencing token counter
- `state_path: PathBuf` — path to `.state/coordination/state.loro`
- `ttl_secs: u64` — claim TTL (default 4200s / 70 min)

**Persistence:** `LoroDoc::export(ExportMode::Snapshot)` writes the binary snapshot.
`LoroDoc::from_snapshot(&bytes)` restores it. On corrupt or empty snapshot, starts fresh.

### 10.4 Relevant routing constants

**Location:** `codeflow-cli/core/src/ledger/mod.rs:22-44`

```rust
pub const COORDINATION_EVENTS: &str = "coordination-events";

pub const CANONICAL: &[&str] = &[WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG];

pub const ALL: &[&str] = &[
    WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG,
    PATHFLOW_EVENTS, COORDINATION_EVENTS, AUTORUN_EVENTS,
];
```

`COORDINATION_EVENTS` is present in `ALL` (7 types) but absent from `CANONICAL` (4 types).
This is the authoritative code-level definition of the exclusion documented in Section 7.
