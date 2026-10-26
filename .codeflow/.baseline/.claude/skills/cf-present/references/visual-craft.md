# `cf-present` visual craft

**Load order:** the one list in [SKILL.md](../SKILL.md).

If the draft fails the method's check or the composition gate below, **do
not open** the session.

`cf-design` stays product-generic. This skill is **utility present** only.
Author **this session's** subject and draw figures to
`resources/figure-grammar.md`.

Display chrome (runtime-owned, as the shared doctrine states): Font (Archivo,
Inter, IBM Plex Sans; Inter when unset), Size (Compact /
Default / Large), Palette (Graphite / Slate / Sage), Appearance (Light / Dark /
System). Comment is one
mode: gesture → float → composer; rail only while armed; Esc backs out.

---

## Present profile (normative for this surface)

### Document composition gate (before `present open`)

If any fails, stay in chat or restructure; do not open a text-wall session.
The method behind each question is `resources/how-presentation-works.md`
section 3.

1. What **job** does this surface do that chat cannot?
2. Does the page open with an **anchor** (three to five lines: what, why
   with one real instance, where it stands, what is decided) and then one
   **picture of the whole** naming every part that follows?
3. Do the parts follow the order understanding grows (the familiar problem,
   the new model, how it behaves, the decision), each opening with a `## `
   section whose title names the topic in plain words, never a question,
   and one or two lead lines tying it to the picture?
4. Is every concept a figure in its family, with its governing path in
   accent-bearing marks, and does structure still argue if sentences are
   removed?
5. **Cold-reader question**, at every block: can a reader who has seen only
   the blocks above this one follow it? Every term is drawn above or on it,
   and its lead says what it adds.
6. Catalog only; stable IDs; no secrets/paths; one clear ask?

### The pre-handoff check

**Count the blocks.** After the anchor, count three things:

- figures (`figure` blocks and `html` stages);
- narrative blocks longer than a section heading and its lead (at most
  three sentences each);
- tables outside a disclosure.

Every section has its figure, the second count is zero or near it, and the
third is zero. A concept-shaped document with no figure, or with narrative
as its largest part, is restructured, not opened.

**Read the check.** `codeflow present check --file <document.json>` prints
the same signals as warnings, and `present open` and `update` print them
too. None refuses a document; each is restructured before handoff.

| Rule | Warns when | The fix |
|---|---|---|
| `figure-balance` | a Concept section (block id `concept` or `concept-*`, or a `## Concept` heading) has no figure or `html` stage, or narrative passes 500 words per figure (500 words with no figure) | draw the governing relationship |
| `title` | the document title, a section heading or a block title ends in a question mark (a `decision`, `form` or `feedback_prompt` asks the reader and is exempt) | name the topic in plain words |
| `mannered` | prose carries a known mannered pattern: ceremonial framing ("it is worth noting"), self-narration ("let me walk you through"), a colon reveal ("the result:"), a contrast turn ("not just X, but Y"), stock puffery, or an em or en dash outside a numeric range | state the fact directly (`cf-editorial-review`, "Mannered prose") |

The warnings are a floor: they read words and counts, not meaning, so the
count above and the cold-reader question still apply.

**Look at the rendered page.** Export the session (`codeflow present export
<session-id> --out <path>`) or open it, and look at every figure in light
and dark before the handoff. The reason: `present check` passed documents
whose figures did not draw, because a fact whose check kind does not suit
its source fails only when the page renders ("The figure was not drawn").

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
Submit review or answer
  → session store (events.jsonl, responses.jsonl)
  → codeflow present feedback <session-id> --follow --format v2
      (one stream under the host monitor; it ends when the session closes)
  → harness takes each event into its active turn
  → codeflow present ack <session-id> <event-id>
  → reviews: codeflow present resolve … addressed|dismissed
```

Delivery is at least once by `event_id`. No harness-specific transport;
the loop is in `references/feedback-loop.md`.

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

## Before handoff

- [ ] Passed the method's stage 5 check and the cold-reader question
- [ ] Anchor lines first, then the picture of the whole; each `## ` section
      titles its topic in plain words and its lead ties it to the picture
- [ ] Every concept a figure; governing path in accent-bearing marks;
      structure survives sentence removal
- [ ] Block count passed; tables in a disclosure; no narrative over three
      sentences; no secrets/paths
- [ ] `present check` clean, or every warning restructured
- [ ] Exported or opened and looked at: every figure drew
- [ ] Handoff: **bootstrap link first**, then session id
