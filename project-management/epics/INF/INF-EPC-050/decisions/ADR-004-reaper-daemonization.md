---
id: ADR-004
title: Reaper daemonization model
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Decides between long-running reaper daemon and on-demand reaper invocation for stale autorun session cleanup.
tags: [autorun, reaper, daemon, ops]
---

# ADR-004: Reaper Daemonization Model

## Status

Accepted.

## Context

The reaper (`autorun::sweep_stale_sessions`,
`codeflow-cli/core/src/autorun/stale.rs:784`) detects autorun sessions
whose worker PID is dead, whose tmux session is gone, or whose
`Aborting` state has timed out, and runs `cleanup_stale_session` on
each. It is currently invoked on-demand at two CLI entry points:

- `codeflow-cli/cli/src/cmd/autorun.rs:602` — at `codeflow autorun
  run` startup, before launching new workers.
- `codeflow-cli/cli/src/cmd/autorun.rs:4237` — at `codeflow autorun
  resume` startup, before resuming.

There is no periodic reaper. A stale session detected at run-N is
cleaned up no earlier than run-(N+1).

### Problem Statement

Section 4.4 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records:

> Currently the reaper runs on-demand at session start. Should there
> be a long-running reaper daemon that prunes stale state every N
> minutes? Trade-offs: detection lag (current = N minutes since last
> session start, daemon = N seconds), resource usage.

In practice the on-demand model has three observed failure modes:

1. **Stale worktree resource consumption.** A crashed worker that
   left its worktree in `PendingCleanup` state holds disk and a slot
   in `WorktreeRegistry::max_concurrent` until the next reaper run.
   On a busy host this may starve new batches.
2. **Orphaned claims.** CRDT claims held by a crashed worker remain
   until the next reaper run. Dead-worker cleanup
   (`sync::cleanup_dead_workers`) runs once per sync cycle inside the
   sync daemon, but it depends on the worktree registry being
   accurate, which the reaper updates.
3. **Stuck merge queue head.** A crashed worker whose merge queue
   entry was never dequeued blocks the queue until the next reaper
   run.

### Constraints

- The reaper writes to the SurrealDB and to the worktree registry;
  concurrent reapers must not corrupt state. The current code uses
  the DataStore's per-call locking but is not protected against two
  reapers running simultaneously on the same project.
- A long-running reaper daemon adds a third long-running process
  alongside the orchestrator (per-batch) and the sync daemon
  (cross-worktree).
- The codeflow-cli is a user-installed CLI; it does not assume
  systemd / launchd availability.
- The worktree registry is read by every CLI entry point;
  registry-mutating operations need to be quick.
- Sync daemon already exists with start/stop / status / supervision
  primitives that ADR-003 reuses. Adding a fourth lifecycle would
  triple the complexity.

### Assumptions

- Detection lag of "next CLI invocation" is acceptable for current
  workloads. The codeflow repo's typical pattern is ≤ 5 batches per
  workday; stale state lives at most one workday before the next
  invocation reaps it.
- Reaper runs are cheap (single DB query, a handful of file removes)
  — running them every 30 s is not a resource concern.
- The same operator who runs `codeflow autorun run` is responsible
  for noticing stale state in the TUI and intervening if the
  detection lag is unacceptable for a specific incident.

## Decision

**Keep the on-demand reaper. Add a periodic invocation arm to the
existing sync daemon.**

The sync daemon already runs as a long-lived process when
worktrees > 1. It has a sync loop (`run_sync_cycle`, called every
`config.sync.interval_secs`, default 5 s). The reaper is added as a
SECOND arm of that same loop, gated on a slower cadence:

```text
sync daemon main loop:
    every 5 s: run_sync_cycle (existing)
    every 60 s: sweep_stale_sessions (new)
```

The 60-second cadence is configured via
`config.autorun.reaper_interval_secs` in
`parallel-work-config.json` (default 60 s, validated range 30 s –
600 s). On-demand invocations at CLI entry points are kept for the
"daemon was not running" case.

The sync daemon does NOT escalate to running the reaper if the sync
daemon itself is not running (i.e., worktrees ≤ 1 → no daemon → no
periodic reaper, but the next CLI entry point reaps).

A new event type `ReaperPeriodicSweep { sessions_cleaned, errors }`
is emitted to `coordination-events.jsonl` after each periodic
invocation.

Concurrency: the reaper acquires a flock on
`.state/coordination/reaper.lock` before running; on-demand and
periodic invocations cannot stomp each other.

### Rationale

Running the reaper inside the sync daemon is the cheapest way to
add a periodic sweep. The sync daemon:

- already runs when `worktrees > 1` (the case where stale state has
  the most impact),
- already has a configured supervision strategy (ADR-003),
- already has signal-handler shutdown semantics,
- is the natural owner of cross-worktree consistency (which is
  what stale-session reaping affects).

A separate reaper daemon would duplicate every one of those
properties without changing the failure envelope. The sync daemon's
existing infrastructure is the right home.

The 60-second default is chosen because:

- 30 s is the supervision interval for the daemon itself (ADR-003);
  reaping faster than supervision would cause race conditions
  (reaper runs while supervisor is restarting the daemon).
- 600 s is too long — observed stale-worktree starvation in
  INF-EPC-024 batches happened at the 5-10 minute mark.
- The flock cost dominates the actual reaper work, so 60 s vs. 30 s
  cadence is a wash on resource use.

When the sync daemon is NOT running (worktree count ≤ 1, e.g., a
single interactive session), there is no periodic reaper. This is
intentional: a stale state in a single-worktree-only environment
self-heals at the next CLI entry point because the same process is
both producer and consumer of the registry.

