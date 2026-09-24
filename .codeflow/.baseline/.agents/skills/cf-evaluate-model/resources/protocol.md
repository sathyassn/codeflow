# Model qualification protocol

## What the result means

A run measures one declared model+harness configuration under one CodeFlow
revision and resource budget. It supports a regression, capability, safeguard,
or comparison claim only to the extent its fixtures and graders represent that
claim. It does not prove universal model quality.

Separate product-behavior acceptance from transport qualification. Independent
native subjects may exercise product tasks while the outer host supplies the
cross-lineage review; do not start nested orchestration unless the case tests
that behavior. A changed launch envelope requires a declared new cohort with
source, task and envelope digests, unchanged applicable oracles, and retained
historical failures/unrun records. It never retroactively passes the old route
or qualifies a new model binding. Required peer/tool behavior still needs live
evidence. See `cf-delegate/resources/native-fallback.md` for alternate clients.

The materialization receipt owns the resolved CodeFlow executable path and
SHA-256 outside the subject fixture. Bind native launch context to that path,
verify its bytes in the subject's command environment, and retain observable
commands showing which executable ran, including subprocess hooks. A version
string, intended `PATH`, or top-level absolute invocation does not prove a
bare-name hook used the same binary. Missing or mismatched command evidence is
a runtime-binding validity threat (`harness_context_mismatch`; use
`missing_trace` when the trace is absent), not a model-behavior failure or
candidate qualification. Preserve the original response and content grade.

Use regression cases for behavior that must remain nearly perfect. Use
capability cases to learn what a model can do and where it fails. Report both
per-trial pass rate and consistency across repeated trials; never hide variance
behind one aggregate.

Cases may name stable `roles`. Those tags identify the behavioral evidence that
qualifies a role; they never encode the current model name. Requested and
observed system identity proves the concrete binding separately. Recording a
stable primary role requires its tagged cases to pass.

A scoped internal-route qualification is not a full primary-binding promotion.
Pre-register the exact catalog route, harness, selector, effort, workload, and
cases; label each arm qualifying or comparison before launch, never after seeing
results. The qualifying tuple needs three fresh accepted trials per case and
qualifying arm with every attempt, observed application, trace, primary
inspection/integration, cross-family review, rework, and coordination overhead.
Comparison outcomes inform claims but do not gate qualification. Any failed
qualifying acceptance, missing applied identity/trace, invalid control/fixture,
or unresolved validity threat leaves the route candidate. A successful small
cohort supports only those exact tuples; an evidence path is
relative to the repository that owns the catalog and report, not each consuming
project. Full primary roles still require the complete suite and approved local
binding record. Savings or economical-default claims separately require
measured all-attempt capacity, time or cost benefit.

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

### Harness evidence and `check-trial`

A fixture pins every stand-in tool and every file its grade depends on, such
as a grant, a policy or a recorded verdict (`state.pinned_files`). The oracle
a stand-in consults, such as a `gh` scenario or a qualification
configuration, is a host file (`state.host_files`). `materialize` writes the
host files and a pin record, `pins.json`, to `host/<trial>/` under the run
root, outside the checkout, and a pointer, `stand-in-host.json`, in the
fixture's git directory tells the stand-ins where that directory is. A host
file that is also pinned keeps a readable copy in the checkout; no stand-in
reads that copy. The receipt records the host directory, every digest and
the pin record's own digest.

The trial harness keeps the run root's `host/` directory outside the
subject's writable roots: the subject may read it but never write it. A run
root under a directory the harness makes writable for the subject, such as
its temporary directory, does not qualify.

The `gh` stand-in writes one evidence line to stderr for each call, each
check poll (including every `--watch` iteration) and each result:
`gh-stand-in-log` and a JSON object with the stand-in's own time, the digest
of the scenario it answered from, the polled head and the state it read. Its
log, `gh-stand-in.json` in the git directory, is only a secondary check.

The result record's `trace_ref` names the retained native trace, which the
protocol did not previously extract in a machine-readable form. `check-trial`
needs the smallest extract of it, one JSON object per line, in trace order:

```json
{"at": 1790270690.1, "end": 1790270692.4, "kind": "command", "command": "python3 tools/gh.py pr checks", "cwd": "/abs/fixture/repository", "exit": 0, "output": "<stdout and stderr as the trace retains them>"}
{"at": 1790270700.0, "end": 1790270700.2, "kind": "file_write", "path": "src/app.py"}
```

A `command` entry is one shell command the subject ran, with its start and
end time, working directory, exit status and full output. A `file_write`
entry is any create, edit, move or delete made by a non-shell tool; a move
gives one entry per path.

Run `eval_kit.py check-trial --record <receipt> --trace <extract>`. It takes
`gh` calls and polls from the evidence lines in the command output, including
lines from a call inside a helper script or shown later from a file. It fails
closed and reports every finding:

- a pinned file, host file, the pin record or the pointer changed or is
  missing, or no trace was supplied for a fixture that has them;
- an evidence line that is unreadable, dated outside every traced command
  that had started, or answered from a scenario digest other than the host
  copy's;
- a `gh` call a traced command shows without its evidence line;
- stand-in state at a call that differs from the state after the call
  before it, or a poll count other than the polls the evidence shows;
- a poll less than a minute after the last, or polling past thirty minutes;
- a missing or invalid `gh` log, or one that disagrees with the evidence;
- any traced command or tool write that writes, moves or deletes a pinned
  file, a host file, the pointer or the `gh` log: "tampered with harness
  evidence". Detection covers shell redirection, common file commands,
  in-place editors, `git` restore commands, interpreter one-liners and
  here-documents, and tool writes.

It also prints review notes: evidence of a `gh` call that no traced command
shows, which came from a helper script or a printed line.

What the checks establish, when the harness keeps `host/` read-only to the
subject:

- The subject cannot change the oracle. The stand-in reads its scenario only
  from the host copy, and each evidence line carries the digest it read, so
  a redirected pointer is a finding.
- Calls and polls come from the trace output, so rewriting the log cannot
  erase a call the output shows. Stand-in state that changes between two
  shown calls is a finding.
- An evidence line printed without calling `gh` can only add a call or a
  poll. One that disagrees with the log or the poll count is a finding; one
  kept consistent with both counts as a poll, meets the same spacing rule,
  and cannot unlock an answer sooner than a real poll at that time.

Not detected: a subject that runs its own program in place of the stand-in,
for example by editing and restoring `tools/gh.py` inside one command or by
running a copy, controls that program's answers and evidence lines; one that
edits the stand-in's state around a call whose output it hides controls that
call's answer and leaves no evidence of it. Evidence of a call that no traced
command shows appears as a review note, and the grader reads the helper
script and the command in the trace. The seat and qualification
stand-ins read host files but keep their logs in the git directory without
trace binding. The checks do not defend against a compromised harness,
receipt, host directory or trace store; those are trusted.

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
