# Adopting CodeFlow

## Concept

**Pick a tier; the enforcement floor is the same at every tier.**

```cf-stage
install | one binary on PATH @accent
->
codeflow init | minimal · standard · full, recorded in project.toml
->
first session | orient digest · /cf-customize
->
gates | hooks at commit and push · guards in session · CI on the PR · remote at merge
->
codeflow update | refresh managed files by ownership class @positive
caption: a tier change is additive and idempotent; update merges by ownership class and preserves your edits
```

Adoption is one command at one of three tiers, and the tier decides only how
much method comes with the floor, never how strictly the floor is enforced. A
project can start at minimal on an existing repository and move up later without
undoing anything. Architecture below is the tier and ownership structure;
Technical is the commands, the files, and what `update` does to each of them.

## Architecture

Tier is recorded in `.codeflow/project.toml`; re-running `init` at a higher tier
is an idempotent additive upgrade. A lower-tier request is ignored: the recorded
tier remains active and no files are removed.

```cf-stage
minimal | hooks · CI · guards · armed policy @accent
->
standard | + skills · reviewer agents · knowledge model
->
full | + project-management records · work start gate @positive
caption: the floor never changes; tiers add method on top
```

Enforcement is the floor; the tiers scale installed method and work artifacts
(ADR-0019). Every tier is a clean superset of the one below. Tier describes
what init installs, not by itself whether historical CodeFlow task records
still activate durable-work checks in an existing repository. Minimal installs
the local floor and CI scaffold, not remote branch protection (ADR-0019).

| Tier | Adds | For |
|---|---|---|
| `--minimal` | The complete git-discipline enforcement floor: all five git hooks (`pre-commit`, `commit-msg`, `pre-push`, `pre-merge-commit`, `reference-transaction`), the CI check, the in-session `git-guard`/`exec-guard` + orient/summary hooks (`.claude/settings.json` + the `.codex/` starter), the armed `policy.json`, `.gitignore`, and a lean `AGENTS.md` + `CLAUDE.md` | Any repo: doc-sets, config repos, small tools |
| `--standard` (default) | + the develop-loop method (cf-* skills, reviewer agents, the pipeline), the six-layer `docs/` spine, the full contract, the test gate, recall capture, and harness integration | Code projects |
| `--full` | + `project-management/` (epics, tasks, specs, templates) and the `validate --docs` referential lint | Programs whose work outlives sessions |

Once the files are on disk, ownership decides what a later `update` may touch.
Five classes cover every managed path:

| Class | Examples | What `update` does |
|---|---|---|
| Fully-managed | `.claude/` agents, skills; git-hook shims; CI template | Replaced if you never touched them; 3-way merged from `.codeflow/.baseline/` if you did, and conflicts land as `.new` plus a report |
| Managed-region | `AGENTS.md` / `CLAUDE.md` markers; `.gitignore` markers; `.claude/settings.json` codeflow keys | Only the marked region or codeflow-owned keys are rewritten; everything else is yours |
| User-owned, schema-versioned | `.codeflow/policy.json`, `.codeflow/project.toml` | Only *new* keys are added with their defaults and reported; values you set are never mutated |
| User-owned docs (write-once seeds) | all of `docs/`: product, architecture, capabilities, ADRs | Seeded once at init; `update` never mutates them, so they are yours to edit and own |
| Engine-generated | `.codeflow/manifest.json`, `.codeflow/.baseline/`, `status` / `orient` views | Rewritten by the binary; never hand-edit |

`docs/product.md` always describes the consuming project's purpose, users,
scope, and non-goals, not the CodeFlow CLI. `docs/architecture.md` describes how
that project is built. Common project facts, commands, and constraints belong in
`AGENTS.md`; `CLAUDE.md` carries only Claude-specific differences. CodeFlow does
not introduce a competing `project.md` or `projects.md`.

Runtime autonomy and harness settings live in
[harness posture](harness-posture.md); qualifying a new model or harness lives
in [model and harness upgrades](model-upgrades.md).

## Technical

