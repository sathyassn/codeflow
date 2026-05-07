---
id: ADR-002
title: Integration branch reset semantics on `autorun resume`
status: accepted
date: 2026-05-07
deciders: [cf-planning]
consulted: []
informed: []
description: Defines whether `autorun resume` rebuilds, reuses, or resets the auto-generated integration branch from PR #255.
tags: [autorun, integration-branch, resume, ops]
---

# ADR-002: Integration Branch Reset Semantics on `autorun resume`

## Status

Accepted.

## Context

PR #255 (INF-TSK-022-032) introduced auto-generated integration branches
for autorun batches. The orchestrator either uses an explicit
`integration_branch` from the batch file or auto-generates
`autorun/<batch-id>/<short-sid>` and creates+pushes it from
`origin/main`. Resume reuses the original batch file and therefore the
original integration branch — but the question of whether to reset that
branch on resume was not addressed.

### Problem Statement

Section 4.2 of the comprehensive audit
(`project-management/epics/INF/INF-EPC-050/analysis/2026-05-03-autorun-interactive-comprehensive-audit.md`)
records:

> When `autorun resume` reuses an integration branch, should it (a)
> reset to `origin/{target}` (lose merged work), (b) keep state (orphan
> commits from prior partial run), or (c) require explicit user choice
> via flag? PR #255 introduced auto-generated integration branches
> without addressing resume.

`run_resume` (`codeflow-cli/cli/src/cmd/autorun.rs:4222-4442`) re-parses
the original batch file, calls `validate_batch_extended`, and runs the
filtered batch through the normal execution path. It does NOT call
`ensure_integration_branch` to reset state, and it does NOT delete the
remote branch. The integration branch on remote retains:

- Merged worker PRs from the prior partial run (the desired state).
- Possibly: orphaned worker PR commits that were merged but whose
  worker ultimately failed CI or was cancelled (not desired but
  observable).
- Possibly: out-of-order branch heads if the merge queue dequeued
  twice.

The orchestrator at `orchestrator.rs:255` only calls
`ensure_integration_branch` on first run (when the branch doesn't yet
exist on remote); on resume, the branch exists and the function is a
no-op.

### Constraints

- **Auto-merge work must not be lost.** If three worker PRs merged
  successfully into the integration branch before the batch crashed,
  the resume must inherit those merges. Any reset to `origin/{target}`
  would re-execute already-merged work.
- **Remaining workers must rebase against current state.** The four
  workers that did NOT merge before the crash will, on resume, open
  new PRs against the integration branch. Their rebase target is the
  CURRENT integration branch tip (which includes the prior merges).
- **Auto-generated branch names embed the batch id**, so a resume of
  the same batch always lands on the same branch — there is no
  ambiguity on resume identity.
- **PR #255's `cleanup_auto_integration_branch`** deletes the remote
  branch only after the orchestrator returns success. A crashed batch
  leaves the branch in place — which is the property resume relies on.

### Assumptions

- An integration branch's commit history is append-only during a batch
  run (workers merge in, no force-push).
- A successful merge to the integration branch is permanent — the
  orchestrator never rolls one back.
- Resume's responsibility is to complete the *remaining* tasks, not to
  redo the completed ones. The DataStore filter at
  `autorun.rs:4306-4309` (`!matches!(r.status, AutorunTaskRunStatus::Completed)`)
  encodes this guarantee.

## Decision

**`autorun resume` keeps integration branch state — option (b) from the
audit.**

The orchestrator code path is:

1. Read the original batch file.
2. Filter out completed task runs (existing behavior at
   `autorun.rs:4306-4309`).
3. Call `validate_batch_extended` against the remaining tasks.
4. Verify the integration branch exists on remote with `git ls-remote
   --heads origin <integration_branch>`. If it does, do nothing
   (preserve all prior merges).
5. If the integration branch does NOT exist on remote (manually
   deleted, or never created — should not happen for an auto-generated
   branch from a previously-run batch), recreate from `origin/main`
   and emit a structured warning event
   `integration_branch_recreated_on_resume`.
6. Proceed to orchestrator execution.

A new helper `verify_integration_branch_for_resume(project_dir, branch)`
in `autorun/orchestrator.rs` returns `Ok(BranchExists)` /
`Ok(BranchRecreated)` / `Err(GitFailure)` and logs each case with the
batch id, branch name, and the expected vs. observed state.

The auto-merge invariant remains: workers continue to PR against the
integration branch, the merge queue continues to serialize, and PR
#287's `pr_pushed` flag (post-PR auto-save guard) covers the rest of
the lifecycle. Resume is therefore a pure "restart the orchestrator
loop" operation; the integration branch is treated as a black-box
read-only handle.

### Rationale

Option (a) — reset to `origin/{target}` — is unsafe by construction:
it would discard merged worker output. Even if the orchestrator could
re-execute the affected tasks, their PR numbers, branch names, and
ledger event history are already recorded in the DataStore as
`Completed`. A reset would create a permanent divergence between the
DB state ("this task completed and merged its PR") and the git state
("the integration branch shows no such merge"). This is the worst
failure mode — silent data loss.

Option (c) — flag-gated reset — is unnecessary complexity for a path
that should never be wanted. An operator who wants a full restart can
delete the autorun session row, delete the remote branch manually, and
re-run the original batch as a fresh batch (it gets a new
`integration_branch_id`). The flag would only paper over a missing
operator workflow.

