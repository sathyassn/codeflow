# Exact continuation boundary

W5 is a **successor** board to W4. W4 is not edited in any respect and stays
byte-identical under its own locks; `tools/verify.mjs` checks that on every run.

The board is deterministically qualified: 90 renders, every gate in
`rubric.v5.md` section A passing, no band invalidated. **No composition on it has
been observed, and no direction is selected.** This session authored all of it
and is disqualified from every observation gate.

## What happens next, in order

1. **A fresh answer-blind observation.** A session that authored nothing in this
   directory and has read no answer runs **G1–G12** against the PNGs in
   `renders/`, the questions and declarations in `registry.json`, and
   `rubric.v5.md` — and nothing else. It must not open `answer-key.md`, any
   `shared/` source, any `cases/**`, `directions/**` or `portal/**` markup,
   `checks/`, or any earlier board's answer key. Reading any of those costs the
   timed gates permanently for this board.

   The key it is scored against is **`answer-key.md` at this directory's own
   root**. That path is the correction of the W4 inconsistency recorded in
   `rubric.v5.md`; do not send an observer to `../tsk-014-w3/answer-key.md`,
   which belongs to a different board with different candidates.

   Two gates are new and must be run explicitly: **G11**, decoding each keyed
   distinction from the figure at each width and mode *without* consulting the
   legend, naming which pairs are confusable; and **G12**, checking the answer
   against the key *after* it is given.

2. **Codex reviews the same exact inputs**, at the contract altitude:
   feasibility of each content model, proportionality, maintainability,
   testability, and whether any direction's stated limit is understated.

3. **Operator selection** — the content model, the portal families in scope, and
   the balance between authored composition and any earned reusable primitive.

Only after step 3 does anything move: `SPC-004`/`SPC-005` amendments, TSK-011,
TSK-009, the renderer, the schema, the portal, the ADRs, Agent OS.

## What the observer should know before starting

- Three artefacts are **rejection records, not candidates**: `rejected/d-authored-svg/`,
  `rejected/e-raster/` and `rejected/p3b-source-region-history/`. They carry W4
  renders and no repair. Do not score them.
- `cases/p3` has **one** candidate. The sibling comparison W4 offered is gone;
  `registry.json` already declared it unscorable, and `amendments.json` A-03
  records the withdrawal and its reasons.
- `portal/area-drilldown` is a **declared fixture**, not this repository. It says
  so on the page, in the registry, and in the figure's own description.
- Every scorable surface now has a plain-chat, plain-Markdown and plain-HTML
  control. W4 shipped three portal families and a control for one; `baselines/d3`
  is the one this board added.

## What must not happen

- **No production change before step 3.** The presentation runtime and schema,
  the documentation portal implementation, the ADR direction and Agent OS are
  deliberately untouched.
- **No shared visual primitive.** One study, one author and mutually exclusive
  artefacts earn no system layer. `registry.convergence_hypotheses` stays a
  hypothesis.
- **No rewriting of W2, W3 or W4.** All three are append-only evidence and stay
  byte-identical.
- **No edit to `rubric.v5.md` or `registry.json`.** Both are digest-pinned. A
  changed gate needs a new versioned rubric and a new lock; a changed declaration
  needs a new registration and a re-render. What *is* permitted, and required, is
  appending to `amendments.json`.

## Known weaknesses this board carries, stated rather than found

- **A19 is too loose to catch what it was written for.** `amendments.json` A-04
  measures it: the artefact whose 6279px narrow band motivated the gate passes it
  at 1.87x, because a ratio compares a composition with itself. A stronger gate
  is a rubric v6 change and is deliberately not invented here after seeing which
  artefact it would have failed.
- **The reuse re-proof is partial.** F13 asked for three things: variance in the
  second binding, a third recipe W4 never demonstrated, and a real extension
  exercise costing out a form the library lacks. Only the first is done. See
  `README.md`, "What is not done".
- **Rendering coverage is not observation coverage.** A16 closes the first and
  says so; the second stays open until step 1.
- **A15 proves distinctness, not legibility.** Every pair of states in every
  material distinction is now proved to differ on at least two non-hue channels,
  in all six contexts, from tokens measured off the render. Whether those
  distinct encodings can actually be told apart — an upright rule against a
  diagonal hatch, a dash against a dot, at 15px and in dark — is **G11**, and it
  is the gate this board most needs run. The repair added encoded distinctions;
  it did not make any of them readable.
- **A15 was wrong twice, and nothing on the board noticed either time.** Its
  rendered half was never implemented; the replacement then selected evidence
  with an always-true predicate and proved differences on channels the registry
  never named. Every run reported green throughout, and independent review found
  both. Treat a clean section-A result as evidence that the checks that exist
  passed — not that the checks that should exist do, and not that the ones that
  do exist measure what they claim. The gate now carries five negative controls
  for exactly this reason; a gate nobody has watched fail is not yet evidence.

## Safety and rerun boundary

`render.mjs` launches headless only, on its own loopback port with its own
temporary profile, and never touches the operator's browser or active view. It
needs host state a restrictive sandbox denies; under denial it fails at launch
and writes nothing. Its lifecycle proof is weaker than W3's and
`checks/lifecycle.json` says so: it closes through Playwright and proves its own
profile root absent rather than reimplementing W3's process-inventory ownership
contract.
