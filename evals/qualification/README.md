# Native qualification runner

This repository tool launches one fresh interactive seat from an evaluation
kit fixture record. TSK-194 can use it for seat qualification. It does not
install into consuming projects, authenticate accounts, grade its own subject,
or treat a terminal status as proof that the model completed the task.

Materialize with the current binary and kit first. The run root and subject
ancestors must contain no `CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md` or
`.claude` entry. The kit checks at materialization and launch, recording the
checked ancestor paths in `launch.json`. Even a root under the operator's
home fails if that home contains `.claude`; use a clean location outside both
the home and repository. Only metadata is checked, never ancestor contents.
Put runner evidence outside every watched directory; overlapping evidence
directories are refused.

```sh
kit=assets/base/agents/skills/cf-evaluate-model/scripts
run_parent=$(mktemp -d /private/tmp/codeflow-eval.XXXXXX)
python3 "$kit/eval_kit.py" materialize \
  --run-root "$run_parent/run" --case model-independent-plans \
  --trial 1 --codeflow "$PWD/target/release/codeflow"
python3 evals/qualification/runner.py launch \
  --record <printed-fixture-record> \
  --output "$PWD/target/qualification/trial-1" \
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

Before the first trial, prepare dedicated evaluator authentication:

```sh
kit=assets/base/agents/skills/cf-evaluate-model/scripts
python3 "$kit/eval_kit.py" prepare-eval-homes
```

Run the printed native commands yourself and complete each harness's sign-in.
The command only creates folders and missing non-secret settings; it never
signs in, reads credentials, or overwrites existing config. Claude uses
`/login`, Codex uses its ChatGPT sign-in, and Grok uses browser approval.
For hook-dependent Codex fixtures, open `/hooks` and review and trust the
fixture's hooks once. Codex records hook trust by content hash, and the
materialized hook files are byte-identical across cases and paths. That one
operator review covers trials until hook contents change. No trust is seeded.

`HOME`, `TMPDIR`, `CODEFLOW_HOME` and `XDG_CONFIG_HOME` stay disposable per
trial. `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `GROK_HOME` point respectively to
`~/.codeflow-eval/claude`, `codex` and `grok`, signed in once by the operator.
Grok supports `GROK_HOME`, so no symlink is used. The kit never reads, copies
or uses the operator's personal harness folders. Symlinked evaluator folders
and any symlink inside them are refused without following the link. Checks
are bounded to 100,000 entries and ten seconds per home; an incomplete check
refuses. Executable symlinks under `~/.local/bin` may resolve to harness
installation folders; this executes the installed binary without importing
personal configuration. Credentials are managed only by the harness: on macOS Claude
may use a Keychain entry scoped to its dedicated config directory; Codex is
explicitly set to the file credential store in its dedicated folder.

Before creating a tab, Claude must report `loggedIn: true` with the expected
`configDirectory` from `claude auth status`; Codex must report `Logged in
using ChatGPT` from `codex login status`. Raw account details are not retained.
Grok has no installed auth-status subcommand: after startup its fresh welcome
must show the authenticated `New worktree` and `Resume session` menu rows
and an empty or placeholder editor, without a login screen. Unknown states
refuse before any prompt, using `evaluator home not signed in: run
prepare-eval-homes`. Grok may show a browser-approval screen before refusal;
the runner never answers it. A positive status is local sign-in evidence,
not a guarantee that a remote subscription or token will remain valid.

During startup, the runner may accept only Claude's workspace-trust or
Codex's project-trust dialog naming this trial's exact materialized subject
path (see the [Codex trust renderer][codex-trust]). It re-reads the visible
pane before each selection key and Enter,
requires the affirmative choice visibly selected, and records harness,
path, screen-text SHA-256 and time in `trust_acceptances`. This implements
the operator's standing authorization for harness-created disposable samples.
A different path, truncated path, import dialog or other trust prompt
refuses. The kit never writes trust grants into config. This workspace
acceptance is separate from the operator's one-time review of Codex hooks.

Caller arguments use a per-harness allowlist: Claude model, effort and
permission mode; Codex model, reasoning effort (`-c model_reasoning_effort`),
approval policy and sandbox; Grok model, reasoning effort and permission mode
or always-approve. No other config, directory, plugin, agent or profile flag
is accepted. Values resolving under personal harness folders are refused.
The refused flag name is recorded in `launch.json`. State-control arguments
are appended only by the kit and are included in the exact launch arguments.

At trial start, after authorized workspace trust and readiness but before
prompt delivery, `config_start` records SHA-256 digests of non-secret
configuration in all three evaluator homes. `finish` writes `config_finish`
to the same `launch.json`. Added, edited or removed files raise
`evaluator_config_drift`; unreadable files or new symlinks raise
`evaluator_config_unreadable`. Coverage is the settings/config files,
CLAUDE/AGENTS/GROK instruction files and recursive `rules/` listed in the
runner's `CONFIG_FILES`. Account stores, credentials, `.claude.json`, session
history and caches are not hashed. Native workspace-trust writes before
readiness are not trial drift. The digests do not prove absence of transient
changes restored before finish, or changes outside the listed coverage.

Claude auto-memory and Grok cross-session memory are disabled in the launch
environment. Codex launch overrides disable history and memory generation
and injection, and move its SQLite state and logs into the disposable HOME.
Grok logs and its leader socket are disposable. Claude transcripts and
history, Codex session rollouts, and Grok sessions can still accumulate in
their dedicated folders, along with native config, caches and auth refreshes.
Every fixture has a fresh path, Claude has a unique project storage name,
and resume/continue arguments are refused. Retained conversations are not
automatically resumed and shared memory is not injected into new trials.
Do not install personal instructions, skills or plugins in these folders.
This is context isolation, not a filesystem read barrier: a subject with
sufficient permissions could explicitly read retained files. Treat such a
read or shared config mutation as contamination; native trace review is
still required. Do not claim that path separation makes history inaccessible.

Sources: [Claude environment](https://code.claude.com/docs/en/env-vars),
[Claude authentication](https://code.claude.com/docs/en/authentication),
[Claude status](https://code.claude.com/docs/en/cli-reference),
[Codex settings](https://learn.chatgpt.com/docs/config-file/config-reference),
[Grok settings](https://docs.x.ai/build/settings/reference), and the
[Grok welcome renderer][grok-welcome].
Claude's `theme` and `hasCompletedOnboarding` bootstrap in `.claude.json`
are described in its public [onboarding report][claude-onboarding]
and the native theme dialog; the onboarding key is not a stable settings API.
The kit seeds only a missing file. A changed first-run UI is a refusal for
inspection, not permission to invent flags or answer login automatically.

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

[grok-welcome]: https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/src/views/welcome/mod.rs
[claude-onboarding]: https://github.com/anthropics/claude-code/issues/67149
[codex-trust]: https://github.com/openai/codex/blob/main/codex-rs/tui/src/onboarding/trust_directory.rs
