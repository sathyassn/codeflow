# codeflow

The AI-development discipline layer you install into any repo: one Rust binary
(`codeflow`) that scaffolds, enforces, verifies, and remembers — while Claude
Code (or any harness) does the developing. Policy lives in one config
(`.codeflow/policy.json`) and is enforced across four planes, so the same rules
bind any agent or human.

## Install

This repo is **private**, so the cargo-dist `curl | sh` shell installer does
**not** work (GitHub returns 404 for private release assets even with a token).
Install one of two authenticated ways:

```sh
# from a checkout (needs a Rust toolchain):
cargo install --path crates/codeflow-cli

# prebuilt binary, another machine (needs `gh auth login` with read access):
A=codeflow-cli-aarch64-apple-darwin   # or x86_64-apple-darwin / x86_64-unknown-linux-gnu
gh release download v2.0.0 -R sathyassn/codeflow -p "$A.tar.xz" -D /tmp/cf --clobber
tar -xf "/tmp/cf/$A.tar.xz" -C /tmp/cf
mkdir -p ~/.cargo/bin && install "/tmp/cf/$A/codeflow" ~/.cargo/bin/   # or any dir on PATH
```

If the repo is later made **public**, the standard installer works anonymously:
`curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh`.
See [docs/adoption.md](docs/adoption.md) for the full install and adoption guide.

## Quickstart

```sh
cd your-project
codeflow init --standard --yes   # scaffold; offline; sane defaults
```

`init` is idempotent and non-destructive: it writes managed scaffold files,
wires git hooks via `core.hooksPath`, and (in a fresh repo) makes the scaffold
commit and arms branch policy. See [docs/adoption.md](docs/adoption.md) for the
greenfield/brownfield paths, tiers, ownership model, and the daily flow.

## Commands

| Command | What it does |
|---|---|
| `init` | Scaffold this project (idempotent, non-destructive, offline) |
| `update` | Refresh managed scaffold files (3-way merge; never clobbers) |
| `hook` | Claude-layer hooks (`git-guard`, `session-orient`, `session-summary`) |
| `git-hook` | Git client hook target the `.git/hooks` shims exec |
| `orient` | Print the session-start digest (pointers, not content) |
| `test` | Run the test gate (configured targets or runtime stack detection) |
| `validate` | Validate record frontmatter; `--docs` adds the doc-graph lint |
| `status` | Generated view: branch, worktrees, in-flight work, capabilities |
| `integrate` | Land a branch into a target: flock(rebase → test → ff-merge) |
| `doctor` | Health checks (7): hooks, claude, config, permissions, network, delegates, repo-integrity |
| `recall` | Search project memory: ledger, session summaries, ADRs, epics, capabilities |
| `remote` | Remote provider operations (branch protection) |

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
  (headless `codex exec` 0.142.5 does not run project PreToolUse hooks).
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
- [docs/decisions/](docs/decisions/) — ADRs (the record of why)
