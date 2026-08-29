# Pre-registered comparison rubric — version 5

Version 5 governs the **W5** board. It replaces nothing. `../tsk-014-w2/rubric.md`,
`../tsk-014-w3/rubric.v2.md`, `../tsk-014-w3/rubric.v3.md` and
`../tsk-014-w4/rubric.v4.md` stay byte-identical under their own locks. W4 is not
edited by this study in any respect, including its amendments file and its
recorded inventory.

Written **before any W5 artefact was authored**, and pinned by
`rubric.v5.lock.json`. At the moment of pinning, the W5 directory held only this
file, `registry.json`, the pin tool, and seed copies that are byte-identical to
their W4 or W3 counterparts. The lock records that identity per file, by digest,
so "no W5 artefact existed yet" is a checkable statement rather than a claim.

Observer: **anyone who authored nothing here and has read no answer.** This
session authored all of it and is disqualified from every observation gate.

## The evidence boundary, stated exactly

Every finding this board answers to came from an independent answer-blind Claude
observer working over an anonymised bundle of the W4 renders. That observer
**completed exactly two turns.**

- **Turn 1** — first exposure, the twelve light-wide candidates only.
- **Turn 2** — the case and portal light-middle, light-narrow and dark-wide
  renders; contract light-narrow; contract 1, 2, 4 and 5 dark-wide; and the four
  available baselines.

Turn 2 therefore did **not** inspect contract light-middle, contract/3 dark-wide,
or any contract dark-middle or dark-narrow corner. Those corners were present in
the bundle. They were not looked at. An uninspected corner is not a corner that
passed, and this board records them as unobserved rather than as clean.

A **third turn was drafted and armed and never accepted.** It produced no
observer report, no ranking, no direction disposition and no selection. Nothing
on this board may cite it, and nothing does. There is no observer ranking of the
directions and no observer-selected direction in existence at the time this
rubric was pinned.

**This session did not read the observer's own reports.** They live in a
transcript this session's permission settings deny; reads were attempted and
refused. Every finding recorded in `registry.json` is transcribed from the
operator's enumeration supplied in the orchestration prompts, in the operator's
wording, not the observer's. Where that enumeration is silent, this board is
silent and says so. No text here paraphrases, infers or reconstructs a report
this session could not open.

The one piece of observer-adjacent evidence this session **did** establish
firsthand is the anonymised mapping: each bundle folder was matched to a real W4
artefact by SHA-256 of its renders against `../tsk-014-w4/renders/`. That method
and its result are recorded in `registry.json`; 90 of the bundle's 96 PNGs match a
W4 render exactly, and the six that do not are the p1 and p3 plain baselines,
which W4 never rendered.

## Why there is a W5

W4 was rendered, deterministically qualified, and then read across those two
observer turns. The observer returned material failures that no deterministic
gate on that board could see, including one that is not a composition defect at
all:

- **a factual error.** W4's Orient figure drew the repository's traceability
  spine as `capability + PR → ADR/spec → epic/task → ledger`. The contract
  declares `capability → epic/task → ADR/spec → PR → ledger`. Two nodes were
  merged and two transposed, because the drawing forced the spine's own order
  onto the layers' volatility order. Every A-gate passed;
- **a decoding failure.** A five-state lane glyph vocabulary produced observer
  decoding errors in *every* inspected context. The states were keyed, described
  and distinct in the markup, and still unreadable;
- **narrow bands that keep the facts and lose the job** on five separate
  surfaces, and one that answers by growing to 6279 px rather than recomposing;
- **a reuse proof with zero variance**, which demonstrated nothing;
- **a direction whose own evidence showed a ceiling**, and another whose narrow
  variant abandoned the axis that was its entire claim.

So v5 adds the gates those failures argue for, and — more importantly — v5 does
not pretend the gates are the point. Six of the findings are unreachable by any
check written here. They are recorded as the human gates they are.

## Decision rule

Each gate is decided by its protocol and the observed result. There is no score
and no aggregate. A gate not run is recorded as *not run* — never a pass. A gate
recorded as **void** is not a weak pass.

**Intent is not outcome.** A planned repair is a `target_disposition` with
`outcome: pending` until the artefact exists *and* a named check or a named
observation supports it. This rule exists because a draft of this very
registration was written with planned repairs recorded as completed dispositions
and with an uncompleted observer turn cited as evidence. Orchestration caught it
before a single W5 artefact was authored; A21 and A22 below are the standing
form of that correction.

**No production recommendation follows from mechanism promise.** A direction is
recommendable only on demonstrated reach across materially different subjects
plus an honestly exposed extension cost. A mechanism that works on one subject
has demonstrated one subject.

## A — admissibility, machine-decided

Carried unchanged from `rubric.v4.md`: **A1** answer-channel separation, **A2**
declared distinctness, **A3** a real intermediate context, **A4** carrier
honesty, **A5** motion, **A6** source binding, **A7** frame containment, **A8**
registration honesty, **A9** demonstration by its own mechanism, **A10** the
neutral shell, **A11** disposition completeness, **A12** every drawn mark keyed,
**A13** legible at the size it renders, **A14** a non-colour channel per state.

New in v5:

**A15 — two independently legible channels.** *Expected: every distinction the
registry marks `matters: true` is carried by at least two channels that are
legible independently of each other, and neither is hue alone. For every such
mark, the registry names both channels and `checks/channels.json` records the
token for each as present in the rendered markup. A mark whose smallest
information-bearing dimension is under the declared mark floor fails regardless
of how many channels it claims.*

W4 keyed five lane states, described all five, gave each a non-colour marker, and
passed A12 and A14 — and the observer still misread them in every inspected
context. A14 asks whether a channel that is not colour exists. A15 asks whether
**two** channels exist and whether either is legible on its own. That is the
difference between a mark being distinguishable in principle and being readable.

