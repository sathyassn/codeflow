---
id: ADR-0102
uid: 4f4dd15e-4827-4724-bd3e-05ce79753706
title: "A marked epic reaches main once and complete, and line pushes start no hosted run"
date: 2026-11-12
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

# ADR-0102: A marked epic reaches main once and complete, and line pushes start no hosted run

## Context

Every task of an epic now reaches `main` through its own pull request, with
its own hosted runs, review passes and maintainer merge. An adopter measured
that cost over four weeks: about 14 hosted runs, 3 to 4 review passes and
one maintainer merge per task (issue 770). CodeFlow's own records show the
same pattern: 208 of 681 task dependencies cross an epic, and checkpoint
pull requests carry unfinished epics to `main` piece by piece. The operator
ruled on the delivery unit and on hosted CI placement on 2026-11-11, at
about 20:05 EST.

## Decision

There are two delivery units. A task that lands alone goes to `main`
through its own pull request with the full hosted run. A marked epic's
tasks land on the epic's line with no pull request and no hosted run of
their own, and the epic reaches `main` once, through one pull request,
only when it is complete: every task complete, criteria as planned or
amended, and its proof and whole-epic review on a commit that holds
`main`'s tip. Every landing, a task or an epic, leaves `main` valid: it
builds and passes on the code that holds `main`, its contracts and docs
claim only what is there, and its behaviour is absent or complete, never
half reachable. The marker is a field on the epic record (proposed as
`main_landing`, values `whole-epic` and `checkpoint-prs`; an absent field
means `checkpoint-prs`), read only from the default branch's copy and set
only by a planning range into the default branch. Epics planned before the
marker exists, EPC-022 among them, are unmarked and keep checkpoint pull
requests until they close. No push to a line starts a hosted run, in
CodeFlow's own CI and in the shipped templates. A pull request into a line
runs only the short checks: commit standards, the secret scan and the
security review. The Linux gates and every Windows job run only on the pull
request into `main`. A push to `main` reuses that pull request's run only
under strict equality: the same tree, the same workflow files, the same
trust inputs, and each reused job really ran and passed, with the deciding
script read from the trusted base and pinned by digest; anything else runs
in full, and the secret scan runs on every push.

A check leaves hosted CI only when it runs locally on every landing, or
when a recorded reason says it can only run hosted, in which case it stays
hosted. This is fixed for CodeFlow's own repository and is guidance for
projects; a project that drops a hosted check without either records why.
Windows cannot run on CodeFlow's macOS host, so while `main`'s heavy
checks are relaxed (see the decision history), CodeFlow runs Windows once
a day on `main`, not blocking, and by manual dispatch.

