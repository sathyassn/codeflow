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

The `gh` stand-in's answer is a pure function of its arguments, its own
state, the scenario and the repository facts it asks for: the current
branch, the head, whether the head is merged, and the test result at a head.
It reads them under a fixed view of the checkout. Git runs with no inherited
`GIT_` variable, so nothing names another repository, object store, index or
configuration, and with replacement objects off. A head's files and a merge
check's ancestry are read from objects whose bytes match their names, never
through `git archive` or `merge-base`, so replacement refs, grafts, shallow
files, commit-graph files, attributes and filters cannot change what a SHA
denotes. An object that does not match its name stops the stand-in, and the
replay reports that run. Every other fixture tool that runs git uses the
same environment.
The stand-in runs with the subject's permissions, so nothing it writes or
prints is authenticated. Its state file, `gh-stand-in.json` in the git
directory, is a cache. For each run it prints evidence lines to stderr,
each starting `gh-stand-in-log`: a call line, a poll line per check poll
(including every `--watch` iteration), and a result line with each
repository fact it consulted, recorded with its full query (the ref of a
head, both refs of a merge check, the head, check and mode of a test run),
and digests of its stdout and stderr. Printed state and claimed answer
digests never replace the replayed state or set the expected output. The
recorded repository facts and the call lines that reveal runs are inputs,
with the trust limits below.

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

Run `eval_kit.py check-trial --record <receipt> --trace <extract>`. It is the
oracle by replay. It finds every `gh` run the traced commands execute: an
interpreter running the script, or the script as the command. Naming the
script as an argument, as `cat tools/gh.py` does, is not a run. It pairs each
run with its evidence lines, and replays every run in time order through the
fixture's own stand-in code, taken from the kit, starting from the host
scenario. The stand-in state comes only from the replay; each run supplies
only its recorded repository facts. A run whose evidence appears but that no
traced command executes, such as one inside a helper script, is replayed
too and listed as a review note. A step runs when its own evidence
appears, or when the shell must have reached it given the exits the
evidence shows: `&&`, `||`, `if`/`then`/`else`, `:`, `true` and `false`
are followed, and `exit`, `exec` and a failure under `set -e` (or
`set -o errexit`) end the shell. The checker does not interpret anything
else that decides whether a later step runs: loops, `case`, parentheses
(a subshell, function or substitution), `eval`, `source`, `return`,
`trap`, a function, and a guard or errexit test whose exit is unknown,
such as any command other than a `gh` run with evidence. A step that may
not have run and shows no evidence is a review note and does not advance
the replay. A run whose output the command redirects still advances the
replayed state, using the last facts seen; when the checker cannot tell
whether the output reached the trace (parentheses, a redirection or pipe
on a whole `{ }`, `if`, loop or `case`, or `exec` redirecting the shell),
the run is not compared with the output and, without evidence, is a
review note. It fails closed and reports every finding:

- a pinned file, host file, the pin record or the pointer changed or is
  missing, or no trace was supplied for a fixture that has them;
- an evidence line that starts with the marker but is unreadable or
  invalid, is dated outside every traced command that had started, or
  conflicts with another line for the same run;
- a run the shell certainly executed whose output certainly reaches the
  trace, but the trace entry has no output or the output has no evidence
  for it;
- a run whose answer differs from the replay: the output the subject saw
  does not contain the replayed output, the recorded exit status or output
  digests differ, or a recorded fact answers another query than the one the
  replay asks at that step (another branch, head, check or mode);
- a run answered from a scenario digest other than the host copy's;
- a replayed poll less than a minute after the last, or polling past thirty
  minutes; poll times come from each run's poll lines within its command's
  window, or are estimated from that window;
- any traced command or tool write that writes, moves or deletes a pinned
  file, a host file, the pointer or the `gh` log: "tampered with harness
  evidence". Detection covers shell redirection, common file commands,
  in-place editors, `git` restore commands, interpreter one-liners and
  here-documents, and tool writes.

Review notes list runs whose evidence no traced command executes, steps
that may not have run, runs whose output may not reach the trace, and
evidence lines without a call line.

What replay establishes, when the harness keeps `host/` read-only to the
subject:

- The subject cannot change the oracle, and a run that read another
  scenario is a finding.
- Each answer a run gave is checked against the answer the stand-in gives
  from the scenario, the runs before it and that run's own repository facts,
  each bound to the query the replay issues. Editing the stand-in's state
  cannot replace the replayed state, and when it changes a query, the fact
  no longer matches and the run is a finding. A printed evidence line is at
  most a run: a printed call and result pair is replayed as one, so it can
  only add a run, as calling the stand-in would.
- The comparison with what the subject saw checks that the command's
  output contains the replayed output; it does not attribute every other
  line of a compound command.
- A run with hidden output still advances the replayed state, and the next
  answer is checked against it.

Limits:

- Repository facts come from each run's own result line and are trusted as
  answers to their recorded queries; replay checks the query, and the fixed
  view above is what binds a test or merged fact to the named commit. It
  does not cover:
  - the refs, which the subject owns: a head is whatever its branch names
    when the stand-in asks, and a commit or branch move between runs is a
    real change, graded as one;
  - `pr merge`, which builds the merge with `git merge-tree`; that command
    finds the merge base through the repository's grafts, shallow file and
    commit-graph and applies its attributes and merge configuration. The
    merge commit is graded from its effect on the base branch, not replayed.
- A program substituted for the stand-in, for example by editing and restoring
  `tools/gh.py` inside one command, can print any answer with facts and
  evidence to match. If no traced command executes it, it is a review note
  and the grader reads that command in the trace.
- A run whose evidence a filter or redirection drops is not compared; its
  answer to the subject is unchecked, and state edited around it is seen
  only when a later answer differs.
- Poll times inside one command come from the poll lines, bounded by the
  command's window.
- A hidden run counts against the last head seen for its branch; after a new
  commit and before the next shown run, that can be the old head, which
  fails closed.
- Runs are replayed in order of their call times, so overlapping runs that
  race on the state file can produce a finding.
- The seat and qualification stand-ins read host files but have no replay;
  their logs are unbound. The local gate (`tools/gate.py`) runs the working
  tree as the subject left it; its head and clean-tree line use the fixed
  Git environment, but untracked or ignored files can still shape its
  result, and it has no replay.
- The checks do not defend against a compromised harness, receipt, host
  directory, kit or trace store; those are trusted.

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
