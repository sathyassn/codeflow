---
id: ADR-0080
uid: e0199111-9f1a-4c96-8741-ab653efd7d27
title: "A completed standalone task's criteria text is corrected by a planning records pull request"
date: 2026-10-07
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

# ADR-0080: A completed standalone task's criteria text is corrected by a planning records pull request

## Context

SPC-013 R-52 freezes a task's criteria once the task is complete on its
target, and a reopened task keeps them (R-119). The one route that changes a
completed task's criteria is the planning amendment that names its epic
(ADR-0078). A standalone task has no epic, so a completed standalone task's
criteria could not be corrected at all, not even to remove a person's name
or fix a typo. An adopter had to keep a person's name in a criterion and
record it as a workaround (sathyassn/codeflow#103). Corrections of the
acceptance text already have a route: a range of planning records only
keeps the binding and the `complete` status.

## Decision

A pull request whose range changes planning records only (records and plans,
judged by the planning amendment's path rule of ADR-0078, with no symbolic
link or submodule) may change the text of a completed standalone task's
existing criteria when its `Task:` line names that task, or names a
follow-up task whose `follow_up_of`, read at the target when the follow-up
is there, is that task. The task must be complete and standalone at the
target and at the head, and the range must not reopen it. The criteria set
stays frozen: an added, removed, renumbered or reordered criterion, or a
changed `(journey)`, `(after release)` or `(serves ...)` tag or checkbox
form, or an added list item that is no criterion, is refused as
`work.criteria_frozen` with the reason. `codeflow ci`
prints the criteria delta as a `work.criteria_frozen` note, and a human
reviewer confirms that the substance of each changed criterion is
unchanged; the machine guarantees structure only. The task stays
`complete`, and its acceptance block stays bound to its reviewed commit:
the range changes no block, so the binding is not judged again. A range
that changes the record of the completed task, and no task record outside
the correctable set (the named task and the task it follows), starts no
work, so the anchored preflight for tracked work (R-72) does not apply to
it. A follow-up named by the `Task:` line must be held by the target with
its record unchanged. The range is its own diff, from the merge-base of
the target and the head through the head, so a task record the target
changed after the branch point and the head does not touch is no change of
the range, and a task record the range changes is one even when the target
holds the same bytes. A range that changes only a plan note, the
follow-up's own record or another task's record corrects nothing and
keeps the preflight. On the corrected task's own `task/` branch, a range
keeps today's behaviour, since that branch is the task's reopen route. An
epic task keeps the amendment that names its epic.

## Consequences

- A wording fix in a completed standalone task's criteria lands as one
  small pull request of records, reviewed like any other.
- The check is structural: a correction that changes meaning while keeping
  every id and tag passes CI, and only the reviewer can refuse it. The note
  and its remedy say so.
- A release range brings the correction as a planning landing, as it
  already brings an amendment's criteria change (R-120).
- Projects on CodeFlow 3.0.0 still refuse such a range; the correction
  needs 3.1.0 on the CI that judges it.
- Unchanged: a task pull request changes only its own criteria; a reopened
  task keeps its criteria; an epic task's criteria change in its epic's
  amendment; product and enforcement paths never ride in a records range.

SPC-013 R-52 carries the requirement text.

## Architecture impact

none
