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

## P3, D1, D2

Not authored — see `NEXT.md`.
