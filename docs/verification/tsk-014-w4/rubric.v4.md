# Pre-registered comparison rubric — version 4

Version 4 governs the **W4** board. It replaces nothing. `../tsk-014-w2/rubric.md`,
`../tsk-014-w3/rubric.v2.md` and `../tsk-014-w3/rubric.v3.md` stay byte-identical
under their own locks; they are the record of how the earlier boards were judged
and why they failed.

Written **before any W4 candidate, direction demonstration or portal page
existed**, and pinned by `rubric.v4.lock.json`. `tools/verify.mjs` compares this
file's digest against that lock and asserts its modification time precedes every
authored file under `cases/`, `directions/`, `portal/`, `baselines/` and
`renders/`. What that assertion is worth is stated in
§ *What the ordering evidence is worth*, not assumed.

Observer: **anyone who authored nothing here and has read no answer.** The author
never supplies the observation for the timed gates. The operator holds final
authority over every direction on this board.

## What W4 is for, and why it is not W3 again

W3 asked one question: *which composition explains this subject best?* The board
answered a different one first. Eleven of its candidates were qualified against
the product's real technical contract, and **ten came back negative** — five
`cf-present` candidates `needs-new-block`, four portal candidates
`needs-source-model`, one `needs-generator-work`. Only `p3/b-source-region-history`
could be carried at all, and only with loss.

That is not a result about compositions. It is a result about the **carrier**:
the closed `cf-present` block catalogue cannot express the subject-native
compositions this product's own subjects want, and the portal's source graph does
not carry the facts its own pages would need to draw. Choosing a winning
composition before deciding that is choosing which drawing to fail to ship.

So W4 has **two decision altitudes**, and they are judged separately:

```text
altitude 1 — the contract          which content model should carry drawn
  (directions/)                    explanation at all, and what does each
                                   one make impossible?

altitude 2 — the composition       given a carrier, which encoding explains
  (cases/, portal/)                this subject best?
```

Altitude 1 decides first. A composition winner selected under a carrier the
product will not build is a preference, not a decision.

## Decision rule

Each gate is decided by its protocol and the observed result. There is no score,
no weighting and no aggregate number. A gate not run is recorded as *not run* —
never as a pass. A gate recorded as **void** is not a weak pass and not a failure
of the candidate: it means the evidence cannot speak to that question at all.

Everything outside these gates is contextual judgment, graded by materiality and
non-blocking unless anchored to the brief, this rubric, the accepted
`DESIGN_INTENT`, an accessibility target, or observed behaviour.

Agreement between model seats is not a gate and never substitutes for the
operator.

## A — admissibility, machine-decided, before any observer sees the board

Run by `tools/render.mjs` and `tools/verify.mjs`. They judge no composition. They
decide whether the observation gates below are capable of meaning anything.

**A1 — answer-channel separation.** Every case candidate publishes its derived
answer into a non-visible machine channel (`<html data-answer>`), and the verifier
recomputes that answer from the repository rather than reading it off the page.
The *visible* text captured from every render is then checked against the case's
registered forbidden phrases and patterns. *Expected: no match. On a match, G2 and
G3 are **void** for that candidate — not weakened.*

Visible prose may orient the reader, name what is already on the page, and label
an axis. It may not state the relationship the composition is being tested to
communicate. **A fact the encoding is built out of is admissible; a sentence that
performs the derivation for the reader is not.**

**A2 — declared distinctness.** Every candidate declares a primary unit, primary
axis and encoded relationship in `registry.json` before authoring. *Expected:
within a case, no two candidates share unit and axis, and no two share all three.*

**A3 — the intermediate context is real.** Every candidate declares the two widths
at which its own composition changes and an explicit composition for each of the
three resulting bands. *Expected: the tablet render's width falls strictly between
the two declared breakpoints, and the page really declares a media query at each.*

**A4 — carrier honesty.** Every candidate declares, before authoring, what would
carry it in the product's real technical contract and what is lost if nothing can.
*Expected: every referenced block exists in the live document schema, every carrier
claim is quoted verbatim from the source that imposes it, and any verdict other
than `native` states its material loss.*

**A5 — motion.** *Expected: the registry declares which artefacts encode motion;
every render is probed under both `prefers-reduced-motion` settings for a running
animation, a declared animation name or a non-zero transition duration, and the
result matches the declaration. A duplicate still of a motionless page is not
motion evidence and none is taken.*

