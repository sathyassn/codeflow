# `cf-docs-portal` visual craft (utility presentation system)

Load this when configuring portal themes, authoring layered docs pages, or
reviewing portal visual fidelity. The portal is a **durable repository guide**
under the same utility presentation system as `cf-present`, with a different
job: source-linked docs, not session Comment.

Normative intent:
[`docs/verification/utility-presentation-system-design-intent-2026-08-07.md`](../../../../docs/verification/utility-presentation-system-design-intent-2026-08-07.md).
Stack: ADR-0048 (Starlight + Pagefind), ADR-0053 (shared utility craft).

`cf-design` remains product-generic. This file is **utility portal** doctrine
only. Product brand must not absorb portal fallback themes, and portal craft
must not drive product UI.

## 1. Same craft, different shell

| Shared with present | Portal-only |
|---------------------|-------------|
| Semantic tokens (ink, surface, line, proof/warn, …) | Left nav, crumbs, search, source pins |
| Type roles (display, prose, label, mono) | Starlight layout + Pagefind |
| Altitude: concept → architecture → technical | Layers over **source-in-place** docs |
| Stage / diagram / diff conventions | Evidence manifest, Markdown twins, `llms.txt` |
| Light / dark (and system where configured) | No session Comment rail |

Do not reinvent present’s ephemeral chrome inside portal pages.

## 2. Altitude on durable pages

Expose progressive depth where sources support it:

```text
purpose and mental model          (concept)
  → capabilities and journeys
    → architecture, decisions, work   (architecture)
      → technical references, evidence  (technical)
```

Each layer should be complete for its audience—not a stub that only makes sense
after three more clicks of the same prose.

## 3. Type, themes, UI

- Use bundled utility themes (e.g. **signal**, **folio**) as fallbacks; each
  supports light and dark. Configure via `portal.config.json`—do not paste
  product fonts or marketing kits into the generator.
- Prefer plain language, descriptive titles, scannable bullets.
- Visuals only when they clarify relationship, hierarchy, state, or flow.
  Text in decorated boxes is not a visual explanation.
- Architecture pages: full-width stages with margins; engineer-legible nodes
  and edges—not caption micro-boxes.
- Code and records: mono evidence roles; provenance/freshness when real.

## 4. Motion

- Portal motion is minimal (nav, theme preference). Prefer none for content.
- Never encode unique meaning only in transition or animation.
- Respect reduced motion and no incorrect-mode flash on first paint.

## 5. Anti-patterns

- Portal as a vision / marketing site
- Second content authority (prose only in portal, not in repo sources)
- Product design-system components forced into Starlight
- Utility theme fed back into the product’s brand
- Prose-in-pretty-boxes as architecture
- Comment / review lifecycle copied from present (not required)

## 6. Verification (visual)

When craft or theme changes, verify at least:

- both themes × light/dark (and system preference path)
- contrast, focus, keyboard order, reduced motion
- layered journeys and overflow on narrow widths
- `npm run browser:verify` and `codeflow validate --portal …` per skill ops

Judgment: portal still reads as **docs**, not a campaign site; structure
survives sentence removal on architecture pages.
