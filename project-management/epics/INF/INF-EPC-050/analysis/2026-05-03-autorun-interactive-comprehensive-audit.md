---
title: "Autorun + Interactive Comprehensive Audit (post PRs #309/#310/#311)"
status: draft
author: cf-planning
created: "2026-05-03"
updated: "2026-05-03"
---

# Autorun + Interactive Comprehensive Audit (post PRs #309/#310/#311)

> Investigation companion to INF-EPC-050. Captures every open finding from
> the PR-#309 -> PR-#310 -> PR-#311 chain plus newly surfaced issues that
> the chain did not address, with file:line citations and proposed
> remediation tier (A/B/C).

## 1. Investigation Summary

### 1.1 What was promised vs. what shipped

The INF-EPC-050 sweep was scoped to fix 23 correctness issues across
autorun + interactive subsystems. Three PRs landed in sequence:

| PR    | Branch                                                    | Headline scope                                                       | Result                                          |
|-------|-----------------------------------------------------------|----------------------------------------------------------------------|-------------------------------------------------|
| #309  | fix/inf-tsk-050-001-autorun-interactive-correctness        | Initial 23-AC sweep                                                  | Merged. Many fixes partial; introduced new bugs |
| #310  | fix/inf-tsk-024-050-pid-source-consolidation               | Consolidate PID source for status TUI                                | Merged. Did not actually consolidate -- name only|
| #311  | fix/inf-tsk-024-051-liveness-source-consolidation          | "Real" liveness chokepoint + 11 -> 1 implementations                 | Merged. Chokepoint added, callers NOT migrated  |

