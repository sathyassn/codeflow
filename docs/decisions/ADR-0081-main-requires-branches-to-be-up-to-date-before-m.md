---
id: ADR-0081
uid: 8336ca3b-7fed-4415-a2d0-d4155b7145b9
title: "Main requires branches to be up to date before merging"
date: 2026-10-08
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: docs/architecture.md, the doctor check table names remote-perimeter # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0081: Main requires branches to be up to date before merging

## Context

On 2026-10-08 `main` went red although every pull request that landed had
green required checks. A pull request check tests the merge of its branch
with `main` as it was when the run started; PRs 97, 109 and 112 were each
green on a `main` from before PR 93 and merged after it, untested together.
The `main` ruleset (24379074) requires six checks with
`strict_required_status_checks_policy: false`, so GitHub accepted a green
about an older `main`. ADR-0006 left host protection to "Revisit if the repo
goes public"; the repository is public, and merge queues need an
organization-owned repository ("Merging a pull request with a merge queue",
docs.github.com, 2026-10-08).

## Decision

`main` requires branches to be up to date before merging: the ruleset that
requires its checks sets "Require branches to be up to date before merging",
so a pull request that is behind `main` is updated and its checks run again
before it can merge. The operator turns the setting on; nobody runs `codeflow
remote protect` on this repository, whose rules were made by hand. CodeFlow
ships the same protection to adopters: `codeflow remote protect` requires the
checks in `git.required_checks` (default: the shipped CI job names) on an
up-to-date branch on both host paths, updates every active ruleset that
already targets the branch in place, and `codeflow doctor --check
remote-perimeter` reads the live rules back and warns when they are not
strict. This supersedes the
"Revisit if the repo goes public" line of ADR-0006; the rest of ADR-0006
stands. Integration lines keep `codeflow integrate` and the batch candidate.

## Consequences

- No pull request merges into `main` on checks about a different `main`;
  any test added on `main` runs against every pull request before it lands.
- Each landing behind `main` costs one update and one fresh hosted run, and
  landings serialize at CI latency. When several ready pull requests should
  land in a day, they go on one integration candidate with one full gate.
- A clean update keeps a completed task's acceptance block only when the pull
  request did not reopen its task (SPC-013 R-60); a hand-resolved update
  invalidates it. A pull request that reopened its task inside its own range
  never takes that allowance, so after the update it needs a new review and a
  new acceptance binding (SPC-013 R-60, R-119).
- The rule binds the operator too, since the ruleset has no bypass actors.
- Merge queue stays out of reach unless the repository moves to an
  organization, which is the operator's call.

## Architecture impact

`docs/architecture.md`: the doctor check table names `remote-perimeter`
(21 checks).
