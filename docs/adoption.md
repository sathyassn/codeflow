# Adopting CodeFlow

## Concept

**You adopt CodeFlow by running one command at one of three tiers.**

Every tier installs the same git-discipline floor, and the tier only decides
how much working method comes with it. Minimal suits any repository, standard
suits code projects, and full suits programs whose work outlives sessions. No
tier changes how strictly the floor is enforced,
and no tier installs remote branch protection. A project can start at minimal and move up later, and a
lower-tier request never removes files.

## Architecture

**The tier sets what `init` installs, and the ownership class sets what a later
`update` may change.**

The tier is recorded in `.codeflow/project.toml`. Running `init` again at a
higher tier is an idempotent additive upgrade. A lower-tier request is ignored,
so the recorded tier stays active and no files are removed. Each tier is a clean
superset of the one below, and the enforcement floor is the same at all three
(architecture decision record ADR-0019). The tier alone does not decide whether
durable-work checks are active, because historical CodeFlow task records in an
existing repository can keep them on.

| Tier | Adds | For |
|---|---|---|
| `--minimal` | The complete git-discipline enforcement floor: all five git hooks (`pre-commit`, `commit-msg`, `pre-push`, `pre-merge-commit`, `reference-transaction`), the CI check, the in-session `git-guard`/`exec-guard` + orient/summary hooks (`.claude/settings.json` + the `.codex/` starter), the armed `policy.json`, `.gitignore`, and a lean `AGENTS.md` + `CLAUDE.md` | Any repo: doc-sets, config repos, small tools |
| `--standard` (default) | + the develop-loop method (cf-* skills, reviewer agents, the pipeline), the six-layer `docs/` spine, the full contract, the test gate, recall capture, and harness integration | Code projects |
| `--full` | + `project-management/` (epics, tasks, specs, templates) and the `validate --docs` referential lint | Programs whose work outlives sessions |

Once the files are on disk, ownership decides what a later `update` may touch.
Five classes cover every managed path.

| Class | Examples | What `update` does |
|---|---|---|
| Fully-managed | `.claude/` agents, skills; git-hook shims; CI template | Replaced if you never touched them; 3-way merged from `.codeflow/.baseline/` if you did, and conflicts land as `.new` plus a report |
| Managed-region | `AGENTS.md` / `CLAUDE.md` markers; `.gitignore` markers; `.claude/settings.json` codeflow keys | Only the marked region or codeflow-owned keys are rewritten; everything else is yours |
| User-owned, schema-versioned | `.codeflow/policy.json`, `.codeflow/project.toml` | Only *new* keys are added with their defaults and reported; values you set are never mutated |
| User-owned docs (write-once seeds) | all of `docs/`: product, architecture, capabilities, ADRs | Seeded once at init; `update` never mutates them, so they are yours to edit and own |
| Engine-generated | `.codeflow/manifest.json`, `.codeflow/.baseline/`, `status` / `orient` views | Rewritten by the binary; never hand-edit |

`docs/product.md` describes the consuming project's purpose, users, scope and
non-goals. It does not describe the CodeFlow CLI. `docs/architecture.md` describes how that
project is built. Common project facts, commands and constraints belong in
`AGENTS.md`, and `CLAUDE.md` carries only Claude-specific differences. CodeFlow
adds no competing `project.md` or `projects.md`.

Runtime autonomy and harness settings are in
[harness posture](harness-posture.md). Qualifying a new model or harness is in
[model and harness upgrades](model-upgrades.md).

## Technical

Each section below is one step, in the order a project meets them.

### Install the binary

Install the single `codeflow` binary on `PATH`, where every repository's hooks
call it.

| Release | Platforms | Assets |
|---|---|---|
| 3.1.0 and later | `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` | A `.tar.xz` archive per macOS and Linux target, a `.zip` archive for Windows, a `.sha256` file per archive, a `sha256.sum`, a shell installer, a PowerShell installer and a source archive |
| 3.0.0 | `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` | A `.tar.xz` archive per target, a `.sha256` file per archive, a `sha256.sum`, a shell installer and a source archive; no Windows asset |
| Before 3.0.0 | Varies by release | Check a release's asset list before pinning it |

WSL2 installs the Linux archive and is the preferred Windows route for
Linux-native tooling or Claude sandboxing.

