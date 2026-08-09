# `cf-present` document authoring reference

Load this reference when creating or revising a presentation document. The
canonical machine contract is
`.codeflow/schemas/present/document-v1.schema.json`; this page explains how to
choose and compose it. The runtime remains authoritative for semantic and byte
limits.

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
| connected explanation | `narrative` | short Markdown passages that need continuity |
| enumerable points | `bullets` | steps, findings, criteria, or concise options |
| exceptional emphasis | `callout` | one material note, risk, success, warning, or danger |
| peer alternatives | `comparison` | two to four genuinely comparable directions |
| choice or open question | `decision` | proposed, accepted, rejected, or open decisions |
| exact rows and columns | `table` | mappings, measurements, or evidence matrices |
| verification state | `status` | pass, fail, pending, or not-run evidence |
| source text | `code` / `diff` | inspectable code or a unified change |
| hierarchy | `tree` | ownership, composition, or repository structure |
| relationship or sequence | `diagram` | Mermaid flow, sequence, timeline, state, class, ER, or mind map |
| actual visual/audio evidence | `media` | bounded embedded PNG/JPEG/GIF/WebP/MP4/WebM/MP3/Ogg |
| secondary depth | `disclosure` | detail that should not dominate the first read |
| true peer views | `tabs` | one-at-a-time alternatives sharing the same context |
| requested response | `feedback_prompt` | the exact question or review decision sought |
| exceptional static layout | `html` | bounded inert HTML that standard blocks cannot express |

Do not add a block category merely for variety. Repeat a block when the
information warrants it, but consolidate fragments that form one thought. A
diagram needs `acc_title` and `acc_description`; media needs meaningful `alt`.
Colour is never the only carrier of state. Keep the first reading path complete
without opening disclosures or switching tabs.

Before authoring, state the question the richer surface must answer better than
ordinary chat and choose one primary carrier for it. A successful first view
lets the reader perceive the governing relationship before reading supporting
paragraphs. A sequence of headings, prose, status pills, and text cards is still
an illustrated document—not a visual explanation—when their geometry encodes
nothing. Use diagrams, trees, tables, diffs, media, or a justified bounded HTML
composition only when their position, connection, scale, state, or actual image
carries meaning. If removing the sentences leaves no useful relationship, the
surface has not earned its visual claim.

Also read [visual-craft.md](visual-craft.md): utility themes and modes, type
roles, altitude (concept → architecture → technical), stage grammar, motion
limits, Comment chrome boundaries, and anti-patterns. Author into that system;
do not invent a parallel visual language.

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

### Status and diagram

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

```json
{
  "type": "diagram",
  "id": "delivery-flow",
  "kind": "flowchart",
  "source": "flowchart LR\n  Plan --> Build --> Review --> Ship",
  "acc_title": "Delivery flow",
  "acc_description": "The approved plan proceeds through implementation and review before shipping."
}
```

### Progressive depth

`disclosure.blocks` and each `tabs[].blocks` use the same block catalog. Nest
only when the hierarchy improves comprehension; the runtime rejects excessive
depth and total blocks. Avoid tabs when readers need to compare the content
side by side.

### Sandboxed HTML

Use `html` only when the standard catalog cannot carry the required static
layout. It runs without scripts, same-origin access, forms, navigation, or
network. Never use it as a component SDK, a way around the schema, or a place
for product runtime code. Prefer a standard block over equivalent custom HTML.

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
