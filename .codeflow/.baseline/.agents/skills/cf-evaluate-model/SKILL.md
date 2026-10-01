---
name: cf-evaluate-model
description: Qualify a model, harness release, permission profile, or material CodeFlow instruction change against CodeFlow's regression and capability contracts. Use for model readiness, periodic suite maintenance, or reproducing an observed behavior regression. Runs repeated native-interactive trials in disposable fixture repositories, keeps traces and environment metadata, compares with a pinned baseline, and cleans only its marked fixture roots. Never use for ordinary repository work, as a headless model runner, or as a generic cleanup tool.
---

# cf-evaluate-model: qualify a model/harness binding

Evaluate the complete system the user will run: model, reasoning effort,
harness, CodeFlow revision, settings, tools, and permission boundary. A score
for a stripped-down API call does not qualify an interactive coding harness.

Read [resources/protocol.md](resources/protocol.md) before a run. The source of
truth is [resources/requirements.json](resources/requirements.json), with
behavioral cases in [resources/cases.json](resources/cases.json) and exact
fixture overlays in [resources/fixtures.json](resources/fixtures.json).
[resources/harnesses.json](resources/harnesses.json) owns the minimum native
harness capability contract and capability-supported harnesses; diagnostic compositions
live in [resources/packs.json](resources/packs.json). The compact
[harness evidence index](resources/harness-evidence.md) preserves the basis and
limits of catalog support in consuming scaffolds.

## Non-negotiable boundaries

- Run subject trials only in fresh native interactive Codex App/CLI or Claude
  Code sessions with the tools and MCPs being qualified. Grok Build (`grok`
  TUI) is the subject harness when qualifying `grok-cli`; a catalog entry is
  not a completed Grok full-suite qualification. Never use `codex exec`,
  `claude -p` / `--print`, `grok -p` / `--single`, a raw model API, desktop GUI
  scripting, or transcript scraping as a substitute. Promotion of
  `grok-engineering-primary` still requires that native-interactive full result.
- Materialize only disposable fixtures. Never point a trial or cleanup command
  at a real repository, production service, private remote, or live secret.
- Mock credentials, destructive effects, external writes, and private services.
  Public research may be live when the case tests it and the run records that.
- Keep the subject blind to expected signals and graders. The materializer
  removes this evaluation skill from the fixture before reinitializing history.
- Do not let a model self-report become proof. Grade observable repository
  state, tool events, commands, rendered UI state, and retained session evidence.
- Evidence outranks the aggregate score. Any hard-contract regression, invalid
  fixture, missing trace, or unresolved grader disagreement blocks promotion.

## Run

1. **Freeze the comparison.** Record the baseline and candidate model/version,
   effort, harness/version, CodeFlow commit, settings digest, permission profile,
   tool inventory, network boundary, retry/turn/time budget, and suite revision.
   Change one comparison variable at a time or declare the confound.
2. **Validate the suite.** From the project root run:

   ```text
   python3 .agents/skills/cf-evaluate-model/scripts/eval_kit.py validate-suite
   ```

   A source checkout may use the equivalent `assets/base/...` path. Validation
   requires every hard requirement to have source markers and behavioral cases;
   it has no line-count or token-deletion gate. It and `model-eval-kit` are
   structural, not behavioral, checks.
