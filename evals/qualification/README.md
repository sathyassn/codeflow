# Native qualification runner

This repository tool launches one fresh interactive seat from an evaluation
kit fixture record. TSK-194 can use it for seat qualification. It does not
install into consuming projects, authenticate accounts, grade its own subject,
or treat a terminal status as proof that the model completed the task.

Use Python 3.11 or newer (the runner parses TOML with `tomllib`).
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
mkdir "$run_parent/watch"
python3 "$kit/eval_kit.py" materialize \
  --run-root "$run_parent/run" --case model-independent-plans \
  --trial 1 --codeflow "$PWD/target/release/codeflow"
python3 evals/qualification/runner.py launch \
  --record <printed-fixture-record> \
  --output "$PWD/target/qualification/trial-1" \
  --watch-dir "$run_parent/watch" \
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
The command creates folders and missing non-secret settings; it never signs
in or reads credentials. Claude uses `/login`, Codex uses its ChatGPT sign-in,
and Grok uses browser approval.

No plugin loads in the dedicated Codex home. A signed-in Codex 0.159.1 syncs
the account's installed remote plugins into `plugins/cache/` at every start
whenever its `plugins` feature is on; `remote_plugin` alone turns off only the
remote catalog. So the command writes `plugins = false` and
`remote_plugin = false` under `[features]` in that home's `config.toml`,
adding only what is missing and verifying that the rest of the file parses
unchanged. It refuses a config that sets either one to another value (a
number included), or that defines `[features]` in a form other than one table
header, and leaves the file as it was. It then moves an
account-managed plugin cache out of the home (the `openai-curated-remote`
marketplace, any marketplace holding Codex's remote-install marker, and a
non-empty install staging folder) into
`~/.codeflow-eval/removed-remote-plugins/<time>/`, printing each move and the
reason. A linked destination folder is refused before anything moves. Delete
that folder once it is no longer needed. Reading that config
needs Python 3.11 or newer, as the runner does. Every Codex start the
kit builds (the trial seat, a peer's answer, the hook-review command and the
setup helper) also passes `-c features.plugins=false -c
features.remote_plugin=false`. The command touches only the dedicated home,
never `~/.codex`.

Every launch checks the dedicated Codex home before any seat starts, whatever
the harness, since any trial can open a Codex peer: a missing or different
setting, or a remote plugin cache, refuses with `run prepare-eval-homes`. The
check runs again after readiness, before the prompt, and for every Codex peer
start; `config_finish` records the same state, so a change flags
`evaluator_config_drift`. `launch.json` records it as `codex_remote_plugins`.

No account plugin, synced skill or claude.ai connector loads in the dedicated
Claude home either. A Claude Code home signed in to a claude.ai account
downloads the plugins and skills enabled on that account into
`plugins/synced/` and `skills/synced/` and connects the account's claude.ai
MCP connectors; all of them load into every session. The 2026-10-01 first
batch shows it: each Claude trial listed about 100 account skills and up to
148 claude.ai and plugin MCP tools. `prepare-eval-homes` therefore writes
`syncClaudeAiPlugins: false`, `syncClaudeAiSkills: false` and
`disableClaudeAiConnectors: true` into that home's `settings.json`, adding
only what is missing and refusing a key set to anything else, and moves
non-empty synced folders into `~/.codeflow-eval/removed-synced-content/<time>/`,
printing each move. It refuses, without changing them, user skills, commands
and agents and installed or enabled plugins in that home; remove those
yourself. Every Claude start the kit builds, the trial seat and a peer's
answer, also passes `--settings` with the same three keys. Every launch checks
the home before any seat starts, again after readiness and for every Claude
peer, and refuses with `run prepare-eval-homes`; `launch.json` records the
state as `claude_account_content` and `config_finish` as part of the drift
check. The home's `.claude.json` holds account state and is never parsed.

