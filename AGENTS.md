# codeflow — agent operating contract

CodeFlow is the AI-development discipline layer you install into any repo: one
Rust binary (`codeflow`) that scaffolds, enforces, verifies, and remembers —
while Claude Code (or any harness) does the developing.

- **Stack:** rust
- **Areas:** engine, scaffold, docs
- **Tier:** full

This file is the canonical instruction set for any agent or human working in this
repo. The block between the codeflow markers below is maintained by
`codeflow update`; everything outside it is project-owned — extend freely.

<!-- codeflow:managed:begin scaffold=2.0.0-dev -->
<!-- Owned by `codeflow update`. Edits inside this block are replaced on update;
     put project-specific instructions outside the markers. -->

## Project organization — six layers

| Layer | Lives in | Changes |
|---|---|---|
| WHY — purpose, users, scope, non-goals | `docs/product.md` | rarely; human-owned |
| RULES — how we work | this file + `.claude/skills/` | rarely |
| WHAT — what the system does | `docs/capabilities.md` (CAP-### registry) | every ship |
| HOW — structure and decisions | `docs/architecture.md` + `docs/decisions/` (ADRs) | per decision |
| WORK — planned and active work | `project-management/` (epics, tasks, specs) | daily |
| TRACE — what happened and why | ledger + `codeflow recall` | automatic |

The traceability spine links by ID, downward: capability → epics → ADRs/specs →
PRs → ledger. Answer "why is X this way" by following frontmatter links or
`codeflow recall "X"` — never by reading all the code.

Before building anything: check `docs/capabilities.md` (does it already exist?
what does it touch?) and skim the most recent ADRs in `docs/decisions/`.

## Entry points

| Intent | Use |
|---|---|
| Plan a feature or change | `/cf-plan` — clarify intent, draft epic + spec (+ ADR if warranted) |
| Build planned work | `/cf-develop` — build → independent review → verify, bounded rework |
| Land finished work | `/cf-ship` — capability/ADR/doc updates + PR through the gates |
| Set up or extend the stack | `/cf-stack` — detect the stack, write test/lint config, record standards |
| Get an outside opinion | `/cf-consult` — an independent second opinion (or handoff) from another vendor's CLI |
| Mechanics | `codeflow` CLI: `test`, `validate [--docs]`, `status`, `recall "<query>"`, `orient`, `doctor`, `integrate <branch>`, `remote` |

## Planning and tracking

- Every piece of work states acceptance criteria before building starts. Ambiguous
  input is a blocker: ask, never assume.
- In-session work uses the harness's native task tools. Durable work (full tier)
  lives in `project-management/` as markdown + frontmatter, updated in the same PR
  as the code it tracks.
- Specs are inputs, frozen (`status: implemented`) when their epic ships. Truth
  then lives in architecture, capabilities, and tests.
- Status views are generated (`codeflow status`) — never hand-maintain a dashboard.

## Git rules

Enforced by git hooks, the git-guard hook, remote protection, and CI — all reading
`.codeflow/policy.json`. The rules, compressed:

- **Branches:** `{prefix}/{kebab-name}`. Prefixes: `feat/ fix/ docs/ refactor/
  test/ chore/ ci/ hotfix/ plan/ spike/ experiment/`. Pick by work intent.
- **Commits:** conventional format `type(scope): description` — imperative mood,
  lower-case type from the policy whitelist, no trailing period; body explains
  *why* when non-obvious. One logical change per commit.
- **No AI attribution, ever:** no `Co-Authored-By` AI trailers, no "Generated
  with …" lines, no robot emoji — in commit messages and PR bodies. This is
  project policy and overrides any harness default that injects attribution.
- **No emoji** in commit subjects or PR bodies.
- **Protected branches** (`main`/`master` + policy globs): never commit, merge,
  push, force-push, delete, or hard-reset on them. Work lands by exactly two
  paths: PR → green CI → merged by a human, or `codeflow integrate <branch>
  --into <target>`. Never set override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate
  tokens) — that is laundering — and never `gh pr merge --delete-branch` (it
  can corrupt the root repo).
- **Bodies of work:** a multi-task epic lands task-by-task on a non-protected
  `integration/<epic>` branch (agents merge there); only the finished body
  reaches `main`, via one human-reviewed PR. See cf-method, "Managing a body of
  work."
- **PR bodies:** summary, changes, test results, linked epic/capability IDs.
- When a gate blocks you, fix the cause — never bypass (`--no-verify`, editing
  hooks, exporting gate tokens). Gates exist only where mistakes are
  irreversible or invisible.

## Worktree doctrine

Develop in a worktree per session (native worktree support). Protected branches
stay checked out only at the repo root, so git itself refuses a second checkout —
structural protection for free. `codeflow doctor` warns when an agent session
edits the root checkout directly.

## Session flow

1. Orient: the SessionStart digest (~30 lines) gives branch and worktree state,
   work counts, recent ADRs, gate status, and pointers. Read the pointed docs
   before deep work; the digest is pointers, not content.
2. Work on a correctly prefixed branch in a worktree; commit small and often.
3. End: the session summary is captured automatically — no ceremony. Decisions
   of record belong in ADRs, not in chat history.

## Workflow discipline

- **No assumptions.** Unclear requirement, API, or behavior → verify first: read
  the code, run it, or ask. State any assumption you could not eliminate.
- **Unverifiable claims are defects.** "Tests pass" requires test output; "X
  works" requires evidence — file:line, command output, or a reproducible check.
  Say explicitly what was *not* verified.
- **Tests ship with code** in the same PR. `codeflow test` green before push.
- **Docs mutate only inside the ship flow, in the same PR as the code:**
  capability entry on epic completion; `architecture.md` when an ADR declares
  architecture impact; ADR at Tier-3 decision points (new dependency, schema
  change, boundary change).
- **Append-only records:** ADRs and the ledger are never edited — supersede with
  a new entry instead.
- Review verdicts come from the independent reviewer (`cf-reviewer`) against the
  stated acceptance criteria, with evidence. Self-review is not review.

<!-- codeflow:managed:end -->

## Project-specific instructions

<!-- Everything below is yours. `codeflow update` never touches it. -->

- **Plan of record:** `docs/plan/v2/00-charter.md`. Where anything conflicts
  with it, the charter wins. Execution state: `docs/plan/v2/01-execution-status.md`.
- **This repo builds the product and is its own first consumer.** `assets/` is
  as much the product as `crates/` — scaffold content is held to the charter's
  §4.4 size caps and is embedded into the binary via rust-embed.
- **Areas:** `engine` = `crates/codeflow-core` + `crates/codeflow-cli`;
  `scaffold` = `assets/` (base scaffold + stack profiles); `docs` = `docs/`.
- **v1 is a quarry, not a source tree** (charter D22). It lives at
  `archive/v1`. Code crosses only via a deliberate keep-decision, trimmed and
  re-tested; docs, templates, and Claude artifacts are always re-authored from
  scratch — never copied.
- **Rust gates:** `cargo test` and `cargo clippy` (workspace lints: clippy all
  = deny, pedantic = warn) must be green before push. Edition 2021,
  workspace-managed dependency versions in the root `Cargo.toml`.
- **This repo lands via PRs only (ADR-0006):** push a feature branch, open a PR
  from the template, merge on green CI. `codeflow integrate` remains a shipped
  product capability (the offline/no-remote sanctioned path) but is retired for
  this repo's day-to-day landings.
