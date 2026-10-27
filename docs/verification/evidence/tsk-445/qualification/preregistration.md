# TSK-445 native qualification: pre-registration

Registered before any trial launched, 2026-10-26T20:16:03Z.

- Kind: focused diagnostic of an instruction change (explanation-method pack
  cases), not a canary or full suite; one native trial per case and arm.
- Suite: sha256:8f39d76721c552debd496941313328865b4b0f995852b3ae54bf1f030becd15f at d9b5954e9,
  re-registered as sha256:7bd8b3eae964b65d4fc721fe80bb0a5eb0a3c05cc80917c244711251fa6a567b after
  the review rounds (anchor wording, then the exact and judged split in the
  retry fixture's grading note). The
  candidate binary is rebuilt at the head the trials run on, and its digest
  is recorded here before launch.
- Cases: retry-review-opens-with-the-whole (CF-METH-004, CF-PRES-004) and
  migration-review-leads-with-the-picture (CF-METH-004, CF-PRES-004).
- One variable: the CodeFlow revision the fixture is scaffolded with.
  - Qualifying arm, candidate: d9b5954e9 (TSK-445 head), binary
    sha256 7c80dddb89bffb5aa6545cc53762af73e34a9f0ff0f229c58d8f08fc614df0fa.
  - Comparison arm, baseline: 0f08dd7f8 (integration line before TSK-445),
    binary sha256 70426e4eead8ff8f7bed53a7686154303f995c635fca2f408a7832540f066e49.
- Fixed: harness Claude Code 2.1.294 in a Herdr tab
  (interactive TUI, not headless), model opus (Opus 5.5), effort high,
  permission mode auto (the runner's proven footers are manual, auto and
  bypass; acceptEdits was refused at readiness), dedicated evaluator home
  ~/.codeflow-eval/claude, launched by evals/qualification/runner.py.
- Order: sequential, retry case then migration case, candidate arm first
  in each pair.
- Grading: the present document the subject opened (its stored revision),
  in two parts. Exact: `evals/qualification/anchored/check.py`, run in its
  managed venv with the parser pinned in `anchored/requirements.txt`,
  settles structure (anchor, sections, figure order, one ask, no question
  titles, code left out) and that the retry mechanism's named states and
  transitions, a label with a duration and a label with a failure count are
  drawn at both widths; it reads values for the judge and judges none. Judged: the evaluator, Claude Opus 5.5, applies the rubric in
  the fixture's grading note (J1 facts: that the backoff wait is drawn and
  labelled, values, counts, which transition they belong to, negations and added claims; J2 the uncertainty statement)
  and the migration fixture's facts (downtime, cost, evidence), and checks
  that each figure is legible. The other-lineage grading pass is owed to
  the primary.
- Pass for AC-7: the candidate retry trial opens with an anchor and an
  orienting figure and carries the mechanism in a figure, with no
  prohibited signal.

## Outcome so far

- Three launches were refused at readiness before any prompt: twice
  because Claude Code installed an update into the trial's disposable home
  at start (fixed: the kit now sets `DISABLE_AUTOUPDATER=1` for subjects),
  once because the runner's proven Claude footers do not include
  `acceptEdits` (the arm now uses `auto`).
- The fourth launch delivered the prompt to a ready seat. The evaluator
  account answered "You've hit your weekly limit · resets Oct 29 at 6am
  (America/Toronto)" before any model turn, so the trial is `not_run`
  (no model output). No trial of either arm has run.
- The qualification waits for the evaluator account's limit to reset or
  for the operator to supply another evaluator seat.
