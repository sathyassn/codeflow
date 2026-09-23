---
id: ADR-0068
title: "Figure grammar is the default form of a utility figure"
status: accepted
date: 2026-09-22
supersedes: []
superseded_by: []
architecture_impact: "docs/architecture/utility-presentation.md: the figure grammar joins the shared doctrine as the normative figure text; the declaration schema is a new interface"
---

# ADR-0068: Figure grammar is the default form of a utility figure

## Context

ADR-0053 settled the utility presentation design system and ADR-0063 made one
shared resource its normative text. Both name a stage grammar as the default
visual form: a full-width labelled flow with nodes and named edges. The
portal renders it from a `cf-stage` fence and present from an authored `html`
block. In practice the portal explained with text in boxes (a row of cards for
Concept, a table for Architecture, prose for Technical), and neither skill
could ask for anything else, because the stage was the only figure the
doctrine named.

The TSK-014 design boards under `docs/verification/tsk-014-w5/` had already
shown, on this repository's own facts, what a figure that explains looks like:
extent along an axis, travel that stops, coverage as a grid of distinct marks,
layers whose bar length is their cadence with a spine crossing them. The board
also recorded, with a verifier that could fail, the defects a figure doctrine
must name: two states told apart by hue alone, a cross under the mark floor
inside a cell that passed, a label printed 13 px into another column with
nothing overflowing, a narrow band 6279 px tall, and an internally consistent
spine that stated the repository's order incorrectly. The round 2 design
(EPC-016, `design-r2`) then drew six such figures with one mark vocabulary and
one token set, reviewed by the design primary in three skins, two modes and
two widths.

The stage grammar is one flow-family form. Making it the default meant every
relationship was drawn as a flow or not drawn.

## Decision

The default form of a figure on both utility surfaces is a family figure
under the figure grammar, `resources/figure-grammar.md`, byte-identical in
`cf-present` and `cf-docs-portal` and pointed to by the shared doctrine. The
grammar states:

- nine families keyed to the relationship they encode: flow, structure,
  layering, sequence, state, coverage, extent, derivation, graph, each with
  its geometry, its non-colour state channels and the reader question it
  answers;
- twelve rules every figure obeys: one governing idea, every mark keyed, two
  channels never hue alone and both measured off the render, legible at
  render size with the floor measured on every inner mark, narrow recomposes
  with its own mark set rather than elongates, declared facts derived from
  source, no text boxes as the primary form, no text over text or over a
  mark, a one-sentence caption, token-only colour, a description, a table
  twin;
- the altitude contract: figures lead at every altitude and in every how-to
  section; Concept owns what the subject is, who it is for and what it is
  not; an ordinary how-to section carries a sequence, state or extent figure;
  prose around a figure is short and plain under ADR-0067.

The worked specimens, one drawn figure per family, sit in a companion,
`resources/figure-grammar-specimens.md`, byte-identical in both skills and
loaded when authoring a figure, so the doctrine an agent always loads stays
small.

Thresholds are declared once by the project, never by a figure: the text
floor by the token sheet (micro 12.5 px); the mark floor (9 px on the
information-bearing dimension of the inner mark), the narrow break (646 px
container), the elongation ceiling (1.5 times the wide height) and the
clearance (8 px, collision depth 1 px) by the grammar module; a per-figure
elongation ceiling with its reason by the portal configuration.

The stage grammar stops being the default form. `cf-stage` fences and `text`
figures stay renderable carriers: `cf-stage` is the flow family's interim
portal form until the figure block lands, and `text` fences carry the other
families in the portal until then and in chat always. ADR-0053 and ADR-0063
stay accepted and unedited; this record narrows the form they name and does
not supersede either, because each still governs its other decisions (the
runtimes, the doctrine's home).

### Figure declaration schema

The declaration is a new interface. It is the contract the runtime draws from
in the derived binding and the contract the verifier checks in both bindings.
`figure-grammar.md` carries the field reference; this is the decision of
record.

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | stable kebab-case id, unique on its page or document |
| `family` | yes | one of the nine family names |
| `binding` | yes | `authored` (SVG supplied) or `derived` (drawn from `source`) |
| `question` | yes | the reader question the figure answers |
| `idea` | yes | one sentence, the answer to the masked-title test |
| `caption` | yes | one sentence below the figure |
| `states[]` | yes | every drawn state: `name` (the `data-state` value and legend key), `means`, `channels` (two non-hue channels) |
| `facts[]` | yes | every fact the figure asserts: `claim`, `source` (repository path with an optional anchor), `derive` (how the verifier re-derives it) |
| `narrow` | yes | `recompose` (rotate, stack, strip, list), `drops`, `marks` (`same` or the narrow mark names), optional `elongation_max` with `reason` |
| `twin` | yes | `inline` or `derived` |
| `source` | derived only | `path` and `select`; forbidden in the authored binding |
| `description` | no | replaces the description generated from `idea`, `states` and `facts` |

A verifier rejects a declaration whose `states` do not equal the drawn
`data-state` set, the legend keys and the description; whose `facts` do not
re-derive from `source`; or whose `binding` and `source` disagree.

## Consequences

- Authors choose a family before they draw, and a relationship that fits no
  family is carried by a table or a sentence, not by a decorated box.
- Every figure carries a declaration, so the gate that the figure runtime
  adds can fail on an unkeyed mark, a hue-only pair, an inner mark under the
  floor, an overprint, an elongated narrow render, a literal colour or a fact
  that disagrees with its source. Until that runtime lands, the rules are
  judged by inspection and by the evaluation kit.
- The portal's carriers do not change with this record. Raw HTML is still
  escaped, so a portal source authors the declaration beside a `cf-stage` or
  `text` fence until the figure block exists; the shared doctrine says so.
- The chat rule follows: the lifecycle reference names the same families for
  ASCII figures, drawn only when a relationship carries the point.
- Both skill copies, their mirrors and the manifest hashes move together;
  the parity tests fail the build on drift, as for the shared doctrine.

## Alternatives considered

### Keep the stage as the default and add families as exceptions

Rejected. The stage would stay the path of least resistance, and the board
showed that a flow drawn where the relationship is coverage or extent
miscommunicates rather than communicates weakly.

### Adopt an external diagram syntax as the figure form

Rejected. Mermaid is unsupported in the portal and supporting-only in present
(ADR-0063), and no syntax carries the two-channel, inner-mark, overprint and
fidelity rules. The grammar is a set of rules over inline SVG drawn with the
utility tokens, which both runtimes already render.

### Put the rules in the kit's CSS and README only

Rejected. The kit ships classes and values; the doctrine must say which
family answers which reader question and what a verifier may fail. The kit
states the written rules; it does not own them.

## Architecture impact

`docs/architecture/utility-presentation.md` records the figure grammar as the
normative figure text beside the shared doctrine and the declaration schema as
a new interface. The runtimes, the token contract and the page classes are
unchanged by this record.
