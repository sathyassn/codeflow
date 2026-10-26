# TSK-445 native qualification: pre-registration

Registered before any trial launched, 2026-10-26T20:16:03Z.

- Kind: focused diagnostic of an instruction change (explanation-method pack
  cases), not a canary or full suite; one native trial per case and arm.
- Suite: sha256:8f39d76721c552debd496941313328865b4b0f995852b3ae54bf1f030becd15f at d9b5954e9,
  re-registered as sha256:a42fceb623ca4bed5d8ac126fa3a9e0601019d241d4eb4dc63cf31a535c18dd8 after
  the round 2 grading fixes (anchor wording in the fixture notes). The
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
  graded by `evals/qualification/anchored/check.py` under the parser pinned
  in `anchored/requirements.txt`. The checker settles the structure (anchor,
  sections, nested asks, titles) and the retry facts exactly: the 30 and 60
  second backoff and the 5 minute timeout in any usual notation, the
  dead-letter step after the third failure, the lost lease, the requeue, and
  the outage named as not measured. A judged pass by the evaluator, Claude
  Opus 5.5, against the fixture's grading note covers what it cannot: the
  migration facts (downtime, cost, evidence), any fact the retry figures
  draw beyond those, and whether each figure is legible. The other-lineage
  grading pass is owed to the primary.
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
