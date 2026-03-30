# Autorun Subsystem Gap Analysis

**Date:** 2026-03-29
**PR:** fix/worktree-path-resolution-v2 (PR #231)
**Status:** All gaps resolved

## 1. Session ID Mismatch (HIGH)

**Problem:** Worker acquires CRDT claims using `worker_sid` (generated per-worker in `worker.rs:282`), but exports `cfg.session_id` (the batch-level session ID) as `AUTORUN_SESSION_ID`. The SessionStart hook inside Claude uses `AUTORUN_SESSION_ID` as the session identity, so all hook-side claim operations (gate-check, scope enforcement) reference the batch session ID, not the worker's claim-holding ID.

**Impact:** Claim isolation is broken. A worker's hooks cannot release or verify claims because they use a different session ID than the one that acquired them.

**Fix:** Export `worker_session_id` as `AUTORUN_SESSION_ID` instead of `cfg.session_id`. Introduce `AUTORUN_BATCH_ID` for batch-level correlation. Thread `worker_session_id` through `InvokeConfig`.

**Files changed:**
- `codeflow-cli/cli/src/cmd/autorun.rs` -- export fix, new AUTORUN_BATCH_ID
- `codeflow-cli/core/src/autorun/worker.rs` -- InvokeConfig field, active-task.json fix

## 2. Merge Queue Advisory Nature (MEDIUM)

**Problem:** The merge queue (`coordination/merge_queue.rs`) is documented as providing PR merge ordering, but it is currently advisory only. Enqueue/dequeue happens AFTER Claude finishes work and creates the PR. No queue-position checking occurs before PR merge.

**Impact:** The merge queue records ordering but does not enforce it. GitHub's own conflict detection provides the actual safety net.

**Fix:** Added doc comment at `worker.rs:597` clarifying advisory nature. No code change needed -- the current behavior is correct for the project's needs.

## 3. Resume Capability (MEDIUM)

**Problem:** When an autorun batch partially fails (some tasks complete, others fail/timeout), the only option is to re-run the entire batch. This wastes time re-executing already-completed tasks.

**Fix:** Added `codeflow autorun resume` subcommand that:
1. Queries the DB for the batch session's task runs
2. Filters to non-completed tasks (failed, skipped, timeout, blocked, cancelled)
3. Reads the original batch file to reconstruct config
4. Builds a new batch with only the filtered tasks, removing satisfied dependencies
5. Runs through the existing `run_batch` flow

**Files changed:**
- `codeflow-cli/cli/src/cmd/autorun.rs` -- Resume variant, `run_resume()` handler
- `codeflow-cli/core/src/autorun/batch.rs` -- `build_resume_batch()` function

## 4. LOW Severity Fixes

### 4a. Sync Daemon Auto-Start

**Problem:** Worker creates a worktree but does not check if the sync daemon should be started. The auto-stop call existed at cleanup but auto-start was missing.

**Fix:** Added `maybe_auto_start_daemon()` call after worktree setup in `worker.rs`, before claim acquisition.

### 4b. Dead Config Field

**Problem:** `stale_heartbeat_threshold_secs` in `parallel-work-config.json` has no corresponding field in `AutorunConfig`. It is silently ignored by serde.

**Fix:** Removed the dead field from the config file.

### 4c. Logs Follow Mode

**Problem:** Follow mode used byte-length comparison (`content.len() != last_len`) which could miss updates if tmux rewrote content at the same byte length, and would reprint the entire buffer on any change.

**Fix:** Switched to line-count tracking. Only prints new lines incrementally.

### 4d. Status Watch Mode

**Problem:** No way to continuously monitor autorun status without manually re-running the command.

**Fix:** Added `--watch` / `-w` flag and `--interval` (default 5s) to `codeflow autorun status`. Wraps `run_status()` in a loop with terminal clear.

### 4e. Batch Listing Command

**Problem:** No way to see available batch files without manually inspecting the directory.

**Fix:** Added `codeflow autorun batches` subcommand that scans `.codeflow/config/autorun/` for YAML files, parses each, and displays a summary table (tasks, target, auto_merge).
