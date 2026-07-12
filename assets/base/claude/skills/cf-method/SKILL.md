---
name: cf-method
description: The CodeFlow working method — how to plan an epic, when an ADR is warranted, capability registry discipline, spec lifecycle, graduation rules, and anti-patterns. Use when planning work, making a Tier-3 decision (new dependency, schema change, boundary change), shipping an epic, or deciding how much process a change needs.
---

# cf-method — the discipline guide

The model in one line: **clarity in → light rails through → verification out.**
The scarce resources are clear inputs and verified outputs, not supervised
middles. Instructions tell, workflows do, gates verify — and a gate exists only
where a mistake is irreversible or invisible.

## Choosing process weight

Match machinery to the work; escalate only when the lighter rung fails. Every
rung that builds code carries an independent review pass — the `cf-reviewer`
subagent in Claude Code; a separate read-only interactive review pass on any
other harness, never headless (cf-develop carries the same branch) — review is
a stage, not a courtesy, and self-review is not review. Selecting the execution
skill is itself a planning decision, orthogonal to weight: the rungs below set
how much *process*; *who reviews* is set too — the duo (codex as a second,
independently-trained model beside the independent pass, each reviewing the
other's work) is the **default for all dev work**; solo is only the
degradation when the duo is unavailable (`cf-model-orchestrator`). Make the
weight call in `cf-plan`, not mid-build.

- **No workflow** for conversational or trivial changes — answer, edit, done.
- **Inline `/cf-develop` loop** for interactive work — the default path:
  build → independent review → verify, with bounded rework.
- **Pipeline preset** (`.claude/workflows/pipeline.workflow.js`, Claude Code) for
  unattended, batch, or parallel fan-out runs — the same build/review/verify
  stages, composed from a named preset. (Workflows are a Claude-Code runtime;
  on another harness this rung is unavailable — escalate by other means.)
- **Custom ad-hoc workflow** (Claude Code) when no preset fits — for genuinely
  novel orchestration (a one-off audit sweep, a migration), not a shortcut around
  the review stage. Presets are defaults, not constraints.
- **Integration-branch flow** for a multi-task body of work — an epic of serial
  and/or parallel tasks lands task-by-task on a shared `integration/<epic>`
  branch, not on `main`, and the human reviews one final PR. See "Managing a
  body of work" below.
- **Stage and model composition lives in the invocation args** (`args.stages`,
  `args.models`) — never hardcoded into the workflow file.

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

If the user's request leaves any of these open, ask. Questions before drafting
are cheap; assumptions discovered at review are expensive.

Good criterion (EARS): "When a capability entry references a nonexistent epic
ID, `codeflow validate --docs` shall exit non-zero in CI."
Bad criterion: "validation works correctly" (not testable, no check named), or a
14-item list restating the implementation plan (that is design, not acceptance).

Right-size the epic: it should ship in days, not weeks. If the criteria list
will not fit on one screen, split the epic.

## Managing a body of work

When an epic is a multi-task body — serial chains and/or parallel tasks — do
**not** land each task on `main`. Land them on a shared **integration branch**
so agents proceed autonomously and the human reviews **one** final PR. Every
other gate (commit standards, secret scan, destructive-op rules, the test gate)
still applies on every branch; only the merge-into-`main` step is deferred.

1. **Plan.** One epic with per-task acceptance criteria; mark each task
   dependent (serial) or independent (parallelizable).
2. **Integration branch.** Cut `integration/<epic-id>-<slug>` off `main` and
   push it. It is **non-protected** — agents merge into it freely.
3. **Task branches.** Each task on `feat/<epic-id>-<task-slug>`, branched *from
   the integration branch*: serial tasks branch from the updated integration
   after their predecessor lands; parallel tasks branch concurrently, one
   agent + worktree each.
4. **Land a task** by one of two sanctioned modes:
   - **Local** — `codeflow integrate <task-branch> --into integration/<…>`:
     flock-serialized (safe for parallel agents), rebases the task branch, runs
     the full test gate, fast-forward-merges. Preferred for tight loops.
   - **PR** — open a PR with base = the integration branch; CI runs (the
     `pull_request` trigger fires regardless of base) and the agent merges on
     green, because the base is non-protected.
5. **Drift control** (long-running epics): periodically **merge** `origin/main`
   *into* the integration branch. Merge only — never rebase a shared branch;
   rebase only task branches.
6. **Finish.** After the last task lands, run the ship flow *on the integration
   branch* (full suite, `validate --docs`, capability/doc/epic-record updates as
   the final commits), then raise **one** PR `integration → main` with the epic
   summary. The human reviews and merges; delete the integration branch after.

Boundaries are unchanged: `main` and every protected branch stay
human-merge-only. The integration branch is not a backdoor — its content reaches
`main` only through that final reviewed PR.

## Why the git boundary is remote

The git standards are enforced in layers, and the layers are not equal. Local git
hooks and the `git-guard` PreToolUse hook are **fast feedback** — they catch the
normal ways work goes wrong in-session, before a push, but an agent on the local host
can in principle edit or skip them. CI and remote branch protection are the
**authoritative perimeter**: server-enforced, so that same agent cannot bypass them
(arm it with `codeflow remote protect`). The local layer is convenience; the remote
layer is the real boundary.

That asymmetry is what makes the override envs (`CODEFLOW_HUMAN_OVERRIDE`, gate
tokens) safe to exist: they are a human-terminal convenience, not authentication, and
are contained precisely because the boundary that matters is remote. Setting one
in-session to slip past a local gate is laundering — the gate stands in for the remote
check it mirrors, so defeating it locally proves nothing.

## When an ADR is warranted — Tier-3 triggers

ADRs are append-only and written at the moment of decision, when context is
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
  pinning down before building. Many epics need no spec at all.
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
- **Doc drift by separate ceremony.** Docs updated "later, in a docs pass"
  rot. Docs mutate only inside the ship flow, in the same PR as the code.
- **Speculative artifacts.** New agents, skills, commands, or templates are
  added when usage proves the need — never because they might help.
- **Assumption-driven building.** Filling an input gap with a guess instead of
  a question. The expensive failures all start here.
- **Agent-merging a protected branch.** An agent never merges into protected —
  a human merges the PR, or `codeflow integrate` lands it. Override envs
  (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) are human-only; setting them
  in-session is laundering — blocked wherever a PreToolUse guard binds (Claude
  Code always; interactive codex after the one-time `/hooks` trust), while the
  git-hook plane honors the env by design as the sanctioned human path; the
  remote perimeter is the hard line. Never `gh pr merge --delete-branch`.