| Path | Use it when |
|---|---|
| The shell installer | You install the latest release on macOS or Linux |
| The PowerShell installer | You install the latest release, 3.1.0 or later, on native Windows; Git for Windows is required |
| `cargo install --path crates/codeflow-cli` | You build from a checkout and have a Rust toolchain |
| One platform archive checked against its `.sha256` file | You are pinning a version or scripting the install |

1. Pick a path from the table.
2. Run the installer on macOS or Linux:

   ```sh
   curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
   ```

   On native Windows, in PowerShell:

   ```powershell
   powershell -ExecutionPolicy Bypass -c "irm https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.ps1 | iex"
   ```

   Or build from a checkout:

   ```sh
   cargo install --path crates/codeflow-cli
   ```

   Or pin one archive. Substitute the version you pin and your platform's
   target (the Windows archive is a `.zip`). Each archive unpacks to a
   directory of the same name that holds the `codeflow` binary. The commands
   run in a subshell that stops at the first failure, so nothing is extracted
   or installed unless the download and the checksum pass; `D` must be a
   directory on your `PATH`:

   ```sh
   V=v3.0.0 A=codeflow-cli-aarch64-apple-darwin D="$HOME/.cargo/bin"
   B=https://github.com/sathyassn/codeflow/releases/download/$V
   (
     set -e
     mkdir -p /tmp/cf && cd /tmp/cf
     curl -fsSLO "$B/$A.tar.xz"
     curl -fsSLO "$B/$A.tar.xz.sha256"
     shasum -a 256 -c "$A.tar.xz.sha256"
     tar -xf "$A.tar.xz"
     mkdir -p "$D"
     install "$A/codeflow" "$D/"
   )
   ```

`codeflow --version` prints the version you installed.

### Adopt a new or existing repository

Run `codeflow init` from the project root at the lowest tier whose conventions
the project accepts.

1. Change to the intended project root. `init` and `update` act on the current
   directory and do not look for a Git root.
2. Inventory what exists: source and build units, deployables, release and data
   owners, instructions, docs, work tracking, hooks and CI.
3. Pick a tier. Init does not rearrange product source, but it installs the
   fixed homes in the table below, and no link or config setting relocates them.
4. For an existing repository, create an adoption branch and run
   `codeflow init --minimal` or a higher tier. For a new one, run:

   ```sh
   mkdir myproject && cd myproject && git init
   codeflow init --standard --yes    # scaffold; offline; sane defaults
   ```

5. Inspect every created, skipped or merged file. Init preserves existing
   content mechanically, but a skipped file or combined instruction file may
   still need reconciling by hand. Do not use `--force` to get past a collision.
6. At standard or full, run `/cf-customize`. It verifies the installed harness
   settings and tools, then walks the consuming project's `docs/product.md`,
   `docs/architecture.md`, `AGENTS.md`, `CLAUDE.md` differences, README and
   manifests, CI commands, policy, and required Model Context Protocol (MCP)
   servers. CodeFlow never invents these facts or silently changes global
   harness settings, so review them and commit.
7. With an empty remote, push the scaffold commit yourself with the
   override, which lifts only the protected-branch rules:

   ```sh
   git remote add origin <url>
   CODEFLOW_HUMAN_OVERRIDE=1 git push -u origin main
   ```

8. Start the first feature on a `feat/*` branch in a worktree and land it by
   PR, or with `codeflow integrate` when there is no remote.

| Behavior | New repository | Existing repository |
|---|---|---|
| Commit | `init` makes the `chore: scaffold codeflow standard tier` commit for you | Nothing is committed; the files wait for you to review and commit them on a branch |
| Existing files | None to protect | Never overwritten; an existing `CLAUDE.md`, `.claude/settings.json` or `.gitignore` is merged by region |
| Git hooks | Wired through `core.hooksPath`, and branch policy is armed | If `.husky/` or a custom `core.hooksPath` exists, codeflow leaves hooks alone, records `git_hooks = "unwired"`, and the report shows how to add the `pre-commit`, `commit-msg`, `pre-merge-commit`, `reference-transaction` and `pre-push` shims to your manager |

| Fixed home | Installed at |
|---|---|
| Root `AGENTS.md` and `CLAUDE.md` | Every tier |
| Documentation entry points under `docs/` | Standard and full |
| CodeFlow records and templates under `project-management/` | Full |

