---
id: ADR-0051
title: extend product design with bounded variation, trustworthy sourcing, and revision provenance
date: 2026-08-01
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — cf-design gains a delta-only progressive-disclosure sourcing contract and the existing versioned plan retains reviewed-design provenance
---

# ADR-0051: bounded design sourcing and revision

## Context

ADR-0043 established a proportionate product-design method, consuming-project
authority, multiple initial directions only for materially open work, and
evidence-anchored fidelity review. Subsequent work found three bounded gaps: it
does not govern useful variation after an operator selects a direction; it does
not cover licensing, consent, privacy, and provenance when inspiration or
assets are sourced or generated; and it does not explicitly retain which
revision received material operator feedback.

Most requested design-system and handoff behavior already exists in `cf-design`
and the orchestrator quality contract. Restating it would increase token cost
and create competing authorities rather than improve design quality.

## Decision

Extend ADR-0043 without editing or superseding it. `cf-design` gains three
compact behaviors and one on-demand sourcing reference:

1. after direction selection, explore bounded, meaningful in-direction variants
   only while a named material choice remains unresolved;
2. distinguish inspiration from user evidence and retain proportionate source,
   rights/consent, generation/transformation, restriction, and product-use
   provenance for material assets;
3. retain the exact reviewed version and accepted/rejected rationale when
   material feedback creates Plan vN+1.

Private or sensitive material is not sent to external asset or generation
services without explicit authority. Concrete providers, models, and tools are
qualified/configurable project choices rather than durable recommendations.
Existing canonical guidance remains the authority for design-system structure,
accessibility, implementation quality, and fidelity review; the delta links to
it instead of copying it.

## Consequences

- Open exploration can narrow productively after selection without turning
  every review into a palette/font/theme control panel.
- Asset and reference use becomes reviewable and safer without a provider
  catalog or mandatory asset ledger for low-risk work.
- Operator feedback remains attributable to the design actually reviewed.
- The skill stays proportionate: bounded work collapses, and durable doctrine
  does not depend on model or service names.
- Utility implementations are checked against the final sourcing contract at
  aggregate review, but this decision cannot silently change their schemas or
  runtime surfaces.

## Rejected

- Reopen ADR-0043 or SPC-003: accepted decisions and implemented specs are
  frozen inputs; this is a new, bounded delta.
- Add a second design skill or a design database: both would split authority and
  impose maintenance unrelated to the missing behavior.
- Require variants, mood boards, asset ledgers, or external research for every
  interface change: process weight follows material uncertainty and risk.
- Publish a preferred provider list in durable doctrine: availability, policy,
  and capability change independently of the design invariants.
- Restate existing token/component/accessibility doctrine in this decision:
  duplicated rules drift and do not improve enforcement.
