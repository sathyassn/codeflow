# Figure grammar (shared skill resource)

**Status:** normative for every figure a `cf-present` document or a
`cf-docs-portal` page carries. This file is byte-identical in both skills.
The shared craft is `utility-presentation-system.md`; this file is the figure
doctrine it points to. The decision of record is ADR-0068; the durable
architecture home is `docs/architecture/utility-presentation.md`.

The specimens, one drawn figure per family, are in
`figure-grammar-specimens.md` beside this file, also byte-identical in both
skills. Load it when authoring a figure; this file alone is enough to choose a
family, review a figure or answer in chat.

A figure is a drawing that encodes one relationship with marks, so a reader
sees the relationship before reading about it. Text arranged in boxes is not a
figure. A figure leads every altitude panel and every ordinary how-to section;
prose around it is short and plain.

## 1. Nine families

Choose the family by the relationship the reader must see, never by the data's
shape or by habit. One figure encodes one family.

| Family | Relationship it encodes | Geometry | Non-colour channels that carry state | Reader question it answers |
|---|---|---|---|---|
| flow | travel between places: order, fork, join, stop | places as nodes on lanes, travel as edges left to right, a stop as a bar across the edge | edge width and dash (travelled, not yet, blocked); node shape (agent disc, human ring, merge diamond); stop bar | how does it get from here to there, and what stops it |
| structure | parts, containment and boundaries | regions with named edges, nested where one contains another, connectors crossing a boundary | region fill and outline weight (owner, copy); edge dash and terminal shape (copies, compares) | what are the parts, what owns what, where is the boundary |
| layering | planes over one shared axis: what each covers and where it acts | one axis of moments, one bar per plane whose length is its cover, a marker where it acts, a spine crossing the planes in order | bar length; act marker disc; end cap on the plane that is a boundary; outline weight | which plane covers what, and which one is the boundary |
| sequence | ordered exchanges between participants over time | participants as lifelines, time down, messages as arrows between lifelines | arrowhead fill and line dash (call, return); human ring at a decision; stop bar at a refusal | in what order, who asks whom, who waits |
| state | the states of one thing and its transitions | states as rounded boxes, transitions as arrows, the governing route heavier | transition head shape (arrow, open square); arc dip and width for the return route; dashed line with a cross for the forbidden route | what state is it in, and how does it move |
| coverage | one set against another: which pairs hold and how | a grid, rows and columns named, one mark per cell | five cell marks: solid, dashed outline, hatch, faint empty, box with a cross | which rules apply where, and is anything unclaimed |
| extent | measured reach along a scale | bars drawn to length on one shared scale, limit ticks, room left as a dashed run | bar length; solid used fill; dashed room; limit bar; denied bar with a cross; dashed optional outline | how far, how much room, what is over the limit |
| derivation | how a product is made from sources, and what checks it | sources at left, transform in the middle, product at right, the check drawn as a comparison across them | edge dash and head (derives, declares, compares); node shape (source pin, transform diamond, product outline, check ring) | where does this come from, and what proves it |
| graph | dependencies among peers with no single order | labelled ring nodes ranked left to right, directed arcs, the critical path heavier | arc width (critical, ordinary); arrowhead; dashed ring for a blocked node; dashed arc for optional | what depends on what, and what is on the critical path |

The families are not templates. A family names the relationship; the drawing is
composed for the subject each time. When no family fits, the point is not a
relationship, and a table or a sentence carries it.

## 2. Twelve rules

Every figure obeys all twelve. Each rule names the test that decides it and
where its threshold is declared: the token sheet (`tokens.json`), the grammar
module (the shared figure module both runtimes load), or the portal
configuration (`portal.config.json`). Thresholds are declared once, by the
project, never by the figure.

