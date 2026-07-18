# Adopting codeflow

How a project takes on the discipline layer — greenfield or brownfield — what it
gets at each tier, what codeflow owns versus what stays yours, and the daily
loop. Every claim here reflects current behavior; nothing aspirational.

## Install the binary

`codeflow` is a single binary. Install the prebuilt build (macOS arm64/x64,
Linux x64) with the shell installer from the latest release — this anonymous
one-liner works once codeflow's releases are public; while the repo is private,
use the checkout build or the `gh release download` path below (both authenticate
as a collaborator):

```sh
curl -fsSL https://github.com/sathyassn/codeflow/releases/latest/download/codeflow-cli-installer.sh | sh
```

Or build from a checkout, with a Rust toolchain:

```sh
cargo install --path crates/codeflow-cli
```

To grab a specific platform tarball directly (e.g. to pin a version or script the
install), use `gh`:

```sh
# platform: aarch64-apple-darwin | x86_64-apple-darwin | x86_64-unknown-linux-gnu
A=codeflow-cli-aarch64-apple-darwin
gh release download v2.1.0 -R sathyassn/codeflow -p "$A.tar.xz" -D /tmp/cf --clobber
tar -xf "/tmp/cf/$A.tar.xz" -C /tmp/cf
install "/tmp/cf/$A/codeflow" ~/.cargo/bin/    # or any directory on PATH
```

Upgrading the binary improves every repo at once, because hooks call `codeflow`
from `PATH` (see "The update story").

## What each tier installs

Tier is recorded in `.codeflow/project.toml`; re-running `init` at a higher tier
is an idempotent additive upgrade (downgrade = stop managing, never delete).

Enforcement is the floor; the tiers scale project-management (ADR-0019). Every
tier is a clean superset of the one below.

| Tier | Adds | For |
|---|---|---|
| `--minimal` | The complete git-discipline enforcement floor: all five git hooks (`pre-commit`, `commit-msg`, `pre-push`, `pre-merge-commit`, `reference-transaction`), the CI check, the in-session `git-guard`/`exec-guard` + orient/summary hooks (`.claude/settings.json` + the `.codex/` starter), the armed `policy.json`, `.gitignore`, and a lean `AGENTS.md` + `CLAUDE.md` | Any repo — doc-sets, config repos, small tools |
| `--standard` (default) | + the develop-loop method (cf-* skills, reviewer agents, the pipeline), the six-layer `docs/` spine, the full contract, the test gate, recall capture, and harness integration | Code projects |
| `--full` | + `project-management/` (epics, tasks, specs, templates) and the `validate --docs` referential lint | Programs whose work outlives sessions |

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
floor is the same at every tier — the upgrade never *adds* enforcement you were
missing, only the method on top. An existing `--minimal` repo initialized before
this floor moved down gains the rest of the enforcement plane automatically on
its next `codeflow update` (the reconciliation installs the now-in-tier files).
When you add the standard/full method, run `/cf-customize` before treating the
generated product, architecture, commands, or tool posture as project truth;
`codeflow doctor` keeps a reminder visible while scaffold sentinels remain.

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

## Network, tools, and autonomy

These are enabled in runtime settings as well as described in the skills
(ADR-0025):

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
  without a legacy `sandbox_mode` override. It enables live search and broad
  public egress, grants exact loopback for local verification, keeps private
  destinations and arbitrary Unix sockets closed, and sends eligible
  `on-request` escalations through reviewer-subagent safety review.
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
- Codex `auto_review` sends eligible escalation prompts to its reviewer
  subagent. It preserves autonomy but does not make profile denies absolute:
  an approved request can cross the sandbox boundary. Set
  `approvals_reviewer = "user"` in the project or launch override when
  organizational policy requires a human decision, and constrain allowed
  reviewers in managed requirements where available.

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
   template (summary, changes, testing, linked epic/capability IDs); a
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

Parallelize only independent tasks whose isolation pays for the coordination:
one owner/branch/worktree per task, one owner for shared contracts and conflict
hotspots, and a concurrency cap based on available memory, CPU, disk, model
contexts, and browser/tool capacity. Land tasks serially through
`codeflow integrate`, rerun affected gates after each landing, then run the
aggregate suite and both-model review on the combined integration diff. Never
run concurrent writers in one worktree or rebase the shared integration branch.

`main` stays human-merge-only throughout — the integration branch is never a
backdoor to it. See cf-method's "Managing a body of work" for the full procedure.

## Model and harness upgrades

