# Native qualification runner

This repository tool launches one fresh interactive seat from an evaluation
kit fixture record. TSK-194 can use it for seat qualification. It does not
install into consuming projects, authenticate accounts, grade its own subject,
or treat a terminal status as proof that the model completed the task.

Materialize with the current binary and kit first. Keep fixture roots and
runner evidence outside `/private/tmp` when possible. Put runner evidence
outside every watched directory; overlapping evidence directories are refused.

```sh
python3 assets/base/agents/skills/cf-evaluate-model/scripts/eval_kit.py materialize \
  --run-root "$PWD/target/qualification/run" --case model-independent-plans \
  --trial 1 --codeflow "$PWD/target/release/codeflow"
python3 evals/qualification/runner.py launch \
  --record <printed-fixture-record> --output "$PWD/target/qualification/trial-1" \
  --workspace <Herdr-workspace-id> --harness codex -- \
  --model <qualified-selector> -c 'model_reasoning_effort="high"' \
  --ask-for-approval never --sandbox danger-full-access
python3 evals/qualification/runner.py finish \
  --output "$PWD/target/qualification/trial-1"
```

Use `--harness claude -- --model <selector> --effort high --permission-mode
<approved-mode>` or `--harness grok -- --model <selector> --permission-mode
<approved-mode>` for those native seats. Pass the actual approved flags;
Grok's explicit `--always-approve` is also recorded. Headless arguments are
refused. Model and permission flags in the launch record are **requested**,
not independently observed effective settings. Retain native settings and
identity evidence before claiming them as observed.

HOME, TMPDIR, CODEFLOW_HOME and harness config directories come from the kit.
The primary must provision permitted authentication through the harness's
supported mechanism in the isolated home. A login failure stops the trial;
never replace HOME with the real home or copy credentials to make it pass.
A new tab has its own cwd and environment; no existing pane is reused. The
runner waits for shell readiness before starting a seat, then for seat readiness
before taking the `before.json` baseline, then delivering the prompt.
Startup writes before readiness are not counted. If startup refuses before
readiness, no baseline exists and `finish` flags incomplete observation.
Claude delivery recognizes only the complete editor frame
proven by the release harness. It sends at most two Enters, the second only
when that frame still holds this trial's unsent prompt. An empty editor is
never evidence of pending input. Unknown frames, dialogs and unreadable panes
stop delivery. Codex and Grok keep the managed helper's one-Enter refusal.
No prompt is automatically resent. A refusal retains the created tab IDs for
inspection and cleanup by the primary.

`finish` is a manual observation boundary. Run it after the primary verifies
the native turn finished, including any peer work, and before cleanup. Copy
all `observation.json` validity flags into the trial's `validity_flags` before
kit scoring. Retain native transcripts, exact command and tool results,
fixture state, and requested versus observed model and settings evidence.
Then close the recorded tab with `herdr tab close <tab-id>` and verify it
closed. A `started` launch record is not a completed or graded trial.

The subject's TMPDIR is observed recursively in full. At the `/private/tmp`
top level, every direct entry is observed by name and type. Regular files
and symlinks also retain their modification times; directory mtimes are
ignored. A new or removed top-level entry, a type change, or a changed file
or symlink mtime invalidates the trial. Symlinks are never followed.

`--watch-dir` adds recursively observed directories. Recursive snapshots
retain mode, size, modification and change times without reading file
contents. New, changed or removed observed entries invalidate the trial.
An entry unreadable in both snapshots with unchanged readable metadata is
recorded in `limitations`, without invalidating the trial. New or changed
unreadability, missing roots and entries without metadata invalidate
observation.

Each snapshot defaults to 100,000 entries and ten seconds, configurable via
`--max-entries` and `--snapshot-seconds` (at most 60 seconds).
Reaching a cap raises `directory_observation_entry_cap` or
`directory_observation_time_cap`; partial observation never silently passes.
The time cap is cooperative between metadata calls; a blocking OS call may
overrun it. Entry count and elapsed time are retained with each snapshot.

Writes inside pre-existing top-level directories of `/private/tmp` are
unobserved, except within separately watched roots such as the subject's
TMPDIR. Writes outside declared directories, startup writes before readiness,
and transient entries gone before the final snapshot are also unobserved.
The comparison records these limits. Changes are not attributed to the subject.
This is a bounded diagnostic, not host confinement or a tamper-resistant
audit. Run trials sequentially when attribution would otherwise be ambiguous.

For the planted-write control, take the launch snapshot, write a uniquely
named benign file under a declared test directory with the native subject,
then run `finish`. Its path must appear in `added` and the trial must carry
`declared_directory_changed`. Preserve this intentionally invalid trial as a
control, separate from clean behavioral reruns. Remove only that owned file
after retaining the evidence. Repeat once per tested harness.

Judge controls live in `judge-controls.json`, outside the shipped scaffold.
They are fictional excerpts for calibration, never fabricated native evidence.
Produce a blind sheet with `eval_kit.py judge-check --controls <file>`, have the
actual other-lineage judge assess it, record each verdict with
`record-judgement`, and run `judge-check --controls <file> --judgements <file>`.
Retain that judge's native provenance. The unit test's synthetic calibration
only checks the plumbing; it does not qualify a judge. Controls cover the
process-round case boundaries and all six R-105 scenarios from SPC-013.

The delivery test suite is offline by default. Its two live Herdr canaries
require `CF_HERDR_LIVE_CANARY=1`; the primary may opt in when native launch
is authorized. A live canary is a transport check, not an acceptance trial.