At `finish` the runner reads every native Claude transcript of the trial, the
subject's and its peers', from the trial's project folder in that home. It
reads only the structured inventory and invocation fields Claude Code 2.1.286
writes: the skill listing's names, invoked skills, the agent listing, the MCP
servers named by instructions and by pending or failed connections, the MCP
tools a session listed, surfaced or recorded, and every tool call. A name
mentioned in a description, an instruction or a reply never counts. A
namespaced skill or agent (`<plugin>:<name>`, the form plugin and synced
content takes), or any MCP server or tool the fixture did not declare, adds
`extra_extension_loaded` to the trial's validity flags, so the trial is
invalid, never a model result. The declaration is the fixture's `.mcp.json`
as launch read it, recorded in `launch.json` as `declared_mcp_servers`, so an
edit during the trial cannot authorize a server. A tool the transcript
attributes to a server counts as declared only when that server is declared.
Without attribution a name `mcp__<server>__<tool>` is ambiguous, since server
names may hold `__`, so it counts as declared only when every way of splitting
it names a declared server. `observation.json` lists what was found under `claude_extensions`,
including the unnamespaced skills and agents the session listed (the
fixture's own and the harness's bundled ones), which are recorded but not
flagged. The walk opens each folder and file relative to its parent's
descriptor and follows no link, even one swapped in during the walk: a link
anywhere from the Claude home down, dangling or not, a special file such as
a pipe named `.jsonl`, an entry that changed between look and open, a folder
that cannot be listed, a transcript that cannot be read or an unreadable
declaration adds `evaluator_config_unreadable`. Claude Code 2.1.286 documents the three
settings; a live Claude trial is the proof that they take effect.
For an operator's own interactive Codex hook review, materialize a fresh
fixture with the current binary, then print the operator command:

```sh
python3 evals/qualification/runner.py print-hook-review \
  --record <fresh-fixture-record>
```

This validates the unused fixture and prints an isolated native Codex command;
it does not launch Codex or answer any screen. The command keeps the
dedicated `CODEX_HOME`, the file credential store and the disposable HOME,
and starts Codex with `--sandbox read-only` and its default approval policy,
since the session only reviews hooks. Run the printed command once
in an app terminal, accept that fixture's folder, review the hooks and trust
them yourself. Use `/hooks` if needed. Exit without submitting `TASK.md`.
The review covers `.codex/hooks.json`: the PreToolUse `git-guard`,
`exec-guard` and `edit-guard` commands, plus `session-orient` at SessionStart
and UserPromptSubmit. These invoke the pinned fixture `codeflow` binary.
Codex keys manual hook trust by the hooks file's absolute path and a per-hook
hash. A review in one fixture does not carry to another. The printed helper
and an operator's own interactive Codex still review hooks through `/hooks`.

Hook-review bypass is **off by default**. Without an option, the runner
prints the operator's hook-review command and never passes the native bypass
flag. The person running the evaluation may choose
`--codex-hook-trust=bypass` on each `runner.py launch` command, before the
native-argument `--` separator. This skips Codex's per-folder hook review
for that evaluation trial only; it is not a saved preference or permission
for adopters' ordinary sessions. `--codex-hook-trust=review` is the default.

