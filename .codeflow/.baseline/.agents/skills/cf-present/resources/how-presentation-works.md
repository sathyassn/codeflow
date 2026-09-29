# How presentation works (how to think)

**Required.** Read this before writing any present JSON.

You are not “filling a schema.” You are staging **what a human will see** when
the review surface opens. The JSON is only the handoff format. The operator
never reads it. They see a page: chrome above, document column in the middle,
optional Comment rail when armed. **Whatever structure you choose in JSON is
exactly the structure of that page**—calm utility styling, no magic upgrade.

If the JSON is a chat dump in blocks, the page is a chat dump with better type.
That is a failed present.

---

## 1. What the human actually encounters

Imagine the window after open—not the file on disk.

- **Top:** title of the review, appearance, **Comment**. Quiet. Not content.
- **Left (wide layouts):** a thin route of block labels. Those labels come from
  your block titles/ids. If every label is “thesis / shipped / notes,” the nav
  itself announces a memo, not a review of structure.
- **Centre:** a **vertical stack** of sections, top to bottom, with real space
  between them. The first screenful is almost the whole argument for many
  people. Later blocks are for those who scroll.
- **Comment (when armed):** mode strip + notes rail. Humans mark **what is on
  the page**—figures, lines of a diff, a status row—not your JSON keys.

So: **order of `blocks[]` = order of attention.** First block owns the fold.
A long narrative first means they start by reading. A structural figure first
means they start by *seeing* a relationship.

Chrome does not fix weak content. It only frames it.

The design-exploration board that settled utility craft is a **reference**, not
a document to clone. Do not reproduce its demo subject (system + present +
portal tabs, sample lineage figure, sample comments). Author **this session's**
subject into catalog blocks so the same chrome, tokens, and Comment system can
be used again. Portal work is a different skill (`cf-docs-portal`) on the same
craft.

---

## 2. Why structure is the presentation

The runtime turns each block into a **perceptual instrument**. Different
instruments teach differently. Choosing a block type is choosing **how the
claim is perceived**, not which Markdown flavour to use.

Think in jobs, not tags:

| Job for the reader | What they need to perceive | Instrument (block) | What goes wrong if you substitute prose |
|--------------------|----------------------------|--------------------|----------------------------------------|
| Grasp a relationship, flow, or split | Geometry: nodes, edges, order, fork | authored **stage** (justified `html`) / **tree** | They re-linearise your sentences and miss the shape |
| Compare peers | Side-by-side columns of equal rank | **comparison** | A bullet list collapses peers into sequence (implies ranking by order) |
| Trust evidence | Scanable states: pass / fail / pending / not-run | **status** | A paragraph “tests are mostly fine” cannot be annotated as a row |
| Inspect exact change | Monospace change surface | **diff** / **code** | Paraphrase hides the line they need to mark |
| Hold one decision | Named choice + state | **decision** | Buried ask in a closing paragraph |
| Absorb continuity | Short reading band | **narrative** | Fine *after* the figure; fatal as the only carrier of structure |
| Enumerate peer points | Equal list items | **bullets** | Everything same weight—no hierarchy, no spine |
| Signal risk / exception | One edged callout | **callout** | If everything is a callout, nothing is |
| Ask for a verdict | One clear closing demand | **feedback_prompt** | Vague “thoughts?” wastes the surface |

**Important:** `comparison` of three text cards is still **text** if the cards
only restate chat. Geometry only helps when the **difference between columns**
is the point. A figure that is a fig leaf for more sentences is still a wall,
just with a drawing on top. And when the governing claim needs a true
stage — labeled nodes, named edges, deliberate scale and margins — **author
one**: a justified `html` stage drawn with utility tokens (see the example
JSON). ASCII is a quick supporting form; it is **never the primary page
form** when geometry should teach.
Mermaid is unsupported: present refuses a `diagram` block and names its
conversion.

The system will not invent a lineage board, a confidence strip, or a stage
diagram for you. Those exist only if **you** author a carrier whose shape
encodes them (an `html` stage, tree, table or media), or you stay in chat.

---

