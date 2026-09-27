---
id: ADR-0001
uid: adbd005d-27cd-4494-8916-5b45bd66efe5
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

Rust ships a single static binary, which is exactly what the hook shims and
installer story require — nothing to install on the consumer machine beyond
the binary itself. The engine modules predate v2 as tested Rust code, so the
stack was already proven in place rather than chosen on speculation. And the
test/clippy gate culture (workspace deny lints, tests shipping with code)
matches the discipline this project exists to enforce.

## Consequences

- Tooling, test commands, and lint configuration follow the rust stack
  profile (`codeflow stack add` keeps `test-config.json` and CI aligned).
- Future stack additions (a second language, a new runtime) are Tier-3
  decisions and require their own ADR.

## Architecture impact

Establishes the initial stack section of `docs/architecture.md`.
