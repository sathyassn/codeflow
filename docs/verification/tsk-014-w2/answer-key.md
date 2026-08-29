# Task questions, expected answers, and recorded governing ideas

Recorded before observation. The observer is given only the question; this file
holds the answer used to grade G1, G2, and G4.

> **Observer state — read `observer-state.md`.** G1–G8 are **not run**. Codex
> read this file during the pass-1 integrity review, so the blind gates are void
> for pass 1 and will be run by a fresh non-author session once the full board
> exists. Nothing below is a verdict.

## P1 — plan graph

**Task question (pass 2).** *Which EPC-005 tasks can start today, which chain of
unfinished tasks is the longest path to `TSK-010`, and which of the startable
tasks could wait without moving that finish?*

The pass-1 question was withdrawn: it asserted that one task "controls every
remaining path to TSK-010", which is false — `TSK-013 → TSK-010` reaches the
sink without passing through `TSK-014`. It also asked for slack, which two of
the three candidates did not encode. The replacement asks only what all three
candidates now derive from the same dataset.

**Expected answer**, computed from the frontmatter in `content-inventory.md`:

- **Startable today: `TSK-013` and `TSK-014`.** Every other unfinished task has
  at least one unfinished dependency.
- **Longest path to the sink: `TSK-014 → TSK-011 → TSK-007 → TSK-010`** — four
  tasks, four waves. No other remaining path is longer; `TSK-013 → TSK-010` is
  two.
- **`TSK-013` could wait.** It has two waves of room. `TSK-014` has none: it is
  on the longest chain, so any delay to it moves the finish.
- Secondary: `TSK-009` also has room (one wave) but is not startable today.
  `TSK-012` is `cancelled` and is not outstanding work.

`tools/verify.mjs` recomputes all three parts from the repository frontmatter
and compares them against the answer each candidate publishes at runtime, so a
candidate cannot display an answer this key does not support.

**Recorded governing ideas.**

- `a-wave-lanes` — *two tasks can start today; one of them sits on the chain that
  sets the finish, and that chain runs through both utility streams.*
- `b-blocking-matrix` — *the blocking relation is concentrated in one column, and
  the same table says which rows are free to start and which have room.*
- `c-critical-ribbon` — *the finish is set by a single four-task chain;
  everything else has measurable room.*

**Claimed relationships for G4.** `a-wave-lanes`: ordered waves with cross-stream
fan-out, longest chain emphasised. `b-blocking-matrix`: pairwise blocking
incidence with derived readiness and room. `c-critical-ribbon`: criticality
ranked against room. ("Slack" survives only as an internal variable name in
`shared/plan-model.js`; every visible surface says *room*.)

## P2 — qualification evidence

**Task question.** *At the exact implementation candidate `82671551`, which
platforms have executed journey evidence, and which claim is blocked by an
authorization failure rather than by a test failure?*

**Expected answer**, from `docs/verification/tsk-007-presentation/README.md`:

- Executed at the exact candidate: **macOS arm64 only**.
- **Linux arm64** evidence exists but at the older revision `74feaf04`, is
  explicitly retained as bounded diagnostic, and is **not** promoted to
  exact-candidate approval; its desktop Chromium run exited `SIGTRAP` and is not
  claimed as passed.
- **Native Windows and WSL2**: no run at all — the Parallels licence had
  expired. The Windows guard proves start-time confinement only.
- Blocked by authorization, not by a test: the **Claude judgment-primary
  verdict**, whose accepted turn ended with `oauth_org_not_allowed`.

Both P2 candidates render from one byte-identical fact registry whose every
material claim carries a verbatim quotation; `tools/verify.mjs` checks each
quotation against the actual TSK-007 record.

**Recorded governing ideas.**

- `a-evidence-grid` — *coverage is one platform deep; the empty cells are the
  finding.*
- `b-provenance-rail` — *each claim runs along one rail from production to
  acceptance, and four of six rails stop short of the exact candidate.*

