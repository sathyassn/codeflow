# How presentation works (what the human sees)

**Load order:** the one list in [SKILL.md](../SKILL.md).

The way to think before any explanation is `explanation-method.md`: reader
and question, altitude, carrier, draft, check. This file adds what is
particular to present: what the human sees when the surface opens, why block
order is attention order, and a worked contrast.

You are not “filling a schema.” You are staging **what a human will see** when
the review surface opens. The JSON is only the handoff format. The operator
never reads it. They see a page: chrome above, document column in the middle,
optional Comment rail when armed. **Whatever structure you choose in JSON is
exactly the structure of that page**: calm utility styling, no magic upgrade.

If the JSON is a chat dump in blocks, the page is a chat dump with better type.
That is a failed present.

---

## 1. What the human actually encounters

Imagine the window after open, not the file on disk.

- **Top:** title of the review, appearance, **Comment**. Quiet. Not content.
- **Left (wide layouts):** a thin route of block labels. Those labels come from
  your block titles/ids. If every label is “thesis / shipped / notes,” the nav
  itself announces a memo, not a review of structure.
- **Centre:** a **vertical stack** of sections, top to bottom, with real space
  between them. The first screenful is almost the whole argument for many
  people. Later blocks are for those who scroll.
- **Comment (when armed):** mode strip + notes rail. Humans mark **what is on
  the page** (figures, lines of a diff, a status row), not your JSON keys.

So: **order of `blocks[]` = order of attention.** The first screen owns the
fold. A long narrative first means they start by reading. A figure with no
anchor means they start by guessing what they are looking at. A few anchor
lines, then the picture of the whole, means they know why they are looking
and then *see* how it fits together (section 3).

Chrome does not fix weak content. It only frames it.

Author **this session's** subject into catalog blocks so the same chrome,
tokens, and Comment system can be used again, and draw figures to
`figure-grammar.md`. Portal work is a different skill (`cf-docs-portal`) on the
same craft.

---

## 2. Why structure is the presentation

The runtime turns each block into a **perceptual instrument**. Different
instruments teach differently. Choosing a block type is choosing **how the
claim is perceived**, not which Markdown flavour to use.

Think in jobs, not tags:

| Job for the reader | What they need to perceive | Instrument (block) | What goes wrong if you substitute prose |
|--------------------|----------------------------|--------------------|----------------------------------------|
| Grasp a relationship, flow, or split | Geometry: nodes, edges, order, fork | **figure** in one grammar family / **tree**; a flow stage (`html`) only as the flow interim | They re-linearise your sentences and miss the shape |
| Compare peers | Side-by-side columns of equal rank | **comparison** | A bullet list collapses peers into sequence (implies ranking by order) |
| Trust evidence | Scanable states: pass / fail / pending / not-run | **status** | A paragraph “tests are mostly fine” cannot be annotated as a row |
| Inspect exact change | Monospace change surface | **diff** / **code** | Paraphrase hides the line they need to mark |
| Hold one decision | Named choice + state | **decision** | Buried ask in a closing paragraph |
| Absorb continuity | Short reading band | **narrative** | Fine *after* the figure; fatal as the only carrier of structure |
| Enumerate peer points | Equal list items | **bullets** | Everything same weight: no hierarchy, no spine |
| Signal risk / exception | One edged callout | **callout** | If everything is a callout, nothing is |
| Ask for a verdict | One clear closing demand | **feedback_prompt** | Vague “thoughts?” wastes the surface |

**Important:** `comparison` of three text cards is still **text** if the cards
only restate chat. Geometry only helps when the **difference between columns**
is the point. A figure whose geometry only restates more sentences is still a
wall, just drawn. When the governing claim needs geometry,
**author a `figure` block**: a declaration in the family whose relationship the
reader must see (`figure-grammar.md`), which the runtime draws with its legend,
caption and table twin. A justified `html` stage drawn with utility tokens
remains only as the flow family's interim form (see the example JSON). ASCII is
a chat form, **never the primary page form** when geometry should teach.
Mermaid is unsupported on both surfaces: present refuses a `diagram` block and
the portal shows a Mermaid fence as code.

The system will not invent a lineage board, a confidence strip, or a stage
diagram for you. Those exist only if **you** author a carrier whose shape
encodes them (figure/tree/table/media), or you stay in chat.

---

## 3. Anchor first, then build in order

Work through `explanation-method.md` first: its stages settle the reader, the
altitudes and the carrier, and its check decides whether the page opens. Then
walk the page top to bottom in plain words before JSON encodes it, with
stable `id`s for blocks that persist across revisions.

