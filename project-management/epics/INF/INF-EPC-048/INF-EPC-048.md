---
id: "epic-01KPNA05JCP5TT8GT0DJNTW1BH"
format_id: "INF-EPC-048"
title: "Autorun Reliability P0"
summary: "Critical reliability fixes for autorun: gh-pr-guard base enforcement, rescue-to-patch-bundle refactor, batch report commit via ephemeral worktree, status TUI async refresh, CI-green wait, and pathflow-config session_complete gate"
status: in_progress
area_type: "INF"
work_type: "FIX"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-20T00:00:00Z"
updated_at: "2026-04-20T00:00:00Z"
---

# INF-EPC-048: Autorun Reliability P0

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Six P0 reliability fixes for the autorun subsystem: (1) gh-pr-guard hook must enforce `--base` for autorun PRs; (2) rescue flow refactored to write patch bundles instead of branch commits, with new `codeflow rescue` CLI subcommands; (3) batch report commit uses an ephemeral detached-HEAD git worktree; (4) status TUI switches to bulk query, decoupled async refresh, and correct TASK/IDLE/freeze semantics; (5) autorun merge waits for CI-green before proceeding; (6) pathflow-config.json gains a `session_complete` gate entry targeting PF7-END.

## Scope

### In Scope

- `codeflow-cli/core/src/hooks/pre_tool_use.rs` — gh-pr-guard `--base` enforcement
- `codeflow-cli/core/src/autorun/` — rescue refactor, CI-wait, report commit
- `codeflow-cli/core/src/session/` — rescue patch-bundle storage
- `codeflow-cli/cli/src/` — `rescue` CLI subcommands
- `codeflow-cli/core/src/tui/` — status TUI bulk query + async refresh
- `codeflow-cli/core/src/store/schema.surql` — AutorunSession new fields + migration
- `.codeflow/config/pathflow/pathflow-config.json` — `session_complete` gate entry

### Out of Scope

- New autorun batch scheduling features
- Unrelated TUI columns or panels
- Non-autorun workflow changes

## Acceptance Criteria

- [ ] gh-pr-guard blocks autorun `gh pr create` missing `--base` or mismatched with `AUTORUN_INTEGRATION_BRANCH`/`active-task.json:target_branch`
- [ ] Rescue writes patches to `.state/rescue/{sid}-{ts}-{pid}/`; no git commit/push/branch in rescue path
- [ ] `codeflow rescue list|show|apply|drop` subcommands functional; `apply` uses `git apply --3way`
- [ ] `session_complete` gate entry added to pathflow-config.json gates section
- [ ] `commit_report_to_branch` uses ephemeral detached-HEAD worktree; captures stderr on failure; `--force-with-lease` push with retry
- [ ] Pre-commit hooks run in ephemeral worktree (no `--no-verify`); markdown output passes markdownlint rules
- [ ] Status TUI uses bulk `WHERE session_id IN $ids` query; background task + `tokio::sync::watch`; 2s fetch timeout with stale fallback
- [ ] `AutorunSession` gains `current_task_id`, `updated_at`, `last_heartbeat_at` fields; schema.surql migration added
- [ ] TUI TASK column from `current_task_id`; ELAPSED freezes on terminal status; IDLE column with heartbeat TTL; stale footer; Esc/r/? keys
- [ ] Autorun merge waits for CI checks with configurable timeout; on fail/timeout PR left open, task blocked
- [ ] Zero hardcoded `pf-N`/`ws-*` string literals in new modules; existing hardcoded fallback at `autorun.rs:3922` replaced with fail-loud error
- [ ] Unit + integration tests per fix area; guard test greps for hardcoded phase/stage names

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-048-001 | Autorun Reliability P0 — combined fix | complete | high |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

All six fixes are independent at the source-file level and can be implemented in a single WS-DEV pass. The pipeline is FIX: WS-DEV → WS-SEC → WS-REV → WS-QA.

Key files to touch:
- `codeflow-cli/core/src/hooks/pre_tool_use.rs` (gh-pr-guard)
- `codeflow-cli/core/src/autorun/worker.rs` (rescue, CI-wait)
- `codeflow-cli/core/src/autorun/orchestrator.rs` (report commit)
- `codeflow-cli/core/src/tui/status.rs` (TUI refresh)
- `codeflow-cli/core/src/store/schema.surql` (schema migration)
- `.codeflow/config/pathflow/pathflow-config.json` (gate entry)

### Autorun Batching

Not applicable — delivered via interactive session.

## Related

- INF-EPC-044: Prior autorun reliability fixes (complete, PR #287)
- INF-EPC-047: Status TUI fixes (complete, PR #297)