**Claimed relationships for G4.** `a-evidence-grid`: coverage incidence across
claim × lane, with absence explicit. `b-provenance-rail`: derivation distance
travelled along a continuous rail, with the stopping point and the untravelled
remainder both visible.

## P3 — review convergence

**The registered question was corrected before authoring.** `NEXT.md` registered:
*which finding was raised, addressed, and then re-opened by a later round, and
which round introduced a defect that a previous round had not caught?* Two
problems, both recorded in `content-inventory.md`:

- it presumed a single re-opened finding and a single defect-introducing round.
  The history has **three** multi-round findings and **two** rounds whose own fix
  caused the next round's work. A question with one answer where the evidence has
  several is unanswerable, not hard;
- its inventory named "the six review findings", which are the W1 design-method
  batch. That batch carries no per-iteration timing, so no honest ledger can place
  it on an iteration axis.

**Task question (corrected).** *Which findings needed more than one round to
close, and which single one of them was re-opened by something outside the
harness rather than by the previous fix?*

**Expected answer**, from the rounds and findings in `content-inventory.md`:

- **Three findings needed more than one round:** `F1` (the child environment
  inherited unrelated host values), `F5` (cold-runner startup exceeded the
  qualification's bounded waits), `F6` (the browser context close timed out with
  an earlier task-owned Chrome present).
- **`F6` is the one re-opened from outside the harness.** R7 built the bounded
  exact-owned termination fallback and the gate that rejects any fallback in a
  normal run; the full integration retry then rejected an intermittent normal
  fallback anyway. The cause was not the fix: the qualification had been driving
  Chrome 150 through Playwright 1.55, outside that release's tested stable-browser
  window. R8 aligned `playwright-core` to 1.62.1 and began recording the observed
  browser and Playwright versions so a recurrence is attributable.
- The other two were re-opened by their own fixes: `F1` because R2's allow-list
  omitted Windows' real `SystemRoot` casing, and `F5` twice — R5 replaced the
  timeout with the right readiness semantics but dropped the bound, and R6 had to
  cap every wait against the journey's remaining 180 seconds.
- Secondary: `F3` is *extended* at R7 and R8, not re-opened — the later rounds
  widened what the injected-failure check proves without the earlier fix having
  failed. `F4` and `F6` close with open residuals (start-time Windows confinement
  only; an intermittent close fallback reduced, not eliminated).

`tools/verify.mjs` recomputes the multi-round set and the external-cause finding
from the shared dataset and compares them against the answer each P3 candidate
publishes at runtime. It also checks every quoted diff line against
`git show <sha>` and every quoted sentence against `TSK-014.md`.

**Recorded governing ideas.**

- `a-finding-anchored-delta` — *the same handful of lines was rewritten in three
  consecutive rounds; the marks beside the code say which round finally settled
  them and which round caused the next one's work.*
- `b-convergence-ledger` — *three findings did not close on first fix; the ledger
  separates a round that answered an earlier finding from one that merely widened
  it, and it shows that only one re-opening came from outside the harness.*

**Claimed relationships for G4.** `a-finding-anchored-delta`: change located in
source, with verdict state and round attached to the exact region it concerns.
`b-convergence-ledger`: state transition over an ordered round axis, with
back-reference from a re-opening round to the round it re-opened.

## D1 — portal Orient

**Task question.** *Before starting a task on this repository, what must already
be true, and which gate refuses you if it is not?*

**Expected answer**, from `AGENTS.md` and the three enforcement planes in code:

- **Branch identity.** You are on `task/TSK-NNN-<slug>`, a prefix in the policy
  whitelist, and that branch resolves to a task record visible in the checkout.
- **A valid visible workgraph.** `codeflow validate --docs` passes.
- **Everything anchored at the merge-base** with the task's declared
  `integration_target` — which must itself be a stable, resolvable, non-task
  branch matching the record's declaration: the task record exists there, its
  status is `todo` or `in_progress`, its epic resolves (or it carries a
  `standalone_reason`), every spec it and its epic require is `approved` or
  `implemented`, and every dependency is `complete`.