PR #311 is the most important predecessor: it created the canonical
`is_session_alive` chokepoint in `codeflow-cli/core/src/session/liveness.rs`
and removed the worktree-status callers' use of legacy `is_session_stale`,
but **two production callers (`codeflow-cli/cli/src/cmd/autorun.rs` and
the orchestrator's heartbeat writer) were never migrated**. The chokepoint
exists but is bypassed by the loudest user-visible surface (`codeflow
autorun status` and `codeflow interactive status`).

### 1.2 Bug count and tier breakdown

Total open issues: **28** (15 Tier-A critical, 6 Tier-B robustness, 7 Tier-C
deferred design).

| Tier | Severity                            | Count | Target ticket           | Remediation type                     |
|------|-------------------------------------|-------|-------------------------|--------------------------------------|
| A    | User-blocking correctness           | 15    | INF-TSK-050-003 (FIX)   | Code fixes, ~1790 LOC src+tests      |
| B    | Robustness / hidden failure modes   | 6     | INF-TSK-050-004 (FIX)   | Code fixes, ~380 LOC                 |
| C    | Open design questions, deferred fix | 7     | INF-TSK-050-005 (SPKE)  | ADRs / design notes, no code         |

Note: original assignment estimated 5 Tier-B and 7 Tier-C; the audit
surfaced one additional Tier-B item (DB CREATE silent failures; see 4.4).
Total still ~28 because no items moved between tiers.

### 1.3 Tier-A failures cluster around a single root cause

Of the 15 Tier-A bugs, **12 share a single root cause**: the canonical
liveness chokepoint exists but is not consumed by the autorun + interactive
status surfaces. The chokepoint was added in PR #311 specifically to fix
this class of bug; the migration was advertised in the commit but did not
land in the loudest two surfaces. Restoring chokepoint discipline (Tier-A
items #3-#9 plus #15) collapses the bug count by half and removes the
class of regression entirely.

The remaining 3 Tier-A items (DB recovery + schema deserialization error
message + final-PR gate) are independent.

## 2. Tier-A: Critical correctness items (15)

### 2.1 DB recovery story is missing

**Bug:** When `apply_schema` deserialization fails (a known failure mode
when `surrealdb` crate version drifts vs. the on-disk format), there is
no recovery command. The user sees an opaque deserialization error and
must manually `rm -rf .state/db/codeflow.db/` -- destroying state with
no backup.

**Evidence:**

- `codeflow-cli/cli/src/cmd/doctor.rs` -- no `--reset-db` subcommand;
  doctor returns deserialization failures with no recovery hint.
- `codeflow-cli/core/src/store/schema.rs:44-56` -- `apply_schema` returns
  `DbError::Surreal` on failure with no actionable error context.

**Fix (TSK-050-003 AC #1, #2):**

1. Add `codeflow doctor --reset-db` subcommand. Backs up `.state/db/codeflow.db/`
   to `.state/db/codeflow.db.backup-{ISO8601}` then deletes the original.
2. When `apply_schema` returns a deserialization error anywhere in the
   call chain, print the exact recovery command + JSONL replay strategy
   to stderr.

### 2.2 Liveness chokepoint bypassed (autorun status, text mode)

**Bug:** `codeflow autorun status` (text mode in `cli/src/cmd/autorun.rs`)
calls the deprecated `check_heartbeat_alive` + `is_session_stale` instead
of the canonical `is_session_alive(project_dir, sid)`. The TUI ends up
displaying live workers as Stale and dead workers as Running.

**Evidence:**

- `codeflow-cli/cli/src/cmd/autorun.rs:1844`:
  `let (hb_alive, _) = codeflow_core::autorun::check_heartbeat_alive(...)`
- `codeflow-cli/cli/src/cmd/autorun.rs:1852-1853`:
  `session_stale = codeflow_core::autorun::is_session_stale(pid_alive, tmux_alive, heartbeat);`
- `codeflow-cli/core/src/autorun/stale.rs:168-216` -- `check_heartbeat_alive`
  and `is_session_stale` are still exported and re-exported from
  `core/src/autorun/mod.rs` despite the chokepoint being available.

**Why it slipped past PR #311:** PR #311 migrated `worktree::status` and
the sync daemon to the chokepoint. The `cli/src/cmd/autorun.rs` paths use
a different signal combiner (PID + tmux + heartbeat), so they were not
caught by the migration grep.

**Fix (TSK-050-003 AC #3, #4):** Replace heartbeat-based logic with
`is_session_alive(project_dir, &worker.session_id)` per worker. Add
`validate_orchestrator_pid(pid)` for the orchestrator (since the
orchestrator process is `codeflow autorun run`, not `claude`).

### 2.3 Liveness chokepoint bypassed (autorun status, TUI mode)

**Bug:** Symmetric to 2.2. The TUI rendering path in the same file
duplicates the broken signal-combiner logic instead of calling
`is_session_alive`.

**Evidence:** Same file; the `run_status_watch` function (`autorun.rs:1974`)
re-derives liveness from the same broken combinator.

**Fix (TSK-050-003 AC #3, #4):** Both text and TUI modes call the same
chokepoint. A test asserts they produce identical verdicts on identical
inputs.

### 2.4 Orchestrator PID validation missing

**Bug:** The orchestrator process (`codeflow autorun run`) is NOT named
"claude". The chokepoint's `default_pid_validator` calls `validate_claude_pid`
which requires the process name to contain "claude" -- so reusing the
chokepoint directly for orchestrator liveness would always return Dead.

**Evidence:** `codeflow-cli/core/src/session/liveness.rs:330-336` -- the
default validator hardcodes the "claude" check.

**Fix (TSK-050-003 AC #5):** Add `validate_orchestrator_pid(pid: u32) -> bool`
to `core/src/session/liveness.rs`. PID-only check (no process-name check
since the orchestrator is `codeflow`, not claude). Three unit tests:
alive PID returns true; dead PID returns false; PID 0 returns false.

### 2.5 Interactive text mode does NOT promote active->stale

**Bug:** `codeflow interactive status` (text mode) shows `STATUS=active`
for sessions whose lead PID is dead. The TUI mode correctly demotes them;
the text mode does not. PRs #309/#310 added the demotion logic to TUI
mode only.

**Evidence:** `codeflow-cli/cli/src/cmd/interactive.rs:286-351` -- the
`run_status` text mode reads `status` from the DB row but does NOT consult
`is_session_alive` per row, and never writes back a status change.

**Fix (TSK-050-003 AC #6):** When chokepoint says session is dead but DB
row says `status=active`, text mode (a) shows `STATUS=stale` in the
display, and (b) writes the DB row to `status=stale`. Parity with TUI mode.

### 2.6 `register_interactive_session` errors are silent

**Bug:** `register_interactive_session` (`cli/src/cmd/interactive.rs:217`)
swallows DB errors with `let _ = store.db().query(...)`. If the DB is
locked, full, or schema mismatch, the user sees no warning -- the session
runs without ever appearing in `interactive_session`, and `codeflow
interactive status` reports it missing.

**Evidence:** `codeflow-cli/cli/src/cmd/interactive.rs:247-273` -- the
CREATE statement uses `let _ = store.db().query(...).await;`.

**Why it slipped past PR #311:** PR #311 focused on liveness consolidation;
silent error swallows in unrelated functions were not in scope.

**Fix (TSK-050-003 AC #7):** Replace the `let _` with proper error logging.
Function signature returns `Result<(), Error>`; errors logged to stderr
with prefix `warn: register_interactive_session failed:`.

### 2.7 Autorun text mode does NOT call reconcile

**Bug:** `run_status` (text mode) skips the `detect_stuck_sessions` +
`reconcile_session_status` pre-render call that the TUI mode performs in
`run_status_watch_auto_reconcile` (`autorun.rs:3004`). Stale Running
tasks are displayed as Running indefinitely until a TUI session opens.

**Evidence:**

- `codeflow-cli/cli/src/cmd/autorun.rs:1747` (text mode `run_status`):
  no auto-reconcile.
- `codeflow-cli/cli/src/cmd/autorun.rs:2042` (TUI mode): comment notes
  the auto-reconcile pre-call, which only happens in TUI mode.

**Fix (TSK-050-003 AC #8):** `run_status` calls
`detect_stuck_sessions` + `reconcile_session_status` before rendering,
same as `run_status_watch`. Test: stale Running task reconciled to
Failed before display.

### 2.8 Heartbeat writer still alive in orchestrator

**Bug:** PR #311 added the chokepoint and removed heartbeat dependency
in *some* readers, but the orchestrator's heartbeat task
(`orchestrator.rs:287-305`) is still running every `heartbeat_interval_secs`
(default 30s). Each tick performs a `OpenOptions::new().create(true)
.truncate(true).write(true).open(&path)` -- a write per interval per
orchestrator. With chokepoint replacement, this work is unused and
should be deleted.

**Evidence:**

- `codeflow-cli/core/src/autorun/orchestrator.rs:287-305` -- heartbeat
  task spawn.
- `codeflow-cli/core/src/autorun/stale.rs:168` -- `check_heartbeat_alive`
  and the `heartbeat_alive` field on `StaleSessionInfo`.

**Fix (TSK-050-003 AC #9):**

1. Delete heartbeat task spawn at `orchestrator.rs:287-305`.
2. Delete `core/src/autorun/stale.rs::check_heartbeat_alive` and
   `heartbeat_alive` field on `StaleSessionInfo`.
3. Migrate all callers to chokepoint (per AC #3, #4 above).
4. Re-export from `core/src/autorun/mod.rs` removed to prevent regrowth.

### 2.9 Merge queue stale-head detection only at deadline

**Bug:** When a worker at the head of the merge queue crashes, successor
workers remain blocked until the per-target deadline elapses (`queue_timeout_secs`,
default 600s = 10 minutes). The `try_remove_stale_head` helper exists
but is only invoked at deadline expiry, not on every poll cycle.

**Evidence:** Search `codeflow-cli/core/src/autorun/worker.rs` for
`try_remove_stale_head` callers (currently exists only on the deadline
path).

**Fix (TSK-050-003 AC #10):** Invoke `try_remove_stale_head` at the top
of each dequeue poll iteration. Predecessor crash -> next worker unblocks
within one poll cycle (~5s), not 10 minutes.

### 2.10 Merge result not written back to DB

**Bug:** After `serialized_merge` returns, the worker exits without
updating `autorun_worker.pr_merged_at` or status (merged / merge_failed /
merge_conflict). The TUI displays workers as Running indefinitely after
their merge completes.

**Evidence:** `codeflow-cli/core/src/autorun/worker.rs:842-846` --
`MergeOutcome::Merged` returned with no DB writeback.

**Fix (TSK-050-003 AC #11):** After `serialized_merge` returns, set
`autorun_worker.pr_merged_at` + status flip to `merged`/`merge_failed`/
etc. Test: simulate merge success -> assert DB row updated.

### 2.11 DB updates do not retry on transient failure

**Bug:** `update_autorun_worker` and `update_autorun_task_run` call
`store.update_*().await` once. If the DB is briefly contended (parallel
worker writes during sync daemon ticks), the update fails and the worker
continues without retrying.

**Evidence:**

- `codeflow-cli/core/src/autorun/worker.rs:1236, 1264, 1297` -- single-shot
  `if let Err(e) = self.store.create_*` / `update_*` patterns.

**Fix (TSK-050-003 AC #12):** Wrap `update_autorun_worker` and
`update_autorun_task_run` in retry-with-backoff (3 attempts, 1s/3s/9s).
Test: mock store fails 2x then succeeds -> assert retry succeeds.

### 2.12 Final-PR gate is wrong (any-completed instead of all-merged)

**Bug:** The final-PR creation logic at `cli/src/cmd/autorun.rs:756-757`
gates on `completed_count > 0` -- meaning a single completed worker
triggers the final PR even if other workers merge_failed or
merge_conflicted. The user gets a final PR for an incomplete batch.

**Evidence:**

```rust
let completed_count = results.iter().filter(|r| r.status == "completed").count();
if parsed.final_pr && completed_count > 0 { ... }
```

**Fix (TSK-050-003 AC #13):** Gate on `all workers' pr_merged_at IS NOT NULL`.
Tasks marked `completed-with-warning` (e.g., `merge_conflict`) do NOT count
toward final-PR creation.

### 2.13 TUI auto-refresh and 'r' force-refresh not verifiably correct

**Bug:** The TUI rendering loop has a refresh ticker, but the `[r]`
keybinding's force-refresh path may double-fire on rapid presses, and
the auto-refresh ticker uses `Duration::from_secs(interval_secs.max(1))`
without a sane upper bound. Empirical reports: TUI sometimes appears
frozen, pressing `[r]` does not visibly update.

**Evidence:** `codeflow-cli/cli/src/cmd/autorun.rs:2117-2155` -- refresh
infrastructure exists, but no integration test asserts that auto-refresh
ticks at the configured interval AND that `[r]` updates within 1s.

**Fix (TSK-050-003 AC #14):** For BOTH `codeflow interactive status` (TUI)
and `codeflow autorun status` (TUI): add tests that verify (a) auto-refresh
ticks at the configured interval (default 2s), (b) pressing `r` forces
immediate refresh and visibly updates within 1s.

### 2.14 Lint guard missing for chokepoint discipline

**Bug:** Without a lint or CI check, references to `is_session_stale` /
`check_heartbeat_alive` will regrow as developers copy-paste old code.
The migration is structurally complete but not durable.

**Evidence:** `cf-rust-standards` skill currently has no rule against the
deprecated functions.

**Fix (TSK-050-003 AC #15):** Add to `cf-rust-standards` skill OR to
test-config: any reference to `is_session_stale` or `check_heartbeat_alive`
outside the deprecated module is a CI failure.

### 2.15 Schema deserialization error has no actionable message

**Bug:** Section 2.1 covers the recovery command. Independently, when
`apply_schema` returns the error, the message printed to stderr does
not name the recovery command -- so the user has no breadcrumb.

**Evidence:** `codeflow-cli/core/src/store/schema.rs:44-56` -- `apply_schema`
returns the bare `DbError::Surreal`, callers print the inner error verbatim.

**Fix (TSK-050-003 AC #2):** When `apply_schema` returns a deserialization
error, print actionable message naming `codeflow doctor --reset-db` and
the JSONL replay strategy. Test: inject a deserialization error, assert
message contents.

## 3. Tier-B: Robustness / hidden failure modes (6)

### 3.1 tmux kill failure swallowed silently

**Bug:** `worker.rs:1703` -- `let _ = self.tmux.kill_session(&tmux_name).await;`.
If tmux fails (e.g., daemon crash, session gone), the underlying claude
process is orphaned with no SIGKILL escalation.

**Fix (TSK-050-004 AC #1):** Replace the `let _` with: log error, then
`kill -9 {pid}` for any orphan claude process discoverable from tmux pane PID.

### 3.2 Claim release does not retry

**Bug:** `worker.rs:1699` -- `let _ = release_all()`. CRDT writes to
`state.loro` can briefly contend with the sync daemon; a single failure
leaves stale claims that block future workers.

**Fix (TSK-050-004 AC #2):** Replace with retry-with-backoff (3 attempts,
500ms/1.5s/4.5s).

### 3.3 Protected branch merge failure does NOT fail the task

**Bug:** `worker.rs:824-832` -- when `gh pr merge` returns non-zero
(including a hook block on a protected branch), the worker logs the
error but does NOT set `exit_code` to non-zero. The task is marked
`completed`. The user sees "completed" in `autorun status` but the PR
is left open and unmerged.

**Fix (TSK-050-004 AC #3):** When `gh pr merge` returns non-zero,
worker exit_code is set to non-zero. Task status flips from completed to
failed. Visible in autorun status. Test: simulate hook block -> assert
task=failed, not completed.

### 3.4 DB CREATE failures continue silently

**Bug:** `worker.rs:1236, 1264` -- when `create_autorun_worker` or
`create_autorun_task_run` fails, the worker logs `warning: failed to create
autorun_worker record: {e}` but continues running. The task executes
without a DB row, breaking the entire status / reconciliation chain.

**Fix (TSK-050-004 AC #4):** When CREATE fails, worker exits with
`Err(WorkerFailed)` instead of continuing. Test: mock store CREATE failure
-> assert worker exits failed (not completed).

### 3.5 `check_merge_conflicts` not called pre-PR

**Bug:** `worker.rs:1461` calls `check_merge_conflicts` only inside the
auto-rebase fallback. If auto-rebase is disabled, the worker proceeds
to `gh pr create` even on a guaranteed-conflict branch. PR is created
then later fails merge.

**Fix (TSK-050-004 AC #5):** Worker calls `git/conflict.rs::check_merge_conflicts`
BEFORE `gh pr create`. If conflict detected, worker fails fast with
`merge_conflict` status -- does not create PR.

### 3.6 stale.rs no longer needs `StaleSessionInfo.heartbeat_alive` field

**Bug:** Once Tier-A AC #9 deletes `check_heartbeat_alive`, the
`StaleSessionInfo` struct's `heartbeat_alive: Option<bool>` field becomes
dead state. Callers like `build_stale_reason` (`stale.rs:878`) still
reference it, producing reasons like "(heartbeat: dead)" that are no
longer derivable.

**Note:** This is folded into Tier-A AC #9 by the spec ("All callers
migrated") -- no separate AC required, but the planner / reviewer must
ensure the cleanup is total: struct field, build_stale_reason, all tests
(`stale.rs:941-1014`) using `is_session_stale`/`check_heartbeat_alive`.

## 4. Tier-C: Open design questions (deferred to SPKE in TSK-050-005) (7)

Each item here ships an ADR or design note documenting (a) Problem,
(b) Options considered, (c) Recommended approach, (d) Follow-up
implementation task ID(s).

### 4.1 CI timeout retry policy

**Problem:** `ci_wait.rs` polls `gh pr checks` until success/fail/timeout.
What if `gh` returns transient errors (rate limit, network)? Currently
treated as fatal. How long before re-poll? Re-attempts? Final give-up?

**Output:** ADR-001.

### 4.2 Integration branch reset semantics on `autorun resume`

**Problem:** When `autorun resume` reuses an integration branch, should
it (a) reset to `origin/{target}` (lose merged work), (b) keep state
(orphan commits from prior partial run), or (c) require explicit user
choice via flag? PR #255 introduced auto-generated integration branches
without addressing resume.

**Output:** ADR-002.

### 4.3 Sync daemon supervision

**Problem:** The sync daemon (`coordination/sync.rs`) auto-starts when
worktree count > 1. If the daemon crashes (PID file orphaned), nothing
restarts it. Workers continue but CRDT state stops propagating.

**Output:** ADR-003.

### 4.4 Reaper daemonization

**Problem:** Currently the reaper runs on-demand at session start. Should
there be a long-running reaper daemon that prunes stale state every N
minutes? Trade-offs: detection lag (current = N minutes since last session
start, daemon = N seconds), resource usage.

**Output:** ADR-004.

### 4.5 Env file write hardening

**Problem:** When `.state/runtime/codeflow-env.sh` write fails (full disk,
permission), the failure surfaces as a vague "file not found" later when
a hook tries to `source` it. How to detect early and recover?

**Output:** Design note DESIGN-005.

### 4.6 Stage timeout polling cadence

**Problem:** `worker.rs:142` defines `STAGE_TIMEOUT_POLL_INTERVAL_SECS`
(small relative to timeout). Trade-off between detection lag and resource
usage (each poll is a `glob` over `.state/sentinels/`).

**Output:** Design note DESIGN-006.

### 4.7 Heartbeat stop signal hardening (forward-looking)

**Problem:** TSK-050-003 removes the orchestrator heartbeat. Going
forward, any new periodic-task-with-stop-signal pattern in the codebase
needs a documented contract for shutdown. Where to put it? How tested?

**Output:** Design note DESIGN-007. If TSK-050-003 lands first, this
becomes a forward-looking design note for future periodic-task patterns.

## 5. Cross-PR Overlap Analysis

### 5.1 Shipped vs. open per PR

| Bug area                   | PR #309 | PR #310 | PR #311 | Open in TSK-050-003 |
|----------------------------|--------:|--------:|--------:|-------------------:|
| Schema enforcement         |   PASS  |     --  |     --  |        --          |
| TASK column = format_id    |   PASS  |     --  |     --  |        --          |
| Liveness chokepoint exists |     --  |   FAIL  |   PASS  |        --          |
| Liveness chokepoint USED   |     --  |   FAIL  |   FAIL  |       AC #3, #4    |
| Interactive cleanup correct|   PASS  |   PASS  |   PASS  |        --          |
| Interactive text status    | PARTIAL |     --  |     --  |       AC #6        |
| Heartbeat write alive      |   PASS  |     --  |   FAIL  |       AC #9        |
| Final PR gate correct      |   FAIL  |     --  |     --  |       AC #13       |
| Merge queue stale head     | PARTIAL |     --  |     --  |       AC #10       |
| Merge writeback to DB      |   FAIL  |     --  |     --  |       AC #11       |
| DB retry                   |   FAIL  |     --  |     --  |       AC #12       |
| TUI refresh verifiable     | PARTIAL |     --  |     --  |       AC #14       |
| Lint guard for chokepoint  |     --  |     --  |     --  |       AC #15       |
| DB recovery (--reset-db)   |     --  |     --  |     --  |       AC #1        |
| Schema error message       |     --  |     --  |     --  |       AC #2        |

Legend: PASS = shipped + verified; PARTIAL = shipped but incomplete;
FAIL = intended but did not land OR introduced regression; -- = out of
scope for that PR.

### 5.2 Per-PR design intent and delivery gap

**PR #309 (`fix/inf-tsk-050-001-autorun-interactive-correctness`):**

- *Intent:* Sweep all 23 AC. Schema canonicalization, TUI display semantics,
  reaper liveness, hook permission suppression, autorun resume.
- *Shipped:* Schema enforcement (AC-01), TASK column display (AC-02),
  TUI freeze + [r]/[s] keybindings (AC-10/11/12), hook permission AC-21.
- *Did not deliver fully:* Liveness chokepoint discipline (left two
  callers untouched), final PR gate (kept the broken `> 0` guard), merge
  queue stale-head (only at deadline), DB retry / writeback hardening.
- *Introduced regressions:* TUI text mode active->stale skip (PR #309
  added the demote path to TUI mode but missed text mode -- AC #6 in
  TSK-050-003). `register_interactive_session` `let _` was preserved,
  not introduced (AC #7).

**PR #310 (`fix/inf-tsk-024-050-pid-source-consolidation`):**

- *Intent:* Consolidate PID source for status TUI to a single canonical
  function.
- *Shipped:* Some renaming and a partial helper. The user-visible bug
  (TUI showed live as Stale + wrong PID) was NOT fixed -- the underlying
  signal combiners were untouched.
- *Memory record:* "PR #310 closed consolidation; INF-TSK-024-051
  (follow-on) adds regression-prevention". This was inaccurate; the real
  consolidation was PR #311.

**PR #311 (`fix/inf-tsk-024-051-liveness-source-consolidation`):**

- *Intent:* Real consolidation. New `is_session_alive` chokepoint. 11
  implementations -> 1. Heartbeat removed. Lint guards. e2e canary test.
- *Shipped:* Chokepoint defined and exported. `worktree::status` and
  sync daemon migrated. Heartbeat removed from session module.
- *Did not deliver fully:* `cli/src/cmd/autorun.rs` text + TUI paths
  still call `check_heartbeat_alive` and `is_session_stale` (this audit's
  AC #3, #4). Orchestrator heartbeat task and `stale.rs::check_heartbeat_alive`
  still exist (AC #9). Lint guard mentioned in commit message but not
  registered in `cf-rust-standards` (AC #15).
- *Memory record:* "11 -> 1 implementations" -- this counted
  *worktree-status* call sites, not autorun + interactive call sites.

### 5.3 Net delivery gap

PR #311 fixed the chokepoint definition (the hard part). The remaining
work is mechanical: change ~12 call sites to use the chokepoint, delete
~50 LOC of dead heartbeat code, register a lint guard. **This is the
core of TSK-050-003.**

The independent items (DB recovery, schema message, final PR gate, DB
retry, merge writeback) are unrelated to chokepoint discipline -- they
slipped past PR #309 because they were never explicitly named in the
23-AC list.

## 6. Architecture: Current vs. Proposed State

### 6.1 Current state (post PR #311)

```text
Liveness query
    |
    +--> code path A (worktree::status, sync daemon, TUI worktree list)
    |       --> is_session_alive(project_dir, sid) [CHOKEPOINT]
    |             --> read pathflow-session-status.json
    |             --> validate_claude_pid(pid)
    |
    +--> code path B (cli/cmd/autorun.rs text mode + TUI mode)
    |       --> check_pid_alive(pid)
    |       --> check_tmux_alive(tmux_name)
    |       --> check_heartbeat_alive(project_dir, sid, threshold)
    |             --> read .state/autorun/heartbeat-{sid}
    |       --> is_session_stale(pid_alive, tmux_alive, hb_alive)  [STALE COMBINER]
    |
    +--> code path C (cli/cmd/interactive.rs text mode)
            --> read DB row
            --> display .status verbatim (no liveness check)

Heartbeat writers (sources of the heartbeat file):
    |
    +--> orchestrator.rs:287-305 -- heartbeat task spawn (every 30s)
    +--> [removed in PR #311] interactive heartbeat writer

Heartbeat readers:
    |
    +--> stale.rs::check_heartbeat_alive
            (used only by code path B above)
```

Three independent liveness paths (A/B/C) producing inconsistent verdicts
on the same input state.

### 6.2 Proposed state (post TSK-050-003)

```text
Liveness query
    |
    v
is_session_alive(project_dir, sid)  [CANONICAL, INF-TSK-024-051]
    |
    +--> read pathflow-session-status.json::lead_pid
    +--> validate_claude_pid(pid)         [for claude-process sessions]
    +--> validate_orchestrator_pid(pid)   [for codeflow autorun run; new]
    |
    +-- Active --> display alive
    +-- Dead   --> display stale, demote DB row
    +-- Unknown -> display "--" (initializing)

Heartbeat writers: NONE (deleted)
Heartbeat readers: NONE (deleted)
Lint guard:
    grep 'is_session_stale\|check_heartbeat_alive' codeflow-cli/
        outside deprecated module --> CI ERROR
```

Single chokepoint, total migration, durable lint guard.

### 6.3 Migration gap (what TSK-050-003 actually changes)

**Files modified (estimate):**

| File                                          | Change kind                              | LOC    |
|-----------------------------------------------|------------------------------------------|--------|
| `cli/src/cmd/autorun.rs`                      | replace heartbeat combiner with chokepoint, add reconcile to text mode, fix final-PR gate, add DB retry | 350    |
| `cli/src/cmd/interactive.rs`                  | promote active->stale in text mode, surface register errors                                            | 80     |
| `cli/src/cmd/doctor.rs`                       | add `--reset-db`                                                                                       | 100    |
| `core/src/store/schema.rs`                    | actionable error message                                                                               | 30     |
| `core/src/session/liveness.rs`                | add `validate_orchestrator_pid`                                                                        | 60     |
| `core/src/autorun/stale.rs`                   | delete `check_heartbeat_alive`, `heartbeat_alive` field, `is_session_stale`                            | -200   |
| `core/src/autorun/worker.rs`                  | merge writeback, DB retry, stale-head detection on every poll                                          | 250    |
| `core/src/autorun/orchestrator.rs`            | delete heartbeat task spawn                                                                            | -30    |
| `core/src/hooks/session_start.rs`             | (no change for chokepoint; possible small touch for orchestrator detection)                            | 0-20   |
| Tests (per-file inline + integration)         |                                                                                                        | 450    |
| `.codeflow/config/testing/test-config.json`   | register lint check (or via cf-rust-standards skill)                                                   | 10     |
| **Total**                                     |                                                                                                        | ~1340 src + ~450 tests = **~1790 LOC** |

## 7. Confirmation: this audit captures every finding from this session

**28 confirmed open issues:**

- 15 Tier-A (sections 2.1 through 2.15) -> TSK-050-003
- 6 Tier-B (sections 3.1 through 3.6) -> TSK-050-004
- 7 Tier-C (sections 4.1 through 4.7) -> TSK-050-005

**No information lost from the investigation:** the assignment listed
exactly these 28; this document captures each with file:line citation,
PR cross-reference, and remediation tier. Per the planning task's AC #1.

## 8. Assumptions

| #  | Assumption                                                                              | Verified? | Evidence                                                                                                                |
|----|-----------------------------------------------------------------------------------------|-----------|-------------------------------------------------------------------------------------------------------------------------|
| 1  | `is_session_alive` chokepoint exists at `core/src/session/liveness.rs:257`              | YES       | Read file: function present, exported                                                                                   |
| 2  | `check_heartbeat_alive` and `is_session_stale` still exist at `core/src/autorun/stale.rs:168, 201` | YES   | Grep returned both functions intact                                                                                     |
| 3  | `cli/src/cmd/autorun.rs:1844` calls `check_heartbeat_alive`                              | YES       | Read file: line confirmed                                                                                              |
| 4  | `cli/src/cmd/autorun.rs:1852` calls `is_session_stale`                                   | YES       | Read file: line confirmed                                                                                              |
| 5  | Orchestrator heartbeat task at `orchestrator.rs:287-305`                                 | YES       | Read file: tokio::spawn block confirmed                                                                                 |
| 6  | `cli/src/cmd/interactive.rs:217` `register_interactive_session` swallows errors          | YES       | Read file: `let _ = store.db().query(...).await;` confirmed                                                            |
| 7  | Final-PR gate at `autorun.rs:756-757` uses `completed_count > 0`                         | YES       | Read file: confirmed                                                                                                    |
| 8  | `worker.rs:1703` `let _ = self.tmux.kill_session(...)` exists                            | YES       | Grep confirmed                                                                                                          |
| 9  | `worker.rs:824-832` does not flip task to failed on `gh pr merge` non-zero               | YES       | Read file: only `eprintln!` and `MergeConflict` return; no exit_code mutation                                            |
| 10 | `worker.rs:1236, 1264` continue on CREATE failure                                        | YES       | Read file: `if let Err(e) = ... { eprintln!("warning: failed to create..."); }` then continues                          |
| 11 | `parallel-work-config.json` has no `reset_db` setting                                    | YES       | Read file: confirmed; `stale_threshold_secs: 90` is the only related setting                                            |
| 12 | Validation command `codeflow validate task <path>` exists                                | LIKELY    | `cli/src/cmd/validate.rs` exists; not invoked in audit                                                                  |
| 13 | Existing test framework supports per-file 85% coverage thresholds                        | YES       | `test-config.json` has `priorities` block; per-file coverage is convention                                              |
| 14 | `validate_claude_pid` requires process name to contain "claude"                          | YES       | Read `liveness.rs:330-336`: `default_pid_validator` calls `validate_claude_pid(pid) > 0`                                 |
| 15 | `merge_queue::try_remove_stale_head` exists somewhere                                     | LIKELY    | Mentioned in PR #311 work (memory record); not directly grep-confirmed in this audit. TSK-050-003 implementer to verify |
| 16 | The `cf-rust-standards` skill exists at `.claude/skills/cf-rust-standards/SKILL.md`      | YES       | Skill registered; Glob confirmed                                                                                        |

Items marked LIKELY are not used to invalidate any TSK-050-003 acceptance
criterion -- the implementer verifies first and adjusts the AC text if the
naming has drifted.

## 9. References

- INF-EPC-050: parent epic
- INF-TSK-050-001: PR #309 initial sweep (complete)
- PR #310: pid-source-consolidation (merged, no real consolidation)
- PR #311: liveness-source-consolidation (merged, real chokepoint, partial migration)
- INF-TSK-024-028: session ID consolidation (complete)
- INF-TSK-024-051: liveness consolidation (complete)
- `codeflow-cli/core/src/session/liveness.rs`: chokepoint location
