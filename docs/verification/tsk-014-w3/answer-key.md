# Expected answers and recorded governing ideas

> **Do not give this file to an observer.** It holds the answers used to grade G1, G2 and
> G4. A session that has read it is disqualified from the timed gates for this board, on
> the same terms as every earlier pass — see `observer-state.md`.

Recorded before observation. The questions themselves are in `registry.json` and may be
shown; the answers below may not.

Every candidate publishes its case's derived answer into `document.documentElement.dataset.answer`,
which is not rendered anywhere. `tools/render.mjs` captures it into `checks/answers.json`,
and `tools/verify.mjs` recomputes the expected value from the repository — never from the
page — and compares them for **every recorded render**, not one per page. Separately, A1
checks the visible text of every render against the case's registered leak tests, so a
candidate cannot satisfy the machine channel and also state the answer in prose.

---

## P1 — the plan graph

**Question.** *If exactly one unfinished EPC-005 task slipped by one wave, which tasks
would move the finish and which would not?*

**Expected answer**, recomputed by the verifier from `project-management/tasks/*.md`
frontmatter:

- **Moves the finish: `TSK-014`, `TSK-011`, `TSK-007`, `TSK-010`.** Each has zero room:
  its earliest and latest wave are the same.
- **Does not move the finish: `TSK-013` (two waves of room) and `TSK-009` (one).**
- Supporting: the run with no room is `TSK-014 → TSK-011 → TSK-007 → TSK-010`, four tasks
  over four waves. `TSK-013` and `TSK-014` are the two that nothing still open blocks.
  `TSK-012` is `cancelled` and is not outstanding work.

**Recorded governing ideas.**

- `a-critical-path-and-room` — *one unbroken run sets the finish, and every task off it
  carries a measured amount of later start.*
- `b-paths-to-the-sink` — *four routes still reach the end and one is longer than the
  others; a task's exposure is which routes it stands on.*

**Claimed relationships for G4.** `a-critical-path-and-room`: consequence of delay, as a
continuous run plus extent on a shared wave scale, with zero extent drawn rather than
omitted. `b-paths-to-the-sink`: dominance and overlap between whole routes, measured by
reach against one shared scale of positions.

## P2 — the qualification evidence

**Question.** *Which platforms carry executed evidence for the exact candidate, and for
each platform that does not, what kind of thing stopped it?*

**Expected answer**, recomputed from `shared/tsk007-facts.js` and checked verbatim against
`docs/verification/tsk-007-presentation/README.md`:

- **Executed lanes: `macOS arm64` and `independent review`.** No other lane carries an
  executed run at candidate `82671551`.
- **Causes, by lane:** macOS arm64 — the check proves less than the claim (the launcher
  journey ran `--no-launch`). Linux arm64 — run but not at this candidate, a run that
  produced nothing to stand on, and the check proving less than the claim. Native Windows —
  no environment to run it in, and the check proving less than the claim. WSL2 — no
  environment to run it in. Independent review — refused before it began
  (`oauth_org_not_allowed`).
- Supporting: of twenty-two places where a run could have existed, eighteen do not, and
  eleven of those eighteen are the single cause "no environment to run it in" — the
  expired Parallels licence. Four runs happened.

**Recorded governing ideas.**

- `a-stopped-provenance` — *each claim travels a fixed distance and stops, and where it
  stops says what stopped it; the part not travelled stays on the page.*
- `b-absence-by-cause` — *most of this evidence set is absence, and almost all of that
  absence has one cause.*

**Claimed relationships for G4.** `a-stopped-provenance`: distance travelled along a
staged rail with the stopping point positional and the untravelled remainder drawn.
`b-absence-by-cause`: classification with magnitude, where a cause's share of the evidence
set is its drawn length.

## P3 — the review convergence

**Question.** *Which findings took more than one round to close, and which of those was
re-opened by something that did not come from the review itself?*

**Expected answer**, recomputed from the findings array rather than read off the source's
own summary:

- **Three findings needed more than one round: `F1`, `F5`, `F6`.**
- **`F6` is the one whose re-opening did not come from a round.** R7 built the bounded
  exact-owned termination fallback and the gate that rejects any fallback in a normal run;
  the retry tripped that gate anyway, and the cause was the pinned browser toolchain —
  Chrome 150 driven through Playwright 1.55, outside that release's tested window. R8
  aligned `playwright-core` to 1.62.1.
- `F1` and `F5` were re-opened by their own previous fixes: R2's allow-list omitted
  Windows' real `SystemRoot` casing; R5 replaced a timeout with the right readiness
  semantics but dropped the bound, and R6 had to cap every wait against the journey's
  remaining time.
