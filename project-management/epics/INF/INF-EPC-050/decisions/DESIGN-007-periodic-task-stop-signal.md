---
id: DESIGN-007
title: Stop-signal contract for periodic background tasks
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Defines the contract for any future periodic-tokio-task-with-stop-signal pattern in the codeflow-cli codebase.
tags: [autorun, sync, periodic-task, async, ops]
---

# DESIGN-007: Stop-Signal Contract for Periodic Background Tasks

## Status

Accepted.

## Context

INF-TSK-050-003 removed the orchestrator-side heartbeat task — a
periodic tokio task that wrote `last_heartbeat_at` on the autorun
session row every 30 seconds. The removal was driven by the
canonical liveness chokepoint replacing heartbeat as a signal
source. With it went a known stop-signal pattern: a `oneshot`
channel that the orchestrator dropped at shutdown, with the task
selecting on `tokio::select!{ _ = ticker.tick() => …, _ =
&mut stop_rx => break }`.

Going forward, similar patterns will recur:

- Sync daemon supervision (ADR-003) — orchestrator periodic check.
- Periodic reaper invocation (ADR-004) — sync daemon periodic
  cleanup.
- Worker heartbeat updater at `worker.rs:1686-1725` — still in
  place, writing `last_heartbeat_at` every 10 s on the autorun
  session row.
- Future telemetry / metrics emitters.

Without a documented contract, each new periodic task reinvents
the stop-signal mechanism, sometimes incorrectly (forgotten
shutdown signal → orphan task; signal sent twice → panic on
oneshot consumer; signal sent before tick → first iteration
skipped silently).

### Problem

Section 4.7 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records:

> TSK-050-003 removes the orchestrator heartbeat. Going forward,
> any new periodic-task-with-stop-signal pattern in the codebase
> needs a documented contract for shutdown. Where to put it? How
> tested?

The model implementation that survived (worker heartbeat at
`worker.rs:1686`) shows the right shape:

```rust
let (heartbeat_stop_tx, heartbeat_stop_rx) = tokio::sync::oneshot::channel::<()>();
let heartbeat_handle = tokio::spawn(async move {
    let mut ticker = tokio::time::interval(interval);
    ticker.tick().await; // skip immediate first tick
    tokio::pin!(heartbeat_stop_rx);
    loop {
        tokio::select! {
            _ = ticker.tick() => { /* do work */ }
            _ = &mut heartbeat_stop_rx => { break }
        }
    }
});
// ... later, on shutdown:
let _ = heartbeat_stop_tx.send(());
let _ = heartbeat_handle.await;
```

This pattern works but is hand-rolled at every call site. The
sync daemon has a structurally different shape (signal-handler
flag on `AtomicBool` from `signal-hook`); they should be
unified in description even if the underlying machinery differs.

### Constraints

- The codeflow-cli uses `tokio` for async work; the contract
  must compose with `tokio::select!`.
- Some periodic tasks live in long-running daemons (sync daemon,
  future supervisor); others live in batch-bounded
  orchestrator / worker scopes. Both lifetimes need the same
  contract.
- Shutdown must be GRACEFUL by default (last in-flight tick
  completes) but with a hard timeout (don't wait forever for a
  hung tick).
- Errors during shutdown must not crash the parent task.
- The contract must be testable: tests should be able to
  spawn a periodic task, advance time, and assert exactly one
  iteration ran.

### Assumptions

- `tokio::sync::oneshot::channel::<()>` is the right primitive
  for one-shot stop signals (not `mpsc`, not `broadcast`).
- `signal-hook` is the right primitive for OS signal-driven
  shutdowns (the sync daemon model).
- Periodic tasks should NOT spawn additional periodic tasks
  recursively; if a single tokio task needs to do periodic
  work A and periodic work B at different cadences, two arms
  of a single `select!` is the right shape, not two tasks.

## Design

### The Contract

