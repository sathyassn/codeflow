# Observer state

## The first blind observation is preserved but confounded

The eight protocol gates in `rubric.md` require an observer who has not authored
the candidates and has not seen the expected answers. A fresh native Codex CLI
session met that boundary and inspected only the registered PNGs, questions and
rubric. Its exact record is
`observations/codex-sol-high-2026-08-04.md`.

The following earlier reviews remain evidence rather than observations:

- **The author cannot observe.** Every self-assessment in this directory and in
  the TSK-014 closeout is the author's own critique. It is evidence for the
  observer to weigh, never a verdict.
- **Codex's pass-1 review was an integrity review, not a blind observation.** To
  check that the candidates matched their sources it had to read
  `answer-key.md`. That is the correct thing to have done, and it permanently
  voids the timed blind gates for pass 1 — a reader who knows the answer cannot
  produce a five-second or thirty-second reading.
- **Codex's pass-2 review was a visual integrity review, on the same footing.**
  It read the renders and raised two specific compositional defects — a drawn
  return path crossing its own explanatory text in `p3-b`, and a narrow layout
  that kept `d1-c`'s facts but lost its cross-plane relationship. Both are
  corrected in this pass. That is a finding acted on, not a gate: the same
  reviewer had already read the answer key, so it cannot supply G1–G4 either.
- **The author's own visual pass is also not an observation.** This session
  reviewed all twelve candidates at both viewports in both modes and corrected
  what it found. Every one of those judgements is the author's, made with the
  answer key in hand.
- **The pass-3 harness integrity review is not an observation either.** A second
  Claude session reviewed the study's own tooling — how a shared subject source
  is evaluated, and how everything it declares reaches the filesystem and `git`
  — and corrected what it found. Establishing that the verifier recomputes the
  right answers meant reading those answers, so that session is disqualified from
  G1–G4 on the same terms as every other. It authored no candidate and changed no
  candidate, baseline or render.

## What has been run

Deterministic checks only: axe, network isolation, render coverage, dataset and
fact-registry integrity, derived-answer agreement, rubric-lock verification, and
checksums. Their results are recorded in `checks/` and `SHA256SUMS`. The
harness's own refusal paths — bounded shared-source evaluation, single-root
source authority, and revision and pathspec qualification — are checked by
`tools/selftest.mjs`. The pass-3 Claude review ran the non-browser subset because
its sandbox denied process inventory. The coordinating host then ran all 78
assertions across A–J and two consecutive normal 70-image renders; both render
inventories were byte-identical and retained the clean lifecycle, network,
console and axe results.

A clean deterministic pass says the study is internally consistent and
accessible. It says nothing about whether a composition communicates. The
blind run separately tested whether each composition
communicates. It recovered every pre-registered material answer, while finding
candidate-specific proportionality, mobile and sibling-distinctness failures.
Those findings are not erased by an overall direction recommendation.

## What remains

The observer did not and could not test motion from stills, so no motion claim
exists. G2 records immediate versus scan-required recovery but not a stopwatch
measurement.

After that record was fixed, an independent native Claude Opus 5/high design
review inspected the sources as well as the renders. It found that multiple
candidates visibly state material parts of the registered answer in prose even
though the verifier already receives the answer through an invisible machine
channel. The observer therefore could recover those answers without decoding
the composition. G2 cannot qualify and G3 is confounded for this board version.
The review also found that still-image G8 preserved labels while several mobile
variants lost the spatial idea, and that no tablet evidence exists.

The exact design judgment and required corrections are recorded in
`observations/claude-opus-high-design-judgment-2026-08-04.md`. The blind record
is not deleted or rewritten; it remains evidence about what the rendered pages
made readable and which defects the observer found. It is not selection
evidence. A corrected, versioned rubric and board require a fresh observer,
fresh Claude judgment and operator direction.