| Case | What to know |
|---|---|
| The scaffold commit on a new repository | The one sanctioned commit before the gates guard the repository, so the first PR meets no policy wall. The secret scan is never relaxed, even for that commit |
| Hooks in `.git/hooks`, such as those `pre-commit install` writes | Git stops running them once `init` sets `core.hooksPath`. The report names each one and moves nothing; you choose to move the check into CI or a supported hook manager, or to call it from a project-owned hooks folder, set as `core.hooksPath`, that also calls the CodeFlow shims. `update` and `codeflow doctor` repeat it while `core.hooksPath` names CodeFlow's hooks and the files stay |
| Update-ignore settings and `/cf-customize` | Update-ignore settings do not choose paths for the first scaffold, and `/cf-customize` runs after init without resolving collisions for you |
| Unrelated files in a CodeFlow record home | They can make `validate --docs` report a collision even when durable-work tracking is inactive. Resolve the conflict |
| Moving up later | Re-run `init` at `--standard`, then `--full`, as the work earns the weight. Each step is additive and idempotent. |
| Where task work happens | The root checkout stays on its root branch and takes no task work, which happens in linked worktrees under `.worktrees/` |
| An umbrella repository that holds several projects, each its own repository | Uses workspace mode instead: see [workspace mode](workspace-mode.md) and `codeflow init --workspace` |

| Policy key in `.codeflow/policy.json` | Sets |
|---|---|
| `git.root_branch` | The root branch, when it is not the default branch |
| `git.root_checkout_commits` | How a commit on the root checkout on another branch is judged |
| `git.worktree_locations` | Where linked worktrees may live |

The printed report lists every file written and ends with the next step.
`codeflow doctor` shows unwired hooks and keeps a reminder visible while
scaffold sentinels remain.

### Bots, kept PR templates and release tools

`init` reports what it finds that CodeFlow's checks would otherwise break.
None of it changes a policy level.

**Dependency and release bots.** Dependabot, Renovate and release actions
open pull requests whose branch names and commit messages fail CodeFlow's
rules. Add a trusted profile per bot to `git.automation_profiles` in
`.codeflow/policy.json`:

```json
"automation_profiles": [
  {
    "name": "dependabot",
    "actors": ["dependabot[bot]"],
    "branch_pattern": "dependabot/**",
    "task": "dependency update",
    "sections": {
      "Summary": "Automated dependency update opened by Dependabot.",
      "Changes": "- the dependency bump named in the title",
      "Testing": "The full CI suite runs on this pull request.",
      "Reviews": "None: automated update, reviewed at merge.",
      "Release impact": "- Impact: patch\n- Breaking: no\n- Rationale: dependency update.\n- Migration: none"
    }
  }
]
```

| Profile rule | Behavior |
|---|---|
| When it applies | In `codeflow ci` only, when the actor the workflow passes (`--actor`) and the branch both match, and only as the target branch's policy states it, so a pull request cannot add a profile for itself |
| What it skips | Branch naming, the commit message shape rules and the PR Summary shape (`git.pr_summary`), since the bot writes its body to its own format |
| What still runs | Tests, the secret scan, AI attribution, emoji, the dash rule, the release declaration, and PR sections at their configured level; `sections` only supplies the headings the bot body leaves out |
| Trusted actor | Only in a GitHub Actions pull request event from the same repository, and only as that event's own actor. In a local run, in another CI and on a fork pull request the actor is `unknown`, whatever `--actor` says, and no profile applies |
| `task` | Names the unit the bot's pull requests are. Every pull request needs a `Task:` line and a bot names no task record, so `codeflow ci` puts the profile's unit on that line when the bot body has none; a profile without `task` leaves every bot pull request refused for the missing line. This works where durable work tracking is off (the standard and minimal tiers), where any non-empty unit name is accepted |
| Bots at the full tier | The `Task:` line must name a `TSK-NNN` or `EPC-NNN` record, so a bot's unit name is refused there: a dependency update lands through a task of its own, opened by a person or an agent, whose pull request carries the bump; the bot's pull request is closed once that task lands |

**A PR template you already have.**

