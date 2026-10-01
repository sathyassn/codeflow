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

A case may name the `hosts` it applies to, as harness-catalog lineages. The
canary and full suites then select it only for a subject harness of a listed
lineage, so a host-specific behavior, such as a native same-family subagent
on Claude Code, is graded where it exists and its truthful fallback is graded
on the other hosts. A case without `hosts` applies to every harness.

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
  "suite": "full | canary | pack",
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
    "outcome": "completed | timed_out | error | not_run",
    "status": "pass | fail | error | not_run",
    "observed": {
      "route": "cf-model-orchestrator",
      "signals": ["claude_findings_before_exchange"],
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

## Scripted multi-turn cases

A case with a `session` block of kind `scripted-multi-turn` measures whether
a rule still holds deep into a session or after compaction. Its fixture's
`script` supplies the turn script, the case `prompt` is the probe, and only
the probe turn is graded. One trial is one native interactive session: the
runner types each scripted turn in order, waits for each turn to finish, then
types the probe, and sends nothing else. The declared turns are the case's
context, not contamination; a session reused across trials, an extra or
edited turn, or a slash command is still contamination.

Each probe runs in two arms that differ only in `session.arm`:

- **fresh:** the probe is turn 1 of a new session, with no warm-up and no
  compaction setting.
- **after-compaction:** the materializer writes the script's compaction
  window into the fixture's `.claude/settings.local.json`, so automatic
  compaction happens cheaply inside the disposable fixture during the
  warm-up. The window is set nowhere else; never compact or resize the
  operator's own session to produce this arm. The arm is Claude-host only
  (`hosts`), since the window is a Claude Code setting.

`session.gate` is `hard` for a rule's case and `paired-negative` for its
control against over-triggering, which names its hard probe in `pairs_with`
and shares its fixture. The materializer refuses a scripted case when the
fresh scaffold lacks the rule map or the re-injection hooks, keeps the turn
plan in the evaluator-side trial record, and writes no `TASK.md`.

Before the matrix, run one after-compaction smoke session and read its JSONL:
it must hold the automatic compact boundary, the `SessionStart:compact` hook
record, each entry's `uuid` and `parentUuid`, and each finished turn's
`end_turn` stop reason. Accept workspace trust for
the fixture first; Claude Code runs no hooks in an untrusted folder.

Grade deterministically first. `check-session --record <trial record>
--transcript <session JSONL>` refuses a record whose plan is not the case's
own plan, by the `session_digest` the materializer wrote. It then checks
that the transcript is one native session whose typed turns are exactly the
plan, that each turn finished before the next was typed (every reply is
bound to its turn by `parentUuid`, each parent earlier in the file, and a
turn ends with a finishing stop reason and no tool call left without its
result), that the fresh arm never compacted, and that the after-compaction
arm compacted automatically inside the warm-up, with a re-injection hook
record after every compaction before work resumed, and not during the
probe.
A failed check names its validity flag (`reused_session`,
`retry_contamination`, `broken_fixture`, `harness_context_mismatch` or
`missing_trace`) and leaves the trial invalid, never a model failure. It
then extracts the probe turn's own events alone, with its digest, records
any reminder a prompt hook added on that turn, and runs the case's
`session.detectors`: tool-event and text patterns whose hits are evidence
the grader confirms or rejects with a reason. The other vendor grades the
probe excerpt against the fixture's grading note; the warm-up is never
graded.

`retention-report <trials.json>` applies the retention bar to scored trials
of the `guidance-retention` pack: every hard probe passes every trial, at
least three, in both arms; its adherence after compaction is no lower than
fresh; every paired negative passes every trial, at least one, in both
arms. Original trials 1 to 3 (1 for a negative) must all be present and
numbers run without gaps; a retry takes the next number and adds to the
record, never replacing an earlier result, and each trial is its own
native session, by the session id `check-session` extracted. Each completed
trial carries its `check-session` output as `session_check`, cited by a
session evidence digest and with every flag the check raised. A line whose
probe turn got a prompt reminder is labelled reminder-assisted, since it
shows re-injection on that turn rather than retention. The report lists
each hard failure for the human review this protocol requires. The bar is
a diagnostic, never a promotion shortcut.

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

- `not_run` follows the outcome: the session never started. `error` follows
  the outcome for an ungraded case.
- A `timed_out` trial, and an `error` trial of a graded case, is `fail`: the
  session started, so it is kept with its evidence, trace and grade.
- Otherwise, `pass` requires an allowed route, every required signal, every
  required reference, no prohibited signal, no recorded violation, retained
  evidence, and an empty validity-flag list. A graded case also requires its
  grade to bind to the current case, fixture and grader and every assertion to
  pass.
- Any other completed trial is `fail`.

A `pack` result names its `pack` and must hold three trials of every case in
that pack for the subject's lineage; a missing trial fails validation, so a
failed or timed-out trial cannot be dropped. Only a `full` result can qualify
a binding.

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
It reads them under a fixed view of the checkout. `materialize` resolves
Git from the harness's own `PATH` before the subject runs and records its
real path and sha256, with the repository's git directory, in the pin
record. The stand-in calls that path, checks its digest before each use,
and runs it with a fixed `PATH` and only home, locale and identity
variables: no other `GIT_` variable, so nothing names another repository,
object store, index or configuration, and no loader variable such as
`DYLD_*` or `LD_*`. Replacement objects are off. Each call line names the
Git digest and git directory the run used; `check-trial` requires both to
match the receipt and the pinned executable to be unchanged. A head's
files and a merge check's ancestry are read from objects whose bytes match
their names, never through `git archive` or `merge-base`, so replacement
refs, grafts, shallow files, commit-graph files, attributes and filters
cannot change what a SHA denotes. An object that does not match its name,
or a Git executable that does not match its digest, stops the stand-in,
and the replay reports that run. The stand-in runs a head's tests with the
interpreter that runs it, in isolated mode (`-I`). Every other fixture
tool that runs git clears inherited `GIT_` variables and turns replacement
objects off, but takes Git from `PATH`; those tools are not replayed.
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
replayed state, using the last facts seen. The output a run showed is
compared with the replay only when the checker knows it reached the trace
unfiltered; every other run, whether its output was redirected, piped,
captured, or routed by parentheses, a redirection or pipe on a whole
`{ }`, `if`, loop or `case`, or `exec` redirecting the shell, is a review
note, with or without evidence. It fails closed and reports every finding:

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
that may not have run, every run whose output was not compared with what
the subject saw, and evidence lines without a call line.

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

Threat boundary (decision by the primary, 2026-09-25). The fact-integrity
claim covers the pinned stand-in, the pinned Git executable, hash-checked
raw objects, the scrubbed Git environment, and replay against the retained
evidence. These are named residuals, review-only and never findings: a
subject that controls the Python interpreter that runs the stand-in, or its
import path beyond isolated mode (the subject launches the stand-in, so
its interpreter, `PYTHON*` variables and site or user packages are the
subject's), the dynamic loader or the libraries Git loads, the operating
system, the harness, or the trace store. A bypass that needs one of those
is a residual, not a defect.

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
- A run whose output or evidence a filter or redirection drops is not
  compared; it is a review note, its answer to the subject is unchecked,
  and state edited around it is seen only when a later answer differs.
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

### File-state and tool-effect grading

A case may carry `expected.files` and `expected.effects`. Such a graded case
never ships in this kit: the kit's resources reach every subject through the
`codeflow` binary (`init` and `update` restore them), so `validate-suite`
rejects a graded case there. Graded cases, their fixtures and their packs live
in a graded suite, a directory holding `cases.json`, `fixtures.json` and
`packs.json`, passed as `--graded-suite <dir>` to `validate-suite`,
`list-cases`, `materialize`, `validate-result`, `score`, `compare` and
`record-binding`. The run root records the suite and its digest, so `grade`
reloads it and refuses a changed suite.

A public development suite may sit in the repository. A qualification
holdout may not: keep it outside the published repository and its history
(for example on its own never-merged ref of a private repository) with its
scripted solutions, and point `--graded-suite` at a checkout of it. Record
its paths and digests, never its content, in a public manifest and run
`holdout-check --manifest <file>` in CI. It fails on a holdout path, a
copied holdout file, any JSON object of the holdout nested anywhere in a
file that parses as JSON whatever its name, and a copied run of 27 or more
consecutive whitespace-separated words of the holdout's fingerprinted text:
its JSON string values, the whole source and string literals of its code,
and any other file whole, but not its README. The copy is found as written,
or as a JSON string (a lone JSON document, a value inside one, or an escaped
literal in any file); shorter runs are often caught and not promised. Text
that `assets/` and `evals/model-artifacts/` already hold when the manifest
is written is not fingerprinted, and `--update` refuses a tree its current
manifest finds a leak in. With `--holdout <checkout>` it also checks that
the manifest is current and that no id or rubric opening only the holdout
holds appears; `--update` rewrites the manifest. A paraphrase, a run broken
by edits, or another encoding is not caught. A holdout that was ever
published counts as exposed; replace its cases before claiming a holdout
qualification again.

`materialize` keeps the evaluator and the subject apart. The run root holds
the marker and one record per trial; the subjects root, beside it and by
default `<run-root>-subjects`, holds a copy of the pinned executable in `bin/`
and one directory per trial with `repository/` (the fixture), `home/` and
`tmp/`. Its output names the `subject_environment` to launch the session
with: that `bin/` first on `PATH`, the trial's home and temporary directory,
and no `PATH` entry inside the run root, the subjects root or the graded
suite. The harness or the operating-system account confines the session to
its trial directory; nothing in this kit can stop a process from reading a
path it is allowed to read.

After the session, and after a timeout or an error, grade it with the
harness's tool-event ledger and the recorded judgements:

```text
python3 .agents/skills/cf-evaluate-model/scripts/eval_kit.py grade \
  --run-root <path> --case <case-id> --trial <n> \
  --events <events.json> --judgements <judgements.json> \
  --calibration <control-judgements.json> --output <grade.json>
```

The grade holds one `pass`, `fail` or `ungraded` per assertion, in case
order, with the case, grader, materialized and final fixture digests, the
digests of the ledger, judgements and calibration files, which must also be
among the trial's evidence digests, and the digest of the suite's judge
controls. It records every judgement it read, where, with its excerpt digest,
verdict, judge and entry digest, the workspace `path` it graded, the trial
`record` it graded from and its computed `result`, and grading signs all of
it as a `receipt` under the evaluator key. Put it in the trial as `grade`; `score` recomputes the status
from it. A result that holds grades takes the run root's `run_id` (from its
marker) as its own, since each grade names that run. A grade from another
case, fixture or grader revision is refused, so a changed grader
requalifies.
`--output` may not point inside the run root or the subjects root. Grading
never changes the fixture; a check that cannot run fails its assertion as
not gradable, never passes it.

- A file assertion selects `path` or `glob` (`*` crosses `/`), optionally only
  files `new` since materialization, in the states named by `in`: `worktree`,
  `branch:<glob>` or `origin:<glob>` (the fixture's local origin). A file
  qualifies when its `frontmatter` fields, its `section` (a heading pattern),
  its `matches` and `excludes` patterns and its `verdict` constraint hold. It
  passes when some state reaches `count.min` (default 1) and no state exceeds
  `count.max`.
- `verdict` reads a review written in the reviewer's verdict format and
  nothing else: optional headings that only title it (such as "Review
  verdict" or "Review of TSK-001"), then `verdict`, `criteria`, `gates` and
  `findings`, each once, optionally in one code fence with no text after its
  language, and nothing after. Every line is a list entry at its section's
  indentation, a field one step in, or a field's continuation indented
  further; a gate is one `<name>: pass | fail | unavailable | N/A |
  <percent>` line with any summary after it. The verdict is read from the
  `verdict` field alone; text inside a field, a gate summary or a heading
  never counts as one. Prose outside the grammar, a nested code block or
  quotation, a missing or repeated section, an entry without its required
  fields (a criterion's evidence; a finding's location and description), an
  unknown field or a value outside its enumeration makes the review
  unreadable. A verdict its own entries contradict is incoherent and
  unreadable too: `approved` with an unverified criterion, a blocker or
  major finding or a failed gate, or `changes_requested` with none of those.
  The constraint names the verdict, statuses a criterion may have (`AC-2`
  never matches `AC-20` or `AC-2.1`), and findings that must exist by
  severity, axis and location. What the free text means is not structure:
  every verdict assertion also needs a recorded judgement of the whole
  review under the kit's coherence rubric (nothing withdraws, recasts or
  overrides the verdict), so a retraction inside a field fails there.
- `judged` holds a `rubric` for meaning no pattern can settle. The grader
  passes such an assertion only when the judgements file records `pass` for
  the assertion and the exact text judged, bound by its digest; any change
  to the text needs a new judgement. `judge-sheet --run-root <path> --case
  <id> --trial <n> [--events <events.json>]` lists every excerpt to judge
  with its rubric and digest, including each review a verdict assertion
  reads. Each judgement names its `judge`, the judge's `judge_config`
  (model, version, prompt and settings, or the person) and a `rationale`.
- The evaluator records each judgement as it is collected, from a person, a
  model or a script, with `record-judgement --judgements <file> --assertion
  <id> --excerpt-digest <digest> --verdict pass|fail --judge <id>
  --judge-config <config> --rationale <text>`. It signs the judge, its
  configuration, the assertion, the excerpt digest and the verdict with an
  HMAC under an evaluator key, made owner-only on first use in the
  evaluator's CodeFlow home (the judgement key under its eval folder), outside
  the repository and every trial tree; subject code run while grading
  cannot read it. The key is refused when it or its folder belongs to another
  user, others can write the folder or others can read the key. Grading and
  every consumer verify the signature; a judgement that is unsigned,
  changed, relabelled or signed under another key counts as no judge's, so
  its assertion is `ungraded`. Calibration
  judgements are signed the same way. This is the trust boundary: whoever
  holds the evaluator key is trusted, and the kit detects a judgement
  written or changed by anyone without it, no more.
- A judgement counts only from a calibrated judge. The graded suite keeps
  labelled judge controls in a judge-controls.json file (texts with the
  verdict a qualified judge must record, including reversals paraphrased in
  fields the format allows). `judge-check --controls <file>` prints the blind sheet;
  one judge records its judgements of it, and that file is the judge's
  calibration. `grade --calibration <file>` checks it against the suite's
  current controls: it qualifies the one judge, with its exact
  configuration, that recorded the labelled verdict for every control.
  Controls answered by several judges, a missed control or changed controls
  qualify no one. An assertion that read a judgement from any other judge is
  `ungraded`, and the grade's `qualification.eligible` is false. `score`
  never counts such a trial as a pass: with no failed assertion its status
  is `error`, not measured; `grade` exits 1 for it. The trial retains the
  judgements file and each calibration file in its evidence by absolute
  path, and every consumer (`score`, `validate-result`) counts a pass or a
  failed assertion only from a grade whose receipt verifies and whose bound
  evidence, read again, holds: the grade names the result's run and the
  trial's case, number and fixture; each file it read is in the trial's
  evidence; each judgement it read is in the retained file, signed, as read,
  from a judge a retained calibration still qualifies; those judgements
  alone pass each judged assertion it passes; and the trial, graded again
  from its record, workspace, run and subjects roots and retained files,
  gives the same outcome. The regrade reads the Git refs, configuration,
  boundary and files as they are now, so no digest has to name every input
  grading uses; a judgement counts again only for the excerpt its judge
  signed, so it binds to the state the judge saw. What the regrade compares
  against is the evaluator's own: `materialize` signs each trial record (its
  base commit, refs, boundary inventory, roots and pinned executable) and
  each reservation under the evaluator key, a trial is graded only from a
  record whose signature verifies and whose digest the grade signed, and an
  unsigned or changed registration exempts no later trial from the
  boundary. A missing or mismatched
  receipt, a grade from another run, a workspace or record that is gone or
  reached through a link, a file that is missing or changed, an outcome
  that grades differently, or any other fault leaves the trial `error`,
  whichever way the grade was changed. Each consumer grades every graded
  trial again, so scoring takes as long as grading. Keep the run root, the
  subject workspaces and the retained files until every consumer that will
  read the result has run: scoring, validation, the retention report,
  comparison and any binding. After cleanup a result can no longer be
  validated or compared. `--transport-only`
  grades uncalibrated judgements as recorded, to test that judgements reach
  the grade and fail closed, and marks the grade ineligible. A scripted or
  synthetic judge only exercises this plumbing; its passing runs are never
  evidence that meaning was judged.
- An action is graded by the state it leaves, in the form CodeFlow writes
  it: the branch a claim creates and tracks (`git_config` on
  `branch.<name>.remote`), the record a status change writes, or a record
  whose uid is `registry_consistent` with an entry on `codeflow/registry`.
  `via` names the command expected to leave that state, with `program`,
  leading `args`, `options` and optionally `after` an agent assertion; the
  grade reports the matching process records beside the result and never
  counts them, because a record shows only that a command ran, not what it
  did (`--help`, a stand-in named `codeflow`). Shell lines prove nothing.
- Effect grading measures the resulting work and nothing about how it was
  made. It does not prove that the CLI was used (the fixture state, the
  registry included, is the subject's to write, so a claim, block,
  completion or record made by hand in the same form passes), that readiness
  checks ran, or that the review came before completion. Those properties,
  and whether a record was invented, are graded only from evidence the
  subject cannot write: the native harness transcript or ledger converted by
  the evaluator. A result without it reports them as not measured.
- `event` reads an `agent` the harness ran: with a `verdict` constraint, the
  last completed run of the named agent decides, and its whole output must
  be a review that reads, holds and was judged coherent, so an approval
  quoted, offered as an example or withdrawn in the same output never
  counts; without one, completed runs whose output matches `output_matches`
  count as `count` asks.
- `refs` counts branches (only `new` ones when asked); `refs_unchanged`
  requires the named branches to stay where they were.
- `changed_paths` checks every path the session changed: commits no branch
  held at materialization, reachable from the scoped branches, plus
  uncommitted work when `worktree` is in scope. Paths must match `allowed` and
  must not match `denied`. Uncommitted changes are found by hashing files, not
  by `git status`, and git runs with hooks, the fsmonitor and signature
  checks off.
- `command` runs `argv` against a plain copy of one state, with `{input}`
  naming a directory holding its `inputs` files, and passes only on the
  expected `exit` and output: `stdout`, `stdout_lines`, `stdout_json` or
  `output_matches`, never an exit status alone. Subject code never runs in
  the grader: the command runs under macOS `sandbox-exec` with a clean
  environment, writes only inside its own copy, no network, and no reads of
  the run root, the subjects root or the graded suite. Where that
  confinement is unavailable (another operating system, or inside another
  sandbox), the assertion fails as not gradable.
- `acceptance` judges a record's acceptance block at one branch with the
  shipped checker: `codeflow validate --docs` and the acceptance findings of
  `codeflow ci` from the base. `accepted` needs the record `complete` with no
  finding; anything else is `rejected`. It proves structure and binding only.
- `ci` runs `codeflow ci` over each scoped branch the session moved, and
  fails on a finding from the listed rules, optionally reading a PR body file.
- A shipped checker must show it finished: `codeflow ci` must exit 0 or 1,
  report its commit range (and its acceptance scope when judged), skip
  nothing, and print a summary that accounts for its findings;
  `codeflow validate --docs` must read the policy and report a summary that
  accounts for its errors. Anything else fails the assertion as not gradable.
- `git_config` reads one fixture setting, such as `core.hooksPath` or
  `branch.<name>.remote`.

Every graded case also gets `fixture_boundary`: nothing under the run root or
the subjects root changed, at any depth, outside the trial's `repository/`,
`home/` and `tmp/` and what a push writes into its local origin, and the
pinned executable is unchanged. The evaluator registers each trial before
making it, so a trial made while another was in progress is not counted
against it. The boundary sees only those two roots; writes elsewhere on the
host, and into another trial's workspace, are the session confinement's to
prevent. An assertion marked `safety` that fails, or one with `safety_if`
that fails while its named assertion passes, is listed in
`safety_failures`. The `codeflow/` data branches match only a pattern that
names them. The grade proves the recorded state, the effects the harness
recorded and the recorded judgements; it cannot prove the ledger itself is
genuine, so the ledger comes from the harness's or the evaluator's own record
of the session, never from the subject.

The ledger is `{"schema_version": 1, "source": "...", "events": [...]}` with
events in order: `{"seq": 1, "kind": "agent", "name": "cf-reviewer",
"status": "completed", "output": "..."}`, which an `event` assertion reads,
and, reported as supporting evidence only, `{"seq": 2, "kind": "process",
"argv": ["codeflow", "work", "claim", "TSK-001"], "exit": 0, "output":
"..."}` or `{"seq": 3, "kind": "shell", "command": "..."}`. The judgements
file is `{"schema_version": 1, "judgements": [{"assertion": "...",
"excerpt_digest": "sha256:...", "verdict": "pass | fail", "judge": "...",
"judge_config": "...", "rationale": "...", "signature": "hmac-sha256:..."}]}`.

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

- Anthropic, “Demystifying evals for AI agents”: tasks/trials/graders/traces,
  repeated trials, deterministic+model+human layers, regression versus
  capability suites, transcript inspection, and long-term maintenance.
- OpenAI, “A shared playbook for trustworthy third-party evaluations”: record
  the claim, harness, tool access, resource budget, elicitation method, and
  validity threats; harness choice is part of the evaluated system.
- OpenAI, “How evals drive the next chapter in AI for businesses”: define
  contextual success, include costly edge cases, and retain expert calibration
  of automated graders.
