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

## Resume cohort re-registered 2026-11-03, after the line merge

This registration supersedes the 2026-11-03 candidate, suite and harness
selections above for new trials. The merge of the integration line changed
the suite and the embedded skills, so the earlier pin cannot qualify this
head. No trial of the earlier cohort ran. Everything else above stays in
force: requested subject, seat, comparison, order, prompts, pass rule and
evidence.

- Candidate source: `9e0ba03e266842b65290518ad1a16c5d9a9b2e08`, the merge of
  integration tip `fe66d4c627408656b982d341ddf1d0dada5dec19`.
- Candidate executable: `target/qualification-pins/9e0ba03e2/codeflow`, copied
  from the `target/debug/codeflow` built at that commit before this record
  changed. Version: `codeflow 4.0.0`, source as above, `dirty=false`, hook
  inputs `d84c012f36a202d8094dd883da1cc4c1a75e5f7aeea6b8344d2ee296392746af`.
  SHA-256: `2af8ab99879e4ef33d12f369e1c43e5b4016261b7ea2d14e36201d1997ffa4ba`.
  The commit carrying this record changes evidence only. Later builds must
  not replace this pin. A later change to the skills or the suite needs a new
  registration before any trial.
- Suite: `sha256:72104600edd82c7c151e6d373795b3121f4654ccf5f39db808054003cb61701e`,
  from `suite_digest()` in `eval_kit.py` at this commit.
- Harness: `claude --version` reports `2.1.318` in this builder environment.
  The host verifies the same version at launch.
- Comparison executable: source `0f08dd7f8`, SHA-256
  `70426e4eead8ff8f7bed53a7686154303f995c635fca2f408a7832540f066e49`, is not in
  this worktree's `target/`. It must be found and verified, or rebuilt and
  registered under its new digest, before the comparison arm runs.
- State: no trial in this cohort has launched.

## Cohort registered 2026-11-04, at the review fix head

This registration supersedes every candidate, comparison executable, suite
and harness selection above for new trials. The review fixes changed the
skill text and `requirements_version`, so the `9e0ba03e2` pin cannot qualify
this head. No trial of any earlier cohort ran. Requested subject, seat,
order, prompts, pass rule and evidence stay as registered on 2026-11-03.

- Candidate source: `c5de5a9743e1bc12cce1c0ea7f60a044106421a3`, which carries
  integration tip `9c5f665cf014b3e1aa3213c1fd6175db952d2b1c` and the review
  fixes.
- Candidate executable: `target/qualification-pins/c5de5a974/codeflow`, copied
  from the clean `target/debug/codeflow` built at that commit. Version:
  `codeflow 4.0.0`, source as above, `dirty=false`, hook inputs
  `d84c012f36a202d8094dd883da1cc4c1a75e5f7aeea6b8344d2ee296392746af`.
  SHA-256: `a896e6dd09a7b299a48ef5812e2f89f04a409577ed10ce72884957c96da9af54`.
- Comparison executable: the binary with SHA-256 `70426e4e...` registered
  earlier was not found on this host after the 2026-11-04 restart. It was
  rebuilt from source `0f08dd7f805f7efb8b04e04f180687e08236f481` in its own
  worktree and target. Version: `codeflow 3.2.0`, source as above,
  `dirty=false`, hook inputs
  `4d6bc8929722dfb1059d6735b22fa903d68d5070a2984a3a31c9a16a0f6a3eeb`.
  Pinned at `target/qualification-pins/0f08dd7f8/codeflow`, SHA-256
  `923439d2e68341151be6106b8671992eed532e209c2f0434c3de95271dfc8dcd`.
- Suite: `sha256:793e2a8ab5fa82811a9eb27f1523376012213c637d7ed88921777e8ed883e7cd`,
  from `suite_digest()` in `eval_kit.py` at the candidate.
- Harness: `claude --version` reports `2.1.318`.
- Commits after this registration change evidence only. A change to the
  skills or the suite needs a new registration before any trial.
- State: no trial in this cohort has launched.

### Launch attempts and the requested model, 2026-11-04

Two launches of the retry candidate arm were refused at readiness. No
prompt was delivered and no model turn ran. Both are `not_run`; their
evidence is kept on the host.

- Attempt 1: the Claude Code 2.1.318 trust dialog opened with "No, exit"
  selected, because the fixture pre-approves tool permissions. The runner
  sent Down and re-read after 0.2 s, but the selection had not moved yet,
  so it refused with "trust choice not visibly selected; no Enter sent". A
  Down sent to the same pane one second later moved it.
- Attempt 2: trust was accepted, and the seat showed Opus 5.5 with high
  effort and auto mode. The runner refused with "seat did not become
  ready". `claude_expectation` in `evals/qualification/runner.py` requires
  the shipped status line to show the requested `--model` value, which was
  the alias `opus`, while Claude Code shows the resolved id
  `claude-opus-5-5`. A trial requested with an alias cannot become ready
  while that status line is on screen.

Amendment before any further launch: the requested model is the exact id,
`--model claude-opus-5-5 --effort high --permission-mode auto`, in both
arms. It names the same intended model, Opus 5.5, and lets the status line
check compare the requested id with the one displayed. Everything else
registered above is unchanged.
- Attempt 3, with `--model claude-opus-5-5`: trust was accepted, the seat
  was ready and the prompt was pasted whole, but the runner refused with
  "the visible editor does not hold this prompt; no Enter sent". With input
  in the editor, Claude Code 2.1.318 draws the "ctrl+g to edit in Nvim" hint
  on the status line row, and `footer_rows` knows that hint only on the
  branch row or on its own row under the mode row. No Enter was sent and
  no model turn ran. The tab was closed with the prompt unsent.

No trial can run on Claude Code 2.1.318 until `runner.py` accepts that row
layout. The coord/catalog ruling of 2026-11-04 put that fix in this task.
Attempts 1 to 3 are `not_run`.
- Runner fix: `ef7209a9b` teaches `runner.py` both 2.1.318 frames, with
  tests on the captures from attempts 1 and 3. The candidate pin is
  unchanged, since no skill file or suite entry moved.
- Attempt 4, at 11:49 EST with the fixed runner: trust accepted, seat ready,
  prompt delivered with one Enter. The evaluator account answered "You've
  hit your weekly limit · resets 6am (America/Toronto)" before any model
  output. `finish` ran with no validity flags, and the tab was closed so
  the session could not resume unobserved at 6am. The trial is `not_run`.
  The retry candidate trial is next, as the first trial of this cohort,
  once the evaluator account has usage.
