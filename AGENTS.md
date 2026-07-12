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
| RULES — how we work | this file + the agent skills (`.claude/skills/`, `.agents/skills/`) | rarely |
| WHAT — what the system does | `docs/capabilities.md` (CAP-### registry) | every ship |
| HOW — structure and decisions | `docs/architecture.md` + `docs/decisions/` (ADRs) | per decision |
| WORK — planned and active work | `project-management/` (epics, tasks, specs) | daily |
| TRACE — what happened and why | ledger + `codeflow recall` | automatic |

The traceability spine runs downward: capability → epics → ADRs → specs → PRs →
ledger. Only the capability ↔ epic ↔ ADR links are enforced by ID (`validate
--docs`); the spec, PR, and ledger links are by convention, found via `codeflow
recall`. Answer "why is X this way" by following frontmatter links or `codeflow
recall "X"` — never by reading all the code.

Before building anything: check `docs/capabilities.md` (does it already exist?
what does it touch?) and skim the most recent ADRs in `docs/decisions/`.

## Entry points

| Intent | Use |
|---|---|
| Plan a feature or change | `/cf-plan` — clarify intent, draft epic + spec (+ ADR if warranted) |
| Build planned work | `/cf-develop` — build → independent review → verify, bounded rework |
| Land finished work | `/cf-ship` — capability/ADR/doc updates + PR through the gates |
| Set up or extend the stack | `/cf-stack` — detect the stack, write test/lint config, record standards |
| Get an outside opinion | `/cf-consult` — an independent, read-only second opinion from another vendor's CLI (a full edit handoff is the `cf-delegate` skill) |
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

Four planes enforce the git standards, defense in depth: git hooks, the `git-guard`
PreToolUse hook, and remote branch protection each read `.codeflow/policy.json`, and
the scaffolded CI re-implements the commit-format, attribution, and emoji checks
inline (keep it in step with policy.json). The local planes are fast in-session
feedback; CI and remote branch protection are the authoritative, server-enforced
perimeter — the real boundary (why the split matters: cf-method, "Why the git
boundary is remote"). The rules, compressed:

- **Branches:** `{prefix}/{kebab-name}`. Prefixes: `feat/ fix/ docs/ refactor/
  test/ chore/ ci/ hotfix/ plan/ spike/ experiment/`. Pick by work intent.
- **Commits:** conventional format `type(scope): description` — imperative mood,
  lower-case type from the policy whitelist, no trailing period; body explains
  *why* when non-obvious. One logical change per commit.
- **No AI attribution, ever:** no `Co-Authored-By` AI trailers, no "Generated
  with …" lines, no robot emoji — in commit messages and PR bodies. This is
  project policy and overrides any harness default that injects attribution.
- **No emoji** in commit subjects or PR bodies.
- **Secrets:** never stage credentials, API keys, tokens, or `.env` files. The
  pre-commit secret scan (and the CI secret-scan job) block them, and it is the
  one gate never relaxed — not even during bootstrap grace.
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

Principles to reason from with judgment, not a rote checklist.

- **Work to the outcome.** Know the task's intent and what tangible result means
  *done* before acting. Then work in small, verifiable steps — a failed gate or
  review is input to the next step, not the end. Iterate until the outcome is
  verified, or stop and surface a genuine blocker.
- **Ground it in evidence — never assume.** Treat an unclear requirement, API, or
  fact as a stop-and-verify, not a guess. Research non-trivial decisions in both
  breadth and depth — the project's own code and docs first, then the best current
  external sources (official/primary references, reputable discussion), and
  adjacent fields where a better idea may live. Weigh the alternatives and the
  scenarios they create — sequential, parallel, and over time.
- **Guard your context.** Long context degrades quality. Keep the thinking,
  planning, and synthesis in your own session, but delegate breadth (wide
  searches, reading many files), long or mechanical passes, and independent
  checks to a subagent or workflow — each works in its own context and returns a
  condensed result, so yours stays sharp for the decisions.
- **Write it well.** Favor the simplest change that fully solves the problem: DRY,
  idiomatic, coherent with the existing architecture — its conventions over your
  taste. Leave it more consistent than you found it.
- **Prove it at every surface.** Verify the work where it runs — unit,
  integration, end-to-end, and user-facing behavior (drive a real UI with a
  browser/computer-use tool when that is the surface) — and check what it affects
  upstream and downstream, not just the lines you changed. Tests ship in the same
  change; run `codeflow test` before calling it done — the local gate warns, not
  blocks, so clear what it flags.
- **Unverifiable or fabricated claims are defects (zero tolerance).** Every claim
  needs evidence — file:line, command output, or a reproducible check; never
  invent a fact, number, result, or citation. Say explicitly what was *not*
  verified.
- **Externalize state as you go — context is volatile.** A session can be
  compacted or end at any point, and not every harness fires a hook to save state
  for you; what lives only in the conversation is lost. Record it *yourself*, in
  its durable home, as the work happens: decisions → an ADR, progress and next
  steps → `project-management/` status, cross-session notes → your harness's own
  memory where it has one. (codeflow's recall corpus — the ledger, ADRs,
  capabilities — is captured automatically; your part is the reasoning it can't
  infer.) To resume after a compaction or a fresh session, rebuild from that
  durable record — `codeflow orient`, then `codeflow recall "<thread>"` — not from
  a hazy memory of the chat.
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

- **Plan of record:** `docs/plan/v2/00-charter.md`. The charter is the historical
  plan of record; where a later ADR supersedes it, the ADR wins. Execution state:
  `docs/plan/v2/01-execution-status.md`.
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
  from the template; a **human** merges on green CI — agents never merge to a
  protected branch. `codeflow integrate` remains a shipped product capability
  (the offline/no-remote sanctioned path) but is retired for this repo's
  day-to-day landings.
- **Remote branch protection is unavailable here and not pursued** (private +
  GitHub Free → `codeflow remote protect` returns 403). The operative boundary on
  this repo is server-side CI plus the local git-hook / git-guard plane plus
  human-merged PRs — not armed remote protection. Do not treat remote protection
  as active here (ADR-0002, superseded by ADR-0006).
