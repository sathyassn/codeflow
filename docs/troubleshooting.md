# Troubleshooting with doctor

<!-- Reference layer. Every check name, status, message and fix below is
     derived from crates/codeflow-core/src/doctor/mod.rs (CHECK_NAMES and
     check_registry) and crates/codeflow-cli/src/cmd/doctor.rs; re-derive this
     page when a check or its message changes. -->

## Concept

`codeflow doctor` runs twenty health checks against a repository and the tools around it.

It is for someone whose setup misbehaves after `codeflow init`, after
`codeflow update` or after a harness change.

- Doctor only reads and probes, so it changes no file in the repository and
  fixes nothing. Each message names the fix for you to apply.
- Live account and tool access stays with the harness, where an interactive
  canary proves it.

## Architecture

The twenty checks fall into six groups by the surface each one reads.

- **Git hooks and CI.** `hooks` and `ci-perimeter` read the hook subcommands
  of the installed binary, git's active hooks directory and the scaffolded CI
  workflow.
- **Each harness's wiring.** `claude`, `codex` and `grok` look for each
  harness CLI on PATH and for its project hook files.
- **Policy and config.** `config`, `permissions`, `policy-source` and
  `adopter-fit` read the `.codeflow/` directory, including `policy.json` and
  `project.toml`, whether it is writable, and which git ref the agent guards
  read policy from.
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
check failed. Nine checks can fail: `hooks`, `config`, `permissions`,
`policy-source`, `model-bindings`, `delegate-roundtrip`, `repo-integrity`,
`id-registry` and `adopter-fit`. The other eleven warn at most.

- `codeflow doctor --list` prints the twenty names and exits 0.
- `codeflow doctor --check <name>` runs one check. An unknown name prints an
  error to stderr and exits 1.

Each row below is one check, in the order doctor prints it.