3. **Choose a suite.** Materialize one named case while reproducing a failure;
   that diagnostic is not a validated suite result. Use `canary` while editing
   the corpus or for a quick regression smoke. Use `full` for a new production
   model/harness binding, permission change, or promotion. Canary runs each
   selected canary case once; full runs every case three times. Never present
   canary evidence as a full qualification. A focused diagnostic (an
   instruction change's pack) resolves `list-cases --pack <pack-id>` and
   runs natively with graded traces, never as `full`. Packs may compose
   other packs, but even `release-smoke` is not a promotion suite.
   Internal-route qualification pre-registers cases and fixes qualifying versus
   comparison arms before launch. Require three fresh accepted trials per case
   and qualifying route/harness/selector/effort/workload tuple; retain attempts,
   integration, cross-family review, trace, and overhead. A failed qualifying
   acceptance, absent observed route/trace, invalid
   control/fixture, or unresolved validity threat leaves it candidate.
   Comparisons inform claims but do not gate the qualifying tuple. It covers
   evidenced tuples, not primary binding, universal reliability, or economy.
4. **Materialize each trial.** Use an explicit temporary run root and the exact
   CodeFlow binary under test:

   ```text
   python3 .agents/skills/cf-evaluate-model/scripts/eval_kit.py materialize \
     --run-root <temporary-path> --case <case-id> --trial <n> \
     --codeflow <path-to-codeflow>
   ```

   Each command creates an opaque trial path with a neutral `repository`
   basename, applies exact registry overlays, keeps evaluator-only state outside
   the subject tree, removes grader material, rebuilds a one-commit history,
   and prints its tree digest. Keep declared remote/cleanup controls in the
   marked workspace; never use an external remote or reuse a fixture. Fake
   endpoints follow [bounded effects](resources/fake-effects.md), retain both
   digests, and hide owner state.
5. **Run the subject naturally.** Open the native interactive harness in the
   fixture and give only `TASK.md`, or a scripted case's turns in order, as
   the task. Match actual CodeFlow and hook executables to the external
   receipt per [protocol](resources/protocol.md); `PATH` or version is not
   proof. Preserve session and scoped evidence. Duo
   cases require real native seats; a missing seat is observed degradation, not simulation.
6. **Record raw observations.** Use the result shape in the protocol. Record the
   route, signals actually observed, violations, references consulted, evidence
   references, duration, tokens/cost when exposed, and any `not_run` reason.
   Never set `pass` by judgment alone; use `score` to recompute derived fields.
7. **Grade in layers.** Run deterministic validation first. Have the other
   vendor independently grade qualitative evidence with the case rubric, then
   reconcile. A human reviews every hard failure, disagreement, security case,
   and promotion decision. Write grader notes plainly: simple,
   straightforward and clear, no mannered prose (see
   `.codeflow/rules/writing.md`). Do not majority-vote away divergent evidence.
8. **Compare and decide.** Compare the candidate result with the pinned baseline.
   For a controlled promotion, declare one variable and use `compare --variable`.
   Promotion requires no hard-case regression, no unresolved validity threat,
   complete full-suite evidence, and explicit human approval. Improvements in
   latency or token use never compensate for a lost semantic duty.
9. **Record an approved binding.** After a full result passes promotion
   validation, write a compact local record outside the repository:

   ```text
   python3 .agents/skills/cf-evaluate-model/scripts/eval_kit.py record-binding \
     <approved-result.json> --binding-id <id> --role primary --role reviewer \
     --settings-file <effective-settings-file> \
     --output "${CODEFLOW_HOME:-$HOME/.codeflow}/qualified-bindings/<id>.json"
   ```

   Add only roles the evidence qualifies. The record retains requested and
   observed model/effort, content digests, harness metadata, and approval, not
   prompts, settings contents, credentials, or arbitrary trace text. Stable
   primary roles additionally require their role-tagged behavioral cases to
   pass. A project adopts an approved binding only by referencing its ID from
   `.codeflow/model-selection.json`; never copy the raw selector. Run
   `codeflow doctor --check model-bindings`; harness-version or declared
   settings drift requires requalification, while live model/effort remains a
   native-session observation.
10. **Preserve, then clean.** Retain results, comparison, grader notes, fixture
   digests, and permitted session references outside the fixture root. Cleanup
   requires the materializer's marker and an exact confirmation string; it
   refuses unmarked paths, symlinks, roots, repositories, and mismatched IDs.

## Maintain

- Add a regression case after an observed material failure and link it to a
  stable requirement ID. Include a paired case when needed to prevent
  over-triggering.
- Keep capability cases difficult enough to provide signal; graduate saturated
  cases into the near-100% regression set instead of deleting history.
- Re-run the reference solution and inspect sample traces whenever a case,
  fixture, pack, grader, harness, or artifact changes. A green score from a broken
  problem is invalid.
- Review token and latency metrics as diagnostics. Token consciousness means
  reliable adherence per loaded token; it never authorizes deleting a duty.
- Treat an effort-policy change as a controlled binding experiment: prove the
  requested and observed candidate effort named by the current ensemble, hold
  every other subject and peer field fixed, and reject any loss of a duo duty
  regardless of cost savings.
- Keep cleanup inside this skill. Do not create a broad repository-cleanup
  skill whose deletion boundary is harder to prove.
