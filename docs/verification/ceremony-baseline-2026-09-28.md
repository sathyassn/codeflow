# Ceremony baseline, pull requests 568 to 644

## Scope

This is the recorded baseline of the ceremony report (TSK-149 AC-4): the
process cost of the 77 pull requests numbered 568 to 644, all merged into
the epic lines on 2026-09-27. A release compares its own window with it
(`docs/releasing.md`, "Ceremony check before a release"). The numbers are
information; nothing gates on them.

## How to reproduce it

From a clone of this repository that has fetched every epic line
(`git fetch origin`), run:

```sh
codeflow report ceremony --prs 568..644
```

The pull request counts come from the merge commits in the clone's refs, so
they are the same in every clone that has fetched those lines. Review rounds
come from the host through `gh`. Refusals come from the clone's own ledger.

## The report

Run on 2026-09-28 in `.worktrees/tsk-149` at `dac6be863`, with `gh`
authenticated to `sathyassn/codeflow-archive`:

```text
Ceremony report: pull requests 568 to 644
  77 merged, 2026-09-27T04:35:13Z to 2026-09-27T17:19:02Z, read from this clone's refs

Pull requests per logical change
  kind             pull requests  logical changes  per change
  docs                         2                2        1.00
  fix                         20               20        1.00
  plan                        12               12        1.00
  task                        26               25        1.04
  record status               17                -           -
  all                         77               59        1.31
  A task's pull requests are one change. A record status pull request changes
  only a record's status, acceptance evidence or closeout.
  Record status pull requests: 576, 579, 591, 594, 597, 602, 609, 612, 616, 619, 623, 628, 630, 633, 639, 640, 641

Review rounds per pull request: unknown (the host holds no submitted review for these 77 pull requests)
Refusals hit by this clone's hooks and guards: unknown (this clone has recorded none; hooks and guards record from their first run of this version)
```

## The baseline

| Measure | Baseline |
|---|---|
| Merged pull requests | 77: 28 from `plan/`, 26 from `task/`, 20 from `fix/`, 3 from `docs/` |
| Record status pull requests | 17 |
| Logical changes | 59 |
| Pull requests per logical change | 1.31 (77 over 59); 1.02 without the 17 record status pull requests (60 over 59) |
| Review rounds per pull request | unknown |
| Refusals hit by own operations | unknown |

## Against the planning count

The task's planning count (TSK-149, Description) was made by hand. The
report matches it on the pull requests and differs on the record status
count; the difference is reported here, not forced.

| Measure | Planning | Report | Difference |
|---|---|---|---|
| Pull requests | 77 | 77 | none |
| By branch prefix | 28 plan, 26 task, 20 fix, 3 docs | the same: the table's 12 plan and 2 docs rows plus 16 plan and 1 docs record status pull requests | none |
| Record status or ownership only | 20 | 17 | 614, 618 and 627 |
| Review rounds | not counted | unknown | none |
| Refusals | unknown, never recorded | unknown | none |

- 614, 618 and 627 moved ownership rows of SPC-013's R-118 table (618 also
  edited TSK-129's affected surfaces). The report's rule reads a record's
  structure: a status field, acceptance evidence, a closeout. An ownership
  row is spec text, and 617, 620 and 634 edit the same section of the same
  spec as real planning changes, so no structural rule separates them. They
  count here as planning pull requests.
- The 17 are exactly the other 17 of the planning list, so the rule and the
  hand count agree on every status move.

## Why review rounds are unknown

The host holds no submitted review for any pull request in the window: a
GraphQL count of reviews in the states `APPROVED`, `CHANGES_REQUESTED`,
`COMMENTED` and `DISMISSED` returns 0 for all 77. Reviews on this line are
run by the review seats and written into each pull request body's Reviews
section as prose ("rounds 1 to 3 changes requested; round 4 approved"), in
tables for only 6 of the 77. The report does not parse that prose: a count
read from free text would be an estimate, and R-103 says to print `unknown`
instead. Review rounds become measurable when reviews are submitted on the
host or the Reviews section carries one row per round.

## Cross-check against the host

The pull request list was checked against GitHub separately:

```sh
gh pr list --state all --limit 400 \
  --json number,headRefName,baseRefName,state,mergedAt,mergeCommit
```

All 77 are `MERGED`; 60 into `integration/EPC-020-delivery-system`, 10 into
`integration/EPC-016-visual-guide`, 5 into
`integration/EPC-018-autonomy-roster` and 2 into
`integration/EPC-014-public-docs`. Each merge commit is in this clone with
two parents and the subject `Merge pull request #N from <branch>`, so the
report's history source misses none of them.
