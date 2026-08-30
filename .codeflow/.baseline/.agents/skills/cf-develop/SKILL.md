---
name: cf-develop
description: Build planned work through a build → review → verify loop with bounded rework. Use when implementing a feature or change that already has acceptance criteria.
---

# cf-develop — build, review, verify

Drive the planned work to done.

1. Locate the work and its acceptance criteria (epic, task, spec, or the user's
   prompt). No stated criteria → stop and run the clarity gate from `cf-plan`
   first. For a durable task, use `task/TSK-NNN-<slug>` and run
   `codeflow work start TSK-NNN` before product edits; a missing stable planning
   anchor, parent/standalone rationale, approved spec, or completed predecessor
   returns to planning rather than being bypassed.
2. Consult `docs/capabilities.md` and `docs/architecture.md` before touching
   code; note what the change touches. For a material product, UX, interaction,
   or visual-design change, require the plan's settled `DESIGN_INTENT`; if it is
   absent, apply `cf-design` before implementation. A valid `N/A` or `conform`
   record does not add ceremony.
3. Work on a correctly prefixed branch in a worktree — never on the root
   protected-branch checkout.
4. Run the loop:
   a. **Build**: implement the smallest clear, idiomatic, durable scoped change
      with tests through the plan's named interfaces first, and internal unit
      tests where they carry the risk; preserve
      justified reuse, modular boundaries, and explicit
      failure handling while adding no speculative behavior, abstraction, or
      dependency. Use small
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
      read-only review pass (self-review is not review). For unattended or batch
      runs in Claude Code, the pipeline workflow
      (`.claude/workflows/pipeline.workflow.js`) composes the same
      build/review/verify stages.
   c. On `changes_requested`: address blocker and major findings, re-review.
      Maximum 3 evidence-moving cycles. Never repeat the same repair without a
      new hypothesis or changed evidence. At the bound, diagnose the persistent
      constraint: take a safe approved-outcome-preserving route when one remains,
      or surface the genuine external dependency or operator-owned decision with
      attempts, options, consequences, and a recommendation. For a defect that
      resists a first glance, require one already-run command that fails on the
      exact reported symptom before hypothesising (quality-contract blocker
      navigation). Prefer a cheap local failing test when one exists; otherwise
      name the closest executable check.
   d. **Verify**: `codeflow test` and `codeflow validate --docs` green. Apply
      the orchestrator's verification-selection resource: run any property,
      mutation, or architecture fitness check earned by the plan's trigger
      evidence, and report `none selected` rather than inventing ceremony.
5. Report completion with evidence (test output, review verdict, file:line for
   each criterion). Hand off to `cf-ship` to land it.
