# The CodeFlow product

<!-- WHY layer. Human-owned: agents propose changes here, the human accepts.
     Keep it small and stable; this file should change rarely.
     The non-goals in Concept are what planning is checked against. -->

CodeFlow adds guardrails to a git repository where AI coding agents work.
It blocks the mistakes that are hard to undo, in git, in the agent's session
and in CI, and it keeps a record of why work was done. The agent you already use, such as Claude Code, Codex or Grok
Build, still does the developing.

## Concept

### What CodeFlow is

- One Rust binary, `codeflow`, with its scaffold built in. It works offline.
- `codeflow init` writes git hooks, in-session guards, CI checks and an
  agent contract into a repository.
- Every rule is written once, in `.codeflow/policy.json`. Each check reads
  that file.
- It does not run agents. The harness runs the session, and CodeFlow sets
  the rules the session works inside.

### The problem it solves

An agent works fast, and some of its mistakes cannot be undone:

- a commit or force push to the main branch
- a secret committed to history
- a commit message that credits the AI tool
- a destructive shell command
- work that nobody can explain a month later

A prompt that asks an agent to behave does not stop any of these. CodeFlow
turns the rules into checks that run in git, in the agent's own session and
in CI. It also keeps decisions, plans and results in the repository, so the
reason for a change survives the session that made it.

### Who it is for

| Audience | Standing |
|---|---|
| Solo, AI-assisted developers | First audience. They run Claude Code, Codex or another capable harness and want git, secret and test discipline and a durable record of why, without standing up a framework |
| Cross-harness development | A design center of the scaffold, where vendor-native interactive sessions work together on one repository |
| Multi-user coordination | Deferred |

### What it does

- **Scaffolds.** `init` and `update` lay down and refresh managed files.
  `update` regenerates only the marked blocks of shared files such as
  `AGENTS.md`, merges a managed file you edited (a conflict is proposed in a
  `.new` file), and replaces edited files only under `--force`.
- **Enforces.** Four planes check the rules (see Architecture). An agent
  never merges into a protected branch; a human merges the pull request.
- **Verifies.** `codeflow test` runs the project's test gate. `validate`
  checks records and docs, and `doctor` checks hooks, harness settings and
  CI.
- **Remembers.** Decisions, capabilities and work live as Markdown in the
  repository, and `codeflow recall` searches them with the ledger.
- **Supports the harness.** Standard and full tiers add skills and reviewer
  agents, so a second model can review or build in its own harness.
  `codeflow present` opens one review page in a local browser window.

### What it is not

Planning is checked against each row.

| It is not | What that rules out |
|---|---|
| Not a runtime harness, agent framework, or model router | No daemon and no autorun. Vendor sessions, plugins, tools, worktrees, memory, sandbox and permissions are used at the process boundary and never reimplemented (ADR-0023) |
| Not a process-enforcement engine | The CodeFlow binary does not run or schedule agents. Its gates sit where a mistake is irreversible or invisible, such as work start (approved specs first), task completion and protected branches. The standard and full tiers seed an optional pipeline workflow that runs build, review and verify stages in Claude Code; you own and adapt that copy |
| Not a graphical or terminal UI product | The one exception is `cf-present` (SPC-004): an explicit command opens a short-lived review page in an isolated browser window, served from a loopback-only, per-session service that stops itself. The opt-in docs portal is a generated static site, not an operated interface |
| Not memory infrastructure | No embeddings, vector database or database as authority. Markdown and JSONL are the truth, with a full-text search cache over them |
| Not an OS or harness security boundary | CodeFlow gives deterministic safety feedback and does not claim every harness contains an agent equally. Binaries ship for macOS, Linux (WSL2 uses the Linux binary) and, from 3.1.0, native Windows x86-64. Claude work that needs an OS sandbox on Windows runs in WSL2 or a container |

## Architecture

### The four enforcement planes

One policy file feeds four planes. Each sees the work at a different moment,
and the later planes are the ones a local edit cannot change.

```text
  .codeflow/policy.json            one source of truth for every rule
          |
          v
  agent or human works in the repository
          |
          v
  +---------------------------------------------------------------+
  | 1  git hooks            run when git runs: pre-commit,        |
  |                         commit-msg, pre-merge-commit,         |
  |                         reference-transaction, pre-push       |
  +---------------------------------------------------------------+
  | 2  in-session guards    run before an agent's commands and    |
  |                         file edits: git-guard, exec-guard,    |
  |                         edit-guard                            |
  +---------------------------------------------------------------+
                  |  push
                  v
  +---------------------------------------------------------------+
  | 3  scaffolded CI        runs the same `codeflow ci` checks    |
  |                         on the server                         |
  +---------------------------------------------------------------+
  | 4  remote branch        the host refuses direct pushes to a   |
  |    protection           protected branch; a human merges      |
  +---------------------------------------------------------------+

  Planes 1 and 2: fast local feedback; the files can be edited.
  Planes 3 and 4: the perimeter, where the host runs CI and the
  remote requires it.
```

- Minimal init installs planes 1, 2 and 3. It never arms plane 4; that is a
  repository setting, and `codeflow remote protect` can apply it where the
  host supports it.
- Installed files do not prove a plane is working. Check that hooks run, the
  harness trusts the project hooks, CI results are required and the remote
  rules hold.
- Detail for each plane is in the
  [enforcement planes page](architecture/enforcement-planes.md).

### The record

The record CodeFlow maintains has six layers, one for each question a later
reader asks: WHY, RULES, WHAT, WORK, HOW and TRACE.

- The first five layers are committed Markdown. Structured records carry YAML
  frontmatter, and the capability registry carries one YAML fence per
  capability.
- The ledger is the append-only JSONL event log the binary writes. A
  full-text search cache over the whole record serves `codeflow recall`.
- The traceability spine links a capability to its epics and tasks, their
  decisions and specs, the pull request and the ledger.

The tier chosen at `init` is recorded in the repository. The
[adoption page](adoption.md) walks that choice.

## Technical

### What ships

| Commitment | What CodeFlow ships |
|---|---|
| One binary | One installable binary with the scaffold embedded (rust-embed), offline and locked to its version. `init` and `update` write and refresh managed files by ownership class |
| Hard gates | A small set of gates around git, secrets and tests, read from `.codeflow/policy.json` by every plane |
| The knowledge model | The six layers above, kept up to date inside the ship flow, with `recall` over the record |
| Graduated weight | Minimal, standard and full tiers. The git-discipline floor is the same at every tier, and the tiers add only project-management on top (ADR-0019). The binary validates every shape, so a project can move up a tier |
| Duo contracts | Skill contracts for a Claude and Codex pair on any non-trivial task, host-neutral and run through each vendor's own interactive harness: independent parallel work, routed production and review, versioned dual approval, bounded worktree parallelism and evidence-based verification (ADR-0023, ADR-0024, ADR-0025, ADR-0046) |

### Init tiers

| Tier | Installs | Pick it when |
|---|---|---|
| `--minimal` | The enforcement floor: five git hooks, the CI check, the in-session guards, an armed `policy.json`, and a lean `AGENTS.md` and `CLAUDE.md` | The repository is a doc set, a config repo or a small tool, or you want only the guardrails |
| `--standard` (default) | Minimal plus the develop-loop skills, reviewer agents, the pipeline workflow and the `docs/` spine (product, architecture, capabilities, decisions) | The repository is a code project |
| `--full` | Standard plus `project-management/` templates for epics, tasks and specs | Work outlives a session and needs durable plans and criteria |

Every decision cited here is listed in the [decision map](decision-map.md).