**A6 — source binding.** Every candidate renders from its case's one shared subject
source; every quotation is verbatim against the repository file that declares it;
every commit, diff line and frontmatter value matches the repository.
*Expected: exact.*

**A7 — frame containment.** *Every render carries what its page contains.* A still
render has no interaction, so `overflow-x: auto` is a silent crop, not an
affordance, and a label drawn past its own `viewBox` is removed with no notice.
Per render, `checks/frame.json` records:

- every element whose `scrollWidth` exceeds `clientWidth` while computed
  `overflow-x` is `auto`, `scroll` or `hidden`, with the hidden pixel count;
- every SVG node whose `getBBox()` extends past its own `viewBox`;
- horizontal overflow of the document itself.

*Expected: none of the three, on any render.* A band that fails is inadmissible
and is recorded with the failure that made it so — `kind`, `severity`, failing
axes — never as a neutral omission.

**A8 — the registration is an account, not an assertion.** *Expected:
`registry.json` is pinned by `registry.lock.json` written before any authored
candidate file existed; it carries a `pre_registration_evidence` object stating
what can and cannot be shown from the files; and every post-registration change to
a registered field appears in `amendments` with its field, candidate, timing,
reason, and which of drawing or declaration moved first.*

**When a drawing diverges from its registration, the divergence is recorded — the
registration is never edited to match the drawing.** Editing it converts a
prediction into a description, and a description constrains nothing. A registry
claiming no amendments while an amendment is known fails this gate.

**A9 — the demonstration is produced by the mechanism it demonstrates.** New in
v4, and the gate the contract altitude rests on. Each direction under
`directions/` must render its figures through **its own declared mechanism**, from
a data or document input the direction's contract would really accept. *Expected:
for every direction, the rendered figure is emitted by that direction's own
compiler/renderer at load time from its declared input, and `checks/mechanism.json`
records the input digest, the emitting function, and the fact that the page
contains no hand-authored geometry for that figure.*

A hand-drawn picture of what a content model *would* produce is a claim about a
content model. It is the exact failure that produced this task: a board of
attractive artefacts nothing could actually build. A direction that cannot render
its own demonstration is recorded as **unbuilt**, which is not a weak pass.

**A10 — the shell does not decide the contest.** *Expected: every direction
demonstration and every portal page family loads the byte-identical neutral shell
stylesheet `shared/shell.css`, whose digest is recorded, and adds no styling to the
comparison chrome — only to the figure the direction owns. `checks/shell.json`
records the digest actually served to each page.*

The figure is the thing under test. If one direction arrives in nicer furniture,
the board measures furniture.

**A11 — every failed W3 band has a disposition.** *Expected: `dispositions.json`
carries one entry per band in `../tsk-014-w3/registry.json` → `invalidated_bands`
— fourteen — each naming the W3 render, its demonstrated failure verbatim, the
disposition (`repaired`, `replaced`, `withdrawn`), the W4 artefact that carries it
now, and what changed. A band with no entry fails this gate; so does an entry whose
`after` render does not exist.*

The W3 registry, rubrics and renders are **append-only**. Nothing here edits them.

**A12 — every information-bearing mark is keyed.** *Expected: for every figure,
the set of state tokens the page actually draws equals the set the registry
declares, equals the set the visible legend keys, and equals the set named in the
figure's accessible description. A mark the drawing distinguishes and the legend
omits is unreadable by construction.* `checks/keys.json` records all four sets and
their differences.

This gate exists because W3 drew a fifth cell state on one candidate that appeared
in neither its registry, its legend nor its `<desc>`, and A2, A4 and axe all
passed.

**A13 — legible at the size it renders.** *Expected: on every render, every text
node inside a figure has a computed font size at or above the floor the registry
declares for that surface, measured from the rendered page rather than the source.*
`checks/type.json` records the smallest computed size per render and the node that
carries it. The floor is the project's, declared once in the registry; this rubric
names no size.

**A14 — non-colour channel.** *Expected: every declared state names a channel that
is not colour — a shape, a fill pattern, a stroke style, a position, a glyph — and
that channel's token is present in the rendered markup.* Dark-mode legibility is
then a rendering question, not a meaning question.

## G — observation gates, decided by an observer who authored nothing

