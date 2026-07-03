---
id: ADR-0009
title: human authorization is out-of-band; local guards are the honest-agent floor
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: none
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0009 — human authorization out-of-band; local guards are the honest-agent floor

## Context

An adversarial review of the v2 guard/hook planes reproduced, live, a set of
self-disarm and evasion holes in the Claude `git-guard` and the git-hook plane:
the guard never inspected `git config core.hooksPath` / `git -c
core.hooksPath=` / hook-skip env prefixes, nor Bash writes to the hook shims and
policy files, so an agent could quietly disarm the plane; the
`reference-transaction` stage trusted the *agent-writable*
`refs/remotes/origin/*` ref as its sync oracle, which a hand-crafted
`update-ref` could poison; `policy_armed = false` in the agent-writable
`project.toml` was a persistent, session-flippable off-switch rather than a
first-commit-only grace; the guard inspected only the first token of each
`&&`/`||`/`;`/`|` segment, so a subshell, `bash -c`, `$(…)`, newline, or
backgrounding hid a `git` op, and a wrong-directory op (`-C`, `GIT_DIR`, `cd`)
was judged against the session branch; and the override-laundering block missed
`/usr/bin/env`, `command env`, `declare -x`, and aliases.

The deeper cause is a category error worth recording: **an agent that shares the
host shares every in-band credential.** Environment variables
(`CODEFLOW_HUMAN_OVERRIDE`, the integrate token), token files, and the TTY are
all reachable by the agent, so none of them authenticates a *human*. Genuine
human-only authorization of an irreversible or security-sensitive action needs
an **out-of-band** factor on a channel the agent cannot reach. Today the de-facto
out-of-band factor already exists: the remote PR-merge is server-enforced and
agent-unreachable, which is exactly why remote branch protection plus CI is the
authoritative boundary (charter D19), and the local env-var override is only a
convenience for a human at their own terminal.

## Decision

Treat the local git-guard as **fast feedback that raises the honest-agent
floor**, never as a boundary against a determined one, and harden it on that
basis while recording the residuals honestly:

- **Hook/policy integrity** (`git.hook_integrity`, default `block`,
  `--minimal`-softenable): the guard blocks `git config core.hooksPath` (set and
  `--unset`, but not a `--get` read), `git -c core.hooksPath=`, the hook-skip env
  prefixes (`GIT_SKIP_HOOKS=`, `HUSKY=0`, `SKIP_HOOKS=`,
  `PRE_COMMIT_ALLOW_NO_CONFIG=`, `GIT_HOOKS_PATH=`), and Bash writes/removes
  (`rm`/`mv`/`cp`-dest/`tee`/`chmod`/`sed -i`/`git rm`/redirects) targeting the
  hook shims (`.git/hooks`, `.codeflow/git-hooks`) or the integrity files
  (`.codeflow/policy.json`, `.codeflow/project.toml`). Reads stay allowed.
- **Reference-transaction oracle**: the sync allowance narrows to an *exact*
  match with `refs/remotes/origin/<branch>` (the state a `git pull`
  fast-forward produces); the prior "or an ancestor of it" allowance is dropped.
  The git-guard additionally forbids writing `refs/remotes/*/<protected>` so the
  oracle cannot be poisoned in the Claude plane. `git pull` on a protected
  branch stays allowed (verified end-to-end).
- **Bootstrap grace**: `policy_armed = false` is honored only in the genuine
  pre-first-commit window (an unborn HEAD). Once any commit exists, a disarmed
  flag is ignored and the policy is armed — so a mid-session flip is inert
  however it is written.
- **Token detection**: the guard expands the shell constructs an agent can hide
  a `git`/`gh` behind — subshells `( )`, brace groups `{ }`, `bash -c '…'`,
  `$(…)`/backticks, newlines, backgrounding `&` — and resolves the effective
  target of a retargeted op (`-C`, `--git-dir`, `GIT_DIR=`, `cd X &&`) against
  the *target* repo's branch via an injected resolver. Unknown ref-writers
  (`update-ref` of a protected `refs/heads`/`refs/remotes`, `symbolic-ref`,
  `fast-import`) and bulk pushes (`--all`/`--mirror`/wildcard refspecs, counted
  as touching every protected branch) are evaluated conservatively.
- **Override laundering**: also catches `/usr/bin/env VAR=`, `command env`,
  `declare -x`/`typeset`/`readonly`, and an override carried inside a
  `git config alias.*` value.
- **Exec-guard exit code**: a dangerous-command block maps to exit 2
  (`PreToolUse` deny), asserted by test.

Add an inert seam for the real fix. A new top-level policy key
`human_authorization` (default `"none"`) is read at the override/integrate
check-points as a no-op: `none` passes the in-band env override through
unchanged. The future out-of-band adapters (`totp`, `push`, `webauthn`) are
deferred — no adapter ships here; the read is wired so one slots in without
re-threading the call sites.

## Consequences

- The honest-agent floor is materially higher: every hole the review reproduced
  is now blocked in the Claude plane (reproduced BYPASSED → BLOCKED), and the
  git-hook plane no longer trusts an ancestor-of-remote move.
- This is **not** airtightness, and the code and this ADR say so. The git-guard
  sees only Bash tool calls, so the Write/Edit tools can still overwrite a hook
  or `project.toml` without passing through it — which is exactly why the
  bootstrap-grace fix is a *runtime* inertness, not merely a write-block.
  Residuals, all backstopped by remote + CI (D19): the reference-transaction
  stage still consults the local `refs/remotes` ref for the exact-match sync
  case, so an off-Claude agent that both writes `refs/remotes` and
  fast-forwards onto it can still pass that one check; effective-cwd resolution
  falls back to the session branch when no resolver is available or the target
  dir is unreadable; and deeply obfuscated shell nesting can still hide a token.
  These are floor-raises against honest mistakes, not defenses against a
  determined adversary.
- The env-var human override remains a convenience, not authentication — it is
  contained because the authoritative boundary is the remote, which the agent
  cannot reach. `human_authorization: "none"` keeps today's behavior; a later
  ADR that ships an adapter turns the seam into a real out-of-band factor.
- New policy surface: `git.hook_integrity` (softened by `--minimal`, suspended
  only in the pre-first-commit window) and top-level `human_authorization`.
  Consumer `policy.json` files predating this ADR gain both via serde defaults.

## Architecture impact

None — the enforcement stays on the existing four planes; this ADR adds policy
keys (config, not architecture) and hardens the guard logic within them.