| # | Rule | Test and threshold | Declared by |
|---|---|---|---|
| 1 | One governing idea | Masked-title test: with kicker, title, caption and legend masked, an observer names the idea in five seconds and decodes every keyed pair at each width and mode; a confusable pair fails (W5 gates G6, G11) | the design primary's rendered review and the evaluation kit |
| 2 | Every mark keyed | The set of drawn `data-state` values equals the set the legend keys, the description names and the declaration lists (W5 gate A12) | grammar module |
| 3 | Two channels, never hue alone, both measured off the render | Every pair of drawn states differs on at least two of: paint kind (fill, stroke, none), stroke width by 0.5 px or more, dash ratio, shape vocabulary, overlay (cap, cross, ring). Hue is not counted. Measured from the rendered DOM in light and dark; the non-hue tokens of a state are identical in both modes (W5 gate A15) | grammar module |
| 4 | Legible at render size, the floor measured on every inner mark | Text 12.5 px or larger at both widths. Every drawn state mark 9 px or larger on its information-bearing dimension: length for a line, diameter for a disc, the cross for a crossed box, never the container around it (W5 amendment A-09, the mark-floor half of gate A15) | text floor: token sheet (micro 12.5); mark floor: grammar module |
| 5 | Narrow recomposes with its own mark set | Under a 646 px container the narrow composition shows; it declares its own mark set and what it drops, and its height is at most 1.5 times the wide height unless the declaration states why more is needed and what the reader gets (W5 gate A19) | break and ceiling: grammar module; a per-figure ceiling: portal configuration |
| 6 | Declared facts derived from source | Every `facts` entry names a repository source and a derivation; the verifier re-derives the value and compares it exactly with the drawn value (W5 gate A18) | grammar module |
| 7 | No text boxes as the primary form | A box is drawn only where its edges mean containment, a state or a boundary. Labels in boxes with nothing between them are not a figure (W5 gate G7) | the design primary's review and the evaluation kit |
| 8 | No text over text or over a mark | Oriented ink boxes: text overlapping text, or a mark it does not label, at a depth of 1 px or more fails; text keeps 8 px clear of any mark it does not label (W5 collision gate) | grammar module |
| 9 | One-sentence caption | Exactly one sentence below the figure saying what the reader takes from it; the caption never repeats the title | grammar module |
| 10 | Token-only colour | Every colour, font and rule is a `--cf-fig-*` custom property; a literal colour or any other property fails | token sheet names the roles; grammar module lints |
| 11 | A description | `<title>` and `<desc>` on the SVG; the description names every state and states every drawn fact in words | grammar module |
| 12 | A table twin | A disclosure below the caption holding the same facts as a table, inline or derived from the declaration | grammar module |

The chat form of a figure obeys the same rules where the medium allows: one
idea, every mark explained, a caption line, and no box of text.

## 3. Altitude contract

Figures lead at every altitude. Concept owns what the subject is, who it is for
and what it is not. The other altitudes answer their own question and do not
repeat Concept's.

| Altitude | Reader question | Families that answer it | Prose role |
|---|---|---|---|
| Concept | what is it, who is it for, what is it not | structure for what it is and its boundary; flow for what it does; extent for its edge, what it is not | one lead sentence above the figure; two to four plain sentences below |
| Architecture | how do the parts relate and where are the boundaries | structure, layering, derivation, graph | one lead sentence; the acting sentences below; bullets where they scan better |
| Technical | what exactly holds, in what order, and how far | sequence, state, coverage, extent | minimal; tables and evidence blocks carry the lookup |
| How-to section | what do I do, in what order, and what tells me it worked | sequence, state or extent | one lead sentence, the steps as a list, the figure between them |

Prose around a figure follows the written content policy (ADR-0067): short
plain sentences, bullets or a table where they carry facts better than a
sentence, no em or en dash, no slogans, contrast turns, rhetorical triplets,
colon reveals or paragraph walls. The lead sentence says what the reader is
looking at; it does not restate the caption. A Technical altitude carries at
most 1500 words of prose outside its tables.

## 4. Tokens and marks

Figures draw with the `--cf-fig-*` roles only. The token sheet gives each role
a value per skin and mode; a figure never sees the values.

| Role | Carries |
|---|---|
| `--cf-fig-ground` | the figure surface; the fill of a human ring and a state box |
| `--cf-fig-rule` | the frame rule, axis rules |
| `--cf-fig-line` | primary ink: ordinary edges, solid cells, human ring stroke, limit bars |
| `--cf-fig-line-mid` | secondary ink: paths not yet travelled, optional outlines, muted labels |
| `--cf-fig-line-soft` | tertiary ink: blocked and denied paths, faint cells, axis strokes, region outlines |
| `--cf-fig-fill` | layer and region fills |
| `--cf-fig-hatch` | the hatch stroke of a not-run cell |
| `--cf-fig-accent` | the governing path, agent steps, act markers, used extent, the return route |
| `--cf-fig-accent-soft` | a soft accent field behind a governing region, rarely |
| `--cf-fig-warn` | room left before a limit |
| `--cf-fig-warn-soft` | a soft warning field, rarely |
| `--cf-fig-stop` | a gate that stops, a cross, a forbidden route |
| `--cf-fig-font`, `--cf-fig-mono` | the label and evidence faces |
| `--cf-fig-na-alpha` | the stroke opacity of a not-applicable cell (0.5 light, 0.65 dark) |

