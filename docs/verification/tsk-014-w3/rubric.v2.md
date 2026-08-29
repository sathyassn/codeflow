# Pre-registered comparison rubric — version 2

Version 2 of the TSK-014 rendered-composition rubric. It **does not replace**
`../tsk-014-w2/rubric.md`: that file is pinned by its own lock, was the rubric
the W2 board was judged under, and stays byte-identical. This file governs the
W3 board only.

Written **before any W3 candidate was authored**, and pinned by
`rubric.v2.lock.json`. `tools/verify.mjs` compares this file's digest against
that lock and asserts its modification time precedes every file in `renders/`;
a later edit that inverts either fails the study rather than quietly
reinterpreting a result.

Observer: **anyone who authored nothing here and has read no answer.** The
author never supplies the observation for gates 1–4. The operator holds final
authority over the direction.

## What changed from version 1, and why

Version 1 produced a clean deterministic pass and an unusable comprehension
result. Five of its gates could not mean what they claimed:

| v1 gate | What went wrong | v2 |
|---|---|---|
| G2, G3 | Candidates printed material parts of the expected answer in visible prose, so the observer could read rather than decode | **A1** runs first and machine-voids G2/G3 for any candidate whose visible text states its answer |
| G6 | Sibling distinctness was discovered after both candidates were fully authored | **A2** checks it from `registry.json` before authoring, and G6 re-tests it on the renders |
| G8 | Passed when the labels survived a narrow viewport, even where the spatial idea did not | **G8** now grades idea survival separately from fact presence, and fact presence alone fails |
| — | No intermediate width was rendered, so the context where compositions first break was never seen | **G8** covers three widths, and **A3** requires the intermediate one to fall between each candidate's own declared breakpoints |
| — | Reduced-motion stills were byte-identical to the normal stills and were counted as motion evidence | **A5** proves motion absent at runtime; no reduced-motion screenshot is taken |
| — | Carrier feasibility was assessed after authoring, so a candidate could win and then turn out to be unbuildable | **A4** binds every candidate's registered carrier verdict to the real schema and generator |

## Decision rule

Each gate is decided by its protocol and the observed result. There is no score,
no weighting, and no aggregate number. A gate not run is recorded as *not run* —
never as a pass. A gate recorded as **void** is not a weak pass and not a
failure of the candidate: it means the evidence cannot speak to that question at
all, and it is reported as void.

Everything outside these gates is contextual judgment, graded by materiality and
non-blocking unless anchored to the brief, this rubric, the accepted
`DESIGN_INTENT`, an accessibility target, or observed behaviour.

## A — admissibility, machine-decided, before any observer sees the board

These run in `tools/verify.mjs`. They do not judge a composition. They decide
whether the observation gates below are capable of meaning anything.

**A1 — answer-channel separation.** Every candidate publishes its derived answer
into a non-visible machine channel (`<html data-answer>`), and the verifier
recomputes that answer from the repository rather than reading it off the page.
The *visible* text captured from every render is then checked against the
case's registered forbidden phrases and patterns. *Expected: no match. On a
match, G2 and G3 are **void** for that candidate — not weakened — and the board
is not observable until the prose is removed and the renders retaken.*

Visible prose may orient the reader, name what is already on the page, and label
an axis. It may not state the relationship the composition is being tested to
communicate. The distinction the check draws, and the one an observer applies to
anything the check cannot see: **a fact the encoding is built out of is
admissible; a sentence that performs the derivation for the reader is not.**

**A2 — declared distinctness.** Every candidate declares a primary unit, a
primary axis and an encoded relationship in `registry.json` before authoring.
*Expected: within a case, no two candidates share their primary unit and their
primary axis, and no two share all three fields. A pair that does is one
candidate in two costumes and is rejected before it is built.*

**A3 — the intermediate context is real.** Every candidate declares the two
widths at which its own composition changes and an explicit composition for each
of the three resulting bands. *Expected: the tablet render's width falls
strictly between the two declared breakpoints, and the page really declares a
media query at each. An intermediate render that shows the same composition as
the desktop one tests nothing.*

**A4 — carrier honesty.** Every candidate declares, before authoring, what would
carry it in the product's real technical contract and what is lost if nothing
can. *Expected: every referenced block exists in the live document schema, every
carrier claim is quoted verbatim from the real source that imposes it, and any
verdict other than `native` carries a stated material loss. A candidate whose
carrier was never qualified may not be selected.*

