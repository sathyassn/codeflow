---
name: cf-develop
description: Implements a planned feature or change through a build → review → verify loop. Use when acceptance criteria are settled and implementation is authorized, either inside cf-model-orchestrator or as its recorded solo fallback after a required interactive seat is unavailable. Do not use as an alternate entry point for routed work.
---

# cf-develop — build, review, verify

Drive the planned implementation stage to done. For routed work this skill
supports `cf-model-orchestrator`; it runs alone only after orchestrator preflight
records the required interactive seat unavailable and the reduced assurance.

1. Locate the work and its acceptance criteria (epic, task, spec, or the user's
   prompt). No stated criteria → stop and run the clarity gate from `cf-plan`
   first. For a durable task, use `task/TSK-NNN-<slug>` and run
   `codeflow work start TSK-NNN` before product edits; a missing stable planning
   anchor, parent/standalone rationale, approved spec, or completed predecessor
   returns to planning rather than being bypassed.
2. Consult `docs/capabilities.md` and `docs/architecture.md` before touching
   code; name the bounded impact set (quality contract). For a material
   product, UX, interaction, or visual-design change, require the plan's
   settled `DESIGN_INTENT`; if it is absent, apply `cf-design` before
   implementation. A valid `N/A` or `conform` record does not add ceremony.
3. Work on a correctly prefixed branch in a worktree — never on the root
   protected-branch checkout.
4. Follow the approved assignment from `cf-model-orchestrator`'s canonical
   capability-routing resource. The responsible primary may use a permitted
   candidate for bounded non-design execution when native routing is proven;
   it still inspects, integrates and accepts the return. Record actual execution
   and authored lineage rather than assuming the host or primary wrote it.
5. Run the loop:
   a. **Build**: implement the smallest clear, idiomatic, durable scoped change
      with tests through the plan's named interfaces first, and internal unit
      tests where they carry the risk; preserve justified reuse, modular
      boundaries, and explicit failure handling while adding no speculative
      behavior, abstraction, or dependency. Apply the quality contract's
      typed-interface and runtime trust-boundary rule and test accepted
      invalid-input behavior; never force a stricter compiler, dependency,
      language, or stack migration merely for compliance. When the change fixes
      a defect, apply Repair in
      `cf-model-orchestrator/resources/quality/findings.md`: state the
      evidenced mechanism and add a regression test that fails before the fix
      and passes after; for a defect that resists a first glance, first run one
      command that fails on the exact reported symptom (a cheap local failing
      test, else the closest executable check). Use small
      conventional commits. When a remote
      is configured, push the branch after each committed unit so work survives a
      machine failure — backup, not a merge (`--force-with-lease` if you rewrote
      history).
      At every multi-task node transition, verify predecessor/decision evidence
      against the approved graph. Stop for Plan vN+1 on a material graph
      mutation; do not replan ordinary work inside the approved node. Persist
      that in-node classification and its supporting evidence in the execution
      ledger before continuing.
   b. **Review**: get an *independent* review against the criteria — in Claude
      Code, spawn the `cf-reviewer` subagent; in another harness, run a separate
      read-only review pass (self-review is not review). Claude Code unattended/
      batch runs use `.claude/workflows/pipeline.workflow.js` for the same
      build/review/verify stages.
      For lifecycle-tracked Claude runs, invoke `cf-reviewer` in the foreground
      (`run_in_background: false` when offered), collect its actual verdict before
      the primary turn ends, and never defer it to a later callback or bypass
      review.
   c. On `changes_requested`, act on the round's findings as
      `cf-model-orchestrator/resources/quality/findings.md` sets out: one
      batch, one apply-and-verify cycle, confirmed by each finder. Maximum 2
      evidence-moving cycles for code; docs and records follow that section's
      bound. At the bound, take a strategic route that keeps the approved
      outcome or surface the external dependency or operator-owned decision.
   d. **Verify**: `codeflow test` and `codeflow validate --docs` green. Apply
      the orchestrator's verification-selection resource: run any property,
      mutation, or architecture fitness check earned by the plan's trigger
      evidence, and report `none selected` rather than inventing ceremony.
6. Report completion with evidence (test output, review verdict, file:line for
   each criterion). Hand off to `cf-ship` to land it.
