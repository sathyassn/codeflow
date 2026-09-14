---
name: cf-method
description: The CodeFlow working method — how to plan an epic, when an ADR is warranted, capability registry discipline, spec lifecycle, graduation rules, and anti-patterns. Use when planning work, making a Tier-3 decision (new dependency, schema change, boundary change), shipping an epic, or deciding how much process a change needs.
---

# cf-method — the discipline guide

The model in one line: **clarity in → light rails through → verification out.**
The scarce resources are clear inputs and verified outputs, not supervised
middles. Instructions tell, workflows do, gates verify — and a gate exists only
where a mistake is irreversible or invisible.

For every non-trivial task, read
[references/workflow-lifecycle.md](references/workflow-lifecycle.md) after the
orchestrator selects the outcome mode. It is the required transition map for
composing research, planning, design, implementation, review, documentation,
repair, and ship stages without turning them into one fixed ceremony.

## Choosing process weight

Match machinery to the work; escalate only when the lighter rung fails. The
orchestrator selects stages and weight separately. The host-neutral duo remains
the default for non-trivial work: independent Claude and Codex planning, Claude
design and integrated judgment, capability-based production, and
author-relative cross-lineage review. Solo is only a recorded degradation after
an interactive seat is unavailable. Research- or planning-only work stops after
its settled artifact. Make the weight call inside the orchestrator and
materialize durable planning with `cf-plan`, not mid-build.

Mature accepted work does not restart open-ended discovery or re-interview
settled intent. Perform a compact currency, acceptance, dependency, and anchor
check, reuse the approved records and evidence, and continue at the applicable
stage. Return to planning only for a material change to outcome, scope,
authority, acceptance/interface, dependency or decision graph, security
boundary, or irreversible tradeoff; ordinary reversible detail inside an
approved node remains execution evidence.

- **No workflow** for conversational or trivial changes — answer, edit, done.
- **Interactive `/cf-model-orchestrator` loop** for non-trivial work — parallel
  discovery, versioned joint settlement, then only the stages the outcome needs.
- **Inline `/cf-develop` loop** supports an orchestrated implementation; used
  alone, it is the recorded solo fallback: build → independent review → verify.
