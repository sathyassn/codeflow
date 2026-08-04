# Exact continuation boundary

The W4 board is deterministically qualified and fully admissible: 84 renders, every gate in
`rubric.v4.md` section A passing, no band invalidated. **No composition on it has been observed,
and no direction is selected.**

## What happens next, in order

1. **A fresh answer-blind observation.** A session that authored nothing in this directory and has
   read no answer runs G1–G10 against the PNGs in `renders/`, the questions and declarations in
   `registry.json`, and `rubric.v4.md` — and nothing else. It must not open any `shared/` source,
   any `cases/**`, `directions/**` or `portal/**` markup, `checks/`, or
   `../tsk-014-w3/answer-key.md`. Reading any of those costs the timed gates permanently for this
   board. `observer-state.md` records why this session cannot supply that observation for any of
   the ten gates.
2. **Codex reviews the same exact inputs**, at the contract altitude: feasibility of each content
   model, the security delta direction D names, proportionality, maintainability, testability, and
   whether any direction's stated limit is understated.
3. **Operator selection**, from `board.html`: the content model, the portal families in scope, and
   the balance between authored composition and any earned reusable primitive.

Only after step 3 does anything move: `SPC-004`/`SPC-005` amendments, TSK-011, TSK-009, the
renderer, the schema, the portal, the ADRs, Agent OS.

## What must not happen

- **No production change before step 3.** The presentation runtime and schema, the documentation
  portal implementation, the ADR direction and Agent OS are deliberately untouched. The only
  non-disposable files this study changed are `cf-design` and its two references, and those
  changes are direction-neutral by construction — they add a rung, a protocol and three
  measurement obligations, and they name no form, palette, size or component.
- **No shared visual primitive.** One study, one author and mutually exclusive artefacts earn no
  system layer. `registry.convergence_hypotheses` stays a hypothesis.
- **No rewriting of W2 or W3.** Both directories are append-only evidence and stay byte-identical.
  `tools/verify.mjs` checks the W3 rubric against its own lock on every run for exactly this
  reason.
- **No edit to `rubric.v4.md` or `registry.json`.** Both are digest-pinned. A changed gate needs a
  new versioned rubric and a new lock; a changed declaration needs a new registration and a
  re-render. What *is* permitted, and required, is appending to `amendments.json`.

## One thing that is genuinely open, and is not a defect

The five directions are **not** presented with a hybrid as the assumed answer, and the author's
recommendation is recorded in the session handoff rather than on the board, so it cannot lead the
blind observer. A hybrid is a real option; it inherits both extension paths and both failure modes
of whatever it combines, and it earns its place only against alternatives that have each been
demonstrated separately — which, now, they have.

## Safety and rerun boundary

`render.mjs` launches headless only, on its own loopback port with its own temporary profile, and
never touches the operator's browser or active view. It needs host state a restrictive sandbox
denies (Mach port rendezvous on this platform); under denial it fails at launch and writes
nothing. Its lifecycle proof is weaker than W3's and `checks/lifecycle.json` says so: it closes
through Playwright and proves its own profile root absent rather than reimplementing W3's
process-inventory ownership contract.
