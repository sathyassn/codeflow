# Explanation method (shared skill resource)

**Status:** normative for every explanation a `cf-docs-portal` page or a
`cf-present` document carries, and the way to think before a chat reply, a
README, a pull request body or an ADR is drawn or written. This file is
byte-identical in both skills and is item 1 of each skill's load list. The
shared craft is `utility-presentation-system.md`; the figure doctrine is
`figure-grammar.md`.

Work through five stages in order. Each stage answers one question and
produces one output.

| Stage | Question | Output |
|---|---|---|
| 1. Reader and question | who reads this, and what one question must they answer afterwards | one line per altitude: reader, question |
| 2. Altitude | which altitudes this deliverable carries, in what order | the altitudes |
| 3. Carrier | what relationship the reader must see first, and in which medium | a family and a medium, or none |
| 4. Draft | declaration, lead, figure, caption, acting text, twin | the draft |
| 5. Check | masked title, removal, two channels, sources, policy characters, copy | pass, or back to stage 3 |

The stage outputs are the author's working notes. Only the draft and its
figure declaration are artifacts, and nothing asks a page or a document to
record the stages.

The method never rewrites a source it must leave alone (an accepted record, a
source an agent loads into its context, governance text): such a source
renders as it is, declared `illustrated` with a companion figure bound in
configuration or `pass-through` when there is nothing to draw, as the
doctrine's "Page classes in configuration" defines them.

## 1. Reader and question

Name the reader of each altitude the deliverable carries, and the one
question that reader must be able to answer afterwards.

| Altitude or form | Reader | Question they must answer |
|---|---|---|
| Concept | someone deciding whether the subject is for them | what is it, who is it for, and what is it not |
| Architecture | an engineer who will change or integrate it | how do the parts relate, and where are the boundaries |
| Technical | an operator or a reviewer | what exactly holds, in what order, and how far |
| How-to section | someone doing the task now | what do I do, in what order, and what tells me it worked |
| Reply | the operator who asked | what is the answer, and what do I decide or do next |

A pull request body takes the reply row's reader, and an ADR takes the
Architecture row's. When the question does not fit on one line, the subject
is not ready to explain yet.

## 2. Altitude

Choose the altitudes the deliverable carries and their order.

- A portal explanatory page carries Concept, Architecture and Technical in
  that order, and a how-to section wherever the reader acts.
- A present document walks the same altitudes as a path and ends with one
  ask.
- A reply, a pull request body or a README usually carries one or two
  altitudes; lead with the one its reader came for.

Each altitude answers its own question and does not repeat another's. Only
Concept says what the subject is not. The altitude contract, with the
families and the prose role per altitude, is `figure-grammar.md` section 3.

## 3. Carrier

Name the relationship the reader must see first, then choose the carrier by
that relationship and by the medium. The smallest carrier that keeps the
depth wins. Facts with no relationship between them take bullets or a table
and no figure.

| Subject | Carrier | Chat and README form | Portal and present form |
|---|---|---|---|
| travel between places: order, fork, join, stop | flow | lanes left to right, travelled and not-yet edges, a stop bar | figure block, flow family |
| parts, containment and boundaries | structure | named regions with the edges between them | figure block, structure family |
| planes over one shared axis | layering | one row per plane, a bar to its cover on the shared axis | figure block, layering family |
| ordered exchanges over time | sequence | one column per participant, time down, arrows between | figure block, sequence family |
| the states of one thing and its transitions | state | bracketed states and arrows, the governing route heavier | figure block, state family |
| one set against another | coverage | a grid of named rows and columns, one mark per cell | figure block, coverage family |
| measured reach along a scale | extent | bars to length on one scale, limit ticks | figure block, extent family |
| a product made from sources, and what checks it | derivation | sources, transform and product left to right, the check below | figure block, derivation family |
| dependencies among peers with no single order | graph | ranked nodes, the critical path heavier | figure block, graph family |
| a surface as it is | screenshot | chat: the committed file's path; README: the image with its alt text | the committed image: portal media, a present `media` block |
| facts to look up, with no relationship | table | a Markdown table | a Markdown table, a present `table` block |
| a distribution, or a series over time | no family draws it today | a table and one sentence naming that boundary | a table and one sentence naming that boundary |

