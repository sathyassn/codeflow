# Answer key — W5

**An observer must not open this file until after answering.** Reading it costs
G1, G2, G3 and G12 permanently, for this board, for that session.

`rubric.v4.md` G1 required an `answer-key.md` "recorded before observation", and
W4 shipped none — its `observer-state.md` and `NEXT.md` pointed a future observer
at `../tsk-014-w3/answer-key.md`, which is the key to a different board with
different candidates. So W4 had no admissible key at all. This file is that
correction, at the path `NEXT.md` names.

Every value below is **computed from the shared subject sources**, not typed from
memory: `shared/plan-model.js` over `shared/plan-data.js`, `shared/tsk007-facts.js`,
and `shared/p3-convergence.js` each publish an `answer` object derived from their
own raw arrays, and each candidate renders from the same object it is scored
against. `tools/verify.mjs` recomputes them rather than reading them off a page.

---

## P1

**Question.** If exactly one unfinished EPC-005 task slipped by one wave, which
tasks would move the finish and which would not?

**Answer.**

- **Would move the finish** — `TSK-007`, `TSK-010`, `TSK-011`, `TSK-014`. These
  four have zero slack: each sits on the longest chain through the unfinished
  graph, so a one-wave slip on any of them moves the finish by one wave.
- **Would not** — `TSK-009` has one wave of room; `TSK-013` has two. A one-wave
  slip on either is absorbed.
- The chain itself, in order: `TSK-014 → TSK-011 → TSK-007 → TSK-010`.
- Startable today, with no unfinished dependency: `TSK-013`, `TSK-014`.

**A right answer names the four and separates them from the two.** Naming the
four without the room, or listing all six as equivalent, is wrong.

---

## P2

**Question.** Which platforms carry executed evidence for the exact candidate,
and for each platform that does not, what kind of thing stopped it?

**Answer.**

- **Carry executed evidence** — `macOS arm64` and `independent review`. Two of
  the five lanes.
- **Do not, and what stopped each:**

| Lane | What stopped it |
|---|---|
| `Linux arm64` | run, but not at this candidate; ran and produced nothing to stand on; the check proves less than the claim |
| `native Windows` | no environment to run it in; the check proves less than the claim |
| `WSL2` | no environment to run it in |

  `macOS arm64` also carries one absence of its own — the check proves less than
  the claim, on the launcher — and `independent review` carries one, refused
  before it began. A lane can carry an executed run and still have a gap.
- Across the board there are **18 classified absences**: no environment 11,
  the check proves less than the claim 3, run but not at this candidate 2,
  ran and produced nothing 1, refused before it began 1.

**A right answer names the two lanes that carry executed evidence and gives a
*kind* of cause per lane that does not** — not a count, and not "it wasn't run"
for every one of them, which erases the difference between a missing machine, a
failed run, and a check that proves less than the claim.

---

## P3

**Question.** Which findings took more than one round to close, and which of
those was re-opened by something that did not come from the review itself?

**Answer.**

- **Took more than one round** — `F1`, `F5`, `F6`.
- **Of those, re-opened from outside the review** — `F6` only. Its re-opening
  came from a browser-version change (`Chrome 150` × `Playwright 1.55.0`), not
  from anything an earlier round did.
- `F1` and `F5` were re-opened by their own previous fixes.

**A right answer names all three and singles out F6.** Naming F6 alone, or
naming three without separating F6, is a partial answer. Attributing the outside
cause to any other finding is wrong — that is the specific miscommunication the
p3a channel repair (F08, D10, D11) exists to prevent.

---

## What a wrong answer means

G12 outranks every other result for that candidate. A candidate that produces a
*wrong* answer has not weakly communicated; it has miscommunicated. The previous
board carried one of those — `p3b`, which is withdrawn from W5 and kept as a
rejection record under `rejected/p3b-source-region-history/` (see
`amendments.json`, A-03).
