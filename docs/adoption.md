# Adopting codeflow

How a project takes on the discipline layer — greenfield or brownfield — what it
gets at each tier, what codeflow owns versus what stays yours, and the daily
loop. Every claim here reflects current behavior; nothing aspirational.

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
  codeflow's shims from your manager (add the `pre-commit`, `commit-msg`, and
  `pre-push` shim paths to your existing steps). `codeflow doctor` surfaces the
  unwired state so it stays visible.

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

## The daily flow

1. **Orient.** The SessionStart digest (or `codeflow orient`) gives branch and
   worktree state, work counts, recent ADR titles, gate status, and pointers —
   read the pointed docs, not the digest, for depth.
2. **Branch in a worktree.** Work on a `{prefix}/{kebab-name}` branch in a
   worktree; never develop on the root protected-branch checkout.
3. **Gates as you go.** pre-commit (secret scan), commit-msg (format, no AI
   attribution, no emoji), pre-push (branch naming, protected-branch rules, test
   gate). Keep `codeflow test` and `codeflow validate --docs` green before push.
4. **Land by PR.** Push the branch, open a PR from the template (summary,
   changes, test results, linked epic/capability IDs), merge on green CI. With
   no remote, `codeflow integrate <branch> --into <target>` is the sanctioned
   local path.

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
