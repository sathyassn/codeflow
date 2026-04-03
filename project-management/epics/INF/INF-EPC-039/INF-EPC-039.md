---
id: "epic-01KN82H8MFYJ48GZN4KCEMYVRF"
format_id: "INF-EPC-039"
title: "Worktree PID Detection and Cleanup UX"
summary: "Fix broken PID detection in get_claude_code_pid(), centralize liveness checks into session/liveness.rs, and implement interactive cleanup UX with --force flag support"
status: complete
area_type: "INF"
work_type: "FIX"
domain: "GENL"
is_ongoing: false
file_scope:
  - "codeflow-cli/core/src/session/process.rs"
  - "codeflow-cli/core/src/session/liveness.rs"
  - "codeflow-cli/core/src/session/mod.rs"
  - "codeflow-cli/core/src/hooks/session_start.rs"
  - "codeflow-cli/core/src/hooks/session_end.rs"
  - "codeflow-cli/cli/src/cmd/worktree.rs"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-02T21:46:00Z"
updated_at: "2026-04-02T21:46:00Z"
---

# INF-EPC-039: Worktree PID Detection and Cleanup UX

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

Fix the broken PID detection system and implement a reliable liveness layer. The root cause is `get_claude_code_pid()` in `session/process.rs` which walks up exactly 2 levels assuming a `claude → sh -c → codeflow` process tree, but the actual tree is `claude → zsh → codeflow` (no `sh -c` intermediate). Additionally, `worktrees.yaml` writers use `std::process::id()` (the ephemeral hook PID), which is always dead by the time liveness is checked.

The fix is three-layered:
1. **PID writers:** name-based tree walk in `get_claude_code_pid()` + `validate_claude_pid()` before storage
2. **Centralized readers:** new `session/liveness.rs` module with `check_session_liveness()`, all 11 readers call it
3. **Interactive cleanup UX:** analyze → display → confirm → execute, `--force <id>` targeted, `--force all` nuclear

## Scope

| File | Action |
|------|--------|
| `codeflow-cli/core/src/session/process.rs` | Fix PID walk logic (name-based, not depth-based) |
| `codeflow-cli/core/src/session/liveness.rs` | NEW — centralized liveness module |
| `codeflow-cli/core/src/session/mod.rs` | Route all readers through liveness.rs |
| `codeflow-cli/core/src/hooks/session_start.rs` | Update PID storage (HIGH-tier, staged edit) |
| `codeflow-cli/core/src/hooks/session_end.rs` | Update cleanup reads (HIGH-tier, staged edit) |
| `codeflow-cli/cli/src/cmd/worktree.rs` | Interactive cleanup UX |

## Tasks

| Task | Title | Status |
|------|-------|--------|
| INF-TSK-039-001 | Fix PID detection, centralized liveness, interactive cleanup UX | complete |
