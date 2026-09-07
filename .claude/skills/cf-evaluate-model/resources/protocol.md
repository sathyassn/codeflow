# Model qualification protocol

## What the result means

A run measures one declared model+harness configuration under one CodeFlow
revision and resource budget. It supports a regression, capability, safeguard,
or comparison claim only to the extent its fixtures and graders represent that
claim. It does not prove universal model quality.

Use regression cases for behavior that must remain nearly perfect. Use
capability cases to learn what a model can do and where it fails. Report both
per-trial pass rate and consistency across repeated trials; never hide variance
behind one aggregate.

Cases may name stable `roles`. Those tags identify the behavioral evidence that
qualifies a role; they never encode the current model name. Requested and
observed system identity proves the concrete binding separately. Recording a
stable primary role requires its tagged cases to pass.

## Result record

Store JSON with this logical shape. The validator rejects missing or extra full
trials and recomputes each status and the summary from observations.

```json
{
  "schema_version": 1,
  "run_id": "candidate-2026-07-17",
  "suite": "full",
  "suite_digest": "sha256:...",
  "system": {
    "model": "actual model version",
    "effort": "medium",
    "harness": "qualified id from harnesses.json",
    "harness_version": "actual version",
    "codeflow_revision": "commit",
    "settings_digest": "sha256:...",
    "permission_profile": "name",
    "tools": ["observed tool inventory"],
    "network": "effective boundary",
    "budget": {"turns": 0, "tokens": null, "wall_seconds": 0, "retries": 0},
    "observed": {
      "model": "model displayed by the harness",
      "effort": "effort displayed by the harness",
      "evidence": [{"kind": "session | tool | file | command | ui", "ref": "durable reference", "digest": "sha256:..."}]
    },
    "peer_seats": [{
      "seat": "claude | codex",
      "model": "fixed peer model",
      "effort": "fixed peer effort",
      "harness": "native harness",
      "harness_version": "actual version",
      "settings_digest": "sha256:..."
    }]
  },
  "experiment": {"variable": "system.effort", "baseline_run_id": "baseline run ID"},
  "trials": [{
    "case_id": "model-independent-plans",
    "trial": 1,
    "fixture_digest": "sha256:...",
    "outcome": "completed | error | not_run",
    "status": "pass | fail | error | not_run",
    "observed": {
      "route": "cf-model-orchestrator",
      "signals": ["claude_complete_plan_before_exchange"],
      "violations": [],
      "references": []
    },
    "evidence": [{"kind": "session | tool | file | command | ui", "ref": "durable reference", "digest": "sha256:..."}],
    "trace_ref": "retained scoped session or trajectory reference",
    "duration_ms": 0,
    "tokens": null,
    "cost": null,
    "validity_flags": []
  }],
  "summary": {
    "total": 0, "pass": 0, "fail": 0, "error": 0, "not_run": 0,
    "pass_rate": 0.0, "consistent_case_rate": 0.0,
    "categories": {}
  },
  "human_approval": {"reviewer": "", "reviewed_at": "", "decision": "pending | approved | rejected", "notes": ""}
}
```

`system.observed`, `system.peer_seats`, and `experiment` are optional for
schema-version 1 compatibility. Strict one-variable comparisons require
observed binding evidence and an experiment declaration. When requested and
observed model or effort differ, record `harness_context_mismatch`; that run is
evidence of a mismatch, never promotion evidence. A duo case records the
subject in the normal system fields and every fixed other seat in `peer_seats`.
Promotion additionally requires `system.observed`; a requested-only binding is
useful diagnostic evidence but cannot become a production qualification.

## Harness capability contract

