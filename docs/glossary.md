# Glossary

The terms a new reader meets in this guide, each with a short meaning and the page that explains it.

| Term | Meaning | Where to read more |
|---|---|---|
| Altitude | One of the three reading levels every guide page uses. Concept says what the subject is, who it is for and what it is not; Architecture shows how the parts relate; Technical states exactly what holds | [utility presentation](architecture/utility-presentation.md) |
| Architecture decision record (ADR) | A numbered record in `docs/decisions/` of one decision and why it was made. An accepted ADR is never rewritten; a later ADR supersedes it | [AGENTS.md](../AGENTS.md) |
| Baseline | The pristine shipped copy of each managed file, kept in `.codeflow/.baseline/`. `codeflow update` merges from it when you have edited the file | [adoption](adoption.md) |
| Bootstrap grace | CodeFlow's own scaffold commit, the one sanctioned commit that passes before the gates guard a new repository. The secret scan is never graced | [adoption](adoption.md) |
| Canary | A small live run in the real interactive harness that proves access or behavior a status command cannot, such as a delegate lane working in each direction | [adoption](adoption.md) |
| Capability (CAP) | One entry, `CAP-NNN`, in the registry of what the system does, with its status and the tests that verify it | [capabilities](capabilities.md) |
| Consult | `/cf-consult`, a read-only second opinion from the other vendor's model. You weigh its reply against your own analysis | [delegation](delegation.md) |
| Cross-lineage review | Review of a unit of work by a model family other than the one that wrote it | [duo orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
| Delegate | `/cf-delegate`, a full edit handoff to the other vendor's model. It runs only in a worktree on a feature branch, so its commits pass the same gates as yours | [delegation](delegation.md) |
| Delegate turn | One prompt handed to a peer harness and tracked to a result through the lifecycle commands `codeflow delegate init`, `arm` and `wait` and the `delegate-turn` hook | [architecture](architecture.md) |
| Doctor | `codeflow doctor`, nineteen health checks that report ok, warn or fail and change no file in the repository | [troubleshooting](troubleshooting.md) |
| Duo | The default way non-trivial work runs: a Claude seat and a Codex seat research and plan independently, then reconcile one plan, with Claude leading design | [duo orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
| Enforcement floor | The git discipline every tier installs: the five git hooks, the CI check, the in-session guards and the armed `policy.json`. It is the same at every tier | [adoption](adoption.md) |
| Enforcement plane | One of the four places a git rule is checked: git client hooks, in-session guards, CI and remote branch protection. All four read `.codeflow/policy.json` | [enforcement planes](architecture/enforcement-planes.md) |
| Epic (EPC) | A record, `EPC-NNN`, for a body of work that groups several tasks. Epics come with the full tier | [adoption](adoption.md) |
| exec-guard | The in-session guard that checks a shell command before the harness runs it. It blocks destructive commands and privilege escalation | [enforcement planes](architecture/enforcement-planes.md) |
| Feedback | Comments a reviewer leaves on a present session. They are kept as append-only history, delivered with `codeflow present feedback` and marked addressed or dismissed with `codeflow present resolve` | [present sessions](architecture/present.md) |
| Figure family | One of the nine kinds of drawn figure: flow, structure, layering, sequence, state, coverage, extent, derivation and graph. The author picks the family by the relationship the reader must see | [figure grammar](../assets/base/agents/skills/cf-docs-portal/resources/figure-grammar.md) |
| git-guard | The in-session guard that checks git and `gh` commands before the harness runs them. It blocks commits, merges, pushes and pull request merges into protected branches, attribution in pull request bodies and gate bypasses | [enforcement planes](architecture/enforcement-planes.md) |
| Harness | The interactive tool a model develops in, such as Claude Code, Codex or Grok Build. CodeFlow installs rules around the harness and does not replace it | [product](product.md) |
| Herdr | The named-tab terminal host that runs an interactive peer CLI in its own tab. tmux is the degraded fallback | [delegation](delegation.md) |
| Integration branch | A non-protected `integration/<epic>` branch where the tasks of a multi-task epic land one at a time. Only the finished body reaches `main`, through one human-reviewed pull request | [adoption](adoption.md) |
| Ledger | The append-only JSON Lines event log the binary writes. It is one of the sources `codeflow recall` searches | [architecture](architecture.md) |
| Managed file | A file CodeFlow ships and refreshes on `codeflow update`, such as skills, agents, git hook shims and the CI template. When you have edited it, `update` merges your edits and writes any conflict beside the file as `<name>.new` | [adoption](adoption.md) |
| Managed region | The block between codeflow markers inside a file you otherwise own, such as `AGENTS.md`, `CLAUDE.md` or `.gitignore`. `codeflow update` rewrites only that block | [adoption](adoption.md) |
| Model binding | An approved record that ties one model, effort and harness version to a passed full evaluation, kept under `~/.codeflow/qualified-bindings/`. A project may select one per role in `.codeflow/model-selection.json` | [model upgrades](model-upgrades.md) |
| Model orchestrator | `/cf-model-orchestrator`, the skill every non-trivial repository task starts with. It picks the stages the outcome needs and runs the duo | [AGENTS.md](../AGENTS.md) |
| Orient digest | The short session-start summary of branch and worktree state, work counts, recent decisions, gate status and pointers. The SessionStart hook prints it, or run `codeflow orient` | [adoption](adoption.md) |
| Ownership class | One of the five classes that decide what `codeflow update` may change in a file: fully-managed, managed-region, user-owned schema-versioned, user-owned docs and engine-generated | [adoption](adoption.md) |
| Page class | The kind of guide page, set in the portal configuration: explanatory (a figure per altitude), illustrated (an unchanged source with a companion figure), pass-through (rendered as it is, with a reason) or derived lookup (a table generated from a source) | [utility presentation](architecture/utility-presentation.md) |
| policy.json | `.codeflow/policy.json`, the one file where the git rules are written. Every enforcement plane reads it | [enforcement planes](architecture/enforcement-planes.md) |
| Portal | The optional repository guide built from the committed Markdown after `codeflow portal setup`. Its output is disposable and the Markdown stays the only authority | [documentation portal](capabilities/CAP-015-opt-in-documentation-portal.md) |
| Present session | One bounded, owner-private review opened with `codeflow present open`. The agent writes the document and an isolated local browser window shows it for comment | [present sessions](architecture/present.md) |
| Primary | The lead model of a family in the duo. The responsible primary owns a unit of work, may hand it to an executor, and inspects and accepts the result | [duo orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
| Recall | `codeflow recall "<query>"`, full-text search over the ledger, session summaries, decision records, epics, capabilities, tasks and frozen specs | [command line](cli.md) |
| Seat | One of the two model roles in the duo, a Claude seat and a Codex seat. Each is reached through its vendor's own interactive harness | [duo orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
| Skill | A packaged set of instructions an agent loads for one kind of task, such as `/cf-customize`. Standard and full tiers install them under `.claude/skills/` and `.agents/skills/` | [AGENTS.md](../AGENTS.md) |
| Spec (SPC) | A record, `SPC-NNN`, holding the agreed change for an epic or task. It is approved once its open questions are resolved and frozen when that work ships | [AGENTS.md](../AGENTS.md) |
| Task (TSK) | A record, `TSK-NNN`, for one reviewable unit of work. It belongs to an epic or states why it stands alone, and it is built on a `task/TSK-NNN-<slug>` branch | [adoption](adoption.md) |
| Test gate | `codeflow test`, which runs the configured test targets, or the tests of a detected stack, and must be green before push | [command line](cli.md) |
| Tier | How much method `codeflow init` installs, recorded in `.codeflow/project.toml`. Minimal is the enforcement floor alone, standard (the default) adds skills, reviewer agents and the docs spine, and full adds project-management records | [adoption](adoption.md) |
| Work start | `codeflow work start TSK-NNN`, a read-only check that a durable task was planned and anchored on its target branch before implementation begins | [adoption](adoption.md) |
| Worktree | A separate checkout under `.worktrees/` where one session develops one branch. Protected branches stay at the repository root | [AGENTS.md](../AGENTS.md) |
