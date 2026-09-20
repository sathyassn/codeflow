# Utility presentation system (shared skill resource)

**Status:** normative for every `cf-present` and `cf-docs-portal` invocation.
This file is byte-identical in both skills; the profile-specific rules live in
each skill's `references/visual-craft.md`. The durable architecture record is
`docs/architecture/utility-presentation.md` (ADR-0053, ADR-0063).
**Product name:** CodeFlow **utility presentation system**.
**Not:** product brand, consuming-app design system, free-form agent HTML, or
internal exploration codenames.

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

The design-exploration board that settled this system is a **design reference
only**: use it for tokens, altitude, stage grammar, Comment SM, portal layers,
and anti-patterns; never re-render its demo subject as a present session, a
portal page, or product HTML. Each invocation applies the same craft to **new**
subject matter. Comment is present-only chrome the runtime already owns; the
portal has **no** Comment lifecycle.

`cf-design` stays product-generic. It does not own this utility.

---

## 1. Separation of planes

| Plane | Owns | Does not own |
|-------|------|--------------|
| **Utility presentation system** | Tokens, altitude, stage grammar, themes, type roles, present Comment SM, portal craft overlay | Product brand, consumer UI kits |
| **cf-present** | Ephemeral review document + Comment lifecycle + feedback envelopes | Durable docs, portal search |
| **cf-docs-portal** | Source-linked durable guide, layers, twins, evidence | Session Comment, review verdicts |
| **cf-design** | Generic product/UX craft for **any** consuming surface | Utility tokens, present/portal chrome |

---

## 2. The composition rule

**A utility surface is a composed visual presentation of its subject, with
supporting text. It is never the subject's prose re-rendered.**

- A present document that restates a chat answer as blocks fails.
- A portal page whose body is the source Markdown re-printed under the docs
  shell fails. The portal composes what the sources say; every claim on the
  page traces to a repository source, and supporting sentences that make a
  carrier legible are expected, not a second authority.
- Structure carries meaning before prose: remove the sentences and the page
  must still argue.

### Page classes (portal) and document shapes (present)

| Class | First screen must show | Required carrier | Prose role |
|-------|------------------------|------------------|------------|
| **Orient** (purpose, capabilities, adoption, journeys) | One governing claim in the display role | One primary carrier: stage, table, or full-width figure | One short lead; bullets only where they aid scanning |
| **Architecture** (system, subsystems, boundaries) | The Concept panel of the altitude trio | `## Concept` / `## Architecture` / `## Technical` trio plus a subject-led `cf-stage` (or a justified `text` figure) | Frames each panel; never the carrier |
| **Technical and records** (references, specs, epics, tasks, evidence) | The lookup form: table, status, code, diff | Tables and evidence blocks | Minimal; lookup, not essay |
| **Historical decisions** (accepted ADRs) | Their own append-only text | Exempt from the trio; rendered as records | Unchanged |

A present document follows the same ladder as a path: Concept carrier first,
Architecture view only when a second structural view is needed, Technical
panes, then one Ask.

### Supported carriers

List only what the runtimes render. Anything else is **unsupported** and must
not be promised in a source or a document.

| Carrier | Portal (adapter) | Present (runtime blocks) |
|---------|------------------|--------------------------|
| Subject-led labeled flow | `cf-stage` fence (nodes `NAME \| sublabel @role`, `->` between stages, one `caption:`; roles accent / positive / warn / danger / neutral; limits 6 stages, 5 nodes per stage) | figure / stage block |
| Altitude trio | depth-2 `## Concept`, `## Architecture`, `## Technical` rendered as a tablist; at least two of the three | block order as attention order |
| Full-width figure | fenced `text` block inside a trio panel | figure block |
| Table | Markdown table | table block |
| Code, diff, evidence | fenced code | code / diff / status blocks |
| Media | image with committed source | media block |
| Callout | not a carrier | callout block, sparingly |
| Mermaid | **unsupported**: a Mermaid fence renders as plain code | `diagram` block, supporting form only, never the primary carrier |

Unsupported on both surfaces: tree carriers and arbitrary diagram syntaxes.
In the portal, Mermaid is also unsupported; do not use it as a figure there.

---

## 3. Shared craft (both profiles)

### Experience target

Exact, calm, subject-led, full-width with breathing margins. **Structure
carries meaning before prose.** Progressive altitude: each level complete, not
a teaser dump.

