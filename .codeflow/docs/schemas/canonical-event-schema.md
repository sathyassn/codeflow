---
title: "Canonical Event Schema"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-07"
updated_at: "2026-04-07"
scope: INF-TSK-024-007
feeds_into:
  - INF-TSK-024-008
  - INF-TSK-024-009
  - INF-TSK-024-010
  - INF-TSK-024-011
  - INF-TSK-024-012
---

# Canonical Event Schema

This document is the normative reference for all JSONL event types written by the
CodeFlow ledger. It defines universal required fields, per-event-type field tables,
deprecated field mappings, and Rust type proposals for all events across the four
canonical ledger files and the pathflow-events log.

Downstream code tasks (INF-TSK-024-008 through 012) use this document as the field
contract for implementation.

## Table of Contents

- [1. Overview](#1-overview)
- [2. Universal Required Fields](#2-universal-required-fields)
- [3. Details Object Pattern](#3-details-object-pattern)
- [4. Deprecated Field Mapping](#4-deprecated-field-mapping)
- [5. Event Types by File](#5-event-types-by-file)
  - [5.1 sessions.jsonl](#51-sessionsjsonl)
  - [5.2 work-graph.jsonl](#52-work-graphjsonl)
  - [5.3 memory-events.jsonl](#53-memory-eventsjsonl)
  - [5.4 config.jsonl](#54-configjsonl)
  - [5.5 pathflow-events.jsonl](#55-pathflow-eventsjsonl)
- [6. Rust Type Proposals](#6-rust-type-proposals)
- [7. Error Type Proposals](#7-error-type-proposals)
- [8. Backward Compatibility](#8-backward-compatibility)

---

## 1. Overview

### 1.1 Ledger File Family

Four files constitute the **canonical ledger** (synced to SurrealDB):

| File | Ledger constant | Event types |
|------|----------------|-------------|
| `sessions.jsonl` | `files::SESSIONS` | 2 active + 5 Go-compat |
| `work-graph.jsonl` | `files::WORK_GRAPH` | 8 primary + 9 Go-compat |
| `memory-events.jsonl` | `files::MEMORY_EVENTS` | 8 |
| `config.jsonl` | `files::CONFIG` | 2 |

One additional file is **not canonical** (not synced to SurrealDB):

| File | Ledger constant | Event types |
|------|----------------|-------------|
| `pathflow-events.jsonl` | `files::PATHFLOW_EVENTS` | 5 |

### 1.2 Note on Security Log Files

Security log files under `.state/logs/security/` (audit, blocked, network, protection
subdirectories) are **outside the JSONL ledger family**. They were written by retired
shell enforcement scripts and are no longer produced by any active code path. They are
not routed, not synced to SurrealDB, and use a different schema family
(`ts` instead of `timestamp`, pretty-printed JSON instead of compact JSONL). These
files are explicitly out of scope for the canonical ledger schema.

### 1.3 Note on memory-events.jsonl Field Naming

`memory-events.jsonl` uses a **different canonical field naming convention** from the
other four ledger files. The discriminator is `event_type` (not `event`), and the
timestamp is `created_at` (not `timestamp`). This is a legacy inconsistency. The
Rust `MemoryEvent` model (`codeflow-cli/core/src/models/memory.rs`) uses `event_type`
and `created_at`. The sections below document the per-file canonical names explicitly.

---

## 2. Universal Required Fields

Every canonically conformant event in `sessions.jsonl`, `work-graph.jsonl`,
`config.jsonl`, and `pathflow-events.jsonl` must include these three fields:

| Field | Rust type | Serialized name | Format | Notes |
|-------|-----------|----------------|--------|-------|
| `timestamp` | `chrono::DateTime<Utc>` | `"timestamp"` | RFC 3339 (`2026-03-04T09:22:50Z`) | Always UTC with Z suffix |
| `event` | `EventType` (string discriminator) | `"event"` | snake_case string | Routes and discriminates the event variant |
| `session_id` | `SessionId` | `"session_id"` | `ses-{ULID}` (Rust era) or `ses-{unix_ms}{hex}` (shell era) | Required on all new events; optional on legacy entries |

`memory-events.jsonl` uses a different convention (see Section 1.3):

| Field | Rust type | Serialized name | Format | Notes |
|-------|-----------|----------------|--------|-------|
| `created_at` | `chrono::DateTime<Utc>` | `"created_at"` | RFC 3339 | Canonical timestamp field for memory events |
| `event_type` | string | `"event_type"` | snake_case string | Discriminator for memory events |
| `id` | string | `"id"` | `memory-{26-char-ULID}` | Required; identifies the memory record |

### 2.1 Optional Universal Fields

These fields are optional on all events but have a standardized meaning when present:

| Field | Type | Meaning |
|-------|------|---------|
| `worktree` | string | Present when the event was written inside a git worktree session. Value is the worktree directory name (e.g., `worktree-ses-01kjw2...`). |

### 2.2 Current Rust Implementation

The `Event` struct in `codeflow-cli/core/src/ledger/mod.rs` is the transport type used by
`JsonlWriter`. Its fields map to the universal schema as follows:

```rust
pub struct Event {
    #[serde(rename = "event")]
    pub event_type: String,        // → serialized as "event"
    pub timestamp: String,         // → RFC 3339 string
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>, // → omitted when None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,   // → omitted when None
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>, // → event-specific fields at top level
}
```

**Gap:** `session_id` is `Option<String>` at the struct level, permitting omission. The
canonical target requires `session_id` on all new events. This is enforced at the call
site by event-specific typed structs (see Section 6).

**Gap:** No `WorktreeId` newtype exists in `codeflow-cli/core/src/types/ids.rs`. The
`worktree` field is `Option<String>`. A `WorktreeId` type is proposed in Section 6.

---

## 3. Details Object Pattern

Variable event-specific content belongs in a `details` object, not as ad-hoc top-level
fields. This prevents unbounded schema sprawl and makes universal fields identifiable by
their position.

### 3.1 Rule

- **Universal fields** (`event`, `timestamp`, `session_id`, `worktree`) remain at the
  top level.
- **Primary identity fields** specific to an event type remain at the top level
  (e.g., `id` on `epic_created`, `task_id` on `begin_work`).
- **Operational metadata and ad-hoc context** moves to the `details` object.

### 3.2 Concrete Example

Before (current — ad-hoc fields at top level):

```json
{
  "event": "session_end",
  "session_id": "ses-01kjvsv5rat20cj16np25y45tg",
  "timestamp": "2026-03-04T09:22:13Z",
  "cleanup_completed": true,
  "pf7_valid": true,
  "sentinels_cleaned": 1,
  "task_preserved": false
}
```

After (canonical target — metadata in `details`):

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

### 3.3 Fields to Migrate to `details`

The tables in Section 5 mark each ad-hoc field with "candidate for `details`". These
fields are handled by INF-TSK-024-012 (details migration).

---

## 4. Deprecated Field Mapping

Three field name aliases appear in historical entries. All new events must use the
canonical name. Readers must handle both names for backward compatibility with pre-migration
entries.

| Deprecated name | Canonical name | Applies to | Source era |
|----------------|---------------|------------|------------|
| `ts` | `timestamp` | pathflow-events (shell era), security logs (shell era) | Shell |
| `e` | `event` | pathflow-events (shell era, Variant A) | Shell |
| `type` | `event` | pathflow-events (shell era, Variant B) | Shell |

Additional work-graph field renames (not simple aliases — they renamed semantic concepts):

| Deprecated name | Canonical name | Event types | Source era |
|----------------|---------------|-------------|------------|
| `old_status` | `from_status` | `epic_status_changed`, `task_status_changed` | Go era |
| `new_status` | `to_status` | `epic_status_changed`, `task_status_changed` | Go era |
| `from` | `from_status` | `task_status_changed` (agent-written subset) | Ad-hoc |
| `to` | `to_status` | `task_status_changed` (agent-written subset) | Ad-hoc |

`memory-events.jsonl` also uses `event_type` instead of `event` and `created_at` instead
of `timestamp` (see Section 1.3). These are not deprecated aliases — they are the
canonical names for that file family.

---

## 5. Event Types by File

### 5.1 sessions.jsonl

**Canonical event types:**

| Event type | Status |
|-----------|--------|
| `session_start` | Active — written by Rust CLI |
| `session_end` | Active — written by Rust CLI |

**Go-compatibility event types** (in router, no typed Rust variants, no active writers):

| Event type | Status |
|-----------|--------|
| `session_progress` | Go-compat only |
| `work_claimed` | Go-compat only |
| `claim_created` | Go-compat only |
| `claim_released` | Go-compat only |
| `claim_renewed` | Go-compat only |

#### session_start

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"session_start"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Yes | `ses-{ULID}` format in Rust era |
| `worktree` | string | No | Yes | Present in worktree-isolated sessions |
| `claude_id` | string (UUID) | No | Yes (top-level) | Per-agent Claude UUID; absent on shell-era entries |
| `user_host` | string | No | Yes (top-level) | Hostname of the machine |
| `user_id` | string | No | Yes (top-level) | Git user identity (email or GitHub handle) |
| `interaction_mode` | string | No | Move to `details` | Shell-era only (`"interactive"` / `"autorun"`); absent on Rust-era entries |

#### session_end

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"session_end"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Yes | Required on all entries |
| `worktree` | string | No | Yes | Present in worktree-isolated sessions |
| `interaction_mode` | string | No | Move to `details` | Shell-era; absent on Rust-era |
| `branch` | string | No | Move to `details` | Shell-era only |
| `pr_pending` | boolean | No | Move to `details` | Shell-era only |
| `summary` | string | No | Move to `details` | Shell-era only |
| `work_completed` | string[] | No | Move to `details` | Shell-era only; array of task format IDs |
| `cleanup_completed` | boolean | No | Move to `details` | Rust-era operational metadata |
| `pf7_valid` | boolean | No | Move to `details` | Rust-era PathFlow validity flag |
| `sentinels_cleaned` | integer | No | Move to `details` | Rust-era count of removed sentinels |
| `task_preserved` | boolean | No | Move to `details` | Rust-era active task preservation flag |

---

### 5.2 work-graph.jsonl

**Canonical event types:**

| Event type | Status |
|-----------|--------|
| `epic_created` | Active |
| `epic_status_changed` | Active |
| `task_created` | Active |
| `task_status_changed` | Active |
| `begin_work` | Active |
| `complete_work` | Active |
| `pr_created` | Active |
| `pr_merged` | Defined; no active emitter |

**Go-compatibility event types** (present in router; no active Rust writers for most):

`task_updated`, `task_id_corrected`, `task_cancelled`, `work_complete` (alias for
`complete_work`), `commit`, `work_finding`, `void`, `work_cancelled`, `stale_work_cleanup`

#### epic_created

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"epic_created"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | Often missing in early entries |
| `worktree` | string | No | Yes | Present in worktree sessions |
| `id` | `EpicId` | Yes (Rust era) | Yes | Internal ULID key (`epic-01KJ...`); absent on Go-era entries that use `epic_id` |
| `format_id` | `FormatId` | No | Yes (top-level) | Human-readable ID (e.g., `INF-EPC-008`) |
| `title` | string | Yes | Yes (top-level) | Epic title |
| `status` | string | No | Yes (top-level) | Initial status (`"planning"`, `"draft"`, `"in_progress"`) |
| `area_type` | string | No | Yes (top-level) | Area code (e.g., `"INF"`) |
| `domain` | string | No | Yes (top-level) | Domain code (e.g., `"PMGT"`) |
| `work_type` | string | No | Yes (top-level) | Work type (e.g., `"CHOR"`) |
| `is_ongoing` | boolean | No | Move to `details` | Present on some entries |
| `origin` | string | No | Move to `details` | E.g., `"planned"` |

#### epic_status_changed

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"epic_status_changed"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | |
| `worktree` | string | No | Yes | |
| `epic_id` | `EpicId` | Yes | Yes (top-level) | Internal ULID of the epic |
| `format_id` | `FormatId` | No | Yes (top-level) | Human-readable epic ID |
| `from_status` | string | Yes | Yes (top-level) | Previous status (Rust era; replaces `old_status`) |
| `to_status` | string | Yes | Yes (top-level) | New status (Rust era; replaces `new_status`) |
| `old_status` | string | Yes (Go era) | Deprecated | Use `from_status` in new events |
| `new_status` | string | Yes (Go era) | Deprecated | Use `to_status` in new events |
| `reason` | string | No | Move to `details` | Ad-hoc context |
| `trigger` | string | No | Move to `details` | Trigger description (e.g., `"all_tasks_complete"`) |
| `task_count` | integer | No | Move to `details` | Count of tasks at time of change |
| `task_id` | string | No | Move to `details` | Triggering task ULID |
| `task_format_id` | string | No | Move to `details` | Triggering task format ID |
| `all_tasks_complete` | boolean | No | Move to `details` | Derived flag |

#### task_created

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"task_created"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | |
| `worktree` | string | No | Yes | |
| `id` | `TaskId` | Yes (Rust era) | Yes (top-level) | Internal ULID key (`task-01KH...`) |
| `format_id` | `FormatId` | No | Yes (top-level) | Human-readable task ID |
| `epic_id` | `EpicId` | Yes | Yes (top-level) | Parent epic internal ULID |
| `epic_format_id` | `FormatId` | No | Yes (top-level) | Parent epic human-readable ID |
| `title` | string | Yes | Yes (top-level) | Task title |
| `status` | string | No | Yes (top-level) | Initial status (e.g., `"todo"`) |
| `area_type` | string | No | Yes (top-level) | Area code |
| `domain` | string | No | Yes (top-level) | Domain code |
| `work_type` | string | No | Yes (top-level) | Work type |
| `note` | string | No | Move to `details` | Ad-hoc context (e.g., `"retroactive_registration"`) |
| `estimate` | string | No | Move to `details` | Effort estimate (e.g., `"M"`) |
| `priority` | string | No | Move to `details` | Priority level |
| `origin` | string | No | Move to `details` | E.g., `"planned"`, `"informal"` |
| `branch` | string | No | Move to `details` | Branch when task was created |
| `file_scope` | array | No | Move to `details` | File scope list |
| `acceptance` | array | No | Move to `details` | Acceptance criteria list |

#### task_status_changed

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"task_status_changed"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | |
| `worktree` | string | No | Yes | |
| `task_id` | `TaskId` | Yes | Yes (top-level) | Task ULID in Rust era; overloaded in Go era |
| `format_id` | `FormatId` | No | Yes (top-level) | Human-readable task ID |
| `from_status` | string | Yes | Yes (top-level) | Previous status (Rust era) |
| `to_status` | string | Yes | Yes (top-level) | New status (Rust era) |
| `old_status` | string | Yes (Go era) | Deprecated | Use `from_status` in new events |
| `new_status` | string | Yes (Go era) | Deprecated | Use `to_status` in new events |
| `from` | string | No | Deprecated | Ad-hoc variant; use `from_status` |
| `to` | string | No | Deprecated | Ad-hoc variant; use `to_status` |
| `branch` | string | No | Move to `details` | Active branch at status change |
| `summary` | string | No | Move to `details` | Completion summary |
| `pr_number` | integer | No | Move to `details` | PR number for completed tasks |
| `completed_at` | string | No | Move to `details` | Completion timestamp |
| `work_id` | `WorkId` | No | Move to `details` | Associated work session ID |
| `reason` | string | No | Move to `details` | Ad-hoc reason (e.g., `"PR #39 merged to main"`) |

#### begin_work

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"begin_work"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Yes | Required on `begin_work` events |
| `worktree` | string | No | Yes | |
| `task_id` | `TaskId` | Yes | Yes (top-level) | Task ULID |
| `format_id` | `FormatId` | No | Yes (top-level) | Human-readable task ID |
| `work_id` | `WorkId` | Yes | Yes (top-level) | Work session ID (`work-01KJ...`) |
| `id` | string | No | Deprecated | Duplicates `work_id` on some entries; use `work_id` |
| `branch` | string | No | Move to `details` | Active branch at work start |
| `topic` | string | No | Move to `details` | Work topic description |
| `domain` | string | No | Move to `details` | Domain code |
| `epic_id` | `EpicId` | No | Move to `details` | Parent epic (inconsistently present) |

#### complete_work

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"complete_work"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | |
| `worktree` | string | No | Yes | |
| `id` | `WorkId` | Yes | Yes (top-level) | Work session ID; maps to `active_work.id` |
| `task_id` | `TaskId` | Yes | Yes (top-level) | Task ULID |
| `format_id` | `FormatId` | No | Yes (top-level) | Human-readable task ID |
| `pr_number` | integer | No | Move to `details` | PR number when work completed with a PR |
| `summary` | string | No | Move to `details` | Completion summary |

#### pr_created

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"pr_created"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | |
| `worktree` | string | No | Yes | |
| `pr_number` | integer | Yes | Yes (top-level) | GitHub PR number |
| `pr_url` | string | Yes | Yes (top-level) | Full PR URL |
| `task_id` | `TaskId` | Yes | Yes (top-level) | Task ULID |
| `task_format_id` | `FormatId` | No | Yes (top-level) | Human-readable task ID |
| `work_id` | `WorkId` | No | Yes (top-level) | Associated work session ID |
| `branch` | string | No | Move to `details` | Source branch |
| `epic_format_id` | `FormatId` | No | Move to `details` | Parent epic ID |

#### pr_merged

**Note:** No active Rust emitter. No `PrMerged` variant exists in `LedgerEvent`. The event type is routable but no entries exist in the ledger. Schema is forward-looking.

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"pr_merged"` |
| `timestamp` | RFC 3339 string | Yes | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | No (legacy) / Yes (new) | Yes | |
| `worktree` | string | No | Yes | |
| `pr_number` | integer | Yes | Yes (top-level) | GitHub PR number |
| `task_id` | `TaskId` | Yes | Yes (top-level) | Task ULID |
| `task_format_id` | `FormatId` | No | Yes (top-level) | Human-readable task ID |

---

### 5.3 memory-events.jsonl

**Canonical event types** (discriminator field: `event_type`, timestamp field: `created_at`):

| `event_type` value | `LedgerEvent` variant | Status |
|--------------------|----------------------|--------|
| `milestone` | `Milestone` | Active |
| `progress` | `Progress` | Active |
| `decision` | `Decision` | Active |
| `finding` | `Finding` | Active |
| `session_summary` | `SessionSummary` | Active |
| `memory_event` | `MemoryEvent` | Active |
| `memory_milestone` | `MemoryMilestone` | Active |
| `memory_store` | `MemoryStore` | Legacy wrapper — unwrap to inner `event_type` |

Note: `memory_stored` and `blocker` are Go-compatibility types in the router with no
active writers.

#### Canonical schema for memory events

```json
{
  "event_type": "<canonical-type>",
  "id": "memory-<26-char-ULID>",
  "domain": "<domain-string>",
  "work_id": "<work-id or null>",
  "data": { },
  "memory_type": "<episodic|semantic|procedural|stage|null>",
  "created_at": "<RFC-3339-timestamp>"
}
```

**Required fields:** `event_type`, `id`, `data`, `created_at`
**Optional fields:** `domain`, `work_id`, `memory_type`

#### memory event field table

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event_type` | string | Yes | Discriminator (`"milestone"`, `"progress"`, etc.) |
| `id` | string | Yes | `memory-{26-char-ULID}` |
| `data` | object | Yes | Event-specific payload (structured JSON object, not a string) |
| `created_at` | RFC 3339 string | Yes | Canonical timestamp for memory events |
| `domain` | string | No | Domain string (e.g., `"development"`, `"infrastructure"`) |
| `work_id` | `WorkId` | No | Associated work session |
| `memory_type` | string | No | Category: `"episodic"`, `"semantic"`, `"procedural"`, `"stage"` |

#### Historical migration patterns

Memory events exist in 4 structural patterns requiring different migration treatment
(covered in INF-TSK-024-009):

| Pattern | Share | Migration needed |
|---------|-------|-----------------|
| Direct canonical events | ~84% | Rename `event`→`event_type`, `timestamp`→`created_at`, wrap loose fields into `data` |
| `memory_store` wrapper | ~16% | Hoist `event_type`, `id`, etc. from nested `data` to top level; parse `data.data` JSON string |
| Non-canonical `event` values | ~10% | Map `event`→`event_type` using canonical enum |
| Hybrid (already has `event_type`) | ~10% | Remove redundant `event` field |

---

### 5.4 config.jsonl

**Note:** `config.jsonl` currently has zero entries. All entries are forward-looking.

**Canonical event types:**

| Event type | Status |
|-----------|--------|
| `config_set` | Defined; no active emitter yet |
| `config_updated` | Defined; no active emitter yet |

#### config_set

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"config_set"` |
| `timestamp` | RFC 3339 string (ms precision) | Yes | Yes | UTC ISO 8601 with milliseconds |
| `session_id` | `SessionId` | Yes | Yes | Promoted to required for all new events |
| `worktree` | string | No | Yes | |
| `key` | string | Yes | Yes (top-level) | Dot-separated config path (e.g., `"logging.prompts.enabled"`) |
| `value` | any | Yes | Yes (top-level) | New value after set |
| `previous_value` | any | No | Yes (top-level) | Prior value; `null` for first set |
| `source` | string | No | Yes (top-level) | Who changed it: `"user"`, `"autorun"`, `"migration"` |

#### config_updated

| Field | Type | Required | Canonical? | Notes |
|-------|------|----------|-----------|-------|
| `event` | string | Yes | Yes | Always `"config_updated"` |
| `timestamp` | RFC 3339 string (ms precision) | Yes | Yes | UTC ISO 8601 with milliseconds |
| `session_id` | `SessionId` | Yes | Yes | Required |
| `worktree` | string | No | Yes | |
| `key` | string | Yes | Yes (top-level) | Dot-separated config path |
| `value` | any | Yes | Yes (top-level) | New value |
| `previous_value` | any | No | Yes (top-level) | Prior value before update |
| `source` | string | No | Yes (top-level) | Who changed it |

---

### 5.5 pathflow-events.jsonl

Not in the `CANONICAL` set — not synced to SurrealDB. Read by `codeflow doctor` from the
legacy path `.state/logs/pathflow-events.jsonl`. Current ledger location is under
`.state/ledger/pathflow-events/`.

**Canonical event types:**

| Event type | `LedgerEvent` variant |
|-----------|----------------------|
| `phase_transition` | `PhaseTransition` |
| `stage_transition` | `StageTransition` |
| `session_register` | `SessionRegister` |
| `session_metadata` | `SessionMetadata` |
| `pathflow_task_update` | `PathflowTaskUpdate` |

All pathflow event variants include `session_id`, `timestamp`, and optional `worktree`
as explicit named fields (not in the `data` catch-all). See `codeflow-cli/core/src/types/events.rs:175-230`.

#### phase_transition

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"phase_transition"` |
| `timestamp` | RFC 3339 string | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Session identifier |
| `worktree` | string | No | Present in worktree sessions |
| `phase` | string | Yes | `PF1-INIT` through `PF7-END` |
| `status` | string | Yes | `entered` / `completed` / `skipped` |
| `task_id` | string | No | Optional task reference |
| `work_type` | string | No | Optional work type (e.g., `"FEAT"`) |

#### stage_transition

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"stage_transition"` |
| `timestamp` | RFC 3339 string | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Session identifier |
| `worktree` | string | No | Present in worktree sessions |
| `stage` | string | Yes | `WS-DEV` / `WS-PLAN` / `WS-DOCS` / `WS-TEST` / `WS-REV` / `WS-QA` |
| `status` | string | Yes | `pending` / `in_progress` / `complete` / `failed` |
| `iteration` | integer | Yes | Rework iteration count (>= 1) |
| `verdict` | string | No | `pass` / `fail` / `approved` / `changes_requested` |

#### session_register

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_register"` |
| `timestamp` | RFC 3339 string | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Session identifier |
| `worktree` | string | No | Present in worktree sessions |
| `tracking_level` | string | Conditional | `"pending"` / `"tracked"` / `"untracked"` — present on first event of pair |
| `interaction_mode` | string | Conditional | `"interactive"` / `"autorun"` — present on second event of pair |

Note: The Rust emitter writes two separate `session_register` events per call. Each event
carries exactly one of `tracking_level` or `interaction_mode`.

#### session_metadata

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_metadata"` |
| `timestamp` | RFC 3339 string | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Session identifier |
| `worktree` | string | No | Present in worktree sessions |
| `key` | string | Yes | Metadata key (e.g., `"work_type"`, `"branch"`) |
| `value` | string | Yes | Metadata value |

Shell-era deprecated fields on `session_metadata`: `type` (use `event`), `ts` (use
`timestamp`), `id` with `EVT-{ULID}` prefix (remove).

#### pathflow_task_update

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"pathflow_task_update"` |
| `timestamp` | RFC 3339 string | Yes | UTC ISO 8601 |
| `session_id` | `SessionId` | Yes | Session identifier |
| `worktree` | string | No | Present in worktree sessions |
| `task_id` | string | Yes | Format: `PF[1-7]-TSK-[0-9]{2}` (e.g., `PF3-TSK-01`) |
| `task_status` | string | Yes | `pending` / `in_progress` / `completed` / `skipped` / `blocked` |

---

## 6. Rust Type Proposals

### 6.1 Existing Types (use as-is)

The following types in `codeflow-cli/core/src/types/ids.rs` are already defined and should
be used in canonical event structs without modification:

| Type | Definition | Maps to field |
|------|-----------|--------------|
| `SessionId` | `ids.rs:87-91` | `session_id` on all ledger events |
| `TaskId` | `ids.rs:93-97` | `task_id` on work-graph events |
| `EpicId` | `ids.rs:99-103` | `epic_id` on work-graph events |
| `WorkId` | `ids.rs:105-109` | `work_id` on work-graph events |
| `FormatId` | `ids.rs:117-121` | `format_id` on work-graph events |

All types are `#[serde(transparent)]` newtype wrappers around `String`, created via the
`define_id!` macro. All validate non-empty on construction via `new()`. All implement
`Serialize`, `Deserialize`, `Display`, `FromStr`, and `AsRef<str>`.

### 6.2 New Type Proposals

#### WorktreeId

The `worktree` field on the `Event` struct is currently `Option<String>`. A typed newtype
should follow the same pattern as other ID newtypes:

```rust
define_id!(
    /// A worktree directory identifier (e.g., `worktree-ses-01kjw2gertbnn8xnpmr8mcrmc8`).
    WorktreeId,
    "worktree id"
);
```

Location: `codeflow-cli/core/src/types/ids.rs`, alongside the other ID newtypes.

Usage: Replace `worktree: Option<String>` with `worktree: Option<WorktreeId>` on the
`Event` struct and on typed event variants that include it.

### 6.3 CanonicalEvent Struct

The current `Event` struct (`codeflow-cli/core/src/ledger/mod.rs`) is a generic transport
type. A `CanonicalEvent` struct provides the typed canonical form for new event writing.
It should live in `codeflow-cli/core/src/types/` (a new module or alongside `events.rs`):

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use crate::types::ids::SessionId;

/// Canonical typed event for writing to the ledger.
///
/// All new events must be constructed via `CanonicalEvent` rather than the
/// generic `Event` struct in `ledger/mod.rs`. This enforces `session_id` as
/// required and provides typed fields for the universal schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalEvent {
    /// Event type discriminator (snake_case).
    #[serde(rename = "event")]
    pub event_type: EventType,

    /// UTC timestamp. Serialized as RFC 3339 string (e.g., `"2026-03-04T09:22:50Z"`).
    /// Use a custom RFC 3339 serializer; do not use `chrono::serde::ts_seconds`
    /// (that serializes as a Unix integer, not an ISO 8601 string).
    pub timestamp: DateTime<Utc>,

    /// Session that produced this event. Required on all new events.
    pub session_id: SessionId,

    /// Worktree context, present when the session runs in an isolated worktree.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeId>,

    /// Event-specific payload. Variable-content fields use `details` sub-object;
    /// primary identity fields appear at top level via flatten.
    #[serde(flatten)]
    pub payload: serde_json::Value,
}
```

**Design notes:**

- `session_id` is `SessionId` (not `Option<SessionId>`), enforcing the requirement at
  compile time.
- `timestamp` uses `chrono::DateTime<Utc>` instead of a bare string, enforcing UTC and
  RFC 3339 format at the type level.
- `worktree` uses the proposed `WorktreeId` newtype.
- `payload` uses `serde_json::Value` with `#[serde(flatten)]` to retain backward
  compatibility with the existing event type variety.

### 6.4 EventType String Enum

An `EventType` string enum representing all canonical event type discriminators:

```rust
/// Canonical event type discriminator strings.
///
/// Covers all event types recognized by `route_event_type()` in
/// `codeflow-cli/core/src/ledger/routing.rs`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    // sessions.jsonl
    SessionStart,
    SessionEnd,

    // work-graph.jsonl
    EpicCreated,
    EpicStatusChanged,
    TaskCreated,
    TaskStatusChanged,
    BeginWork,
    CompleteWork,
    PrCreated,
    PrMerged, // routable but no active emitter; no LedgerEvent variant

    // memory-events.jsonl
    Milestone,
    Progress,
    Decision,
    Finding,
    SessionSummary,
    MemoryEvent,
    MemoryMilestone,

    // config.jsonl
    ConfigSet,
    ConfigUpdated,

    // pathflow-events.jsonl
    PhaseTransition,
    StageTransition,
    SessionRegister,
    SessionMetadata,
    PathflowTaskUpdate,
}
```

Location: `codeflow-cli/core/src/types/events.rs` or a new `codeflow-cli/core/src/types/event_type.rs`.

This type complements the existing `LedgerEvent` enum (which is used for deserialization
of the full event including payload) with a lightweight discriminator-only enum for use
in the `CanonicalEvent` struct.

---

## 7. Error Type Proposals

A `SchemaError` enum for schema validation errors encountered during event reading,
migration, or validation:

```rust
use thiserror::Error;

/// Errors produced during canonical event schema validation.
#[derive(Debug, Error)]
pub enum SchemaError {
    /// A required field is absent from the event.
    #[error("missing required field '{field}' in event '{event_type}'")]
    MissingField {
        event_type: String,
        field: &'static str,
    },

    /// The `timestamp` or `created_at` field cannot be parsed as RFC 3339.
    #[error("invalid timestamp '{value}': {reason}")]
    InvalidTimestamp {
        value: String,
        reason: String,
    },

    /// A deprecated field name is used where the canonical name should appear.
    #[error("deprecated field '{deprecated}' found; use '{canonical}' instead")]
    DeprecatedField {
        deprecated: &'static str,
        canonical: &'static str,
    },

    /// The event type string does not match any canonical event type.
    #[error("unknown event type '{event_type}'")]
    UnknownEventType {
        event_type: String,
    },

    /// The JSON is malformed and cannot be deserialized.
    #[error("malformed JSON: {0}")]
    MalformedJson(#[from] serde_json::Error),
}
```

Location: `codeflow-cli/core/src/types/schema_error.rs`, exported from
`codeflow-cli/core/src/types/mod.rs`.

Dependencies: `thiserror` crate (already used in `codeflow-cli/core`).

---

## 8. Backward Compatibility

### 8.1 Read-Path Compatibility

Readers must handle both pre-migration (flat) and post-migration (nested `details`) entries
for any ledger file. The pattern for safe reading:

1. Check top-level fields first.
2. If not found at top level, check `details.{field}`.
3. For deprecated field names (`ts`, `e`, `type`), fall back to the deprecated name when
   the canonical name is absent.

### 8.2 Existing `LedgerEvent` Enum

The existing `LedgerEvent` enum in `codeflow-cli/core/src/types/events.rs` is used for
deserialization of all existing entries. It uses `#[serde(default)]` on all fields and
`#[serde(flatten)]` on `data: serde_json::Value` to absorb unknown fields. This design
provides backward compatibility with all historical entry shapes.

The proposed `CanonicalEvent` struct (Section 6.3) is for the write path only — new events
written after migration implementation. The `LedgerEvent` enum continues to be used for
the read path.

### 8.3 Append-Only Constraint

JSONL ledger files are append-only. Historical entries are never modified in place. Schema
normalization happens at read time (via migration functions in INF-TSK-024-008 through 012)
or at write time (by replacing the generic `Event` struct with `CanonicalEvent` in new
emitters).
