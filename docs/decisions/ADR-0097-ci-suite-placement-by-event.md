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
impact. Linux coverage and every Windows job leave line pull requests. Line
pushes, which are the landing batch's hosted proof, pull requests into
`main`, and the daily and weekly runs keep the full Linux and Windows
suites. The daily run also measures Linux coverage. A required check passes
when its suite is not selected for the event, through the reserved "not
selected" result (TSK-1259, TSK-1190), and an event or base the workflow
cannot read fails closed to the full run. TSK-1321 (PR 620) owns building
the line pull request placement and the daily coverage.

Changing where a suite runs is an operator decision. Agents and
Coordinators never rule on it; they bring the evidence and a
recommendation to the operator.

Placement as decided on 2026-11-05:

| Event | Short checks | Linux build, lint and tests | Linux coverage | Windows checks and journeys | Windows partitions | Release gates |
|---|---|---|---|---|---|---|
| Draft pull request, any base | secret scan and security review; commit standards passes only on a ready run | no | no | no | no | no |
| Pull request into an integration line | yes | build and lint only | no | no | no | no |
| Push to an integration line | secret scan and security review | yes | yes | yes | yes | no |
| Pull request into `main` | yes | yes | yes | yes | yes | yes |
| Push to `main` | secret scan and security review | yes, or the merged pull request's reused verdict | yes, or reused | yes, or reused | yes, or reused | yes, or reused |
| Daily run, `main` and each active line | no | no | yes | yes | yes | no |
| Weekly run, `main` | secret scan of the full history and dependency audit | yes | yes | yes | yes | yes |

"Short checks" means the Linux build and lint checks, the policy check
(`codeflow-policy.yml`, pull requests only) with its `trusted: commit
standards` and `trusted: release impact` jobs, and the secret scan in
`codeflow-ci.yml`. The security review runs on every event beside them. On
this repository the Linux tests run inside the
coverage shards, so leaving coverage off line pull requests also leaves the
Linux tests off them.

Until TSK-1321 lands, the line at `1107c5974` differs from this table:

- Line pull requests still run every Linux gate part and both coverage
  shards with their report (`codeflow-ci.yml:251`, `:562`, `:645`).
- Line pull requests still run the Windows checks, the release journey and
  the reduced tier's journey job; only the build, the partitions and their
  journey job are skipped, by the reduced tier switch
  (`codeflow-ci.yml:758`, `REDUCED_TIER: "on"`).
- The daily run is Windows only (`windows-daily-run.yml`) and measures no
  Linux coverage.
- The daily workflows start only from `main` (`windows-daily.yml`, the
  `sources` job), and `main` does not hold them until the EPC-022 line
  merges, so no daily run exists yet. PR 597 (TSK-1314, merge `df718e015`)
  moved each line's daily run under that line's own ref.

## Consequences

- A line pull request reaches its verdicts after the short checks, so it
  holds no Windows runner and no coverage shard.
- A Windows or coverage failure that a line pull request brings is found on
  the line push, the daily run or the pull request into `main`. The
  operator accepted fixing such a failure forward on the line until the
  candidate push task lands.
- A line push cannot reuse a line pull request's verdict, since that run
  skipped the suites the push needs, so every line push runs the full
  suites (`scripts/ci-reuse.py`).
- Moving a suite between events is one recorded operator decision, added to
  the history below and to the maintainer page, never a ruling in a task or
  a sweep.

## Decision history

Each entry gives the operator's words where the record marks them as such,
then the record's own text, each quoted exactly. The workspace repository
holds `RELEASE-PLAN.md` and `coordination/HANDOFF-2026-10-27.md`.

- 2026-11-01, 19:00 to 19:15 EST. The record reads: "parallel decision 4
  approved as recommended (verdict reuse yes, path-scoped Windows skipping
  no)". Decision 5 was replaced by the operator's "run in CI only what must
  run hosted, prove the rest locally" (`HANDOFF-2026-10-27.md:1504-1507`).
- 2026-11-02, 19:35 EST. The record reads: "card 2 yes (uninstrumented
  Linux on integration PRs, coverage on PRs to main and nightly); card 3
  yes (Windows partitions to PRs to main once a daily exact-SHA run
  exists)" (`HANDOFF-2026-10-27.md:382-385`; also `RELEASE-PLAN.md:1279`).
- 2026-11-03, Windows switch order. Operator: "fine with A.". The record
  reads: "pull requests into a line drop the twelve partitions after one
  dispatched run of all twelve at the line tip's exact SHA; line pushes and
  pull requests into main keep them" (`RELEASE-PLAN.md:78`; EPC-022
  AC-232; TSK-1242, TSK-1273).
- 2026-11-03, 19:32, Linux coverage. Operator: "on keep coverage on line -
  ok as recommended". The record reads: "line pull requests keep the
  coverage shards; this replaces card 2 of 2026-11-02 for line pull
  requests only" (`RELEASE-PLAN.md:76`; `HANDOFF-2026-10-27.md:691`;
  TSK-1241, TSK-1290, EPC-022 AC-232). The next entry replaces it for line
  pull requests.
- 2026-11-05, 15:57, this record. Operator: "A plus Linux off line PRs".
  The record reads: "pull requests into an integration line run only the
  short checks (Linux build and lint checks, policy check, commit
  standards, secret scan, release impact). Linux coverage, Windows checks,
  Windows journeys and the Windows release journey leave line PRs"
  (`RELEASE-PLAN.md:1287`).

## Architecture impact

None. The maintainer page for this placement is the "Where CI suites run"
section of `docs/releasing.md`.
