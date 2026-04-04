---
title: "pr-events Schema Audit"
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

# pr-events Schema Audit

This document audits the historical `pr-events-{date}.jsonl` schema, documents the
dual-source issue that exists due to two active write paths, and establishes the current
writer status. PR events are written by two sources: the cf-git-operations SOP appends to
`.state/logs/git/pr-events-{date}.jsonl` directly, while the Rust CLI ledger system writes
`pr_created` and `pr_merged` event types to `work-graph.jsonl`.

## Table of Contents

- [1. File Location and Current Status](#1-file-location-and-current-status)
- [2. Dual-Source History](#2-dual-source-history)
- [3. Event Type Inventory](#3-event-type-inventory)
  - [3.1 pr_created](#31-pr_created)
  - [3.2 pr_merged](#32-pr_merged)
- [4. Field Inconsistencies Catalog](#4-field-inconsistencies-catalog)
- [5. BEFORE/AFTER Schema Table](#5-beforeafter-schema-table)
- [6. BEFORE/AFTER JSON Examples](#6-beforeafter-json-examples)
- [7. Rust Type Reference](#7-rust-type-reference)
- [8. Cross-Reference with work-graph.jsonl](#8-cross-reference-with-work-graphjsonl)

---

## 1. File Location and Current Status

**Standalone files (cf-git-operations SOP):** `.state/logs/git/pr-events-{YYYY-MM-DD}.jsonl`

The cf-git-operations SOP (`.claude/agents/cf-git-operations.md`) actively instructs the
agent to construct a JSON line and append it to a date-suffixed file under
`.state/logs/git/`. This write path uses bash echo and is distinct from the Rust CLI
ledger system.

**Ledger location (Rust CLI):** PR events routed to `work-graph.jsonl` via `routing.rs:35-36`.

```text
.state/ledger/
└── work-graph/
    ├── work-graph.jsonl                        ← base (includes pr_created and pr_merged)
    └── work-graph-ses-{id}.jsonl               ← per-session fragment
```

**Writer status:**

| Writer | Status | Target Location | Notes |
|--------|--------|----------------|-------|
| Early LLM agent bash echo (ad-hoc) | RETIRED | `.state/logs/pr-events-{date}.jsonl` | Agents constructed JSON manually before cf-git-operations SOP existed; used `event_type` field name |
| cf-git-operations SOP (bash echo) | ACTIVE | `.state/logs/git/pr-events-{YYYY-MM-DD}.jsonl` | SOP at `.claude/agents/cf-git-operations.md` lines 322, 375, 390, 400; uses `ts` field name, writes `event` field |
| Go CLI hooks | RETIRED | `.state/logs/pr-events-{date}.jsonl` | Go CLI replaced by Rust CLI |
| Rust CLI ledger | ACTIVE | `.state/ledger/work-graph/` | Routes `pr_created` and `pr_merged` to `work-graph.jsonl` via `JsonlWriter` |

**Dual-write reality:** Both the cf-git-operations SOP and the Rust CLI ledger are active.
The standalone `.state/logs/git/pr-events-{date}.jsonl` files produced by the SOP are not
synced to SQLite and serve as a supplemental audit trail. The `work-graph.jsonl` entries are
the authoritative source for programmatic consumption.

---

## 2. Dual-Source History

Three distinct sources have written PR events across the project's history.

### Source 1: Early LLM Agent Bash Echo (RETIRED)

Before the cf-git-operations SOP was standardized, agents constructed JSON manually and
appended to date-suffixed files in `.state/logs/`:

```json
{"event_type":"pr_created","pr_number":73,"task_format_id":"INF-TSK-008-003","timestamp":"2026-02-22T..."}
```

**Key characteristics:**

- Uses `event_type` (not `event`) — inconsistent with the rest of the ledger system
- No `session_id` field — cannot be correlated with session records
- Wrote to `.state/logs/pr-events-{date}.jsonl` (no `git/` subdirectory)

### Source 2: cf-git-operations SOP Bash Echo (ACTIVE)

The cf-git-operations agent SOP instructs appending a JSON line to
`.state/logs/git/pr-events-{YYYY-MM-DD}.jsonl` after PR creation and merge. The SOP
specifies the following fields (`cf-git-operations.md:322`):

```json
{"ts":"2026-03-04T20:00:00Z","event":"pr_created","pr_number":134,"pr_url":"https://github.com/...","task_id":"INF-TSK-008-001","branch":"feat/worktree-session-start","target":"main","session_id":"ses-01kj..."}
```

**Key characteristics:**

- Uses `ts` (not `timestamp`) — field name differs from the ledger standard
- Uses `event` (canonical) — consistent with the rest of the ledger system
- Has `session_id` — correlatable with session records
- Path uses `git/` subdirectory: `.state/logs/git/pr-events-{YYYY-MM-DD}.jsonl`

### Source 3: Go CLI Hooks (RETIRED)

The Go CLI wrote PR events using the canonical field naming directly to the log:

```json
{"event":"pr_created","pr_number":134,"session_id":"ses-01kj...","task_format_id":"INF-TSK-008-001","timestamp":"2026-03-04T..."}
```

**Key characteristics:**

- Uses `event` (canonical)
- Uses `timestamp` (canonical)
- Has `session_id`
- Used `task_format_id` (not `task_id`)

### Current: Rust CLI (ACTIVE)

The Rust CLI routes PR events to `work-graph.jsonl` via the ledger system. The `pr_created`
and `pr_merged` event types are handled by `JsonlWriter::append_event()` in
`codeflow-cli/core/src/ledger/jsonl.rs`.

---

## 3. Event Type Inventory

### 3.1 pr_created

Emitted when a pull request is created.

**Actual entry (cf-git-operations SOP, current):**

```json
{"ts":"2026-03-04T20:00:00Z","event":"pr_created","pr_number":134,"pr_url":"https://github.com/...","task_id":"INF-TSK-008-001","branch":"feat/worktree-session-start","target":"main","session_id":"ses-01kj..."}
```

**Actual entry from Go CLI era (retired):**

```json
{"event":"pr_created","pr_number":134,"session_id":"ses-01kj...","task_format_id":"INF-TSK-008-001","timestamp":"2026-03-04T..."}
```

**Actual entry from early LLM bash echo era (retired):**

```json
{"event_type":"pr_created","pr_number":73,"task_format_id":"INF-TSK-008-003","timestamp":"2026-02-22T..."}
```

**Union of fields from all sources:**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes (target) | Always `"pr_created"` (Source 1 used `event_type`) |
| `ts` / `timestamp` | string (RFC 3339) | Yes | SOP writes `ts`; Go CLI and Rust ledger write `timestamp` |
| `session_id` | string | No (Source 1) / Yes (others) | Absent in early LLM entries |
| `worktree` | string | No | Present when session runs in a worktree (Rust ledger only) |
| `pr_number` | integer | Yes | GitHub PR number |
| `pr_url` | string | No | GitHub PR URL (SOP-specific field) |
| `task_id` | string | No | SOP uses `task_id` (human-readable format ID) |
| `task_format_id` | string | No | Go CLI era used `task_format_id` (same values as `task_id`) |
| `branch` | string | No | Feature branch (SOP field) |
| `target` | string | No | Target branch (SOP uses `target`; target schema uses `target_branch`) |
| `target_branch` | string | No | Target branch (target schema field name) |
| `title` | string | No | PR title (optional) |

---

### 3.2 pr_merged

Emitted when a pull request is merged.

**Actual entry (cf-git-operations SOP, current — from `cf-git-operations.md:375`):**

```json
{"ts":"2026-03-04T20:10:00Z","event":"pr_merged","pr_number":134,"merge_sha":"abc123...","task_id":"INF-TSK-008-001","session_id":"ses-01kj..."}
```

The `pr_merged` event type is routed to `work-graph.jsonl` by the routing system
(`routing.rs:36`). However, there is no explicit `PrMerged` variant in the `LedgerEvent`
enum — `pr_merged` events fall through to a deserialization error or require the catch-all
path. See Section 7 for details.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"pr_merged"` |
| `ts` / `timestamp` | string (RFC 3339) | Yes | SOP writes `ts`; target schema uses `timestamp` |
| `session_id` | string | Yes (target) | Session identifier |
| `worktree` | string | No | Present when session runs in a worktree |
| `pr_number` | integer | Yes | GitHub PR number |
| `merge_sha` | string | No | Merge commit SHA |
| `task_id` | string | No | Human-readable task ID |
| `target_branch` | string | No | Target branch |

---

## 4. Field Inconsistencies Catalog

### 4.1 Event Type Key

| Source | Field | Value Example | Issue |
|--------|-------|---------------|-------|
| Source 1 (early LLM bash echo, retired) | `event_type` | `"pr_created"` | Non-standard; conflicts with all other ledger event types using `event` |
| Source 2 (cf-git-operations SOP, active) | `event` | `"pr_created"` | Correct |
| Source 3 (Go CLI, retired) | `event` | `"pr_created"` | Correct |
| Rust CLI (active) | `event` | `"pr_created"` | Correct |

### 4.2 Timestamp Key

| Source | Field | Value Example | Issue |
|--------|-------|---------------|-------|
| Source 1 (early LLM bash echo, retired) | `timestamp` | `"2026-02-22T..."` | Correct field name, but source is retired |
| Source 2 (cf-git-operations SOP, active) | `ts` | `"2026-03-04T20:00:00Z"` | Non-standard abbreviation; diverges from ledger standard |
| Source 3 (Go CLI, retired) | `timestamp` | `"2026-03-04T..."` | Correct |
| Rust CLI (active) | `timestamp` | `"2026-03-04T..."` | Correct |

### 4.3 Session ID

| Source | Field | Present | Issue |
|--------|-------|---------|-------|
| Source 1 (early LLM bash echo, retired) | `session_id` | No | Cannot correlate PR event with session record |
| Source 2 (cf-git-operations SOP, active) | `session_id` | Yes | Correct |
| Source 3 (Go CLI, retired) | `session_id` | Yes | Correct |
| Rust CLI (active, in work-graph) | `session_id` | Yes (via data catch-all) | Present but not a named struct field |

### 4.4 Task Reference Field Name

| Source | Field | Value Example | Issue |
|--------|-------|---------------|-------|
| Source 1 (some entries) | `task_id` | `"INF-TSK-008-003"` | Contains human-readable ID despite name suggesting DB key |
| Source 1 (other entries) | `task_format_id` | `"INF-TSK-008-003"` | Correct canonical name |
| Source 2 (SOP) | `task_id` | `"INF-TSK-008-001"` | SOP specifies `task_id` for the human-readable ID |
| Source 3 (Go CLI) | `task_format_id` | `"INF-TSK-008-001"` | Correct canonical name |

### 4.5 Target Branch Field Name

| Source | Field | Value Example | Issue |
|--------|-------|---------------|-------|
| Source 2 (SOP) | `target` | `"main"` | Abbreviated name |
| Target schema | `target_branch` | `"main"` | Full descriptive name |

---

## 5. BEFORE/AFTER Schema Table

### pr_created

| Field | BEFORE (Source 1 — early LLM era, retired) | BEFORE (Source 2 — SOP, active) | AFTER (target) | Change |
|-------|------------------------------------------|--------------------------------|----------------|--------|
| Event type key | `event_type` | `event` | `event` | Retire Source 1 |
| Timestamp key | `timestamp` | `ts` | `timestamp` | Fix SOP to use `timestamp` |
| `session_id` | (absent) | `session_id` | `session_id` (required) | Enforce |
| `worktree` | (absent) | (absent) | string (optional) | Add to Rust ledger path |
| `task_format_id` / `task_id` | either field | `task_id` | `task_format_id` | Standardize |
| `target` vs `target_branch` | (absent) | `target` | `target_branch` | Rename SOP field |
| File location | `.state/logs/pr-events-{date}.jsonl` | `.state/logs/git/pr-events-{date}.jsonl` | `work-graph.jsonl` | Consolidate |

### pr_merged

| Field | BEFORE (SOP, active) | AFTER (target) | Change |
|-------|---------------------|----------------|--------|
| Timestamp key | `ts` | `timestamp` | Fix SOP |
| `worktree` | (absent) | string (optional) | Add to Rust ledger path |
| Explicit Rust variant | `PrCreated` (aliased / catch-all) | `PrMerged` (distinct) | Add variant |

---

## 6. BEFORE/AFTER JSON Examples

### pr_created — BEFORE (Source 1, early LLM bash echo, retired)

```json
{"event_type":"pr_created","pr_number":73,"task_format_id":"INF-TSK-008-003","timestamp":"2026-02-22T04:15:00Z"}
```

### pr_created — BEFORE (Source 2, cf-git-operations SOP, active)

```json
{"ts":"2026-03-04T20:00:00Z","event":"pr_created","pr_number":134,"pr_url":"https://github.com/owner/repo/pull/134","task_id":"INF-TSK-008-001","branch":"feat/worktree-session-start","target":"main","session_id":"ses-01kj..."}
```

### pr_created — AFTER (target schema)

```json
{
  "event": "pr_created",
  "timestamp": "2026-03-04T20:00:00.000Z",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "pr_number": 134,
  "task_format_id": "INF-TSK-008-001",
  "branch": "feat/worktree-session-start",
  "target_branch": "main",
  "title": "feat: add worktree setup to SessionStart"
}
```

---

## 7. Rust Type Reference

Located in `codeflow-cli/core/src/types/events.rs`.

### PrCreated variant (events.rs:92-97)

```rust
// codeflow-cli/core/src/types/events.rs:92-97
PrCreated {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(flatten)]
    data: serde_json::Value,
},
```

**Notable gaps in the current Rust type:**

1. **No `session_id` field at the struct level.** Unlike the pathflow variants which
   have `session_id: Option<String>` as a named field, `PrCreated` does not. The
   `session_id` value (if present in the JSON) is captured by the `data` catch-all via
   `#[serde(flatten)]`.

2. **No `PrMerged` variant.** The routing system maps `"pr_merged"` to `files::WORK_GRAPH`
   (`routing.rs:36`), but there is no explicit `PrMerged` variant in `LedgerEvent`. The
   `event_type()` method does not return `"pr_merged"` for any variant
   (`events.rs:247-285`). This is a gap that should be addressed in INF-TSK-024-007.

**Routing constant:** `files::WORK_GRAPH = "work-graph"` at
`codeflow-cli/core/src/ledger/mod.rs:23`.

**Event type discriminator** (from `LedgerEvent::event_type()` at `events.rs:264`):

| Variant | `event_type()` return value |
|---------|----------------------------|
| `PrCreated` | `"pr_created"` |
| (no `PrMerged` variant) | `"pr_merged"` — **not handled** |

---

## 8. Cross-Reference with work-graph.jsonl

The `pr_created` and `pr_merged` event types are documented in detail in the
work-graph audit at `.codeflow/docs/schemas/work-graph-jsonl-audit.md` (Sections 2.7
and 2.8). That document covers these events in the context of the work-graph ledger.

This document focuses on the standalone `pr-events-{date}.jsonl` files produced by the
cf-git-operations SOP, the dual-source inconsistencies, and the migration path.

**Key relationship:**

- The cf-git-operations SOP writes `pr_created` / `pr_merged` to `.state/logs/git/pr-events-{YYYY-MM-DD}.jsonl` using `ts` (not `timestamp`) and `task_id` (not `task_format_id`)
- The Rust CLI ledger writes `pr_created` / `pr_merged` to `work-graph.jsonl` using `timestamp` and the `PrCreated` catch-all variant
- The Rust ledger path is the authoritative source for programmatic consumption (indexed in SQLite via the CANONICAL set)
- The SOP-written files serve as a supplemental human-readable audit trail but are not read by any Rust tooling
- Migration target: consolidate to a single write path (Rust ledger) and update the SOP to use `timestamp` and `task_format_id`
