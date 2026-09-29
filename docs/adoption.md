# Adopting codeflow

How a project takes on the discipline layer — greenfield or brownfield — what it
gets at each tier, what codeflow owns versus what stays yours, and the daily
loop. Every claim here reflects current behavior; nothing aspirational.

## Install the binary

`codeflow` is a single binary. From 3.0.0 on, each release publishes
`.tar.xz` archives for `aarch64-apple-darwin`, `x86_64-apple-darwin` and
`x86_64-unknown-linux-gnu`, a `.zip` archive for `x86_64-pc-windows-msvc`, a
`.sha256` file for each archive, a `sha256.sum`, a shell installer, a
PowerShell installer and a source archive. Install the latest release on macOS
or Linux:

```sh
curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
```

On native Windows, in PowerShell (Git for Windows is required):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.ps1 | iex"
```

Or build from a checkout, with a Rust toolchain:

```sh
cargo install --path crates/codeflow-cli
```

To pin a version or script the install, download one platform archive and
check it against its `.sha256` file. Substitute the version you pin and your
platform's target (the Windows archive is a `.zip`). Releases before 3.0.0
publish fewer assets, so check a release's asset list before pinning it. Each
archive unpacks to a directory of the same name containing the `codeflow`
binary. The commands run in a subshell that stops at the first failure, so
nothing is extracted or installed unless the download and the checksum pass;
`D` must be a directory on your `PATH`:

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

## What each tier installs

Tier is recorded in `.codeflow/project.toml`; re-running `init` at a higher tier
is an idempotent additive upgrade. A lower-tier request is ignored: the recorded
tier remains active and no files are removed.

Enforcement is the floor; the tiers scale installed method and work artifacts
(ADR-0019). Every tier is a clean superset of the one below. Tier describes
what init installs, not by itself whether historical CodeFlow task records
still activate durable-work checks in an existing repository.

| Tier | Adds | For |
|---|---|---|
| `--minimal` | The complete git-discipline enforcement floor: all five git hooks (`pre-commit`, `commit-msg`, `pre-push`, `pre-merge-commit`, `reference-transaction`), the CI check, the in-session `git-guard`/`exec-guard` + orient/summary hooks (`.claude/settings.json` + the `.codex/` starter), the armed `policy.json`, `.gitignore`, and a lean `AGENTS.md` + `CLAUDE.md` | Any repo — doc-sets, config repos, small tools |
| `--standard` (default) | + the develop-loop method (cf-* skills, reviewer agents, the pipeline), the six-layer `docs/` spine, the full contract, the test gate, recall capture, and harness integration | Code projects |
| `--full` | + `project-management/` (epics, tasks, specs, templates) and the `validate --docs` referential lint | Programs whose work outlives sessions |

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
work is multi-team, cross-repo, assignment/roadmap/SLA driven, or already owned
by an established method. Active full or recognizable historical CodeFlow task
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

## Greenfield — an empty directory

```sh
mkdir myproject && cd myproject && git init
codeflow init --standard --yes    # scaffold; offline; sane defaults
```

`init` scaffolds every managed file, then — because this is a fresh repo — makes
the initial `chore: scaffold codeflow standard tier` commit for you, wires git
hooks via `core.hooksPath`, and arms branch policy. The printed report lists
every file written and closes with the next step. From there:

1. Run `/cf-customize`. It verifies the installed harness settings and tools,
   then walks the **consuming project's** `docs/product.md`,
   `docs/architecture.md`, `AGENTS.md`, Claude-specific differences in
   `CLAUDE.md`, README/manifests, CI commands, policy, and required MCPs. Review
   and commit those project facts; CodeFlow never invents them or silently
   changes global harness settings.
2. Start your first feature on a `feat/*` branch, in a worktree.
3. Build with tests; commit small (`type(scope): description` — description ≤ 50
   chars, subject line ≤ 72, a body of only `-` bullets when one is needed).
4. Land via a PR (or `codeflow integrate` with no remote).

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

**Bootstrap grace.** codeflow needs exactly one commit before its gates guard
the repo — its own scaffold commit — and that is a sanctioned path (it arms
`policy_armed` and passes the hooks via the gate-context token), so you never
hit a policy wall on the way to your first PR (charter §16 AC #1). The secret
scan is the one rule that is never graced (charter §6.3).

## Brownfield — an existing repo

`init` on a repo with history is deliberately gentler:

- **Nothing is committed for you.** Scaffold files are written and left
  uncommitted with a note to review and commit them on a branch.
- **Nothing is clobbered.** Existing files are never overwritten (`--force` is
  never the default); an existing `CLAUDE.md`, `.claude/settings.json`, or
  `.gitignore` is merged by region, not replaced (see the ownership table).
- **Your hook manager is respected.** If `.husky/` or a custom `core.hooksPath`
  already exists, codeflow detects it and does **not** take over hooks. It
  records `git_hooks = "unwired"`, and the report tells you how to call
  codeflow's shims from your manager (add the `pre-commit`, `commit-msg`,
  `pre-merge-commit`, `reference-transaction`, and `pre-push` shim paths to your
  existing steps). `codeflow doctor` surfaces the unwired state so it stays
  visible.

Adopt gradually: start `--minimal` (the full enforcement floor — all the git
hooks, CI, the in-session guards, and the armed policy — with none of the method
machinery), run for a while, then re-init `--standard` and later `--full` as the
work earns the weight. Each step is additive and idempotent, and — because the
floor is the same at every tier — the upgrade adds the method on top; existing
recognizable historical CodeFlow tasks may already keep durable tracking
active below full. Inspect installed state and the gate result rather than
assuming the tier name switches it off. A `--minimal` repo initialized before
this floor moved down gains the rest of the enforcement plane automatically on
its next `codeflow update` (the reconciliation installs the now-in-tier files).
When you add the standard/full method, run `/cf-customize` before treating the
generated product, architecture, commands, or tool posture as project truth;
`codeflow doctor` keeps a reminder visible while scaffold sentinels remain.

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

A profile applies in `codeflow ci` only when the actor the workflow passes
(`--actor`) and the branch both match, and only as the target branch's policy
states it, so a pull request cannot add a profile for itself. It skips branch
naming and the commit message shape rules. Tests, the secret scan, AI
attribution, emoji, the dash rule and the release declaration still run, and
PR sections are still checked at their configured level: `sections` only
supplies the headings the bot body leaves out. The actor is trusted only in a
GitHub Actions pull request event from the same repository, and only as that
event's own actor. In a local run, in another CI and on a fork pull request
the actor is `unknown`, whatever `--actor` says, and no profile applies.

**A PR template you already have.** `init` keeps it and installs no second
template; `update` never merges into it or writes a `.new` beside it. When its
headings differ from `git.pr_required_sections`, the run records
`git.pr_section_mapping` as `diagnosed` with a proposed mapping (for example
`Summary -> Description`) and asks you to decide:

| Decision | What happens |
|---|---|
| accepted | the check reads your template's headings in place of the mapped ones |
| refused | the missing required headings are appended to your template |
| custom | you set `git.pr_required_sections` and `git.pr_code_sections` yourself in a reviewed change |

Whatever the decision, a pull request that changes only Markdown under
`docs/` or `project-management/`, outside your product and watched contract
paths and the other shared contract surfaces, needs just Summary and Changes
of your required list, under the template's headings when the mapping is
accepted, and may leave out Release impact.

A run without a terminal (`--yes`, CI) leaves the state `diagnosed`, and
`codeflow doctor` repeats it. While it is diagnosed the PR-section check runs
at `warn` only on a fresh install whose policy file `init` created; a
`pr_sections` value already in your policy file stays in force, even when it
equals the default. `codeflow ci` prints each check's effective level and
where it comes from (`configured`, `shipped default` or `diagnosed`). These
policy edits, and the keys `codeflow update` adds, are spliced into the file
as you wrote it, so no other byte changes; a policy with no `git` object
gains one. A template reached through a symlink out of the repository is
never read or written.

**Release tools.** `release.backend` in `.codeflow/project.toml` names who owns
the version of each release unit:

| Value | Meaning |
|---|---|
| `none` (default at every tier) | CodeFlow checks only the PR's Release impact declaration (`git.pr_release_impact`, warn by default); it calculates no version |
| `external` | another tool owns versions (release-please, Changesets, semantic-release, cargo-release, GoReleaser); CodeFlow checks the declaration and `breaking_watch_paths` only |
| `codeflow` | you adopted CodeFlow's release calculator explicitly |

Keep one version authority per release unit: `init`, `codeflow ci` and
`doctor` say so when a release tool's configuration sits beside `none` or
`codeflow`. Keeping the PR template is not consent to a calculator. The
calculator CodeFlow ships, `scripts/release.py`, serves CodeFlow's own
repository: one Rust workspace versioned from `Cargo.toml`, with its
`CHANGELOG.md` in Keep a Changelog form. Other projects use `none` or
`external`.

**Minimal tier.** The minimal tier installs no release process. Declare each
PR's Release impact in its body; if you publish releases, let your own tool
own the version and set `backend = "external"`.

## Ownership model — who owns what on update

| Class | Examples | What `update` does |
|---|---|---|
| Fully-managed | `.claude/` agents, skills; git-hook shims; CI template | Replaced if you never touched them; 3-way merged from `.codeflow/.baseline/` if you did — conflicts land as `.new` + a report |
| Managed-region | `AGENTS.md` / `CLAUDE.md` markers; `.gitignore` markers; `.claude/settings.json` codeflow keys | Only the marked region or codeflow-owned keys are rewritten; everything else is yours |
| User-owned, schema-versioned | `.codeflow/policy.json`, `.codeflow/project.toml` | Only *new* keys are added with their defaults and reported; values you set are never mutated |
| User-owned docs (write-once seeds) | all of `docs/` — product, architecture, capabilities, ADRs | Seeded once at init; `update` never mutates them — they are yours to edit and own |
| Engine-generated | `.codeflow/manifest.json`, `.codeflow/.baseline/`, `status` / `orient` views | Rewritten by the binary; never hand-edit |

## The update story

Two motions (charter §10): upgrade the binary — which improves every repo at
once, because hooks call `codeflow` from `PATH` — then run `codeflow update` per
repo to refresh scaffold files. Until you do, every command prints a
version-skew warning.

There is no self-updater (`install-updater = false`), so "upgrade the binary"
means re-running an install path: the `curl | sh` installer again, or `git pull`
then `cargo install --path crates/codeflow-cli`, or re-download the newer tarball
via `gh release download vX.Y.Z …` (see Install).

`codeflow update` refreshes managed files by the classes above: unmodified
managed files are replaced, files you changed get a 3-way merge from the
baseline, and anything that cannot merge cleanly is written beside your file as
`<name>.new` with a report entry — never clobbered, never silently skipped.
`--diff <FILE>` writes the report plus unified diffs; `--force` replaces
user-modified managed files instead of merging.

`update` also **reconciles orphans**: when an artifact is renamed or dropped
upstream (as the `.claude/commands/*` slash commands became `.claude/skills/*`),
an unmodified managed file the new version no longer ships is removed — with its
baseline and manifest record — so it cannot linger and collide with its
replacement. Anything that could hold your content — a managed file you modified,
a managed-region file, or a user-owned file — is kept and simply unmanaged, never
deleted (ADR-0011).

New policy keys arrive this way too. When a codeflow upgrade adds a
`.codeflow/policy.json` key (for example the `merge_to_protected`,
`pr_merge_to_protected`, and `local_ref_protection` keys added in ADR-0007),
`update` inserts it with its shipped default and reports it, and never touches
the values you already set — so tightening ships without a manual migration.
Keep the order: upgrade the `codeflow` on `PATH` before `codeflow update`. The
hooks run that binary, and one older than a new key rejects the policy file,
which blocks every commit until the binary is upgraded (for example
`git.policy_characters`, ADR-0067).

CI pins its binary too. The scaffolded workflows install the release named by
`scaffold_version` in the target branch's `.codeflow/project.toml` and verify it
against that release's `sha256.sum`; a missing or wrong checksum fails the job
and nothing unverified is installed. The commit and PR-body standards run in
`codeflow-policy.yml` on `pull_request_target`. GitHub runs that workflow from
the default branch, so a pull request cannot edit the job that judges it, and
the job checks out the pull request's base commit, so a pull request into an
integration branch is judged by that branch's pin and policy, not the default
branch's. An upgrade therefore takes two pull requests, in order:

1. Install the new binary locally, then land a pull request that raises only
   `scaffold_version`. The target's current binary judges it, and the
   `candidate codeflow` job tests the new one.
2. On a new branch, run `codeflow update` and land its new keys and files; the
   new binary judges them.

A pull request that adds policy keys before step 1 has landed fails with a
message naming this order: the enforcing job reads the head's policy as data
and fails when the pinned binary cannot read it. The git hook shims check the binary first and warn
when it is older than they are, then run the checks it has.

Work records follow the same order. `codeflow update` adds
`git.work_records` (`block` or `warn`; an `off` from an unreleased build is
rewritten to `warn` with a notice) and, in a project that already has epic,
spec or task records, records `work_records_baseline` in
`.codeflow/project.toml` once, as the current commit. Records whose bytes are
unchanged since that commit keep the rules they were written under; a status
change, a criteria change or a new record follows the status verbs' rules.
A task completed before that commit, with no acceptance block, reopens with
`codeflow task status <id> todo --reason <text>`, which keeps its Closeout and
adds the line `- reopened: <text>`.

`codeflow update` also adds `git.work_planning` (`block` or `warn`, default
`block`), the level at which `work start` and `codeflow ci` report the
planning checks: a valid workgraph, a record for the task the branch
carries, and that record anchored on its target. Pre-commit does not run
them. A value the project set is kept.

## Optional repository guide portal

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
after first adoption, even its absence is preserved and reported. Configure source roots there rather
than copying authoritative prose into the portal. Generated content and search
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

### Reading the CodeFlow guide locally

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

The home page names the exact repository commit the guide was built from. A
`release_version` value renders as a release label, so it stays `null` unless
a verified published release exists for the built commit.

Node roles differ by lane, and neither pin changes here: the aggregate CI gate
runs on Node 26.4.0, and its full strict target installs, checks, builds, and
validates this portal; the portal-local `.node-version` and the Windows
adapter-test lane use 24.18.0; the starter itself accepts 22.19.0 or newer.

## Optional interactive review documents

Standard and full tiers also include `cf-present`. It is a bounded review
utility for complex explanations, alternatives, plans, diffs, and evidence—not
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

## Network, tools, and autonomy

These are enabled in runtime settings as well as described in the skills
(ADR-0025):

### Platform assurance

CodeFlow releases target macOS, Linux, and x86-64 native Windows. Windows users
can also run the Linux build inside WSL2; this is the preferred route for a
Linux-native toolchain or Claude work that needs OS-enforced sandboxing.

- macOS uses each harness's native sandbox. Linux and WSL2 use their Linux
  sandbox implementations; install the dependencies reported by the harness
  and keep fail-closed startup enabled.
- Native Windows Codex uses its elevated Windows sandbox by default. The
  scaffolded deterministic guard recognizes both Bash and PowerShell command
  events and Windows drive, system, profile, disk, recovery, and permission
  operations.
- Native Windows Claude Code can use Git Bash or PowerShell, but Claude's OS
  sandbox is not available there. The settings and hooks still apply, but they
  are not equivalent containment. Use WSL2 or a container for catastrophic or
  otherwise high-blast-radius work; if that boundary is unavailable, stop.
- The test runner's per-target `shell` is `auto` by default (`sh` on
  macOS/Linux/WSL2, `cmd.exe` on native Windows). This keeps generated `&&`
  command chains compatible with the shells built into each OS. Set it to `sh`,
  `powershell`, or `cmd` only when the consuming project's toolchain requires a
  specific shell. `/cf-customize` must canary the selected shell and commands.

The catastrophic classifier is a non-relaxable floor, not a complete endpoint
security product. Managed organization policy, least-privilege host accounts,
verified backups, and authenticated human approval remain necessary at higher
blast radii.

Authenticated command-line tools must be able to read their own configuration,
so the OS sandbox does not deny `~/.config/gh` or
`~/.docker/config.json` to every subprocess; doing so would also disable `gh`
and Docker. Prefer OS keychains and credential helpers rather than plaintext
tokens in those files. The harness denies direct file-reading tools, but a host
that cannot provide brokered or helper-backed credentials must treat arbitrary
shell access as credential-bearing and tighten that task's tool boundary.

- Claude's project preset enables a fail-closed OS sandbox, autonomous
  sandbox-contained Bash, web search/fetch, wildcard public-domain egress for
  dependency/tool subprocesses, and local port binding for dev/UI tests;
  common private, link-local, and internal-name destinations remain denied. It
  permits an auto-classified unsandboxed retry only after sandbox failure and
  only for a trusted installed tool that needs host state, such as the official
  Codex plugin. Arbitrary unsandboxed commands remain out of bounds, and
  destructive, privileged, publish, and secret-read boundaries remain. Claude deliberately ignores repository
  requests for auto mode and classifier policy, so a Codex-hosted peer launches
  interactively with `--permission-mode auto` and CLI-scoped
  `autoMode.classifyAllShell`; `/cf-customize` can offer the equivalent user
  default but never writes it without approval.
- Codex's `.codex/config.toml` selects the guarded workspace permission profile
  without a legacy `sandbox_mode` override, enables live search, and sets
  `approval_policy = "never"` (never request approval) with `model_reasoning_effort =
  "medium"`. Production launch also passes `--sandbox danger-full-access` (full
  access), so the OS sandbox is off for that process; git-guard, exec-guard,
  git hooks, and CI remain the floor. Catastrophic work still stops for the
  operator. `never` alone grants no access; operations outside the effective
  sandbox fail instead of asking. Approval policy and sandbox authority are
  separate controls ([Codex security](https://learn.chatgpt.com/docs/security)).
- A settings file cannot install or authenticate every task-specific tool.
  `/cf-customize` inventories and canaries authoritative-doc research, GitHub,
  the stack format/lint/test/coverage/security toolchain, browser/Playwright,
  Computer Use or a surface driver, design tooling, and project-specific MCPs.
  It proposes only the missing pieces. Authentication stays in OAuth, keychain,
  app/MCP, or supported credential-broker paths; raw tokens do not enter the
  repository, prompts, logs, or arbitrary commands. GitHub/Docker configuration
  is permitted for autonomous tool use only after `/cf-customize` proves secure
  keychain or credential-helper storage; on a keyring-less inline-credential
  host it adds a file deny until a broker is configured.
- The shipped Claude sandbox removes the exact Anthropic, OpenAI, and AWS raw
  credentials named in ADR-0026 from Bash without stripping credentials from
  every hook or stdio MCP. When a project needs a raw GitHub, npm, Cargo, or
  provider token, prefer a broker/keychain; otherwise configure Claude's
  user/managed credential mask with TLS termination and exact `injectHosts`.
  `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` is a user/managed hardening option only
  after proving the project's hooks and stdio MCPs do not require those
  provider credentials.
- Codex `auto_review` is configured but is not a human authorization path.
  Production `approval_policy = "never"` plus `--sandbox danger-full-access`
  does not pause for that subagent. Catastrophic work still stops for the
  operator. Use `on-request` and `approvals_reviewer = "user"` only on a
  consult/no-edit lane when policy requires a human decision.

`docs/product.md` always describes the consuming project's purpose, users,
scope, and non-goals—not the CodeFlow CLI. `docs/architecture.md` describes how
that project is built. Common project facts, commands, and constraints belong in
`AGENTS.md`; `CLAUDE.md` carries only Claude-specific differences. CodeFlow does
not introduce a competing `project.md` or `projects.md`.

## The daily flow

1. **Orient and route.** The SessionStart digest (or `codeflow orient`) gives branch and
   worktree state, work counts, recent ADR titles, gate status, and pointers —
   read the pointed docs, not the digest, for depth. Begin every non-trivial
   repository task with `/cf-model-orchestrator`; it uses only the research,
   planning, implementation, or review stages the requested outcome needs and
   degrades visibly if a native peer seat is unavailable.
2. **Branch in a worktree.** Work on a `{prefix}/{kebab-name}` branch in a
   worktree; never develop on the root protected-branch checkout.
3. **Gates as you go.** pre-commit (secret scan), commit-msg (conventional
   format with the restored 50-char description / 72-char subject budget and the
   bullet-only body shape — ADR-0020 — plus no AI attribution and no emoji),
   pre-merge-commit and reference-transaction
   (protected-branch merge/ref rules — the latter also catches fast-forward
   merges, `reset --hard`, and `branch -D`), pre-push (branch naming,
   protected-branch rules, test gate). Keep `codeflow test` and `codeflow
   validate --docs` green before push.
4. **Land by PR, merged by a human.** Push the branch, open a PR from the
   template (summary, changes, testing, reviews, release impact); a
   human merges it when required checks are evidenced green (an infra-killed
   duplicate CI job is not a failed check; an agent-performed `gh pr merge`
   into a protected base is blocked — that is the boundary). With no remote, `codeflow
   integrate <branch> --into <target>` is the sanctioned local path, and a human
   can override the git layer for a local merge with `CODEFLOW_HUMAN_OVERRIDE=1`.

### A body of work — the integration branch

The loop above lands one standalone task per PR onto `main`. When the work is a
multi-task epic — several tasks, some serial, some parallel — its default
landing shape is one shared **integration branch**. Do not casually turn the
epic into a series of task-to-`main` PRs: a different shape needs a recorded
Plan vN rationale and approval from both primary model seats before allocation.
The normal path is:

- `integration/<epic>` is branched off `main` and is **non-protected**, so
  agents merge tasks into it — by `codeflow integrate <task> --into
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

`main` stays human-merge-only throughout — the integration branch is never a
backdoor to it. See cf-method's "Managing a body of work" for the full procedure.

## Task graphs and verification strength

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

## Model and harness upgrades

CodeFlow separates stable method from changing bindings (ADR-0039):

```text
durable doctrine
  -> universal harness capability contract
    -> approved concrete binding evidence
      -> managed current ensemble
        -> project selection by qualified binding ID
```

The orchestrator and quality/routing resources own duties that should survive
model releases. `harnesses.json` owns the minimum guarantees and evidence for
each capability-supported native harness; that catalog status does not qualify
a model. A promoted local record binds one actual model, effort, harness,
settings digest, and approved full result.
`current-ensemble.json` selects the managed primaries, effort defaults, worker
classes, and escalation triggers. Standard/full projects also own
`.codeflow/model-selection.json`. Leave it absent or empty to use those
defaults; an override maps only a stable role to a promoted local binding ID.
Run `codeflow doctor --check model-bindings` before using an override. The
entire selection fails closed rather than partly applying when a record is
missing, ineligible, unsupported, drifted, or would collapse the two primary
lineages.

For a model upgrade on a capability-supported harness, evaluate the new
concrete binding and update the ensemble record; do not rewrite the doctrine. A
new harness additionally needs evidence for every universal capability and
only the transport-specific code or instructions its observed behavior
requires. It becomes eligible for a concrete binding only after the full
native evaluation and approval. Removing a harness retires its
catalog/ensemble entry while keeping graceful degradation. None of these paths
adds automatic discovery, promotion, routing, or vendor-internal worker
tracking.

The maintenance boundary is deliberate:

| Change | Update | Re-prove | Do not add |
|---|---|---|---|
| New managed model/version or effort policy on a supported harness | Promoted binding evidence, then `current-ensemble.json` | Controlled full native evaluation with requested/observed identity | Doctrine rewrites or automatic routing |
| Consuming-project model choice | `.codeflow/model-selection.json` references an approved local binding ID for an exact stable role | `codeflow doctor --check model-bindings` plus normal duo canary | Raw selectors, partial fallback, or same-lineage pseudo-duos |
| Material harness release/configuration change | Capability evidence only if the contract changed; refresh the concrete binding | Native capability canary plus full binding qualification where behavior or settings changed | An inferred pass from `--version` alone |
| Genuinely new harness/provider | One catalog entry and the smallest reviewed transport/probe seam actually required | Every universal capability, then each production binding | Generic plugin machinery, arbitrary catalog commands, or speculative providers |
| Durable orchestration duty change | Doctrine/quality/routing contract and linked requirement/cases | Regression and over-trigger cases plus both primary judgments | Binding facts duplicated through many skills |
| Harness/model retirement | Remove its ensemble use and catalog support when no retained binding needs it | Graceful-degradation and remaining-ensemble canaries | Harness-internal worker tracking |
| Diagnostic case grouping | `packs.json` only | Pack resolution and underlying unchanged cases | A promotion shortcut |

This division keeps model-family upgrades localized while making a new harness
earn the guarantees CodeFlow depends on. The evaluator and human approval
promote evidence; neither the catalog, doctor, nor current ensemble promotes
anything automatically.

Do not promote a new production model, harness release, permission profile, or
material instruction rewrite from a single successful task. Run
`/cf-evaluate-model` from the orchestrated maintenance flow: validate the
requirement/case traceability, materialize fresh disposable fixtures, exercise
the candidate through its native interactive harness with the real tools/MCPs,
run the full three-trial suite, independently grade retained evidence, and
compare it with the pinned baseline. A hard semantic regression blocks even
when the candidate is faster or uses fewer tokens. Preserve the result and
review evidence. An approved full result may produce a compact non-secret
record under `~/.codeflow/qualified-bindings/`; `codeflow doctor --check
model-bindings` reports requested/observed contradictions and observable
harness/settings drift without inferring live model state. Diagnostic packs
help isolate failures but never qualify a binding. Then use the skill's
marker+run-ID-gated cleanup for fixtures;
never use it against the consuming project itself (ADR-0027).

## Selecting deterministic code analysis

CodeFlow does not impose one SAST service on every stack. During
`/cf-customize`, `cf-stack` inventories languages, trust boundaries, hosting,
existing tools, and CI constraints, then records the smallest maintained lane
that provides relevant source/data-flow or taint evidence. CodeQL default setup
is a low-maintenance choice for an eligible GitHub-hosted repository and a
supported language; Semgrep, Sonar, or a language-native analyzer may fit other
stacks or governance requirements. SCA and secret scanning remain separate
evidence and do not substitute for SAST.

Verify the chosen analyzer rather than merely installing it: record applicable
rules, scanned-file/tool status, extraction errors, suppressions, owner,
cadence, and whether it gates locally, in CI, or through branch protection. If
no relevant lane is available, record the residual risk and disposition.
CodeFlow's own post-public CodeQL setting is repository-specific and is not
copied into consuming projects by `init` or `update`.

## Enforcement planes — who catches what

One policy (`.codeflow/policy.json`), four complementary planes. Minimal installs
the local floor and CI scaffold, not remote branch protection (ADR-0019).
Git hooks are harness-agnostic (any agent or human — including Codex); the
in-session PreToolUse guards (`git-guard` + `exec-guard`) are a fast bonus for
Claude and, through a byte-compatible payload, an **interactive** Codex session
(ADR-0008). Other harnesses need their qualified event contract. CI re-runs the
checks; configured remote rules can require their results. The matrix describes
available coverage, not proof that every plane is active. Verify hook execution,
harness trust, policy severity, CI results, and actual remote rules and bypass
permissions. Local checks are required feedback, but remain editable.

| Protection | git hooks | in-session guard | CI | remote |
|---|---|---|---|---|
| Commit / non-ff merge commit on protected | pre-commit / pre-merge-commit | git-guard | yes | yes |
| FF-merge, `reset --hard`, `branch -D` on protected | reference-transaction | git-guard | — | yes (result unpushable) |
| Push / force-push / delete to protected | pre-push | git-guard | — | yes |
| `gh pr merge` into a protected base | — (hooks can't see a PR) | git-guard | — | yes |
| Destructive command (`rm -rf /`, `mkfs`, fork bomb) | — | exec-guard (block) | — | — |
| Privilege escalation (`sudo`, `LD_PRELOAD`) | no | preset deny rules, exec-guard (block) | no | no |
| Commit format, no-attribution, no-emoji, secrets | commit-msg / pre-commit | partial | yes | — |
| Override-token laundering, `--no-verify` bypass | — | git-guard (structural) | — | — |

Two facts the matrix encodes. **PR-content checks are git-guard/CI by design** —
a git hook never sees `gh pr create`/`gh pr merge`, so attribution/emoji scans
and the protected-base check live in the Claude layer and CI, not the hooks.
**The human override (`CODEFLOW_HUMAN_OVERRIDE=1`) and the integrate token apply
to the git-hook plane only** — the git-guard never trusts them, because an agent
in a session cannot prove it is a human. `reference-transaction` needs git ≥
2.28; on older git it is absent and protection falls back to the other planes.
With active protection, an unreadable or unevaluable prepared transaction
blocks rather than silently skipping the check. The current input contract is
UTF-8: non-UTF-8 input also blocks, including remote-only transactions; valid
UTF-8 remote-only input retains its fast path. Inspect the reported cause and
tool/backend compatibility; do not disable the guard or automatically convert
repository storage to evade it.

### How far the discipline reaches across harnesses

codeflow has two kinds of thing: **enforcement** (gates that block) and
**guidance** (instructions and skills that inform). They reach different
distances, so be precise about what a given harness actually gets:

- **Git hooks and CI are harness-neutral.** The git
  client hooks and CI are harness-agnostic: they act on git operations and PRs,
  not on which tool produced them. So conventional-commit format, the secret
  scan, no-AI-attribution, branch/push/protected-merge rules, and the test gate
  run for Claude Code, Codex, a future CLI, or a human when the relevant hooks
  and CI execute. Installed files do not make these checks unbypassable.
- **In-session guards require qualified harness integration.** The PreToolUse
  `git-guard`/`exec-guard` add fast, pre-git feedback. They are wired for Claude
  (`.claude/settings.json`) and, via a byte-compatible payload, an **interactive**
  Codex session (`.codex/hooks.json`, ADR-0008); the Grok adapter has its own
  qualified event contract. Historical headless hook gaps motivated ADR-0018,
  but are not a universal claim about every current harness. The
  consult/delegate/duo flows remain interactive-only independently of whether
  a headless mode can run hooks.
- **Guidance (AGENTS.md + the `cf-*` skills) is Claude + Codex.** Both read the
  repo `AGENTS.md` operating contract; the skills ship to `.claude/skills/`
  (Claude) and `.agents/skills/` (Codex). The **workflow** runtime
  (`pipeline.workflow.js`) is Claude-Code-only.
- **A harness codeflow does not specifically integrate** (for example Google's
  Antigravity `agy`) can still receive the git-hook plane + CI because those
  are harness-agnostic — but does **not** receive the in-session guards, the
  skills, or (verified on `agy` 1.0.15) the `AGENTS.md` instructions. Its reliable
  coverage is the configured, verified hook/CI plane, not assumed guidance.

The one-line version: CodeFlow shares policy across harness-neutral checks;
native guidance and in-session guards depend on the installed integration.
Missing planes are disclosed without relaxing safety or review duties.

**Verified against:** codex-cli 0.144.3 and Claude Code 2.1.220 on 2026-08-02
(with earlier hook-specific evidence retained by ADR-0008, ADR-0013, and
ADR-0014). These surfaces (hook payload contracts, config schemas) move fast on
both sides; the release checklist
([docs/releasing.md](releasing.md)) re-verifies them before each codeflow tag.

## Delegation quickstart

Standard/full projects use the duo for routed work; standalone consults
are available when an outside opinion is useful. Minimal does not install the
method. Transport remains interactive-only, with preferred lanes and qualified
native fallback (ADR-0059). One-time setup: authenticate Codex manually, enable
`codex@openai-codex` in Claude Code, and install the Claude CLI with Herdr
(tmux as the documented degraded host) for the reverse lane. Codeflow never automates auth.
`codeflow doctor` reports inspectable prerequisites; retain a scoped
interactive canary in each direction.

- `/cf-consult` gets an independent, read-only second opinion from the vendor
  the session is *not* — from Claude Code through the official
  `codex-plugin-cc` plugin (`/codex:review`); from Codex by driving the
  interactive `claude` CLI via Herdr (tmux degraded) with Stop/StopFailure
  hook completion—and makes you synthesize it against your
  own analysis (never paste its reply as fact). Headless `codex exec` /
  `claude -p` are not sanctioned delegation transports (ADR-0023).
- A full edit handoff (`cf-delegate`; from Claude Code, `/codex:rescue`) runs
  only inside a worktree on a feature branch, where the delegate's commits pass
  the same gates and independent review as yours — enforcement is
  author-agnostic.
- An incompatible plugin permits the qualified official native client fallback
  in `cf-delegate`; verify its tools, boundaries and recheckable native result.
  If no qualified peer route remains, degrade legibly and say so.

### Codex parity

When a repo is driven through OpenAI's Codex CLI instead of Claude, protection
comes from two layers, and it helps to be precise about which does what.

- **The git-hook plane is harness-agnostic when installed and executed.**
  A Codex `git push --force origin main` against protected `main` is refused by
  the `pre-push` shim (`codeflow pre-push: BLOCKED — policy rule
  git.push_to_protected`) exactly as any agent's would be. Verified live on
  codex-cli 0.142.5. This needs no Codex configuration, but local hook files and
  Git configuration remain editable; it is not an unbypassable boundary.
  An active prepared-transaction check that cannot read or evaluate its input
  blocks the transaction. Inspect the reported cause and repository/toolchain
  compatibility before retrying; preserve work and repair the supported path,
  rather than disabling the hook or automatically converting repository storage.
- **The in-session PreToolUse guards are an interactive-Codex bonus.** The
  scaffold ships a `.codex/` starter (part of the enforcement floor, from
  `--minimal` up): `hooks.json`
  wires `codeflow hook git-guard` and `codeflow hook exec-guard` onto Codex's
  `PreToolUse` (Bash) event, and `config.toml` enables the hooks engine with a
  guarded workspace permission profile, broad public network, live search,
  `approval_policy = "never"`, production `--sandbox danger-full-access`, and
  `approvals_reviewer = "auto_review"` (vestigial under `never`: it is not a
  human gate and does not fire on-request prompts). Catastrophic work still
  stops for the operator. Git-guard, exec-guard, git hooks, and CI remain the
  floor.
  The config intentionally contains no legacy `sandbox_mode`, because that
  would shadow the named profile. Codex's hook payload is byte-compatible with
  Claude's, so the same binaries run unchanged.

One-time setup for the in-session guards: Codex loads a project's `.codex/hooks.json`
only when that project's `.codex/` layer is trusted. Run `/hooks` inside an
interactive `codex` session once to trust the CodeFlow hooks. **Note:** in testing
on codex-cli 0.142.5, headless `codex exec` did not run project PreToolUse hooks
even with `--dangerously-bypass-hook-trust` and the layer trusted — so treat the
in-session guards as an interactive-session safeguard, and rely on the git-hook
plane where Git invokes the installed hooks. codeflow's own flows no
longer produce headless runs: ADR-0018 makes cross-model transport
interactive-only (consult/delegate/duo never shell out to `codex exec`), so a
headless Codex run is outside those flows. Do not generalize that historical
Codex observation to every harness: Claude's
[programmatic-mode documentation](https://code.claude.com/docs/en/headless)
states that ordinary noninteractive sessions load project hooks, while bare
mode skips their automatic discovery. This does not authorize headless
CodeFlow work. In every mode, verify actual local hook execution and any
required CI/remote rules rather than inferring protection from installed files.

A Codex-primary session can host the full duo, not only a consult. The host
owns orchestration and routes production by the qualified capability binding,
risk, evidence needs, and available capacity; no vendor receives implementation
work merely because of its name. From a Claude Code host, the official Codex
plugin provides the independent Codex lane. Both seats independently research
and plan before approving the same versioned contract; material design and
implementation receive cross-lineage review (ADR-0023, ADR-0046).

Google's Antigravity `agy` is **not** bound automatically (its hook dialect
differs and its macOS reliability is unresolved); the cf-delegate skill carries
an experimental, manual opt-in snippet for those who want it. As a *delegate*,
`agy` is retired: its only documented drive shape is headless one-shot, which
ADR-0018 prohibits.