Every family's chat form is drawn beside its SVG specimen in
`figure-grammar-specimens.md`, on the same facts.

The medium changes the form of a figure; the family stays the one the
relationship names.

- On the portal and in present, a figure is inline SVG drawn through the
  figure block from its declaration.
- In chat and in a README, a figure is the fenced ASCII chat form. An SVG
  file and a Mermaid fence are not README figures: the portal rejects SVG
  media and shows a Mermaid fence as code, and GitHub shows a `cf-stage`
  fence as code.
- A screenshot shows a surface as it is and never a relationship. Its
  capture rules are the doctrine's "Screenshots and raster images".

## 4. Draft

Draft the parts in this order, each from the notes of stages 1 to 3.

1. **Declaration:** the stage 1 question is the figure's `question`, and its
   one-sentence answer is the `idea`; states, facts with their sources, the
   narrow composition and the twin follow `figure-grammar.md` section 6. A
   chat form has no declaration file, so its idea, states and facts stay in
   the notes.
2. **Lead:** one sentence above the figure saying what the reader is looking
   at.
3. **Figure:** drawn in the stage 3 family, one idea, every mark keyed.
4. **Caption:** one sentence saying what the reader takes from the figure;
   it never repeats the title.
5. **Acting text:** the sentences, steps or bullets below the figure that
   the reader acts on.
6. **Twin:** the same facts as a table, inline or derived from the
   declaration.

A reply that carries no figure drafts only the lead and the acting text.

## 5. Check

Run every check on the draft. A failed check sends the draft back to stage
3, since it most often means the carrier is wrong.

| Check | Passes when |
|---|---|
| Masked title | with kicker, title, caption and legend masked, an observer names the idea in five seconds and decodes every keyed pair |
| Removal | with every sentence removed, the carrier still states the relationship |
| Two channels | every pair of states differs on two channels that are not hue, in light and in dark |
| Sources | every fact names a repository source, and that source says it today |
| Policy characters | no em or en dash and no emoji; a chat form is printable ASCII and under 78 columns |
| Copy | the prose around the carrier is short and plain under the written content policy (ADR-0067); substantial prose passes `cf-editorial-review` |

## Worked decisions

Three decisions on this repository's facts, one per altitude.

### The portal itself

- **Altitude:** Concept.
- **Reader:** someone deciding whether the docs portal is for their
  repository.
- **Question:** what is the portal, and what is it not?
- **Relationship:** the repository's sources own every fact, the portal is
  a derived view over them, and records are pointed to as folders
  (`cf-docs-portal` skill, opening section).
- **Family:** structure, two regions and one derive edge across the
  boundary; a flow would draw travel the subject does not have.

### The four enforcement planes

- **Altitude:** Architecture.
- **Reader:** an engineer who will change a gate or add one.
- **Question:** which plane covers which moment of a change, and which one
  is the boundary?
- **Relationship:** git hooks, the session git-guard, CI and remote branch
  protection over one axis from edit to merge, with the remote plane the
  only boundary (`AGENTS.md`, "Git rules").
- **Family:** layering; a structure figure would draw four boxes and lose
  the shared axis.

### A gate's coverage of surfaces

- **Altitude:** Technical.
- **Reader:** a reviewer checking what the portal's figure gate proves.
- **Question:** which rules does the gate measure on each surface, and which
  rest on review?
- **Relationship:** `browser:verify` renders each figure at 1440 and 390 px
  in light and dark, and rules 1 and 7 are decided by the design primary's
  rendered review and the evaluation kit (`figure-grammar.md` section 2).
- **Family:** coverage, twelve rules against four surfaces with rules 1 and
  7 marked not claimed; an extent bar per rule would hide which surface is
  missing.
