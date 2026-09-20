# CodeFlow command line reference

<!-- Reference layer. Every purpose line, flag and argument below is the
     binary's own `--help` text; re-derive this page from `codeflow <cmd>
     --help` when the surface changes. -->

## Concept

**One binary, twenty two subcommands, and every one of them does a job the
guide already names.**

```cf-stage
scaffold | init · update @accent
->
enforce | hook · git-hook · ci · policy · remote
->
verify | test · validate · integrate · doctor
->
remember | orient · status · recall
->
plan | epic · spec · task · work · estimate
->
delegate and present | delegate · portal · present @positive
caption: the same six jobs the capabilities page uses, read as commands
```

`codeflow help <command>` prints the same surface this page lists, and
`codeflow <command> --help` prints the flags. Architecture is the one-line
purpose of each subcommand and when you reach for it; Technical is the exact
arguments, flags and exit behaviour.

## Architecture

Purposes are the binary's own short help. Nothing here is a wrapper around a
second implementation: the hooks, CI, and the in-session guards all call the
same functions through this surface.

| Subcommand | Purpose | When |
|---|---|---|
| `init` | Scaffold this project (idempotent, non-destructive, offline) | Once per repository, run from the intended project root |
| `update` | Refresh managed scaffold files (3-way merge; never clobbers) | After upgrading the binary, per repository |
| `hook` | Claude-layer hooks, wired by the settings presets (charter §3.3) | Never by hand: the settings presets invoke it |
| `git-hook` | Git client hook target, the `.codeflow/git-hooks/` shims exec this | Never by hand: the installed shims invoke it |
| `ci` | Verify a commit range + branch name against policy, the portable, binary-sourced CI check | In CI, from the scaffolded workflow |
| `policy` | Inspect `.codeflow/policy.json` | When you need a key's schema or the effective value and its source |
| `remote` | Remote provider operations (branch protection) | Once the repository has a remote to protect |
| `test` | Run the test gate (configured targets or runtime stack detection) | Before push, in full. CI runs it too; pre-push runs only the conditional quick gate described under Verify |
| `validate` | Validate record frontmatter; `--docs` adds the doc-graph integrity lint | Before push, and for a portal with `--portal` |
| `integrate` | Land a branch into a target: flock(rebase to test to ff-merge) | Landing locally with no remote, or into an integration branch |
| `doctor` | Health checks | After init or update, and when something is wired but not working |
| `orient` | Print the session-start digest (pointers, not content) | At session start; the SessionStart hook runs it for you |
| `status` | Generated status view: branch, worktrees, in-flight work, capabilities | When you need the current shape of the repository |
| `recall` | Search project memory: ledger, session summaries, ADRs, epics, capabilities | When you need why something was decided |
| `epic` | Create an epic: allocate the next EPC-NNN and scaffold it from the template | Full tier, planning a body of work |
| `spec` | Create a spec: allocate the next SPC-NNN and link its consuming work item | Full tier, when a change agreement needs freezing |
| `task` | Create an epic-linked or reasoned standalone task | Full tier, planning a unit of work |
| `work` | Durable-work lifecycle checks | Before implementing a durable task |
| `estimate` | Check explicit forecast allocations and pinned evidence without writes | Only where the optional cf-estimate method was adopted |
| `delegate` | Transport-neutral lifecycle for interactive delegate turns | From a host driving a peer harness turn |
| `portal` | Adopt or reconcile the opt-in documentation portal | Adopting or transferring the documentation portal |
| `present` | Review this session on the utility presentation surface (catalog JSON, Comment) | When a bounded review surface materially helps |

## Technical

Every argument, flag and default below is the binary's own `--help` output at
this head. A required argument is written in angle brackets, an optional one in
square brackets.

### Scaffold

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow init` | `--minimal`, `--standard`, `--full`, `--yes`, `--force` | `--minimal` is the discipline floor, `--standard` the default tier, `--full` adds project-management. `--yes` takes sane defaults at standard tier. `--force` overwrites existing files and is never the default |
| `codeflow update` | `--diff <FILE>`, `--force` | `--diff` writes the report plus unified diffs of applied changes to a file; `--force` replaces user-modified managed files instead of merging |

