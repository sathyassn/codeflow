# `cf-present` visual craft

**Required load order (do not skip):**

1. [resources/how-presentation-works.md](../resources/how-presentation-works.md)
   — **how to think**: what the human sees, why structure is the presentation,
   how to choose instruments before JSON
2. [resources/utility-presentation-system.md](../resources/utility-presentation-system.md)
   — shared craft: composition rule, page classes, carriers, tokens
3. This file — the present profile: composition gate, Comment surface,
   feedback pipeline, runtime mapping, checklist
4. [document-authoring.md](document-authoring.md) — envelope and block fields
   only after the thinking is settled
5. Prefer [resources/present-document.example.json](../resources/present-document.example.json)
   as a **shape** (figure → frame → evidence → ask), not a template to pad

If you cannot pass the self-check in *how presentation works*, **do not open**
the session.

`cf-design` stays product-generic. This skill is **utility present** only.
Author **this session's** subject. Do not clone the design-exploration board.

Display chrome (runtime-owned, same as the settled design reference): Font
(Archivo / Inter / Plex Sans), Size (Compact / Default / Large), Palette
(Neutral / Cool / Warm), Appearance (Light / Dark / System). Comment is one
mode: gesture → float → composer; rail only while armed; Esc backs out.

---

## Present profile (normative for this surface)

### Document composition gate (before `present open`)

If any fails, stay in chat or restructure; do not open a text-wall session.

1. What **job** does this surface do that chat cannot?
2. What is the **5-second picture** (no bullet recap)?
3. What is the **single primary carrier**, and is it first?
4. Does structure still argue if sentences are removed?
5. Does a top-to-bottom walk avoid "memo restyled as blocks"?
6. Catalog only; stable IDs; no secrets/paths; one clear ask?

### Comment surface (runtime-owned chrome)

Agents author **document blocks**. They do not rebuild Comment UI.

1. **Single Comment mode**: one control arms annotation; no permanent per-block `+`.
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
   bootstrap path / file URL CodeFlow printed, not a scavenger hunt of ports.

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
| Appearance | Session chrome + utility skins |
| Submit / feedback | Review API + `codeflow present feedback` |

### Present-only generation rules

| Generate | Do not generate |
|----------|-----------------|
| Stable block IDs across present revisions | Guessed re-anchors for moved regions |
| Bootstrap link + session id on present open | Port-only "open localhost" instructions |

## Thinking (non-negotiable)

- Present JSON is a **handoff**, not the product. The product is the open page.
- **Block order is attention order.** First block owns the fold.
- Choosing a block type chooses a **perceptual instrument** (see how-presentation-works).
  Prose and bullets are reading instruments. They do not become a stage by
  wishing.
- One **primary carrier** for the governing idea; everything else supports,
  proves, or asks.
- Utility themes stay quiet so subject structure can be loud. Do not paper over
  a weak figure with more words.

## Altitude as a path

1. **Concept** — the carrier that creates the 5‑second picture  
2. **Architecture** — only if another structural view is required  
3. **Technical** — verifiable panes (status, diff, code, table)  
4. **Ask** — one clear feedback prompt  

Short narrative **frames**; it does not replace the carrier.

## Motion

Meaning at rest. No dependence on animation to teach structure.

## Before open

- [ ] Walked the page top-to-bottom in plain language (how-presentation-works §3E)
- [ ] 5‑second picture named; primary carrier first
- [ ] Structure survives sentence removal
- [ ] No chat-restyle wall; no secrets/paths
- [ ] Handoff: **bootstrap link first**, then session id