A reader opens the page knowing only what they brought: the conversation,
the project and their own words for things. The page connects to that before
it shows anything new, and every later block connects to the blocks above
it. A figure dropped in with no lead makes the reader rebuild the context
and connect the dots alone; that is the failure this section prevents.

### The spine of every document

```text
 1  ANCHOR     what this is, why it matters, where it stands, what you decide
 2  PICTURE    one figure of the whole, naming every part that follows
 3  PARTS      one figure per part, each opening a named part of the picture
 4  PRESSURE   how it holds when things go wrong            (when it matters)
 5  ASK        the one decision, in the terms the figures taught
```

- **Anchor.** The first block is a `narrative` block: a `## ` heading that
  names what the reader is looking at, then a Markdown list of three to five
  short lines, one fact each: what this is, named in the reader's words and tied
  to the request or issue it answers; why it matters, as the problem the
  reader already knows with one concrete instance; where it stands; and what
  the reader decides at the end. It defines nothing: terms arrive in the
  picture.
- **Picture of the whole.** The second block is one figure that shows every
  part the document covers, each labelled with the name the later blocks
  use. It is the map: the reader sees the whole before any part. When the
  subject replaces something, it can show today beside the proposal.
- **Parts in order.** Each later figure opens one part of the picture. Its
  lead sentence names that part and says what the block adds, pointing back
  with `[fig:<id>]`: "[fig:whole] showed each lane's follow-ups; this is how
  one follow-up lives." Order the parts the way understanding grows: the
  familiar before the new, the normal path before the exceptions, structure
  before timing.
- **Introduce before use.** A term first appears drawn and labelled in a
  figure, with its legend entry. A sentence that defines several terms at
  once is never the introduction.
- **Pressure.** When the subject must survive failures, show them after the
  reader knows the normal path, as a coverage or state figure.
- **Ask.** End with the one decision or verdict.

### Sections: a title and a lead wherever the topic changes

Anchoring is not only for the top of the page. Every time the reader moves
to a new part, a new question or a new level of detail, they need the same
three things again, in miniature: where they are, why this part, and how it
connects to what they just saw.

- **Open each part with a section block:** a `narrative` block whose
  Markdown is a `## ` heading and one or two lead lines, then the part's
  figure. The heading renders as a section title and becomes the label in
  the page's section list; a block with no heading shows a cut-off sentence
  there instead.
- **Title the section by the question it answers, in words:** "The problem,
  in this project's own words", "How one follow-up lives", "What happens
  when a session dies". Never a bare identifier, an acronym or a component
  name alone.
- **The lead says what this part adds and ties it back:** which part of the
  picture it opens, or which problem from the anchor it answers, in one or
  two sentences. It is the subtitle, never a paragraph.
- **Words before identifiers.** A figure shows a thing in words first
  ("a follow-up", "a lane"); an identifier such as `FU-050` appears only as
  an example after the reader knows what it names.
