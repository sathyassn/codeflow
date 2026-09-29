---
id: ADR-0074
uid: e637024b-bdf7-477d-b11b-83b3acbe7f36
title: "The root checkout keeps its root branch, and hooks tell agents by harness markers"
date: 2026-09-28
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "`docs/architecture.md`: the enforcement modules gain `root_checkout.rs`, which holds the root-checkout rule, the actor read from harness markers, and workspace mode (TSK-165)."
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only: new dependency, schema change, boundary change, not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0074: The root checkout keeps its root branch, and hooks tell agents by harness markers

## Context

CodeFlow's doctrine says task work happens in a linked worktree and the root
checkout stays put, but only prose said so. Nothing stopped an agent from
switching the root checkout to a feature branch and committing there, which
leaves the next session that starts at the root on the wrong branch with
someone else's work. The operator decided on 2026-09-28 that the rule is
enforced: agents are blocked, humans at their own terminal are warned.
git-guard runs only inside a harness session, so it knows every command
comes from an agent. The git hooks run for everyone, so they need a way to
tell an agent from a human, and a defined answer for when they cannot.

A second case needs the same rule with a different branch. An umbrella
repository that holds several projects, each its own repository, is a
working checkout: sessions start there and load their instructions and
settings from it, so its root stays on a working branch on purpose instead
of the default branch. The agentic-systems workspace is the first such
umbrella.

## Decision

Each repository has a root branch, the branch its root checkout holds:
`git.root_branch` in the policy, and when that is empty, the default branch
(the target of `origin/HEAD`, then the first protected branch that exists
locally, then `main`). A commit at the root checkout on any other branch,
or on a detached HEAD, is a finding under `git.root_checkout_commits`
(default `block`, suspended in bootstrap grace). git-guard applies the level
to every commit-creating command without reading the environment. The git
hooks read the harness markers in the process environment:

| Harness | Markers | Source |
|---|---|---|
| Claude Code | `CLAUDECODE` | observed in Claude Code 2.1.281's Bash tool |
| Codex CLI | `CODEX_SESSION_ID`, `CODEX_THREAD_ID`, `CODEX_CI` | `openai/codex` `4fd5745`, `core/src/exec_env.rs:40-50` and the unified exec process manager |
| Grok Build | `GROK_SESSION_ID` | `xai-grok-build` `f0e3be1`, `xai-grok-shell/src/session/agent_rebuild.rs:297-300` |

Any non-empty marker makes the actor an agent and the hook applies the
level. With no marker the hook cannot tell, treats the actor as a human, and
only warns. `CODEFLOW_HUMAN_OVERRIDE=1`, the override ADR-0007 already
honours in the git layer and git-guard never trusts, makes the hook treat
the actor as a human too; like the rest of the git layer, only the value
`1` counts. The markers live in one list in
`root_checkout.rs`, so a new harness is one entry.

An umbrella uses workspace mode: its root branch is `integration/workspace`
by convention, held in one constant, and `codeflow init --workspace` creates
or reuses that branch, sets the key and ignores every nested repository in
the umbrella's `.gitignore`, leaving registered submodules alone. `codeflow
doctor` reports the root branch and its source, a root checkout off it or
holding task edits, nested repositories no tracked `.gitignore` covers, and
linked worktrees outside `git.worktree_locations`, whose default covers
`.worktrees/` and the folders Claude, Codex and Grok manage.
`integration/workspace` is not an epic line; TSK-166 teaches `codeflow ci`
so.

## Consequences

- A single repository that already follows the doctrine sees no new
  refusal or warning; doctor gains one line naming its root branch.
- An adopter whose agents commit at the root checkout on a feature branch
  is now refused. This is a behaviour change named in the CHANGELOG; the
  level can be lowered to `warn` or `off` per repository.
- The hook's view of the actor is only as good as the markers. An agent
  that strips its environment reaches the hooks as a human and is only
  warned, but git-guard still refuses it. A human in a harness's own shell
  mode carries a marker and is judged as an agent unless they set the
  override.
- A harness that adds or renames its marker needs a new entry and a fresh
  source citation; until then its agents are warned, not blocked, at the
  hook plane.
- Switching the root checkout (`checkout`, `switch`) stays unguarded. The
  rule judges commits, and doctor reports a root checkout left off its
  branch.
- Until TSK-166 lands, an umbrella with durable work tracking has its
  `integration/workspace` ranges refused by `codeflow ci` as an unverified
  epic line. A minimal-tier umbrella is unaffected.

## Architecture impact

`docs/architecture.md`: the enforcement modules gain `root_checkout.rs`,
which holds the root-checkout rule, the actor read from harness markers, and
workspace mode.