| Event | Runs |
|---|---|
| A task of a marked epic lands on its line | No pull request and no hosted run; the local hooks and gate |
| Push to any line | No hosted run |
| Pull request into a line (an unmarked epic's task) | Short checks only: commit standards, secret scan, security review |
| Pull request into `main` (a task alone, a marked epic, a checkpoint) | The full set: the Linux gates and every Windows job |
| Push to `main` | Reuse of the pull request's run under strict equality, else the full set; the secret scan always |

Relation to earlier records:

- ADR-0097 (CI suite placement by event): this record replaces its rows for
  a pull request into a line and a push to a line, and its reuse of Linux
  verdicts from a line push on a pull request into `main`. Its other rows
  stand.
- ADR-0098 (on the EPC-022 line, not yet on `main`): this record replaces,
  for marked epics only, its checkpoint pull request at every green line
  tip and its one pull request per task. For unmarked epics ADR-0098 stays
  in force until they close.
- A "batch" is not a delivery unit. Reviewed task merges pushed to a line
  together are a landing candidate, which is transport only.

The build is pending in three epics: EPC-055 (the route, the review
binding, the clean merge of `main` and the complete-epic gate),
EPC-056 (the epic graph and the landing answers) and EPC-057
(the shipped workflows and CodeFlow's own CI).

## Consequences

- Hosted runs, review passes and maintainer merges fall from one set per
  task to one set per epic for marked epics.
- Finished work of a marked epic waits on its line until the epic is
  complete, so epics are cut small: valid on `main` alone, about 5 to 10
  tasks as a reference point, never a cap. A part needed on `main` sooner
  becomes its own epic.
- A Windows-only break is first seen on the pull request into `main`: a
  task's own pull request when it lands alone, and the epic's pull request
  for a line. A project cannot place a hosted run on a line push for a
  protected check, so the epic's close check carries that residual.
- A pull request into a line no longer has the Linux tests, coverage or any
  Windows job; it rests on the short checks, the independent review and the
  builder's local gate.
- Two landing rules run side by side until the last unmarked epic closes.
- Ruled out: a hosted run on a line push, a task pull request inside a
  marked epic, and a marked epic reaching `main` before it is complete.
- The daily scheduled run on each active line is suspended; it is not a
  push, and Windows coverage before `main` comes from the daily
  non-blocking run on `main` while the relaxation lasts.

## Decision history

- 2026-11-11, about 20:05 EST. The operator's ruling, relayed by the
  Director: no hosted run on any line push, CodeFlow included; Windows runs
  only on the pull request into `main`; the operator agreed the rest of the
  epic delivery plan, with any exception still to be named. The Director
  applied it the same evening: pull requests into a line run the short
  checks only, and unmarked epics keep checkpoint pull requests.
- 2026-11-11, about 20:35 to 20:50 EST. The operator, recorded by the
  Director in `RELEASE-PLAN.md`: `main`'s CI checks are temporarily
  "disabled - as long as they're locally run and successful ... put things
  back after"; the dependency audit and the full-range secret scan with its
  allowlist canary "run always" locally; and a check is skipped in hosted
  CI only if it runs locally or has a reason to run only hosted. Checks
  that run outside the pull request's control (merge admission, trusted
  release impact, the authority checks, CodeQL, the hosted secret scan)
  stay on. Each relaxed check returns on a condition recorded with it.
- 2026-11-11. Written at the planning landing that files EPC-055,
  EPC-056 and EPC-057. Status stays proposed until the operator
  approves the wording that splits ADR-0098 between marked and unmarked
  epics.
- 2026-11-12, about 10:36 EST. The operator accepted this record's
  wording as drafted, in item D6 of the "yes to all" answer recorded in
  the workspace `RELEASE-PLAN.md` (lines 1354 to 1361): "built_by,
  cf-seats, ADR-0102's wording and the Codex builder seat profile are
  accepted as drafted". The status was set to accepted on 2026-11-20 by
  the planning amendment that adds this entry and the next one. The text
  above is unchanged.
- 2026-11-20, about 21:2x EST. The operator ruled, relayed
  by the Director and recorded in the workspace `RELEASE-PLAN.md` (lines
  1711 to 1717): "merge all tasks that have been locally properly tested directly
  into the epics lines and not through PRs, only PRs to main should be
  raised - either from the lines (mostly) or via tasks (when individual
  tasks are needed for certain reasons)". In CodeFlow's own repository
  this applies to every line, EPC-022 included, and to unmarked epics as
  well as marked ones:
  - A tested and reviewed task merges directly into its epic's line with
    no pull request. No new pull request into an integration line is
    opened.
  - Pull requests are raised only into `main`: from a line, mostly, or
    from a task that must land alone.
  - After each task landing, the landing owner checks `main` and, when it
    moved, merges `main` into the line.
  - For CodeFlow's own repository this replaces the table row "Pull
    request into a line (an unmarked epic's task)"; the Decision's
    sentence that a pull request into a line runs the short checks; the
    Decision's sentence that epics planned before the marker keep
    checkpoint pull requests until they close, and the matching sentence
    on ADR-0098 for unmarked epics, both now narrowed to the milestone
    line below; and the Consequences items on a pull request into a line
    and on two landing rules running side by side. The shipped product
    keeps the task pull request route for adopters' unmarked epics.
  - The marker still decides how a line reaches `main`: `whole-epic` once
    and complete, and `checkpoint-prs`, set explicitly, in checkpoint pull
    requests for a milestone line only (Director's rulings of 2026-11-20).
    EPC-022 is the one milestone line named today. It reaches `main`
    through its checkpoint pull request, carries no marker now, and takes
    `whole-epic` after the TSK-1513 sync and its next checkpoint. Every
    other epic line is a whole-epic line, marked by a planning range into
    `main`.

## Architecture impact

None. The maintainer page for CodeFlow's own placement is the "Where CI
suites run" section of `docs/releasing.md`, which TSK-1445 updates.
