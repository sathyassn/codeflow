---
name: cf-design
description: Establish and settle proportionate product, UX, interaction, and visual-design intent before implementing a new or materially reshaped user-facing surface. Use for web, native, mobile, desktop, or other interfaces when the task changes experience direction, information hierarchy, interaction, brand expression, typography, colour, layout, imagery, or motion; also use to review implementation fidelity against an accepted direction. Conform to an existing system without ceremony for bounded changes, and skip for cosmetic edits whose design intent is already explicit.
---

# cf-design — design direction before implementation

Produce the design intent that the CodeFlow quality contract later verifies.
Ground it in the creator, audience, use context, subject, and systems already in
force. Treat design as a reasoned product decision, not decoration or a catalog
of fashionable patterns.

This is a supporting flow inside `cf-model-orchestrator`. The host still owns
Plan vN and multi-model settlement. The role qualified as
`claude-judgment-primary` leads design judgment; the Codex primary challenges
feasibility, proportionality, failure modes, implementation fidelity, and
testability. Concrete model names and effort live in the ensemble binding, not
here.

## 1. Select the process weight

Choose the lightest path that still resolves material design uncertainty:

```text
cosmetic or exact local correction
  -> DESIGN_INTENT: N/A — accepted direction is unchanged

bounded change inside an established system
  -> DESIGN_INTENT: conform — <system or approved surface>

new or materially reshaped user-facing surface
  -> settle one evidence-grounded direction before implementation

novel product surface with materially open direction
  -> compare two or three distinct, viable directions before settlement
```

Do not promote a tweak into a redesign. Do not use an existing system as an
excuse to avoid resolving a genuinely new interaction or experience.

## 2. Inspect before inventing

Read the brief and the consuming project's `docs/product.md`, relevant
capabilities, architecture, accepted decisions, research, content, and
project-owned brand or design guidance. Inspect the actual product and existing
design system before proposing a parallel visual language:

- tokens, typography, colour roles, spacing, imagery, and motion;
- accessible primitives, application components, and view composition;
- information architecture, content patterns, journeys, and interaction state;
- platform conventions and the constraints of the target medium;
- previous operator-approved examples and explicit rejected directions.

Use authoritative product evidence first. External references inform a
direction; they do not override the brief or license copying.

## 3. Establish design intent

Resolve only the dimensions that materially steer the surface:

1. **Creator intent.** What the surface is meant to achieve and what must be
   preserved, in the operator's or product's terms.
2. **Audience, job, and context.** Who uses it, what they are trying to
   accomplish, and the conditions in which they do so.
3. **Experience target.** The interaction qualities and emotional register
   that materially affect decisions: for example calm, dense, playful,
   restrained, authoritative, reassuring, or urgent.
4. **Systems and constraints.** Existing design system, brand, platform
   conventions, content, accessibility target, technical boundaries, and
   accepted non-goals.
5. **Operator direction.** Treat an explicit direction as a binding constraint
   owed honest counsel. Surface evidence-backed accessibility, usability, or
   feasibility concerns; never silently override it and never amplify it
   without examination.

Label the provenance of material claims. Audience facts come from the operator,
repository, or real research. If a reversible inference is necessary, label it
`inferred` and expose it for settlement. Never fabricate research, users,
personas, preferences, quotes, metrics, testimonials, or brand history.

Clarify when missing intent would materially change the outcome. Otherwise use
the safest established convention and disclose the assumption.

## 4. Research and explore proportionately

For a novel or materially open surface, research the subject's own world and
the audience's real context before selecting a visual direction. References
may come from products, physical materials, editorial systems, environments,
tools, or cultural forms relevant to the brief. Record what is being borrowed:
an information rhythm, type character, palette anchor, density, imagery
approach, or motion stance—not pixels or protected expression.

Use a small annotated reference or mood board when visual alignment is
materially uncertain and cheaper than building competing high-fidelity
surfaces. It is optional evidence, not a required deliverable.

At the top process rung, present two or three directions that differ in
meaningful structure or experience, not colour swaps. For each, state:

