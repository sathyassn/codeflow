# CodeFlow

**The AI-development discipline layer you install into any repo.**

One Rust binary (`codeflow`) with an embedded scaffold: it scaffolds, enforces,
verifies, and remembers — while Claude Code (or any harness) does the
developing. It is not a harness, agent framework, or orchestrator; native
harness primitives (worktrees, subagents, workflows, memory) are consumed,
never reimplemented.

## Install

```sh
git clone <this repo> && cd codeflow
cargo install --path crates/codeflow-cli
```

Prebuilt installers via cargo-dist GitHub Releases are pending the v2.0.0
release. The scaffold is embedded in the binary (rust-embed): init is offline,
instant, and version-locked to the binary.

## Quickstart

```sh
cd your-project
codeflow init            # standard tier; idempotent and non-destructive
```

| Tier | Adds | For |
|---|---|---|
| `--minimal` | AGENTS.md, secret-scan pre-commit, gitignore; branch policy at warn | Throwaways |
| `--standard` (default) | + full git gates, Claude settings/agents/skills/commands, docs knowledge model, test gate, recall capture, CI template | Real projects |
| `--full` | + `project-management/` (epics, tasks, specs, validate gates incl. the capability hard-gate) | Work that outlives sessions |

Re-running init at a higher tier is an idempotent additive upgrade. Existing
files are never clobbered: managed files refresh by 3-way merge
(`codeflow update`), user-owned files are never touched.

## Command surface

From `codeflow --help`:

| Command | Does |
|---|---|
| `init` | Scaffold this project (idempotent, non-destructive, offline) |
| `update` | Refresh managed scaffold files (3-way merge; never clobbers) |
| `hook` | Claude-layer hooks: git-guard, session-orient, session-summary |
| `git-hook` | Git client hook target — the hook shims exec this |
| `orient` | Session-start digest (pointers, not content; ≤30 lines) |
| `test` | Test gate — configured targets or runtime stack detection |
| `validate` | Record frontmatter checks; `--docs` adds the doc-graph integrity lint |
| `status` | Generated view: branch, worktrees, in-flight work, capabilities |
| `integrate` | Land a branch: flock(rebase → test → ff-merge), gate-passing |
| `doctor` | Health checks: hooks, Claude wiring, config, permissions, network |
| `recall` | Search project memory: ledger, summaries, ADRs, epics, capabilities |
| `remote` | Remote provider operations (branch protection) |

The CLI stops at deterministic, judgment-free mechanics (ADR-0003); anything
requiring project judgment is a Claude command — stack setup is `/cf-stack`.

## Enforcement planes

All policy lives in `.codeflow/policy.json` (user-owned config, not code) and
is read by every plane:

1. **Git client hooks** — pre-commit / commit-msg / pre-push shims: protected
   branches, secret scan, commit format, branch naming, no AI attribution,
   no emoji, test gate. Harness-agnostic; no-op gracefully without the binary.
2. **Claude `git-guard` hook** (PreToolUse) — instant in-session feedback on
   what client hooks can't reach: force-push/hard-reset/delete on protected.
3. **CI** — re-runs tests, secret scan, and `validate --docs`.
4. **Remote protection** (`codeflow remote protect`) — PR + green CI required
   on protected branches. CI + remote are the authoritative perimeter; local
   layers are fast feedback by design.

Work lands on protected branches by exactly two paths: PR → green CI → merge,
or `codeflow integrate <branch> --into <target>`.

## Knowledge model

Six layers, one traceability spine (capability → epics → ADRs/specs → PRs →
ledger):

| Layer | Lives in |
|---|---|
| WHY | `docs/product.md` |
| RULES | `AGENTS.md` + skills |
| WHAT | `docs/capabilities.md` (CAP-### registry) |
| HOW | `docs/architecture.md` + `docs/decisions/` (append-only ADRs) |
| WORK | `project-management/` (full tier) |
| TRACE | ledger + session summaries + `codeflow recall` |

Docs mutate only inside gated workflows, in the same PR as the code;
`validate --docs` fails CI on dangling references.

## More

- Plan of record: [`docs/plan/v2/00-charter.md`](docs/plan/v2/00-charter.md)
- Operating contract for agents and humans: [`AGENTS.md`](AGENTS.md)
- v1 is archived in full at branch `archive/v1` (tag `v1-final`).
