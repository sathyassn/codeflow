---
id: ADR-0097
uid: 81e92713-da0c-41dd-8028-17c160c2534e
title: "CI suite placement by event"
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

# ADR-0097: CI suite placement by event

## Context

CodeFlow's hosted CI runs several suites: the Linux build, lint and tests,
Linux coverage, the Windows checks and journeys, the twelve Windows test
partitions and the release gates. Which event runs which suite was decided
by the operator in steps between 2026-11-01 and 2026-11-05. Those decisions
were recorded only in workspace notes and in task and epic criteria, and no
CodeFlow document said where each suite runs. Later rulings drifted from
the decisions as a result.


## Decision

Suite placement is decided by the event that starts the run. A pull request
into an integration line runs only the short checks: the Linux build and
lint checks, the policy check, commit standards, secret scan and release
impact. The Linux tests, Linux coverage and every Windows job leave line
pull requests. A line push, one per batch, runs the full Linux suites,
coverage included, and no Windows job. A pull request into `main` runs the
full Windows suites once and reuses Linux from the line push on the same
tree. A push to `main` reuses when the tree matches. The daily run on `main`
and each active line runs Linux and Windows, skipped when that tip was
already tested, and the weekly run is unchanged. The rule across events: a
tree that has passed a suite does not run it again, on a line or on `main`.
A reuse that cannot be proven runs the suite. A suite that is not selected for
an event never counts as a pass of a required check: GitHub treats a
skipped required check as a success, so a skip cannot stand in for one,
and TSK-1259 AC-9 holds that a not-selected result never satisfies a
required check. Once this placement lands, the `integration/**` ruleset is
to require only the short-check names, and `codeflow gates` and `windows`
stay required on `main`, where they run. The operator applies that change
as a script. An event or base the workflow cannot read fails
closed to the full run. Building this placement, the daily Linux
suites included, is assigned to TSK-1321 (PR 620), and the reuse to TSK-1026;
coord/ci amends those tasks' criteria to it.

Changing where a suite runs is an operator decision. Agents and
Coordinators never rule on it; they bring the evidence and a
recommendation to the operator.

Placement as decided on 2026-11-05 and amended on 2026-11-06:

| Event | Short checks | Linux build and lint | Linux tests | Linux coverage | Windows checks and journeys | Windows partitions | Release gates |
|---|---|---|---|---|---|---|---|
| Draft pull request, any base | secret scan and security review only; commit standards passes only on a ready run | no | no | no | no | no | no |
| Pull request into an integration line | yes | yes | no | no | no | no | no |
| Push to an integration line, one per batch | secret scan and security review | yes, or the pull request's reused verdict on an equal tree | yes | yes | no | no | no |
| Pull request into `main` | yes | reused from the line push on the same tree, else yes | reused from the line push on the same tree, else yes | reused from the line push on the same tree, else yes | yes, once | yes, once | yes |
| Push to `main` | secret scan and security review | reused when the tree matches, else yes | reused when the tree matches, else yes | reused when the tree matches, else yes | reused when the tree matches, else yes | reused when the tree matches, else yes | reused when the tree matches, else yes |
| Daily run, `main` and each active line | no | yes, skipped when that tip was already tested | yes, skipped when that tip was already tested | yes, skipped when that tip was already tested | yes, skipped when that tip was already tested | yes, skipped when that tip was already tested | no |
| Weekly run, `main` | secret scan of the full history and dependency audit | yes | yes | yes | yes | yes | yes |

How to read the table:

- The short checks are the five the operator named: the Linux build and
  lint checks, the policy check (`codeflow-policy.yml`, pull requests only)
  with its `commit standards` and `trusted: release impact` jobs,
  and the secret scan in `codeflow-ci.yml`.
- The security review is not on the operator's list. It stays on every
  event, line pull requests included, as a secure-default reading: the job
  has no event condition today (`codeflow-ci.yml`, `security-review`), and
  removing a security check needs its own operator decision.
- A "no" cell means the suite does not run for that event. It never means
  the suite's required check reports a pass. Which checks each base
  requires follows the ruleset change in the Decision. A "not selected" result exists only as a reported,
  non-evidence outcome where TSK-1259 and TSK-1190 define it.
- On this repository the Linux tests run inside the coverage shards, so the
  Linux tests and Linux coverage columns move together.
- The decision does not mention the release gates (the benchmark, the
  exhaustive prose checks and the present browser parts). This record reads
  them as outside "the full Linux and Windows suites", so they keep their
  current events, which the daily run is not one of; moving them is an
  operator decision.

TSK-1321 brings the workflows to this table. Its AC-12 adds a parity
check, `scripts/test_ci_placement.py`, that fails when the table, the
maintainer page's table or the workflows differ; that script is not on
`main` or the EPC-022 line until TSK-1321 (PR 620) lands. The daily workflows start from `main`
(`windows-daily.yml`, the `sources` job), and PR 597 (TSK-1314, merge
`df718e015`) moved each line's daily run under that line's own ref.

## Consequences

- A line pull request reaches its verdicts after the short checks, so it
  holds no Windows runner and no coverage shard. A line push holds no
  Windows runner either.
- A Linux test or coverage failure that a line pull request brings is found
  on the line push. A Windows failure is found on the daily run or on the
  pull request into `main`, so Windows proof of a change comes from one of
  those two, never from a line push. The operator accepted fixing such a
  failure forward on the line until the candidate push task lands.