Define a documented, in-repo type — `PeriodicTask` — in
`codeflow-cli/core/src/util/periodic.rs` (new module) that
encapsulates:

```rust
pub struct PeriodicTask {
    handle: tokio::task::JoinHandle<()>,
    stop_tx: tokio::sync::oneshot::Sender<()>,
}

impl PeriodicTask {
    /// Spawn a periodic task. The closure is invoked once per `interval`,
    /// starting after the first interval has elapsed. Cancellation is
    /// graceful (current iteration completes) with a `shutdown_timeout`
    /// upper bound.
    pub fn spawn<F, Fut>(
        interval: Duration,
        shutdown_timeout: Duration,
        body: F,
    ) -> Self
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    { ... }

    /// Send the stop signal and await graceful shutdown.
    /// Returns Ok(()) on graceful exit, Err(Elapsed) on timeout.
    pub async fn shutdown(self) -> Result<(), tokio::time::error::Elapsed>;
}
```

Internally `spawn` runs the canonical `tokio::select!` body with
the oneshot stop signal. `shutdown` sends, awaits the handle
with `tokio::time::timeout(shutdown_timeout, ...)`, and returns
the elapsed error if the body did not complete.

### When to Use This Pattern

Use `PeriodicTask` for:

- Time-driven background work that does NOT need to react to
  external state changes (heartbeat, telemetry emission, periodic
  reaper).
- Per-component supervisory loops (the orchestrator's sync
  daemon supervision; ADR-003).

DO NOT use `PeriodicTask` for:

- Event-driven loops that react to messages (use `tokio::select!`
  on a `mpsc::Receiver` directly).
- Long-running daemons with their own lifecycle (sync daemon,
  Claude subprocess wrapper). Those have their own SIGTERM /
  signal-hook discipline.
- One-shot async tasks (use `tokio::spawn` directly).

### The Signal-Hook Variant

For long-running daemons (sync daemon, future supervisors) the
contract is different because the trigger is an OS signal:

```rust
let term_flag = sync::register_signal_handler()?; // sync.rs:760
loop {
    if term_flag.load(Ordering::Relaxed) { break }
    do_work();
    tokio::time::sleep(interval).await;
}
```

This stays as-is in `sync.rs`. `PeriodicTask` does not unify it
because the underlying primitive (signal vs in-process oneshot)
is fundamentally different.

### Numeric tuning

| Knob | Value | Rationale |
|------|-------|-----------|
| Default `shutdown_timeout` | 5 s | A periodic body that takes longer than 5 s to finish a single iteration is broken; 5 s is enough for any reasonable database write or filesystem operation. |
| Stop signal repeat policy | once | `oneshot` cannot be sent twice; the stop is idempotent at the API surface (`shutdown` consumes self). |
| Drop-on-task-handle policy | abort | If the parent drops `PeriodicTask` without calling `shutdown`, the handle is dropped; tokio aborts the task on next executor poll. The contract documents that callers SHOULD `shutdown` explicitly to flush the last iteration. |

### Test Contract

For every code site that spawns a `PeriodicTask`, the test
suite must include:

1. A unit test that asserts the body runs at least once before
   shutdown (start, advance time, assert work happened).
2. A unit test that asserts shutdown is graceful (start, advance
   time, shutdown, assert handle returns within timeout).
3. A unit test that asserts shutdown timeout fires for a
   pathological body (start, set body to `sleep(1h)`, shutdown
   with 100 ms timeout, assert `Elapsed` error).

These are mechanical and the test fixtures live in the same
module. `tokio::time::pause` + `advance` is the right primitive.

## Alternatives Considered

### Option 1: Documented pattern only, no abstraction

Add a section to `cf-rust-standards` describing the
`oneshot+select!` pattern; require new periodic tasks to follow
it.

**Pros:**

- Zero new code.
- Maximum flexibility — call sites tune internals as needed.

**Cons:**

