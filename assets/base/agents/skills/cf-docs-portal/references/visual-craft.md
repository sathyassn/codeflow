# `cf-docs-portal` visual craft

**Load order:** the one list in [SKILL.md](../SKILL.md).

If a page violates the canonical resource’s anti-patterns or fails the portal
composition gate, **do not** treat it as craft-complete. Fix sources or refuse
decorative portal chrome.

`cf-design` stays product-generic. This skill is **utility portal** only.
Author repository sources for this project or any consuming project, do not
copy present Comment chrome, and draw figures to
`resources/figure-grammar.md`. Prefer
[resources/portal-page-shape.example.md](../resources/portal-page-shape.example.md)
as the shape of an explanatory page.

---

## 0. Page composition gate (before publish / browser verify)

1. What **question** does this page answer better than the raw Markdown?
2. Which page class is it (orient, architecture, reference, record pointer),
   and does it carry that class's required carrier? Is a source that must not
   be edited declared illustrated or pass-through in `page_classes`?
3. Are concept → architecture → technical each complete for their audience?
4. Does the architecture view survive sentence removal?
5. Is every **claim** traceable to a repository source, while the page itself
   is a composed visual of those sources with supporting text, not the
   Markdown re-printed?
6. Themes × light/dark readable; no product brand pack forced into Starlight?
7. Evidence manifest still authenticates claims after the change?

A page that fails this gate is not craft-complete: fix the source composition
or refuse decorative chrome.

## 1. How a portal page thinks

The reader lands in a **docs shell** (nav, crumbs, search), not a session
review. One page taken through `resources/explanation-method.md`: the git
discipline page of a consuming repository whose remote protection has been
verified active. Each panel names its reader, the question the reader leaves
with, and the family that answers it.

| Panel | Reader | Question | Family and what it draws |
|---|---|---|---|
| Concept | someone deciding whether CodeFlow's enforcement fits their repository | what does it enforce, and what does it leave to people | structure: the repository and its remote as two regions, the four planes placed in them, the human merge as the one decision no plane makes |
| Architecture | an engineer who will change or add a gate | which plane covers which moment of a change, and which one is the boundary | layering: hooks, git-guard, CI and remote protection over edit to merge |
| Technical | a reviewer checking a rule | which plane enforces each rule, and is any rule unclaimed | coverage: rules against planes, one mark per cell, with the table twin beside it |
| How-to: land a change | someone landing a change now | what do I do before the first edit, and what tells me it worked | sequence: worktree list, fetch, `codeflow work start`, first edit |

Substitute your repository's verified enforcement state before drawing these
panels. CodeFlow's own repository has remote protection unavailable (its
`AGENTS.md`, project-specific instructions), so its page draws three planes and
says in the captions that no remote plane is armed.

The walk shows what the method's stages decide on a portal:

- Stage 1 gives each panel its own reader; a panel that serves two readers is
  two panels or a how-to section.
- Stage 3 picks one family per panel by relationship, so the three panels
  draw three different families and none restates another.
- The how-to section carries a sequence, state or extent figure between its
  lead and its steps.
- Stage 5's removal check is the gate's item 4: with the sentences gone, the
  Architecture figure still shows the planes and the boundary.

Same utility craft as present; a different job (a durable source-linked guide).

## 2. Same craft, different shell

| Shared with present | Portal-only |
|---------------------|-------------|
| Semantic tokens, type roles, altitude, figure grammar | Left nav, crumbs, search, source pins |
| Light / dark (and system where configured) | Starlight + Pagefind shell |
| Anti-patterns (prose-in-boxes, walls of cards) | No session Comment / verdicts |
| Family figures drawn by one grammar module | Evidence manifest, Markdown twins, `llms.txt` |

## 3. Layers composed from sources in place

```text
purpose and mental model              (concept)
  -> capabilities and journeys
    -> architecture and boundaries in effect   (architecture)
      -> reference, operations and evidence    (technical)
records: decisions, epics, tasks and specs are pointed to as folders
```

Each layer complete for its audience. Architecture: full-width figures with
engineer-legible structure, not caption micro-boxes.

Author the trio as depth-2 sections, `## Concept`, `## Architecture` and
`## Technical`, in the repository source. The adapter renders them as a
**real altitude tablist**: exactly one layer visible at a time, arrow-key
navigable, the selected layer recorded in the URL hash (`#architecture` loads
that panel only). Raw source HTML stays escaped, so no source hand-authors a
figure or chrome.

Composition gate by page class: every explanatory source (orient, architecture,
reference that explains) carries the trio with **a figure in every panel**,
chosen by the altitude contract in `figure-grammar.md`; the Technical panel
adds a table, which never stands in for its figure. An orient page leads with
one governing claim, not a bullet wall; the record pointer page is one table
of folders with purpose, count and repository link and is never expanded into
per-record pages. Author the trio on **every** explanatory source, never one
hero page. Browser verification fails closed when a trio page shows more than
one layer at once, when an explanatory page has no trio, and when any panel
lacks its figure; it exercises every tabbed route, not the first it finds.

### The figure block (the default carrier)

A figure is a declaration file (`figure-grammar.md` section 6), committed in
the repository and bound to its page in `portal.config.json` `figures`: a
`route` plus a `panel` for an explanatory page, or an `anchor` (or none, for
the page head) for an illustrated source. Nothing marks the source. The adapter
checks the declaration, re-derives each fact from the source anchor it names,
draws the figure with the grammar module and the kit's `figure.css`, and
records it in the evidence manifest. `browser:verify` then holds every drawn
figure to the twelve rules at 1440 and 390 px in light and dark, and names the
page, the panel or anchor, the declaration and the rule when one fails.

A source the guide must not edit, such as an instruction an agent loads, is
declared `illustrated` in `page_classes`: it renders as it is, with its
companion figures above it or at the head of an anchored section, each
captioned as declared outside the source. A source with nothing to draw is
declared `pass-through` with a reason from the closed set. The doctrine's
"Page classes in configuration" subsection is the reference.

### `cf-stage`: the flow interim

A `cf-stage` fence stays only as the flow family's interim form while the flow
specimen fails rule 3. Node lines are `NAME | sublabel @accent` (roles
`accent` / `positive` / `warn` / `danger` / `neutral`), a `->` line separates
stages (nodes inside one stage are parallel), and one `caption:` line ends it.
The adapter renders it with the `--cf-*` tokens; invalid grammar fails the page
loudly. A stage counts as a stage, never as the figure a panel demands.

## 4. Themes and type

- Bundled skins via `portal.config.json`: **Graphite**, **Slate**, **Sage**;
  light/dark from Display. Font is independent: Archivo, Inter (the unset
  default), or IBM Plex Sans, bundled locally with system fallbacks.
- Author for type roles; Display controls face and scale separately from skin.
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
- [ ] Every explanatory panel has its figure, and every bound figure passes the figure gate
- [ ] Every claim traces to a source; supporting framing text is expected, not a second authority
- [ ] Evidence manifest still authenticates claims