- Supporting, and the distinction the question turns on: `F3` is **extended** at R7 and R8,
  not re-opened — those rounds widened what the injected-failure check proves without the
  earlier fix having failed. `F4` and `F6` close with recorded residuals.

**Recorded governing ideas.**

- `a-convergence-with-source-depth` — *three of these findings came back, and exactly one
  of the things that brought one back was never a round at all.*
- `b-source-region-history` — *the review landed in a handful of places, and one file
  absorbed nearly all of it.*

**Claimed relationships for G4.** `a-convergence-with-source-depth`: state over an ordered
axis with back-reference, plus a region explicitly off that axis, plus the exact source a
row's work landed in. `b-source-region-history`: accumulation in place, in file order.

## D1 — the portal's Orient surface

**Question.** *Where does this repository keep each kind of knowledge, and how is each kind
allowed to change?*

The W2 question was withdrawn, not amended. It asked for an exact precondition lookup
against an orientation surface, and a numbered list wins that by construction; the
comparison was decided by the question's shape before anything was drawn.

**Expected answer**, recomputed by re-parsing the organisation table out of `AGENTS.md`:

- **Six layers, in the contract's own order** from least to most frequently changing:
  WHY (`docs/product.md`), RULES (`AGENTS.md` + the agent skills), WHAT
  (`docs/capabilities.md`), HOW (`docs/architecture.md` + `docs/decisions/`), WORK
  (`project-management/`), TRACE (the ledger + `codeflow status`). WHY and RULES share a
  position: the contract gives both the cadence "rarely".
- **What may be written by hand:** `docs/product.md` (human-owned) and `AGENTS.md` outside
  the codeflow markers (project-owned).
- **What may be written, but only inside the ship flow:** `docs/capabilities.md` and
  `docs/architecture.md`.
- **What refuses an ordinary edit:** `.claude/skills` and `.agents/skills` and the block
  between the codeflow markers in `AGENTS.md` (tool-maintained), `project-management`
  (CLI-allocated), `docs/decisions` (append-only), the local ledger (append-only) and
  `codeflow status` (generated).
- **`AGENTS.md` is the one artefact whose authority is split** across that boundary.
- **TRACE has no path in the working tree at all** — the layer that changes most often is
  the one with nothing to open.

**Recorded governing ideas.**

- `a-layers-by-change-rate` — *the parts of this repository that change most are the parts
  a person writes least.*
- `b-the-spine` — *one continuous descent explains how work reaches its own record, and
  two layers govern that descent rather than sitting on it.*
- `c-write-authority-map` — *the repository is partitioned by who may write it, and a
  single line marks where an ordinary edit stops.*

**Claimed relationships for G4.** `a-layers-by-change-rate`: stratification by volatility
with a genuine tie. `b-the-spine`: derivation along one unbroken descent, with attachment
shape carrying change discipline. `c-write-authority-map`: partition with a drawn
perimeter and one artefact straddling it.

## D2 — the portal's record surface

**Question.** *Which decision governs this record now, how much of it does that decision
actually reach, and which of its claims has nothing behind it?*

The record is `project-management/specs/SPC-004.md`.

**Expected answer**, recomputed from the claims array and checked against real frontmatter:

- **ADR-0052 governs it now, and its reach is partial.** Its only link to CLM-3 is a
  named-mechanism supersession; its own scope sentence says ADR-0049 and ADR-0050 "remain
  historical decisions" and that it "supersedes only their same-tree runtime assumption and
  post-create Windows DACL mechanism where those details conflict". `superseded_by` is
  `null` on all three, which is exactly why frontmatter alone cannot answer this.
- **ADR-0049 and ADR-0050 are still in force**, and the verifier confirms both are
  `accepted` with `superseded_by: null`.
- **CLM-5 has nothing behind it.** SPC-004 § Behavior 3 requires the selected default to be
  "modern, elegant, aesthetically coherent, pleasing, functional, responsive, and suited to
  focused explanation/review". TSK-014 exists because the operator's real trial invalidated
  the prior design acceptance, and the replacement board's blind gates are not run.
- Supporting: CLM-1 and CLM-2 are backed by executed runs; CLM-3 and CLM-4 are bounded.
  Across the record's sixteen sections, the four attached decisions land on five places in
  total.

**Recorded governing ideas.**

- `a-decision-reach` — *four decisions are attached to this record and between them they
  cover almost none of it.*
- `b-evidence-margin` — *every claim has a place for its evidence, and one of those places
  is deliberately empty.*

**Claimed relationships for G4.** `a-decision-reach`: partial supersession as measured
extent over the record's own sections, with the skipped runs drawn. `b-evidence-margin`:
claim-to-artefact adjacency by vertical position, with absence rendered as reserved space
rather than as a blank cell.
