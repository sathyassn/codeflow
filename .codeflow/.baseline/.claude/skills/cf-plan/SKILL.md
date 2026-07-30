---
name: cf-plan
description: Turn a brief into a clear, evidence-grounded plan and materialize the agreed epic, spec, task, or ADR records. It asks only consequential operator-owned questions and normally runs inside cf-model-orchestrator after independent Claude+Codex discovery and settlement; direct non-trivial use routes there first.
---

# cf-plan — plan work, do not build it

You are clarifying and materializing planned work, not building it.

1. Confirm routing. If this non-trivial task did not arrive from an active
   `cf-model-orchestrator` run and has no recorded solo degradation, invoke the
   orchestrator first. Do not recurse when the orchestrator already supplied
   the immutable brief and plan version. Then load `cf-method` for artifact
   discipline.
2. Establish context before asking questions: read `docs/product.md` (scope
   and non-goals), `docs/capabilities.md` (does this exist? what does it
   touch?), and the most recent `docs/decisions/` ADRs. Run `codeflow recall`
   for prior related work.
   When the work spans areas/teams, or an external tracker/planning method may
   already own it, load `cf-method/references/project-organization.md` and
   identify the single authority before creating records.
3. Clarity gate — before drafting durable records, be able to state the problem
   and who it serves; intended outcome and public behavior; scope and non-goals;
   material authority/security/recovery constraints; testable acceptance; and
   affected areas/capabilities. Resolve discoverable facts from the repository,
   tools, and authoritative sources. Make a reversible implementation choice
   from evidence when it preserves the accepted outcome. Ask the operator only
   when plausible answers would change the outcome, public behavior, authority,
   material security boundary, irreversible action, or another decision they
   own. Ask the smallest consequential question and include evidence, viable
   options, consequences, and a recommendation; do not ask them to perform
   repository discovery for you.
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
   durable task with a non-empty `standalone_reason`; challenge standalone use
   when the task is actually one node of a broader outcome. Add a spec only
   where interfaces, formats, or behavior need pinning down, and an ADR draft if
   a Tier-3 decision is involved (new dependency, schema change, boundary
   change). Allocate with `codeflow epic new`, `codeflow spec new --for
   EPC-NNN|TSK-NNN`, and `codeflow task new --epic EPC-NNN` or
   `--standalone-reason "..."`; after the stable body-of-work integration
   branch exists, pass `--into integration/<epic-id>-<slug>`. A task branch is
   never an integration target; it cannot authorize its own planning record.
   The target must be a real local or remote-tracking branch, not `HEAD`, a
   tag, an object ID, or another Git revision expression.
   Do not hand-invent IDs. If another method already owns equivalent specs or
   tasks, reference its settled artifact in `external_refs`; do not mirror its
   status or duplicate its work tree.
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
10. Run `codeflow validate --docs`, then have both primary seats review the
    exact materialized graph and present it for operator approval only where
    the operator owns the decision. Merge the planning PR into every task's
    declared `integration_target`. Implementation begins later from
    `task/TSK-NNN-<slug>` only after `codeflow work start TSK-NNN` passes. That
    preflight is read-only; this skill does not create implementation branches
    or worktrees, change task status, or build code.

Failure paths are explicit: repair allocation collisions and links on the
planning branch; resolve cycles, missing parents, draft specs, and incomplete
predecessors rather than bypassing validation; return a material discovery to
Plan vN+1 and both seats; record a proved unavailable seat as reduced assurance,
never as dual approval. If the outcome is cancelled, stop product work and
record `cancelled`, its reason, preserved evidence, and resource disposition
through a reviewed non-task planning/closeout change; never label cancellation
complete or shipped.
