---
id: ADR-0076
uid: 2b3eb168-2661-4484-beab-62113437b153
title: "One PR per task and planning once per epic"
date: 2026-09-29
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "`docs/architecture.md`: the `work start` preflight reads a standalone task's record at HEAD when the record arrives in the task's own PR, and CI admits that record at completion through the structural core of readiness without the start gate. Updated in the PR that accepts this record (TSK-184)."
---

# ADR-0076: One PR per task and planning once per epic

## Context

On the EPC-020 delivery line, 55 planning PRs landed against 42 task PRs.
The method planned again for every task change, required a separate
planning merge before a task branch could start, capped review at two
rounds and then escalated, asked for a closeout narrative and a
verification file per task, and ran the full gate on every task PR, which
executed the Rust suite up to three times. Much of that work protected
nothing a check or a reviewer read. On 2026-09-29 the operator approved one
delivery process, the standalone one-PR task for any single outcome, and
the rule on who may add process, and directed that the waste be removed in
one round without lowering any quality floor (TSK-184).

## Decision

CodeFlow adopts one delivery process.

- **Plan once, at the breakdown.** Planning happens when an approved brief
  or spec is broken into an epic and its tasks: one planning PR creates the
  epic, every task with its criteria, and the edges, reviewed once by the
  other model lineage. Later changes of scope (a follow-up, a new or split
  outcome, a reassignment, an amendment of another task's criteria, a spec
  amendment before the spec ships) ride in one batched epic amendment on a
  `plan/` branch with one other-lineage reviewer. A change of outcome,
  cross-task interface, dependency graph or safety boundary still goes to
  both seats as a new plan version.
- **One PR per task.** A task is one outcome a user or operator can
  observe, delivered in one PR that carries the code, tests and docs, the
  task's own criteria change, its status change and its acceptance block.
  CI prints the task's own criteria change for the reviewer; every other
  record's criteria stay frozen. The builder merges the integration line
  into the task branch before review; a merge whose tree equals the clean
  re-merge keeps the completion bound to the reviewed commit, and a
  hand-resolved product hunk still unbinds it.
- **The standalone one-PR task.** The smallest unit of work is a
  standalone task: its record and its code land in the same reviewed PR.
  Its record may be allocated on its own task branch, and CI admits that
  one record, read at head, through the structural checks of readiness,
  while `work start` still refuses a closed task. Every other record added
  in that PR stays refused. An epic task keeps its record anchored in the
  epic's planning change.
- **Every change names a task.** A PR names its task, or its epic for a
  planning-only range and for the PR to `main`; the direct-change class
  `Task: none: <reason>` is removed, and a PR that names neither is refused
  whatever it touches.
- **Holistic review, no round caps.** One review pass by the other lineage
  covers the whole task change. A material finding is fixed in the same PR
  and confirmed by its finder; a small fix that came with a failing probe
  is confirmed by rerunning that probe and the affected tests. Nits get
  one disposition and never block. Review ends on evidence: continue while
  repairs produce relevant evidence, and diagnose a stalled mechanism, an
  invalid assumption or a materially changed scope. No round count decides
  it.
- **One full gate per batch candidate.** Builders run targeted tests and
  the quick gate and cite them. The primary assembles reviewed heads into a
  small candidate in dependency order and runs the full gate once on that
  exact candidate before the integration line moves; each completion stays
  bound to its own reviewed commit. A red gate is diagnosed first, and a
  member is dropped only when evidence attributes the failure to it. A
  standalone PR is its own candidate. A later task may build on a
  predecessor's reviewed head, named with `--on TSK-NNN@<sha>`, and the
  predecessor still lands first.
- **Only the operator adds process.** A rule that adds a PR, an approval,
  a review round or a record to every task needs the operator's explicit
  approval before it lands, with what it protects and what it costs.
  Agreement between model seats is never enough. Removing duplicate
  ceremony is ordinary reviewed work; removing a safety or authority
  protection needs its owner's approval.

