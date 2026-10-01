---
name: cf-develop
description: Implements a planned feature or change through a build → review → verify loop. Use when acceptance criteria are settled and implementation is authorized, either inside cf-model-orchestrator or as its recorded solo fallback after a required interactive seat is unavailable. Do not use as an alternate entry point for routed work.
---

# cf-develop: build, review, verify

Drive the planned implementation stage to done. For routed work this skill
supports `cf-model-orchestrator`; it runs alone only after orchestrator preflight
records the required interactive seat unavailable and the reduced assurance.

1. Locate the work and its acceptance criteria (epic, task, spec, or the user's
   prompt). No stated criteria → stop and run the clarity gate from `cf-plan`
   first. For a durable task, use `task/TSK-NNN-<slug>` and run
   `codeflow work start TSK-NNN` before product edits; its refusal returns to
   planning, never a bypass. When a work item is planned, started, blocked,
   completed or cancelled, follow
   [the work lifecycle](../cf-method/references/project-organization.md#the-work-lifecycle).
2. Reuse the current evidence set; consult `docs/capabilities.md` and
   `docs/architecture.md` for what it lacks, then name the bounded impact
   set (quality contract). For a material product, UX, interaction, or
   visual-design change, require the plan's settled `DESIGN_INTENT`; if it is
   absent, apply `cf-design` before implementation. A valid `N/A` or
   `conform` record does not add ceremony.
3. Work on a correctly prefixed branch in a worktree, never on the root
   protected-branch checkout. Follow the approved assignment from
   `cf-model-orchestrator`'s canonical capability-routing resource; the
   responsible primary inspects, integrates and accepts a permitted
   candidate's return and records actual execution and authored lineage
   rather than assuming the host or primary wrote it.
4. Run the loop:
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
      test, else the closest executable check). Use small conventional
      commits; with a remote, push each for durability (backup, not a merge;
      `--force-with-lease` after a rewrite). At a multi-task node transition,
      verify predecessor and decision evidence against the approved graph;
      stop for Plan vN+1 on a material graph mutation, and log only a
      material dependency or decision change in the task.
   b. **Review**: first merge the current integration line into the task
      branch and resolve conflicts there. Then get an *independent* review
      against the criteria: in Claude Code, spawn the `cf-reviewer` subagent;
      in another harness, run a separate read-only review pass (self-review
      is not review). For lifecycle-tracked Claude runs, invoke `cf-reviewer`
      in the foreground (`run_in_background: false` when offered) and collect
      its actual verdict before the primary turn ends; never defer it to a
      later callback or bypass review. Claude Code
      unattended/batch runs use `.claude/workflows/pipeline.workflow.js` for
      the same build/review/verify stages.
   c. On `changes_requested`, act on the round's findings as
      `cf-model-orchestrator/resources/quality/findings.md` sets out: one
      batch, fixed in this PR, each material fix confirmed by its finder. No
      cycle count decides: continue while repairs produce relevant evidence;
      diagnose a stalled mechanism, an invalid assumption or a materially
      changed scope (split, redesign, or surface the operator-owned
      decision).
   d. **Verify**: targeted tests (for example `cargo test -p <crate>`),
      `codeflow test --mode quick` (the pre-push run counts for the same
      tree) and `codeflow validate --docs`, each cited with revision and
      command. The full gate runs once on the landing candidate. Run a
      property, mutation or fitness check only where verification-selection
      earns it.
5. Report completion: first the result for its consumer and what still
   depends on other work, then the evidence (test output, review verdict,
   file:line for each criterion). Write the report plainly: simple,
   straightforward and clear, no mannered prose (see
   `.codeflow/rules/writing.md`). Hand off to `cf-ship` to land it.
