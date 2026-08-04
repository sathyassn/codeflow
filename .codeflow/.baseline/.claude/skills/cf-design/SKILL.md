---
name: cf-design
description: Establish and settle proportionate product, UX, interaction, composition, and visual-design intent before implementing or materially reshaping a user-facing surface. Use for web, mobile, tablet, desktop, native, or other interfaces when work changes experience direction, hierarchy, composition, interaction, brand expression, typography, colour, layout, imagery, or motion, and for fidelity review against an accepted direction. Conform without ceremony for bounded changes and skip cosmetic edits with explicit design intent.
---

# cf-design — design direction before implementation

Produce the design intent that the quality contract later verifies, grounded in
the creator, audience, context, subject, and systems in force. Design is a
reasoned product decision, not decoration or a catalog of fashionable patterns.

This supports `cf-model-orchestrator`, which owns Plan vN and settlement. The
role qualified as `claude-judgment-primary` leads design
judgment; the Codex primary challenges feasibility, proportionality, failure
modes, fidelity, and testability. Model names and effort live in the ensemble
binding, not here.

## 1. Select the process weight

Choose the lightest path that resolves material uncertainty:

```text
cosmetic or exact local correction
  -> DESIGN_INTENT: N/A — accepted direction is unchanged

bounded change inside an established system
  -> DESIGN_INTENT: conform — <system or approved surface>

new or reshaped surface whose direction and primary composition follow
from accepted evidence
  -> settle one direction, compose its surfaces, then build

materially open direction, primary composition, or experience
  -> render and compare materially different candidates, then settle
```

Direction and composition are separate uncertainties. An accepted direction
does not settle how a surface must be composed, and an established system does
not settle a genuinely new explanation, collection, comparison, journey, or
interaction. Resolve each at the rung its own evidence requires. Do not promote
a tweak into a redesign, and do not use an existing system as an excuse to
avoid resolving something genuinely new.

## 2. Inspect before inventing

Read the brief, `docs/product.md`, relevant
capabilities, architecture, accepted decisions, research, content, and
project-owned brand or design guidance. Inspect the actual product and existing
design system before proposing a parallel visual language:

- the subject: real objects, data, artefacts, states, and vocabulary in play;
- tokens, typography, colour roles, spacing, imagery, and motion;
- accessible primitives, application components, and view composition;
- information architecture, content patterns, journeys, and interaction state;
- product language, terminology, approved voice examples, and copy states;
- applicable appearance modes, user or system preferences, and persistence;
- platform conventions and the constraints of the target medium;
- previous operator-approved examples and explicit rejected directions.

Use authoritative evidence first. References never override the brief or
license copying.

## 3. Establish design intent

Resolve only the dimensions that materially steer the surface:

1. **Creator intent.** What the surface must achieve and preserve, in the
   operator's or product's terms.
2. **Audience, job, and context.** Who uses it, their task, and its conditions.
3. **Experience target.** The interaction qualities and emotional register
   that materially steer decisions.
4. **Language and voice.** The product's documented voice, terminology,
   audience literacy, trust and risk context, and the copy its navigation,
   actions, guidance, validation, empty, loading, error, success, destructive,
   and recovery states need. Refer substantial language judgment to
   `cf-editorial-review`; do not invent a product personality.
5. **Appearance modes.** Decide only the applicable light, dark, high-contrast,
   system-following, override, persistence, reduced-motion, and mode-safe media
   behavior.
6. **Systems and constraints.** Existing design system, brand, platform
   conventions, content, accessibility target, technical boundaries, and
   accepted non-goals.
7. **Operator direction.** Treat an explicit direction as a binding constraint
   owed honest counsel. Surface evidence-backed accessibility, usability, or
   feasibility concerns; never silently override it and never amplify it
   without examination.

Label the provenance of material claims. Audience facts come from the operator,
repository, or real research. If a reversible inference is necessary, label it
`inferred` and expose it for settlement. Never fabricate research, users,
personas, preferences, quotes, metrics, testimonials, or brand history.

Clarify when missing intent would materially change the outcome. Otherwise use
the safest established convention and disclose the assumption.

The consuming product owns its themes, palettes, and tokens; CodeFlow supplies
no preset and no house style. This skill never absorbs a CodeFlow utility's
runtime mechanics. A utility may use this reasoning for its own design, but its
themes, components, and runtime choices never become a product's direction, and
product evidence reaches a utility only by explicit project choice.

For materially open composition or reusable-system work, read
[references/composition-and-design-system.md](references/composition-and-design-system.md)
before sections 4 to 6. It carries the working detail and stopping rules.

## 4. Model the subject before choosing form