| Situation | Behavior |
|---|---|
| `init` finds your template | Keeps it and installs no second template; `update` never merges into it or writes a `.new` beside it |
| Its headings differ from `git.pr_required_sections` | The run records `git.pr_section_mapping` as `diagnosed` with a proposed mapping (for example `Summary -> Description`) and asks you to decide, as in the next table |

| Decision | What happens |
|---|---|
| accepted | the check reads your template's headings in place of the mapped ones |
| refused | the missing required headings are appended to your template |
| custom | you set `git.pr_required_sections` and `git.pr_code_sections` yourself in a reviewed change |

| Situation | Behavior |
|---|---|
| A pull request that changes only Markdown under `docs/` or `project-management/`, outside your product and watched contract paths and the other shared contract surfaces | Whatever the decision, it needs just Summary and Changes of your required list, under the template's headings when the mapping is accepted, and may leave out Release impact |
| A run without a terminal (`--yes`, CI) | Leaves the state `diagnosed`, and `codeflow doctor` repeats it |
| While the mapping is diagnosed | The PR-section check runs at `warn` only on a fresh install whose policy file `init` created; a `pr_sections` value already in your policy file stays in force, even when it equals the default |
| The Summary shape while diagnosed | The Summary shape check (`git.pr_summary`) runs at `warn` at most while the mapping is diagnosed, whatever `pr_sections` says; a lower `pr_summary` level stays as you set it |
| `codeflow ci` | Prints each check's effective level and where it comes from (`configured`, `shipped default` or `diagnosed`) |
| These policy edits, and the keys `codeflow update` adds | Spliced into the file as you wrote it, so no other byte changes; a policy with no `git` object gains one |
| A template reached through a symlink out of the repository | Never read or written |

**Release tools.** `release.backend` in `.codeflow/project.toml` names who owns
the version of each release unit.

| Value | Meaning |
|---|---|
| `none` (default at every tier) | CodeFlow checks only the PR's Release impact declaration (`git.pr_release_impact`, warn by default); it calculates no version |
| `external` | another tool owns versions (release-please, Changesets, semantic-release, cargo-release, GoReleaser); CodeFlow checks the declaration and `breaking_watch_paths` only |
| `codeflow` | you adopted CodeFlow's release calculator explicitly |

| Release rule | Detail |
|---|---|
| One version authority per release unit | `init`, `codeflow ci` and `doctor` say so when a release tool's configuration sits beside `none` or `codeflow` |
| A kept PR template | Keeping the PR template is not consent to a calculator |
| The calculator CodeFlow ships, `scripts/release.py` | Serves CodeFlow's own repository: one Rust workspace versioned from `Cargo.toml`, with its `CHANGELOG.md` in Keep a Changelog form. Other projects use `none` or `external` |
| The minimal tier | Installs no release process: declare each PR's Release impact in its body, and if you publish releases, let your own tool own the version and set `backend = "external"` |

### Configure test targets

Tell `codeflow test` which commands to run for each part of the project.

1. Run `codeflow test setup`. With no options it checks only root-level
   `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml` or `setup.py`
   markers and writes the detected targets into an absent or empty
   `.codeflow/test-config.json`. It does not inspect workspaces recursively or
   guess package boundaries.
2. Or start from a template, or add targets one at a time:

   ```sh
   codeflow test setup --list-templates
   codeflow test setup --template example-rust.json
   codeflow test setup --add-target
   ```

3. For a monorepo, start from `monorepo-multi-target.json` or add one target
   per package, each with its repository-relative `cwd`.
4. Review every command against the project.
5. Setup never replaces a populated or malformed config by itself. To replace
   one, run `codeflow test setup --template <name> --replace`.

`codeflow test` and `codeflow doctor --check test-config` both pass.

### Organize durable work

At the full tier, durable work lives in `project-management/` as records with
stable ids. How they move to `main` is on
[how work moves to main](delivery.md); this table is the mechanics.