Option (b) — keep state — is the safe default and matches the existing
DataStore filter behavior. The new helper handles the "branch was
manually deleted" edge case explicitly so resume does not silently
fail with `git push` errors when the workers try to PR against a
non-existent base.

## Alternatives Considered

### Option 1: Reset to `origin/{target}` (option (a))

Force-reset the integration branch to its starting commit on every
resume.

**Pros:**

- Idempotent in the strict sense: every resume yields the same
  starting state.
- Easier to reason about for first-time operators.

**Cons:**

- Loses all merged worker output between the first crash and resume.
- Diverges from DataStore state — completed task runs claim a merge
  that no longer exists.
- The next worker PR's merge queue position is bogus (target tip
  changed under it).

**Why rejected:** corrupts the DB-vs-git invariant. Unrecoverable
without a full state rebuild.

### Option 2: Explicit `--reset-integration-branch` flag (option (c))

Default to keep-state, opt in to reset.

**Pros:**

- Operator has the choice.

**Cons:**

- Adds a CLI surface to a path that should not be used.
- Operators who genuinely want a fresh start are better served by
  starting a new batch (fresh DB row, fresh branch name).
- The flag would leak into batch documentation, autorun examples,
  and tests for no real benefit.

**Why rejected:** the flag is a workaround for a missing workflow. The
right workflow ("start a new batch") already exists and is better
isolated.

### Option 3: Verify-and-fail (no auto-recreate)

Like the recommended decision, but on missing branch error out
instead of recreating.

**Pros:**

- Operators see the inconsistency immediately.

**Cons:**

- A manually-deleted branch fails resume with an opaque `git push`
  error from inside `cf-git-operations` rather than a structured
  diagnostic.
- The recreate path is cheap and safe (the branch starts at
  `origin/main`, just like the first run); failing seems harsh.

**Why rejected:** the structured recreate emits a warning and the
operator can still cancel the batch if they intended the deletion.
Better-than-fail.

## Consequences

### Positive

- Resume is now well-defined: completed merges are preserved, remaining
  workers continue against current state.
- A new diagnostic event (`integration_branch_recreated_on_resume`)
  surfaces the rare manual-deletion case without breaking the flow.
- No new flags on `codeflow autorun resume`; the existing CLI surface
  is sufficient.

### Negative

- Operators who DID want a clean slate must learn the "start a new
  batch" workflow. This is documented in the resume command help text.
- The recreate path emits a warning even when the operator's intent
  was deletion + recreate; it is not detectable from inside
  `cf-git-operations` alone.

### Risks

- **DataStore-vs-remote skew.** If a non-codeflow process force-pushes
  to the integration branch between batch crash and resume, the new
  workers may see a different base. Mitigation: detected by the
  worker's pre-PR `check_merge_conflicts` from
  `codeflow-cli/core/src/git/conflict.rs`. Resume itself does not need
  to detect this — workers do.
- **Auto-cleanup after resume.** PR #255's
  `cleanup_auto_integration_branch` runs at orchestrator exit; on
  successful resume it correctly deletes the auto-generated branch.
  On crashed resume it correctly leaves it for the next resume. No
  change required.

## Implementation

### Action Items

The follow-up implementation ticket
[INF-TSK-050-007](../tasks/INF-TSK-050-007.md) will:

- [ ] Add `verify_integration_branch_for_resume(project_dir, branch)`
  helper to `autorun/orchestrator.rs`.
- [ ] Wire the helper into `run_resume` in
  `codeflow-cli/cli/src/cmd/autorun.rs`, immediately after
  `validate_batch_extended`.
- [ ] Emit `integration_branch_recreated_on_resume` to
  `coordination-events.jsonl` when recreate path is taken.
- [ ] Add 2 unit tests: branch-exists (no-op + Verified outcome),
  branch-missing (recreate + RecreatedFromOrigin outcome).
- [ ] Add an integration test using a temporary git repo: simulate a
  partial-completion batch, manually delete the integration branch,
  resume, assert worker PRs target the recreated branch.

### Timeline

Defer to the implementation ticket. ADR-002 is design-only.

## Related

- Follow-up implementation: [INF-TSK-050-007](../tasks/INF-TSK-050-007.md).
- Anchored on: `codeflow-cli/cli/src/cmd/autorun.rs:4222-4442`,
  `codeflow-cli/core/src/autorun/orchestrator.rs:1057-1132`,
  `codeflow-cli/core/src/coordination/merge_queue.rs:48-130`.
- PR #255 (INF-TSK-022-032): introduced auto-generated integration
  branches.
- PR #287 (INF-TSK-044-?): introduced `pr_pushed` flag for post-PR
  auto-save suppression — orthogonal but relevant context.

## References

- [Comprehensive Audit (2026-05-03)](../analysis/2026-05-03-autorun-interactive-comprehensive-audit.md), section 4.2.
- [`codeflow-cli/cli/src/cmd/autorun.rs`](../../../../../codeflow-cli/cli/src/cmd/autorun.rs).
- [`codeflow-cli/core/src/autorun/orchestrator.rs`](../../../../../codeflow-cli/core/src/autorun/orchestrator.rs).
