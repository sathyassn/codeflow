---
name: cf-plan
description: Turn a brief into a clear, evidence-grounded plan and materialize the agreed epic, spec, task, or ADR records. Use when clarifying, planning, or allocating epics, specs, tasks, or ADRs after independent Claude+Codex discovery. Use when the operator asks to plan work, write a spec, or break work into tasks. Do not use to implement, merge, or re-interview ground both seats already settled; still ask live operator-owned questions. Routed work (decided by touched paths; when unsure, route) goes through cf-model-orchestrator first.
---

# cf-plan — plan work, do not build it

You are clarifying and materializing planned work, not building it.

1. Confirm routing. If this routed task did not arrive from an active
   `cf-model-orchestrator` run and has no recorded solo degradation, invoke the
   orchestrator first. Do not recurse when the orchestrator already supplied
   the immutable brief and plan version. Then load `cf-method` for artifact
   discipline.
2. Establish context before asking questions: read `docs/product.md` (scope
   and non-goals), `docs/capabilities.md` (does this exist? what does it
   touch?), and the most recent `docs/decisions/` ADRs. Run `codeflow recall`
   for prior related work.
   When work spans areas/teams, involves source/build/runtime/release/data/trust
   boundaries, or an external method may own it, load
   `cf-method/references/project-organization.md`. Identify each item's
   authority and the actual installed CodeFlow tracking state before records.
   For agentic operating, estimation, capacity or deadline decisions, use
   `cf-estimate` to offer a context-specific preview, reuse compatible adoption
   or honor decline. An estimate answer does not authorize adoption or work.
3. Clarity gate — before drafting durable records, be able to state the problem
   and who it serves; intended outcome and public behavior; scope and non-goals;
   material authority/security/recovery constraints; testable acceptance; and
   affected areas/capabilities. Resolve discoverable facts from the repository,
   tools, and authoritative sources. Make a reversible implementation choice
   from evidence when it preserves the accepted outcome. Ask the operator only
   what `cf-method/references/autonomy.md` reserves to them. Ask the smallest
   consequential question (smallest is *scope*, not count) and include
   evidence, viable options, consequences, and a recommendation; do not ask
   them to perform repository discovery for you.
   After both seats have settled Plan vN, **synthesize that settled ground** —
   do not open a second interview on boundaries, landing shape, or reversible
   implementation choices already approved. Still ask every *live*
   operator-owned question not already answered by the brief or Plan vN
   whose answer would change the outcome; when more
   than one is unblocked, ask them in one round. A later material graph, scope,
   interface, ownership, acceptance, or safety change is Plan vN+1, not a
   re-grill.
4. Check the request against `product.md` non-goals; surface conflicts instead
   of planning around them.
5. Use the exact dual-settled Plan vN; never add scope or design silently.
   Substantive amendments return to both seats as Plan vN+1, under the
   seat-loss rule in `cf-model-orchestrator/resources/task-graph.md`. In a
   recorded solo degradation, perform the same clarity/evidence work and name
   the missing cross-vendor assurance.
   Before allocating, check whether active tracking or approved adoption assigns
   CodeFlow execution. Otherwise retain the approved external or native/session
   plan at earned durability; create no CodeFlow records or gate claims.
   For CodeFlow multi-task work, load the orchestrator's
   `resources/task-graph.md` and put the approved topology in `depends_on` task
   frontmatter. A node, edge, decision guard, ownership, acceptance/interface,
   or safety-boundary mutation creates Plan vN+1; an in-node step does not.
6. For CodeFlow execution, draft on a `plan/` branch. Write each record
   plainly: simple, straightforward and clear, no mannered prose (see
   `.codeflow/rules/writing.md`), in short prose and bullets, with a fenced
   ASCII figure where a flow or structure carries the point. Partition a
   task set by
   coherent durable outcomes and direct dependencies. Related tasks may form
   epics; unrelated or standalone tasks keep separate landing routes. A batch
   is not an epic boundary. When a work item is planned, started, blocked,
   completed or cancelled, follow
   [the work lifecycle](../cf-method/references/project-organization.md#the-work-lifecycle):
   its standalone test decides between an epic and a standalone task, and it
   names the allocation verbs, dependency forms and planning-PR rule. Add a
   spec only where interfaces, formats, or behavior need pinning down, and an
   ADR draft if a Tier-3 decision is involved (new dependency, schema change,
   boundary change). For each multi-task epic, the shared
   `integration/<epic-id>-<slug>` branch is passed with `--into` for every
   task in that body. This is the default,
   not an optional optimization: task branches land there in graph order and
   only the integrated body reaches the protected target through one final
   human-reviewed PR. Select this shape autonomously when the clarity gate,
   acceptance boundaries, and settled graph show one coherent multi-task
   outcome; do not ask the operator merely to choose the routine landing
   mechanism. Honor an explicit operator request when it fits that evidence,
   but never let it bypass planning, safety, or protected-branch boundaries. A
   different landing shape needs an explicit Plan vN rationale and approval
   from both primary seats before task allocation. Do not hand-invent IDs. If another method owns product specs or task
   decomposition, link the settled source in epic/task template `external_refs`
   metadata or an SPC body with its revision; `specs` arrays contain only SPC
   IDs. Do not mirror status or copy its tree. Active full/historical tracking
   nevertheless requires distinct anchored repository-execution tasks; an
   external artifact cannot satisfy or waive those gates.
7. Record the execution contract — `/cf-model-orchestrator` is the default for
   routed work (decided by touched paths): both seats plan independently, Claude leads
   design, and each implementation task uses the canonical assignment record in
   `cf-model-orchestrator/resources/capability-routing.md` to separate its
   responsible primary from actual execution and cross-lineage review. A
   permitted executor change within unchanged primary/task/file ownership,
   scope, lineage-review and isolation remains execution evidence rather than a
   Plan vN+1 mutation. That per-task review occurs before integration and is not
   replaced by the combined integration review. The qualified Claude
   judgment primary owns the
   integrated Claude verdict regardless of the host. Solo `/cf-develop` appears
   only as the noted fallback
   when a required interactive seat is unavailable.
8. Apply the orchestrator's `resources/verification-selection.md` when drafting
   the test strategy. Name evidence for each selected property/generative,
   mutation, or architecture fitness check, or record `none selected`; concrete
   tools and thresholds remain project-owned.
9. Record the closeout contract on each CodeFlow task: capture bounded in-node
   discoveries only when review-relevant; a material graph, scope, interface,
   ownership, acceptance, or safety change stops work and requires Plan vN+1
   before implementation continues. Closeout cannot approve a deviation after
   the fact.
10. For CodeFlow records: Run `codeflow validate --docs`, have both primary
    seats review the exact graph, and present it for approval only where
    the operator owns the decision. Merge the planning PR into every task's
    declared `integration_target`. Implementation begins later from
    `task/TSK-NNN-<slug>` only after `codeflow work start TSK-NNN` passes. That
    preflight is read-only; this skill does not create implementation branches
    or worktrees, change task status, or build code.

Failure paths are explicit: repair allocation collisions and links on the
planning branch; resolve cycles, missing parents, draft specs, and incomplete
predecessors rather than bypassing validation; return a material discovery to
Plan vN+1 and both seats; record a proved unavailable seat as reduced assurance,
never as dual approval. If a CodeFlow task is cancelled, stop product work,
preserve its evidence and cancel it as the work lifecycle states; never label
cancellation complete or shipped. Close or cancel other work at its declared authority.
