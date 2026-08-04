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
- **The board is incomplete.** Seven candidates (P3 ×2, D1 ×3, D2 ×2) do not
  exist yet, so G6 sibling-interchangeability and G3 differential cannot be run
  across the set the operator will actually decide on.

## What has been run

Deterministic checks only: axe, network isolation, render coverage, dataset and
fact-registry integrity, derived-answer agreement, rubric-lock verification, and
checksums. Their results are recorded in `checks/` and `SHA256SUMS`.

A clean deterministic pass says the study is internally consistent and
accessible. It says nothing about whether a composition communicates. That is
exactly what G1–G8 exist to test, and it remains untested.

## When the gates can run

After P3, D1 and D2 are authored and rendered, in a fresh session with an
observer who has authored none of the candidates and has not read
`answer-key.md`. That session receives the board and the questions only.
