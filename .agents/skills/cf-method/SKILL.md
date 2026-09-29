---
name: cf-method
description: The CodeFlow working method — how to plan an epic, when an ADR is warranted, capability registry discipline, spec lifecycle, graduation rules, and anti-patterns. Use when planning work, making a Tier-3 decision (new dependency, schema change, boundary change), shipping an epic, or deciding how much process a change needs.
---

# cf-method: the discipline guide

The model in one line: **clarity in → light rails through → verification out.**
The scarce resources are clear inputs and verified outputs, not supervised
middles. Instructions tell, workflows do, gates verify — and a gate exists only
where a mistake is irreversible or invisible.

For routed work (by touched paths), read
[references/workflow-lifecycle.md](references/workflow-lifecycle.md) after the
orchestrator selects the outcome mode. It is the required transition map for
composing research, planning, design, implementation, review, documentation,
repair, and ship stages without turning them into one fixed ceremony. The
whole delivery flow, from a request to main, is stated once with its figures
in [references/delivery-process.md](references/delivery-process.md): read it
when shaping a body of work, landing a batch, or handling a change midway.

## Choosing process weight

Match machinery to the work; escalate only when the lighter rung fails. The
orchestrator selects stages and weight separately. The host-neutral duo remains
the default for routed work: independent Claude and Codex discovery, one
Claude-drafted plan challenged by Codex, Claude design and integrated
judgment, capability-based production, and author-relative cross-lineage
review. Solo is only a recorded degradation after an interactive seat is
unavailable. Research- or planning-only work stops after its settled
artifact. Make the weight call inside the orchestrator and materialize
durable planning with `cf-plan`, not mid-build.

Mature accepted work does not restart open-ended discovery or re-interview
settled intent. Perform a compact currency, acceptance, dependency, and anchor
check, reuse the approved records and evidence, and continue at the applicable
stage. Return to planning only for a material change to outcome, scope,
authority, acceptance/interface, dependency or decision graph, security
boundary, or irreversible tradeoff; ordinary reversible detail inside an
approved node remains execution evidence.

- **No workflow** for conversational or trivial changes — answer, edit, done.
- **Interactive `/cf-model-orchestrator` loop** for routed work: parallel
  discovery, one plan approval, then only the stages the outcome needs.
  `/cf-develop` supports it; used alone, it is the recorded solo fallback.
- **Pipeline preset** (`.claude/workflows/pipeline.workflow.js`, Claude Code) for
  unattended, batch, or parallel fan-out runs; its assurance is explicitly
  single-vendor and never claims the interactive duo's dual approval. Stage
  and model composition lives in invocation args (`args.stages`,
  `args.models`), never hardcoded. On another harness this rung is
  unavailable; use that harness's native task composition.
- **Standalone task** for one outcome: its record and code land together in
  one reviewed PR, which is its own candidate.
- **Integration-branch flow** for a multi-task body of work: an epic of serial
  and/or parallel tasks lands in gated batch candidates on a shared
  `integration/<epic>` branch, not on `main`, and the human reviews one final
  PR. See "Managing a body of work" below.

## Planning an epic

An epic exists to make one question answerable before any code is written:
*what does done look like, verifiably?* Warrant an epic only when the work
fails the standalone test in the work lifecycle reference; otherwise it is a
single task with acceptance criteria and no epic, and an epic never gates a
single task. The clarity checklist below applies either way.

Input clarity checklist, the one statement of it (`cf-plan` and the
orchestrator point here). Do not draft until you can state all five:

1. **Problem and audience.** What hurts, for whom, in one or two sentences.
2. **Outcome and public behavior.** The intended result and what users or
   adopters will observe.
3. **Scope boundary.** What is in, and, more important, what is explicitly
   out. Check `docs/product.md` non-goals; an epic that violates a non-goal is
   a conversation with the human, not a workaround. Name material authority,
   security and recovery constraints.
