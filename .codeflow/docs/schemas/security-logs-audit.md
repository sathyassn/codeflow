---
title: "Security Logs Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-04"
updated_at: "2026-04-04"
scope: INF-TSK-024-006
feeds_into:
  - INF-TSK-024-007
---

# Security Logs Schema Audit

This document audits the schema of all files under `.state/logs/security/`. It catalogs
each subdirectory, documents field schemas from actual log entries, identifies dead writers,
and assesses the state of the Rust replacement in
`codeflow-cli/core/src/hooks/security/`.

## Table of Contents

- [1. Directory Structure](#1-directory-structure)
- [2. Critical Finding: No Active Writers](#2-critical-finding-no-active-writers)
- [3. Critical Finding: Pretty-Printed JSON](#3-critical-finding-pretty-printed-json)
- [4. Subdirectory Schemas](#4-subdirectory-schemas)
- [5. Discontinued Logs: sentinel Subdirectory](#5-discontinued-logs-sentinel-subdirectory)
- [6. Non-JSONL File: protection-audit.log](#6-non-jsonl-file-protection-auditlog)
- [7. Rust Security Module Assessment](#7-rust-security-module-assessment)
- [8. Field Naming Catalog](#8-field-naming-catalog)
- [9. Gap Analysis Against Rust Target](#9-gap-analysis-against-rust-target)

---

## 1. Directory Structure

**Base path:** `.state/logs/security/`

```text
.state/logs/security/
├── audit/
│   └── audit-{date}.jsonl          — 27 date files (2026-02-05 to 2026-03-03)
├── blocked/
│   └── blocked-{date}.jsonl        — 27 date files (2026-02-05 to 2026-03-03)
├── network/
│   └── network-{date}.jsonl        — 15 date files (2026-02-03 to 2026-03-03)
├── protection/
│   └── protection-{date}.jsonl     — 27 date files (2026-02-05 to 2026-03-03)
├── sentinel/
│   └── sentinel-{date}.jsonl       — 10 date files (2026-02-05 to 2026-02-14) [DISCONTINUED]
└── protection-audit.log            — plain text, root-owned [NOT JSONL]
```

All date-partitioned files follow the pattern `{subdirectory-name}-YYYY-MM-DD.jsonl`. The
last date across all active subdirectories is `2026-03-03`, which matches the shell-era
retirement date. No files have appeared since then.

---

## 2. Critical Finding: No Active Writers

**Status: ALL security JSONL files are dead. No active code writes to `.state/logs/security/`.**

The shell-era enforcement scripts that populated these files have been retired as part of the
Rust CLI integration (INF-EPC-022). The Rust security module at
`codeflow-cli/core/src/hooks/security/` replaces the shell scripts but does **not** write
to any JSONL log file.

| Subdirectory | Shell-Era Writer | Shell Writer Status | Rust Replacement | Rust Writes JSONL? |
|-------------|-----------------|---------------------|------------------|--------------------|
| `audit/` | Shell enforcement scripts (path, fileops, branch guards) | RETIRED | `SecurityHandler` in `security/mod.rs` | No — stderr only |
| `blocked/` | Shell cf-path-protection.sh, cf-fileops-guard.sh | RETIRED | `SecurityChecker` modules | No — stderr only |
| `network/` | Shell cf-webfetch-guard.sh | RETIRED | `NetworkModule` in `security/network.rs` | No — stderr only |
| `protection/` | Shell protection guard scripts | RETIRED | `PathModule` in `security/path.rs` | No — stderr only |
| `sentinel/` | Shell sentinel skill tracking | DISCONTINUED | None | N/A |

**Implication:** Security events (blocked commands, network checks, path violations) are
captured only in the Rust hook's stderr output. They are not persisted to any JSONL file.
This means security events are unqueryable, non-auditable after the session ends, and
invisible to the WorkGraph and ledger rebuild pipeline.

**Downstream task:** INF-TSK-024-007 (canonical event schema) must decide whether to add
Rust JSONL writers for security events or formally declare them out of scope for the ledger.

---

## 3. Critical Finding: Pretty-Printed JSON

**Status: ALL active security JSONL files use multi-line pretty-printed JSON, not compact JSONL.**

Standard JSONL specification (ndjson.org) requires one JSON object per line with no embedded
newlines. Every file in `audit/`, `blocked/`, `network/`, and `protection/` uses multi-line
indented JSON with no line delimiters between records.

**Example from `.state/logs/security/blocked/blocked-2026-03-03.jsonl`:**

```json
{
  "ts": "2026-03-03T05:40:45.000Z",
  "level": "WARN",
  "session_id": "ses-177251642060519291f84641b",
  "event": "command_blocked",
  "log_type": "blocked",
  "tool": "Bash",
  "target": "rm /Volumes/.../pathflow/is-pathflow-active",
  "reason": "Dangerous operation on protected path",
  "module": "/path/to/cf-path-protection.sh"
}
```

**Impact:** Tools that process JSONL by iterating lines (e.g., `jq -Rs`, standard JSONL
parsers) will fail or produce incorrect results. The records must be parsed as multi-document
JSON files, not as JSONL streams.

The `sentinel/` subdirectory is the sole exception — it uses compact single-line JSONL
(see Section 5).

---

## 4. Subdirectory Schemas

### 4.1 audit/audit-{date}.jsonl

The `audit/` subdirectory contains a mixed-event aggregation log. It does not have a single
event type — it receives multiple event types from different enforcement scripts.

**Observed event types (from `audit-2026-03-03.jsonl`):**

| Event value | Count | Description |
|-------------|-------|-------------|
| `command_blocked` | 486 | Command was blocked by a security rule |
| `path_protected` | 13 | Path access blocked by protection tier |
| `staging_area_access` | 7 | Access to managed staging area allowed |
| `network_allowed` | 7 | Network operation allowed through |
| `edit_write_allowed` | varies | Edit/Write tool allowed on monitored path |
| `protected_edit_blocked` | 4 | Edit/Write blocked on protected resource |

**Universal fields (present on all observed entries):**

| Field | Type | Notes |
|-------|------|-------|
| `ts` | string (ISO 8601 with milliseconds) | e.g., `"2026-03-03T05:40:45.000Z"` |
| `level` | string | `"INFO"`, `"WARN"`, `"BLOCK"`, or `"blocked"` |
| `session_id` | string | Session identifier or `"unknown"` |
| `event` | string | Event discriminator (see table above) |
| `log_type` | string | Value is `"audit"` — used for routing/filtering |
| `tool` | string | Claude Code tool name (e.g., `"Bash"`, `"Edit"`, `"Write"`) |
| `target` | string | The command or file path subject to the check |
| `reason` | string | Human-readable explanation (may be empty) |

**Event-specific additional fields:**

| Event | Additional Fields |
|-------|------------------|
| `command_blocked` | `module` (path to shell script that blocked) |
| `path_protected` | `tier` (`"critical"`, `"high"`, `"moderate"`), `outcome` (`"blocked"`) |

**Sample compact representation:**

```json
{
  "ts": "2026-03-03T05:40:45.000Z",
  "level": "WARN",
  "session_id": "ses-177251642060519291f84641b",
  "event": "path_protected",
  "log_type": "protection",
  "tool": "Bash",
  "target": ".state/session/**",
  "reason": "",
  "tier": "critical",
  "outcome": "blocked"
}
```

### 4.2 blocked/blocked-{date}.jsonl

The `blocked/` subdirectory contains entries for commands that were blocked by enforcement
scripts. In practice, `audit-{date}.jsonl` and `blocked-{date}.jsonl` for the same date
contain duplicate entries — the shell scripts wrote the same event to both files.

**Primary event type:** `command_blocked`

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string (ISO 8601) | Yes | Millisecond precision |
| `level` | string | Yes | Typically `"WARN"` |
| `session_id` | string | Yes | Session ID or `"unknown"` |
| `event` | string | Yes | `"command_blocked"` |
| `log_type` | string | Yes | `"blocked"` |
| `tool` | string | Yes | Tool that was blocked (e.g., `"Bash"`) |
| `target` | string | Yes | The full command string |
| `reason` | string | Yes | Human-readable block reason |
| `module` | string | No | Path to the shell script that issued the block |

### 4.3 protection/protection-{date}.jsonl

The `protection/` subdirectory contains entries for path-level protection events, written by
the tiered resource protection system (shell-era `cf-protect-resources.sh` and its
enforcement library scripts).

**Primary event type:** `path_protected`

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string (ISO 8601) | Yes | Millisecond precision |
| `level` | string | Yes | Typically `"WARN"` |
| `session_id` | string | Yes | Session ID or `"unknown"` |
| `event` | string | Yes | `"path_protected"` |
| `log_type` | string | Yes | `"protection"` |
| `tool` | string | Yes | Tool that triggered the check |
| `target` | string | Yes | The path or glob pattern matched |
| `reason` | string | No | Explanation (may be empty) |
| `tier` | string | Yes | Protection tier: `"critical"`, `"high"`, or `"moderate"` |
| `outcome` | string | Yes | `"blocked"` or `"allowed"` |

**Sample entry (compact representation):**

```json
{
  "ts": "2026-03-03T05:40:45.000Z",
  "level": "WARN",
  "session_id": "ses-177251642060519291f84641b",
  "event": "path_protected",
  "log_type": "protection",
  "tool": "Bash",
  "target": ".state/session/**",
  "reason": "",
  "tier": "critical",
  "outcome": "blocked"
}
```

---

## 5. Discontinued Logs: sentinel Subdirectory

**Status: DISCONTINUED. Last entry: 2026-02-14.**

The `sentinel/` subdirectory tracked shell-era skill sentinel creation events. These
sentinels were ephemeral markers that shell-era skills created to indicate that a given
operation had been applied in a session (e.g., `documentation-standards:apply-standard`).

**This subdirectory is unique among security logs** — it uses compact single-line JSONL
(not pretty-printed), and its `session_id` values are raw Claude UUIDs (not
`ses-{ULID}`-format session IDs).

**Schema:**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string (ISO 8601) | Yes | Millisecond precision, compact JSONL |
| `level` | string | Yes | `"INFO"` |
| `session_id` | string | Yes | Raw Claude UUID (e.g., `"acaad041-da67-4eac-a72d-a61c938a6e05"`) |
| `event` | string | Yes | `"sentinel_created"` |
| `sentinel_type` | string | Yes | `"skill"` |
| `skill` | string | Yes | Skill name (e.g., `"documentation-standards"`) |
| `operation` | string | Yes | Operation performed (e.g., `"apply-standard"`, `"lint-file"`) |
| `sentinel_id` | string | Yes | Unique sentinel key: `"{skill}:{operation}-{UUID}"` |
| `ttl_sec` | integer | Yes | Time-to-live in seconds (typically `600`) |
| `path` | string | Yes | Same value as `sentinel_id` |

**Sample entry:**

```json
{"ts":"2026-02-14T06:16:59.000Z","level":"INFO","session_id":"acaad041-da67-4eac-a72d-a61c938a6e05","event":"sentinel_created","sentinel_type":"skill","skill":"documentation-standards","operation":"apply-standard","sentinel_id":"documentation-standards:apply-standard-6D20C0F4-098E-4EDA-8197-B62A8F7C3DE7.json","ttl_sec":600,"path":"documentation-standards:apply-standard-6D20C0F4-098E-4EDA-8197-B62A8F7C3DE7.json"}
```

**Why discontinued:** The shell-era skill sentinel system was replaced by the Rust-based
PathFlow sentinel system (`codeflow-cli/core/src/hooks/post_tool_use.rs` sentinel-write
handler). PathFlow sentinels are stored under `.state/sentinels/pathflow/{session-id}/`
as files, not as JSONL log entries. No Rust equivalent for skill-sentinel JSONL entries
exists.

---

## 6. Non-JSONL File: protection-audit.log

**File:** `.state/logs/security/protection-audit.log`
**Format:** Plain text (NOT JSONL)
**Access:** Root-owned; requires elevated permissions to read

This file records sudo-level resource protection operations performed by
`.codeflow/scripts/security/cf-protect-resources.sh`. It is a human-readable audit trail
of filesystem permission changes.

**Format:**

```text
[YYYY-MM-DD HH:MM:SS] USER={username} ACTION={action} PATH={path}
```

**Example:**

```text
[2026-02-08 00:35:54] USER=sathya ACTION=PROTECT PATH=.claude/hooks/codeflow
```

**Fields:**

| Field | Format | Notes |
|-------|--------|-------|
| Timestamp | `[YYYY-MM-DD HH:MM:SS]` | Local time (no timezone) |
| `USER` | string | Unix username that ran the sudo operation |
| `ACTION` | string | `PROTECT`, `PROMOTE`, or `RELOAD` |
| `PATH` | string | Filesystem path subject to the operation |

This file is outside the JSONL schema family. It is not parseable by JSONL tools and is
not routed to the ledger. It is a syslog-style audit trail for the sudo protection subsystem.

---

## 7. Rust Security Module Assessment

The Rust security module at `codeflow-cli/core/src/hooks/security/` is the current active
replacement for the retired shell enforcement scripts.

**Module structure** (10 files, verified via Glob):

| File | Module Name | Purpose |
|------|-------------|---------|
| `mod.rs` | orchestrator | Defines `Verdict`, `CheckContext`, `SecurityChecker`, `SecurityHandler` |
| `dangerous.rs` | `dangerous-commands` | Destructive commands (`rm -rf /`, fork bombs) |
| `privilege.rs` | `privilege-protection` | Privilege escalation (`sudo`, `su`, `doas`) |
| `git.rs` | `git-protection` | Git hook bypass, force push, protected branches |
| `path.rs` | `path-protection` | Protected path operations |
| `fileops.rs` | `file-operations` | Indirect file operations, glob bypass |
| `branch.rs` | `branch-file-protection` | File writes on protected branches |
| `tmp.rs` | `tmp-protection` | Managed tmp folder protection |
| `network.rs` | `network-protection` | Network operations without sandbox bypass |
| `pattern.rs` | pattern utilities | Shared pattern matching helpers |

**Key types (from `security/mod.rs`):**

```rust
pub struct Verdict {
    pub allow: bool,
    pub category: String,
    pub reason: String,
    pub pattern: String,
    pub module: String,
}

pub struct CheckContext<'a> {
    pub tool_name: &'a str,
    pub command: &'a str,
    pub sandbox_bypass: bool,
    pub project_dir: &'a str,
    pub session_id: &'a str,
    pub current_branch: &'a str,
    pub is_pathflow_active: bool,
    pub policy: &'a EnforcementPolicy,
}
```

**Output mechanism:** `SecurityHandler::handle()` returns `HookOutput::Block { reason, category }` on violation. The `reason` is written to stderr only. No JSONL file is written.

**Session ID source:** `SecurityHandler` reads session ID from `session::current_session_id(&self.project_dir)`, which reads `codeflow-env.sh`. It does not use the `session_id` field from stdin. This is the correct pattern.

**NetworkModule specifics** (from `security/network.rs`):

The `NetworkModule` blocks git network commands (`git push/pull/fetch/clone`) and GitHub
CLI commands (`gh pr/issue/release/api/workflow/run/repo/gist`) when `sandbox_bypass` is
false. When `is_pathflow_active` is true, the block message includes a delegation hint to
`cf-git-operations`. Returns `None` (allow) when `sandbox_bypass: true`.

---

## 8. Field Naming Catalog

### 8.1 Timestamp field: `ts` vs `timestamp`

Security logs use `ts` (not `timestamp`). This differs from the ledger's `sessions.jsonl`
and `work-graph.jsonl`, which use `timestamp`.

| Log family | Timestamp field | Format |
|------------|----------------|--------|
| Security logs (all subdirectories) | `ts` | ISO 8601 with milliseconds (`2026-03-03T05:40:45.000Z`) |
| Ledger files (sessions, work-graph) | `timestamp` | ISO 8601 without milliseconds (`2026-03-04T09:22:50Z`) |

**Finding:** The `ts` vs `timestamp` inconsistency is a systematic difference between the
security log family (shell-era) and the ledger family (Go/Rust era). Security logs were
written by separate shell scripts that used a different field naming convention than the
ledger writers.

### 8.2 Event discriminator: `event` field

All security logs use an `event` field as the primary discriminator. Event values are not
namespaced — they use simple snake_case strings (e.g., `command_blocked`, `path_protected`,
`network_allowed`). This is consistent across subdirectories.

### 8.3 Routing field: `log_type`

All entries include a `log_type` field matching the subdirectory name:

| Subdirectory | `log_type` value |
|-------------|-----------------|
| `audit/` | `"audit"` |
| `blocked/` | `"blocked"` |
| `network/` | `"network"` |
| `protection/` | `"protection"` |
| `sentinel/` | (absent — sentinel files predate this field) |

### 8.4 Session ID format

Security logs show two session ID formats in the wild:

| Format | Example | Source |
|--------|---------|--------|
| Shell-era ULID-prefix | `"ses-177251642060519291f84641b"` | Shell-era session IDs |
| `"unknown"` literal | `"unknown"` | Network and audit logs from sessions where session ID was not injected |
| Raw Claude UUID | `"acaad041-da67-4eac-a72d-a61c938a6e05"` | Sentinel subdirectory only — pre-dates session ID normalization |

**Finding:** The `"unknown"` session_id value in network logs indicates that the shell-era
webfetch guard did not have access to the session ID at write time. This is a traceability
gap.

---

## 9. Gap Analysis Against Rust Target

| Aspect | Current State | Gap | Downstream Task |
|--------|--------------|-----|----------------|
| JSONL writer | None — Rust module writes to stderr only | Security events not persisted | INF-TSK-024-007 |
| Pretty-printed format | Multi-line JSON in all active subdirectories | Violates JSONL spec; parsers fail | INF-TSK-024-007 |
| `ts` vs `timestamp` | `ts` in security logs; `timestamp` in ledger | Cross-family inconsistency | INF-TSK-024-007 |
| `"unknown"` session_id | Network and some audit logs | Traceability gap for those events | INF-TSK-024-007 |
| Sentinel subdirectory | Discontinued 2026-02-14 | No Rust equivalent | N/A — closed chapter |
| protection-audit.log | Plain text, root-owned | Not parseable; outside JSONL family | Out of scope for ledger |
| Duplicate entries (audit + blocked) | Same events written to both subdirectories | Redundancy in shell-era design | INF-TSK-024-007 |
