---
title: "config.jsonl Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-03-30"
updated_at: "2026-03-30"
scope: INF-TSK-024-004
feeds_into:
  - INF-TSK-024-007
---

# config.jsonl Schema Audit

This document audits the schema of `.state/ledger/config/config.jsonl`, the simplest of the
CodeFlow ledger files. Config events (`config_set`, `config_updated`) record key-value
configuration changes. This audit verifies the routing and validation infrastructure in the
Rust codebase, maps fields to Rust types, and provides the gap analysis required for
INF-TSK-024-007 (canonical event schema synthesis).

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Current State](#2-current-state)
- [3. Event Types and Routing](#3-event-types-and-routing)
- [4. Event Schemas](#4-event-schemas)
- [5. Rust Type Reference](#5-rust-type-reference)
- [6. Validation Infrastructure Assessment](#6-validation-infrastructure-assessment)
- [7. Why config.jsonl is the Simplest Ledger File](#7-why-configjsonl-is-the-simplest-ledger-file)
- [8. BEFORE/AFTER Schema Comparison](#8-beforeafter-schema-comparison)

---

## 1. File Location and Layout

**Current (subdirectory layout, post-PR #221 migration):**

```text
.state/ledger/
└── config/
    ├── config.jsonl                      ← base (compacted history)
    └── config-ses-{id}.jsonl             ← per-session fragment
```

The subdirectory layout is implemented by `JsonlWriter` in
`codeflow-cli/core/src/ledger/jsonl.rs`. When `session_id` is `Some`, events write to the
session fragment file. When `None`, events write to the base file.

**Writer:** `JsonlWriter::append_event()` in `codeflow-cli/core/src/ledger/jsonl.rs`, using
the `Event` struct from `codeflow-cli/core/src/ledger/mod.rs`.

**Ledger type constant:** `files::CONFIG = "config"` (defined in
`codeflow-cli/core/src/ledger/mod.rs:26`).

**Canonical file set membership:** `config` is included in `files::CANONICAL` (the four
ledger types synced to the database), defined at
`codeflow-cli/core/src/ledger/mod.rs:33`:

```rust
pub const CANONICAL: &[&str] = &[WORK_GRAPH, MEMORY_EVENTS, SESSIONS, CONFIG];
```

---

## 2. Current State

**Both the main repo and worktree ledger files are empty (0 entries):**

- `.state/ledger/config/config.jsonl` — 0 bytes
- `.git-worktrees/worktree-{SID}/.state/ledger/config/config.jsonl` — 0 bytes

The config ledger directory exists and the base file is present, but no events have ever
been written to it. Routing and validation infrastructure exists in the Rust codebase, but
no production code path emits `config_set` or `config_updated` events. The emitter is not
yet implemented.

**Historical note (from `schema-standardization.md` Section 1.4):** This was also the case
in the Go CLI era — `routing.go:61-62` routed these event types and the file appeared in
`CanonicalFiles()` at `routing.go:77`, but nothing in the Go codebase emitted config events.

---

## 3. Event Types and Routing

**Routed event types** (from `codeflow-cli/core/src/ledger/routing.rs:49`):

```rust
// config.jsonl
"config_set" | "config_updated" => Ok(files::CONFIG),
```

Both `config_set` and `config_updated` route to the `config` ledger type. The routing
function `route_event_type()` returns `LedgerError::UnknownEventType` for any unrecognized
event type, which means both config events are explicitly accepted and any typo or novel
event name would be rejected at write time.

**Routing test coverage** (from `codeflow-cli/core/src/ledger/routing.rs:150-158`):

```rust
#[test]
fn test_config_events_route_to_config() {
    for event_type in &["config_set", "config_updated"] {
        assert_eq!(
            route_event_type(event_type).unwrap(),
            files::CONFIG,
            "{event_type} should route to config.jsonl"
        );
    }
}
```

Both event types are also included in `test_all_ledger_event_variants_routable`
(`routing.rs:234-292`), confirming they participate in the exhaustive routing coverage check.

---

## 4. Event Schemas

### 4.1 config_set

Records the initial setting of a configuration key or an explicit set operation.

**Rust variant** (`codeflow-cli/core/src/types/events.rs:233-238`):

```rust
// -- config.jsonl --
ConfigSet {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
```

**Field table (current schema — no entries exist, schema inferred from Rust types and test
fixtures):**

| Field | Type | Required | Source |
|-------|------|----------|--------|
| `event` | string | Yes | Discriminator, always `"config_set"` |
| `timestamp` | string (RFC 3339) | Yes | `Event.timestamp` in `ledger/mod.rs` |
| `session_id` | string | No | `Event.session_id` (omitted if `None`) |
| `worktree` | string | No | `Event.worktree` (omitted if `None`) |
| `key` | string | No (current) | Event-specific data (not enforced in Rust) |
| `value` | any | No (current) | Event-specific data (not enforced in Rust) |

**Test fixture** (`codeflow-cli/core/src/types/events.rs:384-388`):

```json
{"event":"config_set","key":"mode","value":"interactive","timestamp":"2026-03-07T00:00:00Z"}
```

**Note on required fields:** The `key` and `value` fields are semantically required for a
meaningful `config_set` event but are not structurally enforced by the Rust type system. The
`ConfigSet` variant uses `serde_json::Value` (via `#[serde(flatten)]`) to capture all
event-specific data without schema validation.

### 4.2 config_updated

Records an update to an existing configuration key (i.e., a change from one value to
another, as opposed to an initial set).

**Rust variant** (`codeflow-cli/core/src/types/events.rs:239-244`):

```rust
ConfigUpdated {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
```

**Field table (current schema):**

| Field | Type | Required | Source |
|-------|------|----------|--------|
| `event` | string | Yes | Discriminator, always `"config_updated"` |
| `timestamp` | string (RFC 3339) | Yes | `Event.timestamp` in `ledger/mod.rs` |
| `session_id` | string | No | `Event.session_id` (omitted if `None`) |
| `worktree` | string | No | `Event.worktree` (omitted if `None`) |
| `key` | string | No (current) | Event-specific data (not enforced in Rust) |
| `value` | any | No (current) | New value after update |
| `previous_value` | any | No (current) | Prior value before update |

**No test fixtures exist** for `config_updated` beyond the roundtrip tests in
`codeflow-cli/core/src/types/events.rs:540-556` which use only `timestamp`.

### 4.3 Structural Difference Between config_set and config_updated

Both variants have identical Rust structure. The semantic distinction is:

- `config_set` — initial write of a key or an unconditional set (no assumed prior value)
- `config_updated` — targeted update to an existing key (implies a `previous_value`)

In practice, since no emitter exists, this distinction is a design intent, not an enforced
constraint.

---

## 5. Rust Type Reference

### 5.1 LedgerEvent Variants

The `LedgerEvent` enum in `codeflow-cli/core/src/types/events.rs` provides typed
deserialization for config events. Both variants follow the same pattern used across all
ledger event types: a strongly-typed discriminator field (`event`) and a flexible `data:
serde_json::Value` for event-specific payload.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LedgerEvent {
    // ...
    ConfigSet {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    ConfigUpdated {
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
}
```

**Key design properties:**

- `#[serde(tag = "event", rename_all = "snake_case")]` — the `event` field acts as the
  discriminator; `ConfigSet` maps to `"config_set"`, `ConfigUpdated` to `"config_updated"`
- `timestamp` is `Option<String>` with `#[serde(default)]` — absent in old entries
  deserializes to `None` rather than erroring
- `#[serde(flatten)]` on `data` — all remaining JSON fields merge into `data` at the top
  level; no nested object required

### 5.2 Event Struct (Transport Layer)

The `Event` struct in `codeflow-cli/core/src/ledger/mod.rs:86-107` is the transport type
used by `JsonlWriter`. Universal fields with explicit Rust types:

| Field | Rust Type | Serialized Name | Behavior |
|-------|-----------|-----------------|----------|
| `event_type` | `String` | `"event"` | Required; routed via `route_event_type()` |
| `timestamp` | `String` | `"timestamp"` | Required; RFC 3339 |
| `session_id` | `Option<String>` | `"session_id"` | Omitted if `None` |
| `worktree` | `Option<String>` | `"worktree"` | Omitted if `None` |
| `data` | `HashMap<String, serde_json::Value>` | (flattened) | Event-specific fields at top level |

For config events, `key`, `value`, and `previous_value` (for `config_updated`) are passed
via the `data` HashMap and appear as top-level fields in the serialized JSON.

---

## 6. Validation Infrastructure Assessment

### 6.1 What Exists

**Routing validation (structural):** `route_event_type()` in
`codeflow-cli/core/src/ledger/routing.rs:17-73` validates that the event type is recognized.
Any event with type `"config_set"` or `"config_updated"` is accepted; all other types are
rejected with `LedgerError::UnknownEventType`. This is enforced at write time by
`JsonlWriter::append_event()`.

**Misrouting prevention:** `JsonlWriter::append_event_to_file()` in `jsonl.rs:174-185`
cross-checks the caller-supplied target file against the routing table. Attempting to write a
`config_set` event to `work-graph.jsonl` returns `LedgerError::MisroutedEvent`.

**Deserialization validation (read path):** `LedgerEvent` deserialization rejects unknown
event types (verified by `test_unknown_event_rejected` in `events.rs:377-381`). A
`config_set` entry with a typo in the `event` field would fail to deserialize.

**Rebuild validation:** `rebuild_ledger_type()` in `codeflow-cli/core/src/ledger/rebuild.rs`
skips corrupt JSON lines (verified by `test_rebuild_skips_corrupt_lines` in `rebuild.rs:180-197`),
maintaining ledger readability even if a partially-written event exists.

### 6.2 What Does Not Exist

**Field-level validation:** No Rust code validates that `key`, `value`, or `previous_value`
are present or well-formed in config events. The `ConfigSet` and `ConfigUpdated` variants
use `serde_json::Value` to accept any JSON structure. A `config_set` event with no `key`
field would be accepted by the writer and stored in the ledger without error.

**Schema enforcement at emit time:** There is no builder, constructor, or validation
function that enforces the intended config event schema before writing. This is consistent
with the pattern used across all ledger event types — the `Event` struct is a generic
transport and imposes no per-event-type field requirements.

**No active emitter:** No production code path calls `append_event` with `config_set` or
`config_updated`. All occurrences in the codebase are in tests only.

### 6.3 Validation Summary

| Validation Layer | Status | Location |
|-----------------|--------|----------|
| Event type accepted | Present | `routing.rs:49` |
| Misrouting rejected | Present | `jsonl.rs:174-185` |
| Corrupt line tolerance | Present | `rebuild.rs` |
| `key` field required | Absent | No enforcer |
| `value` field required | Absent | No enforcer |
| `previous_value` type check | Absent | No enforcer |
| Active emitter exists | Absent | Unimplemented |

---

## 7. Why config.jsonl is the Simplest Ledger File

config.jsonl has the smallest footprint of any ledger file for three reasons:

1. **Fewest event types:** Two event types (`config_set`, `config_updated`) versus
   7 for sessions, 16 for work-graph, and 10 for memory-events.

2. **No historical baggage:** The file has been empty since its introduction. There are no
   shell-era entries, no Go-era entries, and no schema evolution to document. Every other
   ledger file has at least two distinct historical schemas to reconcile; config.jsonl has
   none.

3. **Minimal required payload:** Configuration changes reduce to a key-value pair. The
   schema requires only `event`, `timestamp`, and `key`+`value` — the smallest meaningful
   payload of any ledger event type. Other event types (e.g., `task_created`, `epic_created`)
   require many more fields to be meaningful.

The combination of an empty ledger, two event types, and a minimal payload makes this the
natural starting point for the canonical schema standardization effort.

---

## 8. BEFORE/AFTER Schema Comparison

### 8.1 BEFORE: Current Format

The current format is inferred from the Rust type definitions and test fixtures, since no
actual entries exist.

**config_set (current — no `details` wrapper, minimal fields):**

```json
{"event":"config_set","timestamp":"2026-03-07T00:00:00Z","key":"mode","value":"interactive"}
```

**config_updated (current — no `details` wrapper):**

```json
{"event":"config_updated","timestamp":"2026-03-07T00:00:00Z","key":"mode","value":"autorun"}
```

**BEFORE field table:**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | `"config_set"` or `"config_updated"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | No | Present when session context available |
| `worktree` | string | No | Present when worktree context available |
| `key` | string | No (current) | Dot-separated config path |
| `value` | any | No (current) | New value |
| `previous_value` | any | No | Only in `config_updated` |

### 8.2 AFTER: Canonical Target Format

The canonical target schema (from `schema-standardization.md` Section 1.4) enforces
`session_id` as required and promotes `key` and `value` from optional to required fields.
Fields remain at the top level — there is no `details` wrapper for config events.

**config_set (canonical target):**

```json
{"timestamp":"2026-03-04T20:00:00.000Z","event":"config_set","session_id":"ses-01kjxabc123","key":"logging.prompts.enabled","value":true,"previous_value":null,"source":"user"}
```

**AFTER field table (canonical target):**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | `"config_set"` or `"config_updated"` |
| `timestamp` | string (ISO 8601 with ms) | Yes | UTC with milliseconds |
| `session_id` | string | Yes | Originating session (promoted to required) |
| `worktree` | string | No | Present in worktree sessions |
| `key` | string | Yes | Dot-separated config path |
| `value` | any | Yes | New value after change |
| `previous_value` | any | No | Prior value (`null` if first set) |
| `source` | string | No | Who changed it (`"user"`, `"autorun"`, `"migration"`) |

### 8.3 Gap Summary

| Gap | Current | Canonical Target | Impact |
|-----|---------|-----------------|--------|
| `session_id` enforcement | Optional | Required | No entries to migrate (empty file) |
| `key` enforcement | Optional | Required | Must be enforced in emitter |
| `value` enforcement | Optional | Required | Must be enforced in emitter |
| `previous_value` semantics | Undefined | Explicit (`null` for first set) | Design clarification |
| `source` field | Absent | Optional | New field; no migration needed |
| Timestamp precision | RFC 3339 | ISO 8601 with milliseconds | Emitter must use ms precision |

Since config.jsonl is empty, all gaps are forward-looking: they must be addressed in the
emitter implementation, not in a migration of existing entries.
