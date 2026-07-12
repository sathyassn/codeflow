---
name: cf-plan
description: Plan a piece of work — clarify intent, then draft an epic, spec, and ADR as warranted. Use when starting a feature or change, before writing code, to establish testable acceptance criteria and scope.
disable-model-invocation: true
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
5. Draft on a `plan/` branch: an epic in `project-management/epics/` from the
   template; a spec only where interfaces, formats, or behavior need pinning
   down; an ADR draft if a Tier-3 decision is involved (new dependency, schema
   change, boundary change). For a multi-task epic, propose the
   integration-branch flow, `integration/<epic-id>-<slug>` (see cf-method,
   "Managing a body of work").
6. Run `codeflow validate`, then present the plan for approval. Do not start
   building — that is `cf-develop`.