4. **Acceptance criteria.** Testable statements, preferably in **EARS** ("When
   <trigger>, the system shall <response>") or **Given/When/Then** form. Each
   names the evidence that proves it: a test, a bounded observation, or an
   independent review. Good: "When a capability entry references a
   nonexistent epic ID, `codeflow validate --docs` shall exit non-zero in
   CI." Bad: "validation works correctly", or a 14-item list restating the
   implementation plan (that is design, not acceptance).
5. **Touched surface.** Which areas and which existing capabilities
   (`docs/capabilities.md`) this creates or changes.

If a missing answer changes the outcome, public behavior, authority, material
security boundary, irreversible action, or another operator-owned choice, ask
the smallest consequential question (smallest in scope, not count) with
evidence, options, consequences, and a recommendation. Discover repository
facts yourself and make reversible, outcome-preserving implementation choices
from evidence; do not offload research to the operator.

An epic is one outcome. When its criteria describe two outcomes, it is two
epics on one integration line. Size tasks by the rule in the orchestrator's
task-graph contract: one observable outcome per task, split only for a
written reason. For an explicit estimate, capacity or deadline request, use
`cf-estimate`. For a durable repository guide, `cf-docs-portal` owns the
opt-in lifecycle; Markdown stays authoritative.

## Project organization and work authority

Full-tier records keep independent EPC/SPC/TSK IDs and frontmatter links in
flat `project-management/` homes; historical nested records remain readable.
An epic or justified standalone task owns a coherent repository outcome, not
a team or folder. External portfolio/product work may have its own authority,
but active CodeFlow tracking still requires distinct Git execution anchors;
links never waive `work start`, pre-commit, or CI. Local databases are caches,
not team truth.

Load `references/project-organization.md` for new-project boundary choices,
brownfield adoption, monorepos, artifact/authority selection, or implementation
discoveries. It supplies the decision model and fixed-path limits; an obvious
bounded task needs no extra reading. When a work item is planned, started,
blocked, completed or cancelled, follow
[the work lifecycle](references/project-organization.md#the-work-lifecycle). When authoring or editing a skill, load
`references/skill-authoring.md` for description-trigger rules.

## Managing a body of work

If the request arrives as a task set or batch, first partition it by coherent
durable outcome and direct dependencies. A batch may yield several epics,
standalone tasks, or both; it is not itself a reason to share a branch. Apply
the flow below independently to each coherent multi-task epic; the delivery
process reference draws it.

A multi-task epic lands on a shared, non-protected **integration branch**, not
on `main`, and the human reviews **one** final PR. Every other gate (commit
standards, secret scan, destructive-op rules, the test gate) still applies on
every branch; only the merge into `main` is deferred. This is the default
landing shape for a multi-task epic; an exception needs a recorded rationale
in the approved plan before tasks are allocated. The branch holds one coherent
epic outcome, never unrelated standalone tasks. The orchestrating agent
chooses this routine mechanism without an operator question once the outcome,
acceptance boundaries, and dependency graph are clear. An explicit operator
request about the landing shape is valid input and is honoured, but it cannot
waive the clarity, safety, review or protected-branch conditions.

1. **Break down once.** Before task allocation, cut and push
   `integration/<epic-id>-<slug>` off the current protected target
   (`epic new --integration`). One planning PR creates the epic, every task
   with its criteria and `integration_target`, and the approved graph as
   `depends_on`; it is validated, reviewed once and merged there. Later
   changes ride in the batched epic amendment.
2. **One PR per task.** Each task works on `task/<task-id>-<slug>` from the
   integration branch, in its own worktree, and runs
   `codeflow work start <task-id>` before product edits. Its one PR carries
   the code, tests, docs, any change to its own criteria, its status and its
   acceptance block. Before review, it merges the current integration line
   into the task branch and resolves conflicts there. Each task still
   completes its producer verification and cross-lineage review before it
   joins a batch.
3. **Land in batches.** The primary assembles reviewed heads into a small
   batch candidate in dependency order, inspects the resolved hunks and
   integration seams on product paths (asking the other lineage only when it
   hand-resolved a product hunk or two tasks touched one hotspot), and runs
   one full gate on that exact candidate. It lands through one of two
   sanctioned modes: **Local**, `codeflow integrate <candidate> --into
   integration/<…>` on a candidate cut from the current line tip (serialized,
   runs the full gate, keeps the tested tip); or **PR** into the integration
   branch, merged by the primary once the candidate's full gate is green and
   the PR's required checks pass. The PR cites the gate run by its id and
   revision from its durable home. Unit reviews are not repeated.
4. **A red batch is diagnosed first.** Drop a member only when evidence
   attributes the failure to it; it and its dependents leave the batch, the
   fix goes in that task's PR, and the rest is regated. A shared runner or
   environment defect is fixed at its owner and the same candidate regated.
5. **Drift control** (long-running epics): periodically **merge** `origin/main`
   *into* the integration branch. Merge only: never rebase a shared branch;
   rebase only task branches.
6. **Finish once.** After the last batch lands, prove the epic once on the
   integration branch (the full gate over everything, `validate --docs`, the
   capability, doc and epic-record updates), close the epic, and raise
   **one** PR `integration -> main`. The human reviews and merges; cleanup
   follows the worktree rules' "Cleanup" section, merge proof first.

Boundaries are unchanged: `main` and every protected branch stay
human-merge-only. The integration branch is not a backdoor: its content
reaches `main` only through that final reviewed PR.

## Why the git boundary is remote

The git standards are enforced in layers, and the layers are not equal. Local
git hooks and the `git-guard` PreToolUse hook are **fast feedback**; an agent
on the local host can edit or skip them. Required CI and remote rules form a
server-side boundary only when configured and enforced for the actor's
permissions: `codeflow remote protect` configures supported rules, and you
inspect required checks, bypass rights and actual results before claiming an
**authoritative perimeter**. Override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate
tokens) are not authentication or proof of safety; agents never set them to
slip past a local gate, which is laundering even when remote protection is
unavailable. Report a missing boundary and keep the project's safety and
review duties. The rule file `.codeflow/rules/git-rules.md`, "Enforcement",
is the home of what each plane checks.

## When an ADR is warranted — Tier-3 triggers

Accepted ADRs are append-only and finalized at the moment of decision, when
context is loaded: the cheapest "why" capture, and the best-value reading for
a fresh session. But ADR over-production is its own swamp. Write one only at
a **Tier-3 decision point**:

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

`docs/capabilities.md` is the agent's index of what the system actually does:
the first thing to consult before building ("does this exist? what does it
touch?"). Entry fields: `id` (CAP-###), `name`, `area`, `status`
(planned → building → shipped → deprecated), `verified_by` (test tags),
`epics[]`, `adrs[]`, plus exactly one paragraph of prose.

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

Specs are **inputs to work, not living documents.** Drafted during `/cf-plan`
only when interfaces, formats, or behavior need pinning down before building
(allocate and link with `codeflow spec new --for EPC-NNN|TSK-NNN`; many work
items need no spec). `status: approved` only after open questions are empty;
draft specs block `codeflow work start`. Consumed during `/cf-develop`.
**Frozen at ship:** `implemented` is derived once every consumer is complete,
never written; after that, truth lives in architecture, capabilities, and
tests, and a frozen spec is never "updated" to match later reality.

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
