---
description: Build planned work through a build, review, verify loop with bounded rework
argument-hint: [epic/task ID or description of the work]
---

Drive this work to done: $ARGUMENTS

1. Locate the work and its acceptance criteria (epic, task, spec, or the user's
   prompt). No stated criteria → stop and run the clarity gate from `/cf-plan`
   first.
2. Consult `docs/capabilities.md` and `docs/architecture.md` before touching
   code; note what the change touches.
3. Work on a correctly prefixed branch in a worktree — never on the root
   protected-branch checkout.
4. Unattended/batch work, or the user names a preset: invoke the pipeline
   workflow (`.claude/workflows/pipeline.workflow.js`) with composed args —
   stages/models from `[workflows]` in `.codeflow/project.toml` when present.
   Otherwise run the loop inline:
   a. Build: implement with tests; small conventional commits.
   b. Review: spawn the `cf-reviewer` subagent with the criteria and branch.
   c. On `changes_requested`: address blocker and major findings, re-review.
      Maximum 3 cycles — then stop, summarize what is stuck, and ask the user.
   d. Verify: `codeflow test` and `codeflow validate` green.
5. Report completion with evidence (test output, reviewer verdict, file:line
   for each criterion). Hand off to `/cf-ship` to land it.