**A5 — motion.** *Expected: the registry declares no candidate encodes motion,
and every render proves it — no running animation and no non-zero transition
duration, captured under both `prefers-reduced-motion` settings. A duplicate
still of a motionless page is not motion evidence and none is taken.*

**A6 — source binding.** Every candidate renders from its case's one shared
subject source; every quotation is verbatim against the repository file that
declares it; every commit, diff line and frontmatter value matches the
repository. *Expected: exact.*

## G — observation gates, decided by a human observer who authored nothing

**G1 — Governing idea (five seconds).** The intended idea for each candidate is
recorded in `answer-key.md` before observation. The observer views only the
at-rest desktop light render for about five seconds and states what the page
claims. *Expected: the statement matches the recorded idea in subject and claim.
A page that says nothing beyond its heading fails.*

**G2 — Task question (thirty seconds).** The observer answers the case's exact
registered question from the rendered candidate, unaided. *Expected: correct,
within thirty seconds. Void if A1 failed for this candidate.*

**G3 — Plain-baseline differential.** The same observer answers the same
question from `baselines/<case>/baseline.html` and from `chat.md`. *Expected:
the candidate is materially faster or materially more certain than both. A
candidate that is not is recorded as failing the differential — a valid,
reportable outcome, not a defect to argue away. Void if A1 failed.*

**G4 — Form match.** The registry names the relationship each candidate claims
to encode; the observer states the relationship the rendered page actually
shows. *Expected: sequence reads as ordered, comparison as comparable,
containment as nested, criticality as ranked, absence as absent, magnitude as
sized.*

**G5 — Primary-form inventory.** Every information-bearing composition on the
page is listed and run through G4 — not only the lead composition. *Expected: no
unshaped block sitting beneath a strong lead.*

**G6 — Interchangeability, re-tested on the renders.** A2 asserted distinctness
from the declaration; here the observer attempts to describe one candidate's
content in its sibling's structure without reconstruction. *Expected:
impossible. A pair that passes A2 on paper and fails here is reported as a
declaration that did not survive contact with the drawing.*

**G7 — No-box.** Applied where a figure is the primary explanatory form. *A
payload consisting entirely of text in rectangles fails; ornamental connectors
and icons do not rescue it. It does not apply to tables, forms, lists, or any
container carrying real product semantics, and it prohibits no component.*

**G8 — The idea survives every context.** For each candidate the observer grades
the tablet and mobile renders against the desktop one, and the dark renders
against the light, on **two separate axes**, recorded separately:

- **fact presence** — is the same information still on the page?
- **idea survival** — is the governing relationship still readable *at rest*, in
  the composition itself?

*Expected: both. **Fact presence without idea survival is a fail**, and is the
specific failure this gate exists to catch: a narrow layout that keeps every
label while the bars all become full width, the arcs become sentences, or the
reserved space becomes an ordinary empty cell has lost the candidate. A declared
alternate composition that carries the idea differently passes; a compressed
copy of the wide one does not. For dark, a distinction carried by colour alone
fails — every state declares a non-colour channel in the registry and the
observer checks it is doing real work.*

## Deterministic checks that gate the study's own integrity

Run by `tools/render.mjs` and `tools/verify.mjs`, alongside A1–A6:

- axe-core WCAG 2.0/2.1/2.2 A and AA tags, per page, per mode, per viewport.
  *Expected: zero violations.*
- No non-`file:` request, and no `file:` request outside the canonical study
  root. *Expected: zero of each.*
- Zero console and page errors.
- Every candidate declares a `lang`, `title`, `h1` and viewport meta.
- Task-owned browser identities: all proven exited, marked root removed.
- Every authored source and render digested into `SHA256SUMS`, with two
  consecutive normal render runs byte-identical.

Deterministic results never substitute for G1–G8. Zero axe violations is not
evidence that a composition communicates, and a clean A1 means only that the
comprehension gates are *admissible* — not that they passed.

## What this rubric refuses to do

It creates no house style, bans no component, and names no preferred visual
form. Tables, lists and panels are correct wherever they carry a real grouping,
boundary, state or action. The failure it hunts is substitution: a structure
standing in for a subject nobody modelled.

It also refuses to promote convergence. Where several candidates independently
reach the same treatment, that is recorded in `registry.json` as a hypothesis
with the tests it has not yet passed. This study extracts no shared primitive
from it.
