# Adopting codeflow

How a project takes on the discipline layer — greenfield or brownfield — what it
gets at each tier, what codeflow owns versus what stays yours, and the daily
loop. Every claim here reflects current behavior; nothing aspirational.

## Install the binary

**Once v2.0.0 is published**, install a prebuilt binary (macOS arm64/x64, Linux
x64) onto your `PATH` with the release's shell installer:

```sh
curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
```

The installer (cargo-dist) fetches the right prebuilt binary for your platform
and places `codeflow` in your Cargo bin dir; the exact asset name is on the
[releases page](https://github.com/sathyassn/codeflow/releases). **Works today**
from a checkout, with a Rust toolchain: `cargo install --path crates/codeflow-cli`.
Upgrading the binary improves every repo at once, because hooks call `codeflow`
from `PATH` (see "The update story").

## What each tier installs

Tier is recorded in `.codeflow/project.toml`; re-running `init` at a higher tier
is an idempotent additive upgrade (downgrade = stop managing, never delete).

| Tier | Adds | For |
|---|---|---|
| `--minimal` | `AGENTS.md`, secret-scan pre-commit, `.gitignore`; branch/commit policy at warn | Throwaways — blocking policy on a scratch repo trains bypassing |
| `--standard` (default) | + full git gates, `.claude/` settings/agents/skills/commands, the six-layer `docs/`, the test gate, recall capture, a CI template | Real projects |
| `--full` | + `project-management/` (epics, tasks, specs, templates) and the `validate --docs` referential lint | Work that outlives sessions |

## Greenfield — an empty directory

```sh
mkdir myproject && cd myproject && git init
codeflow init --standard --yes    # scaffold; offline; sane defaults
```

`init` scaffolds every managed file, then — because this is a fresh repo — makes
the initial `chore: scaffold codeflow standard tier` commit for you, wires git
hooks via `core.hooksPath`, and arms branch policy. The printed report lists
every file written. From there:

1. Start your first feature on a `feat/*` branch, in a worktree.
2. Build with tests; commit small (`type(scope): description`).
3. Land via a PR (or `codeflow integrate` with no remote).

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

Adopt gradually: start `--minimal` (just the secret scan + gitignore, policy at
warn), run for a while, then re-init `--standard` and later `--full` as the work
earns the weight. Each step is additive and idempotent.

## Ownership model — who owns what on update

| Class | Examples | What `update` does |
|---|---|---|
| Fully-managed | `.claude/` agents, skills, commands; git-hook shims; CI template | Replaced if you never touched them; 3-way merged from `.codeflow/.baseline/` if you did — conflicts land as `.new` + a report |
| Managed-region | `AGENTS.md` / `CLAUDE.md` markers; `.gitignore` markers; `.claude/settings.json` codeflow keys | Only the marked region or codeflow-owned keys are rewritten; everything else is yours |
| User-owned, schema-versioned | `.codeflow/policy.json`, `.codeflow/project.toml`, all of `docs/` | Only *new* keys are added with defaults and reported; your values are never mutated |
| Engine-generated | `.codeflow/manifest.json`, `.codeflow/.baseline/`, `status` / `orient` views | Rewritten by the binary; never hand-edit |

## The update story

Two motions (charter §10): upgrade the binary — which improves every repo at
once, because hooks call `codeflow` from `PATH` — then run `codeflow update` per
repo to refresh scaffold files. Until you do, every command prints a
version-skew warning.

`codeflow update` refreshes managed files by the classes above: unmodified
managed files are replaced, files you changed get a 3-way merge from the
baseline, and anything that cannot merge cleanly is written beside your file as
`<name>.new` with a report entry — never clobbered, never silently skipped.
`--diff <FILE>` writes the report plus unified diffs; `--force` replaces
user-modified managed files instead of merging.

New policy keys arrive this way too. When a codeflow upgrade adds a
`.codeflow/policy.json` key (for example the `merge_to_protected`,
`pr_merge_to_protected`, and `local_ref_protection` keys added in ADR-0007),
`update` inserts it with its shipped default and reports it, and never touches
the values you already set — so tightening ships without a manual migration.

## The daily flow

1. **Orient.** The SessionStart digest (or `codeflow orient`) gives branch and
   worktree state, work counts, recent ADR titles, gate status, and pointers —
   read the pointed docs, not the digest, for depth.
2. **Branch in a worktree.** Work on a `{prefix}/{kebab-name}` branch in a
   worktree; never develop on the root protected-branch checkout.
3. **Gates as you go.** pre-commit (secret scan), commit-msg (format, no AI
   attribution, no emoji), pre-merge-commit and reference-transaction
   (protected-branch merge/ref rules — the latter also catches fast-forward
   merges, `reset --hard`, and `branch -D`), pre-push (branch naming,
   protected-branch rules, test gate). Keep `codeflow test` and `codeflow
   validate --docs` green before push.
4. **Land by PR, merged by a human.** Push the branch, open a PR from the
   template (summary, changes, test results, linked epic/capability IDs); a
   human merges it on green CI (an agent-performed `gh pr merge` into a
   protected base is blocked — that is the boundary). With no remote, `codeflow
   integrate <branch> --into <target>` is the sanctioned local path, and a human
   can override the git layer for a local merge with `CODEFLOW_HUMAN_OVERRIDE=1`.

### A body of work — the integration branch

The loop above lands one branch per PR onto `main`. When the work is an epic —
several tasks, some serial, some parallel — landing each on `main` floods the
human with reviews and makes agents wait on one another. Instead, cut a shared
**integration branch** and land the tasks there:

- `integration/<epic>` is branched off `main` and is **non-protected**, so
  agents merge tasks into it — by `codeflow integrate <task> --into
  integration/<…>` or a PR based on the integration branch. Every other gate
  (commits, secrets, tests, protected-branch rules) still applies.
- Only the finished body reaches `main`, as **one** human-reviewed
  `integration → main` PR raised after the ship flow runs on the integration
  branch.

`main` stays human-merge-only throughout — the integration branch is never a
backdoor to it. See cf-method's "Managing a body of work" for the full procedure.

## Enforcement planes — who catches what

One policy (`.codeflow/policy.json`), four planes. Git hooks are
harness-agnostic (any agent or human); the Claude `git-guard` is a fast
in-session bonus; CI re-runs the gates as the perimeter; remote branch
protection is the server-side backstop. Local planes are feedback — CI and
remote are the authoritative line (charter §6.5).

| Protection | git hooks | git-guard (Claude) | CI | remote |
|---|---|---|---|---|
| Commit / non-ff merge commit on protected | pre-commit / pre-merge-commit | yes | yes | yes |
| FF-merge, `reset --hard`, `branch -D` on protected | reference-transaction | yes | — | yes (result unpushable) |
| Push / force-push / delete to protected | pre-push | yes | — | yes |
| `gh pr merge` into a protected base | — (hooks can't see a PR) | yes | — | yes |
| Commit format, no-attribution, no-emoji, secrets | commit-msg / pre-commit | partial | yes | — |
| Override-token laundering, `--no-verify` bypass | — | yes (structural) | — | — |

Two facts the matrix encodes. **PR-content checks are git-guard/CI by design** —
a git hook never sees `gh pr create`/`gh pr merge`, so attribution/emoji scans
and the protected-base check live in the Claude layer and CI, not the hooks.
**The human override (`CODEFLOW_HUMAN_OVERRIDE=1`) and the integrate token apply
to the git-hook plane only** — the git-guard never trusts them, because an agent
in a session cannot prove it is a human. `reference-transaction` needs git ≥
2.28; on older git it is absent and protection falls back to the other planes.

## Delegation quickstart (optional)

Cross-vendor consult and delegation is opt-in (ADR-0005). One-time setup: run
`codex login` on your own ChatGPT subscription — codeflow never automates auth.
`codeflow doctor` reports the `delegates` check.

- `/cf-consult` gets an independent, read-only second opinion from `codex` and
  makes you synthesize it against your own analysis (never paste its reply as
  fact).
- A full edit handoff runs only inside a worktree on a feature branch, where the
  delegate's commits pass the same gates and `cf-reviewer` as yours — enforcement
  is author-agnostic.
- Missing or unauthenticated `codex` degrades legibly: do the work yourself and
  say so.