- **Figure title and caption do different jobs:** the title names what is
  drawn ("How one follow-up lives"); the caption states the one claim the
  reader takes from it ("It closes only through proof, and a revert reopens
  it"). Arrows carry verbs (`reads`, `adopts`, `closes`), never bare lines.

A document of more than three parts groups them under two or three section
headings that follow the spine (for example "The problem", "The design",
"Will it hold"), so the section list reads as the argument.

The first screen, anchor and picture together, gives the reader the subject
in about ten seconds: five short lines and one figure. Ask what the reader
holds after that. If the honest answer is "they will need to read on to know
what this is", the anchor is missing; if it is "they know what it is but not
how it fits together", the picture is missing.

### The failure this replaces

Before: one paragraph opened the page.

> This is the design for issue 272, with every operator decision folded in.
> A follow-up is a commitment that must be closed with proof. A lane is
> delegated work that outlives any one session. A work stream is a body of
> work with its own coordinator. The ledger never blocks git, records, CI or
> landing.

Three definitions and a boundary arrive before any picture, so the reader
must hold them all and draw the picture in their own head. After: anchor
bullets (what it is, the follow-up lost at compaction yesterday, where the
design stands, the decision asked), then one structure figure drawing the
ledger with its work streams, each stream's coordinator, lanes and
follow-ups, and the records it reads. The reader learns each term by seeing
where it sits.

### Cold-reader walk

Before open, read the blocks top to bottom as a reader who has seen only the
request. At each block:

- Do I know why I am looking at this? Its lead ties it to the anchor or to a
  part of the picture.
- Is every term on it drawn above or drawn here?
- Can I say in one sentence what it added?

A "no" moves a block or adds a figure. It is never fixed with a paragraph.

### Visual weight

- Every concept, mechanism, lifecycle, ownership model or comparison is a
  figure in its grammar family. Text is the anchor, one lead sentence per
  figure, the caption, and short bullets. A narrative block is at most three
  sentences.
- A table is reference detail: it goes in a `disclosure` after the figures
  that teach the concept, never as the carrier of the concept.
- A figure draws its governing path or actors with the accent-bearing marks
  (`agent`, `done`, `human`, `merge`, `return`, `stop`), as
  `design-system/present.reference.html` does, and keeps grey marks for
  context. A figure of grey boxes with text has not found its governing path.
- Draw the mechanism, not its name. A box labelled "cache" says less than
  the path a request takes through it; show the parts the argument turns
  on (the boundary crossed, the hop added, the thing that moves) and leave
  out the rest. When comparing options, draw the difference: the edge each
  option adds or removes, side by side.
- Match the drawing to the stakes: a one-hop question is three marks; a
  change that reroutes work needs every part the decision turns on, and no
  inventory beyond that.
- Ground it in the subject: use its real names, numbers and units (the
  30 minute claim, the 22 findings, the issue it answers), never invented
  sample data.
- Title the document as a name a reader would recognise among others
  ("Review the coordination ledger design"), and put its one-line purpose
  in `summary`.
- Look at the rendered page before the handoff: `present check` validates
  the declarations, but a figure can still fail to draw at render time (a
  fact whose check kind does not suit its source shows "The figure was not
  drawn"). Export the session and look at every figure.

Read the walk for its shape. "Paragraph, list, list, three cards,
paragraph" is chat restyled: choose the carriers again. "Anchor, picture of
the whole, three parts, the failures, one ask" is structure doing the work
before any JSON exists.

---

## 4. Mental models for present blocks

### “Blocks are not headings”

Putting `### Architecture` inside a narrative does **not** create an
architecture view. It creates a heading in a reading band. Architecture is
when **layout** carries mechanism (stage, tree, table of responsibilities).

### “The catalog is a palette of instruments, not a form to complete”

You do not score points for using every block type. Present is expensive:
every block is another band of attention, so prefer fewer, heavier
instruments over many light ones. A strong present might be one figure, one
status, one prompt. A weak present often uses eight types and still teaches
nothing.

### “Comparison is for peers; sequence is for process”

If the truth is a pipeline, a left‑to‑right (or top‑to‑bottom) flow
**figure** beats three columns labeled Phase 1/2/3 full of prose. If the
truth is a fork in the road, **comparison** beats a numbered list.

### “Evidence is a surface you can point at”

Comment exists so humans can pin **what they see**. Prefer carriers with
durable visual targets: a node, a row, a diff line, a column. A soup of prose
forces them to select sentences and trains you to dump more sentences.

### “Utility craft is quiet on purpose”

Themes, type, and chrome stay calm so **subject structure** can be loud. Do
not compensate for a weak structure with louder words, emoji, or extra
callouts. Fix the carrier.

### “html is an escape hatch, not a design studio”

Sandboxed HTML is for a static layout the catalog truly cannot express. It is
not a place to rebuild a product UI or dodge thinking. Prefer a catalog
instrument when it can carry the claim.

---

## 5. Worked contrast (same facts, different thinking)

**Facts:** dual explore lanes; meet at settle; tests green; Windows canary not
run; need ship-or-hold.

### Bad thinking → bad page

“I’ll explain the situation, list what we did, compare options in text, paste
commands, ask for feedback.”

Result on screen: narrow paragraphs, long bullets, three text cards, a code
brick. Eye has nowhere to land. Comment has nothing structural to mark. Looks
“professional,” teaches slowly. **This is the usual failure mode.**

### Good thinking → good page

“First, four anchor lines: this is the ship decision for the dual-lineage
boundary, everything is green except the Windows canary, and they choose
ship or hold. Then the thing they must see, that the lanes only meet at
settle: that’s a **shape**, the picture of the whole. Then the evidence rows,
with the canary as not_run, since the canary is the open part of that
picture. Then ship vs hold as real peers. Then one explicit ask.”

Result on screen: anchor bullets, the flow figure, evidence rows with visible
state, two option columns, one demand. Each block’s lead points to the part
of the figure it opens. Comment can pin the missing canary or a wrong edge on
the figure.

Same facts. Different **responsibility** for structure. Only the second is a
present.

---

Catalog fields and envelope rules:
[references/document-authoring.md](../references/document-authoring.md).  
System craft and Comment lifecycle:
[utility-presentation-system.md](utility-presentation-system.md).