With that explicit choice, the runner appends
`--dangerously-bypass-hook-trust` once, only for Codex with `CODEX_HOME`
resolving to `~/.codeflow-eval/codex`. A caller-supplied native bypass flag
also requires that choice; another home refuses, and so does the flag in a
Claude or Grok seat's arguments. On a Claude or Grok launch the option runs
the same checks and covers only the Codex peers that seat opens (see "Peers
the subject opens"); the native flag never reaches Claude or Grok. Before a
bypass launch, the runner checks:

- The dedicated home has no `hooks.json` or TOML hooks entries (including old
  `hooks.state` trust records). Malformed or unreadable config refuses.
  Config in profiles receives the same checks.
- Installed and cached plugins under `plugins`/`.plugins`, plus configured
  local marketplace sources, are inspected. The runner reads root `plugin.json`
  (Agent Plugins) and `.codex-plugin/plugin.json`, `.claude-plugin/plugin.json`
  and `.cursor-plugin/plugin.json`, including fallbacks beside a root manifest.
  Any `hooks` key anywhere in any manifest, including extensions and inline
  definitions, refuses regardless of the referenced filename. A `hooks.json`
  anywhere in the tree or any `hooks` directory also refuses, even for a
  disabled plugin. Cached plugin/version folders without a parseable manifest,
  symlinked, unreadable or non-regular paths, and invalid manifests refuse. Inspection
  is capped at 100,000 entries and ten seconds per tree; either cap refuses.
  Enabled plugins must match an inspected name and marketplace. A configured
  marketplace must have an inspectable local source or cached plugins on
  disk; unresolved sources refuse without fetching them.
- The fixture's `.codex/hooks.json` matches the complete shipped definition
  in `assets/base/codex/hooks.json`, including matchers, handlers and the
  `codeflow hook <name> --contract 3 || { ...; exit 2; }` wrappers, which
  carry no `$`. Missing, extra or changed
  definitions refuse. Fixture TOML hooks refuse; fixture plugin sources
  receive the same inspection. Symlinked hook/config sources refuse.

With plugins turned off, Codex loads no plugin in the dedicated home, and an
account-managed remote cache is refused before these checks run (see the
sign-in setup above). Any other plugin left on disk is still inspected:
hook-free ones are recorded rather than refused, and the `hooks` rule above is
unchanged. `hook_trust.plugins` in `launch.json` lists each inspected plugin's
name, declared version (null if absent), source, marketplace, path, skills
and apps presence, every manifest's SHA-256, and a digest of entry metadata
(paths, modes, sizes and mtimes). This is a disk inventory, not proof
that each plugin was enabled or used. Trace review must account for it.

These checks restrict bypass to the reviewed shipped hook commands and
exclude user and plugin hooks that could otherwise run without review.
The runner never writes trust grants, moves or deletes plugin caches, removes
existing trust records, or touches the operator's personal `~/.codex`; only
`prepare-eval-homes`, run by the operator, moves a remote plugin cache.
An unexpected "Hooks need review" screen still refuses without a key.
Existing exact-path workspace-trust checks remain in force; trial paths
stay fresh. No stable path or archive lifecycle is used.

`launch.json` records `hook_trust.option` (`review` or `bypass`),
`flag_used`, the resolved `evaluator_home`, `hooks_sha256`, and both
`checks` results (`passed`, `refused` or `not_checked`). For bypass launches,
exact `native_args` and `permission_flags` include the flag. Default review
launches record no flag use and leave bypass-specific checks `not_checked`.
A failed preflight records its results and launches no seat. For bypass
launches, `config_preflight` includes that plugin inventory. After readiness,
the runner inspects plugins again in `config_start` and refuses prompt
delivery on any inventory change, including a new plugin, or an unsafe or
unreadable source. At finish, `config_finish` includes another inspection;
changes flag `evaluator_config_drift`, and unsafe or unreadable sources flag
`evaluator_config_unreadable`.

These are checks at three points, not a filesystem barrier. A plugin
SessionStart hook downloaded during startup could run before readiness;
the refusal prevents prompt delivery but cannot undo that execution.
Changes between checks, content edits preserving entry metadata outside
manifests, and system-managed hook sources remain outside the guarantee.
Validity flags and trace review remain necessary. Native dry trials are
separate evidence; this runner change alone does not prove the native flag
took effect.

If a bypass launch refuses stale `hooks.state` records, the operator backs
up the evaluator home's `config.toml`, then removes only those tables before
retrying. The runner never edits that file or removes trust records.

`HOME`, `TMPDIR`, `CODEFLOW_HOME` and `XDG_CONFIG_HOME` stay disposable per
trial. `CLAUDE_CONFIG_DIR`, `CODEX_HOME` and `GROK_HOME` point respectively to
`~/.codeflow-eval/claude`, `codex` and `grok`, signed in once by the operator.
Grok supports `GROK_HOME`, so no symlink is used. The kit never reads, copies
or uses the operator's personal harness folders. Symlinked evaluator folders
and config symlinks inside them are refused without following the link.
Codex's three native executable dispatch links under `tmp/arg0/codex-arg0*`
are allowed only when their target is exactly the installed Codex executable.
No configuration or credential link receives that exception. Checks
are bounded to 100,000 entries and ten seconds per home; an incomplete check
refuses. Executable symlinks under `~/.local/bin` may resolve to harness
installation folders; this executes the installed binary without importing
personal configuration. Credentials are managed only by the harness:
on macOS Claude
may use a Keychain entry scoped to its dedicated config directory; Codex is
explicitly set to the file credential store in its dedicated folder.

On macOS the disposable HOME links only `Library/Keychains` to the
operator's real `~/Library/Keychains`. Nothing else in Library is linked.
This lets macOS locate the login keychain; Claude uses the evaluator's own
entry keyed to its dedicated config folder. The kit does not read or copy
keychain contents, reset a keychain, or reference personal harness config.
This exposes the login keychain's presence to the subject's `security`
commands; individual entries remain governed by macOS access controls.
The printed setup helper creates the same link. Both `USER` and `LOGNAME`
are populated in the subject environment and passed unchanged to the native
status command, since Claude's keychain account uses the username.

Before creating a tab, Claude must report `loggedIn: true` with the expected
`configDirectory` from `claude auth status`; Codex must report `Logged in
using ChatGPT` from `codex login status`. Raw account details are not retained.
Grok has no installed auth-status subcommand: after startup its fresh welcome
must show the authenticated `New worktree` and `Resume session` menu rows
and an empty or placeholder editor, without a login screen. A visible
sign-in screen refuses with `evaluator home not signed in: run
prepare-eval-homes`. Any other state refuses before any prompt with a
message naming the Grok version (or `(version not shown)`) and the screen:
trust dialog, welcome screen, blank screen or unknown screen, for example
`Grok Build 1.0.46: welcome screen not recognized`. The screen a startup
refusal met is kept as `startup-screen.txt`. Grok may show a
browser-approval screen before refusal; the runner never answers it. A positive status is local sign-in evidence,
not a guarantee that a remote subscription or token will remain valid.

During startup, the runner may accept only Claude's workspace-trust or
Codex's project-trust dialog naming this trial's exact materialized subject
path (see the [Codex trust renderer][codex-trust]). It re-reads the visible
pane before each selection key and confirmation, requires the affirmative
choice visibly selected for menu dialogs, and records harness, path,
screen-text SHA-256 and time in `trust_acceptances`. This implements
the operator's standing authorization for harness-created disposable samples.
A different path, truncated path, import dialog or other trust prompt
refuses. The kit never writes trust grants into config. This workspace
acceptance is separate from the operator's review of that fixture's Codex hooks.

Caller arguments use a per-harness allowlist: Claude model, effort and
permission mode; Codex model, reasoning effort (`-c model_reasoning_effort`),
approval policy, sandbox and the scoped hook-trust flag above; Grok model,
reasoning effort and permission mode
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

The runner accepts Grok's workspace-trust dialog, as observed in 1.0.44 and
1.0.46, only when its wording and full repository path match the disposable
fixture. The record includes the screen digest, time and Grok version, just
as for Claude and Codex.
For Claude's exact "Try the new fullscreen renderer?" dialog it selects
"Not now", verifies that selection before Enter, and records the display
choice in `launch.json`. This keeps the current renderer; the kit writes no
setting to suppress the dialog. Unknown or changed dialogs still refuse,
and the strict empty-editor check runs before any trial prompt is pasted.
The Grok match is pinned to the trust dialog's `Grok Build <version> [stable]`
footer for the captured versions 1.0.44 and 1.0.46. The 1.0.46 welcome shows
no channel, so the version comes from that dialog: a Grok seat reaches
readiness only after the runner accepted a trust dialog of a captured version
for the fresh fixture, and `authentication.grok_version` and a `ready` entry
in `verified_frames` (with `grok-ready.txt`) record it. Another version fails
closed, naming itself, until its trust and welcome screens are captured
again and added to `GROK_VERSIONS` in the runner.

A new tab has its own cwd and environment; no existing pane is reused. The
runner waits for shell readiness before starting a seat, then for seat readiness
before taking the `before.json` baseline, then delivering the prompt.
Startup writes before readiness are not counted. If startup refuses before
readiness, no baseline exists and `finish` flags incomplete observation.
Claude delivery recognizes only the complete editor frame
proven by the release harness, plus the observed branch status row only
when its branch equals the signed fixture record. That row may show the
known effort label or `ctrl+g to edit in Nvim` hint. Editor captures retain
the actual before-paste and pending-input screens in the evidence folder.
It sends at most two Enters, the second only when that frame still holds
this trial's unsent prompt. An empty editor is never evidence of pending
input. Unknown frames, dialogs and unreadable panes stop delivery.

Codex readiness and delivery use the captured idle frame, not Herdr status
alone: the empty composer line `› Ask Codex to do anything`, one blank line,
the status line and the `? for shortcuts` footer. The status line must show
the requested `--model` label (case-insensitive), the requested effort when
one is passed, and this fixture's repository path as the cwd; the session
title, branch and warning count are not required. Codex trials must pass
`--model`. Any other frame refuses without a key: a dialog, a draft, another
cwd or model, or a changed placeholder. The runner re-reads that frame just
before pasting, then sends one Enter only when the composer holds this
prompt, as text or as `[Pasted Content N chars]` with this prompt's length.
The footer under pending input is not checked: Codex 0.160.0 shows only the
warning count there. Codex 0.160.0's idle and pending frames, captured in
TSK-194's dry trial of 2026-10-04, are kept under `frames/` and tested. The
ready, before-paste and pending frame digests and times are recorded in
`verified_frames` in `launch.json`, with the screens in the evidence folder.
There is no second Enter. Grok keeps the managed helper's one-Enter refusal
after its authenticated-welcome check. No prompt is automatically resent.
A refusal retains the created tab IDs for inspection and cleanup by the
primary.

`finish` is a manual observation boundary. Run it after the primary verifies
the native turn finished, including any peer work, and before cleanup. Copy
all `observation.json` validity flags into the trial's `validity_flags` before
kit scoring. Retain native transcripts, exact command and tool results,
fixture state, and requested versus observed model and settings evidence.
Then close the recorded tab with `herdr tab close <tab-id>` and verify it
closed. A `started` launch record is not a completed or graded trial.

Only directories named with `--watch-dir` are observed, including any
planted-control root. At least one root is required. There are no implicit
TMPDIR or `/private/tmp` watches. The subject's TMPDIR is harness-owned
scratch and is completely unobserved: runtime caches, sockets and other
writes there do not raise `declared_directory_changed`. No filename
allowlist is used. A watch root equal to, inside, or containing TMPDIR
refuses, including aliases resolved through symlinks. Planted controls must
never be placed under TMPDIR. Choose a separate directory for them.

If `/private/tmp` is explicitly watched and does not overlap the trial
TMPDIR, it retains the top-level-only observation: names and types, plus
mtimes for regular files and symlinks. Directory mtimes are ignored.
Symlinks are never followed. Other watch roots are recursive.

Recursive snapshots
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
unobserved, except within separately watched roots. Harness TMPDIR is
unobserved in its entirety. Writes outside declared directories, startup
writes before readiness, and transient entries gone before the final
snapshot are also unobserved.
The comparison records these limits. Changes are not attributed to the subject.
This is a bounded diagnostic, not host confinement or a tamper-resistant
audit. Run trials sequentially when attribution would otherwise be ambiguous.

For the planted-write control, take the launch snapshot, write a uniquely
named benign file under a declared test directory outside TMPDIR with the
native subject,
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

## Peers the subject opens

A subject may open another seat, for example a Codex reviewer in a new Herdr
tab, as cases 00 and 11 did in TSK-194's first batch. A Herdr tab started by
the subject gets Herdr's server environment, not the subject's, unless the
call passes it with `--env`. Without help such a peer would run with the
operator's HOME and harness folders, and it stops at native trust screens
that nobody answers.

The subject runs as the operator's user, so nothing it can write is
authority: requests, answers, `context.json` and ready records are all
writable by it, and it can start a seat by absolute path or through the real
Herdr binary. The launchers only arrange a correct start. What counts is the
watcher's check of each peer's live process, read from the OS, and anything
it cannot verify invalidates the trial.

```text
 subject seat (trial PATH)          peer pane                      runner watch
 herdr tab create ...  ──> peer-bin/herdr adds trial --env and workspace
 codex ...             ──> peer-bin/codex ── request ──────────> checks the start
                                          <── argv, environment ── (or refusal)
                           exec in place (same pid)
                           live process   ──────────────────────> verify from the OS
                           folder dialog  <── Enter, after an identity recheck
                           ready          ──────────────────────> recheck, ready record
 herdr pane send-text ──> peer-bin/herdr waits for the ready record, else refuses
```

At launch the runner writes `peer-bin/` and `peer-launches/` next to the
fixture (outside the repository, HOME and TMPDIR) and puts `peer-bin` first
on the trial PATH. It holds copies of `peer_shim.py` named `herdr`, `claude`,
`codex` and `grok`, and a `context.json`; nothing names the evaluator's
checkout. Watch roots overlapping these folders are refused.

- `herdr`: `tab create` and `pane split` get every trial environment
  variable; `tab create` gets the trial's `--workspace` when none is given.
  `pane split` asks Herdr which workspace its target pane is in, whether the
  target is positional, `--pane`, `--current` or implied by the caller's
  pane, and refuses any other workspace. `workspace create`,
  `worktree create|open`, another workspace, or an `--env` that changes a
  trial variable is refused. Typing into a pane (`pane send-text`,
  `send-keys` and `run`, `agent prompt` and `send-keys`) is allowed for the
  subject's own pane; any other pane must be in the trial workspace and have
  the watcher's ready record, waited for up to 30 seconds, or nothing is sent.
  Each delivery let through is logged with its time and pane, and `finish`
  checks it against the time the watcher recorded that peer ready.
  Other Herdr calls pass through.
- `claude`, `codex`, `grok`: version, help and sign-in status calls pass
  through. The runner's own seat start passes through once, in the subject's
  pane, before readiness; `launch.json` records in
  `peers.subject_start_via_launcher` whether Herdr's launch route reached the
  trial PATH. Any other start writes a request and waits up to 20 seconds for
  an answer; without one nothing runs. With an answer it execs the answer's
  argv in place, with the answer's environment plus the pane's Herdr and
  terminal variables, not its own.

After the before snapshot and before prompt delivery, `launch` starts
`runner.py watch` in the background (`peer_watch` in `launch.json`, log in
`peers.log`) and waits for its first completed poll. It answers each request
after these checks, and refuses otherwise:

- the request comes from its own pane, not the subject's, in the trial's
  Herdr workspace, with the fixture repository as its working directory;
- every trial environment variable except TERM and PATH is unchanged, so
  HOME, TMPDIR, the evaluator homes and the memory switches are the trial's
  (another, personal or symlinked home refuses);
- the arguments pass the same per-harness allowlist as a top-level seat, so
  resume, continue, profiles, config overrides, headless subcommands and
  personal paths refuse; approval flags are the subject's choice;
- for every Codex peer: the dedicated home still turns plugins off and holds
  no remote plugin cache;
- for Codex with `--codex-hook-trust=bypass`: the dedicated home, user and
  plugin hook sources and the fixture's shipped hooks are checked again, and
  the plugin inventory must equal the launch preflight's.

The answer is the exact argv and the trial environment: the real executable
found at launch, the subject's arguments, the kit's state-isolation arguments
(as for a top-level seat) and, only for a Codex peer under the bypass option,
the native flag. The bypass flag is never added to a Claude or Grok peer.

The watcher then reads `herdr agent list`. A pane is a peer once the watcher
allowed a start there, or once Herdr sees an agent working in the trial's
folders. Each harness process in a peer pane is a separate launch, identified
by pid, start time and a digest of its executable, argv, working directory
and every trial environment value except TERM and PATH. An exec keeps the
pid, so any change to the rest, an unreadable environment included, is a new
launch with no bound answer; the old launch is flagged and loses its ready
record, and going back to the old state does not restore it. Only the
launcher still waiting for its answer is not a launch: the interpreter pinned
at launch running `-B` and the launcher file, exactly as its first line
starts it. For
each launch the watcher reads the live process from the OS: on macOS the
executable, argv and environment through `sysctl` (`KERN_PROCARGS2`), the
start time through `ps` and the working directory through libproc's
`proc_pidinfo` (`PROC_PIDVNODEPATHINFO`); on Linux from `/proc`. `lsof` is not
used: it took 9 to 16 seconds for one process on a loaded host, longer than
the watcher's 10-second poll gap. A launch counts only when:

- an answer for that pane is bound to it: the request's pid and the answer's
  argv equal the live process's, and each answer binds once;
- the live executable, argv tail, working directory and environment pass the
  same checks as the request, with hook sources and plugins checked again.

A verified launch gets keys only for the dialogs the subject's own startup
accepts: the exact-path workspace-trust dialog and Claude's renderer prompt.
Before each key the watcher re-reads the screen and the live process; any
change to its identity, environment or working directory gets no key. It never
pastes text and never presses Enter in an editor. When Herdr reports the peer
idle, working or done, the watcher verifies the live process again, hook and
plugin checks included, and only then writes the pane's ready record, which
the trial's `herdr` waits for before it types into that pane. The watcher
keeps polling after readiness: a new process in the pane closes the ready
record and is a new launch, and a process that ends closes it too. The watcher
removes every ready record when it stops.

`finish` stops the watcher (`peers.stop`, then up to 20 seconds) before the
final snapshot and writes `peers` into `observation.json`, with the watcher's
poll errors and coverage gaps. These validity flags invalidate a trial:

| Flag | Meaning |
|---|---|
| `peer_launch_refused` | a start or a Herdr call was refused |
| `peer_launch_unanswered` | a request got no answer |
| `peer_launch_unobserved` | an answered start was never seen running |
| `peer_launch_unrecorded` | a seat ran in a peer pane without a bound answer: absolute path, another PATH, a raw Herdr call or a forged answer; it got no key |
| `peer_launch_unverified` | the live process failed verification, for example another home; it got no key |
| `peer_outside_trial_workspace` | an agent in the trial's folders ran in another workspace |
| `peer_startup_refused` | a hook review, an unknown trust screen, another path, or a process change before a key; no key sent |
| `peer_not_ready` | a verified peer never became ready |
| `peer_plugin_drift` | the plugin inventory changed by the peer's readiness; no ready record, so the trial's `herdr` sends it nothing |
| `peer_delivered_before_ready` | the trial's `herdr` let a delivery to the peer through before the watcher recorded it ready, or Herdr saw the unready peer working after it sat idle at its editor; working during startup does not count |
| `peer_delivery_refused` | the trial's `herdr` refused to type into a pane without a ready record |
| `peer_launcher_changed` | a launcher file or `context.json` changed |
| `peer_watch_incomplete` | the watcher did not cover the trial through finish: a failed poll, no completed poll, more than 10 seconds between polls or before the stop, its time limit, or no clean stop |

Accepting a Codex peer's folder-trust dialog makes Codex write a
`[projects."<path>"]` entry into the evaluator home's `config.toml`. A
top-level trial takes its `config_start` snapshot after its own trust
acceptance, so that write is already in the baseline. A peer accepts later,
so `finish` allows exactly one change: a `trust_level = "trusted"` entry for
the exact trial repository, with the rest of the file unchanged, after a
trust acceptance the watcher recorded for that path. `observation.json` then
lists it under `config_allowances`. Any other change is
`evaluator_config_drift`.

Without the bypass option a Codex peer meets its hook review; the watcher
leaves it unanswered and flags the trial. The launchers are not containment.
A seat started by another route can run before the watcher sees it and can
use the operator's own harness folders; the watcher flags it by pane, cwd and
live process, and only while it runs. A prompt sent through the real Herdr
binary bypasses the ready record and the trial's delivery log; the watcher
flags it as `peer_delivered_before_ready` only when Herdr reports the peer
working after it sat idle at its editor while still unready. The delivery log
is written by the subject's own process, so a subject that deletes it, or a
sandboxed shell that cannot write it, leaves no logged delivery. A process whose environment the OS does not show fails
verification; macOS hides it for its own platform binaries, such as
`/bin/sleep`. A subject that edits the launcher files is flagged, not
stopped. A peer working outside the trial's folders in the trial workspace
is not attributed. The watcher needs `ps` and, on macOS, libproc; the runner runs outside
the harness sandbox. Native dry runs by the primary are the evidence that
Herdr's real launch route reaches the trial PATH; the unit tests use fake
executables, a scripted Herdr and a scripted process table, plus one real
process read.

`observation.json` lists each peer's tab. After retaining its evidence, the
primary closes peer tabs the subject left open, as it closes the trial tab.

[grok-welcome]: https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/src/views/welcome/mod.rs
[claude-onboarding]: https://github.com/anthropics/claude-code/issues/67149
[codex-trust]: https://github.com/openai/codex/blob/main/codex-rs/tui/src/onboarding/trust_directory.rs
