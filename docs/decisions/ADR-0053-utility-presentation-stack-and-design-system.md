---
id: ADR-0053
uid: d0cb2a61-94a7-4414-b539-45abdac66975
title: utility presentation design system with Preact present chrome and Starlight portal
date: 2026-08-07
status: accepted
superseded_by: null
architecture_impact: none — confirms ADR-0048/0049 runtimes; adds shared utility presentation design-system boundary
---

# ADR-0053: utility presentation design system with Preact present chrome and Starlight portal

## Context

EPC-005 implementation landed functional present and portal runtimes under
ADR-0049 (Preact chrome beside a Rust-owned document) and ADR-0048 (opt-in
Astro Starlight + Pagefind portal). Operator review rejected lived craft:
subject-led presentation failed, and design exploration was opened to settle a
utility presentation system for agent responses and durable docs.

Exploration and operator discussion raised a possible move to a single
React + shadcn stack for both utilities. That choice was not previously
recorded. Stack must be settled before visual recovery and skill updates so
implementation is not deferred into a second migration.

## Decision

**Design system.** CodeFlow owns one **utility presentation design system**:
semantic tokens, type scale and modes, altitude grammar (concept →
architecture → technical), stage/diagram grammar, provenance and code/diff
conventions, present Comment interaction rules, and skill doctrine that
authoring composes into that system rather than inventing free-form pages.
Consuming product brand and product UI are out of scope. Normative craft
intent is recorded in
[`utility-presentation-system-design-intent-2026-08-07.md`](https://github.com/sathyassn/codeflow/blob/main/docs/verification/utility-presentation-system-design-intent-2026-08-07.md).

**Present stack.** Keep **Rust-owned document DOM** and **Preact-owned chrome
only** as in ADR-0049. Implement the settled design system in that chrome
(including Comment mode). Do **not** migrate present to React + shadcn for this
epic. shadcn/Base UI may inform patterns and prototypes; they are not required
runtime dependencies.

**Portal stack.** Keep **Astro Starlight + Pagefind** as the generator and
search baseline (ADR-0048 / SPC-005). Apply the **same design-system tokens and
page grammar** through custom layout/CSS (and small portal-local components as
needed). Do **not** replace the portal with a React SPA docs framework for this
epic.

**Unity.** Shared craft is the design system and asset/token contracts—not a
single SPA monorepo. Present remains a session-scoped review surface; portal
remains a static source-linked guide. The CLI and skills exist so each
invocation applies that craft to **new** subject matter. The design-exploration
board that compared both profiles is a reference, not product HTML and not a
session document to clone.

## Consequences

What gets easier: visual recovery and skill updates can ship on the existing
qualified pipelines; offline binary embedding, dual-tree annotation safety, and
portal SSG/search stay intact; one doctrine for how models present information.

What gets harder: two UI technology surfaces (Preact chrome vs Starlight HTML)
must stay visually coherent through tokens and review; shadcn-shaped design
artifacts must be translated rather than pasted.

What is ruled out for this epic: React+shadcn as the default shipped library for
both utilities; postponing stack choice until after another migration; treating
Starlight’s default theme as the design system.

A later ADR may revisit present chrome (e.g. React) only with remeasured
bundle, dual-tree, and qualification evidence. Portal departure from Starlight
requires the same bar against SPC-005.

## Architecture impact

none — runtime boundaries of ADR-0048 and ADR-0049 stand; this ADR adds the
utility presentation design-system scope and freezes stack for EPC-005 recovery.

## Update 2026-10-23: page anatomy, type scale and figure companions

TSK-444 settled the design direction with the operator from rendered options
(`docs/verification/evidence/tsk-444/`) and changed the shared kit and both
products together. The stack decision above stands.

- Page anatomy A1: the document title, summary and provenance meta open the
  present document column, as the portal's title and lede already open a
  page; the paragraph after a section heading is that section's lead. No
  section block is added; headings stay in narrative Markdown.
- Type scale S2 in `tokens.css` and every product sheet: display 34, lede 19,
  h2 24, h3 18, block title 15 (floor 15), body 15, caption 13, micro 12.5 px
  at the default size, times 0.94 compact and 1.12 large, so title > section
  > subsection > block title at every size step.
- Four colour roles join `tokens.json` (`fig-cat1`, `fig-cat1-soft`,
  `fig-cat2`, `fig-cat2-soft`, palette "Cobalt and plum"), one set per mode
  shared by Graphite, Slate and Sage; accents and greys are unchanged. The
  design kit test holds every companion ink at 4.5:1 on surface, canvas,
  figure fill and its tint, and the figure ink at 4.5:1 on each tint.
