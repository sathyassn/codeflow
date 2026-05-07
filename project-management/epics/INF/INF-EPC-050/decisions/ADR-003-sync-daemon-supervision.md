---
id: ADR-003
title: Sync daemon supervision strategy
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Defines who detects sync daemon death and who restarts it when worktrees are still active.
tags: [autorun, sync, daemon, coordination, ops]
---

# ADR-003: Sync Daemon Supervision Strategy

## Status

Accepted.

## Context

The CRDT sync daemon (`codeflow-cli/core/src/coordination/sync.rs`)
propagates state.loro deltas between worktrees via git-ref transport.
It auto-starts when the worktree count exceeds 1 and auto-stops when
the count drops to 1 or 0. The daemon writes its PID to
`.state/coordination/sync-daemon.pid` and exits gracefully on SIGTERM
via the `register_signal_handler` shim at `sync.rs:760`.

### Problem Statement

Section 4.3 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records:

> The sync daemon (`coordination/sync.rs`) auto-starts when worktree
> count > 1. If the daemon crashes (PID file orphaned), nothing
> restarts it. Workers continue but CRDT state stops propagating.

`daemon_status` (`sync.rs:672`) detects death cleanly — it reads the
PID file, calls `is_pid_alive`, and reports `running: false` when the
PID is gone. `start_daemon` (`sync.rs:702`) is idempotent: if the
existing PID is alive, it returns the existing PID; if dead, it
removes the stale file and spawns a new instance. The PRIMITIVES for
detection and restart already exist.

What is missing is a CALLER: nothing currently invokes
`daemon_status` periodically and reacts to a `running: false` verdict
when there are still active worktrees. Without that caller, a crashed
daemon stays dead until the next CLI invocation that calls
`maybe_auto_start_daemon` (e.g., the next `codeflow autorun resume`
or the next `codeflow -i` session start).

The consequences of an undetected dead daemon are:

- Claims acquired in worktree A do not propagate to worktree B; both
  may believe they hold a claim on the same path → silent CRDT-merge
  conflict on the next sync.
- Merge queue position changes do not propagate; stale heads accumulate.
- Worker liveness changes do not propagate to the orchestrator host;
  the reaper sees a "live" PID that is actually dead in another
  worktree.

### Constraints

- Each autorun worker process is short-lived (minutes to ~2 hours);
  embedding the supervisor in the worker is fragile.
- The orchestrator is similarly bounded by the batch lifetime.
- Long-running `codeflow -i` sessions are interactive — adding a
  background supervision thread to every Claude session is wasteful
  when only autorun batches need cross-worktree sync.
- The daemon already runs as a detached child process (`stdin/stdout/stderr`
  set to `Stdio::null()`); there is no parent process to reattach to
  on crash.
- A separate "supervisor of the supervisor" introduces a turtles-all-
  the-way-down problem.

### Assumptions

- Sync daemon crashes are rare (the production daemon ran for the
  entire INF-EPC-024 batches without observed crash).
- A 30-second detection window is acceptable. Longer than 60 seconds
  starts to approach the merge queue's worst-case-rebase window.
- A restart attempt that itself fails (spawn error) should be visible
  to the operator and recorded as an event, but should not crash the
  process that observed it.

## Decision

**The orchestrator is the supervisor.**

`autorun/orchestrator.rs` already calls `maybe_auto_start_daemon`
before execution and `maybe_auto_stop_daemon` after. Extend the
orchestrator's main work loop with a periodic check, structured as:

```text
loop {
    select! {
        _ = work_cycle_done => { ... normal task dispatch ... }
        _ = sleep(SUPERVISION_INTERVAL) => {
            if active_worktree_count() > 1 {
                let status = sync::daemon_status(&config);
                if !status.running {
                    record event: sync_daemon_died { last_observed_pid }
                    match sync::start_daemon(&config) {
                        Ok(new_pid) => record event: sync_daemon_restarted { new_pid }
                        Err(e) => {
                            record event: sync_daemon_restart_failed { error }
                            // Continue work; do not crash.
                        }
                    }
                }
            }
        }
    }
}
```

Numeric defaults:

| Knob | Default | Rationale |
|------|---------|-----------|
| `SUPERVISION_INTERVAL` | 30 s | Long enough to avoid hot loop; short enough that the worst-case dead-daemon window is half a sync interval. |
| Restart attempts | unbounded | Each failure emits an event; the orchestrator does not abandon supervision. |
| Cooldown after restart | 5 s | Prevents tight respawn loop if the daemon binary is broken. |

Interactive `codeflow -i` sessions do NOT supervise. Their
`maybe_auto_start_daemon` call at SessionStart and
`maybe_auto_stop_daemon` at SessionEnd handle the start/stop; they
delegate cross-session supervision to whichever orchestrator is
running concurrently. If no orchestrator is running, sync events are
not load-bearing for an interactive-only workload (no autorun workers,
no merge queue activity), so a crashed daemon during an
interactive-only session is not an immediate correctness problem; it
self-heals at the next `codeflow autorun run` or at the next
`codeflow -i` session start.

A new event type `SyncDaemonDied` / `SyncDaemonRestarted` /
`SyncDaemonRestartFailed` is added to
`coordination::types::events::CoordinationEvent` and is emitted by
the supervisor.

### Rationale

The orchestrator is already long-lived for a batch (the
`run_resume` and `execute_with_batch_file` functions hold their
runtime for the full batch duration), it already starts and stops
the daemon, and it already has access to `worktree::read_registry`
to count active worktrees. Adding a supervision arm to its main
`select!` is a mechanical change with bounded scope.

