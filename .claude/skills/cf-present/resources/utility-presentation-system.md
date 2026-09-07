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
- Comment is present-only chrome the runtime already owns. Agents author
  annotatable document structure; they do not rebuild Comment UI.

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
| **Concept** | Thesis / outcome / decision in ~5s | One primary visual carrier (stage, diagram, tree, table, media, or justified `html`) that still works if sentences are removed |
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
- Motion is optional and purposeful: it may explain a transition, sequence, or
  causal relationship, or give interaction feedback. **Meaning holds at rest**;
  honour reduced motion with an equivalent static explanation. Avoid decorative
  loops or animation that delays access to information.

### Stage and diagram grammar

- Full-width stage with margins; large labeled nodes; named edges.
- One governing path at rest; secondary crossings only if they teach.
- Material state change (e.g. offline lineage, reduced confidence) must change
  structure or labels—not only a tint.
- Mermaid/ASCII may **support** a diagram block; they are not a substitute for
  a subject-led composition when the claim needs a true stage.

### Anti-patterns (fail closed)

1. Prose re-housed in pretty boxes presented as “visuals”
2. Wall of equal-weight cards / bullets with no primary carrier
3. Permanent annotate `+` on every block
4. Free-form HTML inventing a second visual language
5. Product design-system components or brand packs inside the utility
6. Mermaid/ASCII as the primary page form when geometry should teach
7. Cryptic headings, emoji personality, promotional filler
8. Restyling the same chat answer without a new information structure

### Review rubric

- **5s:** governing idea without reading a wall of cards
- **20s:** engineer can explain the architecture figure
- **Subject-led:** structure survives sentence removal
- **Portal:** still reads as docs, not a campaign site
- **Present Comment:** one mode; rail only while armed; harness feedback path works

---

## 3. Present profile (`cf-present`)

### How to think (required)

Read **`how-presentation-works.md`** in this skill’s `resources/` folder. The
JSON is a handoff; the open page is the product. Block order is attention
order; block type is a perceptual instrument. Work backward from the 5‑second
picture and a mental top‑to‑bottom walk before encoding.

### Document composition gate (before `present open`)

If any fails, stay in chat or restructure—do not open a text-wall session.

1. What **job** does this surface do that chat cannot?
2. What is the **5‑second picture** (no bullet recap)?
3. What is the **single primary carrier**, and is it first?
4. Does structure still argue if sentences are removed?
5. Does a top‑to‑bottom walk avoid “memo restyled as blocks”?
6. Catalog only; stable IDs; no secrets/paths; one clear ask?

### Comment surface (runtime-owned chrome)

Agents author **document blocks**. They do not rebuild Comment UI.

1. **Single Comment mode** — one control arms annotation; no permanent per-block `+`.
2. **Gesture priority:** text selection → region drag → element click.
3. **Flow:** gesture → float (optional) → composer → notes rail → **Submit review**.
4. **Annotatable surface:** Rust-owned document root only. Chrome, settings,
   float, markers, and rail are non-annotatable.
5. **Markers:** numbered speech-style marks while notes pending; re-measure on resize.
6. **Esc ladder:** composer → float → exit Comment mode. Notes may stay queued
   (count badge). The **notes rail appears only while Comment mode is on**.
7. **Keyboard:** `C` toggles Comment **only inside an open present session
   window**. Ignore when focus is editable. `⌘/Ctrl+Enter` saves note body.
8. **Submit** posts the session review API. Each note carries an `excerpt`
   (visible quote, element contents, or text inside a region, plus an optional
   JPEG crop) so the harness can see what was marked; selectors still re-anchor.
   Any harness consumes via `codeflow present feedback` (Claude Code, Codex,
   Grok CLI, …).
9. **Openable handoff:** when reporting a session, lead with the owner-private
   bootstrap path / file URL CodeFlow printed—not a scavenger hunt of ports.

### Feedback → harness

```text
Submit review
  → session store (FeedbackEnvelope)
  → codeflow present feedback <session-id> [--follow]
  → harness includes envelope in active turn
  → codeflow present resolve … addressed|dismissed
```

Delivery is at-least-once by `event_id`. No harness-specific transport.

### Runtime mapping

| Concern | Owner |
|---------|--------|
| Document column | Rust `#cf-present-document` |
| Comment chrome / rail / markers | Preact `#cf-present-chrome` |
| Appearance | Session chrome + utility themes |
| Submit / feedback | Review API + `codeflow present feedback` |

---

## 4. Portal profile (`cf-docs-portal`)

- Same craft tokens and altitude grammar overlaid on the docs shell.
- Layers: **concept / architecture / technical** from **source-in-place** repository docs.
- Agents maintain repository sources; portal is derived (adapter + evidence).
- **No** session Comment lifecycle, verdicts, or notes rail.
- Architecture pages: full-width stages with margins—not caption micro-boxes.
- Verification: locked build, evidence verifier, `browser:verify`, both themes ×
  light/dark, WCAG 2.2 AA journeys per skill operations.

---

## 5. What agents generate

| Generate | Do not generate |
|----------|-----------------|
| Content into catalog blocks / portal sources | Custom CSS, remote fonts, component kits |
| One primary visual form for the governing idea | Decorative card grids of the same prose |
| Stable block IDs across present revisions | Guessed re-anchors for moved regions |
| Bootstrap link + session id on present open | Port-only “open localhost” instructions |

Self-check against this resource is mandatory. Profile details continue in each
skill’s `references/visual-craft.md` and (for present)
`references/document-authoring.md`.