- the product and audience rationale;
- the information and interaction concept;
- typography, colour, layout, imagery, density, and motion stance where
  relevant;
- fit with existing systems and implementation constraints;
- the strongest tradeoff or failure condition.

When the brief fixes the direction or an established system governs it, record
that constraint and why alternatives were waived.

## 5. Settle `DESIGN_INTENT`

Record the result in the existing versioned plan contract, not in a mandatory
new document:

```text
DESIGN_INTENT:
  APPLICABILITY: <N/A with reason | conform to named system | direction pass>
  CREATOR_INTENT:
  AUDIENCE_JOB_CONTEXT: <evidence source or labeled inference>
  EXPERIENCE_TARGET:
  SYSTEMS_AND_CONSTRAINTS:
  OPERATOR_DIRECTION:
  RESEARCH_OR_REFERENCES: <evidence or proportionate N/A>
  DIRECTIONS_CONSIDERED: <chosen/rejected rationale or valid waiver>
  SETTLED_DIRECTION:
  ACCESSIBILITY_TARGET:
  FIDELITY_EVIDENCE_PLAN:
```

Collapse irrelevant dimensions rather than filling them mechanically. A
one-line `conform` or `N/A` record is valid. A durable design specification or
ADR is created only when the project needs a long-lived design-system or
product decision; normal project documentation conventions own it.

The directly invoked Claude judgment primary proposes the design intent and
direction. Codex challenges the choice. Both approve the exact Plan vN before
implementation.

## 6. Critique before build

For a direction pass, ask:

- Does the hierarchy make the user's next action and the product's priorities
  clear?
- Do interaction, content, and state design serve the user's actual job?
- Do typography, colour, layout, imagery, density, and motion express the
  intended experience rather than decorate it?
- Which choices came from the subject, brief, system, or research?
- Which choices lack evidence from the subject, brief, system, or research?
  Which familiar choices are still right here, and why?
- Does the direction remain coherent across loading, empty, error, disabled,
  success, destructive, and recovery states?
- Is the design feasible, maintainable, responsive, accessible, and
  proportionate to the accepted lifetime and scale?

Read [references/design-choice-audit.md](references/design-choice-audit.md)
only after forming a candidate direction, and only for a new/reshaped surface
or when unexamined-choice risk is material. It audits the reasoning behind
choices; it never supplies formats, bans, or a replacement house style.

## 7. Hand off and verify

Implementation follows the established repository architecture and the
orchestrator's quality contract. Reuse the existing design system where it
fits; build justified reusable foundations and application components where
recurrence is evidenced; do not create a new framework layer for one surface.

For web surfaces, WCAG 2.2 AA is the default minimum target unless the project
sets a stronger standard or records a different surface-appropriate standard
with its rationale. Accessibility is a design input and a verification
obligation. Automated checks are partial evidence, never proof of complete
conformance.

Compare the rendered implementation with the settled `DESIGN_INTENT` using
evidence appropriate to the claim: structured interaction assertions,
accessibility state, relevant viewports, same-environment screenshots or visual
comparisons, console/network evidence, and failure traces where material.
Review hierarchy, interaction, content, type roles, colour roles, spacing,
imagery, density, motion, states, and platform fit only where the intent makes
them applicable.

A difference from the settled intent is not automatically a defect: determine
whether it is an approved improvement, an evidence-backed implementation
constraint requiring Plan vN+1, or unjustified drift. A finding anchored in the
brief, `DESIGN_INTENT`, accessibility target, or observed user behavior is
graded by materiality. An unanchored taste preference remains non-blocking.

## Completion

Return:

- the selected process weight and evidence for it;
- the settled `DESIGN_INTENT` or explicit collapse;
- alternatives and references only where warranted;
- unresolved operator-owned decisions;
- the exact fidelity and accessibility evidence required;
- both primary-seat approvals of the same plan version.

Do not claim a user was researched, a direction was approved, a standard was
met, or a rendered surface was verified without recheckable evidence.
