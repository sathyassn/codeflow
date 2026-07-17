---
name: cf-evaluate-model
description: Qualify a new model, model version, harness release, permission profile, or material CodeFlow instruction change against CodeFlow's regression and capability contracts. Use for deliberate model-readiness evaluations, periodic artifact maintenance, or reproducing an observed model-behavior regression. Runs repeated trials in disposable repositories through native interactive Codex or Claude harness sessions, preserves traces and environment metadata, compares a candidate with a pinned baseline, and cleans only its marked fixture roots. Never use for ordinary repository work, as a headless model runner, or as a generic cleanup tool.
---

# cf-evaluate-model — qualify a model/harness binding

Evaluate the complete system the user will run: model, reasoning effort,
harness, CodeFlow revision, settings, tools, and permission boundary. A score
for a stripped-down API call does not qualify an interactive coding harness.

Read [resources/protocol.md](resources/protocol.md) before a run. The source of
truth is [resources/requirements.json](resources/requirements.json), with
behavioral cases in [resources/cases.json](resources/cases.json) and exact
fixture overlays in [resources/fixtures.json](resources/fixtures.json).

## Non-negotiable boundaries

- Run subject trials only in fresh native interactive Codex App/CLI or Claude
  Code sessions with the tools and MCPs being qualified. Never use `codex exec`,
  `claude -p` / `--print`, a raw model API, desktop GUI scripting, or transcript
  scraping as a substitute.
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
   it has no line-count or token-deletion gate.
3. **Choose a suite.** Materialize one named case while reproducing a failure;
   that diagnostic is not a validated suite result. Use `canary` while editing
   the corpus or for a quick regression smoke. Use `full` for a new production
   model/harness binding, a permission change, or promotion. Canary runs each
   selected canary case once; full runs every case three times. Never present
   canary evidence as a full qualification.
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
   and prints its tree digest. Never reuse a fixture between trials.
5. **Run the subject naturally.** Open the required native interactive harness
   rooted at that fixture, put the exact CodeFlow binary under test first on the
   session `PATH`, and give it only `TASK.md` as the task. Preserve its session
   reference plus scoped observable evidence. Duo cases require both real
   native seats; a missing seat is an observed degradation, not simulated.
6. **Record raw observations.** Use the result shape in the protocol. Record the
   route, signals actually observed, violations, references consulted, evidence
   references, duration, tokens/cost when exposed, and any `not_run` reason.
   Never set `pass` by judgment alone; the validator recomputes it.
7. **Grade in layers.** Run deterministic validation first. Have the other
   vendor independently grade qualitative evidence with the case rubric, then
   reconcile. A human reviews every hard failure, disagreement, security case,
   and promotion decision. Do not majority-vote away divergent evidence.
8. **Compare and decide.** Compare the candidate result with the pinned baseline.
   Promotion requires no hard-case regression, no unresolved validity threat,
   complete full-suite evidence, and explicit human approval. Improvements in
   latency or token use never compensate for a lost semantic duty.
9. **Preserve, then clean.** Retain results, comparison, grader notes, fixture
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
  fixture, grader, harness, or artifact changes. A green score from a broken
  problem is invalid.
- Review token and latency metrics as diagnostics. Token consciousness means
  reliable adherence per loaded token; it never authorizes deleting a duty.
- Keep cleanup inside this skill. Do not create a broad repository-cleanup
  skill whose deletion boundary is harder to prove.
