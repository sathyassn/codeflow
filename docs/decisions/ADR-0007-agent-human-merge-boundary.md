---
id: ADR-0007
title: agent/human merge boundary — protected-branch merge controls
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — doctor check count 6→7 (adds repo-integrity) and the pre-merge-commit git-hook shim in the enforcement-planes description
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0007 — agent/human merge boundary

## Context

Protected branches were guarded against agent commits, pushes, force-pushes,
deletes, and hard resets, but not against *merges*. A raw `git merge` was
caught only incidentally (it rode `commit_to_protected`), a `gh pr merge` into
`main` was not caught at all, and there was no sanctioned way for a human to
land a merge locally other than `codeflow integrate`. Two production incidents
sharpened the need: a `gh pr merge --delete-branch` issued while the PR branch
was checked out in a worktree flipped the root repository to `core.bare=true`
(reproduced twice). The requirement: protected branches are protected from
local *and* remote merges by agents; humans can override; every non-protected
branch stays fully free for agents (merge and force-push allowed).

## Decision

Add two policy keys, `git.merge_to_protected` and `git.pr_merge_to_protected`
(levels `block|warn|allow`, default `block`), read by all planes; the
`--minimal` tier softens them to `warn` with the siblings. Enforce them:

- **pre-merge-commit git hook** — a new client-hook stage blocks a merge
  commit onto a protected branch per `merge_to_protected`.
- **git-guard (Claude layer)** — `git merge`/`git cherry-pick` onto protected
  now rides `merge_to_protected`; `gh pr merge` is blocked when its base is
  protected (cheap, bounded `gh pr view` base introspection — allow only a
  provably-unprotected base, block on any doubt).

Two sanctioned exits from the merge block, honored by the **git layer only**:
the existing `CODEFLOW_INTEGRATE_TOKEN` (unchanged — `codeflow integrate`
stays the offline path), and a new **human-only** `CODEFLOW_HUMAN_OVERRIDE=1`
that lets a person run `CODEFLOW_HUMAN_OVERRIDE=1 git merge …` from their own
terminal. The Claude `git-guard` never consults the human override — an agent
does not get to claim humanity — and actively blocks any in-session attempt to
*set* either override token (env-prefix, `export`, or `env`): setting an
override token in-session is bypass, not override. `git commit/merge/push
--no-verify` targeting a protected branch is likewise blocked as a bypass.
`gh pr merge --delete-branch` is blocked outright, regardless of base, because
it can flip a worktree-checked-out root to `core.bare`. A new `repo-integrity`
doctor check detects both symptoms (bare flag on a working repo; a protected
branch checked out in a non-root worktree) regardless of trigger.

HONEST BOUNDARY: git fires **no** client hook for a fast-forward merge (no
merge commit is created), so a ff-merge onto a protected branch is caught only
by the in-session git-guard; the push gate plus remote protection remain the
perimeter for other harnesses (charter D19). The override tokens and the
`--delete-branch`/`--no-verify`/laundering blocks are discipline and structural
safety aids for agents, not a security boundary — CI and remote protection are
the hard line (charter §6.5).

## Consequences

- Agent-performed merges onto protected branches are now uniformly blocked
  across the git hook and Claude planes; humans retain a clean local override.
- After this lands, an agent-performed `gh pr merge` into a protected base is
  blocked — the integrator or user merges subsequent PRs (GitHub UI / their own
  terminal), or explicitly sanctions the path.
- The anti-laundering and `--delete-branch`/`--no-verify` blocks are structural
  (always block), not policy-configurable: they guard the integrity of the
  enforcement system and the repo layout, which sit above the per-repo dial.
- The ff-merge gap on the git plane is accepted and documented, not papered
  over; the perimeter still holds because the result cannot be pushed.

## Architecture impact

`docs/architecture.md` updated in this PR: the doctor check count moves from 6
to 7 (adds `repo-integrity`), and the enforcement-planes description names the
new `pre-merge-commit` git-hook shim.
