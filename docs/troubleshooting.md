# Troubleshooting with doctor

<!-- Reference layer. Every check name, status, message and fix below is
     derived from crates/codeflow-core/src/doctor/mod.rs (CHECK_NAMES and
     check_registry) and crates/codeflow-cli/src/cmd/doctor.rs; re-derive this
     page when a check or its message changes. -->

## Concept

`codeflow doctor` runs twenty-five health checks against a repository and the tools around it.

It is for someone whose setup misbehaves after `codeflow init`, after
`codeflow update` or after a harness change.

- Doctor only reads and probes, so it changes no file in the repository and
  fixes nothing. Each message names the fix for you to apply.
- Live account and tool access stays with the harness, where an interactive
  canary proves it.

## Architecture

The twenty-five checks fall into six groups by the surface each one reads.

- **Git hooks, CI and host rules.** `hooks`, `ci-perimeter` and
  `remote-perimeter` read the hook subcommands of the installed binary, git's
  active hooks directory, the scaffolded CI workflow and, through `gh`, the
  host rules of the default branch.
- **Each harness's wiring.** `claude`, `codex` and `grok` look for each
  harness CLI on PATH and for its project hook files. `guard-timeouts`
  reads the CodeFlow guard entries in every hook file a harness reads for
  the shipped timeout and, in Claude settings, `"onFailure": "block"`.
  `startup-files` reads
  the project's Claude settings and Codex profile for the shell startup
  class, and the `source` lines of the home's startup files. `sandbox`
  reports the text rule the exec guard applies, read from the landed
  policy the way the hook reads it, and the state root's write grants.
- **Policy and config.** `config`, `permissions`, `policy-source` and
  `adopter-fit` read the `.codeflow/` directory, including `policy.json` and
  `project.toml`, whether it is writable, and which git ref the agent guards
  read policy from.
  `git-config-includes` names included Git configuration files that may be
  writable and reports a configuration it cannot verify. The operator
  inspects `git config --show-origin --includes --list` and repairs or
  secures its include paths.
- **Delegates, model bindings and network.** `delegates`, `model-bindings`,
  `delegate-roundtrip` and `network` probe the peer CLIs, the approved model
  binding records, the binary's delegate lifecycle and a lookup of github.com.
- **Repository and managed files.** `repo-integrity`, `managed-drift` and
  `id-registry` read git's repository and worktree state, the managed regions
  recorded at install and the fetched id registry refs.
- **Customization, instructions and test config.** `customization`,
  `instructions`, `reading` and `test-config` read the product, architecture
  and agent docs for scaffold placeholders, the sizes of the instruction files
  and installed skills, and the optional `.codeflow/test-config.json`.

## Technical

Doctor inspects the nearest directory at or above the current one that holds
`.git`. The checks run concurrently, and the report keeps the order below. One
failed check never hides the others. Each line starts with a status badge.

- `ok` means the check passed, or found nothing to inspect, such as a harness
  hook file that was never scaffolded.
- `warn` means something needs attention. It never changes the exit code.
- `FAIL` means the check found a broken state.

Doctor exits 0 when no check failed, with or without warnings, and 1 when any
check failed. Eleven checks can fail: `hooks`, `guard-timeouts`, `sandbox`,
`config`, `permissions`, `policy-source`, `model-bindings`,
`delegate-roundtrip`, `repo-integrity`, `id-registry` and `adopter-fit`. The
other fourteen warn at most.

- `codeflow doctor --list` prints the twenty-five names and exits 0.
- `codeflow doctor --check <name>` runs one check. An unknown name prints an
  error to stderr and exits 1.

Each row below is one check, in the order doctor prints it.

