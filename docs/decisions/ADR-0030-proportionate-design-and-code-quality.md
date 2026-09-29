---
id: ADR-0030
uid: 287b9c1a-6d4e-4de2-8801-2b33fc02af7d
title: make proportionate design and code quality a duo gate
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: the shared duo contract and evaluator now block speculative or unjustified design and implementation complexity, with Codex first verification and Fable final review
---

# ADR-0030: proportionate design and code quality

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

The inverse failure is also blocking. “Smallest coherent” preserves justified
modularity, single sources of business truth, explicit state and side effects,
idiomatic declarative or reactive composition where the stack uses it, and
accepted edge/error handling. Stable invariants may be named in code; supported
variability, secrets, or duplicated domain decisions are not hidden as literals.
The evaluator therefore pairs the over-engineering fixture with passing but
duplicated under-design, and separately checks that UI work reuses an existing
design system's tokens, accessible primitives, and components without requiring
a new design system or higher-order framework abstraction for a one-off view.

Proportionality also depends on accepted operating context: expected lifetime,
scale, rate and shape of change, contributor and integration breadth,
operational or security risk, and cost of reversal. Project size alone does not
justify architecture. When missing context would materially change the design,
the duo clarifies it before approval; if clarification is unavailable, it states
the assumption and chooses established safe practices with reversible
boundaries rather than either a brittle shortcut or speculative framework.

## Consequences

- Consuming repositories receive an operational right-sizing rule without a
  new skill or another always-loaded checklist.
- Both over-engineering and unsafe over-compression fail the same requirement:
  the smallest coherent solution that preserves accepted behavior and evidence.
- Reuse and abstraction remain evidence-routed: repeated current behavior,
  observed constraints, or accepted failure/change cases justify structure;
  hypothetical flexibility does not.
- A disposable experiment and a durable multi-team service need not receive the
  same structure; lifecycle and risk context are explicit evaluation inputs.
- A passing test suite is necessary but insufficient when the integrated design
  carries unjustified maintenance or security burden.

## Sources

- OpenAI Codex best practices: <https://learn.chatgpt.com/guides/best-practices>
- Anthropic Claude Code best practices: <https://code.claude.com/docs/en/best-practices>
- Google engineering review guidance: <https://google.github.io/eng-practices/review/reviewer/looking-for.html>
- React component hierarchy and minimal-state guidance:
  <https://react.dev/learn/thinking-in-react>
- W3C Design Tokens Community Group:
  <https://www.w3.org/groups/cg/design-tokens>
