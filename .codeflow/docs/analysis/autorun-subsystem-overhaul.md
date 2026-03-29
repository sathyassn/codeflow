# Autorun Subsystem Overhaul

**Date:** 2026-03-28
**PR:** fix/autorun-subsystem-overhaul
**Epic:** INF-EPC-033
**Task:** INF-TSK-033-001

## Summary

End-to-end audit and fix of the autorun subsystem. Seven root-cause issues identified across worker invocation, naming conventions, observability, cleanup, and session initialization. Thirteen fixes applied across 14 files.

## Problems Found

### P1: `claude -p` non-interactive invocation (CRITICAL)

**Root cause:** `RealClaude::invoke()` in `autorun.rs:1353-1358` used `claude -p` (print mode) with `--output-format json` and stdout redirect (`> file 2>&1`). This made workers non-interactive: no TUI visible in tmux, `attach` showed a blank pane, and Claude ran as a one-shot batch tool.

**Fix (FIX 6):** Changed to `claude --dangerously-skip-permissions '{prompt}'` (interactive mode). Removed `-p`, `--output-format json`, and stdout redirect. Claude now runs interactively in the tmux pane, enabling `attach` to show a live session.

**File:** `codeflow-cli/cli/src/cmd/autorun.rs:1399-1402`

### P2: SessionStart env var timing (CRITICAL)

**Root cause:** `write_env_json(writer, &result.env_vars)` in `session_start.rs:545` was called AFTER all heavy operations (worktree creation, stale sweep, checkpoint init). With a 5-second hook timeout, the hook was killed before outputting env JSON to stdout. Claude Code never received `CODEFLOW_SESSION_ID`, `CF_PROJECT_ROOT`, or `CODEFLOW_WORKTREE_PATH`, causing all downstream hooks to lose worktree context.

**Fix (FIX 1):** Added early `write_env_json()` call at Section 2c (after worktree creation and env var population, before heavy operations). Claude Code processes the LAST env JSON line, so the final write at the end reinforces if it completes. If the hook times out, the early write ensures env vars are delivered.

**Fix (FIX 13):** Increased init hook timeout from 5s to 15s across all 4 settings templates and settings.json.

**File:** `codeflow-cli/core/src/hooks/session_start.rs:460-467`

### P3: Inconsistent worktree naming

**Root cause:** `worker.rs:285-288` generated a NEW session ID per worker, stripped the `ses-` prefix, and truncated to chars 4..20, producing names like `worktree-01kmv0qy8p9djshx`. Manual sessions used `worktree-{full_session_id}` with `ses-` prefix.

**Fix (FIX 2):** Changed to `format!("worktree-{worker_sid}")` using the full session ID with `ses-` prefix intact. Consistent with manual session naming.

**File:** `codeflow-cli/core/src/autorun/worker.rs:283`

### P4: Opaque tmux session naming

**Root cause:** `orchestrator.rs:497-498` used `codeflow-{session_id[0..8]}-w{N}` (e.g., `codeflow-ses-01km-w2`). The worker number was a positional dispatch index, not tied to the task ID. Impossible to identify which task was in which tmux session without DB lookup.

**Fix (FIX 5):** Changed to `cf-ar-{task_id.to_lowercase()}` (e.g., `cf-ar-inf-tsk-024-004`). Self-documenting. Replaced `tmux_prefix` field in `WorkerConfig` with `tmux_name` (full name passed from orchestrator).

**Files:** `codeflow-cli/core/src/autorun/orchestrator.rs:498`, `worker.rs:273`

### P5: tmux sessions killed immediately after worker exit

**Root cause:** `worker.rs:655` called `self.tmux.kill_session()` in cleanup, destroying all scrollback. `RealTmux::create_session()` didn't set `remain-on-exit`.

**Fix (FIX 7):** Added `set-option remain-on-exit on` after session creation. Pane persists after command exits, allowing post-mortem inspection via `attach` and `logs`.

**File:** `codeflow-cli/cli/src/cmd/autorun.rs:1230-1231`

### P6: Autorun events dirty `main` branch

**Root cause:** `emit_autorun_event()` wrote to `project_dir/.state/ledger/autorun-events/` and batch reports wrote to `project-management/tracking/autorun/`. Both paths were git-tracked. The orchestrator runs on `main`, so these files appeared as uncommitted changes.

**Fix (FIX 10):** Added `.state/ledger/autorun-events/` and `.state/ledger/coordination-events/` to `.gitignore`.

**Fix (FIX 11):** Changed default `report_dir` from `project-management/tracking/autorun` to `.state/autorun/reports` (already gitignored).

**Files:** `.gitignore`, `codeflow-cli/core/src/autorun/config.rs:52`

### P7: `.state/registry` phantom directory warnings

**Root cause:** `SHARED_STATE_DIRS` in `worktree/mod.rs:43-50` included `"registry"` but nothing ever created or wrote to `.state/registry/`. Every worktree creation logged a warning.

**Fix (FIX 4):** Removed `"registry"` from `SHARED_STATE_DIRS`.

**File:** `codeflow-cli/core/src/worktree/mod.rs:44`

## Additional Improvements

### Observability: `logs` fallback (FIX 8)
When tmux session is gone, `run_logs` now falls back to reading `worker-output.json` from the worktree instead of failing with "does not exist".

**File:** `codeflow-cli/cli/src/cmd/autorun.rs:694-703`

### Observability: `status` per-task detail (FIX 9)
`run_status` now shows per-worker detail (task ID, status, tmux session name, duration) below the batch summary line.

**File:** `codeflow-cli/cli/src/cmd/autorun.rs:605-639`

### Data model: `source` field on WorktreeEntry (FIX 3)
Added `source: Option<String>` to `WorktreeEntry` to distinguish `"interactive"` vs `"autorun"` worktrees. Enables targeted cleanup.

**File:** `codeflow-cli/core/src/worktree/registry.rs:35`

### Autorun worktree tagging (FIX 12)
`RealWorktreeProvider::setup()` now calls `locked_update_source()` to mark autorun-created worktrees with `source: "autorun"`.

**File:** `codeflow-cli/core/src/autorun/worker.rs:1084-1088`

## Test Results

- **cargo test:** 2693 passed, 0 failed, 1 ignored
- **cargo clippy:** Clean (zero warnings, `--all-targets --all-features -- -D warnings`)
- Updated tests: tmux naming format, WorkerConfig struct, WorktreeEntry constructors, conformance harness JSON parsing

## Files Changed (14)

| File | Fixes | Lines |
|------|-------|-------|
| session_start.rs | FIX 1, 3 | +18 -1 |
| worker.rs | FIX 2, 5, 12 | +28 -28 |
| orchestrator.rs | FIX 5 | +48 -48 |
| autorun.rs (cli) | FIX 6, 7, 8, 9 | +83 -83 |
| config.rs | FIX 11 | +16 -16 |
| registry.rs | FIX 3, 12 | +88 |
| mod.rs | FIX 4, 3 | +26 -26 |
| setup.rs | FIX 3 | +2 |
| sync.rs | FIX 3 | +1 |
| worktree.rs (cli) | FIX 3 | +9 |
| coordination.rs (cli) | FIX 3 | +2 |
| parallel.rs (cli) | FIX 3 | +1 |
| harness.rs (test) | multi-line JSON | +46 -46 |
| .gitignore | FIX 10 | +4 |
