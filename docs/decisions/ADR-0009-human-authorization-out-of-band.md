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
  `--minimal`-softenable): the guard blocks hook-path manipulation via
  `git config core.hooksPath` (set and `--unset`, not a `--get` read),
  `git -c core.hooksPath=`, and git's `GIT_CONFIG_*` env mechanism
  (`GIT_CONFIG_KEY_n=core.hooksPath`, `GIT_CONFIG_PARAMETERS`); the hook-skip env
  prefixes (`GIT_SKIP_HOOKS=`, `HUSKY=0`, `SKIP_HOOKS=`,
  `PRE_COMMIT_ALLOW_NO_CONFIG=`, `GIT_HOOKS_PATH=`); and Bash writes/removes
  (`rm`/`mv`/`cp`-dest/`tee`/`chmod`/`sed -i`/`git rm`/redirects) targeting the
  hook shims (`.git/hooks`, `.codeflow/git-hooks`) or the integrity files
  (`.codeflow/policy.json`, `.codeflow/project.toml`). The matchers are
  *generalized*, not spelling lists: config keys compare case-insensitively
  (git keys are case-insensitive); paths are normalized (`//`, `/./`, trailing
  and leading `./`) before comparison; the redirect matcher recognizes the
  operator *shape* (`\d*>`, `>>`, `>|`) rather than a fixed set. Reads stay
  allowed.
- **Reference-transaction oracle**: the sync allowance narrows to an *exact*
  match with the branch's remote-tracking head (the state a `git pull`
  fast-forward produces) — the branch's *configured* upstream when set (so a
  non-origin `git pull upstream main` is not a false positive), else
  `refs/remotes/origin/<branch>`; the prior "or an ancestor of it" allowance is
  dropped. The git-guard additionally forbids writing `refs/remotes/*/<protected>`
  so the oracle cannot be poisoned in the Claude plane. `git pull` on a
  protected branch stays allowed (verified end-to-end).
- **Bootstrap grace**: `policy_armed = false` is honored only in the genuine
  pre-first-commit window (an unborn HEAD). Once any commit exists, a disarmed
  flag is ignored and the policy is armed — so a mid-session flip is inert
  however it is written.
- **Token detection**: the guard expands the shell constructs an agent can hide
  a `git`/`gh` behind — subshells `( )`, brace groups `{ }`, `bash -c '…'`
  (including a clustered `-lc` and an `env FOO=1 bash -c …` prefix),
  `eval '…'`, `$(…)`/backticks, newlines, backgrounding `&` — resolving the
  effective program through a shared launcher-normalization (leading `VAR=val`,
  `command`/`builtin`/`exec`, and the `env` family in any path form), so the
  program is judged by what runs, not the surface word. It resolves the
  effective target of a retargeted op (`-C`, `--git-dir`, `GIT_DIR=`, `cd X &&`)
  against the *target* repo's branch via an injected resolver. Unknown
  ref-writers (`update-ref` of a protected `refs/heads`/`refs/remotes`,
  `symbolic-ref`, `fast-import`) and bulk pushes (`--all`/`--mirror`/wildcard
  refspecs, counted as touching every protected branch) are evaluated
  conservatively.
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
  Residuals, all backstopped by remote + CI (D19):
    - **Interpreter / pipe-to-shell indirection is unbounded and deliberately
      not enumerated.** An interpreter (`python3 -c '…'`, `perl -e`, `ruby -e`,
      `node -e`) or a pipe-to-shell (`echo 'git …' | sh`, `| bash`, `xargs
      git`) can run any git command or write any file with no spelling the
      guard can match — chasing each interpreter and pipe form is whack-a-mole.
      The guard blocks the direct and common-wrapper spellings
      (shell `-c`, `eval`, subshells, substitution, launcher prefixes) to raise
      the honest-agent floor; a determined agent with an interpreter or a
      pipe-to-shell bypasses local guards, which is precisely why the boundary
      is remote branch protection + CI, not the client.
    - **Transparent process launchers are the same unbounded family.** The
      guard resolves the grammar-free launcher prefixes (`command`, `builtin`,
      `exec`, the `env` family) to the real program, but the wider set that runs
      a following command in a modified environment — `nohup`, `timeout`,
      `nice`, `stdbuf`, `setsid`, `ionice`, `taskset`, … — is open-ended and
      each carries its own option/positional grammar. Enumerating a handful
      gives false assurance (the next one still passes) and fragile per-launcher
      parsing risks false positives on legitimate use (`timeout 5 cargo test`),
      so they are deliberately not enumerated; a `nohup bash -c 'git …'` on a
      protected branch is a floor-residual backstopped by remote + CI, not the
      client.
    - The reference-transaction stage still consults the local `refs/remotes`
      ref for the exact-match sync case, so an off-Claude agent that both writes
      `refs/remotes` and fast-forwards onto it can still pass that one check.
    - Effective-cwd resolution falls back to the session branch when no resolver
      is available or the target dir is unreadable; path normalization does not
      resolve `..` traversal.
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

## Note — 2026-07-03 (appended)

Repo-specific caveat: on this repo (private + GitHub Free) remote branch
protection is unavailable (403; see ADR-0006), so the server-side backstop that
contains the residuals here is CI plus human-merged PRs, not armed remote branch
protection. The "remote is the authoritative boundary" argument above is the
general product design (charter D19); this repo cannot fully arm it while
private, so the residuals are accepted only alongside the human-merge discipline.

## Amendment: 2026-09-27 (appended, TSK-137)

Removed: the top-level `human_authorization` policy key and its read at the
override and integrate check-points. Why: it accepted only `none`, no adapter
ever shipped, and an inert key in every scaffolded policy suggested a control
that did not exist. The env-var human override and the rest of this decision
are unchanged. A policy file that still has the key loads with one
deprecation warning, and `codeflow update` removes it. A future out-of-band
factor needs its own ADR and key.
