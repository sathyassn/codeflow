---
id: ADR-0003
title: CLI vs Claude command boundary
date: 2026-06-12
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0003 — CLI vs Claude command boundary

## Context

The planned CLI surface included `stack add`, and `assets/profiles` was the
planned home for per-stack scaffold overlays. Both put judgment calls —
which stack a project really uses, what its lint and test configuration
should look like — inside a deterministic binary. Each new stack would have
grown CLI code and shipped assets for work that is inherently
read-the-project-and-decide. A boundary rule was needed before more surface
shipped on the wrong side of it.

## Decision

The CLI owns deterministic, judgment-free mechanics that must work with no
AI present: gates, scaffold, generated views, search. Claude commands own
anything that requires reading the project and making choices. Stack setup
is judgment, so it is agent-led: `/cf-stack` detects and confirms the stack,
instantiates `.codeflow/test-config.json` from the shipped test-config
templates, authors lint configuration, and records project standards —
verified afterwards by the deterministic side (`codeflow test`, `doctor`).

## Consequences

- `stack add` is removed from the planned CLI surface; stack setup happens
  via `/cf-stack`.
- The `assets/profiles` concept is dropped. The test-config templates
  (`assets/base/testing/templates/`) remain the deterministic substrate the
  agent instantiates from.
- Stack setup now requires an AI harness; binary-only consumers hand-write
  `.codeflow/test-config.json` from the same templates. Accepted: the
  binary still verifies the result, it just no longer guesses it.
- Future surface questions resolve by the same test: no judgment → CLI;
  judgment → Claude command.

## Architecture impact

None — this removes planned surface and reassigns responsibility; no
shipped module boundary changes.