`harnesses.json` is a source-controlled qualification catalog, not a plugin
registry or launch configuration. Every listed harness must satisfy all
contract capabilities: a native interactive session, native runtime
provenance, configured tool access, a scoped workspace, bounded failure,
recheckable results, an effective permission boundary, and the git backstop.
Adding a harness requires evidence for every capability. Catalog status is
not binding qualification: promoting a concrete binding still requires the
full model suite; a name in a vendor catalog or protocol handshake is
insufficient.
The catalog maps each capability to retained repository evidence, and suite
validation fails when a capability is unmapped or its reference is missing.
The scaffolded `harness-evidence.md` is the portable index of CodeFlow's dated
source canaries, decisions, tests, and platform limits; it is not live
installation evidence.

The catalog may name a code-allowlisted version-probe ID; it cannot supply an
executable or arguments. The probe lets `codeflow doctor` compare an externally
visible installed version with an approved binding. Adding another live probe
requires a reviewed binary change and tests. A harness without such a surface
remains explicitly unobservable and needs a native canary; doctor never
launches a model merely to turn that unknown into a pass.

## Diagnostic packs

`packs.json` groups existing cases and may include other acyclic packs.
`list-cases --pack <id>` resolves an ordered, de-duplicated case list for
focused diagnosis or pre-release smoke work. Packs do not define new graders,
weaken a case, or create a promotion shortcut. Only the complete `full` suite
with three trials per case may qualify a binding.

## Promoted binding record

`record-binding` accepts only a human-approved full result with every hard case
passing, no error/not-run trial, no validity flag, and native evidence that
requested and observed model/effort match. It derives provider and lineage from
the capability-supported harness catalog and writes only:

- binding ID and eligible roles;
- requested and observed model/effort plus evidence content digests;
- harness/version, CodeFlow revision, suite/result digests, and approval;
- optional absolute settings-source paths and their current content digests.

The user-owned record belongs under
`${CODEFLOW_HOME:-~/.codeflow}/qualified-bindings/`; it is not scaffold state,
runtime routing config, or permission to auto-promote a newer model. Do not put
prompts, trace bodies, settings contents, secrets, tokens, or credentials in
it. A requested/observed mismatch fails closed. Harness or settings drift
requires a new native evaluation and human approval.

`status` is derived:

- `error` or `not_run` follows the trial outcome.
- Otherwise, `pass` requires an allowed route, every required signal, every
  required reference, no prohibited signal, no recorded violation, retained
  evidence, and an empty validity-flag list.
- Any other completed trial is `fail`.

Every evidence item requires a content digest. An `error` record requires an
`error_message`; a `not_run` record requires a `not_run_reason`. These outcomes
remain visible and never count as passes.

Validity flags use the tool's declared vocabulary and represent unresolved
threats; any entry blocks a pass. Promotion validation accepts only a full
suite and an `approved` decision with a nonempty human reviewer and review
timestamp. It also requires every hard-linked trial to pass, no validity flags,
and no `error` or `not_run` outcomes.

Accepted flags are `ambiguous_task`, `baseline_contamination`,
`budget_exhaustion`, `broken_fixture`, `evaluation_awareness`,
`grader_false_negative`, `grader_false_positive`, `grader_material_exposed`,
`harness_context_mismatch`, `missing_trace`, `refusal`, `reward_hacking`,
`retry_contamination`, `reused_session`, `sandbagging`,
`unavailable_fixture_tool`, and `unresolved_grader_disagreement`. Add a new
flag to the protocol and validator together; an unknown spelling is invalid.

The validator proves structural consistency and expected-vs-observed scoring;
it cannot prove that a cited trace is genuine. That requires independent trace
inspection and, for promotion, human approval.

Use `score <result.json>` to recompute statuses and the summary from raw
observations. It prints to stdout by default. Persist only through explicit
`--output <path>` or `--in-place`; output writes are atomic, and `--output`
cannot alias the input.

## Evidence and graders

Prefer outcome evidence over prose: plan files and their timestamps/digests,
tool events, git state, test/coverage output, rendered UI state, and permission
canaries. A model grader evaluates only the rubric dimensions that deterministic
checks cannot settle. Give it an `unknown` outcome when evidence is insufficient.
Calibrate model graders against human decisions and retain disagreements.