Do not promote a new production model, harness release, permission profile, or
material instruction rewrite from a single successful task. Run
`/cf-evaluate-model` from the orchestrated maintenance flow: validate the
requirement/case traceability, materialize fresh disposable fixtures, exercise
the candidate through its native interactive harness with the real tools/MCPs,
run the full three-trial suite, independently grade retained evidence, and
compare it with the pinned baseline. A hard semantic regression blocks even
when the candidate is faster or uses fewer tokens. Preserve the result and
review evidence, then use the skill's marker+run-ID-gated cleanup for fixtures;
never use it against the consuming project itself (ADR-0027).

## Enforcement planes — who catches what

One policy (`.codeflow/policy.json`), four planes — and all four install from
`--minimal` up: enforcement is the floor, not a standard-tier feature (ADR-0019).
Git hooks are harness-agnostic (any agent or human — including Codex); the
in-session PreToolUse guards (`git-guard` + `exec-guard`) are a fast bonus for
Claude and, through a byte-compatible payload, an **interactive** Codex session
(ADR-0008); CI re-runs the gates as the perimeter; remote branch protection is
the server-side backstop. Local planes are feedback — CI and remote are the
authoritative line (charter §6.5).

| Protection | git hooks | in-session guard | CI | remote |
|---|---|---|---|---|
| Commit / non-ff merge commit on protected | pre-commit / pre-merge-commit | git-guard | yes | yes |
| FF-merge, `reset --hard`, `branch -D` on protected | reference-transaction | git-guard | — | yes (result unpushable) |
| Push / force-push / delete to protected | pre-push | git-guard | — | yes |
| `gh pr merge` into a protected base | — (hooks can't see a PR) | git-guard | — | yes |
| Destructive command (`rm -rf /`, `mkfs`, fork bomb) | — | exec-guard (block) | — | — |
| Privilege escalation (`sudo`, `LD_PRELOAD`) | — | exec-guard (warn) | — | — |
| Commit format, no-attribution, no-emoji, secrets | commit-msg / pre-commit | partial | yes | — |
| Override-token laundering, `--no-verify` bypass | — | git-guard (structural) | — | — |

Two facts the matrix encodes. **PR-content checks are git-guard/CI by design** —
a git hook never sees `gh pr create`/`gh pr merge`, so attribution/emoji scans
and the protected-base check live in the Claude layer and CI, not the hooks.
**The human override (`CODEFLOW_HUMAN_OVERRIDE=1`) and the integrate token apply
to the git-hook plane only** — the git-guard never trusts them, because an agent
in a session cannot prove it is a human. `reference-transaction` needs git ≥
2.28; on older git it is absent and protection falls back to the other planes.

### How far the discipline reaches across harnesses

codeflow has two kinds of thing: **enforcement** (gates that block) and
**guidance** (instructions and skills that inform). They reach different
distances, so be precise about what a given harness actually gets:

- **Enforcement is universal — it binds *any* harness (and a human).** The git
  client hooks and CI are harness-agnostic: they act on git operations and PRs,
  not on which tool produced them. So conventional-commit format, the secret
  scan, no-AI-attribution, branch/push/protected-merge rules, and the test gate
  apply to Claude Code, Codex, a future CLI, or a human at a terminal, equally.
  This is the authoritative floor; nothing opts out of it.
- **In-session guards are Claude + interactive Codex.** The PreToolUse
  `git-guard`/`exec-guard` add fast, pre-git feedback. They are wired for Claude
  (`.claude/settings.json`) and, via a byte-compatible payload, an **interactive**
  Codex session (`.codex/hooks.json`, ADR-0008). Headless `codex exec` does not
  fire PreToolUse hooks — it is bound by the git-hook plane + CI instead. That
  guard gap is one reason headless execution is no longer a sanctioned
  cross-model transport (ADR-0018): the consult/delegate/duo flows run
  interactive-only, where the guards live.
- **Guidance (AGENTS.md + the `cf-*` skills) is Claude + Codex.** Both read the
  repo `AGENTS.md` operating contract; the skills ship to `.claude/skills/`
  (Claude) and `.agents/skills/` (Codex). The **workflow** runtime
  (`pipeline.workflow.js`) is Claude-Code-only.
- **A harness codeflow does not specifically integrate** (for example Google's
  Antigravity `agy`) is **still bound by the git-hook plane + CI** — because those
  are harness-agnostic — but does **not** receive the in-session guards, the
  skills, or (verified on `agy` 1.0.15) the `AGENTS.md` instructions. Its reliable
  boundary is enforcement, not guidance.

The one-line version: **codeflow *enforces* the same rules on every harness (git
hooks + CI); it *guides* Claude and Codex.** Any tool that touches the repo is
disciplined; the richer in-session help is where the integrations are.

**Verified against:** codex-cli 0.144.3 and Claude Code 2.1.211 on 2026-07-16
(with earlier hook-specific evidence retained by ADR-0008, ADR-0013, and
ADR-0014). These surfaces (hook payload contracts, config schemas) move fast on
both sides; the release checklist
([docs/releasing.md](releasing.md)) re-verifies them before each codeflow tag.

## Delegation quickstart (optional)

Cross-vendor consult and delegation is opt-in (ADR-0005; transport refined by
ADR-0023—interactive-only, one lane per direction). One-time setup: authenticate
Codex manually, enable `codex@openai-codex` in Claude Code, and install the
Claude CLI plus tmux for the reverse lane. Codeflow never automates auth.
`codeflow doctor` reports inspectable prerequisites; retain a scoped
interactive canary in each direction.

- `/cf-consult` gets an independent, read-only second opinion from the vendor
  the session is *not* — from Claude Code through the official
  `codex-plugin-cc` plugin (`/codex:review`); from Codex by driving the
  interactive `claude` CLI in a task-scoped tmux session with Stop/StopFailure
  hook completion—and makes you synthesize it against your
  own analysis (never paste its reply as fact). Headless `codex exec` /
  `claude -p` are not sanctioned delegation transports (ADR-0023).
- A full edit handoff (`cf-delegate`; from Claude Code, `/codex:rescue`) runs
  only inside a worktree on a feature branch, where the delegate's commits pass
  the same gates and independent review as yours — enforcement is
  author-agnostic.
- A missing lane degrades legibly: do the work yourself and say so.

### Codex parity

When a repo is driven through OpenAI's Codex CLI instead of Claude, protection
comes from two layers, and it helps to be precise about which does what.

- **The git-hook plane binds Codex unconditionally.** It is harness-agnostic —
  a Codex `git push --force origin main` against protected `main` is refused by
  the `pre-push` shim (`codeflow pre-push: BLOCKED — policy rule
  git.push_to_protected`) exactly as any agent's would be. Verified live on
  codex-cli 0.142.5. This needs no Codex configuration.
- **The in-session PreToolUse guards are an interactive-Codex bonus.** The
  scaffold ships a `.codex/` starter (part of the enforcement floor, from
  `--minimal` up): `hooks.json`
  wires `codeflow hook git-guard` and `codeflow hook exec-guard` onto Codex's
  `PreToolUse` (Bash) event, and `config.toml` enables the hooks engine with a
  guarded workspace permission profile, broad public network, live search,
  `approval_policy = "on-request"`, and `approvals_reviewer = "auto_review"`.
  The latter sends eligible prompts to a reviewer subagent, not a human; select
  `user` in the project or launch override when policy requires a human approval
  boundary, and enforce the allowed reviewer through managed requirements.
  The config intentionally contains no legacy `sandbox_mode`, because that
  would shadow the named profile. Codex's hook payload is byte-compatible with
  Claude's, so the same binaries run unchanged.

One-time setup for the in-session guards: Codex loads a project's `.codex/hooks.json`
only when that project's `.codex/` layer is trusted. Run `/hooks` inside an
interactive `codex` session once to trust the CodeFlow hooks. **Note:** in testing
on codex-cli 0.142.5, headless `codex exec` did not run project PreToolUse hooks
even with `--dangerously-bypass-hook-trust` and the layer trusted — so treat the
in-session guards as an interactive-session safeguard, and rely on the git-hook
plane (which always applies) for headless Codex runs. codeflow's own flows no
longer produce headless runs: ADR-0018 makes cross-model transport
interactive-only (consult/delegate/duo never shell out to `codex exec`), so a
headless Codex run happens only when a user starts one — and the git-hook
plane + CI still bind it.

A Codex-primary session can host the full duo, not only a consult. Codex
coordinates and implements in its current App/interactive CLI session while
Claude, reached through a task-scoped interactive tmux session, leads design and
performs the final independent review. From a Claude Code host, the same fixed
roles run through the official Codex plugin. Both seats independently research
and plan before approving the same versioned contract (ADR-0023).

Google's Antigravity `agy` is **not** bound automatically (its hook dialect
differs and its macOS reliability is unresolved); the cf-delegate skill carries
an experimental, manual opt-in snippet for those who want it. As a *delegate*,
`agy` is retired: its only documented drive shape is headless one-shot, which
ADR-0018 prohibits.
