---
id: ADR-001
title: CI timeout retry policy for autorun PRs
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Defines how `wait_for_ci_green` retries transient gh failures, when it gives up, and what verdict the orchestrator routes on.
tags: [autorun, ci, retry, ops]
---

# ADR-001: CI Timeout Retry Policy for Autorun PRs

## Status

Accepted.

## Context

The autorun merge path polls `gh pr checks <pr>` until every required check
concludes, a required check fails, or a hard wall-clock timeout elapses. The
implementation lives in `codeflow-cli/core/src/autorun/ci_wait.rs` and is
invoked from the orchestrator before `gh pr merge` runs.

### Problem Statement

Section 4.1 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records the open question:

> `ci_wait.rs` polls `gh pr checks` until success/fail/timeout. What if
> `gh` returns transient errors (rate limit, network)? Currently treated
> as fatal. How long before re-poll? Re-attempts? Final give-up?

Since the audit was written, `ci_wait.rs:402` introduced `is_transient(err)`
which classifies `gh` failures into transient (retry inside the poll loop)
and permanent (surface immediately). The retry loop is bounded by
`CiWaitConfig::timeout_minutes` (default 30 min, validated range 1–1440 min)
and the cadence by `poll_interval_seconds` (default 30 s, range 5–300 s).
What the audit found — "treated as fatal" — is no longer true.

The remaining open questions are:

1. Is the 30-minute / 30-second envelope correct for autorun PRs?
2. What disposition does the orchestrator route on when the deadline is hit
   without a conclusive check verdict?
3. How are transient errors surfaced so an operator can tell apart a flaky
   run from a stuck one?

### Constraints

- The CI-wait runs inside the orchestrator process; a longer wait keeps the
  worker tmux session alive holding worktree resources.
- GitHub API rate limits: an authenticated `gh pr checks` call costs a
  REST quota point; aggressive polling at scale (many concurrent autorun
  PRs) risks 403 throttling.
- `wait_for_ci_green` already returns `CiOutcome::Timeout` when the
  deadline is hit pending; that is a distinct outcome from
  `RequiredFailed` (one or more checks concluded as failures) and from
  `GhUnavailable` (permanent error).
- The merge queue (`coordination/merge_queue.rs`) holds the worker's
  position until it dequeues; a CI-wait that hangs holds the merge slot.

### Assumptions

- A single autorun PR's CI run typically completes in 5–15 minutes for
  the codeflow repo; outliers (rebuilds, infra issues) reach 30 minutes.
  Source: prior INF-EPC-024 batch logs.
- The orchestrator wraps `wait_for_ci_green` in a `tokio::select!` against
  worker_timeout; the outer worker timeout (default 7 200 s, configurable)
  always exceeds the CI-wait timeout.
- Transient `gh` failures cluster (rate limit windows, network blips);
  successive polls within the same blip are wasted, but the deadline is
  the only correct give-up signal at this layer (gh-side recovery is not
  predictable).

## Decision

The CI-wait policy is anchored on three numbers, each with a configurable
override under `.codeflow/config/autorun/ci-wait.json`:

| Knob | Default | Validated range | Rationale |
|------|---------|-----------------|-----------|
| `timeout_minutes` | 30 | 1–1 440 | Covers the codeflow repo's CI 95th-percentile run plus a 2× safety margin. |
| `poll_interval_seconds` | 30 | 5–300 | Two polls per minute keeps GitHub API usage well below the 5 000-req/h authenticated quota even with 10 concurrent autorun PRs. |
| `is_transient` classifier | code-defined (`ci_wait.rs:402`) | n/a | Permanent: 401 / 404 / "bad credentials" / "not found". Transient: 403 / 429 / 5xx / network / parse errors. |

The poll loop runs until ONE of:

1. **`AllGreen`** — every required check concluded success. Orchestrator
   proceeds to merge.
2. **`RequiredFailed(names)`** — at least one required check concluded
   failure / cancelled / timed-out. Orchestrator marks the worker
   `failed` (with the failing check names recorded in the worker row).
3. **`Timeout`** — the configured `timeout_minutes` elapsed without a
   conclusive verdict. Orchestrator marks the worker `failed_ci_timeout`
   and emits a structured event with the elapsed time and the most recent
   check states. **The worker is NOT retried.** A new autorun batch is
   the operator's recourse.
4. **`NoRequiredChecks`** — only when GitHub reports no required contexts
   AND `fall_back_to_all_checks_when_no_required=false`. Orchestrator
   may proceed to merge (config-gated).
5. **`GhUnavailable(permanent)`** — auth/PR-not-found/etc. Orchestrator
   marks the worker `failed_gh_unavailable` and surfaces the message.

Transient errors are logged to stderr at every retry with the message
`ci-wait: transient error fetching PR #<n> checks (<reason>); retrying in <s>s`
(already implemented at `ci_wait.rs:363`). They do NOT shorten the
deadline.

### Rationale

