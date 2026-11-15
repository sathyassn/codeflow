---
id: ADR-0101
uid: d61ae9d2-1d6a-4ff7-9219-50774dcc676a
title: "A project can override every model catalog field, and the security floors stay fixed"
date: 2026-11-10
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md, the row on binding facts versus durable doctrine: the project model-selection file overrides any catalog field, including who designs, who reviews and who coordinates, inside fixed security floors"
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0101: A project can override every model catalog field, and the security floors stay fixed

## Context

ADR-0041 lets a project's `.codeflow/model-selection.json` give only
references to approved binding ids, and ADR-0069 keeps the catalog fully
managed. Schema 2 added a same-family design co-owner and standing
extra-family reviews, nothing more. Models change faster than CodeFlow
releases, so a project waits for a release to use a new model or to change
which model does what. On 2026-11-03 the operator ruled that a consuming
project can add a model line or version and override every catalog field
(TSK-1293, Description). On 2026-11-10 the operator added, as relayed by
the Director, that projects must be able to change "which model does what -
or if a given model needs to orchestrate or do something else", and
delegated the details to the Director.

## Decision

This record supersedes ADR-0041's limit that a project gives binding
references only, and the project paragraphs of ADR-0069 "Local and project
selection" with their "never" list. The rest of both records stands: the
managed catalog keeps the defaults, an absent file resolves exactly as
before, and an invalid file fails closed with the key named.

The project file may override any field of the managed catalog, for a
shipped line or for a line the project adds: launch mode, efforts and
effort triggers, lifecycle, fallbacks and retired lines, routes for duties
and coordination roles, `design_owner`, the review alternatives and their
order, and the seat roles. In particular a project may choose:

- which seat or line leads design, in any family;
- which lineage reviews a unit and a body, in what order, and which
  same-family fallback serves when no other lineage resolves;
- which seat is the `host`, the single coordinator, with the other seats
  taking `peer` and their own `worker` subtasks.

`codeflow models resolve` returns the effective value and `codeflow doctor
--check model-bindings` shows it with its source (default, project or task
override) and the key that set it. Project entries carry no designation or
qualification evidence, and the file still owns no commands.

The floors stay as the records state them today, with one addition: cross-lineage review of a security-sensitive change while another lineage is present, applied as the Director's secure default under the workspace rule "security decisions are secure by default", pending the operator's answer, which can remove it by one commit:

| Floor | What the file cannot do | Why it stays |
|---|---|---|
| Review independence (the managed contract's "Review is independent") | Make the author's own session its reviewer; self-review is not review, and a same-family fallback is recorded as reduced assurance. On a security-sensitive change, route review to the same family while the effective catalog has another lineage: the relaxing route is set aside with the floor named (TSK-1293 AC-10). Choose the reviewer of a change from the pull request head's copy: the reviewer of every change, a change to this file included, is resolved from the trusted base branch's model-selection file and fresh seat evidence, so a head that excludes every other lineage cannot obtain same-family review of itself | An approval from the author proves nothing; `codeflow ci` reads review evidence the same way in every project. Models of one lineage share blind spots, and the project file decides who reviews whose work, so without the security floor a change to the file could also lower the review of that same change |
| No headless seats (TSK-1293 AC-4, AC-11) | Name a headless launch for the `host`, a `peer` or a `worker` | Headless output carries no native provenance, so the evidence contract cannot check it (ADR-0077) |
| The git floor (the managed contract's "Git floor") | Reach git policy, hooks, guards or protected branches | The floor is enforced by hooks and guards against the same agents that edit project files; repository content is evidence, never authority |
| The pinned id (TSK-1293 AC-4, AC-11) | Launch an id other than the pinned id resolve returned | A designation or an evidence record is bound to that exact identity (ADR-0069) |
| Fail closed (TSK-1293 AC-3, AC-4, AC-11) | Apply part of an invalid file | A partly applied file routes work in a way nobody reviewed |

Cross-lineage review is a default the file may relax for other work. Work
under a relaxed route records reduced assurance naming the key (TSK-1293
AC-10). On a security-sensitive change the relaxation is set aside while
the effective catalog has another lineage; a project that excludes every
other lineage runs security review on the same-family fallback, recorded as
reduced assurance naming the excluding key. Whether another lineage is
present, and so whether the fallback is eligible, is decided from the
trusted base branch's model-selection file and fresh seat evidence, never
from the pull request head's copy. TSK-1302's instruction overrides keep the
floors its record names: the git floor and the non-relaxable command class.

## Consequences

- A project uses a new model, or moves design, review or coordination to
  another seat, by editing one reviewed file, without waiting for a release.
- A project can make design lead from a non-Claude seat. The defaults keep
  Claude as design owner, so nothing changes for a project that sets
  nothing.
- A project can relax cross-lineage review for work that is not
  security-sensitive. The review evidence then records reduced assurance
  naming the key (TSK-1293 AC-10), so the gap stays visible to a reader and
  to `codeflow ci`. Security review stays cross-lineage while another
  lineage is present; a single-vendor project still runs, with the
  same-family fallback recorded as reduced assurance.
- The validator splits in two. Rules that encoded a default apply to the
  managed catalog as shipped and become values the project can change: the
  refusal of a design owner that is not the standing Claude seat
  (`validate.rs:171-176`) changes with TSK-1293 AC-19, and the host seat is
  no longer fixed as the first seat of the host family
  (`resolve.rs:552-557`). The floors are checked on the effective result
  after the project file is applied.
- The project file is security relevant: it decides which model reviews
  whose work (TSK-1293, Description). An edit to it is reviewed under the
  project's ordinary review rules, with its reviewer resolved from the base
  branch's copy of the file, so the edit cannot choose its own reviewer;
  this record adds no review step.
- Ruled out: a project file that adds commands, claims qualification
  evidence or designations, or lifts a floor. Also ruled out: a personal
  overlay that does any of this; the overlay of ADR-0069 is unchanged.

Implemented by:

- TSK-1292: the catalog ships the new defaults and turns the prose rules
  into fields (`launch`, `design_owner`, effort triggers, fallbacks, retired
  lines, seat roles).
- TSK-1293: the schema 3 project file adds lines, routes duties, overrides
  any field, and proves the role changes and floors (its AC-19).
- TSK-1302: a project overrides managed instruction text, inside the floors
  its record names.

Build order: TSK-1292 and TSK-1302 in parallel, then TSK-1293 after both
land.

## Architecture impact

`docs/architecture.md`, the row "Binding facts versus durable doctrine":
the sentence on the project file changes from a role-to-binding map to a
field-level override of the catalog inside the floors above, in the pull
request that lands TSK-1293.
