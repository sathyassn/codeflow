---
id: ADR-0030
title: make proportionate design and code quality a duo gate
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: the shared duo contract and evaluator now block speculative or unjustified design and implementation complexity, with Codex first verification and Fable final review
---

# ADR-0030 — proportionate design and code quality

## Context

The scaffold already asks for the simplest idiomatic change, independent design
options, deterministic verification, and Claude final review. Those duties can
still be satisfied superficially by code that is correct but needlessly broad:
speculative abstractions, configuration, dependencies, compatibility paths, or
dead structure may pass tests while increasing maintenance and security cost.
Subjective labels such as “senior” or “perfect” do not make that quality bar
observable and may reward ornament or raw line-count reduction.

## Decision

Make proportionality a shared design, implementation, and review gate. The
accepted solution is the smallest coherent change that fully satisfies current
requirements—not the fewest lines. Every material new concept or surface must
map to a current requirement, observed constraint, or evidenced risk; otherwise
it is removed or simplified.

Both primary seats review design proportionality before approval. Codex
implements and first-verifies necessity, clarity, idiomatic structure,
maintainability, failure behavior, and security. The directly invoked primary
Fable seat reviews the settled design and actual integrated diff and owns the
final quality verdict. Helpers may collect evidence but cannot replace either
primary. Material avoidable complexity yields `changes_requested` even when
tests pass. A Fable fallback is recorded as reduced assurance and is never
reported as a Fable review.

The model evaluator carries a regression fixture whose behavior is correct and
tests pass, but whose unnecessary strategy/configuration framework must be
rejected. Raw LOC remains diagnostic only; the evaluator rewards traced
requirements, simpler equivalents, and maintainability rather than compression.

## Consequences

- Consuming repositories receive an operational right-sizing rule without a
  new skill or another always-loaded checklist.
- Both over-engineering and unsafe over-compression fail the same requirement:
  the smallest coherent solution that preserves accepted behavior and evidence.
- A passing test suite is necessary but insufficient when the integrated design
  carries unjustified maintenance or security burden.

## Sources

- OpenAI Codex best practices: <https://learn.chatgpt.com/guides/best-practices>
- Anthropic Claude Code best practices: <https://code.claude.com/docs/en/best-practices>
- Google engineering review guidance: <https://google.github.io/eng-practices/review/reviewer/looking-for.html>