`codeflow update` exits 2 when the scaffold report or the adopted portal report
contains conflicts, so a caller can distinguish a clean refresh from one that
left `.new` sidecars behind.

### Enforce

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow hook <NAME>` | `--run-id <ID>`, `--result <FILE>`, `--state-dir <DIR>` | `<NAME>` is one of `git-guard`, `exec-guard`, `session-orient`, `session-summary`, `delegate-turn`. The payload is read from stdin. The other four names take no flags. `delegate-turn` requires `--run-id` and exactly one of `--result`, which selects the legacy mode, or `--state-dir`, which selects the schema-v2 mode |
| `codeflow git-hook <STAGE> [ARGS]...` | `<STAGE>` is one of `pre-commit`, `commit-msg`, `pre-merge-commit`, `reference-transaction`, `pre-push` | `[ARGS]` are the arguments git passes through, for example the commit-msg file path or the pre-push remote name and URL |
| `codeflow ci` | `--base <REF>`, `--head <REF>`, `--branch <NAME>`, `--pr-body <TEXT>`, `--pr-body-file <FILE>` | Base and head are auto-detected from the CI environment when omitted; branch defaults to the CI-provided or current HEAD branch. `--pr-body` and `--pr-body-file` scan for AI attribution, emoji, and the required section structure |
| `codeflow policy explain` | no flags | Prints every key's type, default, valid values, and purpose from the schema registry |
| `codeflow policy show` | no flags | Prints each key's current value, its source (project file or built-in default), and any invalid values |
| `codeflow remote protect` | `--provider <PROVIDER>` (default `github`), `--dry-run` | Only `github` has an adapter; another provider prints the manual checklist. `--dry-run` prints the intended rules without applying anything |

Each hook name under `codeflow hook` carries its own exit contract, and exit 0
does not by itself mean enforcement succeeded. `git-guard` exits 0 to allow the
tool call and 2 to make the harness block it. `exec-guard` exits 0 to allow,
including when it only warns, and 2 to block. Both exit 0 on a payload they
cannot read, after printing a warning to stderr, so an unrecognized payload
looks the same to the harness as an allowed call. `session-orient` prints the
digest and always exits 0; `session-summary` always exits 0 as well, warning on
stderr instead, because a failed summary must never fail the session. Read
stderr, not the exit code, to tell an advisory failure from a clean pass.
`codeflow ci` proceeds when only warnings were raised and reports the count.

### Verify

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow test` | `--mode <full\|quick\|essential>` (default `full`), `--strict` | `quick` is an alias for `essential`. With no stack detected the run is a loud no-op with exit 0; `--strict` makes that no-op exit non-zero for scripted and unattended callers |
| pre-push test gate | not a command | The `pre-push` hook runs the `quick` gate only when the `test_gate_on_push` policy key is active and `.codeflow/test-config.json` exists; without that file it skips. This repository sets the key to `warn`, so a failure reports and the push proceeds. It is fast feedback, never the full verification |
| `codeflow test setup` | `--list-templates`, `--template <NAME>`, `--replace`, `--add-target` | The three actions are mutually exclusive. `--list-templates` lists the templates embedded in this binary, `--template` writes one by name, `--add-target` appends one target interactively. `--replace` requires `--template` and explicitly replaces an existing config. With no flag, safe root-only auto-detection runs |
| `codeflow validate [PATH]` | `--docs`, `--portal <DIR>` | `PATH` defaults to `project-management/`. `--docs` adds the doc-graph referential-integrity lint. `--portal` verifies a portal evidence manifest without executing project code and conflicts with `PATH`, so pass one or the other |
| `codeflow integrate <BRANCH>` | `--into <INTO>` (default `main`) | Rebase, test, then fast-forward, under a flock and a gate-context token |
| `codeflow doctor` | `--check <CHECK>`, `--list` | `--list` prints the available check names; `--check` runs a single named check |

