---
id: ADR-0087
uid: 3ae4f9a9-02b4-4713-90c4-95acd8d3d9ec
title: "Effort is each person's setting, not the project's"
date: 2026-10-28
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

# ADR-0087: Effort is each person's setting, not the project's

## Context

ADR-0056 pinned effort in the shared settings: "Codex project settings pin
high. Claude settings provide `effortLevel: high` as a starting default".
Shared project settings outrank each person's own settings in both
harnesses, so the pin replaced a contributor's own effort, `xhigh`
included, in every session in the repository (issue 351). The pin never set
the seats' effort: every seat launch in `cf-herdr` already passes its own
(`--effort`, `-c model_reasoning_effort=...`, `--reasoning-effort`). The
operator approved removing the pin on 2026-10-27.

## Decision

This record supersedes only ADR-0056's settings-pin sentence quoted above;
the rest of ADR-0056 stands, including high as the primary seats' launch
effort. The scaffold ships no `effortLevel` in `.claude/settings.json` and
no `model_reasoning_effort` in `.codex/config.toml`. A person sets their own
effort in `.claude/settings.local.json` or `~/.claude/settings.json` for
Claude Code, and in `~/.codex/config.toml` or a launch flag for Codex. A
project that wants a shared effort opts in through `cf-customize`.
`codeflow update` removes a pin whose value is exactly the one CodeFlow
recorded as shipped and keeps a value the project changed.

## Consequences

- A contributor's own effort applies in a CodeFlow project again.
- Seat effort is unchanged: launches through `cf-herdr` pass the ensemble's
  effort explicitly.
- A session someone starts by hand, with no effort of their own, gets the
  harness default, which may be lower than high. `cf-customize` states this
  tradeoff where it offers the project opt-in.
- A project that wants one effort for everyone adds the key back in its
  shared files; update keeps that value.

## Architecture impact

None. `docs/architecture.md` describes the primaries' launch effort, which
this record does not change.
