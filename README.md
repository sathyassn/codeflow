# codeflow

The AI-development discipline layer you install into any repo: one Rust binary
(`codeflow`) that scaffolds, enforces, verifies, and remembers — while Claude
Code (or any harness) does the developing. Policy lives in one config
(`.codeflow/policy.json`) and is enforced across four planes, so the same rules
bind any agent or human.

## Why codeflow

- **Vs. a bare coding harness** (Claude Code or Codex alone) — discipline lasts
  only as long as you remember to apply it, and none of it survives the
  session: no durable decision record, no cross-session traceability, no
  enforcement once the conversation ends. codeflow adds a capability → epic →
  ADR spine (linted by `codeflow validate --docs`), four enforcement planes
  reading one `.codeflow/policy.json`, and an automatically captured recall
  corpus (ledger + session summaries).
- **Vs. spec-driven frameworks** (Spec Kit, OpenSpec, BMAD) — those prescribe a
  per-change authoring ceremony: a spec, plan, and task breakdown for each unit
  of work. codeflow is a discipline layer instead, with a graduation ladder
  (trivial or conversational work needs no artifact at all) and a **living**
  `docs/capabilities.md` registry of what the system does, not a disposable
  per-change spec.
- **When not to use it** — a scratch or throwaway repo (`--minimal`, or skip
  it), or a team that wants a full workflow framework rather than guardrails;
  codeflow is deliberately not a harness, agent framework, or runtime model
  router. Its standard/full scaffold does provide a portable Claude+Codex
  orchestration skill while each model remains in its native harness.

## Install

Prebuilt binary (macOS arm64/x64, Linux x64, Windows x64) — the shell or
PowerShell installer from the
latest release (works once codeflow's releases are public; for private/early
access use the checkout build below or `gh release download`):

```sh
curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
```

```powershell
irm https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.ps1 | iex
```

Or from a checkout, with a Rust toolchain:

```sh
cargo install --path crates/codeflow-cli
```

On native Windows, use the PowerShell installer from the release or build with
Cargo. Git for Windows is required. WSL2 uses the Linux installer and is the
preferred Windows route for Linux-native tooling or Claude sandboxing. See the
platform-assurance section in the adoption guide before high-blast-radius work.

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
| `hook` | Claude-layer hooks (`git-guard`, `exec-guard`, `session-orient`, `session-summary`) |
| `git-hook` | Git client hook target the `.codeflow/git-hooks` shims exec (wired via `core.hooksPath`) |
| `orient` | Print the session-start digest (pointers, not content) |
| `test` | Run the test gate; `test setup` safely detects root stacks, lists/applies embedded templates, or appends explicit targets |
| `validate` | Validate `.codeflow/policy.json` (loudly) + record frontmatter; `--docs` adds the doc-graph lint |
| `ci` | Portable, binary-sourced CI check: verify a commit range + branch name against policy (auto-detects the platform's range); exit 2 on a violation or invalid policy |
| `status` | Generated view: branch, worktrees, in-flight work, capabilities; `--delivery` shows the capability-delivery rollup |
| `integrate` | Land a branch into a target: flock(rebase → test → ff-merge) |
| `doctor` | Health checks (12): hooks, claude, codex, config, permissions, network, delegates, repo-integrity, ci-perimeter, managed-drift, customization, test-config |
| `policy` | Inspect `.codeflow/policy.json`: `explain` renders every key's type, default, and valid values from the binary; `show` prints the effective values, their source, and flags invalid ones |
| `recall` | Search project memory: ledger, session summaries, ADRs, epics, capabilities |
| `remote` | Remote provider operations (branch protection) |
| `epic new` | Allocate the next `EPC-NNN` and scaffold the epic from the template |
| `task new` | Allocate the next `TSK-NNN-MMM` under an epic and scaffold it |

`codeflow test setup` with no options detects only stack markers at the project
root and fills an absent or empty config; it never replaces a populated or
malformed config. Use `--list-templates`, `--template <name>`, or `--add-target` for
explicit setup. Template replacement requires the deliberate
`--template <name> --replace` combination. Monorepos should apply
`monorepo-multi-target.json` or append one target per package with its `cwd`;
auto-detection does not recursively guess package boundaries or commands.

## Enforcement planes

One policy, four planes (charter §6.5; [ADR-0007](docs/decisions/ADR-0007-agent-human-merge-boundary.md), [ADR-0008](docs/decisions/ADR-0008-harness-parity-and-exec-guard.md)):

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
  `.codex/hooks.json` (ADR-0008). Codex-driven work is bound unconditionally by
  the git-hook plane above; the in-session guards are an interactive-Codex bonus
  (headless `codex exec` is not a sanctioned peer transport). The scaffolded
  Claude and Codex settings also enable fail-closed workspace autonomy, public
  research/tool access, live search, and guarded escalation; see ADR-0025.
- **CI** — re-runs the gates as the authoritative perimeter; PR-content checks
  are CI-plane by design (a git hook never sees a PR).
- **Remote branch protection** — the server-side backstop (`codeflow remote
  protect`).

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
