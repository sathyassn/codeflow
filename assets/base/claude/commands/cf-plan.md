---
description: Plan a piece of work — clarify intent, then draft epic, spec, and ADR as warranted
argument-hint: [what you want to build or change]
---

You are planning work, not building it. Input: $ARGUMENTS

1. Load the `cf-method` skill for planning discipline.
2. Establish context: read `docs/product.md` (scope and non-goals),
   `docs/capabilities.md` (does this exist? what does it touch?), and the most
   recent `docs/decisions/` ADRs. Run `codeflow recall` for prior related work.
3. Clarity gate — before drafting anything you must be able to state:
   - the problem and who it serves;
   - what is in scope and what is explicitly out;
   - testable acceptance criteria;
   - the areas and capabilities affected.
   Ask the user concise questions until you can. Never assume.
4. Check the request against `product.md` non-goals; surface conflicts instead
   of planning around them.
5. Draft on a `plan/` branch:
   - epic in `project-management/epics/` from the template;
   - spec only where interfaces, formats, or behavior need pinning down;
   - ADR draft if a Tier-3 decision is involved (new dependency, schema change,
     boundary change).
6. Run `codeflow validate`, then present the plan for approval. Do not start
   building — that is `/cf-develop`.
7. Unattended/batch work or a named preset: plan the handoff as the pipeline
   workflow with composed args (stages/models from `[workflows]` in
   `.codeflow/project.toml` when present); otherwise the inline loop is it.
