---
title: "Schema Standardization"
type: analysis
status: draft
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-05"
parent: "parallel-work/README.md"
---

# Schema Standardization

[← Back to Overview](README.md)

## Table of Contents

- [1. Ledger Files Audit](#1-ledger-files-audit)
- [2. Log Files Audit](#2-log-files-audit)
- [3. Discontinued Logs Assessment](#3-discontinued-logs-assessment)
- [4. Retention Policies](#4-retention-policies)
- [5. Design Principles](#5-design-principles)
- [6. Canonical Event Schema](#6-canonical-event-schema)
- [7. Target Schemas Per File Type](#7-target-schemas-per-file-type)
- [8. DB Table to JSONL Rebuild Mapping](#8-db-table-to-jsonl-rebuild-mapping)
- [9. Schema Inconsistencies Summary](#9-schema-inconsistencies-summary)

---

## 1. Ledger Files Audit

### `sessions.jsonl` -- 10 entries

**Location:** `.state/ledger/sessions.jsonl`
**Writer:** `ledger.Writer.AppendEvent()` (Go CLI, `internal/ledger/writer.go`)
**Required fields:** `session_id` (for session_start, session_end, session_progress, work_claimed), `id` (for claim_created), `claim_id` (for claim_released, claim_renewed) — per `schema.go:11-18`

**Current schema (session_start — shell era):**

```json
{"event":"session_start","interaction_mode":"interactive","session_id":"ses-177137202131769e89b2d5688","timestamp":"2026-02-19T13:45:12Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_start"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | Yes | Session identifier |
| `interaction_mode` | string | No | `"interactive"` or `"autorun"` (shell-era only) |

**Current schema (session_start — Go canonical):**

```json
{"claude_id":"test-uuid-recovery","event":"session_start","session_id":"ses-01kjvsp7d4vp7bde744yk405tw","timestamp":"2026-03-04T06:48:42Z","user_host":"BlackSwan-mPro.local","user_id":"26560960+sathyassn@users.noreply.github.com"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_start"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | Yes | Session identifier |
| `claude_id` | string | No | Claude per-agent UUID (Go era) |
| `user_host` | string | No | Hostname (Go era) |
| `user_id` | string | No | Git user identity (Go era) |

**Current schema (session_end):**

```json
{"branch":"chore/inf-tsk-005-007-...","event":"session_end","interaction_mode":"interactive","pr_pending":true,"session_id":"ses-177137...","summary":"Updated docs...","timestamp":"2026-02-21T10:37:09Z","work_completed":["INF-TSK-005-007"]}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"session_end"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | Yes | Session identifier |
| `branch` | string | No | Ad-hoc, shell-era |
| `interaction_mode` | string | No | Shell-era only |
| `pr_pending` | boolean | No | Ad-hoc |
| `summary` | string | No | Ad-hoc |
| `work_completed` | string[] | No | Array of task format IDs, ad-hoc |

**Parallel concern:** Low — file-level flock ensures append safety (I/O protection, not coordination), events are independent.

**Migration notes:** Standardize to single schema. Add `worktree` field. Move shell-era ad-hoc fields (`interaction_mode`, `branch`, `pr_pending`, `summary`, `work_completed`) into a `details` object. Keep `claude_id`, `user_host`, `user_id` as top-level fields (useful metadata). Backward compatibility: old entries remain readable; new reader code handles both flat and `details`-nested formats.

### `work-graph.jsonl` -- 359 entries

**Location:** `.state/ledger/work-graph.jsonl`
**Writer:** `ledger.Writer.AppendEvent()` (Go CLI, `internal/ledger/writer.go`)
**Required fields:** Per `schema.go:21-29` — `id`+`title` (epic_created), `epic_id`+`new_status` (epic_status_changed), `id`+`epic_id`+`title` (task_created), `task_id`+`new_status` (task_status_changed), `id` (begin_work, complete_work, work_complete), `task_format_id`+`pr_number` (pr_created), `task_format_id` (pr_merged)

**Current schema (epic_created):**

```json
{"area_type":"INF","domain":"PMGT","epic_id":"epic-01KJ12VSN1YSWYQ03CDK8ENG78","event":"epic_created","format_id":"INF-EPC-008","status":"planning","timestamp":"2026-02-21T21:51:07Z","title":"PathFlow PR Verification...","work_type":"CHOR"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"epic_created"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | No | Often missing in early entries |
| `id` | string (ULID) | Yes | Internal DB key (e.g., `epic-01KJ...`) |
| `format_id` | string | No | Human-readable (e.g., `INF-EPC-008`). Present on most entries. |
| `title` | string | Yes | Epic title |
| `status` | string | No | Initial status (e.g., `planning`) |
| `area_type` | string | No | Area code (e.g., `INF`) |
| `domain` | string | No | Domain code (e.g., `PMGT`) |
| `work_type` | string | No | Work type (e.g., `CHOR`) |

**Current schema (task_created):**

```json
{"area_type":"INF","domain":"PMGT","epic_format_id":"INF-EPC-005","epic_id":"epic-01KHSQPQRNQP0XTXCRHXX9YW1T","event":"task_created","format_id":"INF-TSK-005-002","id":"task-01KHSQPQSF26W0SQE9GZPM8XDE","status":"todo","timestamp":"2026-02-19T03:57:15Z","title":"Update schema.sql for new format ID convention","work_type":"CHOR"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"task_created"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | No | Often missing |
| `id` | string (ULID) | Yes | Internal DB key (e.g., `task-01KH...`) |
| `format_id` | string | No | Human-readable (e.g., `INF-TSK-005-002`) |
| `epic_id` | string (ULID) | Yes | Parent epic internal ID |
| `epic_format_id` | string | No | Parent epic human-readable ID |
| `title` | string | Yes | Task title |
| `status` | string | No | Initial status (e.g., `todo`) |
| `area_type` | string | No | Area code |
| `domain` | string | No | Domain code |
| `work_type` | string | No | Work type |
| `note` | string | No | Ad-hoc (e.g., `retroactive_registration`, `tier1_sync_from_tier2`) |
| `estimate` | string | No | Effort estimate (e.g., `M`) |
| `priority` | string | No | Priority level (e.g., `high`) |

**Current schema (task_status_changed):**

```json
{"branch":"chore/schema-format-id-convention","event":"task_status_changed","format_id":"INF-TSK-005-002","id":"task-01KHSQPQSF26W0SQE9GZPM8XDE","new_status":"in_progress","old_status":"todo","timestamp":"2026-02-19T03:57:15Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"task_status_changed"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | No | Present on newer entries |
| `task_id` | string (ULID) | Yes* | Internal DB key — some entries use `id` instead |
| `format_id` | string | No | Human-readable task ID |
| `new_status` | string | Yes | New status value |
| `old_status` | string | No | Previous status value |
| `branch` | string | No | Ad-hoc |
| `summary` | string | No | Ad-hoc completion summary |
| `pr_number` | integer | No | Ad-hoc (on completed tasks with PRs) |
| `completed_at` | string | No | Ad-hoc completion timestamp |
| `work_id` | string | No | Work session ID |
| `epic_format_id` | string | No | Parent epic (inconsistently present) |
| `reason` | string | No | Ad-hoc (e.g., `"PR #39 merged to main"`) |

**Current schema (begin_work / complete_work):**

```json
{"event":"begin_work","session_id":"ses-177137202131769e89b2d5688","task_id":"INF-TSK-005-007","work_id":"work-01KHZKK5SM27XF26S7TXFSH2GJ","timestamp":"2026-02-21T00:00:00Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | `"begin_work"` or `"complete_work"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | No | Present on newer entries |
| `id` | string | Yes | Required by schema.go but actual entries use `work_id` or `task_id` |
| `work_id` | string | No | Work session ID |
| `task_id` | string | No | Task format ID (not ULID — uses format_id values like `INF-TSK-005-007`) |
| `epic_id` | string | No | Epic ID (inconsistently present) |
| `branch` | string | No | Ad-hoc |
| `summary` | string | No | Ad-hoc (on complete_work) |
| `domain` | string | No | Ad-hoc |
| `data` | object | No | Ad-hoc nested details (on complete_work from cf-knowledge-layer) |

**Current schema (pr_created / pr_merged):**

```json
{"event":"pr_created","task_format_id":"INF-TSK-008-003","pr_number":73,"timestamp":"2026-02-22T..."}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | `"pr_created"` or `"pr_merged"` |
| `timestamp` | string (RFC 3339) | Yes | UTC |
| `session_id` | string | No | Often missing |
| `task_format_id` | string | Yes | Human-readable task ID |
| `pr_number` | integer | Yes* | Required for pr_created, not for pr_merged |

**Dual-ID coexistence:** Both `id` (ULID, e.g., `task-01KH...`) and `format_id` (e.g., `INF-TSK-005-002`) appear on the same entries. Some older entries use `epic_id`/`task_id` with format_id-style values while newer entries use them for ULID values. This inconsistency is the single largest schema problem in this file.

**Parallel concern:** Medium — task updates from parallel sessions interleave. Session ID traceability is present on newer entries but missing on many older ones.

**Migration notes:** Standardize dual-ID usage: `id` always ULID, `format_id` always human-readable. Move ad-hoc fields (`branch`, `summary`, `reason`, `note`, `completed_at`, `data`) into a `details` object. Add `worktree` field. Ensure `session_id` is present on all entries. Backward compatibility: reader handles both flat and nested formats.

### `memory-events.jsonl` -- 211 entries

**Location:** `.state/ledger/memory-events.jsonl`
**Writer:** `ledger.Writer.AppendEvent()` (Go CLI via `codeflow ledger append`)
**Required fields:** Per `schema.go:32-38` — `id` (memory_store, memory_stored, milestone, progress, finding, decision, blocker)
**Routed event types:** `memory_store`, `memory_stored`, `milestone`, `progress`, `finding`, `decision`, `blocker` — per `routing.go:52-58`

**MOST CHAOTIC FILE.** Three coexisting schema patterns from different agent eras:

**Pattern 1 — Nested `data` with `event_type` (dominant, ~80% of entries):**

```json
{"data":{"content":"Completed protection-lib review..."},"domain":"development","event":"memory_store","event_type":"milestone","id":"memory-01KH7A1ZB0P2K8PYNZBW60E7K3","timestamp":"2026-02-11T21:34:59.431076+00:00","work_id":"INF-TSK-FIX-GENL-005"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"memory_store"` |
| `timestamp` | string (RFC 3339) | Yes | UTC, variable precision (some ms, some µs) |
| `id` | string | Yes | Memory ID (e.g., `memory-01KH...` ULID or `memory-{timestamp}{random}`) |
| `event_type` | string | No | Sub-type (e.g., `milestone`, `progress`, `stage_complete`, `decision`, `finding`) — duplicates `event` role |
| `domain` | string | No | Domain (e.g., `development`, `ops`, `planning`) |
| `work_id` | string | No | Associated work ID (inconsistent format — ULID, format_id, or custom) |
| `memory_type` | string | No | Ad-hoc (e.g., `episodic`, `semantic`, `complete-work`) |
| `data` | object | No | Nested payload with `content` (string), plus ad-hoc fields |
| `data.content` | string | No | Main textual content |
| `data.task_format_id` | string | No | Ad-hoc |
| `data.branch` | string | No | Ad-hoc |
| `data.status` | string | No | Ad-hoc |
| `data.summary` | string | No | Ad-hoc |
| `data.type` | string | No | Ad-hoc (e.g., `"finding"`, `"decision"`, `"session-summary"`) |

**Pattern 2 — Flat event type aliases (rare, ~10% of entries):**

```json
{"domain":"ops","event":"memory_milestone","summary":"DB schema migration complete...","task_format_id":"INF-TSK-005-001","timestamp":"2026-02-19T03:36:14Z","work_id":"work-1771452340410uo858e56z2sq"}
```

| Field | Type | Notes |
|-------|------|-------|
| `event` | string | Non-canonical type `"memory_milestone"` — not in routing table, would fail `ValidateEvent()` |
| `summary` | string | Flat top-level (not nested in `data`) |
| `task_format_id` | string | Flat top-level |

**Pattern 3 — Non-canonical event types (rare, ~10% of entries):**

```json
{"data":{"decision":"Expanding branch scope...","rationale":"...","summary":"...","tier":2},"domain":"development","event":"memory_event","event_type":"decision","id":"memory-1771338123-85f75c31","timestamp":"2026-02-17T14:22:03Z","work_id":"work-1771337433-d56020a8"}
```

Uses `event: "memory_event"` — not in routing table. Also uses `event: "finding"`, `event: "milestone"`, `event: "session_summary"` directly, which ARE in the routing table but have different semantics.

**Schema chaos summary:**

| Issue | Frequency | Impact |
|-------|-----------|--------|
| `event_type` duplicates `event` role | ~80% entries | Consumers must check both fields |
| Non-canonical event types (`memory_event`, `memory_milestone`) | ~10% entries | Fail `ValidateEvent()`, bypass routing |
| `data.content` vs flat `summary` | Mixed | Two locations for the same concept |
| `data.type` vs `event_type` vs `event` | Triple overlap | Three fields encoding the same information |
| Inconsistent `id` format | Throughout | ULID (`memory-01KH...`), timestamp-based (`memory-177...`), UUID-like (`memory-e2c68358`) |
| Inconsistent `work_id` format | Throughout | ULID, format_id, custom (`work-17713...`) |
| Doubly-nested data | Rare | `data.data` contains stringified JSON |

**Parallel concern:** Low — rare writes, independent events.

**Migration notes:** Collapse to single schema. Use canonical event types only (`memory_store`, `milestone`, `progress`, `finding`, `decision`, `blocker`). Move all variable content into a `details` object. Eliminate `event_type` field. Standardize `id` format. Add `worktree` field. Backward compatibility: reader handles all three patterns; writer enforces new schema only.

### `config.jsonl` -- 0 entries (empty)

**Location:** `.state/ledger/config.jsonl`
**Writer:** `ledger.Writer.AppendEvent()` (Go CLI)
**Required fields:** Per `schema.go:41-42` — none beyond universal `event`+`timestamp` (both `config_set` and `config_updated` have `{}` required fields)
**Routed event types:** `config_set`, `config_updated` — per `routing.go:61-62`

**Current state:** Empty file (0 entries). Routing and validation infrastructure exists but nothing in the codebase emits these event types. The file is included in `CanonicalFiles()` at `routing.go:77` and would be synced to SurrealDB if events existed.

**Target schema (proposed):**

```json
{"timestamp":"2026-03-04T20:00:00.000Z","event":"config_set","session_id":"ses-01kjxabc123","key":"logging.prompts.enabled","value":true,"previous_value":null,"source":"user"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | `"config_set"` or `"config_updated"` |
| `timestamp` | string (ISO 8601) | Yes | UTC with milliseconds |
| `session_id` | string | Yes | Originating session |
| `key` | string | Yes | Dot-separated config path |
| `value` | any | Yes | New value |
| `previous_value` | any | No | Previous value (null if first set) |
| `source` | string | No | Who changed it (e.g., `user`, `autorun`, `migration`) |

**Parallel concern:** Low currently. Becomes important with parallel sessions for configuration drift detection — two sessions could change the same setting.

**Decision:** Keep. Config change tracking is useful for audit trails and becomes critical with parallel sessions. The routing/validation infrastructure already exists; only the emitter needs to be implemented.

---

## 2. Log Files Audit

### `pathflow-events.jsonl` -- 380 entries

**Location:** `.state/logs/pathflow-events.jsonl` (outside ledger directory — in `.state/logs/`, not `.state/ledger/`)
**Writer:** `pathflow.TransitionWriter` (Go CLI, `internal/pathflow/transitions.go`) wrapping `ledger.Writer.AppendEventToFile()`
**Required fields:** Per `schema.go:45-50` — `session_id`+`phase`+`status` (phase_transition), `session_id`+`stage`+`status` (stage_transition), `session_id` (session_register), `session_id`+`key`+`value` (session_metadata), `session_id`+`task_id`+`task_status` (pathflow_task_update)
**Routed event types:** `phase_transition`, `stage_transition`, `session_register`, `session_metadata`, `pathflow_task_update` — per `routing.go:65-69`
**Note:** NOT included in `CanonicalFiles()` — pathflow events are not synced to SurrealDB.

**Two schema generations coexist:**

**Shell era (deprecated, no longer written):**

```json
{"ts":"2026-02-17T15:41:56Z","e":"begin_work","work_id":"work-1771342801-sandbox","task_id":"INF-TSK-FEAT-GENL-003","session_id":"d67b4d04-...","branch":"feat/sandbox-skill"}
```

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Shell-era timestamp (not `timestamp`) |
| `e` | string | Shell-era event type (not `event`) |
| Other fields | various | Ad-hoc flat fields |

**Go era — phase_transition (via `RecordPhaseTransition` at `transitions.go:195-209`):**

```json
{"event":"phase_transition","phase":"PF3-CLASSIFY","session_id":"ses-01kjvsp7d4vp7bde744yk405tw","status":"completed","timestamp":"2026-03-04T06:48:42Z"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"phase_transition"` |
| `timestamp` | string (RFC 3339) | Yes | Auto-generated by `ledger.Writer` |
| `session_id` | string | Yes | CodeFlow session ID |
| `phase` | string | Yes | One of: PF1-INIT through PF7-END (validated by `validPhases` map) |
| `status` | string | Yes | One of: `entered`, `completed`, `skipped` (validated by `validPhaseStatuses`) |

**Go era — stage_transition (via `RecordStageTransition` at `transitions.go:214-235`):**

```json
{"event":"stage_transition","iteration":1,"session_id":"ses-01kjvsp7d4vp7bde744yk405tw","stage":"WS-REV","status":"complete","timestamp":"2026-03-04T...","verdict":"approved"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"stage_transition"` |
| `timestamp` | string (RFC 3339) | Yes | Auto-generated |
| `session_id` | string | Yes | CodeFlow session ID |
| `stage` | string | Yes | One of: WS-DEV, WS-PLAN, WS-DOCS, WS-TEST, WS-REV, WS-QA |
| `status` | string | Yes | One of: `pending`, `in_progress`, `complete`, `failed` |
| `iteration` | integer | Yes | Must be >= 1 |
| `verdict` | string | No | One of: `pass`, `fail`, `approved`, `changes_requested` (omitted if empty) |

**Go era — session_register (via `RegisterSession` at `transitions.go:240-269`):**

Writes TWO events per call: one `tracking_level` and one `interaction_mode`.

```json
{"event":"session_register","session_id":"ses-01kj...","timestamp":"2026-03-04T...","tracking_level":"pending"}
{"event":"session_register","session_id":"ses-01kj...","timestamp":"2026-03-04T...","interaction_mode":"interactive"}
```

**Go era — session_metadata (via `RecordSessionMetadata` at `transitions.go:291-306`):**

```json
{"event":"session_metadata","key":"work_type","session_id":"ses-01kj...","timestamp":"2026-03-04T...","value":"PLAN"}
```

**Go era — pathflow_task_update (via `RecordTaskUpdate` at `transitions.go:273-288`):**

```json
{"event":"pathflow_task_update","session_id":"ses-01kj...","task_id":"PF3-TSK-01","task_status":"completed","timestamp":"2026-03-04T..."}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `event` | string | Yes | Always `"pathflow_task_update"` |
| `timestamp` | string (RFC 3339) | Yes | Auto-generated |
| `session_id` | string | Yes | CodeFlow session ID |
| `task_id` | string | Yes | Format: `PF[1-7]-TSK-[0-9]{2}` (validated by regex) |
| `task_status` | string | Yes | One of: `pending`, `in_progress`, `completed`, `skipped`, `blocked` |

**Legacy shell events still in the file:** `begin_work`, `complete_work`, `stale_work_cleanup`, `progress` — these are NOT valid pathflow event types per `routing.go`. They were written by the shell-era system before Go CLI took over. The Go `TransitionWriter` never writes these event types.

**Also contains shell-era session_metadata with different schema:**

```json
{"id":"EVT-01KHMW34QT9W05S3XVPDYTXK53","type":"session_metadata","session_id":"d67b4d04-...","key":"work_type","value":"PLAN","ts":"2026-02-17T04:00:19.000Z"}
```

Note: Uses `type` instead of `event`, `ts` instead of `timestamp`, has extra `id` field — completely different schema from Go-era entries.

**Parallel concern:** Medium — events from parallel sessions interleave in a single file. Consumers MUST filter by `session_id` for correct phase ordering. The file is append-only with flock protection, so no data corruption risk.

**Migration notes:** Standardize all entries to Go-era schema (`event`/`timestamp`/`session_id`). Remove shell-era event types from the file. Add `worktree` field. Historical shell-era entries can be preserved as-is (reader handles both), but the writer must enforce Go-era schema only.

### Security Logs (`.state/logs/security/`)

**Writer:** Shell enforcement scripts (`cf-pre-tool-use-*.sh`, `cf-post-tool-use-*.sh` from shell era) and Go CLI (`codeflow hooks pre-tool-use security`, `codeflow hooks pre-tool-use protection-guard`)
**Format:** Pretty-printed multi-line JSON (NOT compact JSONL) for shell-era files; compact JSONL for Go-era files

**CRITICAL FORMAT ISSUE:** Shell-era security logs use pretty-printed multi-line JSON. Each entry spans multiple lines with indentation. Standard JSONL parsers that read line-by-line FAIL on these files. First line is just `{`, second line is `  "ts": "2026-02-03T22:44:45.000Z",`, etc.

**`audit/audit-{date}.jsonl`** -- 27 date files, up to ~6,500 lines/day.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Timestamp (ms precision, `2006-01-02T15:04:05.000Z`) |
| `level` | string | `"info"`, `"warn"`, `"blocked"` |
| `session_id` | string | CodeFlow session ID |
| `event` | string | Event type (e.g., `"pre_tool_use"`, `"post_tool_use"`) |
| `log_type` | string | `"audit"` |
| `tool` | string | Tool name (e.g., `"Bash"`, `"Edit"`, `"Write"`) |
| `target` | string | Target path or command |
| `reason` | string | Description of what was audited/allowed |

**`blocked/blocked-{date}.jsonl`** -- Same pretty-print format. 27 date files.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Timestamp |
| `level` | string | Always `"blocked"` |
| `session_id` | string | CodeFlow session ID |
| `event` | string | Event type |
| `log_type` | string | `"blocked"` |
| `tool` | string | Tool that was blocked |
| `target` | string | Target that triggered the block |
| `reason` | string | Why the operation was blocked |
| `rule` | string | Which rule triggered the block |

**`network/network-{date}.jsonl`** -- Same pretty-print format. 15 date files.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Timestamp |
| `level` | string | `"info"` or `"blocked"` |
| `session_id` | string | CodeFlow session ID |
| `event` | string | `"network_access"` |
| `log_type` | string | `"network"` |
| `tool` | string | Tool name |
| `target` | string | URL or host |
| `domain` | string | Extracted domain |

**`protection/protection-{date}.jsonl`** -- Same pretty-print format. 27 date files.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Timestamp |
| `level` | string | `"info"`, `"warn"`, `"blocked"` |
| `session_id` | string | CodeFlow session ID |
| `event` | string | `"protection_check"` |
| `log_type` | string | `"protection"` |
| `tier` | string | Protection tier (e.g., `"critical"`, `"high"`, `"moderate"`) |
| `domain` | string | Protection domain |
| `module` | string | Protected module/file |
| `tool` | string | Tool that triggered the check |
| `target` | string | Target path |
| `reason` | string | Outcome description |

**`sentinel/sentinel-{date}.jsonl`** -- **DISCONTINUED** after 2026-02-14. Old shell sentinel hooks superseded by Go CLI. 10 files, last entry 2026-02-14. **Remove.**

**`protection-audit.log`** -- **PLAIN TEXT, NOT JSONL.** Written by `cf-protection-common.sh` (AUDIT_LOG variable).

Current format: `[timestamp] USER=X ACTION=Y PATH=Z`

| Field | Type | Notes |
|-------|------|-------|
| `timestamp` | bracketed string | ISO 8601 |
| `USER` | string | Unix username (typically `root` due to sudo) |
| `ACTION` | string | `protect`, `unprotect`, `verify`, `promote` |
| `PATH` | string | Filesystem path |

**Root-owned** (owner=root, group=staff, 0644) because protection scripts run via `sudo`. The PROTECTED files themselves are also root-owned with chmod 000. The log should be converted to JSONL with `session_id` scope. The root ownership is a side-effect of the sudo requirement — the log file itself does not need root ownership; only the protected files do.

**NEEDS for all security logs:**
1. Convert pretty-printed JSON to compact single-line JSONL (shell-era files)
2. Convert `protection-audit.log` from plain text to JSONL format, add `session_id` field
3. Fix root ownership on `protection-audit.log` (chown to user, or write via Go CLI that drops privileges)
4. Remove discontinued `sentinel/` directory
5. Standardize `ts` to `timestamp` across all security log files
6. Add `worktree` field to all security log entries

### Session Logs (`.state/logs/sessions/`)

**Writer:** `logging.ActivityWriter` (Go CLI, `internal/hooks/logging/writer.go`)
**Timestamp format:** `ts` field using `2006-01-02T15:04:05.000Z` (ms precision, literal Z) — per `writer.go:88`
**Session ID resolution:** `ResolveSessionID()` at `writer.go:95-110` — priority: (1) parse `codeflow-env.sh`, (2) `CODEFLOW_SESSION_ID` env, (3) `"unknown"`

**`session-{date}.jsonl`** -- 32 date files.

Written by `LogSessionStart()` at `session.go:23-43` and `LogSessionEnd()` at `session.go:53-69`.

**session_start event:**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | Ms-precision UTC (not `timestamp`) |
| `session_id` | string | Yes | Resolved via `ResolveSessionID()` |
| `event` | string | Yes | `"session_start"` |
| `metadata` | object | No | Present when `CaptureMetadata` config is true |
| `metadata.cwd` | string | No | Project directory |
| `metadata.git_branch` | string | No | Current branch |
| `metadata.git_commit` | string | No | Short HEAD hash |
| `metadata.approval_mode` | string | No | `"standard"` default |
| `metadata.active_task` | string | No | Task ID from `active-task.json` |

**session_end event:**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | Ms-precision UTC |
| `session_id` | string | Yes | Resolved via `ResolveSessionID()` |
| `event` | string | Yes | `"session_end"` |
| `duration_seconds` | integer | Yes | Computed from `.meta` file's `started_epoch` |

**`prompts-{date}.jsonl`** -- 34 date files.

Written by `LogPrompt()` at `prompt.go:18-53`.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | Ms-precision UTC |
| `session_id` | string | Yes | Resolved via `ResolveSessionID()` |
| `event` | string | Yes | `"prompt_submitted"` |
| `prompt_length` | integer | Yes | Character count |
| `prompt_hash` | string | Yes | SHA-256 hex (useful for dedup, not human-readable) |
| `prompt_type` | string | Yes | `"general"` by default. When `DetectIntent` enabled: `command`, `question`, `debugging`, `creation`, `modification`, `review`, `navigation` — per `prompt.go:56-77` |
| `prompt_text` | string | No | Full prompt text (only when `CaptureFullText` true AND `PrivacyMode` false) |

**Parallel enhancement needs:** `agent_role` (which teammate), `task_id` (which task context), `word_count` (quick size indicator), `summary` (truncated first N chars for context).

**`tool-use-{date}.jsonl`** -- 32 date files.

Written by `LogToolUse()` at `tooluse.go:20-64`.

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | Ms-precision UTC |
| `session_id` | string | Yes | Resolved via `ResolveSessionID()` |
| `event` | string | Yes | `"tool_completed"` |
| `tool_name` | string | Yes | Tool name (e.g., `"Bash"`, `"Edit"`, `"Read"`) |
| `tool_input` | string | Yes | Compact JSON of tool input |
| `tool_result` | string | Yes | Compact JSON of tool result (truncated at `MaxResultSize`) |
| `result_truncated` | boolean | Yes | Whether result was truncated |

Sensitive content redacted when `RedactSensit` config is true — patterns: passwords, tokens, keys, secrets, Bearer tokens, long tokens, email addresses — per `tooluse.go:85-93`.

**`stop-events-{date}.jsonl`** -- **DISCONTINUED.** 6 files (2026-02-04 to 2026-02-08 only). Merged into `session-{date}.jsonl`. **Remove.**

**`skills-{date}.jsonl`** -- **DISCONTINUED.** 9 files (2026-02-04 to 2026-02-14). No longer written. **Consider reviving** if teammate name is added — skill usage per agent is useful for optimization.

**`file-changes-{date}.jsonl`** -- **DISCONTINUED.** Single file (2026-02-04 only). **REVIVE for parallel work** — file-to-session attribution is critical for understanding which session modified which files. Add teammate identity (`agent_role`, `agent_name`) to each entry.

**`git-operations-{date}.jsonl`** -- **DISCONTINUED.** Single file (2026-02-04 only). Superseded by `git/commits-*.jsonl`. **Remove.**

**`tasks-{date}.jsonl`** -- **DISCONTINUED.** Single file (2026-02-04 only). **REVIVE** — agent_type, description, result_summary per task is valuable for parallel work debugging and agent performance analysis.

**Session `.meta` files** -- ~300+ files. One per session. Written by `writeSessionMetadata()` in `start.go`. Session-scoped by filename. Contains `started_epoch` (int64, Unix timestamp) used for duration calculation.

### Git Logs (`.state/logs/git/`)

**`commits-{date}.jsonl`** -- 22 date files.

**Writer:** `githooks.RunPostCommit()` (Go CLI, `internal/githooks/postcommit.go:35-97`)
**Struct:** `CommitLogEntry` at `postcommit.go:15-23`

```json
{"ts":"2026-03-04T06:48:42.000Z","event":"commit","hash":"abc123def456...","short_hash":"abc123d","message":"feat: add worktree setup","author":"user","branch":"feat/worktree-setup"}
```

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | Ms-precision UTC — uses `ts` not `timestamp` (Go struct tag: `json:"ts"`) |
| `event` | string | Yes | Always `"commit"` |
| `hash` | string | Yes | Full commit SHA |
| `short_hash` | string | Yes | Short commit SHA |
| `message` | string | Yes | First line of commit message |
| `author` | string | Yes | Commit author name |
| `branch` | string | Yes | Current branch (empty if detached HEAD) |

**Missing fields:** `session_id` — critical gap. Cannot attribute commits to sessions. Also missing `worktree`.

**Written via:** `appendJSONL()` at `postcommit.go:146-163` — direct file I/O, NOT via `ActivityWriter` or `ledger.Writer`. Uses its own `CommitLogEntry` struct with hardcoded `ts` JSON tag. This is why it uses `ts` instead of `timestamp` — it bypasses the standard writer infrastructure.

**`pr-events-{date}.jsonl`** -- 11 date files.

**MOST INCONSISTENT log.** Written by two different sources:

**Source 1 — LLM agent via bash echo:**

```json
{"event_type":"pr_created","pr_number":73,"task_format_id":"INF-TSK-008-003","timestamp":"2026-02-22T..."}
```

Uses `event_type` (not `event`). Written by agents constructing JSON manually and echoing to the file.

**Source 2 — Go CLI hooks:**

```json
{"event":"pr_created","pr_number":134,"session_id":"ses-01kj...","task_format_id":"INF-TSK-008-001","timestamp":"2026-03-04T..."}
```

Uses `event` (canonical). Has `session_id`.

| Field | Type | Notes |
|-------|------|-------|
| `event` OR `event_type` | string | **Inconsistent** — `event` (Go) vs `event_type` (LLM) |
| `timestamp` | string | RFC 3339 UTC |
| `session_id` | string | Present on Go-era entries only |
| `pr_number` | integer | PR number |
| `task_format_id` OR `task_id` | string | **Inconsistent** — some use format_id, others use ULID |
| `branch` | string | Optional |
| `target_branch` | string | Optional |
| `title` | string | Optional |
| `url` | string | Optional |

**Parallel concern:** Medium — PR events from parallel sessions interleave. Session attribution is critical.

---

## 3. Discontinued Logs Assessment

| Log | Status | Action | Rationale |
|-----|--------|--------|-----------|
| `file-changes-*.jsonl` | Discontinued | **REVIVE** | Critical for parallel: file-to-session attribution. Add `agent_role`, `agent_name`, `worktree`. |
| `tasks-*.jsonl` | Discontinued | **REVIVE** | High value: agent_type + description + result_summary per task. Useful for performance analysis. |
| `skills-*.jsonl` | Discontinued | **Consider reviving** | Moderate value if teammate name added. Skill usage per agent aids optimization. |
| `stop-events-*.jsonl` | Discontinued | **Remove** | Low value, already merged into session-*.jsonl. |
| `git-operations-*.jsonl` | Discontinued | **Remove** | Low value, superseded by commits-*.jsonl. |
| `sentinel-*.jsonl` | Discontinued | **Remove** | Superseded by Go CLI sentinel system. |
| `test-category/` | Empty | **Remove** | Never populated. |

---

## 4. Retention Policies

**Current state:** NO retention policies exist for any log category. Files grow unbounded. With parallel sessions increasing write volume proportionally, this becomes unsustainable.

**Proposed policies by category:**

| Category | Policy | Retention | Rationale |
|----------|--------|-----------|-----------|
| Ledger JSONL (Tier 0) | NEVER delete | Archive to `.state/ledger/archive/` after 90 days | Rebuild authority -- loss means data loss. Archive compresses cold data. |
| Security audit/blocked/network/protection logs | Rotate daily (already date-based) | Retain 30 days active. Compress (gzip) after 7 days. | Compliance and debugging. 30 days covers most incident investigation windows. |
| Session logs (session/prompts/tool-use) | Rotate daily (already date-based) | Retain 30 days. | Operational debugging. Older sessions rarely investigated. |
| Git commit logs | Retain 90 days | Delete older. | Git itself retains commit history permanently. |
| PathFlow events | Retain 30 days of entries | Archive older to per-month files. | Phase transitions are useful for recent debugging; historical value decreases. |
| Discontinued files | Delete immediately | N/A | stop-events, git-operations, sentinel, test-category -- all superseded. |
| `protection-audit.log` (plain text) | Convert to JSONL first | Then follow security log policy. | Must convert format before applying standard retention. |

**CLI interface for cleanup:**

```text
codeflow cleanup --retention                    # Apply all retention policies (interactive confirmation)
codeflow cleanup --retention --dry-run          # Show what would be deleted/archived
codeflow cleanup --retention --type ledger      # Only process ledger files
codeflow cleanup --retention --older-than 90d   # Override retention period
codeflow cleanup --retention --event-type session_start  # Filter by event type
codeflow cleanup --retention --compress         # Compress files older than 7 days
codeflow cleanup --retention --archive          # Archive Tier 0 files older than 90 days
```

---

## 5. Design Principles

1. **Field order:** `timestamp` first, then `event`, then `session_id`, then `worktree`, then event-specific fields
2. **Variable content:** Use a `details` object for event-specific data instead of ad-hoc top-level fields
3. **All CRUD through CLI binary:** No direct file manipulation by agents (Rust CLI after Epic 0 cutover)
4. **Worktree always included:** `worktree` is a standard field (empty string for non-worktree sessions)

---

## 6. Canonical Event Schema

All events across ledger and log files conform to:

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "event_name",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  ...event-specific fields (flat, not nested)...
}
```

**Universal required fields:**

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | string (ISO 8601 UTC with milliseconds) | Event time |
| `event` | string | Event type name |
| `session_id` | string | Originating session |
| `worktree` | string | Worktree name (empty for main repo) |

### Deprecated Field Names

| Deprecated | Replacement | Found In |
|-----------|-------------|----------|
| `ts` | `timestamp` | ActivityWriter, CommitLogEntry, session logs, tool-use logs |
| `event_type` | `event` | pr-events (LLM agent writes), some memory-events |
| `e` | `event` | pathflow-events (shell era) |

---

## 7. Target Schemas Per File Type

Each subsection shows: (1) the target schema example, (2) a field table showing what changes from current, and (3) migration notes covering backward compatibility.

### sessions.jsonl

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "session_start",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "user_id": "user@example.com",
  "user_host": "hostname",
  "claude_id": "agent-uuid",
  "interaction_mode": "interactive"
}
```

```json
{
  "timestamp": "2026-03-04T21:00:00.000Z",
  "event": "session_end",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "duration_seconds": 3600,
  "pf7_valid": true,
  "sentinels_cleaned": 5
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Add `worktree` | Missing | Required (empty string if non-worktree) | Old entries without `worktree` treated as main repo |
| Unify `interaction_mode` | Shell-era only, top-level | Always present on session_start | No change needed — field already exists |
| Move ad-hoc fields | `branch`, `pr_pending`, `summary`, `work_completed` top-level | Move to `details` object | Reader handles both flat and `details`-nested |
| Keep Go-era fields | `claude_id`, `user_host`, `user_id` | Keep as top-level | No change |

### work-graph.jsonl

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "task_created",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "id": "task-01kj...",
  "format_id": "INF-TSK-008-001",
  "epic_id": "epic-01kj...",
  "epic_format_id": "INF-EPC-008",
  "title": "Add worktree setup to SessionStart",
  "status": "pending",
  "area_type": "INF",
  "work_type": "FEAT"
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Add `worktree` | Missing | Required (empty string if non-worktree) | Old entries without field treated as main repo |
| Add `session_id` | Often missing | Required on all entries | Old entries without field attributed to unknown |
| Standardize `id`/`format_id` | Mixed: `id`=ULID sometimes, `task_id`=format_id sometimes | `id` always ULID, `format_id` always human-readable | Reader normalizes on read |
| Move ad-hoc fields | `branch`, `summary`, `reason`, `note`, `completed_at` top-level | Move to `details` object | Reader handles both formats |
| Standardize `task_id` on task_status_changed | Sometimes ULID, sometimes format_id, sometimes missing | Rename to `id` (ULID), always include `format_id` | Reader checks both `task_id` and `id` |

### memory-events.jsonl

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "memory_store",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "id": "mem-01kj...",
  "memory_type": "milestone",
  "domain": "infrastructure",
  "content": "Completed worktree integration for parallel sessions",
  "details": {
    "task_format_id": "INF-TSK-008-001",
    "files_affected": ["internal/hooks/session/start.go"]
  }
}
```

**Migration from current (LARGEST CHANGE):**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Eliminate `event_type` | Sub-type field duplicates `event` role | Remove. Use `memory_type` for sub-categorization | Reader maps `event_type` to `memory_type` on read |
| Eliminate `data` wrapper | `data.content`, `data.summary`, etc. | Flatten: `content` top-level, variable fields in `details` | Reader unwraps `data` on read if present |
| Canonical event types only | `memory_event`, `memory_milestone`, `session_summary` | Only: `memory_store`, `milestone`, `progress`, `finding`, `decision`, `blocker` | Reader maps non-canonical types to canonical |
| Standardize `id` format | ULID, timestamp-based, UUID-like, custom | Always ULID (`mem-{ULID}`) | Reader accepts any format on read |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Add `session_id` | Often missing | Required | Old entries attributed to unknown |
| Flatten `data.data` | Doubly-nested stringified JSON | Eliminate — parse and flatten | Reader parses stringified JSON on read |

### pathflow-events.jsonl

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "phase_transition",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "phase": "PF3-CLASSIFY",
  "status": "completed",
  "task_id": "INF-TSK-008-001",
  "work_type": "FEAT"
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Eliminate shell-era schema | `ts`/`e` fields, `type` instead of `event` | All entries use `timestamp`/`event`/`session_id` | Reader handles `ts`→`timestamp`, `e`→`event`, `type`→`event` |
| Remove legacy event types | `begin_work`, `complete_work`, `stale_work_cleanup`, `progress` in pathflow file | These belong in work-graph.jsonl, not pathflow-events | Reader ignores legacy types in pathflow file |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Remove shell-era `id` | EVT-{ULID} prefix on session_metadata entries | Not needed — events identified by session_id + timestamp | Reader ignores extra `id` field |

### session-{date}.jsonl (Activity logs)

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "session_start",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "task_context": {
    "task_format_id": "INF-TSK-008-001",
    "title": "Add worktree setup"
  }
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Rename `ts` to `timestamp` | `ts` (ActivityWriter.Timestamp()) | `timestamp` | Reader accepts both `ts` and `timestamp` |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Keep `metadata` object | Present on session_start when `CaptureMetadata` enabled | Keep — structured metadata is useful | No change |

### prompts-{date}.jsonl (Enhanced)

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "prompt_submitted",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "prompt_length": 245,
  "word_count": 42,
  "prompt_hash": "a1b2c3...",
  "prompt_type": "debugging",
  "agent_role": "cf-development",
  "task_id": "INF-TSK-008-001",
  "summary": "Fix the session start hook to create worktree..."
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Rename `ts` to `timestamp` | `ts` | `timestamp` | Reader accepts both |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Add `word_count` | Missing | New field | Old entries without field = null |
| Add `agent_role` | Missing | New field (critical for parallel) | Old entries attributed to lead |
| Add `task_id` | Missing | New field | Old entries = null |
| Add `summary` | Missing | New field (truncated first N chars) | Old entries = null |

### commits-{date}.jsonl

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "commit",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "hash": "abc123def456...",
  "short_hash": "abc123d",
  "message": "feat: add worktree setup to SessionStart",
  "author": "user@example.com",
  "branch": "feat/worktree-session-start"
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Rename `ts` to `timestamp` | `ts` (CommitLogEntry struct tag) | `timestamp` — requires changing Go struct tag | Reader accepts both |
| Add `session_id` | **Missing** (critical gap) | Required — resolve via `ResolveSessionID()` in `RunPostCommit` | Old entries attributed to unknown |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Use standard writer | `appendJSONL()` direct I/O | Use `ActivityWriter.Append()` for consistency | Internal refactor, no format change |

**Implementation note:** The `CommitLogEntry` struct at `postcommit.go:15-23` has a hardcoded `json:"ts"` tag. Changing to `json:"timestamp"` is the simplest fix. The `RunPostCommit` function at `postcommit.go:35` needs to call `ResolveSessionID()` (import from logging package) and add the session_id to the entry.

### pr-events-{date}.jsonl (Unified)

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "pr_created",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "pr_number": 134,
  "task_format_id": "INF-TSK-008-001",
  "branch": "feat/worktree-session-start",
  "target_branch": "main",
  "title": "feat: add worktree setup to SessionStart"
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Standardize `event` field | `event_type` (LLM) vs `event` (Go) | Always `event` | Reader checks both `event_type` and `event` |
| Add `session_id` | Missing on LLM-written entries | Required | Old entries attributed to unknown |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Standardize `task_format_id` | Some use `task_id` with ULID, some with format_id | Always `task_format_id` with human-readable ID | Reader normalizes field name |
| Single writer | Two sources (LLM echo + Go hooks) | Go CLI only — eliminate LLM direct writes | LLM agents use `codeflow ledger append` instead of `echo >>` |

### file-changes-{date}.jsonl (Revived)

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "file_changed",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "file_path": "internal/hooks/session/start.go",
  "change_type": "modified",
  "agent_role": "cf-development",
  "agent_name": "cf-development",
  "task_id": "INF-TSK-008-001"
}
```

**Migration:** New log — no backward compat needed. Implement as PostToolUse hook for Edit/Write tools. Capture tool input (file path), determine change type, resolve agent identity from session context.

### tasks-{date}.jsonl (Revived)

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "task_executed",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "agent_type": "cf-development",
  "agent_name": "cf-development",
  "task_format_id": "INF-TSK-008-001",
  "description": "Add worktree setup to SessionStart hook",
  "result_summary": "Implemented SetupDetached method and integrated into StartInit",
  "duration_seconds": 120,
  "status": "completed"
}
```

**Migration:** New log — no backward compat needed. Implement as SubagentStop/Stop hook. Capture agent identity, task context, and duration from session metadata.

### security-{date}.jsonl (Consolidated)

Target: merge all security subdirectories (`audit/`, `blocked/`, `network/`, `protection/`) into a single `security-{date}.jsonl` file, distinguished by `log_type` field.

```json
{
  "timestamp": "2026-03-04T20:00:00.000Z",
  "event": "security_check",
  "session_id": "ses-01kjxabc123",
  "worktree": "worktree-ses-01kjxabc123",
  "level": "blocked",
  "log_type": "protection",
  "tool": "Edit",
  "target": ".claude/settings.json",
  "tier": "critical",
  "reason": "File is in critical protection tier"
}
```

**Migration from current:**

| Change | Current | Target | Backward Compat |
|--------|---------|--------|-----------------|
| Compact format | Pretty-printed multi-line JSON | Compact single-line JSONL | New files only — old files preserved as-is |
| Rename `ts` to `timestamp` | `ts` | `timestamp` | Reader accepts both |
| Add `worktree` | Missing | Required | Old entries without field treated as main repo |
| Consolidate directories | 4 subdirs (audit/, blocked/, network/, protection/) | Single file per date | Old subdirectory structure preserved until retention cleanup |
| Convert protection-audit.log | Plain text format | JSONL with `log_type: "protection_audit"` | Old .log file preserved until converted |

---

## 8. DB Table to JSONL Rebuild Mapping

The three-tier data model requires Tier 0 (JSONL) to be the rebuild authority for Tier 1 (SurrealDB). This section maps every legacy database table (Go CLI era) to the JSONL ledger events needed to rebuild it, identifies field-by-field coverage, and flags critical gaps where DB columns have no corresponding JSONL field.

### 8.1 Rebuild Coverage Summary

| Table | Category | JSONL Source | Rebuildable? | Coverage | Gap Count |
|-------|----------|-------------|-------------|----------|-----------|
| `epics` | Core Work Graph | `work-graph.jsonl` | Partial | 8/16 (50%) | 8 missing |
| `tasks` | Core Work Graph | `work-graph.jsonl` | Partial | 9/32 (28%) | 23 missing |
| `sessions` | Session | `sessions.jsonl` | Partial | 8/18 (44%) | 10 missing |
| `active_work` | Active Work | `work-graph.jsonl` | Partial | 7/12 (58%) | 5 missing |
| `memory_events` | Memory | `memory-events.jsonl` | Partial | 5/7 (71%) | 2 inconsistent |
| `work_claims` | Active Work | `sessions.jsonl` | Needs Analysis | ~6/10 (60%) | 4 uncertain |
| `security_logs` | Audit | `security-*.jsonl` | **No active writer** | 7/11 (64%) | 4 missing + no writer |
| `network_logs` | Audit | `security-*.jsonl` | **No active writer** | 5/12 (42%) | 7 missing + no writer |
| `schema_version` | Infra | N/A | Seed data | N/A | N/A |
| `project_config` | Core | `config.jsonl` | No events emitted | 0/8 (0%) | 8 missing |
| `users` | Core | None | Not event-sourced | N/A | N/A |
| `project_members` | Core | None | Not event-sourced | N/A | N/A |
| `area_types` | Reference | None | Seed from migrations | N/A | N/A |
| `work_types` | Reference | None | Seed from migrations | N/A | N/A |
| `domains` | Reference | None | Seed from migrations | N/A | N/A |
| `estimate_types` | Reference | None | Seed from migrations | N/A | N/A |
| `area_folder_mapping` | Reference | None | Seed from migrations | N/A | N/A |
| `task_dependencies` | Work Graph | None | Gap -- no events | 0/3 (0%) | 3 missing |
| `acceptance_criteria` | Work Graph | None | Gap -- no events | 0/6 (0%) | 6 missing |
| `conversation_logs` | Session | None | Not event-sourced | N/A | N/A |
| `worktrees` | Active Work | None | Not event-sourced | N/A | N/A |
| `operation_log` | Infra | None | Not event-sourced | N/A | N/A |
| `recovery_queue` | Infra | None | Not event-sourced | N/A | N/A |
| `entities` | Memory (Derived) | None | Re-derive from `memory_events` | N/A | N/A |
| `relationships` | Memory (Derived) | None | Re-derive from `entities` | N/A | N/A |
| `decisions` | Memory (Derived) | None | Re-derive from `memory_events` | N/A | N/A |
| `memory_categories` | Memory (Derived) | None | Re-derive from `memory_events` | N/A | N/A |
| `extraction_queue` | Memory (Internal) | None | Internal processing queue | N/A | N/A |
| `memory_chunks` | Memory (Derived) | None | Re-derive from `memory_events` | N/A | N/A |
| `chunk_embeddings` | Memory (Derived) | None | Re-derive from `memory_chunks` | N/A | N/A |
| `topics` | Memory (Derived) | None | Re-derive from `active_work` | N/A | N/A |
| `topic_files` | Memory (Derived) | None | Re-derive from `topics` | N/A | N/A |
| `long_term_summaries` | Memory (Derived) | None | Re-derive from `active_work` archival | N/A | N/A |
| `autorun_sessions` | Autorun | None | Own tracking system | N/A | N/A |
| `autorun_workers` | Autorun | None | Own tracking system | N/A | N/A |
| `autorun_task_runs` | Autorun | None | Own tracking system | N/A | N/A |
| `memory_fts` | FTS Virtual | N/A | Rebuild from `memory_events` | N/A | N/A |
| `entities_fts` | FTS Virtual | N/A | Rebuild from `entities` | N/A | N/A |
| `summaries_fts` | FTS Virtual | N/A | Rebuild from `long_term_summaries` | N/A | N/A |

**Overall:** Of the 8 tables that should be rebuildable from JSONL ledgers, none achieve 100% coverage. The `tasks` table is the worst at 28% field coverage. Critical gaps exist in `task_dependencies` and `acceptance_criteria` which have zero JSONL representation. Additionally, `security_logs` and `network_logs` have **no active JSONL writer** -- the legacy shell hooks that wrote `security-*.jsonl` files were retired during the Go CLI migration, and Go's `internal/db/logging.go` uses `slog` (structured logging to stderr), not JSONL files. Similarly, `pr-events-{date}.jsonl` and `db/operations-{date}.jsonl` have no active writer in the current Go codebase.

### 8.2 Rebuildable Tables -- Field-by-Field Mapping

#### `epics` table -- `work-graph.jsonl` (`epic_created`, `epic_status_changed`)

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | `epic_id` or `id` | `epic_created` | ⚠️ inconsistent | JSONL uses `epic_id` in some entries, `id` in others |
| `format_id` | TEXT UNIQUE | `format_id` | `epic_created` | ✅ mapped | |
| `title` | TEXT | `title` | `epic_created` | ✅ mapped | |
| `summary` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `status` | TEXT | `status` / `new_status` | `epic_created` / `epic_status_changed` | ✅ mapped | `epic_created` has `status`; `epic_status_changed` has `new_status` |
| `area_type` | TEXT | `area_type` | `epic_created` | ✅ mapped | |
| `work_type` | TEXT | `work_type` | `epic_created` | ✅ mapped | |
| `domain` | TEXT | `domain` | `epic_created` | ✅ mapped | |
| `is_ongoing` | BOOL | -- | -- | ❌ missing | Not present in any JSONL event |
| `file_scope` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `priority` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `pr_number` | INT | -- | -- | ❌ missing | Not present in any JSONL event |
| `external_id` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `external_url` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `created_at` | TEXT | `timestamp` | `epic_created` | ✅ mapped | Derivable from event timestamp |
| `updated_at` | TEXT | -- | -- | ❌ missing | No explicit update timestamp; derivable from latest event timestamp for this epic |

**Coverage: 8/16 fields mapped (50%). 8 fields missing from JSONL.**

#### `tasks` table -- `work-graph.jsonl` (`task_created`, `task_status_changed`)

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | `id` | `task_created` | ✅ mapped | ULID format `task-{ulid}` |
| `format_id` | TEXT UNIQUE | `format_id` | `task_created` | ✅ mapped | |
| `epic_id` | TEXT FK | `epic_id` | `task_created` | ✅ mapped | Sometimes also has `epic_format_id` |
| `title` | TEXT | `title` | `task_created` | ✅ mapped | |
| `description` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `status` | TEXT | `status` / `new_status` | `task_created` / `task_status_changed` | ✅ mapped | |
| `area_type` | TEXT | `area_type` | `task_created` | ✅ mapped | |
| `work_type` | TEXT | `work_type` | `task_created` | ✅ mapped | |
| `domain` | TEXT | `domain` | `task_created` | ✅ mapped | Sometimes missing; inherited from epic |
| `origin` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `file_scope` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `scope_policy` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `scope_root` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `estimate` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `priority` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `assignee_id` | TEXT FK | -- | -- | ❌ missing | Not present in any JSONL event |
| `autorun_eligible` | BOOL | -- | -- | ❌ missing | Not present in any JSONL event |
| `auto_commit` | BOOL | -- | -- | ❌ missing | DEPRECATED column; still missing from JSONL |
| `raise_pr` | BOOL | -- | -- | ❌ missing | Not present in any JSONL event |
| `auto_merge` | BOOL | -- | -- | ❌ missing | Not present in any JSONL event |
| `target_branch` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `acceptance` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `tests` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `branch` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `pr_number` | INT | `pr_number` | `pr_created` | ✅ mapped | Via separate `pr_created` event with `task_format_id` |
| `external_id` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `external_url` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `created_at` | TEXT | `timestamp` | `task_created` | ✅ mapped | Derivable from event timestamp |
| `updated_at` | TEXT | -- | -- | ❌ missing | Derivable from latest event timestamp |
| `started_at` | TEXT | -- | -- | ❌ missing | Not present; derivable from first `in_progress` status change |
| `completed_at` | TEXT | -- | -- | ❌ missing | Not present; derivable from `complete` status change timestamp |
| `stage` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `stage_status` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `stage_history` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |

**Coverage: 9/32 fields mapped (28%). 23 fields missing from JSONL.** This is the most critical gap -- the tasks table contains the bulk of work item metadata, and most of it is not captured in any JSONL event.

#### `sessions` table -- `sessions.jsonl` (`session_start`, `session_end`)

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | `session_id` | `session_start` | ✅ mapped | |
| `project_id` | TEXT FK | -- | -- | ❌ missing | Not present in any JSONL event |
| `user_id` | TEXT FK | `user_id` | `session_start` (Go era) | ✅ mapped | Only in Go-era events; shell-era events lack this |
| `user_host` | TEXT | `user_host` | `session_start` (Go era) | ✅ mapped | Only in Go-era events |
| `machine_fingerprint` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `started_at` | TEXT | `timestamp` | `session_start` | ✅ mapped | Derivable from event timestamp |
| `ended_at` | TEXT | `timestamp` | `session_end` | ✅ mapped | Derivable from session_end event timestamp |
| `duration_seconds` | INT | -- | -- | ❌ missing | Derivable by computing `session_end.timestamp - session_start.timestamp` |
| `status` | TEXT | -- | -- | ❌ missing | Not present explicitly; partially derivable (start=active, end=completed) |
| `work_ids` | TEXT (JSON) | `work_completed` | `session_end` (shell era) | ⚠️ inconsistent | Only in shell-era session_end; Go-era lacks this |
| `previous_session_id` | TEXT FK | -- | -- | ❌ missing | Not present in any JSONL event |
| `context_summary` | TEXT | `summary` | `session_end` (shell era) | ⚠️ inconsistent | Only in shell-era session_end |
| `tool_stats` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `metadata` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `pathflow_mode` | TEXT | -- | -- | ❌ missing | V4 column; not in any JSONL event |
| `tracking_level` | TEXT | -- | -- | ❌ missing | V4 column; not in any JSONL event |
| `work_item_id` | TEXT | -- | -- | ❌ missing | V4 column; not in any JSONL event |
| `interaction_mode` | TEXT | `interaction_mode` | `session_start` (shell era) | ⚠️ inconsistent | Only in shell-era events |

**Coverage: 8/18 fields mapped (44%). 10 fields missing or inconsistent.** The V4 PathFlow columns (`pathflow_mode`, `tracking_level`, `work_item_id`) have no JSONL representation at all.

#### `active_work` table -- `work-graph.jsonl` (`begin_work`, `complete_work`)

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | `work_id` | `begin_work` | ⚠️ inconsistent | JSONL uses `work_id`, DB uses `id` |
| `task_id` | TEXT FK | `task_id` | `begin_work` | ✅ mapped | |
| `topic` | TEXT | `topic` | `begin_work` | ✅ mapped | |
| `status` | TEXT | -- | -- | ❌ missing | Derivable: `begin_work` = `in_progress`, `complete_work` = `complete` |
| `branch` | TEXT | `branch` | `begin_work` | ✅ mapped | |
| `scope` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `deliverables` | TEXT (JSON) | -- | -- | ❌ missing | Not present in any JSONL event |
| `agent` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `session_id` | TEXT | `session_id` | `begin_work` | ✅ mapped | |
| `current_stage` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `team_name` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event (removed from flag in PR #133) |
| `created_at` | TEXT | `timestamp` | `begin_work` | ✅ mapped | Derivable from event timestamp |
| `updated_at` | TEXT | -- | -- | ❌ missing | Derivable from latest event timestamp |

**Coverage: 7/13 fields mapped (54%). 6 fields missing from JSONL.** The `work_id` vs `id` naming inconsistency is a known issue from Section 9.

#### `memory_events` table -- `memory-events.jsonl`

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | `id` | All events | ✅ mapped | Various format inconsistencies (ULID, timestamp-based, UUID-like) |
| `event_type` | TEXT | `event_type` | All events | ⚠️ inconsistent | JSONL sometimes uses `event` as the sub-type instead of `event_type` |
| `domain` | TEXT | `domain` | All events | ✅ mapped | |
| `work_id` | TEXT | `work_id` | All events | ✅ mapped | |
| `data` | TEXT (JSON) | `data` | All events | ⚠️ inconsistent | JSONL has `data` as a nested object; DB stores as JSON TEXT. Some JSONL entries have doubly-nested stringified JSON inside `data.data` |
| `memory_type` | TEXT | `memory_type` | Some events | ⚠️ inconsistent | Not always present in JSONL; when present, sometimes conflicts with `event_type` semantics |
| `created_at` | TEXT | `timestamp` | All events | ✅ mapped | Derivable from event timestamp |

**Coverage: 5/7 fields mapped (71%). 2 fields have significant inconsistencies.** The `data` field nesting and `event_type` vs `memory_type` confusion are the primary issues documented in Section 7 (memory-events.jsonl target schema).

#### `work_claims` table -- `sessions.jsonl` (`claim_created`, `claim_released`, `claim_renewed`)

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | `id` | `claim_created` | ✅ mapped | Per `schema.go:14`, `id` is required for `claim_created` |
| `work_id` | TEXT FK | `work_id` | `claim_created` | ✅ mapped | Expected in event payload |
| `pattern` | TEXT | `pattern` | `claim_created` | ✅ mapped | Expected in event payload |
| `mode` | TEXT | `mode` | `claim_created` | ✅ mapped | Expected in event payload |
| `owner_id` | TEXT FK | `owner_id` | `claim_created` | ⚠️ uncertain | May use user email string rather than ULID FK |
| `owner_host` | TEXT | `owner_host` | `claim_created` | ⚠️ uncertain | May not be included in JSONL events |
| `fencing_token` | INT | `fencing_token` | `claim_created` | ✅ mapped | Required for correctness |
| `expires_at` | TEXT | `expires_at` | `claim_created` | ✅ mapped | Expected in event payload |
| `status` | TEXT | -- | -- | ❌ missing | Derivable: `claim_created` = active, `claim_released` = released |
| `created_at` | TEXT | `timestamp` | `claim_created` | ✅ mapped | Derivable from event timestamp |

**Coverage: ~7/10 fields mapped (70%). 1 missing, 2 uncertain.** The claim events are defined in `schema.go:14-18` with required fields `id` (for claim_created) and `claim_id` (for claim_released, claim_renewed). Full field inventory of the actual events requires runtime capture since no claim events exist in the current ledger data.

#### `security_logs` table -- `security-*.jsonl` (audit, blocked, protection subdirs)

**Writer status: NO ACTIVE WRITER.** The shell hooks that wrote these JSONL files (`.state/logs/security/audit/*.jsonl`, `blocked/*.jsonl`, `protection/*.jsonl`) were retired during the Go CLI migration. The Go replacement (`internal/db/logging.go`) uses `slog` structured logging to stderr, not JSONL files. Existing JSONL files are historical artifacts only -- no new entries are being appended. The field mapping below documents the legacy format for reference.

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | -- | -- | ❌ missing | JSONL entries have no `id` field |
| `log_type` | TEXT | `log_type` | All events | ✅ mapped | Values: `protection`, `blocked`, `sentinel` |
| `event_type` | TEXT | `event` | All events | ⚠️ inconsistent | DB uses `event_type` with values like `PROTECT`, `BLOCKED`; JSONL uses `event` with values like `path_protected`, `tool_blocked` |
| `tool` | TEXT | `tool` | All events | ✅ mapped | |
| `target` | TEXT | `target` | All events | ✅ mapped | |
| `reason` | TEXT | `reason` | All events | ✅ mapped | |
| `skill` | TEXT | -- | -- | ❌ missing | Only present in sentinel-type logs, not in standard security logs |
| `operation` | TEXT | -- | -- | ❌ missing | Only present in sentinel-type logs |
| `outcome` | TEXT | `outcome` | Some events | ⚠️ inconsistent | JSONL has `outcome` (e.g., `blocked`); also has `level` (e.g., `WARN`) which DB lacks |
| `session_id` | TEXT FK | `session_id` | All events | ✅ mapped | |
| `created_at` | TEXT | `ts` / `timestamp` | All events | ⚠️ inconsistent | JSONL uses `ts`; target schema uses `timestamp` |

**Additional JSONL fields not in DB:** `level` (WARN/INFO/ERROR), `tier` (critical/high/moderate) -- these are present in JSONL but have no DB column.

**Coverage: 7/11 fields mapped (64%). 4 fields missing or inconsistent.**

#### `network_logs` table -- `security-*.jsonl` (network subdirectory)

**Writer status: NO ACTIVE WRITER.** Same as `security_logs` above -- the shell hooks that wrote network JSONL files (`.state/logs/security/network/*.jsonl`) were retired during the Go CLI migration. No new network log JSONL entries are being created. The field mapping below documents the legacy format.

| DB Column | Type | JSONL Field | JSONL Event | Status | Notes |
|-----------|------|-------------|-------------|--------|-------|
| `id` | TEXT PK | -- | -- | ❌ missing | JSONL entries have no `id` field |
| `operation` | TEXT | `tool` | All events | ⚠️ inconsistent | DB uses `operation` (WebFetch, curl); JSONL uses `tool` |
| `url` | TEXT | `target` | All events | ⚠️ inconsistent | DB uses `url`; JSONL uses `target` |
| `domain` | TEXT | `domain` | All events | ✅ mapped | |
| `method` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `status_code` | INT | -- | -- | ❌ missing | Not present in any JSONL event |
| `response_size` | INT | -- | -- | ❌ missing | Not present in any JSONL event |
| `duration_ms` | INT | -- | -- | ❌ missing | Not present in any JSONL event |
| `purpose` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `work_id` | TEXT | -- | -- | ❌ missing | Not present in any JSONL event |
| `session_id` | TEXT FK | `session_id` | All events | ✅ mapped | |
| `created_at` | TEXT | `ts` / `timestamp` | All events | ⚠️ inconsistent | JSONL uses `ts`; target schema uses `timestamp` |

**Additional JSONL fields not in DB:** `level`, `log_type`, `event` (event type name), `reason`, `allowlist` -- present in JSONL but no DB column.

**Coverage: 5/12 fields mapped (42%). 7 fields missing from JSONL.** The network logs JSONL captures only the security-gate decision (allowed/blocked), not the actual HTTP request/response metadata that the DB schema expects.

### 8.3 Tables Without JSONL Sources -- Classification

#### (a) Reference Data -- Seed from Migrations

These tables contain static configuration seeded by `schema.sql` INSERT statements. They are not event-sourced and do not need JSONL representation. Rebuild by re-running the schema.

| Table | Columns | Seed Source |
|-------|---------|-------------|
| `schema_version` | `version`, `applied_at` | `INSERT OR IGNORE INTO schema_version (version) VALUES (2)` |
| `area_types` | `code`, `name`, `description`, `scope_patterns`, `is_active` | 6 rows seeded in schema.sql |
| `work_types` | `code`, `name`, `branch_prefix`, `commit_type`, `urgency`, `default_scope_policy`, `is_active` | 10 rows seeded in schema.sql |
| `domains` | `code`, `name`, `description`, `is_reserved`, `is_active` | 3 rows seeded in schema.sql |
| `estimate_types` | `code`, `name`, `description`, `sort_order`, `is_active` | 5 rows seeded (XS through XL) |
| `area_folder_mapping` | `area_type`, `folder_name`, `display_name` | 6 rows seeded in schema.sql |

#### (b) Derived Data -- Re-derive from Source Tables

These tables contain data derived from other tables via processing pipelines. They can be regenerated from their source data without JSONL events.

| Table | Derived From | Rebuild Strategy |
|-------|-------------|-----------------|
| `entities` | `memory_events` | Re-run entity extraction pipeline on `memory_events.data` |
| `relationships` | `entities` | Re-run relationship extraction on `entities` pairs |
| `decisions` | `memory_events` | Re-extract decisions from events with `event_type='decision'` |
| `memory_categories` | `memory_events` | Re-categorize each event via classification pipeline |
| `extraction_queue` | `memory_events` | Re-populate pending extractions; internal processing queue |
| `memory_chunks` | `memory_events` | Re-chunk event data for embedding |
| `chunk_embeddings` | `memory_chunks` | Re-embed chunks via model; requires `all-MiniLM-L6-v2` |
| `topics` | `active_work` | Re-derive topic slugs and folder paths from active work records |
| `topic_files` | `topics` | Re-scan filesystem for topic files |
| `long_term_summaries` | `active_work` (archival) | Re-generate summaries from archived work; may lose quality without original context |

**FTS Virtual Tables** (rebuilt automatically from source tables):

| Table | Source Table | Rebuild |
|-------|-------------|---------|
| `memory_fts` | `memory_events` | DROP + re-CREATE + re-INSERT from `memory_events` |
| `entities_fts` | `entities` | DROP + re-CREATE + re-INSERT from `entities` |
| `summaries_fts` | `long_term_summaries` | DROP + re-CREATE + re-INSERT from `long_term_summaries` |

#### (c) Not Event-Sourced -- Populated by Runtime Code

These tables are populated by application code during runtime operations. They are not intended to be rebuilt from JSONL and do not need event sources.

| Table | Populated By | Rebuild Strategy |
|-------|-------------|-----------------|
| `users` | User resolution during session_start (Go CLI) | Re-derive from git config on next session; historical users lost |
| `project_config` | Project initialization | Re-initialize from `PROJECT.md` and git remote |
| `project_members` | Project + user association | Re-derive from users + project on next session |
| `conversation_logs` | Session hooks (conversation capture) | Not rebuildable; ephemeral session data |
| `worktrees` | Worktree manager at session start | Re-scan filesystem for `.claude/worktrees/` |
| `operation_log` | Operation executor (idempotency tracking) | Not rebuildable; historical idempotency keys lost but functionally harmless |
| `recovery_queue` | Recovery system (failure queue) | Not rebuildable; transient queue, empty at steady state |

#### (d) Autorun Subsystem -- Own Tracking

These tables are part of the autorun subsystem and have their own lifecycle management. They are not part of the standard JSONL rebuild path.

| Table | Purpose | Rebuild Strategy |
|-------|---------|-----------------|
| `autorun_sessions` | Batch run tracking | Not rebuildable from JSONL; autorun has own state management |
| `autorun_workers` | Worker assignment per batch | Recreated each batch run |
| `autorun_task_runs` | Individual task execution within batch | Recreated each batch run |

#### (e) Gap -- Requires New JSONL Events

These tables should be rebuildable from JSONL but currently have NO event representation. New event types are needed.

| Table | Required New Event | Suggested JSONL File | Fields Needed |
|-------|-------------------|---------------------|---------------|
| `task_dependencies` | `task_dependency_created` | `work-graph.jsonl` | `task_id`, `depends_on_id`, `dependency_type` |
| `acceptance_criteria` | `acceptance_criterion_created`, `acceptance_criterion_met` | `work-graph.jsonl` | `id`, `epic_id`, `criterion`, `met`, `met_at`, `met_by` |
| `project_config` | `project_initialized` | `config.jsonl` | `id`, `name`, `description`, `git_remote_url`, `default_branch` |

### 8.4 Critical Gaps Analysis

**Priority 0 -- JSONL writers completely missing (no data being captured):**

Several DB tables have JSONL sources in theory but **no active Go writer** is producing those JSONL files. The legacy shell hooks that wrote them were retired during the Go CLI migration (INF-EPC-021). This is more severe than missing fields -- no data is being captured at all for new sessions.

| Affected Table | Legacy JSONL Source | Writer Status | Impact |
|---------------|-------------------|---------------|--------|
| `security_logs` | `.state/logs/security/{audit,blocked,protection}/*.jsonl` | **Dead** -- retired shell hooks | No security audit trail in JSONL for sessions after Go migration |
| `network_logs` | `.state/logs/security/network/*.jsonl` | **Dead** -- retired shell hooks | No network request log in JSONL for sessions after Go migration |
| (no DB table) | `pr-events-{date}.jsonl` | **Dead** -- orphaned from retired shell hooks | PR events not captured in JSONL; `pr_created`/`pr_merged` events in `work-graph.jsonl` partially compensate |
| (no DB table) | `db/operations-{date}.jsonl` | **Dead** -- legacy artifact, last entry 2026-02-09 | DB operation logging moved to `slog` via `internal/db/logging.go` |

**Fix:** For `security_logs` and `network_logs`, implement new Go JSONL writers in the PreToolUse hook handlers that currently only write to the DB. For `pr-events`, evaluate whether `work-graph.jsonl` `pr_created`/`pr_merged` events provide sufficient coverage or whether a dedicated log is still needed. For `db/operations`, no fix needed -- `slog` is the intended replacement.

**Priority 1 -- Core work graph fields missing from JSONL:**

The `tasks` table has 23 missing fields. Many of these are essential metadata set at task creation time that cannot be derived from later events:

| Missing Field | Impact | Fix |
|--------------|--------|-----|
| `description` | Cannot rebuild task descriptions | Add to `task_created` event |
| `origin` | Cannot distinguish planned vs adhoc | Add to `task_created` event |
| `estimate` | Cannot rebuild effort estimates | Add to `task_created` event |
| `acceptance` | Cannot rebuild acceptance criteria list | Add to `task_created` event |
| `file_scope` | Cannot rebuild file scope restrictions | Add to `task_created` event |
| `scope_policy` | Cannot rebuild scope enforcement | Add to `task_created` event |
| `autorun_eligible` | Cannot rebuild autorun eligibility | Add to `task_created` event |
| `auto_merge` | Cannot rebuild merge policy | Add to `task_created` event |
| `target_branch` | Cannot rebuild branch targets | Add to `task_created` event |
| `stage` / `stage_status` / `stage_history` | Cannot rebuild pipeline stage tracking | Add new `task_stage_changed` event or include in `task_status_changed` |

**Priority 2 -- Session and work lifecycle gaps:**

| Missing Area | Impact | Fix |
|-------------|--------|-----|
| V4 PathFlow columns on sessions (`pathflow_mode`, `tracking_level`, `work_item_id`) | Cannot rebuild PathFlow session state | Add to `session_start` or new `session_mode_set` event |
| `active_work.scope`, `active_work.deliverables`, `active_work.agent`, `active_work.current_stage` | Cannot fully rebuild active work records | Add to `begin_work` event |
| `task_dependencies` (entire table) | Cannot rebuild dependency graph | Add `task_dependency_created` event |
| `acceptance_criteria` (entire table) | Cannot rebuild epic acceptance criteria | Add `acceptance_criterion_created` event |

**Priority 3 -- Audit log enrichment:**

| Missing Area | Impact | Fix |
|-------------|--------|-----|
| `security_logs.id` | Cannot deduplicate security events | Add `id` field to security log entries |
| `network_logs` HTTP metadata (method, status_code, response_size, duration_ms) | Cannot rebuild request/response metadata | Enrich network log events at write time |
| `security_logs.skill`, `security_logs.operation` | Cannot rebuild sentinel audit trail | Include in sentinel-type security events |
| `network_logs.purpose`, `network_logs.work_id` | Cannot attribute network requests to work items | Add work context to network log events |

**Priority 4 -- Naming inconsistencies (not gaps, but rebuild complexity):**

| Inconsistency | Tables Affected | Resolution |
|--------------|----------------|-----------|
| `id` vs `work_id` vs `epic_id` | `active_work`, `epics` | Standardize: JSONL uses `id` for ULID PK; add `format_id` alongside |
| `url` vs `target` | `network_logs` | Standardize: use `url` in both DB and JSONL |
| `operation` vs `tool` | `network_logs` | Standardize: use `tool` in both DB and JSONL |
| `event_type` vs `event` | `security_logs` | Standardize: use `event` in JSONL, map to `event_type` on rebuild |
| `ts` vs `timestamp` | `security_logs`, `network_logs` | Standardize: use `timestamp` everywhere (target schema from Section 7) |

---

## 9. Schema Inconsistencies Summary

| Issue | Where | Impact | Resolution |
|-------|-------|--------|-----------|
| `ts` vs `timestamp` | ActivityWriter/git hooks use `ts`; ledger.Writer uses `timestamp` | Consumers must handle both | **`timestamp`** -- ISO 8601 UTC with milliseconds. Update ActivityWriter and CommitLogEntry. |
| `event` vs `event_type` vs `e` | pr-events uses both; pathflow shell uses `e`; everything else uses `event` | Consumers must handle all three | **`event`** -- matches ledger canonical format. |
| Pretty-printed vs compact | Security logs: multi-line; everything else: single-line | JSONL parsers fail on security logs | **Compact single-line** -- this is the JSONL specification. |
| Nested `data` vs flat | memory-events has both patterns | Inconsistent parsing | **Flat fields** for standard metadata; `details` object for variable event-specific payload. |
| ULID vs format_id | work-graph has both coexisting | Ambiguous primary key | **format_id** as primary reference in logs; ULID as internal DB key only. |
| Missing session_id | commits-*.jsonl has no session_id | Cannot attribute commits to sessions | **Add session_id** to CommitLogEntry. |
| Missing worktree | No file has worktree field | Cannot attribute events to worktrees | **Add worktree** field to all schemas. |

---

[← Back to Overview](README.md)
