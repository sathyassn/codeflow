---
id: ADR-0079
uid: 792358c4-c3e3-40e3-a348-ad091fd5c973
title: "Estimate outcomes are derived from git by a second read-only verb"
date: 2026-10-05
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: docs/architecture.md, the estimation row names the outcomes report # none | one line naming what in architecture.md changes
---

# ADR-0079: Estimate outcomes are derived from git by a second read-only verb

## Context

ADR-0057 gave the native CLI one verb, `estimate check`, and said a future
verb needs its own justified decision. The method asks a project to keep
every started outcome and compare it with the frozen forecast, but nothing
produced the timings, so in practice no outcome was recorded and a cold-start
estimate ran far from the actuals for days before anyone noticed (GitHub
issue 77). Git already holds the timings: the commit that adds a task record,
the task branch's first commit, the commit that writes the acceptance block
and the merge that lands it.

## Decision

Add `codeflow estimate outcomes`, a second read-only verb. It derives each
complete task's planned, started, blocked, completed and landed points from
git author times, joins them by task id to the planning scenario of the
adopted home's frozen forecasts (or one given with `--forecast`), and prints
per-task ratios and, past a minimum, the median and range by work type with
the line "outcomes contradict the forecast". The minimum and the two
thresholds are flags with printed defaults, not policy keys and not a gate.
It reports an adopted estimate home that does not exist and exits 1 for it.
It writes nothing: no record, acceptance-block key, frontmatter key, claim
commit, forecast edit or registry entry. `codeflow status` prints one
estimates line only where `.codeflow/estimate.json` exists.

## Consequences

- An outcome record can cite derived timings instead of an agent's memory,
  and a missing estimate home is visible.
- Started is a lower bound: the first commit after the target, never the
  moment work began. A squash, rebase or fast-forward landing, or a branch
  whose only commit is the reviewed one, reports started as unknown.
- Waits are never inferred; the outcome record names them.
- The report reads only `HEAD`, its task records' integration targets and
  their history. A record edited only in a merge's conflict resolution is
  not seen, and a long history makes the one walk slower.
- No validator over a project's planning documents for session units or
  summed percentiles: every duration CodeFlow reads is in seconds and the
  report prints hours or days, and those rules live in project documents
  CodeFlow cannot lint honestly. They stay method prose.

## Architecture impact

The optional estimation row of `docs/architecture.md` names the read-only
outcomes report beside the allocation checker.
