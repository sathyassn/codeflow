# memory-events.jsonl Schema Audit

## Overview

- **File**: `.state/ledger/memory-events/memory-events.jsonl`
- **Total Events**: 369
- **Date Range**: 2026-02-11 to 2026-03-19
- **Purpose**: Append-only event log for memory operations (milestones, progress, decisions, findings, session summaries). Note: stage transitions route to `pathflow-events.jsonl`, not this file.
- **Tier**: Tier 0 (JSONL) — rebuild authority for Tier 1 (SurrealDB) and Tier 2 (Markdown)

## Canonical Rust Types

Source: `codeflow-cli/core/src/types/events.rs`

The `LedgerEvent` enum defines 8 memory-related variants:

Note: The `LedgerEvent` enum uses `#[serde(tag = "event")]` — the discriminator field in JSONL is `event`, not `event_type`.

| Variant | `event` field value | event_type() Return |
|---------|---------------------|---------------------|
| MemoryStore | `"memory_store"` | `"memory_store"` |
| Milestone | `"milestone"` | `"milestone"` |
| Progress | `"progress"` | `"progress"` |
| Finding | `"finding"` | `"finding"` |
| Decision | `"decision"` | `"decision"` |
| SessionSummary | `"session_summary"` | `"session_summary"` |
| MemoryEvent | `"memory_event"` | `"memory_event"` |
| MemoryMilestone | `"memory_milestone"` | `"memory_milestone"` |

### JSONL Routing (routing.rs)

Events routed to `memory-events.jsonl`:

```
"memory_store" | "memory_stored" | "milestone" | "progress" | "finding" | "decision"
| "session_summary" | "memory_event" | "memory_milestone" | "blocker"
```

Note: `"memory_stored"` and `"blocker"` are legacy Go compatibility types.

### Rust Model (models/memory.rs)

```rust
pub struct MemoryEvent {
    pub id: String,
    pub event_type: String,
    pub domain: String,
    pub work_id: Option<String>,
    pub data: String,           // JSON-encoded event data
    pub memory_type: Option<String>,
    pub created_at: String,     // RFC 3339 timestamp
}
```

## Schema Patterns

### Pattern 1: Direct Canonical Events (~84% of events)

The most common pattern (309/369). Events written directly with a canonical `event` value (`milestone`, `progress`, `decision`, etc.) at top level, `data` contains event-specific content. Does NOT use the `memory_store` wrapper.

**BEFORE (current):**

```json
{
  "domain": "development",
  "event": "milestone",
  "format_id": "INF-TSK-015-001",
  "id": "memory-01KJ8MWTWZPXD5898SNASTN1BR",
  "summary": "Work complete: INF-TSK-015-001 Go project initialization. WS-DEV+WS-REV+WS-QA all passed.",
  "task_id": "task-01KJ8HECFZGFS1NGWFEA4G7FRN",
  "timestamp": "2026-02-24T20:19:44Z",
  "work_id": "work-01KJ8HECFZZ1Y72X0GM29DBA1H"
}
```

**AFTER (canonical):**

```json
{
  "event_type": "milestone",
  "id": "memory-01KJ8MWTWZPXD5898SNASTN1BR",
  "domain": "development",
  "work_id": "work-01KJ8HECFZZ1Y72X0GM29DBA1H",
  "data": {
    "summary": "Work complete: INF-TSK-015-001 Go project initialization. WS-DEV+WS-REV+WS-QA all passed.",
    "task_id": "task-01KJ8HECFZGFS1NGWFEA4G7FRN",
    "format_id": "INF-TSK-015-001"
  },
  "created_at": "2026-02-24T20:19:44Z"
}
```

**Changes needed:**
- Rename `timestamp` → `created_at` (match Rust model)
- Rename `event` → `event_type`
- Wrap loose top-level fields (`summary`, `task_id`, `format_id`) into `data` object

### Pattern 2: Nested event_type in data wrapper (~16% of events)

The `memory_store` wrapper pattern (60/369). `event_type` is INSIDE the `data` object, not at top level. Often has doubly-nested `data.data`.

**BEFORE (current):**