## 3. How you should think (before any JSON)

Work **backward from the open page**, not forward from your notes.

### Step A — Name the job

In one sentence: what should the human be able to **do or decide** after this
surface that they could not do from chat? If you cannot name it, do not open
present.

### Step B — Name the 5‑second picture

With almost no reading, what should still be true?

Examples of real 5‑second pictures:

- “Two lanes only meet at settle.”
- “Ship vs hold—these two options, this one open risk.”
- “This diff is the whole dispute.”

If your honest answer is “they’ll need to read the bullets,” you do not have a
presentation yet. You have a memo.

### Step C — Choose one primary carrier

Pick the **single** instrument that makes that 5‑second picture true **without
depending on sentences**. That block goes **first** (or immediately after a
one-line frame if the figure needs a title in prose—still keep prose short).

Ask: *If I delete every sentence on the page and leave only this carrier’s
structure, is the governing idea still there?*  
If no → wrong carrier, or the idea is not ready to present.

### Step D — Support, then prove, then ask

Altitude is a **reading path**, not section labels:

1. **Concept** — primary carrier + at most a short frame of prose  
2. **Architecture / mechanism** — only if the claim needs a second structural
   view (not a second essay)  
3. **Technical** — status, diff, code, table: things someone can verify or mark  
4. **Ask** — one `feedback_prompt` that matches the job from Step A  

Do not “cover everything you know.” Present is expensive. Every block is another
band of attention. Prefer fewer, heavier instruments over many light ones.

### Step E — Mentally walk the page

Top to bottom, say out loud what the eye hits:

1. …  
2. …  
3. …  

If the walk is “paragraph, list, list, three cards, paragraph,” you have
restyled chat. Stop. Rebuild from Step B.

If the walk is “figure of the dispute → two options → evidence rows → one ask,”
you are thinking correctly—even before JSON exists.

### Step F — Only then encode

Now write JSON as a **faithful encoding** of that walk. Stable `id`s for blocks
that will persist across revisions (so comments stay meaningful). No secrets,
no paths, no performance of thoroughness.

---

## 4. Mental models that prevent stupid presents

### “Blocks are not headings”

Putting `### Architecture` inside a narrative does **not** create an
architecture view. It creates a heading in a reading band. Architecture is
when **layout** carries mechanism (stage, tree, table of responsibilities).

### “The catalog is a palette of instruments, not a form to complete”

You do not score points for using every block type. A strong present might be:
one diagram, one status, one prompt. A weak present often uses eight types and
still teaches nothing.

### “Comparison is for peers; sequence is for process”

If the truth is a pipeline, a left‑to‑right (or top‑to‑bottom) `html` **stage**
beats three columns labeled Phase 1/2/3 full of prose. If the truth is a fork
in the road, **comparison** beats a numbered list.

### “Evidence is a surface you can point at”

Comment exists so humans can pin **what they see**. Prefer carriers with
durable visual targets: a node, a row, a diff line, a column. A soup of prose
forces them to select sentences—and trains you to dump more sentences.

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

“The thing they must see is that the lanes only meet at settle. That’s a
**shape**. Put that shape first. Then ship vs hold as real peers. Then one
evidence list with the canary as not_run. Then one explicit ask.”

Result on screen: figure first (relationship), short frame sentence, two
option columns, evidence rows with visible state, one demand. Comment can pin
the missing canary or a wrong edge on the figure.

Same facts. Different **responsibility** for structure. Only the second is a
present.

---

## 6. Self-check (judgment, not ceremony)

Before `present open`:

1. Can I describe the **5‑second picture** without listing bullets?  
2. Is the **first** block the instrument that creates that picture?  
3. If sentences vanished, would the **structure** still argue?  
4. Does each block earn its place on the walk, or is it leftover chat?  
5. Does the final ask match the job—and can a human mark the page to disagree?

If any answer is weak, do not open. Restructure or stay in chat.

Catalog fields and envelope rules:
[references/document-authoring.md](../references/document-authoring.md).  
System craft and Comment lifecycle:
[utility-presentation-system.md](utility-presentation-system.md).
