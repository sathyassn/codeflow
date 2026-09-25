# `cf-present` visual craft

**Load order:** the one list in [SKILL.md](../SKILL.md).

If the draft fails the method's check or the composition gate below, **do
not open** the session.

`cf-design` stays product-generic. This skill is **utility present** only.
Author **this session's** subject and draw figures to
`resources/figure-grammar.md`.

Display chrome (runtime-owned, as the shared doctrine states): Font (Archivo
for instrument, Inter for editorial, IBM Plex Sans for ink), Size (Compact /
Default / Large), Palette (Neutral / Cool / Warm), Appearance (Light / Dark /
System). Comment is one
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

## Thinking

`resources/explanation-method.md` owns the thinking: reader and question,
altitude, carrier, draft, check. `resources/how-presentation-works.md` owns
what the human sees and block order as attention order. The present altitude
path is the shared doctrine's "Page classes (portal) and document shapes
(present)".

## Motion

Meaning at rest. No dependence on animation to teach structure.

## Before open

- [ ] Passed the method's stage 5 check; walked the page top to bottom
- [ ] 5‑second picture named; primary carrier first
- [ ] Structure survives sentence removal
- [ ] No chat-restyle wall; no secrets/paths
- [ ] Handoff: **bootstrap link first**, then session id
