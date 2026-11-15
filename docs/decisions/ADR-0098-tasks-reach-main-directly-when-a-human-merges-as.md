---
id: ADR-0098
uid: 178bffed-3a90-4903-b45d-657676167029
title: "Tasks reach main directly when a human merges as they are ready, otherwise through checkpointed epic lines"
date: 2026-11-05
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0098: Tasks reach main directly when a human merges as they are ready, otherwise through checkpointed epic lines

## Context

The managed guidance said a multi-task epic reaches `main` only when it is
finished, through one final pull request. Epics stayed open for weeks, so
landed work waited on lines and little reached `main` (issue #280). Records
were also edited on lines other than their own, and each sync of `main`
into a line needed a task invented to admit it (TSK-1312). On 2026-11-05 the
operator chose one landing rule for every project, CodeFlow and adopters
alike ("general run is fine", recorded in the workspace release plan).

## Decision

A task pull request targets `main` when a human can merge it as it becomes
ready. Otherwise the task targets its epic's integration line, where agents
merge reviewed tasks in batches without waiting for a human, and the line
reaches `main` through a checkpoint pull request that a human merges when
available. A checkpoint pull request from the line to `main` is opened, or
refreshed, at every green line tip; the last one carries the epic close. A
standalone task that does not target `main` lands on the line it targets, as
its own candidate. Each task or epic record changes only in its one home. A
task's home is the branch its `integration_target` names: its epic's line, the
line a standalone task targets, or `main`. An epic's home is the branch its
tasks target. A line takes `main` only through the admitted sync route that
`codeflow ci` judges (TSK-1312), never through a merge that needs a task of
its own. That sync takes `main`'s version only of records homed on `main` and
never changes a record homed on the line. The other landing rules stand: one
pull request per task, batched candidates with one full gate, and only a human
merges into `main`.

## Consequences

- Finished work reaches `main` at each green line tip instead of at epic
  close, and a project whose humans merge as work becomes ready skips lines
  entirely.
- Unfinished work that reaches `main` through a checkpoint is a risk, and
  the epic's own criteria guard it.
- A human reviews more, smaller pull requests to `main`; each checkpoint is
  judged as the line's own completions (TSK-433).
- A record edit on any branch other than the record's home is out of
  bounds; it goes through that home.
- Until TSK-1312 lands, a sync of `main` into a line still needs the carrying
  task the current checks require.

## Architecture impact

None.

## Note, 2026-11-11: ADR-0102 narrows this record for marked epics

ADR-0102 (a marked epic reaches `main` once and complete, and line pushes
start no hosted run), proposed on 2026-11-11, replaces two parts of this
record for marked epics only: the checkpoint pull request at every green
line tip, and the one pull request per task. For unmarked epics, EPC-022
among them, this record's checkpoint rule stays in force until they close.
The rest of this record stands.