| Check | What it reads | Fail or warn means | Fix |
|---|---|---|---|
| `hooks` | Asks the `codeflow` on PATH for its hook contract (`git-hook capabilities` must print `hooks 3`) and to answer `--help` for eight hook subcommands (`hook git-guard`, `hook session-orient`, `hook session-summary` and the five `git-hook` stages), then compares git's active hooks directory with `.codeflow/git-hooks` | Fail: `codeflow` is not on PATH, is older than hook contract 3, or a listed subcommand did not answer. Warn: `core.hooksPath` is absolute, the shims are not git's active hooks (often a fresh clone), or init recorded `git_hooks = "unwired"` because another hook manager owns the hooks | Install the binary on PATH ([adoption](adoption.md)). Run a listed subcommand with `--help` to see its error. For the wiring warnings run `git config core.hooksPath .codeflow/git-hooks`, or with another manager call the `.codeflow/git-hooks/` shims from its stages |
| `claude` | Whether the `claude` CLI is on PATH | Warn: not found, so the Claude-layer hooks are inactive. Git hooks and CI still enforce | Put the `claude` CLI on PATH if you use Claude Code. Without it the warning is expected |
| `codex` | `.codex/hooks.json`, whether `codex` is on PATH, and the hook trust Codex records in its own state | Warn when Codex runs fewer than all the hooks, because one is untrusted, changed or disabled. Note when all are trusted and enabled (configured; a live run is not verified), or when the trust could not be read, such as in a linked worktree. Ok when the file is absent | Once, run `/hooks` inside interactive Codex and approve and enable the CodeFlow hooks; the check then reports a note |
| `grok` | Any `.grok/hooks/*.json` file, whether `grok` is on PATH, Grok's folder trust store, and the CodeFlow hook commands in the hook files Grok reads. It also reads the answer the exec-guard in doctor's own process gives a fixed canary dangerous command under the catastrophic-command floor, with no policy or repository read | Warn when Grok does not trust the folder or cannot read its trust store, when a CodeFlow hook command carries a `$` Grok skips, or when the shipped shell guard does not refuse the canary with exit 2, a reason and Grok's deny answer. Note when the folder is trusted or folder trust is off (configured; a live run is not verified). A customised shell guard handler is reported unverified and never run. Where PATH resolves `codeflow` is reported, never run. Ok when no hook file exists | Once, run `/hooks-trust` in Grok, or start it with `--trust`, so the project hooks load; the check then reports a note. For a hook command, follow the printed step: `codeflow update`, resolving any `.new` file it writes, or a hand edit of a file update does not rewrite, for the reason doctor names. With stale files of both kinds, take both steps. For a missing shell guard that update would not bind again, add the guard group doctor quotes to `.grok/hooks/codeflow.json` |
| `guard-timeouts` | The CodeFlow guard hook entries (git-guard, exec-guard and edit-guard) in `.claude/settings.json`, `.claude/settings.local.json`, `.grok/hooks/*.json` and `.codex/hooks.json` | Warn when a guard entry has no `timeout`, a timeout other than the shipped 30 seconds, or, in Claude settings, no `"onFailure": "block"`, since a guard that does not answer in time can then let the tool call through. Ok otherwise; when Grok or Codex hook files hold guards, the message says those harnesses have no fail-closed setting, so a guard that cannot reach its own 20 second deadline fails open there after the timeout. Fail when a hook file cannot be read or parsed | Run `codeflow update`, resolving any `.new` file it writes in favour of the shipped fields. Set the fields by hand in a file update does not manage, such as `.claude/settings.local.json`. Codex asks you to approve a changed hook again in `/hooks` |
| `startup-files` | The shell startup class (issue 86) and the default git configuration files in `.claude/settings.json` (the `Edit` denies and the sandbox `denyWrite` entries) and in the profile `.codex/config.toml` selects and in `cf-builder`, read through `extends` with the nearest entry winning, as listed rules; whether `ZDOTDIR` or `XDG_CONFIG_HOME` is moved from its default; then the `source` and `.` lines of `~/.bashrc`, `~/.bash_profile`, `~/.profile`, `~/.zshenv`, `~/.zprofile` and `~/.zshrc`, each read once up to 256 KiB and never followed | Warn: a settings file is absent, unreadable or lacks entries of the class, or the Codex config selects another profile, sets `sandbox_mode`, or lets the selected profile or `cf-builder` write a class path or a path above or below one, in any spelling of the home, so that harness's sandbox does not deny every startup write. Warn: `ZDOTDIR` or `XDG_CONFIG_HOME` is moved or relative, so the startup files there have no generated deny. Warn: a startup file sources a literal path outside the class, or a path the check cannot read (a variable other than `$HOME`, a glob or a substitution), where an agent could plant a definition no rule protects. This check never fails doctor, and it cannot observe whether a harness enforces the rules it lists | For the settings, run `codeflow update`, resolving any `.new` file it writes. For a write grant in a profile, remove it from `.codex/config.toml`. For a moved directory, add deny rules for it to the harness settings yourself, or set the variable back to its default. All findings print together. For a sourced file, move what the named line sources into a protected startup path such as `~/.zsh/`, `~/.bashrc.d/` or `~/.config/fish/conf.d/`, or inline it |
| `sandbox` | The text rule the exec guard applies (ADR-0088), through the function the hook uses, for a session started at the project root; the policy; the CodeFlow state root against the project, the temporary directory and every Claude settings layer's `allowWrite` and `additionalDirectories` | Fail: the policy cannot be read, or a Claude write grant reaches the state root. Note: `text rule: strict` with the reason, or `text rule: relaxed by operator choice, not verified` with the ref it was read from, which is the remote-tracking state as last fetched; either way, the level each kind of finding takes; Codex and Grok write grants are not checked | Nothing for the strict rule. To relax it, the operator lands `security.text_guard: relaxed` in `.codeflow/policy.json` on every protected ref the hook reads, then fetches; a `relaxed` in the working copy, a local branch or tag, or a repository without a remote is not honoured |
| `config` | Every `.json` file under `.codeflow/`, parsed recursively | Warn: `.codeflow/` does not exist. Fail: a listed file does not parse, or the directory could not be scanned | Run `codeflow init` when the directory is missing. Repair the JSON in each listed file |
| `permissions` | The owner write bit on `.codeflow/`, on Unix only, then whether this process can write the directories essential and full gates write outside the worktree: the lock directories (`locks/` under the CodeFlow home and `codeflow/` under the git common directory) and `gate-runs/` under the CodeFlow home. Says so when it runs inside the Claude Code sandbox (`SANDBOX_RUNTIME` set). Also reports what the `cf-builder` and `cf-guard` profiles in `.codex/config.toml` grant on `~/.codeflow/locks` and `~/.codeflow/gate-runs`, following `extends` and counting a grant on a covering folder; that report reads configuration and never changes the result. Inside a Codex seat (`CODEX_SANDBOX` set), an unwritable `gate-runs/` is reported as by design, since a seat sandbox cannot keep gate evidence there | Fail: `.codeflow/` is not writable. Warn: a named gate directory is not writable, so `codeflow test --mode essential` or `--mode full` refuses with `gate lock unavailable` or fails to keep its evidence | Give the owner write permission on `.codeflow/`. For a gate directory, inside the Claude sandbox the operator adds it to `sandbox.filesystem.allowWrite` (the shipped presets allow `~/.codeflow/locks` and `~/.codeflow/gate-runs`, and `codeflow update` adds them), or runs the gate outside the sandbox. Inside a Codex seat (`CODEX_SANDBOX` set), a review seat writes neither folder by design, so `codeflow reserve` and quick-mode heavy targets run in a builder seat relaunched with the builder launch in cf-model-orchestrator's `resources/routing/transport.md`; `codeflow doctor --preflight` checks that launch |
| `policy-source` | Where the agent guards read `.codeflow/policy.json` and `.codeflow/project.toml`: the remote's default branch as last fetched, named by `refs/remotes/<remote>/HEAD`, at one commit; `HEAD` (or the working copy on an unborn `HEAD`) only for initialization, a remote that advertises no refs, or local only, no remote. It never fetches; with no tracking refs it asks the remote whether it has any | Fail: the source cannot be read, such as several remotes without an `origin`, an unset remote HEAD or one that names no tracking branch, a dangling remote HEAD, or no tracking refs for a remote that has refs. Ok names the authority ref and its commit, or the labelled state, and any local policy drift | Run `git fetch`. For an unset remote HEAD, the operator runs `git remote set-head <remote> -a` ([enforcement planes](architecture/enforcement-planes.md#in-session-guards)) |
| `network` | Whether `host` is on PATH, then a two-second lookup of github.com. When `SANDBOX_RUNTIME` is set, as in the Claude Code sandbox, which has no direct DNS and carries traffic through its proxy, an HTTPS request to github.com with `curl` instead, which passes only on an HTTP 2xx or 3xx answer | Warn: the probe tool is missing so the probe was skipped, or the probe failed; the message quotes the error or the HTTP status, such as a 403 from a proxy that refuses the host | Nothing is required. Install the probe tool to enable the probe, or restore the connection |
| `delegates` | `codex` and `claude` on PATH, `codex login status`, `codex mcp list`, `claude mcp list`, `claude plugin list --json` for an enabled `codex@openai-codex` plugin, and `tmux` on PATH | Warn: cross-vendor delegation is partly unavailable. It is optional, and the message lists each gap | Apply the step each gap names: `codex login`, run the named list command, install `codex@openai-codex` in Claude Code and run `/codex:setup`, or put the missing tool on PATH. Then confirm access with an interactive canary, since status output alone does not prove it. When `SANDBOX_RUNTIME` is set, the presets deny sandboxed commands the Codex auth file, so a failed `codex login status` that does not say it is signed out is a note that quotes the failure and leaves the sign-in unconfirmed: run `codeflow doctor --check delegates` outside the sandbox to check it. An explicit "Not logged in" is a sign-in to fix there too |
| `model-bindings` | Approved binding records in `~/.codeflow/qualified-bindings/` (or under `CODEFLOW_HOME`), `.codeflow/model-selection.json`, each harness's version probe and the recorded settings digests. It also lists the managed worker routes without launching a model | Fail: a binding record or the project selection is invalid, or a selected binding drifted, in which case no override applies. Warn: a binding that is not selected drifted, or a harness version has no external probe. Ok with no local bindings | Requalify a drifted binding with `/cf-evaluate-model`, or correct `.codeflow/model-selection.json`. Leave that file absent or empty to use the shipped ensemble ([model upgrades](model-upgrades.md)) |
| `delegate-roundtrip` | Drives the `codeflow` on PATH through a synthetic delegate lifecycle (init, ready, arm, accepted, terminal) in a private temporary directory outside the repository, removed afterwards | Fail: `codeflow` is not on PATH, the temporary workspace could not be created, or a lifecycle stage failed. The message names the stage | Install the current `codeflow` build on PATH and rerun. The check passes only against an installed build that has the lifecycle ([capabilities](capabilities.md)) |
| `repo-integrity` | `git rev-parse --is-bare-repository`, the `.git` entry, and `git worktree list` against the protected branches in `policy.json` | Fail: `core.bare=true` on a repository that has a working tree, or a protected branch is checked out in a linked worktree | Run `git config core.bare false`. Switch the named worktree to its feature branch |
| `ci-perimeter` | The CI workflow recorded at install, by default `.github/workflows/codeflow-ci.yml` | Warn: its Install codeflow step is still the shipped placeholder, so the CI test and validate gates fail red and enforce nothing. Ok when no workflow exists | Replace the placeholder step with the release installer or a from-source build |
| `remote-perimeter` | Through `gh`, the GitHub `origin`'s default branch: the rules its rulesets apply (`gh api repos/<owner>/<repo>/rules/branches/<branch>`), its classic protection and the bypass lists of the rules that require checks, against `git.required_checks`; every repository ruleset, the `codeflow-authority` environment and the default branch's workflows | Fail: "codeflow authority checks" or "codeflow line checks" lists a bypass actor, a recovery window left open. Warn: the branch requires no checks, no rule that requires checks requires the branch to be up to date, a name in `git.required_checks` is not required, or every rule that requires a listed name, or the up-to-date setting, lets administrators or listed actors bypass it; classic protection does not bind administrators; with `--profile`, the owned rulesets differ from the profile; a line rule is not strict or no rule requires a pull request on the lines; an `authority:` check is accepted from GitHub Actions; the authority environment admits more than the default branch or a second job references it; a foreign ruleset requires a name no workflow on the default branch produces. Note: no GitHub `origin`, no `gh`, a policy file that does not parse, a `git.required_checks` list the schema refuses (empty, a blank name or the wrong type) or a misspelt key in its place, or the host rules, the classic protection or a bypass list that decides the answer cannot be read. Only a 404 counts as no classic protection; any other failure to read it is a note unless the rulesets alone meet the policy. A schema error on another key does not stop the check; doctor names it beside the answer | Run `codeflow remote protect`, or have the repository owner turn on "Require branches to be up to date before merging" with the named checks. Without it, two pull requests each green on an older base can merge and leave the branch red. A failure closes with `codeflow remote protect`; a foreign ruleset is retired on the host ([remote protection](policy-reference.md#remote-protection)) |
| `managed-drift` | Each managed-region file in `.codeflow/manifest.json`, hashing the block between its codeflow markers against the recorded hash | Warn: a listed region was edited inside its markers, and the next `codeflow update` will regenerate it and lose the edit | Move the edit outside the markers, where the content is project-owned |
| `customization` | `docs/product.md` and `docs/architecture.md` for scaffold placeholders such as `{{PRODUCT_PURPOSE}}`, and `AGENTS.md` for its placeholder comment | Warn: a listed file still holds a placeholder, or one of the two docs is missing. Ok when neither doc exists, as on the minimal tier | Run `/cf-customize` |
| `instructions` | The `AGENTS.md` chain Codex loads for each directory that holds one, from the root down to that directory, against Codex's instruction limit: `project_doc_max_bytes` from the project's `.codex/config.toml` when set, else 32 KiB | Warn: a chain is over the limit, so Codex cuts its end, where the project section lives. Ok when there is no `AGENTS.md` | Move project detail out of the project section of `AGENTS.md` into files it points at, or raise `project_doc_max_bytes` in `.codex/config.toml` |
| `reading` | The kernel (the managed block of `AGENTS.md`) and the per-task reading chain of the installed shipped skills in bytes, each shipped skill and agent body and the always-loaded instruction set (`CLAUDE.md` with its imports, or `AGENTS.md`) in lines, against advisory guidelines (ADR-0083) or the project's `[reading]` values in `.codeflow/project.toml`; each shipped description against the API's 1,024-character limit | Note: a measure is above its advisory guideline; the note states the basis. Warn: a description is over the API limit, or the `[reading]` section does not read. Fail: `AGENTS.md`, the skill tree, `.codex/config.toml`, the always-loaded instructions or a shipped agent file exists but cannot be read, and the message names it. A size is never a failure: it is a note or a pass, and within every number the check passes and states them. The chain is information when the installed skills' read edges differ from the map this binary ships | Move detail in the named file or skill behind a trigger (an index entry or a conditional read), never cutting a duty; shorten a long description; repair the named `[reading]` key |
| `test-config` | `.codeflow/test-config.json`, through the test engine's own health checks: schema, each target's `cwd`, command parsing, paths, globs, coverage transforms and runner probes | Warn: a listed sub-check warned or failed. This check never fails doctor. Ok when the file is absent | Correct the named entries in `.codeflow/test-config.json`, or replace it with `codeflow test setup --template <name> --replace`, then run `codeflow test` |
| `id-registry` | The shared id registry on the fetched refs, when durable work tracking is on; it never fetches | Fail: the registry cannot be read or shows damage. Warn: an id no ref can place, tracking state that cannot be read, or a registry without host rules (reduced assurance). Ok when tracking is off | Resolve each finding named (`codeflow ids admit` registers an unplaced id, `codeflow ids retarget <id>` renumbers a collision), then confirm with `codeflow ids check`. Run `codeflow remote protect` to apply the host rules |
| `adopter-fit` | The effective `git.pr_sections` level and its origin, a kept PR template's pending mapping decision, and the `[release] backend` in `.codeflow/project.toml` against the release tools found | Fail: the release backend is invalid or unreadable. Warn: a mapping decision is pending, policy provenance does not parse, or the backend and a detected release tool disagree | Record the decision in `git.pr_section_mapping.decided` (accepted, refused or custom), or set `[release] backend` to the one version authority the project uses, then rerun `codeflow doctor --check adopter-fit` |

### Lock for heavy work

Essential and full gates hold both `full-gate.lock` files for the whole run:
`locks/full-gate.lock` under the CodeFlow home and `codeflow/full-gate.lock`
under the git common directory. The home lock spans repositories; the common
directory lock spans a repository's worktrees. A second gate refuses before
starting a target and names the holder's process, mode and directory.
`gate lock unavailable` means a directory cannot be opened or written;
`codeflow doctor --check permissions` names the paths to repair.

Quick gates can run light targets while another gate holds the lock. Declare
`"heavy": true` on targets that compile or run substantial parallel work, or
`"heavy": false` for light checks. Undeclared targets in a configured quick
mode use `execution.quick_default_class`, which accepts `"light"` (the
default) or `"heavy"`. A project with substantial undeclared quick suites
can set it to `"heavy"` and set `execution.heavy_wait_seconds` to a bounded
wait. An explicit target declaration
takes precedence. When there are no quick targets and quick falls back to
essential, undeclared targets are heavy. Heavy quick targets wait up to
`--wait-seconds` or `execution.heavy_wait_seconds` (default zero), then
refuse if the lock is still held. This repository waits up to 1800 seconds.
Waiting quick runs and reserves take the lock in arrival order: each
queues a ticket in the `waiters` folder beside the CodeFlow home lock, and
a later caller is refused while an earlier waiter's ticket is live. A
ticket of an exited process, or past its deadline, is removed; tickets
that arrived at the same instant go in process-id order. An unreadable
queue refuses. A ticket name that does not parse refuses once and is
renamed aside as `quarantined-<time>-<name>`, so it cannot block the queue;
if it cannot be renamed, every waiter refuses until it is removed. Full
and essential gates do not queue. The result names the light targets that ran. Pre-push runs no
quick target, so it never waits for the lock.

Read the holder's process, worktree, mode and start time with
`codeflow test --lock-status`. This reads both records without taking a
lock, changing a record or starting a target. A killed gate can leave a
stale record; the displayed snapshot does not replace admission's OS-lock
and live-process-group checks.

Run a direct heavy command with `codeflow reserve -- cargo build` to take
the same locks. `--wait-seconds` sets a bounded wait, and
`--timeout-seconds` sets the command deadline. Arguments after `--` reach
the executable unchanged. The wrapper ends its command's process group
when it exits or is killed. Use the worktree's own Cargo target directory.

On Unix, `codeflow reserve -- codeflow test --mode quick` runs the quick
run's heavy targets under the reserve's lock. The reserve hands its lock
descriptors only when the command is the same executable file as the
running `codeflow` (same device and inode after resolving links, whether
named by path or found on `PATH`); any other program, including one that
is only named `codeflow`, gets none. The receiving `codeflow` makes them
close-on-exec so no target or daemon keeps them, and uses them only after
the OS lock shows they are the holder's own; a value set any other way
falls back to ordinary admission. The nested run adds its targets'
process groups to the holder's record and leaves those still running
there when it ends, so if the nested run is killed and the reserve ends
while a target survives, the next gate refuses until that target exits. Each rewrite of the holder record holds the OS lock of
`full-gate.record.lock` beside it from read to rename, so the reserve and
the nested run never drop a group line the other just added. If that lock
cannot be taken within five seconds, the rewrite runs without it, as
before this lock existed, and the gate reports the missed serialization.
Full and essential gates under a reserve still take
the lock themselves and refuse. On Windows a nested quick run waits or
refuses as a direct one does.

`execution.worker_budget`, when configured, divides workers among ready
heavy targets and records their Cargo, nextest, Rust-test and shell-oracle
counts. A heavy target can set `workers` to wait until that share is free;
otherwise it shares the available workers with other ready heavy targets.
Light targets can run beside them. Omit the budget to retain tool
defaults. This repository sets nine workers and gives clippy a separate
`target/clippy` directory; its binary build waits for the all-target build.
Each Cargo producer takes the full budget to avoid package-cache and
build-directory waits; light checks still run beside it.

The unchanged lock names also exclude full runs of a 3.1 binary. Essential
runs of 3.1 take no lock and can still overlap. Direct commands outside the
wrapper and other applications can also consume resources: the lock does
not make the machine otherwise quiet. Declared heavy targets and reserved
commands end their process groups when the controller exits. On Unix,
other targets can outlive a killed gate; the lock refuses new heavy work
until they end.

### Fix a failing check

Work one check at a time, starting with the `FAIL` lines.

1. Run `codeflow doctor` from inside the repository, or run one check with
   `codeflow doctor --check <name>`.
2. Read the message after the check name. It says what the check found and,
   for most checks, the command that fixes it.
3. Apply that fix, or the one in the table above.
4. Rerun the same check with `codeflow doctor --check <name>`.

The fix worked when that check prints `ok`, or when a full run exits 0 with no
`FAIL` line. After a completed trust step the `codex` and `grok` checks
report a note, since doctor cannot see a live harness run.

## A cancelled standalone task has no planning anchor

If a standalone task was cancelled before its record reached the target,
record the decision in a `plan/` pull request. On the task branch, run
`codeflow task status TSK-NNN cancelled --reason <why> --scope <where it went>`.
Cut the planning branch from the target and copy only the cancelled record
onto it, retaining the Closeout's `- cancelled:` and `- scope:` lines. Set
`Task: TSK-NNN` in the PR body. The range must stay inside
`project-management/`; it must not bring the abandoned product commits.

`codeflow ci` and pre-push accept this planning amendment without a
`codeflow work start` claim and report its task under `no epic`. This route
requires the record to be absent from the merge-base. An active task still
uses its own task branch and `work start`; a record already on the target
uses the ordinary task status route. A push has no `Task:` line yet, so
pre-push refuses only a new cancelled record without its reason or scope,
or a path a planning amendment cannot carry; the pull request check judges
the rest.

## A push is blocked by a quick target

A `git.test_gate_on_push` failure names each failed target and its cause:
an exit code, a timeout with its limit, a termination signal on Unix, or a
runner error. Run `codeflow test --mode quick` to reproduce the failure,
then fix the named target before pushing again. A timeout uses that target's
`timeout_seconds` value in `.codeflow/test-config.json`, or 600 seconds when
it is omitted. Check why the command is slow before increasing the limit.

If all target commands pass but tracked files change during the gate, the
block names those files. Review and commit the intended generated changes,
then rerun the gate on the resulting checkout. A gate error names the error
itself, including failures that occur after the configuration has loaded.

## Every command is refused because the policy authority is unresolved

The guards read policy from the remote's default branch. When
`refs/remotes/origin/HEAD` is unset or unreadable they refuse commands and
edits, naming the ref. Read-only git (`git status`, `log`, `diff`, `show`,
`rev-parse` and the other commands in the
[policy reference](policy-reference.md#read-only-git-while-the-policy-authority-is-unresolved))
still passes, so you can inspect the repository. To clear it, the operator
runs `git remote set-head origin -a`, or `git fetch origin` on git 2.48 or
later; `codeflow doctor --check policy-source` shows the state.
