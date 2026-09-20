---
id: ADR-0006
title: PR based landings supersede the integrate path on this repo
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0006: PR-based landings on this repo

## Context

ADR-0002 set `git.push_to_protected = "warn"` here because this repo is
private on GitHub Free (no remote branch protection) and the locally
`codeflow integrate`d main could not otherwise sync to origin. The entire
v2 rebaseline (~130 commits) landed through integrate as a result — which
works, but leaves no pull-request trail: the GitHub PR tab shows nothing,
there is no per-landing review surface, and CI verdicts are not attached
to a visible merge decision. The user asked for more transparency.

## Decision

Land work on protected branches via pull requests only:

- `git.push_to_protected` returns to `"block"` — the pre-push hook and
  git-guard now force the PR path locally, compensating for the missing
  server-side protection on this plan.
- Flow: feature branch → push branch → `gh pr create` (template) → CI
  green → `gh pr merge --squash` (linear history preserved).
- `codeflow integrate` remains a shipped product capability (the
  offline/no-remote sanctioned path, charter D9); it is retired for this
  repo's day-to-day landings.
- Auto-merge-on-green is unavailable on private+Free (requires branch
  protection); merges are explicit after CI passes. Revisit if the repo
  goes public.

## Consequences

- Every landing gains a browsable PR: diff, CI checks, review comments.
- Slightly more ceremony per landing; merge is a manual step after green.
- ADR-0002 is superseded by this decision.

## Architecture impact

None — a policy value and process change; no code changes.

## Correction — 2026-07-13: merge method superseded by ADR-0020

The Decision above prescribes `gh pr merge --squash`. ADR-0020 later made this
repo **rebase-only**: squash and merge-commits are disabled at the GitHub level
(`squashMergeAllowed=false`, `mergeCommitAllowed=false`,
`rebaseMergeAllowed=true`), so each commit lands on `main` exactly as authored
and the per-commit standard is never papered over by a squash. Follow ADR-0020
for the merge method; the PR-based-landings decision here otherwise stands.
(Append-only note; the decision body above is unchanged.)
