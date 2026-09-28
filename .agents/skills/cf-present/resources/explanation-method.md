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
doctrine's "Page classes in configuration" defines them. An instruction an
agent loads is normally `illustrated`. A pass-through source takes the
reason that matches it: `accepted-record` for an accepted record,
`governance` for governance text, and `no-relationship`, with the design
primary's note, for any other source with nothing to draw, an agent-loaded
instruction included.

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

A pull request body takes the reply row's reader, an ADR takes the
Architecture row's, and a README takes the Concept row's. When the question does not fit on one line, the subject
is not ready to explain yet.

## 2. Altitude

Choose the altitudes the deliverable carries and their order.

- A portal explanatory page carries Concept, Architecture and Technical in
  that order, and a how-to section wherever the reader acts.
- A present document walks the same altitudes as a path, with Architecture
  only when a second structural view is needed, and ends with one ask.
- A reply, a pull request body or a README usually carries one or two
  altitudes; lead with the one its reader came for.

Each altitude answers its own question and does not repeat another's. Only
Concept says what the subject is not. The altitude contract, with the
families and the prose role per altitude, is `figure-grammar.md` section 3.

## 3. Carrier

Name the relationship the reader must see first, then choose the carrier by
that relationship and by the medium. The smallest carrier that keeps the
depth wins. Facts with no relationship between them take bullets or a table
and no figure. The family must also be one that `figure-grammar.md` section 3
lists for the altitude; when it is not, the relationship belongs at another
altitude, so go back to stage 2.

| Subject | Carrier | Plain-text chat and README form | Portal and present form |
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
- In a chat reply, the surface rule in
  `cf-method/references/workflow-lifecycle.md` picks the form; the fenced
  ASCII chat form is its plain-text form.
- In a README, a figure is the fenced ASCII chat form. An SVG file and a
  Mermaid fence are not README figures: the portal rejects SVG media and
  shows a Mermaid fence as code, and GitHub shows a `cf-stage` fence as code.
- A screenshot shows a surface as it is and never a relationship. Its
  capture rules are the doctrine's "Screenshots and raster images".

## 4. Draft

Draft only the parts the stage 3 carrier needs, in this order, each from the
notes of stages 1 to 3.

| Part | Drafted for | What it is |
|---|---|---|
| Declaration | a figure on the portal or in present | the stage 1 question is the figure's `question` and its one-sentence answer the `idea`; states, facts with their sources, the narrow composition and the twin follow `figure-grammar.md` section 6 |
| Lead | every answer that has a carrier or a summary | one sentence saying what the reader is looking at, above the carrier when there is one; a short answer takes no lead |
| Figure | a figure | drawn in the stage 3 family, one idea, every mark keyed |
| Caption | a figure or a screenshot | one sentence saying what the reader takes from it |
| Acting text | every answer | the sentences, steps or bullets the reader acts on |
| Twin | a figure on the portal or in present | the same facts as a table, inline or derived from the declaration |

The copy guide (`cf-editorial-review/references/copy-guide.md`) says how to
write the words of each part. A chat form has no declaration file or twin, so
its idea, states and facts stay in the notes. A table is its own carrier and
needs no twin; a screenshot also takes the doctrine's capture rules and alt
text. An answer with no carrier drafts only the lead and the acting text, and
a short answer drafts only the answer: it takes no lead.

## 5. Check

Run the checks that apply to the carrier. The universal checks hold for
every answer; the figure checks hold only for a figure.

| Check | Applies to | Passes when |
|---|---|---|
| Sources | every answer | every fact names a repository source, and that source says it today |
| Copy | every answer | the prose around the carrier follows the copy guide under the written content policy (ADR-0067); substantial prose passes `cf-editorial-review` |
| Policy characters | every answer | no em or en dash and no emoji; a chat form is also printable ASCII and under 78 columns |
| Masked title | a figure | with kicker, title, caption and legend masked, an observer names the idea in five seconds and decodes every keyed pair |
| Removal | a figure | with every sentence removed, the carrier still states the relationship |
| State channels | a figure | a drawn SVG: every pair of states differs on two channels that are not hue, in light and in dark; a chat form: every pair of marks differs in glyph and each is keyed in the legend line |

An answer with no figure, a table lookup included, passes on the universal
checks alone, and a failed universal check is fixed in the draft. A failed
figure check sends the draft back to stage 3, since it most often means the
carrier is wrong. A figure also passes the grammar's self-check,
`figure-grammar.md` section 8, and on the portal the figure gate measures
the rest.

## Worked decisions

Three decisions, one per altitude. The first and third use this
repository's facts. The second draws the supported architecture of a
consuming repository whose remote protection has been verified active.

### The portal itself

- **Altitude:** Concept.
- **Reader:** someone deciding whether the docs portal is for their
  repository.
- **Question:** what is the portal, and what is it not?
- **Relationship:** the repository's sources own every fact, the portal is
  a derived view over them, and records are pointed to as folders
  (`cf-docs-portal` skill, opening section).
- **Family:** structure, two regions and one derive edge across the
  boundary. Derivation is the rival and loses here: it answers where a page
  comes from and what proves it, an Architecture question under section 3.

### The four enforcement planes

- **Altitude:** Architecture.
- **Reader:** an engineer who will change a gate or add one.
- **Question:** which plane covers which moment of a change, and which one
  is the boundary?
- **Relationship:** in a consuming repository whose remote protection has
  been verified active, git hooks, the session git-guard, CI and remote
  branch protection cover one axis from edit to merge, and the remote plane
  is the only boundary (the supported architecture in `AGENTS.md`, "Git
  rules").
- **Substitute:** draw your own repository's verified enforcement state.
  CodeFlow's own repository has remote protection unavailable (its
  `AGENTS.md`, project-specific instructions), so a page about it draws the
  planes it has and says in the caption that no remote plane is armed.
- **Family:** layering; a structure figure would draw four boxes and lose
  the shared axis.

### A gate's coverage of surfaces

- **Altitude:** Technical.
- **Reader:** a reviewer checking what the portal's figure gate proves.
- **Question:** which rules does the gate measure on each surface, and which
  rest on review?
- **Relationship:** `browser:verify` renders each figure at 1440 and 390 px
  in light and dark. The grammar module measures rules 1 and 7 only in part
  (a declared family, boxed text), and the design primary's rendered review
  and the evaluation kit decide them (`figure-grammar.md` section 2).
- **Family:** coverage, twelve rules against the four gate surfaces and a
  review column, with rules 1 and 7 partial on the gate surfaces and covered
  in the review column; an extent bar per rule would hide which surface is
  missing.
