---
title: "work-graph.jsonl Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-03-25"
updated_at: "2026-03-25"
scope: INF-TSK-024-002
feeds_into:
  - INF-TSK-024-007
  - INF-TSK-024-008
  - INF-TSK-024-013
---

# work-graph.jsonl Schema Audit

This document audits the schema of `.state/ledger/work-graph.jsonl` across all 8 event
types, catalogs the dual-ID coexistence bug with concrete examples, and provides a gap
analysis against the Rust target model. It feeds directly into INF-TSK-024-007 (canonical
event schema synthesis), INF-TSK-024-008 (dual-ID fix implementation), and
INF-TSK-024-013 (work-graph details migration).

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Event Type Inventory](#2-event-type-inventory)
  - [2.1 epic_created](#21-epic_created)
  - [2.2 epic_status_changed](#22-epic_status_changed)
  - [2.3 task_created](#23-task_created)
  - [2.4 task_status_changed](#24-task_status_changed)
  - [2.5 begin_work](#25-begin_work)
  - [2.6 complete_work](#26-complete_work)
  - [2.7 pr_created](#27-pr_created)
  - [2.8 pr_merged](#28-pr_merged)
- [3. Dual-ID Coexistence Bug](#3-dual-id-coexistence-bug)
- [4. Field Inconsistencies Catalog](#4-field-inconsistencies-catalog)
- [5. Gap Analysis Against Rust Target](#5-gap-analysis-against-rust-target)
- [6. BEFORE/AFTER Schema Table](#6-beforeafter-schema-table)
- [7. BEFORE/AFTER JSON Examples](#7-beforeafter-json-examples)
- [8. Rust Type Reference](#8-rust-type-reference)

---

## 1. File Location and Layout

**Current (flat layout):** `.state/ledger/work-graph.jsonl`

**Target (subdirectory layout, post-migration per PR #221):**

```text
.state/ledger/
└── work-graph/
    ├── work-graph.jsonl                      ← base (compacted history)
    └── work-graph-ses-{id}.jsonl             ← per-session fragment
```

The subdirectory layout is implemented by `JsonlWriter` in
`codeflow-cli/core/src/ledger/jsonl.rs`. When `session_id` is `Some`, events write to
the session fragment file. When `None`, events write to the base file. The current
`.state/ledger/work-graph.jsonl` is the flat-layout base file (pre-migration).

**Writer:** `JsonlWriter::append_event()` in `codeflow-cli/core/src/ledger/jsonl.rs`,
using the `Event` struct from `codeflow-cli/core/src/ledger/mod.rs`.

**Event types routed to `work-graph`** (from `codeflow-cli/core/src/ledger/routing.rs`):

```text
epic_created | epic_status_changed | task_created | task_status_changed
task_updated | task_id_corrected | task_cancelled | begin_work
complete_work | work_complete | commit | pr_created | pr_merged
work_finding | void | work_cancelled | stale_work_cleanup
```

Of these, 8 are the primary documented event types. The remaining 9 (`task_updated`,
`task_id_corrected`, `task_cancelled`, `work_complete`, `commit`, `work_finding`, `void`,
`work_cancelled`, `stale_work_cleanup`) are Go-compatibility or ad-hoc event types
present in the router for backward compatibility with entries written by the Go CLI.

---

## 2. Event Type Inventory

### 2.1 epic_created

**Actual entry from `.state/ledger/work-graph.jsonl` line 2 (ULID-era):**

```json
{"area_type":"INF","domain":"PMGT","event":"epic_created","format_id":"INF-EPC-005","id":"epic-01KHSQPQRNQP0XTXCRHXX9YW1T","status":"planning","timestamp":"2026-02-19T03:57:15Z","title":"Project Management Standardization","work_type":"CHOR"}
```

**Actual entry from `.state/ledger/work-graph.jsonl` line 27 (inconsistent `id` field):**

```json
{"area_type":"INF","domain":"PMGT","epic_id":"epic-01KJ12VSN1YSWYQ03CDK8ENG78","event":"epic_created","format_id":"INF-EPC-008","status":"planning","timestamp":"2026-02-21T21:51:07Z","title":"PathFlow PR Verification, Merge Protection & Validation Hardening","work_type":"CHOR"}
```

**Actual entry from `.state/ledger/work-graph.jsonl` (Rust-era, with `id` field):**

From `create_epic()` in `codeflow-cli/core/src/workgraph/epic.rs:104-130`, the Rust CLI
emits both `id` (ULID) and `format_id` (human-readable):

```json
{"area_type":"INF","domain":"INF","event":"epic_created","format_id":"INF-EPC-001","id":"epic-01km...","status":"draft","timestamp":"2026-03-25T00:00:00Z","title":"Example Epic","work_type":"FEAT"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"epic_created"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Often missing in early entries; present on newer entries |
| `worktree` | string | No | Present when session runs in a worktree |
| `id` | string (ULID) | No (legacy) / Yes (Rust) | Internal DB key, e.g. `epic-01KJ...`. Emitted by Rust CLI; **absent on many Go-era entries** which use `epic_id` instead |
| `format_id` | string | No | Human-readable ID (e.g., `INF-EPC-008`). Present on most entries. |
| `title` | string | Yes | Epic title |
| `status` | string | No | Initial status (e.g., `"planning"`, `"draft"`, `"in_progress"`) |
| `area_type` | string | No | Area code (e.g., `"INF"`) |
| `domain` | string | No | Domain code (e.g., `"PMGT"`) |
| `work_type` | string | No | Work type (e.g., `"CHOR"`) |
| `is_ongoing` | boolean | No | Present on some entries (e.g., line 72) |
| `origin` | string | No | Present on some entries (e.g., `"planned"`) |

**Dual-ID note:** Go-era entries use `epic_id` for the ULID and omit `id`. Rust-era entries
use `id` for the ULID and omit `epic_id`. See Section 3 for detailed examples.

---

### 2.2 epic_status_changed

**Actual entry from `.state/ledger/work-graph.jsonl` line 39:**

```json
{"epic_id":"epic-01KJ12VSN1YSWYQ03CDK8ENG78","event":"epic_status_changed","format_id":"INF-EPC-008","new_status":"in_progress","old_status":"planning","reason":"3 of 11 tasks complete (INF-TSK-008-001, INF-TSK-008-002, INF-TSK-008-003)","session_id":"ses-177137202131769e89b2d5688","timestamp":"2026-02-22T04:04:41Z"}
```

**Actual entry (Rust-era, with `from_status`/`to_status`):**

From `update_epic()` in `codeflow-cli/core/src/workgraph/epic.rs:172-194`, the Rust CLI
emits `from_status` and `to_status` instead of `old_status` and `new_status`:

```json
{"epic_id":"epic-01km...","event":"epic_status_changed","format_id":"INF-EPC-001","from_status":"draft","to_status":"in_progress","timestamp":"2026-03-25T00:00:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"epic_status_changed"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Present on newer entries |
| `worktree` | string | No | Present when session runs in a worktree |
| `epic_id` | string (ULID) | Yes | Internal DB key for the epic |
| `format_id` | string | No | Human-readable epic ID |
| `new_status` | string | Yes (Go-era) | New status — Go-era field name |
| `old_status` | string | No (Go-era) | Previous status — Go-era field name |
| `from_status` | string | Yes (Rust-era) | New field name in Rust CLI (replaces `old_status`) |
| `to_status` | string | Yes (Rust-era) | New field name in Rust CLI (replaces `new_status`) |
| `reason` | string | No | Ad-hoc context — candidate for `details` |
| `trigger` | string | No | Trigger description (e.g., `"all_tasks_complete"`) — candidate for `details` |
| `task_count` | integer | No | Count of tasks — candidate for `details` |
| `task_id` | string | No | Triggering task ULID — candidate for `details` |
| `task_format_id` | string | No | Triggering task format ID — candidate for `details` |
| `all_tasks_complete` | boolean | No | Derived flag — candidate for `details` |

**Field name inconsistency:** Go-era entries use `old_status`/`new_status`. Rust-era
entries use `from_status`/`to_status`. Readers must check all four field names.

---

### 2.3 task_created

**Actual entry from `.state/ledger/work-graph.jsonl` line 3:**

```json
{"area_type":"INF","domain":"PMGT","epic_format_id":"INF-EPC-005","epic_id":"epic-01KHSQPQRNQP0XTXCRHXX9YW1T","event":"task_created","format_id":"INF-TSK-005-002","id":"task-01KHSQPQSF26W0SQE9GZPM8XDE","status":"todo","timestamp":"2026-02-19T03:57:15Z","title":"Update schema.sql for new format ID convention","work_type":"CHOR"}
```

**Actual entry (Go-era, without `id`, using `task_id`):**

```json
{"area_type":"INF","branch":"chore/sandbox-network-settings","domain":"GENL","epic_id":"INF-EPC-006","event":"task_created","status":"todo","task_id":"INF-TSK-006-001","timestamp":"2026-02-19T05:34:44Z","title":"Add sandbox network allowedDomains to settings templates and update docs","work_type":"CHOR"}
```

**Actual entry (Rust-era, from `create_task()` in `codeflow-cli/core/src/workgraph/task.rs:124-155`):**

```json
{"area_type":"INF","epic_id":"epic-01km...","event":"task_created","format_id":"INF-TSK-001-001","id":"task-01km...","status":"todo","timestamp":"2026-03-25T00:00:00Z","title":"Example Task","work_type":"FEAT"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"task_created"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Often missing |
| `worktree` | string | No | Present when session runs in a worktree |
| `id` | string (ULID) | No (legacy) / Yes (Rust) | Internal DB key, e.g. `task-01KH...`. Emitted by Rust CLI; absent on many Go/shell-era entries |
| `format_id` | string | No | Human-readable ID (e.g., `INF-TSK-005-002`) |
| `task_id` | string | No | **Overloaded:** sometimes holds ULID (`task-01KH...`), sometimes holds format_id (`INF-TSK-006-001`) |
| `epic_id` | string | Yes | Parent epic internal ULID (most entries) |
| `epic_format_id` | string | No | Parent epic human-readable ID |
| `title` | string | Yes | Task title |
| `status` | string | No | Initial status (e.g., `"todo"`, `"in_progress"`) |
| `area_type` | string | No | Area code |
| `domain` | string | No | Domain code |
| `work_type` | string | No | Work type |
| `note` | string | No | Ad-hoc (e.g., `"retroactive_registration"`, `"tier1_sync_from_tier2"`) |
| `estimate` | string | No | Effort estimate (e.g., `"M"`) |
| `priority` | string | No | Priority level (e.g., `"high"`) |
| `origin` | string | No | Present on some entries (e.g., `"planned"`, `"informal"`) |
| `branch` | string | No | Branch name when task created on a branch |
| `file_scope` | array | No | File scope list (present on some entries; e.g., line 60) |
| `acceptance` | array | No | Acceptance criteria (present on some entries; e.g., line 60) |

**Critical gap from Rust `Task` model:** The `create_task()` function in `task.rs`
stores 24+ fields in the DB but emits only 7 fields (`id`, `format_id`, `epic_id`,
`title`, `area_type`, `work_type`, `status`) in the JSONL event. See Section 5 for
the full gap analysis.

---

### 2.4 task_status_changed

**Actual entry from `.state/ledger/work-graph.jsonl` line 4:**

```json
{"branch":"chore/schema-format-id-convention","event":"task_status_changed","format_id":"INF-TSK-005-002","id":"task-01KHSQPQSF26W0SQE9GZPM8XDE","new_status":"in_progress","old_status":"todo","timestamp":"2026-02-19T03:57:15Z"}
```

**Actual entry (Go-era, using `task_id` as format_id):**

```json
{"epic_id":"INF-EPC-006","event":"task_status_changed","new_status":"complete","old_status":"in_progress","reason":"PR #39 merged to main","task_id":"INF-TSK-006-001","timestamp":"2026-02-19T13:44:49Z"}
```

**Actual entry (Rust-era, with `from_status`/`to_status` and ULID `task_id`):**

From `update_task()` in `codeflow-cli/core/src/workgraph/task.rs:197-219`:

```json
{"event":"task_status_changed","format_id":"INF-TSK-001-001","from_status":"todo","task_id":"task-01km...","timestamp":"2026-03-25T00:00:00Z","to_status":"in_progress"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"task_status_changed"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Present on newer entries |
| `worktree` | string | No | Present when session runs in a worktree |
| `task_id` | string | Yes* | **Overloaded:** holds ULID in some entries (`task-01KH...`), holds format_id in others (`INF-TSK-006-001`). Rust-era always ULID |
| `format_id` | string | No | Human-readable task ID |
| `new_status` | string | Yes (Go-era) | New status — Go-era field name |
| `old_status` | string | No (Go-era) | Previous status — Go-era field name |
| `from_status` | string | Yes (Rust-era) | New field name in Rust CLI (replaces `old_status`) |
| `to_status` | string | Yes (Rust-era) | New field name in Rust CLI (replaces `new_status`) |
| `branch` | string | No | Ad-hoc — candidate for `details` |
| `summary` | string | No | Ad-hoc completion summary — candidate for `details` |
| `pr_number` | integer | No | Ad-hoc (on completed tasks with PRs) — candidate for `details` |
| `completed_at` | string | No | Ad-hoc completion timestamp — candidate for `details` |
| `work_id` | string | No | Work session ID |
| `epic_format_id` | string | No | Parent epic (inconsistently present) |
| `reason` | string | No | Ad-hoc (e.g., `"PR #39 merged to main"`) — candidate for `details` |
| `from` / `to` | string | No | Alternate status field names (some entries, e.g., lines 51-56) |

**Four status field name variants exist:** `new_status`/`old_status` (Go-era),
`from_status`/`to_status` (Rust-era), and `from`/`to` (ad-hoc variant from some agent
writes on lines 51-56). Readers must handle all four pairs.

---

### 2.5 begin_work

**Actual entry from `.state/ledger/work-graph.jsonl` line 136:**

```json
{"branch":"fix/session-end-cleanup","event":"begin_work","format_id":"INF-TSK-018-001","session_id":"ses-17720365524152d18a44455b5","task_id":"task-01kjaw24v60kfvdw857kqmcggy","timestamp":"2026-02-25T17:05:00Z"}
```

**Actual entry with dual `work_id` and `id` fields (line 341):**

```json
{"branch":"docs/inf-tsk-021-021-agent-defs-go-cli","event":"begin_work","format_id":"INF-TSK-021-021","id":"work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ","task_id":"task-01KJHQCM92SPJJSC8V306TA50E","timestamp":"2026-03-04T04:21:16Z","work_id":"work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"begin_work"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Present on newer entries |
| `worktree` | string | No | Present when session runs in a worktree |
| `task_id` | string | Yes | Task ULID (`task-01kj...`) or format_id — overloaded |
| `format_id` | string | No | Human-readable task ID |
| `work_id` | string | No | Work session ID (`work-01KJ...`) |
| `id` | string | No | Sometimes same value as `work_id`; sometimes missing entirely |
| `branch` | string | No | Active branch — candidate for `details` |
| `topic` | string | No | Ad-hoc work topic — candidate for `details` |
| `domain` | string | No | Ad-hoc — candidate for `details` |
| `epic_id` | string | No | Epic ID — inconsistently present |

**Work ID inconsistency:** Some entries use `work_id` only, some use `id` only, and some
(line 341) duplicate the value in both `work_id` and `id`. The `active_work` table uses
`id` as the primary key but JSONL uses `work_id` as the canonical field name.

---

### 2.6 complete_work

**Actual entry from `.state/ledger/work-graph.jsonl` line 339:**

```json
{"event":"complete_work","id":"work-01KJV7FY4XSGEQ9JD4H9BY8FQ7","task_id":"task-01KJHQCMBBPS4HWK7MQ8A7NP1A","timestamp":"2026-03-04T04:21:16Z"}
```

**Actual entry with additional fields (line 343):**

```json
{"event":"complete_work","id":"work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ","task_id":"task-01KJHQCM92SPJJSC8V306TA50E","format_id":"INF-TSK-021-021","pr_number":129,"timestamp":"2026-03-04T00:00:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"complete_work"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Often missing |
| `worktree` | string | No | Present when session runs in a worktree |
| `id` | string | Yes | Work session ID (`work-01KJV...`); maps to `active_work.id` |
| `task_id` | string | Yes | Task ULID |
| `format_id` | string | No | Human-readable task ID |
| `pr_number` | integer | No | PR number if work completed with a PR — candidate for `details` |
| `summary` | string | No | Ad-hoc completion summary — candidate for `details` |
| `data` | object | No | Nested payload from some cf-knowledge-layer writes — candidate for `details` |

**Go-compat variant:** The router also accepts `work_complete` (alias for `complete_work`
per routing.rs line 33). No `work_complete` entries appear in the current base file.

---

### 2.7 pr_created

**Actual entry from `.state/ledger/work-graph.jsonl` line 129:**

```json
{"branch":"docs/stage-reporting-in-task-docs","event":"pr_created","pr_number":73,"pr_url":"https://github.com/sathyassn/codeflow/pull/73","task_format_id":"DOC-TSK-001-003","task_id":"task-01KJ9J33E92WJ2EZY83HJQSJ2Z","timestamp":"2026-02-25T06:26:02Z"}
```

**Actual entry with additional fields (line 150):**

```json
{"branch":"feat/session-package","entry_id":"wg-01KJBHP22KQ8V78G4KWWGM2ETA","epic_format_id":"INF-EPC-015","event":"pr_created","pr_number":77,"pr_url":"https://github.com/sathyassn/codeflow/pull/77","session_id":"ses-17720365524152d18a44455b5","task_format_id":"INF-TSK-015-006","task_id":"task-01KJBFCQ6BEBRM6CPWC3EHMMCF","timestamp":"2026-02-25T23:20:00Z","work_id":"work-01KJBFHEA5W49Y21FDS21NYSWY"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"pr_created"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Often missing |
| `worktree` | string | No | Present when session runs in a worktree |
| `pr_number` | integer | Yes | PR number |
| `pr_url` | string | No | Full GitHub PR URL |
| `task_id` | string | No | Task ULID |
| `task_format_id` | string | No | Human-readable task ID |
| `format_id` | string | No | Alternate field name for task format ID (some entries) |
| `branch` | string | No | Branch that was PRed — candidate for `details` |
| `work_id` | string | No | Work session ID |
| `epic_format_id` | string | No | Parent epic format ID |
| `entry_id` | string | No | Ad-hoc entry identifier (some entries) — candidate for `details` |

**No `pr_merged` entries found:** The router accepts `pr_merged` but no entries of this
type appear in the current base file. Events may have been written to Go-era `pr-events-{date}.jsonl`
files instead of `work-graph.jsonl`.

---

### 2.8 pr_merged

**No entries found** in `.state/ledger/work-graph.jsonl`. The event type is registered
in `codeflow-cli/core/src/ledger/routing.rs:36` and routes to `work-graph`. Based on
the `pr_created` pattern, the expected schema is:

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"pr_merged"` |
| `timestamp` | string (RFC 3339) | Yes | UTC ISO 8601 |
| `session_id` | string | No | Present on newer entries |
| `worktree` | string | No | Present when session runs in a worktree |
| `pr_number` | integer | Yes | PR number |
| `task_format_id` | string | Yes | Human-readable task ID |
| `task_id` | string | No | Task ULID |
| `branch` | string | No | Merged branch — candidate for `details` |
| `merged_at` | string | No | Merge timestamp — candidate for `details` |

**Finding:** No `pr_merged` events have been written to `work-graph.jsonl`. The Go CLI
wrote these events to `pr-events-{date}.jsonl`. The Rust CLI routes them to
`work-graph.jsonl` but no Rust implementation yet emits this event type.

---

## 3. Dual-ID Coexistence Bug

This is the highest-priority schema finding. The `id` and `task_id` fields in
`work-graph.jsonl` have inconsistent semantics across eras. The bug causes rebuild
failures because a reader cannot reliably determine whether `task_id` holds a ULID or a
format_id without inspecting the value itself.

### 3.1 The Bug Described

**Root cause:** Different eras of the writer populated `id` / `task_id` with different
values — sometimes ULID (`task-01KH...`), sometimes format_id (`INF-TSK-006-001`),
sometimes both, sometimes neither.

**Three coexisting patterns on `task_status_changed`:**

| Pattern | Fields | Example value | Era |
|---------|--------|--------------|-----|
| A | `id` = ULID, `format_id` = format_id | `"id":"task-01KH..."`, `"format_id":"INF-TSK-005-002"` | Go-era (canonical) |
| B | `task_id` = format_id (no ULID at all) | `"task_id":"INF-TSK-006-001"` | Go-era (early shell/agent writes) |
| C | `task_id` = ULID, `format_id` = format_id | `"task_id":"task-01kjaw24v60kfvdw857kqmcggy"`, `"format_id":"INF-TSK-018-001"` | Rust-era |

**Three coexisting patterns on `epic_created`:**

| Pattern | Fields | Example value | Era |
|---------|--------|--------------|-----|
| A | `id` = ULID, `format_id` = format_id | `"id":"epic-01KHSQPQ..."`, `"format_id":"INF-EPC-005"` | Go-era (canonical) |
| B | `epic_id` = ULID (no `id`), `format_id` = format_id | `"epic_id":"epic-01KJ..."`, `"format_id":"INF-EPC-008"` | Go-era (many entries) |
| C | `id` = ULID, `format_id` = format_id | Same as A | Rust-era (from `create_epic()`) |

### 3.2 Concrete BEFORE Examples from JSONL

**Pattern A — Canonical (id = ULID on `task_created`):**

Line 3 of `.state/ledger/work-graph.jsonl`:

```json
{
  "area_type": "INF",
  "domain": "PMGT",
  "epic_format_id": "INF-EPC-005",
  "epic_id": "epic-01KHSQPQRNQP0XTXCRHXX9YW1T",
  "event": "task_created",
  "format_id": "INF-TSK-005-002",
  "id": "task-01KHSQPQSF26W0SQE9GZPM8XDE",
  "status": "todo",
  "timestamp": "2026-02-19T03:57:15Z",
  "title": "Update schema.sql for new format ID convention",
  "work_type": "CHOR"
}
```

Both `id` (ULID) and `format_id` (human-readable) are present. This is the target state.

**Pattern B — Bug: `task_id` holds format_id (no ULID) on `task_created`:**

Line 7 of `.state/ledger/work-graph.jsonl`:

```json
{
  "area_type": "INF",
  "branch": "chore/sandbox-network-settings",
  "domain": "GENL",
  "epic_id": "INF-EPC-006",
  "event": "task_created",
  "status": "todo",
  "task_id": "INF-TSK-006-001",
  "timestamp": "2026-02-19T05:34:44Z",
  "title": "Add sandbox network allowedDomains to settings templates and update docs",
  "work_type": "CHOR"
}
```

`task_id` holds a format_id string (`INF-TSK-006-001`), not a ULID. No `id` field present.
`epic_id` holds a format_id string (`INF-EPC-006`), not a ULID.

**Pattern B — Bug: `epic_id` holds format_id on `epic_created`:**

Line 6 of `.state/ledger/work-graph.jsonl`:

```json
{
  "area_type": "INF",
  "domain": "GENL",
  "epic_id": "INF-EPC-006",
  "event": "epic_created",
  "status": "in_progress",
  "timestamp": "2026-02-19T05:34:44Z",
  "title": "Sandbox Network Settings & Templates",
  "work_type": "CHOR"
}
```

`epic_id` holds `"INF-EPC-006"` — a format_id, not a ULID (`epic-01KJ...`). No ULID
present at all in this entry.

**Pattern C — Rust-era: `task_id` holds ULID on `task_status_changed`:**

Line 136 of `.state/ledger/work-graph.jsonl`:

```json
{
  "branch": "fix/session-end-cleanup",
  "event": "begin_work",
  "format_id": "INF-TSK-018-001",
  "session_id": "ses-17720365524152d18a44455b5",
  "task_id": "task-01kjaw24v60kfvdw857kqmcggy",
  "timestamp": "2026-02-25T17:05:00Z"
}
```

`task_id` holds `"task-01kjaw24v60kfvdw857kqmcggy"` — a ULID (lowercase). `format_id`
holds the human-readable ID separately. This is consistent but uses `task_id` instead of
`id`, unlike the Rust `task.rs` emit which uses `task_id` for ULID in `task_status_changed`.

### 3.3 Impact on Rebuild

A rebuild algorithm reading `work-graph.jsonl` to reconstruct the `tasks` table cannot
reliably identify the ULID primary key for a task without inspecting every field value:

1. If `id` is present → it is the ULID (Pattern A / Rust epic_created)
2. If `task_id` starts with `task-` → it is the ULID (Pattern C / Rust task_status_changed)
3. If `task_id` matches `AREA-TSK-NNN-NNN` format → it is the format_id, and the ULID
   is unknown from this event alone (Pattern B)
4. If `epic_id` starts with `epic-` → it is the ULID; if it matches `AREA-EPC-NNN` → it
   is the format_id

This ambiguity breaks deterministic rebuild and is the primary motivation for
INF-TSK-024-008 (dual-ID fix).

---

## 4. Field Inconsistencies Catalog

### 4.1 Status field naming on `*_status_changed` events

| Era | Event type | Old status field | New status field |
|-----|-----------|-----------------|-----------------|
| Go | `epic_status_changed` | `old_status` | `new_status` |
| Go | `task_status_changed` | `old_status` | `new_status` |
| Rust | `epic_status_changed` | `from_status` | `to_status` |
| Rust | `task_status_changed` | `from_status` | `to_status` |
| Ad-hoc | `task_status_changed` | `from` | `to` |

**Finding:** Three different field name pairs encode the same concept. The Rust CLI
(`epic.rs:180-185`, `task.rs:204-209`) uses `from_status`/`to_status`. Readers must
check all three pairs.

**Canonical target:** Use `from_status` (Rust naming) going forward. Map `old_status` →
`from_status` and `new_status` → `to_status` during migration.

### 4.2 Work ID field naming on work events

| Entry | Field | Value |
|-------|-------|-------|
| line 136 (`begin_work`) | `task_id`, `format_id` only | No `work_id` or `id` field |
| line 341 (`begin_work`) | Both `work_id` and `id` | Both hold same ULID |
| line 339 (`complete_work`) | `id` only | `work_id` absent |
| line 343 (`complete_work`) | `id` only | `work_id` absent |

**Finding:** `work_id` and `id` are used inconsistently for the work session identifier.
The `active_work` table uses `id` as primary key. JSONL uses `work_id` as the conventional
field name in most entries.

**Canonical target:** Use `work_id` consistently in JSONL. `id` (ULID) maps to `active_work.id`
in the DB. When both are present, they hold the same value.

### 4.3 `task_id` overloading on task events

| Entry | `task_id` value | Semantic |
|-------|----------------|---------|
| line 7 | `"INF-TSK-006-001"` | format_id (bug) |
| line 8 | `"INF-TSK-006-001"` | format_id (bug) |
| line 61 | `"task-01KJ3V6EGTPP2ZTCSQGZR5Z3DP"` | ULID (correct) |
| line 136 | `"task-01kjaw24v60kfvdw857kqmcggy"` | ULID (correct) |

**Finding:** `task_id` holds format_id strings in early entries and ULID strings in later
entries. The distinction can be inferred by checking for the `task-` prefix but requires
heuristic parsing.

### 4.4 `session_id` presence

| Era | Event type | `session_id` present? |
|-----|-----------|---------------------|
| Shell/early Go | `epic_created` | Mostly absent |
| Shell/early Go | `task_created` | Mostly absent |
| Shell/early Go | `task_status_changed` | Mixed |
| Rust | `epic_created` | Always `None` (omitted) |
| Rust | `task_created` | Always `None` (omitted) |
| Rust | `task_status_changed` | Always `None` (omitted) |

**Finding:** The Rust CLI `create_task()` and `create_epic()` functions set
`session_id: None` in the emitted `Event` struct (task.rs:152, epic.rs:126). This means
all Rust-era work-graph events lack `session_id`, making it impossible to attribute
JSONL events to sessions after rebuild.

**Canonical target:** Rust CLI must pass the current `SessionId` when constructing
`Event` structs for work-graph events.

### 4.5 Ad-hoc top-level fields (candidates for `details` object)

These fields appear inconsistently at the top level and are candidates for migration to
a `details` object in the canonical schema:

| Field | Event types where present | Count of entries |
|-------|--------------------------|-----------------|
| `branch` | `task_created`, `task_status_changed`, `begin_work`, `pr_created` | Many |
| `summary` | `task_status_changed`, `complete_work` | ~15 entries |
| `reason` | `task_status_changed`, `epic_status_changed` | ~10 entries |
| `note` | `task_created` | ~8 entries |
| `completed_at` | `task_status_changed` | ~3 entries |
| `pr_number` | `task_status_changed`, `complete_work` | ~5 entries |
| `trigger` | `epic_status_changed` | ~5 entries |
| `task_count` | `epic_status_changed` | ~2 entries |
| `all_tasks_complete` | `epic_status_changed` | ~1 entry |
| `data` | `complete_work` | ~2 entries |

---

## 5. Gap Analysis Against Rust Target

The Rust target model is defined by `TaskOutput` in
`codeflow-cli/core/src/workgraph/task.rs:31-43` and `EpicOutput` in
`codeflow-cli/core/src/workgraph/epic.rs:29-45`. The `Task` and `Epic` database models
(in `codeflow-cli/core/src/models/`) define the full set of fields persisted to SurrealDB.

### 5.1 task_created — Fields emitted vs. DB fields

The Rust `create_task()` function (task.rs:125-155) builds this JSONL payload:

| JSONL field emitted | DB column equivalent | Notes |
|--------------------|---------------------|-------|
| `id` | `Task.id` | ULID; emitted |
| `format_id` | `Task.format_id` | Emitted |
| `epic_id` | `Task.epic_id` | Emitted |
| `title` | `Task.title` | Emitted |
| `area_type` | `Task.area_type` | Emitted |
| `work_type` | `Task.work_type` | Emitted |
| `status` | `Task.status` | Emitted (always `"todo"`) |

**Fields in `Task` model NOT emitted in `task_created` JSONL:**

| DB field | Type | JSONL status | Impact on rebuild |
|----------|------|-------------|------------------|
| `description` | `Option<String>` | Missing | Cannot rebuild task description |
| `domain` | `String` | Missing | Cannot rebuild domain |
| `origin` | `String` | Missing | Cannot distinguish planned vs adhoc |
| `file_scope` | `Vec<String>` | Missing | Cannot rebuild file scope restrictions |
| `scope_policy` | `String` | Missing | Cannot rebuild scope enforcement mode |
| `scope_root` | `Option<String>` | Missing | Cannot rebuild scope root |
| `estimate` | `Option<String>` | Missing | Cannot rebuild effort estimate |
| `priority` | `String` | Missing | Cannot rebuild priority |
| `assignee_id` | `Option<String>` | Missing | Cannot rebuild assignee |
| `autorun_eligible` | `bool` | Missing | Cannot rebuild autorun eligibility |
| `auto_commit` | `bool` | Missing | Cannot rebuild auto-commit policy |
| `raise_pr` | `bool` | Missing | Cannot rebuild PR raise policy |
| `auto_merge` | `bool` | Missing | Cannot rebuild merge policy |
| `target_branch` | `Option<String>` | Missing | Cannot rebuild branch target |
| `acceptance` | `Vec<String>` | Missing | Cannot rebuild acceptance criteria |
| `tests` | `Vec<String>` | Missing | Cannot rebuild test requirements |
| `branch` | `Option<String>` | Missing | Cannot rebuild feature branch |
| `pr_number` | `Option<i64>` | Missing | Partially derivable from `pr_created` |
| `external_id` | `Option<String>` | Missing | Cannot rebuild external reference |
| `external_url` | `Option<String>` | Missing | Cannot rebuild external URL |
| `started_at` | `Option<String>` | Missing | Derivable from first `in_progress` event |
| `completed_at` | `Option<String>` | Missing | Derivable from `complete` status change |
| `stage` | `Option<String>` | Missing | Cannot rebuild pipeline stage |
| `stage_status` | `Option<String>` | Missing | Cannot rebuild stage status |
| `stage_history` | `Vec<_>` | Missing | Cannot rebuild stage history |

**Summary:** The Rust `create_task()` emits 7 of 34 DB fields (21%). 27 fields are not
captured in any JSONL event (`created_at` and `updated_at` are derivable from event
timestamps but are not explicitly emitted).

### 5.2 epic_created — Fields emitted vs. DB fields

The Rust `create_epic()` function (epic.rs:104-130) emits:

| JSONL field emitted | DB column equivalent | Notes |
|--------------------|---------------------|-------|
| `id` | `Epic.id` | ULID; emitted |
| `format_id` | `Epic.format_id` | Emitted |
| `title` | `Epic.title` | Emitted |
| `area_type` | `Epic.area_type` | Emitted |
| `work_type` | `Epic.work_type` | Emitted |
| `status` | `Epic.status` | Emitted (always `"draft"`) |

**Fields in `Epic` model NOT emitted in `epic_created` JSONL:**

| DB field | Type | JSONL status | Impact on rebuild |
|----------|------|-------------|------------------|
| `summary` | `Option<String>` | Missing | Cannot rebuild epic description |
| `domain` | `String` | Missing | Cannot rebuild domain |
| `is_ongoing` | `bool` | Missing | Cannot rebuild ongoing flag |
| `file_scope` | `Vec<String>` | Missing | Cannot rebuild file scope |
| `priority` | `String` | Missing | Cannot rebuild priority |
| `pr_number` | `Option<i64>` | Missing | Partially derivable from PR events |
| `external_id` | `Option<String>` | Missing | Cannot rebuild external reference |
| `external_url` | `Option<String>` | Missing | Cannot rebuild external URL |
| `created_at` | `String` | Missing | Derivable from event timestamp |
| `updated_at` | `String` | Missing | Derivable from latest event timestamp |

**Summary:** The Rust `create_epic()` emits 6 of 16 DB fields (38%). 10 fields are not
captured in any JSONL event (`created_at` and `updated_at` are derivable from event
timestamps but are not explicitly emitted).

### 5.3 Gaps in `begin_work` / `complete_work` events

The `active_work` DB table tracks active work sessions. Current JSONL coverage (from
schema-standardization.md Section 8.2):

| DB field | JSONL coverage | Notes |
|----------|---------------|-------|
| `id` | Partial — `work_id` or `id` field | Naming inconsistency (Section 4.2) |
| `task_id` | Yes — `task_id` present | |
| `topic` | Partial | Present on some entries |
| `status` | Derivable | `begin_work` = `in_progress`, `complete_work` = `complete` |
| `branch` | Partial | Present on some `begin_work` entries |
| `scope` | Missing | Not emitted in any JSONL event |
| `deliverables` | Missing | Not emitted in any JSONL event |
| `agent` | Missing | Not emitted in any JSONL event |
| `session_id` | Partial | Present on some entries; always `None` in Rust CLI |
| `current_stage` | Missing | Not emitted in any JSONL event |
| `team_name` | Missing | Removed in PR #133 |
| `created_at` | Yes — `timestamp` | Derivable from event timestamp |
| `updated_at` | Missing | Derivable from latest event timestamp |

### 5.4 `session_id` always None in Rust CLI

The Rust CLI work-graph functions (`create_task`, `create_epic`, `update_task`,
`update_epic`) all construct `Event` with `session_id: None`:

```rust
// From codeflow-cli/core/src/workgraph/task.rs:149-155
let event = Event {
    event_type: "task_created".to_string(),
    timestamp: now,
    session_id: None,   // ← always None; session context not passed in
    worktree: None,     // ← always None; worktree context not passed in
    data,
};
```

This means all Rust-era work-graph events lack `session_id` and `worktree`, breaking
session traceability for rebuilt DB records.

---

## 6. BEFORE/AFTER Schema Table

### epic_created

| Field | BEFORE (current) | AFTER (canonical target) | Notes |
|-------|-----------------|--------------------------|-------|
| `event` | `"epic_created"` | `"epic_created"` | Unchanged |
| `timestamp` | RFC 3339 string | RFC 3339 string | Format unchanged |
| `session_id` | Optional; absent on most entries | Required | Rust CLI must pass session context |
| `worktree` | Absent | Optional; present when in worktree mode | Rust CLI must pass worktree context |
| `id` | ULID on some entries (`id`); format_id on others (`epic_id`) | `id` always ULID | Standardize: `id` = ULID, `format_id` = human-readable |
| `format_id` | Present on most entries | Present on all entries | Make required |
| `title` | Present | Present (required) | Unchanged |
| `status` | Present | Present (required) | Unchanged |
| `area_type` | Present | Present | Unchanged |
| `work_type` | Present | Present | Unchanged |
| `domain` | Present | Present | Add to Rust `create_epic()` emit |
| `summary` | Absent | `details.summary` (if set) | Add to Rust `create_epic()` emit |
| `is_ongoing` | Ad-hoc top-level on some entries | `details.is_ongoing` | Move to `details` |
| `priority` | Absent | `details.priority` | Add to Rust `create_epic()` emit |

### task_created

| Field | BEFORE (current) | AFTER (canonical target) | Notes |
|-------|-----------------|--------------------------|-------|
| `event` | `"task_created"` | `"task_created"` | Unchanged |
| `timestamp` | RFC 3339 string | RFC 3339 string | Format unchanged |
| `session_id` | Optional; absent on most entries | Required | Rust CLI must pass session context |
| `worktree` | Absent | Optional; present when in worktree mode | Rust CLI must pass worktree context |
| `id` | ULID on canonical entries; absent on many legacy entries | `id` always ULID | Standardize |
| `format_id` | Present on most entries | Required | Make required |
| `task_id` | **Overloaded**: ULID OR format_id | Remove this field (use `id` for ULID) | Bug fix (INF-TSK-024-008) |
| `epic_id` | ULID on canonical entries; format_id on legacy entries | ULID | Standardize; add `epic_format_id` separately |
| `title` | Present | Present (required) | Unchanged |
| `status` | Present | Present (required) | Unchanged |
| `area_type` | Present | Present | Unchanged |
| `work_type` | Present | Present | Unchanged |
| `domain` | Present | Present | Add to Rust `create_task()` emit |
| `description` | Absent | `details.description` (if set) | Add to Rust `create_task()` emit |
| `origin` | Ad-hoc top-level on some entries | `details.origin` | Move to `details` |
| `estimate` | Ad-hoc top-level on some entries | `details.estimate` | Move to `details` |
| `priority` | Ad-hoc top-level on some entries | `details.priority` | Move to `details` |
| `acceptance` | Ad-hoc top-level on some entries | `details.acceptance` | Move to `details` |
| `file_scope` | Ad-hoc top-level on some entries | `details.file_scope` | Move to `details` |
| `note` | Ad-hoc top-level on some entries | `details.note` | Move to `details` |
| `branch` | Ad-hoc top-level on some entries | `details.branch` | Move to `details` |

### task_status_changed

| Field | BEFORE (current) | AFTER (canonical target) | Notes |
|-------|-----------------|--------------------------|-------|
| `event` | `"task_status_changed"` | `"task_status_changed"` | Unchanged |
| `timestamp` | RFC 3339 string | RFC 3339 string | Format unchanged |
| `session_id` | Partial | Required | Rust CLI must pass session context |
| `worktree` | Absent | Optional | Rust CLI must pass worktree context |
| `task_id` | ULID (Rust-era) | Rename to `id` (ULID) | Fix field name to match `id` convention |
| `format_id` | Present on most entries | Required | Unchanged |
| `new_status` | Go-era field name | Rename to `to_status` | Canonical field name |
| `old_status` | Go-era field name | Rename to `from_status` | Canonical field name |
| `from_status` | Rust-era field name | `from_status` (canonical) | Unchanged |
| `to_status` | Rust-era field name | `to_status` (canonical) | Unchanged |
| `summary` | Ad-hoc top-level | `details.summary` | Move to `details` |
| `reason` | Ad-hoc top-level | `details.reason` | Move to `details` |
| `branch` | Ad-hoc top-level | `details.branch` | Move to `details` |
| `completed_at` | Ad-hoc top-level | `details.completed_at` | Move to `details` |

### begin_work / complete_work

| Field | BEFORE (current) | AFTER (canonical target) | Notes |
|-------|-----------------|--------------------------|-------|
| `event` | `"begin_work"` / `"complete_work"` | Unchanged | |
| `timestamp` | RFC 3339 string | RFC 3339 string | Format unchanged |
| `session_id` | Partial | Required | |
| `worktree` | Absent | Optional | |
| `work_id` | Partial; sometimes `id` | `work_id` (canonical) | Standardize to `work_id` |
| `id` | Partial; sometimes `work_id` | Remove (use `work_id`) | Avoid dual field names |
| `task_id` | ULID | `task_id` (ULID) | Unchanged |
| `format_id` | Partial | Required | Add to all entries |
| `branch` | Ad-hoc top-level | `details.branch` | Move to `details` |
| `topic` | Ad-hoc top-level | `details.topic` | Move to `details` |
| `domain` | Ad-hoc top-level | `details.domain` | Move to `details` |
| `summary` | Ad-hoc top-level on `complete_work` | `details.summary` | Move to `details` |
| `data` | Ad-hoc nested object on `complete_work` | `details` (merged) | Flatten into `details` |

---

## 7. BEFORE/AFTER JSON Examples

### 7.1 epic_created (BEFORE — Pattern B bug, `epic_id` holds format_id)

Actual entry from `.state/ledger/work-graph.jsonl` line 6:

```json
{
  "area_type": "INF",
  "domain": "GENL",
  "epic_id": "INF-EPC-006",
  "event": "epic_created",
  "status": "in_progress",
  "timestamp": "2026-02-19T05:34:44Z",
  "title": "Sandbox Network Settings & Templates",
  "work_type": "CHOR"
}
```

Issues: `epic_id` holds format_id string `"INF-EPC-006"` instead of a ULID. No `id`
field. No `session_id`. `format_id` absent.

### 7.2 epic_created (AFTER — canonical target)

```json
{
  "event": "epic_created",
  "timestamp": "2026-02-19T05:34:44Z",
  "session_id": "ses-01kj...",
  "id": "epic-01KJ...",
  "format_id": "INF-EPC-006",
  "title": "Sandbox Network Settings & Templates",
  "status": "in_progress",
  "area_type": "INF",
  "work_type": "CHOR",
  "details": {
    "domain": "GENL"
  }
}
```

### 7.3 task_created (BEFORE — Pattern B bug, `task_id` holds format_id)

Actual entry from `.state/ledger/work-graph.jsonl` line 7:

```json
{
  "area_type": "INF",
  "branch": "chore/sandbox-network-settings",
  "domain": "GENL",
  "epic_id": "INF-EPC-006",
  "event": "task_created",
  "status": "todo",
  "task_id": "INF-TSK-006-001",
  "timestamp": "2026-02-19T05:34:44Z",
  "title": "Add sandbox network allowedDomains to settings templates and update docs",
  "work_type": "CHOR"
}
```

Issues: `task_id` holds format_id string `"INF-TSK-006-001"`. `epic_id` holds format_id
string `"INF-EPC-006"`. No ULID for the task or epic. No `session_id`. No `format_id`.

### 7.4 task_created (AFTER — canonical target)

```json
{
  "event": "task_created",
  "timestamp": "2026-02-19T05:34:44Z",
  "session_id": "ses-01kj...",
  "id": "task-01KJ...",
  "format_id": "INF-TSK-006-001",
  "epic_id": "epic-01KJ...",
  "epic_format_id": "INF-EPC-006",
  "title": "Add sandbox network allowedDomains to settings templates and update docs",
  "status": "todo",
  "area_type": "INF",
  "work_type": "CHOR",
  "details": {
    "branch": "chore/sandbox-network-settings",
    "domain": "GENL"
  }
}
```

### 7.5 task_status_changed (BEFORE — Pattern B bug, `task_id` holds format_id)

Actual entry from `.state/ledger/work-graph.jsonl` line 8:

```json
{
  "epic_id": "INF-EPC-006",
  "event": "task_status_changed",
  "new_status": "complete",
  "old_status": "in_progress",
  "reason": "PR #39 merged to main",
  "task_id": "INF-TSK-006-001",
  "timestamp": "2026-02-19T13:44:49Z"
}
```

Issues: `task_id` holds format_id `"INF-TSK-006-001"`. `epic_id` holds format_id. Uses
Go-era field names `old_status`/`new_status`. `reason` at top level. No ULID for task.

### 7.6 task_status_changed (AFTER — canonical target)

```json
{
  "event": "task_status_changed",
  "timestamp": "2026-02-19T13:44:49Z",
  "session_id": "ses-01kj...",
  "id": "task-01KJ...",
  "format_id": "INF-TSK-006-001",
  "from_status": "in_progress",
  "to_status": "complete",
  "details": {
    "reason": "PR #39 merged to main"
  }
}
```

### 7.7 task_status_changed (BEFORE — canonical Pattern A, but missing details nesting)

Actual entry from `.state/ledger/work-graph.jsonl` line 4:

```json
{
  "branch": "chore/schema-format-id-convention",
  "event": "task_status_changed",
  "format_id": "INF-TSK-005-002",
  "id": "task-01KHSQPQSF26W0SQE9GZPM8XDE",
  "new_status": "in_progress",
  "old_status": "todo",
  "timestamp": "2026-02-19T03:57:15Z"
}
```

Issues: Uses Go-era `old_status`/`new_status`. `branch` at top level. No `session_id`.

### 7.8 task_status_changed (AFTER — canonical target)

```json
{
  "event": "task_status_changed",
  "timestamp": "2026-02-19T03:57:15Z",
  "session_id": "ses-01kj...",
  "id": "task-01KHSQPQSF26W0SQE9GZPM8XDE",
  "format_id": "INF-TSK-005-002",
  "from_status": "todo",
  "to_status": "in_progress",
  "details": {
    "branch": "chore/schema-format-id-convention"
  }
}
```

### 7.9 begin_work (BEFORE — work ID inconsistency)

Actual entry from `.state/ledger/work-graph.jsonl` line 341:

```json
{
  "branch": "docs/inf-tsk-021-021-agent-defs-go-cli",
  "event": "begin_work",
  "format_id": "INF-TSK-021-021",
  "id": "work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ",
  "task_id": "task-01KJHQCM92SPJJSC8V306TA50E",
  "timestamp": "2026-03-04T04:21:16Z",
  "work_id": "work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ"
}
```

Issues: `id` and `work_id` both present with the same value — duplicated. No `session_id`.
`branch` at top level.

### 7.10 begin_work (AFTER — canonical target)

```json
{
  "event": "begin_work",
  "timestamp": "2026-03-04T04:21:16Z",
  "session_id": "ses-01kj...",
  "work_id": "work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ",
  "task_id": "task-01KJHQCM92SPJJSC8V306TA50E",
  "format_id": "INF-TSK-021-021",
  "details": {
    "branch": "docs/inf-tsk-021-021-agent-defs-go-cli"
  }
}
```

### 7.11 pr_created (BEFORE)

Actual entry from `.state/ledger/work-graph.jsonl` line 150:

```json
{
  "branch": "feat/session-package",
  "entry_id": "wg-01KJBHP22KQ8V78G4KWWGM2ETA",
  "epic_format_id": "INF-EPC-015",
  "event": "pr_created",
  "pr_number": 77,
  "pr_url": "https://github.com/sathyassn/codeflow/pull/77",
  "session_id": "ses-17720365524152d18a44455b5",
  "task_format_id": "INF-TSK-015-006",
  "task_id": "task-01KJBFCQ6BEBRM6CPWC3EHMMCF",
  "timestamp": "2026-02-25T23:20:00Z",
  "work_id": "work-01KJBFHEA5W49Y21FDS21NYSWY"
}
```

Issues: `branch`, `entry_id`, `epic_format_id` at top level — candidates for `details`.

### 7.12 pr_created (AFTER — canonical target)

```json
{
  "event": "pr_created",
  "timestamp": "2026-02-25T23:20:00Z",
  "session_id": "ses-17720365524152d18a44455b5",
  "pr_number": 77,
  "task_id": "task-01KJBFCQ6BEBRM6CPWC3EHMMCF",
  "task_format_id": "INF-TSK-015-006",
  "work_id": "work-01KJBFHEA5W49Y21FDS21NYSWY",
  "details": {
    "branch": "feat/session-package",
    "epic_format_id": "INF-EPC-015",
    "pr_url": "https://github.com/sathyassn/codeflow/pull/77"
  }
}
```

---

## 8. Rust Type Reference

These types exist in the current codebase and are used by the work-graph module.

### 8.1 TaskId

**Location:** `codeflow-cli/core/src/types/ids.rs:93-97`

A `#[serde(transparent)]` newtype wrapper around `String`. Validates non-empty on
construction. Maps to the `id` field in `task_created` events and `task_id` field in
`task_status_changed` events.

**Current JSONL field usage:** `id` (canonical) and `task_id` (Go-era and Rust-era).
The canonical target uses `id` for the ULID and removes `task_id` ambiguity.

**Format:** `task-{ULID}` (e.g., `task-01KHSQPQSF26W0SQE9GZPM8XDE`).

### 8.2 EpicId

**Location:** `codeflow-cli/core/src/types/ids.rs:99-103`

A `#[serde(transparent)]` newtype wrapper around `String`. Maps to the `epic_id` field
and (in canonical entries) the `id` field of `epic_created` events.

**Current JSONL field usage:** `epic_id` (all eras, holds either ULID or format_id
depending on era — see Section 3). `id` on Rust-era `epic_created` events.

**Format:** `epic-{ULID}` (e.g., `epic-01KHSQPQRNQP0XTXCRHXX9YW1T`).

### 8.3 WorkId

**Location:** `codeflow-cli/core/src/types/ids.rs:105-109`

Maps to the `work_id` and (inconsistently) `id` field in `begin_work` and
`complete_work` events.

**Current JSONL usage:** `work_id` (convention), `id` (some entries), or both (line 341).
The canonical target uses `work_id` consistently.

**Format:** `work-{ULID}` (e.g., `work-01KJVHDYAJ4QHBYJ7TP5SXQZBJ`).

### 8.4 FormatId

**Location:** `codeflow-cli/core/src/types/ids.rs:117-121`

A human-readable format identifier. Maps to the `format_id` field across all work-graph
events. Also used (incorrectly) as the value of `task_id` and `epic_id` in legacy entries.

**Format:** `{AREA}-{EPC|TSK}-{NNN}[-{NNN}]` (e.g., `INF-TSK-022-006`, `INF-EPC-008`).

### 8.5 SessionId

**Location:** `codeflow-cli/core/src/types/ids.rs:87-91`

Maps to the `session_id` field on all events. Currently `None` in all Rust-era work-graph
events (see Section 5.4). The canonical target makes `session_id` required on all events.

**Format:** `ses-{ULID}` (e.g., `ses-01kjw2gertbnn8xnpmr8mcrmc8`).

### 8.6 BranchName

**Location:** `codeflow-cli/core/src/types/ids.rs:111-115`

Maps to the `branch` field in various events. Currently written as a flat top-level
field; the canonical target moves it to the `details` object.

### 8.7 LedgerEvent enum

**Location:** `codeflow-cli/core/src/types/events.rs`

Defines typed variants discriminated by `"event"` tag with
`#[serde(tag = "event", rename_all = "snake_case")]`. The work-graph relevant variants
accommodate both Go-era and Rust-era field variations via `data: serde_json::Value` with
`#[serde(flatten)]`.

### 8.8 WorktreeId (not yet defined)

**Finding:** No `WorktreeId` newtype exists in `codeflow-cli/core/src/types/ids.rs` as
of this audit. The `worktree` field on the `Event` struct is `Option<String>`. A
`WorktreeId` type is expected to be defined as part of INF-TSK-024-007's canonical
schema synthesis.

### 8.9 Event struct (LedgerWriter output format)

**Location:** `codeflow-cli/core/src/ledger/mod.rs`

All Rust CLI work-graph events are serialized through:

```rust
pub struct Event {
    #[serde(rename = "event")]
    pub event_type: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}
```

Key behaviors:

- `session_id` is omitted from JSON when `None` — all current Rust work-graph functions
  set `session_id: None`, so all Rust-era work-graph events lack this field.
- `worktree` is omitted from JSON when `None` — same situation.
- `data` fields are flattened into the top-level JSON object.

The fix for INF-TSK-024-008 requires passing `session_id: Some(...)` when constructing
`Event` in `create_task()`, `create_epic()`, `update_task()`, and `update_epic()`.
