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

Work through `explanation-method.md` first: its stages settle the reader,
the altitudes and the carrier, and its check decides whether the page
opens. Then walk the page top to bottom in plain words before JSON encodes
it, with stable `id`s for blocks that persist across revisions.

A reader opens the page knowing only what they brought: the conversation,
the project and their own words for things. The page connects to that before
it shows anything new, and every later block connects to the blocks above
it. A figure dropped in with no lead makes the reader rebuild the context
and connect the dots alone; that is the failure this section prevents.

### The spine of every document

```text
 1  ANCHOR     what this is, why it matters, where it stands, what you decide
 2  PICTURE    one figure of the whole, naming every part that follows
 3  PROBLEM    the familiar problem, in the reader's own terms
 4  MODEL      the new model, one part per section, each part drawn
 5  BEHAVIOUR  how it behaves: the normal path, then what it survives
 6  DECISION   the one ask, in the terms the figures taught
```

- **Anchor.** The first block is a `narrative` block: a `## ` heading that
  names what the reader is looking at, then a Markdown list of four or five
  short lines, one fact each: what this is, named in the reader's words and
  tied to the request or issue it answers; why it matters, as the problem
  the reader already knows with one concrete instance; where it stands;
  what the reader decides at the end; and at most one more line of context.
  It defines nothing and names no term the picture has not drawn yet.
- **Picture of the whole.** The second section holds one figure that shows
  every part the document covers, each labelled with the name the later
  blocks use: its heading and lead, then the figure. It is the map: the
  reader sees the whole before any part.
- **Problem, then model, then behaviour.** Order the parts the way
  understanding grows: the familiar problem before the new model, the
  normal path before the exceptions, structure before timing. When the
  subject replaces something, the problem section can draw today beside the
  proposal.
- **One variant.** When the problem can be drawn in the reader's own words
  with no new term, its section may come before the picture of the whole,
  as the worked example below does. Every other order is anchor, picture,
  problem.
- **Decision.** End with the one decision or verdict, as a `decision` block
  whose options use the names the figures drew.

### Sections: a title and a lead wherever the topic changes

Every time the reader moves to a new part, a new question or a new level of
detail, they need the anchor again in miniature: where they are, why this
part, and how it connects to what they just saw.

- **Open each part with a section block:** a `narrative` block whose
  Markdown starts with a `## ` heading, then a lead of one or two lines. The
  heading renders as a section title and becomes the label in the page's
  section list; a block with no heading shows a cut-off sentence there
  instead. A `### ` heading opens a subsection under it.
- **Title the topic in plain words, never as a question:** "The problem in
  this project's own words", "How one follow-up lives", "What it must
  survive". Not "Will it hold?", and never a bare identifier, an acronym or
  a component name alone. `present check` warns on a title that ends in a
  question mark; a `decision`, `form` or `feedback_prompt` asks the reader
  and is exempt.
- **The lead ties the part to the whole:** which part of the picture it
  opens, or which problem from the anchor it answers, pointing back with
  `[fig:<id>]` where it helps: "[fig:whole] showed each lane's follow-ups;
  this is how one follow-up lives." It is a subtitle, never a paragraph.
