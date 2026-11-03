# TSK-445 native qualification: pre-registration

Registered before any trial launched, 2026-10-26T20:16:03Z.

- Kind: focused diagnostic of an instruction change (explanation-method pack
  cases), not a canary or full suite; one native trial per case and arm.
- Suite: sha256:8f39d76721c552debd496941313328865b4b0f995852b3ae54bf1f030becd15f at d9b5954e9,
  re-registered as sha256:7bd8b3eae964b65d4fc721fe80bb0a5eb0a3c05cc80917c244711251fa6a567b (confirmed
  unchanged at 201b471eb) after
  the review rounds (anchor wording, then the exact and judged split in the
  retry fixture's grading note). The
  candidate binary is rebuilt at the head the trials run on, and its digest
  is recorded here before launch.
- Cases: retry-review-opens-with-the-whole (CF-METH-004, CF-PRES-004) and
  migration-review-leads-with-the-picture (CF-METH-004, CF-PRES-004).
- One variable: the CodeFlow revision the fixture is scaffolded with.
  - Qualifying arm, candidate: 201b471eb (the TSK-445 head Codex approved),
    binary `codeflow 3.2.0 source=201b471eb4e82c89f35bf80037c821983ceef60b
    dirty=false`, sha256
    df38eedab280c7a6c924a92c73570960c1eb40953cba87210e9b92dfaf05b57f, built
    2026-10-27 into `target/qual/` before any trial. (The first registration
    named d9b5954e9; no trial ran on it.)
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
  in two parts. Exact: `evals/qualification/anchored/check.py`, run isolated
  (`python3 -I -S -B`) with the verified wheels pinned in
  `anchored/requirements.txt`,
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

## Historical launch outcomes (October 2026)

- Three launches were refused at readiness before any prompt: twice
  because Claude Code installed an update into the trial's disposable home
  at start (fixed: the kit now sets `DISABLE_AUTOUPDATER=1` for subjects),
  once because the runner's proven Claude footers do not include
  `acceptEdits` (the arm now uses `auto`).
- The fourth launch delivered the prompt to a ready seat. The evaluator
  account answered "You've hit your weekly limit · resets Oct 29 at 6am
  (America/Toronto)" before any model turn, so the trial is `not_run`
  (no model output). No trial of either arm has run.
- Those attempts produced no completed trial. Account availability must be
  observed again; the old quota incident does not establish a current limit.

## Resume cohort registered 2026-11-03

This registration supersedes the candidate and harness selections above for
new trials. Historical attempts remain `not_run`. This is a focused
instruction diagnostic, not qualification of a production model binding.

- Candidate source: `391fc4f75b5d9fe44d451f73279fa96cc8771a7d`.
- Candidate executable: `target/qualification-pins/391fc4f75/codeflow`, copied
  from the host-built `target/debug/codeflow` before this record changed.
  Version observed by this builder: `codeflow 4.0.0`, source as above,
  `dirty=false`, hook inputs
  `3258dafbdbda04572087e486f98e259c5cbc939b65efb38f076205a566d412f5`.
  SHA-256: `84fa0c01a48a57dc4455bb53d8f414cc23b5743578ed9e7a940422285f42b245`.
  The registration commit changes evidence only; it does not claim a rebuild
  of this pinned candidate. Later builds must not replace this pin.
- Suite: `sha256:7bd8b3eae964b65d4fc721fe80bb0a5eb0a3c05cc80917c244711251fa6a567b`,
  recomputed from the five shipped registry JSON documents. No external
  graded suite is selected.
- Requested subject: native Claude Code, `--model opus --effort high
  --permission-mode auto`, dedicated evaluator home `~/.codeflow-eval/claude`.
  The intended model is Opus 5.5. The alias is requested, not observed model
  identity; the host must record the native displayed identity and effort.
- Harness: `claude --version` reports `2.1.317` in this builder environment.
  The host must verify the same version at launch. This is a new cohort
  relative to the historical 2.1.294 attempts. Both comparison arms use the
  same harness, model, effort, tools and permission boundary.
- Seat: a fresh runner-created Herdr tab per trial, in the Director's live
  project workspace, never the Director pane. The runner names the subject
  `eval-<opaque-trial-id>` and delivers the materialized `TASK.md` after
  readiness. Herdr discovery was denied by this builder's sandbox, so no
  workspace or ready subject identity is claimed here.
- Comparison: retain source `0f08dd7f8` and executable SHA-256
  `70426e4eead8ff8f7bed53a7686154303f995c635fca2f408a7832540f066e49`.
  The host must locate and verify this executable before its arm runs.
  The 3.1.1 release executable for AC-8 is not this comparison baseline.
- Order: retry candidate, retry comparison, migration candidate, migration
  comparison. One fresh trial per case and arm, sequentially. Refused or
  interrupted attempts remain recorded; never silently replace a failure.
- Retry prompt: "Open a review page that explains how our job retries work,
  so I can sign off on the design."
- Migration prompt: "Open a review of the two migration options for me."
  The runner sends the generated `TASK.md` unchanged. Neither prompt gets
  the expected signals, grading rubric or a suggested document structure.
- Pass rule: the retry candidate opens with an anchor and an orienting
  figure naming the parts, carries the retry mechanism in a figure, uses
  plain section titles and introduces terms before use, has one ask, and
  states the unmeasured outage. No invented facts, question-form section
  title, definitions paragraph before the figure, table as main carrier or
  multiple asks. Apply the exact structural checker and the registered J1/J2
  meaning rubric at both widths, with rendered legibility inspected.
  Migration is graded against its registered comparison, downtime, cost,
  evidence, one-ask and uncertainty signals. Any unresolved validity threat
  prevents an acceptance claim; comparison results inform the contrast.
- Evidence: retain fixture and executable digests, native readiness and
  observed identity, the opened document revision, rendered views, finish
  observation, exact-check output and independent qualitative grading.
  The separate review seat also owes the method's editorial verdict.
- State: no trial in this cohort has launched. Host reports build, fmt,
  clippy and rustdoc passed at the candidate; tests paused before the first
  Cargo test. These passes are not behavioral qualification evidence.