The mark vocabulary. Every drawn state resolves to one row here, and every row
carries at least two channels that are not hue. The name is the `data-state`
value and the legend key.

| Mark | Token | Width | Dash | Shape | Means |
|---|---|---|---|---|---|
| done | accent | 3 | none | round caps | travel completed, the governing path |
| todo | line-mid | 2 | 6 4 | round caps | travel not yet made |
| blocked | line-soft | 1.5 | 2 4 | butt caps | a blocked or forbidden path |
| human | line stroke 2.5 on ground fill | ring | none | 9 px radius ring | a human decision |
| agent | accent fill | disc | none | 8 px radius disc | an agent step |
| merge | accent stroke 2.5 on ground fill | outline | none | diamond | an agent merge point |
| stop | stop | 3 | none | 22 px bar across the edge | a gate that stops travel |
| cross | stop | 2 | none | 12 px cross | denied, not claimed |
| warn | warn | 2 | 5 3 | butt caps | room left before a limit |
| layer | fill with line-soft edge 1.5 | bar | none | rounded rect | a plane covers this span, local |
| layer-remote | fill with line edge 2 and a solid cap | bar | none | rounded rect with end cap | a plane covers this span and is the boundary |
| act | accent fill on ground stroke 2.5 | disc | none | 8 px radius disc | the plane acts here |
| cov | line fill | cell | none | solid square | covered |
| part | line 1.4 | cell | 2.5 1.8 | outline square | partial |
| notrun | hatch 1 | cell | none | 45 degree hatch fill | not run |
| na | line-soft 1 at na-alpha | cell | none | empty square | not applicable |
| nc | line-soft box 1.4 with a stop cross | cell | none | crossed square, cross inset 6 percent | not claimed |
| state | line 1.5 on ground fill | box | none | rounded rect | a state |
| node | line 1.5 on ground fill | ring | none | 17 px radius ring with its label inside | a peer in a graph |
| trans | line 1.75 with a filled head | arrow | none | round caps | a transition, a call |
| return | accent 3 with an accent head | arrow | none | dipped arc | the return route |
| used | accent fill | bar | none | rect | used extent |
| limit | line 2 | bar | none | vertical bar | a limit |
| optional | line-mid 1.5 | outline | 6 4 | rounded rect | optional |
| denied | line-soft 1.5 with a cross | outline | 2 4 | rounded rect | not allowed |

## 5. The evidence board and its anti-patterns

`docs/verification/tsk-014-w5/` is the evidence board behind this grammar. Its
`shared/svg.js` carries the mark vocabulary the table above distils, and its
`baselines/` (p1, p2, p3, d1, d2, d3: plain chat, plain Markdown and plain HTML
for every scorable surface) are the negative controls a figure must beat. The
board is evidence, never a subject to re-render: do not clone its cases, its
portal families or its comments into a present session or a portal page.

Named anti-patterns, each observed on that board and each now a rule:

| Anti-pattern | What was observed | Rule |
|---|---|---|
| Confusable pair | two states identical on every non-hue channel, carried by colour alone, and misread in every inspected context | 3 |
| Inner mark floor miss | a cross 6.6 px wide inside a 12 px cell passed a gate that measured the cell, not the cross | 4 |
| Overprint | a label printed 13 px deep into another column passed every containment gate, because nothing overflowed | 8 |
| Elongation by reflow | a narrow band 6279 px tall passed as a composition because a ratio compared the figure with itself | 5 |
| Wrong fact | an internally consistent, keyed, accessible spine that stated the repository's order incorrectly | 6 |
| Text in boxes | labelled cards standing in for a relationship | 7 |
| Stage by default | a labelled flow reached for before the relationship was named | 1 and section 1 |

## 6. Figure declaration