**A16 — no uninspected corner.** *Expected: every artefact is rendered and probed
at every declared width and every declared mode — the full product, not a
diagonal through it — and `checks/coverage.json` lists the complete matrix with
no gap. A context that was not rendered is not a context that passed.*

A16 governs rendering and probing, which this session can complete. It does not
and cannot deliver observation coverage: the corners the W4 observer never looked
at stay unobserved until a fresh answer-blind observer looks at the W5 board.

**A17 — a control exists, or the question is marked unscorable.** *Expected: every
case and every demonstrated page family carries a plain-chat control, a plain-
Markdown control and a plain-HTML baseline, or carries an explicit
`scorable: false` with the reason. A differential that was not run is never
inferred, and a surface with no control cannot claim to beat one.*

W4 shipped three portal families and controls for one. The observer correctly
refused to score the other two rather than guessing.

**A18 — declared facts are checked against the repository.** *Expected: where a
figure asserts an ordering, a count, a vocabulary or a membership that the
repository declares, the verifier re-derives it from the source and compares.
`checks/fidelity.json` records each assertion, its source, the derived value and
the drawn value.*

This is the gate that would have caught the spine. No composition gate can: the
drawing was internally consistent, contained, keyed and accessible, and wrong.

**A19 — a narrow band is a composition, not an elongation.** *Expected: each
artefact's narrow render is no taller than the multiple of its wide render
declared in the registry, or the registry declares why the subject genuinely
requires more and what the reader gets for it. Answering by growing is not
recomposing.*

**A20 — the reuse proof varies.** *Expected: where a direction claims reuse, it is
demonstrated on at least two materially different subjects, and the verifier
checks that the rendered instances differ — that the second binding is not the
first binding with the labels changed. A proof in which every instance is
identical demonstrates that the mechanism runs, not that it carries anything.*

**A21 — intent and outcome are separate, and outcome is earned.** *Expected: every
entry in `blind_findings`, `part_d_defects` and the direction and family records
carries a `target_disposition` and a separate `outcome` drawn from `pending`,
`achieved`, `partial`, `failed` or `withdrawn`. The verifier refuses `achieved`
unless the entry names an artefact path that exists and names the check result or
the observation that supports it, and it refuses any `outcome` other than
`pending` on a claim whose only support is a human gate this session may not run.*

**A22 — no uncompleted turn is cited, and the reading boundary is declared.**
*Expected: every external finding names a source turn; that turn appears in the
registry's `completed_turns`; a drafted or armed but unaccepted request appears
only in `not_evidence` and is cited nowhere; and the registry states in terms what
this session could read and what it could not, so a later reader can tell
transcription from transcript.*

## G — observation gates, decided by an observer who authored nothing

Carried from v4: **G1** governing idea, **G2** task question, **G3** plain-baseline
differential, **G4** form match, **G5** primary-form inventory, **G6**
interchangeability with titles and captions masked, **G7** no-box, **G8** idea
survival on two separately recorded axes, **G9** expressive reach, **G10**
extension cost.

New in v5:

**G11 — channel legibility, in context.** The observer decodes each keyed
distinction from the figure at each width and mode, without consulting the
legend, and states which pairs are confusable. A15 cannot decide this: a mark can
carry two declared channels and still be unreadable at 6 px, in dark, at the
narrow band, beside its neighbour.

**G12 — the answer is correct.** For every case candidate, the observer's answer
to the registered question is checked against `answer-key.md` **after** it is
given. A candidate that produces a *wrong* answer has not weakly communicated; it
has miscommunicated, and that outranks every other result for that candidate.

## What v5 corrects about v4, and what it cannot

**The W4 answer-key path inconsistency.** `rubric.v4.md` G1 says the intended idea
is "recorded in `answer-key.md` before observation", and W4 shipped no such file;
its `observer-state.md` and `NEXT.md` instead pointed a future observer at
`../tsk-014-w3/answer-key.md`, which is the key to a different board with
different candidates. So W4's G1 and its G12-equivalent had no admissible key at
all. `rubric.v4.md` is pinned and is **not** edited; this section is the
correction, and W5 ships `answer-key.md` at its own root with `NEXT.md` naming
that exact path.

**The pre-registration correction.** A draft of `registry.json` and of this file
stated that the observer had completed three turns, cited the drafted third turn
as the source of per-direction dispositions, and recorded fifteen planned repairs
as completed `disposition: repaired` outcomes — including a claim that complete
context coverage had already been repaired, before any render existed.
Orchestration interrupted and corrected it **before any W5 candidate artefact was
authored**, so no drawing, render or check ever rested on it. The false draft is
not preserved as evidence; what is preserved is this disclosure, the
`correction_history` block in `registry.json`, and gates A21 and A22, which make
the same mistake fail a check next time rather than depend on someone noticing.

**What v5 still cannot decide.** Six of the observer's material findings are
human gates, and no check in section A reaches them: whether a governing idea
survives a recomposition, whether two marks are confusable at the size they
render, whether a figure's payload is the figure or the prose beside it, whether
a sibling pair is one template in two costumes, whether a demonstrated reach is
the reach that matters, and whether an answer is right. Section A decides
admissibility. It has never decided design, and on the previous board it passed a
figure that stated a repository fact incorrectly.

## What this rubric refuses to do

It creates no house style, bans no component, names no preferred form, and sets
no palette, typeface or spacing scale. Where a gate needs a threshold — a
legibility floor, a mark floor, an elongation multiple, a viewport, a mode — the
threshold comes from the registry, declared once, by the project.
