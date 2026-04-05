---
title: "Network Logs Schema Audit"
type: reference
status: active
author: cf-documentation
created_at: "2026-04-04"
updated_at: "2026-04-04"
scope: INF-TSK-024-006
feeds_into:
  - INF-TSK-024-007
---

# Network Logs Schema Audit

This document audits the schema of `.state/logs/security/network/`. It documents the log
schema from actual entries, catalogs the dead writer status, and assesses the Rust
replacement in `codeflow-cli/core/src/hooks/security/network.rs`.

Network logs are a subdirectory of the security log family. They share the security logs'
pretty-printed JSON format issue and dead-writer status. See
[security-logs-audit.md](security-logs-audit.md) for the full security log context.

## Table of Contents

- [1. File Location and Layout](#1-file-location-and-layout)
- [2. Writer Status: Dead](#2-writer-status-dead)
- [3. Pretty-Printed JSON Issue](#3-pretty-printed-json-issue)
- [4. Schema](#4-schema)
- [5. Event Type Catalog](#5-event-type-catalog)
- [6. Rust NetworkModule Assessment](#6-rust-networkmodule-assessment)
- [7. Field Naming](#7-field-naming)
- [8. Gap Analysis](#8-gap-analysis)

---

## 1. File Location and Layout

**Path:** `.state/logs/security/network/`

**Files:** 15 date-partitioned files, pattern `network-YYYY-MM-DD.jsonl`

```text
.state/logs/security/network/
├── network-2026-02-03.jsonl
├── network-2026-02-04.jsonl
├── ...
└── network-2026-03-03.jsonl        ← most recent file
```

**Date range:** 2026-02-03 through 2026-03-03 (not all dates are present — only dates with
network activity produced a file). No files exist after 2026-03-03, which marks the
retirement of the shell-era webfetch guard.

---

## 2. Writer Status: Dead

**Status: NO ACTIVE WRITER. Network logs have no active code writing to them.**

The shell-era writer was the webfetch guard hook script
(`.codeflow/scripts/security/enforcement/cf-webfetch-guard.sh`). This script was retired
as part of the Rust CLI integration (INF-EPC-022).

The Rust replacement is `NetworkModule` in
`codeflow-cli/core/src/hooks/security/network.rs`. It handles network enforcement (blocking
`git push/pull/fetch/clone` and `gh` CLI commands without sandbox bypass) but produces no
JSONL output. Network allow/block decisions are in-memory only, surfaced via stderr on
blocks.

| Component | Status | Output |
|-----------|--------|--------|
| `cf-webfetch-guard.sh` | RETIRED | Was: `network-{date}.jsonl` |
| `NetworkModule` (Rust) | ACTIVE | stderr only — no JSONL |

---

## 3. Pretty-Printed JSON Issue

Network log files use multi-line pretty-printed JSON, not compact single-line JSONL. Each
record spans multiple lines with 2-space indentation. Standard JSONL parsers that iterate
by line will fail on these files.

**Example from `network-2026-03-03.jsonl`** (as stored on disk):

```
{
  "ts": "2026-03-03T11:31:53.000Z",
  "level": "WARN",
  "session_id": "unknown",
  "event": "network_allowed",
  ...
}
{
  "ts": "2026-03-03T11:32:14.000Z",
  ...
}
```

Records are separated by blank lines with no delimiter characters. The file must be parsed
as a multi-document JSON stream, not as a JSONL file.

This is a shell-era artifact. The Rust-era ledger uses compact single-line JSONL. If a
Rust network log writer is added, it should use the standard `JsonlWriter` which produces
compact format.

---

## 4. Schema

**Universal fields (present on all observed entries):**

| Field | Type | Required | Notes |
|-------|------|----------|-------|
| `ts` | string (ISO 8601) | Yes | Millisecond precision (e.g., `"2026-03-03T11:31:53.000Z"`) |
| `level` | string | Yes | `"WARN"` on all observed entries |
| `session_id` | string | Yes | Session ID or `"unknown"` (see Section 7) |
| `event` | string | Yes | Event discriminator — see Section 5 |
| `log_type` | string | Yes | Always `"network"` |
| `tool` | string | Yes | Tool that triggered the check — `"WebFetch"` on all observed entries |
| `target` | string | Yes | Full URL being accessed |
| `reason` | string | Yes | Human-readable reason (may be empty string) |
| `domain` | string | Yes | Hostname extracted from `target` URL |
| `allowlist` | string | Yes | Classification of the domain — see Section 5 |

**Sample entry (compact representation):**

```json
{
  "ts": "2026-03-03T11:31:53.000Z",
  "level": "WARN",
  "session_id": "unknown",
  "event": "network_allowed",
  "log_type": "network",
  "tool": "WebFetch",
  "target": "https://docs.anthropic.com/",
  "reason": "",
  "domain": "docs.anthropic.com",
  "allowlist": "trusted"
}
```

---

## 5. Event Type Catalog

Three event types appear in network logs:

### 5.1 network_allowed

Recorded when a WebFetch request was permitted because the domain is in the trusted
allowlist.

| Additional fields | Value |
|------------------|-------|
| `allowlist` | `"trusted"` |

This is the most common event type in the observed files (7 of 8 entries in
`network-2026-03-03.jsonl`).

### 5.2 network_ask

Recorded when a WebFetch request was directed to a domain not in the trusted allowlist.
The shell-era webfetch guard would prompt or ask before allowing.

| Additional fields | Value |
|------------------|-------|
| `allowlist` | `"not_in_allowlist"` |

**Sample entry (compact):**

```json
{
  "ts": "2026-03-03T11:32:14.000Z",
  "level": "WARN",
  "session_id": "unknown",
  "event": "network_ask",
  "log_type": "network",
  "tool": "WebFetch",
  "target": "https://evil-untrusted.example.com/",
  "reason": "",
  "domain": "evil-untrusted.example.com",
  "allowlist": "not_in_allowlist"
}
```

### 5.3 network_blocked

Recorded when a network request was explicitly blocked (domain is in a block list or the
operation type is disallowed). Observed in earlier date files.

| Additional fields | Value |
|------------------|-------|
| `allowlist` | `"blocked"` or domain-specific block category |

**Note on Rust coverage:** The Rust `NetworkModule` handles a different class of network
operations than the shell webfetch guard. The Rust module focuses on Bash tool calls (git
network, gh CLI), not WebFetch tool calls. The webfetch guard handled `WebFetch` tool
calls. Neither Rust module writes JSONL for WebFetch events.

---

## 6. Rust NetworkModule Assessment

The Rust replacement for network enforcement is `NetworkModule` in
`codeflow-cli/core/src/hooks/security/network.rs`.

**What it checks:**

The `NetworkModule` only inspects Bash tool calls (enforced by the `SecurityHandler`
which returns `HookOutput::Allow` immediately for non-Bash tools). It does not intercept
WebFetch tool calls.

| Operation class | Patterns | Action when sandbox_bypass=false |
|----------------|----------|----------------------------------|
| Git network | `git push/pull/fetch/clone`, `git remote update`, `git ls-remote` | Block with `"Network Operation"` category |
| GitHub CLI | `gh pr/issue/release/api/workflow/run/repo/gist` | Block with `"Network Operation"` category |
| WebFetch URL access | (not checked) | Not in scope for this module |

**PathFlow-aware messaging:** When `is_pathflow_active` is true, the block reason appends
`" In PathFlow mode, delegate to cf-git-operations teammate."` This provides actionable
guidance to the agent without a separate log file.

**No WebFetch equivalent in Rust:** The shell-era webfetch guard logged every WebFetch
request (allowed and blocked) to `network-{date}.jsonl`. The Rust hook pipeline has a
`webfetch-guard` entry in `settings.json` that invokes `codeflow hooks pre-tool-use
webfetch-guard`, but this handler does not produce JSONL output.

**Session ID:** `SecurityHandler` (which wraps `NetworkModule`) reads session ID from
`session::current_session_id()` which reads `codeflow-env.sh`. It does not use the
`session_id` from stdin. This corrects the `"unknown"` session_id problem that appeared in
shell-era logs.

---

## 7. Field Naming

### 7.1 Timestamp: `ts` vs `timestamp`

Network logs use `ts`, not `timestamp`. This is consistent with the broader security log
family but inconsistent with the ledger family (sessions, work-graph, coordination-events).

| Log family | Timestamp field |
|------------|----------------|
| Network logs (security family) | `ts` |
| Ledger JSONL files | `timestamp` |

### 7.2 Session ID: `"unknown"` literal

All 8 observed entries in `network-2026-03-03.jsonl` use `"unknown"` for `session_id`.
The shell-era webfetch guard did not inject the session ID into its log entries. This
creates a traceability gap — it is impossible to associate a network log entry with a
specific session by querying on `session_id`.

The Rust `SecurityHandler` corrects this by reading the session ID from `codeflow-env.sh`.
However, since `NetworkModule` does not write JSONL, no new network log entries are
produced.

### 7.3 allowlist field values

The `allowlist` field encodes the domain's classification from the webfetch guard's
allowlist configuration:

| Value | Meaning |
|-------|---------|
| `"trusted"` | Domain is in the trusted allowlist — request allowed |
| `"not_in_allowlist"` | Domain is not in any configured list — request prompted |
| `"blocked"` | Domain is in the block list — request denied |

---

## 8. Gap Analysis

| Aspect | Current State | Gap | Downstream Task |
|--------|--------------|-----|----------------|
| Active writer | None | Network requests not persisted | INF-TSK-024-007 |
| WebFetch coverage | No Rust equivalent for WebFetch logging | WebFetch allow/block invisible | INF-TSK-024-007 |
| Pretty-printed JSON | All files multi-line JSON | Violates JSONL spec | INF-TSK-024-007 |
| `ts` field | Uses `ts` | Inconsistent with ledger `timestamp` | INF-TSK-024-007 |
| `"unknown"` session_id | All observed entries | No session traceability | Fixed in Rust (if JSONL added) |
| `network_blocked` event | Observed in older files | No Rust equivalent defined | INF-TSK-024-007 |
| Bash vs WebFetch scope | Rust NetworkModule handles Bash only | WebFetch requests not checked by Rust | Architecture decision needed |
