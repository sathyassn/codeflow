# CodeFlow

CodeFlow adds guardrails to a git repository where AI coding agents work. It
blocks the mistakes that are hard to undo, in git, in the agent's session and
in CI, and it keeps a record of why work was done. It is one Rust binary,
`codeflow`. The agent you already use, such as Claude Code, Codex or Grok
Build, still does the developing.

Read [the product page](docs/product.md) for who CodeFlow is for and what it is
not.

## The problem it solves

An agent works fast, and some of its mistakes cannot be undone:

- a commit or force push to the main branch
- a secret committed to history
- a commit message that credits the AI tool
- a destructive shell command
- work that nobody can explain a month later

A prompt that asks an agent to behave does not stop any of these. CodeFlow
turns the rules into checks, and the repository holds the record. Session
summaries are captured automatically only when a supported harness's
SessionEnd hook is installed and actually executes; otherwise write decisions,
progress and evidence down as you go. Every rule is written once, in
`.codeflow/policy.json`, and four planes read it:

```text
  .codeflow/policy.json   one place where every rule is written
        |
        +--> 1 git hooks            run when git runs
        +--> 2 in-session guards    run before an agent's commands and edits
        +--> 3 scaffolded CI        runs `codeflow ci` on the server
        +--> 4 remote protection    the host's branch rules protect main
```

Planes 1 and 2 give fast local feedback and can be edited. Planes 3 and 4 are
the perimeter, where CI runs and the remote requires it. CI becomes a merge
gate when the remote requires its result.