### Remember

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow orient` | no flags | Prints the session-start digest to stdout |
| `codeflow status` | `--capabilities`, `--delivery` | Default output is counts by status; `--capabilities` shows the full capability table; `--delivery` shows each capability's epics with their open and total task counts and next actionable tasks |
| `codeflow recall <QUERY>` | `--all`, `--rebuild`, `--limit <LIMIT>` | `--all` searches every repo in the user registry. `--rebuild` drops the index for the searched repos and re-syncs. `--limit` defaults to 20, or `[recall].limit` from `~/.codeflow/config.toml` |

### Plan

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow epic new <TITLE>` | no flags | Allocates the next `EPC-NNN` and scaffolds the epic from the template |
| `codeflow spec new <TITLE> --for <EPC-NNN\|TSK-NNN>` | `--for` is required | Allocates the next `SPC-NNN`, scaffolds it, and links the consuming epic or task |
| `codeflow task new <TITLE>` | `-e`/`--epic <EPC-NNN>`, `--standalone-reason <REASON>`, `--into <BRANCH>` | `--epic` and `--standalone-reason` are mutually exclusive, and a durable task needs one of them. `--into` names the existing local or remote-tracking non-task branch this task will integrate into |
| `codeflow work start <TASK_ID>` | `--into <REF>` | Verifies that a durable task was planned and anchored before implementation. `--into` names the non-task branch or ref this task will merge into |
| `codeflow estimate check <FORECAST_PATH>` | `--json` | `<FORECAST_PATH>` is a forecast JSON file, relative to the current directory or absolute. `--json` emits the versioned JSON report |

### Delegate and present

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow delegate init` | `--run-id <ID>` (required), `--state-dir <DIR>` (required) | Creates the owner-only run directory and the task hook settings. `--state-dir` must be an absolute path |
| `codeflow delegate arm` | `--run-id <ID>` (required), `--state-dir <DIR>` (required), `--turn-id <ID>` (required), `--prompt-file <FILE>` (required) | `--prompt-file` holds the exact prompt bytes the host will deliver |
| `codeflow delegate wait` | `--run-id <ID>` (required), `--state-dir <DIR>` (required), `--until <ready\|accepted\|terminal>` (required), `--timeout-seconds <N>` (required), `--turn-id <ID>` | There is no default timeout, so `--timeout-seconds` is always passed. `--turn-id` is required for `accepted` and `terminal` waits and ignored for `ready` |
| `codeflow portal setup` | `--path <DIR>` (required) | The repository-relative portal workspace directory |
| `codeflow portal transfer` | `--confirm` (required) | Confirms responsibility for future runtime reconciliation |
| `codeflow present open <DOCUMENT>` | `--no-launch` | `--no-launch` starts the service but does not launch a browser window |
| `codeflow present list` | no flags | Lists presentation sessions for this project |
| `codeflow present show <SESSION_ID>` | `--no-launch` | `--no-launch` prints the session endpoint and profile without launching |
| `codeflow present update <SESSION_ID> <DOCUMENT>` | no flags | Appends a validated immutable revision to an active session |
| `codeflow present history <SESSION_ID>` | no flags | Prints the append-only feedback history as JSON |
| `codeflow present feedback <SESSION_ID>` | `--follow` | `--follow` continues until the session closes |
| `codeflow present resolve <SESSION_ID> <EVENT_ID>` | `--event-version <N>` (required), `--status <addressed\|dismissed>` (required) | `--event-version` is the current event version printed by the review surface or by `history` |
| `codeflow present close <SESSION_ID>` | no flags | Repeating close is safe |
| `codeflow present export <SESSION_ID>` | `--out <FILE>` (required), `--theme <editorial\|instrument\|technical>` (default `editorial`), `--mode <system\|light\|dark>` (default `system`) | Exports a deterministic self-contained read-only HTML artifact |
| `codeflow present clear [SESSION_ID]` | `--older-than <OLDER_THAN>` (default `30d`), `--dry-run` | With no session id it removes every eligible closed session older than the window |

`codeflow delegate wait` has the stable exit contract the host polls: 0 on the
observed state, 10 on a failed terminal, 11 on a poisoned, unsafe, or invalid
run, 124 on timeout, and 130 when interrupted. `delegate init` and
`delegate arm` exit 0 or 1.

### General exit behaviour

| Result | Exit |
|---|---|
| The command succeeded, or the check it runs passed | 0 |
| The check failed, or the command could not complete | non-zero, usually 1 |
| `codeflow update` left conflicts | 2 |
| `codeflow hook` wants the harness to block the tool call | 2 |
| An unknown subcommand or an invalid flag | 2 |