```json
{
  "data": {
    "created_at": "2026-02-13T21:28:51.000Z",
    "data": "{\"summary\": \"User feedback round 2: 8 items received...\"}",
    "domain": "development",
    "event_type": "progress",
    "id": "memory-019c58e759cb1f236f11157c43c6a136",
    "memory_type": "episodic",
    "work_id": "work-17710089861398"
  },
  "event": "memory_store",
  "timestamp": "2026-02-13T21:28:51.000Z"
}
```

**AFTER (canonical):**

```json
{
  "event_type": "progress",
  "id": "memory-019c58e759cb1f236f11157c43c6a136",
  "domain": "development",
  "work_id": "work-17710089861398",
  "data": {"summary": "User feedback round 2: 8 items received..."},
  "memory_type": "episodic",
  "created_at": "2026-02-13T21:28:51.000Z"
}
```

**Changes needed:**
- Hoist `event_type`, `id`, `domain`, `work_id`, `memory_type` from `data` to top level
- Parse `data.data` JSON string into proper object, assign to `data`
- Remove redundant `event` wrapper field
- Rename `timestamp` → `created_at`

### Pattern 3: Non-canonical event types (~10% of events)

Events using `event` values that don't map to canonical `event_type` values, or missing `event_type` entirely.

**BEFORE (current — missing event_type):**

```json
{
  "event": "memory_milestone",
  "summary": "DB schema migration complete: added 9 V4 columns...",
  "task_format_id": "INF-TSK-005-001",
  "timestamp": "2026-02-19T03:36:14Z",
  "work_id": "work-1771452340410uo858e56z2sq"
}
```

**AFTER (canonical):**

```json
{
  "event_type": "memory_milestone",
  "id": "memory-<generated-ulid>",
  "domain": "infrastructure",
  "work_id": "work-1771452340410uo858e56z2sq",
  "data": {"summary": "DB schema migration complete: added 9 V4 columns..."},
  "created_at": "2026-02-19T03:36:14Z"
}
```

**Changes needed:**
- Map `event` → `event_type` using canonical enum values
- Generate `id` if missing
- Wrap loose fields (`summary`, `task_format_id`) into `data` object
- Rename `timestamp` → `created_at`

### Pattern 4: Hybrid/Clean Structure (~10% of events)

Newer events that have both `event` and `event_type` at the top level with structured `data` fields. Closest to canonical — only change needed is removing the redundant `event` field.

Note: This pattern uses genuine memory-events types (milestone, decision, session_summary, etc.). `stage_transition` events route to `pathflow-events.jsonl`, not `memory-events.jsonl`.

**BEFORE (current):**

```json
{
  "event": "milestone",
  "id": "memory-01KKZJTY39G916YVN1WET0ZSCX",
  "work_id": "work-01KKZGPC64ZXH50SSKEH90FPA9",
  "task_id": "task-01kk0t2nj5cf52mrt4a0rc25s8",
  "event_type": "milestone",
  "domain": "development",
  "memory_type": "milestone",
  "data": {
    "summary": "INF-TSK-023-009 complete: Integrate worktree creation into SessionStart hook",
    "deliverables": ["WorktreePaths struct in worktree/paths.rs", "Manager.setup_detached() wired into SessionStart on source=startup", "CODEFLOW_WORKTREE_PATH exported in result.env_vars", "Compact/resume path reads existing worktree path", "All init steps use WorktreePaths", "Unit and integration tests"],
    "acceptance_criteria_met": "13/13",
    "pipeline": "WS-DEV->WS-REV->WS-QA"
  },
  "created_at": "2026-03-18T04:20:45Z"
}
```

**AFTER (canonical):**

```json
{
  "event_type": "milestone",
  "id": "memory-01KKZJTY39G916YVN1WET0ZSCX",
  "domain": "development",
  "work_id": "work-01KKZGPC64ZXH50SSKEH90FPA9",
  "data": {
    "summary": "INF-TSK-023-009 complete: Integrate worktree creation into SessionStart hook",
    "deliverables": ["WorktreePaths struct in worktree/paths.rs", "Manager.setup_detached() wired into SessionStart on source=startup", "CODEFLOW_WORKTREE_PATH exported in result.env_vars", "Compact/resume path reads existing worktree path", "All init steps use WorktreePaths", "Unit and integration tests"],
    "acceptance_criteria_met": "13/13",
    "pipeline": "WS-DEV->WS-REV->WS-QA"
  },
  "memory_type": "milestone",
  "created_at": "2026-03-18T04:20:45Z"
}
```

