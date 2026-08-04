# Observer state

## G1–G8 are not run

The eight protocol gates in `rubric.md` require an observer who has not authored
the candidates and has not seen the expected answers. Neither condition is
currently satisfiable:

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
accessible. It says nothing about whether a composition communicates. That is
exactly what G1–G8 exist to test, and it remains untested.

## When the gates can run

Now, as far as the board is concerned: P3, D1 and D2 are authored and rendered,
so G3 differential and G6 sibling-interchangeability can finally be run across
the whole set the operator will actually decide on. The only remaining condition
is the observer: a fresh session that authored none of the candidates and has
not read `answer-key.md`, receiving the board and the questions only.

Until that session reports, every gate stands at **not run** — which is a
result, not a gap to be filled by anyone who has already seen the answers.
