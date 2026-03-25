---
title: "sessions.jsonl Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-03-25"
updated_at: "2026-03-25"
scope: INF-TSK-024-001
feeds_into:
  - INF-TSK-024-007
  - INF-TSK-024-012
---

# sessions.jsonl Schema Audit

This document audits the schema of `.state/ledger/sessions.jsonl` across all eras of the
CodeFlow ledger, catalogs field inconsistencies, and provides a gap analysis against the
canonical Rust target schema. It feeds directly into INF-TSK-024-007 (canonical event schema
synthesis) and INF-TSK-024-012 (sessions.jsonl migration).

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Shell-Era Schema](#2-shell-era-schema)
- [3. Rust-Era Schema](#3-rust-era-schema)
- [4. Claim Events (Go-Compatibility)](#4-claim-events-go-compatibility)
- [5. Field Inconsistencies Catalog](#5-field-inconsistencies-catalog)
- [6. Variable vs Universal Fields](#6-variable-vs-universal-fields)
- [7. Gap Analysis Against Rust Target](#7-gap-analysis-against-rust-target)
- [8. BEFORE/AFTER Schema Table](#8-beforeafter-schema-table)
- [9. BEFORE/AFTER JSON Examples](#9-beforeafter-json-examples)
- [10. Rust Type Reference](#10-rust-type-reference)

---

## 1. File Location and Layout

**Current (flat layout):** `.state/ledger/sessions.jsonl`

**Target (subdirectory layout, post-migration):**

```text
.state/ledger/
└── sessions/
    ├── sessions.jsonl                      ← base (compacted history)
    └── sessions-ses-{id}.jsonl             ← per-session fragment
```

The subdirectory layout is implemented by `JsonlWriter` in
`codeflow-cli/core/src/ledger/jsonl.rs`. When `session_id` is `Some`, events write to
the session fragment file. When `None`, events write to the base file. The current
`.state/ledger/sessions.jsonl` is the flat-layout base file (pre-migration).

**Writer:** `JsonlWriter::append_event()` in `codeflow-cli/core/src/ledger/jsonl.rs`, using
the `Event` struct from `codeflow-cli/core/src/ledger/mod.rs`.

**Event types routed to `sessions`** (from `codeflow-cli/core/src/ledger/routing.rs`):

```text
session_start | session_end | session_progress | work_claimed
claim_created | claim_released | claim_renewed
```

The last five (`session_progress`, `work_claimed`, `claim_created`, `claim_released`,
`claim_renewed`) are Go-compatibility event types — present in the router for backward
compatibility with entries written by the Go CLI. No typed Rust variants exist for them
in `codeflow-cli/core/src/types/events.rs`.

---

## 2. Shell-Era Schema

The shell era predates the Go CLI and produced a small number of entries (2 entries in the
current base file: lines 1 and 2). These entries use a different field naming convention
from later eras.

### 2.1 session_start (shell era)

**Actual entry from `.state/ledger/sessions.jsonl` line 1:**

```json
{"event":"session_start","interaction_mode":"interactive","session_id":"ses-177137202131769e89b2d5688","timestamp":"2026-02-19T13:45:12Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_start"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Session identifier |
| `interaction_mode` | string | No | `"interactive"` or `"autorun"` — shell-era only |

**Session ID format (shell era):** `ses-{unix_timestamp}{hex_suffix}` (e.g.,
`ses-177137202131769e89b2d5688`). No ULID structure.

### 2.2 session_end (shell era)

**Actual entry from `.state/ledger/sessions.jsonl` line 2:**

```json
{"branch":"chore/inf-tsk-005-007-update-dual-id-docs","event":"session_end","interaction_mode":"interactive","pr_pending":true,"session_id":"ses-177137202131769e89b2d5688","summary":"Updated dual-id-system-findings.md...","timestamp":"2026-02-21T10:37:09Z","work_completed":["INF-TSK-005-007"]}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_end"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Session identifier |
| `interaction_mode` | string | No | Shell-era only — candidate for `details` |
| `branch` | string | No | Ad-hoc — candidate for `details` |
| `pr_pending` | boolean | No | Ad-hoc — candidate for `details` |
| `summary` | string | No | Ad-hoc — candidate for `details` |
| `work_completed` | string[] | No | Array of task format IDs — candidate for `details` |

**Shell-era characteristics:**

- No `claude_id`, `user_host`, or `user_id` fields.
- `session_end` carries ad-hoc context fields (`branch`, `pr_pending`, `summary`,
  `work_completed`) at the top level — not nested.
- Uses `interaction_mode` on both `session_start` and `session_end`.

---

## 3. Rust-Era Schema

The Rust era began after INF-EPC-022 (Rust CLI integration). The current Rust-era entries
appear on lines 3–48 of `.state/ledger/sessions.jsonl`. Events are written by the
`JsonlWriter` struct using the `Event` struct defined in `codeflow-cli/core/src/ledger/mod.rs`.

### 3.1 Event struct (Rust LedgerWriter output format)

The `Event` struct in `codeflow-cli/core/src/ledger/mod.rs:87-107` defines the canonical
on-wire format for all events written by the Rust CLI:

```rust
pub struct Event {
    #[serde(rename = "event")]
    pub event_type: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub worktree: Option<String>,
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}
```

Key serialization behaviors:

- `event_type` serializes as `"event"` (renamed field).
- `session_id` is omitted from JSON when `None` (`skip_serializing_if`).
- `worktree` is omitted from JSON when `None` (`skip_serializing_if`).
- `data` fields are flattened into the top-level JSON object (no nesting).

### 3.2 session_start (Rust era)

**Actual entries from `.state/ledger/sessions.jsonl` lines 7–48:**

```json
{"claude_id":"368d9474-5db0-4fd4-abca-8f18ea6b0add","event":"session_start","session_id":"ses-01kjw2gertbnn8xnpmr8mcrmc8","timestamp":"2026-03-04T09:22:50Z","user_host":"BlackSwan-mPro.local","user_id":"26560960+sathyassn@users.noreply.github.com"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_start"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | ULID-based session ID (e.g., `ses-01kjw2...`) |
| `claude_id` | string (UUID) | No | Per-agent Claude UUID — written by Rust CLI |
| `user_host` | string | No | Hostname of the machine |
| `user_id` | string | No | Git user identity (email or GitHub handle) |

**Session ID format (Rust era):** `ses-{ULID}` (e.g., `ses-01kjw2gertbnn8xnpmr8mcrmc8`).
The `SessionId` newtype in `codeflow-cli/core/src/types/ids.rs:87-91` wraps this string.

**Notable:** `interaction_mode` is absent from Rust-era `session_start` entries. The Rust
CLI does not emit it.

### 3.3 session_end (Rust era)

**Actual entries from `.state/ledger/sessions.jsonl`:**

```json
{"cleanup_completed":true,"event":"session_end","pf7_valid":true,"sentinels_cleaned":1,"session_id":"ses-01kjvsv5rat20cj16np25y45tg","task_preserved":false,"timestamp":"2026-03-04T09:22:13Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_end"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | Yes | Present on all Rust-era entries |
| `cleanup_completed` | boolean | No | Rust-era metadata — candidate for `details` |
| `pf7_valid` | boolean | No | PathFlow PF7 validity flag — candidate for `details` |
| `sentinels_cleaned` | integer | No | Count of sentinels removed — candidate for `details` |
| `task_preserved` | boolean | No | Whether active task was preserved — candidate for `details` |

**Notable:** Rust-era `session_end` entries lack `branch`, `summary`, `pr_pending`, and
`work_completed` that appeared in shell-era entries. The Rust-era fields (`cleanup_completed`,
`pf7_valid`, `sentinels_cleaned`, `task_preserved`) are operational metadata not present in
shell-era entries.

### 3.4 LedgerEvent typed variants

The `LedgerEvent` enum in `codeflow-cli/core/src/types/events.rs:10-245` defines typed Rust
variants for deserialization. The session-relevant variants are:

```rust
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LedgerEvent {
    SessionStart {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    SessionEnd {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        timestamp: Option<String>,
        #[serde(flatten)]
        data: serde_json::Value,
    },
    // ...
}
```

The `data` field uses `serde_json::Value` with `#[serde(flatten)]` to capture all
event-specific fields without a fixed schema — this accommodates both shell-era and
Rust-era field variations.

---

## 4. Claim Events (Go-Compatibility)

The following event types route to `sessions.jsonl` for backward compatibility with the
Go CLI. They appear in `routing.rs` but have no typed variants in `types/events.rs`.
No entries of these types appear in the current `sessions.jsonl` base file.

These are **distinct from** the Rust-era coordination events (`claim_acquired`,
`claim_conflict`, `coord_claim_released`, `scope_expansion`) which route to
`coordination-events.jsonl` and are defined in
`codeflow-cli/core/src/coordination/types/events.rs`.

### 4.1 claim_created

Emitted when a file claim is created (Go-era). Required fields per Go `schema.go`: `id`
(the claim identifier).

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"claim_created"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `id` | string | Yes | Claim identifier (Go-era field name) |
| `session_id` | string | No | Session that created the claim |
| `path` | string | No | File path being claimed |

### 4.2 claim_released

Emitted when a file claim is released (Go-era). Required fields per Go `schema.go`:
`claim_id`.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"claim_released"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `claim_id` | string | Yes | Claim identifier (Go-era field name) |
| `session_id` | string | No | Session releasing the claim |

### 4.3 claim_renewed

Emitted when a file claim TTL is renewed (Go-era). Required fields per Go `schema.go`:
`claim_id`.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"claim_renewed"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `claim_id` | string | Yes | Claim identifier (Go-era field name) |
| `session_id` | string | No | Session renewing the claim |

**Canonical Rust replacement:** The Rust coordination layer uses `CoordinationEvent` in
`codeflow-cli/core/src/coordination/types/events.rs`, which routes to
`coordination-events.jsonl` — not `sessions.jsonl`. The `claim_created` / `claim_released`
/ `claim_renewed` event types in `sessions.jsonl` are Go-era artifacts with no Rust
implementation.

---

## 5. Field Inconsistencies Catalog

### 5.1 Timestamp field name

| Era | Field name | Type | Example |
|-----|-----------|------|---------|
| Shell (pre-Go) | `timestamp` | string (RFC 3339) | `"2026-02-19T13:45:12Z"` |
| Go canonical | `timestamp` | string (RFC 3339) | `"2026-03-04T06:48:42Z"` |
| Rust era | `timestamp` | string (RFC 3339) | `"2026-03-04T09:22:50Z"` |

**Finding:** No field name inconsistency in `sessions.jsonl` — `timestamp` is used
consistently across all eras. The `ts` vs `timestamp` inconsistency noted in the broader
schema analysis applies to other ledger files and shell scripts, but not to the actual
entries in `sessions.jsonl`.

**Canonical target:** `timestamp` as `chrono::DateTime<Utc>` in Rust (serialized as
RFC 3339 string). The string format is already correct in all eras.

### 5.2 session_id presence

| Era | Event type | session_id present? |
|-----|-----------|---------------------|
| Shell | `session_start` | Yes |
| Shell | `session_end` | Yes |
| Rust | `session_start` | Yes (all 46 Rust-era entries observed) |
| Rust | `session_end` | Yes (all observed entries) |

**Finding:** `session_id` is present on all 48 entries in the current base file. However,
the `Event` struct marks it as `Option<String>` with `skip_serializing_if = "Option::is_none"`,
meaning it can be absent. The `LedgerEvent::SessionStart` variant also marks it as
`#[serde(default)] session_id: Option<String>`, accommodating absent entries.

**Gap:** For claim events (`claim_created`, `claim_released`, `claim_renewed`), `session_id`
is not required by Go `schema.go`, creating potential orphaned claim events with no
session traceability.

### 5.3 Interaction mode field

| Era | Field | Present on |
|-----|-------|-----------|
| Shell | `interaction_mode` | `session_start`, `session_end` |
| Rust | (absent) | — |

**Finding:** `interaction_mode` appears only in shell-era entries (lines 1–2 of the base
file). The Rust CLI does not emit it. This is a schema inconsistency: shell-era readers
that depend on `interaction_mode` will not find it on Rust-era entries.

**Canonical target:** `interaction_mode` moves to the `details` object in migrated entries.

### 5.4 Ad-hoc fields on session_end

| Era | Ad-hoc fields (top-level) |
|-----|--------------------------|
| Shell | `branch`, `pr_pending`, `summary`, `work_completed` |
| Rust | `cleanup_completed`, `pf7_valid`, `sentinels_cleaned`, `task_preserved` |

**Finding:** Each era added different ad-hoc context fields to `session_end` at the top
level. These fields are not universal — they are era-specific operational metadata and prime
candidates for migration to a `details` object.

### 5.5 Identity fields

| Era | Fields present on session_start |
|-----|--------------------------------|
| Shell | (none) |
| Go/Rust | `claude_id`, `user_host`, `user_id` |

**Finding:** Go and Rust eras added `claude_id` (per-agent UUID), `user_host` (hostname),
and `user_id` (git identity) to `session_start`. Shell-era entries lack these. These
identity fields are useful cross-session metadata and should remain top-level in the
canonical schema.

### 5.6 Session ID format evolution

| Era | Format | Example |
|-----|--------|---------|
| Shell | `ses-{unix_ms}{hex}` | `ses-177137202131769e89b2d5688` |
| Go/Rust | `ses-{ULID}` | `ses-01kjw2gertbnn8xnpmr8mcrmc8` |

**Finding:** Session ID format changed between eras. Both are valid string values for
`SessionId` (which validates non-empty only). The `SessionId` newtype in `types/ids.rs`
wraps the string with no format enforcement beyond non-empty, so both formats deserialize
correctly.

---

## 6. Variable vs Universal Fields

This classification guides INF-TSK-024-012 (migration to `details` object).

### 6.1 Universal fields (remain top-level)

These fields are expected on every canonical session event:

| Field | Event types | Justification |
|-------|------------|---------------|
| `event` | all | Discriminator — required for routing and deserialization |
| `timestamp` | all | Universal metadata — required for ordering and audit |
| `session_id` | `session_start`, `session_end` | Primary session identifier |
| `claude_id` | `session_start` | Identity metadata — agent traceability; absent on shell-era |
| `user_host` | `session_start` | Identity metadata — machine identity |
| `user_id` | `session_start` | Identity metadata — git identity |
| `worktree` | all (when in worktree mode) | Isolation context — required for per-worktree routing |

### 6.2 Variable fields (candidates for `details` object)

These fields are era-specific, event-type-specific, or ad-hoc:

| Field | Event type | Era | Migration target |
|-------|-----------|-----|-----------------|
| `interaction_mode` | `session_start`, `session_end` | Shell | `details.interaction_mode` |
| `branch` | `session_end` | Shell | `details.branch` |
| `pr_pending` | `session_end` | Shell | `details.pr_pending` |
| `summary` | `session_end` | Shell | `details.summary` |
| `work_completed` | `session_end` | Shell | `details.work_completed` |
| `cleanup_completed` | `session_end` | Rust | `details.cleanup_completed` |
| `pf7_valid` | `session_end` | Rust | `details.pf7_valid` |
| `sentinels_cleaned` | `session_end` | Rust | `details.sentinels_cleaned` |
| `task_preserved` | `session_end` | Rust | `details.task_preserved` |

**Backward compatibility:** Existing flat entries remain readable. Readers must check both
top-level keys and `details.*` keys to handle pre-migration (flat) and post-migration
(nested) entries.

---

## 7. Gap Analysis Against Rust Target

The canonical Rust target is the `Event` struct in `codeflow-cli/core/src/ledger/mod.rs`
plus typed variants in `codeflow-cli/core/src/types/events.rs`.

### 7.1 Fields currently emitted by LedgerWriter

The `Event` struct emits these fixed top-level fields for every event:

| Field | Always present? | Rust source |
|-------|----------------|-------------|
| `event` | Yes | `Event::event_type` (renamed) |
| `timestamp` | Yes | `Event::timestamp` |
| `session_id` | No (omitted if None) | `Event::session_id` |
| `worktree` | No (omitted if None) | `Event::worktree` |
| `{data fields}` | Varies | `Event::data` (flattened HashMap) |

### 7.2 Gaps: fields in canonical target that LedgerWriter lacks

| Gap | Description | Downstream task |
|-----|-------------|----------------|
| No typed event payload | `data` is `HashMap<String, serde_json::Value>` — no compile-time schema per event type | INF-TSK-024-007 |
| No `details` nesting | Ad-hoc fields go into `data` HashMap, flattened to top-level | INF-TSK-024-012 |
| No `WorktreeId` typed field | `worktree` is `Option<String>`; no `WorktreeId` newtype in `types/ids.rs` | INF-TSK-024-007 |
| `session_id` optional at struct level | Can be `None` and omitted; canonical target requires it on session events | INF-TSK-024-007 |

### 7.3 Fields present in LedgerWriter not yet canonical

| Field | Current status | Note |
|-------|---------------|------|
| `claude_id` | Written via `data` HashMap on `session_start` | Should be universal on all Rust-era `session_start` entries |
| `user_host` | Written via `data` HashMap on `session_start` | Same as above |
| `user_id` | Written via `data` HashMap on `session_start` | Same as above |
| `pf7_valid`, `sentinels_cleaned`, etc. | Written via `data` HashMap on `session_end` | Rust-era operational fields; no canonical schema mandates them |

### 7.4 Claim event gap

| Event type | File | Typed struct in Rust | Status |
|-----------|------|---------------------|--------|
| `claim_created` | `sessions` | No | Go-compat only; no Rust implementation |
| `claim_released` | `sessions` | No | Go-compat only; no Rust implementation |
| `claim_renewed` | `sessions` | No | Go-compat only; no Rust implementation |
| `claim_acquired` | `coordination-events` | Yes (`CoordinationEvent::ClaimAcquired`) | Rust-era replacement |
| `coord_claim_released` | `coordination-events` | Yes (`CoordinationEvent::ClaimReleased`) | Rust-era replacement |
| `claim_conflict` | `coordination-events` | Yes (`CoordinationEvent::ClaimConflict`) | New in Rust era |
| `scope_expansion` | `coordination-events` | Yes (`CoordinationEvent::ScopeExpansion`) | New in Rust era |

Go-era claim events (`claim_created`, `claim_released`, `claim_renewed`) in `sessions.jsonl`
have no direct Rust-era equivalent in the same file. The Rust coordination layer writes
`CoordinationEvent` variants to `coordination-events.jsonl` instead.

---

## 8. BEFORE/AFTER Schema Table

### session_start

| Field | BEFORE (current) | AFTER (canonical target) | Notes |
|-------|-----------------|--------------------------|-------|
| `event` | `"session_start"` | `"session_start"` | Unchanged |
| `timestamp` | RFC 3339 string | RFC 3339 string (`chrono::DateTime<Utc>`) | Format unchanged; Rust type enforced at struct level |
| `session_id` | string (optional on struct) | `SessionId` (required) | Made required on `SessionStart` typed struct |
| `claude_id` | string (Rust-era only, in `data`) | string (top-level) | Universal on all Rust-era entries |
| `user_host` | string (Rust-era only, in `data`) | string (top-level) | Universal on all Rust-era entries |
| `user_id` | string (Rust-era only, in `data`) | string (top-level) | Universal on all Rust-era entries |
| `worktree` | string (optional, top-level) | string (optional, top-level) | Unchanged |
| `interaction_mode` | string (top-level, shell-era only) | `details.interaction_mode` | Moved to `details` object |

### session_end

| Field | BEFORE (current) | AFTER (canonical target) | Notes |
|-------|-----------------|--------------------------|-------|
| `event` | `"session_end"` | `"session_end"` | Unchanged |
| `timestamp` | RFC 3339 string | RFC 3339 string (`chrono::DateTime<Utc>`) | Format unchanged; Rust type enforced |
| `session_id` | string (optional on struct) | `SessionId` (required) | Made required |
| `worktree` | string (optional, top-level) | string (optional, top-level) | Unchanged |
| `interaction_mode` | string (top-level, shell-era) | `details.interaction_mode` | Moved to `details` |
| `branch` | string (top-level, shell-era) | `details.branch` | Moved to `details` |
| `pr_pending` | boolean (top-level, shell-era) | `details.pr_pending` | Moved to `details` |
| `summary` | string (top-level, shell-era) | `details.summary` | Moved to `details` |
| `work_completed` | string[] (top-level, shell-era) | `details.work_completed` | Moved to `details` |
| `cleanup_completed` | boolean (top-level, Rust-era) | `details.cleanup_completed` | Moved to `details` |
| `pf7_valid` | boolean (top-level, Rust-era) | `details.pf7_valid` | Moved to `details` |
| `sentinels_cleaned` | integer (top-level, Rust-era) | `details.sentinels_cleaned` | Moved to `details` |
| `task_preserved` | boolean (top-level, Rust-era) | `details.task_preserved` | Moved to `details` |

---

## 9. BEFORE/AFTER JSON Examples

### 9.1 Shell-era session_start (BEFORE)

Actual entry from `.state/ledger/sessions.jsonl` line 1:

```json
{
  "event": "session_start",
  "interaction_mode": "interactive",
  "session_id": "ses-177137202131769e89b2d5688",
  "timestamp": "2026-02-19T13:45:12Z"
}
```

Issues: no `claude_id`, `user_host`, `user_id`; `interaction_mode` at top-level.

### 9.2 Shell-era session_start (AFTER — canonical target)

```json
{
  "event": "session_start",
  "session_id": "ses-177137202131769e89b2d5688",
  "timestamp": "2026-02-19T13:45:12Z",
  "details": {
    "interaction_mode": "interactive"
  }
}
```

### 9.3 Shell-era session_end (BEFORE)

Actual entry from `.state/ledger/sessions.jsonl` line 2:

```json
{
  "branch": "chore/inf-tsk-005-007-update-dual-id-docs",
  "event": "session_end",
  "interaction_mode": "interactive",
  "pr_pending": true,
  "session_id": "ses-177137202131769e89b2d5688",
  "summary": "Updated dual-id-system-findings.md and codeflow-revamp-proposal-v2.md with historical context notes for the old verbose format ID convention. Session disconnected before PR creation; work complete, 4 commits on branch.",
  "timestamp": "2026-02-21T10:37:09Z",
  "work_completed": ["INF-TSK-005-007"]
}
```

Issues: `interaction_mode`, `branch`, `pr_pending`, `summary`, `work_completed` at
top-level.

### 9.4 Shell-era session_end (AFTER — canonical target)

```json
{
  "event": "session_end",
  "session_id": "ses-177137202131769e89b2d5688",
  "timestamp": "2026-02-21T10:37:09Z",
  "details": {
    "branch": "chore/inf-tsk-005-007-update-dual-id-docs",
    "interaction_mode": "interactive",
    "pr_pending": true,
    "summary": "Updated dual-id-system-findings.md...",
    "work_completed": ["INF-TSK-005-007"]
  }
}
```

### 9.5 Rust-era session_start (BEFORE)

Actual entry from `.state/ledger/sessions.jsonl` line 7:

```json
{
  "claude_id": "368d9474-5db0-4fd4-abca-8f18ea6b0add",
  "event": "session_start",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-04T09:22:50Z",
  "user_host": "BlackSwan-mPro.local",
  "user_id": "26560960+sathyassn@users.noreply.github.com"
}
```

### 9.6 Rust-era session_start (AFTER — canonical target)

```json
{
  "event": "session_start",
  "session_id": "ses-01kjw2gertbnn8xnpmr8mcrmc8",
  "timestamp": "2026-03-04T09:22:50Z",
  "claude_id": "368d9474-5db0-4fd4-abca-8f18ea6b0add",
  "user_host": "BlackSwan-mPro.local",
  "user_id": "26560960+sathyassn@users.noreply.github.com"
}
```

This entry already conforms closely to the canonical target — no `details` migration
needed. Field ordering is illustrative; JSON objects are unordered.

### 9.7 Rust-era session_end (BEFORE)

Actual entry from `.state/ledger/sessions.jsonl` line 6:

```json
{
  "cleanup_completed": true,
  "event": "session_end",
  "pf7_valid": true,
  "sentinels_cleaned": 1,
  "session_id": "ses-01kjvsv5rat20cj16np25y45tg",
  "task_preserved": false,
  "timestamp": "2026-03-04T09:22:13Z"
}
```

Issues: `cleanup_completed`, `pf7_valid`, `sentinels_cleaned`, `task_preserved` at
top-level.

### 9.8 Rust-era session_end (AFTER — canonical target)

```json
{
  "event": "session_end",
  "session_id": "ses-01kjvsv5rat20cj16np25y45tg",
  "timestamp": "2026-03-04T09:22:13Z",
  "details": {
    "cleanup_completed": true,
    "pf7_valid": true,
    "sentinels_cleaned": 1,
    "task_preserved": false
  }
}
```

---

## 10. Rust Type Reference

These types exist in the current codebase and will be used when INF-TSK-024-007 defines
the canonical typed event schema.

### 10.1 SessionId

**Location:** `codeflow-cli/core/src/types/ids.rs:87-91`

A `#[serde(transparent)]` newtype wrapper around `String`. Validates non-empty on
construction. Maps to the `session_id` field in session events.

**Current usage:** `session_id: Option<String>` in `Event` struct and `LedgerEvent`
variants. The canonical target makes `SessionId` a required field on typed `SessionStart`
and `SessionEnd` structs.

**Format:** `ses-{ULID}` (Rust era) or `ses-{unix_ms}{hex}` (shell era). Both are valid
non-empty strings.

### 10.2 WorkId

**Location:** `codeflow-cli/core/src/types/ids.rs:105-109`

Maps to the `work_id` field in `begin_work` / `complete_work` events (`work-graph.jsonl`).
Not present in `sessions.jsonl`.

### 10.3 FormatId

**Location:** `codeflow-cli/core/src/types/ids.rs:117-121`

A human-readable format identifier (e.g., `INF-TSK-022-006`). Maps to `format_id` fields
across ledger files.

### 10.4 WorktreeId (not yet defined)

**Finding:** No `WorktreeId` newtype exists in `codeflow-cli/core/src/types/ids.rs` as of
this audit. The `worktree` field on the `Event` struct is `Option<String>`. A `WorktreeId`
type is expected to be defined as part of INF-TSK-024-007's canonical schema synthesis.

### 10.5 LedgerEvent enum

**Location:** `codeflow-cli/core/src/types/events.rs:10-245`

Defines 32 typed variants discriminated by `"event"` tag, with `#[serde(tag = "event",
rename_all = "snake_case")]`. Session-relevant variants:

- `LedgerEvent::SessionStart` — `session_id: Option<String>`, `timestamp: Option<String>`,
  `data: serde_json::Value` (flattened)
- `LedgerEvent::SessionEnd` — same structure

The `data: serde_json::Value` with `#[serde(flatten)]` absorbs all unrecognized fields,
enabling backward compatibility with both shell-era and Rust-era entries. No typed
variants exist for `claim_created`, `claim_released`, or `claim_renewed`.
