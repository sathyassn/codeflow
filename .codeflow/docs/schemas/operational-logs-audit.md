---
title: "Operational Logs Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-04"
updated_at: "2026-04-04"
scope: INF-TSK-024-006
feeds_into:
  - INF-TSK-024-007
---

# Operational Logs Schema Audit

This document audits the schema of operational logs: git commit and PR event logs,
database operation logs, session cleanup logs, and all discontinued log categories. It
covers writers in `.state/logs/git/`, `.state/logs/db/`, `.state/logs/sessions/`
(cleanup only), and the empty `.state/logs/test-category/` directory. It feeds into
INF-TSK-024-007 (canonical event schema synthesis).

## Table of Contents

- [1. Directory Layout](#1-directory-layout)
- [2. git/commits-{date}.jsonl — Git Commit Events](#2-gitcommits-datedatejsonl--git-commit-events)
- [3. git/pr-events-{date}.jsonl — PR Events](#3-gitpr-events-datedatejsonl--pr-events)
- [4. sessions/cleanup-{date}.jsonl — Session Cleanup Events](#4-sessionscleanup-datedatejsonl--session-cleanup-events)
- [5. db/operations-{date}.jsonl — Database Operation Events](#5-dboperations-datedatejsonl--database-operation-events)
- [6. Discontinued Operational Logs](#6-discontinued-operational-logs)
- [7. Field Naming Inconsistencies](#7-field-naming-inconsistencies)
- [8. Rust Target Assessment](#8-rust-target-assessment)

---

## 1. Directory Layout

Operational logs are distributed across three subdirectories of `.state/logs/`:

```text
.state/logs/
├── git/
│   ├── commits-{date}.jsonl        (32 files, 2026-02-07 to 2026-03-14)
│   └── pr-events-{date}.jsonl      (39 files, 2026-02-22 to 2026-04-04)
├── db/
│   └── operations-{date}.jsonl     (5 files, 2026-02-03 to 2026-02-09)
├── sessions/
│   └── cleanup-{date}.jsonl        (21 files, 2026-03-14 to 2026-04-04)
└── test-category/                   (EMPTY directory)
```

File counts as of 2026-04-04.

---

## 2. git/commits-{date}.jsonl — Git Commit Events

**Writer:** Go CLI (`codeflow hooks post-tool-use logging`). This is NOT a Rust writer.

**Count:** 32 files (2026-02-07 to 2026-03-14). Last entry observed: 2026-03-14.

**Status:** Semi-active. The Go CLI post-tool-use logging path fires when the Go hooks
are active. The file series stops at 2026-03-14, which aligns with the Go-to-Rust
transition period. New entries may not be written in sessions that use only the Rust CLI.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` — ms precision, literal Z |
| `event` | string | Yes | Always `"commit"` |
| `hash` | string | Yes | Full 40-character commit SHA |
| `short_hash` | string | Yes | Short commit SHA (7 chars) |
| `message` | string | Yes | Full commit message |
| `author` | string | Yes | Commit author (GitHub username) |
| `branch` | string | Yes | Branch at time of commit |

**CRITICAL GAP:** No `session_id` field. Commits cannot be attributed to sessions.
This prevents cross-referencing commit activity with session lifecycle events.

**Actual example:**

```json
{"ts":"2026-03-14T07:41:43.437Z","event":"commit","hash":"7d84ab9b41f0fd40f2ff3394c16654c3bd044cc0","short_hash":"7d84ab9","message":"Merge pull request #173 from fix/session-cleanup-redesign","author":"sathyassn","branch":"refactor/pre-cutover-cleanup"}
```

**Retention alignment:** No rotation config identified for `git/commits-*.jsonl`. Files
accumulate without age-based or size-based cleanup.

---

## 3. git/pr-events-{date}.jsonl — PR Events

**Writer:** Go CLI + cf-knowledge-layer ledger events (two source paths).

**Count:** 39 files (2026-02-22 to 2026-04-04). The most recent entries are from
2026-04-04 — this is an active log.

**Status:** Active. PR events continue to be written on each session that creates or
merges a PR.

Two distinct schemas appear in the same files, depending on the write path:

### 3.1 Go-era schema (standard entries)

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` |
| `event` | string | Yes | `"pr_created"` or `"pr_merged"` |
| `pr_number` | integer | Yes | GitHub PR number |
| `pr_url` | string | Conditional | Present on `pr_created`; absent on `pr_merged` |
| `task_id` | string | No | Associated task format ID |
| `branch` | string | Conditional | Present on `pr_created` |
| `target` | string | Conditional | Target branch (e.g., `"main"`); present on `pr_created` |
| `session_id` | string | No | Session that created/merged the PR |
| `merge_sha` | string | Conditional | Short merge commit SHA; present on `pr_merged` |

**Actual example (pr_created):**

```json
{"ts":"2026-04-04T17:58:31Z","event":"pr_created","pr_number":247,"pr_url":"https://github.com/sathyassn/codeflow/pull/247","task_id":"INF-EPC-033","branch":"fix/autorun-worker-invocation","target":"main","session_id":"ses-01knck44qb3yhpp722rpbgx80h"}
```

**Actual example (pr_merged):**

```json
{"ts":"2026-04-04T19:46:28Z","event":"pr_merged","pr_number":247,"merge_sha":"5b6fcab3","task_id":"INF-EPC-033","session_id":"ses-01knck44qb3yhpp722rpbgx80h"}
```

### 3.2 LLM-written entries (schema variation)

Some entries use `event_type` instead of `event` as the discriminator field name. This
is an LLM-era artifact where cf-knowledge-layer wrote entries directly rather than via
the Go CLI hook path.

**Known field variation:**

| Canonical field | Variant field | Notes |
|----------------|--------------|-------|
| `event` | `event_type` | Appears in some older LLM-written entries |

Readers of `pr-events-*.jsonl` must handle both `event` and `event_type` field names
to parse all entries correctly.

---

## 4. sessions/cleanup-{date}.jsonl — Session Cleanup Events

**Writer:** Rust standalone function `write_cleanup_log()` in
`codeflow-cli/core/src/hooks/session_end.rs:740-765`.

**Count:** 21 files (2026-03-14 to 2026-04-04). First appeared 2026-03-14 — introduced
with the session cleanup redesign. Active writer.

**Note:** This writer is NOT `ActivityWriter`. It uses `std::fs::OpenOptions` directly
with no flock locking.

Three event types appear in `cleanup-{date}.jsonl`:

### 4.1 cleanup_started

Written at the beginning of `SessionEndCleanup::run()` before PathFlow guard checks.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"cleanup_started"` |
| `session_id` | string | Yes | Session being cleaned up |
| `pid` | integer | Yes | Lead process PID (`lead_pid` from hook context) |
| `timestamp` | string | Yes | RFC 3339 (via `chrono::Utc::now().to_rfc3339()`) |

### 4.2 cleanup_skipped

Written when the PathFlow guard determines the session is still active (skip path).

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"cleanup_skipped"` |
| `session_id` | string | Yes | Session that was skipped |
| `reason` | string | Yes | Always `"session_active"` in current code |
| `timestamp` | string | Yes | RFC 3339 |

### 4.3 cleanup_completed

Written after cleanup finishes (sentinel removal, state directory cleanup).

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"cleanup_completed"` |
| `session_id` | string | Yes | Session that was cleaned up |
| `pf7_valid` | boolean | Yes | Whether PathFlow PF7 was valid at cleanup time |
| `sentinels_cleaned` | integer | Yes | Count of sentinel files removed |
| `task_preserved` | boolean | Yes | Whether the active task was preserved |
| `team_name` | string | No | Team name (from cleanup result) |
| `warnings_count` | integer | Yes | Count of warnings generated during cleanup |
| `timestamp` | string | Yes | RFC 3339 |

**Actual example (from 2026-03-14, earliest file):**

```json
{"event":"cleanup_started","pid":66336,"session_id":"ses-01kkpkvmh1xbvbggw5khtn0mve","stdin_session_id":"041edb05-c7fd-4045-8327-e41f9f32092c","timestamp":"2026-03-14T17:00:52Z"}
{"event":"cleanup_completed","env_file_removed":true,"pf7_valid":false,"sentinels_cleaned":1,"session_id":"ses-01kkpkvmh1xbvbggw5khtn0mve","session_state_removed":true,"team_config_removed":false,"team_tasks_removed":false,"timestamp":"2026-03-14T17:00:52Z"}
```

**Note:** Early cleanup entries (2026-03-14) contain additional fields not present in
the current code path (`stdin_session_id`, `env_file_removed`, `session_state_removed`,
`team_config_removed`, `team_tasks_removed`). The current `write_cleanup_log()` source
in `session_end.rs` only writes the fields documented in sections 4.1–4.3 above.

**INCONSISTENCY:** Uses `timestamp` (RFC 3339) rather than `ts` (ms-precision literal Z).
All other logs in `.state/logs/sessions/` that use `ActivityWriter` use `ts`. Cleanup
uses a standalone writer producing a different timestamp field and format.

**Locking:** No `flock` locking. Uses raw `OpenOptions::new().create(true).append(true)`.
Concurrent writes from parallel session cleanup could interleave entries.

---

## 5. db/operations-{date}.jsonl — Database Operation Events

**Writer:** Test harness only. Not an active production writer.

**Count:** 5 files (2026-02-03 to 2026-02-09). No entries after 2026-02-09.

**Status:** DISCONTINUED. The test harness that produced these entries was replaced by
`slog`-based internal DB logging in `internal/db/logging.go`. No active writer emits
to `db/operations-*.jsonl` in the current codebase.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | Timestamp with microsecond precision |
| `level` | string | Yes | `"INFO"` or `"ERROR"` |
| `session_id` | string | Yes | Always `"unknown"` — test harness artifact |
| `event` | string | Yes | Always `"db_operation"` |
| `operation` | string | Yes | `"TRANSACTION"`, `"QUERY"`, `"WRITE"`, or `"BACKUP"` |
| `status` | string | Yes | `"COMMITTED"`, `"ROLLED_BACK"`, or `"SUCCESS"` |
| `details` | string | No | Additional context (e.g., row counts, error message) |
| `duration_ms` | integer | Yes | Always 0 — test harness does not measure real duration |

**Evidence of test harness origin:** `session_id` is `"unknown"` in all 5 files;
`duration_ms` is 0 in all entries; field values match test fixture patterns, not
production operation patterns.

**Actual example:**

```json
{"ts":"2026-02-09T05:00:09.972028Z","level":"ERROR","session_id":"unknown","event":"db_operation","operation":"TRANSACTION","status":"COMMITTED","duration_ms":0}
```

---

## 6. Discontinued Operational Logs

### 6.1 test-category/ directory (EMPTY)

**Location:** `.state/logs/test-category/`

**Status:** Empty directory. Never populated. No writer emits to this path. Remove
candidate — the directory serves no current purpose.

### 6.2 pathflow-events.jsonl (cross-reference)

**Location:** `.state/logs/pathflow-events.jsonl` (at log root, not in a subdirectory)

This file is documented separately in the pathflow and work-graph audits. It is
referenced here for completeness because it uses a mixed schema across eras:

| Era | Timestamp field | Event field |
|-----|----------------|-------------|
| Shell | `ts` | `type` or `e` |
| Go | `timestamp` | `event` |

A `.lock` side-file exists at `.state/logs/pathflow-events.jsonl.lock`.

---

## 7. Field Naming Inconsistencies

Field naming is inconsistent across operational logs. The table below catalogs the
divergences:

| File | Timestamp field | Timestamp format | Event field | session_id |
|------|----------------|-----------------|-------------|-----------|
| `git/commits-{date}.jsonl` | `ts` | ms, literal Z | `event` | ABSENT |
| `git/pr-events-{date}.jsonl` | `ts` | seconds, literal Z | `event` (or `event_type`) | Present |
| `sessions/cleanup-{date}.jsonl` | `timestamp` | RFC 3339 | `event` | Present |
| `db/operations-{date}.jsonl` | `ts` | microseconds, suffix Z | `event` | Present (always `"unknown"`) |
| `pathflow-events.jsonl` | `timestamp` (Go) / `ts` (shell) | RFC 3339 (Go) | `event` (Go) / `type`/`e` (shell) | Varies |

**Key findings:**

1. **`commits-{date}.jsonl` has no `session_id`.** This prevents correlating git
   commits with sessions. All other active operational logs include `session_id`.

2. **`cleanup-{date}.jsonl` uses `timestamp` (RFC 3339)** while all `ActivityWriter`
   logs in the same directory use `ts` (ms-precision literal Z). This is because
   cleanup uses a standalone writer (`write_cleanup_log()`) rather than `ActivityWriter`.

3. **`pr-events-{date}.jsonl` has an `event`/`event_type` schema split** between the
   Go CLI path and LLM-written entries.

4. **Timestamp precision varies:** `ActivityWriter` produces ms precision (`%.3f`);
   `write_cleanup_log()` produces RFC 3339 (second precision); the Go CLI produces
   varying precision depending on the writer.

---

## 8. Rust Target Assessment

This section assesses which operational logs have Rust writers in
`codeflow-cli/core/src/hooks/` and which are obsolete.

| Log file | Rust writer? | Status | Notes |
|---------|-------------|--------|-------|
| `git/commits-{date}.jsonl` | No | Semi-active (Go CLI) | No Rust implementation. Go CLI still writes when hook path fires. |
| `git/pr-events-{date}.jsonl` | No (Go CLI) | Active | Go CLI writes via hook. LLM path also writes directly. No Rust hook implementation. |
| `sessions/cleanup-{date}.jsonl` | Yes (`session_end.rs`) | Active | `write_cleanup_log()` in `hooks/session_end.rs`. Standalone, not `ActivityWriter`. |
| `db/operations-{date}.jsonl` | No | DISCONTINUED | Test harness only. Replaced by `slog`-based DB logging in Go. |
| `test-category/` | No | EMPTY | Never populated. Remove candidate. |

**Rust implementation gaps for operational logs:**

- `git/commits-{date}.jsonl`: No Rust hook writes git commit events. If the Go hooks
  stop firing (full Rust CLI transition), commit logging will stop entirely.
- `git/pr-events-{date}.jsonl`: No Rust hook implementation. PR events are written by
  the Go CLI and LLM-era knowledge-layer path. A Rust implementation would use a new
  `LedgerEvent::PrCreated` / `LedgerEvent::PrMerged` variant routing to
  `git/pr-events.jsonl`.

**Cleanup log Rust implementation note:** `write_cleanup_log()` is a standalone
function in `session_end.rs`, not integrated with `ActivityWriter`. It lacks flock
locking. A future improvement would integrate cleanup events into `ActivityWriter` for
consistent locking and field naming, or route them through the `LedgerWriter` as ledger
events.
