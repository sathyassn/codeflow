---
id: ADR-0001
title: stack choice — rust
date: 2026-06-12
status: accepted
superseded_by: null
architecture_impact: establishes the initial stack described in architecture.md
---

# ADR-0001 — stack choice: rust

## Context

cf-dogfood needs an implementation stack chosen before any code exists.
The choice shapes tooling, testing, and hiring of both humans and agents, and
is expensive to reverse once capabilities ship on it.

## Decision

The project is built on **rust**.

{{STACK_RATIONALE}}

## Consequences

- Tooling, test commands, and lint configuration follow the rust stack
  profile (`codeflow stack add` keeps `test-config.json` and CI aligned).
- Future stack additions (a second language, a new runtime) are Tier-3
  decisions and require their own ADR.

## Architecture impact

Establishes the initial stack section of `docs/architecture.md`.
