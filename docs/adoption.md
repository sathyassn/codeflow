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
| v2.1.0, the latest verified published release | `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu` | A `.tar.xz` archive and `.sha256` file per platform, a `sha256.sum`, the shell installer and a source archive. No Windows archive and no PowerShell installer |
| 3.0.0, the pending source on `main` | Adds `x86_64-pc-windows-msvc` | Adds a PowerShell installer. No 3.0.0 assets exist until a release is published |

| Path | Use it when |
|---|---|
| The anonymous installer one-liner | Once codeflow's releases are public. While the repository is private it will not resolve, so use one of the two rows below; both authenticate as a collaborator |
| `cargo install --path crates/codeflow-cli` | You want the pending 3.0.0 source from a checkout and have a Rust toolchain |
| `gh release download` of one platform archive | You are pinning a version or scripting the install |

1. Pick a path from the table. This guide describes the pending 3.0.0 source,
   and some of its commands are not in v2.1.0, so build from a source checkout
   to follow it exactly.
2. Run the installer one-liner:

   ```sh
   curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
   ```

   Or build from a checkout:

   ```sh
   cargo install --path crates/codeflow-cli
   ```

   Or pin a published archive. Each archive unpacks to a directory of the same
   name that holds the `codeflow` binary.

   ```sh
   TAG=vX.Y.Z                                # the published release you are pinning
   A=codeflow-cli-aarch64-apple-darwin       # or x86_64-apple-darwin, x86_64-unknown-linux-gnu
   gh release download "$TAG" -R sathyassn/codeflow -p "$A.tar.xz" -D /tmp/cf --clobber
   tar -xf "/tmp/cf/$A.tar.xz" -C /tmp/cf
   install "/tmp/cf/$A/codeflow" ~/.cargo/bin/    # or any directory on PATH
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
   content mechanically, but a skipped file or a combined instruction file may
   still need reconciling by hand. Do not use `--force` to get past a collision.
6. At standard or full, run `/cf-customize`. It verifies the installed harness
   settings and tools, then walks the consuming project's `docs/product.md`,
   `docs/architecture.md`, `AGENTS.md`, `CLAUDE.md` differences, README and
   manifests, CI commands, policy, and required Model Context Protocol (MCP)
   servers. CodeFlow never invents these facts or silently changes global
   harness settings, so review them and commit.
7. Start the first feature on a `feat/*` branch in a worktree and land it by
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

- On a new repository, the scaffold commit is the one sanctioned commit before
  the gates guard the repository, so the first PR meets no policy wall. The
  secret scan is never relaxed, even for that commit.
- Update-ignore settings do not choose paths for the first scaffold, and
  `/cf-customize` runs after init without resolving collisions for you.
- Unrelated files in a CodeFlow record home can make `validate --docs` report a
  collision even when durable-work tracking is inactive. Resolve the conflict.
- To move up later, re-run `init` at `--standard`, then `--full`, as the work
  earns the weight. Each step is additive and idempotent.

The printed report lists every file written and ends with the next step.
`codeflow doctor` shows unwired hooks and keeps a reminder visible while
scaffold sentinels remain.

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

At the full tier, durable work lives in `project-management/` as epic (EPC),
spec (SPC) and task (TSK) records with stable IDs, created with
`codeflow epic new`, `codeflow spec new --for <epic-or-task>` and
`codeflow task new`. Plan them on a `plan/` branch, merge the planning PR into
each task's `integration_target`, then run `codeflow work start TSK-NNN` on
`task/TSK-NNN-<slug>` before editing. When a team tracker already owns the
portfolio, link its IDs in `external_refs` and do not mirror its status, specs
or task trees.

### Update

Upgrade the binary once, then run `codeflow update` in each repository.

| Motion | How | Effect |
|---|---|---|
| Upgrade the binary | Re-run an install path: the `curl \| sh` installer again, or `git pull` then `cargo install --path crates/codeflow-cli`, or re-download the newer tarball via `gh release download` (see Install). There is no self-updater (`install-updater = false`) | Improves every repo at once, because hooks call `codeflow` from `PATH` |
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

### The daily flow

Every task follows the same loop, and a gate checks each step.

1. **Orient and route.** Read the SessionStart digest, or run `codeflow orient`,
   for branch and worktree state, work counts, recent ADR titles, gate status
   and pointers. Read the pointed docs for depth. Begin every non-trivial task
   with `/cf-model-orchestrator`, which runs only the stages the outcome needs
   and says so when a native peer seat is unavailable.
2. **Branch in a worktree.** Work on a `{prefix}/{kebab-name}` branch in a
   worktree, never on the root protected-branch checkout.
3. **Commit small.** The hooks in the table below check every commit, merge and
   push. Keep `codeflow test` and `codeflow validate --docs` green before push.
4. **Land by PR, merged by a human.** Push the branch and open a PR from the
   template (summary, changes, testing, reviews, release impact). A human merges
   it when the required checks are evidenced green. An infrastructure-killed
   duplicate CI job is not a failed check. An agent's `gh pr merge` into a
   protected base is blocked.
5. **With no remote**, land with `codeflow integrate <branch> --into <target>`.
   A human can override the git layer for a local merge with
   `CODEFLOW_HUMAN_OVERRIDE=1`.

| Hook | Checks |
|---|---|
| `pre-commit` | Secret scan |
| `commit-msg` | Conventional format, description of at most 50 characters, subject of at most 72, bullet-only body (ADR-0020), no AI attribution, no emoji |
| `pre-merge-commit`, `reference-transaction` | Protected-branch merge and ref rules; `reference-transaction` also catches fast-forward merges, `reset --hard` and `branch -D` |
| `pre-push` | Branch naming, protected-branch rules, test gate |

The PR shows green required checks, and a human merges it.

### A body of work: the integration branch

A multi-task epic lands on one shared integration branch, and only the finished
body reaches `main`.

1. Branch `integration/<epic>` off `main`. It is not protected, so agents merge
   tasks into it. Every other gate still applies.
2. Parallelize only independent tasks whose isolation pays for the
   coordination. Give each one owner, branch and worktree, give shared contracts
   and conflict hotspots one owner, and cap concurrency by memory, CPU, disk,
   model contexts and browser or tool capacity.
3. Land tasks one at a time with `codeflow integrate <task> --into
   integration/<epic>` or a PR based on the integration branch, and rerun the
   affected gates after each landing.
4. Run the ship flow, the aggregate suite and both-model review on the
   combined integration diff.
5. Raise one human-reviewed PR from the integration branch into `main`.

- Never run two writers in one worktree, and never rebase the shared
  integration branch.
- The integration branch is never a backdoor to `main`, which stays
  human-merge-only.
- A different landing shape needs a recorded Plan vN rationale and approval
  from both primary model seats before allocation.

The integration worktree is green, and `main` changes only through the one
human-merged PR. The full procedure is in cf-method's "Managing a body of work".

### Related guides

- The opt-in repository guide portal, and reading this guide locally, are on
  [opt-in documentation portal](capabilities/CAP-015-opt-in-documentation-portal.md).
- Second opinions and edit handoffs to the other model family are on
  [delegation](delegation.md).
- Interactive review documents for complex results are on
  [interactive review documents](present-guide.md).
- Task graphs, stronger verification techniques and choosing a code analyzer
  are on [duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md).
- What each of the four enforcement planes catches is on
  [enforcement planes](architecture/enforcement-planes.md).
- Where durable work belongs next to an external tracker is on
  [duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md).