| Record or step | How |
|---|---|
| Epic (EPC) | `codeflow epic new`, in the one planning PR that breaks a brief or spec into the epic, every task with its criteria, and the edges |
| Spec (SPC) | `codeflow spec new --for <epic-or-task>`, when a contract consumers rely on must be pinned; approved once it has no open questions |
| Task (TSK) in an epic | `codeflow task new` on the epic's `plan/` branch; the record is anchored in that planning change |
| Standalone task | `codeflow task new --standalone-reason <why>` on the task's own branch; the record and the code land in the same reviewed PR |
| Start a task | `codeflow work claim TSK-NNN`, then `codeflow work start TSK-NNN` on `task/TSK-NNN-<slug>` before editing. Add `--on TSK-NNN@<sha>` to build on a predecessor's reviewed head before it lands |
| A later change of scope | One batched epic amendment on a `plan/` branch, reviewed once; a task changes only its own criteria, in its own PR, and CI prints the change for the reviewer |
| A team tracker already owns the portfolio | Link its ids in `external_refs`; do not mirror its status, specs or task trees |

### Update

Upgrade the binary once, then run `codeflow update` in each repository.

| Motion | How | Effect |
|---|---|---|
| Upgrade the binary | Re-run an install path: the `curl \| sh` installer again, or `git pull` then `cargo install --path crates/codeflow-cli`, or download the newer archive (see Install). There is no self-updater (`install-updater = false`) | Improves every repo at once, because hooks call `codeflow` from `PATH` |
| `codeflow update` per repo | Run it in the repository | Refreshes scaffold files. Until you do, every command prints a version-skew warning |

1. Upgrade the `codeflow` on `PATH` first. The hooks run that binary, and one
   older than a new policy key rejects the policy file, which blocks every
   commit until you upgrade (for example `git.policy_characters`, ADR-0067).
2. Run `codeflow update` in the repository. Add `--diff <FILE>` to write the
   report with unified diffs.
3. Resolve each `<name>.new` file against the file beside it.

| Case | What `update` does |
|---|---|
| Unmodified managed file | Replaced |
| Managed file you changed | 3-way merged from the baseline; anything that cannot merge is written beside it as `<name>.new` with a report entry, never clobbered or skipped. `--force` replaces instead of merging |
| Unmodified managed file the new version no longer ships | Removed with its baseline and manifest record, so it cannot collide with its replacement (ADR-0011), as when `.claude/commands/*` became `.claude/skills/*` |
| No longer shipped, but could hold your content: a modified managed file, a managed-region file, a user-owned file | Kept and left unmanaged, never deleted |
| New `.codeflow/policy.json` key, such as those added in ADR-0007 | Inserted with its shipped default and reported; values you set are untouched |
| A `--minimal` repository initialized before the full floor moved into minimal | Gains the rest of the enforcement plane |

The version-skew warning is gone and no `.new` file remains.

**CI pins its binary too.** The scaffolded workflows install the release
named by `scaffold_version` in the target branch's `.codeflow/project.toml`
and verify it against that release's `sha256.sum` and the archive digests
the target pins (below); a mismatch fails the job and nothing unverified is
installed. The commit and
PR-body standards run in `codeflow-policy.yml` on `pull_request_target`.
GitHub runs that workflow from the default branch, so a pull request cannot
edit the job that judges it, and the job checks out the pull request's base
commit, so a pull request into an integration branch is judged by that
branch's pin and policy, not the default branch's. An upgrade therefore takes
two pull requests, in order:

1. Install the new binary locally, run `codeflow update --pin <version>`,
   and land the pin it raises. The target's current binary judges it, and
   the `candidate codeflow` job tests the new one.
2. On a new branch, run `codeflow update` and land its new keys and files;
   the new binary judges them.

| Case | What happens |
|---|---|
| A pull request adds policy keys before step 1 has landed | It fails with a message naming this order: the enforcing job reads the head's policy as data and fails when the pinned binary cannot read it |
| A pull request lowers the pin | The target's binary still judges it, and the job then fails |
| A branch started before the target raised its pin and kept the pin it started from | It lowers nothing and is judged by the new binary |
| A raised pin | Its release is installed separately and only tested |
| A hand-raised pin that leaves an older `[scaffold_sha256]` table | The `candidate codeflow` job fails closed and names `codeflow update --pin <version>` |
| `codeflow doctor` | Reports the version CI installs and whether it is checked against pinned digests or `sha256.sum` alone, a digest table CI would refuse, a raise alone (step 1), a lowered pin, or new policy keys or schema carried before the raise has landed, with this order |
| The git hook shims | Check the binary first and warn when it is older than they are, then run the checks it has |

