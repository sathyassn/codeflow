# Utility presentation system (shared skill resource)

**Status:** normative for every `cf-present` and `cf-docs-portal` invocation.
This file is byte-identical in both skills; the explanation method that comes
before it is `explanation-method.md` beside it, and the figure doctrine it
points to is `figure-grammar.md`, both also byte-identical; the
profile-specific rules live in each skill's `references/visual-craft.md`. The
durable architecture record is `docs/architecture/utility-presentation.md`
(ADR-0053, ADR-0063, ADR-0068).
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

The evidence board that settled this system is `docs/verification/tsk-014-w5/`:
its `shared/svg.js` is the mark vocabulary the figure grammar distils, and its
`baselines/` (plain chat, plain Markdown and plain HTML for every scorable
surface) are the negative controls every figure must beat. It is **evidence
only**. Do not clone it: never re-render its cases, portal families or
comments as a present session, a portal page, or product HTML. Each
invocation applies the same craft to **new** subject matter. Comment is
present-only chrome the runtime already owns; the portal has **no** Comment
lifecycle.

`cf-design` stays product-generic. It does not own this utility.

---

## 1. Separation of planes

| Plane | Owns | Does not own |
|-------|------|--------------|
| **Utility presentation system** | Tokens, altitude, figure grammar, themes, type roles, present Comment SM, portal craft overlay | Product brand, consumer UI kits |
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

The portal is a guide to the project as it stands: what it is, what it does,
how to adopt it, how it is built and how it is operated. Every explanatory
page walks the altitude trio, `## Concept` / `## Architecture` /
`## Technical`, and each panel pairs one carrier with the prose that frames it:
a lead sentence above the figure saying what the reader is looking at, labels
inside it, and the acting sentences or table below. Figures without framing
text and text without a figure both fail. Decisions, epics, tasks and specs are
not portal pages: one pointer page names their folders.