- **Words before identifiers.** A figure shows a thing in words first ("a
  follow-up", "a lane"); an identifier such as `FU-050` appears only as an
  example after the reader knows what it names.
- **Introduce before use.** A term first appears drawn and labelled in a
  figure, with its legend entry, above or on the block that uses it.

A document of more than three parts groups them under two or three `## `
sections that follow the spine (for example "The problem", "The design",
"What it must survive"), with `### ` subsections, so the section list reads
as the argument.

### Visual weight

- **Every concept is a figure.** A concept, mechanism, lifecycle, ownership
  model or comparison is a figure in the grammar family its relationship
  names (`figure-grammar.md` section 1), drawn by its lessons ("Composing a
  figure"). Text is limited to the anchor, a lead of one or two lines per
  section, the caption and short bullets.
- **A narrative block is a few sentences.** At most three sentences, or a
  short list. A longer passage is a figure, a list or a table waiting to be
  drawn.
- **Tables are reference detail.** A table goes in a `disclosure` after the
  figures that teach the concept ("Record fields", "Every check"), never as
  the main carrier of a concept.
- **The governing path is in accent.** A figure draws its governing path or
  actors with the accent-bearing marks (`agent`, `done`, `human`, `merge`,
  `return`, `stop`), as `design-system/present.reference.html` does, and
  keeps grey marks for context. Named peers that are not the governing
  path take the companion tones `cat1` and `cat2` (`figure-grammar.md`
  section 4). A figure of grey boxes with text has not found its governing
  path.
- **The document title is a name.** "Coordination ledger design", "Ship or
  hold the dual-lineage boundary", with its one-line purpose in `summary`,
  which the page shows under the title.

### The several-terms paragraph and its figure rewrite

The most common failure is a paragraph that defines several terms at once.
Before, as one opening paragraph:

> This is the design for issue 272, with every operator decision folded in.
> A follow-up is a commitment that must be closed with proof. A lane is
> delegated work that outlives any one session. A work stream is a body of
> work with its own coordinator. The ledger never blocks git, records, CI or
> landing.

Three definitions and a boundary arrive before any picture, so the reader
must hold them all and draw the picture in their own head. After, the
anchor list (what it is, the ask a stopped session lost, where the design
stands, the decision asked) and then one structure figure that draws each
term where it sits. Its chat form:

```text
 +- Coordination ledger (this clone) -----------------------------+
 |  +- Work stream: release 3.2 ---+  +- Work stream: docs -----+ |
 |  | (C) coordinator              |  | (C) coordinator         | |
 |  |   | hands out                |  |   | hands out           | |
 |  |   v                          |  |   v                     | |
 |  | [lane] --owes--> <follow-up> |  | [lane]                  | |
 |  +------------------------------+  +-------------------------+ |
 +-------------------------------|--------------------------------+
                                 | closes with proof from
                                 v
                tasks, pull requests, ADRs (read only)
Legend: (C) coordinator  [lane] delegated work  <..> follow-up
Caption: A follow-up closes only with proof from the records.
```

Each term is now a mark with a legend entry, the boundary is a drawn edge
the ledger only reads across, and the verbs (`hands out`, `owes`, `closes
with proof from`) carry what the definitions said.

### The worked example: the coordination ledger presentation

The coordination ledger design review is CodeFlow's own design review, the
document this method was written from (its final revision asked the
operator to accept ADR-0086). It is not shipped with the skill, and this
section describes all of it that matters. What it does right:

- The anchor list says what it is, why it matters with real instances (a
  hand-written list that stopped after nine days, three builders resumed
  from memory), where the design stands and the decision asked, in four
  lines.
- It takes the variant above: the problem comes first, as a flow figure of
  an ask today and with the ledger, drawn in the reader's own words (your
  ask, a session that stops).
- One structure figure draws the whole: the ledger, its work streams, each
  stream's coordinator, lanes and follow-ups, and the records it reads.
- Every later part opens with a `## ` or `### ` heading and a lead of one
  or two lines, so the section list reads as the argument.
- Each part is a figure in its family: the record links as structure, one
  follow-up's life as state, a harness handover as sequence, the failures
  against the mechanisms as coverage, the plan as graph.
- Arrows carry verbs, and the figures use the subject's real numbers (the
  30 minute expiry, the four failures seen that week).
- The record fields sit in a disclosure as tables, and the page ends with
  one decision.

Its one miss is a section titled "Will it hold?". A plain title names the
topic ("What it must survive"); `present check` now warns on it.

### Cold-reader walk

Before open, read the blocks top to bottom as a reader who has seen only the
request. At each block ask: can a reader who has seen only the blocks above
this one follow it?

- Do I know why I am looking at this? Its lead ties it to the anchor or to a
  part of the picture.
- Is every term on it drawn above or drawn here?
- Can I say in one sentence what it added?

A "no" moves a block or adds a figure. It is never fixed with a paragraph.

Read the walk for its shape. "Paragraph, list, list, three cards,
paragraph" is chat restyled: choose the carriers again. "Anchor, picture of
the whole, the problem, three parts, what it survives, one decision" is
structure doing the work before any JSON exists.

### Look at the rendered page

`present check` validates the declarations and warns on balance and
writing, but a figure can still fail to draw at render time: a fact whose
check kind does not suit its source shows "The figure was not drawn" on a
page the check passed. Before the handoff, export the session or open it,
and look at every figure (`references/visual-craft.md`, "The pre-handoff
check").

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
picture. Then ship and hold drawn side by side, since they differ only in
where the canary runs. Then one explicit ask.”

Result on screen: anchor bullets, the flow figure, evidence rows with visible
state, a figure of the two options that draws their one difference, one
decision. Each section’s lead points to the part of the picture it opens.
Comment can pin the missing canary or a wrong edge on a figure.

Same facts. Different **responsibility** for structure. Only the second is a
present.

---

Catalog fields and envelope rules:
[references/document-authoring.md](../references/document-authoring.md).  
System craft and Comment lifecycle:
[utility-presentation-system.md](utility-presentation-system.md).