The sections below are the operating detail, in the order a project meets them:
installing the binary, choosing a root and deciding where durable work lives,
running `init` on a greenfield repository and configuring its test targets, the
same on a brownfield one, what `update` does afterwards, and the daily loop and
options that follow. Read the section matching the step you are on rather than
the whole panel.

### Install the binary

`codeflow` is a single binary. The latest verified published release is
v2.1.0. Its assets are `.tar.xz` archives with `.sha256` files for
`aarch64-apple-darwin`, `x86_64-apple-darwin`, and `x86_64-unknown-linux-gnu`,
a `sha256.sum`, the shell installer, and a source archive. It has no Windows
archive and no PowerShell installer. The workspace on `main` is the pending
3.0.0 source; its distribution targets add `x86_64-pc-windows-msvc` and a
PowerShell installer, but no 3.0.0 assets exist until a release is published.

| Path | Use it when |
|---|---|
| The anonymous installer one-liner | Once codeflow's releases are public. While the repository is private it will not resolve, so use one of the two rows below; both authenticate as a collaborator |
| `cargo install --path crates/codeflow-cli` | You want the pending 3.0.0 source from a checkout and have a Rust toolchain |
| `gh release download` of one platform archive | You are pinning a version or scripting the install |

```sh
curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
```

```sh
cargo install --path crates/codeflow-cli
```

Substitute the tag you mean to pin and one of the published archive names; each
archive unpacks to a directory of the same name containing the `codeflow`
binary:

```sh
TAG=vX.Y.Z                                # the published release you are pinning
A=codeflow-cli-aarch64-apple-darwin       # or x86_64-apple-darwin, x86_64-unknown-linux-gnu
gh release download "$TAG" -R sathyassn/codeflow -p "$A.tar.xz" -D /tmp/cf --clobber
tar -xf "/tmp/cf/$A.tar.xz" -C /tmp/cf
install "/tmp/cf/$A/codeflow" ~/.cargo/bin/    # or any directory on PATH
```

Any published binary predates parts of this guide: the commands and workflows
documented here describe the pending 3.0.0 source, and some of them are not
available in v2.1.0. To follow the current guide, build the pending source
checkout above.

Upgrading the binary improves every repo at once, because hooks call `codeflow`
from `PATH` (see "The update story").

### Before `init`: choose the root and authority

Run `init` or `update` from the intended project root: these commands use the
current directory, not a discovered Git root. Inventory existing source/build
units, deployables, release and data owners, instructions, docs, work tracking,
hooks, and CI before choosing a tier. Init does not rearrange product source.
It does install fixed operating destinations: root `AGENTS.md`/`CLAUDE.md`,
standard documentation entry points under `docs/`, and (at full) CodeFlow
records/templates under `project-management/`; these parsed homes are not
relocated by an arbitrary link or config setting. Pick the lowest tier whose
conventions the project accepts. Reinitializing lower does not undo a recorded
higher tier.

Use default init on an adoption branch and inspect every created, skipped, or
merged file. It preserves existing content mechanically, but a skip or combined
instruction file may still need semantic reconciliation. Do not use `--force`
as a collision shortcut. Later update conflicts preserve the original and
write a `.new` sidecar to resolve; update-ignore settings do not choose paths
for the initial scaffold. `/cf-customize` is a post-init skill installed with
the method, not a pre-init discovery tool or a promise that conflicts were
resolved automatically.
Unrelated files occupying a parsed CodeFlow record home can still make an
explicit `validate --docs` report a collision even when automatic durable-work
tracking is inactive; resolve the conflict rather than claiming graph validity.

### Organize durable work without duplicating it

The full tier writes independently allocated `epics/EPC-NNN.md`,
`specs/SPC-NNN.md`, and `tasks/TSK-NNN.md`; filenames are stable IDs, while
titles can evolve. Use `codeflow epic new`, `spec new --for <epic-or-task>`,
and `task new`. A single-surface repo and a shared-integration monorepo use the
same repo-level namespace. Plan tasks around reviewable outcomes and direct
dependencies, not one record per team or folder; the epic owns cross-area
integration evidence.