- **The gate is one read-only merge-base preflight surfaced in three planes** —
  `codeflow work start TSK-NNN` interactively, the `pre-commit` hook, which blocks
  the commit, and `codeflow ci`, which is authoritative. It reports
  **`work.stable_planning_anchor`** for every anchoring failure, with
  `work.task_record` when the branch names no visible record and `work.valid_graph`
  when the workgraph itself is invalid.
- Secondary, and not part of the anchor gate: `git.branch_naming` (pre-push and
  CI) refuses an unlisted prefix; `git.secret_scan` (pre-commit) is the one gate
  never relaxed; `git.commit_format`, `git.commit_body`, `git.ai_attribution` and
  `git.commit_emoji` (commit-msg and CI) refuse the message; the protected-branch
  family refuses the mutation itself. On *this* repository, remote branch
  protection is unavailable and not pursued, so the authoritative perimeter is CI
  plus human-merged PRs.

All three D1 candidates render from one byte-identical dataset and publish the
same derived answer.

**Recorded governing ideas.**

- `a-concept-dependency-path` — *this repository is understood in one order:
  nothing about the gate makes sense until the merge-base anchor does, and nothing
  about the anchor makes sense until durable records do.*
- `b-task-first-entry` — *there is a single path from "I have a task" to a first
  commit, and every place it can refuse you is on that path, named.*
- `c-contract-map` — *one policy document, five enforcement planes; only two of
  them are the real boundary, and on this repository one of those two is disarmed.*

**Claimed relationships for G4.** `a-concept-dependency-path`: prerequisite
ordering with depth — which concept cannot be read before which.
`b-task-first-entry`: a sequence with refusal branches leaving the main line.
`c-contract-map`: ownership incidence across planes, with an authority boundary
drawn through it.

## D2 — portal Record

**Task question.** *Which decision governs this record now, and what evidence
backs its central claim?*

The record is `project-management/specs/SPC-004.md`.

**Expected answer**, from the record frontmatter and ADR bodies:

- **ADR-0052 governs it now — but only partially.** SPC-004's decision lineage is
  ADR-0049 (runtime and renderer boundary) and ADR-0050 (document and session
  contract). ADR-0052, accepted the same day, states that those two "remain
  historical decisions" and that it "supersedes only their same-tree runtime
  assumption and post-create Windows DACL mechanism where those details conflict".
  Everything else in ADR-0049 and ADR-0050 still stands, `superseded_by` is `null`
  on all three, and SPC-004's behavioural envelope is unchanged — ADR-0052 says
  in terms that it "changes no product outcome, task owner, graph, or integration
  target".
- **The central design claim is not backed.** SPC-004 § Behavior 3 requires the
  selected default to be "modern, elegant, aesthetically coherent, pleasing,
  functional, responsive, and suited to focused explanation/review". TSK-014 exists
  because the operator's real trial invalidated the prior design acceptance, and the
  replacement rendered board's blind gates G1–G8 are **not run**. There is no
  current evidence for that claim.
- The evidence that *does* exist backs the surrounding runtime and lifecycle
  claims, and is itself bounded: the TSK-007 qualification record covers macOS
  arm64 only at the exact candidate `82671551`, and its Claude judgment verdict is
  blocked on `oauth_org_not_allowed`.
- Secondary: SPC-004 is `approved`, not `implemented`, because its consuming work
  (TSK-011, `blocked` on TSK-006 and TSK-014) has not shipped.

Both D2 candidates render from one byte-identical dataset; `tools/verify.mjs`
checks every frontmatter value against the real record files and every quotation
verbatim against its source.

**Recorded governing ideas.**

- `a-chain-in-place` — *the decision that governs this record reaches only part of
  it; the chain is visible where it applies and visibly absent where it does not.*
- `b-evidence-adjacent-margin` — *every claim carries its evidence at eye level,
  and the margin is empty exactly where the record's central claim should be
  backed.*

**Claimed relationships for G4.** `a-chain-in-place`: derivation and partial
supersession, in reading order, with scope shown rather than asserted.
`b-evidence-adjacent-margin`: claim-to-artifact pairing by vertical position, with
absence rendered as absence.