The other CI templates carry the same pin. `.gitlab-ci.yml`,
`bitbucket-pipelines.yml` and `ci-generic.sh` (in `assets/base/ci/` of the
CodeFlow repository; copy the one your host needs) run one shared script: it
reads the pin from the target branch's current commit, installs that release
with the same verification, and runs `codeflow ci` from a
checkout of that commit, so the target's policy judges the change.

| Managed CI, 3.1.0 | What it does |
|---|---|
| Pinned release digests | `codeflow update --pin <version>` downloads the release's `sha256.sum` and its Linux and macOS archives, refuses any that does not match, and writes only `scaffold_version` and a `[scaffold_sha256]` table of one digest per platform. Once the target pins it, every installer requires the archive to match it as well as `sha256.sum`, because whoever can replace a release asset can replace `sha256.sum` too |
| A table CI cannot use | One from another version, missing the runner's platform, declared or keyed twice, written as a quoted header, an inline or dotted table or a sub-table, or a state with any escaped table name or key fails the job closed. Only names count, never values or comments. Keep the plain table `--pin` writes |
| No table | The install checks `sha256.sum` alone and warns, so a fresh `codeflow init` and the pull request that adds the table still pass |
| Project setup hook | A project that needs its own toolchain commits `.codeflow/ci-setup.sh`. The gates job and the shared script source it under `set -eu` just before `codeflow test --strict`, so its exports reach the gate and a failing command fails the job. `codeflow update` never writes it. It is project code with the gate's authority |
| Secret scan range | A pull request scans only its own commits and a push only its pushed range, so a finding already in the base no longer fails every pull request. A weekly schedule and manual dispatch scan the full history, as do a branch-creating push and a push whose previous tip is gone; each run prints what it read. Exemptions come only from the trusted commit |
| `codeflow doctor --check ci-perimeter` | Names the check CI applies on the target (pinned digests or `sha256.sum` alone), a table it would refuse, a table the checkout changes, and the setup hook with its first command |

Details: `assets/base/ci/README.md`.

| Host | Target commit |
|---|---|
| GitLab | `CI_MERGE_REQUEST_TARGET_BRANCH_SHA` in a merged results pipeline. An ordinary merge request pipeline leaves it empty, so the job fetches `CI_MERGE_REQUEST_TARGET_BRANCH_NAME` from the merge request's project and fails when it cannot. It never uses `CI_MERGE_REQUEST_DIFF_BASE_SHA`, the diff's base, which stays behind when the target advances |
| Bitbucket | `BITBUCKET_PR_DESTINATION_COMMIT`. Atlassian does not list that variable, so when it is unset the step fetches `BITBUCKET_PR_DESTINATION_BRANCH` from `origin` and fails when it cannot. Bitbucket merges the destination branch into the working tree before the step, so `codeflow test` and `validate --docs` run on that merge while `codeflow ci` judges `BITBUCKET_COMMIT` against the target |
| `ci-generic.sh` | Its first argument; it refuses to run without one |

The pin does not defend the CI file itself. On a GitHub `pull_request` event,
and on every GitLab and Bitbucket pipeline, the job file runs from the pull
request, so a pull request that edits it can change its own install step.
Only `codeflow-policy.yml`, which also carries the id registry check, runs
from the default branch. Require review of your CI files (`.github/workflows/`,
`.gitlab-ci.yml`, `bitbucket-pipelines.yml`) and `.codeflow/` in your host's
rules, for example with a code owners file and a branch rule that requires
code owner review; CodeFlow does not configure those settings.

**Keys `codeflow update` adds for records and checks.** A value the project
set is kept.

| Key | Values | Effect |
|---|---|---|
| `git.work_records` | `block` or `warn` | An `off` from an unreleased build is rewritten to `warn` with a notice. In a project that already has epic, spec or task records, `update` also records `work_records_baseline` in `.codeflow/project.toml` once, as the current commit. Records whose bytes are unchanged since that commit keep the rules they were written under; a status change, a criteria change or a new record follows the status verbs' rules. A task completed before that commit, with no acceptance block, reopens with `codeflow task status <id> todo --reason <text>`, which keeps its Closeout and adds the line `- reopened: <text>` |
| `git.work_planning` | `block` or `warn`, default `block` | The level at which `work start` and `codeflow ci` report the planning checks: a valid workgraph, a record for the task the branch carries, and that record anchored on its target. Pre-commit does not run them |
| `git.conflict_markers` | default `block` | Reported as added. The pre-commit hook and `codeflow ci` refuse an unresolved conflict marker on a line a change adds to a text file; existing lines are not judged. A file that must hold markers, such as a test fixture or a page about git, sets `conflict-marker-size` for its path in `.gitattributes` to a length its markers do not have. A team that wants a softer start sets the key to `warn` or `off` in a reviewed policy change |