Direction states what a surface is for, not what it must show. Before choosing
a layout, template, component, chart, or diagram, model the **subject** — real
objects, data, artefacts, vocabulary; the **governing idea** a viewer must take
away; the **user action** it serves; the actual **relationships and states**;
the **hierarchy and depth** separating the at-rest idea, mechanics, and
evidence; and the target **platform and medium**.

Then choose form from the relationship it must expose. Remove the sentences
from a candidate: if the remaining structure no longer expresses the
relationship, the structure was furniture. Earn every container — a card,
table, panel, tab, badge, chip, or step marker is right when it represents a
real object, boundary, grouping, state, or action, and wrong when it
manufactures hierarchy the content lacks. Keep the at-rest view carrying the
governing idea, mechanics one layer in, and evidence deeper, without dropping
material content to buy calm. Form families are examples, never defaults; when
none fits the subject, design one and justify it inside the product's own
system.

Decide which viewports, input modes, and platforms are applicable, and choose
form for all of them: the **governing idea** must survive each, not merely the
information. A context that cannot carry the idea gets its own declared
composition, never a compressed copy of another.

## 5. Explore encodings when the composition is open

When the direction, primary composition, or experience is materially open,
compare two or three candidates that differ in what they encode, and render the
ones a reviewer or the operator must judge instead of describing them — a
described composition is no evidence that it reads. Use the lightest medium
that shows the behavior at issue, with the product's real content, at
representative viewports and applicable modes, including the intermediate ones
where composition actually changes. The same content in different furniture, a palette or
typeface swap, a familiar template refilled, a generically generated diagram, or
a variant differing only in ornament are not materially different candidates.

Qualify the comparison before authoring it. The question must fit the surface's
real job, be answerable from each candidate's encoding, and discriminate. Each
candidate declares its primary unit, axis, and encoded relationship up front,
and names what would carry it in the product's real technical contract and at
what loss. A compound question is decomposed, or the part the direction
optimises and what carries the other are both named.

Collapse this rung when the brief, an accepted system, or a settled direction
already governs the composition; record the constraint and why alternatives
were waived. A bounded conformance change needs no exploration at all. After
selection, reopen a bounded comparison only for a named choice that can still
change the accepted outcome, and stop when it is settled.

When external inspiration, sourced or generated assets, material
transformations, or a design revision affect the outcome, read
[references/design-sourcing-and-revision.md](references/design-sourcing-and-revision.md)
first; it governs evidence, authority, provenance, privacy, and bounded
revision.

## 6. Develop the selected direction into an earned system

A settled direction is not yet a design system. Build only layers recurrence
and accepted lifetime earn: semantic tokens, modes, accessible primitives,
recurring or subject-specific components, copy states, platform behavior,
localization, and fidelity controls. Follow the reference's evidence and
stopping rules; neither abstract one consumer nor hard-code a recurring pattern.

Mine rejected candidates for transferable primitives. But convergence between
candidates is a **hypothesis, not recurrence** — test it against subject
independence, whether those surfaces will really coexist and last, and a reuse
need in accepted product surfaces. Convergence alone earns nothing: shipped
recurrence still decides, and the evidence still stops where it stops. If a
layer is earned, take the primitive and its state vocabulary, never the
subject-specific forms built on it.

## 7. Settle `DESIGN_INTENT`

Record the result in the existing versioned plan contract, not in a mandatory
new document:

```text
DESIGN_INTENT:
  APPLICABILITY: <N/A with reason | conform to named system | direction pass>
  CREATOR_INTENT:
  AUDIENCE_JOB_CONTEXT: <evidence source or labeled inference>
  EXPERIENCE_TARGET:
  LANGUAGE_AND_VOICE: <authority, applicable copy states | collapsed>
  APPEARANCE_MODES: <modes, preference/persistence | collapsed>
  VIEWPORTS_AND_PLATFORMS: <applicable sizes, input modes, platforms, where
    composition changes | collapsed>
  SYSTEMS_AND_CONSTRAINTS:
  OPERATOR_DIRECTION:
  RESEARCH_OR_REFERENCES: <evidence or proportionate N/A>
  COMPOSITION: <subject, governing idea, user action, primary form, unit and
    axis | collapsed>
  DIRECTIONS_CONSIDERED: <rendered candidates, declared encodings, carrier
    feasibility, rationale | valid waiver>
  SETTLED_DIRECTION:
  SYSTEM_SCOPE: <earned layers with recurrence evidence | conform | none>
  ACCESSIBILITY_TARGET:
  REVIEW_RUBRIC: <qualified question, pre-registered gates, expected
    results | collapsed>
  FIDELITY_EVIDENCE_PLAN:
```

