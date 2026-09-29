---
id: ADR-0001
title: stack choice for rust
date: 2026-07-17
status: accepted
superseded_by: null
architecture_impact: establishes the initial stack described in architecture.md
---

# ADR-0001: stack choice for rust

## Context

codeflow-lead-model-routing needs an implementation stack chosen before any code exists.
The choice shapes tooling, testing, and hiring of both humans and agents, and
is expensive to reverse once capabilities ship on it.

## Decision

The project is built on **rust**.

<!-- TODO: replace this comment with 2-3 honest sentences on why rust
     fits this project. Written by the adopting team at init time; codeflow
     does not generate rationale. -->

## Consequences

- Tooling, test commands, and lint configuration follow the rust stack
  (`/cf-stack` keeps `test-config.json` and lint config aligned).
- Future stack additions (a second language, a new runtime) are Tier-3
  decisions and require their own ADR.

## Architecture impact

Establishes the initial stack section of `docs/architecture.md`.
