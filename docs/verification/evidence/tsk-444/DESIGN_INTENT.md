# TSK-444 design intent: presentation and portal design direction

Phase 1 of TSK-444: the direction, shown to the operator as rendered pages,
before any token or style changes. The operator's sign-off (AC-1) is recorded
in this file once the review session returns its answers; until then the
`SETTLED_DIRECTION` below is the design primary's recommendation.

Process weight: direction pass, rendered candidates compared (the open rung
of `cf-design` section 1). The three questions are decomposed, one rendered
comparison each, because a compound winner would be a compromise nobody
chose.

```text
DESIGN_INTENT:
  APPLICABILITY: direction pass (page anatomy, type scale, figure colour)
    for the two utility surfaces, present and the docs portal
  CREATOR_INTENT: documents that are visually rich and explain concepts with
    figures; every page and section leads with context; headings in one
    correct hierarchy; the governing path in colour, grey only for context;
    elegant and coherent across Graphite, Slate and Sage, light and dark
    (operator direction, 2026-10-22 and 23, TSK-444 description)
  AUDIENCE_JOB_CONTEXT: an operator reviewing a present document or reading a
    portal page, usually on a wide window, sometimes compact size; needs to
    know what the page is about before the first block, and to see the one
    relationship a figure carries before reading (explanation-method stage 1;
    operator report in TSK-444; no other research, so the rest is inferred)
  EXPERIENCE_TARGET: exact, calm, subject-led (utility-presentation-system
    section 3); the figure says its idea in five seconds with the title
    masked (figure-grammar rule 1)
  LANGUAGE_AND_VOICE: copy guide and writing rules; collapsed, no new strings
    beyond block titles and legend text
  APPEARANCE_MODES: Graphite, Slate, Sage in light and dark; system follows
    the OS; persistence as today (cf-present-theme and cf-present-mode in
    present, cf.skin and cf.theme on the portal); every candidate rendered
    in all six pairs
  VIEWPORTS_AND_PLATFORMS: web only; 1280 px window verified; the present
    document column of about 900 px; the compact and large sizes change
    nothing in the order of the scale (the floors hold); narrow (375 px)
    not rendered in this phase
  WRITING_DIRECTION_AND_I18N: collapsed (LTR, en only)
  SYSTEMS_AND_CONSTRAINTS: the kit (tokens.json, tokens.css, chrome.css,
    figure.css) is the system; rule 3 (never hue alone) and rule 10 (token
    only colour); contrast floors text 4.5, accent 4.5 on surface, focus 3
    (document-authoring "Utility tokens"); warn and stop hues reserved;
    present html blocks refuse cf- classes and data-cf-* outside the closed
    vocabulary, so the candidates carry an fg- copy of the figure sheet
  OPERATOR_DIRECTION: binding: context first, correct hierarchy, governing
    path in colour, taste is the operator's; colours may be the existing
    ones or added ones that harmonise; no hard restriction
  RESEARCH_OR_REFERENCES: the docs-portal audit (29 of 70 repo figures draw
    no accent mark); the present runtime styles (comparison and decision
    titles 1.15rem, h3 1.12rem, h2 clamp 1.45 to 2rem, no h1 or summary in
    the document column: crates/codeflow-present/web/src/styles.css:567-641);
    the kit scale (tokens.css:10-18); OKLCH derivation of companion hues
  COMPOSITION:
    subject: a present document and a portal page: head (title, summary,
      meta), sections (## with a lead), subsections (###), blocks with titles
      (comparison columns, decision, figure line), and figure marks
    governing idea: where am I and what is this about (anatomy); what
      contains what (scale); what is the one thing this figure says (colour)
    user action: read, then decide or comment
    primary form: the sample page itself, rendered; the four reference
      figures (structure, layering, sequence, coverage) redrawn per candidate
    unit and axis: anatomy: the page head and a section head; scale: one
      ladder of seven roles in px; colour: the same four figures, colour
      rules the only variable
  DIRECTIONS_CONSIDERED:
    anatomy A1 head and headings: title, summary, meta at the top; a section
      is a ## heading with its first paragraph styled as the lead; no schema
      change. Carrier: the existing title, summary and provenance fields plus
      one style rule.
    anatomy A2 head with anchor lines and section plates: adds four anchor
      lines (what, why, where it stands, decide) and a numbered section plate
      (kicker, title, lead, hairline). Carrier: a new section block and an
      anchor field in the schema; a migration for existing documents.
    scale S1 kit scale as shipped: 30 / 20 / 22 / 16.5 / 14 / 15 / 13.
      Carrier: tokens.css unchanged; present adopts the kit values.
    scale S2 wider steps: 34 / 19 / 24 / 18 / 15 / 15 / 13, about 1.33 per
      step. Carrier: the :root type values in tokens.css change for both
      surfaces.
    colour C1 accent governs: tokens byte-identical; a governing-region mark
      (accent-soft fill, accent edge) and the covered cell in accent.
      Carrier: figure.css and the mark vocabulary.
    colour C2 accent and two companions: two category inks and two tints per
      skin and mode, derived in OKLCH from the accent with hue rotations that
      stay clear of the warn and stop hues; used only for peers the subject
      names (planes, regions, participants), never for the governing path,
      never alone (dash, width and cap stay). Carrier: four new roles in
      tokens.json, tokens.css, figure.css, the portal token sheet and the
      drift test.
    colour C3 tonal ramp: tokens byte-identical; two tints of the accent,
      derived with color-mix in figure.css, grade the regions below the
      governing one. Carrier: figure.css only.
    baseline: the runtime today, rendered as real figure blocks and a mock of
      the present head.
  SETTLED_DIRECTION: recommended, pending the operator's answers:
    A1 (head and headings), S2 (wider steps), C2 (accent and two companions),
    no dedicated section block.
    Why: A1 fixes the head and the leads with the fields the schema already
    has; A2 buys a numbered plate at the cost of a schema change and a
    migration. S2 separates every step by 2 px or more at the compact size,
    where S1's subsection (15.5) and block title (14) nearly meet, and gives
    the document title room to anchor a page that has no breadcrumb. C2 is
    the only candidate that answers the operator's report on a structure or
    layering figure whose peers must be told apart: C1 and C3 colour one
    region and leave the rest grey. C2 stays within the floors (table in
    options.md) and keeps rule 3 by construction, since no category mark
    loses its non-hue channel.
  SYSTEM_SCOPE: earned by recurrence: the type scale (every page), the head
    (every document), the governing-region mark (every structure and
    layering figure) and, with C2, two category roles (every figure with
    named peers). Not earned: a section block (one consumer would be the
    plate); the participant tint in sequence figures (one family; recorded
    as optional, decided at build).
  ACCESSIBILITY_TARGET: WCAG 2.2 AA; ink 4.5 to 1 on ground and canvas, the
    figure ink 4.5 to 1 on every tint, measured; every state pair differs on
    two non-hue channels; text 12.5 px or larger
  REVIEW_RUBRIC: pre-registered: (1) the title and summary read first on
    the page; (2) at 1280 px and compact size, title > ## > ### > block title
    with at least 1 px between each, measured; (3) with the title masked, the
    governing region is named within five seconds; (4) no state pair differs
    by hue alone; (5) every category ink meets the floors in all six pairs;
    (6) the render carries the page: no hidden overflow, no "not drawn"
  FIDELITY_EVIDENCE_PLAN: phase 2 builds the chosen candidates; a render
    test measures computed sizes (AC-3), a contrast table and the browser
    check cover the hues (AC-4), captures per skin and mode and the operator's
    visual sign-off close AC-6
```

## Evidence for this phase

- Rendered candidates: `tsk-444-design-direction.json`, opened with
  `codeflow present open`; the lab page that built them: `lab/`.
- Live captures of the document in all six skin and mode pairs (headless
  Chrome over CDP against the running session), checked for "not drawn"
  text (none) and for every candidate figure drawing: see `options.md`.
- The export per theme and mode confirms the four real figure blocks draw;
  the exported html blocks are unstyled by design (sandboxed frames), so the
  live captures and the lab captures are the evidence for the candidates.

## Operator sign-off

Pending. The answers to the four decision blocks and any notes are recorded
here when the review returns.
