---
name: cf-develop
description: Build planned work through a build → review → verify loop with bounded rework. Use when implementing a feature or change that already has acceptance criteria.
---

# cf-develop — build, review, verify

Drive the planned work to done.

1. Locate the work and its acceptance criteria (epic, task, spec, or the user's
   prompt). No stated criteria → stop and run the clarity gate from `cf-plan`
   first.
2. Consult `docs/capabilities.md` and `docs/architecture.md` before touching
   code; note what the change touches.
3. Work on a correctly prefixed branch in a worktree — never on the root
   protected-branch checkout.
4. Run the loop:
   a. **Build**: implement the smallest clear, idiomatic, durable scoped change
      with tests; preserve justified reuse, modular boundaries, and explicit
      failure handling while adding no speculative behavior, abstraction, or
      dependency. Use small
      conventional commits. When a remote
      is configured, push the branch after each committed unit so work survives a
      machine failure — backup, not a merge (`--force-with-lease` if you rewrote
      history).
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
      attempts, options, consequences, and a recommendation.
   d. **Verify**: `codeflow test` and `codeflow validate` green.
5. Report completion with evidence (test output, review verdict, file:line for
   each criterion). Hand off to `cf-ship` to land it.
