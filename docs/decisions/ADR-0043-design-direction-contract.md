---
id: ADR-0043
uid: 50fc5c87-44e7-4546-9c9e-eab9ea8f64e3
title: make product and interface design direction an explicit proportionate contract
date: 2026-07-26
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — standard/full scaffolds gain a progressive-disclosure design skill, a versioned DESIGN_INTENT plan field, anchored fidelity review, and behavioral design evals
---

# ADR-0043: design direction contract

## Context

CodeFlow already requires Claude-led design, dual approval, reusable UI
foundations, accessibility evidence, and rendered verification. Those duties
protect implementation quality, but they do not fully explain how to establish
an appropriate product, interaction, or visual direction before building.
Models can otherwise reproduce familiar interface formulas, infer an audience
without evidence, over-process a bounded change, or judge fidelity by taste
rather than the accepted brief.

The missing layer must remain proportionate and consuming-project-owned. A
cosmetic correction does not need a mood board; an established design system
should not be displaced casually; an explicit operator direction must not be
silently overridden. Concrete model releases and harness mechanics also change
faster than the design duties.

## Decision

Standard/full scaffolds ship a mirrored, on-demand `cf-design` skill. The
orchestrator loads it for new or materially reshaped user-facing surfaces and
for material fidelity review. It selects one of four process weights:

```text
exact/cosmetic change             -> DESIGN_INTENT: N/A
bounded established-system work   -> conform to the named system
new or reshaped surface           -> settle one evidenced direction
novel surface with open direction -> compare 2–3 viable directions, then settle
```

The result is recorded as `DESIGN_INTENT` inside the existing versioned plan.
It captures only applicable creator intent, audience/job/context, experience
target, systems and constraints, operator direction, research or references,
directions considered, settled direction, accessibility target, and fidelity
evidence. Fields collapse when irrelevant. A separate design document or ADR is
created only when the consuming project needs a durable design-system or
product decision.

The role qualified as `claude-judgment-primary` leads design judgment; the
Codex primary challenges feasibility, proportionality, implementation risk,
testability, and fidelity. Both approve the same Plan vN. Stable roles—not
Fable, Opus, Sol, or another release name—own the duties; the qualified ensemble
binding selects concrete models.

The skill starts from the consuming project's brief, product record, existing
product, design system, platform conventions, brand evidence, content, and
accessibility target. External research and annotated references are used only
when they materially reduce uncertainty. Claims retain provenance; users,
personas, research, preferences, metrics, and brand history are never invented.
For web work, WCAG 2.2 AA is the default minimum unless the project records a
stronger target or a different surface-appropriate target with its rationale.

Review compares the rendered result with `DESIGN_INTENT` using evidence suited
to the claim. Findings anchored in the brief, accepted intent, accessibility
target, or observed behavior are graded by materiality. Unanchored taste remains
non-blocking. Behavioral evals cover proportionality, explicit operator
direction, counterfactual evidence-grounded choice review, fidelity, and
accessibility; a rendered case uses same-environment artifacts and blinded
paired comparison.

## Note (2026-09-06)

Claude still produces `DESIGN_INTENT`. Default UI assignment is Claude
producer (implementer check) and Codex reviewer (Computer Use QA on the
official app-server from a Claude host via plugin only). If Codex produces
the UI, Claude independently QAs via Computer Use in Claude Code. Playwright
remains the deterministic web driver; Computer Use is not a default web
driver and not design authorship. Adaptive viewports, accessibility,
i18n/l10n including LTR/RTL, and system layers stay proportionate intent
dimensions — collapse with an evidenced `N/A`.

## Consequences

- Design reasoning becomes explicit and reviewable without expanding the
  always-loaded doctrine into a design manual.
- New surfaces receive discovery and direction work when uncertainty warrants
  it; bounded changes can conform or collapse without ceremony.
- Consuming-project evidence and operator direction outrank unexamined model
  completion or a CodeFlow house style.
- Model-family changes require binding qualification, not edits to design
  semantics.
- Visual quality remains partly judgment-based. Deterministic accessibility,
  interaction, and visual-regression evidence complements but cannot replace
  contextual review.

## Rejected

- Put the whole method in `AGENTS.md` or the orchestrator resource: this would
  charge every task for specialist context and blur one canonical authority.
- Require `design.md`, a mood board, multiple concepts, or external research for
  every UI change: this would reward ceremony rather than uncertainty
  reduction.
- Import Hallmark, Anthropic's frontend skill, or OpenAI's frontend guidance
  wholesale: each offers useful prompts, but CodeFlow needs a
  framework-neutral contract integrated with its plan, evidence, and review
  model.
- Maintain a prohibited-style list or theme catalog: familiar patterns can be
  correct; the defect is a material choice without sufficient evidence, not the
  pattern itself.
- Give Agent OS a second repository-design method: development delegated to a
  CodeFlow repository should use `cf-design`, while Agent OS keeps only its
  runtime-neutral product and artifact-design invariants.

## References

- [Anthropic frontend-design skill](https://github.com/anthropics/skills/blob/main/skills/frontend-design/SKILL.md)
- [OpenAI frontend app builder skill](https://github.com/openai/plugins/blob/main/plugins/build-web-apps/skills/frontend-app-builder/SKILL.md)
- [Hallmark](https://github.com/nutlope/hallmark)
- [Design Council — Framework for Innovation](https://www.designcouncil.org.uk/our-resources/framework-for-innovation/)
- [W3C — Designing for Web Accessibility](https://www.w3.org/WAI/tips/designing/)
- [Figma — How to make a mood board](https://www.figma.com/resource-library/how-to-make-a-mood-board/)
- [Anthropic skills evaluation pull request](https://github.com/anthropics/skills/pull/210)
