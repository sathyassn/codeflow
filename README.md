# codeflow

The AI-development discipline layer you install into any repo: one Rust binary
(`codeflow`) that scaffolds, enforces, verifies, and remembers — while Claude
Code (or any harness) does the developing. Policy lives in one config
(`.codeflow/policy.json`) and can reach four complementary protection planes.
Installed files alone do not make those planes effective; verify the relevant
hooks, harness integration, CI requirements, and remote rules.

## Why codeflow

- **Integrated, proportional discipline** — codeflow connects a living
  `docs/capabilities.md` registry, architecture and decision records, shared
  policy, staged workflows, and project recall. Full tier adds durable epics,
  tasks, and specs plus referential checks for work that outlives sessions;
  accepted external trackers and approved plans keep their authority. Trivial
  or conversational work needs no new artifact, and non-trivial work uses only
  the stages its outcome warrants.
- **Durable, conditional recall** — the ledger and repository records provide
  cross-session traceability. Session summaries are captured automatically only
  when a supported harness's SessionEnd hook is installed and actually executes;
  without that event, externalize decisions, progress, and evidence as you go.
- **When not to use it** — a scratch or throwaway repo (`--minimal`, or skip
  it), or a team that wants a full workflow framework rather than guardrails;
  codeflow is deliberately not a harness, agent framework, or runtime model
  router. Its standard/full scaffold does provide a portable Claude+Codex
  orchestration skill while each model remains in its native harness.

## Install

From 3.0.0 on, each release publishes archives for macOS (arm64 and x64),
Linux x64 and Windows x64, each with a `.sha256` file, plus a shell installer
and a PowerShell installer. Install the latest release on macOS or Linux:

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

WSL2 uses the Linux installer and is the preferred Windows route for
Linux-native tooling or Claude sandboxing. See the platform-assurance section
in the adoption guide before high-blast-radius work.

See [docs/adoption.md](docs/adoption.md) for the full install and adoption guide.

## Quickstart

```sh
cd your-project
codeflow init --standard --yes   # scaffold; offline; sane defaults
```

`init` is idempotent and non-destructive: it writes managed scaffold files,
wires git hooks via `core.hooksPath`, and (in a fresh repo) makes the scaffold
commit and arms branch policy. Standard/full init then points to
`/cf-customize`, which reconciles the consuming project's product,
architecture, commands, harness settings, and required tools before the first
non-trivial task enters `/cf-model-orchestrator`. See
[docs/adoption.md](docs/adoption.md) for the greenfield/brownfield paths, tiers,
ownership model, autonomy posture, and daily flow.

When a production model, harness, permission profile, or material instruction
changes, `/cf-evaluate-model` provides a separate native-interactive
qualification flow over reproducible disposable fixtures. It is not part of
ordinary task execution and adds no model-running CLI command.

## Commands

