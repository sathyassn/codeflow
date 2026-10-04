---
name: cf-plan
description: Turn a brief into a clear, evidence-grounded plan and materialize the agreed epic, spec, task, or ADR records. Use when clarifying, planning, or allocating epics, specs, tasks, or ADRs after independent Claude+Codex discovery. Use when the operator asks to plan work, write a spec, or break work into tasks. Do not use to implement, merge, or re-interview ground both seats already settled; still ask live operator-owned questions. Routed work (decided by touched paths; when unsure, route) goes through cf-model-orchestrator first.
---

# cf-plan: plan work, do not build it

You are clarifying and materializing planned work, not building it. Planning
happens once, when a brief or spec is broken into an epic and its tasks, or
when a single outcome becomes one standalone task.

1. Confirm routing. If this routed task did not arrive from an active
   `cf-model-orchestrator` run and has no recorded solo degradation, invoke the
   orchestrator first; do not recurse when it already supplied the immutable
   brief and plan. Then load `cf-method` for artifact discipline.
2. Reuse the current evidence set: what discovery or the epic plan already
   read. Read `docs/product.md` (scope and non-goals), `docs/capabilities.md`
   and the recent `docs/decisions/` ADRs, or run `codeflow recall`, only where
   that set lacks them.
   When work spans areas/teams, involves source/build/runtime/release/data/trust
   boundaries, or an external method may own it, load
   `cf-method/references/project-organization.md`. Identify each item's
   authority and the actual installed CodeFlow tracking state before records.
3. Apply the input clarity checklist in `cf-method` ("Planning an epic")
   before drafting durable records; it is the one statement of what must be
   clear and when to ask the operator. Ask the operator only what
   `cf-method/references/autonomy.md` reserves to them. After the seats have settled the plan,
   **synthesize that settled ground**: do not open a second interview on
   boundaries, landing shape, or reversible implementation choices already
   approved. Still ask every *live* operator-owned question the brief and the
   plan do not answer, all unblocked ones in one round. Check the request
   against `product.md` non-goals; surface conflicts instead of planning
   around them. Before materializing records, check that every task names
   its deliverables and their homes (checklist item 6) against the project's
   structure authority where one exists, and that an epic names the homes
   its tasks write.
4. Use the exact approved plan; never add scope or design silently. In a
   recorded solo degradation, perform the same clarity and evidence work and
   name the missing cross-vendor assurance.
   Before allocating, check whether active tracking or approved adoption assigns
   CodeFlow execution. Otherwise retain the approved external or native/session
   plan at earned durability; create no CodeFlow records or gate claims.
   For CodeFlow multi-task work, load the orchestrator's
   `resources/task-graph.md` and put the approved topology in `depends_on` task
   frontmatter; its mutation rules say which later change needs a new plan
   version and which rides in the batched epic amendment.
5. Materialize by route. Write each record plainly: simple, straightforward
   and clear, no mannered prose (see `.codeflow/rules/writing.md`), in short
   prose and bullets, with a fenced ASCII figure where a flow or structure
   carries the point. Partition a task set by coherent durable outcomes and
   direct dependencies; a batch is not an epic boundary. When a work item is
   planned, started, blocked, completed or cancelled, follow
   [the work lifecycle](../cf-method/references/project-organization.md#the-work-lifecycle):
   its standalone test decides between an epic and a standalone task, and it
   names the allocation verbs and dependency forms.
   - **Epic breakdown:** on a `plan/` branch, one planning PR creates the epic,
     every task with its criteria, and the edges. For each multi-task epic,
     the shared `integration/<epic-id>-<slug>` branch is passed with `--into`
     for every task; task branches land there and only the integrated body
     reaches the protected target through one human-reviewed PR. Select this
     shape without asking the operator to choose the routine landing
     mechanism; a different landing shape needs an explicit rationale in the
     approved plan before allocation. Nothing later plans again.
   - **Standalone task:** on the task branch, allocate the record with
     `codeflow task new --standalone-reason "<why>"`; the record and its code
     are reviewed together and land in one PR.
   - **Batched epic amendment:** follow-ups, re-sizing, reassignment and
     criteria changes of other tasks ride together in one amendment on a
     `plan/` branch, reviewed by one other-lineage seat. One amendment may
     span several epics and carry its docs and `AGENTS.md` project section
     (`Task: EPC-001, EPC-002`, ADR-0078). A task's own criteria change
     rides in its own PR.
   Add a spec only where interfaces, formats, or behavior need pinning down,
   and an ADR draft if a Tier-3 decision is involved. Do not hand-invent IDs.
   If another method owns product specs or task decomposition, link the
   settled source with its revision (an opaque link in the record, or an SPC
   body); `specs` arrays contain only SPC IDs. Do not mirror status or copy
   its tree. Active full/historical tracking nevertheless requires distinct
   anchored repository-execution tasks; an external artifact cannot satisfy
   or waive those gates.
6. For CodeFlow records, run `codeflow validate --docs`; the planning PR is
   reviewed once by the other lineage, and presented for approval only where
   the operator owns the decision. Implementation begins from
   `task/TSK-NNN-<slug>` only after `codeflow work start TSK-NNN` passes. That
   preflight is read-only; this skill does not create implementation branches
   or worktrees, change task status, or build code.

Failure paths are explicit: repair allocation collisions and links on the
planning branch; resolve cycles, missing parents, draft specs, and incomplete
predecessors rather than bypassing validation; return a material discovery to
the plan under the task-graph mutation rules; record a proved unavailable seat
as reduced assurance, never as approval by both seats. If a CodeFlow task is
cancelled, stop product work, preserve its evidence and cancel it as the work
lifecycle states; never label cancellation complete or shipped. Close or
cancel other work at its declared authority.
