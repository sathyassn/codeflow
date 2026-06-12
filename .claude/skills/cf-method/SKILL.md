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

Match machinery to the work; escalate only when the lighter rung fails:

- **No workflow** for conversational or trivial changes — answer, edit, done.
- **Inline `/cf-develop` loop** for interactive work — the default path.
- **Pipeline preset** (`.claude/workflows/pipeline.workflow.js`) for
  unattended, batch, or parallel fan-out runs.
- **Custom ad-hoc workflow** when no preset fits — author one for the
  occasion; presets are defaults, not constraints.
- **Stage and model composition lives in config-args** (`args.stages`,
  `args.models`, `[workflows]` in `.codeflow/project.toml`) — never
  hardcoded into the workflow file.

## Planning an epic

An epic exists to make one question answerable before any code is written:
*what does done look like, verifiably?*

Input clarity checklist — do not draft until you can state all four:

1. **Problem and audience.** What hurts, for whom, in one or two sentences.
2. **Scope boundary.** What is in, and — more important — what is explicitly
   out. Check `docs/product.md` non-goals; an epic that violates a non-goal is
   a conversation with the human, not a workaround.
3. **Acceptance criteria.** Testable statements. Each one must be verifiable by
   a command, a test tag, or observable behavior.
4. **Touched surface.** Which areas and which existing capabilities
   (`docs/capabilities.md`) this creates or changes.

If the user's request leaves any of these open, ask. Questions before drafting
are cheap; assumptions discovered at review are expensive.

Good criterion: "`codeflow validate --docs` fails CI when a capability entry
references a nonexistent epic ID."
Bad criterion: "validation works correctly" (not testable), or a 14-item list
restating the implementation plan (that is design, not acceptance).

Right-size the epic: it should ship in days, not weeks. If the criteria list
will not fit on one screen, split the epic.

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
- The entry is updated in the same PR that ships the work — at full tier a
  FEAT epic cannot close without it (`codeflow validate` blocks).
- `verified_by` names real test tags. An entry whose tests cannot be found is
  a dangling claim; `validate --docs` treats it as an error.
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
| Throwaway becomes real | `--minimal` → `--standard` re-init (idempotent, additive) |

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
