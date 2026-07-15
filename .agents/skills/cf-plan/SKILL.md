---
name: cf-plan
description: Plan a piece of work — clarify intent, then draft an epic, spec, and ADR as warranted. Use when starting a feature or change, before writing code, to establish testable acceptance criteria and scope.
---

# cf-plan — plan work, do not build it

You are planning work, not building it.

1. Load the `cf-method` skill for planning discipline.
2. Establish context: read `docs/product.md` (scope and non-goals),
   `docs/capabilities.md` (does this exist? what does it touch?), and the most
   recent `docs/decisions/` ADRs. Run `codeflow recall` for prior related work.
3. Clarity gate — before drafting anything you must be able to state: the
   problem and who it serves; what is in scope and explicitly out; testable
   acceptance criteria; the areas and capabilities affected. Ask concise
   questions until you can. Never assume.
4. Check the request against `product.md` non-goals; surface conflicts instead
   of planning around them.
5. For duo-capable work, invoke `cf-model-orchestrator` **now**, at independent
   research/analysis/planning—not only after a host-written plan exists. Give
   both seats the same immutable brief; let the orchestrator settle the
   versioned dual-approved plan and detailed task breakdown, then pause before
   implementation for the required approval. If a required seat is unavailable,
   record the legible solo degradation and continue with this planning flow.
6. Draft on a `plan/` branch, matching artifact to work weight. An **epic** (in
   `project-management/epics/`) is warranted only for a body of work that is >1
   PR, >1 session, or spans multiple capabilities; anything smaller is a single
   task with acceptance criteria and no epic — an epic never gates a single
   task. Add a spec only where interfaces, formats, or behavior need pinning
   down, and an ADR draft if a Tier-3 decision is involved (new dependency,
   schema change, boundary change). For a multi-task epic, propose the
   integration-branch flow, `integration/<epic-id>-<slug>` (see cf-method,
   "Managing a body of work").
7. Record the execution contract — `/cf-model-orchestrator` is the default for
   all dev work, with Claude as design lead/final reviewer and Codex as
   implementer/first verifier regardless of the host. Solo `/cf-develop`
   appears only as the noted fallback when a required interactive seat is
   unavailable.
8. Run `codeflow validate`, then present the plan for approval. Do not start
   building — that is `cf-develop`.
