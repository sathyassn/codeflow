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
| `test` | Run the test gate (configured targets or runtime stack detection) | Before push; also run for you by pre-push and CI |
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

### Scaffold

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow init` | `--minimal`, `--standard`, `--full`, `--yes`, `--force` | `--minimal`: the discipline floor. `--standard` is the default. `--yes` takes sane defaults at standard tier. `--force` overwrites existing files and is never the default |
| `codeflow update` | `--diff <FILE>`, `--force` | `--diff` writes the report plus unified diffs of applied changes to a file; `--force` replaces user-modified managed files instead of merging |

`codeflow update` exits 2 when the scaffold report or the adopted portal report
contains conflicts, so a caller can distinguish a clean refresh from one that
left `.new` sidecars behind.

### Enforce

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow hook <NAME>` | `--run-id <ID>`, `--result <FILE>`, `--state-dir <DIR>` | `<NAME>` is one of `git-guard`, `exec-guard`, `session-orient`, `session-summary`, `delegate-turn`. The payload is read from stdin |
| `codeflow git-hook <STAGE> [ARGS]...` | `<STAGE>` is one of `pre-commit`, `commit-msg`, `pre-merge-commit`, `reference-transaction`, `pre-push` | `[ARGS]` are the arguments git passes through, for example the commit-msg file path or the pre-push remote name and URL |
| `codeflow ci` | `--base <REF>`, `--head <REF>`, `--branch <NAME>`, `--pr-body <TEXT>`, `--pr-body-file <FILE>` | Base and head are auto-detected from the CI environment when omitted; branch defaults to the CI-provided or current HEAD branch. `--pr-body`/`--pr-body-file` scan for AI attribution, emoji, and the required section structure |
| `codeflow policy explain` | none | Prints every key's type, default, valid values, and purpose from the schema registry |
| `codeflow policy show` | none | Prints each key's current value, its source (project file or built-in default), and any invalid values |
| `codeflow remote protect` | none | Applies protected-branch policy to the remote provider |

Hook exit behaviour is the contract the harness reads: `hook` exits 2 to make
the harness block the tool call, and 1 on other failures. `codeflow ci`
proceeds when only warnings were raised and reports the count.

### Verify

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow test` | `--mode <full\|quick\|essential>` (default `full`), `--strict` | `quick` is an alias for `essential`. With no stack detected the run is a loud no-op with exit 0; `--strict` makes that no-op exit non-zero for scripted and unattended callers |
| `codeflow test setup` | see `codeflow test setup --help` | Configures `.codeflow/test-config.json` by root detection, an embedded template, or an appended target; safe auto-detection is the default |
| `codeflow validate [PATH]` | `--docs`, `--portal <DIR>` | `PATH` defaults to `project-management/`. `--docs` adds the doc-graph referential-integrity lint. `--portal` verifies a portal evidence manifest without executing project code |
| `codeflow integrate <BRANCH>` | `--into <INTO>` (default `main`) | Rebase, test, then fast-forward, under a flock and a gate-context token |
| `codeflow doctor` | `--check <CHECK>`, `--list` | `--list` prints the available check names; `--check` runs a single named check |

### Remember

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow orient` | none | Prints the session-start digest to stdout |
| `codeflow status` | `--capabilities`, `--delivery` | Default output is counts by status; `--capabilities` shows the full capability table; `--delivery` shows each capability's epics with their open/total task counts and next actionable tasks |
| `codeflow recall <QUERY>` | `--all`, `--rebuild`, `--limit <LIMIT>` | `--all` searches every repo in the user registry. `--rebuild` drops the index for the searched repos and re-syncs. `--limit` defaults to 20, or `[recall].limit` from `~/.codeflow/config.toml` |

### Plan

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow epic new` | see `--help` | Allocates the next `EPC-NNN` and scaffolds the epic from the template |
| `codeflow spec new` | `--for <epic-or-task>` | Allocates the next `SPC-NNN`, scaffolds it, and links the consuming work item |
| `codeflow task new` | see `--help` | Allocates the next independent `TSK-NNN` and scaffolds it |
| `codeflow work start <TASK_ID>` | `--into <REF>` | Verifies that a durable task was planned and anchored before implementation. `--into` names the non-task branch or ref this task will merge into |
| `codeflow estimate check` | see `--help` | Checks explicit allocations and pinned evidence without scheduling or writes |

### Delegate and present

| Command | Arguments and flags | Notes |
|---|---|---|
| `codeflow delegate init` | `--run-id <ID>`, `--state-dir <DIR>` | Creates the owner-only run directory and the task hook settings. `--state-dir` must be an absolute path |
| `codeflow delegate arm` | `--run-id <ID>`, `--state-dir <DIR>`, `--turn-id <ID>`, `--prompt-file <FILE>` | `--prompt-file` holds the exact prompt bytes the host will deliver |
| `codeflow delegate wait` | `--run-id <ID>`, `--state-dir <DIR>`, `--until <ready\|accepted\|terminal>`, `--turn-id <ID>`, `--timeout-seconds <N>` | `--turn-id` is required for `accepted` and `terminal` waits |
| `codeflow portal setup` | `--path <DIR>` | The repository-relative portal workspace directory |
| `codeflow portal transfer` | `--confirm` | Confirms responsibility for future runtime reconciliation |
| `codeflow present <SUB>` | `open`, `list`, `show`, `update`, `history`, `feedback`, `resolve`, `close`, `export`, `clear` | `open` takes a validated presentation document; `clear` removes eligible closed session state |

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
