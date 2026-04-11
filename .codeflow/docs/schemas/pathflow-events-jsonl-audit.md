---
title: "pathflow-events.jsonl Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-04"
updated_at: "2026-04-04"
scope: INF-TSK-024-005
feeds_into:
  - INF-TSK-024-007
  - INF-TSK-024-035
---

# pathflow-events.jsonl Schema Audit

This document audits the schema of the `pathflow-events` ledger across all 5 active event
types, catalogs the dual-generation field naming issues inherited from the shell era, and
provides a gap analysis against the Rust target model. It feeds directly into
INF-TSK-024-007 (canonical event schema synthesis) and INF-TSK-024-035 (pathflow-events
migration).

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Two Schema Generations](#2-two-schema-generations)
- [3. Event Type Inventory](#3-event-type-inventory)
  - [3.1 phase_transition](#31-phase_transition)
  - [3.2 stage_transition](#32-stage_transition)
  - [3.3 session_register](#33-session_register)
  - [3.4 session_metadata](#34-session_metadata)
  - [3.5 pathflow_task_update](#35-pathflow_task_update)
- [4. Field Inconsistencies Catalog](#4-field-inconsistencies-catalog)
- [5. Legacy Event Types in the File](#5-legacy-event-types-in-the-file)
- [6. BEFORE/AFTER Schema Table](#6-beforeafter-schema-table)
- [7. BEFORE/AFTER JSON Examples](#7-beforeafter-json-examples)
- [8. Rust Type Reference](#8-rust-type-reference)
- [9. Parallel Concern](#9-parallel-concern)

---

## 1. File Location and Layout

**Legacy location (still read by `codeflow doctor`):** `.state/logs/pathflow-events.jsonl`

**Current ledger location (post PR #221 subdirectory layout):**

```text
.state/ledger/
└── pathflow-events/
    ├── pathflow-events.jsonl                 ← base (compacted history)
    └── pathflow-events-ses-{id}.jsonl        ← per-session fragment
```

The subdirectory layout is implemented by `JsonlWriter` in
`codeflow-cli/core/src/ledger/jsonl.rs`. When `session_id` is `Some`, events write to the
session fragment file. When `None`, events write to the base file.

**Writer:** `JsonlWriter::append_event()` in `codeflow-cli/core/src/ledger/jsonl.rs`, via
event routing in `codeflow-cli/core/src/ledger/routing.rs`.

**Not in CANONICAL set:** The `pathflow-events` type is NOT included in `files::CANONICAL`
(`codeflow-cli/core/src/ledger/mod.rs:33`). CANONICAL contains only `work-graph`,
`memory-events`, `sessions`, and `config`. This means pathflow events are **not synced to
SurrealDB** — they are only available from the JSONL ledger files.

**Event types routed to `pathflow-events`** (from `routing.rs:51-56`):

```text
phase_transition | stage_transition | session_register | session_metadata | pathflow_task_update
```

**Doctor reader:** The `codeflow doctor` command reads pathflow events via
`read_pathflow_events()` at `codeflow-cli/core/src/doctor/mod.rs:653`. This function reads
from the **legacy** `.state/logs/pathflow-events.jsonl` path rather than the ledger
subdirectory. This is an existing inconsistency to note for migration.

---

## 2. Two Schema Generations

Two distinct schemas coexist in the file, written by different eras of the toolchain.

### Shell Era (deprecated — no longer written)

The shell-era agent hooks wrote events directly to the file using ad-hoc JSON construction.
Two variants existed:

**Variant A — short field names:**

```json
{"ts":"2026-02-17T15:41:56Z","e":"begin_work","work_id":"work-1771342801-sandbox","task_id":"INF-TSK-FEAT-GENL-003","session_id":"d67b4d04-...","branch":"feat/sandbox-skill"}
```

- Uses `ts` instead of `timestamp`
- Uses `e` instead of `event`
- Contains `begin_work` — a work-graph event type that does not belong in pathflow-events

**Variant B — `type` field with extra `id`:**

```json
{"id":"EVT-01KHMW34QT9W05S3XVPDYTXK53","type":"session_metadata","session_id":"d67b4d04-...","key":"work_type","value":"PLAN","ts":"2026-02-17T04:00:19.000Z"}
```

- Uses `type` instead of `event`
- Uses `ts` instead of `timestamp`
- Has extra `id` field (EVT-{ULID}) — not present in the target schema

### Go/Rust Era (canonical — current)

The Go CLI and subsequent Rust CLI write the 5 canonical event types with consistent field
naming (`event`, `timestamp`, `session_id`).

---

## 3. Event Type Inventory

### 3.1 phase_transition

Emitted when a PathFlow phase enters, completes, or is skipped.

**Actual entry (Go/Rust era):**

```json
{"event":"phase_transition","phase":"PF3-CLASSIFY","session_id":"ses-...","status":"completed","timestamp":"2026-03-04T06:48:42Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"phase_transition"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Session identifier |
| `worktree` | string | No | Present when session runs in a worktree |
| `phase` | string | Yes | Phase name: `PF1-INIT` through `PF7-END` |
| `status` | string | Yes | `entered` / `completed` / `skipped` |
| `task_id` | string | No | Optional task reference |
| `work_type` | string | No | Optional work type (e.g., `"FEAT"`) |

---

### 3.2 stage_transition

Emitted when a work stage (WS-DEV, WS-REV, etc.) changes state. Includes a `verdict` field
for review and QA outcomes.

**Actual entry (Go/Rust era):**

```json
{"event":"stage_transition","iteration":1,"session_id":"ses-...","stage":"WS-REV","status":"complete","timestamp":"2026-03-04T...","verdict":"approved"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"stage_transition"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Session identifier |
| `worktree` | string | No | Present when session runs in a worktree |
| `stage` | string | Yes | `WS-DEV` / `WS-PLAN` / `WS-DOCS` / `WS-TEST` / `WS-REV` / `WS-QA` |
| `status` | string | Yes | `pending` / `in_progress` / `complete` / `failed` |
| `iteration` | integer | Yes | Rework iteration count (>= 1) |
| `verdict` | string | No | `pass` / `fail` / `approved` / `changes_requested` |

---

### 3.3 session_register

Emitted when a session is registered in the PathFlow system. Writes **two separate events**
per call — one for `tracking_level` and one for `interaction_mode`.

**Actual entries (pair emitted together):**

```json
{"event":"session_register","session_id":"ses-...","timestamp":"...","tracking_level":"pending"}
{"event":"session_register","session_id":"ses-...","timestamp":"...","interaction_mode":"interactive"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_register"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Session identifier |
| `worktree` | string | No | Present when session runs in a worktree |
| `tracking_level` | string | Conditional | `pending` / `tracked` / `untracked` (first event of pair) |
| `interaction_mode` | string | Conditional | `interactive` / `autorun` (second event of pair) |

**Note:** Each `session_register` event carries exactly one of `tracking_level` or
`interaction_mode`. Consumers reading these events must handle both single-field variants.

---

### 3.4 session_metadata

Emitted to record key-value metadata about a session (e.g., work type, branch).

**Actual entry (Go/Rust era):**

```json
{"event":"session_metadata","key":"work_type","session_id":"ses-...","timestamp":"...","value":"PLAN"}
```

**Shell-era variant (deprecated):**

```json
{"id":"EVT-01KHMW34QT9W05S3XVPDYTXK53","type":"session_metadata","session_id":"d67b4d04-...","key":"work_type","value":"PLAN","ts":"2026-02-17T04:00:19.000Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_metadata"` (shell era used `type`) |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 (shell era used `ts`) |
| `session_id` | string | Yes | Session identifier |
| `worktree` | string | No | Present when session runs in a worktree |
| `key` | string | Yes | Metadata key (e.g., `"work_type"`, `"branch"`) |
| `value` | string | Yes | Metadata value |
| `id` | string | No (legacy only) | Shell-era EVT-{ULID} — removed in target schema |

---

### 3.5 pathflow_task_update

Emitted when a PathFlow task tracker entry (PF{N}-TSK-{NN}) changes state.

**Actual entry (Go/Rust era):**

```json
{"event":"pathflow_task_update","session_id":"ses-...","task_id":"PF3-TSK-01","task_status":"completed","timestamp":"..."}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"pathflow_task_update"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Session identifier |
| `worktree` | string | No | Present when session runs in a worktree |
| `task_id` | string | Yes | Format: `PF[1-7]-TSK-[0-9]{2}` (e.g., `PF3-TSK-01`) |
| `task_status` | string | Yes | `pending` / `in_progress` / `completed` / `skipped` / `blocked` |

---

## 4. Field Inconsistencies Catalog

### 4.1 Event Type Key

| Era | Field | Value Example | Issue |
|-----|-------|---------------|-------|
| Shell (Variant A) | `e` | `"begin_work"` | Single-char abbreviation |
| Shell (Variant B) | `type` | `"session_metadata"` | Conflicts with JSON Schema's `type` keyword |
| Go/Rust (canonical) | `event` | `"phase_transition"` | Correct |

### 4.2 Timestamp Key

| Era | Field | Value Example | Issue |
|-----|-------|---------------|-------|
| Shell (both variants) | `ts` | `"2026-02-17T15:41:56Z"` | Non-standard abbreviation |
| Go/Rust (canonical) | `timestamp` | `"2026-03-04T06:48:42Z"` | Correct |

### 4.3 Extra ID Field

| Era | Field | Value Example | Issue |
|-----|-------|---------------|-------|
| Shell (Variant B) | `id` | `"EVT-01KHMW34QT9W05S3XVPDYTXK53"` | Not needed; identified by `session_id + timestamp` |
| Go/Rust (canonical) | (absent) | — | Correct |

---

## 5. Legacy Event Types in the File

The shell era wrote work-graph event types directly into `pathflow-events.jsonl`. These do
not belong there and are not valid pathflow event types:

| Event Type | Belongs In | Why It Appears |
|------------|-----------|----------------|
| `begin_work` | `work-graph.jsonl` | Shell hooks wrote it to the wrong file |
| `complete_work` | `work-graph.jsonl` | Shell hooks wrote it to the wrong file |
| `stale_work_cleanup` | `work-graph.jsonl` | Shell hooks wrote it to the wrong file |
| `progress` | `memory-events.jsonl` | Shell hooks wrote it to the wrong file |

The Rust CLI routing system (`routing.rs`) correctly routes all these event types away from
`pathflow-events`. Consumers of the pathflow-events file should filter for only the 5
canonical types listed in Section 3.

---

## 6. BEFORE/AFTER Schema Table

### phase_transition

| Field | BEFORE (shell era) | AFTER (target) | Change |
|-------|-------------------|----------------|--------|
| Event type key | `e` or `type` | `event` | Rename |
| Timestamp key | `ts` | `timestamp` | Rename |
| Extra ID field | `id` (EVT-{ULID}) | (absent) | Remove |
| `session_id` | string (UUID format) | string (`ses-{id}` format) | Normalize |
| `worktree` | (absent) | string | Add (optional) |

### stage_transition

| Field | BEFORE (shell era) | AFTER (target) | Change |
|-------|-------------------|----------------|--------|
| Event type key | `e` | `event` | Rename |
| Timestamp key | `ts` | `timestamp` | Rename |
| `iteration` | (absent) | integer | Add |
| `verdict` | (absent) | string (optional) | Add |
| `worktree` | (absent) | string | Add (optional) |

### session_metadata

| Field | BEFORE (shell era) | AFTER (target) | Change |
|-------|-------------------|----------------|--------|
| Event type key | `type` | `event` | Rename |
| Timestamp key | `ts` | `timestamp` | Rename |
| `id` | `"EVT-{ULID}"` | (absent) | Remove |
| `worktree` | (absent) | string | Add (optional) |

---

## 7. BEFORE/AFTER JSON Examples

### phase_transition — BEFORE (shell era, Variant A)

```json
{"ts":"2026-02-17T15:41:56Z","e":"phase_transition","phase":"PF1-INIT","session_id":"d67b4d04-...","status":"entered"}
```

### phase_transition — AFTER (target schema)

```json
{
  "event": "phase_transition",
  "timestamp": "2026-03-04T20:00:00.000Z",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "phase": "PF3-CLASSIFY",
  "status": "completed",
  "task_id": "INF-TSK-008-001",
  "work_type": "FEAT"
}
```

### session_metadata — BEFORE (shell era, Variant B)

```json
{"id":"EVT-01KHMW34QT9W05S3XVPDYTXK53","type":"session_metadata","session_id":"d67b4d04-...","key":"work_type","value":"PLAN","ts":"2026-02-17T04:00:19.000Z"}
```

### session_metadata — AFTER (target schema)

```json
{
  "event": "session_metadata",
  "timestamp": "2026-03-04T20:00:00.000Z",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "key": "work_type",
  "value": "PLAN"
}
```

### stage_transition — current (Go/Rust era, already near-canonical)

```json
{
  "event": "stage_transition",
  "iteration": 1,
  "session_id": "ses-...",
  "stage": "WS-REV",
  "status": "complete",
  "timestamp": "2026-03-04T06:48:42Z",
  "verdict": "approved"
}
```

---

## 8. Rust Type Reference

Located in `codeflow-cli/core/src/types/events.rs:175-230`.

All pathflow variants share the same structure: top-level `session_id`, `timestamp`, and
optional `worktree` fields, with event-specific fields in the `data` catch-all. Event-specific
fields have not yet been promoted to named struct fields — they remain in
`data: serde_json::Value` with `#[serde(flatten)]`.

```rust
// codeflow-cli/core/src/types/events.rs:175-230

// -- pathflow-events.jsonl --
PhaseTransition {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
StageTransition {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
SessionRegister {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
SessionMetadata {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
PathflowTaskUpdate {
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
```

**Event type discriminators** (from `LedgerEvent::event_type()` at `events.rs:277-281`):

| Variant | `event_type()` return value |
|---------|----------------------------|
| `PhaseTransition` | `"phase_transition"` |
| `StageTransition` | `"stage_transition"` |
| `SessionRegister` | `"session_register"` |
| `SessionMetadata` | `"session_metadata"` |
| `PathflowTaskUpdate` | `"pathflow_task_update"` |

**Routing constant:** `files::PATHFLOW_EVENTS = "pathflow-events"` at
`codeflow-cli/core/src/ledger/mod.rs:27`.

**Sentinel-write handler note:** The `SentinelWrite` handler in
`codeflow-cli/core/src/hooks/post_tool_use.rs` does NOT write pathflow events to the
ledger. It creates sentinel files at `.state/sentinels/pathflow/{session-id}/` and updates
`pathflow-session-status.json`. Pathflow events go through the JSONL ledger via
`JsonlWriter::append_event()`.

---

## 9. Parallel Concern

**Risk level: Medium**

Events from parallel sessions (multiple worktrees running concurrently) interleave in the
same base file. The `pathflow-events.jsonl` base file is append-only with `flock` protection,
so there is no data corruption risk. However, consumers reading the file for analysis MUST
filter by `session_id` to isolate a single session's phase progression. Sorting by
`timestamp` alone is insufficient when parallel sessions overlap in time.

The per-session fragment layout (`.state/ledger/pathflow-events/pathflow-events-ses-{id}.jsonl`)
mitigates this: each session's events are isolated in their own fragment file during active
execution. The base file accumulates compacted history from completed sessions.
