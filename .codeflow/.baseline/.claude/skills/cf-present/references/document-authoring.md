# `cf-present` document authoring reference

**Load order:** the one list in [SKILL.md](../SKILL.md).

This file is the **encoding** reference (envelope, fields, limits) and comes
last: judgment about structure is settled before it. Encode **this session's**
subject.

The machine contract is
`.codeflow/schemas/present/document-v1.schema.json`. The runtime is
authoritative for semantic and byte limits. Schema validity never means the
page is a good present.

## Envelope

Every document is a closed version-1 JSON object:

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
| choice or open question | `decision` | proposed, accepted, rejected, or open decisions |
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
an illustrated document—not a visual explanation—when their geometry encodes
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
figure with its states, facts, wide and narrow compositions and table twin. The service checks the
envelope (schema version, a family from the nine, the authored binding, a
title and a caption, at most 64 KiB); the runtime validates the rest with the
grammar module before it draws and shows the refusal in place of the figure.
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

## Converting a diagram block

The `diagram` block was removed with its Mermaid renderer, and Mermaid is
unsupported in present and in the portal. `present open` and `present update`
refuse a document that still carries one, naming the block and its
replacement. A revision stored before the removal still opens read only, with
a notice and each diagram's source to convert from.

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
`title` and the `acc_description` into its `caption` or `description`. Keep the
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
