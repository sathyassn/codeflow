---
title: "Conversation Logs Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-04"
updated_at: "2026-04-04"
scope: INF-TSK-024-006
feeds_into:
  - INF-TSK-024-007
---

# Conversation Logs Schema Audit

This document audits the schema of conversation and interaction logs written to
`.state/logs/sessions/`. These logs capture session lifecycle, user prompts, tool use
activity, and session metadata. Writers are implemented in the Rust `ActivityWriter`
(`codeflow-cli/core/src/hooks/logging/writer.rs`). It feeds into INF-TSK-024-007
(canonical event schema synthesis).

## Table of Contents

- [1. Directory Layout](#1-directory-layout)
- [2. ActivityWriter Mechanics](#2-activitywriter-mechanics)
- [3. session-{date}.jsonl — Session Lifecycle Events](#3-session-datedatejsonl--session-lifecycle-events)
- [4. prompts-{date}.jsonl — User Prompt Events](#4-prompts-datedatejsonl--user-prompt-events)
- [5. tool-use-{date}.jsonl — Tool Use Events](#5-tool-use-datedatejsonl--tool-use-events)
- [6. session-{session_id}.meta — Session Metadata Files](#6-session-session_idmeta--session-metadata-files)
- [7. Discontinued Conversation-Adjacent Logs](#7-discontinued-conversation-adjacent-logs)
- [8. Field Naming Inconsistencies](#8-field-naming-inconsistencies)
- [9. Configuration Reference](#9-configuration-reference)

---

## 1. Directory Layout

All conversation logs write to `.state/logs/sessions/`. As of 2026-04-04, the directory
contains:

| File pattern | Count | Date range | Status |
|-------------|-------|-----------|--------|
| `session-{date}.jsonl` | 60 | 2026-02-03 to 2026-04-04 | Active |
| `prompts-{date}.jsonl` | 94 | 2026-02-04 to 2026-04-04 | Active (capture broken) |
| `tool-use-{date}.jsonl` | 92 | 2026-02-04 to 2026-04-04 | Active |
| `cleanup-{date}.jsonl` | 21 | 2026-03-14 to 2026-04-04 | Active (separate writer) |
| `session-{session_id}.meta` | 919 | 2026-02-11 to 2026-04-04 | Active (shell-era writer) |
| `skills-{date}.jsonl` | 9 | 2026-02-04 to 2026-02-14 | Discontinued |
| `stop-events-{date}.jsonl` | 6 | 2026-02-04 to 2026-02-08 | Discontinued |
| `file-changes-{date}.jsonl` | 1 | 2026-02-04 | Discontinued |
| `git-operations-{date}.jsonl` | 1 | 2026-02-04 | Discontinued |
| `tasks-{date}.jsonl` | 1 | 2026-02-04 | Discontinued |

The `cleanup-{date}.jsonl` files use a standalone writer in `session_end.rs`, not
`ActivityWriter` — see the Operational Logs Audit for their schema.

---

## 2. ActivityWriter Mechanics

**Source:** `codeflow-cli/core/src/hooks/logging/writer.rs`

`ActivityWriter` is the common writer for session, prompt, and tool-use logs. Key
behaviors:

| Property | Value |
|----------|-------|
| Timestamp field | `ts` (format: `YYYY-MM-DDTHH:MM:SS.sssZ`, ms precision, literal Z) |
| Rotation | Date-based: `{log_type}-{YYYY-MM-DD}.jsonl` |
| Locking | `fs2::FileExt::lock_exclusive` with `.lock` side-files |
| Directory | Configurable via `enforcement-policy.json` `logging.*_directory` |
| Session ID | Resolved via `session::current_session_id()` reading `codeflow-env.sh` |

The `ActivityWriter::timestamp()` method produces the `ts` field:

```rust
pub fn timestamp(&self) -> String {
    (self.now)().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
```

The `ts` field name differs from the `timestamp` field used in `sessions.jsonl` ledger
events and in `cleanup-{date}.jsonl`. See Section 8 for the field naming inconsistency
catalog.

---

## 3. session-{date}.jsonl — Session Lifecycle Events

**Writer:** Rust `ActivityWriter` via three handlers in
`codeflow-cli/core/src/hooks/logging/session.rs` and
`codeflow-cli/core/src/hooks/logging/stop.rs`

Three distinct event types multiplex into the same `session-{date}.jsonl` file.

### 3.1 session_start

**Handler:** `SessionStartLogging` (fires on `HookEvent::SessionStart`)

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` — ms precision, literal Z |
| `session_id` | string | Yes | Resolved from `codeflow-env.sh` |
| `event` | string | Yes | Always `"session_start"` |
| `metadata` | object | Conditional | Present when `capture_metadata: true` |
| `metadata.cwd` | string | No | Absolute path of project directory |
| `metadata.git_branch` | string | No | Current git branch (absent when not in git repo) |
| `metadata.git_commit` | string | No | Short HEAD commit hash |
| `metadata.approval_mode` | string | No | From `permission_mode` in hook input; defaults `"standard"` |
| `metadata.active_task` | string | No | Task ID from `active-task.json` (absent when none) |

**Example:**

```json
{"ts":"2026-04-04T10:23:15.042Z","session_id":"ses-01kna7...","event":"session_start","metadata":{"cwd":"/Volumes/DATA/Local/software-workspace/projects/codeflow","git_branch":"docs/log-audit-schemas","git_commit":"5b6fcab","approval_mode":"default"}}
```

### 3.2 session_end

**Handler:** `SessionEndLogging` (fires on `HookEvent::SessionEnd`)

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` |
| `session_id` | string | Yes | Resolved from `codeflow-env.sh` |
| `event` | string | Yes | Always `"session_end"` |
| `duration_seconds` | integer | Yes | Calculated from `session-{session_id}.meta`; 0 if meta absent |

Duration calculation reads `started_epoch` (int64) from the `.meta` file and subtracts
from current Unix timestamp. See Section 6 for `.meta` file format.

**Example:**

```json
{"ts":"2026-04-04T11:45:22.314Z","session_id":"ses-01kna7...","event":"session_end","duration_seconds":4927}
```

### 3.3 stop

**Handler:** `StopLogging` (fires on `HookEvent::Stop` — covers both `Stop` and
`SubagentStop` events)

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` |
| `session_id` | string | Yes | Resolved from `codeflow-env.sh` |
| `event` | string | Yes | Always `"stop"` |
| `stop_reason` | string | Yes | From hook input `stop_reason`; defaults `"unknown"` |
| `git_branch` | string | No | Current git branch (absent when not in git repo) |
| `has_uncommitted_changes` | boolean | Yes | From `git status --porcelain` |
| `task_context` | object | Conditional | Present when `capture_task_context: true` and active task exists |
| `task_context.task_id` | string | No | Task ID from `active-task.json` |
| `task_context.status` | string | No | Task status from `active-task.json` |

**Example:**

```json
{"ts":"2026-04-04T11:45:22.100Z","session_id":"ses-01kna7...","event":"stop","stop_reason":"end_turn","git_branch":"docs/log-audit-schemas","has_uncommitted_changes":false,"task_context":{"task_id":"INF-TSK-024-006","status":"in_progress"}}
```

---

## 4. prompts-{date}.jsonl — User Prompt Events

**Writer:** Rust `ActivityWriter` via `PromptLogging` handler in
`codeflow-cli/core/src/hooks/logging/prompt.rs`

**Fires on:** `HookEvent::UserPromptSubmit`

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` |
| `session_id` | string | Yes | Resolved from `codeflow-env.sh` |
| `event` | string | Yes | Always `"prompt_submitted"` |
| `prompt_length` | integer | Yes | Length of `user_prompt` in bytes |
| `prompt_hash` | string | Yes | SHA-256 hex digest of prompt; empty string when prompt empty |
| `prompt_type` | string | Yes | Intent classification (see below) |
| `prompt_text` | string | Conditional | Full prompt text; only present when `capture_full_text: true` AND `privacy_mode: false` AND prompt non-empty |

**Intent classification values** (from `detect_prompt_type()` in `prompt.rs`):

| Value | Detection rule |
|-------|----------------|
| `command` | Prompt starts with `/` |
| `question` | Prompt contains `?` |
| `debugging` | Contains: `fix`, `bug`, `error`, `issue` |
| `creation` | Contains: `create`, `add`, `implement`, `build` |
| `modification` | Contains: `update`, `change`, `modify`, `edit` |
| `review` | Contains: `review`, `check`, `verify`, `test` |
| `navigation` | Contains: `find`, `search`, `locate`, `where` |
| `general` | Default — none of the above matched |

**KNOWN ISSUE:** In current data, `prompt_length` is 0 and `prompt_hash` is `""` in all
observed entries. The `user_prompt` field in the Claude Code hook input may be empty or
absent when `UserPromptSubmit` fires. The capture handler writes the record regardless,
resulting in zero-length, zero-hash entries. This is a data capture defect, not a schema
defect — the fields are correct; the source data is empty.

**Example (current observed state):**

```json
{"ts":"2026-04-04T10:23:16.001Z","session_id":"ses-01kna7...","event":"prompt_submitted","prompt_length":0,"prompt_hash":"","prompt_type":"general"}
```

**Example (expected when capture is functional):**

```json
{"ts":"2026-04-04T10:23:16.001Z","session_id":"ses-01kna7...","event":"prompt_submitted","prompt_length":47,"prompt_hash":"a1b2c3...","prompt_type":"question"}
```

---

## 5. tool-use-{date}.jsonl — Tool Use Events

**Writer:** Rust `ActivityWriter` via `ToolUseLogging` handler in
`codeflow-cli/core/src/hooks/logging/tooluse.rs`

**Fires on:** `HookEvent::PostToolUse`

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string | Yes | `YYYY-MM-DDTHH:MM:SS.sssZ` |
| `session_id` | string | Yes | Resolved from `codeflow-env.sh` |
| `event` | string | Yes | Always `"tool_completed"` |
| `tool_name` | string | Yes | Tool that was used (e.g., `"Bash"`, `"Edit"`) |
| `tool_input` | string | Yes | Summarized tool input; sensitive patterns redacted |
| `tool_result` | string | Yes | Summarized tool result; truncated to `max_result_size`; sensitive patterns redacted |
| `result_truncated` | boolean | Yes | `true` if `tool_result` was truncated |

**Tool filter:** Only tools in `tools_to_log` are logged. Current config:
`["Edit", "Write", "Bash", "Read", "Grep"]`. Events for other tools are silently
skipped.

**Redaction patterns** (applied to both `tool_input` and `tool_result`):

| Pattern | Replacement |
|---------|-------------|
| `password: <value>` (case-insensitive) | `password: [REDACTED]` |
| `token: <value>` (case-insensitive) | `token: [REDACTED]` |
| `key: <value>` (case-insensitive) | `key: [REDACTED]` |
| `secret: <value>` (case-insensitive) | `secret: [REDACTED]` |
| `Bearer <token>` | `Bearer [REDACTED]` |
| 40+ character alphanumeric strings | `[LONG_TOKEN_REDACTED]` |
| Email addresses | `[EMAIL_REDACTED]` |

**Truncation:** `tool_result` is truncated to `max_result_size` bytes (current config:
2000) with `...[truncated]` appended. `result_truncated` is `true` when truncation
occurred.

**Example:**

```json
{"ts":"2026-04-04T10:24:01.772Z","session_id":"ses-01kna7...","event":"tool_completed","tool_name":"Bash","tool_input":"{\"command\":\"ls .state/logs/\"}","tool_result":"db\ngit\nsessions\n","result_truncated":false}
```

---

## 6. session-{session_id}.meta — Session Metadata Files

**Writer:** Shell-era session start hook. No active Rust writer identified.

**Count:** 919 files as of 2026-04-04.

**Format:** Single-line JSON (NOT JSONL) — each file contains one JSON object. Filename
pattern: `session-{session_id}.meta` where `session_id` is either a `ses-`-prefixed
CodeFlow session ID or a Claude per-agent UUID, depending on session era:

| Naming variant | Count | Example |
|----------------|-------|---------|
| `session-ses-{ULID}.meta` | 747 | `session-ses-01jnm3k...meta` (newer, CodeFlow session IDs) |
| `session-{UUID}.meta` | 172 | `session-01d26d6d-...meta` (older, Claude per-agent UUIDs) |

The `ses-` prefix variant is now the majority. The `calculate_duration()` function in
`session.rs` uses `format!("session-{session_id}.meta")` with whatever session_id is
passed in — both patterns are valid at the code level, reflecting the naming evolution.

**Observed schema** (verified against actual files):

| Field | Type | Notes |
|-------|------|-------|
| `session_id` | string | Either `ses-` prefixed CodeFlow ID (newer) or Claude per-agent UUID (older) |
| `started_at` | string (ISO 8601 ms) | Session start time (e.g., `"2026-02-16T07:35:02.000Z"`) |
| `started_epoch` | integer | Unix timestamp of session start — used by `SessionEndLogging` to compute duration |
| `repo_root` | string | Absolute path of the repository |
| `git_branch` | string | Git branch at session start |
| `git_commit` | string | Short commit hash at session start |
| `user` | string | OS username |
| `ended_at` | string (ISO 8601 ms) | Session end time |
| `duration_seconds` | integer | Session duration in seconds |
| `ended_epoch` | integer | Unix timestamp of session end |

**Actual example** (older UUID-pattern file — `ses-` prefix files have the same schema):

```json
{
  "session_id": "01d26d6d-e4c9-4cca-b5e3-dc59821ff0f7",
  "started_at": "2026-02-16T07:35:02.000Z",
  "started_epoch": 1771227302,
  "repo_root": "/Volumes/DATA/Local/software-workspace/projects/codeflow",
  "git_branch": "fix/session-id-cross-teammate",
  "git_commit": "b10cb5a",
  "user": "sathya",
  "ended_at": "2026-02-16T08:10:51.000Z",
  "duration_seconds": 2149,
  "ended_epoch": 1771229451
}
```

**Purpose:** Quick session metadata lookup without parsing full JSONL files.
`SessionEndLogging` in `session.rs` reads `started_epoch` (integer) from the `.meta`
file to compute `duration_seconds` in the `session_end` event.

**Accumulation:** ~919 files as of 2026-04-04, growing at approximately 15 files per
day. No rotation or cleanup mechanism identified — files accumulate indefinitely in
`.state/logs/sessions/`.

**Note on `started_epoch` vs `started_at`:** The `calculate_duration()` function in
`session.rs:221-228` reads `started_epoch` (int64). Both `started_epoch` and `started_at`
are present in actual files — there is no mismatch between what the code reads and what
the files contain.

---

## 7. Discontinued Conversation-Adjacent Logs

These log types existed in `.state/logs/sessions/` but have no active writers and will
not receive new entries.

### 7.1 skills-{date}.jsonl (DISCONTINUED)

**Count:** 9 files (2026-02-04 to 2026-02-14)

**Era:** Shell-era skill tracking. No Rust writer.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Shell-era timestamp |
| `session_id` | string | `"unknown"` in all observed entries |
| `event` | string | `"skill_completed"` |
| `skill_name` | string | Name of the skill invoked |
| `skill_args` | string | Arguments passed to the skill (often empty) |
| `success` | boolean | Whether the skill completed successfully |

**Sample entry:**

```json
{"ts":"2026-02-04T06:00:12.000Z","session_id":"unknown","event":"skill_completed","skill_name":"cf-git-workflow","skill_args":"","success":true}
```

Skills are no longer tracked as discrete log events. CodeFlow V4 does not emit skill
events.

### 7.2 stop-events-{date}.jsonl (DISCONTINUED)

**Count:** 6 files (2026-02-04 to 2026-02-08)

**Era:** Shell-era stop tracking. Superseded by `stop` events in `session-{date}.jsonl`.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Shell-era timestamp |
| `session_id` | string | `"unknown"` in all observed entries |
| `event` | string | `"stop"` or `"stop_task_check"` |
| `reason` | string | Stop reason (present on `stop` events) |
| `git_branch` | string | Current branch (present on `stop` events) |
| `has_uncommitted_changes` | boolean | Uncommitted change state |

**Sample entries:**

```json
{"ts":"2026-02-04T06:00:14.000Z","session_id":"unknown","event":"stop","reason":"unknown","git_branch":"feat/phase-3-hooks","has_uncommitted_changes":true}
{"ts":"2026-02-04T06:00:14.000Z","session_id":"unknown","event":"stop_task_check"}
```

Replaced by `stop` events in `session-{date}.jsonl` via `StopLogging` handler.

### 7.3 file-changes-{date}.jsonl (DISCONTINUED)

**Count:** 1 file (2026-02-04 only)

**Era:** Shell-era file change tracking.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Shell-era timestamp |
| `session_id` | string | `"unknown"` |
| `event` | string | `"file_modified"` |
| `tool_name` | string | Tool that modified the file |
| `file_path` | string | Absolute path of modified file |
| `success` | boolean | Whether the operation succeeded |

**Sample entry:**

```json
{"ts":"2026-02-04T06:00:11.000Z","session_id":"unknown","event":"file_modified","tool_name":"Edit","file_path":"/tmp/test.txt","success":true}
```

Never grew beyond initial test entries. `schema-standardization.md` suggests reviving
this log for parallel work tracking.

### 7.4 git-operations-{date}.jsonl (DISCONTINUED)

**Count:** 1 file (2026-02-04 only)

**Era:** Shell-era git operation tracking.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Shell-era timestamp |
| `session_id` | string | `"unknown"` |
| `event` | string | `"git_operation"` |
| `operation` | string | Operation type (e.g., `"other"`) |
| `details` | string | Operation details (often empty) |
| `branch` | string | Current branch |
| `success` | boolean | Operation outcome |

Replaced by `git/commits-{date}.jsonl` and `git/pr-events-{date}.jsonl`.

### 7.5 tasks-{date}.jsonl (DISCONTINUED)

**Count:** 1 file (2026-02-04 only)

**Era:** Shell-era task tracking.

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string | Shell-era timestamp |
| `session_id` | string | `"unknown"` |
| `event` | string | `"task_completed"` |
| `agent_type` | string | Agent type (often empty) |
| `description` | string | Task description (often empty) |
| `success` | boolean | Task outcome |
| `result_summary` | string | Summary of result (often empty) |

Never grew beyond initial test entries. `schema-standardization.md` suggests reviving.

---

## 8. Field Naming Inconsistencies

The conversation logs use `ts` as the timestamp field name, which differs from other
log types in the same or adjacent directories.

| File type | Timestamp field | Format | Writer |
|-----------|----------------|--------|--------|
| `session-{date}.jsonl` | `ts` | `YYYY-MM-DDTHH:MM:SS.sssZ` (ms, literal Z) | Rust ActivityWriter |
| `prompts-{date}.jsonl` | `ts` | `YYYY-MM-DDTHH:MM:SS.sssZ` (ms, literal Z) | Rust ActivityWriter |
| `tool-use-{date}.jsonl` | `ts` | `YYYY-MM-DDTHH:MM:SS.sssZ` (ms, literal Z) | Rust ActivityWriter |
| `cleanup-{date}.jsonl` | `timestamp` | RFC 3339 (via `now.to_rfc3339()`) | Rust standalone function |
| `git/commits-{date}.jsonl` | `ts` | `YYYY-MM-DDTHH:MM:SS.sssZ` | Go CLI |
| `git/pr-events-{date}.jsonl` | `ts` | `YYYY-MM-DDTHH:MM:SS.sssZ` | Go CLI |
| `.state/ledger/sessions.jsonl` | `timestamp` | RFC 3339 | Rust LedgerWriter |

**Summary:** `ActivityWriter`-based logs consistently use `ts`. `LedgerWriter` and
cleanup logs use `timestamp`. The divergence is an implementation artifact between the
activity logging subsystem and the ledger subsystem.

---

## 9. Configuration Reference

All conversation log behavior is controlled by the `"logging"` key in
`.codeflow/config/enforcement/enforcement-policy.json`.

**Current active configuration:**

```json
{
  "logging": {
    "session_start": {
      "enabled": true,
      "log_directory": ".state/logs/sessions",
      "capture_metadata": true,
      "rotation": {"max_age_days": 30, "max_logs": 100, "max_size_mb": 100}
    },
    "session_end": {
      "enabled": true,
      "generate_summary": true,
      "trigger_rotation": true,
      "log_directory": ".state/logs/sessions",
      "rotation": {"max_age_days": 30, "max_logs": 100, "max_size_mb": 100}
    },
    "post_tool_use": {
      "enabled": true,
      "log_directory": ".state/logs/sessions",
      "max_result_size": 2000,
      "redact_sensitive": true,
      "tools_to_log": ["Edit", "Write", "Bash", "Read", "Grep"]
    },
    "stop": {
      "enabled": true,
      "capture_pcv_details": true,
      "capture_task_context": true
    },
    "user_prompt": {
      "enabled": true,
      "capture_full_text": true,
      "detect_intent": true,
      "privacy_mode": false
    }
  }
}
```

**Configuration loaded by:** `read_config()` in
`codeflow-cli/core/src/hooks/logging/mod.rs`, which reads
`enforcement-policy.json` from the project directory at hook execution time.
