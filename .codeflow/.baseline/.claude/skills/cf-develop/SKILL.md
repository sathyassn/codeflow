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
   a. **Build**: implement with tests; small conventional commits.
   b. **Review**: get an *independent* review against the criteria — in Claude
      Code, spawn the `cf-reviewer` subagent; in another harness, run a separate
      read-only review pass (self-review is not review). For unattended or batch
      runs in Claude Code, the pipeline workflow
      (`.claude/workflows/pipeline.workflow.js`) composes the same
      build/review/verify stages.
   c. On `changes_requested`: address blocker and major findings, re-review.
      Maximum 3 cycles — then stop, summarize what is stuck, and ask the user.
   d. **Verify**: `codeflow test` and `codeflow validate` green.
5. Report completion with evidence (test output, review verdict, file:line for
   each criterion). Hand off to `cf-ship` to land it.