Every figure carries a declaration: one JSON file the grammar module draws
from and the gate checks. The docs portal binds the file to a page in
`portal.config.json`; a present `figure` block carries the same object in its
`declaration` field. ADR-0068 records the schema; this is the field reference,
and `figure-grammar-specimens.md` shows one complete file per family.

```json
{
  "schema_version": 1,
  "figure": {
    "id": "change-states",
    "family": "state",
    "binding": "authored",
    "question": "What state is a change in, and what is the only way out of a red check?",
    "idea": "The only route out of a red check returns to editing.",
    "title": "States of a change and the one route out of a red check",
    "kicker": "Change states",
    "caption": "A red check sends the change back to editing, and no route leads from red to merged.",
    "states": [{ "name": "return", "mark": "return", "means": "The only route out of red" }],
    "facts": [{
      "claim": "a red check returns to editing",
      "source": "AGENTS.md#git-rules",
      "derive": "the sentence beginning 'When a gate blocks you, fix the cause'",
      "check": { "kind": "contains", "text": "When a gate blocks you, fix the cause" },
      "value": true
    }],
    "wide": { "width": 640, "height": 220, "draw": [{ "state": "return", "shape": "path", "d": "M 40 120 C 200 200 440 200 600 120", "head": "end" }] },
    "narrow": { "recompose": "stack", "drops": [], "marks": "same", "width": 360, "height": 300, "draw": [{ "state": "return", "shape": "path", "d": "M 40 60 C 120 260 240 260 320 60", "head": "end" }] },
    "twin": { "columns": ["From", "To", "On"], "rows": [["checks red", "editing", "fix the cause"]] }
  }
}
```

| Field | Rule |
|---|---|
| `id`, `family`, `binding` | kebab-case id unique on the page; one of the nine families; `authored` or `derived` |
| `question`, `idea`, `title`, `caption`, `kicker` | plain text, no em or en dash; the caption is one sentence ending in a full stop and never repeats the title (rule 9); `kicker` is optional |
| `description` | optional; generated from the title, states and facts when absent (rule 11) |
| `states` | every drawn state in legend order: `name` (the `data-state` value), `mark` from the vocabulary in section 4, `means` (the legend text) |
| `facts` | every fact the figure asserts (rule 6): `source` is a repository path with an optional `#anchor` naming a heading; `check.kind` is `contains` (the anchored section holds `check.text`; `value` is `true`), `count-items` (`value` is the number of list items under the anchor) or `json` (`check.select` is a dotted path into a JSON file; `value` is what it holds) |
| `wide`, `narrow` | each composition's `width`, `height` and `draw` list; a draw item is one state mark (`state`, `shape` and its geometry), one decoration (`deco`: rule, axis or tick) or one text (`text`, `x`, `y`, and `for` naming the mark ids it labels); `narrow` also declares `recompose` (rotate, stack, strip or list), `drops` and `marks` (rule 5) |
| `twin` | `"facts"` for a table of the facts, or `{ columns, rows }` (rule 12) |
| `source`, `layout` | derived binding only: `source` is `{ path, select }` into a committed JSON file, and `layout` (extent or coverage) replaces `wide` and `narrow` with rows whose values are selectors into that source |

Coordinates are user units; the frame scales them. The module refuses an
unknown key, an undeclared state, a declared state it never draws and a
narrow composition that drops a state it does not declare. The portal
re-derives every fact from its source at build time and fails a figure whose
drawn value differs; present draws facts as declared.

## 7. Chat form

In chat the family choice is the same; the medium changes the marks. Draw an
ASCII figure only when a relationship carries the point, and then draw the
family the relationship names: lanes and bars for flow, a grid for coverage,
bars to length for extent, ranked nodes and arrows for graph. One idea, every
mark explained in a legend line, one caption line, no box of text.

## 8. Self-check before publishing a figure

- Which family, and which relationship does the reader see first?
- Masked test passed: idea named, every keyed pair decoded, no confusable pair.
- Every drawn state is in `data-state`, the legend, the description and the
  declaration; each pair differs on two non-hue channels.
- Text 12.5 px or larger and every inner mark 9 px or larger at both widths.
- Narrow recomposes with its declared mark set inside the elongation ceiling.
- Every fact names its source and matches what the source says today.
- No text box carries the form; no text overprints text or a mark.
- One-sentence caption, `--cf-fig-*` only, a description, a table twin.
- The prose around it is short and plain, with no em or en dash.