### Altitude grammar

| Altitude | Job | Prefer |
|----------|-----|--------|
| **Concept** | Thesis / outcome / decision in ~5s | One primary visual carrier that still works if sentences are removed |
| **Architecture** | Engineer-legible structure in ~20s | Full-width stage: labeled nodes, named edges, margins |
| **Technical** | Evidence and gates | Status, tables, code, diff, not decorative wrappers around more prose |

### Type roles (author roles, not font names)

| Role | Use for |
|------|---------|
| Display | Title / governing claim (short, literal) |
| Prose | Narrative that needs continuity |
| Label | Kickers, meta, stage labels, chips |
| Mono / evidence | Code, diffs, IDs, measurements, paths |

### Appearance

Three utility skins, each in light and dark, selectable at runtime through the
Display panel on both surfaces. The portal's bundled theme names map to skins
at build time: `signal` → instrument, `folio` → ink.

| Display control | Values | Token effect |
|-----------------|--------|--------------|
| Font | Archivo (instrument), Inter (editorial), IBM Plex Sans (plex) | `--cf-font-sans`; bundled Latin variable faces, system fallbacks, no remote fonts |
| Size | Compact 0.94 · Default 1.00 · Large 1.12 | `--cf-ui-scale`; floors below are never crossed |
| Palette | Neutral = instrument · Cool = editorial · Warm = ink | full semantic role set per skin |
| Appearance | Light · Dark · System | `data-theme`; no wrong-mode flash before first paint |

Type floors (px, held under Compact): micro 12.5 · caption 13 · ui 13.5 ·
body 15. Reading measure 68ch. Radius 0.5rem. Semantic colour roles: canvas,
surface, surface-raised, surface-subtle, text, text-muted, border,
border-strong, accent, accent-strong, accent-soft, focus, positive, warning,
danger, diagram-line. Colour is never the only carrier of state. The portal
token sheet and the present stylesheet must be value-identical per role; a
Rust contract test fails the build on drift.

### Motion

Optional and purposeful: it may explain a transition, sequence, or causal
relationship, or give interaction feedback. **Meaning holds at rest**; honour
reduced motion with an equivalent static explanation. No decorative loops.

### Stage and diagram grammar

- Full-width stage with margins; large labeled nodes; named edges.
- One governing path at rest; secondary crossings only if they teach.
- Material state change must change structure or labels, not only a tint.
- ASCII `text` figures may **support** a stage; they are not a substitute for
  a subject-led composition when the claim needs a true stage.

### Anti-patterns (fail closed)

1. Prose re-housed in pretty boxes presented as "visuals"
2. Wall of equal-weight cards / bullets with no primary carrier
3. Permanent annotate `+` on every block (present)
4. Free-form HTML inventing a second visual language
5. Product design-system components or brand packs inside the utility
6. ASCII or code as the primary page form when geometry should teach
7. Cryptic headings, emoji personality, promotional filler
8. Restyling the same chat answer without a new information structure
9. Portal as a vision / marketing site or second content authority
10. A page that is the source Markdown re-rendered (see §2)

### Review rubric

- **5s:** governing idea without reading a wall of cards
- **20s:** engineer can explain the architecture figure
- **Subject-led:** structure survives sentence removal
- **Portal:** still reads as docs, not a campaign site
- **Present Comment:** one mode; rail only while armed; harness feedback path works

---

## 4. Profiles

The shared rules end here. Each surface's operational contract lives in its
own skill and is loaded after this file:

- **Present:** `cf-present/references/visual-craft.md` (document composition
  gate, Comment surface, feedback pipeline, runtime mapping) and
  `resources/how-presentation-works.md`.
- **Portal:** `cf-docs-portal/references/visual-craft.md` (page composition
  gate, layers over sources, verification matrix) with
  `information-architecture.md`, `content-contract.md`, `operations.md`.

---

## 5. What agents generate

| Generate | Do not generate |
|----------|-----------------|
| Content into catalog blocks / portal sources | Custom CSS, remote fonts, component kits |
| One primary visual form for the governing idea | Decorative card grids of the same prose |
| Supporting text that frames a carrier | Portal-only facts or decisions absent from the sources |
| Evidence-bound claims | Inferred "current" without the manifest |

Self-check against this resource is mandatory.