Installed files alone do not make those planes effective. Verify hook
execution, harness trust and event support, required CI results, and actual
remote rules, permissions, and bypasses. Codex-driven work receives the
git-hook plane where those hooks are installed and executed. See
[the product page](docs/product.md#the-four-enforcement-planes) and
[the enforcement planes](docs/architecture/enforcement-planes.md).

## Choose a tier

`codeflow init` installs one of three tiers. Each tier includes everything in
the tier listed before it, and all three install the same enforcement floor.
A project can start small and move up later, and a lower-tier request never
removes files.

| Tier | Installs | Pick it when |
|---|---|---|
| `--minimal` | The enforcement floor: five git hooks, the CI check, the in-session guards, an armed `policy.json`, and a lean `AGENTS.md` and `CLAUDE.md` | The repository is a doc set, a config repo or a small tool, or you want only the guardrails |
| `--standard` (default) | Minimal plus the develop-loop skills, reviewer agents, the pipeline workflow and the `docs/` spine (product, architecture, capabilities, decisions) | The repository is a code project |
| `--full` | Standard plus `project-management/` templates for epics, tasks and specs | Work outlives a session and needs durable plans and criteria |

Full tier adds durable epics, tasks, and specs plus referential checks for
work that outlives sessions. Trivial or conversational work needs no new
artifact.

Minimal init scaffolds local hooks, in-session settings, and CI while
preserving an existing hook manager; it does not configure remote branch
protection, and no tier does. See [docs/adoption.md](docs/adoption.md) for the
tiers, ownership and the daily flow.

## Install

From 3.1.0 on, each release publishes archives for macOS (arm64 and x64), Linux
x64 and Windows x64, each with a `.sha256` file, plus a shell installer and a
PowerShell installer. 3.0.0 published macOS and Linux only.

Install the latest release on macOS or Linux:

```sh
curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
```

On native Windows, once 3.1.0 is the latest release, run this in PowerShell (Git
for Windows is required):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.ps1 | iex"
```

To build from a checkout, with a Rust toolchain:

```sh
cargo install --path crates/codeflow-cli
```

WSL2 uses the Linux installer. It is the preferred Windows route for
Linux-native tooling or Claude sandboxing; see
[platform assurance](docs/harness-posture.md#platform-assurance) before
high-blast-radius work. The full install guide is in
[docs/adoption.md](docs/adoption.md).

## Quickstart

```sh
cd your-project
codeflow init --standard --yes   # scaffold; offline; sane defaults
```

`init` is idempotent and non-destructive. It writes the managed scaffold files
and wires the git hooks through `core.hooksPath`. In a fresh repository it also
makes the scaffold commit and arms branch policy.

The standard and full tiers then point to `/cf-customize`. That skill
reconciles the project's product, architecture, commands, harness settings and
required tools before the first non-trivial task goes to
`/cf-model-orchestrator`.

- [docs/adoption.md](docs/adoption.md) covers the greenfield and brownfield
  paths, the tiers, the ownership model and the daily flow.
- [docs/harness-posture.md](docs/harness-posture.md) covers the autonomy,
  sandbox and harness-settings posture.
- [docs/model-upgrades.md](docs/model-upgrades.md) covers qualifying a new
  production model, harness, permission profile or material instruction change
  with `/cf-evaluate-model`. That flow adds no model-running command.

## Commands

| Command | What it does |
|---|---|
| `init` | Scaffold this project (idempotent, non-destructive, offline) |
| `update` | Refresh managed scaffold files (3-way merge, never clobbers) |
| `orient` | Print the session-start digest (pointers, not content) |
| `status` | Show the branch, worktrees, in-flight work and capabilities; `--delivery` adds the capability-delivery rollup |
| `doctor` | Health checks (20): hooks, claude, codex, grok, config, permissions, policy-source, network, delegates, model-bindings, delegate-roundtrip, repo-integrity, ci-perimeter, managed-drift, customization, instructions, reading, test-config, id-registry, adopter-fit |
| `test` | Run the test gate; `test setup` detects root stacks, lists templates and appends targets |
| `validate` | Validate policy and records; `--docs` adds doc-graph checks and `--portal <dir>` verifies portal evidence without running project code |
| `ci` | Check a commit range and branch name against policy, the same check CI runs; exit 2 on a violation or invalid policy |
| `policy` | `explain` renders every policy key with its default and valid values; `show` prints the values in effect |
| `recall` | Search project memory: ledger, session summaries, ADRs, epics, tasks, specs and capabilities |
| `epic new`, `spec new`, `task new` | Allocate the next `EPC-NNN`, `SPC-NNN` or `TSK-NNN` and scaffold the record |
| `work start <task-id>` | Check, read-only, that planning is anchored and the task's graph is ready |
| `integrate` | Land a branch into a target: rebase, test, then fast-forward merge under a lock |
| `remote protect` | Apply protected-branch policy to the remote provider |
| `hook`, `git-hook` | Targets for the harness hooks and the `.codeflow/git-hooks` shims |
| `delegate` | Track one delegate turn handed to a peer model: `init`, `arm`, `wait`; the host launches the harness |
| `present` | Open, review, export and close a review of this session in a local browser window |
| `portal setup`, `portal transfer` | Adopt the offline docs-portal starter, or take ownership of the adopted runtime |
| `estimate check <forecast.json>` | Read-only check of a project-owned forecast; it makes no estimate (ADR-0057) |
| `models` | Resolve catalog duties without launching models |
| `ids`, `adr`, `report` | The shared ID registry, ADR allocation, and read-only process reports |

[docs/cli.md](docs/cli.md) documents every command and flag. `codeflow test
setup` with no options only fills an absent or empty config; for templates,
explicit targets and monorepos see
[configure test targets](docs/adoption.md#configure-test-targets).

## Who merges

Protected branches change through a pull request that a human merges, or
through `codeflow integrate`. An agent never merges into a protected branch.

A human can override the git-hook plane's protected-branch commit, merge and
push rules from their own terminal with `CODEFLOW_HUMAN_OVERRIDE=1`, for
example `CODEFLOW_HUMAN_OVERRIDE=1 git push -u origin main` for the first push
to an empty remote. Force pushes, deletions and the secret checks stay refused.
The in-session guard ignores the variable and blocks agents from setting it.

## Docs

The docs are listed in reading order, by what the reader needs next.

**Understand**

- [docs/product.md](docs/product.md): what CodeFlow is, who it is for and what it is not
- [docs/glossary.md](docs/glossary.md): the terms the docs use

**Start**

- [docs/adoption.md](docs/adoption.md): install, tiers, adoption, ownership and the daily flow
- [docs/workspace-mode.md](docs/workspace-mode.md): an umbrella repository that holds several projects

**Use**

- [docs/delivery.md](docs/delivery.md): how work moves from a request to `main`
- [docs/cli.md](docs/cli.md): every command and flag
- [docs/delegation.md](docs/delegation.md): handing a turn to a peer model
- [docs/present-guide.md](docs/present-guide.md): reviewing a document in a present session
- [docs/skills.md](docs/skills.md): the skills each tier installs

**Configure**

- [docs/policy-reference.md](docs/policy-reference.md): every policy key and its default
- [docs/harness-posture.md](docs/harness-posture.md): autonomy, sandbox and settings per harness
- [docs/model-upgrades.md](docs/model-upgrades.md): qualifying a new model, harness or permission profile

**Reference**

- [docs/capabilities.md](docs/capabilities.md): the registry of what the system does
- [docs/capabilities/CAP-010-duo-model-orchestration.md](docs/capabilities/CAP-010-duo-model-orchestration.md) and [docs/capabilities/CAP-015-opt-in-documentation-portal.md](docs/capabilities/CAP-015-opt-in-documentation-portal.md): two models working together, and the opt-in docs portal
- [docs/architecture.md](docs/architecture.md): how the binary and scaffold are built
- [docs/architecture/enforcement-planes.md](docs/architecture/enforcement-planes.md): what each plane catches
- [docs/architecture/present.md](docs/architecture/present.md) and [docs/architecture/utility-presentation.md](docs/architecture/utility-presentation.md): how review pages and figures are built
- [docs/decisions/](docs/decisions/) and [docs/decision-map.md](docs/decision-map.md): the decisions and why they were made

**Maintain**

- [docs/releasing.md](docs/releasing.md): the release runbook, and versioning in a project that uses CodeFlow
- [docs/release-checklist.md](docs/release-checklist.md): the evidence every release needs
- [docs/troubleshooting.md](docs/troubleshooting.md): fixing what `codeflow doctor` reports

### Repository guide

The same documents can be read as a layered, searchable guide with source
links, altitude tabs and Markdown twins. It is generated from `docs-portal/`
and lists the pages in the same order as above. A workflow builds it and
deploys it to GitHub Pages once the repository owner enables Pages for the
repository; the planned address, not live yet, is
`https://sathyassn.github.io/codeflow/`. To build it locally, run these from a
clean committed checkout with the Node version pinned in
`docs-portal/.node-version` (24.18.0):

```sh
cd docs-portal
npm run deps:install   # locked install, dependency scripts off
npm run build          # derive pages and evidence, build site
npm run preview        # serve the built site on loopback
```

The guide names the commit it was built from. It shows a release version only
when a verified published release exists for that commit. See
[this repository's own guide](docs/capabilities/CAP-015-opt-in-documentation-portal.md#this-repositorys-own-guide)
for the check, validate, preview and cleanup details.

## Contributing

Contributions are welcome; see [CONTRIBUTING.md](docs/CONTRIBUTING.md). In
short, format, tests, clippy and rustdoc stay green, commits are conventional,
and no commit carries AI attribution (CodeFlow's own hooks enforce this). Read
[SECURITY.md](docs/SECURITY.md) before reporting a vulnerability, and follow the
[Code of Conduct](docs/CODE_OF_CONDUCT.md).

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