| Command | What it does |
|---|---|
| `init` | Scaffold this project (idempotent, non-destructive, offline) |
| `update` | Refresh managed scaffold files (3-way merge; never clobbers) |
| `portal setup --path <dir>` | Adopt or reconcile the offline documentation-portal starter (same utility craft as present; durable docs; no Comment) |
| `portal transfer --confirm` | Take responsibility for the adopted runtime while preserving edits and intentional deletions |
| `present` | Open, revise, review, export, close, and clear a this-session catalog review (runtime owns chrome and Comment; not a docs portal) |
| `hook` | Claude-layer hooks (`git-guard`, `exec-guard`, `session-orient`, `session-summary`, `delegate-turn`) |
| `git-hook` | Git client hook target the `.codeflow/git-hooks` shims exec (wired via `core.hooksPath`) |
| `orient` | Print the session-start digest (pointers, not content) |
| `test` | Run the test gate; `test setup` safely detects root stacks, lists/applies embedded templates, or appends explicit targets |
| `validate` | Validate policy + records; `--docs` adds doc-graph integrity and `--portal <dir>` verifies portal evidence without running project code |
| `ci` | Portable, binary-sourced CI check: verify a commit range + branch name against policy (auto-detects the platform's range); exit 2 on a violation or invalid policy |
| `status` | Generated view: branch, worktrees, in-flight work, capabilities; `--delivery` shows the capability-delivery rollup |
| `integrate` | Land a branch into a target: flock(rebase → test → ff-merge) |
| `doctor` | Health checks (16): hooks, claude, codex, grok, config, permissions, network, delegates, qualified model bindings, delegate-roundtrip, repo-integrity, ci-perimeter, managed-drift, customization, test-config, id-registry |
| `policy` | Inspect `.codeflow/policy.json`: `explain` renders every key's type, default, and valid values from the binary; `show` prints the effective values, their source, and flags invalid ones |
| `recall` | Search project memory: ledger, session summaries, ADRs, epics/tasks/specs, capabilities |
| `remote` | Remote provider operations (branch protection) |
| `epic new` | Allocate the next `EPC-NNN` and scaffold the epic from the template |
| `spec new --for <id>` | Allocate the next `SPC-NNN`, scaffold it, and link it from an epic or task |
| `task new` | Allocate the next independent `TSK-NNN` under an epic or with an explicit standalone rationale |
| `work start <task-id>` | Read-only proof that durable planning is anchored and its parent/spec/dependency graph is ready |
| `estimate check <forecast.json>` | Read-only check of a project-owned forecast's explicit allocations and pinned evidence; schedules nothing, writes nothing, and makes no estimate itself; `--json` emits the versioned report (ADR-0057) |
| `delegate` | Durable delegate-turn lifecycle: `init` an owner-only run, `arm` one prompt, `wait` for ready/accepted/terminal — the host launches the harness and delivers the prompt (ADR-0036) |

`codeflow test setup` with no options detects only stack markers at the project
root and fills an absent or empty config; it never replaces a populated or
malformed config. Use `--list-templates`, `--template <name>`, or `--add-target` for
explicit setup. Template replacement requires the deliberate
`--template <name> --replace` combination. Monorepos should apply
`monorepo-multi-target.json` or append one target per package with its `cwd`;
auto-detection does not recursively guess package boundaries or commands.

## Enforcement planes

One policy, four available planes (charter §6.5;
[ADR-0007](docs/decisions/ADR-0007-agent-human-merge-boundary.md),
[ADR-0008](docs/decisions/ADR-0008-harness-parity-and-exec-guard.md)). Minimal
init scaffolds local hooks, in-session settings, and CI while preserving an
existing hook manager; it does not configure remote branch protection. Verify
hook execution, harness trust and event support, required CI results, and actual
remote rules, permissions, and bypasses before claiming effective coverage:

- **Git client hooks** — harness-agnostic, five shims: `pre-commit` (secret
  scan, protected-branch commit), `commit-msg` (conventional format, no AI
  attribution, no emoji), `pre-merge-commit` (non-fast-forward merge commits
  onto protected), `reference-transaction` (the backstop for fast-forward
  merges, `reset --hard`, and `branch -D` on protected; git ≥ 2.28), and
  `pre-push` (branch naming, protected push/force/delete, test gate).
- **In-session PreToolUse guards** — `git-guard` (git policy, plus the checks
  git hooks cannot see: `gh pr merge` into a protected base, AI attribution /
  emoji in `gh pr create` bodies) and `exec-guard` (destructive commands block,
  privilege escalation warns). Wired for Claude via `.claude/settings.json` and,
  through a byte-compatible payload, for an interactive Codex session via
  `.codex/hooks.json` (ADR-0008). Codex-driven work receives the git-hook plane
  where those hooks are installed and executed; the in-session guards are an
  interactive-Codex bonus (headless `codex exec` is not a sanctioned peer
  transport). The scaffolded
  Claude and Codex settings also enable fail-closed workspace autonomy, public
  research/tool access, live search, and guarded escalation; see ADR-0025.
- **CI** — re-runs the gates; PR-content checks are CI-plane by design (a git
  hook never sees a PR). CI becomes a merge gate when the remote requires its
  result.
- **Remote branch protection** — the configured server-side backstop. Where the
  provider and permissions support it, `codeflow remote protect` applies the
  supported rules; inspect the actual rules and bypass access.

Protected-branch merges land via a PR **merged by a human**, or `codeflow
integrate`; an agent never merges into protected. A human can override the git
layer for a local merge with `CODEFLOW_HUMAN_OVERRIDE=1` — an env the git-guard
never honors and blocks agents from setting in-session.

## Docs

- [docs/adoption.md](docs/adoption.md) — tiers, install, ownership, the daily flow, the enforcement matrix
- [docs/architecture.md](docs/architecture.md) — how the binary and scaffold are built
- [docs/product.md](docs/product.md) — what codeflow is for and its non-goals
- [docs/release-checklist.md](docs/release-checklist.md) — evidence required for every release
- [docs/decisions/](docs/decisions/) — ADRs (the record of why)

### Repository guide

The same documents can be read as a layered, searchable guide with source
links, altitude tabs, and Markdown twins. It is generated locally from
`docs-portal/` and is not a hosted website: GitHub renders the Markdown above,
and nothing publishes the generated HTML. From a clean committed checkout, with
the Node version pinned in `docs-portal/.node-version` (24.18.0):

```sh
cd docs-portal
npm run deps:install   # locked install, dependency scripts off
npm run build          # derive pages and evidence, build site
npm run preview        # serve the built site on loopback
```

The guide names the exact commit it was built from; it shows a release
version only when a verified published release exists for that commit. See
[reading the CodeFlow guide locally](docs/adoption.md#reading-the-codeflow-guide-locally)
for the check, validate, preview, and cleanup details.

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). In short:
format, tests, clippy, and rustdoc green; conventional commits; no AI
attribution (codeflow's own hooks enforce it). Please read
[SECURITY.md](SECURITY.md) before reporting a vulnerability, and be mindful of the
[Code of Conduct](CODE_OF_CONDUCT.md).

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution
intentionally submitted for inclusion in the work by you, as defined in the
Apache-2.0 license, shall be dual licensed as above, without any additional
terms or conditions.
