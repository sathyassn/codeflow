---
id: DESIGN-006
title: Stage timeout polling cadence
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Defines the autorun worker's polling interval for ws-* sentinel mtime watching, the trade-off vs detection lag and resource use, and how to tune.
tags: [autorun, stage-timeout, sentinels, ops]
---

# DESIGN-006: Stage Timeout Polling Cadence

## Status

Accepted.

## Context

The autorun worker (`codeflow-cli/core/src/autorun/worker.rs`)
runs a parallel watcher that observes `ws-*` sentinel mtimes under
`.state/sentinels/pathflow/{session_id}/` to detect a stuck stage.
If the worker makes no stage progress within `stage_timeout_secs`
(default 3 600 in `parallel-work-config.json:autorun.stage_timeout_secs`),
the watcher resolves and races the worker's main `select!` arm,
aborting the worker.

The cadence at which the watcher polls the sentinel directory is
defined at `worker.rs:260`:

```rust
pub(crate) const STAGE_TIMEOUT_POLL_INTERVAL_SECS: u64 = 30;
```

### Problem

Section 4.6 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records:

> `worker.rs:142` defines `STAGE_TIMEOUT_POLL_INTERVAL_SECS` (small
> relative to timeout). Trade-off between detection lag and resource
> usage (each poll is a `glob` over `.state/sentinels/`).

The constant is a hard-coded `pub(crate) const`. Two questions:

1. Is the 30-second cadence the right default?
2. Should the cadence be configurable, and if so, how?

The watcher is documented at `worker.rs:401-410` as intentionally
polling rather than using `inotify` / `fsevents`:

> This is intentionally a polling loop rather than a filesystem
> notification watcher (a) because the `.state/sentinels/`
> directory is created by the hook pipeline and may not exist at
> watcher-start time on some platforms, and (b) because `inotify`
> / `fsevents` add a platform-specific dependency for what is in
> practice a 30-second poll.

### Constraints

- Each poll calls `std::fs::read_dir(&dir)` and iterates entries
  with `metadata()` calls. On a session with 5–10 sentinels this
  is 5–10 stat() syscalls every cadence interval; cost is
  measured in microseconds.
- The polling watcher runs once per worker (one watcher per
  active session); 5 concurrent autorun workers = 5 watchers.
- The `stage_timeout_secs` default is 3 600 s (1 hour). A
  cadence of 30 s = 120 polls per stage-timeout window.
- The watcher must NOT poll faster than the sentinel-write
  hook can keep up. Sentinels are written by the
  `post-tool-use sentinel-write` hook on every tool-call
  matching the `STAGE-COMPLETE: WS-*` pattern; this is sub-ms
  per write. Polling at 1 Hz would not race the hook.
- The watcher's purpose is to abort STUCK workers, not slow
  ones. A correct decision needs only to fire eventually before
  the next operator intervention; sub-second precision is
  unnecessary.

### Assumptions

- Default `stage_timeout_secs` (3 600 s) is appropriately large
  for the largest legitimate stage (a 30-minute cf-development
  iteration with retries). Stages that legitimately take longer
  than 1 hour are operator-tunable via config.
- Filesystem stat cost is negligible compared to claude-process
  cost (the worker spends most of its time inside the
  Claude subprocess, not in tokio tasks).
- An operator who is debugging a hung stage is willing to wait
  up to one polling interval for the abort to fire. 30 s is
  the upper end of acceptable; 5 s is overkill given the
  3 600 s default timeout.

## Design

### Decision

Keep `STAGE_TIMEOUT_POLL_INTERVAL_SECS = 30 s` as the default.
Make it configurable via `parallel-work-config.json` under
`autorun.stage_timeout_poll_interval_secs`, with validated range
[5 s, 300 s] and default 30 s. Drop the `pub(crate) const`; use
the loaded config value.

The constraint `poll_interval_secs * 2 < stage_timeout_secs` is
enforced at config load (a 60-second poll interval against a
30-second timeout would never fire). Validation error message:

```text
stage_timeout_poll_interval_secs (X) must be less than half of
stage_timeout_secs (Y). Configured values would never trigger
abort.
```

A second validation: `poll_interval_secs >= 5` (anti-thrash).

### Rationale

The trade-off ladder is:

| Cadence | Detection lag | Stat cost / hour | Notes |
|---------|---------------|------------------|-------|
| 5 s | ≤ 5 s | 720 polls × ~100 µs ≈ 72 ms | Useful for debugging hung stages locally. |
| 15 s | ≤ 15 s | 240 polls ≈ 24 ms | Fine for development. |
| 30 s | ≤ 30 s | 120 polls ≈ 12 ms (current default) | Production sweet spot. |
| 60 s | ≤ 60 s | 60 polls ≈ 6 ms | Acceptable but over-conservative. |
| 300 s | ≤ 300 s | 12 polls ≈ 1 ms | Maximum allowed; saves nothing meaningful vs 60 s. |

