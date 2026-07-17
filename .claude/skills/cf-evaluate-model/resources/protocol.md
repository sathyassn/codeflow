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
    "effort": "xhigh",
    "harness": "codex-app | codex-cli | claude-code",
    "harness_version": "actual version",
    "codeflow_revision": "commit",
    "settings_digest": "sha256:...",
    "permission_profile": "name",
    "tools": ["observed tool inventory"],
    "network": "effective boundary",
    "budget": {"turns": 0, "tokens": null, "wall_seconds": 0, "retries": 0}
  },
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
timestamp.

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

## Evidence and graders

Prefer outcome evidence over prose: plan files and their timestamps/digests,
tool events, git state, test/coverage output, rendered UI state, and permission
canaries. A model grader evaluates only the rubric dimensions that deterministic
checks cannot settle. Give it an `unknown` outcome when evidence is insufficient.
Calibrate model graders against human decisions and retain disagreements.

For the independent-plan case, record both plan digests and evidence that each
was completed before the first cross-exposure. Two summaries created after one
model saw the other's plan do not satisfy the requirement.

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
