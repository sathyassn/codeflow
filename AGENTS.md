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

<!-- codeflow:managed:begin scaffold=2.1.0 -->
<!-- Owned by `codeflow update`. Edits inside this block are replaced on update;
     put project-specific instructions outside the markers. -->

## Project organization — six layers

| Layer | Lives in | Changes |
|---|---|---|
| WHY — purpose, users, scope, non-goals | `docs/product.md` | rarely; human-owned |
| RULES — how we work | this file + the agent skills (`.claude/skills/`, `.agents/skills/`) | rarely |
| WHAT — what the system does | `docs/capabilities.md` (CAP-### registry) | every ship |
| HOW — structure and decisions | `docs/architecture.md` + `docs/decisions/` (ADRs) | per decision |
| WORK — planned and active work | `project-management/` (epics, tasks — allocated via `epic new`/`task new`; specs — hand-authored) | daily |
| TRACE — what happened and why | ledger + `codeflow recall` | automatic |

The traceability spine runs downward: capability → epics → ADRs → specs → PRs →
ledger. Only the capability ↔ epic ↔ ADR links are enforced by ID (`validate
--docs`); the spec, PR, and ledger links are by convention, found via `codeflow
recall`. Specs carry no CLI tooling of their own (no allocator, no model, no
validation) — they are hand-authored input docs, frozen (`status:
implemented`) when their epic ships, findable via `codeflow recall`. Answer
"why is X this way" by following frontmatter links or `codeflow recall "X"` —
never by reading all the code.

Before building anything: check `docs/capabilities.md` (does it already exist?
what does it touch?) and skim the most recent ADRs in `docs/decisions/`.

## Entry points

Every non-trivial repository task **must begin with**
`/cf-model-orchestrator`. Non-trivial includes work that needs repository or
external research, analysis, planning, design judgment, implementation,
debugging, security review, substantive documentation, or verification beyond
one obvious local check. The orchestrator selects only the stages the outcome
needs; a research- or planning-only task stops before implementation. A trivial
edit or conversational answer needs no skill.

`/cf-plan`, `/cf-develop`, and the other skills are supporting flows, not
competing ways around the duo default. Use them inside the orchestrated flow or
after its preflight has proved a required interactive seat unavailable and
recorded the reduced assurance. When uncertain whether work is trivial, treat
it as non-trivial.

| Intent | Use |
|---|---|
| Any non-trivial repository work | `/cf-model-orchestrator` — the host-neutral Claude+Codex default: both independently research/analyze/plan; Claude leads design; the host assigns each task a producer and cross-lineage reviewer by verified capability; Fable owns the integrated Claude judgment. Claude Code hosts through the official Codex plugin; Codex hosts through interactive Claude CLI + tmux. Missing seats degrade legibly after preflight |
| Materialize an agreed plan | `/cf-plan` — supporting flow for acceptance criteria, epic/spec/ADR artifacts, used inside the duo or after a recorded solo degradation |
| Build when the duo is proven unavailable | `/cf-develop` — the solo fallback: build → fresh-context independent review (`cf-reviewer` where available) → verify, bounded rework |
| Land finished work | `/cf-ship` — capability/ADR/doc updates + PR through the gates |
| Set up or extend the stack | `/cf-stack` — detect the stack, write test/lint config, record standards |
| Tailor a scaffolded project | `/cf-customize` — verify the tools its flows need and fill the project-owned specifics, after `codeflow init` or when an update brings new defaults |
| Qualify a model or harness change | `/cf-evaluate-model` — deliberate native-interactive regression/capability evaluation over disposable fixtures; use inside the orchestrated maintenance flow, never for ordinary work |
| Get an outside opinion | `/cf-consult` — an independent, read-only second opinion from another vendor's CLI (a full edit handoff is the `cf-delegate` skill) |
| Mechanics | `codeflow` CLI: `test [setup]`, `validate [--docs]`, `status [--delivery]`, `recall "<query>"`, `orient`, `doctor`, `integrate <branch>`, `remote`, `epic new`, `task new` |

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
the scaffolded CI runs the same commit-format, attribution, emoji, and branch
checks through the `codeflow ci` binary — one source of truth with the hooks, no
inline drift (CodeFlow ADR-0017). The PreToolUse plane is per-harness: Claude Code
always; interactive codex after the one-time `/hooks` trust; a harness with no
hooks engine not at all — and since headless task execution is prohibited
outright (CodeFlow ADR-0018), that last case is the whole gap. The local planes are fast
in-session feedback; CI and remote branch protection are the authoritative,
server-enforced perimeter — the real boundary (why the split matters:
cf-method, "Why the git boundary is remote"). The rules, compressed:

- **Branches:** `{prefix}/{kebab-name}`. Prefixes: `feat/ fix/ docs/ refactor/
  test/ chore/ ci/ hotfix/ plan/ spike/ experiment/ integration/`. Pick by work intent.
- **Commits:** conventional format `type(scope): description` (scope optional) —
  imperative mood, lower-case type from the policy whitelist, no trailing period;
  the description ≤ 50 chars and the whole subject line ≤ 72. A body, when
  present, is **only** `-` bullets — at most 3, each a single line ≤ 72 chars —
  optionally followed by a `BREAKING CHANGE:` footer; no prose paragraphs. One
  logical change per commit. Other git-trailer footers (`Refs:`, `Signed-off-by:`,
  …) are blocked unless the project opts them in — a team can allow specific
  trailers, require a ticket reference, or require `Signed-off-by` (DCO) via
  `policy.json`.
- **Breaking changes are a judgment call, made every commit.** Before each
  commit, ask whether it changes anything a consumer depends on — API, CLI flags,
  config schema, file formats, defaults, or managed-file semantics. If yes, mark
  the subject `type!:` and add a `BREAKING CHANGE:` footer stating the migration
  path; that footer is what drives the major version bump. A `policy.json`
  `breaking_watch_paths` glob warns when a declared contract surface is touched
  unmarked, but the glob only nudges — detection is yours, not the gate's.
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
- **Durability push:** when a remote is configured, push the working branch after
  each committed logical unit so work survives a machine failure; use `git push
  --force-with-lease` (never bare `--force`) when history was rewritten. This is
  backup, not a merge — the pre-commit secret scan still guards what is pushed and
  every merge gate still stands. Teams that want to forbid it set
  `git.force_push_unprotected` in `policy.json` (default allow).
- **Bodies of work:** a multi-task epic lands task-by-task on a non-protected
  `integration/<epic>` branch (agents merge there); only the finished body
  reaches `main`, via one human-reviewed PR. See cf-method, "Managing a body of
  work."
- **PR bodies:** follow the template — summary, changes, testing, linked
  capability/ADR IDs — and **match the presentation to the shape of the
  data**: tables for tabular data (coverage, test→pins, exit-code or
  before/after matrices), fenced blocks for pasted output, short one-line
  bullets for the rest — never paragraph-walls. The Summary is plain
  language a reader with zero context understands — no jargon, say what
  it means for the user. A code PR **must** carry real test
  evidence in `## Testing`: the pasted test-summary output, the coverage
  number, the new tests it adds, and what was NOT tested — "tests pass" as
  prose is a claim, not evidence, and is not reviewable. Docs-only PRs say
  so in one line plus the doc checks run. Type and breaking-change come
  from the conventional commits, not the body.
- When a gate blocks you, fix the cause — never bypass (`--no-verify`, editing
  hooks, exporting gate tokens). Gates exist only where mistakes are
  irreversible or invisible.

## Worktree doctrine

Develop in a worktree per session (native worktree support). Protected branches
stay checked out only at the repo root, so git itself refuses a second checkout —
structural protection for free.

Before a task's first branch, worktree, commit, merge, rebase, push, or delete
mutation, make three ordered work-start assertions:

1. **IDENTITY** — establish where you actually are: the worktree path and
   checked-out branch, read from `git worktree list` and
   `git branch --show-current`, never from memory or the prompt.
2. **INTENT-MATCH** — confirm this is the worktree and branch *this task* was
   assigned. On a mismatch, stop and surface it — never silently adapt to
   where you happen to be, and never accept name resemblance as a match.
   Wrong-branch is the failure to prevent; it outranks stale-branch. If the
   assignment explicitly calls for creating a worktree that does not exist
   yet, the protected root may only create that exact named branch/worktree
   after the currency check; do no task edits there, then rerun all three
   assertions inside the new worktree. Absence alone never authorizes repair.
3. **CURRENCY** — fetch, then check the base against the repository's
   configured target branch (normally `origin/main` or `origin/master`). New
   work starts in a fresh worktree off its current tip; build on an older base
   only when the task explicitly pins it.

A landed task ends with cleanup that proves the merge and inspects worktree
state first. Ancestry never proves a squash merge: require PR state `MERGED`
and match the branch tip to its recorded head SHA, or show that `git cherry`
against the updated target has no unapplied `+` entry, before any branch
force-delete. Never force-remove a dirty worktree; preserve or harvest dirty
or untracked work first. Unproven work is retained, never guessed safe.

Parallelize independent work when it shortens the critical path, but make the
dependency graph, file ownership, and integration order explicit first. Each
parallel task gets one owner, branch, and worktree; never let two sessions write
the same worktree or concurrently edit a shared contract, schema, migration, or
other merge hotspot. The host sets a bounded concurrency cap from available CPU,
memory, disk, and tool limits, monitors pressure, and reduces fan-out before
swapping, duplicate heavyweight builds, or context sprawl degrade quality.
Dependent work stays sequential.

For a multi-task body, integrate through `integration/<epic>` and serialize each
landing with `codeflow integrate`; rerun the affected and aggregate gates after
every merge. Rebase task branches, never a shared integration branch. Parallel
output is not complete until the integration worktree is green and the combined
diff has received the same producer verification, cross-lineage unit review, and
integrated Fable judgment as a serial change.

## Session flow

1. Orient: the SessionStart digest (~30 lines) gives branch and worktree state,
   work counts, recent ADRs, gate status, and pointers. Read the pointed docs
   before deep work; the digest is pointers, not content. No digest (hook
   unwired, or not yet trusted on your harness)? Run `codeflow orient` yourself.
2. Work on a correctly prefixed branch in a worktree; commit small and often.
3. End: on a harness with a SessionEnd hook (Claude Code), the session summary
   is captured automatically; elsewhere nothing is captured for you —
   externalize per "Externalize state as you go" below. Decisions of record
   belong in ADRs, not in chat history.

## Workflow discipline

Reason like a senior engineer and architect: outcome-driven, evidence-bound, and
proportional — deep thinking for consequential or novel work, a light pass for
the trivial; knowing which weight a task warrants is itself judgment. Principles
to reason from, not a rote checklist.

- **Work to the outcome.** Know the task's intent and what tangible result means
  *done* before acting. Then work in small, verifiable steps — a failed gate or
  review is input to the next step, not the end. Iterate until the outcome is
  verified, or stop and surface a genuine blocker — promptly and well-framed:
  the situation, the options weighed, and a recommendation; never late, never
  bare.
- **Ground it in evidence — never assume.** Treat an unclear requirement, API, or
  fact as a stop-and-verify, not a guess. Research non-trivial decisions in both
  breadth and depth — the project's own code and docs first, then the best current
  external sources (official/primary references, reputable discussion), and
  adjacent fields where a better idea may live.
- **Find broadly; act by materiality.** Do not let easy cosmetics displace
  consequential work. Substantiate candidate issues, classify the consequence
  if unresolved, then prioritize by severity, confidence, likelihood or
  reachability, blast radius, urgency, recurrence or systemic leverage, and
  dependencies. Remediation effort informs sequencing, never severity. Lead
  with material strategic, architectural, structural, correctness, security,
  robustness, operability, and meaningful edge/error concerns. Repeated small
  symptoms may reveal one systemic issue; isolated preferences and nits stay
  explicitly non-blocking and are batched. Route an evidenced material issue
  outside scope to one tracked item—escalating an imminent severe risk—without
  silently expanding scope or mutating external state. "Nothing material
  found" is valid; never farm issues.
- **Why — and why not.** Interrogate a non-trivial decision in both directions
  until it hits bedrock: why this, and why *not* this — why was the alternative
  rejected? Steelman the rejected option before the decision stands; a choice
  that has not survived its strongest rival is a default, not a decision.
- **Think independently — not a yes-man.** Do not accept a request, an opinion, a
  claim, or a proposed approach — the operator's included — on assertion alone.
  Research it, weigh the alternatives, and stress-test it; when you disagree or
  see a better path, say so with reasoning and evidence. The operator makes the
  final call and their decision is respected — but they are owed your honest
  analysis, not agreement. Agreement without examination is a failure mode, not
  deference.
- **Think in depth, not at the surface.** Push past the first-order read to the
  second, third, and further order. Chase the implication chain — "and therefore?
  … and therefore?" — until it lands on the fundamental that actually decides the
  matter (the forward twin of the "why? … why?" root-cause drill: consequences
  forward, causes backward, both to fundamentals). And follow those consequences
  not only down one thread but across — trace how each order ripples through the
  related domains and aspects, the whole value chain and sphere it touches, not
  just the immediate area — and let that full picture inform the decision. Surface
  thinking yields dumb answers; the useful insight lives a few levels down and a
  few domains over.
- **Decide by options and horizons.** Enumerate the real options with pros and
  cons for *this* situation and the scenarios each creates — sequential,
  parallel, and over time — then decide against short- and long-term priorities,
  both stated. Prefer the robust, durable solution that stands the test of time;
  when expedience wins, it wins deliberately and says so.
- **Guard your context.** Long context degrades quality. Keep the thinking,
  planning, and synthesis in your own session, but delegate breadth (wide
  searches, reading many files), long or mechanical passes, and independent
  checks to a subagent or workflow (where your harness has them) — each works in
  its own context and returns a condensed result, so yours stays sharp for the
  decisions.
- **Write only what earns its keep.** Make the smallest clear, idiomatic, durable
  change that fully satisfies approved behavior—not minimum LOC. Add no
  speculative feature, abstraction, configuration, dependency, compatibility
  layer, or dead path; every material complexity maps to a current requirement,
  observed constraint, or evidenced risk. Preserve justified structure: stay
  DRY with judgment, modular, and coherent with the repository's architecture.
  Unexplained hard-coding, duplicated business knowledge, swallowed errors, or
  missing accepted edge cases are brittle under-design, not simplicity.
  Calibrate structure to accepted lifetime, scale, change rate, contributor and
  integration breadth, operational risk, and reversibility—not size alone. If
  missing context would materially change the design, clarify it; otherwise use
  established safe practices and the least speculative reversible choice.
- **Shape the deliverable.** Layer it concept → detail, each layer complete at
  its own altitude; reveal depth progressively — never dump, and never cut key
  information to condense. Presentation is contextual and proportionate: a
  simple answer stays a simple answer. Bullets for the enumerable; prose only
  where it earns its place; and when relationships, hierarchy, state,
  timelines, mappings, or a decision are materially clearer drawn, prefer the
  smallest useful ASCII diagram with a one-line caption. Never add decorative
  or forced diagrams, headings, tables, or recaps. Presentation creative,
  elegant, modern, fit to the domain; web artifacts componentized, never
  monolithic. Then take the audience's seat: structured, logical, progressive,
  the sought depth findable? Craft lives in the details — sloppy work is a
  defect, not a style. For substantial prose, apply `cf-editorial-review`:
  verified truth and policy outrank CodeFlow philosophy, the consuming
  project's documented voice/examples, audience/medium/task, and requested
  tone — in that order. Preserve technical meaning; never fabricate personality,
  experience, feelings, familiarity, or slang.
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
- **Match the gate to the blast radius.** Recoverable, task-scoped project
  edits and deletions are ordinary work. A system-level, cross-boundary,
  credential/IAM, production, destructive-disk, security-weakening, or other
  irreversible/high-blast-radius action stops for exact scope, preview/dry-run
  evidence where supported, a current verified checkpoint or backup with a
  restore path, and explicit authenticated human approval. Model agreement or
  an automatic safety reviewer is not authorization. The peer cannot be used
  to bypass the host's stricter boundary. CodeFlow's non-relaxable command
  class remains agent-blocked even after approval: a human operator performs
  it through a separate controlled channel while the models prepare and verify
  evidence. For other high-blast-radius actions that effective host policy
  permits after approval, execute one bounded step at a time and verify it.
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
- Review verdicts come from an independent pass — the `cf-reviewer` subagent in
  Claude Code; a separate read-only interactive review pass on any other harness
  (never headless — a headless pass fires no in-session guards) — against the
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
- **Wire a managed artifact in the same change you add it.** When you add or
  rename a managed artifact (a skill, agent, resource, or workflow), do it
  end-to-end: add its `assets/base/scaffold-manifest.toml` `[[entry]]`(ies),
  mirror any skill byte-identically across `.claude/skills` and `.agents/skills`,
  and resync the managed baseline. The `manifest_consistency` test
  (`crates/codeflow-core/tests/`) enforces the first two — an asset missing from
  the manifest or a drifted mirror fails the build.
- **Areas:** `engine` = `crates/codeflow-core` + `crates/codeflow-cli`;
  `scaffold` = `assets/` (base scaffold + stack profiles); `docs` = `docs/`.
- **v1 is a quarry, not a source tree** (charter D22). It lives on the
  `archive/v1` branch — no archive folder in the working tree; retrieve files
  via `git checkout archive/v1 -- <path>`. Code crosses only via a deliberate
  keep-decision, trimmed and re-tested; docs, templates, and Claude artifacts
  are always re-authored from scratch — never copied.
- **Rust gates:** `cargo fmt --all -- --check`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` must be green
  before push (workspace lints: clippy all = deny, pedantic = warn). Edition
  2021, workspace-managed dependency versions in the root `Cargo.toml`.
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