The default 30-minute / 30-second envelope was already chosen by the
INF-TSK-048-001 implementer (PR #300) and has held up across INF-EPC-024
batches without false-positive timeouts. Increasing the timeout would
extend the merge-queue tail in pathological cases without improving
recovery from the failure modes that actually occur (long CI runs are
rare and equally well served by a `codeflow autorun resume`).

Treating `Timeout` as a distinct verdict — separate from `RequiredFailed`
and from gh-permanent — lets operators tell apart "CI ran and one check
broke" from "CI hung" from "auth/PR misconfigured", which routes
recovery differently:

- `RequiredFailed` → fix the failing build, push.
- `Timeout` → check runner availability, then `codeflow autorun resume`.
- `GhUnavailable(permanent)` → fix `gh auth status` or the PR target.

In-loop retry is bounded by the deadline and is invisible to the merge
queue (no entry shuffling occurs while the worker waits). This avoids
the auto-retry / exponential-backoff trap that would multiply hold time
in the merge queue under correlated CI outages.

## Alternatives Considered

### Option 1: Numeric retry budget (e.g. 5 retries × 2 minutes)

Replace the wall-clock deadline with a fixed retry count.

**Pros:**

- Predictable upper bound on `gh` API calls per worker.
- Simple to communicate ("we retry 5 times").

**Cons:**

- Couples retry budget to error shape: a single 30-second blip
  consumes the same budget as a 5-minute outage.
- Doesn't compose with check duration — a successful but slow CI run
  could exhaust the budget mid-pending.
- Loses the "wall-clock guarantee" property the orchestrator depends on
  for merge-queue progress.

**Why rejected:** wall-clock deadline + transient classification already
gives a tighter, easier-to-reason-about contract. Retry count would
require a second dimension (per-retry timeout) that adds complexity
without addressing a real failure mode.

### Option 2: Exponential backoff between polls

Start at 5 s poll interval and double up to 5 minutes.

**Pros:**

- Reduces API calls during long pending states.

**Cons:**

- Late-arriving check verdicts (success / failure) are detected with
  several minutes of lag; in autorun the merge queue is held that long.
- Users observing the live log see less frequent updates, hurting
  debuggability.
- The default 30-second cadence is already cheap (2 calls/minute) — the
  cost saving does not justify the visibility loss.

**Why rejected:** the constant 30-second cadence is a better cost /
latency point for the codeflow repo's actual scale. Exponential backoff
would matter at 100+ concurrent autorun PRs, which is not the current
operating envelope.

### Option 3: Drop the in-loop retry and treat any `gh` error as fatal

Pre-`is_transient` behavior.

**Pros:**

- Fewer code paths to test.

**Cons:**

- A single network blip during a 30-minute CI run aborts the wait. The
  worker is then in `failed_gh_unavailable` even though CI was healthy.
- Forces the operator to `codeflow autorun resume` for every blip,
  multiplying overhead per batch.

**Why rejected:** observed in INF-EPC-024 telemetry; the cost-of-blip
is real and the code already mitigates it. Going back is regression.

## Consequences

### Positive

- Single, easily-tunable knob (`timeout_minutes`) controls the
  wall-clock envelope.
- Distinct `Timeout` verdict means operators see CI hangs as a different
  failure class from CI failures and gh failures.
- In-loop retry is invisible to downstream code; the merge queue and
  worker state machine see exactly one of five conclusive outcomes.

### Negative

- A pathological CI hang exhausts the 30-minute budget per worker and
  blocks the merge queue position for that long. A `--ci-timeout-secs`
  CLI override on `codeflow autorun run` would let an operator shorten
  the wait when they know CI is stuck; this is deferred to the
  implementation ticket.
- Transient errors silently extend total wait time without a numeric
  budget; an operator reviewing logs sees the retry messages but no
  aggregated "CI was unreachable for X minutes" summary. A retry-stats
  metric on the worker row is also deferred.

### Risks

- **GitHub rate-limit cascade.** Many parallel autorun workers polling
  every 30 s could exhaust the 5 000 req/h quota on a shared token.
  Mitigation: `is_transient` correctly handles 403/429 by retrying
  inside the existing window, and the per-worker call cost is
  ≤ `timeout_minutes × 2`. For the codeflow repo's max-5 concurrent
  worktrees this stays well under quota. Future scale-out is the
  follow-up ticket's concern.

## Implementation

### Action Items

The follow-up implementation ticket
[INF-TSK-050-006](../tasks/INF-TSK-050-006.md) will:

- [ ] Verify `wait_for_ci_green` already maps the 5 verdicts to distinct
  worker statuses; if `Timeout` and `GhUnavailable(permanent)` collapse
  into one status today, split them.
- [ ] Add a `--ci-timeout-secs` CLI override on `codeflow autorun run`
  that propagates into `CiWaitConfig::timeout_minutes` for the batch.
- [ ] Emit a `ci_wait_summary` ledger event with retry count + max
  observed transient-blip duration when the loop exits.
- [ ] Add a unit test that asserts `Timeout` is returned when every
  poll within the deadline reports pending.
- [ ] Add a unit test asserting transient errors do not abort the loop.

### Timeline

Defer to the implementation ticket. ADR-001 is design-only; no code
changes ship under TSK-050-005.

## Related

- Follow-up implementation: [INF-TSK-050-006](../tasks/INF-TSK-050-006.md).
- Anchored on: `codeflow-cli/core/src/autorun/ci_wait.rs:80-107`,
  `:326-381`, `:402-442`.
- Source code constants: `default_timeout_minutes`, `default_poll_interval_seconds`,
  `TIMEOUT_MINUTES_MIN/MAX`, `POLL_INTERVAL_SECS_MIN/MAX`.

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.1.
- [`codeflow-cli/core/src/autorun/ci_wait.rs`](../../../../../codeflow-cli/core/src/autorun/ci_wait.rs).