## Alternatives Considered

### Option 1: Separate `codeflow reaper daemon` long-running process

A dedicated reaper daemon, parallel to the sync daemon.

**Pros:**

- Clear ownership boundary between sync (state propagation) and
  reaper (cleanup).
- Independent supervision allowed.

**Cons:**

- Three long-running processes (sync, reaper, orchestrator) with
  three lifecycles; correctness depends on all three being supervised.
- The reaper's data dependencies (DataStore, worktree registry,
  pathflow-events.jsonl) overlap heavily with the sync daemon's;
  splitting them is artificial.
- More install / start / stop surface.

**Why rejected:** the work duplicates sync daemon infrastructure
without addressing a real failure mode the sync daemon doesn't already
solve.

### Option 2: Stay on-demand only

Status quo — reap only at CLI entry points.

**Pros:**

- Simplest possible model; no daemon code changes.

**Cons:**

- Stale-state detection lag is unbounded (waits for next CLI
  invocation).
- Resource starvation observed in long-running batches.
- Doesn't address the audit-stated problem.

**Why rejected:** the audit explicitly flags this as a problem;
status quo is unacceptable.

### Option 3: Run reaper inside the orchestrator's supervision loop

Same as ADR-003's sync daemon supervision: orchestrator runs the
reaper periodically.

**Pros:**

- Reuses the orchestrator's existing main `select!` arm.
- No daemon changes.

**Cons:**

- The orchestrator only runs during a batch. Between batches, no
  reaper runs.
- Stale state from one batch would persist to the next batch's
  startup, exactly the failure mode we're trying to avoid.

**Why rejected:** the orchestrator's lifetime is too short to
catch the cross-batch stale-state cases.

### Option 4: Cron / launchd unit file

OS-level scheduler.

**Pros:**

- Mature scheduling.

**Cons:**

- Per-platform install required.
- Doesn't compose with worktree-count-driven start/stop.
- Out of scope for the user-mode CLI deployment.

**Why rejected:** same reason ADR-003 rejected option 3
(systemd/launchd) — wrong deployment model.

## Consequences

### Positive

- Stale-state detection lag bounded to 60 s during multi-worktree
  workloads (the case that needs it).
- Reuses sync daemon lifecycle, signal handling, and supervision.
- Single configurable knob (`reaper_interval_secs`) covers the
  cadence question.

### Negative

- Single-worktree (interactive-only) workloads continue to depend
  on next-CLI-invocation reaping. This is documented as a known
  limitation. Mitigation in practice: an operator running an
  interactive session over many hours could occasionally invoke
  `codeflow autorun status --watch` (which calls the reaper as a
  side effect of `sweep_stale_sessions`).
- A flock contention point is added to the reaper's hot path; a
  hung reaper invocation could block subsequent invocations until
  the lock is released. Mitigation: `flock` with `LOCK_NB` returns
  immediately if the lock is held, and the periodic invocation
  treats lock-held as "another invocation handled it, skip".

### Risks

- **Flock orphan.** A reaper crashed mid-cleanup could leave the
  flock held until the OS reclaims it on process exit. In practice
  Linux/macOS release file locks on process exit; this is not a
  real risk but is documented in case a future port lands on a
  filesystem that doesn't.
- **Concurrent on-demand + periodic invocation.** Two reapers
  trying to clean the same stale session would have raced on the
  current code. The flock fixes this; without the flock the
  periodic addition would be unsafe. The flock is therefore a
  hard requirement of this decision.

## Implementation

### Action Items

The follow-up implementation ticket
[INF-TSK-050-009](../tasks/INF-TSK-050-009.md) will:

- [ ] Add `reaper_interval_secs` to `parallel-work-config.json`'s
  `autorun` block (default 60, range 30–600).
- [ ] Add a flock helper to `autorun::stale` (acquire-or-skip
  semantics on `.state/coordination/reaper.lock`).
- [ ] Wrap `sweep_stale_sessions` and `sweep_stale_sessions_with`
  with the flock so existing on-demand callers also pick up the
  protection.
- [ ] Add a periodic reaper invocation to the sync daemon's main
  loop in `coordination/sync.rs`.
- [ ] Emit `ReaperPeriodicSweep` events to
  `coordination-events.jsonl`.
- [ ] Add unit tests: lock-not-held → reap, lock-held → skip,
  zero-stale → no-op.
- [ ] Add an integration test: simulate two periodic reapers with
  staggered start, assert exactly one runs.

### Timeline

Defer to the implementation ticket. ADR-004 is design-only.

## Related

- Follow-up implementation: [INF-TSK-050-009](../tasks/INF-TSK-050-009.md).
- Anchored on: `codeflow-cli/core/src/autorun/stale.rs:784-869`,
  `codeflow-cli/cli/src/cmd/autorun.rs:602`, `:4237`,
  `codeflow-cli/core/src/coordination/sync.rs` (sync daemon main
  loop).
- ADR-003 (sync daemon supervision) defines the supervisor that
  this ADR's periodic reaper depends on.

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.4.
- [`codeflow-cli/core/src/autorun/stale.rs`](../../../../../codeflow-cli/core/src/autorun/stale.rs).
- [`codeflow-cli/core/src/coordination/sync.rs`](../../../../../codeflow-cli/core/src/coordination/sync.rs).
- [`.codeflow/config/parallel-work/parallel-work-config.json`](../../../../../.codeflow/config/parallel-work/parallel-work-config.json).
