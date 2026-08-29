# Exact continuation boundary

The W3 board is deterministically qualified and **partly admissible**. Fourteen of its
bands failed and cannot be observed at all; the rest can. No composition on it has been
observed, and no direction is selected.

An independent Claude design-primary audit has been run. It found that A1–A6 could all pass
while ten renders clipped their own content, that the registry's account of its own
amendments was false — two of one candidate's composition declarations were rewritten after
its renders were inspected, and the study then said only marker tokens had changed — and
that four further bands lose their governing idea invisibly to any machine check.
`rubric.v3.md` governs from here; `rubric.v2.md` is untouched and still pinned.

Every one of the fourteen carries a demonstrated failure as well as an admissibility
verdict: six disqualifying, six material, two minor. **Inadmissible is not neutral.**

## What happens next, in order

1. **Disposition of the fourteen failed bands**, by the candidates' author or by the
   operator. Six are mechanically fixable: `p3-a`'s clipped off-axis label at mobile in both
   modes, `p3-b`'s viewBox descenders at desktop in both modes, and the two baselines that
   overflow a 390px viewport. Eight need a design decision rather than a repair: `p1-a` and
   `p2-a` at mobile in both modes, and `p3-b`'s silently truncated quoted source at tablet
   and mobile in both modes. Each band is either redrawn and re-rendered, or withdrawn for
   that context with the loss stated. The audit deliberately redrew nothing: the seat judging
   a board cannot repair the drawings it is judging without becoming their author.
2. **A fresh answer-blind observation.** A session that authored nothing here and has read
   no answer runs G1–G8 from `rubric.v3.md` against the still-valid PNGs in `renders/`, the
   questions in `registry.json`, and the rubric. It must not open `answer-key.md`,
   `content-inventory.md`, any `shared/` source, any `cases/**/index.html`, or `checks/`.
   Reading any of those costs the timed gates, permanently, for this board. It records
   **nothing** for a band listed in `registry.invalidated_bands`.
3. **G8 is graded on two axes, separately.** Fact presence and idea survival are recorded
   as distinct results for every candidate at tablet and mobile, and in dark. A variant that
   keeps every label while losing the governing relationship is a **fail**, not a pass with
   a note. That is the specific failure W2's G8 let through.
4. **Operator selection** from the corrected board — the direction per case, and the
   balance between authored composition and any earned reusable primitive.

Only after step 4 does anything move: `SPC-004`/`SPC-005` amendments, TSK-011 and TSK-009,
the renderer, the schema, the portal, the ADRs.

## What must not happen

- **No production change before step 4.** The presentation runtime and schema, the
  documentation portal implementation, the ADR direction and Agent OS are all deliberately
  untouched by this study.
- **No shared visual primitive.** The absence-as-a-state convergence stays in
  `hypotheses.md` until real product recurrence exists. One study, one author and mutually
  exclusive candidates earn no system layer.
- **No rewriting of W2.** `../tsk-014-w2/` — its board, its pinned rubric, both observation
  records — is append-only evidence and stays byte-identical. `tools/verify.mjs` checks the
  W2 rubric against its own lock on every run for exactly this reason.
- **No edit to `rubric.v2.md`, `rubric.v3.md`, or any registered field of `registry.json`
  after this point.** A changed gate needs a new versioned rubric and a new lock; a changed
  question or encoding declaration needs a new registration and a re-render. What *is*
  permitted, and required, is appending to `registry.amendments` and
  `registry.invalidated_bands`: those exist so a divergence is recorded instead of a
  declaration being quietly edited to match a drawing. `A8` enforces that the record exists;
  it cannot enforce that it is complete, and the study has already shown that two divergences
  went unnoticed until an independent seat looked.

## If a candidate is selected

Read its carrier verdict first, from `registry.json` or the table at the bottom of
`board.html`. Five of the six `cf-present` candidates are `needs-new-block`: selecting one
commits to a new typed block whose renderer owns responsive and mode behaviour, because the
sandboxed HTML escape block cannot carry a media query. Four of the five portal candidates
are `needs-source-model`: the drawing is ordinary generator work, and what is missing is a
fact the source graph does not carry. That work is plannable, and it is stated now rather
than discovered after a winner is chosen — which is the whole point of registering it
before authoring.

## Safety and rerun boundary

Every source-authority, repository-binding, process-identity, temporary-root, network,
accessibility, checksum and cleanup contract carried over from W2 still applies, and the
W3 additions (visible-text capture, motion probing, registry contract) sit on top of it.
Use the study's own render, self-test and verify tools. Never substitute a manual `rm -rf`
or a broad process match for the ownership proof; if the harness fails closed and retains
its marked root, that is the contract working.
