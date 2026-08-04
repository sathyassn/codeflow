# Exact continuation boundary

This pass authored the study frame and the five P1/P2 candidates in full. Seven
candidates remain. They are absent, not sketched — nothing in this directory
stands in for them.

## Remaining work, in order

### 1. P3 — review convergence (2 candidates)

Content inventory to extract first, into `content-inventory.md`:

- the six review findings and their resolution across rounds, from
  `project-management/tasks/TSK-014.md` closeout paragraphs;
- the harness changes each round produced, from
  `crates/codeflow-present/web/scripts/real-browser-check.mjs` and
  `real-browser-cleanup-check.mjs` git history on this branch;
- the exact iteration in which each finding was raised, fixed, and re-verified.

Then author:

- `cases/p3/a-finding-anchored-delta/index.html` — the diff is primary; findings
  attach to the hunk they concern, carrying verdict state and iteration marker.
- `cases/p3/b-convergence-ledger/index.html` — findings × iterations, cells
  showing verdict transitions; the diff is secondary. Encodes whether a later
  iteration actually answered an earlier finding.

Task question to register in `answer-key.md` before rendering: *which finding
was raised, addressed, and then re-opened by a later round, and which round
introduced a defect that a previous round had not caught?*

### 2. D1 — portal Orient (3 candidates)

Content inventory: `docs/product.md`, `AGENTS.md` (six-layer table, git rules,
worktree doctrine), `docs/capabilities.md`.

- `cases/d1/a-concept-dependency-path/index.html`
- `cases/d1/b-task-first-entry/index.html`
- `cases/d1/c-contract-map/index.html`

Task question: *before starting a task on this repository, what must already be
true, and which gate refuses you if it is not?*

### 3. D2 — portal Record (2 candidates)

Content inventory: `docs/decisions/ADR-0052`, `project-management/tasks/TSK-014.md`,
`project-management/specs/SPC-004.md` frontmatter and their real relationships.

- `cases/d2/a-chain-in-place/index.html`
- `cases/d2/b-evidence-adjacent-margin/index.html`

Task question: *which decision governs this record now, and what evidence backs
its central claim?*

## Constraints that carry forward

- D1 and D2 must not contradict the TSK-008 `system-atlas` boundary/legend/
  evidence-chain idea; it is a conformance input, not a reopened question.
- Add the new page paths to `CANDIDATES` in `tools/render.mjs` and to `cases`
  in `board.html`, and to the `P1`/`P2`-style lists in `tools/verify.mjs`. All
  three iterate lists; no other structural change is needed.
- Each new case needs a shared subject source under `shared/` on the pattern of
  `plan-model.js` (derivation) or `tsk007-facts.js` (quotation registry), plus
  the matching rule in `tools/verify.mjs`, so those candidates cannot drift from
  their sources or from each other.
- Register each new candidate in the implementation-feasibility table in
  `board.html`, citing the block catalogue in
  `.codeflow/schemas/present/document-v1.schema.json` and SPC-004 §2/§11.
- Re-run `render.mjs`, then `verify.mjs --update`, then `verify.mjs`. The rubric
  is pinned by `rubric.lock.json`: it must not be edited when the remaining
  candidates land. If a gate genuinely needs to change, that is a new dated
  rubric and a new lock, never an edit in place.
- G1–G8 run only after the full board exists, in a fresh session with an
  observer who authored none of the candidates and has not read
  `answer-key.md`.