**Changes needed:**
- Remove redundant `event` field (already has `event_type`)
- Otherwise already canonical

## Field Inconsistencies

### 1. event vs event_type Semantics

| Issue | Detail |
|-------|--------|
| Redundancy | Many events have BOTH `event` and `event_type` with different values |
| Missing | Some events have `event` but no `event_type` |
| Mismatch | `event: "memory_store"` + `event_type: "milestone"` — which is authoritative? |

**Resolution:** `event_type` is the canonical discriminator (matches Rust enum). Remove `event` field entirely.

### 2. Doubly-Nested data.data

| Issue | Detail |
|-------|--------|
| Pattern | `data.data` contains a JSON string that should be parsed |
| Scope | ~16% of events (Pattern 2 — 60/369) |
| Root cause | Legacy writer serialized data as string instead of object |

**Resolution:** Parse `data.data` JSON string into object, flatten to single `data` level.

### 3. Inconsistent ID Formats

| Format | Example | Count |
|--------|---------|-------|
| ULID-style | `memory-01KH7A1ZB0P2K8PYNZBW60E7K3` | ~60% |
| Unix-millis | `memory-1771001127021108` | ~20% |
| Short hex | `memory-e2c68358` | ~10% |
| Timestamp-hex | `memory-1771338123-85f75c31` | ~10% |

**Resolution:** Standardize on ULID format (`memory-{26-char-ULID}`). Existing IDs are immutable (append-only log), but new events must use ULID.

### 4. Timestamp Field Naming

| Field | Format | Usage |
|-------|--------|-------|
| `timestamp` | ISO-8601 with microseconds | Early events |
| `created_at` | ISO-8601 with Z suffix | Later events |

**Resolution:** Standardize on `created_at` (matches Rust model).

### 5. Field Name Aliases

| Concept | Aliases Found | Canonical |
|---------|---------------|-----------|
| Task ID | `task_id`, `task_format_id`, `format_id` | `task_id` (internal), `format_id` (display) |
| Event classification | `event`, `event_type`, `type`, `memory_type`, `subtype` | `event_type` (discriminator), `memory_type` (category) |
| Record ID | `id`, `memory_id`, `entry_id` | `id` |
| Work reference | `work_id`, `task_completed`, `tasks_completed` | `work_id` |
| Completion status | `status`, `verdict`, `approval`, `pipeline_result` | In `data` (context-dependent) |

### 6. Unique event Values vs Canonical event_type

| `event` value | Maps to `event_type` | Status |
|---------------|----------------------|--------|
| `memory_store` | (wrapper, not a type) | Remove — use inner `event_type` |
| `memory_event` | `memory_event` | Canonical |
| `memory_milestone` | `memory_milestone` | Canonical |
| `milestone` | `milestone` | Canonical |
| `progress` | `progress` | Canonical |
| `decision` | `decision` | Canonical |
| `finding` | `finding` | Canonical |
| `session_summary` | `session_summary` | Canonical |

Note: `stage_transition` is NOT routed to `memory-events.jsonl`. Per `routing.rs` lines 52-56, it routes to `pathflow-events.jsonl`. Any `stage_transition` events found in the memory-events file are misrouted and should be investigated.

## Canonical Schema Target

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

Required fields: `event_type`, `id`, `data`, `created_at`
Optional fields: `domain`, `work_id`, `memory_type`

## Recommendations

1. **Remove `event` field** — redundant with `event_type`
2. **Flatten doubly-nested data.data** — parse JSON strings into objects
3. **Standardize IDs to ULID** — new events only (existing are immutable)
4. **Rename `timestamp` → `created_at`** — match Rust model
5. **Enforce canonical field names** — no aliases in new events
6. **Add `memory_type` to all events** — currently optional/missing on ~50%
7. **Validate at write time** — Rust writer should enforce schema before append