For active CodeFlow durable work, plan and validate records on a `plan/` branch, merge the
planning PR into each task's declared `integration_target`, then implement from
`task/TSK-NNN-<slug>`. `codeflow work start TSK-NNN` verifies that stable
anchor, parent or standalone rationale, approved specs, and completed
predecessors without mutating repository state.

CodeFlow is the natural authority for finite repo-local gated work. Keep the
team's external tracker authoritative for its own portfolio/product items when
work is multi-team, cross-repo, driven by assignment, roadmap or service-level
agreement (SLA), or already owned by an established method. Active full or recognizable historical CodeFlow task
tracking still requires distinct repository-execution records and planning
anchors: an external ticket or approved spec cannot satisfy or waive
`work start`, pre-commit, or CI. Link opaque IDs or URLs in epic/task template
`external_refs` metadata, not as gate inputs; do not mirror status, specs, or
task trees. When durable tracking is inactive, keep the approved external
method or native/session plan at earned durability without claiming these
workgraph guarantees or silently upgrading tier. A host-local database may
cache/index records but is not shared team authority. `/cf-customize` records
this post-init project choice; the agent-facing decision model lives in
`cf-method/references/project-organization.md`.

### Greenfield: an empty directory

```sh
mkdir myproject && cd myproject && git init
codeflow init --standard --yes    # scaffold; offline; sane defaults
```

`init` scaffolds every managed file, then, because this is a fresh repo, makes
the initial `chore: scaffold codeflow standard tier` commit for you, wires git
hooks via `core.hooksPath`, and arms branch policy. The printed report lists
every file written and closes with the next step. From there:

1. Run `/cf-customize`. It verifies the installed harness settings and tools,
   then walks the **consuming project's** `docs/product.md`,
   `docs/architecture.md`, `AGENTS.md`, Claude-specific differences in
   `CLAUDE.md`, README/manifests, CI commands, policy, and required Model
   Context Protocol (MCP) servers. Review and commit those project facts;
   CodeFlow never invents them or silently changes global harness settings.
2. Start your first feature on a `feat/*` branch, in a worktree.
3. Build with tests; commit small (`type(scope): description`, with description ≤ 50
   chars, subject line ≤ 72, a body of only `-` bullets when one is needed).
4. Land via a PR (or `codeflow integrate` with no remote).

