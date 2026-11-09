---
id: ADR-0100
uid: fd62e1f9-e756-4115-9252-badc57a861f7
title: "Projects choose up-to-date branches, no requirement, or a merge queue"
date: 2026-11-08
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0100: Projects choose up-to-date branches, no requirement, or a merge queue

## Context

ADR-0081 made "require branches to be up to date before merging" part of
the protection CodeFlow ships: `codeflow remote protect` always writes
strict required checks, and `codeflow doctor --check remote-perimeter`
warns whenever the live rules are not strict. A project that turns the
setting off on its host gets a warning that never clears, and no shipped
workflow ran for a merge queue entry, so a merge queue could not replace
the rule (ADR-0091: "Merge queues are not covered until `merge_group` is
wired"). The operator's direction (relayed 2026-11-08, TSK-1396): the
strict rule stays on by default, a project can opt out, and a merge queue
is the alternative where the host offers one. Choosing anything but the
default relaxes a shipped protection.

## Decision

A policy key, `git.require_up_to_date`, takes `true` (the default, also
when absent), `false` or `"merge_queue"`. `remote protect` writes strict
checks under `true`; checks that are not strict under `false`; and under
`"merge_queue"` GitHub's `merge_queue` rule with non-strict checks on the
default branch, while the integration lines and other protected branches
keep strict checks. A value that relaxes the rule counts only when it is
committed on the remote's default branch and read through plain
remote-tracking refs, as `security.text_guard: relaxed` is (ADR-0088): a
working-copy value, a local commit, an environment value, a repository
without a remote and an authority that cannot be read all keep `true`,
and a working copy that sets `true` keeps it. An update never lowers a
live strict rule. A merge queue is refused, with the reason and the strict
rule kept, for a repository a user account owns, an owner `gh` does not
report, `--check-app` and a provider other than GitHub; when the host
refuses the queue rule, the strict rule is written instead. `doctor`
accepts what the value chooses and names it in its message. The shipped
CI and policy workflows trigger on `merge_group` and run every job for a
queue entry: the policy job judges from the queue's base, which must be on
the default branch, and treats the queue commit as data; the installed
release and the trusted inputs come from the queue's base; and a queue
entry never shares a concurrency group with a pull request or another
entry. This amends ADR-0081, whose rule is now the default, and ADR-0091,
whose `trusted:` jobs may also run on `merge_group`.

## Consequences

- A project can turn the up-to-date rule off without a standing doctor
  warning, and the report says why the rule is relaxed and where the value
  was read.
- Under `false`, nothing stops two pull requests green on an older base
  from merging and leaving the branch red; whoever merges confirms the
  checks ran on the current base. That is the cost the project chose.
- Under `"merge_queue"`, GitHub tests each entry on the base it lands on,
  and the CI and policy workflows run again for every entry. GitHub offers
  merge queues only to organization-owned repositories, on a plan that
  includes them.
- GitHub runs a queue entry's workflow file from the queue commit, so a
  pull request that edits a workflow sees its edit run in the queue before
  it lands. The pull request's own policy run, from the default branch,
  judged that edit first, and the queue's required checks still need the
  `trusted:` names. A project that relies on the pull_request_target-only
  guarantee for every verdict keeps `true`.
- The queue rule builds one entry at a time. GitHub documents a queue
  entry's `base_sha` as its parent commit, which for an entry built behind
  another is the earlier entry's queue commit, and the policy job refuses
  a base that is not on the default branch. A deeper build would fail
  every entry after the first, so throughput is one entry per CI run.
  This was not verified on a hosted queue.
- A pull request body is not judged again in the queue: a queue entry has
  none, and the pull request's own run judged it.
- A user-owned repository cannot choose a merge queue; the run says so and
  keeps the strict rule.
- This repository's own `main` rules stay the operator's; its workflow
  copies take `merge_group` through an operator patch.

## Architecture impact

none
