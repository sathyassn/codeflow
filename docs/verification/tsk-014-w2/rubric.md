# Pre-registered comparison rubric

Written **before any candidate was rendered**. `tools/verify.mjs` asserts this
file's modification time precedes every file in `renders/`; if a later edit
inverts that order the study fails its own pre-registration check rather than
quietly reinterpreting the result.

Observer: **Codex**, who did not author these candidates. The author never
supplies the observation for gates 1–4. The operator holds final authority over
the direction.

## Decision rule

Each gate is decided by its protocol and the observed result. There is no score,
no weighting, and no aggregate number. A gate not run is recorded as *not run* —
never as a pass. Everything outside these gates is contextual judgment, graded
by materiality and non-blocking unless it is anchored to the brief, this rubric,
the accepted `DESIGN_INTENT`, an accessibility target, or observed behavior.

## Protocol gates

**G1 — Governing idea (five seconds).** The author's intended idea for each
candidate is recorded in `answer-key.md` before observation. The observer views
only the at-rest desktop light render for about five seconds and states what the
page claims. *Expected: the observer's statement matches the recorded idea in
subject and claim. A page that says nothing beyond its heading fails.*

**G2 — Task question (thirty seconds).** The observer answers the case's exact
task question from the rendered candidate, timed, without scrolling help from
the author. *Expected: correct answer, unaided, within 30 s.*

**G3 — Plain-baseline differential.** The same observer answers the same
question from `baselines/<case>/baseline.html` and from `chat.md`. *Expected:
the candidate is materially faster or materially more certain than both
baselines. A candidate that is not is recorded as failing the differential —
which is a valid, reportable outcome, not a defect to be argued away.*

**G4 — Form match.** The author names the relationship each candidate claims to
encode; the observer states the relationship the rendered page actually shows.
*Expected: sequence reads as ordered, comparison as comparable, containment as
nested, criticality as ranked, absence as absent.*

**G5 — Primary-form inventory.** Every information-bearing composition on the
page is listed and run through G4 — not only the lead composition. *Expected:
no unshaped block sitting beneath a strong lead.*

**G6 — Interchangeability.** The observer attempts to describe the candidate's
content in the sibling candidate's structure without reconstruction. *Expected:
impossible without rebuilding the encoding. If content can be poured between two
candidates unchanged, they are one candidate in two costumes and at most one
survives.*

**G7 — No-box.** Applied where a figure is the primary explanatory form. *A
payload consisting entirely of text in rectangles fails; ornamental connectors
and icons do not rescue it. It does not apply to tables, forms, lists, or any
container carrying real product semantics, and it prohibits no component.*

**G8 — Adaptation preserves information.** Mobile and dark renders are compared
against desktop and light. *Expected: the same information is present and the
same relationships remain readable. A mobile render that drops a relationship,
or a dark render that loses a distinction carried by colour alone, fails.*

## Deterministic checks (machine, not observer)

These run in `render.mjs` and `verify.mjs` and gate the study's own integrity:

- axe-core WCAG 2.0/2.1/2.2 A and AA tags, per candidate, per mode, per
  viewport. *Expected: zero violations.*
- No non-`file:` request is issued by any candidate. *Expected: zero.*
- Every candidate declares `prefers-reduced-motion` handling and a `lang`,
  `title`, `h1`, and viewport meta. *Expected: all present.*
- The P1 candidates' embedded datasets are byte-identical to each other and
  consistent with `project-management/tasks/*.md` frontmatter. *Expected: exact
  match.*
- Every authored source and render is digested into `SHA256SUMS`.

Deterministic results never substitute for G1–G8. Zero axe violations is not
evidence that a composition communicates.

## What this rubric refuses to do

It creates no house style, bans no component, and names no preferred visual
form. Familiar structures — tables, lists, panels — are correct wherever they
carry a real grouping, boundary, state, or action. The failure this rubric hunts
is substitution: a structure standing in for a subject nobody modelled.
