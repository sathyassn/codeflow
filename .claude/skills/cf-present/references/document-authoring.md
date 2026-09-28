# `cf-present` document authoring reference

**Load order:** the one list in [SKILL.md](../SKILL.md).

This file is the **encoding** reference (envelope, fields, limits) and comes
last: judgment about structure is settled before it. Encode **this session's**
subject.

The machine contract is
`.codeflow/schemas/present/document-v1.schema.json`, and
`document-v2.schema.json` for a `schema_version: 2` document (see
"Schema version 2" below). The runtime is
authoritative for semantic and byte limits. Schema validity never means the
page is a good present.

## Envelope

Every document is a closed JSON object, version 1 or 2:

```json
{
  "schema_version": 1,
  "title": "Review the account recovery plan",
  "language": "en",
  "provenance": {
    "task_id": "TSK-042",
    "spec_id": "SPC-009",
    "adr_id": "ADR-0042"
  },
  "blocks": []
}
```

`language` and each provenance ID are optional. Use provenance only when a real
durable record exists. Every block ID is unique across the whole nested tree,
starts with an alphanumeric character, and contains at most 64 ASCII letters,
digits, `.`, `_`, or `-`. Preserve an ID across revisions only when the block
still represents the same conceptual item.

## Block selection

| Information shape | Block | Use it for |
|---|---|---|
| a relationship the reader must see | `figure` | the default primary carrier: a figure-grammar declaration in one of the nine families, drawn by the runtime with its legend, caption and table twin |
| connected explanation | `narrative` | short Markdown passages that need continuity |
| enumerable points | `bullets` | steps, findings, criteria, or concise options |
| exceptional emphasis | `callout` | one material note, risk, success, warning, or danger |
| peer alternatives | `comparison` | two to four genuinely comparable directions |
| choice or open question | `decision` | in version 2, a question with 2 to 8 options the reviewer answers; in version 1, a decision with its `status` |
| typed questions | `form` | version 2 only: fields the reviewer answers on the page, stored for the agent |
| exact rows and columns | `table` | mappings, measurements, or evidence matrices |
| verification state | `status` | pass, fail, pending, or not-run evidence |
| source text | `code` / `diff` | inspectable code or a unified change |
| hierarchy | `tree` | ownership, composition, or repository structure |
| actual visual/audio evidence | `media` | bounded embedded PNG/JPEG/GIF/WebP/MP4/WebM/MP3/Ogg |
| secondary depth | `disclosure` | detail that should not dominate the first read |
| true peer views | `tabs` | one-at-a-time alternatives sharing the same context |
| requested response | `feedback_prompt` | the exact question or review decision sought |
| flow interim or exceptional static layout | `html` | bounded inert HTML/SVG: a labeled flow stage (the flow family's interim form) or a static layout standard blocks cannot express |

Do not add a block category merely for variety. Repeat a block when the
information warrants it, but consolidate fragments that form one thought. A
figure needs its caption, facts and twin; media needs meaningful `alt`.
Colour is never the only carrier of state. Keep the first reading path complete
without opening disclosures or switching tabs.

Before authoring, state the question the richer surface must answer better than
ordinary chat and choose one primary carrier for it. A successful first view
lets the reader perceive the governing relationship before reading supporting
paragraphs. A sequence of headings, prose, status pills, and text cards is still
an illustrated document, not a visual explanation, when their geometry encodes
nothing. Use figures, trees, tables, diffs, media, or a justified bounded HTML
composition only when their position, connection, scale, state, or actual image
carries meaning. If removing the sentences leaves no useful relationship, the
surface has not earned its visual claim.

## Useful shapes

### Narrative and bullets

```json
{"type":"narrative","id":"outcome","markdown":"The migration is ready for review."}
```

```json
{"type":"bullets","id":"evidence","ordered":false,"items":["Unit tests pass.","Production migration was not run."]}
```

### Comparison and decision

```json
{
  "type": "comparison",
  "id": "options",
  "columns": [
    {"title":"Option A","markdown":"Lower operational cost; slower recovery."},
    {"title":"Option B","markdown":"Faster recovery; one new managed dependency."}
  ]
}
```

```json
{"type":"decision","id":"choice","title":"Recovery strategy","status":"open","markdown":"Choose after the failure canary."}
```

That is the version 1 decision. In a version 2 document a decision is a
question the reviewer answers; see "Forms and decisions" below.

### Status

```json
{
  "type": "status",
  "id": "verification",
  "items": [
    {"label":"Unit tests","state":"pass","detail":"184 passed"},
    {"label":"Native Windows canary","state":"not_run","detail":"Owned by release qualification"}
  ]
}
```

### Figure

```json
{"type":"figure","id":"change-states","declaration":{"schema_version":1,"figure":{"id":"change-states","family":"state","binding":"authored","title":"...","caption":"...","states":[],"facts":[],"wide":{},"narrow":{},"twin":"facts"}}}
```

`declaration` is a figure declaration exactly as `figure-grammar.md` section 6
defines it, the same file the docs portal binds; the elided fields are filled
as the specimens in `figure-grammar-specimens.md` show. The complete block to
copy is the first block of `assets/review-document.example.json`: a flow
figure with its states, facts, wide and narrow compositions and table twin.
Its facts cite CodeFlow's own `AGENTS.md`; substitute your repository's
sources before you copy it. The service checks the envelope (schema version,
a family from the nine, the authored binding, a title and a caption, at most
64 KiB); the runtime validates the rest with the grammar module before it
draws and shows the refusal in place of the figure.
Present draws authored figures only, and it draws each fact as declared: only
the portal re-derives facts from their sources, so cite sources a reviewer
can check. A document draws at most 24 figure blocks.

### Progressive depth

`disclosure.blocks` and each `tabs[].blocks` use the same block catalog. Nest
only when the hierarchy improves comprehension; the runtime rejects excessive
depth and total blocks. Avoid tabs when readers need to compare the content
side by side.

### Sandboxed HTML

`html` has two sanctioned uses: a **labeled flow stage**, the flow family's
interim form until a flow figure passes the rules (utility tokens
`var(--cf-…)`, labeled nodes, named edges; the example JSON's first block), and
an exceptional static layout the standard catalog cannot express. Interactive HTML stays in the document for annotation; the
runtime scopes authored style selectors and contains its layout/paint so it
cannot style or overlay review controls. Scripts, forms, navigation, network
loads, reserved runtime identities, and top-layer controls are prohibited.
Offline exports additionally place authored HTML in a sandboxed frame.
Never use it as a component SDK, a way around the schema, or a
place for product runtime code. Prefer a standard block over equivalent custom
HTML, and never make ASCII stand in for a figure the claim deserves.

## Schema version 2: framing, references and review entities

A `schema_version: 2` document draws every figure and table with its framing,
so a reviewer can comment on one node or arrow and a reader always sees what a
figure is. A version 1 document renders as before, except that an `html`
block's `title` now shows as a visible title line.

- **Framing.** The runtime numbers figures and tables separately, in
  document order through disclosures and tabs, and draws "Figure N · title"
  above each. A `figure` block takes its title and caption from its
  declaration. An `html` block needs `title` and `caption`, and takes an
  optional `legend` (1 to 12 entries of `label` and `means`) and
  `description` (at most 2000 characters), shown in one Details disclosure.
  A `table` needs `title` and takes an optional `caption`. The document may
  carry a one-line `summary` of at most 200 characters.
- **References.** In Markdown, `[fig:<id>]` and `[table:<id>]` render as a
  link reading "Figure N" or "Table N"; each must name a block of that kind.
- **Figure marks.** Every mark an authored figure draws needs an `id`; it is
  the mark's review entity. Ids are kebab-case, at most 64 characters, never
  `none` and never starting `legend-`.
- **Stage entities.** Inside an `html` stage, name what a reviewer may target
  with a closed vocabulary; any other `data-cf-` attribute is refused.
  `data-cf-target="<id>"` makes an element an entity, and
  `data-cf-target="none"` excludes its subtree. `data-cf-group="<id>"` makes
  a group one entity, with no target, group or `data-cf-for` inside it.
  `data-cf-label` names the entity beside it, and `data-cf-for="<ids>"` marks
  text that labels entities of the same stage. Ids follow the mark id rule
  and are unique in the stage. The service labels each entity: its
  `data-cf-label`, else the text that is `data-cf-for` it, else its
  `aria-label`, else its own text, else its id.
- **Legend or migrate.** When a stage's marks encode meaning (a colour, a
  dash, a shape), give it a `legend`. When the stage shows a relationship the
  figure grammar can draw, migrate it to a `figure` block instead.
- **Keep a stage narrow.** A stage scales to the column, so one wider than
  about 600 units is unreadable on a phone. Draw it as a `figure` block,
  which reflows, or split it into narrower stages.

### Forms and decisions

A version 2 `form` asks the reviewer typed questions and a version 2
`decision` asks for one choice. The runtime draws the controls; the page and
the service check each answer against the same rules.

```json
{"type":"form","id":"retention","title":"How long should answers stay?","markdown":"Pick what fits this project.","fields":[
  {"id":"home","label":"Where answers live","kind":"choice","rationale":"optional","options":[
    {"value":"local","label":"Private local store","recommended":true},
    {"value":"repo","label":"Committed JSON Lines"}]},
  {"id":"keep-days","label":"Days to keep","kind":"integer","minimum":1,"maximum":365}],
 "required":["home"]}
```

```json
{"type":"decision","id":"scope","title":"Review scope","markdown":"Choose the scope of this package.","options":[
  {"value":"match","label":"Match review offerings","recommended":true},
  {"value":"parity","label":"Literal parity"}]}
```

- **Fields,** in the order shown, at most 32 per form: `text` (`min_length`,
  `max_length` up to 16,384, `format` one of `email`, `uri`, `date`,
  `date-time` or `multiline`), `number` (`minimum`, `maximum`), `integer`
  (safe-integer bounds), `boolean`, `choice` and `choices` (2 to 24
  `options`; `choices` takes `min_items` and `max_items`). Each field has a
  kebab-case `id` unique in the form, a `label` of 1 to 200 characters, an
  optional `description`, and `rationale` `none` (the default), `optional` or
  `required`. `required` lists field ids. A rationale has no length bound of
  its own: the 64 KiB limit on the answer request body is its only bound.
- **No defaults.** At most one option of a field is `recommended`; the page
  labels it and never preselects it. A field takes no default value.
- **Decisions.** A version 2 `decision` has `title`, `markdown`, 2 to 8
  `options` and `rationale` (default `optional`), and renders as a form with
  one choice field, `choice`. It has no `status`: a version 2 decision with
  one is refused. A document holds at most 32 forms and decisions.
- **Answers.** The reviewer submits, declines with an optional reason, or
  dismisses the question for now; each is stored against the revision shown,
  in the private local session store, never in the repository. A correction
  is stored as an amendment and the original stays. An answer is evidence of
  the operator's choice in this review, never authority to bypass a gate,
  approve a merge, widen scope or run a command your rules would stop.
- **Updating under an open question.** `codeflow present update <id> <file>
  --expected-revision N` applies only while revision N is current; otherwise
  it exits 8, writes nothing and prints the current revision. When the
  reviewer answers against an older revision, the page asks them to confirm
  the answer against the current one.

## Converting a diagram block

The `diagram` block was removed with its Mermaid renderer, and Mermaid is
unsupported in present and in the portal. `present open` and `present update`
refuse a document that still carries one, naming the block and its
replacement. A revision stored before the removal still opens read only, with
a notice and each diagram's source to convert from.

The table names the usual family for each kind. Choose by the relationship the
reader must see (`resources/explanation-method.md` stage 3), so a flowchart
that drew parts and boundaries becomes a structure figure, and one that drew
peers with no single order a graph figure.

| Former `kind` | Replacement |
|---|---|
| `flowchart` | a flow figure |
| `sequence` | a sequence figure |
| `state` | a state figure |
| `class` | a structure figure, or a `table` where no relationship must be seen |
| `entity_relationship` | a structure figure, or a `table` where no relationship must be seen |
| `mindmap` | a `tree` block |
| `timeline` | a `table`, or a sequence figure when participants exchange messages |

A figure declaration is longer than a Mermaid line, so start from a complete
one. The first block of `assets/review-document.example.json` is a flow figure
converted from a flowchart, and `resources/figure-grammar-specimens.md` draws
one specimen per family, whose complete declarations are the portal starter's
`tests/fixtures/figures/*.json`. Carry the old `acc_title` into the figure's
`title`, and the `acc_description` into its `description`, or into its
`caption` when it is one sentence that does not repeat the title. Keep the
block id only when the figure is the same conceptual item; a note anchored on
the removed diagram stays in the history as orphaned. The `cf-stage` interim is
a portal form; in present an `html` block keeps only the two uses above.

## Language and review quality

- Lead with the outcome, decision, or question; add context at the depth needed
  to assess it.
- Separate verified, inferred, pending, and not-run claims. Link evidence by
  exact identifiers or bounded text, not fabricated certainty.
- Keep titles literal and findable. Avoid cryptic labels, canned enthusiasm,
  generic filler, and a wall of repeated cards.
- Adapt terminology and depth to demonstrated context without pretending to
  know the reader personally.
- Apply the consuming project's documented voice when it exists. Utility
  language remains neutral when no project voice is established.

## Feedback and revision integrity

Text annotations bind to one block, immutable revision, exact quote, bounded
prefix/suffix context, and UTF-16 offsets. Element annotations use a
runtime-generated structural path and exact block digest. Area annotations use
bounded normalized coordinates within an exact block or the source document;
whole-document feedback uses that document coordinate space. Do not recreate
or rewrite selectors by hand. Pending markers remain visible and numbered while
notes are edited. Preserve user feedback exactly and treat orphaned annotations
as visible unresolved context, never as permission to guess a new anchor.

### What a reviewer can mark on each block

With Comment on, a reviewer marks text by selecting words, an element by
clicking a part, and an area by dragging a box (Shift and drag on words, or
Tools, Select area). Every block takes an area note. The table says which
blocks take the other two, and why a cell does not apply. The review chrome
(top bar, mode strip, chip, composer, markers and notes rail) is the only
surface that takes no note.

| Block | Text | Element | Area |
|---|---|---|---|
| `narrative` | yes: its words | yes: a heading, paragraph or list item | yes |
| `bullets` | yes: an item's words | yes: an item | yes |
| `callout` | yes: its title and words | yes: the title or a paragraph | yes |
| `comparison` | yes: a column's words | yes: a column title or item | yes |
| `decision` | yes: its title, prompt and option labels | yes: the title, the prompt or its choice | yes |
| `decision` (v1) | yes: its title and words | yes: the title or a paragraph | yes |
| `table` | yes: a cell's words | yes: a cell or header | yes |
| `status` | yes: a row's label and detail | yes: a row | yes |
| `code` | yes: its code | yes: one line | yes |
| `diff` | yes: its lines | yes: one line | yes |
| `tree` | yes: a node's label | yes: a node | yes |
| `figure` | no: its words are part labels, so selecting one names the part it labels | yes: a part (its mark id) | yes |
| `media` | yes: the caption's words, when there is a caption | yes: the image, video or audio | yes |
| `disclosure` | yes: the summary, and the words of an opened block | yes: the summary, and the parts of an opened block | yes, on what shows |
| `tabs` | yes: the tab labels, and the words of an opened tab | yes: a tab label, and the parts of an opened tab | yes, on what shows |
| `feedback_prompt` | yes: the prompt | yes: the prompt | yes |
| `html` | yes: a stage's visible words | yes: a stage entity, else the shape or element clicked | yes |
| `form` | yes: its title, prompt, field labels and option labels | yes: a field, the title or the prompt | yes |

An area over a closed disclosure or an unopened tab belongs to that block,
never to the blocks it hides.

A `decision` row is the schema_version 2 decision, which is a form; the
`decision` (v1) row is the schema_version 1 decision with a `status`, which
renders as before. Marking a form or a decision never answers it: with
Comment on, a click on a field pins a note on the field and leaves its value,
the draft and the stored answers as they were, and nothing is sent.

Revision updates change document content only. Feedback lifecycle changes
through review events. Accepted decisions are summarized to their canonical
durable home; raw history is not replayed automatically.

## Utility tokens

Optional primitive tokens are project-owned and explicitly configured through
`.codeflow/present/config.toml`. Validate their JSON against
`.codeflow/schemas/present/utility-tokens-v1.schema.json`. The closed import may
set declared colours, font-family names, reading measure, spacing scale, radius,
and an embedded PNG/WebP identity image. It cannot import CSS, paths, fonts,
classes, components, frameworks, or executable code. Product tokens influence
this utility only when the project opts in; utility defaults never influence
the product.

When `colors` is present, both light and dark modes supply the complete
`canvas`, `surface`, `text`, `accent`, and `focus` roles as opaque six-digit hex
values. Runtime validation requires text contrast of at least 4.5:1 against
canvas and surface, accent contrast of at least 4.5:1 against surface, and focus
contrast of at least 3:1 against canvas and surface. A partial or inaccessible
palette fails closed instead of silently mixing project and utility defaults.