For the independent-plan case, record both plan digests and evidence that each
was completed before the first cross-exposure. Two summaries created after one
model saw the other's plan do not satisfy the requirement.

### Rendered design comparisons

A rendered design capability case evaluates a real surface, not a prose
description of one. Preserve the subject's settled `DESIGN_INTENT`, source
revision, interaction evidence, console/network evidence, and same-environment
screenshots at the case's named viewports. Automated accessibility results are
one evidence layer and never stand in for complete conformance.

Compare a candidate with a pinned baseline produced from the same case,
revision, harness capabilities, budget, and operator constraints. Randomize the
artifact labels and give the grader neither model identity nor which artifact
is the candidate. The grader returns `candidate regression`, `no material
regression`, or `unknown` against a fixed rubric:

- brief, audience, and user-job fit;
- information and interaction hierarchy;
- typography, colour, spacing, imagery, density, and motion coherence where
  applicable;
- responsive behavior and required state coverage;
- accessibility evidence against the named target;
- fidelity to each artifact's settled design intent; and
- whether familiar design choices were retained or revised from the artifact's
  evidence rather than accepted or rejected by category.

Use paired judgments to detect regression; never report an absolute aesthetic
score or a universal design ranking. A cross-lineage grader challenges the
first judgment, and disagreement remains `unknown` until inspected by the
human reviewer. Keep every predeclared trial, including weak outputs; selecting
only the best of several generations is reward hacking. A baseline must be
re-run when the brief, fixture, rubric, rendering environment, or instruction
revision changes.

## Validity checks

Inspect and report:

- broken or ambiguous tasks and unavailable fixture tools;
- reward hacking or marker parroting without the required outcome;
- evaluation awareness or access to grader material;
- refusals, sandbagging, retries, and budget exhaustion;
- contamination from prior fixture history or reused sessions;
- harness differences in context, compaction, tools, permissions, or network;
- grader disagreement and false positives/negatives.

Do not repair a task and silently compare its result with the old baseline.
Version the suite and re-run the baseline under the new task.

## Promotion bar

A production binding needs a full run, three trials per case, no missing hard
case, no hard regression from the pinned baseline, no unresolved security or
validity finding, and explicit human approval. Record capability improvements
separately. A candidate may be useful for a narrower role even when it does not
replace the production binding; document that scope rather than averaging away
the failure.

For a single-variable promotion, put the same `experiment.variable` in both
results and point the candidate's `baseline_run_id` at the baseline run. Compare
with `compare --variable <system.field>`. This mode requires requested bindings
to match observed harness evidence, rejects drift in every other system field
including peer seats, and blocks on any case regression. It reports
min/median/max and totals for duration, tokens, and cost; incomplete telemetry
is `n/a` and never estimated. Efficiency improvements cannot offset a semantic
regression.

Scope every comparison claim to this suite, harness, settings, tools, budget,
and three-trial sample. Report per-case pass rate and consistency, not a
universal ranking or one lucky point result. A mixed result triggers more
predeclared trials or a narrower qualified role; it is not rounded into a win.
After human promotion, adoption uses an event-based probation over reviewed
orchestrated tasks or pull requests. A material regression rolls back the
project selection to the prior binding and preserves the evidence for
requalification.

## References

- Anthropic, “Demystifying evals for AI agents” — tasks/trials/graders/traces,
  repeated trials, deterministic+model+human layers, regression versus
  capability suites, transcript inspection, and long-term maintenance.
- OpenAI, “A shared playbook for trustworthy third-party evaluations” — record
  the claim, harness, tool access, resource budget, elicitation method, and
  validity threats; harness choice is part of the evaluated system.
- OpenAI, “How evals drive the next chapter in AI for businesses” — define
  contextual success, include costly edge cases, and retain expert calibration
  of automated graders.