- A batch of several pull requests pushes a tree no member tested, so its
  line push reuses nothing from them; a one-member landing on an equal tree
  reuses that member's short checks.
- Today a line push cannot reuse a line pull request's verdict, since that
  run ran the reduced Windows tier and a reduced-tier run is refused as
  proof of the partitions, so every line push runs the full suites,
  Windows included (`scripts/ci-reuse.py:319-323`). The line pull request
  still runs the Linux gate parts and the coverage shards today;
  `docs/releasing.md`, "Where CI suites run", separates today from after
  TSK-1321. TSK-1026 AC-7 and AC-12 give a line push job-by-job reuse of
  the jobs the pull request run ran and passed, which here are the short
  checks; AC-15 gives a pull request into `main` reuse of the line push's
  Linux jobs.
- Moving a suite between events is one recorded operator decision, added to
  the history below and to the maintainer page, never a ruling in a task or
  a sweep.

## Decision history

Each entry gives the operator's words where the record marks them as such,
then the record's own text, each quoted exactly. Entries are cited by their
date and time in the workspace repository's `RELEASE-PLAN.md` and
`coordination/HANDOFF-2026-10-27.md`, since both files grow and their line
numbers move.

- 2026-11-01, 19:00 to 19:15 EST. The record reads: "parallel decision 4
  approved as recommended (verdict reuse yes, path-scoped Windows skipping
  no)". Decision 5 was replaced by the operator's "run in CI only what must
  run hosted, prove the rest locally" (HANDOFF, entry "Operator inputs
  19:00 to 19:15 EST (11-01)").
- 2026-11-02, 19:35 EST. The record reads: "card 2 yes (uninstrumented
  Linux on integration PRs, coverage on PRs to main and nightly); card 3
  yes (Windows partitions to PRs to main once a daily exact-SHA run
  exists)" (HANDOFF, entry "2026-11-02 19:35 EST, operator: CI decisions approved
  as recommended"; also
  `RELEASE-PLAN.md`, entry "2026-11-02 19:35 EST, operator decisions").
- 2026-11-03, Windows switch order. Operator: "fine with A.". The record
  reads: "pull requests into a line drop the twelve partitions after one
  dispatched run of all twelve at the line tip's exact SHA; line pushes and
  pull requests into main keep them" (`RELEASE-PLAN.md`, entry
  "2026-11-03: Operator decision on the Windows switch order"; EPC-022
  AC-232; TSK-1242, TSK-1273).
- 2026-11-03, 19:32, Linux coverage. Operator: "on keep coverage on line -
  ok as recommended". The record reads: "line pull requests keep the
  coverage shards; this replaces card 2 of 2026-11-02 for line pull
  requests only" (`RELEASE-PLAN.md`, entry "2026-11-03: Operator decision
  on Linux coverage"; HANDOFF, entry "2026-11-03 19:32, operator";
  TSK-1241, TSK-1290, EPC-022 AC-232). The next entry replaces it for line
  pull requests.
- 2026-11-05, 15:57, this record. Operator: "A plus Linux off line PRs".
  The record reads: "pull requests into an integration line run only the
  short checks (Linux build and lint checks, policy check, commit
  standards, secret scan, release impact). Linux coverage, Windows checks,
  Windows journeys and the Windows release journey leave line PRs" and
  "Line pushes (the batch's hosted proof), PRs into main, and the daily and
  weekly runs keep the full Linux and Windows suites" (`RELEASE-PLAN.md`,
  entry "2026-11-05 15:57: Operator decision on CI placement").
- 2026-11-06, 15:34, this record. Operator: "agreed with youre latest
  recommendation" and "get this to main sooner ... so we don't revisit
  this". For line pushes this replaces the previous entry; everything else
  in that entry stands. The record reads: "A push to a line, one per batch,
  runs full Linux and no Windows. A PR into main runs full Windows once and
  reuses Linux from the line push on the same tree. A push to main reuses
  when the tree matches." The daily run covers `main` and active lines,
  skipped when that tip was already tested, and "a tree that has passed a
  suite does not run it again, on a line or on main" (`RELEASE-PLAN.md`,
  entry "2026-11-06 15:34: Operator decision on CI placement").

## Architecture impact

None. The maintainer page for this placement is the "Where CI suites run"
section of `docs/releasing.md`.

## Note, 2026-11-06

Amended 2026-11-06 before this record landed, from the CI/CD coherence
result of 2026-11-06 (workspace
`coordination/briefs/cicd-coherence-result-2026-11-06.md`, finding 3, and
Grok's finding 9). The Decision said a required check passes when its
suite is not selected; GitHub treats a skipped required check as a
success, so that text is removed. The Decision and the reading of the
table now say that a not-selected suite never passes a required check, and
that the `integration/**` ruleset requiring only the short-check names,
with `codeflow gates` and `windows` required on `main`, depends on the
operator's decision 3 in that result. After the PR 631 review of
2026-11-06 (Grok, finding 1), the Consequences bullet on line-push reuse
gives the real reason a line push runs the full suites: the pull request
ran the reduced Windows tier, which `scripts/ci-reuse.py` refuses as proof.

Amended again on 2026-11-06, still before this record landed, for the
operator's decision of 15:34: Windows leaves line pushes. The Decision, the
table, the Consequences and the history now say so. This record moved from
PR 631 to the records pull request into `main` so the placement reaches
`main` ahead of the line.
