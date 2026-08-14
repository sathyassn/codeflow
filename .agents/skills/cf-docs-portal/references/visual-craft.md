# `cf-docs-portal` visual craft

**Required load order (do not skip):**

1. [resources/utility-presentation-system.md](../resources/utility-presentation-system.md)
   — canonical utility presentation system (altitude, anti-patterns, planes)
2. This file — portal-only operational checklist
3. [information-architecture.md](information-architecture.md) before source roots / layers
4. [content-contract.md](content-contract.md) before source interpretation changes
5. [operations.md](operations.md) before install / publish / acceptance evidence
6. Prefer [resources/portal-page-shape.example.md](../resources/portal-page-shape.example.md)
   as the shape of architecture pages

If a page violates the canonical resource’s anti-patterns or fails the portal
composition gate, **do not** treat it as craft-complete. Fix sources or refuse
decorative portal chrome.

`cf-design` stays product-generic. This skill is **utility portal** only.

---

## 1. How to think about a portal page

The reader lands in a **docs shell** (nav, crumbs, search)—not a session
review. Your job is still structural: what do they **see** in the first
screen of this layer, and does architecture use **layout** (stage, table,
tree) or only more prose under a heading?

- **Concept** pages orient: one mental model, not a dump of every capability.
- **Architecture** pages must work if sentences thin out—nodes and edges, not
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

## 3. Altitude on durable pages

```text
purpose and mental model          (concept)
  → capabilities and journeys
    → architecture, decisions, work   (architecture)
      → technical references, evidence  (technical)
```

Each layer complete for its audience. Architecture: full-width stages with
margins and engineer-legible structure—not caption micro-boxes.

## 4. Themes and type

- Bundled utility themes (e.g. **signal**, **folio**) via `portal.config.json`.
- Author for type roles; themes own faces and scale.
- Project may adapt utility once from brand; never feed portal palette/type/
  components back into the product design system.

## 5. Motion

Minimal (nav, theme preference). Meaning at rest. Reduced motion. No
incorrect-mode flash on first paint.

## 6. Verification (visual)

When craft or theme changes:

- both themes × light/dark (and system preference path)
- contrast, focus, keyboard order, reduced motion
- layered journeys and overflow on narrow widths
- `npm run browser:verify` and `codeflow validate --portal …` per operations

Judgment: portal still reads as **docs**; structure survives sentence removal
on architecture pages.

## 7. Before publish

- [ ] Can name what the first screen teaches without a bullet recap
- [ ] Architecture claims use structure, not only headings in prose
- [ ] Source-in-place only; no portal-only second authority
- [ ] Evidence manifest still authenticates claims