| Class | First screen must show | Required carrier | Prose role |
|-------|------------------------|------------------|------------|
| **Orient** (purpose, capabilities, adoption, journeys) | The Concept panel: one governing claim in the display role and one figure | A figure in every panel, chosen by the altitude contract in `figure-grammar.md`: Concept owns what the subject is, who it is for and what it is not; Architecture the structure; Technical the commands, files and tables | One lead above each figure; the acting sentences below; bullets only where they aid scanning |
| **Architecture** (system, subsystems, boundaries) | The Concept panel of the trio | The trio with a figure in every panel; Architecture in the structure, layering, derivation or graph family | Frames each panel; never the carrier |
| **Reference** (operations, CLI, checklists, evidence) | The lookup form: table, status, code, diff | The trio where the page explains; every how-to section carries a sequence, state or extent figure; tables and evidence blocks where it looks up | Minimal; lookup, not essay |
| **Record pointer** (decisions, epics, tasks, specs) | One table: folder, purpose, count, repository link | The pointer table, generated from configuration; no per-record pages in the guide (the adapter's records switch is off by default) | One sentence: the records live in the repository |

Accepted decisions stay append-only in the repository and are cited by id
from the pages that rely on them.

### Page classes in configuration (portal)

Every source is an explanatory page unless `portal.config.json` declares
otherwise in `page_classes`, one entry per `source` path or `prefix`. A
declaration never drops a route or changes source bytes; the evidence manifest
lists every route with its class, reason and bound figures, and the build
prints the counts.

| Class | Declared by | What the gate demands |
|-------|-------------|-----------------------|
| **Explanatory** (default) | nothing | The trio, a figure in every panel, and a table beside the Technical figure; a table never stands in for a figure. A `page_carriers` entry `{ "source": ..., "technical": "list" }` lets a checklist carry a list there instead of the table |
| **Illustrated source** | `{ "source" or "prefix", "class": "illustrated" }` | The source rendered as it is, at least one companion figure at the page head, every bound figure held to all twelve rules; no trio |
| **Pass-through** | `{ ..., "class": "pass-through", "reason": ... }` | The source rendered as it is, no figure; `reason` is `accepted-record`, `governance` or `no-relationship`, and `no-relationship` also records the design primary's judgment in `note` |
| **Derived lookup** | `{ ..., "class": "derived-lookup", "derive": "capability-registry" }` | A generated table with a fidelity check against its source |

Figures bind in `figures`, never by a marker inside a source: each binding
names a `declaration` file and a published `route`, plus `panel` (an
explanatory page's altitude) or `anchor` (a heading in an illustrated source;
omit it for the page head). A binding to a missing route or anchor fails the
build. Declarations are committed inputs under the adapter's pins, and a
fact a companion asserts is re-derived from the source anchor it names, so a
wrong fact under a valid anchor fails rule 6. Pages and Markdown twins
attribute companion content to its declaration, never to the source.

A present document walks the same altitudes as a path: the Concept carrier
first, an Architecture view only when a second structural view is needed,
Technical panes, then one Ask (`feedback_prompt`).

### Supported carriers

List only what the runtimes render. Anything else is **unsupported** and must
not be promised in a source or a document.

| Carrier | Portal (adapter) | Present (runtime blocks) |
|---------|------------------|--------------------------|
| Family figure (the default form, `figure-grammar.md`) | Figure block: a declaration file bound in `portal.config.json` `figures` to a route and a panel or section anchor; the adapter draws it with the grammar module, and the figure gate holds it to the twelve rules | `figure` block carrying the declaration; the runtime draws it with the same grammar module (authored binding only) |
| Subject-led labeled flow | `cf-stage` fence (nodes `NAME \| sublabel @role`, `->` between stages, one `caption:`; roles accent / positive / warn / danger / neutral; limits 6 stages, 5 nodes per stage); the flow family's interim form while the flow specimen fails rule 3 | `html` block: the authored primary stage (utility tokens, labeled nodes, named edges) |
| Altitude trio | depth-2 `## Concept`, `## Architecture`, `## Technical` rendered as a tablist; at least two of the three | block order as attention order; `tabs` only for true peer views |
| Full-width figure | the figure block; a fenced `text` block only for a chat-grade sketch | `figure` block; `html` only for a static layout the families cannot express |
| Table | Markdown table | `table` |
| Hierarchy | **unsupported** in the portal (use a structure figure) | `tree` |
| Code, diff, evidence | fenced code | `code` / `diff` / `status` |
| Media (a screenshot or photograph) | a committed PNG, JPEG, GIF or WebP image; an SVG file fails closed | `media` |
| Callout | not a carrier | `callout`, sparingly |
| Reading and framing | Markdown prose and lists | `narrative` / `bullets` / `comparison` / `decision` / `disclosure` / `feedback_prompt` |
| Mermaid | **unsupported**: a Mermaid fence renders as plain code | `diagram` block, supporting form only, never the primary carrier |

Unsupported on both surfaces: arbitrary diagram syntaxes beyond the forms
above. In the portal, hierarchy trees and Mermaid are also unsupported; use a
family figure there.

### Screenshots and raster images

A screenshot shows a surface as it is and never a relationship; a
relationship is drawn in a family.

- Capture CodeFlow utility chrome (a portal or present screen) in the
  Graphite skin in light at 2x, unless the subject is a skin or a mode. A
  screenshot of a consuming project's own product keeps that product's
  default appearance.
- Crop to the surface plus a margin of 16 CSS px on every side (32 image
  pixels at 2x); this doctrine owns that margin.
- Annotate only with numbered markers keyed in the caption; never draw arrows
  on it.
- Write alt text that names the surface and its state.
- Save chrome as PNG and photographs as WebP, inside the adapter's media
  limits (`scripts/adapter.mjs` in the portal): 8 MiB per file
  (`MAX_MEDIA_BYTES`), 64 MiB in all (`MAX_TOTAL_MEDIA_BYTES`), and the
  dimension and pixel checks `content-contract.md` records.
- Commit it beside its source, in a folder named for the page.
- Refresh it when the surface changes.

An imported raster diagram is never a carrier: redraw it in a family.

---

## 3. Shared craft (both profiles)

### Experience target

Exact, calm, subject-led, full-width with breathing margins. **Structure
carries meaning before prose.** Progressive altitude: each level complete, not
a teaser dump.

### Altitude grammar

Figures lead at every altitude and in every how-to section. The altitude
contract (reader question, families, prose role per altitude) is stated once,
in `figure-grammar.md` section 3; the reader of each altitude is named in
`explanation-method.md` stage 1.

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
| Font | Archivo (instrument), Inter (editorial), IBM Plex Sans (ink) | `--cf-font-sans`; bundled Latin variable faces, system fallbacks, no remote fonts |
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

### Figure grammar

`figure-grammar.md` is the figure doctrine: nine families keyed to the
relationship they encode (flow, structure, layering, sequence, state,
coverage, extent, derivation, graph), twelve rules every figure obeys, the
altitude contract, the mark vocabulary and the declaration schema (ADR-0068).
Load it before drawing anything; load `figure-grammar-specimens.md`, one drawn
specimen per family, when authoring a figure. In brief:

- Choose the family by the relationship the reader must see; one figure, one
  family, one governing idea that survives with its title masked.
- Every mark keyed; every pair of states separated by two channels that are
  not hue, measured off the render in light and dark.
- Text 12.5 px or larger; every inner mark 9 px or larger; narrow recomposes
  with its own mark set instead of elongating.
- Facts derived from a named source; no text boxes as the form; no text over
  text or over a mark; one-sentence caption; `--cf-fig-*` tokens only; a
  description and a table twin.
- A labelled stage is one flow-family form, not the default; ASCII `text`
  figures draw the same families in plain-text chat.

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
11. A confusable pair: two states told apart by hue alone
12. An inner mark under the 9 px floor inside a cell that passes
13. Overprint: text on text or on a mark, with nothing overflowing
14. Elongation by reflow: a narrow render that grows instead of recomposing

### Review rubric

- **5s:** governing idea with title and caption masked, no wall of cards
- **20s:** engineer can explain the architecture figure and decode every
  keyed pair without the legend
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
  gate, layers over sources, record pointer, verification matrix) with
  `information-architecture.md`, `content-contract.md`, `operations.md`.

---

## 5. What agents generate

| Generate | Do not generate |
|----------|-----------------|
| Content into catalog blocks / portal sources | Custom CSS, remote fonts, component kits |
| One family figure for the governing idea, with its declaration | Decorative card grids of the same prose |
| Supporting text that frames a carrier | Portal-only facts or decisions absent from the sources |
| Evidence-bound claims | Inferred "current" without the manifest |

Self-check against this resource is mandatory.