SPC-013 carries the requirement text (R-23, R-32, R-40, R-42, R-52, R-60,
R-70, R-73, R-74, R-78, R-110); the method reference
`cf-method/references/delivery-process.md` carries the narrative.

## What this supersedes

Each earlier record stays accepted except for the clause named here. The
accepted text is not edited: each gets a dated Note pointing here, and its
`superseded_by` stays `null`, since no whole decision falls (the form
ADR-0072 used on ADR-0046).

| Record and clause | Change |
|---|---|
| ADR-0046: "Planning belongs on a `plan/` branch: a task branch cannot commit planning records that authorize its own implementation." | Narrowed to epic tasks. A standalone task's own single record may be committed on its task branch and lands with its code in one reviewed PR. |
| ADR-0072, "What this supersedes": ADR-0046 stays accepted for "the rule that a task branch cannot authorise its own planning record" | Narrowed the same way; the planning anchor is the epic's planning change for an epic task and the record at head for a standalone PR. |
| ADR-0023: "Plan and rework loops are each bounded to two rounds before human escalation." | Superseded. No round cap; review ends on evidence under the progress rule above. |
| ADR-0023: "Where line coverage is supported, production code has an 80% hard floor and a normal 90% target." | Superseded. The coverage floor is the project's configured gate (CodeFlow's is `--fail-under-lines 90`), not prose. |
| ADR-0015: "a bounded fix loop (≤2 rounds to plan-agreement, then a human tiebreak; the build loop keeps `cf-develop`'s max-3-rework bound)" | Superseded. No round or rework cap. |
| ADR-0035: "Changing the named producer or reviewer seat or lineage creates Plan vN+1 and requires fresh approval from both primary seats." | Superseded. A reassignment is recorded where the assignment lives and, for unstarted tasks, rides in the batched epic amendment with one other-lineage reviewer. The named other-lineage seat still reviews the actual unit. |
| ADR-0040: "A material node, edge, guard, interface, ownership, acceptance, or safety change creates Plan vN+1" | Narrowed. Plan vN+1 is needed for a change of outcome, cross-task interface, dependency graph or safety boundary; an ownership change and the task's own criteria change do not create one. |

## Consequences

- Planning PRs fall to one per breakdown plus batched amendments; a task
  PR is reviewed once with its record, criteria change and acceptance.
- The full gate runs once per batch candidate instead of once per task PR.
  A batch that turns red costs a diagnosis before any member is dropped.
- The reviewer carries more in one pass: the record, a printed criteria
  change and the code. The criteria delta is computed by CI, never
  declared by the author.
- The checker grows: CI admission of a standalone record apart from start
  readiness, the same-PR waiver route, the clean line merge after review,
  per-completion binding at a batch landing, and pin checks for stacked
  builds. Matching a pin is structural and does not authenticate a
  review's verdict.
- Adopters lose the `Task: none` route. A project with durable tracking
  must name a task or an epic on every PR; where tracking is inactive the
  `Task:` line names the harness's tracked unit.
- Unchanged: other-lineage review of every task, the security pass on its
  trigger, criteria with evidence per criterion, the acceptance block bound
  to the reviewed commit, `work.stable_planning_anchor` at block, completed
  predecessors at the merge-base on landing, one full gate on the exact
  candidate before the line moves, and human-only merges into `main`.

## Architecture impact

`docs/architecture.md`: the `work start` preflight reads a standalone
task's record at HEAD when the record arrives in the task's own PR, and CI
admits that record at completion through the structural core of readiness
without the start gate. Updated in the PR that accepts this record
(TSK-184).

## Note (2026-10-03)

ADR-0078 refines the batched epic amendment and the "Every change names a
task" clause. One planning amendment may name several epics on its one
`Task:` line (`Task: EPC-001, EPC-002`) and may carry docs and the project
section of `AGENTS.md`, with the managed block byte-identical to the
target's. CI reports each change per named epic and refuses a change to an
epic the line does not name. The amendment lands once on `main`, and an
integration line takes it by merging `main`. This decision stays accepted.
