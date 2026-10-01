---
id: ADR-0007
uid: 5354e7b9-89ae-4e8a-b8b1-364235568792
title: "agent and human merge boundary: protected branch merge and ref controls"
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — doctor check count 6→7 (adds repo-integrity); the pre-merge-commit and reference-transaction git-hook shims in the enforcement-planes description
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0007: agent/human merge boundary

## Context

Protected branches were guarded against agent commits, pushes, force-pushes,
deletes, and hard resets, but not against *merges* — and several protections
lived only in the Claude `git-guard` because classic client hooks never fire
for the underlying operation. A raw `git merge` was caught only incidentally
(it rode `commit_to_protected`); a `gh pr merge` into `main` was not caught at
all; a fast-forward merge, a `reset --hard`, and a local `branch -D` on a
protected branch reached no git hook, so any non-Claude harness slipped past
them. There was also no sanctioned way for a human to land a merge locally
other than `codeflow integrate`.

A `core.bare=true` corruption of the root repository (its working tree still
present) prompted the hardening. It was first correlated with
`gh pr merge --delete-branch` issued while the PR branch was checked out in a
worktree — but it then reproduced a **third** time with no `--delete-branch`
involved, falsifying that command as the sole cause. The common factor across
all reproductions is a `git worktree` operation interrupted mid-flight by a
macOS sandbox `EPERM` and retried; the exact mechanism is unconfirmed. The
response is therefore symptom-based, not trigger-based.

The requirement (owner directive): all git-related protection is
harness-agnostic via git hooks, with the Claude layer a bonus on top;
protected branches are protected from local *and* remote merges by agents;
humans can override; every non-protected branch stays fully free for agents
(merge and force-push allowed).

## Decision

Add three policy keys — `git.merge_to_protected`, `git.pr_merge_to_protected`,
and `git.local_ref_protection` (levels `block|warn|allow` — `local_ref` uses
`block|warn|off` — default `block`), read by all planes; the `--minimal` tier
softens them to `warn` with the siblings. Enforce them:

- **pre-merge-commit git hook** — a new client-hook stage blocks a
  non-fast-forward merge *commit* onto a protected branch per
  `merge_to_protected`.
- **reference-transaction git hook** — a new client-hook stage (git ≥ 2.28)
  that fires on ref updates classic hooks miss. On the `prepared` state it
  enforces `local_ref_protection` for every `refs/heads/<protected>` update:
  it **closes the fast-forward-merge gap**, and catches `reset --hard` and
  `branch -D` on a protected branch. A move to — or behind — the
  `refs/remotes/origin/<branch>` head is a legitimate sync (`git pull`, fetch
  fast-forward, branch creation from the remote) and passes; any other local
  move blocks. Deletion of a protected branch is governed by `delete_protected`,
  now reachable here too. A fast path returns success before loading policy for
  any ref outside `refs/heads/`, so a fetch touching hundreds of remote-tracking
  refs stays cheap.
- **git-guard (Claude layer)** — `git merge`/`git cherry-pick` onto protected
  now rides `merge_to_protected`; `gh pr merge` is blocked when its base is
  protected (cheap, bounded `gh pr view` base introspection — allow only a
  provably-unprotected base, block on any doubt).

Two sanctioned exits, honored by the **git layer only**: the existing
`CODEFLOW_INTEGRATE_TOKEN` (unchanged — `codeflow integrate` stays the offline
path), and a new **human-only** `CODEFLOW_HUMAN_OVERRIDE=1` that lets a person
run `CODEFLOW_HUMAN_OVERRIDE=1 git merge …` from their own terminal. The Claude
`git-guard` never consults the human override — an agent does not get to claim
humanity — and actively blocks any in-session attempt to *set* either override
token (env-prefix, `export`, or `env`): setting an override token in-session is
bypass, not override. `git commit/merge/push --no-verify` targeting a protected
branch is likewise blocked as a bypass.

`gh pr merge --delete-branch` is blocked outright, regardless of base. It is
not the confirmed cause of the `core.bare` corruption, but it demonstrably
moves off a branch that may be checked out in a worktree, and is retained as
hygiene. The `repo-integrity` doctor check is deliberately **symptom-based** so
it catches the bad state regardless of cause: it fails on `core.bare=true` on a
repo with a working tree, and on a protected branch checked out in a non-root
worktree.

Git < 2.28 never invokes the reference-transaction hook; the local-ref
protection then degrades to the other planes (pre-merge-commit for merge
commits, git-guard in-session, push gate and remote), gracefully and silently.

## Consequences

- Protected-branch merges and local ref rewrites (ff-merge, `reset --hard`,
  `branch -D`) are now blocked for **any** harness at the git layer, not just
  in a Claude session; humans retain a clean local override.
- `git pull`/fetch-sync of a protected branch stays legal for everyone (the
  sync-allow rule), so the hook does not break normal syncing.
- After this lands, an agent-performed `gh pr merge` into a protected base is
  blocked — the integrator or user merges subsequent PRs (GitHub UI / their own
  terminal), or explicitly sanctions the path.
- The anti-laundering, `--delete-branch`, and `--no-verify` blocks are
  structural (always block), not policy-configurable: they guard the integrity
  of the enforcement system and the repo layout, which sit above the per-repo
  dial.
- These local planes are fast, harness-agnostic feedback; CI and remote
  protection remain the authoritative perimeter (charter §6.5). PR-content
  checks (attribution/emoji in a PR body, base-branch of a `gh pr merge`) are
  CI-plane and git-guard concerns by design — git hooks cannot see PR creation.

## Architecture impact

`docs/architecture.md` updated in this PR: the doctor check count moves from 6
to 7 (adds `repo-integrity`), and the enforcement-planes description names the
new `pre-merge-commit` and `reference-transaction` git-hook shims.

## Note — 2026-07-02 (appended)

Framing clarification, no mechanism change: the scaffold's git-rules guidance
(the AGENTS.md managed block) was reframed to say plainly that local git hooks
and `git-guard` are fast in-session feedback, while CI and remote branch
protection are the authoritative, server-enforced perimeter. `CODEFLOW_HUMAN_OVERRIDE`
is a local convenience for a human at their own terminal, not authentication —
any agent sharing the host can set an env var — and is contained only because
that authoritative boundary is remote.

## Note — 2026-07-03 (appended)

Root cause of the `core.bare` corruption identified — this supersedes the
"mechanism unconfirmed" / worktree-interruption correlation in the Context
above. Git exports `GIT_DIR`, `GIT_WORK_TREE`, and `GIT_INDEX_FILE` to hook
subprocesses; the pre-push `test_gate_on_push` ran `cargo test` with those
inherited, so the workspace's git-spawning tests mutated the real repo instead
of their own tempdirs. Fixed by clearing the three vars in the test-gate runner
and the test git helpers. The symptom-based `repo-integrity` doctor check stands
regardless of trigger. Detail: `docs/plan/v2/01-execution-status.md`.
