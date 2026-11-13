---
id: ADR-0096
uid: 93b551be-8a61-4102-8967-6fc684b79ad0
title: "Tasks carry a graded time band, and CodeFlow runs on it"
date: 2026-11-04
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "docs/architecture.md, the optional estimation row: an adopted project may turn on the time-box profile, whose bands frozen per task are a stop rule beside the scenario forecasts"
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0096: Tasks carry a graded time band, and CodeFlow runs on it

## Context

No planned task carried an estimate. The only pace rule was a flat
throughput rule for every build lane, set by the operator on 2026-10-23.
It rewarded small tasks and flagged nearly every larger one. `cf-estimate`
forbade a default band (`operating-model.md:46-49`), so a project had no
per-task budget to hold work against. An adopter ran issue 334's method
locally: a band per graded task, one clock and a stop rule at the band's
top. On 2026-11-01 the operator decided that issue 334's bands and clock
are CodeFlow's starting model (decisions D1, D2 and P3.5), opt-in for
adopters (P2.4).

## Decision

`cf-estimate` ships an optional time-box profile beside the scenario
forecast (`operating-model.md`, "Time-box profile"):

- **One clock.** It starts at the claim commit, or the earlier first task
  commit for an adopted branch, and ends at the later of the bound approval
  and the completion of verification, less recorded blocked spans. The
  landing stands in for the end only by ancestry, when a records change
  wrote the completion later.
- **Bands by rubric grade.** Easy 1 to 2 hours, Medium 2 to 4, Hard 4 to 6,
  Very Hard 6 to 8. The planner may move a band one step, never two, for a
  reason the drivers miss (proof volume, record volume, a key review set);
  the rubric points stay as scored. A Very Hard task is split before claim.
- **Frozen at breakdown.** The band is written in the task's Estimate block
  with the profile revision. No task created before the profile's
  effective date carries one, and none is back-filled.
- **A stop rule, never a gate.** A Hard task logs one line at half its top.
  At the top, new work stops and the builder chooses a finish of at most 30
  minutes (a miss), a split or a re-plan, with a cause line. Hits, misses
  and breaches are reported apart from forecasts and never fail a check.
- **Template parts.** The spec's Delivery size block, the epic's Estimate
  section and the task's Estimate block ship as optional, commented parts
  of the templates. A project that leaves them out validates as before.
- **Offered once.** `cf-plan` offers the profile once at the first
  breakdown when no `.codeflow/estimate.json` exists. A decline is recorded
  with its reason and re-offer event and honoured until that event.
- **CodeFlow adopts it.** CodeFlow's own `.codeflow/estimate.json` is
  adopted with `project-management/estimates/profile-v1.md`.
- **The pace rule is replaced.** "Planned work keeps to its band" in the
  `cf-method` workflow lifecycle reference replaces the throughput decision
  of 2026-10-23. A claim means work begins now; a lane that cannot start
  records `blocked`; an abandoned claim is handed over on its branch, never
  deleted.

## Alternatives

- **Two clocks with p75 targets from a size proxy.** Closed in the
  proposal's revision 4: two clocks double the reading and the proxy did
  not track the work.
- **Default-on for new scaffolds, or for every project through `update`.**
  Rejected: it writes a per-task record into projects whose operator never
  approved it, and it breaks `records.md`'s rule that neither `init`,
  `update` nor the checker writes adoption.
- **No bands.** The status quo leaves no task a clock and keeps the flat
  throughput rule.

## Consequences

- Every planned task in an adopted project has a budget and a stop rule,
  and a coordinator sees a stalled lane by the task's own clock.
- The epic's outcome record, written on the line at the epic close
  (`cf-method` "Finish once") so it reaches `main` in the line's PR before
  the line is deleted, feeds the next feature's estimate.
- A project that declines sees no block, no warning and no re-offer until
  its recorded event.
- The rubric gains clarifications and a never-counts list, as version
  `codeflow-agentic-2`; its points and cuts are unchanged. Its anchored
  examples wait for an independent two-lineage check (TSK-1331).
- The first prospective evaluation of the bands is CodeFlow's own first
  epic close under the profile; the revisit of default-on waits for it.
- Until TSK-1171, TSK-1172 and TSK-1173 land, `codeflow estimate outcomes`
  does not yet compare a task with its band or read the clock's end points;
  the references say what a reader does today.

## Amendments

Each is a dated Note on the accepted record or reference, whose text is
otherwise unchanged (the ADR-0076 form):

- **ADR-0057, Consequences.** The scaffold ships a profile template with
  starting bands inside `cf-estimate`; no profile is installed and nothing
  is written without the project's confirmation.
- **ADR-0057:48** ("Future verbs need a new justified decision"). `work
  claim` gains one write (the `claimed:` line) and worktree creation; no new
  verb is added.
- **ADR-0079:42-44, started.** A `claimed:` line written by `work claim` is
  the start when present and survives a squash. For an adopted or renamed
  branch, started is the earlier of that line and the first commit after
  the claim base.
- **ADR-0079:45-47, landed.** The end may be the later of a bound approval
  point and a verification point when the task's Estimate block carries
  them; the landing stands in only by ancestry.
- **ADR-0079:51-53, what the report reads.** It gains `--ref <line>`, and a
  landing found from commits reachable from `HEAD` when a target ref is
  gone. Waits are still never inferred (`ADR-0079:50`).
- **`operating-model.md:46-49`.** The time-box profile's bands are the
  starting bands of a project that turns it on: a stop rule, not a
  forecast and not a universal conversion.
- **`rubric.md:87-91`.** Under a time-box profile a grade maps to the
  profile's band, a budget with a stop rule, not a prediction.
- **`workflow-lifecycle.md`, the per-hour pace section.** Superseded by
  "Planned work keeps to its band".

## Unchanged

- ADR-0057's adoption shape and record home.
- ADR-0079's read-only property: `estimate outcomes` writes nothing, and
  the claim line is written by the work graph verb.
- ADR-0076: the band adds no PR, round or approval; the Estimate block
  rides in the planning PR that creates the task.