**Bootstrap grace.** codeflow needs exactly one commit before its gates guard
the repo, its own scaffold commit, and that is a sanctioned path (it arms
`policy_armed` and passes the hooks via the gate-context token), so you never
hit a policy wall on the way to your first PR (the v2 charter, in the repository
at `docs/plan/v2/00-charter.md`, §16 AC #1). The secret scan is the one rule
that is never graced (charter §6.3).

### Configure test targets

`codeflow test setup` is a safe deterministic starting point. With no options it
checks only root-level `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`,
or `setup.py` markers and writes detected targets into an absent or empty
`.codeflow/test-config.json`. It does not recursively inspect workspaces or
guess package boundaries, and it never auto-replaces a populated or malformed
config.

```sh
codeflow test setup --list-templates
codeflow test setup --template example-rust.json
codeflow test setup --add-target
```

For a monorepo, start with `monorepo-multi-target.json` or append one target per
package and set each target's repository-relative `cwd`. Review and tailor every
command to the consuming project. Replacing an existing config is deliberately
separate: `codeflow test setup --template <name> --replace`. Run `codeflow test`
and `codeflow doctor --check test-config` after setup.

### Brownfield: an existing repo

`init` on a repo with history is deliberately gentler:

| Behavior | What it means |
|---|---|
| Nothing is committed for you | Scaffold files are written and left uncommitted with a note to review and commit them on a branch |
| Nothing is clobbered | Existing files are never overwritten (`--force` is never the default); an existing `CLAUDE.md`, `.claude/settings.json`, or `.gitignore` is merged by region, not replaced (see the ownership table) |
| Your hook manager is respected | If `.husky/` or a custom `core.hooksPath` already exists, codeflow detects it and does **not** take over hooks. It records `git_hooks = "unwired"`, and the report tells you how to call codeflow's shims from your manager (add the `pre-commit`, `commit-msg`, `pre-merge-commit`, `reference-transaction`, and `pre-push` shim paths to your existing steps). `codeflow doctor` surfaces the unwired state so it stays visible |

Adopt gradually: start `--minimal` (the full enforcement floor of all the git
hooks, CI, the in-session guards, and the armed policy, with none of the method
machinery), run for a while, then re-init `--standard` and later `--full` as the
work earns the weight. Each tier step is additive and idempotent, and because the
floor is the same at every tier, the upgrade adds the method on top; existing
recognizable historical CodeFlow tasks may already keep durable tracking
active below full. Inspect installed state and the gate result rather than
assuming the tier name switches it off. A `--minimal` repo initialized before
this floor moved down gains the rest of the enforcement plane automatically on
its next `codeflow update` (the reconciliation installs the now-in-tier files).
When you add the standard/full method, run `/cf-customize` before treating the
generated product, architecture, commands, or tool posture as project truth;
`codeflow doctor` keeps a reminder visible while scaffold sentinels remain.

### The update story

Two motions (charter §10):

| Motion | How | Effect |
|---|---|---|
| Upgrade the binary | Re-run an install path: the `curl \| sh` installer again, or `git pull` then `cargo install --path crates/codeflow-cli`, or re-download the newer tarball via `gh release download` (see Install). There is no self-updater (`install-updater = false`) | Improves every repo at once, because hooks call `codeflow` from `PATH` |
| `codeflow update` per repo | Run it in the repository | Refreshes scaffold files. Until you do, every command prints a version-skew warning |

`codeflow update` refreshes managed files by the classes in Architecture:
unmodified managed files are replaced, files you changed get a 3-way merge from
the baseline, and anything that cannot merge cleanly is written beside your file as
`<name>.new` with a report entry, never clobbered and never silently skipped.
`--diff <FILE>` writes the report plus unified diffs; `--force` replaces
user-modified managed files instead of merging.

`update` also **reconciles orphans**: when an artifact is renamed or dropped
upstream (as the `.claude/commands/*` slash commands became `.claude/skills/*`),
an unmodified managed file the new version no longer ships is removed, with its
baseline and manifest record, so it cannot linger and collide with its
replacement. Anything that could hold your content, whether a managed file you modified,
a managed-region file, or a user-owned file, is kept and simply unmanaged, never
deleted (ADR-0011).

New policy keys arrive this way too. When a codeflow upgrade adds a
`.codeflow/policy.json` key (for example the `merge_to_protected`,
`pr_merge_to_protected`, and `local_ref_protection` keys added in ADR-0007),
`update` inserts it with its shipped default and reports it, and never touches
the values you already set, so tightening ships without a manual migration.
Keep the order: upgrade the `codeflow` on `PATH` before `codeflow update`. The
hooks run that binary, and one older than a new key rejects the policy file,
which blocks every commit until the binary is upgraded (for example
`git.policy_characters`, ADR-0067).

### The daily flow

```cf-stage
orient | session digest
->
branch + worktree | task/TSK-NNN under .worktrees/
->
develop | small commits, hooks on every one
->
codeflow test · validate | local gate
->
PR | human-merged on green checks @positive
caption: every step has a plane that catches the mistake it can make
```

1. **Orient and route.** The SessionStart digest (or `codeflow orient`) gives branch and
   worktree state, work counts, recent ADR titles, gate status, and pointers.
   Read the pointed docs, not the digest, for depth. Begin every non-trivial
   repository task with `/cf-model-orchestrator`; it uses only the research,
   planning, implementation, or review stages the requested outcome needs and
   degrades visibly if a native peer seat is unavailable.
2. **Branch in a worktree.** Work on a `{prefix}/{kebab-name}` branch in a
   worktree; never develop on the root protected-branch checkout.
3. **Gates as you go.** pre-commit (secret scan), commit-msg (conventional
   format with the restored 50-char description / 72-char subject budget and the
   bullet-only body shape of ADR-0020, plus no AI attribution and no emoji),
   pre-merge-commit and reference-transaction
   (protected-branch merge/ref rules; the latter also catches fast-forward
   merges, `reset --hard`, and `branch -D`), pre-push (branch naming,
   protected-branch rules, test gate). Keep `codeflow test` and `codeflow
   validate --docs` green before push.
4. **Land by PR, merged by a human.** Push the branch, open a PR from the
   template (summary, changes, testing, linked epic/capability IDs); a
   human merges it when required checks are evidenced green (an infra-killed
   duplicate CI job is not a failed check; an agent-performed `gh pr merge`
   into a protected base is blocked, and that is the boundary). With no remote, `codeflow
   integrate <branch> --into <target>` is the sanctioned local path, and a human
   can override the git layer for a local merge with `CODEFLOW_HUMAN_OVERRIDE=1`.

### A body of work: the integration branch

The loop above lands one standalone task per PR onto `main`. When the work is a
multi-task epic with several tasks, some serial and some parallel, its default
landing shape is one shared **integration branch**. Do not casually turn the
epic into a series of task-to-`main` PRs: a different shape needs a recorded
Plan vN rationale and approval from both primary model seats before allocation.
The normal path is:

- `integration/<epic>` is branched off `main` and is **non-protected**, so
  agents merge tasks into it, by `codeflow integrate <task> --into
  integration/<…>` or a PR based on the integration branch. Every other gate
  (commits, secrets, tests, protected-branch rules) still applies.
- Only the finished body reaches `main`, as **one** human-reviewed
  `integration → main` PR raised after the ship flow runs on the integration
  branch.

Parallelize only independent tasks whose isolation pays for the coordination:
one owner/branch/worktree per task, one owner for shared contracts and conflict
hotspots, and a concurrency cap based on available memory, CPU, disk, model
contexts, and browser/tool capacity. Land tasks serially through
`codeflow integrate`, rerun affected gates after each landing, then run the
aggregate suite and both-model review on the combined integration diff. Never
run concurrent writers in one worktree or rebase the shared integration branch.

`main` stays human-merge-only throughout, and the integration branch is never a
backdoor to it. See cf-method's "Managing a body of work" for the full procedure.

### Task graphs and verification strength

For one obvious task, the settled plan records
`TASK_GRAPH: N/A (single task)`. For multi-task work, `/cf-plan` turns the
approved assignments and real dependencies into one acyclic Plan vN graph.
Every durable task lists its direct structural predecessors:

```text
brief
  |
  v
Plan vN task graph ---- genuine decision guards + selection evidence
  |                                      |
  v                                      v
task `depends_on`                 selected / not_selected
  |
  v
codeflow validate --docs
  |
  +-- identity, references, duplicates, self-edges, cycles
```

The metadata preserves topology; it does not execute the plan. Bare active
predecessors must land. Plan guards select mutually exclusive branches, and a
later join may list every structural candidate while waiting only for active
predecessors plus resolution evidence for the alternatives. Missing or
ambiguous guard evidence creates Plan vN+1 rather than an improvised route.
Material node, edge, ownership, interface, acceptance, or safety changes also
require a new version and both approvals. Ordinary steps, bounded rework, extra
strengthening tests, or another safe topological order inside the same contract
remain execution-ledger evidence.

Normal stack tests, integration/end-to-end checks, regressions, and coverage
remain the baseline. The plan adds a stronger technique only when its evidence
fits:

- property or generative tests need a stable invariant, meaningful input/state
  space, reproducibility and shrinking, plus a material combination risk;
- mutation testing is targeted and time-bounded to consequential guard,
  decision, state, security, or recovery logic after the base suite is reliable;
  and
- an architecture fitness check protects a current project-owned invariant
  through a deterministic observable rule tied to a decision or repeated risk.

`/cf-stack` and `/cf-customize` reuse or propose the consuming project's own
reviewed commands only when earned. They do not install every technique, create
whole-repository score targets, or turn architectural taste into a gate.

### Selecting deterministic code analysis

CodeFlow does not impose one static application security testing (SAST) service
on every stack. During `/cf-customize`, `cf-stack` inventories languages, trust
boundaries, hosting, existing tools, and CI constraints, then records the
smallest maintained lane that provides relevant source/data-flow or taint
evidence. CodeQL default setup is a low-maintenance choice for an eligible
GitHub-hosted repository and a supported language; Semgrep, Sonar, or a
language-native analyzer may fit other stacks or governance requirements.
Software composition analysis (SCA) and secret scanning remain separate
evidence and do not substitute for SAST.

Verify the chosen analyzer rather than merely installing it: record applicable
rules, scanned-file/tool status, extraction errors, suppressions, owner,
cadence, and whether it gates locally, in CI, or through branch protection. If
no relevant lane is available, record the residual risk and disposition.
CodeFlow's own post-public CodeQL setting is repository-specific and is not
copied into consuming projects by `init` or `update`.

### Delegation quickstart

Standard/full projects use the duo for non-trivial work; standalone consults
are available when an outside opinion is useful. Minimal does not install the
method. Transport remains interactive-only, with preferred lanes and qualified
native fallback (ADR-0059). One-time setup: authenticate Codex manually, enable
`codex@openai-codex` in Claude Code, and install the Claude CLI with Herdr for the reverse lane.
Herdr is the named-tab terminal host that runs an interactive peer CLI in its
own tab, with tmux as the documented degraded host. CodeFlow
never automates auth. `codeflow doctor` reports inspectable prerequisites;
retain a scoped interactive canary in each direction.

- `/cf-consult` gets an independent, read-only second opinion from the vendor
  the session is *not*: from Claude Code through the official
  `codex-plugin-cc` plugin (`/codex:review`); from Codex by driving the
  interactive `claude` CLI via Herdr (tmux degraded) with Stop/StopFailure
  hook completion. It makes you synthesize the reply against your
  own analysis (never paste its reply as fact). Headless `codex exec` /
  `claude -p` are not sanctioned delegation transports (ADR-0023).
- A full edit handoff (`cf-delegate`; from Claude Code, `/codex:rescue`) runs
  only inside a worktree on a feature branch, where the delegate's commits pass
  the same gates and independent review as yours, because enforcement is
  author-agnostic.
- An incompatible plugin permits the qualified official native client fallback
  in `cf-delegate`; verify its tools, boundaries and recheckable native result.
  If no qualified peer route remains, degrade legibly and say so.

### Optional repository guide portal

Standard and full tiers include the concise `cf-docs-portal` workflow, but no
Node workspace or lockfile. Adopt the utility only when layered navigation,
search, source links, and machine-readable documentation twins justify it:

```sh
codeflow portal setup --path docs-portal
cd docs-portal
npm run deps:install
npm run check
npm run build
cd ..
codeflow validate --portal docs-portal
```

The managed dependency installer checks the lockfile's lifecycle-script
inventory, keeps those scripts disabled, and passes only a small non-secret
environment to npm. Do not substitute plain `npm ci`; a required future
lifecycle exception belongs in the reviewed managed installer with an
executable canary. The setup itself is offline and repository-relative. It
records one adopted root. Repeated setup and ordinary `codeflow update` replace
unchanged managed files and repair missing managed files, but stop all portal
writes on local runtime edits or unknown collisions. There is no source merge,
sidecar or pristine runtime directory. `portal.config.json` is project-owned:
after first adoption, even its absence is preserved and reported. Configure
source roots there rather than copying authoritative prose into the portal. Its
`records.enabled` key names who owns the record folders, not whether they are
visible: left `false`, the configured folders stay out of the page set and one
generated pointer page names them; set `true`, the project publishes those
folders itself as ordinary pages and declares no pointers. Generated content and search
output remain disposable, and local generation never publishes a site.

Use supported config/token settings and unclaimed asset paths for ordinary
customization. Bundled public assets remain managed. For a bespoke runtime,
explicitly choose `codeflow portal transfer --confirm`: current edits/deletions
survive and future setup/update does not reconcile that runtime. The project
then owns runtime upgrades, dependencies and compatible generator evidence.
Never transfer automatically to clear a conflict. See the installed skill's
operations reference and [release migration guidance](releasing.md#portal-ownership-migration)
for journal-first legacy recovery and baseline-integrity failures.

The portal applies the same utility presentation craft as `cf-present`
(tokens, altitude, stage grammar) over durable source-linked docs for this
repository or any consuming project. It has no session Comment lifecycle. The
design-exploration board that settled the craft is a reference, not a page to
clone. Read `cf-docs-portal` for content, dependency, browser, accessibility,
evidence, and cleanup obligations.

#### Reading the CodeFlow guide locally

This repository dogfoods the starter at `docs-portal/`: a managed runtime, a
project-owned `portal.config.json`, and the `signal` theme. The guide is a
derived view of the Markdown under `docs/` and `project-management/`. Those
files remain the only authority; every generated page, Markdown twin, search
index, and `llms.txt` is disposable output that Git ignores. Nothing publishes
it: there is no hosted site, and GitHub shows the Markdown sources, not the
generated HTML.

Build it from a clean committed checkout, using the Node version pinned in
`docs-portal/.node-version` (24.18.0). The adapter reads only committed bytes
and refuses a snapshot whose portal runtime, configuration, or configured
source roots differ from `HEAD`, including untracked files under those roots,
so commit source edits before building:

```sh
cd docs-portal
npm run deps:install   # locked install, dependency scripts off
npm run check          # adapter tests, derivation, Astro check
npm run build          # derive pages and evidence, build site
npm run preview        # serve dist/ on loopback until stopped
cd ..
codeflow validate --portal docs-portal
```

`check`, `build`, `dev`, and `preview` share one workflow lease: run one at a
time and stop the preview before the next build. `npm run dev` derives the
pages once and serves them through Astro's dev server for authoring; it reads
the same committed snapshot. `npm run browser:verify` runs the isolated
headless journey matrix when Playwright browsers are installed.

The home page names the exact repository commit the guide was built from and
no release version. That is deliberate: the workspace source identifies as
3.0.0, which is pending and unpublished, while `v2.1.0` remains the latest
verified published release (see the
[historical bridge into v3](releasing.md#historical-bridge-into-v3)). A
`release_version` value renders as a release label, so it stays `null` until a
verified published release exists for the built commit.

Node roles differ by lane, and neither pin changes here: the aggregate CI gate
runs on Node 26.4.0, the presentation renderer's pin, and its full strict
target installs, checks, builds, and validates this portal; the portal-local
`.node-version` and the Windows adapter-test lane use 24.18.0; the starter
itself accepts 22.19.0 or newer.

### Optional interactive review documents

Standard and full tiers also include `cf-present`. It is a bounded review
utility for complex explanations, alternatives, plans, diffs, and evidence, not
a product UI or durable documentation store. Author **this session's** subject
into a schema-valid temporary JSON document from the skill's block catalog.
The runtime owns chrome, themes, and Comment. Do not clone the
design-exploration board or invent a second visual language. The CLI enforces
semantic and byte limits and opens a task-owned isolated browser profile:

```sh
codeflow present open /path/to/review-document.json
codeflow present list
codeflow present feedback <session-id>
codeflow present close <session-id>
codeflow present clear --dry-run
```

Use `update` for immutable revisions, `resolve` for delivered feedback, and
`export` only when a portable read-only artifact is required. Keep inputs,
feedback, exports, and session state untracked unless the user deliberately
promotes an outcome to its real task, spec, ADR, capability, or project doc.
Close sessions when review ends and dry-run retention cleanup before removal.
The utility never attaches to the operator's browser or replaces ordinary chat
for a short answer. Read `cf-present` for the exact routing, authoring,
feedback, evidence, and cleanup contract.

### Enforcement planes: who catches what

The four enforcement planes, what each one catches, and the rules behind them
are on [enforcement planes](architecture/enforcement-planes.md) in the System
layer.