| Check | What it reads | Fail or warn means | Fix |
|---|---|---|---|
| `hooks` | Asks the `codeflow` on PATH for its hook contract (`git-hook capabilities` must print `hooks 3`) and to answer `--help` for eight hook subcommands (`hook git-guard`, `hook session-orient`, `hook session-summary` and the five `git-hook` stages), then compares git's active hooks directory with `.codeflow/git-hooks` | Fail: `codeflow` is not on PATH, is older than hook contract 3, or a listed subcommand did not answer. Warn: `core.hooksPath` is absolute, the shims are not git's active hooks (often a fresh clone), or init recorded `git_hooks = "unwired"` because another hook manager owns the hooks | Install the binary on PATH ([adoption](adoption.md)). Run a listed subcommand with `--help` to see its error. For the wiring warnings run `git config core.hooksPath .codeflow/git-hooks`, or with another manager call the `.codeflow/git-hooks/` shims from its stages |
| `claude` | Whether the `claude` CLI is on PATH | Warn: not found, so the Claude-layer hooks are inactive. Git hooks and CI still enforce | Put the `claude` CLI on PATH if you use Claude Code. Without it the warning is expected |
| `codex` | `.codex/hooks.json`, whether `codex` is on PATH, and the hook trust Codex records in its own state | Warn when Codex runs fewer than all the hooks, because one is untrusted, changed or disabled. Note when all are trusted and enabled (configured; a live run is not verified), or when the trust could not be read, such as in a linked worktree. Ok when the file is absent | Once, run `/hooks` inside interactive Codex and approve and enable the CodeFlow hooks; the check then reports a note |
| `grok` | Any `.grok/hooks/*.json` file, whether `grok` is on PATH, Grok's folder trust store, and the CodeFlow hook commands in the hook files Grok reads. It also reads the answer the exec-guard in doctor's own process gives a fixed canary dangerous command under the catastrophic-command floor, with no policy or repository read | Warn when Grok does not trust the folder or cannot read its trust store, when a CodeFlow hook command carries a `$` Grok skips, or when the shipped shell guard does not refuse the canary with exit 2, a reason and Grok's deny answer. Note when the folder is trusted or folder trust is off (configured; a live run is not verified). A customised shell guard handler is reported unverified and never run. Where PATH resolves `codeflow` is reported, never run. Ok when no hook file exists | Once, run `/hooks-trust` in Grok, or start it with `--trust`, so the project hooks load; the check then reports a note. For a hook command, follow the printed step: `codeflow update`, resolving any `.new` file it writes, or a hand edit of a file update does not rewrite, for the reason doctor names. With stale files of both kinds, take both steps. For a missing shell guard that update would not bind again, add the guard group doctor quotes to `.grok/hooks/codeflow.json` |
| `config` | Every `.json` file under `.codeflow/`, parsed recursively | Warn: `.codeflow/` does not exist. Fail: a listed file does not parse, or the directory could not be scanned | Run `codeflow init` when the directory is missing. Repair the JSON in each listed file |
| `permissions` | The owner write bit on `.codeflow/`, on Unix only, then whether this process can write the directories a full gate writes outside the worktree: the lock directories (`locks/` under the CodeFlow home and `codeflow/` under the git common directory) and `gate-runs/` under the CodeFlow home. Says so when it runs inside the Claude Code sandbox (`SANDBOX_RUNTIME` set) | Fail: `.codeflow/` is not writable. Warn: a named gate directory is not writable, so `codeflow test --mode full` refuses with `gate lock unavailable` or fails to keep its evidence | Give the owner write permission on `.codeflow/`. For a gate directory, inside the Claude sandbox the operator adds it to `sandbox.filesystem.allowWrite` (the shipped presets allow `~/.codeflow/locks` and `~/.codeflow/gate-runs`, and `codeflow update` adds them), or runs the full gate outside the sandbox |
| `policy-source` | Where the agent guards read `.codeflow/policy.json` and `.codeflow/project.toml`: the remote's default branch and declared target as last fetched, `HEAD` before any tracking ref exists, or the working copy on an unborn `HEAD`. It never fetches | Fail: the source cannot be read, such as several remotes without an `origin`, a missing authority ref or a dangling remote HEAD. Ok names the source and any local policy drift | Run `git fetch`. For an unset or custom default branch, the operator runs `git remote set-head <remote> --auto` ([enforcement planes](architecture/enforcement-planes.md#in-session-guards)) |
| `network` | Whether `host` is on PATH, then a two-second lookup of github.com. When `SANDBOX_RUNTIME` is set, as in the Claude Code sandbox, which has no direct DNS and carries traffic through its proxy, an HTTPS request to github.com with `curl` instead, which passes only on an HTTP 2xx or 3xx answer | Warn: the probe tool is missing so the probe was skipped, or the probe failed; the message quotes the error or the HTTP status, such as a 403 from a proxy that refuses the host | Nothing is required. Install the probe tool to enable the probe, or restore the connection |
| `delegates` | `codex` and `claude` on PATH, `codex login status`, `codex mcp list`, `claude mcp list`, `claude plugin list --json` for an enabled `codex@openai-codex` plugin, and `tmux` on PATH | Warn: cross-vendor delegation is partly unavailable. It is optional, and the message lists each gap | Apply the step each gap names: `codex login`, run the named list command, install `codex@openai-codex` in Claude Code and run `/codex:setup`, or put the missing tool on PATH. Then confirm access with an interactive canary, since status output alone does not prove it. When `SANDBOX_RUNTIME` is set, the presets deny sandboxed commands the Codex auth file, so a failed `codex login status` that does not say it is signed out is a note that quotes the failure and leaves the sign-in unconfirmed: run `codeflow doctor --check delegates` outside the sandbox to check it. An explicit "Not logged in" is a sign-in to fix there too |
| `model-bindings` | Approved binding records in `~/.codeflow/qualified-bindings/` (or under `CODEFLOW_HOME`), `.codeflow/model-selection.json`, each harness's version probe and the recorded settings digests. It also lists the managed worker routes without launching a model | Fail: a binding record or the project selection is invalid, or a selected binding drifted, in which case no override applies. Warn: a binding that is not selected drifted, or a harness version has no external probe. Ok with no local bindings | Requalify a drifted binding with `/cf-evaluate-model`, or correct `.codeflow/model-selection.json`. Leave that file absent or empty to use the shipped ensemble ([model upgrades](model-upgrades.md)) |
| `delegate-roundtrip` | Drives the `codeflow` on PATH through a synthetic delegate lifecycle (init, ready, arm, accepted, terminal) in a private temporary directory outside the repository, removed afterwards | Fail: `codeflow` is not on PATH, the temporary workspace could not be created, or a lifecycle stage failed. The message names the stage | Install the current `codeflow` build on PATH and rerun. The check passes only against an installed build that has the lifecycle ([capabilities](capabilities.md)) |
| `repo-integrity` | `git rev-parse --is-bare-repository`, the `.git` entry, and `git worktree list` against the protected branches in `policy.json` | Fail: `core.bare=true` on a repository that has a working tree, or a protected branch is checked out in a linked worktree | Run `git config core.bare false`. Switch the named worktree to its feature branch |
| `ci-perimeter` | The CI workflow recorded at install, by default `.github/workflows/codeflow-ci.yml` | Warn: its Install codeflow step is still the shipped placeholder, so the CI test and validate gates fail red and enforce nothing. Ok when no workflow exists | Replace the placeholder step with the release installer or a from-source build |
| `managed-drift` | Each managed-region file in `.codeflow/manifest.json`, hashing the block between its codeflow markers against the recorded hash | Warn: a listed region was edited inside its markers, and the next `codeflow update` will regenerate it and lose the edit | Move the edit outside the markers, where the content is project-owned |
| `customization` | `docs/product.md` and `docs/architecture.md` for scaffold placeholders such as `{{PRODUCT_PURPOSE}}`, and `AGENTS.md` for its placeholder comment | Warn: a listed file still holds a placeholder, or one of the two docs is missing. Ok when neither doc exists, as on the minimal tier | Run `/cf-customize` |
| `instructions` | The `AGENTS.md` chain Codex loads for each directory that holds one, from the root down to that directory | Warn: a chain is over Codex's 32 KiB instruction limit, so Codex cuts its end, where the project section lives. Ok when there is no `AGENTS.md` | Move project detail out of the project section of `AGENTS.md` into files it points at |
| `reading` | The kernel (the managed block of `AGENTS.md`), the per-task reading chain of the installed skills and each shipped skill, each against its guideline number | Warn: a measure is above its guideline. Sizes are guidelines, so this never fails; within them the check passes and states the numbers | Move detail in the named file or skill behind a trigger (an index entry or a conditional read), never cutting a duty |
| `test-config` | `.codeflow/test-config.json`, through the test engine's own health checks: schema, each target's `cwd`, command parsing, paths, globs, coverage transforms and runner probes | Warn: a listed sub-check warned or failed. This check never fails doctor. Ok when the file is absent | Correct the named entries in `.codeflow/test-config.json`, or replace it with `codeflow test setup --template <name> --replace`, then run `codeflow test` |
| `id-registry` | The shared id registry on the fetched refs, when durable work tracking is on; it never fetches | Fail: the registry cannot be read or shows damage. Warn: an id no ref can place, tracking state that cannot be read, or a registry without host rules (reduced assurance). Ok when tracking is off | Resolve each finding named (`codeflow ids admit` registers an unplaced id, `codeflow ids retarget <id>` renumbers a collision), then confirm with `codeflow ids check`. Run `codeflow remote protect` to apply the host rules |
| `adopter-fit` | The effective `git.pr_sections` level and its origin, a kept PR template's pending mapping decision, and the `[release] backend` in `.codeflow/project.toml` against the release tools found | Fail: the release backend is invalid or unreadable. Warn: a mapping decision is pending, policy provenance does not parse, or the backend and a detected release tool disagree | Record the decision in `git.pr_section_mapping.decided` (accepted, refused or custom), or set `[release] backend` to the one version authority the project uses, then rerun `codeflow doctor --check adopter-fit` |

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
