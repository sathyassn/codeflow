# `cf-present` design settlement

**Task:** TSK-006

**Date:** 2026-08-01

**Status:** exact dual-model approval; no operator decision outstanding

## Decision

Claude completed an independent design lane before reading the Codex conclusion,
then led a rendered critique and reconciliation. Codex completed the independent
research and prototype lane recorded in
[`cf-present-prototype-study-2026-08-01.md`](cf-present-prototype-study-2026-08-01.md),
challenged feasibility, durability, scope, and verification, and formally
approves the exact `DESIGN_INTENT` below verbatim.

The settled shell is **the reviewer's document**: a document-first reading
canvas with a quiet section route and a subordinate, width-capped,
block-anchored feedback queue. Narrative and graph shapes remain earned blocks
or templates rather than alternate applications. Two curated utility themes,
a system-first font stack, shortcut constraints, and an inspectable document
boundary are settled inside the approved specification and task authority.

## Exact dual-approved `DESIGN_INTENT`

```text
DESIGN_INTENT:
  APPLICABILITY: direction pass — novel CodeFlow utility surface; settled from
    two independent lanes, rendered light/dark comparison, and cross-seat
    reconciliation
  CREATOR_INTENT: turn a complex agent response into a reusable, local,
    reviewable document with attributable feedback, without page construction
    becoming a second task; a CodeFlow utility, never the consuming product's
    design system
  AUDIENCE_JOB_CONTEXT: a solo AI-assisted operator reviewing explanation,
    plans, comparisons, changes, evidence, and open decisions in an active
    agent session (docs/product.md Users; SPC-004 §3/§6; EPC-005)
  EXPERIENCE_TARGET: focused, calm, exact, quietly elegant; document first,
    review state always legible; editorial reading rhythm with utility-grade
    precision; never a dashboard, slide deck, or card grid
  LANGUAGE_AND_VOICE: cf-editorial-review authority order; SPC-004 §6 — plain,
    direct, session-aware, exact terms, outcome first; chrome copy names
    subjects and actions; no canned personality or decorative emoji
  APPEARANCE_MODES: two curated utility themes, each with coherent light and
    dark modes, one default; theme choice never changes the shell or
    information architecture; system-following mode default with persisted
    manual override; CSP-hashed pre-paint theme script + color-scheme meta
    (no incorrect-mode flash); mid-session system change honored unless
    overridden; reduced motion honored via opt-in motion; one shared semantic
    token contract drives surfaces, text, borders, focus, code-highlight
    themes, and diagram themes so all content stays theme- and mode-coherent
    including print/export
  SYSTEMS_AND_CONSTRAINTS: closed versioned JSON+Markdown block schema;
    reusable semantic block catalog rendered into a selection/annotation-
    compatible inspectable DOM boundary; CodeFlow-owned utility tokens;
    optional closed project primitive-token import flows one way; pinned
    no-network system-first sans/mono type stacks (a future bundled OFL font
    requires measured coverage/size/license evidence and an ADR); no remote
    resources and no consuming-product runtime/components; charter §4.4 size
    caps; macOS/Linux/native-Windows/WSL2
  OPERATOR_DIRECTION: SPC-004 §3 qualities are binding (modern, elegant,
    aesthetically coherent, pleasing, functional); no open operator decisions
    remain for TSK-006 — theme count settled by spec conformance (two curated
    themes) and v1 typography settled as the reversible system-first
    convention with an ADR path for change
  RESEARCH_OR_REFERENCES: two independent sourced lanes — W3C WCAG 2.2 / APG /
    Web Annotation, GitHub review transaction, Tufte/Butterick reading
    structure, Radix 12-step scale method, Shiki dual-theme and Mermaid base
    theming, GOV.UK/NN-g disclosure evidence, JSON Schema versioning, and
    observed lavish-axi interaction ideas; no single product is a template
  DIRECTIONS_CONSIDERED: five across lanes — editorial briefing / annotated
    manuscript family (best pure reading; margin feedback rail rejected for
    collision cost and non-persistent review state); document-first review
    workbench (chosen shell); item-centric review console (rejected: shreds
    explanation narrative); staged guided briefing (rejected: punishes
    re-reading and cross-reference, motion/maintenance cost); decision map
    (rejected as default: fabricates topology for non-graph content, retained
    as a block/earned template); dashboards, text-card grids, prose walls,
    and decorative visuals rejected as compositions (SPC-004 §2;
    design-choice audit)
  SETTLED_DIRECTION: the reviewer's document — one document-first reading
    canvas (measure-disciplined, ≤3 heading levels; full-width labelled local
    scrollers for tables/diffs/diagrams); quiet left section route with
    review-state marks; subordinate width-capped right feedback queue with
    block-anchored entries and jump links, collapsing below the artifact or
    into an explicit non-modal sheet at compact widths with one preserved
    reading order; anchor affordances adjacent to each block; feedback =
    typed intent (comment/question/decision/suggestion) × SPC-004 lifecycle
    (received/delivered/addressed/dismissed), anchored to {session, document
    revision, block ID} plus optional quote/position selector, with explicit
    anchored / re-anchored(flagged) / orphaned-but-visible outcomes — a lost
    anchor never silently resolves feedback; notes accumulate as pending and
    submit once as a review with a typed verdict (approve / approve with
    notes / request changes with instruction); status and evidence glyphs are
    bound to real per-item state, never decorative; elevation by hairline +
    surface step, no shadows, one accent per theme; sans-first utility type
    with tabular figures from the pinned system stacks; a simple reply never
    invokes the shell; v1 ships this single shell — narrative and graph
    shapes live in the block catalog, not as alternate apps
  ACCESSIBILITY_TARGET: WCAG 2.2 AA — contrast incl. every syntax token,
    3:1 non-text (borders, focus, diagram strokes), reflow at 320 CSS px with
    the per-block scroller exception, focus not obscured under sticky chrome,
    24 px targets — plus spec-mandated reduced motion (deliberately above AA);
    semantic landmarks/headings/forms; keyboard-operable annotation traversal
    with native Tab/Shift+Tab and explicit controls as the baseline and
    j/k/r/e shortcuts that are context-scoped to the focused feedback
    surface, inactive in editable controls, discoverable, and disableable/
    remappable (WCAG 2.1.4); announced anchor boundaries and aria-details
    wiring; schema-required diagram accTitle/accDescr; diffs carry ins/del
    semantics with non-colour +/- signals and screen-reader boundary text
  FIDELITY_EVIDENCE_PLAN: representative schema fixtures covering the ten-case
    union (incl. simple-reply restraint and unsupported-version raw view);
    render matrix — default theme fully at light/dark/system/persisted-
    override across 320/390/768/1440, second theme on the representative
    subset in both modes; pinned-stack typography verified with
    representative platform renders incl. Linux fallback quality; automated
    axe plus keyboard passes and a manual assistive-technology spot pass;
    shortcut-scope checks (inactive in editable controls, remap/disable);
    anchor-outcome fixtures (edited and deleted annotated blocks);
    theme-flash and mid-session preference-change checks; code and diagram
    token-bridge render checks in both modes and both themes; print/export
    fidelity; console/network zero-remote assertions; cleanup/teardown
    evidence; blinded paired comparison against dashboard and text-card-grid
    decoys in TSK-007's semantic cases
```

## Prototype corrections and boundary

The disposable prototype was corrected before task closeout so its retained
evidence does not teach false behavior: evidence markers derive from explicit
pass/fail/pending/not-run state; map inspection uses semantic keyboard controls;
the section route reflects actual scroll state; the mode control exposes its
active state and next action; diffs use `ins`/`del`, visible non-colour signs,
and screen-reader boundary text; and the type stack is system-first.

This record does not authorize a production renderer or select Lit, Preact, or
Rust-rendered composition. TSK-011 owns the bounded renderer spike and Tier-3
ADR. Real Mermaid integration and its theme/accessibility evidence remain with
TSK-011 and TSK-007; the prototype's text graph is not evidence for Mermaid.

## Approval

- **Claude judgment primary:** approved this exact `DESIGN_INTENT` after its
  independent lane, rendered critique, and cross-seat reconciliation.
- **Codex engineering primary:** approves this exact `DESIGN_INTENT` verbatim
  after feasibility, durability, implementation-size, and verification review.
- **Operator:** no decision outstanding; former theme-count and typography
  questions were settled within approved specification and task authority.
