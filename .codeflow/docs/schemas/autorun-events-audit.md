---
title: "autorun-events.jsonl Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-07"
updated_at: "2026-04-07"
scope: INF-TSK-024-034
feeds_into:
  - INF-TSK-024-007
---

# autorun-events.jsonl Schema Audit

This document audits the schema of the `autorun-events` ledger across all 10 event types
emitted by the autorun subsystem, documents the latent routing gap for `worker_pool_full`,
maps the JSONL schema against the three autorun SurrealDB tables, and provides retention
policy considerations. It feeds directly into INF-TSK-024-007 (canonical event schema
synthesis).

Unlike most other ledger audit documents in this directory, `autorun-events` has no shell or
Go era — the autorun subsystem was introduced with the Rust CLI and all events originate from
a single typed Rust enum.

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Schema Generation (Rust-Native)](#2-schema-generation-rust-native)
- [3. Event Type Inventory](#3-event-type-inventory)
  - [3.1 batch_started](#31-batch_started)
  - [3.2 batch_completed](#32-batch_completed)
  - [3.3 batch_aborted](#33-batch_aborted)
  - [3.4 worker_started](#34-worker_started)
  - [3.5 worker_completed](#35-worker_completed)
  - [3.6 worker_failed](#36-worker_failed)
  - [3.7 worker_timeout](#37-worker_timeout)
  - [3.8 worker_blocked](#38-worker_blocked)
  - [3.9 worker_cancelled](#39-worker_cancelled)
  - [3.10 worker_pool_full](#310-worker_pool_full)
- [4. Field Inconsistencies Catalog](#4-field-inconsistencies-catalog)
  - [4.4 Latent Variant: WorkerBlocked Never Emitted](#44-latent-variant-workerblocked-never-emitted)
- [5. Legacy Event Types](#5-legacy-event-types)
- [6. BEFORE/AFTER Schema Table](#6-beforeafter-schema-table)
- [7. BEFORE/AFTER JSON Examples](#7-beforeafter-json-examples)
- [8. Rust Type Reference](#8-rust-type-reference)
- [9. Retention and Lifecycle Considerations](#9-retention-and-lifecycle-considerations)
- [10. DB Relationship Analysis](#10-db-relationship-analysis)

---

## 1. File Location and Layout

**Ledger location (subdirectory layout, Rust era):**

```text
.state/ledger/
└── autorun-events/
    ├── autorun-events.jsonl                 ← base (compacted history)
    └── autorun-events-ses-{id}.jsonl        ← per-session fragment
```

The subdirectory layout is implemented by `JsonlWriter` in
`codeflow-cli/core/src/ledger/jsonl.rs`. When `session_id` is `Some`, events write to the
session fragment file. When `None`, events write to the base file.

**Writer (two paths — see Section 4 for the routing gap):**

1. `emit_autorun_event()` in `codeflow-cli/core/src/autorun/worker.rs` and
   `codeflow-cli/core/src/autorun/orchestrator.rs`: writes directly to
   `.state/ledger/autorun-events/autorun-events.jsonl` via `JsonlWriter::append_event()`.
   This path bypasses `route_event_type()` entirely.

2. `JsonlWriter::append_event()` called via `route_event_type()` in
   `codeflow-cli/core/src/ledger/routing.rs`: the normal ledger routing path for most event
   types. This path is NOT used by autorun events in practice — all emission goes through
   `emit_autorun_event()`.

**Not in CANONICAL set:** The `autorun-events` type is NOT included in `files::CANONICAL`
(`codeflow-cli/core/src/ledger/mod.rs`). `files::CANONICAL` contains only `work-graph`,
`memory-events`, `sessions`, and `config`. This means autorun events are **not synced to
SurrealDB via JSONL rebuild** — the autorun DB tables (`autorun_session`, `autorun_worker`,
`autorun_task_run`) are populated directly by the Rust autorun subsystem, not through the
JSONL-to-DB rebuild pipeline.

**Event types routed to `autorun-events`** (from `routing.rs` lines 67-69):

```text
"batch_started" | "batch_completed" | "batch_aborted" | "worker_started"
| "worker_completed" | "worker_failed" | "worker_timeout" | "worker_blocked"
| "worker_cancelled" => Ok(files::AUTORUN_EVENTS)
```

**Note:** Only 9 of the 10 `AutorunEvent` variants appear in the routing table. The 10th
variant, `worker_pool_full`, is missing from `route_event_type()`. This is a latent routing
gap — see [Section 4](#4-field-inconsistencies-catalog) for details.

---

## 2. Schema Generation (Rust-Native)

The autorun subsystem was introduced entirely in the Rust CLI. There is no shell or Go era
for `autorun-events.jsonl` — the schema does not carry legacy inconsistencies from earlier
eras.

**Single schema generation source:** `AutorunEvent` enum in
`codeflow-cli/core/src/coordination/types/events.rs`, lines 99-219.

**Discriminator format:** `#[serde(tag = "type")]`

This is the same discriminator field name used by `CoordinationEvent` but differs from
`LedgerEvent`, which uses `#[serde(tag = "event")]`. Readers of autorun events must parse
the `type` field as the event discriminator, not `event`.

**Emission sites:**

| File | Events Emitted |
|------|---------------|
| `codeflow-cli/core/src/autorun/orchestrator.rs` | `BatchStarted` (line 225), `BatchAborted` (line 506), `BatchCompleted` (line 514) |
| `codeflow-cli/core/src/autorun/worker.rs` | `WorkerPoolFull` (line 552), `WorkerStarted` (line 751), `WorkerFailed` (lines 881, 1162, 1200), `WorkerCompleted` (line 1135), `WorkerTimeout` (line 1147), `WorkerCancelled` (line 1154) |

---

## 3. Event Type Inventory

All events share a common base: `session_id` (string, required), `timestamp` (string, RFC
3339, required), and `type` (string, the serde discriminator). Field tables below list only
event-specific fields beyond these three.

### 3.1 batch_started

Emitted by `orchestrator.rs` when a batch run begins. Marks the start of a multi-worker
autorun session.

**Example entry:**

```json
{"type":"batch_started","session_id":"ses-...","batch_name":"batch-2026-04-07","total_tasks":5,"timestamp":"2026-04-07T10:00:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"batch_started"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `batch_name` | string | Yes | Human-readable batch identifier |
| `total_tasks` | integer (i32) | Yes | Number of tasks in the batch |

---

### 3.2 batch_completed

Emitted by `orchestrator.rs` when all workers in a batch have finished (success or failure).
Provides a summary of outcomes across all tasks.

**Example entry:**

```json
{"type":"batch_completed","session_id":"ses-...","batch_name":"batch-2026-04-07","completed_tasks":4,"failed_tasks":1,"skipped_tasks":0,"timestamp":"2026-04-07T11:30:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"batch_completed"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `batch_name` | string | Yes | Human-readable batch identifier |
| `completed_tasks` | integer (i32) | Yes | Count of successfully completed tasks |
| `failed_tasks` | integer (i32) | Yes | Count of failed tasks |
| `skipped_tasks` | integer (i32) | Yes | Count of skipped tasks |

**Note:** Counts are cast from `usize` with `.min(i32::MAX)` in orchestrator.rs to prevent
overflow on pathological batch sizes.

---

### 3.3 batch_aborted

Emitted by `orchestrator.rs` when a batch is aborted before completion (e.g., user interrupt
or unrecoverable error). Includes a reason string.

**Example entry:**

```json
{"type":"batch_aborted","session_id":"ses-...","batch_name":"batch-2026-04-07","reason":"user_abort","timestamp":"2026-04-07T10:45:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"batch_aborted"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `batch_name` | string | Yes | Human-readable batch identifier |
| `reason` | string | Yes | Abort reason (e.g., `"user_abort"`) |

---

### 3.4 worker_started

Emitted by `worker.rs` when an individual worker begins executing its assigned task. Ties
together the batch session, the worker identity, and the task ID.

**Example entry:**

```json
{"type":"worker_started","session_id":"ses-...","worker_id":"worker-01","task_id":"INF-TSK-024-001","timestamp":"2026-04-07T10:01:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_started"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `worker_id` | string | Yes | Worker identity within the batch |
| `task_id` | string | Yes | Task being executed by this worker |

---

### 3.5 worker_completed

Emitted by `worker.rs` when a worker finishes successfully. Optionally records the PR number
if a pull request was created.

**Example entry (with PR):**

```json
{"type":"worker_completed","session_id":"ses-...","worker_id":"worker-01","task_id":"INF-TSK-024-001","pr_number":262,"timestamp":"2026-04-07T11:00:00Z"}
```

**Example entry (without PR):**

```json
{"type":"worker_completed","session_id":"ses-...","worker_id":"worker-01","task_id":"INF-TSK-024-001","timestamp":"2026-04-07T11:00:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_completed"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `worker_id` | string | Yes | Worker identity within the batch |
| `task_id` | string | Yes | Task executed by this worker |
| `pr_number` | integer (i64) | No | PR number if created; omitted when `None` (`skip_serializing_if`) |

**Implementation note:** `pr_number` is serialized as `Some(n)` only when `n > 0`. When the
worker record's pr_number is 0 (no PR), the field is set to `None` and omitted from the
emitted JSON (`#[serde(skip_serializing_if = "Option::is_none")]`).

---

### 3.6 worker_failed

Emitted by `worker.rs` in three situations: merge conflict during PR creation (line 881),
catch-all for other non-timeout/non-cancelled failure statuses (line 1162), and on `Err(e)`
from the worker execution path (line 1200). Includes an error string describing the failure.

**Example entry:**

```json
{"type":"worker_failed","session_id":"ses-...","worker_id":"worker-02","task_id":"INF-TSK-024-002","error":"merge conflict with main: 3 files","timestamp":"2026-04-07T10:30:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_failed"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `worker_id` | string | Yes | Worker identity within the batch |
| `task_id` | string | Yes | Task that failed |
| `error` | string | Yes | Error description; for merge conflicts, contains conflict details |

---

### 3.7 worker_timeout

Emitted by `worker.rs` when a worker's status is `"timeout"` (line 1147). No error field —
the timeout condition itself is the complete failure description.

**Example entry:**

```json
{"type":"worker_timeout","session_id":"ses-...","worker_id":"worker-03","task_id":"INF-TSK-024-003","timestamp":"2026-04-07T11:00:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_timeout"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `worker_id` | string | Yes | Worker identity within the batch |
| `task_id` | string | Yes | Task that timed out |

---

### 3.8 worker_blocked

**LATENT VARIANT — never emitted by production code.**

`WorkerBlocked` is defined in the `AutorunEvent` enum at
`codeflow-cli/core/src/coordination/types/events.rs:175` but is not emitted by any
production code path in `codeflow-cli/core/src/autorun/`. A grep across all autorun source
files returns zero call sites for `WorkerBlocked`.

In `worker.rs`, the status match arm at line 1162 is a catch-all `_ =>` that maps all
unrecognized statuses — including `"blocked"` — to `WorkerFailed`. Workers that reach a
blocked state produce `worker_failed` events, not `worker_blocked` events.

This variant appears to be intended for future use or was designed but never wired into the
worker execution path. It exists only in the enum definition and serde roundtrip tests
(`coordination/types/events.rs:499,599`).

**Canonical schema (for reference if the variant is wired up in future):**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_blocked"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `worker_id` | string | Yes | Worker identity within the batch |
| `task_id` | string | Yes | Task that became blocked |
| `reason` | string | Yes | Block reason |

---

### 3.9 worker_cancelled

Emitted by `worker.rs` when a worker's status is `"skipped"` (line 1154). The `reason` field
comes from `wr.error` on the worker record, which holds the cancellation explanation.

**Example entry:**

```json
{"type":"worker_cancelled","session_id":"ses-...","worker_id":"worker-05","task_id":"INF-TSK-024-005","reason":"dependency task failed","timestamp":"2026-04-07T10:20:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_cancelled"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `worker_id` | string | Yes | Worker identity within the batch |
| `task_id` | string | Yes | Task that was cancelled |
| `reason` | string | Yes | Cancellation reason (from `wr.error`) |

---

### 3.10 worker_pool_full

Emitted by `worker.rs` (line 552) when a task cannot be dispatched because the worker pool
has reached capacity. This event has two structural differences from all other autorun events:

1. **Missing `worker_id`:** No worker has been assigned yet when the pool is full, so this
   event does not carry a `worker_id`. This makes it structurally inconsistent with all other
   worker-prefixed events.

2. **Routing gap:** `worker_pool_full` is NOT present in the `route_event_type()` match arm
   in `codeflow-cli/core/src/ledger/routing.rs` (lines 67-69). Calling
   `route_event_type("worker_pool_full")` returns `LedgerError::UnknownEventType`.
   Events still reach the file because `emit_autorun_event()` writes directly to
   `.state/ledger/autorun-events/autorun-events.jsonl` via `JsonlWriter::append_event()`,
   bypassing `route_event_type()` entirely. However, any code that routes this event type
   through the normal `route_event_type()` path will fail.

**Example entry:**

```json
{"type":"worker_pool_full","session_id":"ses-...","task_id":"INF-TSK-024-006","timestamp":"2026-04-07T10:05:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `type` | string | Yes | Always `"worker_pool_full"` |
| `session_id` | string | Yes | Batch-level session ID |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `task_id` | string | Yes | Task that could not be dispatched |
| `worker_id` | **absent** | No | Not present — no worker assigned yet |

---

## 4. Field Inconsistencies Catalog

### 4.1 Missing Event Type in Routing Table

| Issue | Detail |
|-------|--------|
| Missing event type | `worker_pool_full` |
| Location | `codeflow-cli/core/src/ledger/routing.rs`, lines 67-69 |
| Effect | `route_event_type("worker_pool_full")` returns `LedgerError::UnknownEventType` |
| Mitigation | `emit_autorun_event()` bypasses `route_event_type()` — events reach disk regardless |
| Risk | Any code that calls `route_event_type()` for this event type will fail at runtime |
| Target fix | Add `"worker_pool_full"` to the routing match arm in `routing.rs` |

### 4.2 Missing worker_id on worker_pool_full

| Issue | Detail |
|-------|--------|
| Missing field | `worker_id` on `WorkerPoolFull` variant |
| Reason | No worker has been assigned when the pool is full |
| Structural effect | `worker_pool_full` cannot be correlated with a worker record in `autorun_worker` DB table |
| Consumers must handle | Absence of `worker_id` as a valid condition for this event type |
| Target schema | Current schema is correct — this is an inherent property, not a defect |

### 4.3 Discriminator Key Differs from LedgerEvent

| Event Family | Discriminator Key | Value Example |
|-------------|-------------------|---------------|
| `AutorunEvent` (`autorun-events.jsonl`) | `type` | `"worker_started"` |
| `CoordinationEvent` (`coordination-events.jsonl`) | `type` | `"claim_acquired"` |
| `LedgerEvent` (all other JSONL files) | `event` | `"phase_transition"` |

Consumers reading `autorun-events.jsonl` must parse the `type` field as the discriminator.
Reading `event` will yield no match. This is consistent with `CoordinationEvent` but
inconsistent with the rest of the ledger.

### 4.4 Latent Variant: WorkerBlocked Never Emitted

| Issue | Detail |
|-------|--------|
| Variant | `WorkerBlocked` / `"worker_blocked"` |
| Enum location | `codeflow-cli/core/src/coordination/types/events.rs:175` |
| Emission sites | None in production code — zero call sites in `codeflow-cli/core/src/autorun/` |
| Actual behavior | Workers with `"blocked"` status fall through catch-all arm at `worker.rs:1162`, producing `WorkerFailed` instead |
| Test presence | Serde roundtrip tests only (`events.rs:499,599`) |
| Risk | Consumers filtering on `"worker_blocked"` will never receive events for blocked workers — they must also handle `"worker_failed"` with an error matching a blocked-state description |
| Target fix | Either emit `WorkerBlocked` from the appropriate match arm in `worker.rs`, or remove the variant if `WorkerFailed` is the intended channel for blocked workers |

---

## 5. Legacy Event Types

No legacy event types exist in `autorun-events.jsonl`. The autorun subsystem is Rust-native
and was not preceded by shell or Go implementations. The file contains only `AutorunEvent`
variants as defined in the current Rust enum.

There are no misrouted events (events from other domains written to the wrong file) because
the `emit_autorun_event()` function is the sole writer and emits only `AutorunEvent` variants.

---

## 6. BEFORE/AFTER Schema Table

Since there is no legacy schema for autorun events, the BEFORE/AFTER table documents the
**current state** versus the **target state** after fixing the routing gap.

### batch_started / batch_completed / batch_aborted

No field changes needed. Current schema is canonical.

### worker_started / worker_timeout / worker_cancelled

No field changes needed. Current schema is canonical.

### worker_blocked

`WorkerBlocked` is a latent variant — never emitted in production. No field migration applies.
The open question is whether to wire it up or remove it (see Section 4.4).

### worker_completed

| Field | BEFORE (current) | AFTER (target) | Change |
|-------|-----------------|----------------|--------|
| `pr_number` | `Option<i64>` — omitted when None | No change | None — current is correct |

### worker_failed

| Field | BEFORE (current) | AFTER (target) | Change |
|-------|-----------------|----------------|--------|
| `error` | string, always present | No change | None — current is correct |

### worker_pool_full

| Field | BEFORE (current) | AFTER (target) | Change |
|-------|-----------------|----------------|--------|
| Routing | Missing from `route_event_type()` | Add to routing match arm | Fix routing gap |
| `worker_id` | Absent | Absent (intentional) | No change needed |

---

## 7. BEFORE/AFTER JSON Examples

All examples show the canonical form (current schema). The "BEFORE/AFTER" framing for
autorun events reflects the routing fix rather than a field migration.

### batch_started

**Current (canonical):**

```json
{
  "type": "batch_started",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "batch_name": "batch-inf-epc-024",
  "total_tasks": 12,
  "timestamp": "2026-04-07T10:00:00Z"
}
```

### batch_completed

**Current (canonical):**

```json
{
  "type": "batch_completed",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "batch_name": "batch-inf-epc-024",
  "completed_tasks": 10,
  "failed_tasks": 1,
  "skipped_tasks": 1,
  "timestamp": "2026-04-07T12:00:00Z"
}
```

### batch_aborted

**Current (canonical):**

```json
{
  "type": "batch_aborted",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "batch_name": "batch-inf-epc-024",
  "reason": "user_abort",
  "timestamp": "2026-04-07T10:30:00Z"
}
```

### worker_started

**Current (canonical):**

```json
{
  "type": "worker_started",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-01",
  "task_id": "INF-TSK-024-001",
  "timestamp": "2026-04-07T10:01:00Z"
}
```

### worker_completed (with PR)

**Current (canonical):**

```json
{
  "type": "worker_completed",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-01",
  "task_id": "INF-TSK-024-001",
  "pr_number": 262,
  "timestamp": "2026-04-07T11:00:00Z"
}
```

### worker_completed (without PR)

**Current (canonical):**

```json
{
  "type": "worker_completed",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-01",
  "task_id": "INF-TSK-024-001",
  "timestamp": "2026-04-07T11:00:00Z"
}
```

### worker_failed

**Current (canonical):**

```json
{
  "type": "worker_failed",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-02",
  "task_id": "INF-TSK-024-002",
  "error": "merge conflict with main: codeflow-cli/core/src/autorun/worker.rs",
  "timestamp": "2026-04-07T10:30:00Z"
}
```

### worker_timeout

**Current (canonical):**

```json
{
  "type": "worker_timeout",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-03",
  "task_id": "INF-TSK-024-003",
  "timestamp": "2026-04-07T11:01:00Z"
}
```

### worker_blocked

**Current (canonical):**

```json
{
  "type": "worker_blocked",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-04",
  "task_id": "INF-TSK-024-004",
  "reason": "max_rework_iterations exceeded",
  "timestamp": "2026-04-07T10:50:00Z"
}
```

### worker_cancelled

**Current (canonical):**

```json
{
  "type": "worker_cancelled",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "worker_id": "worker-05",
  "task_id": "INF-TSK-024-005",
  "reason": "dependency task failed",
  "timestamp": "2026-04-07T10:20:00Z"
}
```

### worker_pool_full (no worker_id)

**Current (with routing gap):**

```json
{
  "type": "worker_pool_full",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "task_id": "INF-TSK-024-006",
  "timestamp": "2026-04-07T10:05:00Z"
}
```

**Target (after routing fix — same JSON, routing now resolved):**

```json
{
  "type": "worker_pool_full",
  "session_id": "ses-01knn3q7bcmvad1r7zmdcm8xmc",
  "task_id": "INF-TSK-024-006",
  "timestamp": "2026-04-07T10:05:00Z"
}
```

---

## 8. Rust Type Reference

Located in `codeflow-cli/core/src/coordination/types/events.rs`, lines 99-219.

All autorun variants use `#[serde(tag = "type")]` as the discriminator. Fields are fully
typed and named (unlike `LedgerEvent` which uses `#[serde(flatten)] data: serde_json::Value`
for event-specific fields).

```rust
// codeflow-cli/core/src/coordination/types/events.rs:99-219

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum AutorunEvent {
    #[serde(rename = "batch_started")]
    BatchStarted {
        session_id: String,
        batch_name: String,
        total_tasks: i32,
        timestamp: String,
    },

    #[serde(rename = "batch_completed")]
    BatchCompleted {
        session_id: String,
        batch_name: String,
        completed_tasks: i32,
        failed_tasks: i32,
        skipped_tasks: i32,
        timestamp: String,
    },

    #[serde(rename = "batch_aborted")]
    BatchAborted {
        session_id: String,
        batch_name: String,
        reason: String,
        timestamp: String,
    },

    #[serde(rename = "worker_started")]
    WorkerStarted {
        session_id: String,
        worker_id: String,
        task_id: String,
        timestamp: String,
    },

    #[serde(rename = "worker_completed")]
    WorkerCompleted {
        session_id: String,
        worker_id: String,
        task_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pr_number: Option<i64>,
        timestamp: String,
    },

    #[serde(rename = "worker_failed")]
    WorkerFailed {
        session_id: String,
        worker_id: String,
        task_id: String,
        error: String,
        timestamp: String,
    },

    #[serde(rename = "worker_timeout")]
    WorkerTimeout {
        session_id: String,
        worker_id: String,
        task_id: String,
        timestamp: String,
    },

    #[serde(rename = "worker_blocked")]
    WorkerBlocked {
        session_id: String,
        worker_id: String,
        task_id: String,
        reason: String,
        timestamp: String,
    },

    #[serde(rename = "worker_cancelled")]
    WorkerCancelled {
        session_id: String,
        worker_id: String,
        task_id: String,
        reason: String,
        timestamp: String,
    },

    #[serde(rename = "worker_pool_full")]
    WorkerPoolFull {
        session_id: String,
        task_id: String,
        timestamp: String,
        // NOTE: no worker_id — no worker assigned when pool is full
    },
}
```

**Event type discriminators** (`type` field values from `#[serde(rename = ...)]`):

| Variant | `type` field value | Has `worker_id` | Has `reason` | Has `error` |
|---------|-------------------|-----------------|--------------|-------------|
| `BatchStarted` | `"batch_started"` | No | No | No |
| `BatchCompleted` | `"batch_completed"` | No | No | No |
| `BatchAborted` | `"batch_aborted"` | No | Yes | No |
| `WorkerStarted` | `"worker_started"` | Yes | No | No |
| `WorkerCompleted` | `"worker_completed"` | Yes | No | No |
| `WorkerFailed` | `"worker_failed"` | Yes | No | Yes |
| `WorkerTimeout` | `"worker_timeout"` | Yes | No | No |
| `WorkerBlocked` | `"worker_blocked"` | Yes | Yes | No |
| `WorkerCancelled` | `"worker_cancelled"` | Yes | Yes | No |
| `WorkerPoolFull` | `"worker_pool_full"` | **No** | No | No |

**Routing constants:**

| Constant | Value | Location |
|----------|-------|----------|
| `files::AUTORUN_EVENTS` | `"autorun-events"` | `codeflow-cli/core/src/ledger/mod.rs` |
| `files::ALL` | Includes `AUTORUN_EVENTS` | `codeflow-cli/core/src/ledger/mod.rs` |
| `files::CANONICAL` | Does NOT include `AUTORUN_EVENTS` | `codeflow-cli/core/src/ledger/mod.rs` |

---

## 9. Retention and Lifecycle Considerations

### Batch-Scoped vs Session-Scoped Retention

Autorun events are fundamentally **batch-scoped**, not session-scoped. A single batch run
may spawn multiple workers, each with its own worker-level session ID. The `session_id` in
autorun events refers to the batch-level session, not an individual interactive session.

This creates a retention boundary distinction:

| Event Family | Scope | Retention Implication |
|-------------|-------|-----------------------|
| `pathflow-events.jsonl` | Per interactive session | Can be pruned when session is archived |
| `work-graph.jsonl` | Per task lifetime | Retained as long as task records exist |
| `autorun-events.jsonl` | Per batch run | Can be pruned after batch is complete and outcomes recorded in DB |

### Redundancy with DB Tables

The three autorun DB tables (`autorun_session`, `autorun_worker`, `autorun_task_run`) store
outcomes redundantly with autorun-events.jsonl. Unlike other CANONICAL ledger types, autorun
events are NOT rebuilt into the DB from JSONL — the DB is populated directly by the Rust
autorun subsystem. This means:

- **JSONL is the audit log:** `autorun-events.jsonl` serves as the append-only audit
  trail of what happened and when.
- **DB is the query interface:** The `autorun_*` tables serve operational queries
  (worker status, task outcomes, PR numbers) without needing to scan JSONL.
- **No rebuild dependency:** Deleting `autorun-events.jsonl` does not corrupt the DB.
  The DB state was populated at runtime, not from JSONL replay.

### Retention Recommendation

Because autorun events are not in `files::CANONICAL` and the DB is authoritative for
operational queries, autorun-events JSONL can be archived (moved to cold storage) after
a batch completes and outcomes are verified in the DB. The per-session fragment layout
(`.state/ledger/autorun-events/autorun-events-ses-{id}.jsonl`) makes batch-scoped archival
straightforward: archive the fragment file when the batch session ends.

For the base file (`.state/ledger/autorun-events/autorun-events.jsonl`), standard ledger
retention applies: compact completed session fragments into the base file periodically, and
archive the base file when it grows beyond operational query needs.

---

## 10. DB Relationship Analysis

Three SurrealDB tables in `codeflow-cli/core/src/store/schema.surql` store autorun state.
These tables are populated by the Rust autorun subsystem at runtime, not through JSONL replay.

### autorun_session (lines 164-177)

Stores one record per batch run. Maps to the batch-level events (`batch_started`,
`batch_completed`, `batch_aborted`).

| DB Field | Type | JSONL Counterpart |
|----------|------|-------------------|
| `batch_file` | string | Not in JSONL events |
| `batch_name` | option\<string\> | `batch_name` in batch events |
| `status` | string | Inferred from event sequence |
| `max_session_workers` | int | Not in JSONL events |
| `total_tasks` | int | `total_tasks` in `batch_started` |
| `completed_tasks` | int | `completed_tasks` in `batch_completed` |
| `failed_tasks` | int | `failed_tasks` in `batch_completed` |
| `skipped_tasks` | int | `skipped_tasks` in `batch_completed` |
| `pid` | option\<int\> | Not in JSONL events |
| `created_at` | string | `timestamp` in `batch_started` |
| `completed_at` | option\<string\> | `timestamp` in `batch_completed` / `batch_aborted` |

**Gap:** `batch_file`, `max_session_workers`, and `pid` are captured in the DB but not in
JSONL events. The JSONL audit log cannot reconstruct these fields from events alone.

### autorun_worker (lines 179-193)

Stores one record per worker instance. Maps to `worker_started` and terminal worker events
(`worker_completed`, `worker_failed`, `worker_timeout`, `worker_blocked`, `worker_cancelled`).

| DB Field | Type | JSONL Counterpart |
|----------|------|-------------------|
| `session_id` | string | `session_id` in worker events |
| `worker_num` | int | Not in JSONL events |
| `task_id` | string | `task_id` in worker events |
| `status` | string | Inferred from event type |
| `tmux_session` | option\<string\> | Not in JSONL events |
| `worktree_path` | option\<string\> | Not in JSONL events |
| `file_scope` | array\<string\> | Not in JSONL events |
| `scope_policy` | string | Not in JSONL events |
| `worker_session_id` | option\<string\> | Not in JSONL events (worker's own session ID, not batch session_id) |
| `pr_number` | option\<int\> | `pr_number` in `worker_completed` |
| `started_at` | option\<string\> | `timestamp` in `worker_started` |
| `completed_at` | option\<string\> | `timestamp` in terminal worker event |

**Gap:** Worker infrastructure details (`tmux_session`, `worktree_path`, `file_scope`,
`scope_policy`, `worker_num`) are captured in the DB but not in JSONL events. The `worker_id`
in JSONL events maps to `session_id + worker_num` in the DB — not a direct field correspondence.

**Note:** `worker_pool_full` events have no `worker_id` and cannot be correlated with an
`autorun_worker` record. They are batch-level events recorded only in JSONL.

### autorun_task_run (lines 195-215)

Stores the most detailed per-task execution record. Maps to the full lifecycle of a worker's
task execution.

| DB Field | Type | JSONL Counterpart |
|----------|------|-------------------|
| `worker_id` | string | `worker_id` in worker events |
| `task_id` | string | `task_id` in worker events |
| `session_id` | string | `session_id` in worker events |
| `status` | string | Inferred from event sequence |
| `branch_name` | option\<string\> | Not in JSONL events |
| `worktree_path` | option\<string\> | Not in JSONL events |
| `pr_number` | option\<int\> | `pr_number` in `worker_completed` |
| `pr_url` | option\<string\> | Not in JSONL events |
| `blocked_reason` | option\<string\> | `reason` in `worker_blocked` |
| `claim_conflicts` | option\<array\<string\>\> | Not in JSONL events |
| `merge_conflicts` | option\<array\<string\>\> | Not in JSONL (error string in `worker_failed`) |
| `started_at` | option\<string\> | `timestamp` in `worker_started` |
| `completed_at` | option\<string\> | `timestamp` in terminal worker event |
| `duration_seconds` | option\<int\> | Not in JSONL events (computed in DB) |
| `exit_code` | option\<int\> | Not in JSONL events |
| `error_message` | option\<string\> | `error` in `worker_failed` |
| `verification_result` | option\<string\> | Not in JSONL events |
| `created_at` | string | `timestamp` in `worker_started` |

**Gap:** Computed and infrastructure fields (`duration_seconds`, `exit_code`, `pr_url`,
`branch_name`, `claim_conflicts`, `verification_result`) exist only in the DB.

### Summary: JSONL vs DB Coverage

| Data Domain | JSONL Coverage | DB Coverage |
|-------------|---------------|-------------|
| Batch lifecycle (start/end/abort) | Full | Full (plus `batch_file`, `pid`) |
| Worker lifecycle (start/complete/fail) | Full | Full (plus infrastructure details) |
| Task execution details | Partial (key events only) | Full |
| Infrastructure details (worktree, tmux, scope) | None | Full |
| Computed fields (duration, exit_code) | None | Full |
| Pool exhaustion events | Full (`worker_pool_full`) | Not stored |

**Conclusion:** `autorun-events.jsonl` is the audit trail (what happened and when). The
`autorun_*` DB tables are the operational query layer (full task execution context). Neither
is a strict superset of the other. `worker_pool_full` events are uniquely captured only in
JSONL, with no DB table counterpart.