A separate supervisor process would solve no actual problem — the
orchestrator's lifetime is exactly the lifetime of the work that
needs the daemon. When the orchestrator exits, the daemon should
also exit (and `maybe_auto_stop_daemon` already enforces this).

The unbounded-retry policy is the right default because the daemon's
spawn failure modes are operator-fixable (PATH, permissions, binary
absent). Surfacing every failure as an event lets the operator see
exactly what is happening; bounding retries would require
choosing between "give up too soon" and "give up too late" with no
clear signal for either.

## Alternatives Considered

### Option 1: Self-supervision (the daemon supervises itself)

Have each daemon spawn a watchdog child that re-execs the daemon
binary on parent exit.

**Pros:**

- Independent of orchestrator lifetime — works for interactive-only
  workloads too.

**Cons:**

- The watchdog child is itself a process that can die (turtles).
- Doubles process count.
- Adds complexity to the daemon itself, which today has clean
  shutdown semantics; a watchdog complicates SIGTERM handling.
- Doesn't survive a host restart, so it doesn't actually solve the
  "daemon must always be alive when worktrees > 1" property.

**Why rejected:** the property "alive when worktrees > 1" is already
guaranteed by `maybe_auto_start_daemon` at every CLI entry point;
self-supervision adds machinery without changing the failure
envelope.

### Option 2: Separate `codeflow sync supervisor` daemon

Long-running supervisor that runs even outside batch execution.

**Pros:**

- Catches crashes during interactive-only workloads too.

**Cons:**

- Yet another long-running process.
- Requires its own start/stop / install workflow (launchd/systemd?).
- Adds a third party to the lifecycle (sync daemon + supervisor +
  consumer); failure modes multiply.

**Why rejected:** out of scope for the codeflow-cli's deployment
model. Could be revisited if interactive-heavy workloads become
common.

### Option 3: SystemD/launchd unit file

Delegate supervision to the OS service manager.

**Pros:**

- Mature, battle-tested supervision.
- Survives host restart.

**Cons:**

- Requires per-platform install (systemd Linux, launchd macOS,
  Windows Service Manager Windows).
- Pulls codeflow-cli from a "user-mode CLI" into a "daemon you
  install" — major change in deployment story.
- Coupling sync daemon lifetime to a system-managed unit fights the
  "auto-start when worktrees > 1, auto-stop otherwise" property.

**Why rejected:** changes the deployment model unacceptably for what
is in practice a rare-failure path.

## Consequences

### Positive

- The longest gap between a crashed daemon and a restart is bounded
  by `SUPERVISION_INTERVAL` (30 s default) for autorun batches.
- Three new structured events make the failure mode debuggable.
- No new processes, no new install steps, no new flags.

### Negative

- Interactive-only workloads remain susceptible to the original bug
  (crashed daemon, no restart until next CLI entry point). This is
  documented as a known limitation; mitigation is "run an autorun
  batch concurrently" or "restart your interactive session", neither
  of which is graceful.
- The orchestrator's main `select!` gains a new arm, increasing
  test surface for the orchestrator.

### Risks

- **Cascading restart loop.** If the daemon binary is broken
  (e.g., dynamic library missing), the supervisor would restart it
  every 30 s. Mitigation: every restart is logged; an operator
  noticing 5+ `SyncDaemonRestartFailed` events in a single
  session can intervene. A future enhancement (out of scope here)
  could add an exponential-backoff cooldown.
- **Supervisor itself crashes with the orchestrator.** If the
  orchestrator dies in a way that does not run the
  `maybe_auto_stop_daemon` cleanup (SIGKILL), the daemon survives
  the orchestrator and is then unsupervised. Mitigation: the next
  CLI entry point (`maybe_auto_start_daemon` at SessionStart, or
  the next `codeflow autorun run`) restores supervision, and the
  daemon's own `cleanup_dead_workers` (`sync.rs:791`) self-heals
  the claims layer.

## Implementation

### Action Items

The follow-up implementation ticket
[INF-TSK-050-008](../tasks/INF-TSK-050-008.md) will:

- [ ] Add `supervise_sync_daemon()` async helper in
  `autorun/orchestrator.rs`.
- [ ] Wire the helper into the orchestrator's main `select!`
  alongside the existing work-dispatch arm.
- [ ] Add `SyncDaemonDied`, `SyncDaemonRestarted`,
  `SyncDaemonRestartFailed` variants to
  `coordination::types::events::CoordinationEvent`.
- [ ] Emit those events to `coordination-events.jsonl` via
  `ledger::routing`.
- [ ] Add unit tests for the supervisor decision (alive +
  worktrees>1 → no-op, dead + worktrees>1 → restart, alive +
  worktrees=1 → no-op).
- [ ] Document the 30 s detection window in
  `parallel-work-config.json` schema notes.

### Timeline

Defer to the implementation ticket. ADR-003 is design-only.

## Related

- Follow-up implementation: [INF-TSK-050-008](../tasks/INF-TSK-050-008.md).
- Anchored on: `codeflow-cli/core/src/coordination/sync.rs:672-749`,
  `:760-777`, `:791-849`,
  `codeflow-cli/core/src/autorun/orchestrator.rs` (orchestrator
  lifecycle).
- ADR-002 (integration branch resume) is orthogonal but shares the
  orchestrator-as-coordinator pattern.

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.3.
- [`codeflow-cli/core/src/coordination/sync.rs`](../../../../../codeflow-cli/core/src/coordination/sync.rs).
- [`codeflow-cli/core/src/autorun/orchestrator.rs`](../../../../../codeflow-cli/core/src/autorun/orchestrator.rs).
