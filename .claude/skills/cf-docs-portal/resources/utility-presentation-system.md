# Utility presentation system (canonical skill resource)

**Status:** normative for every `cf-present` and `cf-docs-portal` invocation  
**Product name:** CodeFlow **utility presentation system** (ADR-0053)  
**Not:** product brand, consuming-app design system, free-form agent HTML, or
internal exploration codenames

This file ships **inside the skill**. Load it before authoring, theming, or
revising either utility surface. Repository history under `docs/verification/`
is supporting evidence, not a substitute for this resource.

Stack boundaries: ADR-0049 (present: Rust document / Preact chrome), ADR-0048
(portal: Starlight + Pagefind), ADR-0053 (shared utility craft).

---

## 0. Job of the CLI and skills (do not skip)

The CLI (`codeflow present …`, `codeflow portal …`) and these skills exist so
an agent **reuses one design system** instead of inventing a page each turn.

| Job | Skill / CLI | What the agent authors | What the runtime already owns |
|-----|-------------|------------------------|-------------------------------|
| Explain or review **this session** | `cf-present` | **This** subject's catalog blocks | Chrome, themes, tokens, **Comment system** |
| Durable docs for **CodeFlow or any consuming project** | `cf-docs-portal` | Repository sources + portal config | Docs shell, search, layers, same craft overlay |

The design-exploration board that settled this system showed **both** present
and portal profiles on one board so craft could be compared. That board is a
**design reference only**:

- Use it for tokens, altitude, stage grammar, Comment SM, portal layers, and
  anti-patterns.
- Do **not** re-render its demo subject (system landing with both tabs, sample
  lineage figure, sample notes) as a present session, a portal page, or product
  HTML.
- Do **not** treat “show present” or “show the design” as “clone the reference
  board.” Each invocation applies the same craft to **new** subject matter.
- Comment is present-only chrome the runtime already owns. Portal has **no**
  Comment lifecycle.

`cf-design` stays product-generic. It does not own this utility.

---

## 1. Separation of planes

| Plane | Owns | Does not own |
|-------|------|--------------|
| **Utility presentation system** | Tokens, altitude, stage grammar, themes, type roles, present Comment SM, portal craft overlay | Product brand, consumer UI kits |
| **cf-present** | Ephemeral review document + Comment lifecycle + feedback envelopes | Durable docs, portal search |
| **cf-docs-portal** | Source-linked durable guide, layers, twins, evidence | Session Comment, review verdicts |
| **cf-design** | Generic product/UX craft for **any** consuming surface | Utility tokens, present/portal chrome |

`cf-design` stays product-generic. Utility authoring doctrine lives **only** in
`cf-present` and `cf-docs-portal` (this resource + each skill’s `visual-craft`).

---

## 2. Shared craft (both profiles)

### Experience target

Exact, calm, subject-led, full-width with breathing margins. **Structure
carries meaning before prose.** Progressive altitude — each level complete, not
a teaser dump.

### Altitude grammar

| Altitude | Job | Prefer |
|----------|-----|--------|
| **Concept** | Thesis / outcome / decision in ~5s | One primary visual carrier (stage, diagram, tree, table, media, or justified layout) that still works if sentences are removed |
| **Architecture** | Engineer-legible structure in ~20s | Full-width stage: labeled nodes, named edges, margins |
| **Technical** | Evidence and gates | Status, tables, code, diff — not decorative wrappers around more prose |

### Type roles (author roles, not font names)

| Role | Use for |
|------|---------|
| Display | Title / governing claim (short, literal) |
| Prose | Narrative that needs continuity |
| Label | Kickers, meta, stage labels, chips |
| Mono / evidence | Code, diffs, IDs, measurements, paths |

### Appearance

- Utility themes (present: e.g. technical / editorial; portal: e.g. signal /
  folio) × **light / dark / system** where the surface supports it.
- Semantic colour roles: canvas, surface, text, line, accent, focus, pass /
  warn / danger. Colour is never the only carrier of state.
- Motion is optional chrome only; **meaning holds at rest**. Honour reduced
  motion.

### Stage and diagram grammar

- Full-width stage with margins; large labeled nodes; named edges.
- One governing path at rest; secondary crossings only if they teach.
- Material state change must change structure or labels—not only a tint.
- Mermaid/ASCII may **support** a figure; they are not a substitute for a
  subject-led composition when the claim needs a true stage.

### Anti-patterns (fail closed)

1. Prose re-housed in pretty boxes presented as “visuals”
2. Wall of equal-weight cards / bullets with no primary carrier
3. Permanent annotate `+` on every block (present)
4. Free-form HTML inventing a second visual language
5. Product design-system components or brand packs inside the utility
6. Mermaid/ASCII as the primary page form when geometry should teach
7. Cryptic headings, emoji personality, promotional filler
8. Restyling the same chat answer without a new information structure
9. Portal as a vision / marketing site or second content authority

### Review rubric

- **5s:** governing idea without reading a wall of cards
- **20s:** engineer can explain the architecture figure
- **Subject-led:** structure survives sentence removal
- **Portal:** still reads as docs, not a campaign site
- **Present Comment:** one mode; rail only while armed; harness feedback path works

---

## 3. Present profile (`cf-present`) — summary for portal authors

Present is the ephemeral review surface. Portal authors must **not** copy its
Comment chrome. Present owns: single Comment mode, notes rail only while armed,
submit → `codeflow present feedback` for any harness. Full present doctrine
lives in the `cf-present` skill’s copy of this resource and its visual-craft.

---

## 4. Portal profile (`cf-docs-portal`)

### Page composition gate (before publish / browser verify)

1. What **question** does this page answer better than raw Markdown in the repo?
2. Is there a **primary structural carrier** (stage, table, tree, diagram) where
   architecture or relationship is the claim?
3. Are concept → architecture → technical each complete for their audience?
4. Does the architecture view survive sentence removal?
5. Is content still **source-in-place** (no portal-only prose authority)?
6. Themes × light/dark readable; no product brand pack forced into Starlight?
7. Evidence manifest still authenticates claims after the change?

### Layers over source-in-place

```text
purpose and mental model            (concept)
  → capabilities and journeys
    → architecture, decisions, work   (architecture)
      → technical references, evidence  (technical)
```

- Same craft tokens overlaid on the docs shell (nav, crumbs, search, source pins).
- Agents maintain repository sources; portal is derived (adapter + evidence).
- **No** session Comment lifecycle, verdicts, or notes rail.
- Architecture pages: full-width stages with margins—not caption micro-boxes.
- Verification: locked build, evidence verifier, `browser:verify`, both themes ×
  light/dark, WCAG 2.2 AA journeys per `references/operations.md`.

### Runtime mapping

| Concern | Owner |
|---------|--------|
| Source docs | Repository paths named in `portal.config.json` |
| Derived site | Starlight generator + adapter + evidence manifest |
| Utility craft overlay | Portal styles / themes under ADR-0053 |
| Product brand | Consuming app only — never back-feed portal themes |

---

## 5. What agents generate

| Generate | Do not generate |
|----------|-----------------|
| Content into catalog blocks / portal sources | Custom CSS, remote fonts, component kits |
| One primary visual form for the governing idea | Decorative card grids of the same prose |
| Layered routes from real sources | Portal-only second authority |
| Evidence-bound claims | Inferred “current” without the manifest |

Self-check against this resource is mandatory. Portal operations continue in
`references/visual-craft.md`, `information-architecture.md`,
`content-contract.md`, and `operations.md`.