- Drift is inevitable. A developer copy-pastes from one site,
  forgets to pin the receiver, or omits the
  `ticker.tick().await` for the first iteration.
- Tests of the pattern have no shared fixture.

**Why rejected:** the audit explicitly asks for a contract;
documentation alone is what we have today and it is failing.

### Option 2: Use a third-party crate (`tokio-cron`,
`background-jobs`)

Pull in an external library.

**Pros:**

- Mature, tested.

**Cons:**

- Heavier dependency surface than needed for the simple
  use cases here.
- The crate's API may not align with the codeflow-cli's
  shutdown discipline (graceful within timeout).
- License / vendoring review for a small abstraction.

**Why rejected:** internal abstraction is small enough to maintain
and aligns with the project's ergonomic style.

### Option 3: Build the abstraction into a framework type
(e.g., extend `Coordinator` to manage all periodic tasks)

Centralize ALL periodic tasks under a single registry that the
sync daemon manages.

**Pros:**

- Single shutdown path for everything.

**Cons:**

- Couples otherwise-independent components.
- The sync daemon's shutdown is signal-driven; orchestrator
  periodic tasks live and die with the orchestrator. Forcing
  them through a single registry breaks the lifetime symmetry.

**Why rejected:** wrong layering. Each layer (orchestrator,
sync daemon, worker) owns its own periodic tasks because
they share its lifetime.

## Consequences

### Positive

- One canonical `PeriodicTask` type for in-process oneshot-driven
  periodic work.
- Test fixtures shared across all callers.
- Easy to audit: `grep -rn "PeriodicTask::spawn" codeflow-cli/`
  enumerates every periodic task.
- The signal-hook variant is documented separately (in
  `sync.rs`) and not unified, reflecting reality.

### Negative

- A new module to maintain. Small (< 100 LOC) but real.
- Existing call sites (worker heartbeat at `worker.rs:1686`)
  could be migrated to use `PeriodicTask`, or left as-is. The
  migration is mechanical but adds noise to a working code
  path; the implementation ticket should decide based on test
  surface concerns.

### Risks

- **Underused abstraction.** If the codebase only ever has 2–3
  periodic tasks, a custom abstraction may be over-engineering.
  Mitigation: ADR-003 and ADR-004 both add periodic tasks; the
  count is growing, justifying the abstraction.
- **Subtle behavior difference vs hand-rolled.** A migrated call
  site might behave differently (e.g., the worker heartbeat's
  immediate `ticker.tick().await; consume the first tick`
  comment is preserved, but the `PeriodicTask` API hides this
  detail). Mitigation: the API documents the behavior
  explicitly; the worker heartbeat's first-tick-skip becomes
  the default.

## Follow-up

The follow-up implementation ticket
[INF-TSK-050-012](../tasks/INF-TSK-050-012.md) will:

- [ ] Create `codeflow-cli/core/src/util/periodic.rs` with the
  `PeriodicTask` type.
- [ ] Document the contract in `cf-rust-standards` skill (link
  the doc, don't duplicate).
- [ ] Add three template unit tests in the new module.
- [ ] Update ADR-003 and ADR-004 implementation tickets to use
  `PeriodicTask` for their new periodic invocations.
- [ ] (Optional, defer-decide-during-impl) migrate the worker
  heartbeat at `worker.rs:1686-1725` to `PeriodicTask` if the
  test surface improves; otherwise leave it as the model
  hand-rolled implementation.

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.7.
- [`codeflow-cli/core/src/autorun/worker.rs:1686-1725`](../../../../../codeflow-cli/core/src/autorun/worker.rs)
  — model implementation (oneshot + select! pattern).
- [`codeflow-cli/core/src/coordination/sync.rs:760-777`](../../../../../codeflow-cli/core/src/coordination/sync.rs)
  — signal-hook variant for daemons.
- ADR-003 (sync daemon supervision) and ADR-004 (reaper
  daemonization) both consume this contract for their periodic
  work.