Stat cost across all options is < 100 ms / hour. Resource use is
not the binding constraint; detection lag is. 30 s is chosen
because it is small enough that a hung stage aborts within one
poll-interval (one minute end-to-end) and large enough to leave
plenty of headroom for the hook pipeline to write the sentinel
before the next poll inspects it.

The configurable range exists to support two operator
workflows:

- **Debugging.** Operator running an autorun batch in foreground
  who wants faster feedback can set 5–10 s.
- **Long-stage workloads.** Operators with `stage_timeout_secs`
  set very high (e.g., 21 600 s = 6 h for batch jobs) might
  prefer a larger poll interval to reduce log noise; 60–300 s
  is appropriate.

The `inotify` / `fsevents` alternative is rejected for the
reasons documented in `worker.rs:401-410`: platform-specific
dependency, race on directory non-existence at watcher-start,
no measurable improvement over polling at this scale.

### Numeric tuning

| Knob | Value | Rationale |
|------|-------|-----------|
| Default cadence | 30 s | Sweet spot; matches the existing constant. |
| Min cadence | 5 s | Anti-thrash floor. 1-second polling would race the sentinel-write hook on some filesystems. |
| Max cadence | 300 s | 5 minutes is the upper limit at which the `poll_interval_secs * 2 < stage_timeout_secs` invariant still allows the default `stage_timeout_secs = 3600`. |
| Validation invariant | `poll * 2 < timeout` | Without this, a misconfiguration could make the watcher useless. |

## Alternatives Considered

### Option 1: Fixed 30 s constant (status quo, no config)

Keep the current `pub(crate) const` and ignore the audit's
configurability question.

**Pros:**

- No code changes.

**Cons:**

- Operators with very long `stage_timeout_secs` see useless
  log noise from frequent polls.
- Operators debugging hung stages locally cannot make the abort
  fire faster.

**Why rejected:** the configurability is cheap and addresses
real-world tuning needs.

### Option 2: Filesystem notifications (`inotify` / `fsevents`)

Replace polling with kernel-level notifications.

**Pros:**

- Zero detection lag.
- Zero stat cost.

**Cons:**

- Platform-specific. Linux uses `inotify`; macOS uses `fsevents`;
  Windows uses `ReadDirectoryChangesW`. A cross-platform crate
  (`notify`) exists but adds dependency and complexity.
- Watcher must handle the case where the directory does not
  exist at start time (it is created by the hook pipeline).
  Current polling code handles this by returning `None` from
  `read_dir.ok()`. Notification API would need either retry-
  watch or a parent-dir watch + filter.
- Complexity is real (test surface for the watcher more than
  doubles); benefit is marginal (sub-second detection on a
  60-minute timeout).

**Why rejected:** documented in `worker.rs:401-410` for the same
reasons.

### Option 3: Adaptive cadence (faster early, slower later)

Poll every 5 s for the first 5 minutes; then every 30 s; then
every 60 s.

**Pros:**

- Low detection lag during the period where stages are most
  active.

**Cons:**

- The "active" period is exactly when sentinels are being
  written rapidly — fast polling catches nothing meaningful.
- The "stuck" period is exactly when sentinels are NOT being
  written — slow polling delays detection by minutes.
- Adaptive cadence inverts the right relationship.

**Why rejected:** wrong shape.

## Consequences

### Positive

- Single configurable knob; default unchanged from current
  behavior.
- Validation catches misconfiguration at config-load time.
- Operators with non-default `stage_timeout_secs` can tune the
  cadence rationally.

### Negative

- Adds one config field; one validation rule.
- Existing tests that import the `pub(crate) const` need to load
  config instead. Mechanical change, but it is a change.

### Risks

- **Operator sets `stage_timeout_poll_interval_secs = 5` in
  production.** Cost is 720 stat calls/hour/worker. With max
  5 concurrent workers, 3 600 stat calls/hour total. This is
  invisible compared to actual workload. Mitigation: the
  validation does not need to enforce a "production
  appropriate" minimum; the 5-s floor is enough.

## Follow-up

The follow-up implementation ticket
[INF-TSK-050-011](../tasks/INF-TSK-050-011.md) will:

- [ ] Add `stage_timeout_poll_interval_secs` to
  `parallel-work-config.json:autorun` block (default 30, range
  5–300).
- [ ] Update `autorun/config.rs` to validate the field including
  the `poll * 2 < stage_timeout_secs` invariant.
- [ ] Replace `STAGE_TIMEOUT_POLL_INTERVAL_SECS` `pub(crate)
  const` with config-loaded value in `worker.rs`.
- [ ] Update `await_stage_timeout` to take the cadence from
  config, not the constant.
- [ ] Add unit tests for the validation (poll < 5 s rejected,
  poll > timeout/2 rejected, poll = timeout/2 - 1 accepted).

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.6.
- [`codeflow-cli/core/src/autorun/worker.rs:255-453`](../../../../../codeflow-cli/core/src/autorun/worker.rs).
- [`.codeflow/config/parallel-work/parallel-work-config.json`](../../../../../.codeflow/config/parallel-work/parallel-work-config.json).