### Refusals and operator relief

Agent sessions are judged by the landed policy on the remote, so a local
edit cannot relax it. A refusal names the rule and the operator's route;
relief is that rule's level, landed through a reviewed change. Install the
new binary before `codeflow update`. Detail:
[enforcement planes](architecture/enforcement-planes.md#in-session-guards)
and, for Codex hook trust, [harness posture](harness-posture.md).

### The daily flow

One task, one pull request, and a check at every step.

1. **Orient and route.** Read the SessionStart digest, or run `codeflow orient`,
   for branch and worktree state, work counts, recent ADR titles, gate status
   and pointers. Read the pointed docs for depth. The paths a change touches
   decide its entry: an adopter-facing path (product code, managed
   instructions, hooks, policy, CI, shipped templates, watched contracts), or
   plan, design, security or irreversible work, starts with
   `/cf-model-orchestrator`, which says so when a native peer seat is
   unavailable; other edits go direct.
2. **Attach to a task and branch in a worktree.** Every change names a task
   before substantive work starts, an existing one or a new one. Work on a
   `task/TSK-NNN-<slug>` branch, or another `{prefix}/{kebab-name}`, in a
   worktree, never on the root checkout.
3. **Commit small.** The hooks in the table below check every commit, merge and
   push. Keep `codeflow test --mode quick` and `codeflow validate --docs` green
   before push, and merge the current integration line into the branch before
   asking for review, so conflicts surface in the task.
4. **Open one PR for the whole task.** Its body carries a `Task:` line naming
   the task, or the epic for a planning-only change, the template sections,
   and the test evidence with revision and command. `codeflow ci` refuses a PR
   that names no task and no epic. The Release impact section is required
   only into a protected branch or with a breaking commit.
5. **One review, then land.** A seat of the other model lineage reviews the
   whole change once, and a material finding is fixed in the same PR. The
   reviewed head lands with the next batch on the integration line, or, when
   the PR targets `main`, a human merges it when the required checks are
   evidenced green. An infrastructure-killed duplicate CI job is not a failed
   check. An agent's `gh pr merge` into a protected base is blocked.
6. **With no remote**, land with `codeflow integrate <branch> --into <target>`.
   A human can override the git layer for a local merge or a protected
   push with `CODEFLOW_HUMAN_OVERRIDE=1`.

| Hook | Checks |
|---|---|
| `pre-commit` | Commit on a protected branch, secret scan, and unresolved conflict markers on added lines |
| `commit-msg` | Conventional format, description of at most 50 characters, subject of at most 72, bullet-only body (ADR-0020), no AI attribution, no emoji |
| `pre-merge-commit`, `reference-transaction` | Protected-branch merge and ref rules; `reference-transaction` also catches fast-forward merges, `reset --hard` and `branch -D` |
| `pre-push` | Branch naming, protected-branch rules, test gate |

The PR shows green required checks, and a human merges it. How a batch of
reviewed tasks lands with one full gate, how an epic closes into `main`, and
what happens when something changes midway are on
[how work moves to main](delivery.md).

### Related guides

| Topic | Guide |
|---|---|
| How work moves from a request to `main`: one PR per task, batches, epic close | [how work moves to main](delivery.md) |
| The opt-in repository guide portal, and reading this guide locally | [opt-in documentation portal](capabilities/CAP-015-opt-in-documentation-portal.md) |
| Second opinions and edit handoffs to the other model family | [delegation](delegation.md) |
| Interactive review documents for complex results | [interactive review documents](present-guide.md) |
| The record rules, stronger verification techniques and choosing a code analyzer | [duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
| What each of the four enforcement planes catches | [enforcement planes](architecture/enforcement-planes.md) |
| Where durable work belongs next to an external tracker | [duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
