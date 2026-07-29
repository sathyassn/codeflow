---
name: cf-plan
description: Materialize an agreed piece of work into acceptance criteria and an epic, spec, or ADR as warranted. This is a supporting planning flow inside cf-model-orchestrator, or the recorded solo fallback after duo preflight proves a required seat unavailable; it is not a competing entry point for non-trivial work.
---

# cf-plan — plan work, do not build it

You are planning work, not building it.

1. Confirm routing. If this non-trivial task did not arrive from an active
   `cf-model-orchestrator` run and has no recorded solo degradation, invoke the
   orchestrator first. Do not recurse when the orchestrator already supplied
   the immutable brief and plan version. Then load `cf-method` for artifact
   discipline.
2. Establish context: read `docs/product.md` (scope and non-goals),
   `docs/capabilities.md` (does this exist? what does it touch?), and the most
   recent `docs/decisions/` ADRs. Run `codeflow recall` for prior related work.
   When the work spans areas/teams, or an external tracker/planning method may
   already own it, load `cf-method/references/project-organization.md` and
   identify the single authority before creating records.
3. Clarity gate — before drafting anything you must be able to state: the
   problem and who it serves; what is in scope and explicitly out; testable
   acceptance criteria; the areas and capabilities affected. Ask concise
   questions until you can. Never assume.
4. Check the request against `product.md` non-goals; surface conflicts instead
   of planning around them.
5. In an orchestrated run, materialize the exact versioned plan both seats
   settled; do not silently add design or scope. A substantive amendment creates
   a new plan version and returns to both seats for approval. In a recorded solo
   degradation, perform the same clarity/evidence work and name the missing
   cross-vendor assurance.
   For multi-task work, load the orchestrator's `resources/task-graph.md` and
   materialize the exact approved topology into canonical `depends_on` task
   frontmatter. A node, edge, decision guard, ownership, acceptance/interface,
   or safety-boundary mutation creates Plan vN+1; an in-node step does not.
6. Draft on a `plan/` branch, matching artifact to work weight. An **epic** (in
   `project-management/epics/`) is warranted only for a body of work that is >1
   PR, >1 session, or spans multiple capabilities; anything smaller is a single
   task with acceptance criteria and no epic — an epic never gates a single
   task. Add a spec only where interfaces, formats, or behavior need pinning
   down, and an ADR draft if a Tier-3 decision is involved (new dependency,
   schema change, boundary change). For a multi-task epic, propose the
   integration-branch flow, `integration/<epic-id>-<slug>` (see cf-method,
   "Managing a body of work"). If another method already owns equivalent specs
   or tasks, reference its settled artifact in `external_refs`; do not mirror
   its status or duplicate its work tree.
7. Record the execution contract — `/cf-model-orchestrator` is the default for
   every non-trivial repository task: both seats plan independently, Claude leads
   design, and each implementation task records its approved producer and
   cross-lineage reviewer. The qualified Claude judgment primary owns the
   integrated Claude verdict regardless of the host. Solo `/cf-develop` appears
   only as the noted fallback
   when a required interactive seat is unavailable.
8. Apply the orchestrator's `resources/verification-selection.md` when drafting
   the test strategy. Name evidence for each selected property/generative,
   mutation, or architecture fitness check, or record `none selected`; concrete
   tools and thresholds remain project-owned.
9. Record the closeout contract on each task: bounded in-node discoveries are
   captured only when review-relevant; a material graph, scope, interface,
   ownership, acceptance, or safety change stops work and requires Plan vN+1
   before implementation continues. Closeout cannot approve a deviation after
   the fact.
10. Run `codeflow validate --docs`, then present the plan for approval. Do not
    start building — that is `cf-develop`.