Collapse irrelevant dimensions rather than filling them mechanically. A
one-line `conform` or `N/A` record is valid. A durable design specification or
ADR is created only when the project needs a long-lived design-system or
product decision; normal project documentation conventions own it.

While the material direction, primary composition, or experience remains
operator-owned, an operator-visible rendered board settles it before
implementation. The board shows the real candidates or the selected composition
with real content and states the decision being asked for. Agreement between
model seats is not that decision and never substitutes for it. No board is
required once the operator has accepted the direction, when an accepted system
governs the work, or for a conformance or cosmetic change.

Material feedback on a settled direction produces Plan vN+1 carrying the exact
reviewed version, the feedback authority, and the accepted and rejected
rationale; the sourcing-and-revision reference owns that record. Bounded
conformance feedback stays in the normal task record.

The Claude judgment primary proposes design intent and direction. Codex
challenges the choice. Both approve the exact Plan vN before
implementation.

## 8. Critique before build

Before viewing final renders, register the applicable gates, their questions,
and acceptable results. The composition observer is never its author; a
threshold invented during review is invalid.

Apply the reference protocols that fit: five-second governing idea,
thirty-second mechanics, form match, primary-form inventory, progressive depth,
and no-box where a figure is the primary explanatory form. For a surface family
or richer medium, add sibling distinctiveness and plain-baseline differential.
Where the surface adapts, add adaptation, which tests idea survival rather than
information presence. Observed results decide these gates, never numeric scores;
semantic containers remain valid.

Machine channels carrying a derived answer, a digest, or provenance stay out of
the visible composition. A candidate that renders its own registered answer
tests the sentence, not the encoding: any comprehension or baseline-differential
result read from it is **void, not merely weak**.

Judge everything else contextually: brief and audience fit, interaction and
states, coherence, restraint, system fit, accessibility, craft, and platform
conventions. Grade findings against the brief, `DESIGN_INTENT`, target, or
observed behavior; unanchored taste is non-blocking. The gates create no house
style or prohibited-pattern list. The operator retains final authority over an
operator-owned direction.

Read [references/design-choice-audit.md](references/design-choice-audit.md)
after forming a candidate direction, for a new or reshaped surface or when
unexamined-choice risk is material. It carries the gate protocols and audits
the reasoning behind choices; it supplies no formats, bans, or house style.

## 9. Hand off and verify

Implementation follows the established repository architecture and the
orchestrator's quality contract, reusing the existing design system and adding
reusable foundations or components only where recurrence is evidenced.

For web surfaces, WCAG 2.2 AA is the default minimum unless the project records
a stronger or different surface-appropriate target with its rationale.
Accessibility is a design input and a verification obligation; automated checks
are partial evidence, never proof of conformance.

Outside the browser the obligation is identical and the vocabulary is not: a
native mobile, tablet, or desktop surface is verified against the conventions
its own platform defines, with evidence collected there. A web render is not
evidence for a native one, and neither is a description of the convention. Name
the applicable conventions and the evidence method in the intent; an evidenced
`N/A` is valid where a platform is out of scope, and an unavailable platform is
recorded as unverified rather than inferred.

Compare the rendered implementation with the settled `DESIGN_INTENT` using
evidence appropriate to the claim: structured interaction assertions,
accessibility state, relevant viewports, same-environment screenshots or visual
comparisons, console/network evidence, and failure traces where material. Where
responsive or cross-platform composition is material, the evidence covers the
intermediate contexts where composition changes and shows the idea intact.
Fidelity review inspects the surface as it actually renders and behaves; an
approval statement, a green build, a passing schema check, or a description of
the intended result is not fidelity evidence. Review only the dimensions the
intent makes applicable, to the depth the quality contract requires, and record
an evidenced `N/A` instead of simulating irrelevant coverage. Where language or
appearance modes apply, verify real localized variants rather than English-only
inference, mode preference and persistence, and no incorrect-mode flash.

A difference from the settled intent is not automatically a defect: determine
whether it is an approved improvement, an evidence-backed implementation
constraint requiring Plan vN+1, or unjustified drift. A finding anchored in the
brief, `DESIGN_INTENT`, accessibility target, or observed user behavior is
graded by materiality. An unanchored taste preference remains non-blocking.

## Completion

Return the process weight and evidence; settled `DESIGN_INTENT` or collapse;
governing idea and composition; rendered candidates and operator decision where
owned; warranted alternatives, references, and system scope; unresolved
decisions; review and fidelity evidence; and both primary-seat approvals of the
same plan version.

Do not claim a user was researched, a direction was approved, a composition was
reviewed, a standard was met, or a rendered surface was verified without
recheckable evidence.
