# INF-TSK-024-028 Session ID Audit Report

**Task:** INF-TSK-024-028 — Audit and fix hook handlers using stdin UUID instead of
`current_session_id()`

**Date:** 2026-04-19

**Auditor:** cf-development (WS-DEV)

---

## Overview

This report documents the findings of the session ID source audit across all 9 hook
handler files in `codeflow-cli/core/src/hooks/`. The audit was conducted as part of
the session ID consolidation work (INF-EPC-024). The goal was to identify any handler
that reads the Claude per-agent UUID from stdin (`input.session_id`) and uses it as
the authoritative session identifier instead of calling `session::current_session_id()`.

**Conclusion:** Zero active code paths use stdin UUID as the authoritative session ID.
All handlers correctly route through `session::current_session_id()` (env file > env
var > error). One intentional fallback exists in `SessionStartLogging` — documented
and justified below. Criterion 5 (no fixes required) applies.

---

## Handler Audit Table

| File | Session ID Source | Usage | Verdict | Evidence |
|------|-------------------|-------|---------|----------|
| `hooks/session_start.rs` | `session::current_session_id()` (primary); `resolve_session_id()` for logging | Init handler uses `current_session_id()` at line 2226. Logging handler uses `resolve_session_id()` which calls `current_session_id()`. One intentional early-startup fallback to `input.session_id` at line 2418 (see note). | CORRECT | session_start.rs:2226, 2417-2419 |
| `hooks/session_end.rs` | `session::current_session_id()` | Main cleanup handler calls `current_session_id()` at line 83. All `result.session_id` assignments derive from this. | CORRECT | session_end.rs:83 |
| `hooks/pre_tool_use.rs` | `session::current_session_id()` (via CLI dispatch) | `GateCheck` struct stores a `SessionId` field. The CLI command handler (`cli/src/cmd/hooks/pre_tool_use.rs:39,59,99`) populates it via `current_session_id()` before constructing `GateCheck::new()`. No direct stdin UUID read in gate logic. | CORRECT | cli/src/cmd/hooks/pre_tool_use.rs:39,59,99; core/src/hooks/pre_tool_use.rs:461-482 |
| `hooks/post_tool_use.rs` | `session::current_session_id()` | All four production call sites (lines 60, 67, 166, 1058) call `current_session_id()` directly. | CORRECT | post_tool_use.rs:60,67,166,1058 |
| `hooks/logging/mod.rs` | `session::current_session_id()` (via `resolve_session_id()`) | `resolve_session_id()` at line 237 wraps `current_session_id()` with an `"unknown"` fallback for safe logging. All logging sub-handlers use this function. | CORRECT | logging/mod.rs:237 |
| `hooks/task_completed.rs` | `session::current_session_id()` | Single production call at line 183. | CORRECT | task_completed.rs:183 |
| `hooks/prompt_validate.rs` | N/A — no session ID read | `prompt_validate.rs` contains no `session_id` usage. No stdin UUID read, no `current_session_id()` call. | CORRECT | (no match) |
| `hooks/gh_pr_guard.rs` | N/A — no session ID read | `gh_pr_guard.rs` does not read session ID at any point. | CORRECT | (no match) |
| `hooks/pipeline.rs` | N/A — no session ID read | `pipeline.rs` does not read session ID at any point. | CORRECT | (no match) |

---

## Notes on the SessionStartLogging Early-Startup Fallback

**Location:** `hooks/session_start.rs:2412-2422` (`SessionStartLogging::handle`)

**Code:**

```rust
// Prefer CODEFLOW_SESSION_ID from env file over stdin UUID.
// SessionStartLogging fires early, before env file may be written on
// first startup, so fall back to input.session_id (Claude per-agent UUID).
let session_id_owned =
    crate::hooks::logging::resolve_session_id(std::path::Path::new(project_dir));
let session_id = if session_id_owned == "unknown" {
    input.session_id.as_deref().unwrap_or("unknown")
} else {
    &session_id_owned
};
```

**Assessment:** This is intentional and correct behavior. `SessionStartLogging` fires
at the very start of a session, potentially before `codeflow -i` has written the env
file. The fallback to `input.session_id` (stdin UUID) is only activated when
`resolve_session_id()` returns `"unknown"` — meaning no env file exists yet. Once the
env file is written (by `SessionStartInit`), all subsequent hook calls will correctly
resolve via `current_session_id()`. This fallback is logging-only and does not affect
any gate checks, claim ownership, or sentinel creation. It does not violate the session
ID consolidation goal.

---

## Logging Sub-Handler Coverage

All four logging sub-handlers route through `resolve_session_id()`:

| Sub-handler | Source |
|-------------|--------|
| `logging/session.rs` | `resolve_session_id()` — lines 36, 107 |
| `logging/prompt.rs` | `resolve_session_id()` — line 34 |
| `logging/stop.rs` | `resolve_session_id()` — line 34 |
| `logging/tooluse.rs` | `resolve_session_id()` — line 50 |

---

## Findings Summary

| Category | Count | Details |
|----------|-------|---------|
| Handlers audited | 9 | All files in `hooks/` (7 Rust files + `logging/mod.rs` + `logging/` sub-handlers) |
| Active stdin UUID usage | 0 | Zero paths use `input.session_id` as authoritative ID |
| `current_session_id()` call sites | 7+ | session_start.rs:2226, session_end.rs:83, post_tool_use.rs:60,67,166,1058, task_completed.rs:183, logging/mod.rs:237, CLI dispatch at pre_tool_use.rs:39,59,99 |
| Intentional fallbacks | 1 | `SessionStartLogging` early-startup fallback (logging-only, correct by design) |
| Issues requiring fixes | 0 | Criterion 5 applies — code is already correct |

---

## Recommendation

No code changes are required in the hook handlers. The session ID resolution is already
consolidated through `session::current_session_id()`. The one existing `input.session_id`
reference in `SessionStartLogging` is intentional and documented, scoped to logging only,
and only activates before the env file exists during the very first startup event.

**Regression test added:** `session::tests::test_current_session_id_env_file_wins_over_env_var`
in `codeflow-cli/core/src/session/mod.rs` guards the env-file-wins invariant against future
regressions.