- **Pipeline preset** (`.claude/workflows/pipeline.workflow.js`, Claude Code) for
  unattended, batch, or parallel fan-out runs. Its assurance preset is
  explicitly single-vendor; it never claims the interactive duo's dual approval.
  (Workflows are a Claude-Code runtime; on another harness this rung is
  unavailable—use that harness's native task composition.)
- **Custom ad-hoc workflow** (Claude Code) for genuinely novel orchestration,
  not a shortcut around review.
- **Integration-branch flow** for a multi-task body of work — an epic of serial
  and/or parallel tasks lands task-by-task on a shared `integration/<epic>`
  branch, not on `main`, and the human reviews one final PR. See "Managing a
  body of work" below.
- **Stage/model composition** lives in invocation args (`args.stages`,
  `args.models`), never hardcoded into a workflow.

## Planning an epic

An epic exists to make one question answerable before any code is written:
*what does done look like, verifiably?*

**An epic is optional — reach for one only when the work needs it.** Warrant an
epic for a body of work that is **more than one PR**, **more than one session**,
or **spans multiple capabilities**. Anything smaller is a single task with
acceptance criteria and no epic; an epic never gates a single task. The clarity
checklist below applies either way — to the epic when there is one, otherwise to
the task.

Input clarity checklist — do not draft until you can state all four:

1. **Problem and audience.** What hurts, for whom, in one or two sentences.
2. **Scope boundary.** What is in, and — more important — what is explicitly
   out. Check `docs/product.md` non-goals; an epic that violates a non-goal is
   a conversation with the human, not a workaround.
3. **Acceptance criteria.** Testable statements, preferably in **EARS** ("When
   <trigger>, the system shall <response>") or **Given/When/Then** form. Each
   one must name a concrete, machine-verifiable check — a command, a test path
   or tag, or an observable with a threshold.
4. **Touched surface.** Which areas and which existing capabilities
   (`docs/capabilities.md`) this creates or changes.

If a missing answer changes the outcome, public behavior, authority, material
security boundary, irreversible action, or another operator-owned choice, ask
the smallest consequential question with evidence, options, consequences, and
a recommendation. Discover repository facts yourself and make reversible,
outcome-preserving implementation choices from evidence; do not offload
research to the operator.

Good criterion (EARS): "When a capability entry references a nonexistent epic
ID, `codeflow validate --docs` shall exit non-zero in CI."
Bad criterion: "validation works correctly" (not testable, no check named), or a
14-item list restating the implementation plan (that is design, not acceptance).

Right-size the epic for one coherent delivery cycle—normally a small number of
agent sessions or PRs after its load-bearing questions are settled. Estimate
from dependencies, remaining discovery, implementation complexity, integration,
and verification rather than translating a human staffing calendar. If the
criteria list will not fit on one screen, split the epic.

For agentic operating/estimation, capacity or deadline decisions, use
`cf-estimate`: actively offer a context-specific preview, reuse compatible
adoption, and honor decline or existing authority. Its anchored grades and
evidence-based scenarios support this workflow; they do not replace its gates,
reinterpret legacy size fields, or authorize adoption/implementation by themselves.

For a durable repository guide, `cf-docs-portal` owns the opt-in lifecycle.
Markdown stays authoritative. Configure supported seams while CodeFlow owns
the runtime, or explicitly transfer the whole runtime for project maintenance;
never turn local drift into an automatic merge, transfer or validation waiver.

## Project organization and work authority

Full-tier records keep independent EPC/SPC/TSK IDs and frontmatter links in
flat `project-management/` homes; historical nested records remain readable.
An epic or justified standalone task owns a coherent repository outcome, not
a team or folder. External portfolio/product work may have its own authority,
but active CodeFlow tracking still requires distinct Git execution anchors;
links never waive `work start`, pre-commit, or CI. Local databases are caches,
not team truth. Maintained requirements remain current; optional SPC inputs
freeze a particular work agreement.

Load `references/project-organization.md` for new-project boundary choices,
brownfield adoption, monorepos, artifact/authority selection, or implementation
discoveries. It supplies the decision model and fixed-path limits; an obvious
bounded task needs no extra reading. When authoring or editing a skill, load
`references/skill-authoring.md` for description-trigger rules.

## Managing a body of work

If the request arrives as a task set or batch, first partition it by coherent
durable outcome and direct dependencies. A batch may yield several epics,
standalone tasks, or both; it is not itself a reason to share a branch. Apply
the flow below independently to each coherent multi-task epic.

When an epic is a multi-task body — serial chains and/or parallel tasks — do
**not** land each task on `main`. Land them on a shared **integration branch**
so agents proceed autonomously and the human reviews **one** final PR. Every
other gate (commit standards, secret scan, destructive-op rules, the test gate)
still applies on every branch; only the merge-into-`main` step is deferred.
This is the default landing shape for a multi-task epic, not a convenience to
drop during planning. An exception requires a recorded Plan vN rationale and
approval from both primary seats before tasks are allocated; convenience,
short task size, or avoiding the integration step is not sufficient evidence.
The branch contains one coherent epic outcome; it is never a holding branch for
unrelated standalone tasks or a reason to invent an epic merely for batching.
The orchestrating agent chooses this routine mechanism without an operator
question once the outcome, acceptance boundaries, and dependency graph are
clear. An explicit operator request is valid input, but cannot waive those
clarity, safety, review, or protected-branch conditions.

The shared branch does not defer or replace task-level orchestration. Each task
still completes its approved producer-verification and cross-lineage review
loop before landing, including qualified Claude judgment at material design or
decision points. The final combined-diff review is an additional integration
layer for cross-task and emergent behavior, not a substitute for those reviews.

1. **Integration branch.** Cut `integration/<epic-id>-<slug>` off the current
   protected target and push it. It is **non-protected** — agents merge into it
   freely.
   Establish this stable target before task allocation; an implementation task
   cannot target a missing branch.
2. **Plan and anchor.** One epic with per-task acceptance criteria. Materialize the
   dual-approved `TASK_GRAPH vN` from `cf-model-orchestrator` into canonical
   `depends_on` task frontmatter. Bare edges are finish-before-start; guarded
   edges are only pre-settled decision points. Parallel eligibility follows
   topology, but actual fan-out still needs a critical-path benefit and safe
   isolation. Set each task's `integration_target` to the integration branch,
   validate the graph, and merge the planning PR there before implementation.
3. **Task branches.** Each task on `task/<task-id>-<slug>`, branched *from
   the integration branch*: serial tasks branch from the updated integration
   after their predecessor lands; parallel tasks branch concurrently, one
   agent + worktree each. Run `codeflow work start <task-id>` before product
   edits; it proves the task, specs, and predecessors from the target merge-base.
   Before fan-out, assign one writer per file/component and a single owner for
   shared schemas, migrations, lockfiles, generated registries, and other merge
   hotspots. Set a concurrency cap from observed host memory, CPU, disk, and
   tool limits; reserve headroom and reduce it before swap pressure, duplicate
   heavyweight builds/browsers, or context sprawl harms quality. Parallelism is
   optional when its coordination cost exceeds its critical-path gain.
4. **Land a reviewed task** only after its producer evidence and cross-lineage
   review are complete, using one of two sanctioned modes:
   - **Local** — `codeflow integrate <task-branch> --into integration/<…>`:
     flock-serialized (safe for parallel agents), rebases the task branch, runs
     the full test gate, fast-forward-merges. Preferred for tight loops.
   - **PR** — open a PR with base = the integration branch; CI runs (the
     `pull_request` trigger fires regardless of base) and the agent merges on
     green, because the base is non-protected.
   Land tasks in a valid topological order and run affected gates after each
   merge; task-local green is not integration evidence. A material node, edge,
   guard, ownership, acceptance/interface, or safety mutation creates Plan
   vN+1; another valid linearization under unchanged constraints does not.
5. **Drift control** (long-running epics): periodically **merge** `origin/main`
   *into* the integration branch. Merge only — never rebase a shared branch;
   rebase only task branches.
6. **Finish.** After the last task lands, run the ship flow *on the integration
   branch* (full suite, `validate --docs`, capability/doc/epic-record updates as
   the final commits) and have both primary seats review the exact combined
   integration diff. Then raise **one** PR `integration → main` with the epic
   summary. The human reviews and merges; delete the integration branch only
   after the merge is proven. Apply cf-ship's same post-landing proof: inspect
   dirty or untracked state first; require ancestry for a normal merge; for a
   squash, require a `MERGED` PR with matching head SHA or no unapplied `+`
   entry from `git cherry` before branch force-delete.

Boundaries are unchanged: `main` and every protected branch stay
human-merge-only. The integration branch is not a backdoor — its content reaches
`main` only through that final reviewed PR.

## Why the git boundary is remote

The git standards are enforced in layers, and the layers are not equal. Local git
hooks and the `git-guard` PreToolUse hook are **fast feedback** — they catch the
normal ways work goes wrong in-session, before a push, but an agent on the local host
can edit or skip them. Required CI and remote rules form a server-side boundary
only when configured and enforced for the actor's permissions.
`codeflow remote protect` configures supported rules; verify availability and
success, not just scaffold files.
Inspect required checks, bypass rights and actual results before claiming an
**authoritative perimeter**. Local checks remain required defense in depth.

Override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are not authentication or
proof of safety. They establish no remote boundary; agents must never set them
to slip past a local gate. That is laundering even
when remote protection is unavailable. Report the missing boundary, retain the
project's safety and review duties, and do not imply local checks replace
server-side enforcement.

## When an ADR is warranted — Tier-3 triggers

Accepted ADRs are append-only and finalized at the moment of decision, when context is
loaded — the cheapest possible "why" capture, and the best-value reading for a
fresh session. But ADR over-production is its own swamp. Write one only at a
**Tier-3 decision point**:

- **New dependency** — a crate, package, service, or external tool joins the
  project.
- **Schema change** — a persisted format, config shape, or API contract
  changes meaning.
- **Boundary change** — responsibility moves between modules, layers, or
  systems; something is split, merged, or re-owned.

Choosing a variable name, an internal refactor, an obvious bug fix: no ADR.
When in doubt, ask: "would a fresh session six months from now need to know
*why* this is so?" If yes, write it.

Use `docs/decisions/template.md`: context (the constraint, 2–5 sentences),
decision (stated as fact), consequences (honest about costs), and
`architecture_impact`. The impact field is load-bearing — when it is not
`none`, update `docs/architecture.md` **in the same PR**. ADRs are never edited
after acceptance; a reversal is a new ADR plus `superseded_by` on the old one.
An unpublished draft may be revised while its decision is unresolved; it gains
append-only authority only when accepted. Do not mislabel exploratory notes as
an accepted decision.

## Capability registry discipline

`docs/capabilities.md` is the agent's index of what the system actually does —
the first thing to consult before building ("does this exist? what does it
touch?") and the artifact that makes the system legible without reading all
the code.

Entry fields: `id` (CAP-###), `name`, `area`, `status`
(planned → building → shipped → deprecated), `verified_by` (test tags),
`epics[]`, `adrs[]` — plus exactly one paragraph of prose.

Rules:

- One entry per user-meaningful capability — what the system *does*, not how.
  "Secret scanning at commit time" is a capability; "the regex module" is not.
- The entry is updated in the same PR that ships the work — the discipline is
  the ship flow itself, not a blocking gate; `validate --docs` does not gate an
  epic from closing.
- `verified_by` names the real test tags that prove the capability. `validate
  --docs` requires a shipped entry to carry a non-empty `verified_by`, but it
  does not resolve the tags — a stale tag is a lie only the reviewer will catch.
- Deprecate, never delete — the ID spine must stay resolvable.
- At roughly 15 entries, graduate the single file to `docs/capabilities/`
  (one CAP-*.md each); the registry file becomes an index.

## Spec lifecycle

Specs are **inputs to work, not living documents.**

- Drafted during `/cf-plan`, only when interfaces, formats, or behavior need
  pinning down before building. Allocate and link with `codeflow spec new --for
  EPC-NNN|TSK-NNN`. Many work items need no spec at all.
- `status: approved` only after open questions are empty; draft specs block
  `codeflow work start`.
- Consumed during `/cf-develop`.
- **Frozen at ship:** `status: implemented` when the epic completes. After
  that, truth lives in architecture, capabilities, and tests — the spec is
  allowed to be historical. Never "update" a frozen spec to match later
  reality; that is what architecture.md is for.
- A spec's open-questions section must be empty before building starts.

## Graduation rules

Weight is graduated: one file until it hurts, and every shape is validated so
growth is mechanical, never re-architecture.

| Signal | Graduation |
|---|---|
| Work outlives sessions; planning spans days | tier standard → full (`codeflow init --full`, additive) |
| ~15 capability entries | `capabilities.md` → `docs/capabilities/CAP-*.md` + index |
| architecture.md section outgrows a screen | → `docs/architecture/<area>.md`, one-line pointer left behind |
| A doc-set or small tool grows into a code project needing the method | `--minimal` → `--standard` re-init (idempotent, additive) |

Downgrade is never destructive: stop managing, do not delete.

## Anti-patterns

- **Over-specification.** A spec for a function rename; acceptance criteria
  that restate the diff line by line. Plan weight must match work weight.
- **Ceremony.** Status meetings with yourself: progress notes nobody reads,
  hand-rolled changelogs, task-tracker mirroring. The ledger and session
  summaries capture trace automatically.
- **Stale dashboards.** Never hand-maintain a status view, a "current state"
  doc, or a progress table. If it can be computed, `codeflow status` computes
  it; stored views rot by design.
- **ADR inflation.** One ADR per task devalues the record. Tier-3 triggers
  only.
- **Doc drift after implementation.** Authoritative docs made stale by code
  updated "later, in a docs pass" rot; synchronize them in that work's ship
  flow and same PR. Standalone documentation, planning records, draft ADRs, and
  contemporaneous evidence remain owned by their applicable stages and do not
  require a fictional code change.
- **Speculative artifacts.** New agents, skills, commands, or templates are
  added when usage proves the need — never because they might help.
- **Assumption-driven building.** Guessing an operator-owned outcome or safety
  decision, or asking the operator to rediscover a fact the agent could verify.
- **Agent-merging a protected branch.** An agent never merges into protected —
  a human merges the PR, or `codeflow integrate` lands it. Override envs
  (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only; setting them
  in-session is laundering — blocked wherever a PreToolUse guard binds (Claude
  Code always; interactive codex after the one-time `/hooks` trust), while the
  git-hook plane honors the env by design as the sanctioned human path; the
  remote perimeter is the hard line. Never `gh pr merge --delete-branch`.
