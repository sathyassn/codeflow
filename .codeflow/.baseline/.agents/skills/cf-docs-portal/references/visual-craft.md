# `cf-docs-portal` visual craft

**Required load order (do not skip):**

1. [resources/utility-presentation-system.md](../resources/utility-presentation-system.md)
   is the shared doctrine: composition rule, page classes, supported carriers, tokens
2. This file is the portal profile: page composition gate, layers, verification
3. [information-architecture.md](information-architecture.md) before source roots / layers
4. [content-contract.md](content-contract.md) before source interpretation changes
5. [operations.md](operations.md) before install / publish / acceptance evidence
6. Prefer [resources/portal-page-shape.example.md](../resources/portal-page-shape.example.md)
   as the shape of architecture pages

If a page violates the canonical resource’s anti-patterns or fails the portal
composition gate, **do not** treat it as craft-complete. Fix sources or refuse
decorative portal chrome.

`cf-design` stays product-generic. This skill is **utility portal** only.
Author repository sources for this project or any consuming project. Do not
clone the design-exploration board or copy present Comment chrome.

---

## 0. Page composition gate (before publish / browser verify)

1. What **question** does this page answer better than the raw Markdown?
2. Which page class is it (orient, architecture, reference, record pointer),
   and does it carry that class's required carrier?
3. Are concept → architecture → technical each complete for their audience?
4. Does the architecture view survive sentence removal?
5. Is every **claim** traceable to a repository source, while the page itself
   is a composed visual of those sources with supporting text, not the
   Markdown re-printed?
6. Themes × light/dark readable; no product brand pack forced into Starlight?
7. Evidence manifest still authenticates claims after the change?

A page that fails this gate is not craft-complete: fix the source composition
or refuse decorative chrome.

## 1. How to think about a portal page

The reader lands in a **docs shell** (nav, crumbs, search), not a session
review. Your job is still structural: what do they **see** in the first
screen of this layer, and does architecture use **layout** (stage, table,
full-width figure) or only more prose under a heading?

- **Concept** pages orient: one mental model, not a dump of every capability.
- **Architecture** pages must work if sentences thin out: nodes and edges, not
  caption chips restating paragraphs.
- **Technical** pages are for lookup and evidence, not another essay.

Same utility craft as present; different job (durable source-linked guide). Do
not copy present Comment chrome. Shape example:
[resources/portal-page-shape.example.md](../resources/portal-page-shape.example.md).

## 2. Same craft, different shell

| Shared with present | Portal-only |
|---------------------|-------------|
| Semantic tokens, type roles, altitude, stage grammar | Left nav, crumbs, search, source pins |
| Light / dark (and system where configured) | Starlight + Pagefind shell |
| Anti-patterns (prose-in-boxes, walls of cards) | No session Comment / verdicts |
| Subject-led stages with margins | Evidence manifest, Markdown twins, `llms.txt` |

## 3. Layers composed from sources in place

```text
purpose and mental model              (concept)
  -> capabilities and journeys
    -> architecture and boundaries in effect   (architecture)
      -> reference, operations and evidence    (technical)
records: decisions, epics, tasks and specs are pointed to as folders
```

Each layer complete for its audience. Architecture: full-width stages with
margins and engineer-legible structure, not caption micro-boxes.

Author the trio as depth-2 sections, `## Concept`, `## Architecture` and
`## Technical`, in the repository source. The adapter renders them as a
**real altitude tablist**: exactly one layer visible at a time, arrow-key
navigable, the selected layer recorded in the URL hash (`#architecture` loads
that panel only). Fenced `text` stages render full-width. Raw source HTML
stays escaped, so the grammar lives in Markdown, never hand-authored chrome.

Composition gate by page class: every explanatory source (orient, architecture,
reference that explains) carries the trio with one figure in Concept; an
architecture page adds a subject-led stage (`cf-stage` or a justified figure);
an orient page leads with one governing claim, not a bullet wall; reference
lookups prefer tables, code, and evidence; the record pointer page is one table
of folders with purpose, count and repository link and is never expanded into
per-record pages. Author the trio on **every** explanatory source, never one
hero page. Browser verification fails
closed when a trio page shows more than one layer at once, and when an
architecture-shaped layer contains zero altitude pages; it exercises every
tabbed route, not the first it finds.

A subject-led labeled figure is a `cf-stage` fence: node lines
(`NAME | sublabel @accent`, roles `accent` / `positive` / `warn` / `danger` /
`neutral`),
a `->` line between stages (nodes inside one stage are parallel), and one
`caption:` line. The adapter renders it into generated HTML styled by the
`--cf-*` tokens; invalid grammar fails the page loudly. Keep ASCII `text`
fences as the fallback for shapes the flow grammar cannot express.

## 4. Themes and type

- Bundled themes via `portal.config.json` map to the utility skins: **signal**
  → instrument (Archivo), **folio** → ink (IBM Plex Sans); light/dark from the
  shell toggle. System-fallback faces only, no remote fonts.
- Author for type roles; themes own faces and scale.
- Project may adapt utility once from brand; never feed portal palette/type/
  components back into the product design system.

## 5. Motion

Minimal (nav, theme preference). Meaning at rest. Reduced motion. No
incorrect-mode flash on first paint.

## 6. Verification (visual)

When craft or theme changes:

- all utility skins × light/dark (and system preference path)
- altitude tabs hide inactive layers; hash loads select the named layer
- Display settings (font / size / palette / appearance) apply and persist
- search: `/` focuses the query and a portal-owned term returns a followable hit
- contrast, focus, keyboard order, reduced motion
- layered journeys and overflow on narrow widths
- `npm run browser:verify` and `codeflow validate --portal …` per operations

Judgment: portal still reads as **docs**; structure survives sentence removal
on architecture pages.

## 7. Before publish

- [ ] Can name what the first screen teaches without a bullet recap
- [ ] Architecture claims use structure, not only headings in prose
- [ ] Every claim traces to a source; supporting framing text is expected, not a second authority
- [ ] Evidence manifest still authenticates claims