**G1 — Governing idea (five seconds).** The intended idea is recorded in
`answer-key.md` before observation. The observer views only the at-rest wide light
render for about five seconds and states what it claims. *Expected: matches in
subject and claim. A page that says nothing beyond its heading fails.*

**G2 — Task question (thirty seconds).** The observer answers the case's exact
registered question from the rendered candidate, unaided. *Expected: correct,
within thirty seconds. Void if A1 failed for that candidate.*

**G3 — Plain-baseline differential.** The same observer answers the same question
from `baselines/<case>/baseline.html` and from `chat.md`. *Expected: the candidate
is materially faster or more certain than both. A candidate that is not is
recorded as failing the differential — a reportable outcome, not a defect to argue
away. Void if A1 failed.*

**G4 — Form match.** The registry names the relationship each artefact claims to
encode; the observer states the relationship the rendered page actually shows.

**G5 — Primary-form inventory.** *Every* information-bearing composition on the
surface is listed and run through G4 — lead, collection, comparison, evidence
block, navigation. *Expected: no unshaped block sitting beneath a strong lead.* A
compliant figure does not excuse a card dump below it.

**G6 — Interchangeability, re-tested on the renders.** The observer attempts to
describe one candidate's content in its sibling's structure without reconstruction.
*Expected: impossible.* Run with **titles, headings and captions masked**, so the
identification comes from content-bearing structure and not from the words.

**G7 — No-box.** Applied where a figure is the primary explanatory form. A payload
consisting entirely of text in rectangles fails; ornamental connectors and icons do
not rescue it. It does not apply to tables, forms, lists, or any container carrying
real product semantics, and it prohibits no component.

**G8 — The idea survives every context.** For each artefact the observer grades the
intermediate and narrow renders against the wide one, and dark against light, on
**two separate axes recorded separately**:

- **fact presence** — is the same information still on the page?
- **idea survival** — is the governing relationship still readable *at rest*, in
  the composition itself?

*Expected: both.* **Fact presence without idea survival is a fail**, not a pass
with a note. A declared alternate composition that carries the idea differently
passes; a compressed copy of the wide one does not.

**G9 — expressive reach, at the contract altitude.** New in v4. From the
demonstrations alone — not from the prose beside them — the observer states, for
each direction: what kinds of explanation it can produce, what it cannot express,
and what a new subject form costs. *Expected: the observer's account matches the
direction's registered `reach` and `cannot_express` without having read them.*

A direction whose limits are only legible in its own description has not been
demonstrated. This gate is why every direction ships a **negative** artefact: a
real subject the direction genuinely cannot carry, rendered as far as it goes.

**G10 — extension.** The observer is shown a subject form no direction was built
for and states, per direction, what would have to change to carry it: a document,
a recipe, product code, or nothing that exists. *Expected: matches the registered
`extension_path`.*

## What the ordering evidence is worth

`rubric.v4.md` and `registry.json` are both pinned by digest, in locks written
**before** the first authored candidate file existed, and `verify.mjs` asserts that
both files' modification times precede every authored artefact and every render in
this working tree.

That assertion is worth exactly this much:

- the **digest** is durable. It survives a checkout, a copy and a commit, and it
  proves the bytes a later reader holds are the bytes that were pinned;
- the **mtime ordering** is not durable. It does not survive a fresh Git checkout,
  so a later reader cannot re-observe it. It is recorded here, in
  `registry.pre_registration_evidence`, because this file is the only witness that
  will survive;
- neither proves the author's *intent* preceded the drawing, only that these bytes
  did. Where a declaration was changed after a drawing existed, `amendments` is the
  record, and A8 fails if it is empty while a change is known.

W3's registry had no lock at all and two of its declarations were rewritten to
match drawings that had already been inspected. That is the specific failure this
section exists to make impossible to repeat quietly.

## What this rubric refuses to do

It creates no house style, bans no component, and names no preferred visual form.
Tables, lists, cards and panels are correct wherever they carry a real grouping,
boundary, state or action. The failure it hunts is **substitution**: a structure
standing in for a subject nobody modelled.

It refuses to promote convergence. Where several artefacts independently reach the
same treatment, that is recorded as a hypothesis with the tests it has not passed.
No shared visual primitive is extracted from this study.

It sets no size, palette, typeface, spacing scale or component library. Where a
gate needs a threshold — a legibility floor, a viewport, a mode — the threshold
comes from the registry, declared once, by the project.
